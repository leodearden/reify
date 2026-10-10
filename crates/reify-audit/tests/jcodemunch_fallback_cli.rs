//! End-to-end CLI coverage for the jcodemunch seam's START-OF-RUN fallback:
//! the run needs a jcodemunch-backed detector, but the seam it binds is the
//! inert stub because the endpoint never answered or refused the handshake.
//!
//! Pins two things the real binary reports about it:
//!
//! - on stderr, the label: an endpoint that never answered is
//!   `jcodemunch unreachable at`, one that answered but is not a live MCP seam
//!   is `jcodemunch handshake refused at`, so an operator is sent to start a
//!   serve in the first case and to check what is listening on the port in
//!   the second;
//! - in the findings array, ONE run-level Low unexamined finding (empty
//!   `task_id`) per affected detector, carrying that reason, so the run's
//!   zero findings from that detector cannot read as a clean result.
//!   `--no-jcodemunch` is a declared scope reduction and records nothing.
//!
//! Hermetic: the refusing endpoint is `common::net::unreachable_mcp_url()`,
//! the answering ones are `common::mcp_mock` responders on ephemeral ports.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test jcodemunch_fallback_cli`

mod common;

#[path = "common/breadcrumbs.rs"]
mod breadcrumbs;

#[path = "common/cli_fixture.rs"]
mod cli_fixture;

#[path = "common/task_json.rs"]
mod task_json;

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};

use breadcrumbs::{CONSTRUCTION_HANDSHAKE_REFUSED, CONSTRUCTION_UNREACHABLE};
use common::findings::parse_findings_from_stderr;
use common::mcp_mock::{InitializeReply, MockConfig, MockServer, spawn_mock_mcp_configured};
use serde_json::Value;

/// A tempdir standing in for the project root, holding the run's own
/// `tasks.json` and `runs.db`, so no run reaches fused-memory or a shared DB.
struct Sandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    tasks_file: PathBuf,
    runs_db: PathBuf,
}

fn sandbox(tasks: &[serde_json::Value]) -> Sandbox {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let root = tmp.path().to_path_buf();
    let tasks_file = root.join("tasks.json");
    std::fs::write(
        &tasks_file,
        serde_json::to_string_pretty(tasks).expect("serialize tasks.json"),
    )
    .expect("write tasks.json");
    let runs_db = cli_fixture::write_empty_runs_db(&root);
    Sandbox {
        _tmp: tmp,
        root,
        tasks_file,
        runs_db,
    }
}

impl Sandbox {
    /// Invoke the binary with `args` plus this sandbox's substrates.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_reify-audit"))
            .args(args)
            .args([
                "--tasks-file",
                self.tasks_file.to_str().unwrap(),
                "--runs-db",
                self.runs_db.to_str().unwrap(),
                "--project-root",
                self.root.to_str().unwrap(),
            ])
            .output()
            .expect("invoke reify-audit")
    }
}

/// A responder that answers `initialize` with `status` and `body` verbatim
/// while still assigning a session id, so only the status or the body can be
/// what refuses it.
fn spawn_handshake_answering(status: u16, body: &'static str) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let config = MockConfig {
        initialize: InitializeReply::Raw { status, body },
        ..MockConfig::default()
    };
    spawn_mock_mcp_configured(listener, config, |_args| None)
}

/// The run completed (exit 0: nothing here is High) and its stderr names the
/// construction failure with `label` pinned to `url`, and not with `wrong`.
fn assert_construction_labelled(out: &Output, label: &str, wrong: &str, url: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a construction failure must fail soft, not refuse; stderr:\n{stderr}"
    );
    let expected = format!("{label} '{url}'");
    assert!(
        stderr.contains(&expected),
        "stderr must carry `{expected}`; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains(wrong),
        "stderr must not carry `{wrong}` for this failure; stderr:\n{stderr}"
    );
}

#[test]
fn an_endpoint_that_never_answered_is_labelled_unreachable() {
    let s = sandbox(&[]);
    let url = common::net::unreachable_mcp_url();
    let out = s.run(&["--pattern", "P1", "--jcodemunch-url", &url]);
    assert_construction_labelled(
        &out,
        CONSTRUCTION_UNREACHABLE,
        CONSTRUCTION_HANDSHAKE_REFUSED,
        &url,
    );
}

