//! Minimal sync MCP HTTP client for loading [`TaskMetadata`] from
//! fused-memory.
//!
//! Used by the `reify-audit` binary's production loader path (T-4). The
//! JSON-file loader (`--tasks-file <path>`) remains for tests and one-off
//! offline runs; see `bin/reify-audit.rs`.
//!
//! ## Wire protocol
//!
//! MCP streamable-HTTP, protocol version `2024-11-05`. Three POSTs per
//! session: `initialize`, `notifications/initialized`, then one
//! `tools/call` per metadata fetch. Mirrors the reference Python client at
//! `/home/leo/src/dark-factory/scripts/migrate_metadata_modules_to_files.py`
//! (`FusedMemoryClient`).
//!
//! ## Why sync `ureq` and not `reqwest`+tokio
//!
//! Per `docs/architecture-audit/f-infra-design.md` §12 (minimal deps): the
//! `reify-audit` binary is a one-shot CLI run inside the pre-done hook hot
//! path; pulling tokio would more than triple our dep tree for no benefit.
//! `ureq` is ~150 LoC of additional deps and stays synchronous.
//!
//! ## Adapter
//!
//! Fused-memory's wire shape nests audit-relevant fields under `metadata`
//! (`metadata.files`, `metadata.done_provenance`, etc.) while
//! [`TaskMetadata`] keeps them flat. [`task_metadata_from_wire`] is the
//! single adapter that bridges the two — public so unit tests in this
//! module can exercise it without round-tripping HTTP.

use std::cell::Cell;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{DoneProvenance, TaskMetadata};

