//! The contestant's reducer. Everything in this crate is yours to change,
//! including dependencies; nothing outside it is part of a submission.
//!
//! The runner calls [`reducer`] once per run, then calls `step` repeatedly
//! on the instance's starting basis until the budget is spent. The basis
//! after the last step that returned in time is verified and scored. See
//! `CHALLENGE.md` at the repository root for the rules and the scoring.

use qp_rusty_crystals_lattice_attack::{reducers::ProgressiveBkz, Reducer};

/// Builds the reducer the runner times. The reference is LLL followed by
/// BKZ tours with enumeration, raising the block size from 10 towards 60.
pub fn reducer() -> impl Reducer {
	ProgressiveBkz::new(10, 60)
}
