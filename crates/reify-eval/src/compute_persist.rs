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

// ── Production code (task #3428 step-6 / step-8) ─────────────────────────────

/// Return `true` if `target` is in the persistent-cache write/lookup
/// allowlist.
///
/// Listed targets: `"solver::elastic_static"` (task #3428),
/// `"solver::buckling"` (task #3459), and `"shell-extract::extract"`
/// (task #4071).  Note that the persistable allowlist is now a strict
/// superset of [`crate::significance_filter::is_opted_in`]'s allowlist
/// (`{elastic_static, buckling}`); `"shell-extract::extract"` is persistable
/// but NOT significance-opted-in.
pub(crate) fn is_persistable_target(target: &str) -> bool {
    matches!(
        target,
        "solver::elastic_static" | "solver::buckling" | "shell-extract::extract"
    )
}

/// Look up a prior result from the on-disk cache and reconstruct both the
/// result [`reify_ir::Value`] and the diagnostics its original solve emitted,
/// without re-running the trampoline.
///
/// Returns `Some((value, diagnostics))` on a hit (the caller should complete
/// the dispatch and return immediately, skipping `invoke_compute_trampoline`)
/// or `None` on a miss or any read error (caller falls through to the normal
/// invoke path).
///
/// The diagnostics come back through the same channel the trampoline's fresh
/// diagnostics use, so a warm serve is indistinguishable from a cold one to
/// every downstream consumer. `diagnostics` is empty for a solve that emitted
/// none — an empty list is a hit, not a miss.
///
/// Covered targets: `"solver::elastic_static"`, `"solver::buckling"`, and
/// `"shell-extract::extract"` (task #4071).
///
/// # Error policy
///
/// All `io::Error`s and deserialization failures are `tracing::warn!`-logged
/// and treated as a miss — identical to the corruption-recovery posture in
/// `read_entry` itself.  A lookup failure must never abort a solve.
///
/// # Preconditions (callers are responsible)
///
/// - `is_persistable_target(target)` must be `true` (enforced by
///   `debug_assert!`).
/// - `cache_dir` must be the resolved on-disk root (callers gate on
///   `persistent_cache_dir.is_some()`).
pub(crate) fn persistent_lookup(
    cache_dir: &std::path::Path,
    target: &str,
    cache_key: reify_core::ContentHash,
) -> Option<(reify_ir::Value, Vec<reify_core::Diagnostic>)> {
    debug_assert!(
        is_persistable_target(target),
        "persistent_lookup called for non-persistable target {:?}",
        target,
    );
    let input_hash = format!("{cache_key}");
    match target {
        "solver::elastic_static" => {
            match crate::persistent_cache::read_entry::<
                crate::persistent_cache::WithDiagnostics<crate::persistent_cache::ElasticResult>,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
            ) {
                Ok(Some(entry)) => Some((
                    crate::compute_targets::elastic_static::value_from_elastic_result(
                        &entry.value,
                    ),
                    entry.diagnostics,
                )),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        %e,
                        cache_dir = %cache_dir.display(),
                        target,
                        %cache_key,
                        "persistent_lookup: read_entry failed (treated as miss)",
                    );
                    None
                }
            }
        }
        "solver::buckling" => {
            match crate::persistent_cache::read_entry::<
                crate::persistent_cache::WithDiagnostics<
                    crate::persistent_cache::BucklingResultCache,
                >,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
            ) {
                Ok(Some(entry)) => Some((
                    crate::compute_targets::buckling::value_from_buckling_result(&entry.value),
                    entry.diagnostics,
                )),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        %e,
                        cache_dir = %cache_dir.display(),
                        target,
                        %cache_key,
                        "persistent_lookup: read_entry failed for solver::buckling \
                         (treated as miss)",
                    );
                    None
                }
            }
        }
        "shell-extract::extract" => {
            match crate::persistent_cache::read_entry::<
                crate::persistent_cache::WithDiagnostics<
                    reify_shell_extract::ShellExtractionResult,
                >,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
            ) {
                Ok(Some(entry)) => Some((
                    crate::shell_extract_compute::shell_extraction_result_to_value(&entry.value),
                    entry.diagnostics,
                )),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        %e,
                        cache_dir = %cache_dir.display(),
                        target,
                        %cache_key,
                        "persistent_lookup: read_entry failed for shell-extract::extract \
                         (treated as miss)",
                    );
                    None
                }
            }
        }
        _ => None,
    }
}

