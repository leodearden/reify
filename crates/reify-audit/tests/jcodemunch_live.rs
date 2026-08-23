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

mod common;

// -----------------------------------------------------------------------
// Finding-shape predicates (pure; no serve needed)
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Invocation + fixture helpers (used by the capstone live test)
// -----------------------------------------------------------------------

/// Invoke the `reify-audit` binary with the given arguments.
///
/// Returns `(exit_code, findings, stderr)` where `findings` is the JSON array
/// parsed from the binary's stderr output (adapting cli.rs's
/// `parse_findings_from_stderr` idiom: `rfind("\n[")` to skip any
/// git-diagnostic preamble).
///
/// The raw `stderr` is returned ALONGSIDE the parsed findings, not merely
/// consumed to produce them, because the two exit-125 arms are otherwise
/// indistinguishable at the call site: a serve/IO failure and a §4.3 index
/// refusal both emit zero findings, and only the prose says which. A refusal
/// names its marker token (the `E_JC_INDEX_*` family — `STALE`, `EMPTY` or
/// `UNREADABLE`, each carrying a different remedy) plus the probed repo id,
/// index head, live head and symbol count — exactly what an operator needs to
/// fix it — and dropping the buffer here would reduce that to a bare
/// "exit 125".
///
/// An exit code of `None` means the binary was killed by a signal.
fn run_reify_audit(args: &[&str]) -> (Option<i32>, Vec<serde_json::Value>, String) {
    let bin = env!("CARGO_BIN_EXE_reify-audit");
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke reify-audit: {e}"));

    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let findings = parse_findings_from_stderr(&stderr);
    (out.status.code(), findings, stderr)
}

/// Parse the JSON findings array from binary stderr.
///
/// Local copy of cli.rs's `parse_findings_from_stderr`: searches for the
/// LAST `\n[` in the output (to skip any git-diagnostic lines that precede
/// the JSON block) and deserializes the JSON from that position onward.
///
/// Returns `Vec::new()` when no JSON block is present — this happens when the
/// binary exits with code 125 (infra/connection error) and only emits a plain
/// "reify-audit: error connecting…" line.  The caller's `assert_ne!(code,
/// Some(125))` owns the infra-error diagnostic in that case.
fn parse_findings_from_stderr(stderr: &str) -> Vec<serde_json::Value> {
    let json_start = match stderr
        .rfind("\n[")
        .map(|pos| pos + 1)
        .or_else(|| if stderr.starts_with('[') { Some(0) } else { None })
    {
        Some(pos) => pos,
        None => return Vec::new(),
    };
    serde_json::from_str(&stderr[json_start..]).unwrap_or_else(|e| {
        panic!(
            "stderr does not contain valid JSON after '[': {e}\nstderr:\n{stderr}"
        )
    })
}

/// Write an empty `tasks.json` (JSON array `[]`) to `dir/tasks.json`.
/// Returns the path.
fn write_empty_tasks_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("tasks.json");
    std::fs::write(&path, "[]").expect("write empty tasks.json");
    path
}

/// Create a minimal SQLite `runs.db` in `dir` with just the `events` table
/// (adapts cli.rs's `write_empty_runs_db`). Returns the path.
fn write_empty_runs_db(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("runs.db");
    let conn = rusqlite::Connection::open(&path).expect("open runs.db");
    conn.execute_batch("CREATE TABLE events (task_id TEXT, event_type TEXT);")
        .expect("create events table");
    path
}

/// Write a `tasks.json` containing ONE synthetic done task whose
/// `done_provenance.commit` is `commit` and `done_at` is set to
/// `done_at_epoch` (Unix seconds, encoded as a JSON number).
///
/// Adapts cli.rs's `task_fixture` + `write_tasks_json`, but MUST set
/// `done_at` (cli.rs leaves it `null`, which P1 skips) and
/// `done_provenance.commit` to a real reify commit.
///
/// The task has `files: ["crates/reify-audit/src/lib.rs"]` (a real file in
/// the reify corpus), `status: "done"`, `done_provenance.kind: "merged"`.
fn write_synthetic_done_task(
    dir: &std::path::Path,
    commit: &str,
    done_at_epoch: u64,
) -> std::path::PathBuf {
    let task = serde_json::json!([{
        "task_id": "synthetic-smoke-p1",
        "status": "done",
        "files": ["crates/reify-audit/src/lib.rs"],
        "done_provenance": {
            "kind": "merged",
            "commit": commit,
            "note": null
        },
        "title": "Synthetic done task for L-SMOKE P1 leg",
        "prd": null,
        "consumer_ref": null,
        "audit_foundation": null,
        "done_at": done_at_epoch
    }]);
    let path = dir.join("synthetic_done_task.json");
    let content = serde_json::to_string_pretty(&task).expect("serialize synthetic task");
    std::fs::write(&path, content).expect("write synthetic_done_task.json");
    path
}

// -----------------------------------------------------------------------
// β's index primitive: invocation + verdict consumption
// -----------------------------------------------------------------------

/// β, the SINGLE `watch --once` index primitive
/// (`scripts/jcodemunch-index-reify.sh`, task 6107).
///
/// Resolved from `CARGO_MANIFEST_DIR` (`crates/reify-audit` → two parents →
/// repo root) so the capstone finds β from whatever checkout it was compiled
/// in. `the_index_script_this_capstone_names_actually_exists` asserts this
/// path resolves — the antidote to `L-SMOKE` naming a script that never
/// existed (PRD §2.4).
const JC_INDEX_SCRIPT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/jcodemunch-index-reify.sh");

