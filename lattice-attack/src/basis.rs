//! Integer lattice basis: the type exchanged between the harness, reducers and
//! the verifier.

use std::io::{self, BufRead, Write};

use num_bigint::BigInt;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

/// A list of integer row vectors generating a lattice.
///
/// Reducers may hold any internal representation they like; this is only the
/// boundary type. The verifier never trusts anything beyond the integers
/// stored here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Basis {
	pub rows: Vec<Vec<i64>>,
}

impl Basis {
	pub fn new(rows: Vec<Vec<i64>>) -> Self {
		Basis { rows }
	}

	/// Number of rows (basis vectors).
	pub fn len(&self) -> usize {
		self.rows.len()
	}

	pub fn is_empty(&self) -> bool {
		self.rows.is_empty()
	}

	/// True when every row has exactly `len()` entries.
	pub fn is_square(&self) -> bool {
		let d = self.rows.len();
		self.rows.iter().all(|r| r.len() == d)
	}

	/// Largest absolute entry, or 0 for an empty basis.
	pub fn max_abs_entry(&self) -> u64 {
		self.rows.iter().flatten().map(|x| x.unsigned_abs()).max().unwrap_or(0)
	}

	/// Squared Euclidean norm of row `i`, exact.
	pub fn row_norm_sq(&self, i: usize) -> i128 {
		self.rows[i].iter().map(|&x| (x as i128) * (x as i128)).sum()
	}

	/// Compact encoding: magic, row count, then every entry as a zigzag
	/// LEB128 varint. Rows must be square; a `d × d` basis with entries near
	/// `2^27` takes about `4 d²` bytes, against `9 d²` as JSON.
	pub fn write_binary(&self, mut w: impl Write) -> io::Result<()> {
		let d = u32::try_from(self.rows.len()).map_err(|_| invalid("too many rows"))?;
		if !self.is_square() {
			return Err(invalid("basis is not square"));
		}
		let mut buf = Vec::with_capacity(BINARY_MAGIC.len() + 4 + 4 * self.rows.len().pow(2));
		buf.extend_from_slice(BINARY_MAGIC);
		buf.extend_from_slice(&d.to_le_bytes());
		for &x in self.rows.iter().flatten() {
			let mut zigzag = ((x << 1) ^ (x >> 63)) as u64;
			while zigzag >= 0x80 {
				buf.push(zigzag as u8 | 0x80);
				zigzag >>= 7;
			}
			buf.push(zigzag as u8);
		}
		w.write_all(&buf)
	}

	/// Inverse of [`Self::write_binary`] for a basis of exactly `dim` rows.
	/// Streams the input, so an untrusted file never costs more memory than
	/// the `dim × dim` basis it claims to hold; rejects any other dimension
	/// and trailing bytes.
	pub fn read_binary(mut r: impl BufRead, dim: usize) -> io::Result<Basis> {
		let mut header = [0u8; BINARY_MAGIC.len() + 4];
		r.read_exact(&mut header)?;
		if &header[..BINARY_MAGIC.len()] != BINARY_MAGIC {
			return Err(invalid("not a basis file"));
		}
		if u32::from_le_bytes(header[BINARY_MAGIC.len()..].try_into().unwrap()) != dim as u32 {
			return Err(invalid("unexpected dimension"));
		}
		let mut rows = Vec::with_capacity(dim);
		for _ in 0..dim {
			rows.push((0..dim).map(|_| read_varint(&mut r)).collect::<io::Result<Vec<i64>>>()?);
		}
		if !r.fill_buf()?.is_empty() {
			return Err(invalid("trailing bytes"));
		}
		Ok(Basis { rows })
	}
}

const BINARY_MAGIC: &[u8] = b"mldsa.fail basis v1\n";

fn read_varint(r: &mut impl BufRead) -> io::Result<i64> {
	let (mut value, mut shift) = (0u64, 0u32);
	loop {
		let byte = *r.fill_buf()?.first().ok_or_else(|| invalid("truncated"))?;
		r.consume(1);
		if shift >= 64 {
			return Err(invalid("varint too long"));
		}
		value |= u64::from(byte & 0x7f) << shift;
		if byte < 0x80 {
			return Ok(((value >> 1) as i64) ^ -((value & 1) as i64));
		}
		shift += 7;
	}
}

