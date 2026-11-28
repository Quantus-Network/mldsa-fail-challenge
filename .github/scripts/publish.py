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

`fold` then commits and pushes the records branch. It closes the pull request
only after that push, and leaves it open while any promotion of that same
pull request is still waiting on `main`. A run from another pull request can
still close. `mark-promoted` runs after that push to `main`; it pushes the
promotion flag and then closes. A failed push leaves the pull request open.
A smoke test stays open. Closing is best effort: a failure is logged and the
next publication of that run tries again. The workflow's own records commit
is then empty, because this script already made it.

Runs are immutable: folding a run id that is already recorded reuses the
stored run (the artifact of a re-run attempt is not consulted) and only
recomputes the decision.

Only `meta.json` and `score.json` are trusted. `report.json`, `SUBMISSION.toml`
and the pull request title and author come from the contestant and are
display-only: each field is kept only if it has the expected type and size,
and anything else is dropped, never fatal. Everything written is strict JSON.
"""

import json
import math
import os
import subprocess
import sys
import tomllib
from pathlib import Path

SCHEMA = 1
MAX_TEXT = 2000
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


def text(value):
	return value if isinstance(value, str) and len(value) <= MAX_TEXT else None


def finite(value):
	"""A number JSON can round-trip exactly, or None. Python ints are unbounded, so the bound comes first."""
	if type(value) not in (int, float) or not -(2**53) < value < 2**53:
		return None
	return value if math.isfinite(value) else None


def count(value):
	return value if type(value) is int and 0 <= value < 2**53 else None


def load_untrusted(path, parse):
	"""A mapping from a contestant-written file, or empty.

	Nothing the file contains may stop publication: a missing file, bad
	encoding, a syntax error, or a parser hitting its own limits (recursion
	depth on deep nesting, integer-string size) all mean "no metadata".
	"""
	try:
		value = parse(path.read_text())
	except Exception as e:
		print(f"ignoring {path.name}: {type(e).__name__}: {str(e)[:200]}", file=sys.stderr)
		return {}
	return value if isinstance(value, dict) else {}


def load_run(artifact, pr):
	meta = load_json(artifact / "meta.json")
	score = load_json(artifact / "score.json", {})
	report = load_untrusted(artifact / "report.json", json.loads)
	submission = load_untrusted(artifact / "SUBMISSION.toml", tomllib.loads)
	self_reported = {
		"termination": text(report.get("termination")),
		"counted_steps": count(report.get("counted_steps")),
		"elapsed_secs": finite(report.get("elapsed_secs")),
	}
	author = pr.get("author") if isinstance(pr.get("author"), dict) else {}
	return {
		"schema": SCHEMA,
		"id": str(meta["run_id"]),
		**{k: meta[k] for k in META_FIELDS},
		"author": text(author.get("login")),
		"title": text(pr.get("title")),
		**{k: text(submission.get(k)) for k in ["name", "model", "harness", "note"]},
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


CLOSE_NOTE = (
	"Recorded. This pull request is closed so finished submissions do not accumulate. "
	"A winning submission is committed to `main` by the publish job; merging the pull request is not how a record lands."
)


def git(records, *args):
	result = subprocess.run(["git", *args], cwd=records, text=True, capture_output=True)
	if result.returncode != 0:
		print(result.stderr, file=sys.stderr)
		raise RuntimeError(result.stderr.strip() or f"git {' '.join(args)} exited {result.returncode}")
	return result


def push_records(records, message):
	"""Commit and push records. A clean tree is already published."""
	if os.environ.get("GITHUB_ACTIONS") != "true":
		return
	git(records, "add", "-A")
	diff = subprocess.run(["git", "diff", "--cached", "--quiet"], cwd=records, text=True, capture_output=True)
	if diff.returncode == 0:
		return
	if diff.returncode != 1:
		print(diff.stderr, file=sys.stderr)
		raise RuntimeError(f"git diff exited {diff.returncode}")
	git(records, "commit", "-q", "-m", message)
	git(records, "push", "origin", "HEAD:refs/heads/records")


def close_pull_request(pr, smoke):
	# The records push, and the promotion when there is one, have already returned.
	# A failure here must not fail the job; the next publication tries again.
	if os.environ.get("GITHUB_ACTIONS") != "true" or smoke or type(pr) is not int or pr <= 0:
		return
	repo = os.environ.get("GITHUB_REPOSITORY", "")
	if not repo:
		return

	def gh(*args):
		result = subprocess.run(["gh", *args], text=True, capture_output=True)
		if result.returncode != 0:
			raise RuntimeError(result.stderr.strip() or f"gh exited {result.returncode}")
		return result.stdout.strip()

	try:
		if gh("pr", "view", str(pr), "--repo", repo, "--json", "state", "--jq", ".state") != "OPEN":
			return
		gh("pr", "comment", str(pr), "--repo", repo, "--body", CLOSE_NOTE)
		gh("api", "-X", "PATCH", f"repos/{repo}/pulls/{pr}", "-f", "state=closed", "-f", "state_reason=completed")
	except Exception as e:
		print(f"leaving pull request #{pr} open: {type(e).__name__}: {str(e)[:300]}", file=sys.stderr)


def write_records(records, index, run=None):
	if run is not None:
		(records / "runs").mkdir(exist_ok=True)
		(records / "runs" / f"{run['id']}.json").write_text(json.dumps(run, indent=1, allow_nan=False) + "\n")
	(records / "index.json").write_text(json.dumps(index, indent=1, allow_nan=False) + "\n")


def fold(artifact, records, pr_path, margin):
	pr = load_json(pr_path, {})
	index = load_json(records / "index.json", {"schema": SCHEMA, "record": None, "runs": []})
	run_id = str(load_json(artifact / "meta.json")["run_id"])
	stored = load_json(records / "runs" / f"{run_id}.json")
	record_before = index["record"]

	if stored is not None:
		run, new_record = stored, False
	else:
		run = load_run(artifact, pr)
		new_record = beats(run, record_before, margin)
		if new_record:
			run["promoted"] = True
			run["promotion"] = "pending"
			index["record"] = summary(run)
		index["runs"] = [summary(run)] + index["runs"]
		write_records(records, index, run)

	record = index["record"]
	pending = record is not None and record["promotion"] == "pending"
	decision = {
		"status": run["status"],
		"promote": pending,
		"record_commit": record["commit"] if pending else None,
		"record_id": record["id"] if pending else None,
		"comment": comment(run, record_before, new_record) if stored is None
			else f"Run {run_id} was already recorded; this publication changed nothing.",
	}
	push_records(records, f"Run {os.environ.get('RUN_ID', run_id)}: PR #{run.get('pr')}, {run['status']}")
	print(json.dumps(decision))
	# Closure is per pull request. A later, lower run of a pull request whose
	# earlier win is still pending must not close it before that promotion lands.
	if not (pending and record.get("pr") == run.get("pr")):
		close_pull_request(run.get("pr"), bool(run.get("smoke")))


def mark_promoted(records, run_id):
	index = load_json(records / "index.json")
	run_path = records / "runs" / f"{run_id}.json"
	run = load_json(run_path)
	run["promotion"] = "done"
	for entry in [index["record"], *index["runs"]]:
		if entry and entry["id"] == run_id:
			entry["promotion"] = "done"
	write_records(records, index, run)
	push_records(records, f"Run {run_id} is on main")
	close_pull_request(run.get("pr"), bool(run.get("smoke")))


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