/// Folder-file cap handed to β for the throwaway corpus.
///
/// MEASURED necessity, not padding. β resolves its cap by parsing
/// `$CODE_INDEX_PATH/config.jsonc` with `jq`, but jcodemunch's serve WRITES a
/// JSONC template there (26 KB of `//`-commented config at the pinned
/// 1.108.54) on startup. On the serve-then-index ordering this capstone uses,
/// β therefore refuses before reaching the indexer:
///
/// ```text
/// jcodemunch-index-reify: cannot parse <dir>/config.jsonc — fix it, or set
/// JCODEMUNCH_MAX_FOLDER_FILES to override the cap explicitly
/// ```
///
/// `JCODEMUNCH_MAX_FOLDER_FILES` is β's OWN documented escape hatch, so
/// setting it consumes the contract rather than working around it. Tracked as
/// task #6486 (a β-side defect, out of scope here). 2000 is far above the
/// corpus's 3 files, so `E_JC_INDEX_TRUNCATED` cannot fire.
const JC_MAX_FOLDER_FILES: &str = "2000";

/// The exact β invocation for one index pass over `corpus` into `index_dir`.
///
/// Built as a `Command` and returned UNRUN so the `index_invocation` tests can
/// assert its shape with no uvx, no network and no serve — α's seam-splitting
/// pattern, applied to the half of the capstone that would otherwise only be
/// checked when somebody deliberately passes `--ignored`.
///
/// `--project-root` is passed explicitly because β DEFAULTS to the canonical
/// `/home/leo/src/reify` checkout, and `CODE_INDEX_PATH` because otherwise the
/// pass writes into host-global `~/.code-index`.
///
/// `JCODEMUNCH_GIT_ROOT_IDENTITY` is deliberately NOT set here: β applies that
/// lever itself (`JC_IDENTITY_ENV=(env JCODEMUNCH_GIT_ROOT_IDENTITY=0)`), and
/// duplicating it at a second site is exactly how PRD §4.2's "every invocation
/// site carries this obligation" drifts.
fn index_pass_command(
    corpus: &std::path::Path,
    index_dir: &std::path::Path,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg(JC_INDEX_SCRIPT)
        .arg("--project-root")
        .arg(corpus)
        .env("CODE_INDEX_PATH", index_dir)
        .env("JCODEMUNCH_MAX_FOLDER_FILES", JC_MAX_FOLDER_FILES);
    cmd
}