const PROTOCOL_VERSION: &str = "2024-11-05";
const CLIENT_NAME: &str = "reify-audit";
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Errors returned by [`FusedMemoryClient`]. All variants map to exit code
/// 125 (`ERROR_EXIT`) at the binary boundary so the pre-done hook's
/// refuse-on-non-zero contract is preserved on every infrastructure
/// failure (per `f-infra-design.md` §10 T-4).
#[derive(Debug)]
pub enum LoadError {
    /// Transport-level failure: connection refused, timeout, non-2xx
    /// status, body read failure.
    Http(String),
    /// Protocol-level failure: malformed JSON-RPC envelope, missing
    /// expected fields, server-returned `error` payload.
    Protocol(String),
    /// `get_task` returned a payload that does not look like a task
    /// (no `id` field). Treated as "task not found" by the CLI.
    NotFound(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Http(m) => write!(f, "MCP HTTP error: {m}"),
            LoadError::Protocol(m) => write!(f, "MCP protocol error: {m}"),
            LoadError::NotFound(id) => write!(f, "task not found: {id}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Sync MCP streamable-HTTP client. One instance == one MCP session.
///
/// Note: [`TaskMetadata::done_at`] populated by this loader is an
/// approximation derived from the wire `updatedAt` field, because
/// fused-memory does not currently expose a dedicated done-flip timestamp.
/// The JSON-file loader (`--tasks-file`) reads an explicit `done_at` field
/// stored at done-flip time, so for the same task the two loaders can
/// disagree by minutes-to-days depending on post-done edits. Time-window
/// audits (`--since`) may therefore include/exclude the same task
/// differently across loader paths.
pub struct FusedMemoryClient {
    url: String,
    session_id: String,
    agent: ureq::Agent,
    next_id: Cell<u64>,
}

impl FusedMemoryClient {
    /// Connect to `url` (e.g. `http://localhost:8002/mcp/`) and complete
    /// the MCP handshake (initialize + notifications/initialized).
    pub fn new(url: impl Into<String>) -> Result<Self, LoadError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let client = Self {
            url: url.into(),
            session_id: random_hex_32(),
            agent,
            next_id: Cell::new(1),
        };
        client.initialize()?;
        Ok(client)
    }

    fn initialize(&self) -> Result<(), LoadError> {
        // `post()` checks every JSON-RPC response for the `error` field, so
        // a server that 200-OKs `initialize` with a `{"error":{...}}` body
        // surfaces here instead of being silently accepted and masked as a
        // confusing `get_task` failure later.
        let _ = self.post(&json!({
            "jsonrpc": "2.0",
            "id": self.next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {},
            },
        }))?;
        let _ = self.post(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {},
        }))?;
        Ok(())
    }

    fn next_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    fn post(&self, payload: &Value) -> Result<Value, LoadError> {
        // Maps each `ureq::Error` shape (Status/Transport) to
        // `LoadError::Http(format!("POST {url}: {e}"))` so the
        // connection-refused / timeout / non-2xx breadcrumb is preserved
        // on the binary's stderr before it exits 125.
        let response = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream")
            .set("mcp-session-id", &self.session_id)
            .send_json(payload.clone())
            .map_err(|e| LoadError::Http(format!("POST {}: {e}", self.url)))?;

        if response.status() == 202 {
            return Ok(Value::Null);
        }

        let ctype = response
            .header("content-type")
            .unwrap_or("")
            .to_string();
        // Use into_reader() instead of into_string() to avoid ureq's
        // 10 MiB into_string cap — the live task corpus now exceeds
        // that limit. Reading to a String (not serde_json::from_reader)
        // keeps the SSE/JSON dual-path intact: the SSE branch must scan
        // the body text for the `data:` line, which from_reader cannot
        // do. For a one-shot CLI, buffering the corpus in memory is fine.
        let mut body = String::new();
        #[allow(unused_imports)]
        use std::io::Read as _;
        response
            .into_reader()
            .read_to_string(&mut body)
            .map_err(|e| LoadError::Http(format!("read body: {e}")))?;

        let value = if ctype.contains("text/event-stream") {
            let mut parsed: Option<Value> = None;
            for line in body.lines() {
                if let Some(rest) = line.strip_prefix("data:") {
                    parsed = Some(serde_json::from_str(rest.trim()).map_err(|e| {
                        LoadError::Protocol(format!(
                            "SSE data parse: {e}; body={body}"
                        ))
                    })?);
                    break;
                }
            }
            parsed.ok_or_else(|| {
                LoadError::Protocol(format!("no SSE data line in response: {body}"))
            })?
        } else if body.is_empty() {
            return Ok(Value::Null);
        } else {
            serde_json::from_str(&body).map_err(|e| {
                LoadError::Protocol(format!("body parse: {e}; body={body}"))
            })?
        };

        // Centralised JSON-RPC error-envelope check. Every response that
        // goes through `post()` (initialize, notifications/initialized,
        // tools/call) gets the same treatment, so a server that 200-OKs
        // any of them with `{"error":{...}}` fails loudly here instead of
        // being silently accepted and masked as a downstream error.
        if let Some(err) = value.get("error") {
            return Err(LoadError::Protocol(format!("JSON-RPC error: {err}")));
        }
        Ok(value)
    }

    fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, LoadError> {
        let resp = self
            .post(&json!({
                "jsonrpc": "2.0",
                "id": self.next_id(),
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }))
            .map_err(|e| match e {
                // Decorate Protocol errors from `post()` with the tool name so
                // the breadcrumb says e.g. "get_task: JSON-RPC error: …"
                // instead of a bare "JSON-RPC error: …".
                LoadError::Protocol(m) => LoadError::Protocol(format!("{name}: {m}")),
                other => other,
            })?;
        let result = resp.get("result").cloned().unwrap_or(Value::Null);
        if let Some(s) = result.get("structuredContent").cloned() {
            return Ok(s);
        }
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            for entry in content {
                if entry.get("type").and_then(|t| t.as_str()) == Some("text")
                    && let Some(text) = entry.get("text").and_then(|t| t.as_str())
                {
                    return serde_json::from_str(text).map_err(|e| {
                        LoadError::Protocol(format!("content text not JSON: {e}"))
                    });
                }
            }
        }
        Ok(result)
    }

    /// Fetch a single task by id. Hot path: pre-done hook.
    pub fn get_task(
        &self,
        task_id: &str,
        project_root: &str,
    ) -> Result<TaskMetadata, LoadError> {
        let v = self.call_tool(
            "get_task",
            json!({"id": task_id, "project_root": project_root}),
        )?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(LoadError::NotFound(format!("{task_id}: {err}")));
        }
        let task = if v.get("id").is_some() {
            v
        } else {
            v.get("task").cloned().unwrap_or(v)
        };
        task_metadata_from_wire(&task).ok_or_else(|| {
            LoadError::Protocol(format!("malformed task response for {task_id}: {task}"))
        })
    }

    /// Fetch all tasks (flattening subtasks). Sweep path: periodic `/audit`.
    pub fn get_tasks(
        &self,
        project_root: &str,
    ) -> Result<Vec<TaskMetadata>, LoadError> {
        let v = self.call_tool(
            "get_tasks",
            json!({"project_root": project_root, "with_subtasks": true}),
        )?;
        // Refuse to treat a missing/non-array `tasks` field as an
        // (empty) success — a server response like `{}` or an
        // unexpectedly-shaped success payload would otherwise let the
        // sweep silently return zero tasks and exit 0 (looking
        // healthy). Raising Protocol routes through the binary's
        // load-error arm and exits 125, matching the get_task
        // refuse-on-malformed contract.
        let tasks = v
            .get("tasks")
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                LoadError::Protocol(format!(
                    "get_tasks: missing or non-array `tasks` field in response: {v}"
                ))
            })?;
        let mut out = Vec::new();
        for t in tasks {
            collect_tasks_recursive(t, &mut out);
        }
        Ok(out)
    }
}

