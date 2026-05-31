//! Live integration smoke test: real `reify-audit` binary vs live jcodemunch serve.
//!
//! Exercises the full wire: binary → `RealJCodemunchOps` → jcodemunch-serve MCP
//! and asserts ≥1 well-formed `P1ProducerOrphan` finding AND ≥1 `PDeadCode`
//! finding from the reify corpus.  The point is to catch a wire/trait/detector
//! mismatch that mock tests cannot.
//!
//! ## On-demand run command (serve must be up)
//!
//! ```sh
//! # Default URL (http://127.0.0.1:8901/mcp):
//! cargo test -p reify-audit --test jcodemunch_live -- --ignored
//!
//! # Custom serve URL:
//! JCODEMUNCH_URL=http://127.0.0.1:8901/mcp \
//!   cargo test -p reify-audit --test jcodemunch_live -- --ignored
//! ```
//!
//! ## Serve prerequisite
//!
//! Start jcodemunch-serve before running the ignored test, e.g.:
//! ```sh
//! cd /path/to/jcodemunch && npm run serve -- --port 8901
//! ```
//!
//! When the serve is not up the ignored test gracefully skips (prints a note
//! to stderr and returns early) rather than hard-failing.  The hermetic unit
//! tests in the `finding_shape` and `serve_preflight` modules (not `#[ignore]`)
//! always run as part of standard `cargo test` and catch compile-time drift
//! in the wire shape.

// -----------------------------------------------------------------------
// Finding-shape predicates (pure; no serve needed)
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Serve-availability preflight (pure TCP connect; no MCP handshake)
// -----------------------------------------------------------------------

/// Returns true iff the jcodemunch-serve process is accepting TCP connections
/// at the address encoded in `url`.
///
/// Parses `host:port` from the URL and attempts
/// [`TcpStream::connect_timeout`] with a 2-second timeout.  A bare TCP
/// connect is sufficient to distinguish "serve process listening" from "serve
/// down" for the skip gate; the binary's own MCP handshake does the deeper
/// protocol check on the live legs.
///
/// Returns false on parse failure, connection refused, or timeout.
fn jcodemunch_serve_reachable(url: &str) -> bool {
    // Strip scheme to get "host:port[/path]"
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    // Take just the "host:port" part (before any slash)
    let host_port = without_scheme.split('/').next().unwrap_or("");
    let addr: std::net::SocketAddr = match host_port.parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).is_ok()
}

/// Returns true iff `v` is a P1ProducerOrphan finding.
///
/// Mirrors cli.rs's pattern-string comparison: `Pattern` serializes to its
/// bare variant name (`"P1ProducerOrphan"`) with no serde rename, so we
/// compare against the raw string.
fn is_p1_finding(v: &serde_json::Value) -> bool {
    v["pattern"].as_str() == Some("P1ProducerOrphan")
}

/// Returns true iff `v` is a PDeadCode finding.
fn is_pdead_finding(v: &serde_json::Value) -> bool {
    v["pattern"].as_str() == Some("PDeadCode")
}

// -----------------------------------------------------------------------
// Pinned live constants
// -----------------------------------------------------------------------
//
// Commit used for the P1 leg.  The commit is a real reify commit whose diff
// touched Rust source; the range PINNED_P1_COMMIT^1..PINNED_P1_COMMIT feeds
// jcodemunch get_changed_symbols and is expected to contain ≥1 still-orphaned
// public symbol.
//
// Resolved on-demand against the running jcodemunch serve.  Update this SHA
// if the commit's diff no longer contains any orphan symbol after corpus churn
// (pick a later commit that introduced new public Rust symbols):
//
//   git log --oneline --no-merges HEAD~20..HEAD -- crates/
//
// ff1cb80c31 = merge of task/4097 (L-PDEAD) which added pdead_dead_code.rs,
// a new Rust source file with new public symbols.
const PINNED_P1_COMMIT: &str = "ff1cb80c31";
const JCODEMUNCH_REPO: &str = "leodearden/reify";
const DEFAULT_SERVE_URL: &str = "http://127.0.0.1:8901/mcp";

// -----------------------------------------------------------------------
// Capstone live integration test (#[ignore]-gated; requires serve up)
// -----------------------------------------------------------------------

