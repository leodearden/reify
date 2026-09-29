//! Tests for [`crate::eval_queue`].

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use crate::diff::{StateDelta, advance_baseline, compute_delta, diff_gui_state};
use crate::eval_queue::{
    EditIdentity, EditLedger, EditOrder, EvalActivity, EvalObserver, EvalOutcome, EvalQueue,
    EvalRequest, EvalTicket, SnapshotPublisher,
};
use crate::tests::test_helpers::{
    ANTI_WEDGE, DEEP_RECURSION_DEPTH, ManualExecutor, ManualQueue, Observed, RecordingObserver,
    deep_recurse_if_on_thread, gui_state_with_values, poll_now, settled,
};
use crate::types::GuiState;

// ── Edit identity: which queued edit a newer one makes redundant ─────────────

const EPOCH: u64 = 7;

fn at(seq: u64) -> EditOrder {
    EditOrder { epoch: EPOCH, seq }
}

fn preview(cell: &str, seq: u64) -> EditIdentity {
    EditIdentity::preview(cell, at(seq))
}

fn commit(cell: &str, seq: u64) -> EditIdentity {
    EditIdentity::commit(cell, at(seq))
}

fn editor(path: &str, seq: u64) -> EditIdentity {
    EditIdentity::editor_source(path, at(seq))
}

fn disk(path: &str) -> EditIdentity {
    EditIdentity::disk_source(path)
}

/// The wire shape `bridge.ts` sends as a command argument.
#[test]
fn an_edit_order_deserializes_from_the_frontend_stamp() {
    let order: EditOrder = serde_json::from_value(serde_json::json!({
        "epoch": 9_007_199_254_740_991_u64,
        "seq": 3,
    }))
    .expect("an {epoch, seq} object must deserialize");
    assert_eq!(
        order,
        EditOrder {
            epoch: 9_007_199_254_740_991,
            seq: 3
        }
    );
}

#[test]
fn a_newer_preview_supersedes_an_older_one_of_its_cell_but_not_the_reverse() {
    assert!(preview("A", 2).supersedes(&preview("A", 1)));
    assert!(!preview("A", 1).supersedes(&preview("A", 2)));
}

#[test]
fn edits_of_one_cell_never_supersede_edits_of_another() {
    for newer in [preview("A", 9), commit("A", 9)] {
        for older in [preview("B", 1), commit("B", 1)] {
            assert!(
                !newer.supersedes(&older),
                "{newer:?} must not supersede {older:?}"
            );
        }
    }
}

#[test]
fn a_commit_supersedes_older_previews_and_older_commits_of_its_cell() {
    assert!(commit("A", 5).supersedes(&preview("A", 4)));
    assert!(commit("A", 5).supersedes(&commit("A", 4)));
}

/// INV-GUI-3: a transient frame must never drop a durable write.
#[test]
fn a_preview_never_supersedes_a_commit_however_new() {
    assert!(!preview("A", 100).supersedes(&commit("A", 1)));
}

#[test]
fn editor_sources_coalesce_per_path_and_never_with_disk_sources() {
    assert!(editor("p.ri", 2).supersedes(&editor("p.ri", 1)));
    assert!(!editor("p.ri", 2).supersedes(&editor("q.ri", 1)));
    assert!(!editor("p.ri", 2).supersedes(&disk("p.ri")));
    assert!(!disk("p.ri").supersedes(&editor("p.ri", 1)));
}

/// A disk reload carries no order: the reload reads the file when it runs, so
/// whichever arrives later is the one worth running.
#[test]
fn a_disk_reload_supersedes_a_queued_reload_of_the_same_file() {
    assert!(disk("p.ri").supersedes(&disk("p.ri")));
    assert!(!disk("p.ri").supersedes(&disk("q.ri")));
}

/// A page reload restarts `seq` under a new epoch, and epochs are compared only
/// for equality, so across epochs the arriving edit wins in either direction.
#[test]
fn across_epochs_the_arriving_edit_wins() {
    let reloaded = |epoch, seq| EditIdentity::preview("A", EditOrder { epoch, seq });
    assert!(reloaded(EPOCH + 1, 1).supersedes(&preview("A", 50)));
    assert!(reloaded(EPOCH - 1, 1).supersedes(&preview("A", 50)));
}