fn collect_tasks_recursive(node: &Value, out: &mut Vec<TaskMetadata>) {
    if let Some(tm) = task_metadata_from_wire(node) {
        out.push(tm);
    }
    if let Some(subs) = node.get("subtasks").and_then(|s| s.as_array()) {
        for sub in subs {
            collect_tasks_recursive(sub, out);
        }
    }
}

/// Adapter: fused-memory wire shape → [`TaskMetadata`].
///
/// Returns `None` only when `id` is missing or non-coercible — everything
/// else has a sensible default (`title=""`, `status=""`, `files=vec![]`,
/// all `Option` fields = `None`) so the detector inputs stay well-typed
/// even for sparse/legacy task payloads.
fn task_metadata_from_wire(v: &Value) -> Option<TaskMetadata> {
    let task_id = match v.get("id") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => return None,
    };
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let metadata = v.get("metadata").cloned().unwrap_or(Value::Null);

    let files = metadata
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let done_provenance = metadata
        .get("done_provenance")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value::<DoneProvenance>(v.clone()).ok());

    let prd = metadata
        .get("prd")
        .and_then(|p| p.as_str())
        .map(String::from);
    let consumer_ref = metadata
        .get("consumer_ref")
        .and_then(|p| p.as_str())
        .map(String::from);
    let audit_foundation = metadata.get("audit_foundation").and_then(|p| p.as_bool());

    // `done_at` approximation: fused-memory does not currently expose a
    // dedicated done-flip timestamp on the `get_task`/`get_tasks` payload,
    // so we fall back to `updatedAt` (last-edit time) for done tasks. This
    // means any post-done metadata edit (re-label, status round-trip, etc.)
    // will shift the recorded `done_at` and can subtly affect the P5
    // detector's time window. The JSON-file loader (`--tasks-file`) gets a
    // `done_at` field stored explicitly at done-flip time, so the two loader
    // paths can disagree for the same task. If fused-memory grows a real
    // `doneAt` field, prefer it here and fall back to `updatedAt` only when
    // it's absent.
    let done_at = if status == "done" {
        v.get("updatedAt")
            .and_then(|s| s.as_str())
            .and_then(parse_iso8601_to_epoch)
    } else {
        None
    };

    Some(TaskMetadata {
        task_id,
        status,
        files,
        done_provenance,
        title,
        prd,
        consumer_ref,
        audit_foundation,
        done_at,
    })
}