/// The handshake-refused label, and no `E_JC_INDEX_` marker: the freshness
/// gate only runs after a SUCCESSFUL handshake, so its absence is what shows
/// the handshake was refused rather than admitted.
fn assert_handshake_refused(status: u16, body: &'static str) {
    let s = sandbox(&[]);
    let mock = spawn_handshake_answering(status, body);
    let url = mock.url().to_string();
    let out = s.run(&["--pattern", "P1", "--jcodemunch-url", &url]);
    mock.stop();
    assert_construction_labelled(
        &out,
        CONSTRUCTION_HANDSHAKE_REFUSED,
        CONSTRUCTION_UNREACHABLE,
        &url,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E_JC_INDEX_"),
        "the freshness gate must not have run; stderr:\n{stderr}"
    );
}

#[test]
fn a_non_mcp_2xx_responder_is_labelled_handshake_refused() {
    assert_handshake_refused(200, "{}");
}

#[test]
fn a_foreign_http_service_answering_404_is_labelled_handshake_refused() {
    assert_handshake_refused(404, "not found");
}

/// One P1-eligible done task, so a P1 run with a live seam would have queried
/// jcodemunch for it.
fn p1_eligible_tasks() -> Vec<Value> {
    vec![task_json::done_task_fixture(
        "7001",
        "0123456789abcdef0123456789abcdef01234567",
        1_700_000_000,
    )]
}

/// The findings array of a run that must have exited 0.
fn findings_of_a_clean_exit(out: &Output) -> Vec<Value> {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a Low unexamined record never moves the exit code; stderr:\n{stderr}"
    );
    parse_findings_from_stderr(&stderr)
}

/// `findings` holds exactly one `pattern` finding, and it is the run-level
/// unexamined record: Low, empty `task_id`, its summary carrying `reason`.
fn assert_one_run_level_unexamined(findings: &[Value], pattern: &str, reason: &str) {
    let all = Value::Array(findings.to_vec());
    let matching: Vec<&Value> = findings
        .iter()
        .filter(|f| f["pattern"].as_str() == Some(pattern))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one {pattern}; findings:\n{all:#}"
    );
    let finding = matching[0];
    assert_eq!(finding["severity"].as_str(), Some("Low"), "{finding:#}");
    assert_eq!(
        finding["task_id"].as_str(),
        Some(""),
        "the start-of-run record is run-level, not about one task: {finding:#}"
    );
    let summary = finding["summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains(reason),
        "the summary must carry the reason `{reason}`; got {summary:?}"
    );
}

#[test]
fn p1_records_itself_unexamined_once_when_the_endpoint_never_answered() {
    let s = sandbox(&p1_eligible_tasks());
    let url = common::net::unreachable_mcp_url();
    let out = s.run(&["--pattern", "P1", "--jcodemunch-url", &url]);
    let findings = findings_of_a_clean_exit(&out);
    assert_eq!(
        findings.len(),
        1,
        "the run-level record is P1's only finding: {:#}",
        Value::Array(findings.clone())
    );
    assert_one_run_level_unexamined(
        &findings,
        "P1TaskUnexamined",
        &format!("jcodemunch unreachable at '{url}'"),
    );
}

#[test]
fn p1_records_itself_unexamined_once_when_the_handshake_was_refused() {
    let s = sandbox(&p1_eligible_tasks());
    let mock = spawn_handshake_answering(404, "not found");
    let url = mock.url().to_string();
    let out = s.run(&["--pattern", "P1", "--jcodemunch-url", &url]);
    mock.stop();
    let findings = findings_of_a_clean_exit(&out);
    assert_eq!(
        findings.len(),
        1,
        "the run-level record is P1's only finding: {:#}",
        Value::Array(findings.clone())
    );
    assert_one_run_level_unexamined(
        &findings,
        "P1TaskUnexamined",
        &format!("jcodemunch handshake refused at '{url}'"),
    );
}

/// `--no-jcodemunch` declares the jcodemunch-backed passes out of scope (the
/// per-run artifact records the argv), so nothing is recorded as unexamined.
#[test]
fn no_jcodemunch_records_nothing_unexamined() {
    let s = sandbox(&p1_eligible_tasks());
    let url = common::net::unreachable_mcp_url();
    let out = s.run(&[
        "--pattern",
        "P1",
        "--no-jcodemunch",
        "--jcodemunch-url",
        &url,
    ]);
    let findings = findings_of_a_clean_exit(&out);
    assert!(
        findings.is_empty(),
        "a declared scope reduction records no unexamined finding: {:#}",
        Value::Array(findings)
    );
}
