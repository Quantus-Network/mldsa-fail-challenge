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
  `benchmark` (job `bench`: sandboxed run → trusted verify → artifact; job
  `publish`: records branch, promotion to `main`, PR comment, site dispatch).
  `.github/scripts/publish.py` holds the records logic.
- Branch `records` – `index.json` and `runs/<run_id>.json`, written only by
  `benchmark`'s `publish` job. The site reads it.

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
- `intake` and the `publish` job run from `main` with write access
  (`pull_request_target`; `publish` is a job of `benchmark`, which intake
  always dispatches on `main`). They must never check out, build or execute
  pull request content; copying files is the limit. `publish` is not a
  separate `workflow_run` workflow because GitHub suppresses `workflow_run`
  for runs dispatched with `GITHUB_TOKEN`.
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

`workflow_dispatch` only exists on `main`, so a branch exercises
`benchmark.yml` through its `workflow_call` entry point from a temporary
workflow on that branch, with `secs`, `n` and `m` overrides for a
minutes-long smoke test on `ubuntu-latest`. The caller must grant
`permissions: {contents: write, pull-requests: write}`: GitHub validates
the called workflow's job permissions against the caller's before any `if`
runs, even though the `publish` job is skipped under a push event. Remove
the temporary workflow before merging. Smoke runs are recorded with
`smoke: true` and never ranked or promoted. `intake.yml` and the `publish`
job only run from `main`.

```yaml
name: Smoke
on: { push: { branches: [my-branch] } }
permissions: { contents: write, pull-requests: write }
jobs:
  smoke:
    uses: ./.github/workflows/benchmark.yml
    with: { commit: <submission commit>, pr: "0", secs: "60", n: "60", m: "60" }
```
