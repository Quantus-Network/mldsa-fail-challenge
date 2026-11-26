//! The trusted side of a run.
//!
//! ```text
//! runner seed   --randomness HEX --commit HEX
//! runner bench  --seed HEX --secs S --basis OUT [--n N --m M] [--snapshot-secs 30]
//!               [--report FILE] [--events FILE] [--trace]
//! runner verify --seed HEX --basis FILE --score OUT [--n N --m M]
//! ```
//!
//! `bench` links the `submission` crate and runs it under the harness,
//! writing the newest counted basis to `--basis` (atomically, every
//! `--snapshot-secs` and once more at the end) so that whatever is on disk
//! when the process is killed is a scorable basis. `verify` regenerates the
//! instance from the seed and scores a basis file in a separate process that
//! never ran contestant code. `seed` is the pure function the workflow uses
//! to mix a public randomness beacon with the submission's commit.

use std::{
	collections::HashMap,
	fs::{self, File},
	io::{BufReader, BufWriter, Write},
	path::{Path, PathBuf},
	process::ExitCode,
	time::{Duration, Instant},
};

use qp_rusty_crystals_dilithium::fips202::{
	shake256_absorb, shake256_finalize, shake256_squeeze, Shake256State,
};
use qp_rusty_crystals_lattice_attack::{
	harness::{run, Budget, JsonlObserver, Observer, StderrObserver},
	verify::verify,
	Basis, Instance, Variant,
};

const SEED_DOMAIN: &[u8] = b"mldsa.fail/seed/v1";
const VARIANT: Variant = Variant::MlDsa44;

const USAGE: &str = "usage:
  runner seed   --randomness HEX --commit HEX
  runner bench  --seed HEX --secs S --basis OUT [--n N --m M] [--snapshot-secs 30]
                [--report FILE] [--events FILE] [--trace]
  runner verify --seed HEX --basis FILE --score OUT [--n N --m M]";

fn main() -> ExitCode {
	let mut args = std::env::args().skip(1);
	let command = args.next().unwrap_or_default();
	let result = Flags::parse(args).and_then(|flags| match command.as_str() {
		"seed" => seed(&flags),
		"bench" => bench(&flags),
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

struct Flags(HashMap<String, String>);

impl Flags {
	fn parse(args: impl Iterator<Item = String>) -> Result<Flags, String> {
		let mut map = HashMap::new();
		let mut args = args.peekable();
		while let Some(flag) = args.next() {
			let key =
				flag.strip_prefix("--").ok_or_else(|| format!("unexpected {flag}\n{USAGE}"))?;
			let value = match args.peek() {
				Some(next) if !next.starts_with("--") => args.next().unwrap(),
				_ => "true".to_string(),
			};
			map.insert(key.to_string(), value);
		}
		Ok(Flags(map))
	}

	fn get(&self, key: &str) -> Option<&str> {
		self.0.get(key).map(String::as_str)
	}

	fn required(&self, key: &str) -> Result<&str, String> {
		self.get(key).ok_or_else(|| format!("--{key} is required\n{USAGE}"))
	}

	fn number<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, String>
	where
		T::Err: std::fmt::Display,
	{
		self.get(key)
			.map(|v| v.parse().map_err(|e| format!("--{key}: {e}")))
			.transpose()
	}

	fn hex(&self, key: &str) -> Result<Vec<u8>, String> {
		hex::decode(self.required(key)?.trim_start_matches("0x"))
			.map_err(|e| format!("--{key}: {e}"))
	}

	fn seed(&self) -> Result<[u8; 32], String> {
		self.hex("seed")?
			.try_into()
			.map_err(|_| "--seed must be 32 bytes of hex".to_string())
	}

	/// The full ML-DSA-44 lattice unless `--n` and `--m` ask for a smaller
	/// instance of the same shape.
	fn instance(&self) -> Result<Instance, String> {
		let seed = self.seed()?;
		match (self.number("n")?, self.number("m")?) {
			(None, None) => Ok(Instance::full(VARIANT, seed)),
			(Some(n), Some(m)) =>
				Instance::generate(VARIANT, seed, n, m).map_err(|e| format!("instance: {e}")),
			_ => Err("--n and --m go together".to_string()),
		}
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

fn bench(flags: &Flags) -> Result<ExitCode, String> {
	let instance = flags.instance()?;
	let secs: f64 = flags.number("secs")?.ok_or("--secs is required")?;
	let basis_path = PathBuf::from(flags.required("basis")?);
	let mut budget =
		Budget::new(Duration::try_from_secs_f64(secs).map_err(|e| format!("--secs: {e}"))?);
	budget.trace = flags.get("trace").is_some();
	budget.include_basis = false;

	let mut snapshots = SnapshotWriter {
		path: basis_path,
		min_interval: Duration::from_secs_f64(flags.number("snapshot-secs")?.unwrap_or(30.0)),
		last_write: None,
	};
	let mut progress = StderrObserver;
	let mut events = flags
		.get("events")
		.map(|path| File::create(path).map(|f| JsonlObserver::new(BufWriter::new(f))))
		.transpose()
		.map_err(|e| format!("--events: {e}"))?;
	let mut observers: Vec<&mut dyn Observer> = vec![&mut snapshots, &mut progress];
	if let Some(events) = &mut events {
		observers.push(events);
	}

	let report = run(&instance, submission::reducer(), &budget, &mut observers);

	if let Some(events) = events {
		events.finish().map_err(|e| format!("--events: {e}"))?;
	}
	if let Some(path) = flags.get("report") {
		write_json(Path::new(path), &report)?;
	}
	Ok(if report.result.is_ok() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// Writes the newest counted basis, throttled to `min_interval` except for
/// the final delivery, through a temporary file and a rename so the path
/// always holds a complete basis.
struct SnapshotWriter {
	path: PathBuf,
	min_interval: Duration,
	last_write: Option<Instant>,
}

impl SnapshotWriter {
	fn write(&self, basis: &Basis) -> std::io::Result<()> {
		let tmp = self.path.with_extension("tmp");
		let mut w = BufWriter::new(File::create(&tmp)?);
		basis.write_binary(&mut w)?;
		w.flush()?;
		w.into_inner()?.sync_all()?;
		fs::rename(&tmp, &self.path)
	}
}

impl Observer for SnapshotWriter {
	fn on_snapshot(&mut self, step: u64, basis: &Basis, last: bool) {
		if !last && self.last_write.is_some_and(|t| t.elapsed() < self.min_interval) {
			return;
		}
		match self.write(basis) {
			Ok(()) => self.last_write = Some(Instant::now()),
			Err(e) =>
				eprintln!("[snapshot] step {step}: cannot write {}: {e}", self.path.display()),
		}
	}
}

fn verify_file(flags: &Flags) -> Result<ExitCode, String> {
	let instance = flags.instance()?;
	let basis_path = flags.required("basis")?;
	let score_path = Path::new(flags.required("score")?);
	let bytes = fs::metadata(basis_path).map_err(|e| format!("{basis_path}: {e}"))?.len();
	let basis = File::open(basis_path)
		.and_then(|f| Basis::read_binary(BufReader::new(f)))
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

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
	let mut w = BufWriter::new(File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
	serde_json::to_writer(&mut w, value)
		.map_err(std::io::Error::from)
		.and_then(|()| w.flush())
		.map_err(|e| format!("{}: {e}", path.display()))
}
