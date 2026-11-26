"""Folds one benchmark artifact into the records branch.

    publish.py ARTIFACT_DIR RECORDS_DIR PR_JSON MARGIN_BITS

Writes `runs/<run_id>.json` (everything, including the Gram–Schmidt profile)
and rewrites `index.json` (one summary per run, newest first, plus the current
record). Prints a JSON decision for the workflow: whether to promote this
submission to `main` and the markdown to post on its pull request.
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
	"promoted", "run_url",
]


def load_json(path, default=None):
	try:
		return json.loads(path.read_text())
	except FileNotFoundError:
		return default


def load_run(artifact, pr):
	meta = load_json(artifact / "meta.json")
	score = load_json(artifact / "score.json", {})
	report = load_json(artifact / "report.json", {})
	try:
		submission = tomllib.loads((artifact / "SUBMISSION.toml").read_text())
	except (FileNotFoundError, tomllib.TOMLDecodeError):
		submission = {}

	run = {
		"schema": SCHEMA,
		"id": meta["run_id"],
		**{k: meta.get(k) for k in ["commit", "pr", "started", "finished", "budget_secs", "runner", "drand", "seed", "bench_exit", "smoke", "run_url"]},
		"author": pr.get("author", {}).get("login"),
		"title": pr.get("title"),
		**{k: submission.get(k) for k in ["name", "model", "harness", "note"]},
		"status": score.get("status", "failed"),
		**{k: v for k, v in score.items() if k not in ("status", "basis_bytes")},
		"self_reported": {k: report.get(k) for k in ["termination", "counted_steps", "elapsed_secs"]} if report else None,
		"promoted": False,
	}
	return run


def summary(run):
	return {k: run.get(k) for k in SUMMARY_FIELDS}


def beats(run, record, margin):
	if run["status"] != "accepted" or run["smoke"]:
		return False
	return record is None or run["score_bits"] > record["score_bits"] + margin


def comment(run, record_before, promoted):
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
	elif promoted:
		lines.append("**New record.** This submission is now `main`.")
	else:
		lines.append("Not a new record; the run is on the board.")
	lines.append(f"\nSeed from drand round {run['drand']['round']} mixed with commit `{run['commit'][:12]}`. [Run log and artifacts]({run['run_url']}).")
	return "\n".join(lines)


def main():
	artifact, records, pr_path, margin = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), float(sys.argv[4])
	pr = load_json(pr_path, {})
	index = load_json(records / "index.json", {"schema": SCHEMA, "record": None, "runs": []})
	run = load_run(artifact, pr)

	record_before = index["record"]
	promoted = beats(run, record_before, margin)
	run["promoted"] = promoted

	(records / "runs").mkdir(exist_ok=True)
	(records / "runs" / f"{run['id']}.json").write_text(json.dumps(run, indent=1) + "\n")
	index["runs"] = [summary(run)] + [r for r in index["runs"] if r["id"] != run["id"]]
	if promoted:
		index["record"] = summary(run)
	(records / "index.json").write_text(json.dumps(index, indent=1) + "\n")

	print(json.dumps({"promote": promoted, "status": run["status"], "comment": comment(run, record_before, promoted)}))


if __name__ == "__main__":
	main()
