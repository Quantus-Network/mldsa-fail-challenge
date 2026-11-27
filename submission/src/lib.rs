//! The contestant's reducer. Everything in this crate is yours to change,
//! including dependencies; nothing outside it is part of a submission.
//!
//! The runner calls [`reducer`] once per run, then calls `step` repeatedly
//! on the instance's starting basis until the budget is spent. The basis
//! after the last step that returned in time is verified and scored. See
//! `CHALLENGE.md` at the repository root for the rules and the scoring.

use qp_rusty_crystals_lattice_attack::{
	reducers::{
		bkz::{enumerate, insert_euclid, insert_lll},
		lll::{lll, DEFAULT_DELTA, DEFAULT_ETA},
		workspace::Workspace,
	},
	Basis, Reducer, Step, StepContext,
};

const BETA_START: usize = 10;
const BETA_MAX: usize = 20;
const BETA_STEP: usize = 2;
/// Positions of one tour handled before returning. A full tour is one step of
/// the reference reducer, and a tour that is still running at the deadline is
/// discarded whole.
const POSITIONS_PER_STEP: usize = 48;

/// LLL, then BKZ tours from [`BETA_START`] up to [`BETA_MAX`]. Each step
/// reduces one slice of the current tour and returns, so a tour that would
/// still be running at the deadline keeps the slices that already finished.
struct CheckpointedBkz {
	lll_done: bool,
	beta: usize,
	/// Next index in the current tour.
	position: usize,
	ws: Option<Workspace>,
}

impl CheckpointedBkz {
	fn new() -> Self {
		CheckpointedBkz { lll_done: false, beta: BETA_START, position: 0, ws: None }
	}
}

impl Reducer for CheckpointedBkz {
	fn name(&self) -> String {
		format!(
			"checkpointed-bkz(beta={BETA_START}..{BETA_MAX} step {BETA_STEP}, {POSITIONS_PER_STEP}/step)"
		)
	}

	fn step(&mut self, basis: &mut Basis, ctx: &StepContext) -> Step {
		let dimension = basis.len();
		let mut ws = match self.ws.take() {
			Some(ws) if ws.rows == basis.rows => ws,
			_ => {
				self.lll_done = false;
				self.beta = BETA_START;
				self.position = 0;
				Workspace::new(basis.rows.clone())
			},
		};
		let mut stop = || ctx.should_stop();

		if !self.lll_done {
			self.lll_done = lll(&mut ws, 0, dimension, DEFAULT_DELTA, DEFAULT_ETA, &mut stop);
			basis.rows.clone_from(&ws.rows);
			self.ws = Some(ws);
			return Step::Continue;
		}

		let beta = self.beta.min(dimension).max(2);
		let tour_end = dimension.saturating_sub(1);
		let slice_end = (self.position + POSITIONS_PER_STEP).min(tour_end);
		let mut position = self.position;
		while position < slice_end {
			if stop() {
				break;
			}
			let block = beta.min(dimension - position);
			if !lll(&mut ws, position, position + block, DEFAULT_DELTA, DEFAULT_ETA, &mut stop) {
				break;
			}
			if stop() {
				break;
			}
			ws.ensure(position + block);
			let radius = ws.r[position] * (1.0 - 1e-6);
			if let Some((coefficients, _)) = enumerate(&ws, position, block, radius, &mut stop) {
				if !insert_lll(
					&mut ws,
					position,
					&coefficients,
					DEFAULT_DELTA,
					DEFAULT_ETA,
					&mut stop,
				) {
					insert_euclid(&mut ws, position, &coefficients);
				}
				if !lll(&mut ws, position, position + block, DEFAULT_DELTA, DEFAULT_ETA, &mut stop)
				{
					position += 1;
					break;
				}
			}
			position += 1;
		}
		self.position = position;
		if self.position >= tour_end {
			self.position = 0;
			self.beta = (self.beta + BETA_STEP).min(BETA_MAX);
		}
		basis.rows.clone_from(&ws.rows);
		self.ws = Some(ws);
		Step::Continue
	}
}

pub fn reducer() -> impl Reducer {
	CheckpointedBkz::new()
}