/// End-to-end smoke: real `reify-audit` binary → live jcodemunch serve.
///
/// Asserts ≥1 `PDeadCode` finding (P-DEAD leg, repo-wide) and ≥1
/// `P1ProducerOrphan` finding (P1 leg, over a pinned real reify commit).
///
/// **Graceful skip**: if the serve is not reachable the test prints a note to
/// stderr and returns without failing (mirrors `baseline_report_freshness`).
///
/// Run with the serve up:
/// ```sh
/// cargo test -p reify-audit --test jcodemunch_live -- --ignored
/// ```
#[ignore = "live integration: requires jcodemunch-serve up on default or $JCODEMUNCH_URL; run via --ignored"]
#[test]
fn live_audit_produces_p1_and_pdead_findings() {
    let serve_url = std::env::var("JCODEMUNCH_URL")
        .unwrap_or_else(|_| DEFAULT_SERVE_URL.to_string());

    // Preflight: skip gracefully if serve is not running.
    if !jcodemunch_serve_reachable(&serve_url) {
        eprintln!(
            "live_audit_produces_p1_and_pdead_findings: jcodemunch-serve not reachable \
             at {serve_url} — skipping (run with serve up to exercise live assertions)"
        );
        return;
    }

    // Resolve repo root from CARGO_MANIFEST_DIR (crates/reify-audit → two parents).
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("crates/reify-audit has a parent")
        .parent()
        .expect("crates/ has a parent (repo root)")
        .to_str()
        .expect("project root is valid UTF-8")
        .to_string();

    let tmp = tempfile::tempdir().expect("create tempdir");
    let dir = tmp.path();
    let runs_db = write_empty_runs_db(dir);

    // -------------------------------------------------------------------
    // P-DEAD leg: --pattern PDEAD (repo-wide; serve-only; no tasks needed)
    // -------------------------------------------------------------------
    let empty_tasks = write_empty_tasks_file(dir);
    let (pdead_code, pdead_findings) = run_reify_audit(&[
        "--pattern",
        "PDEAD",
        "--jcodemunch-url",
        &serve_url,
        "--jcodemunch-repo",
        JCODEMUNCH_REPO,
        "--tasks-file",
        empty_tasks.to_str().unwrap(),
        "--runs-db",
        runs_db.to_str().unwrap(),
        "--project-root",
        &project_root,
    ]);
    assert_ne!(
        pdead_code,
        Some(125),
        "PDEAD leg: exit 125 = infra/connection error; serve may have dropped\n\
         all findings: {:#}",
        serde_json::Value::Array(pdead_findings.clone())
    );
    let pdead_matched: Vec<&serde_json::Value> =
        pdead_findings.iter().filter(|f| is_pdead_finding(f)).collect();
    assert!(
        !pdead_matched.is_empty(),
        "PDEAD leg: expected ≥1 PDeadCode finding from get_dead_code_v2 over reify corpus; \
         got 0\nAll findings: {:#}",
        serde_json::Value::Array(pdead_findings.clone())
    );
    println!(
        "PDEAD leg: {} PDeadCode finding(s) matched — first:\n{:#}",
        pdead_matched.len(),
        pdead_matched[0]
    );

    // -------------------------------------------------------------------
    // P1 leg: --pattern P1 over ONE pinned done-task commit
    //
    // The synthetic --tasks-file contains a single done task with
    // done_at set (so P1 does not skip it) and done_provenance.commit
    // pointing at PINNED_P1_COMMIT. P1 maps this to the range
    // PINNED_P1_COMMIT^1..PINNED_P1_COMMIT via get_changed_symbols.
    // -------------------------------------------------------------------
    // done_at_epoch ≈ 2025-05-23 (any non-zero epoch is fine; P1 only
    // checks that done_at is Some, not the exact value).
    let synthetic_tasks = write_synthetic_done_task(dir, PINNED_P1_COMMIT, 1_748_000_000);
    let (p1_code, p1_findings) = run_reify_audit(&[
        "--pattern",
        "P1",
        "--jcodemunch-url",
        &serve_url,
        "--jcodemunch-repo",
        JCODEMUNCH_REPO,
        "--tasks-file",
        synthetic_tasks.to_str().unwrap(),
        "--runs-db",
        runs_db.to_str().unwrap(),
        "--project-root",
        &project_root,
    ]);
    assert_ne!(
        p1_code,
        Some(125),
        "P1 leg: exit 125 = infra/connection error; serve may have dropped\n\
         all findings: {:#}",
        serde_json::Value::Array(p1_findings.clone())
    );
    let p1_matched: Vec<&serde_json::Value> =
        p1_findings.iter().filter(|f| is_p1_finding(f)).collect();
    assert!(
        !p1_matched.is_empty(),
        "P1 leg: expected ≥1 P1ProducerOrphan finding for pinned commit {PINNED_P1_COMMIT}; \
         got 0\nAll findings: {:#}\n\
         Hint: if the pinned commit's diff has no orphan symbol after corpus churn, \
         update PINNED_P1_COMMIT to a later reify commit that introduced new public symbols.",
        serde_json::Value::Array(p1_findings.clone())
    );
    println!(
        "P1 leg: {} P1ProducerOrphan finding(s) matched — first:\n{:#}",
        p1_matched.len(),
        p1_matched[0]
    );
}

