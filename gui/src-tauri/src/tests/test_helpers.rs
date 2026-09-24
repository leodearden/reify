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
    use reify_constraints::SimpleConstraintChecker;

    crate::engine::EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(seeded_rigid_mass_props_kernel(1..=16))),
    )
}

/// Seed a fresh `MockGeometryKernel` with succeeding volume / centroid /
/// inertia-tensor replies for every `GeometryHandleId` in `ids`. Shared by
/// [`rigid_mass_props_session`] (which seeds `1..=16`) and
/// [`rigid_mass_props_session_seeded_then_failing`] (which seeds a narrower
/// range and fails everything past it) so both draw the seeded reply shapes
/// from one source of truth (task #6471).
fn seeded_rigid_mass_props_kernel(
    ids: std::ops::RangeInclusive<u64>,
) -> reify_test_support::MockGeometryKernel {
    use reify_ir::{GeometryHandleId, Value};
    use reify_test_support::MockGeometryKernel;

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
    kernel
}

/// [`rigid_mass_props_session`] variant that EXPLICITLY fails every geometry
/// query dispatched after `good_ids`, via
/// `MockGeometryKernel::fail_after_n_dispatches` (crates/reify-test-support/src/mocks.rs),
/// instead of leaving handles past `good_ids` unseeded and relying on the
/// generic "no mock result" fallback to fail them incidentally.
///
/// This is the replacement for the seed-range-STARVATION mechanism that
/// `degenerate_geometry_after_rebuild_clears_the_retained_mass_props`
/// (commands_tests.rs) used to induce "the geometry query failed / the body
/// went degenerate": that test picked a narrow `ids` range and inferred the
/// failure from a later dispatch landing outside it, coupling the test to
/// exactly how many kernel dispatches production code happens to perform.
/// Here the failure is stated directly — "queries for any handle past
/// `good_ids.end()` fail" — so no downstream precondition assertion is
/// needed to confirm it actually took effect. (The knob gates on handle ID,
/// which for the box's `execute`-allocated handles IS the dispatch ordinal;
/// see `fail_after_n_dispatches`'s doc for where the two diverge.)
///
/// Reviewer suggestion 4 on task #5338 (escalation `agent-followup-5338`,
/// `suggestion_hash 5338-mock-kernel-query-failure-seeding`); implemented as
/// task #6471 because the knob lives in `crates/reify-test-support`, outside
/// #5338's locked scope. Filed under ticket `tkt_0RSRP1HKTPG0E9XB0YWQVC0RT0`.
///
/// Returns the session alongside the mock's shared operation log
/// (`MockGeometryKernel::operations_ref`), fetched BEFORE the kernel is boxed
/// into the session — the log is push-only from `GeometryKernel::execute`, so
/// a caller that snapshots its length before an edit and compares after can
/// assert the edit actually re-DISPATCHED, independent of the pass/fail
/// verdict the `fail_after_n_dispatches` knob controls.
pub(crate) fn rigid_mass_props_session_seeded_then_failing(
    good_ids: std::ops::RangeInclusive<u64>,
) -> (
    crate::engine::EngineSession,
    std::sync::Arc<std::sync::Mutex<Vec<reify_test_support::GeometryOpRecord>>>,
) {
    use reify_constraints::SimpleConstraintChecker;

    let checker = SimpleConstraintChecker;
    let ceiling = *good_ids.end();
    let kernel = seeded_rigid_mass_props_kernel(good_ids).fail_after_n_dispatches(ceiling);
    let dispatch_log = kernel.operations_ref();
    let session = crate::engine::EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    (session, dispatch_log)
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
        pd.status, "satisfied",
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

// ── Task 7442: observing the evaluation queue ────────────────────────────────

/// A `GuiState` with nothing in it.
pub(crate) fn empty_gui_state() -> crate::types::GuiState {
    crate::types::GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
        display_panes: vec![],
        display_appearance: vec![],
        fea_diagnostics: vec![],
        fea_convergence: None,
    }
}