// -----------------------------------------------------------------------
// ISO-8601 → epoch-seconds parser
// -----------------------------------------------------------------------

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM]` (no other shapes) and
/// return epoch-seconds.
///
/// Hand-rolled (~50 lines) to avoid pulling `chrono` / `time` for one
/// parse-site (per `f-infra-design.md` §12 minimal-deps). The format is
/// pinned by fused-memory's `sqlite_task_backend.py` `updatedAt` writer;
/// anything outside it returns `None` so we fail loud rather than silently
/// mis-computing.
fn parse_iso8601_to_epoch(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once('T')?;
    let mut date_parts = date.split('-');
    let y: i64 = date_parts.next()?.parse().ok()?;
    let m: u32 = date_parts.next()?.parse().ok()?;
    let d: u32 = date_parts.next()?.parse().ok()?;

    let (time_str, tz_offset_seconds) = split_tz(rest)?;
    let time_main = time_str.split('.').next()?;
    let mut t = time_main.split(':');
    let hh: i64 = t.next()?.parse().ok()?;
    let mm: i64 = t.next()?.parse().ok()?;
    let ss: i64 = t.next()?.parse().ok()?;

    let days = days_from_civil(y, m, d);
    Some(days * 86400 + hh * 3600 + mm * 60 + ss - tz_offset_seconds)
}

/// Split the post-`T` segment into `(HH:MM:SS[.fff], tz_offset_seconds)`.
/// Accepts `Z`, `+HH:MM`, `-HH:MM`, or nothing (treated as UTC).
fn split_tz(rest: &str) -> Option<(&str, i64)> {
    if let Some(idx) = rest.find('Z') {
        return Some((&rest[..idx], 0));
    }
    // The time portion contains no '+' or '-' chars; the rightmost match
    // is therefore the tz sign.
    let idx_plus = rest.rfind('+');
    let idx_minus = rest.rfind('-');
    let (idx, sign): (usize, i64) = match (idx_plus, idx_minus) {
        (Some(p), Some(m)) => {
            if p > m {
                (p, 1)
            } else {
                (m, -1)
            }
        }
        (Some(p), None) => (p, 1),
        (None, Some(m)) => (m, -1),
        (None, None) => return Some((rest, 0)),
    };
    let tz = &rest[idx + 1..];
    let mut tz_parts = tz.split(':');
    let h: i64 = tz_parts.next()?.parse().ok()?;
    let mins: i64 = tz_parts.next().unwrap_or("0").parse().ok()?;
    Some((&rest[..idx], sign * (h * 3600 + mins * 60)))
}

/// Days from 1970-01-01 for a civil (proleptic Gregorian) date.
/// Hinnant's algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let m = m as i64;
    let d = d as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

// -----------------------------------------------------------------------
// Session ID
// -----------------------------------------------------------------------

/// 32-hex-char opaque session id. Reads `/dev/urandom`; falls back to a
/// time + pid LCG mix if that fails. MCP session ids are opaque tokens —
/// uniqueness, not cryptographic strength, is the only requirement.
fn random_hex_32() -> String {
    let mut buf = [0u8; 16];
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(&mut buf).is_ok()
        {
            return hex32(&buf);
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut x = now ^ pid.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for chunk in buf.chunks_mut(8) {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bytes = x.to_le_bytes();
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = bytes[i];
        }
    }
    hex32(&buf)
}

fn hex32(buf: &[u8; 16]) -> String {
    let mut out = String::with_capacity(32);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

// -----------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso8601_z_with_fractional() {
        // 2026-05-16T07:39:04Z is 20589 days * 86400 + 27544s = 1778917144
        let e = parse_iso8601_to_epoch("2026-05-16T07:39:04.350Z").unwrap();
        assert_eq!(e, 1778917144);
    }

    #[test]
    fn iso8601_plus_offset_equivalent_to_utc() {
        let e_plus = parse_iso8601_to_epoch("2026-05-16T08:39:04+01:00").unwrap();
        let e_utc = parse_iso8601_to_epoch("2026-05-16T07:39:04Z").unwrap();
        assert_eq!(e_plus, e_utc);
    }

    #[test]
    fn iso8601_minus_offset_equivalent_to_utc() {
        let e_minus = parse_iso8601_to_epoch("2026-05-16T03:39:04-04:00").unwrap();
        let e_utc = parse_iso8601_to_epoch("2026-05-16T07:39:04Z").unwrap();
        assert_eq!(e_minus, e_utc);
    }

    #[test]
    fn iso8601_no_tz_treated_as_utc() {
        let e_naive = parse_iso8601_to_epoch("2026-05-16T07:39:04").unwrap();
        let e_utc = parse_iso8601_to_epoch("2026-05-16T07:39:04Z").unwrap();
        assert_eq!(e_naive, e_utc);
    }

    #[test]
    fn iso8601_garbage_returns_none() {
        assert!(parse_iso8601_to_epoch("not-an-iso-date").is_none());
        assert!(parse_iso8601_to_epoch("2026-05-16").is_none());
        assert!(parse_iso8601_to_epoch("").is_none());
    }

    #[test]
    fn days_from_civil_known_epoch_boundary() {
        // 1970-01-01 → 0 days.
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        // 2024-01-01 → 19723 days (1704067200 / 86400).
        assert_eq!(days_from_civil(2024, 1, 1), 19723);
    }

    #[test]
    fn wire_adapter_happy_path_done_task() {
        let v = json!({
            "id": 3675,
            "title": "F-infra T-8",
            "status": "done",
            "updatedAt": "2026-05-16T07:39:04Z",
            "metadata": {
                "files": ["a.rs", "b.rs"],
                "done_provenance": {"kind": "merged", "commit": "abc", "note": null},
                "prd": "docs/foo.md",
                "consumer_ref": "docs/bar.md",
                "audit_foundation": false
            }
        });
        let tm = task_metadata_from_wire(&v).expect("happy-path wire decode");
        assert_eq!(tm.task_id, "3675");
        assert_eq!(tm.title, "F-infra T-8");
        assert_eq!(tm.status, "done");
        assert_eq!(tm.files, vec!["a.rs".to_string(), "b.rs".to_string()]);
        let dp = tm.done_provenance.as_ref().expect("done_provenance present");
        assert_eq!(dp.kind.as_deref(), Some("merged"));
        assert_eq!(dp.commit.as_deref(), Some("abc"));
        assert_eq!(tm.prd.as_deref(), Some("docs/foo.md"));
        assert_eq!(tm.consumer_ref.as_deref(), Some("docs/bar.md"));
        assert_eq!(tm.audit_foundation, Some(false));
        assert_eq!(tm.done_at, Some(1778917144));
    }

    #[test]
    fn wire_adapter_pending_task_has_no_done_at() {
        let v = json!({
            "id": "3676",
            "title": "x",
            "status": "pending",
            "updatedAt": "2026-05-16T07:39:04Z",
            "metadata": {}
        });
        let tm = task_metadata_from_wire(&v).expect("pending decode");
        assert!(tm.done_at.is_none());
        assert!(tm.files.is_empty());
        assert!(tm.done_provenance.is_none());
    }

    #[test]
    fn wire_adapter_missing_id_returns_none() {
        let v = json!({"title": "no id", "status": "pending"});
        assert!(task_metadata_from_wire(&v).is_none());
    }

    #[test]
    fn wire_adapter_string_id_preserved() {
        let v = json!({"id": "3676.1", "status": "pending"});
        let tm = task_metadata_from_wire(&v).expect("string id decode");
        assert_eq!(tm.task_id, "3676.1");
    }
}
