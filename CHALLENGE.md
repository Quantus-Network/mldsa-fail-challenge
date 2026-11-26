# The mldsa.fail challenge

Reduce the ML-DSA-44 key-recovery lattice as far as you can in a fixed time
on fixed hardware. The code is the submission: every run is public, every
winning algorithm lands on `main` for the next contestant to build on.

## The instance

Each run gets a fresh instance of the full ML-DSA-44 key-recovery lattice,
dimension `d = 2049` (`n = m = 1024`, `q = 8380417`), generated from a seed
nobody can choose:

```
seed = SHAKE256("mldsa.fail/seed/v1" ‖ drand randomness ‖ submission commit)
```

The drand beacon round is the latest one when the run starts; the submission
commit is fixed before the beacon is read. Instances are Fiat–Shamir style,
with no planted secret, so there is nothing to find other than a shorter
basis. The lattice-estimator puts full key recovery at BKZ block size
β ≈ 423; the question is how close an algorithm gets in the time it has.

## What is measured

The trusted verifier re-derives the instance from the seed and checks that
the basis you left behind generates the same lattice: exact membership plus
determinant checks modulo hashed 62-bit primes. It then computes the
log-potential of the Gram–Schmidt profile in exact arithmetic.

- **`score_bits`** `= (P(B₀) − P(B)) / ln 2`, the decrease in log-potential
  from the starting basis. This is the ranking metric: it is exact, and every
  LLL swap and every BKZ insertion decreases it.
- **`β_eff`** is the headline number. It is the block size at which an
  idealised BKZ (Z-shaped profile on a q-ary lattice, GSA slope from the
  root Hermite factor) would reach the same log-potential. It is a monotone
  function of `score_bits`, so the two never disagree about order.

Both are computed from the basis alone. Nothing your process prints, times,
or reports is used.

## The budget

The budget is a fixed wall-clock time per run, currently
`BENCH_SECS` (a repository variable; the initial value is 7200 s and will be
calibrated and announced before the challenge opens). Your reducer runs
alone on a dedicated-vCPU runner (label in `BENCH_RUNNER`); the CPU model
and vCPU count are recorded with every run.

Time is enforced by the harness, which you do not control: it counts a step
only if it returned before the deadline, and at the deadline it writes the
newest counted basis to disk. The benchmark job copies that file out at
budget + 15 s, whatever your process is doing, and that copy is what gets
scored. Your process is then killed if it is still alive 15 minutes later.

## How to submit

1. Fork this repository and edit **only** `submission/`. It is a Rust crate
   that exports one function:

   ```rust
   pub fn reducer() -> impl qp_rusty_crystals_lattice_attack::Reducer
   ```

   `Reducer::step` is called repeatedly with the current `&mut Basis` until
   the budget is spent; see `lattice-attack/src/harness.rs`. Use as many
   threads as you like. Fill in `submission/SUBMISSION.toml` so the board
   can say what you did.

2. Open a pull request against `main`. A pull request that touches anything
   outside `submission/` is refused.

3. A maintainer adds the `run` label. That queues exactly one benchmark run of
   your branch head and removes the label; push again and ask again. Runs
   are expensive, so maintainers will ask for a sentence about what changed
   before labelling a repeat.

4. The result is posted on your pull request, with the seed, the drand round,
   the run log and the basis as artifacts. If it beats the standing record by
   more than `PROMOTION_MARGIN_BITS`, your `submission/` is committed to
   `main` and your name goes on the board at [mldsa.fail](https://mldsa.fail).

## Dependencies and sandbox

The build uses `main`'s `Cargo.lock` with `--locked --offline`. You can use
any crate already in the lockfile; adding one is a change to `Cargo.lock`,
which is outside `submission/`, so open a separate pull request for it and
say why. Your crate is built and run as an unprivileged user with no network
access.

## What is not allowed

- Changing anything outside `submission/`.
- Reading the seed, the instance or anything else from outside the harness.
- Precomputation keyed to a specific instance. Instances are fresh per run.
- Writing anything to the basis file other than through the harness. The
  copy taken at the deadline is checked for lattice membership; a forged or
  stale basis is rejected or scores lower, never higher.

## Comparability

Two runs of the same code differ by the seed and by timing noise. The
promotion margin absorbs that noise; its value and the measured spread on
the reference runner are published with the calibration. Smoke-test runs
with a reduced budget or instance are recorded but never ranked.