/// A `GuiState` holding only the given `(cell_id, value)` parameter values —
/// enough to tell snapshots apart through their deltas.
pub(crate) fn gui_state_with_values(values: &[(&str, &str)]) -> crate::types::GuiState {
    let value = |(cell_id, value): &(&str, &str)| crate::types::ValueData {
        cell_id: cell_id.to_string(),
        name: cell_id.rsplit('.').next().unwrap_or(cell_id).to_string(),
        value: value.to_string(),
        unit: "mm".to_string(),
        determinacy: "determined".to_string(),
        entity_path: cell_id.split('.').next().unwrap_or("").to_string(),
        kind: "Param".to_string(),
        freshness: "final".to_string(),
        reason: None,
        last_substantive_value: None,
        dimension: String::new(),
        si_value: None,
    };
    crate::types::GuiState {
        values: values.iter().map(value).collect(),
        ..empty_gui_state()
    }
}

/// One call an [`crate::eval_queue::EvalObserver`] received.
#[derive(Debug, Clone)]
pub(crate) enum Observed {
    Activity(crate::eval_queue::EvalActivity),
    Delta(Box<crate::diff::StateDelta>),
}

/// An [`Observed`] call and the name of the thread that made it.
#[derive(Debug, Clone)]
pub(crate) struct Observation {
    pub(crate) observed: Observed,
    pub(crate) thread: Option<String>,
}

/// An [`crate::eval_queue::EvalObserver`] that records every call, in order.
#[derive(Default)]
pub(crate) struct RecordingObserver {
    observations: Mutex<Vec<Observation>>,
}

impl RecordingObserver {
    pub(crate) fn observations(&self) -> Vec<Observation> {
        self.observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn activities(&self) -> Vec<crate::eval_queue::EvalActivity> {
        self.observations()
            .into_iter()
            .filter_map(|o| match o.observed {
                Observed::Activity(activity) => Some(activity),
                Observed::Delta(_) => None,
            })
            .collect()
    }

    pub(crate) fn deltas(&self) -> Vec<crate::diff::StateDelta> {
        self.observations()
            .into_iter()
            .filter_map(|o| match o.observed {
                Observed::Delta(delta) => Some(*delta),
                Observed::Activity(_) => None,
            })
            .collect()
    }

    /// The newest published value of `cell_id`, if any delta carried one.
    pub(crate) fn published_value(&self, cell_id: &str) -> Option<crate::types::ValueData> {
        self.deltas().iter().rev().find_map(|delta| {
            delta
                .changed_values
                .iter()
                .find(|value| value.cell_id == cell_id)
                .cloned()
        })
    }

    fn record(&self, observed: Observed) {
        let thread = std::thread::current().name().map(str::to_owned);
        self.observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Observation { observed, thread });
    }
}

impl crate::eval_queue::EvalObserver for RecordingObserver {
    fn activity(&self, activity: crate::eval_queue::EvalActivity) {
        self.record(Observed::Activity(activity));
    }

    fn delta(&self, delta: &crate::diff::StateDelta) {
        self.record(Observed::Delta(Box::new(delta.clone())));
    }
}

/// A queue whose drainers run on the test thread, when the test says.
pub(crate) struct ManualQueue {
    pub(crate) queue: std::sync::Arc<crate::eval_queue::EvalQueue>,
    pub(crate) executor: std::sync::Arc<ManualExecutor>,
    pub(crate) observer: std::sync::Arc<RecordingObserver>,
}

impl ManualQueue {
    pub(crate) fn new() -> Self {
        let executor = ManualExecutor::new();
        let observer = std::sync::Arc::new(RecordingObserver::default());
        let queue = crate::eval_queue::EvalQueue::with_executor(
            executor.executor(),
            std::sync::Arc::new(Mutex::new(None)),
            observer.clone(),
        );
        Self {
            queue,
            executor,
            observer,
        }
    }
}

/// Poll `future` once without waiting: `Some` if it has already resolved.
pub(crate) fn poll_now<F: Future + Unpin>(future: &mut F) -> Option<F::Output> {
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::Pin::new(future).poll(&mut context) {
        std::task::Poll::Ready(output) => Some(output),
        std::task::Poll::Pending => None,
    }
}

/// The reply of a ticket that must already have settled.
pub(crate) fn settled<T>(mut ticket: crate::eval_queue::EvalTicket<T>) -> Result<T, String> {
    poll_now(&mut ticket).expect("the ticket must have settled")
}

/// An [`crate::eval_queue::Executor`] that runs nothing until told: posted jobs
/// wait until [`ManualExecutor::run_pending`] runs them on the calling thread.
#[derive(Default)]
pub(crate) struct ManualExecutor {
    jobs: Mutex<std::collections::VecDeque<Box<dyn FnOnce() + Send>>>,
    refusing: std::sync::atomic::AtomicBool,
}

