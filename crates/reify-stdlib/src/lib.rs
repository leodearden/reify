// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` for once-per-session OOB-warning
// suppression, which is excluded from `PartialEq`/`Ord`/`Hash`/`content_hash`
// but still triggers the `mutable_key_type` lint on every `BTreeMap<Value, _>`.
#![allow(clippy::mutable_key_type)]

use reify_ir::Value;

mod helpers;

/// Public re-export of the shared complex-phase helper, so reify-expr's method
/// path can call the same implementation used by the stdlib builtin path.
pub use helpers::complex_phase;

/// Public re-export of the tolerance stack-up error classifier.
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push `Severity::Error` diagnostics into the `EvalContext` sink when a
/// stackup builtin returns `Value::Undef`.
pub use stackup::diagnose as stackup_diagnose;

/// Public re-export of the DFM (design-for-manufacturing) diagnostic classifier
/// (PRD v0_6 process-dfm-completion, task α).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to push
/// DFM diagnostics into the `EvalContext` sink. Like `flexure_diagnose` (and unlike
/// the post-`Undef`-only `stackup_diagnose` / `fea_diagnose`), it fires on BOTH
/// paths: a successfully-evaluated `fits_build_volume` returning `Bool(false)` is a
/// build-volume VIOLATION whose severity comes from the rule argument, while a
/// `Value::Undef` result is a usage error.
pub use dfm::diagnose as dfm_diagnose;

/// Public re-export of the ISO tolerancing diagnostic classifier (task α/4461).
///
/// Flags `iso_it_tolerance` out-of-envelope calls (well-typed args that fall
/// outside IT5–IT18 / ≤500 mm) with a `Severity::Error` Diagnostic.
/// Returns `None` for valid calls, for `effective_tolerance_zone`, and for
/// non-tolerancing names.
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm
/// (`emit_undef_builtin_diagnostics`, next to `stackup_diagnose`) to push a
/// `Severity::Error` into the `EvalContext` sink when a well-typed but
/// out-of-envelope `iso_it_tolerance` returns `Value::Undef`. Mirrors the
/// `stackup_diagnose` / `fea_diagnose` / `geometry_diagnose` /
/// `dynamics_diagnose` pattern.
pub use tolerancing::diagnose as tolerancing_diagnose;

/// Public re-export of the multi-load-case FEA error classifier.
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push `Severity::Error` diagnostics into the `EvalContext` sink when
/// `linear_combine` returns `Value::Undef` for a task-#10 failure mode
/// (empty/unknown-case weights or incompatible meshes).
pub use fea::diagnose as fea_diagnose;

/// Public re-export of the affine-constructor diagnostic classifier (task β).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push a `Severity::Warning` into the `EvalContext` sink when `affine_scale`
/// returns `Value::Undef` for a zero (degenerate, det=0) or dimensioned scale
/// factor. Fires only on the `Value::Undef` path, like `stackup_diagnose` /
/// `fea_diagnose`.
pub use geometry::diagnose as geometry_diagnose;

/// Public re-export of the PRB-flexure diagnostic classifier (task 3871).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push the §5.3 / §1 flexure diagnostics (`W_FlexureYielding`,
/// `W_FlexurePrbOutOfRange`, `E_FlexureGeometryInvalid`,
/// `W_FlexureFatigueCheckMissing`) into the `EvalContext` sink. Unlike
/// `stackup_diagnose` / `fea_diagnose` (which fire only on a `Value::Undef`
/// result), `flexure_diagnose` runs on BOTH the success and `Undef` paths —
/// the yielding / out-of-range warnings fire on a successfully constructed joint.
pub use flexures::flexure_diagnose;

/// Public re-export of the single canonical body-mass resolver (task 4278).
///
/// Called by `inverse_dynamics` (both open-chain and closed-chain dispatch
/// paths) and by the `dynamics_diagnose` hook.  Task 4271 (modal bridge) uses
/// this as the stable single read-path so every mass consumer shares it.
///
/// Returns `Some(MassProperties StructureInstance)` when `body.solid` is a
/// `MassProperties` StructureInstance; returns `None` for any unresolvable
/// solid (plain geometry, wrong type, missing key).
pub use dynamics::eval::resolve_body_mass;