#[test]
fn the_ledger_refuses_a_preview_older_than_one_already_admitted() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&preview("A", 5)));
    assert!(
        !ledger.admit(&preview("A", 4)),
        "a late preview must be refused"
    );
    assert!(ledger.admit(&preview("A", 6)));
}

#[test]
fn an_admitted_commit_refuses_older_previews_and_older_commits() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&commit("A", 5)));
    assert!(!ledger.admit(&preview("A", 4)));
    assert!(!ledger.admit(&commit("A", 4)));
}

/// INV-GUI-3 again: a late durable write is never dropped for a newer preview.
#[test]
fn a_commit_is_admitted_even_behind_a_newer_preview() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&preview("A", 5)));
    assert!(ledger.admit(&commit("A", 4)));
}

#[test]
fn the_ledger_admits_other_epochs_other_cells_and_every_disk_reload() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&commit("A", 5)));
    let other_epoch = EditOrder {
        epoch: EPOCH + 1,
        seq: 1,
    };
    assert!(ledger.admit(&EditIdentity::preview("A", other_epoch)));
    assert!(ledger.admit(&EditIdentity::commit("A", other_epoch)));
    assert!(ledger.admit(&preview("B", 1)));
    assert!(ledger.admit(&disk("p.ri")));
    assert!(ledger.admit(&disk("p.ri")));
}

// ── SnapshotPublisher: evaluation results become frontend deltas ─────────────

fn json(delta: &StateDelta) -> serde_json::Value {
    serde_json::to_value(delta).expect("a delta serializes")
}

/// A two-cell snapshot, so a diff (only the changed width) and a full delta
/// (both cells) are told apart.
fn bracket(width: &str) -> GuiState {
    gui_state_with_values(&[("Bracket.width", width), ("Bracket.height", "50")])
}

type Baseline = Arc<Mutex<Option<GuiState>>>;

fn publisher() -> (SnapshotPublisher, Baseline, Arc<RecordingObserver>) {
    let baseline: Baseline = Arc::new(Mutex::new(None));
    let observer = Arc::new(RecordingObserver::default());
    let publisher = SnapshotPublisher::new(Arc::clone(&baseline), observer.clone());
    (publisher, baseline, observer)
}

fn baseline_now(baseline: &Baseline) -> Option<GuiState> {
    baseline.lock().expect("baseline lock").clone()
}

#[test]
fn the_first_publish_observes_a_full_delta_and_becomes_the_baseline() {
    let (publisher, baseline, observer) = publisher();
    let state = bracket("80");

    assert!(publisher.publish(1, state.clone()));

    let observations = observer.observations();
    assert_eq!(observations.len(), 1, "exactly one delta, and no activity");
    assert_eq!(
        observations[0].thread.as_deref(),
        std::thread::current().name(),
        "the delta is observed on the publishing thread"
    );
    assert!(observer.activities().is_empty());
    assert_eq!(json(&observer.deltas()[0]), json(&StateDelta::full(&state)));
    assert_eq!(baseline_now(&baseline), Some(state));
}

#[test]
fn a_later_publish_observes_only_the_diff() {
    let (publisher, baseline, observer) = publisher();
    let (first, second) = (bracket("80"), bracket("120"));

    assert!(publisher.publish(1, first.clone()));
    assert!(publisher.publish(2, second.clone()));

    let deltas = observer.deltas();
    assert_eq!(json(&deltas[1]), json(&diff_gui_state(&first, &second)));
    assert_eq!(baseline_now(&baseline), Some(second));
}

/// INV-GUI-2: the debug server advances the same baseline through
/// `compute_delta`, and the next publish must diff against what it left.
#[test]
fn a_publish_diffs_against_a_baseline_advanced_by_another_writer() {
    let (publisher, baseline, observer) = publisher();
    let (external, next) = (bracket("100"), bracket("120"));

    assert!(publisher.publish(1, bracket("80")));
    let _ = compute_delta(&baseline, &external);
    assert!(publisher.publish(2, next.clone()));

    let deltas = observer.deltas();
    assert_eq!(json(&deltas[1]), json(&diff_gui_state(&external, &next)));
}