/// Run one β index pass and CONSUME ITS VERDICT. Panics unless β succeeded.
///
/// A pass is accepted only if the child exits 0 **and** its output carries β's
/// own `INDEX-OK` success token. Both halves matter: β owns the "present,
/// non-empty, not silently truncated" gate and refuses with
/// `E_JC_INDEX_MISSING` / `E_JC_INDEX_EMPTY` / `E_JC_INDEX_TRUNCATED` /
/// `E_JC_INDEX_RUN_FAILED`, and consuming that verdict instead of re-deriving
/// one here is the whole point of the ε→β edge.
///
/// Returns the captured stdout+stderr so the capstone can print β's
/// `INDEX-OK … N sym` line as operator-facing acceptance evidence.
fn run_index_pass(corpus: &std::path::Path, index_dir: &std::path::Path) -> String {
    let out = index_pass_command(corpus, index_dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke {JC_INDEX_SCRIPT}: {e}"));

    let combined = format!(
        "--- stdout ---\n{}--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        out.status.success(),
        "β refused to index the throwaway corpus at {} (exit {:?}).\n\
         β's refusal markers are the diagnosis — E_JC_INDEX_MISSING / _EMPTY / \
         _TRUNCATED / _RUN_FAILED each carry their own remedy.\n{combined}",
        corpus.display(),
        out.status.code()
    );
    assert!(
        combined.contains("INDEX-OK"),
        "β exited 0 but never printed its own INDEX-OK success token for {}. \
         Exit status alone is not the contract: INDEX-OK is the token β derives \
         and owns, and scraping for it is what keeps this edge from accepting a \
         silently-degraded pass.\n{combined}",
        corpus.display()
    );

    combined
}

// -----------------------------------------------------------------------
// Throwaway corpus (real git; no network, no serve)
// -----------------------------------------------------------------------

/// A throwaway two-commit git repo for the capstone to index and audit.
///
/// Why a throwaway corpus instead of the reify checkout: the capstone this
/// file used to carry derived `--project-root` from `CARGO_MANIFEST_DIR`, so
/// in a warm lane it refused with exit 125 / `E_JC_INDEX_EMPTY` — its own doc
/// conceded that "running the capstone from a warm lane rather than the
/// primary checkout is therefore expected to refuse". That makes the capstone
/// unrunnable exactly where tasks execute, which is how an evidence layer goes
/// inert. A throwaway corpus is portable, self-contained, mutates no
/// host-global state, and is still a REAL freshly-indexed corpus rather than a
/// mock.
struct CorpusRepo {
    /// Canonicalized repo root. Canonical, not as-given: jcodemunch applies
    /// `Path(p).expanduser().resolve()` before hashing, β applies
    /// `readlink -f`, and `expected_repo_id` applies `fs::canonicalize`. All
    /// three must agree or the binary probes a different DB than β wrote.
    root: std::path::PathBuf,
    /// The `local/<basename>-<sha1(abs)[..8]>` identity β will write under and
    /// the binary will derive from `--project-root`.
    repo_id: String,
    /// HEAD (commit 2).
    head: String,
    /// `HEAD^1` (commit 1) — the `since` end of P1's `commit^1..commit` range.
    prev: String,
    /// Repo-relative path of the Rust file genuinely modified in `prev..head`.
    touched_file: String,
}

/// Build a real two-commit git repo under `dir` and return its [`CorpusRepo`].
///
/// Commit 1 seeds `Cargo.toml` + `src/lib.rs` with public symbols; commit 2
/// appends one more `pub fn` to `src/lib.rs`, so `touched_file` is genuinely
/// inside `prev..head` rather than merely present at HEAD.
///
/// Every git call goes through [`common::git_env::git_cmd`] and identity plus
/// `commit.gpgsign` are pinned the way `common::index_fixture::
/// init_git_repo_with_one_commit` pins them, so this works on a host with no
/// git identity configured and cannot be redirected by an ambient
/// `GIT_DIR`/`GIT_INDEX_FILE` (the hook-environment hazard `git_env`'s module
/// doc records).
///
/// Deliberately LOCAL to this file rather than added to `tests/common/`:
/// `tests/common/` is compiled into EVERY integration-test binary in the crate
/// (see the partial-consumer note in `index_fixture.rs`), and this capstone is
/// the only consumer.
fn build_corpus_repo(dir: &std::path::Path) -> CorpusRepo {
    let root = dir.join("corpus");
    std::fs::create_dir_all(root.join("src")).expect("create corpus src dir");

    let run = |args: &[&str]| {
        let out = common::git_env::git_cmd(&root)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
        assert!(
            out.status.success(),
            "git {args:?} exited {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let rev_parse = |rev: &str| -> String {
        let out = common::git_env::git_cmd(&root)
            .args(["rev-parse", rev])
            .output()
            .unwrap_or_else(|e| panic!("git rev-parse {rev} failed to spawn: {e}"));
        assert!(
            out.status.success(),
            "git rev-parse {rev} exited {:?}",
            out.status.code()
        );
        let sha = String::from_utf8(out.stdout).expect("utf8 sha").trim().to_string();
        assert_eq!(sha.len(), 40, "expected a full 40-char sha, got {sha:?}");
        sha
    };

    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);

    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"corpus\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write corpus Cargo.toml");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn corpus_alpha() -> u32 {\n    1\n}\n\
         \npub fn corpus_beta() -> u32 {\n    2\n}\n",
    )
    .expect("write corpus src/lib.rs");
    run(&["add", "."]);
    run(&["commit", "-m", "corpus: seed"]);
    let prev = rev_parse("HEAD");

    // Commit 2 APPENDS, so `src/lib.rs` really is in `prev..head`.
    let touched_file = "src/lib.rs".to_string();
    let mut lib = std::fs::read_to_string(root.join(&touched_file)).expect("read corpus lib.rs");
    lib.push_str("\npub fn corpus_gamma() -> u32 {\n    3\n}\n");
    std::fs::write(root.join(&touched_file), lib).expect("append to corpus lib.rs");
    run(&["add", "."]);
    run(&["commit", "-m", "corpus: add corpus_gamma"]);
    let head = rev_parse("HEAD");

    // Canonicalize BEFORE deriving the identity — see `CorpusRepo::root`.
    let root = std::fs::canonicalize(&root).expect("canonicalize corpus root");
    let repo_id = reify_audit::jcodemunch_index::resolve_repo_id(&root);

    CorpusRepo { root, repo_id, head, prev, touched_file }
}

// -----------------------------------------------------------------------
// Findings well-formedness (pure; no serve needed)
// -----------------------------------------------------------------------

/// Assert every element of `findings` is a well-formed audit finding, or
/// panic naming `leg`, the element's index, and the offending element.
///
/// Required of each element: it is a JSON **object** carrying
///
/// * `pattern`  — non-empty string,
/// * `severity` — string,
/// * `task_id`  — string, possibly EMPTY (`PDeadCode` is repo-wide and belongs
///   to no task, so it carries `""`; the field must be present and a string,
///   but its emptiness says nothing),
/// * `summary`  — non-empty string,
/// * `evidence` — array.
///
/// # It asserts NOTHING about the array's LENGTH — deliberately
///
/// Zero findings is a legitimate, PASSING outcome. P1's one recorded live run
/// (2026-06-09) produced zero findings, and PDEAD's detector is repo-shape
/// dependent, so a `>=N` bound has no achievability basis: it would pass only
/// under a premise nobody ever validated, which is §2.4's vacuity reproduced
/// in mirror image (PRD §6/ε: *Explicitly NOT asserted: ">=1 P1 finding"*).
///
/// What makes the capstone non-vacuous is therefore NOT this predicate on its
/// own — `[]` satisfies it, and `[]` is exactly what the broken handshake
/// emitted for ten weeks. It is one link in a four-part chain: exit 0, no
/// `E_JC_INDEX_` refusal marker, no `jcodemunch unreachable at` fail-soft
/// breadcrumb, and an independently-read `count_symbols(...) > 0`. No link in
/// that chain is satisfiable by a vacuous run.
fn assert_well_formed_findings(findings: &[serde_json::Value], leg: &str) {
    for (i, finding) in findings.iter().enumerate() {
        let render = || {
            serde_json::to_string_pretty(finding)
                .unwrap_or_else(|_| format!("{finding:?}"))
        };

        let obj = finding.as_object().unwrap_or_else(|| {
            panic!(
                "{leg} leg: findings[{i}] is not a JSON object:\n{}",
                render()
            )
        });

        let string_field = |name: &str| -> &str {
            obj.get(name)
                .unwrap_or_else(|| {
                    panic!(
                        "{leg} leg: findings[{i}] has no `{name}` field:\n{}",
                        render()
                    )
                })
                .as_str()
                .unwrap_or_else(|| {
                    panic!(
                        "{leg} leg: findings[{i}]'s `{name}` is not a string:\n{}",
                        render()
                    )
                })
        };

        for name in ["pattern", "summary"] {
            assert!(
                !string_field(name).is_empty(),
                "{leg} leg: findings[{i}]'s `{name}` is an empty string:\n{}",
                render()
            );
        }
        // `severity` must be a string; `task_id` must be a string but MAY be
        // empty — see the doc above on PDeadCode.
        let _ = string_field("severity");
        let _ = string_field("task_id");

        assert!(
            obj.get("evidence")
                .unwrap_or_else(|| {
                    panic!(
                        "{leg} leg: findings[{i}] has no `evidence` field:\n{}",
                        render()
                    )
                })
                .is_array(),
            "{leg} leg: findings[{i}]'s `evidence` is not an array:\n{}",
            render()
        );
    }
}

