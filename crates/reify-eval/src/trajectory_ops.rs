//! Engine-side trajectory vibration-evaluation primitives and ComputeNode
//! trampolines for `simulate_trajectory` and `input_shape` (PRD
//! `docs/prds/v0_3/trajectory-input-shaping.md` §5.3, §11 Phase 2, task π).
//!
//! This module is the engine-side seam for *evaluating* the vibration behaviour
//! of an input shaper, as opposed to *constructing* its impulse train (which
//! lives in `reify-stdlib`'s `input_shape` / `impulse_shaper` marshalling
//! layer). It is placed in `reify-eval` because its consumers run on the engine
//! side:
//!
//! - `simulate_trajectory` (task θ/ι) — forward command-waveform simulation that
//!   reports residual vibration of a shaped vs. unshaped move.
//! - the Time-Optimal Trajectory Shaping solver (TOTS, task κ) — which scores
//!   candidate shapers by their worst-case residual across a robustness band.
//!
//! Both reuse [`worst_case_residual_fraction`]: it builds the shaper's
//! [`ImpulseTrain`](reify_stdlib::impulse_shaper::ImpulseTrain) via the
//! re-exported `reify_stdlib::build_train_for_shaper` marshalling boundary and
//! sweeps the Singer–Seering residual-vibration metric across a frequency band,
//! returning the worst (largest) residual fraction — the quantity a robust
//! shaper must keep small under modelling error (e.g. ZVD ≤ 5 % across ±10 %,
//! EI ≤ 5 % across ±15 %).