/// Public re-export of the canonical MassProperties field extractor (task 4271).
///
/// Composes with [`resolve_body_mass`]: call `resolve_body_mass(body)` to get
/// the `MassProperties` `Value`, then `mass_properties_from_value(&mp)` to
/// extract `(mass, com, inertia)` as raw SI `f64` values.  Widened from
/// `pub(crate)` to `pub` so that `reify-eval`'s mechanism-modal bridge can
/// reuse the single canonical mass read-path instead of adding an independent
/// one (task constraint, task 4271).
pub use dynamics::eval::mass_properties_from_value;

/// Public re-export of the inverse_dynamics Undef-path diagnostic classifier
/// (task 4278).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push `DynamicsBodyMassUnresolved` into the `EvalContext` sink when an
/// `inverse_dynamics` call returns `Value::Undef` because a spanning-tree body
/// has no resolvable mass. Mirrors the `stackup_diagnose` / `fea_diagnose` /
/// `geometry_diagnose` pattern.
pub use dynamics::eval::diagnose as dynamics_diagnose;

/// Public re-export of the `center_of_mass` fallback-Warning diagnostic hook
/// (task 4471).
///
/// Called by `crates/reify-expr/src/lib.rs` immediately after
/// `emit_dfm_diagnostics` (both-path hook at `lib.rs:475`) as
/// `emit_snapshot_diagnostics`. Fires a `Severity::Warning` with code
/// `W_SnapshotCenterOfMassDensityFallback` ONLY in the **mixed** case: ≥1
/// body carries resolvable mass (via `resolve_body_mass`) while ≥1 other
/// body does not. Pure-legacy snapshots (all unresolved) and all-resolved
/// snapshots stay silent. Mirrors the `dfm_diagnose` / `flexure_diagnose`
/// both-path pattern.
pub use snapshot::diagnose as snapshot_diagnose;

/// Public re-export of the von Mises scalar kernel for cross-crate reuse.
///
/// Called by `crates/reify-expr/src/field_reductions.rs` in the
/// `project_von_mises_sampled` helper to project each 9-float stride-1 window
/// of a Sampled tensor field to a scalar von Mises value during field
/// reduction (max/min/argmax/argmin of a VonMises-derived field). Avoids
/// duplicating the formula inlined at
/// `crates/reify-eval/src/compute_targets/elastic_static.rs:667`.
pub use analysis::compute_von_mises_3x3;
/// Public re-export of the max-shear scalar kernel for cross-crate reuse.
///
/// Called by `crates/reify-expr/src/field_reductions.rs` in the
/// `project_max_shear_sampled` helper to project each 9-float stride-1 window
/// of a Sampled tensor field to a scalar max-shear value during field
/// reduction (max/min/argmax/argmin of a MaxShear-derived field). Mirrors
/// the `compute_von_mises_3x3` precedent: the formula has a single home in
/// `analysis.rs`, shared by both the `max_shear` builtin and the
/// cross-crate field reduction.
pub use analysis::compute_max_shear_3x3;
/// Public re-export of the symmetric-3×3 eigenvalue kernel for cross-crate reuse.
///
/// Called by `crates/reify-expr/src/field_reductions.rs` in the
/// `project_principal_stresses_sampled` helper to project each 9-float
/// stride-1 window of a Sampled tensor field to a scalar principal-stress
/// value during field reduction (max/min/argmax/argmin of a
/// PrincipalStresses-derived field). Selects `eigs[2]` (max principal σ₁)
/// or `eigs[0]` (min principal σ₃) per window (task 4562).
pub use analysis::compute_eigenvalues_3x3;
/// Public re-export of the impulse-shaper math (`ImpulseTrain` + its residual /
/// convolution API) and the `Shaper`→`ImpulseTrain` marshalling boundary.
///
/// `reify-eval/src/trajectory_ops.rs` calls [`build_train_for_shaper`] to turn a
/// `Shaper` `Value` into an [`impulse_shaper::ImpulseTrain`] and then sweeps
/// [`impulse_shaper::ImpulseTrain::residual_vibration`] across a frequency band
/// for the engine-side robustness metric (task ζ). It reads the swept damping
/// ratio ζ via [`shaper_damping_ratio`] — the same single-source reader
/// `build_train_for_shaper` builds the train with — so the sweep evaluates the
/// train at exactly the ζ it was constructed from.
pub use trajectory::impulse_shaper;
pub use trajectory::input_shape::{build_train_for_shaper, shaper_damping_ratio};
/// Public re-export of the trajectory ComputeNode trampolines' pure content-hash
/// cache keys (task π). `reify-eval/src/trajectory_ops.rs` keys its warm-state
/// result cache on [`SimulateTrajectoryCacheKey`] (`simulate_trajectory`) and
/// [`InputShapeCacheKey`] (`input_shape`) — the keys that decide a cache HIT vs
/// MISS (identical inputs ⇒ HIT; a profile control-point change ⇒ MISS). The
/// `trajectory::trampoline` module is `pub(crate)` (the θ/κ core types it
/// marshals are `pub(crate)`), so these keys reach reify-eval only via this
/// crate-root re-export — mirroring the `build_train_for_shaper` boundary above.
/// The `Value`→`Value` composers (`simulate_trajectory_value` /
/// `input_shape_value`) join this re-export as they land (steps 14 / 16):
/// [`simulate_trajectory_value`] runs the profile/mech/modal → `EndEffectorTrack`
/// forward-pass pipeline that `reify-eval`'s `simulate_trajectory_trampoline`
/// wraps, and [`input_shape_value`] runs the profile/shaper → shaped-`Profile`
/// command-shaping pipeline that `input_shape_trampoline` wraps (the θ/κ core
/// types they marshal are `pub(crate)`, so they must live here).
pub use trajectory::trampoline::{
    input_shape_value, simulate_trajectory_value, InputShapeCacheKey, SimulateTrajectoryCacheKey,
};