fn invalid(msg: &str) -> io::Error {
	io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Largest size-reduction coefficient applied in one pass. Keeps `c * x`
/// finite in `f64` and leaves room for the exponent range when a row has
/// grown large.
pub(crate) const MAX_COEFFICIENT: f64 = 1e300;

/// `row_k -= c * row_j` in `i64`, stopping at the first entry whose result
/// does not fit. Entries before it are updated; returns their count as the
/// error so the caller can finish in wider arithmetic.
pub(crate) fn sub_multiple_i64(row_k: &mut [i64], row_j: &[i64], c: f64) -> Result<(), usize> {
	if c.abs() >= 9.2e18 {
		return Err(0);
	}
	let ci = c as i64;
	for (i, (xk, xj)) in row_k.iter_mut().zip(row_j).enumerate() {
		match ci.checked_mul(*xj).and_then(|p| xk.checked_sub(p)) {
			Some(v) => *xk = v,
			None => return Err(i),
		}
	}
	Ok(())
}

/// `row_k[from..] -= c * row_j[from..]` exactly, `c` being an integer-valued
/// `f64`.
pub(crate) fn sub_multiple_big(row_k: &mut [BigInt], row_j: &[i64], c: f64, from: usize) {
	let c = BigInt::from_f64(c).expect("finite integer coefficient");
	for (xk, &xj) in row_k[from..].iter_mut().zip(&row_j[from..]) {
		if xj != 0 {
			*xk -= &c * xj;
		}
	}
}

#[cfg(test)]
mod tests {
	use std::io::Read;

	use super::*;

	#[test]
	fn binary_round_trip_covers_extreme_entries() {
		let basis = Basis::new(vec![
			vec![0, 1, -1],
			vec![i64::MAX, i64::MIN, 63],
			vec![-64, 1 << 40, -(1 << 40)],
		]);
		let mut bytes = Vec::new();
		basis.write_binary(&mut bytes).unwrap();
		assert_eq!(&bytes[..BINARY_MAGIC.len()], BINARY_MAGIC);
		assert_eq!(Basis::read_binary(&bytes[..], 3).unwrap(), basis);
	}

	#[test]
	fn binary_rejects_malformed_input() {
		let basis = Basis::new(vec![vec![5, 6], vec![7, 8]]);
		let mut bytes = Vec::new();
		basis.write_binary(&mut bytes).unwrap();
		assert!(Basis::read_binary(&bytes[..bytes.len() - 1], 2).is_err());
		assert!(Basis::read_binary(&bytes[..], 3).is_err());
		bytes.push(0);
		assert!(Basis::read_binary(&bytes[..], 2).is_err());
		assert!(Basis::read_binary(&b"nope"[..], 2).is_err());
		assert!(Basis::new(vec![vec![1], vec![2, 3]]).write_binary(Vec::new()).is_err());
	}

	/// A claimed dimension is checked before any entry is read, and reading
	/// stops at the first byte past the basis, so a padded or oversized file
	/// never gets buffered.
	#[test]
	fn binary_read_is_bounded_by_the_expected_dimension() {
		struct Endless(u8);
		impl std::io::Read for Endless {
			fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
				buf.fill(self.0);
				Ok(buf.len())
			}
		}
		let mut header = BINARY_MAGIC.to_vec();
		header.extend_from_slice(&u32::MAX.to_le_bytes());
		let oversized = std::io::BufReader::new(header.as_slice().chain(Endless(0)));
		assert!(Basis::read_binary(oversized, 2).is_err());

		let mut padded = Vec::new();
		Basis::new(vec![vec![1]]).write_binary(&mut padded).unwrap();
		let padded = std::io::BufReader::new(padded.as_slice().chain(Endless(0)));
		assert_eq!(Basis::read_binary(padded, 1).unwrap_err().to_string(), "trailing bytes");
	}
}
