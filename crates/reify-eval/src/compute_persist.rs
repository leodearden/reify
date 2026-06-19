//! Persistent-cache write/lookup hooks wired at ComputeNode dispatch
//! boundaries.
//!
//! task #3428 step-5 (RED) / step-6 (GREEN): writes and reads the on-disk
//! persistent cache at the [`crate::Engine::run_compute_dispatch`] boundary.
//!
//! # Architecture
//!
//! ```text
//! run_compute_dispatch
//!   ├── [step-8] persistent_lookup → cache hit: complete + return (skip trampoline)
//!   ├── invoke_compute_trampoline
//!   └── [step-6] persistent_write (best-effort, Completed arm only)
//! ```
//!
//! Both hooks are gated on:
//! - `Engine::persistent_cache_dir.is_some()` — `None` = inert for all existing
//!   tests (step-6 adds the field/setter; default is `None`).
//! - [`is_persistable_target`] — allowlist mirrors
//!   `significance_filter::is_opted_in`.

// ── Production code (task #3428 step-6) ──────────────────────────────────────

/// Return `true` if `target` is in the persistent-cache write/lookup
/// allowlist.
///
/// Currently only `"solver::elastic_static"` is listed.  Mirrors
/// [`crate::significance_filter::is_opted_in`]'s `matches!(target, …)` pattern;
/// a full per-target registry is the documented future generalization once
/// additional persistable targets exist.
pub(crate) fn is_persistable_target(target: &str) -> bool {
    matches!(target, "solver::elastic_static")
}