#[cfg(test)]
#[macro_use]
mod test_macros;

#[cfg(test)]
mod test_fixtures;

mod analysis;
mod complex;
mod construct;
mod dfm;
mod fea;
mod flexures;
mod geometry;
mod joints;
/// Public re-export of the geometric-joints δ (task 4398) Resolution-node seam: write a
/// relate-solved mount `Value::Frame` into a joint `Value::Map`'s `"origin"` field.
/// Called from `reify-eval/src/engine_build.rs` after `solve_scopes` (ζ) to stamp the
/// solved mount Frame into the mounted joint's `origin` before the FK walk consumes it
/// via `transform_at`'s `origin ∘ motion` pre-compose (KIN-OFFSET α, task 4331).
pub use joints::set_mount_origin;
mod linalg;
mod list;
mod loads;
pub mod dynamics;
pub mod loop_closure;
pub mod loop_closure_solver;
pub mod loop_closure_value;
mod matrix;
mod mechanism;
pub mod modal;
mod numeric;
mod orientation;
mod parse;
mod snapshot;
mod stackup;
mod registry_dispatch;
mod supports;
mod sweep;
mod tensegrity;
mod tolerancing;
mod trajectory;
mod trig;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    // Registry FIRST (PRD §7.3(3)): a name registered as a
    // `BindingKind::EvalBuiltin` row resolves here, ahead of the surviving
    // family dispatchers. `None` falls through exactly as any other family's
    // decline does — the same registry-first coexistence shape the compiler
    // ladder already uses. Hoisting cannot shadow a later arm: no other member
    // of this chain claims a registered name, pinned row-derived by
    // `tests/registry_dispatch_seed_parity.rs`'s no-shadowing sweep —
    // `try_dispatch_answers_for_exactly_the_eval_rows_at_their_declared_arities`
    // (the registry claims exactly its rows' declared arities) and
    // `try_dispatch_declines_every_name_the_registry_does_not_own` (a name a
    // later dispatcher owns falls through untouched).
    if let Some(v) = registry_dispatch::try_dispatch(name, args) {
        return v;
    }
    if let Some(v) = numeric::eval_numeric(name, args) {
        return v;
    }
    if let Some(v) = list::eval_list(name, args) {
        return v;
    }
    if let Some(v) = trig::eval_trig(name, args) {
        return v;
    }
    if let Some(v) = linalg::eval_linalg(name, args) {
        return v;
    }
    if let Some(v) = complex::eval_complex(name, args) {
        return v;
    }
    if let Some(v) = orientation::eval_orientation(name, args) {
        return v;
    }
    if let Some(v) = geometry::eval_geometry(name, args) {
        return v;
    }
    if let Some(v) = matrix::eval_matrix(name, args) {
        return v;
    }
    if let Some(v) = construct::eval_construct(name, args) {
        return v;
    }
    if let Some(v) = joints::eval_joints(name, args) {
        return v;
    }
    if let Some(v) = flexures::eval_flexures(name, args) {
        return v;
    }
    if let Some(v) = loads::eval_loads(name, args) {
        return v;
    }
    if let Some(v) = fea::eval_fea(name, args) {
        return v;
    }
    if let Some(v) = supports::eval_supports(name, args) {
        return v;
    }
    if let Some(v) = mechanism::eval_mechanism(name, args) {
        return v;
    }
    if let Some(v) = snapshot::eval_snapshot(name, args) {
        return v;
    }
    if let Some(v) = dynamics::eval::eval_dynamics(name, args) {
        return v;
    }
    if let Some(v) = stackup::eval_stackup(name, args) {
        return v;
    }
    if let Some(v) = dfm::eval_dfm(name, args) {
        return v;
    }
    if let Some(v) = tolerancing::eval_tolerancing(name, args) {
        return v;
    }
    if let Some(v) = sweep::eval_sweep(name, args) {
        return v;
    }
    if let Some(v) = trajectory::eval_trajectory(name, args) {
        return v;
    }
    if let Some(v) = tensegrity::eval_tensegrity(name, args) {
        return v;
    }
    Value::Undef
}

