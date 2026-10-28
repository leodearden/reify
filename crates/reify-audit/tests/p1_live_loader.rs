//! P1's pending-consumer guard over the live fused-memory loader.
//!
//! P1 skips a done producer when a pending/in-progress/review task's
//! `consumer_ref` names the producer's `TaskMetadata.prd`. The live loader
//! fills that field from whichever PRD key a task carries, so the guard must
//! hold for a producer tagged by either key. The corpus is served from
//! `tests/common/mcp_mock.rs` through the public `FusedMemoryClient`, and P1
//! runs over `MockJCodemunchOps`.

mod common;

use common::mcp_mock::spawn_mock_mcp;
use reify_audit::fused_memory_client::FusedMemoryClient;
use reify_audit::{
    AuditContext, ChangedSymbol, DeclSuppression, Finding, MockGitOps, MockJCodemunchOps, Pattern,
    TaskMetadata, p1_producer_orphan,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;

const NOW: i64 = 1_700_000_000;
const PAST_GRACE_DONE_AT: i64 = NOW - 15 * 86_400;
const PRODUCING_PRD: &str = "docs/prds/v0_6/widget.md";
const PRODUCER_COMMIT: &str = "c0ffee";
const ORPHAN_FILE: &str = "crates/reify-x/src/widget.rs";
const ORPHAN_NAME: &str = "new_widget";

/// Every task the mock fused-memory serves as `wire_tasks`, read back through
/// `FusedMemoryClient::get_tasks`.
fn load_live(wire_tasks: Value) -> Vec<TaskMetadata> {
    let mock = spawn_mock_mcp(move |_args| Some(json!({"tasks": wire_tasks})));
    let tasks = FusedMemoryClient::new(mock.url())
        .expect("connect to the mock fused-memory")
        .get_tasks("/tmp/fake-project")
        .expect("get_tasks over the mock fused-memory");
    mock.stop();
    tasks
}

/// A done producer, past P1's grace window, whose PRD is tagged under
/// `prd_key`.
fn wire_producer(prd_key: &str) -> Value {
    json!({
        "id": 7001,
        "title": "Introduce the widget",
        "status": "done",
        "metadata": {
            prd_key: PRODUCING_PRD,
            "done_at": PAST_GRACE_DONE_AT,
            "done_provenance": {"kind": "merged", "commit": PRODUCER_COMMIT},
        },
    })
}

/// A pending task consuming the producer's PRD.
fn wire_consumer() -> Value {
    json!({
        "id": 7002,
        "title": "Consume the widget",
        "status": "pending",
        "metadata": {"consumer_ref": PRODUCING_PRD},
    })
}

/// P1 over `tasks`, where the producer's commit introduced one public symbol
/// that nothing calls.
fn p1_findings(tasks: Vec<TaskMetadata>) -> Vec<Finding> {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    let git = MockGitOps::new();
    let mut jc = MockJCodemunchOps::new();
    jc.set_changed_symbols(
        &format!("{PRODUCER_COMMIT}^1"),
        PRODUCER_COMMIT,
        vec![ChangedSymbol {
            name: ORPHAN_NAME.to_string(),
            file: ORPHAN_FILE.to_string(),
            line: 42,
            suppression: Some(DeclSuppression::default()),
        }],
    );
    jc.set_find_references(ORPHAN_FILE, ORPHAN_NAME, vec![]);
    let ctx = AuditContext {
        project_root: PathBuf::from("/tmp/fake-project"),
        conn: &conn,
        git: &git,
        jcodemunch: &jc,
        task_metadata: tasks.into_iter().map(|t| (t.task_id.clone(), t)).collect(),
        target_task_id: None,
        window: None,
        now: Some(NOW),
        producer_branch: None,
    };
    p1_producer_orphan::check(&ctx)
}

/// Without a consumer the producer is an orphan, so the silence beside one is
/// the guard firing, not a producer P1 never examined.
#[test]
fn a_pending_consumer_guards_a_producer_tagged_under_either_prd_key() {
    for prd_key in ["prd", "prd_path"] {
        let unguarded = p1_findings(load_live(json!([wire_producer(prd_key)])));
        let patterns: Vec<Pattern> = unguarded.iter().map(|f| f.pattern).collect();
        assert_eq!(
            patterns,
            vec![Pattern::P1ProducerOrphan],
            "metadata.{prd_key}: an unconsumed producer is an orphan; got {unguarded:?}"
        );

        let guarded = p1_findings(load_live(json!([wire_producer(prd_key), wire_consumer()])));
        assert!(
            guarded.is_empty(),
            "metadata.{prd_key}: a pending consumer of the producer's PRD must \
             suppress the orphan; got {guarded:?}"
        );
    }
}
