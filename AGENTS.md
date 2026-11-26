# Repository Guide

## Purpose

This repository is the mldsa.fail challenge: contestants submit a lattice
reduction algorithm as the `submission` crate, a fixed runner times it on a
fresh ML-DSA-44 key-recovery lattice, and a certified verifier scores the
basis it leaves behind. `CHALLENGE.md` is the contract with contestants;
keep it true when changing anything it describes.

## Layout

- `lattice-attack/` – instance generation, the `Reducer` harness, the
  certified verifier, `β_eff`, the binary basis format. Trusted.
- `runner/` – trusted binary: `seed` and `verify`. Also a small library of
  CLI helpers used by `bench`. Must never depend on `submission`.
- `bench/` – the only binary that links `submission`; runs it under the
  harness and writes the basis snapshots the workflow scores.
- `submission/` – the contestant's crate. `main` always holds the record.
- `.github/workflows/` – `intake` (PR label → submission commit → dispatch),
  `benchmark` (sandboxed run → trusted verify → artifact), `publish`
  (records branch, promotion to `main`, PR comment, site dispatch).
  `.github/scripts/publish.py` holds the records logic.
- Branch `records` – `index.json` and `runs/<run_id>.json`, written only by
  `publish`. The site reads it.

## Working rules

- Trust boundary first. Contestant code runs only in `bench`, as the
  `contestant` user, with no network. Anything that scores or publishes
  must be reachable without linking `submission` and must not trust
  `report.json`, `events.jsonl` or the process exit code.
- Scores come from `runner verify` on the basis copied out at the deadline.
  Do not add any other path from a run to a number on the board.
- Every contestant-controlled input crosses the boundary through one guarded
  path, and a new one must too. Files from the sandbox: imported while the
  cgroup is frozen, regular file, bounded size. Files from the submission
  commit: read from the git object store, regular mode, bounded size.
  Contestant text (report values, `SUBMISSION.toml`, PR title): typed and
  bounded in `publish.py`, strict JSON out. Contestant stdout: only inside
  `::stop-commands::` markers. The sandbox never sees `.git` or a token.
- `intake` and `publish` run from `main` with write access
  (`pull_request_target`, `workflow_run`). They must never check out, build
  or execute pull request content; copying files is the limit.
- Keep the verifier exact. `f64` is fine for `β_eff` and reporting, not for
  acceptance.
- Any change to the instance, the scoring, the budget or the seed derivation
  invalidates comparability with earlier runs. Bump the relevant version
  string (`mldsa.fail/seed/v1`, basis magic, records `schema`) and say so in
  `CHALLENGE.md`.
- Hard tabs, `./clippy.sh` clean, `cargo test --workspace --locked` green
  before pushing.
- Do not merge, approve, deploy or add the `run` label without direct
  authorization for that action.

## Testing the workflows

`benchmark.yml` can be dispatched against any branch that contains it, with
`secs`, `n` and `m` overrides for a minutes-long smoke test on
`ubuntu-latest`; such runs are recorded with `smoke: true` and never ranked
or promoted. `intake.yml` and `publish.yml` only run from `main`.