/// Deltas never go backwards: a publish whose generation is not newer than the
/// last published one is refused without observing a delta or touching the
/// baseline.
#[test]
fn a_publish_that_is_not_newer_is_refused_and_touches_nothing() {
    let (publisher, baseline, observer) = publisher();
    let newest = bracket("90");

    assert!(publisher.publish(9, newest.clone()));
    assert!(
        !publisher.publish(7, bracket("70")),
        "an older generation must be refused"
    );
    assert!(
        !publisher.publish(9, bracket("99")),
        "a repeated generation must be refused"
    );

    assert_eq!(observer.deltas().len(), 1);
    assert_eq!(baseline_now(&baseline), Some(newest));
}

#[test]
fn advance_baseline_returns_the_delta_compute_delta_would_and_moves_the_state_in() {
    let (old, new) = (bracket("80"), bracket("120"));
    let via_compute = Mutex::new(Some(old.clone()));
    let via_advance = Mutex::new(Some(old));

    let expected = compute_delta(&via_compute, &new);
    let actual = advance_baseline(&via_advance, new.clone());

    assert_eq!(json(&actual), json(&expected));
    assert_eq!(
        via_advance.lock().expect("baseline lock").as_ref(),
        Some(&new)
    );

    let empty = Mutex::new(None);
    assert_eq!(
        json(&advance_baseline(&empty, new.clone())),
        json(&StateDelta::full(&new))
    );
}

// ── EvalQueue: one front door, one drainer ────────────────────────────────────

type RunLog = Arc<Mutex<Vec<String>>>;

fn log_run(log: &RunLog, label: &str) {
    log.lock().expect("run log").push(label.to_string());
}

/// A snapshot that tells evaluations apart by one `Queue.label` value.
fn labelled(label: &str) -> GuiState {
    gui_state_with_values(&[("Queue.label", label)])
}

/// An edit that logs `label` when it runs and publishes a snapshot carrying it.
fn logged_edit(log: &RunLog, identity: EditIdentity, label: &str) -> EvalRequest<()> {
    let (log, label) = (Arc::clone(log), label.to_string());
    EvalRequest::edit(identity, move || {
        log_run(&log, &label);
        EvalOutcome {
            publish: Some(labelled(&label)),
            reply: Ok(()),
        }
    })
}

/// An evaluation that logs `label`, publishes a snapshot carrying it and
/// replies with it.
fn logged_evaluation(log: &RunLog, label: &str) -> EvalRequest<String> {
    let (log, label) = (Arc::clone(log), label.to_string());
    EvalRequest::evaluation(move || {
        log_run(&log, &label);
        EvalOutcome {
            publish: Some(labelled(&label)),
            reply: Ok(label),
        }
    })
}

/// An engine call that logs `label` and replies with it.
fn logged_engine_call(log: &RunLog, label: &str) -> EvalRequest<String> {
    let (log, label) = (Arc::clone(log), label.to_string());
    EvalRequest::engine_call(move || {
        log_run(&log, &label);
        Ok(label)
    })
}

/// A queue whose drainers run on the test thread, when the test says.
struct ManualRig {
    queue: Arc<EvalQueue>,
    executor: Arc<ManualExecutor>,
    observer: Arc<RecordingObserver>,
    log: RunLog,
}

impl ManualRig {
    fn new() -> Self {
        let ManualQueue {
            queue,
            executor,
            observer,
        } = ManualQueue::new();
        Self {
            queue,
            executor,
            observer,
            log: RunLog::default(),
        }
    }

    fn edit(&self, identity: EditIdentity, label: &str) -> EvalTicket<()> {
        self.queue.submit(logged_edit(&self.log, identity, label))
    }

    fn evaluation(&self, label: &str) -> EvalTicket<String> {
        self.queue.submit(logged_evaluation(&self.log, label))
    }

    fn engine_call(&self, label: &str) -> EvalTicket<String> {
        self.queue.submit(logged_engine_call(&self.log, label))
    }

    /// The labels of the jobs that ran, in order.
    fn ran(&self) -> Vec<String> {
        self.log.lock().expect("run log").clone()
    }

    /// What the observer saw, in order: activities by name, deltas by the
    /// labels they carry.
    fn timeline(&self) -> Vec<String> {
        self.observer
            .observations()
            .into_iter()
            .map(|observation| match observation.observed {
                Observed::Activity(activity) => format!("{activity:?}"),
                Observed::Delta(delta) => {
                    let labels: Vec<_> = delta
                        .changed_values
                        .iter()
                        .map(|v| v.value.as_str())
                        .collect();
                    format!("delta {}", labels.join(","))
                }
            })
            .collect()
    }
}