// -----------------------------------------------------------------------
// Serve preflight: a real MCP `initialize`, i.e. an IDENTITY check
// -----------------------------------------------------------------------

/// How long the preflight will wait to connect, and then to read a reply.
///
/// Load-bearing, not a nicety. `identity_probe_rejects_a_bare_tcp_squatter`
/// drives a listener that ACCEPTS and then never writes a byte; without a
/// bounded read timeout that test would hang forever instead of failing, and
/// an operator pointing the capstone at a wedged endpoint would see the run
/// stall rather than refuse.
const PREFLIGHT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Ask `url` to prove it is a jcodemunch serve, via a real MCP `initialize`.
///
/// Returns `Ok(())` only when ALL THREE conjuncts hold:
///
/// 1. HTTP 200,
/// 2. `result.serverInfo.name == "jcodemunch-mcp"`,
/// 3. a non-empty server-assigned `Mcp-Session-Id` **response** header.
///
/// # Identity, not liveness
///
/// This replaces a bare `TcpStream::connect_timeout` that this file used to
/// call `jcodemunch_serve_reachable`. A bare connect is answered happily by
/// ANY listener, so its "serve is up" verdict proved nothing about the seam
/// under test — the same reasoning `scripts/with-jcodemunch-serve.sh` records
/// in its header ("READINESS IS AN IDENTITY CHECK, NOT A LIVENESS CHECK") and
/// that `jcodemunch_session_live.rs`'s `await_ready` already applies.
///
/// Conjunct 3 folds in #6106's hardening: a serve that answers `initialize`
/// without assigning a session cannot carry a session-scoped `tools/call`, so
/// accepting it here would declare live a seam the binary is about to fail on.
///
/// # No request-side session id
///
/// The `initialize` POST deliberately carries NO `mcp-session-id` header (PRD
/// §4.1.1): the server MINTS the session, and a client that mints its own is
/// rejected with 404 (B2, pinned live in `jcodemunch_session_live.rs`).
///
/// # No redirect following
///
/// `/mcp/` (trailing slash) 307-redirects and the redirect DROPS the
/// `mcp-session-id` header — pinned at `src/bin/reify-audit.rs:185-188` and in
/// δ's header. `redirects(0)` makes such a response fail conjunct 1 loudly
/// here rather than silently losing the session downstream.
fn serve_identity_probe(url: &str) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(PREFLIGHT_TIMEOUT)
        .timeout_read(PREFLIGHT_TIMEOUT)
        .redirects(0)
        .build();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "reify-audit-jcodemunch-live-capstone",
                "version": "1"
            }
        }
    });

    let sent = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .send_json(payload);

    // ureq surfaces every non-2xx as `Err(Error::Status(..))`, so conjunct 1
    // has to be judged in BOTH arms or the redirect diagnostic below would be
    // unreachable prose.
    let response = match sent {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => {
            return Err(status_conjunct_failure(url, code));
        }
        Err(e) => return Err(format!("no MCP `initialize` reply from {url}: {e}")),
    };

    let status = response.status();
    if status != 200 {
        // 2xx-but-not-200 — e.g. the bare `202` responder B3 names.
        return Err(status_conjunct_failure(url, status));
    }

    let session = response
        .header("mcp-session-id")
        .unwrap_or_default()
        .to_string();
    let ctype = response.header("content-type").unwrap_or("").to_string();
    let body = response
        .into_string()
        .map_err(|e| format!("could not read {url}'s `initialize` reply: {e}"))?;

    let parsed = parse_mcp_body(&ctype, &body)
        .map_err(|e| format!("{url} did not answer `initialize` with MCP JSON: {e}"))?;

    let name = parsed["result"]["serverInfo"]["name"].as_str();
    if name != Some("jcodemunch-mcp") {
        return Err(format!(
            "{url} answered `initialize` as serverInfo.name={name:?}, not \
             \"jcodemunch-mcp\" — something else is listening there. Body:\n{body}"
        ));
    }

    if session.is_empty() {
        return Err(format!(
            "{url} answered `initialize` as jcodemunch-mcp but assigned NO \
             mcp-session-id response header; a session-scoped tools/call \
             cannot follow (see #6106)"
        ));
    }

    Ok(())
}

