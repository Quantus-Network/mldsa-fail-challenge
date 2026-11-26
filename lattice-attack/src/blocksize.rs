//! Effective block size: the human-facing reading of the ranking metric.
//!
//! BKZ-β on a q-ary lattice settles into a Z-shaped Gram–Schmidt profile:
//! q-vectors it has not needed to touch (`ln q`) at the head, unit vectors
//! (`0`) at the tail, and between them a straight segment whose slope is
//! `−2 ln δ_β` by the geometric series assumption. The slope fixes the width
//! of the segment and the lattice volume fixes its position; once the width
//! exceeds the dimension the plateaus vanish and the profile is one line. The
//! log-potential of that model profile is a strictly increasing function of
//! the slope, so a submitted basis's certified potential picks out one slope,
//! hence one root Hermite factor `δ`, hence one block size through the
//! `δ(β)` table and formula used by the lattice estimator.
//!
//! `β_eff` is therefore an increasing function of the score, defined for any
//! basis, and cannot be moved without lowering the potential.

use crate::instance::{Instance, Q};

/// `(β, δ)` measured by Chen and Nguyen for small block sizes, as tabulated
/// in the lattice estimator. The formula below takes over past `β = 40`.
const SMALL_BLOCK_SIZES: [(f64, f64); 8] = [
	(2.0, 1.02190),
	(5.0, 1.01862),
	(10.0, 1.01616),
	(15.0, 1.01485),
	(20.0, 1.01420),
	(25.0, 1.01342),
	(28.0, 1.01331),
	(40.0, 1.01295),
];

/// Root Hermite factor LLL achieves; nothing coarser counts as a block size.
pub const LLL_ROOT_HERMITE: f64 = SMALL_BLOCK_SIZES[0].1;

/// Root Hermite factor reached by BKZ-β: the measured table up to 40,
/// interpolated linearly in `ln δ`, then Chen's asymptotic
/// `((πβ)^{1/β} · β / 2πe)^{1 / 2(β − 1)}`.
pub fn root_hermite_factor(beta: f64) -> f64 {
	if beta <= 2.0 {
		return LLL_ROOT_HERMITE;
	}
	if beta > 40.0 {
		return asymptotic_root_hermite(beta);
	}
	let (lo, hi) = SMALL_BLOCK_SIZES
		.windows(2)
		.map(|w| (w[0], w[1]))
		.find(|&(_, (b, _))| beta <= b)
		.expect("beta within the table");
	let t = (beta - lo.0) / (hi.0 - lo.0);
	(lo.1.ln() + t * (hi.1.ln() - lo.1.ln())).exp()
}

fn asymptotic_root_hermite(beta: f64) -> f64 {
	let two_pi_e = 2.0 * core::f64::consts::PI * core::f64::consts::E;
	let log_delta = ((beta / two_pi_e).ln() + (core::f64::consts::PI * beta).ln() / beta) /
		(2.0 * (beta - 1.0));
	log_delta.exp()
}

/// Inverse of [`root_hermite_factor`] on `[2, max_beta]`: the block size
/// whose BKZ output has root Hermite factor `delta`. `None` when `delta` is
/// coarser than LLL's.
pub fn block_size_for(delta: f64, max_beta: f64) -> Option<f64> {
	if delta > LLL_ROOT_HERMITE {
		return None;
	}
	Some(invert_decreasing(root_hermite_factor, delta, 2.0, max_beta))
}