#[test]
fn edits_of_different_targets_run_and_publish_in_acceptance_order() {
    let rig = ManualRig::new();
    let tickets = [
        rig.edit(preview("A", 1), "A"),
        rig.edit(commit("B", 2), "B"),
        rig.edit(editor("p.ri", 3), "P"),
    ];
    assert_eq!(
        rig.executor.pending(),
        1,
        "one drainer serves the whole backlog"
    );

    rig.executor.run_pending();

    assert_eq!(rig.ran(), ["A", "B", "P"]);
    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta A", "delta B", "delta P", "Idle"]
    );
    for ticket in tickets {
        assert_eq!(settled(ticket), Ok(()));
    }
}

/// The previews arrive while the first edit is the running entry, the way
/// slider frames arrive during a long evaluation.
#[test]
fn previews_arriving_while_an_edit_runs_coalesce_to_the_newest() {
    let rig = ManualRig::new();
    let arrivals = Arc::new(Mutex::new(Vec::new()));
    let running = {
        let queue = Arc::clone(&rig.queue);
        let (log, arrivals) = (Arc::clone(&rig.log), Arc::clone(&arrivals));
        rig.queue
            .submit(EvalRequest::edit(preview("B", 1), move || {
                for seq in 1..=5 {
                    let label = format!("A{seq}");
                    let ticket = queue.submit(logged_edit(&log, preview("A", seq), &label));
                    arrivals.lock().expect("arrivals").push(ticket);
                }
                log_run(&log, "B");
                EvalOutcome {
                    publish: Some(labelled("B")),
                    reply: Ok(()),
                }
            }))
    };

    rig.executor.run_pending();

    assert_eq!(
        rig.ran(),
        ["B", "A5"],
        "only the newest queued preview runs"
    );
    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta B", "delta A5", "Idle"],
        "superseded previews leave the busy period, and are never published"
    );
    assert_eq!(settled(running), Ok(()));
    let arrivals = std::mem::take(&mut *arrivals.lock().expect("arrivals"));
    for ticket in arrivals {
        assert_eq!(settled(ticket), Ok(()), "a superseded preview resolves Ok");
    }
}

#[test]
fn a_queued_commit_survives_newer_previews_of_its_cell() {
    let rig = ManualRig::new();
    let tickets = [
        rig.edit(commit("A", 1), "commit"),
        rig.edit(preview("A", 2), "frame 2"),
        rig.edit(preview("A", 3), "frame 3"),
    ];

    rig.executor.run_pending();

    assert_eq!(rig.ran(), ["commit", "frame 3"]);
    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta commit", "delta frame 3", "Idle"]
    );
    for ticket in tickets {
        assert_eq!(settled(ticket), Ok(()));
    }
}

#[test]
fn an_edit_older_than_an_admitted_one_resolves_at_once_without_running() {
    let rig = ManualRig::new();
    let newest = rig.edit(preview("A", 5), "A5");
    rig.executor.run_pending();
    let before = rig.timeline();

    let mut late = rig.edit(preview("A", 4), "A4");

    assert_eq!(
        poll_now(&mut late),
        Some(Ok(())),
        "a late edit resolves at once"
    );
    assert_eq!(rig.executor.pending(), 0, "a late edit posts no drainer");
    assert_eq!(rig.timeline(), before, "a late edit reports no activity");
    assert_eq!(rig.ran(), ["A5"]);
    assert_eq!(settled(newest), Ok(()));
}

/// A superseding edit takes its own arrival position, so it never jumps ahead
/// of a request submitted before it; ordered requests never coalesce.
#[test]
fn ordered_requests_are_barriers_a_superseding_edit_never_jumps() {
    let rig = ManualRig::new();
    let first_edit = rig.edit(preview("A", 1), "A1");
    let call = rig.engine_call("call");
    let evaluation = rig.evaluation("evaluation");
    let second_edit = rig.edit(preview("A", 2), "A2");
    let repeated = [rig.engine_call("again"), rig.engine_call("again")];

    rig.executor.run_pending();

    assert_eq!(rig.ran(), ["call", "evaluation", "A2", "again", "again"]);
    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta evaluation", "delta A2", "Idle"]
    );
    assert_eq!(settled(first_edit), Ok(()));
    assert_eq!(settled(call), Ok("call".to_string()));
    assert_eq!(settled(evaluation), Ok("evaluation".to_string()));
    assert_eq!(settled(second_edit), Ok(()));
    for ticket in repeated {
        assert_eq!(settled(ticket), Ok("again".to_string()));
    }
}

