# mldsa.fail challenge

How much of the ML-DSA-44 key-recovery lattice can your algorithm reduce in a
fixed time on fixed hardware?

This repository holds the challenge: the instance generator and certified
verifier (`lattice-attack`), the trusted runner that times and scores a
submission, and `submission/`, the one crate contestants edit. `main` always
contains the best verified reducer. Results are published at
[mldsa.fail](https://mldsa.fail).