/// Expose the registry-keyed eval dispatcher to the registry seam test without
/// widening reify-stdlib's public API.
///
/// Taking an `EvalBuiltinId` — not a `&str` — is the point: it lets
/// `tests/registry_dispatch_seed_parity.rs` observe that eval dispatch is
/// genuinely keyed on the registry, which a test routed through the public
/// `eval_builtin(name, args)` alone could not distinguish from the old string
/// matchers.
///
/// # Stability
///
/// This function is intentionally named with `__` prefix to signal that it is
/// an internal test shim and **not part of the public API**. It may be removed
/// or changed at any time. Gated behind `feature = "test-support"` (or
/// `cfg(test)` for in-crate tests); not part of the released public API.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
// G-allow: task #6001 (registry α) — test-support-gated eval-seam shim,
// consumed by tests/registry_dispatch_seed_parity.rs (the §7.3(3) I-REG-2
// dispatch-parity pin).
pub fn __registry_dispatch_for_test(id: reify_builtins::EvalBuiltinId, args: &[Value]) -> Value {
    registry_dispatch::dispatch(id, args)
}

/// Expose the registry's NAME resolution to the registry seam test without
/// widening reify-stdlib's public API.
///
/// Why this exists next to [`__registry_dispatch_for_test`]: that shim takes
/// an `EvalBuiltinId`, so it structurally cannot observe how a `&str` becomes
/// one — it bypasses both `try_dispatch` and `reify_builtins::lookup`. This
/// one takes the name, so it is the only way a test can pin the `lookup`-keyed
/// resolution the public `eval_builtin` actually performs: that the registry
/// is consulted, is authoritative for exactly the `BindingKind::EvalBuiltin`
/// rows at their declared arities, and DECLINES every other name so a later
/// member of the dispatch chain keeps its own.
///
/// `registry_dispatch::try_dispatch` itself stays `pub(crate)`; this shim is
/// what keeps the crate's real surface unchanged.
///
/// # Stability
///
/// Same contract as [`__registry_dispatch_for_test`]: `__`-prefixed internal
/// test shim, **not part of the public API**, may be removed or changed at any
/// time. Gated behind `feature = "test-support"` (or `cfg(test)` for in-crate
/// tests).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
// G-allow: task #6001 (registry α) — test-support-gated eval-seam shim,
// consumed by tests/registry_dispatch_seed_parity.rs (the §7.3(3) no-shadowing
// sweep over the name path).
pub fn __try_dispatch_for_test(name: &str, args: &[Value]) -> Option<Value> {
    registry_dispatch::try_dispatch(name, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_function_returns_undef() {
        assert!(eval_builtin("foo", &[Value::Real(1.0)]).is_undef());
    }
}