/// Worst-case (largest) residual-vibration fraction of `shaper` swept uniformly
/// across the frequency band `[f_lo_hz, f_hi_hz]` at `n_samples` points.
///
/// A residual fraction of `0.0` is perfect cancellation; `1.0` is the unshaped
/// baseline. A robust shaper keeps the *worst* residual across its insensitivity
/// band small even as the true plant frequency drifts from the design point.
///
/// A non-`StructureInstance` / unrecognised shaper — one that
/// [`reify_stdlib::build_train_for_shaper`] cannot resolve to an
/// [`ImpulseTrain`](reify_stdlib::impulse_shaper::ImpulseTrain) — returns
/// [`f64::INFINITY`]: a shaper that does not build a valid train must never read
/// as "robust" (a small residual). An empty sweep (`n_samples == 0`) likewise
/// returns [`f64::INFINITY`] rather than `0.0`, so a degenerate band can never
/// masquerade as perfect robustness for this *worst-case* metric.
///
/// The damping ratio ζ used in the residual evaluation is read via
/// [`reify_stdlib::shaper_damping_ratio`] — the *same* single-source reader
/// `build_train_for_shaper` builds the train with — so the sweep evaluates the
/// train at exactly the ζ it was constructed from (no parallel default/parsing
/// path that could drift). The Hz→rad/s conversion (`ω = 2π·f`) matches
/// `build_train_for_shaper`'s marshalling boundary.
///
/// `#[allow(dead_code)]`: permanent internal helper of the wired trajectory
/// evaluation pipeline (simulate_trajectory_value / solve_tots, both wired via
/// trampoline.rs → trajectory_ops.rs:371/429); exercised by in-module unit tests;
/// 0-external-caller by design — the top-level entry points own the external
/// call sites.
#[allow(dead_code)]
// G-allow: trajectory robustness metric seam (worst_case_residual_fraction), task #3869 (θ/ι — simulate_trajectory) + #3870 (κ — TOTS); wired pipeline entry points are in trampoline.rs; helper is 0-external-caller by design.
pub fn worst_case_residual_fraction(
    shaper: &reify_ir::Value,
    f_lo_hz: f64,
    f_hi_hz: f64,
    n_samples: usize,
) -> f64 {
    // A shaper that does not resolve to an impulse train must never read as
    // robust — return +∞ so any "residual ≤ tolerance?" check fails for it.
    let Some(train) = reify_stdlib::build_train_for_shaper(shaper) else {
        return f64::INFINITY;
    };

    // An empty sweep has no worst case to report; returning 0.0 would read as
    // "perfectly robust", so a degenerate band returns +∞ (same fail-closed
    // sentinel as an unresolved shaper) for this worst-case metric.
    if n_samples == 0 {
        return f64::INFINITY;
    }

    // ζ for the residual evaluation comes from the SAME single-source reader that
    // built the train (`reify_stdlib::shaper_damping_ratio`), so the sweep
    // evaluates the train at exactly the ζ it was constructed from — the default
    // and numeric-coercion contract cannot drift between the two.
    let zeta = reify_stdlib::shaper_damping_ratio(shaper);

    // Sweep [f_lo_hz, f_hi_hz] uniformly at n_samples points, convert each Hz to
    // rad/s (ω = 2π·f), evaluate the Singer–Seering residual, and keep the worst
    // (largest) fraction — the quantity a robust shaper must hold small across
    // its insensitivity band. (n_samples == 1 samples only the low edge; the
    // n_samples == 0 empty-sweep case is handled above.)
    let mut worst = 0.0_f64;
    for i in 0..n_samples {
        let frac = if n_samples > 1 {
            i as f64 / (n_samples - 1) as f64
        } else {
            0.0
        };
        let f_hz = f_lo_hz + (f_hi_hz - f_lo_hz) * frac;
        let v = train.residual_vibration(2.0 * std::f64::consts::PI * f_hz, zeta);
        if v > worst {
            worst = v;
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::worst_case_residual_fraction;
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a `Shaper` `Value::StructureInstance` (type_name + String-keyed
    /// fields) as the engine path produces it.
    fn shaper(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    fn freq(hz: f64) -> (&'static str, Value) {
        (
            "target_frequency",
            Value::Scalar {
                si_value: hz,
                dimension: DimensionVector::FREQUENCY,
            },
        )
    }

    /// ZVD(10Hz, ζ=0.05) keeps residual ≤ 5 % across the ±10 % band [9, 11] Hz.
    /// ZVD zeroes both residual and its frequency-derivative at the design
    /// point, giving a flat (quadratically-small) residual whose 5 %-level
    /// insensitivity band (≈±19 %) comfortably contains ±10 % (D8). Measured via
    /// ε's `residual_vibration`.
    #[test]
    fn zvd_worst_case_residual_within_5pct_over_plus_minus_10pct() {
        let zvd = shaper(
            "ZVDShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.05))],
        );
        let worst = worst_case_residual_fraction(&zvd, 9.0, 11.0, 21);
        assert!(
            worst <= 0.05,
            "ZVD worst-case residual over ±10% should be ≤ 0.05, got {worst:.6}"
        );
    }

    /// EI(10Hz, ζ=0.05, vtol=0.05) keeps residual ≤ vtol across the ±15 % band
    /// [8.5, 11.5] Hz. The 2-hump EI is ≤ vtol across its insensitivity band
    /// (half-width ≈±19 % at the 5 % level, Singhose 1996), containing ±15 %.
    #[test]
    fn ei_worst_case_residual_within_tolerance_over_plus_minus_15pct() {
        let ei = shaper(
            "EIShaper",
            vec![
                freq(10.0),
                ("damping_ratio", Value::Real(0.05)),
                ("vibration_tolerance", Value::Real(0.05)),
            ],
        );
        let worst = worst_case_residual_fraction(&ei, 8.5, 11.5, 31);
        assert!(
            worst <= 0.05 + 1e-9,
            "EI worst-case residual over ±15% should be ≤ vtol (0.05), got {worst:.6}"
        );
    }

    /// Robustness ordering: a plain ZV (narrow suppression) yields a strictly
    /// larger worst-case residual than the EI over the same ±15 % band — EI
    /// trades depth for width, so it wins at the band edges.
    #[test]
    fn ei_is_more_robust_than_zv_over_plus_minus_15pct() {
        let zv = shaper(
            "ZVShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.05))],
        );
        let ei = shaper(
            "EIShaper",
            vec![
                freq(10.0),
                ("damping_ratio", Value::Real(0.05)),
                ("vibration_tolerance", Value::Real(0.05)),
            ],
        );
        let zv_worst = worst_case_residual_fraction(&zv, 8.5, 11.5, 31);
        let ei_worst = worst_case_residual_fraction(&ei, 8.5, 11.5, 31);
        assert!(
            zv_worst > ei_worst,
            "ZV worst-case ({zv_worst:.6}) should exceed EI worst-case \
             ({ei_worst:.6}) over ±15%"
        );
    }

    /// An empty sweep (`n_samples == 0`) must report +∞, not 0.0 — a worst-case
    /// metric over no samples has no worst case, and reading as "perfectly
    /// robust" would let a degenerate band mask an unevaluated shaper.
    #[test]
    fn empty_sweep_is_infinity_not_zero() {
        let zvd = shaper(
            "ZVDShaper",
            vec![freq(10.0), ("damping_ratio", Value::Real(0.05))],
        );
        let worst = worst_case_residual_fraction(&zvd, 9.0, 11.0, 0);
        assert!(
            worst.is_infinite() && worst > 0.0,
            "empty sweep (n_samples=0) should be +∞, got {worst}"
        );
    }
}

// ── simulate_trajectory ComputeNode trampoline ────────────────────────────────
//
// Mirrors `dynamics_ops::run_inverse_dynamics` / `solve_inverse_dynamics_trampoline`.
// The pure Value→Value core (`reify_stdlib::simulate_trajectory_value`) lives in
// reify-stdlib (where the θ pub(crate) types are visible); the engine-facing
// trampoline wrapper and warm-state cache live here (reify-eval owns
// ComputeOutcome/OpaqueState/CancellationHandle).

use std::sync::Arc;

use reify_ir::{OpaqueState, Value};
use reify_stdlib::{InputShapeCacheKey, SimulateTrajectoryCacheKey};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Generic warm-state cache entry for a completed trajectory ComputeNode
/// dispatch: the content-hash `key` the result was computed for plus the cached
/// result `Value`.
///
/// `K` is the per-target content-hash cache key ([`SimulateTrajectoryCacheKey`]
/// for the forward-sim arm, [`InputShapeCacheKey`] for the input-shape arm).
/// Recovered on the next invocation via [`OpaqueState::downcast_ref`] and reused
/// only when the incoming key matches (a cache HIT). The result is held behind
/// an [`Arc`] so the mandatory cache-HIT re-donation is an O(1) refcount bump
/// rather than a second deep clone of the result tree; only the output-value-cell
/// copy pays an unavoidable deep clone (the engine cell owns a plain `Value`).
///
/// This single generic collapses what were two near-verbatim per-target structs
/// (`SimulateTrajectoryCache` / `InputShapeCache`) — they now differ only in the
/// key type and are spelled as the type aliases below. Mirrors
/// `dynamics_ops::InverseDynamicsCache` without the per-body solid-hash record
/// (trajectory has no body-granular reuse optimisation).
#[derive(Clone)]
struct ComputeResultCache<K> {
    key: K,
    result: Arc<Value>,
}

impl<K: Copy + Send + Sync + 'static> ComputeResultCache<K> {
    /// Coarse heap-size estimate in bytes: the flat key plus the result tree.
    fn estimated_size_bytes(&self) -> usize {
        std::mem::size_of::<K>() + value_size_estimate(self.result.as_ref())
    }

    /// Wrap this cache in an `OpaqueState` for donation to the warm-state pool,
    /// sized by [`estimated_size_bytes`](Self::estimated_size_bytes).
    fn into_opaque_state(self) -> (OpaqueState, usize) {
        let size = self.estimated_size_bytes();
        (OpaqueState::new(self, size), size)
    }
}

/// Warm-state cache for a completed `simulate_trajectory` dispatch (keyed on the
/// `(profile, mech, modal)` content hash).
type SimulateTrajectoryCache = ComputeResultCache<SimulateTrajectoryCacheKey>;

/// Warm-state cache for a completed `input_shape` dispatch (keyed on the
/// `(profile, shaper)` content hash).
type InputShapeCache = ComputeResultCache<InputShapeCacheKey>;

/// Coarse heap-size estimate of a `Value` tree. Mirrors
/// `dynamics_ops::value_size_estimate` (kept a local copy: that one is private to
/// `dynamics_ops`, which is outside this task's lock scope — hoisting all three
/// copies to a shared module is a follow-up).
fn value_size_estimate(v: &Value) -> usize {
    let base = std::mem::size_of::<Value>();
    match v {
        Value::String(s) => base + s.len(),
        Value::List(items) => base + items.iter().map(value_size_estimate).sum::<usize>(),
        Value::StructureInstance(d) => {
            base + d.type_name.len()
                + d.fields
                    .iter()
                    .map(|(k, val)| k.len() + value_size_estimate(val))
                    .sum::<usize>()
        }
        _ => base,
    }
}

/// Build the `Completed` outcome that donates `cache` as the node's warm state.
/// Performs one deep clone of the result for the output value cell; the
/// warm-state copy re-uses the same `Arc<Value>` (O(1) refcount bump). Shared by
/// both trajectory trampolines (generic over the cache-key type `K`).
fn completed_donating<K: Copy + Send + Sync + 'static>(
    cache: ComputeResultCache<K>,
) -> ComputeOutcome {
    let result = cache.result.as_ref().clone();
    let (state, size_bytes) = cache.into_opaque_state();
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    ComputeOutcome::Completed {
        result,
        new_warm_state: Some(state),
        cost_per_byte,
        diagnostics: Vec::new(),
    }
}

/// Malformed-input short-circuit: `Value::Undef` with no warm state, mirroring
/// `dynamics_ops::undef_outcome`. Shared by both trajectory trampolines.
fn undef_outcome() -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
    }
}

