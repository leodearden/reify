//! Pure cache-key + `Value`↔core marshalling half of the trajectory
//! ComputeNode trampolines (`simulate_trajectory` + `input_shape`), task π
//! (3876; `docs/prds/v0_3/trajectory-input-shaping.md` §6/§11,
//! `docs/prds/v0_3/compute-node-contract.md` §4 GR-002).
//!
//! Mirrors the modal `free_vibration`/`transient_response` split
//! (`modal/trampoline.rs`) and the `inverse_dynamics` split
//! (`dynamics/trampoline.rs`): the engine-facing `ComputeFn` wrappers
//! (warm-state cache + cancellation) live in `reify-eval`
//! (`trajectory_ops.rs`), because `ComputeOutcome` / `OpaqueState` /
//! `CancellationHandle` are `reify-eval` (resp. `reify-ir`) types and the
//! dependency graph `reify-eval → reify-expr → reify-stdlib` forbids
//! `reify-stdlib` from depending on `reify-eval`.
//!
//! This module holds only the pure, `reify-eval`-free half:
//! - the content-hash cache keys (`SimulateTrajectoryCacheKey`,
//!   `InputShapeCacheKey`) the warm-state cache is keyed on;
//! - the `Value`↔core marshalling helpers (`value_to_multijoint_spline` /
//!   `value_to_modal_model` / `value_to_mechanism_model` /
//!   `track_data_to_value`), which must run inside `reify-stdlib` because the
//!   θ/κ core types (`MechanismModel` / `ModalModel` / `MultiJointSpline` /
//!   `EndEffectorTrackData`) are `pub(crate)` here;
//! - the two `Value`→`Value` composers (`simulate_trajectory_value` /
//!   `input_shape_value`) reify-eval calls (re-exported at the crate root,
//!   mirroring `reify_stdlib::build_train_for_shaper`);
//! - the three accessor impls (`end_effector_track_at` /
//!   `deviation_from_nominal_at` / `peak_deviation_at`) routed from
//!   `eval_trajectory`.
//!
//! Populated incrementally across task π's TDD steps (cache keys → marshalling
//! → composers → accessors).

#[cfg(test)]
mod tests {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    use super::SimulateTrajectoryCacheKey;

    /// A registry-free `Value::StructureInstance` with `type_name` + fields,
    /// mirroring the eval-side `mint_instance` shape (same fixture pattern as
    /// `dynamics/trampoline.rs` tests). Used to build distinguishable `Value`
    /// inputs whose `content_hash` folds in every field.
    fn instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// Minimal `PiecewisePolynomialProfile`-shaped fixture distinguished by a
    /// single control value `p` (folded into the content hash). The cache-key
    /// tests care only that distinct `p` ⇒ distinct hash and identical `p` ⇒
    /// identical hash, not the full marshalled shape (that is steps 5/6).
    fn profile(p: f64) -> Value {
        instance(
            "PiecewisePolynomialProfile",
            vec![("control".to_string(), Value::Real(p))],
        )
    }

    /// Minimal `mech` fixture — the simulate path takes `mech : Real`
    /// (trajectory_fns.ri), so the bare `Value::Real` is the canonical input.
    fn mech(m: f64) -> Value {
        Value::Real(m)
    }

    /// Minimal `ModalResult`-shaped fixture distinguished by a single mode
    /// frequency `f`.
    fn modal(f: f64) -> Value {
        instance("ModalResult", vec![("frequency".to_string(), Value::Real(f))])
    }

    // ── step-1: SimulateTrajectoryCacheKey::from_inputs / matches ───────────────

    /// (a) Two keys built from identical `(profile, mech, modal)` match — the
    /// cache-HIT condition.
    #[test]
    fn simulate_cache_key_matches_identical_inputs() {
        let p = profile(1.0);
        let m = mech(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &md);
        assert!(a.matches(&b), "identical (profile, mech, modal) must match");
    }

    /// (b) A different profile `Value` must NOT match — and the relation is
    /// symmetric.
    #[test]
    fn simulate_cache_key_differs_on_profile() {
        let m = mech(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&profile(1.0), &m, &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&profile(2.0), &m, &md);
        assert!(!a.matches(&b), "a different profile must MISS");
        assert!(!b.matches(&a), "matches() must be symmetric");
    }

    /// (c) A different mech `Value` must NOT match.
    #[test]
    fn simulate_cache_key_differs_on_mech() {
        let p = profile(1.0);
        let md = modal(10.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &mech(1.0), &md);
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &mech(2.0), &md);
        assert!(!a.matches(&b), "a different mech must MISS");
    }

    /// (d) A different modal `Value` must NOT match.
    #[test]
    fn simulate_cache_key_differs_on_modal() {
        let p = profile(1.0);
        let m = mech(1.0);
        let a = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &modal(10.0));
        let b = SimulateTrajectoryCacheKey::from_inputs(&p, &m, &modal(20.0));
        assert!(!a.matches(&b), "a different modal must MISS");
    }
}
