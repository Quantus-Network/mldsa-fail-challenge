//! Command-line plumbing shared by the trusted `runner` binary and the
//! submission-linked `bench` binary. Nothing here depends on contestant code.

use std::{
	collections::HashMap,
	fs::File,
	io::{BufWriter, Write},
	path::Path,
};

use qp_rusty_crystals_lattice_attack::{Instance, Variant};

pub const VARIANT: Variant = Variant::MlDsa44;

/// `--key value` pairs; a flag without a value holds `"true"`.
pub struct Flags {
	values: HashMap<String, String>,
	usage: &'static str,
}

impl Flags {
	pub fn parse(args: impl Iterator<Item = String>, usage: &'static str) -> Result<Flags, String> {
		let mut values = HashMap::new();
		let mut args = args.peekable();
		while let Some(flag) = args.next() {
			let key =
				flag.strip_prefix("--").ok_or_else(|| format!("unexpected {flag}\n{usage}"))?;
			let value = match args.peek() {
				Some(next) if !next.starts_with("--") => args.next().unwrap(),
				_ => "true".to_string(),
			};
			values.insert(key.to_string(), value);
		}
		Ok(Flags { values, usage })
	}

	pub fn get(&self, key: &str) -> Option<&str> {
		self.values.get(key).map(String::as_str)
	}

	pub fn required(&self, key: &str) -> Result<&str, String> {
		self.get(key).ok_or_else(|| format!("--{key} is required\n{}", self.usage))
	}

	pub fn number<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, String>
	where
		T::Err: std::fmt::Display,
	{
		self.get(key)
			.map(|v| v.parse().map_err(|e| format!("--{key}: {e}")))
			.transpose()
	}

	pub fn hex(&self, key: &str) -> Result<Vec<u8>, String> {
		hex::decode(self.required(key)?.trim_start_matches("0x"))
			.map_err(|e| format!("--{key}: {e}"))
	}

	pub fn seed(&self) -> Result<[u8; 32], String> {
		self.hex("seed")?
			.try_into()
			.map_err(|_| "--seed must be 32 bytes of hex".to_string())
	}

	/// The full ML-DSA-44 lattice unless `--n` and `--m` ask for a smaller
	/// instance of the same shape.
	pub fn instance(&self) -> Result<Instance, String> {
		let seed = self.seed()?;
		match (self.number("n")?, self.number("m")?) {
			(None, None) => Ok(Instance::full(VARIANT, seed)),
			(Some(n), Some(m)) =>
				Instance::generate(VARIANT, seed, n, m).map_err(|e| format!("instance: {e}")),
			_ => Err("--n and --m go together".to_string()),
		}
	}
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
	let mut w = BufWriter::new(File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
	serde_json::to_writer(&mut w, value)
		.map_err(std::io::Error::from)
		.and_then(|()| w.flush())
		.map_err(|e| format!("{}: {e}", path.display()))
}