/// `@optimized("trajectory::simulate")` public `ComputeFn` for `fn
/// simulate_trajectory` (registered in `compute_targets::register_compute_fns`).
///
/// Three `value_inputs`: `[profile, mech, modal]` (same order as the .ri fn).
///
/// Cooperative cancellation: polls on entry; a pre-cancelled handle returns
/// `Cancelled` immediately without running the simulation.
///
/// Warm-state cache: on a completed run, donates the result under the
/// `SimulateTrajectoryCacheKey(profile,mech,modal)` content-hash key.  A
/// subsequent dispatch with identical inputs HITs the cache and returns the
/// stored `EndEffectorTrack` without re-running `simulate_trajectory_value`.
pub fn simulate_trajectory_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // Entry cancellation checkpoint (mirrors run_inverse_dynamics §0).
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }

    // Arity guard: simulate_trajectory(profile, mech, modal) — 3 value inputs.
    if value_inputs.len() != 3 {
        return undef_outcome();
    }
    let profile = &value_inputs[0];
    let mech = &value_inputs[1];
    let modal = &value_inputs[2];

    // Build the content-hash cache key over the three result-determining inputs.
    let key = SimulateTrajectoryCacheKey::from_inputs(profile, mech, modal);

    // ── cache HIT ─────────────────────────────────────────────────────────────
    // If a prior warm state's key matches, re-donate it and return the cached
    // EndEffectorTrack without re-running the simulation.
    if let Some(cache) = prior_warm_state.and_then(|s| s.downcast_ref::<SimulateTrajectoryCache>())
        && cache.key.matches(&key)
    {
        return completed_donating(cache.clone());
    }

    // ── cache MISS ────────────────────────────────────────────────────────────
    // Delegate to the stdlib Value→Value composer (runs the full simulation).
    let result = reify_stdlib::simulate_trajectory_value(profile, mech, modal);
    if matches!(result, Value::Undef) {
        return undef_outcome();
    }

    let cache = SimulateTrajectoryCache {
        key,
        result: Arc::new(result),
    };
    completed_donating(cache)
}

