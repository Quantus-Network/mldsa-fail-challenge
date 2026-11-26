//! The trusted side of a run. This binary never links the `submission`
//! crate: contestant code only ever runs inside `bench`.
//!
//! ```text
//! runner seed   --randomness HEX --commit HEX
//! runner verify --seed HEX --basis FILE --score OUT [--n N --m M]
//! ```
//!
//! `seed` is the pure function the workflow uses to mix a public randomness
//! beacon with the submission's commit. `verify` regenerates the instance
//! from the seed and scores the basis file `bench` left behind.

use std::{
	fs::{self, File},
	io::BufReader,
	path::Path,
	process::ExitCode,
};

use qp_rusty_crystals_dilithium::fips202::{
	shake256_absorb, shake256_finalize, shake256_squeeze, Shake256State,
};
use qp_rusty_crystals_lattice_attack::{verify::verify, Basis};
use runner::{write_json, Flags};

const SEED_DOMAIN: &[u8] = b"mldsa.fail/seed/v1";

const USAGE: &str = "usage:
  runner seed   --randomness HEX --commit HEX
  runner verify --seed HEX --basis FILE --score OUT [--n N --m M]";

fn main() -> ExitCode {
	let mut args = std::env::args().skip(1);
	let command = args.next().unwrap_or_default();
	let result = Flags::parse(args, USAGE).and_then(|flags| match command.as_str() {
		"seed" => seed(&flags),
		"verify" => verify_file(&flags),
		_ => Err(USAGE.to_string()),
	});
	match result {
		Ok(code) => code,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::from(2)
		},
	}
}

/// `SHAKE256(domain ‖ len(randomness) ‖ randomness ‖ len(commit) ‖ commit)`,
/// 32 bytes, printed as hex.
fn seed(flags: &Flags) -> Result<ExitCode, String> {
	let mut state = Shake256State::default();
	shake256_absorb(&mut state, SEED_DOMAIN);
	for key in ["randomness", "commit"] {
		let bytes = flags.hex(key)?;
		shake256_absorb(&mut state, &(bytes.len() as u64).to_le_bytes());
		shake256_absorb(&mut state, &bytes);
	}
	shake256_finalize(&mut state);
	let mut out = [0u8; 32];
	shake256_squeeze(&mut out, &mut state);
	println!("{}", hex::encode(out));
	Ok(ExitCode::SUCCESS)
}

fn verify_file(flags: &Flags) -> Result<ExitCode, String> {
	let instance = flags.instance()?;
	let basis_path = flags.required("basis")?;
	let score_path = Path::new(flags.required("score")?);
	let bytes = fs::metadata(basis_path).map_err(|e| format!("{basis_path}: {e}"))?.len();
	let basis = File::open(basis_path)
		.and_then(|f| Basis::read_binary(BufReader::new(f), instance.dim()))
		.map_err(|e| format!("{basis_path}: {e}"))?;

	let outcome = verify(&instance, &basis);
	let mut score = serde_json::Map::new();
	score.insert("instance".into(), serde_json::to_value(instance.descriptor()).unwrap());
	score.insert("basis_bytes".into(), bytes.into());
	match &outcome {
		Ok(s) => {
			score.insert("status".into(), "accepted".into());
			if let serde_json::Value::Object(fields) = serde_json::to_value(s).unwrap() {
				score.extend(fields);
			}
			println!(
				"accepted: {:.3} bits, β_eff {}",
				s.score_bits,
				s.effective_block_size
					.map(|b| format!("{b:.1}"))
					.unwrap_or_else(|| "< LLL".into())
			);
		},
		Err(r) => {
			score.insert("status".into(), "rejected".into());
			score.insert("rejection".into(), r.to_string().into());
			println!("rejected: {r}");
		},
	}
	write_json(score_path, &score)?;
	Ok(if outcome.is_ok() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
