//! The non-vacuous live capstone: the real `reify-audit` binary completes a
//! REAL MCP session against a REAL freshly-indexed corpus (PRD
//! `docs/prds/jcodemunch-substrate-restoration.md`, task ε; B6 + B8).
//!
//! ## The defect this file exists to close
//!
//! For ten weeks the jcodemunch seam was broken — `JcodemunchClient` could not
//! complete an MCP handshake (§2.3) — and nothing noticed, because the
//! capstone that lived here was PASS-SHAPED WHETHER OR NOT THE CHAIN WORKED
//! (§2.4, the PRD's central claim: 2.1–2.3 are symptoms, 2.4 is the disease).
//! It failed in three compounding ways, all now removed:
//!
//! * it graceful-skipped on a bare `TcpStream::connect` probe — a verdict any
//!   port squatter satisfies;
//! * it asserted `>=1 PDeadCode` and `>=1 P1ProducerOrphan`, a count premise
//!   nobody had ever validated; and
//! * it derived `--project-root` from `CARGO_MANIFEST_DIR`, so in a warm lane
//!   it refused with exit 125 — unrunnable exactly where tasks execute.
//!
//! ## What the capstone ASSERTS
//!
//! * The endpoint PROVES it is jcodemunch: a real `initialize` returning HTTP
//!   200, `serverInfo.name == "jcodemunch-mcp"`, and a server-assigned
//!   `Mcp-Session-Id`. Identity, not liveness.
//! * β (`scripts/jcodemunch-index-reify.sh`) indexed a throwaway two-commit
//!   corpus and printed its own `INDEX-OK` token.
//! * `count_symbols(index_dir, repo_id) > 0`, read INDEPENDENTLY against the
//!   index the binary is about to be pointed at — this is what realizes B6's
//!   "over a non-empty symbol set" in a way an empty findings array cannot
//!   satisfy.
//! * Per leg (PDEAD, P1): exit 0; no `E_JC_INDEX_` marker in stderr, i.e. the
//!   §4.3 freshness gate ADMITTED the run rather than refusing it; no
//!   `jcodemunch unreachable at` breadcrumb, i.e. the client completed a real
//!   session instead of degrading to `NoopJCodemunchOps`; and a well-formed
//!   findings array.
//!
//! The breadcrumb assertion is the load-bearing one. The degraded path still
//! exits 0 and still serializes a perfectly well-formed EMPTY array — that is
//! precisely how §2.3 survived. No link in the chain
//! (exit 0 / no refusal marker / no fail-soft / symbols > 0) is satisfiable by
//! a vacuous run.
//!
//! ## What the capstone does NOT assert
//!
//! **Any finding count, on either leg.** P1's one recorded live run
//! (2026-06-09) produced ZERO findings (§2.5), so a `>=N` bound has no
//! achievability basis: it would pass only under an unvalidated premise, which
//! is §2.4's vacuity reproduced in mirror image. Count 0 is a legitimate,
//! PASSING outcome — pinned behaviourally by
//! `finding_shape::empty_findings_array_is_well_formed`, which the capstone
//! genuinely depends on, so it cannot rot into a comment. Counts are PRINTED
//! as operator-facing evidence and never asserted.
//!
//! ## There is NO skip path (B8)
//!
//! Every failure in the capstone body is a panic; the body contains no
//! `return`. `JCODEMUNCH_URL` and `CODE_INDEX_PATH` CONFIGURE the run — their
//! absence is a hard failure naming the sanctioned invocation, not a silent
//! success.
//!
//! ### Why `#[ignore]` is the gate, and why an env flag CANNOT be one
//!
//! `#[ignore]` is the SOLE gate. An env flag cannot gate the run without
//! reintroducing the defect being removed: if the flag is unset the test must
//! either return early (a graceful skip — forbidden by B8) or panic (which
//! reddens any `cargo test -- --ignored` sweep just as much as having no flag
//! at all). Both branches are strictly worse than `#[ignore]` alone. Verified
//! that `#[ignore]` genuinely excludes it: `grep -rn -- "--ignored" scripts/`
//! finds no verify-pipeline step that runs ignored tests. So env vars are
//! demoted to configuration, and "invoked" stays load-bearing — you must
//! deliberately pass `--ignored`.
//!
//! ### Why this is a `cargo test`, not a `tests/infra/` shell test (OQ3)
//!
//! `tests/infra/run-all-classification.manifest` has exactly three buckets —
//! `pool`, `intra-run-serial`, `host-exclusive` — and ALL THREE run under
//! `run_all.sh` on the merge gate. There is no "excluded" bucket, so a
//! hard-failing live capstone placed there would be gate-resident by
//! construction and would turn main RED for every merge on any host without
//! uvx, network or jcodemunch — the Error-on-a-healthy-path outcome the
//! capability manifest's `capstone-must-not-become-gate-resident` forbids
//! (jcodemunch is legitimately absent in task worktrees, PRD §9). The two
//! jcodemunch infra tests confirm the boundary: `test_jcodemunch_index_reify.sh`
//! and `test_with_jcodemunch_serve.sh` are both `pool` and both deliberately
//! hermetic. Two further reasons: the capstone must invoke the `reify-audit`
//! BINARY, which `env!("CARGO_BIN_EXE_reify-audit")` hands a cargo test for
//! free and a shell test would have to build itself; and
//! `jcodemunch_session_live.rs` (#6106) already established the
//! `#[ignore]` + hard-fail live-test pattern in this same crate.
//!
//! `tests/infra/run-all-classification.manifest` is therefore DELIBERATELY
//! UNTOUCHED by this change, and the esc-4914-162 same-diff-registration
//! hazard does not arise at all.
//!
//! ## The seams are gate-resident even though the capstone is not
//!
//! The preflight verdict, the findings-shape predicate, the corpus builder and
//! the β invocation are each tested WITHOUT a serve, in the `serve_preflight`,
//! `finding_shape`, `corpus_fixture` and `index_invocation` modules. Only
//! their COMPOSITION is `#[ignore]`d. This is the pattern
//! `jcodemunch_session_live.rs` recorded for `finish_teardown`: leaving the
//! machinery of an opt-in test untested "would reproduce the exact failure
//! mode this file exists to close, one layer down". The sharpest instance is
//! `index_invocation::the_index_script_this_capstone_names_actually_exists` —
//! the prior PRD's `L-SMOKE` binding named `scripts/smoke-jcodemunch-audit.sh`,
//! which never existed, and nothing noticed.
//!
//! ## Running it
//!
//! ```sh
//! CODE_INDEX_PATH=$(mktemp -d) bash scripts/with-jcodemunch-serve.sh --port 8917 -- \
//!   cargo test -p reify-audit --test jcodemunch_live -- --ignored --nocapture
//! ```
//!
//! δ (`scripts/with-jcodemunch-serve.sh`) owns the serve lifecycle — spawn,
//! readiness identity-poll, `JCODEMUNCH_URL` export, and unconditional
//! teardown with leak detection — so this file spawns nothing and tears down
//! nothing. `CODE_INDEX_PATH` must be set OUTSIDE the wrapper because both
//! halves need it: the serve reads its own index directory, and so does the
//! capstone. A throwaway one keeps the run off host-global `~/.code-index`.
//!
//! Requires `uvx` and network. From a landlock-sandboxed agent also export
//! `UV_TOOL_DIR=/tmp/<dir>`: the write-set denies `~/.local/share/uv`, so δ's
//! `uvx` spawn otherwise fails `E_JC_SERVE_SPAWN_FAILED`. `~/.cache/uv` is
//! writable, so the package cache stays warm.
//!
//! A plain `cargo test -p reify-audit` runs the 14 hermetic tests and skips
//! the capstone.

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

