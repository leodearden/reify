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