/// Best-effort write of a completed dispatch result to the on-disk cache.
///
/// # Behaviour
///
/// Extracts a typed cache container from `result` via the target-specific
/// bridge function, wraps it together with `diagnostics` in a
/// [`crate::persistent_cache::WithDiagnostics`] envelope, then calls
/// [`crate::persistent_cache::write_entry`] (atomic temp+rename).
///
/// `diagnostics` are the ones this dispatch's trampoline emitted. They are
/// stored so a later warm serve can replay them; without them the on-disk
/// cache would make every `W_*` warning first-run-only.
///
/// Covered targets: `"solver::elastic_static"`, `"solver::buckling"`, and
/// `"shell-extract::extract"` (task #4071).
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
    diagnostics: &[reify_core::Diagnostic],
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
                crate::persistent_cache::WithDiagnostics<crate::persistent_cache::ElasticResult>,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
                &crate::persistent_cache::WithDiagnostics {
                    diagnostics: diagnostics.to_vec(),
                    value: er,
                },
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
        "solver::buckling" => {
            let Some(brc) =
                crate::compute_targets::buckling::buckling_result_from_value(result)
            else {
                tracing::warn!(
                    %cache_key,
                    "persistent_write: buckling_result_from_value returned None \
                     for solver::buckling; skipping write",
                );
                return;
            };
            if let Err(e) = crate::persistent_cache::write_entry::<
                crate::persistent_cache::WithDiagnostics<
                    crate::persistent_cache::BucklingResultCache,
                >,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
                &crate::persistent_cache::WithDiagnostics {
                    diagnostics: diagnostics.to_vec(),
                    value: brc,
                },
            ) {
                tracing::warn!(
                    %e,
                    cache_dir = %cache_dir.display(),
                    target,
                    %cache_key,
                    "persistent_write: write_entry failed for solver::buckling \
                     (best-effort; solve was not affected)",
                );
            }
        }
        "shell-extract::extract" => {
            let Some(ser) =
                crate::shell_extract_compute::value_to_shell_extraction_result(result)
            else {
                tracing::warn!(
                    %cache_key,
                    "persistent_write: value_to_shell_extraction_result returned None \
                     for shell-extract::extract; skipping write",
                );
                return;
            };
            if let Err(e) = crate::persistent_cache::write_entry::<
                crate::persistent_cache::WithDiagnostics<
                    reify_shell_extract::ShellExtractionResult,
                >,
            >(
                cache_dir,
                crate::persistent_cache::ENGINE_VERSION_HASH,
                &input_hash,
                &crate::persistent_cache::WithDiagnostics {
                    diagnostics: diagnostics.to_vec(),
                    value: ser,
                },
            ) {
                tracing::warn!(
                    %e,
                    cache_dir = %cache_dir.display(),
                    target,
                    %cache_key,
                    "persistent_write: write_entry failed for shell-extract::extract \
                     (best-effort; solve was not affected)",
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
    use crate::persistent_cache::{
        ENGINE_VERSION_HASH, ElasticResult, WithDiagnostics, entry_bin_path, read_entry,
    };

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
                    payload: vec![],
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
            structured_detail: vec![],
        }
    }

    // ── step-7 RED helpers ────────────────────────────────────────────────────
    //
    // Trampoline counting infrastructure for lookup-skip assertions.
    // These trampolines use MODULE-LEVEL `AtomicUsize` counters so they can be
    // registered as plain `fn` pointers. Each test uses its own dedicated
    // counter + target-independent comparison (before/after delta) to avoid
    // inter-test interference when tests run in parallel.

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counting trampoline registered as `solver::elastic_static` in the HIT
    /// test (`persistent_lookup_hit_skips_trampoline`). Records each invocation
    /// in `DISPATCH_COUNT_CP7_HIT`; returns a placeholder Int so the test can
    /// observe WHETHER it was called without running the real FEA solver.
    ///
    /// In the RED state (no lookup path) this IS called and the HIT test's
    /// delta assertion fires immediately.  In the GREEN state it is NEVER called
    /// (the lookup path short-circuits before invoke).
    static DISPATCH_COUNT_CP7_HIT: AtomicUsize = AtomicUsize::new(0);

    fn counting_trampoline_cp7_hit(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_CP7_HIT.fetch_add(1, Ordering::SeqCst);
        // Return a recognisably wrong value so the HIT test's max_von_mises
        // assertion would also fail if the trampoline were somehow called.
        ComputeOutcome::Completed {
            result: Value::Int(-1),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
            structured_detail: vec![],
        }
    }

    /// Counting trampoline registered in the MISS test
    /// (`persistent_lookup_miss_invokes_trampoline`). Returns a placeholder so
    /// the miss test can assert the trampoline WAS invoked.
    static DISPATCH_COUNT_CP7_MISS: AtomicUsize = AtomicUsize::new(0);

    fn counting_trampoline_cp7_miss(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_CP7_MISS.fetch_add(1, Ordering::SeqCst);
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
            structured_detail: vec![],
        }
    }

    /// Build a minimal [`ElasticResult`] with a known `max_von_mises` so tests
    /// can seed the persistent cache without running the FEA solver.
    ///
    /// Layout: 2×2×2 grid (counts=[1,1,1] → 2 nodes per axis → 8 total nodes).
    /// All channel slabs are filled with a distinguishable constant so
    /// serialisation round-trips can be spot-checked if needed.
    fn minimal_elastic_result(max_vm: f64) -> crate::persistent_cache::ElasticResult {
        let total_nodes: usize = 2 * 2 * 2; // (counts[i]+1) per axis product
        crate::persistent_cache::ElasticResult {
            displacement: vec![1.0; total_nodes * 3],
            stress: vec![2.0; total_nodes * 9],
            max_von_mises: max_vm,
            converged: true,
            iterations: 5,
            solve_time_ms: 100,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [1.0, 1.0, 1.0],
            grid_counts: [1, 1, 1], // 2 interval-counts → 2 nodes per axis
            divergence: vec![3.0; total_nodes],
            gradient: vec![4.0; total_nodes * 9],
            curl: vec![5.0; total_nodes * 3],
            aposteriori: None,
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

        let (val, _diags, _) = result.expect("elastic_static dispatch must succeed");

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
        let entry =
            read_entry::<WithDiagnostics<ElasticResult>>(tmp.path(), ENGINE_VERSION_HASH, &input_hash)
                .expect("read_entry must not return Err")
                .expect("read_entry must return Some after a successful write")
                .value;
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

    // ── step-7 RED tests ──────────────────────────────────────────────────────
    //
    // Both tests FAIL until step-8 adds `persistent_lookup` to
    // `compute_persist.rs` and wires the lookup-before-invoke in
    // `run_compute_dispatch`.  The primary RED signal is the delta on the
    // respective `DISPATCH_COUNT_CP7_*` counter: in the RED state the trampoline
    // IS called (delta == 1); in the GREEN state it is skipped (delta == 0) on a
    // hit.

    /// (4) HIT: a persistent lookup short-circuits the trampoline.
    ///
    /// Seeds a persistent entry with `max_von_mises = 42.0`, then dispatches
    /// with the SAME `cache_key` on a fresh engine that has the counting
    /// trampoline `counting_trampoline_cp7_hit` registered for
    /// `"solver::elastic_static"`.
    ///
    /// Asserts (in GREEN state):
    /// (a) the counting trampoline was NOT invoked (delta == 0);
    /// (b) Ok((result, _)) with `result.max_von_mises ≈ 42.0`;
    /// (c) the output VC is `Freshness::Final`;
    /// (d) `engine.persistent_hit_count() == 1`.
    ///
    /// In RED state: the trampoline IS invoked (delta == 1), failing assert (a).
    #[test]
    fn persistent_lookup_hit_skips_trampoline() {
        use crate::persistent_cache::write_entry;

        let tmp = tempfile::TempDir::new().unwrap();
        let known_vm = 42.0_f64;
        let er = minimal_elastic_result(known_vm);

        // Seed the on-disk cache entry for a known cache_key.
        let cache_key = ContentHash(0xf00d_beef_cafe_babe_f00d_beef_cafe_babe_u128);
        let input_hash = format!("{cache_key}");
        write_entry::<WithDiagnostics<ElasticResult>>(
            tmp.path(),
            crate::persistent_cache::ENGINE_VERSION_HASH,
            &input_hash,
            &WithDiagnostics {
                diagnostics: Vec::new(),
                value: er,
            },
        )
        .expect("test seed write_entry must succeed");

        // Fresh engine — same cache dir, counting trampoline registered.
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
        engine.register_compute_fn(
            "solver::elastic_static",
            counting_trampoline_cp7_hit as crate::ComputeFn,
        );

        let cell = ValueCellId::new("T", "r_cp7hit");
        let c_id = ComputeNodeId::new("T", 70);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Take snapshot of dispatch counter BEFORE the call.
        let count_before = DISPATCH_COUNT_CP7_HIT.load(Ordering::SeqCst);

        // RED: `persistent_lookup` does not exist yet — step-8 will add it.
        // In RED state the trampoline IS called (count_before+1), making (a) fail.
        let result = engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "solver::elastic_static",
                &[], // value_inputs not needed — lookup returns seeded result
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                cache_key,
            )
            .expect("dispatch must succeed (hit path returns seeded result)");

        let (val, _diags, _) = result;
        let count_after = DISPATCH_COUNT_CP7_HIT.load(Ordering::SeqCst);

        // (a) Trampoline must NOT have been invoked on a persistent hit.
        //     This is the primary RED signal: fails when no lookup path exists.
        assert_eq!(
            count_after - count_before,
            0,
            "persistent lookup HIT must skip the trampoline (delta={}); \
             step-8 adds the lookup-before-invoke path",
            count_after - count_before,
        );

        // (b) Result max_von_mises must match the seeded entry.
        let max_vm = match &val {
            Value::StructureInstance(data) => {
                match data.fields.get("max_von_mises") {
                    Some(Value::Scalar { si_value, .. }) => *si_value,
                    other => panic!(
                        "max_von_mises must be Scalar, got: {:?}", other
                    ),
                }
            }
            other => panic!("result must be StructureInstance, got: {:?}", other),
        };
        let rel_err = (max_vm - known_vm).abs() / known_vm.abs().max(f64::EPSILON);
        assert!(
            rel_err < 1e-10,
            "result max_von_mises {max_vm:.6e} must match seeded {known_vm:.6e} (rel_err={rel_err})",
        );

        // (c) Output VC must flip to Freshness::Final after a lookup hit.
        assert!(
            matches!(
                engine.freshness(&NodeId::Value(cell.clone())),
                Freshness::Final
            ),
            "output VC must be Final after persistent lookup hit",
        );

        // (d) Hit counter must have incremented exactly once.
        assert_eq!(
            engine.persistent_hit_count(),
            1,
            "persistent_hit_count must be 1 after one lookup hit",
        );
    }

    /// (5) MISS: a lookup miss (different `cache_key`) falls through to the
    /// trampoline.
    ///
    /// Seeds a persistent entry under one key, then dispatches with a
    /// DIFFERENT `cache_key` (no entry).  Asserts:
    /// (a) the counting trampoline WAS invoked (delta == 1);
    /// (b) `engine.persistent_miss_count() == 1`.
    ///
    /// In RED state this test PASSES (the trampoline is always called),
    /// meaning the RED→GREEN transition for the MISS test is: once step-8
    /// wires the lookup, the MISS case must still call the trampoline (correct
    /// fall-through) — so (a) and (b) hold in both RED and GREEN.
    ///
    /// The MISS test is included alongside the HIT test to pin the fall-through
    /// behaviour and exercise `persistent_miss_count`.
    #[test]
    fn persistent_lookup_miss_invokes_trampoline_and_increments_miss_count() {
        use crate::persistent_cache::write_entry;

        let tmp = tempfile::TempDir::new().unwrap();
        let er = minimal_elastic_result(99.0);

        // Seed a cache entry under KEY_A.
        let key_a = ContentHash(0x1111_2222_3333_4444_1111_2222_3333_4444_u128);
        let input_hash_a = format!("{key_a}");
        write_entry::<WithDiagnostics<ElasticResult>>(
            tmp.path(),
            crate::persistent_cache::ENGINE_VERSION_HASH,
            &input_hash_a,
            &WithDiagnostics {
                diagnostics: Vec::new(),
                value: er,
            },
        )
        .expect("test seed write_entry must succeed");

        // Dispatch with KEY_B (no entry → miss → trampoline called).
        let key_b = ContentHash(0x5555_6666_7777_8888_5555_6666_7777_8888_u128);

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
        engine.register_compute_fn(
            "solver::elastic_static",
            counting_trampoline_cp7_miss as crate::ComputeFn,
        );

        let cell = ValueCellId::new("T", "r_cp7miss");
        let c_id = ComputeNodeId::new("T", 71);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let count_before = DISPATCH_COUNT_CP7_MISS.load(Ordering::SeqCst);

        let _ = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "solver::elastic_static",
            &[],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            key_b, // no persistent entry for this key
        );

        let count_after = DISPATCH_COUNT_CP7_MISS.load(Ordering::SeqCst);

        // (a) Trampoline MUST be called on a miss (fall-through).
        assert_eq!(
            count_after - count_before,
            1,
            "persistent lookup MISS must invoke the trampoline (delta={})",
            count_after - count_before,
        );

        // (b) miss_count must increment.
        //     In RED state: miss_count == 0 (no lookup path to increment it).
        //     This is a SECONDARY RED signal for step-8.
        assert_eq!(
            engine.persistent_miss_count(),
            1,
            "persistent_miss_count must be 1 after one lookup miss (step-8 \
             increments it when is_persistable_target && lookup returns None)",
        );
    }

    // ── step-5 RED tests (task #3459) — buckling persistent-cache ────────────
    //
    // (a) allowlist: `is_persistable_target("solver::buckling")` must be `true`.
    // (b) HIT: seed a BucklingResultCache entry, dispatch with matching key on a
    //     fresh engine with a counting trampoline → trampoline NOT called (delta=0),
    //     hit_count==1, output eigenvalue matches seed.
    // (c) MISS: different key → trampoline WAS called (delta=1), miss_count==1.
    //
    // RED signal: (a) fails immediately (`is_persistable_target` returns `false`
    // for "solver::buckling" until step-6 adds it to the `matches!` arm).

    /// Counting trampoline for buckling HIT test.
    /// Returns `Value::Int(-99)` so any eigenvalue assertion would also catch an
    /// accidental trampoline call.
    static DISPATCH_COUNT_CP9_BUCK_HIT: AtomicUsize = AtomicUsize::new(0);

    fn counting_trampoline_cp9_buck_hit(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_CP9_BUCK_HIT.fetch_add(1, Ordering::SeqCst);
        ComputeOutcome::Completed {
            result: Value::Int(-99),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
            structured_detail: vec![],
        }
    }

    /// Counting trampoline for buckling MISS test.
    /// Returns `Value::Int(0)` — the MISS test only checks delta and miss_count.
    static DISPATCH_COUNT_CP9_BUCK_MISS: AtomicUsize = AtomicUsize::new(0);

    fn counting_trampoline_cp9_buck_miss(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_CP9_BUCK_MISS.fetch_add(1, Ordering::SeqCst);
        ComputeOutcome::Completed {
            result: Value::Int(0),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
            structured_detail: vec![],
        }
    }

    /// Build a minimal [`BucklingResultCache`] for test seeding.
    ///
    /// Layout: 2 modes, each with 2 active nodes (mode_shape_stride=6 f64s),
    /// 1×1×1 grid (counts=[1,1,1] → 2 nodes per axis → 8 total nodes).
    ///
    /// Eigenvalues: [1.5, 3.0] — the HIT test checks `eigenvalues[0] ≈ 1.5`.
    fn minimal_buckling_result_cache() -> crate::persistent_cache::BucklingResultCache {
        let total_nodes: usize = 2 * 2 * 2; // (counts[i]+1)^axis product
        crate::persistent_cache::BucklingResultCache {
            eigenvalues: vec![1.5_f64, 3.0],
            // mode_shape_stride = 6 (2 nodes × 3 xyz); 2 modes → 12 f64s total.
            mode_shapes: vec![
                0.1, 0.2, 0.3, 1.1, 0.4, 0.5, // mode 0
                0.6, 0.7, 0.8, 1.6, 0.9, 1.0, // mode 1
            ],
            base_node_positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            converged: true,
            iterations: 0,
            ps_displacement: vec![0.1; total_nodes * 3],
            ps_stress: vec![0.2; total_nodes * 9],
            ps_max_von_mises: 42.0,
            ps_converged: true,
            ps_iterations: 0,
            ps_grid_bounds_min: [0.0, 0.0, 0.0],
            ps_grid_bounds_max: [1.0, 1.0, 1.0],
            ps_grid_counts: [1, 1, 1],
            solve_time_ms: 0,
        }
    }

    /// (a) Allowlist: `is_persistable_target("solver::buckling")` must be `true`.
    ///
    /// RED until step-6 adds `"solver::buckling"` to the `matches!` arm.
    #[test]
    fn buckling_is_persistable_target() {
        assert!(
            super::is_persistable_target("solver::buckling"),
            "solver::buckling must be in the persistable-target allowlist \
             (step-6 adds it to is_persistable_target)",
        );
    }

    /// (b) HIT: persistent lookup short-circuits the buckling trampoline.
    ///
    /// Seeds a `BucklingResultCache` entry with `eigenvalues[0] = 1.5`, then
    /// dispatches with the SAME `cache_key` on a fresh engine that has the
    /// counting trampoline registered for `"solver::buckling"`.
    ///
    /// Asserts (GREEN state):
    /// (a) the counting trampoline was NOT invoked (delta == 0);
    /// (b) modes[0].eigenvalue ≈ 1.5 (seed value);
    /// (c) output VC is `Freshness::Final`;
    /// (d) `engine.persistent_hit_count() == 1`.
    ///
    /// RED signal: `is_persistable_target("solver::buckling")` is `false` → no
    /// lookup attempted → trampoline IS called (delta==1), failing (a).
    #[test]
    fn persistent_lookup_hit_skips_buckling_trampoline() {
        use crate::persistent_cache::{BucklingResultCache, ENGINE_VERSION_HASH, write_entry};

        let tmp = tempfile::TempDir::new().unwrap();
        let brc = minimal_buckling_result_cache();
        let known_eigenvalue = brc.eigenvalues[0]; // 1.5

        // Seed the on-disk cache entry for a known cache_key.
        let cache_key = ContentHash(0xb0c5_1234_b0c5_5678_b0c5_1234_b0c5_5678_u128);
        let input_hash = format!("{cache_key}");
        write_entry::<WithDiagnostics<BucklingResultCache>>(
            tmp.path(),
            ENGINE_VERSION_HASH,
            &input_hash,
            &WithDiagnostics {
                diagnostics: Vec::new(),
                value: brc,
            },
        )
        .expect("test seed write_entry must succeed");

        // Fresh engine — same cache dir, counting trampoline for "solver::buckling".
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
        engine.register_compute_fn(
            "solver::buckling",
            counting_trampoline_cp9_buck_hit as crate::ComputeFn,
        );

        let cell = ValueCellId::new("T", "r_cp9_buck_hit");
        let c_id = ComputeNodeId::new("T", 90);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let count_before = DISPATCH_COUNT_CP9_BUCK_HIT.load(Ordering::SeqCst);

        let result = engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "solver::buckling",
                &[], // value_inputs not needed — lookup returns seeded result
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                cache_key,
            )
            .expect("dispatch must succeed (hit path returns seeded BucklingResult)");

        let (val, _diags, _) = result;
        let count_after = DISPATCH_COUNT_CP9_BUCK_HIT.load(Ordering::SeqCst);

        // (a) Trampoline must NOT have been invoked on a persistent hit.
        //     Primary RED signal: fails when no lookup path exists.
        assert_eq!(
            count_after - count_before,
            0,
            "persistent lookup HIT must skip the buckling trampoline (delta={}); \
             step-6 adds the lookup arm for solver::buckling",
            count_after - count_before,
        );

        // (b) modes[0].eigenvalue must match the seeded 1.5.
        let eigenvalue = match &val {
            Value::StructureInstance(data) => {
                match data.fields.get("modes") {
                    Some(Value::List(modes)) => match modes.first() {
                        Some(Value::StructureInstance(mode_data)) => {
                            match mode_data.fields.get("eigenvalue") {
                                Some(Value::Real(r)) => *r,
                                other => panic!(
                                    "modes[0].eigenvalue must be Real, got: {:?}", other
                                ),
                            }
                        }
                        other => panic!("modes[0] must be StructureInstance, got: {:?}", other),
                    },
                    other => panic!("modes must be List, got: {:?}", other),
                }
            }
            other => panic!("result must be BucklingResult StructureInstance, got: {:?}", other),
        };
        let rel_err = (eigenvalue - known_eigenvalue).abs()
            / known_eigenvalue.abs().max(f64::EPSILON);
        assert!(
            rel_err < 1e-10,
            "modes[0].eigenvalue {eigenvalue:.6e} must match seeded {known_eigenvalue:.6e} \
             (rel_err={rel_err})",
        );

        // (c) Output VC must be Freshness::Final after a lookup hit.
        assert!(
            matches!(
                engine.freshness(&NodeId::Value(cell.clone())),
                Freshness::Final
            ),
            "output VC must be Final after persistent buckling lookup hit",
        );

        // (d) Hit counter must have incremented exactly once.
        assert_eq!(
            engine.persistent_hit_count(),
            1,
            "persistent_hit_count must be 1 after one buckling lookup hit",
        );
    }

    /// (c) MISS: a different key falls through to the buckling trampoline.
    ///
    /// Seeds an entry under KEY_A, dispatches with KEY_B → miss → trampoline
    /// called once.
    ///
    /// Asserts:
    /// (a) counting trampoline WAS invoked (delta == 1);
    /// (b) `engine.persistent_miss_count() == 1`.
    ///
    /// RED signal: `is_persistable_target("solver::buckling")` is `false` →
    /// no lookup/miss increment → (b) fails (miss_count stays 0).
    #[test]
    fn persistent_lookup_miss_invokes_buckling_trampoline() {
        use crate::persistent_cache::{BucklingResultCache, ENGINE_VERSION_HASH, write_entry};

        let tmp = tempfile::TempDir::new().unwrap();
        let brc = minimal_buckling_result_cache();

        // Seed under KEY_A.
        let key_a = ContentHash(0xaaaa_bcde_1234_5678_aaaa_bcde_1234_5678_u128);
        let input_hash_a = format!("{key_a}");
        write_entry::<WithDiagnostics<BucklingResultCache>>(
            tmp.path(),
            ENGINE_VERSION_HASH,
            &input_hash_a,
            &WithDiagnostics {
                diagnostics: Vec::new(),
                value: brc,
            },
        )
        .expect("test seed write_entry must succeed");

        // Dispatch with KEY_B — no persistent entry → miss.
        let key_b = ContentHash(0xbbbb_dcef_8765_4321_bbbb_dcef_8765_4321_u128);

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
        engine.register_compute_fn(
            "solver::buckling",
            counting_trampoline_cp9_buck_miss as crate::ComputeFn,
        );

        let cell = ValueCellId::new("T", "r_cp9_buck_miss");
        let c_id = ComputeNodeId::new("T", 91);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        let count_before = DISPATCH_COUNT_CP9_BUCK_MISS.load(Ordering::SeqCst);

        let _ = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "solver::buckling",
            &[],
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
            key_b, // no persistent entry for this key
        );

        let count_after = DISPATCH_COUNT_CP9_BUCK_MISS.load(Ordering::SeqCst);

        // (a) Trampoline MUST be called on a miss (fall-through).
        assert_eq!(
            count_after - count_before,
            1,
            "persistent lookup MISS must invoke the buckling trampoline (delta={})",
            count_after - count_before,
        );

        // (b) miss_count must increment exactly once.
        //     RED signal: miss_count stays 0 when is_persistable_target is false.
        assert_eq!(
            engine.persistent_miss_count(),
            1,
            "persistent_miss_count must be 1 after one buckling lookup miss \
             (step-6 adds the lookup arm that increments it on a miss)",
        );
    }

    /// (a) Allowlist: `is_persistable_target("shell-extract::extract")` must be `true`.
    ///
    /// RED until step-2 (task #4071) adds `"shell-extract::extract"` to the `matches!` arm in
    /// `is_persistable_target`.
    #[test]
    fn shell_extract_is_persistable_target() {
        assert!(
            super::is_persistable_target("shell-extract::extract"),
            "shell-extract::extract must be in the persistable-target allowlist \
             (task #4071 step-2 adds it to is_persistable_target)",
        );
    }

    // ── Diagnostics carried across the persist bridge (task 7245) ─────────────
    //
    // The bridge is the round trip that decides whether a warm serve can replay
    // what the cold solve said. The table below pins it as TARGET-AGNOSTIC and
    // SEVERITY-GENERIC at once: the `WithDiagnostics` envelope is generic over
    // both, so neither a solver target nor a severity may be special-cased.

    /// A persistable solver diagnostic of `severity`, shaped the way a solver
    /// emits one: a coded message, a machine-readable candidate list, no labels.
    ///
    /// Every severity carries `DiagnosticCode::ShellTooThick`, so the name-based
    /// on-disk code encoding is exercised in each cell rather than only in the
    /// Warning one.
    fn solver_diagnostic(severity: reify_core::Severity) -> reify_core::Diagnostic {
        let message = format!(
            "shell candidate too thick for shell elements; falling back to tet mesh \
             (severity {})",
            severity.as_wire_str(),
        );
        let base = match severity {
            reify_core::Severity::Info => reify_core::Diagnostic::info(message),
            reify_core::Severity::Warning => reify_core::Diagnostic::warning(message),
            reify_core::Severity::Error => reify_core::Diagnostic::error(message),
        };
        base.with_code(reify_core::DiagnosticCode::ShellTooThick)
            .with_candidates(["tet", "hex"])
    }

    /// A `solver::elastic_static` result `Value` in the shape the bridge's
    /// `elastic_result_from_value` reader expects.
    fn elastic_static_cache_value() -> Value {
        crate::compute_targets::elastic_static::value_from_elastic_result(&minimal_elastic_result(
            42.0,
        ))
    }

    /// A `solver::buckling` result `Value` in the shape the bridge's
    /// `buckling_result_from_value` reader expects.
    fn buckling_cache_value() -> Value {
        crate::compute_targets::buckling::value_from_buckling_result(
            &minimal_buckling_result_cache(),
        )
    }

    /// The persist bridge must replay a diagnostic of ANY severity on EVERY
    /// solver target, verbatim in every field a consumer can key off.
    ///
    /// Why a table rather than one fixture per case: "regardless of severity" is
    /// an invariant of the bridge, not a property of whichever diagnostic a test
    /// author happened to pick. Pinning the whole cross product makes a
    /// severity-conditional regression — a `.filter(|d| d.severity !=
    /// Severity::Error)` slipped into `persistent_write`, say — impossible to
    /// land green. The Warning-only coverage this test replaces permitted
    /// exactly that.
    #[test]
    fn persist_bridge_replays_every_severity_on_every_solver_target() {
        use reify_core::Severity;

        let targets: [(&str, fn() -> Value); 2] = [
            ("solver::elastic_static", elastic_static_cache_value),
            ("solver::buckling", buckling_cache_value),
        ];
        let severities = [Severity::Info, Severity::Warning, Severity::Error];

        // ONE cache dir for every cell, so each cell's KEY is what selects its
        // own entry and an aliasing bug cannot hide behind per-cell isolation.
        let tmp = tempfile::TempDir::new().unwrap();

        for (severity_index, severity) in severities.into_iter().enumerate() {
            for (target_index, (target, build_value)) in targets.into_iter().enumerate() {
                let cell = format!("{target} / {}", severity.as_wire_str());
                let cache_key = ContentHash(
                    0x7245_0010_7245_0010_7245_0010_0000_0000_u128
                        | ((severity_index as u128) << 8)
                        | target_index as u128,
                );
                let written = solver_diagnostic(severity);
                let value = build_value();

                super::persistent_write(
                    tmp.path(),
                    target,
                    cache_key,
                    &value,
                    std::slice::from_ref(&written),
                );

                let (got_value, got_diags) = super::persistent_lookup(tmp.path(), target, cache_key)
                    .unwrap_or_else(|| panic!("{cell}: the entry just written must be a hit"));

                assert_eq!(
                    got_value.content_hash(),
                    value.content_hash(),
                    "{cell}: carrying diagnostics must not perturb the reconstructed Value",
                );
                assert_eq!(
                    got_diags.len(),
                    1,
                    "{cell}: exactly the one written diagnostic must be replayed, \
                     got {got_diags:?}",
                );
                let got = &got_diags[0];
                assert_eq!(
                    got.severity, severity,
                    "{cell}: severity must survive the warm serve — the persist path \
                     must neither drop nor downgrade a diagnostic by severity",
                );
                assert_eq!(got.message, written.message, "{cell}: message must survive");
                assert_eq!(
                    got.code,
                    Some(reify_core::DiagnosticCode::ShellTooThick),
                    "{cell}: downstream consumers key off DiagnosticCode, not message \
                     substrings",
                );
                assert_eq!(
                    got.candidates, written.candidates,
                    "{cell}: the machine-readable candidate list must survive",
                );
            }
        }
    }

    // ── Cold -> warm dispatch round trip, Error severity (task 7245) ──────────
    //
    // Every other diagnostics assertion either stops at the persist bridge or
    // SEEDS the on-disk entry with `write_entry` directly, so the COLD write
    // half of the hit path is never exercised end to end at the dispatch
    // boundary. These two tests drive `run_compute_dispatch` twice over one
    // cache dir and close that gap at the severity that matters most.
    //
    // Why Error specifically: an Error-severity diagnostic returned inside
    // `ComputeOutcome::Completed` does not by itself fail the solve, but
    // `reify eval` and `reify build` DO gate their exit code on
    // `Severity::Error`. So a Completed+Error solve exits nonzero cold — and if
    // the Error is not replayed, the SECOND eval of the same scene exits ZERO.
    // A silent exit-code flip between run 1 and run 2 is the sharpest form of
    // this task's harm, and these tests forbid it.
    //
    // The Error must come from a STUB trampoline, not a `.ri` fixture: every
    // `Diagnostic::error` in compute_targets/{elastic_static,buckling}.rs sits
    // on a `ComputeOutcome::Failed` arm, and only the `Completed` arm reaches
    // `persistent_write`, so no fixture on main can produce a persisted
    // Error-severity solver diagnostic. Task #7079 is what will make this shape
    // reachable from real input.

    /// The Error-severity diagnostic the stub trampolines emit on a Completed
    /// outcome.
    ///
    /// `FeaLoadKindUnsupported` is the workspace's existing Error-severity
    /// "declared, and explicitly not honored rather than silently no-op'd"
    /// code, which makes it the closest present-day stand-in for the
    /// `E_PARAM_NOT_HONORED` task #7079 will mint on exactly this shape.
    fn unhonored_param_error() -> reify_core::Diagnostic {
        reify_core::Diagnostic::error("solver: unsupported FEA load kind 'TractionLoad'")
            .with_code(reify_core::DiagnosticCode::FeaLoadKindUnsupported)
    }

    static DISPATCH_COUNT_ERR_ELASTIC: AtomicUsize = AtomicUsize::new(0);

    /// Stub `solver::elastic_static`: Completes with a persistable result AND an
    /// Error, counting each invocation so the warm run can prove it was skipped.
    fn erroring_elastic_trampoline(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_ERR_ELASTIC.fetch_add(1, Ordering::SeqCst);
        ComputeOutcome::Completed {
            result: elastic_static_cache_value(),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![unhonored_param_error()],
            structured_detail: vec![],
        }
    }

    static DISPATCH_COUNT_ERR_BUCKLING: AtomicUsize = AtomicUsize::new(0);

    /// Stub `solver::buckling`, same shape as [`erroring_elastic_trampoline`].
    fn erroring_buckling_trampoline(
        _vi: &[Value],
        _ri: &[RealizationReadHandle],
        _opts: &Value,
        _prior: Option<&reify_ir::OpaqueState>,
        _cancel: &CancellationHandle,
    ) -> ComputeOutcome {
        DISPATCH_COUNT_ERR_BUCKLING.fetch_add(1, Ordering::SeqCst);
        ComputeOutcome::Completed {
            result: buckling_cache_value(),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![unhonored_param_error()],
            structured_detail: vec![],
        }
    }

    /// Build a fresh `Engine` over `cache_dir` and run one dispatch of `target`.
    ///
    /// Returns the engine (so the caller can read its hit/miss counters) and the
    /// diagnostics the dispatch handed back — the same channel a cold solve's
    /// fresh diagnostics arrive on, which is the whole point of the replay.
    fn dispatch_over_cache_dir(
        cache_dir: &std::path::Path,
        target: &'static str,
        trampoline: crate::ComputeFn,
        cell: &ValueCellId,
        c_id: &ComputeNodeId,
        cache_key: ContentHash,
    ) -> (Engine, Vec<reify_core::Diagnostic>) {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.set_persistent_cache_dir(Some(cache_dir.to_path_buf()));
        engine.register_compute_fn(target, trampoline);
        engine.cache_store_mut().put(
            NodeId::Value(cell.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Undef, DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );
        let (_value, diagnostics, _detail) = engine
            .run_compute_dispatch(
                c_id,
                std::slice::from_ref(cell),
                target,
                &[],
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(2),
                cache_key,
            )
            .unwrap_or_else(|e| panic!("{target}: dispatch must succeed, got {e:?}"));
        (engine, diagnostics)
    }

    /// Assert exactly one diagnostic came back and it is the stub's Error,
    /// intact in every field a consumer gates on.
    fn assert_is_the_unhonored_param_error(what: &str, diags: &[reify_core::Diagnostic]) {
        assert_eq!(
            diags.len(),
            1,
            "{what}: exactly the one emitted diagnostic is expected, got {diags:?}",
        );
        assert_eq!(
            diags[0].severity,
            reify_core::Severity::Error,
            "{what}: severity must be Error — this is what `reify eval` gates its \
             exit code on, so losing it flips a nonzero exit to zero",
        );
        assert_eq!(diags[0].message, unhonored_param_error().message, "{what}");
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::FeaLoadKindUnsupported),
            "{what}: the code must survive",
        );
    }

    /// Dispatch `target` COLD on one engine and WARM on a fresh engine over the
    /// SAME cache dir, and assert the warm run serves the Error off disk rather
    /// than re-producing it.
    fn assert_cold_then_warm_dispatch_replays_the_error(
        target: &'static str,
        trampoline: crate::ComputeFn,
        invocations: &AtomicUsize,
        cache_key: ContentHash,
    ) {
        let tmp = tempfile::TempDir::new().unwrap();
        // Derived from the target so the two callers cannot collide on a name.
        let cell = ValueCellId::new("T", &format!("r_cp22_{}", target.replace("::", "_")));
        let c_id = ComputeNodeId::new("T", 220);

        let before = invocations.load(Ordering::SeqCst);

        // ── COLD: no entry on disk, so the trampoline runs and its Error is
        //    persisted alongside the result.
        let (engine_a, cold_diags) =
            dispatch_over_cache_dir(tmp.path(), target, trampoline, &cell, &c_id, cache_key);
        let after_cold = invocations.load(Ordering::SeqCst);

        assert_eq!(
            after_cold - before,
            1,
            "{target} cold: the trampoline must run when nothing is on disk",
        );
        assert_eq!(
            engine_a.persistent_miss_count(),
            1,
            "{target} cold: the first dispatch must be a MISS",
        );
        assert_eq!(
            engine_a.persistent_hit_count(),
            0,
            "{target} cold: the first dispatch must not hit",
        );
        assert_is_the_unhonored_param_error(&format!("{target} cold"), &cold_diags);

        // ── WARM: a FRESH engine over the same dir — no in-process state
        //    survives, so anything it reports came off disk.
        let (engine_b, warm_diags) =
            dispatch_over_cache_dir(tmp.path(), target, trampoline, &cell, &c_id, cache_key);
        let after_warm = invocations.load(Ordering::SeqCst);

        assert_eq!(
            after_warm - after_cold,
            0,
            "{target} warm: the trampoline must NOT run again — the Error has to be \
             REPLAYED from disk, not re-produced, or this test would pass even with \
             the cache path removed entirely",
        );
        assert_eq!(
            engine_b.persistent_hit_count(),
            1,
            "{target} warm: the second dispatch must be a HIT",
        );
        assert_eq!(
            engine_b.persistent_miss_count(),
            0,
            "{target} warm: the second dispatch must not miss",
        );
        assert_is_the_unhonored_param_error(&format!("{target} warm"), &warm_diags);
    }

    #[test]
    fn cold_then_warm_elastic_static_dispatch_replays_an_error_diagnostic() {
        assert_cold_then_warm_dispatch_replays_the_error(
            "solver::elastic_static",
            erroring_elastic_trampoline as crate::ComputeFn,
            &DISPATCH_COUNT_ERR_ELASTIC,
            ContentHash(0x7245_0022_7245_0022_7245_0022_7245_0022_u128),
        );
    }

    #[test]
    fn cold_then_warm_buckling_dispatch_replays_an_error_diagnostic() {
        // `solver::buckling` has no engine-level diagnostics-replay coverage at
        // any severity today; the elastic arm is not evidence for it, because
        // each target has its own `persistent_lookup`/`persistent_write` arm.
        assert_cold_then_warm_dispatch_replays_the_error(
            "solver::buckling",
            erroring_buckling_trampoline as crate::ComputeFn,
            &DISPATCH_COUNT_ERR_BUCKLING,
            ContentHash(0x7245_0023_7245_0023_7245_0023_7245_0023_u128),
        );
    }

    /// The live declared-but-not-honored trampoline warning must survive a warm
    /// buckling cache hit.
    ///
    /// `buckling_unsupported_option_diagnostics` (compute_targets/buckling.rs)
    /// emits `DiagnosticCode::BucklingOptionUnsupported` as a WARNING on a solve
    /// that reaches `ComputeOutcome::Completed` — so the entry IS persisted, and
    /// before this task's fix the warning went silent on every run but the first.
    /// It is the workspace's only present-day code for a parameter the solver
    /// accepted and then ignored, which makes it the live analogue of the
    /// trampoline param-drop class this task exists to keep audible.
    ///
    /// Task #7079 will add `E_PARAM_NOT_HONORED` / `W_PARAM_NOT_APPLICABLE` to
    /// that same class, naming this code as its doc-block precedent. Its
    /// INV-PD-1 is checked only on a COLD run; a warm run gets whatever this
    /// replay path hands it and nothing else. The codes #7079 mints therefore
    /// ride exactly this path, with no further change needed here.
    ///
    /// The diagnostic is built locally rather than by calling the emitter:
    /// widening `buckling_unsupported_option_diagnostics`'s visibility to reach
    /// it from a test would open a seam into the module's internals that nothing
    /// in production needs.
    #[test]
    fn warm_buckling_hit_replays_the_declared_but_unhonored_option_warning() {
        // Shaped after `unsupported_diag`'s template for the `mode: "dense"`
        // case. This is a copy, not a reference to it — the property pinned here
        // is that whatever the emitter says survives VERBATIM, not that this
        // string equals today's template. The message is the actionable payload:
        // it names the ignored param, the value that was dropped, and the
        // default the solve silently fell back to.
        let written = reify_core::Diagnostic::warning(
            "BucklingOptions.mode = \"dense\" is declared but not yet honored by \
             the solver::buckling trampoline (the buckling kernel has no \
             mode-select input yet); solve falls back to the default \
             \"shift_invert\"",
        )
        .with_code(reify_core::DiagnosticCode::BucklingOptionUnsupported);

        let tmp = tempfile::TempDir::new().unwrap();
        let cache_key = ContentHash(0x7245_0021_7245_0021_7245_0021_7245_0021_u128);
        let value = buckling_cache_value();

        super::persistent_write(
            tmp.path(),
            "solver::buckling",
            cache_key,
            &value,
            std::slice::from_ref(&written),
        );

        let (_, got_diags) = super::persistent_lookup(tmp.path(), "solver::buckling", cache_key)
            .expect("the entry just written must be a hit");

        assert_eq!(got_diags.len(), 1, "got {got_diags:?}");
        let got = &got_diags[0];
        assert_eq!(
            got.code,
            Some(reify_core::DiagnosticCode::BucklingOptionUnsupported),
            "the warm serve must keep the code — a consumer auditing for a \
             declared-but-unhonored param keys off it, not off the prose",
        );
        assert_eq!(got.severity, reify_core::Severity::Warning);
        assert_eq!(
            got.message, written.message,
            "the full message must survive: it names the ignored param, its value \
             and the fallback default, which is everything the user needs in order \
             to act and everything they lose when it goes silent warm",
        );
    }

    #[test]
    fn persistent_write_then_lookup_replays_an_empty_diagnostics_list() {
        // The common case: a solve that said nothing must still be a HIT, with
        // an empty list rather than a decode failure.
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_key = ContentHash(0x7245_0003_7245_0003_7245_0003_7245_0003_u128);
        let value = crate::compute_targets::elastic_static::value_from_elastic_result(
            &minimal_elastic_result(7.0),
        );

        super::persistent_write(tmp.path(), "solver::elastic_static", cache_key, &value, &[]);

        let (got_value, got_diags) =
            super::persistent_lookup(tmp.path(), "solver::elastic_static", cache_key)
                .expect("the entry just written must be a hit");

        assert_eq!(got_value.content_hash(), value.content_hash());
        assert!(
            got_diags.is_empty(),
            "a silent solve must replay no diagnostics, got {got_diags:?}"
        );
    }

    #[test]
    fn persistent_round_trip_replays_fea_under_constrained_unanchored() {
        // The one live producer of a SPAN-CARRYING persisted diagnostic, and
        // therefore the one that pins how a warm serve handles the span.
        // Built from the live call site rather than a synthetic diagnostic:
        // compute_targets/elastic_static.rs's present-but-unhonored-support arm
        // computes `first_instance_source_span(&value_inputs[5])` and pushes
        // exactly this diagnostic, which `fea_diagnostic_to_core` decorates with
        // a `DiagnosticLabel` whenever the span is `Some`.
        //
        // Why this path is persisted at all: `UnderConstrained` is NOT an error
        // (`FeaFailure::is_error`, crates/reify-solver-elastic/src/diagnostics.rs
        // lists only SingularStiffness / LoadOnInterior / SelectorNoMatch), so
        // the solve completes and the entry IS written.
        //
        // The warm serve replays it WITHOUT the label: the persistent key is
        // span-invariant by design, so the same entry serves two source layouts
        // with identical FEA inputs, and a replayed span would anchor into
        // unrelated text (see `persistent_cache::PersistedDiagnostic`'s "Why
        // labels are not carried"). Nothing an author reads is lost — the label
        // message here is `failure.message()`, verbatim the diagnostic's own
        // `message`, which IS replayed.
        let diag = crate::compute_targets::fea_diagnostics::fea_diagnostic_to_core(
            &reify_solver_elastic::FeaFailure::UnderConstrained { support_count: 2 },
            Some(reify_core::SourceSpan::new(41, 57)),
        );
        // Guard the premise itself: fea_diagnostics.rs attaches a label only
        // when the span is `Some`, so this test is vacuous if that ever changes.
        assert_eq!(
            diag.labels.len(),
            1,
            "premise: the live call site produces a labelled diagnostic, got {diag:?}"
        );

        let tmp = tempfile::TempDir::new().unwrap();
        let cache_key = ContentHash(0x7245_0004_7245_0004_7245_0004_7245_0004_u128);
        let value = crate::compute_targets::elastic_static::value_from_elastic_result(
            &minimal_elastic_result(13.0),
        );

        super::persistent_write(
            tmp.path(),
            "solver::elastic_static",
            cache_key,
            &value,
            std::slice::from_ref(&diag),
        );

        let (_, got_diags) =
            super::persistent_lookup(tmp.path(), "solver::elastic_static", cache_key)
                .expect("the entry just written must be a hit");

        assert_eq!(got_diags.len(), 1, "got {got_diags:?}");
        let got = &got_diags[0];
        assert_eq!(
            got.code,
            Some(reify_core::DiagnosticCode::FeaUnderConstrained),
            "code must survive the warm serve"
        );
        assert_eq!(got.severity, reify_core::Severity::Warning);
        assert_eq!(
            got.message, diag.message,
            "the message — which is also the label's text — must survive"
        );
        assert!(
            got.labels.is_empty(),
            "the warm serve must NOT replay a source-anchored label: the key \
             that served this entry does not identify the source it was \
             computed from, got {got:?}"
        );
    }
}