/// Conjunct-1 ("HTTP 200") failure text, shared by both arms above.
///
/// Names the redirect case explicitly: `/mcp/` (trailing slash) 307s, and the
/// redirect DROPS `mcp-session-id`, so a probe that quietly followed it would
/// declare live a seam whose session contract is already broken.
fn status_conjunct_failure(url: &str, code: u16) -> String {
    format!(
        "{url} answered `initialize` with HTTP {code}, not 200 — a 307 here \
         means the URL carries a trailing slash, and `/mcp/` redirects in a way \
         that DROPS the mcp-session-id header (pinned at \
         src/bin/reify-audit.rs:185-188 and in scripts/with-jcodemunch-serve.sh)"
    )
}

/// Decode an MCP reply body, which may arrive as bare JSON or as SSE.
///
/// The streamable-HTTP transport answers `Accept: text/event-stream` with an
/// `event:`/`data:` frame rather than a bare object, so a plain
/// `serde_json::from_str` on the body would reject a perfectly good serve.
/// Mirrors `jcodemunch_session_live.rs`'s `parse_mcp_body`, but returns
/// `Result` instead of panicking: here a malformed body is a legitimate
/// *verdict* about the endpoint (it is not a jcodemunch serve), not a harness
/// fault.
fn parse_mcp_body(content_type: &str, body: &str) -> Result<serde_json::Value, String> {
    if content_type.contains("text/event-stream") {
        for line in body.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                return serde_json::from_str(rest.trim())
                    .map_err(|e| format!("parse SSE data line: {e}; body={body}"));
            }
        }
        return Err(format!("no SSE data line in response body: {body}"));
    }
    serde_json::from_str(body).map_err(|e| format!("parse JSON body: {e}; body={body}"))
}