/// Write a `tasks.json` holding ONE synthetic done task, pointed at `commit`
/// with `files: [touched_file]`. Returns the path.
///
/// Adapts cli.rs's `task_fixture` + `write_tasks_json`, with one mandatory
/// difference: `done_at` MUST be set. cli.rs leaves it `null`, and P1 SKIPS a
/// task whose `done_at` is null — a fixture that inherited that would make the
/// P1 leg vacuous no matter what the seam did.
///
/// `commit` and `touched_file` are parameters, not constants. They used to be
/// a pinned reify SHA (`ff1cb80c31`) and a hardcoded reify path, carrying the
/// premise "this commit's diff contains >=1 still-orphaned public symbol" —
/// a premise nobody validated, which rots as the corpus churns, and whose only
/// purpose was to prop up a forbidden count assertion. The capstone now passes
/// its THROWAWAY corpus's own HEAD and the file it genuinely modified in
/// `prev..head`, so the range P1 derives (`commit^1..commit`) is real by
/// construction and cannot rot.
fn write_synthetic_done_task(
    dir: &std::path::Path,
    commit: &str,
    done_at_epoch: u64,
    touched_file: &str,
) -> std::path::PathBuf {
    let task = serde_json::json!([{
        "task_id": "synthetic-capstone-p1",
        "status": "done",
        "files": [touched_file],
        "done_provenance": {
            "kind": "merged",
            "commit": commit,
            "note": null
        },
        "title": "Synthetic done task for the P1 capstone leg",
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
const JC_INDEX_SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/jcodemunch-index-reify.sh"
);

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
        let sha = String::from_utf8(out.stdout)
            .expect("utf8 sha")
            .trim()
            .to_string();
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

    CorpusRepo {
        root,
        repo_id,
        head,
        prev,
        touched_file,
    }
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
        let render =
            || serde_json::to_string_pretty(finding).unwrap_or_else(|_| format!("{finding:?}"));

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
// NOTE — there is deliberately NO `JCODEMUNCH_REPO` constant here
// -----------------------------------------------------------------------
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
// binary derives from `--project-root` — which for this capstone is the
// THROWAWAY corpus `build_corpus_repo` mints, so the id is whatever
// `resolve_repo_id` derives for that temp path. Omitting the flag is what
// selects it, so re-introducing a pinned constant here would re-break the
// capstone. If a future leg genuinely needs an explicit id, derive it with
// `reify_audit::jcodemunch_index::resolve_repo_id` rather than spelling one
// out.
//
// There is likewise no `PINNED_P1_COMMIT` any more. The P1 leg's range comes
// from the throwaway corpus's own `head`/`prev`, so it cannot rot as the
// reify corpus churns — and it no longer encodes the "this commit contains
// >=1 orphan symbol" premise the old constant's comment carried.

// -----------------------------------------------------------------------
// The capstone (#[ignore]-gated; requires a live serve — see the module doc)
// -----------------------------------------------------------------------

/// The real `reify-audit` binary completes a REAL MCP session against a
/// freshly-indexed, NON-EMPTY corpus, on both detector legs.
///
/// # What is asserted, per leg
///
/// 1. `exit == Some(0)`.
/// 2. stderr carries no `E_JC_INDEX_` marker — the §4.3 gate ADMITTED the run
///    rather than refusing it.
/// 3. stderr carries no `jcodemunch unreachable at` breadcrumb — the client
///    completed a real session instead of degrading to `NoopJCodemunchOps`.
///    This is the single assertion that would have caught PRD §2.3: the
///    degraded path still exits 0 and still serializes a well-formed EMPTY
///    array, which is what let a broken handshake survive ten weeks.
/// 4. every element of the findings array is well-formed.
///
/// Plus, once, before either leg: β printed its own `INDEX-OK` token, and
/// `count_symbols(index_dir, repo_id) > 0` read INDEPENDENTLY against the
/// very index the binary is about to be pointed at. That last one is what
/// makes "runs to completion over a non-empty symbol set" (B6) unsatisfiable
/// by an empty findings array — it reads the index, not the detector output.
///
/// # What is NOT asserted
///
/// Any finding COUNT, on either leg. See `assert_well_formed_findings`.
///
/// # There is no skip path
///
/// Every failure below is a panic; the body contains no `return`. Missing
/// configuration is a hard failure naming the sanctioned one-liner, not a
/// silent success (B8).
#[ignore = "live capstone: needs a real jcodemunch serve + index; run it through scripts/with-jcodemunch-serve.sh with --ignored (see the module doc)"]
#[test]
fn live_capstone_completes_a_real_session_over_a_freshly_indexed_non_empty_corpus() {
    // ---------------------------------------------------------------
    // 1. Configuration. Absent config is FATAL, never a skip.
    // ---------------------------------------------------------------
    let serve_url = std::env::var("JCODEMUNCH_URL").unwrap_or_else(|_| {
        panic!(
            "JCODEMUNCH_URL is unset. This capstone does not guess a default and \
             does not skip — run it through δ's lifecycle wrapper, which spawns a \
             serve, readiness-polls it, exports JCODEMUNCH_URL and tears down on \
             every exit path:\n\n{RUN_COMMAND}"
        )
    });
    let index_dir = std::env::var("CODE_INDEX_PATH").unwrap_or_else(|_| {
        panic!(
            "CODE_INDEX_PATH is unset. The serve reads its OWN index directory, so \
             pointing this capstone at a throwaway corpus means telling BOTH halves \
             where that directory lives — the serve (via the environment it was \
             spawned in) and this test. Leaving it unset would silently target \
             host-global ~/.code-index.\n\n{RUN_COMMAND}"
        )
    });
    let index_dir = std::path::Path::new(&index_dir);

    // ---------------------------------------------------------------
    // 2. B8: the endpoint must PROVE it is a jcodemunch serve.
    // ---------------------------------------------------------------
    require_reachable_serve(&serve_url);

    // ---------------------------------------------------------------
    // 3-5. A real corpus, really indexed, with a really non-empty symbol set.
    // ---------------------------------------------------------------
    let tmp = tempfile::tempdir().expect("create tempdir");
    let corpus = build_corpus_repo(tmp.path());
    let index_output = run_index_pass(&corpus.root, index_dir);

    let symbols = reify_audit::jcodemunch_index::count_symbols(index_dir, &corpus.repo_id);
    assert!(
        symbols > 0,
        "the index at {}/{} holds {symbols} symbols. β reported success, so this \
         means the binary and β disagree about the identity or the directory — \
         which is the failure the §4.3 gate exists to make loud. β's output:\n{index_output}",
        index_dir.display(),
        corpus.repo_id
    );

    let corpus_root = corpus.root.to_str().expect("corpus root is valid UTF-8");
    let dir = tmp.path();
    let runs_db = write_empty_runs_db(dir);
    let runs_db = runs_db.to_str().expect("runs.db path is valid UTF-8");

    // ---------------------------------------------------------------
    // 6. PDEAD leg — repo-wide; serve-only; no tasks needed.
    // ---------------------------------------------------------------
    let empty_tasks = write_empty_tasks_file(dir);
    let (pdead_code, pdead_findings, pdead_stderr) = run_reify_audit(&[
        "--pattern",
        "PDEAD",
        "--jcodemunch-url",
        &serve_url,
        "--jcodemunch-index-dir",
        index_dir.to_str().expect("index dir is valid UTF-8"),
        "--project-root",
        corpus_root,
        "--tasks-file",
        empty_tasks
            .to_str()
            .expect("tasks.json path is valid UTF-8"),
        "--runs-db",
        runs_db,
    ]);
    assert_live_leg("PDEAD", pdead_code, &pdead_findings, &pdead_stderr);

    // ---------------------------------------------------------------
    // 7. P1 leg — one synthetic done task pinned to the corpus's own HEAD.
    //
    // P1 maps `done_provenance.commit` to `commit^1..commit` and feeds it to
    // get_changed_symbols, so the range is `corpus.prev..corpus.head` — real
    // commits in a real repo the serve has really indexed. `done_at` must be
    // non-zero: P1 skips a task whose done_at is null.
    // ---------------------------------------------------------------
    let synthetic_tasks =
        write_synthetic_done_task(dir, &corpus.head, 1_748_000_000, &corpus.touched_file);
    let (p1_code, p1_findings, p1_stderr) = run_reify_audit(&[
        "--pattern",
        "P1",
        "--jcodemunch-url",
        &serve_url,
        "--jcodemunch-index-dir",
        index_dir.to_str().expect("index dir is valid UTF-8"),
        "--project-root",
        corpus_root,
        "--tasks-file",
        synthetic_tasks
            .to_str()
            .expect("tasks.json path is valid UTF-8"),
        "--runs-db",
        runs_db,
    ]);
    assert_live_leg("P1", p1_code, &p1_findings, &p1_stderr);

    // ---------------------------------------------------------------
    // 8. Operator-facing acceptance evidence. PRINTED, never asserted.
    // ---------------------------------------------------------------
    for line in index_output.lines().filter(|l| l.contains("INDEX-OK")) {
        println!("capstone: β {}", line.trim());
    }
    println!(
        "capstone: repo_id={} symbols={symbols} range={}..{}",
        corpus.repo_id, corpus.prev, corpus.head
    );
    println!(
        "capstone: PDEAD leg exit={pdead_code:?} findings={}",
        pdead_findings.len()
    );
    println!(
        "capstone: P1    leg exit={p1_code:?} findings={}",
        p1_findings.len()
    );
}

/// The sanctioned way to run the capstone, quoted verbatim into every
/// diagnostic that needs it so an operator never has to reconstruct it.
const RUN_COMMAND: &str = "  CODE_INDEX_PATH=$(mktemp -d) bash scripts/with-jcodemunch-serve.sh --port 8917 -- \\\n\
     \x20   cargo test -p reify-audit --test jcodemunch_live -- --ignored --nocapture";

/// The four-part per-leg assertion, in the order that makes a failure
/// self-diagnosing.
///
/// Order matters. The exit code is checked first because a non-zero exit
/// subsumes everything after it; the `E_JC_INDEX_` check comes next so a §4.3
/// REFUSAL is never misread as a seam failure; the fail-soft breadcrumb comes
/// third because it is the one that distinguishes "a real session happened"
/// from "the run degraded and emitted a well-formed empty array"; and only
/// then is the payload's shape judged.
///
/// Shared by both legs rather than written twice, so the two cannot drift into
/// asserting different things about the same contract.
fn assert_live_leg(leg: &str, code: Option<i32>, findings: &[serde_json::Value], stderr: &str) {
    assert_eq!(
        code,
        Some(0),
        "{leg} leg: expected exit 0, got {code:?}. Exit 125 is either an infra/\
         connection failure or a §4.3 index refusal — the stderr below says \
         which (a refusal names an E_JC_INDEX_* marker).\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("E_JC_INDEX_"),
        "{leg} leg: the §4.3 freshness gate REFUSED this run instead of \
         admitting it, so no detector ever executed. β had just reported \
         INDEX-OK for this corpus, so the gate and β disagree.\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("jcodemunch unreachable at"),
        "{leg} leg: the binary FAILED to open a jcodemunch session and \
         fail-softed to NoopJCodemunchOps. It still exited 0 and still emitted \
         a well-formed empty findings array — which is exactly how PRD §2.3's \
         broken handshake survived ten weeks. The preflight said the endpoint \
         is a real jcodemunch-mcp serve, so the client is the broken half.\
         \nstderr:\n{stderr}"
    );
    assert_well_formed_findings(findings, leg);
}

#[cfg(test)]
mod live_leg_seam {
    use super::*;

    // -------------------------------------------------------------------
    // Why this module exists
    // -------------------------------------------------------------------
    //
    // The only test that drives a REAL leg is `#[ignore]`d, so the per-leg
    // contract would otherwise be checkable only by an operator with a serve
    // in hand — and a check nobody can run on the gate is a check that rots.
    // This is the same seam-splitting the file already applies to
    // `require_reachable_serve` (α's `finish_teardown` pattern) and to
    // `assert_well_formed_findings`: drive the assertion directly with
    // SYNTHETIC stderr, gate-resident, no serve, no network, no `uvx`.
    //
    // Both fail-soft LAYERS are pinned here, because they are independent:
    //
    //   * CONSTRUCTION — `reify-audit: jcodemunch unreachable at '<url>': …`
    //     (`src/bin/reify-audit.rs:797`), emitted when the handshake never
    //     completes and the whole run degrades to `NoopJCodemunchOps`.
    //   * PER CALL — `jcodemunch <tool>: …`
    //     (`src/jcodemunch_client.rs:1074/1117/1135`), emitted when a single
    //     `tools/call` errors AFTER a successful handshake and that one op
    //     returns `Vec::new()`.
    //
    // The second layer is the one the capstone was blind to. A per-call
    // failure leaves the construction breadcrumb silent, the §4.3 gate has
    // already admitted the run, the exit code is 0, and `[]` is perfectly
    // well-formed — every assertion that existed before this module passes.

    /// A healthy run's stderr: β's own `INDEX-OK` token plus one ordinary,
    /// REAL non-breadcrumb line.
    ///
    /// The second line is deliberately a genuine production message that
    /// contains the word `jcodemunch` — the suppression-enrichment read
    /// diagnostic, `src/jcodemunch_client.rs:664` — but is NOT a per-call
    /// failure. A check that matched the bare word rather than the
    /// tool-named breadcrumb would reject this healthy run, and
    /// `live_leg_accepts_a_clean_stderr` below would catch it.
    ///
    /// Deliberately NOT the empty string: an empty fixture would let a
    /// substring check that is subtly too broad still look correct.
    const HEALTHY_STDERR: &str = concat!(
        "INDEX-OK  repo=local/corpus-eb868740  db=/tmp/.tmpJc0/local/corpus-eb868740/code_index.db",
        "  3 sym  indexed=3 cap=2000  jc-changed-files=0\n",
        "reify-audit: jcodemunch suppression enrichment: failed to read",
        " /tmp/corpus/src/gone.rs: No such file or directory (os error 2)\n",
    );

    /// The FULL vacuous-pass shape: `HEALTHY_STDERR` with one fail-soft line
    /// spliced in and NOTHING else changed.
    ///
    /// Every caller below pairs this with `Some(0)` and `&[]`, so each
    /// fixture reproduces a run that is indistinguishable from success under
    /// the four assertions that predate this module: exit 0, no
    /// `E_JC_INDEX_` refusal marker, and a well-formed (empty) findings
    /// array. Only the assertion under test may fire.
    fn vacuous_pass_stderr(fail_soft_line: &str) -> String {
        format!("{HEALTHY_STDERR}{fail_soft_line}\n")
    }

    /// THE POSITIVE CONTROL, and it is load-bearing.
    ///
    /// Without it, an `assert_live_leg` that panicked unconditionally would
    /// satisfy every `#[should_panic]` below — which is precisely the
    /// pass-shaped-whether-or-not-it-works defect (PRD §2.4) this whole task
    /// exists to close, reproduced inside the tests meant to close it.
    #[test]
    fn live_leg_accepts_a_clean_stderr() {
        assert_live_leg(
            "PDEAD",
            Some(0),
            &[],
            HEALTHY_STDERR,
            PDEAD_CALL_BREADCRUMBS,
        );
    }

    /// `--pattern PDEAD` reaches exactly one op, and this is its fail-soft.
    ///
    /// `expected` pins the new check's own phrasing AND the specific tool, so
    /// a panic raised by the exit-code check, the `E_JC_INDEX_` check, the
    /// construction check or `assert_well_formed_findings` cannot satisfy it
    /// — each of those echoes the stderr, which contains the raw breadcrumb,
    /// so the breadcrumb alone would NOT have been a safe fragment.
    #[test]
    #[should_panic(
        expected = "PER-CALL fail-soft breadcrumb `jcodemunch get_dead_code_v2:` is present"
    )]
    fn live_leg_rejects_the_get_dead_code_per_call_fail_soft() {
        let stderr =
            vacuous_pass_stderr("jcodemunch get_dead_code_v2: transport error: connection closed");
        assert_live_leg("PDEAD", Some(0), &[], &stderr, PDEAD_CALL_BREADCRUMBS);
    }

    /// The load-bearing half of the P1 pair: `get_changed_symbols` runs
    /// unconditionally, so its breadcrumb's absence is real evidence.
    #[test]
    #[should_panic(
        expected = "PER-CALL fail-soft breadcrumb `jcodemunch get_changed_symbols:` is present"
    )]
    fn live_leg_rejects_the_get_changed_symbols_per_call_fail_soft() {
        let stderr = vacuous_pass_stderr(
            "jcodemunch get_changed_symbols: transport error: connection closed",
        );
        assert_live_leg("P1", Some(0), &[], &stderr, P1_CALL_BREADCRUMBS);
    }

    /// The SECOND entry of `P1_CALL_BREADCRUMBS`, which also proves the check
    /// scans the whole slice rather than stopping at the first element: this
    /// fixture carries no `get_changed_symbols` line at all.
    #[test]
    #[should_panic(
        expected = "PER-CALL fail-soft breadcrumb `jcodemunch find_references(` is present"
    )]
    fn live_leg_rejects_the_find_references_per_call_fail_soft() {
        let stderr = vacuous_pass_stderr(
            "jcodemunch find_references(some_symbol): transport error: connection closed",
        );
        assert_live_leg("P1", Some(0), &[], &stderr, P1_CALL_BREADCRUMBS);
    }

    /// The CONSTRUCTION layer, which has shipped since step-9 with no seam
    /// test of its own.
    ///
    /// Pinned by the same mechanism as the per-call layer so neither can rot
    /// into a check that can never fire. The fixture quotes the real
    /// `src/bin/reify-audit.rs:797` message verbatim.
    #[test]
    #[should_panic(expected = "FAILED to open a jcodemunch session")]
    fn live_leg_rejects_the_construction_fail_soft() {
        let stderr = vacuous_pass_stderr(
            "reify-audit: jcodemunch unreachable at 'http://127.0.0.1:8917/mcp': \
             transport error: connection refused — P1 degraded to zero findings; \
             P2/P5 still run (pass --no-jcodemunch to silence)",
        );
        assert_live_leg("PDEAD", Some(0), &[], &stderr, PDEAD_CALL_BREADCRUMBS);
    }
}

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
        assert_eq!(
            corpus.head.len(),
            40,
            "head must be a full sha: {:?}",
            corpus.head
        );
        assert_eq!(
            corpus.prev.len(),
            40,
            "prev must be a full sha: {:?}",
            corpus.prev
        );

        let out = common::git_env::git_cmd(&corpus.root)
            .args(["rev-parse", "HEAD^1"])
            .output()
            .expect("git rev-parse HEAD^1");
        assert!(
            out.status.success(),
            "HEAD must have a parent; git rev-parse HEAD^1 exited {:?}",
            out.status.code()
        );
        let parent = String::from_utf8(out.stdout)
            .expect("utf8 sha")
            .trim()
            .to_string();
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
                v.map(|v| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
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
            env.iter()
                .any(|(k, v)| k == "CODE_INDEX_PATH" && Some(v.as_str()) == index_dir.to_str()),
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
            env.iter()
                .any(|(k, v)| k == "JCODEMUNCH_MAX_FOLDER_FILES" && !v.is_empty()),
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
