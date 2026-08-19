/// Process-global mutex for serialising tests that mutate
/// `std::env::set_current_dir`.
///
/// Cargo runs all lib tests of a crate in a SINGLE process with multiple
/// threads, so any test file that mutates CWD must lock the SAME mutex as
/// every other CWD-mutating file.  Putting the lock here in a shared module
/// guarantees that — every call site goes through `crate::tests::test_helpers::cwd_lock`,
/// which returns the same `&'static Mutex<()>` instance every time.
use std::sync::{Mutex, OnceLock};

static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Returns the process-global mutex used to serialise CWD-mutating tests.
///
/// Every call returns the SAME `&'static Mutex<()>` — enforced by
/// `OnceLock::get_or_init`.  Tests across different files in this crate all
/// share a single serialisation point when they call `cwd_lock().lock()`.
pub(crate) fn cwd_lock() -> &'static Mutex<()> {
    CWD_LOCK.get_or_init(|| Mutex::new(()))
}

// ── Task 5194: Rigid auto-derived mass-property cells surface in the GUI panel ──
//
// A `structure def X : Rigid` auto-derives four geometry-query lets via the
// stdlib `Rigid : Physical` traits (crates/reify-compiler/stdlib/structural_physical.ri):
//   mass              = volume(geometry) * material.density
//   centroid          = centroid(geometry)
//   moment_of_inertia = moment_of_inertia(geometry, body_density)
//   moi_principal     = eigenvalues(moment_of_inertia)   (+ PD constraint [0] > 0)
//
// These are populated ONLY by the kernel-bearing build's `run_post_processes`
// (mass/centroid via geometry queries; moment_of_inertia via the topology-selector
// pass; moi_principal via the derived-let pass). The GUI property panel, however,
// sources cell values from the kernel-LESS `check.values` (eval / warm edit_param),
// so all four read `undetermined` and the `moi_principal[0] > 0` PD constraint reads
// `Indeterminate`. The fix overlays the kernel-derived `tessellate_snapshot`
// `result.values` / `result.constraint_results` onto the panel in `build_gui_state`.
//
// Promoted here from `engine_tests.rs` by task #5338 (prerequisite pre-1) so the
// command-level entry-point tests in `commands_tests.rs` can drive the SAME seeded
// kernel instead of duplicating the 1..=4 handle seeding. Pure move, no behaviour
// change.

/// Shared harness for the task-5194 GUI tests: an `EngineSession` whose
/// `MockGeometryKernel` is pre-seeded with volume / centroid / inertia-tensor
/// replies for `GeometryHandleId` 1..=4.
///
/// The id RANGE (not just id 1) covers the post-edit rebuild handle drift: the
/// mock's `next_id` is monotonic and NOT reset between builds, so the initial
/// load realizes the box to id 1 but a subsequent `set_parameter` rebuild
/// re-executes the box and allocates a fresh id (2, 3, …). Seeding a range keeps
/// the geometry queries answered across every rebuild in a test.
///
/// Widened from `1..=4` to `1..=16` by task #5338: its entry-point matrix drives
/// up to four full-scope re-executions per row (open + reload/edit + a
/// `build_gui_state_full_scene` debug read + the first selective rebuild), which
/// sat exactly on the old ceiling. Purely additive — seeding more ids only adds
/// map entries, and an unanswered id would surface as an `Undef` mass-prop cell
/// (i.e. loudly), never as a silent pass.
///
/// The inertia tensor is diagonal `diag(1, 2, 3)` (positive) so `moi_principal`
/// eigenvalues are `[1, 2, 3]` and the PD constraint `moi_principal[0] > 0` is
/// satisfiable. Seeded magnitudes are arbitrary stand-ins — the tests assert on
/// determinacy / constraint status, not analytic values (the OCCT-gated
/// crates/reify-eval/tests/rigid_moment_of_inertia_autoderive_smoke.rs owns the
/// exact-magnitude checks).
///
/// The seeded density `7850.0` must stay bits-exact: `with_inertia_tensor_result`
/// keys on it, and the fixture declares `density: 7850kg/m^3`.
pub(crate) fn rigid_mass_props_session() -> crate::engine::EngineSession {
    rigid_mass_props_session_seeded(1..=16)
}

