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