// -----------------------------------------------------------------------
// Finding-shape predicate unit tests (hermetic; always run — no serve needed)
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Serve-availability preflight unit test (hermetic; always run — no serve needed)
// -----------------------------------------------------------------------

#[cfg(test)]
mod serve_preflight {
    use super::*;
    use std::net::TcpListener;

    /// A freed port (bind → record → drop listener) must be reported as
    /// unreachable.  This mirrors cli.rs's `closed_port_url` idiom and
    /// exercises the TCP-connect gate the `#[ignore]` capstone uses to
    /// skip cleanly when jcodemunch-serve is not running.
    #[test]
    fn closed_port_is_not_reachable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        drop(listener); // port is now freed
        let url = format!("http://127.0.0.1:{port}/mcp");
        assert!(
            !jcodemunch_serve_reachable(&url),
            "freed port {port} must not be reported as reachable"
        );
    }
}

#[cfg(test)]
mod finding_shape {
    use super::*;

    /// `P1ProducerOrphan` satisfies `is_p1_finding` and NOT `is_pdead_finding`.
    #[test]
    fn p1_finding_classified_correctly() {
        let v = serde_json::json!({
            "pattern": "P1ProducerOrphan",
            "severity": "Low",
            "task_id": "t",
            "summary": "s",
            "evidence": []
        });
        assert!(is_p1_finding(&v), "P1ProducerOrphan must satisfy is_p1_finding");
        assert!(!is_pdead_finding(&v), "P1ProducerOrphan must not satisfy is_pdead_finding");
    }

    /// `PDeadCode` satisfies `is_pdead_finding` and NOT `is_p1_finding`.
    #[test]
    fn pdead_finding_classified_correctly() {
        let v = serde_json::json!({
            "pattern": "PDeadCode",
            "severity": "Low",
            "task_id": "",
            "summary": "dead fn foo",
            "evidence": []
        });
        assert!(is_pdead_finding(&v), "PDeadCode must satisfy is_pdead_finding");
        assert!(!is_p1_finding(&v), "PDeadCode must not satisfy is_p1_finding");
    }

    /// `P5PhantomDone` is classified as NEITHER.
    #[test]
    fn p5_finding_classified_as_neither() {
        let v = serde_json::json!({
            "pattern": "P5PhantomDone",
            "severity": "High",
            "task_id": "3242",
            "summary": "phantom",
            "evidence": []
        });
        assert!(!is_p1_finding(&v), "P5PhantomDone must not satisfy is_p1_finding");
        assert!(!is_pdead_finding(&v), "P5PhantomDone must not satisfy is_pdead_finding");
    }

    /// A Value with no `pattern` field is classified as NEITHER.
    #[test]
    fn missing_pattern_field_classified_as_neither() {
        let v = serde_json::json!({
            "severity": "Low",
            "task_id": "t",
            "summary": "no pattern field"
        });
        assert!(!is_p1_finding(&v), "missing pattern must not satisfy is_p1_finding");
        assert!(!is_pdead_finding(&v), "missing pattern must not satisfy is_pdead_finding");
    }
}
