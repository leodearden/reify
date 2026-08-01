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
/// re-executes the box and allocates a fresh id (2, 3, …). Seeding 1..=4 keeps
/// the geometry queries answered across both the load and warm-edit rebuilds.
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
    use reify_constraints::SimpleConstraintChecker;
    use reify_ir::{GeometryHandleId, Value};
    use reify_test_support::MockGeometryKernel;

    let checker = SimpleConstraintChecker;
    let mut kernel = MockGeometryKernel::new();
    for id in 1..=4u64 {
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