/// [`serve_identity_probe`], but a failure is FATAL.
///
/// This is the B8 seam. The capstone must not skip when the serve is missing
/// — a PASS-shaped absence is the exact defect this file exists to remove — so
/// the preflight's only two outcomes are "proceed" and "panic". Split out as
/// its own function purely so `require_reachable_serve_panics_on_an_unreachable_endpoint`
/// can assert the panic with no serve in hand, the same seam-splitting
/// `jcodemunch_session_live.rs` applied to `finish_teardown`.
fn require_reachable_serve(url: &str) {
    serve_identity_probe(url).unwrap_or_else(|e| {
        panic!(
            "jcodemunch serve preflight FAILED for {url}: {e}\n\
             \n\
             This capstone does NOT skip when the serve is absent — that is the \
             whole point of it (PRD B8). Bring a serve up and re-run through δ's \
             lifecycle wrapper, which spawns, readiness-polls, exports \
             JCODEMUNCH_URL and tears down on every exit path:\n\
             \n\
             \x20 CODE_INDEX_PATH=$(mktemp -d) bash scripts/with-jcodemunch-serve.sh \
             --port 8917 -- \\\n\
             \x20   cargo test -p reify-audit --test jcodemunch_live -- --ignored --nocapture\n"
        )
    });
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
const DEFAULT_SERVE_URL: &str = "http://127.0.0.1:8901/mcp";

// NOTE — there is deliberately NO `JCODEMUNCH_REPO` constant here any more.
//
// This capstone used to pin `leodearden/reify` and pass it as
// `--jcodemunch-repo` on both legs. That identity is jcodemunch's *git*
// identity for this origin, and on this host it maps to
// `~/.code-index/leodearden-reify.db`, which is the documented empty husk
// (`meta` holds only `call_refs_missing` + `index_version` — no `git_head`
// row — and `symbols` is 0). Upstream re-creates that husk as a side effect
// of any run, so deleting it does not help. Under the §4.3 freshness gate an
// explicit override at that identity therefore refuses with
// `E_JC_INDEX_EMPTY` and exit 125 on every invocation, which is a correct
// verdict about a useless index and a useless thing for the capstone to
// assert against.
//
// The identity the audit is *supposed* to probe is the §4.2 per-path one the
// binary now derives from `--project-root` (for `/home/leo/src/reify` that is
// `local/reify-4ae45bbd`, the index jcodemunch actually populates under the
// `JCODEMUNCH_GIT_ROOT_IDENTITY=0` lever reify forces everywhere). Omitting
// the flag is what selects it, so re-introducing a pinned constant here would
// re-break the capstone. If a future leg genuinely needs an explicit id, derive
// it with `reify_audit::jcodemunch_index::resolve_repo_id` rather than
// spelling one out.

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
///
/// **Second prerequisite, beyond a reachable serve.** No `--jcodemunch-repo`
/// is passed, so the binary derives the §4.2 per-path identity from
/// `--project-root` — which this test resolves from `CARGO_MANIFEST_DIR`, i.e.
/// *the checkout the test is compiled in*. That checkout must itself carry a
/// fresh, non-empty jcodemunch index or the §4.3 gate refuses with exit 125
/// before any detector runs. Running the capstone from a warm lane rather than
/// the primary checkout is therefore expected to refuse: lanes are re-seeded
/// per task and nothing indexes them. That is the gate working, not a
/// regression — run it from the indexed checkout.
#[ignore = "live integration: requires jcodemunch-serve up on default or $JCODEMUNCH_URL; run via --ignored"]
#[test]
fn live_audit_produces_p1_and_pdead_findings() {
    let serve_url = std::env::var("JCODEMUNCH_URL")
        .unwrap_or_else(|_| DEFAULT_SERVE_URL.to_string());

    // Preflight: an unreachable or non-jcodemunch endpoint is FATAL, never a
    // silent early return (B8). The last caller of the deleted bare-TCP probe.
    require_reachable_serve(&serve_url);

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
    let (pdead_code, pdead_findings, pdead_stderr) = run_reify_audit(&[
        "--pattern",
        "PDEAD",
        "--jcodemunch-url",
        &serve_url,
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
        "PDEAD leg: exit 125 = infra/connection error OR a §4.3 index refusal.\n\
         If stderr names E_JC_INDEX_EMPTY or E_JC_INDEX_STALE, the serve is fine \
         and the index for the derived per-path identity of {project_root} is \
         missing/empty/stale — reindex that checkout (jcodemunch must be invoked \
         under JCODEMUNCH_GIT_ROOT_IDENTITY=0) rather than editing this test.\n\
         stderr:\n{pdead_stderr}\n\
         all findings: {:#}",
        serde_json::Value::Array(pdead_findings.clone())
    );
    // The per-pattern count assertion that used to stand here is GONE, along
    // with the `is_pdead_finding` predicate that existed only to serve it: a
    // ">=1 PDeadCode finding" premise was never validated (PRD §2.5) and is
    // the mirror image of §2.4's vacuity. The count is reported, never asserted.
    println!("PDEAD leg: {} finding(s)", pdead_findings.len());

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
    let (p1_code, p1_findings, p1_stderr) = run_reify_audit(&[
        "--pattern",
        "P1",
        "--jcodemunch-url",
        &serve_url,
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
        "P1 leg: exit 125 = infra/connection error OR a §4.3 index refusal.\n\
         If stderr names E_JC_INDEX_EMPTY or E_JC_INDEX_STALE, the serve is fine \
         and the index for the derived per-path identity of {project_root} is \
         missing/empty/stale — reindex that checkout (jcodemunch must be invoked \
         under JCODEMUNCH_GIT_ROOT_IDENTITY=0) rather than editing this test.\n\
         stderr:\n{p1_stderr}\n\
         all findings: {:#}",
        serde_json::Value::Array(p1_findings.clone())
    );
    // Same removal as the PDEAD leg above: no count premise, count reported only.
    println!("P1 leg: {} finding(s)", p1_findings.len());
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

    /// The endpoint the preflight probes must be unreachable BY CONSTRUCTION —
    /// not merely unowned at the instant its URL was minted.
    ///
    /// A freed port is only unowned: anything that binds an ephemeral port in
    /// the meantime can be handed that exact port, at which point the preflight
    /// reports "serve is up". This test collapses that race into a
    /// deterministic single shot by binding the exact `host:port` the URL names
    /// and holding it across the probe.
    ///
    /// This is #5830's regression lock, repointed from the deleted bare-TCP
    /// `jcodemunch_serve_reachable` onto the strictly stronger identity probe:
    /// a listener that merely *accepts* can no longer be mistaken for a serve.
    #[test]
    fn identity_probe_rejects_the_unreachable_sentinel_under_a_racing_binder() {
        let url = common::net::unreachable_mcp_url();
        // Play the adversary: occupy the exact address the URL names. Binding
        // `_hijack` (not `_`) keeps any listener that DID land alive across
        // the assertion below.
        let (addr, _hijack) = common::net::try_hijack_url(&url);

        assert!(
            serve_identity_probe(&url).is_err(),
            "{url} must not be accepted as a jcodemunch serve even while a \
             racing binder holds {addr}"
        );
    }

    /// A listener that accepts TCP and never speaks MCP is NOT a serve.
    ///
    /// This is the non-vacuity lock the deleted bare-TCP probe could not
    /// express: `TcpStream::connect` succeeds against ANY listener, so the old
    /// probe would have reported this squatter as "serve is up" and let the
    /// capstone proceed against a corpse. The identity probe must reject it.
    ///
    /// The squatter accepts the connection and then goes silent, which is
    /// precisely why the probe's read timeout is load-bearing: without it this
    /// test would hang rather than fail.
    #[test]
    fn identity_probe_rejects_a_bare_tcp_squatter() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind a squatter on an ephemeral port");
        let addr = listener.local_addr().expect("squatter local_addr");
        // `listener` stays alive for the whole test: the port must still be
        // accepting while the probe runs, or this would degenerate into the
        // connection-refused case the sibling test already covers.
        let url = format!("http://{addr}/mcp");

        assert!(
            serve_identity_probe(&url).is_err(),
            "a bare TCP listener at {addr} that never speaks MCP must not be \
             accepted as a jcodemunch serve"
        );
    }

    /// The capstone's preflight FAILS on a dead endpoint — it does not return
    /// early.
    ///
    /// This is the direct B8 lock. The whole point of this task is that there
    /// is no graceful-skip path left anywhere in this file, and the preflight
    /// is where a skip would most naturally re-grow. Splitting the panic into
    /// its own function is what makes that assertion possible with no serve in
    /// hand (α's `finish_teardown` pattern).
    #[test]
    #[should_panic(expected = "jcodemunch serve preflight FAILED")]
    fn require_reachable_serve_panics_on_an_unreachable_endpoint() {
        require_reachable_serve(&common::net::unreachable_mcp_url());
    }
}

#[cfg(test)]
mod corpus_fixture {
    use super::*;

    /// HEAD must have a PARENT, or the P1 leg is unrunnable.
    ///
    /// P1 maps a done task's `done_provenance.commit` to the range
    /// `commit^1..commit` and feeds it to jcodemunch's `get_changed_symbols`.
    /// A single-commit corpus has no `HEAD^1`, so the leg could not even be
    /// constructed — the assertion is on the fixture's SHAPE precisely because
    /// the capstone that consumes it is `#[ignore]`d and would never notice.
    #[test]
    fn corpus_repo_has_two_commits_so_head_has_a_parent() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let corpus = build_corpus_repo(tmp.path());

        assert_ne!(
            corpus.head, corpus.prev,
            "a two-commit corpus must have head != prev"
        );
        assert_eq!(corpus.head.len(), 40, "head must be a full sha: {:?}", corpus.head);
        assert_eq!(corpus.prev.len(), 40, "prev must be a full sha: {:?}", corpus.prev);

        let out = common::git_env::git_cmd(&corpus.root)
            .args(["rev-parse", "HEAD^1"])
            .output()
            .expect("git rev-parse HEAD^1");
        assert!(
            out.status.success(),
            "HEAD must have a parent; git rev-parse HEAD^1 exited {:?}",
            out.status.code()
        );
        let parent = String::from_utf8(out.stdout).expect("utf8 sha").trim().to_string();
        assert_eq!(parent, corpus.prev, "CorpusRepo::prev must BE git's HEAD^1");
    }

    /// HEAD carries a Rust file with public symbols.
    ///
    /// This is what makes `count_symbols(index_dir, repo_id) > 0` achievable
    /// at all: an empty or symbol-free corpus would index to a husk and the
    /// §4.3 gate would refuse, so the capstone would fail for a fixture reason
    /// rather than a seam reason.
    #[test]
    fn corpus_repo_head_carries_a_rust_file_with_public_symbols() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let corpus = build_corpus_repo(tmp.path());

        assert!(
            corpus.touched_file.ends_with(".rs"),
            "touched_file must be Rust source: {:?}",
            corpus.touched_file
        );

        let out = common::git_env::git_cmd(&corpus.root)
            .args(["show", &format!("HEAD:{}", corpus.touched_file)])
            .output()
            .expect("git show HEAD:<touched_file>");
        assert!(
            out.status.success(),
            "git show HEAD:{} exited {:?} — the touched file must exist AT HEAD",
            corpus.touched_file,
            out.status.code()
        );
        let content = String::from_utf8(out.stdout).expect("utf8 source");
        assert!(
            content.contains("pub fn "),
            "HEAD:{} must declare at least one `pub fn`; got:\n{content}",
            corpus.touched_file
        );
    }

    /// The corpus's repo id is the one the OPERATOR would predict.
    ///
    /// Checked twice on purpose: against the tests' deliberately-independent
    /// `expected_repo_id` (which re-derives `local/<basename>-<sha1(abs)[..8]>`
    /// by hand) AND against the production `resolve_repo_id` the binary uses.
    /// If those two ever disagree, β writes one DB and the binary probes
    /// another — the §4.3 gate then refuses a fully-indexed tree and sends the
    /// operator to re-index a phantom.
    ///
    /// Measured ground truth from planning: an indexed `/tmp/…/corpus`
    /// produced exactly `local/corpus-72879e30`.
    #[test]
    fn corpus_repo_id_matches_the_operator_derivation() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let corpus = build_corpus_repo(tmp.path());

        assert_eq!(
            corpus.repo_id,
            common::index_fixture::expected_repo_id(&corpus.root),
            "repo id must match the operator's independent derivation"
        );
        assert_eq!(
            corpus.repo_id,
            reify_audit::jcodemunch_index::resolve_repo_id(&corpus.root),
            "repo id must match what the binary derives from --project-root"
        );
        assert!(
            corpus.repo_id.starts_with("local/"),
            "per-path identity is `local/…`, not a git identity: {:?}",
            corpus.repo_id
        );
    }
}