/// [`rigid_mass_props_session`] with an explicit `GeometryHandleId` seed range.
///
/// Seeding a NARROW range is how a test drives the negative case: the mock's
/// `next_id` is monotonic and never reset, so a rebuild that re-executes the box
/// allocates an id past the range, its volume / centroid / inertia queries go
/// unanswered, and the mass-prop cells resolve to `Undef` — a stand-in for the
/// production "geometry went degenerate / the kernel query failed" case, which
/// the delta encodes identically (an explicit `Undef` entry). Used by
/// `degenerate_geometry_after_rebuild_clears_the_retained_mass_props`
/// (commands_tests.rs) to prove the retention cache does NOT replay a stale value
/// as `determined` once the realization has actually re-run.
pub(crate) fn rigid_mass_props_session_seeded(
    ids: std::ops::RangeInclusive<u64>,
) -> crate::engine::EngineSession {
    use reify_constraints::SimpleConstraintChecker;
    use reify_ir::{GeometryHandleId, Value};
    use reify_test_support::MockGeometryKernel;

    let checker = SimpleConstraintChecker;
    let mut kernel = MockGeometryKernel::new();
    for id in ids {
        let h = GeometryHandleId(id);
        kernel = kernel
            .with_volume_result(h, Value::Real(0.003))
            .with_centroid_result(
                h,
                Value::String("{\"x\":0.05,\"y\":0.05,\"z\":0.15}".to_string()),
            )
            .with_inertia_tensor_result(
                h,
                7850.0,
                Value::List(vec![
                    Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
                    Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
                    Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
                ]),
            );
    }
    crate::engine::EngineSession::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Absolute path to the committed `examples/rigid_mass_props_smoke.ri` fixture,
/// resolved from this crate's manifest dir (two levels up → workspace root),
/// mirroring `load_file_returns_gui_state`.
pub(crate) fn rigid_mass_props_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/rigid_mass_props_smoke.ri")
}

/// Locate the `moi_principal[0] > 0` positive-definiteness constraint injected by
/// the stdlib `Rigid` trait, matching on either its formatted expression or its
/// collected `parameter_ids` (both carry the `moi_principal` cell name).
pub(crate) fn find_moi_principal_constraint(
    state: &crate::types::GuiState,
) -> &crate::types::ConstraintData {
    state
        .constraints
        .iter()
        .find(|c| {
            c.expression.contains("moi_principal")
                || c.parameter_ids.iter().any(|p| p.contains("moi_principal"))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected the Rigid `moi_principal[0] > 0` PD constraint; have: {:?}",
                state
                    .constraints
                    .iter()
                    .map(|c| (c.expression.as_str(), &c.parameter_ids, c.status.as_str()))
                    .collect::<Vec<_>>()
            )
        })
}

/// The four auto-derived `Rigid` mass-property cells, in the order the stdlib
/// `Rigid : Physical` traits declare them.
pub(crate) const RIGID_MASS_PROP_CELLS: [&str; 4] =
    ["mass", "centroid", "moment_of_inertia", "moi_principal"];

/// Assert the full task-5194 post-condition on `state`: each of
/// [`RIGID_MASS_PROP_CELLS`] is present, `determined`, carries no undef-cause
/// `reason`, and the `moi_principal[0] > 0` PD constraint reads `Satisfied`.
///
/// `ctx` names the matrix row / rebuild iteration under test and is interpolated
/// into EVERY failure message, so a red row in task #5338's entry-point matrix
/// (argv / File-Open / watcher / warm edit_param, each followed by N successive
/// selective-demand re-renders) identifies itself without a bisect.
///
/// Determinacy / constraint status only — never analytic magnitudes. The seeded
/// `MockGeometryKernel` replies are arbitrary stand-ins; exact-value checks live
/// in the OCCT-gated `crates/reify-eval/tests/rigid_moment_of_inertia_autoderive_smoke.rs`.
pub(crate) fn assert_rigid_mass_props_determined(state: &crate::types::GuiState, ctx: &str) {
    for name in RIGID_MASS_PROP_CELLS {
        let cell = state
            .values
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "[{ctx}] expected a `{name}` value cell; have: {:?}",
                    state
                        .values
                        .iter()
                        .map(|v| v.name.as_str())
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            cell.determinacy, "determined",
            "[{ctx}] `{name}` must be `determined` (auto-derived from geometry + \
             material.density); got determinacy={:?}, reason={:?}, freshness={:?}",
            cell.determinacy, cell.reason, cell.freshness
        );
        assert!(
            cell.reason.is_none(),
            "[{ctx}] `{name}` must carry no undef-cause `reason` once surfaced; got {:?}",
            cell.reason
        );
    }

    let pd = find_moi_principal_constraint(state);
    assert_eq!(
        pd.status, "Satisfied",
        "[{ctx}] the `moi_principal[0] > 0` PD constraint must be Satisfied once \
         moi_principal resolves; got status={:?}",
        pd.status
    );
}