#[test]
fn engine_calls_alone_never_report_activity() {
    let rig = ManualRig::new();
    let calls = [rig.engine_call("a"), rig.engine_call("b")];

    rig.executor.run_pending();

    assert!(rig.timeline().is_empty(), "got {:?}", rig.timeline());
    for ticket in calls {
        assert!(settled(ticket).is_ok());
    }
}

#[test]
fn a_busy_period_reports_evaluating_once_and_idle_after_its_last_delta() {
    let rig = ManualRig::new();
    let _edit = rig.edit(preview("A", 1), "A1");
    let _call = rig.engine_call("call");
    let _evaluation = rig.evaluation("E");
    assert_eq!(
        rig.timeline(),
        ["Evaluating"],
        "reported as the first edit is accepted"
    );

    rig.executor.run_pending();

    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta A1", "delta E", "Idle"]
    );
}

/// Records, at each observed delta, whether `ticket` had already resolved.
#[derive(Default)]
struct ReplyProbe {
    ticket: Mutex<Option<EvalTicket<String>>>,
    resolved_at_delta: Mutex<Vec<bool>>,
}

impl EvalObserver for ReplyProbe {
    fn activity(&self, _: EvalActivity) {}

    fn delta(&self, _: &StateDelta) {
        let mut ticket = self.ticket.lock().expect("probe ticket");
        let resolved = ticket.as_mut().is_some_and(|t| poll_now(t).is_some());
        self.resolved_at_delta
            .lock()
            .expect("probe log")
            .push(resolved);
    }
}

#[test]
fn an_evaluation_replies_only_after_its_delta_is_published() {
    let executor = ManualExecutor::new();
    let probe = Arc::new(ReplyProbe::default());
    let queue = EvalQueue::with_executor(
        executor.executor(),
        Arc::new(Mutex::new(None)),
        probe.clone(),
    );
    let ticket = queue.submit(logged_evaluation(&RunLog::default(), "E"));
    *probe.ticket.lock().expect("probe ticket") = Some(ticket);

    executor.run_pending();

    assert_eq!(*probe.resolved_at_delta.lock().expect("probe log"), [false]);
    let ticket = probe.ticket.lock().expect("probe ticket").take();
    assert_eq!(settled(ticket.expect("ticket")), Ok("E".to_string()));
}

#[test]
fn a_panicking_job_resolves_err_and_the_queue_carries_on() {
    let rig = ManualRig::new();
    let panicking = rig
        .queue
        .submit(EvalRequest::<()>::evaluation(|| panic!("kaboom")));
    let next = rig.edit(preview("A", 1), "A1");

    rig.executor.run_pending();

    let error = settled(panicking).expect_err("a panicking job must resolve Err");
    assert!(error.contains("kaboom"), "got {error:?}");
    assert_eq!(settled(next), Ok(()));
    assert_eq!(rig.ran(), ["A1"]);
    assert_eq!(rig.observer.activities().last(), Some(&EvalActivity::Idle));
}

#[test]
fn a_refused_drainer_resolves_the_ticket_err_and_leaves_the_queue_idle() {
    let rig = ManualRig::new();
    rig.executor.refuse(true);

    let mut refused = rig.evaluation("refused");

    assert!(matches!(poll_now(&mut refused), Some(Err(_))));
    assert_ne!(
        rig.observer.activities().last(),
        Some(&EvalActivity::Evaluating),
        "a refused request must not leave the queue busy"
    );

    rig.executor.refuse(false);
    let next = rig.evaluation("next");
    rig.executor.run_pending();
    assert_eq!(settled(next), Ok("next".to_string()));
    assert_eq!(rig.ran(), ["next"]);
}

/// A panic payload that panics again when dropped, `.0` more times: the one way
/// a job's panic escapes both of the queue's catches, since each catch drops
/// the payload it caught.
struct PanickingPayload(u32);

impl Drop for PanickingPayload {
    fn drop(&mut self) {
        if let Some(again) = self.0.checked_sub(1) {
            std::panic::panic_any(PanickingPayload(again));
        }
    }
}