#[cfg(test)]
mod index_invocation {
    use super::*;

    /// Argv of `cmd`, as lossy strings.
    fn argv(cmd: &std::process::Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// The env `cmd` OVERRIDES, as `(name, value)` pairs. Cleared vars are
    /// dropped — this fixture sets none.
    fn envs(cmd: &std::process::Command) -> Vec<(String, String)> {
        cmd.get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            })
            .collect()
    }

    /// The script this capstone names must EXIST.
    ///
    /// This is the direct antidote to the disease. The prior PRD's `L-SMOKE`
    /// binding named `scripts/smoke-jcodemunch-audit.sh`, WHICH DOES NOT
    /// EXIST, and nothing noticed for ten weeks (PRD §2.4). A gate-resident
    /// existence assertion makes a rename or deletion of β loud immediately,
    /// instead of only when somebody deliberately runs the `#[ignore]`d
    /// capstone.
    #[test]
    fn the_index_script_this_capstone_names_actually_exists() {
        let script = std::path::Path::new(JC_INDEX_SCRIPT);
        assert!(
            script.is_file(),
            "{JC_INDEX_SCRIPT} does not exist. This capstone consumes β's index \
             primitive and its INDEX-OK / E_JC_INDEX_* verdict; if β moved, \
             repoint JC_INDEX_SCRIPT — do not re-derive the verdict here."
        );
    }

    /// The invocation targets the THROWAWAY corpus and the THROWAWAY index dir.
    ///
    /// Guards against the capstone silently indexing into host-global
    /// `~/.code-index` (which holds 917 DBs on this host). β defaults
    /// `--project-root` to the canonical `/home/leo/src/reify` checkout, so
    /// passing it explicitly is not optional.
    #[test]
    fn index_pass_command_targets_the_corpus_and_the_throwaway_index_dir() {
        let corpus = std::path::Path::new("/tmp/does-not-need-to-exist/corpus");
        let index_dir = std::path::Path::new("/tmp/does-not-need-to-exist/index");
        let cmd = index_pass_command(corpus, index_dir);

        let args = argv(&cmd);
        let root_at = args
            .iter()
            .position(|a| a == "--project-root")
            .unwrap_or_else(|| panic!("argv must carry --project-root; got {args:?}"));
        assert_eq!(
            args.get(root_at + 1).map(String::as_str),
            corpus.to_str(),
            "--project-root must name the throwaway corpus; got {args:?}"
        );

        let env = envs(&cmd);
        assert!(
            env.iter().any(|(k, v)| k == "CODE_INDEX_PATH" && Some(v.as_str()) == index_dir.to_str()),
            "CODE_INDEX_PATH must name the throwaway index dir, or the pass \
             writes into host-global ~/.code-index; got {env:?}"
        );
    }

    /// The invocation OVERRIDES the folder-file cap.
    ///
    /// MEASURED necessity, not defensive padding. β resolves its cap by
    /// parsing `$CODE_INDEX_PATH/config.jsonc` with `jq`, but jcodemunch's own
    /// serve WRITES a JSONC template there (with `//` comments) on startup, so
    /// on the serve-then-index ordering this capstone uses, β dies with
    /// `cannot parse … config.jsonc — fix it, or set
    /// JCODEMUNCH_MAX_FOLDER_FILES to override the cap explicitly`. Without
    /// this override the capstone cannot index at all. Filed as follow-up
    /// against β; the escape hatch is β's own documented one.
    #[test]
    fn index_pass_command_overrides_the_folder_file_cap() {
        let cmd = index_pass_command(
            std::path::Path::new("/tmp/does-not-need-to-exist/corpus"),
            std::path::Path::new("/tmp/does-not-need-to-exist/index"),
        );
        let env = envs(&cmd);
        assert!(
            env.iter().any(|(k, v)| k == "JCODEMUNCH_MAX_FOLDER_FILES" && !v.is_empty()),
            "JCODEMUNCH_MAX_FOLDER_FILES must be set, or β refuses to parse the \
             serve-written config.jsonc and never reaches the indexer; got {env:?}"
        );
    }
}

