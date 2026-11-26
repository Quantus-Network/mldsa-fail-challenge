//! Runs the submission under the timed harness. This is the one binary that
//! links contestant code, so the workflow runs it sandboxed and trusts
//! nothing it prints: only the basis file it leaves behind, re-scored by
//! `runner verify`.
//!
//! ```text
//! bench --seed HEX --secs S --basis OUT [--n N --m M] [--snapshot-secs 30]
//!       [--report FILE] [--events FILE] [--trace]
//! ```
//!
//! `--basis` always holds a complete basis: the starting basis before the
//! reducer runs, then the newest counted basis (atomically replaced every
//! `--snapshot-secs` and once more at the end), so whatever is on disk when
//! the process is killed is scorable. Exits non-zero unless the final scored
//! basis was written.

use std::{
	fs::{self, File},
	io::{self, BufWriter, Write},
	path::{Path, PathBuf},
	process::ExitCode,
	time::{Duration, Instant},
};

use qp_rusty_crystals_lattice_attack::{
	harness::{run, Budget, JsonlObserver, Observer, StderrObserver},
	Basis,
};
use runner::{write_json, Flags};

const USAGE: &str = "usage:
  bench --seed HEX --secs S --basis OUT [--n N --m M] [--snapshot-secs 30]
        [--report FILE] [--events FILE] [--trace]";

fn main() -> ExitCode {
	match Flags::parse(std::env::args().skip(1), USAGE).and_then(|flags| bench(&flags)) {
		Ok(code) => code,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::from(2)
		},
	}
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
		status: Err(io::Error::other("no basis written")),
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
	snapshots.status.map_err(|e| format!("{}: {e}", snapshots.path.display()))?;
	Ok(if report.result.is_ok() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// Writes the newest basis, throttled to `min_interval` except for the first
/// and final deliveries, through a temporary file and a rename so the path
/// always holds a complete basis. `status` is the outcome of the most recent
/// attempt, which after a run is the final scored basis.
struct SnapshotWriter {
	path: PathBuf,
	min_interval: Duration,
	last_write: Option<Instant>,
	status: io::Result<()>,
}

impl SnapshotWriter {
	fn write(&self, basis: &Basis) -> io::Result<()> {
		let tmp = self.path.with_extension("tmp");
		let mut w = BufWriter::new(File::create(&tmp)?);
		basis.write_binary(&mut w)?;
		w.flush()?;
		w.into_inner()?.sync_all()?;
		fs::rename(&tmp, &self.path)
	}
}

impl Observer for SnapshotWriter {
	fn on_snapshot(&mut self, steps_applied: u64, basis: &Basis, last: bool) {
		if !last && self.last_write.is_some_and(|t| t.elapsed() < self.min_interval) {
			return;
		}
		self.status = self.write(basis);
		match &self.status {
			Ok(()) => self.last_write = Some(Instant::now()),
			Err(e) => eprintln!(
				"[snapshot] after {steps_applied} steps: cannot write {}: {e}",
				self.path.display()
			),
		}
	}
}