/// Model Gram–Schmidt profile for the instance's lattice at a given slope
/// magnitude `k` (nats per index): `clamp(ln q − k (i − a), 0, ln q)` with
/// the offset `a` chosen so that the profile has the lattice's volume.
fn model_profile(instance: &Instance, k: f64) -> Vec<f64> {
	let d = instance.dim();
	let log_q = (Q as f64).ln();
	let profile_at = |a: f64| -> Vec<f64> {
		(0..d).map(|i| (log_q - k * (i as f64 - a)).clamp(0.0, log_q)).collect()
	};
	let volume = |a: f64| profile_at(a).iter().sum::<f64>();
	let (mut lo, mut hi) = (-(d as f64) - log_q / k, d as f64);
	for _ in 0..100 {
		let mid = 0.5 * (lo + hi);
		if volume(mid) < instance.log_volume() {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	profile_at(0.5 * (lo + hi))
}

fn potential(profile: &[f64]) -> f64 {
	let d = profile.len();
	profile.iter().enumerate().map(|(i, &l)| (d - 1 - i) as f64 * l).sum()
}

/// Root Hermite factor of the model profile with the given log-potential, or
/// `None` when the potential is not below the starting basis's (the model's
/// limit as the slope becomes vertical).
pub fn effective_root_hermite(instance: &Instance, log_potential: f64) -> Option<f64> {
	if log_potential >= instance.start_potential() {
		return None;
	}
	let model_potential = |k: f64| potential(&model_profile(instance, k));
	// Bisect on ln k; the potential increases with k.
	let (mut lo, mut hi) = (-25.0f64, 10.0f64);
	for _ in 0..100 {
		let mid = 0.5 * (lo + hi);
		if model_potential(mid.exp()) < log_potential {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	let slope_magnitude = (0.5 * (lo + hi)).exp();
	Some((0.5 * slope_magnitude).exp())
}

/// Block size whose equilibrium BKZ profile on this lattice has the given
/// log-potential. `None` below LLL quality (or with no reduction at all).
pub fn effective_block_size(instance: &Instance, log_potential: f64) -> Option<f64> {
	let delta = effective_root_hermite(instance, log_potential)?;
	block_size_for(delta, instance.dim() as f64)
}

/// Largest `x` in `[lo, hi]` with `f(x) >= target`, for decreasing `f`;
/// clamps to the ends of the interval.
fn invert_decreasing(f: impl Fn(f64) -> f64, target: f64, mut lo: f64, mut hi: f64) -> f64 {
	if f(hi) >= target {
		return hi;
	}
	for _ in 0..200 {
		let mid = 0.5 * (lo + hi);
		if f(mid) >= target {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::instance::Variant;

	fn full() -> Instance {
		Instance::full(Variant::MlDsa44, [3u8; 32])
	}

	#[test]
	fn root_hermite_table_and_formula_agree_with_the_estimator() {
		assert_eq!(root_hermite_factor(2.0), 1.0219);
		assert_eq!(root_hermite_factor(40.0), 1.01295);
		assert!((root_hermite_factor(50.0) - 1.01206).abs() < 1e-5);
		assert!((root_hermite_factor(100.0) - 1.00926).abs() < 1e-5);
		assert!((root_hermite_factor(423.0) - 1.00383).abs() < 1e-5);
		let betas: Vec<f64> = (2..=1000).map(|b| b as f64).collect();
		assert!(betas.windows(2).all(|w| root_hermite_factor(w[0]) > root_hermite_factor(w[1])));
	}

	#[test]
	fn block_size_inverts_root_hermite() {
		for beta in [2.0, 3.5, 17.0, 40.0, 41.0, 75.0, 423.0, 900.0] {
			let back = block_size_for(root_hermite_factor(beta), 2049.0).unwrap();
			assert!((back - beta).abs() < 1e-6, "{beta} -> {back}");
		}
		assert_eq!(block_size_for(1.03, 2049.0), None);
		assert_eq!(block_size_for(1.0, 2049.0), Some(2049.0));
	}

	#[test]
	fn model_potential_is_monotone_and_tends_to_the_start_basis() {
		let inst = full();
		let ks = [1e-4, 1e-3, 1e-2, 0.05, 0.1, 1.0, 10.0];
		let potentials: Vec<f64> =
			ks.iter().map(|&k| potential(&model_profile(&inst, k))).collect();
		assert!(potentials.windows(2).all(|w| w[0] < w[1]));
		let vertical = potential(&model_profile(&inst, 1e9));
		assert!((vertical - inst.start_potential()).abs() / inst.start_potential() < 1e-9);
		for &k in &ks {
			let volume: f64 = model_profile(&inst, k).iter().sum();
			assert!((volume - inst.log_volume()).abs() < 1e-6, "{k}: {volume}");
		}
	}

	#[test]
	fn effective_block_size_round_trips_through_the_model() {
		let inst = full();
		for beta in [2.0, 10.0, 60.0, 200.0, 423.0] {
			let k = 2.0 * root_hermite_factor(beta).ln();
			let p = potential(&model_profile(&inst, k));
			let back = effective_block_size(&inst, p).unwrap();
			assert!((back - beta).abs() < 1e-3, "{beta} -> {back}");
		}
	}

	#[test]
	fn no_reduction_has_no_block_size() {
		let inst = full();
		assert_eq!(effective_root_hermite(&inst, inst.start_potential()), None);
		assert_eq!(effective_block_size(&inst, inst.start_potential() + 1.0), None);
		// A little reduction is below LLL quality: δ exists, β does not.
		let slightly = inst.start_potential() - 1.0;
		assert!(effective_root_hermite(&inst, slightly).unwrap() > LLL_ROOT_HERMITE);
		assert_eq!(effective_block_size(&inst, slightly), None);
	}
}