/// The entity-scoped positive twin of [`assert_rigid_mass_props_not_final`]:
/// every one of [`RIGID_MASS_PROP_CELLS`] on `entity` is present, `determined`,
/// `freshness = "final"` and carries no undef-cause `reason`.
///
/// [`assert_rigid_mass_props_determined`] matches on cell NAME alone, which is
/// unambiguous for the single-body `rigid_mass_props_smoke.ri` fixture but not
/// for the multi-body sources the #5338 prune-safety tests use — hence this
/// entity-qualified form.
pub(crate) fn assert_rigid_mass_props_final(
    state: &crate::types::GuiState,
    entity: &str,
    ctx: &str,
) {
    for name in RIGID_MASS_PROP_CELLS {
        let cell = state
            .values
            .iter()
            .find(|v| v.entity_path == entity && v.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "[{ctx}] expected a `{entity}.{name}` value cell; have: {:?}",
                    state
                        .values
                        .iter()
                        .map(|v| (v.entity_path.as_str(), v.name.as_str()))
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            cell.determinacy, "determined",
            "[{ctx}] `{entity}.{name}` must be `determined`; got determinacy={:?}, \
             reason={:?}, freshness={:?}",
            cell.determinacy, cell.reason, cell.freshness
        );
        assert_eq!(
            cell.freshness, "final",
            "[{ctx}] `{entity}.{name}` must read `final` once surfaced; got {:?}",
            cell.freshness
        );
        assert!(
            cell.reason.is_none(),
            "[{ctx}] `{entity}.{name}` must carry no undef-cause `reason`; got {:?}",
            cell.reason
        );
    }
}

/// Assert that `entity`'s geometry-derived mass-prop cells are NOT being served
/// as a Final answer — the arch §8 post-condition ("a pruned realization's cached
/// result is never served as Final") and the task-#5338 staleness post-condition
/// ("a value whose realization re-ran and resolved to nothing is not replayed").
///
/// A cell passes when it is not `determined`, or is determined but still flagged
/// non-`final` — i.e. anything except the `determined` + `freshness = "final"`
/// triple that means "this is a fresh, authoritative value". Deliberately the
/// exact NEGATION of [`assert_rigid_mass_props_final`]'s per-cell half, so the two
/// cannot both hold and a regression that made retention unconditional fails here.
///
/// The cell must be PRESENT. Treating absence as a pass would make every
/// prune-safety caller green for the wrong reason: the fixtures they use always
/// emit all four cells from the kernel-less `build_values`, so a regression that
/// dropped the cells entirely — or a fixture rename that stopped `entity` from
/// matching — would satisfy an absence-tolerant assertion vacuously. This mirrors
/// the positive helpers, which already panic on absence.
pub(crate) fn assert_rigid_mass_props_not_final(
    state: &crate::types::GuiState,
    entity: &str,
    ctx: &str,
) {
    for name in RIGID_MASS_PROP_CELLS {
        let cell = state
            .values
            .iter()
            .find(|v| v.entity_path == entity && v.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "[{ctx}] expected a `{entity}.{name}` value cell to be present (an \
                     absent cell would make this prune-safety assertion vacuous); have: {:?}",
                    state
                        .values
                        .iter()
                        .map(|v| (v.entity_path.as_str(), v.name.as_str()))
                        .collect::<Vec<_>>()
                )
            });
        assert!(
            !(cell.determinacy == "determined" && cell.freshness == "final"),
            "[{ctx}] `{entity}.{name}` must NOT be served as a fresh Final value; got \
             determinacy={:?}, freshness={:?}, reason={:?}, value={:?}",
            cell.determinacy,
            cell.freshness,
            cell.reason,
            cell.value
        );
    }
}

/// The exact `Entity#realization[N]` key list the frontend feeds to
/// `EngineSession::sync_demand` — derived from the loaded state's rendered
/// meshes, i.e. `state.meshes[].entity_path`.
///
/// Deriving the keys from the state (rather than hardcoding
/// `"RigidMassSmoke#realization[0]"`) is what makes the tests' selective-demand
/// posture faithful to production: these are the same strings
/// `parse_realization_key` (engine.rs) turns back into `NodeId::Realization`
/// roots for `Engine::set_demand_selective`. A hardcoded key that stopped
/// resolving would silently yield an EMPTY demand cone and a vacuously-passing
/// test.
pub(crate) fn visible_realization_keys(state: &crate::types::GuiState) -> Vec<String> {
    state
        .meshes
        .iter()
        .map(|m| m.entity_path.clone())
        .collect()
}
