"""Maintains the records branch.

    publish.py fold ARTIFACT_DIR RECORDS_DIR PR_JSON MARGIN_BITS
    publish.py mark-promoted RECORDS_DIR RUN_ID

`fold` writes `runs/<run_id>.json` (everything, including the Gram–Schmidt
profile) and rewrites `index.json` (one summary per run, newest first, plus
the current record), then prints a JSON decision for the workflow: whether a
promotion to `main` is pending, for which commit, and the markdown to post on
the pull request. A new record is written with `promotion: "pending"`;
`mark-promoted` flips it to `"done"` once `main` holds the submission, so a
failed push is retried by the next run rather than silently forgotten.

Only `meta.json` and `score.json` are trusted. `report.json` and
`SUBMISSION.toml` come from the sandbox and are display-only; anything
malformed in them is dropped, never fatal.
"""

import json
import sys
import tomllib
from pathlib import Path

SCHEMA = 1
SUMMARY_FIELDS = [
	"id", "commit", "pr", "author", "title", "name", "model", "harness", "note",
	"started", "finished", "budget_secs", "runner", "drand", "seed", "instance",
	"status", "rejection", "score_bits", "effective_block_size", "effective_root_hermite",
	"first_norm", "first_over_gh", "slope", "self_reported", "bench_exit", "smoke",
	"promoted", "promotion", "run_url",
]
META_FIELDS = ["commit", "pr", "started", "finished", "budget_secs", "runner", "drand", "seed", "bench_exit", "smoke", "run_url"]


def load_json(path, default=None):
	try:
		return json.loads(path.read_text())
	except FileNotFoundError:
		return default


def load_untrusted_json(path):
	try:
		value = json.loads(path.read_text())
	except (FileNotFoundError, UnicodeDecodeError, ValueError) as e:
		print(f"ignoring {path.name}: {e}", file=sys.stderr)
		return {}
	return value if isinstance(value, dict) else {}


def load_untrusted_toml(path):
	try:
		value = tomllib.loads(path.read_text())
	except (FileNotFoundError, UnicodeDecodeError, tomllib.TOMLDecodeError) as e:
		print(f"ignoring {path.name}: {e}", file=sys.stderr)
		return {}
	return {k: v for k, v in value.items() if isinstance(v, str)}


def load_run(artifact, pr):
	meta = load_json(artifact / "meta.json")
	score = load_json(artifact / "score.json", {})
	report = load_untrusted_json(artifact / "report.json")
	submission = load_untrusted_toml(artifact / "SUBMISSION.toml")
	self_reported = {k: report.get(k) for k in ["termination", "counted_steps", "elapsed_secs"]}
	return {
		"schema": SCHEMA,
		"id": str(meta["run_id"]),
		**{k: meta[k] for k in META_FIELDS},
		"author": pr.get("author", {}).get("login"),
		"title": pr.get("title"),
		**{k: submission.get(k) for k in ["name", "model", "harness", "note"]},
		"status": score.get("status", "failed"),
		**{k: v for k, v in score.items() if k not in ("status", "basis_bytes")},
		"self_reported": self_reported if any(v is not None for v in self_reported.values()) else None,
		"promoted": False,
		"promotion": None,
	}


def summary(run):
	return {k: run.get(k) for k in SUMMARY_FIELDS}


def beats(run, record, margin):
	if run["status"] != "accepted" or run["smoke"]:
		return False
	return record is None or run["score_bits"] > record["score_bits"] + margin


def comment(run, record_before, new_record):
	if run["status"] == "failed":
		return f"The benchmark run [failed]({run['run_url']}) before producing a basis; see the job log."
	if run["status"] == "rejected":
		return f"The basis left by this submission was **rejected** by the verifier: {run['rejection']}.\n\n[Run log]({run['run_url']})."

	def beta(r):
		b = r and r.get("effective_block_size")
		return f"{b:.1f}" if b is not None else ("below LLL" if r else "–")

	def bits(r):
		return f"{r['score_bits']:.1f}" if r else "–"

	lines = [
		"| | this run | record |",
		"|---|---|---|",
		f"| score (bits) | {bits(run)} | {bits(record_before)} |",
		f"| β_eff | {beta(run)} | {beta(record_before)} |",
		f"| ‖b₁‖ / GH | {run['first_over_gh']:.2f} | |",
		"",
	]
	if run["smoke"]:
		lines.append("Smoke-test run (budget or instance overridden): recorded, never ranked.")
	elif new_record:
		lines.append("**New record.** This submission becomes `main`.")
	else:
		lines.append("Not a new record; the run is on the board.")
	lines.append(f"\nSeed from drand round {run['drand']['round']} mixed with commit `{run['commit'][:12]}`. [Run log and artifacts]({run['run_url']}).")
	return "\n".join(lines)


def write_records(records, index, run=None):
	if run is not None:
		(records / "runs").mkdir(exist_ok=True)
		(records / "runs" / f"{run['id']}.json").write_text(json.dumps(run, indent=1) + "\n")
	(records / "index.json").write_text(json.dumps(index, indent=1) + "\n")


def fold(artifact, records, pr_path, margin):
	pr = load_json(pr_path, {})
	index = load_json(records / "index.json", {"schema": SCHEMA, "record": None, "runs": []})
	run = load_run(artifact, pr)
	previous = load_json(records / "runs" / f"{run['id']}.json", {})
	run["promoted"] = previous.get("promoted", False)
	run["promotion"] = previous.get("promotion")

	record_before = index["record"]
	new_record = beats(run, record_before, margin)
	if new_record:
		run["promoted"] = True
		run["promotion"] = "pending"
		index["record"] = summary(run)
	index["runs"] = [summary(run)] + [r for r in index["runs"] if r["id"] != run["id"]]
	write_records(records, index, run)

	record = index["record"]
	pending = record is not None and record["promotion"] == "pending"
	print(json.dumps({
		"status": run["status"],
		"promote": pending,
		"record_commit": record["commit"] if pending else None,
		"record_id": record["id"] if pending else None,
		"comment": comment(run, record_before, new_record),
	}))


def mark_promoted(records, run_id):
	index = load_json(records / "index.json")
	run_path = records / "runs" / f"{run_id}.json"
	run = load_json(run_path)
	run["promotion"] = "done"
	for entry in [index["record"], *index["runs"]]:
		if entry and entry["id"] == run_id:
			entry["promotion"] = "done"
	write_records(records, index, run)


def main():
	command, args = sys.argv[1], sys.argv[2:]
	if command == "fold":
		fold(Path(args[0]), Path(args[1]), Path(args[2]), float(args[3]))
	elif command == "mark-promoted":
		mark_promoted(Path(args[0]), args[1])
	else:
		sys.exit(__doc__)


if __name__ == "__main__":
	main()