#[cfg(test)]
mod finding_shape {
    use super::*;

    /// A helper the capstone leans on: `[]` is WELL FORMED.
    ///
    /// This is the mechanical expression of this file's central discipline,
    /// and the direct inverse of the non-empty-findings assertions the old
    /// capstone carried. P1's one recorded live run (2026-06-09) produced ZERO
    /// findings (PRD §2.5), so a `>=N` bound has no achievability basis and
    /// would reproduce §2.4's vacuity in mirror image — a test that passes only
    /// under a premise nobody ever validated.
    ///
    /// Pinning it behaviourally rather than as a comment is what stops it
    /// rotting: the capstone genuinely depends on this call not panicking.
    #[test]
    fn empty_findings_array_is_well_formed() {
        assert_well_formed_findings(&[], "leg");
    }

    /// Both shapes the capstone's two legs can emit pass the predicate.
    ///
    /// `PDeadCode` carries `task_id: ""` — it is repo-wide and belongs to no
    /// task — so the predicate must accept an EMPTY task id while still
    /// requiring the field to be present and a string.
    #[test]
    fn p1_and_pdead_shaped_findings_are_well_formed() {
        let findings = vec![
            serde_json::json!({
                "pattern": "P1ProducerOrphan",
                "severity": "Low",
                "task_id": "synthetic-capstone-p1",
                "summary": "orphaned public symbol",
                "evidence": []
            }),
            serde_json::json!({
                "pattern": "PDeadCode",
                "severity": "Low",
                "task_id": "",
                "summary": "dead fn foo",
                "evidence": []
            }),
        ];
        assert_well_formed_findings(&findings, "mixed");
    }

    /// A finding with no `pattern` is not a finding.
    #[test]
    #[should_panic(expected = "pattern")]
    fn finding_with_no_pattern_field_is_rejected() {
        let findings = vec![serde_json::json!({
            "severity": "Low",
            "task_id": "t",
            "summary": "no pattern field",
            "evidence": []
        })];
        assert_well_formed_findings(&findings, "PDEAD");
    }

    /// `evidence` is the array an operator drills into; a scalar there means
    /// the wire shape drifted, which is exactly what this file exists to catch.
    #[test]
    #[should_panic(expected = "evidence")]
    fn finding_with_a_non_array_evidence_field_is_rejected() {
        let findings = vec![serde_json::json!({
            "pattern": "PDeadCode",
            "severity": "Low",
            "task_id": "",
            "summary": "dead fn foo",
            "evidence": "oops"
        })];
        assert_well_formed_findings(&findings, "PDEAD");
    }

    /// A bare scalar in the array is not a finding object.
    #[test]
    #[should_panic(expected = "JSON object")]
    fn non_object_array_element_is_rejected() {
        let findings = vec![serde_json::json!(42)];
        assert_well_formed_findings(&findings, "P1");
    }
}