/// Submit a job whose panic unwinds out of the drainer running it, then one
/// evaluation behind it.
fn escaping_then_behind(rig: &ManualRig) -> (EvalTicket<()>, EvalTicket<String>) {
    let escaping = rig.queue.submit(EvalRequest::<()>::evaluation(|| {
        std::panic::panic_any(PanickingPayload(2))
    }));
    (escaping, rig.evaluation("behind"))
}

fn run_pending_expecting_an_unwind(rig: &ManualRig) {
    let unwound = std::panic::catch_unwind(AssertUnwindSafe(|| rig.executor.run_pending()));
    assert!(
        unwound.is_err(),
        "the payload's last panic must escape the drainer"
    );
}

#[test]
fn a_drainer_that_unwinds_hands_what_is_queued_to_a_new_drainer() {
    let rig = ManualRig::new();
    let (escaping, behind) = escaping_then_behind(&rig);

    run_pending_expecting_an_unwind(&rig);
    rig.executor.run_pending();

    assert!(settled(escaping).is_err());
    assert_eq!(settled(behind), Ok("behind".to_string()));
    assert_eq!(rig.ran(), ["behind"]);
    assert_eq!(
        rig.timeline(),
        ["Evaluating", "delta behind", "Idle"],
        "the busy period still ends"
    );
}

#[test]
fn a_drainer_that_unwinds_with_no_executor_left_fails_what_is_queued() {
    let rig = ManualRig::new();
    let (escaping, behind) = escaping_then_behind(&rig);
    rig.executor.refuse(true);

    run_pending_expecting_an_unwind(&rig);

    assert!(settled(escaping).is_err());
    assert!(
        settled(behind).is_err(),
        "a request no drainer can reach must resolve Err, not hang"
    );
    assert_eq!(rig.timeline(), ["Evaluating", "Idle"]);

    rig.executor.refuse(false);
    let next = rig.evaluation("next");
    rig.executor.run_pending();
    assert_eq!(settled(next), Ok("next".to_string()));
    assert_eq!(rig.ran(), ["next"]);
}

#[tokio::test]
async fn on_the_engine_lane_requests_run_and_publish_on_the_large_stack_lane() {
    use crate::large_stack::WORKER_THREAD_NAME;

    let observer = Arc::new(RecordingObserver::default());
    let queue = EvalQueue::on_engine_lane(Arc::new(Mutex::new(None)), observer.clone());

    let thread = queue
        .submit(EvalRequest::engine_call(|| {
            Ok(std::thread::current().name().map(str::to_owned))
        }))
        .await;
    assert_eq!(thread, Ok(Some(WORKER_THREAD_NAME.to_string())));

    let depth = queue
        .submit(EvalRequest::engine_call(|| {
            deep_recurse_if_on_thread(WORKER_THREAD_NAME, DEEP_RECURSION_DEPTH)
        }))
        .await;
    assert_eq!(depth, Ok(u64::from(DEEP_RECURSION_DEPTH) + 1));

    let published = queue
        .submit(logged_evaluation(&RunLog::default(), "E"))
        .await;
    assert_eq!(published, Ok("E".to_string()));
    let delta_threads: Vec<_> = observer
        .observations()
        .into_iter()
        .filter(|o| matches!(o.observed, Observed::Delta(_)))
        .map(|o| o.thread)
        .collect();
    assert_eq!(delta_threads, [Some(WORKER_THREAD_NAME.to_string())]);
}

/// `#[tokio::test]` runs a CURRENT-THREAD runtime, so the task that releases the
/// job can only run while the test is parked awaiting the ticket: an await that
/// blocked the thread would leave the job unreleased until its anti-wedge bound.
#[tokio::test]
async fn awaiting_a_ticket_does_not_block_the_runtime() {
    let queue = EvalQueue::on_engine_lane(
        Arc::new(Mutex::new(None)),
        Arc::new(RecordingObserver::default()),
    );
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();

    let ticket = queue.submit(EvalRequest::engine_call(move || {
        release_rx
            .recv_timeout(ANTI_WEDGE)
            .map_err(|_| "never released: awaiting the ticket blocked the runtime".to_string())
    }));
    tokio::spawn(async move {
        let _ = release_tx.send(());
    });

    assert_eq!(ticket.await, Ok(()));
}