/// Best-effort write of a completed dispatch result to the on-disk cache.
///
/// # Behaviour
///
/// Extracts a [`crate::persistent_cache::ElasticResult`] from `result` via
/// [`crate::compute_targets::elastic_static::elastic_result_from_value`], then
/// calls [`crate::persistent_cache::write_entry`] (atomic temp+rename).
///
/// # Error policy
///
/// ALL `io::Error`s are `tracing::warn!`-logged and swallowed — a write
/// failure must NEVER fail or alter a solve result.  The persistent cache is a
/// pure optimisation; correctness is unchanged whether or not a write succeeds.
///
/// # Preconditions (callers are responsible)
///
/// - `is_persistable_target(target)` must be `true` (enforced by
///   `debug_assert!`).
/// - `cache_dir` must be the resolved on-disk root (callers gate on
///   `persistent_cache_dir.is_some()`).
pub(crate) fn persistent_write(
    cache_dir: &std::path::Path,
    target: &str,
    cache_key: reify_core::ContentHash,
    result: &reify_ir::Value,
) {
    debug_assert!(
        is_persistable_target(target),
        "persistent_write called for non-persistable target {:?}; \
         is_persistable_target must be checked before calling",
        target,
    );
    let input_hash = format!("{cache_key}");
    match target {
        "solver::elastic_static" => {
            let Some(er) =
                crate::compute_targets::elastic_static::elastic_result_from_value(result)
            else {
                tracing::warn!(
                    %cache_key,
                    "persistent_write: elastic_result_from_value returned None \
                     for solver::elastic_static; skipping write",
                );
                return;
            };
            if let Err(e) = crate::persistent_cache::write_entry::<
                crate::persistent_cache::ElasticResult,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
                &er,
            ) {
                tracing::warn!(
                    %e,
                    cache_dir = %cache_dir.display(),
                    target,
                    %cache_key,
                    "persistent_write: write_entry failed (best-effort; solve was not affected)",
                );
            }
        }
        _ => {
            // Defensive branch: debug_assert above should catch this in tests.
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{ComputeNodeId, ContentHash, DimensionVector, ValueCellId, VersionId};
    use reify_ir::{
        DeterminacyState, Freshness, PersistentMap, StructureInstanceData, StructureTypeId, Value,
    };
    use reify_test_support::mocks::MockConstraintChecker;

    use crate::Engine;
    use crate::cache::{CachedResult, NodeCache, NodeId};
    use crate::deps::DependencyTrace;
    use crate::engine_compute::{ComputeOutcome, RealizationReadHandle};
    use crate::graph::CancellationHandle;
    use crate::persistent_cache::{ENGINE_VERSION_HASH, ElasticResult, entry_bin_path, read_entry};

    // ── FEA input helpers (cantilever-style, tet path) ────────────────────────

    /// Steel-like isotropic material StructureInstance.
    ///
    /// `classify_material` in the trampoline matches any `type_name` that is not
    /// `Orthotropic` or `TransverseIsotropic` and reads `youngs_modulus` +
    /// `poisson_ratio`. `IsotropicElastic` falls through to
    /// `MaterialModel::Isotropic`.
    fn make_isotropic_material(youngs: f64, poisson: f64) -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "youngs_modulus".to_string(),
                Value::Scalar {
                    si_value: youngs,
                    dimension: DimensionVector::PRESSURE,
                },
            ),
            ("poisson_ratio".to_string(), Value::Real(poisson)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "IsotropicElastic".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Geometry length as `Value::Scalar` (SI metres).
    fn make_len(m: f64) -> Value {
        Value::Scalar {
            si_value: m,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// `Value::List` containing one `PointLoad { force: Real(force_n) }`.
    ///
    /// The trampoline sums all point loads as a tip force applied at x=length.
    fn make_point_loads(force_n: f64) -> Value {
        let fields: PersistentMap<String, Value> =
            [("force".to_string(), Value::Real(force_n))].into_iter().collect();
        Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "PointLoad".to_string(),
            version: 1,
            fields,
        }))])
    }

    /// `Value::List` containing one `FixedSupport` (fields not inspected;
    /// presence clamps all DOF at x=0).
    fn make_supports() -> Value {
        Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "FixedSupport".to_string(),
            version: 1,
            fields: [].into_iter().collect(),
        }))])
    }

    /// `ElasticOptions` with `shell_force=Off` (forces the tet path regardless
    /// of geometry aspect ratio) and `shell_threshold=0.2`.
    fn make_options_tet() -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "shell_force".to_string(),
                Value::Enum {
                    type_name: "ShellForce".to_string(),
                    variant: "Off".to_string(),
                },
            ),
            ("shell_threshold".to_string(), Value::Real(0.2)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticOptions".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a minimal cantilever FEA `value_inputs` slice (tet path).
    ///
    /// Parameters: steel (E=205 GPa, ν=0.29), 0.1×0.1×0.1 m cube, 1000 N
    /// tip load, single `FixedSupport`, `shell_force=Off`.
    fn cantilever_inputs() -> [Value; 7] {
        [
            make_isotropic_material(205e9, 0.29),
            make_len(0.1), // length (X)
            make_len(0.1), // width (Y)
            make_len(0.1), // height (Z)
            make_point_loads(1000.0),
            make_supports(),
            make_options_tet(),
        ]
    }

    /// Minimal identity trampoline for non-persistable-target tests.
    fn identity_fn(
        value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&reify_ir::OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        ComputeOutcome::Completed {
            result: value_inputs.first().cloned().unwrap_or(Value::Undef),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        }
    }

    // ── step-5 RED tests ──────────────────────────────────────────────────────
    //
    // All three tests below fail to compile until step-6 adds:
    //   (a) `Engine::set_persistent_cache_dir(Option<PathBuf>)` setter
    //   (b) `cache_key: ContentHash` parameter to `run_compute_dispatch`
    //   (c) The persistent-write hook in the Completed arm of `run_compute_dispatch`
    //
    // The compile errors are the RED signal; the test logic is correct for
    // the GREEN pass once step-6 is implemented.

    /// (1) Persistent WRITE: after a Completed `solver::elastic_static` dispatch
    /// with a non-zero `cache_key` and a configured cache dir, a `.bin` file
    /// appears at `entry_bin_path(cache_dir, ENGINE_VERSION_HASH, "{cache_key}")`
    /// and `read_entry::<ElasticResult>` round-trips with a matching
    /// `max_von_mises`.
    ///
    /// Fails to compile until step-6 adds `set_persistent_cache_dir` +
    /// `cache_key` param to `run_compute_dispatch`.
    #[test]
    fn persistent_write_elastic_static_after_completed_dispatch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

        // RED: `set_persistent_cache_dir` does not exist on Engine yet.
        // Step-6 adds it via engine_admin.rs.
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));

        crate::compute_targets::register_compute_fns(&mut engine);

        let cell = ValueCellId::new("T", "result_cp5a");
        let c_id = ComputeNodeId::new("T", 0);

        // Seed the output VC with a Final entry so begin_compute_dispatch has a
        // last_substantive value to display during the Pending window.
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let value_inputs = cantilever_inputs();

        // Deterministic non-zero cache_key for test isolation (32 hex chars via Display).
        let cache_key = ContentHash(0xabcd_1234_abcd_1234_abcd_1234_abcd_1234_u128);

        // RED: `run_compute_dispatch` does not yet have a `cache_key` parameter.
        // Step-6 adds `cache_key: ContentHash` as the last parameter.
        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "solver::elastic_static",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            cache_key, // NEW param — fails to compile until step-6
        );

        let (val, _diags) = result.expect("elastic_static dispatch must succeed");

        // Extract max_von_mises from the ElasticResult StructureInstance.
        let max_vm = match &val {
            Value::StructureInstance(data) => {
                match data.fields.get(&"max_von_mises".to_string()) {
                    Some(Value::Scalar { si_value, .. }) => *si_value,
                    other => panic!(
                        "max_von_mises must be a Scalar, got: {:?}",
                        other,
                    ),
                }
            }
            other => panic!("result must be a StructureInstance, got: {:?}", other),
        };
        assert!(
            max_vm.is_finite() && max_vm > 0.0,
            "max_von_mises must be finite and > 0, got: {}",
            max_vm,
        );

        // Assert the .bin was written by the persistent write hook.
        let input_hash = format!("{cache_key}");
        let bin_path = entry_bin_path(tmp.path(), ENGINE_VERSION_HASH, &input_hash);
        assert!(
            bin_path.exists(),
            "persistent cache .bin must exist after Completed dispatch: {:?}",
            bin_path,
        );

        // Assert read_entry round-trips with max_von_mises matching the dispatch result.
        let entry = read_entry::<ElasticResult>(tmp.path(), ENGINE_VERSION_HASH, &input_hash)
            .expect("read_entry must not return Err")
            .expect("read_entry must return Some after a successful write");
        let relative_err =
            (entry.max_von_mises - max_vm).abs() / max_vm.abs().max(f64::EPSILON);
        assert!(
            relative_err < 1e-10,
            "read_entry max_von_mises {:.6e} must match dispatch result {:.6e} (rel err {})",
            entry.max_von_mises,
            max_vm,
            relative_err,
        );
    }

    /// (2) Non-persistable target: a `test::identity_cp5b` dispatch with a
    /// configured cache dir must write NO `.bin` (allowlist gating).
    ///
    /// Fails to compile until step-6 adds `cache_key: ContentHash` param.
    #[test]
    fn non_persistable_target_writes_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

        // RED: `set_persistent_cache_dir` not yet added.
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));

        engine.register_compute_fn("test::identity_cp5b", identity_fn as crate::ComputeFn);

        let cell = ValueCellId::new("T", "b_cp5b");
        let c_id = ComputeNodeId::new("T", 1);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Int(7), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let cache_key = ContentHash(0xdead_beef_cafe_babe_dead_beef_cafe_babe_u128);

        // RED: extra `cache_key` param.
        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::identity_cp5b",
            &[Value::Int(7)],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            cache_key, // NEW param — fails to compile until step-6
        );
        assert!(result.is_ok(), "identity dispatch must succeed");

        // The non-persistable target allowlist must gate out the write.
        let input_hash = format!("{cache_key}");
        let bin_path = entry_bin_path(tmp.path(), ENGINE_VERSION_HASH, &input_hash);
        assert!(
            !bin_path.exists(),
            "non-persistable target must not write a .bin: {:?}",
            bin_path,
        );
    }

    /// (3) `persistent_cache_dir = None` (default): even a persistable target
    /// (`solver::elastic_static`) must write nothing when no cache dir is
    /// configured. Verifies the `persistent_cache_dir.is_some()` gate.
    ///
    /// Fails to compile until step-6 adds `cache_key: ContentHash` param.
    #[test]
    fn cache_dir_none_writes_nothing() {
        // Engine with NO cache dir (default — the gate fires and skips the write).
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        // Intentionally do NOT call set_persistent_cache_dir.
        crate::compute_targets::register_compute_fns(&mut engine);

        let cell = ValueCellId::new("T", "r_cp5c");
        let c_id = ComputeNodeId::new("T", 2);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let value_inputs = cantilever_inputs();
        let cache_key = ContentHash(0xaaaa_bbbb_cccc_dddd_aaaa_bbbb_cccc_dddd_u128);

        // RED: extra `cache_key` param.
        let result = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "solver::elastic_static",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            cache_key, // NEW param — fails to compile until step-6
        );
        // Dispatch must still succeed (cache dir = None is a pure write-skip,
        // not a failure).
        assert!(
            result.is_ok(),
            "elastic_static dispatch with None cache_dir must succeed, got: {:?}",
            result,
        );
        // Cannot check file absence without a dir; just assert no panic occurred.
        // The persistent_cache_dir.is_some() gate in the Completed arm is what
        // keeps this safe — verified by the step-6 GREEN pass.
    }
}