impl ManualExecutor {
    pub(crate) fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::default()
    }

    pub(crate) fn executor(self: &std::sync::Arc<Self>) -> crate::eval_queue::Executor {
        let this = std::sync::Arc::clone(self);
        std::sync::Arc::new(move |job| this.post(job))
    }

    /// Make every later post fail (`true`) or succeed again (`false`).
    pub(crate) fn refuse(&self, refusing: bool) {
        self.refusing
            .store(refusing, std::sync::atomic::Ordering::SeqCst);
    }

    /// How many posted jobs have not run yet.
    pub(crate) fn pending(&self) -> usize {
        self.jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Run posted jobs in order on this thread, including jobs posted while
    /// they run, until none are left.
    pub(crate) fn run_pending(&self) {
        loop {
            let next = self
                .jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            let Some(job) = next else { return };
            job();
        }
    }

    fn post(&self, job: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
        if self.refusing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::other("the test executor refused the job"));
        }
        self.jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(job);
        Ok(())
    }
}

// ── Large-stack probes, shared by the lane and queue tests ───────────────────

/// Bound on every wait for work handed to another thread. It is NOT a timing
/// assertion: it only turns a wedged lane into a failing test instead of a hung
/// test binary.
pub(crate) const ANTI_WEDGE: std::time::Duration = std::time::Duration::from_secs(60);

/// A recursive frame that pins ~8 KiB of live stack per call and USES the
/// recursive result (non-tail), defeating tail-call optimization and dead-frame
/// elision. `#[inline(never)]` keeps each level a real call frame; the
/// `black_box`ed 8 KiB buffer forces the optimizer to materialize the frame.
///
/// `deep_recurse(n) == n + 1` (base case returns 1, each of the `n` recursive
/// frames adds `buf[8191] == 1`), so callers get a deterministic sentinel proving
/// the recursion ran to completion rather than being elided.
#[inline(never)]
pub(crate) fn deep_recurse(depth: u32) -> u64 {
    // 8 KiB per frame. Touch both ends so the whole buffer is committed and the
    // frame cannot be elided.
    let mut buf = [0u8; 8192];
    buf[0] = 1;
    buf[8191] = 1;
    let buf = std::hint::black_box(buf);
    if depth == 0 {
        return u64::from(buf[0]); // sentinel base == 1
    }
    // Use the recursive result (non-tail) so the frame stays live across the call.
    let below = deep_recurse(depth - 1);
    std::hint::black_box(below + u64::from(buf[8191]))
}

/// Depth for the deep-recursion survival tests: ~8 KiB/frame x 2048 ≈ 16 MiB,
/// i.e. 8x the 2 MiB default stack (a no-`stack_size` impl overflows) and 16x
/// under the 256 MiB `COMPILE_STACK_SIZE` constant (GREEN is reliable).
pub(crate) const DEEP_RECURSION_DEPTH: u32 = 2048;

/// Recurse ~16 MiB ONLY if we genuinely landed on the expected large-stack
/// thread; otherwise report where we actually are, without recursing.
///
/// "Invoked through a large-stack helper" does NOT by itself imply "runs on a
/// large stack": every helper documents an INLINE-degradation arm that hands the
/// closure back to the CALLER's default-size stack — `run_on_large_stack` when
/// the OS refuses the 256 MiB mapping, `run_on_worker` additionally when the
/// queue is dead. Recursing there overflows and SIGABRTs the whole test binary,
/// taking every other test's result with it (observed while driving task 5772's
/// step-3 RED, where a panicking job had killed the worker).
///
/// Checking first is what makes `large_stack_tests`' "no violent RED" claim true
/// by CONSTRUCTION rather than by assumption: a degraded helper now yields a
/// clean assertion failure naming the thread it ran on.
pub(crate) fn deep_recurse_if_on_thread(
    expected_name: &'static str,
    depth: u32,
) -> Result<u64, String> {
    let actual = std::thread::current().name().map(str::to_owned);
    if actual.as_deref() != Some(expected_name) {
        return Err(format!(
            "refusing to recurse ~16 MiB on thread {actual:?}: expected the \
             large-stack thread {expected_name:?}. The helper degraded to an \
             inline call, so recursing here would overflow a default-size stack \
             and abort the entire test binary."
        ));
    }
    Ok(deep_recurse(depth))
}