// ── input_shape ComputeNode trampoline ───────────────────────────────────────
//
// Mirrors simulate_trajectory_trampoline above, but for `input_shape(profile,
// shaper)`.  TOTS (heavy, cache-valuable) and impulse ZV/ZVD/EI/Cascaded
// (cheap real shaping) both route through the same trampoline; the
// `input_shape_value` stdlib composer branches internally.

/// `@optimized("trajectory::input_shape")` public `ComputeFn` for `fn
/// input_shape` (registered in `compute_targets::register_compute_fns`).
///
/// Two `value_inputs`: `[profile, shaper]`.
///
/// Cooperative cancellation: polls on entry.
///
/// Warm-state cache: result keyed on `InputShapeCacheKey(profile,shaper)`.
pub fn input_shape_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // Entry cancellation checkpoint.
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }

    // Arity guard: input_shape(profile, shaper) — 2 value inputs.
    if value_inputs.len() != 2 {
        return undef_outcome();
    }
    let profile = &value_inputs[0];
    let shaper = &value_inputs[1];

    // Build the content-hash cache key.
    let key = InputShapeCacheKey::from_inputs(profile, shaper);

    // ── cache HIT ─────────────────────────────────────────────────────────────
    if let Some(cache) = prior_warm_state.and_then(|s| s.downcast_ref::<InputShapeCache>())
        && cache.key.matches(&key)
    {
        return completed_donating(cache.clone());
    }

    // ── cache MISS ────────────────────────────────────────────────────────────
    let result = reify_stdlib::input_shape_value(profile, shaper);
    if matches!(result, Value::Undef) {
        return undef_outcome();
    }

    let cache = InputShapeCache {
        key,
        result: Arc::new(result),
    };
    completed_donating(cache)
}
