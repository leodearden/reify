//! Corpus-wide gate: no committed `.ri` file may call a function the compiler
//! does not know (task #5371).
//!
//! # Why a sweep, and why permanent
//!
//! `DiagnosticCode::UnresolvedFunction` is warn-mode today. The reason it can
//! stay warn-mode and still be worth landing is that #5997 flips it to an
//! Error, and that flip is only safe if the corpus is *already* clean. This
//! binary is the artifact that answers "is it?" — mechanically, on every test
//! run, rather than once by hand at authoring time. #5997 names the sweep as a
//! precondition; #6014 (registry ω) seeds its family-by-family migration from
//! the violation list this sweep produces.
//!
//! A one-shot sweep would have gone stale the first time someone added an
//! example. As a test binary it turns RED the moment a new `.ri` lands calling
//! a name that exists nowhere — which, before #5371, compiled clean.
//!
//! # What the run reports
//!
//! On failure the panic message IS the enumeration: one `(file, line, callee)`
//! triple per warning, sorted, so the run itself is the durable artifact rather
//! than something a human has to reconstruct.
//!
//! # Skips, and why they are two different mechanisms
//!
//! * **Files that do not compile cleanly today are skipped silently.** A file
//!   with parse errors or Error-severity diagnostics is failing for a reason
//!   that has nothing to do with #5371, and its `UnresolvedFunction` warnings
//!   would be downstream noise from a broken parse. `examples_smoke.rs` is the
//!   binary that gates *those*; this one must not duplicate its judgement, nor
//!   inherit its SKIP_SET (which would then have to be kept in sync).
//! * **Deliberate negatives are skipped by name, with a reason.** A file whose
//!   whole purpose is to exhibit an unresolved call cannot also be required not
//!   to have one. The `(path, reason)` tuple shape forces every entry to
//!   justify itself at review time — the idiom is lifted from
//!   `examples_smoke::SKIP_SET`.
//!
//! The list is empty today, and that is the honest state: every unresolved
//! callee found under the swept roots was a real gap, dispositioned into
//! `EVAL_DEFERRED_BUILTIN_NAMES` rather than waved through here. The mechanism
//! stays because the alternative — reaching for a `#[ignore]` the first time a
//! deliberate negative lands under `examples/` — is worse.

use std::path::{Path, PathBuf};

use reify_compiler::module_dag::{ModuleResolver, compile_entry_with_stdlib_cfg};
use reify_compiler::parse_with_stdlib;
use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_compiler::cfg::CfgSet;

/// Workspace root, resolved at compile time from this crate's manifest dir.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The stdlib root every corpus file is compiled against.
const STDLIB_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib");

/// The corpus roots, as workspace-relative paths.
///
/// `crates/reify-compiler/stdlib` is here even though it is also the stdlib
/// root: the stdlib's own `.ri` sources are the highest-leverage place for an
/// unresolved callee to hide, because every downstream file inherits whatever
/// they declare.
///
/// # `tests/prd-gate/fixtures/` is deliberately NOT swept
///
/// Two independent reasons, both measured rather than assumed:
///
/// 1. **A Rust walk of that directory is forbidden by design.**
///    `tests/infra/test_verify_scope.sh`'s PG-DRIFT-DIR scenario reds on any
///    tracked `*.rs` that names the fixtures DIRECTORY with a string/format/
///    glob terminator, because `verify.sh`'s docs no-heavy-checks carve-out for
///    that directory rests on "nothing globs it", which is what makes ADDING a
///    fixture provably inert. A walker here would make every newly added
///    fixture a silently ungated Rust build input — reachable through a
///    hook-gated docs commit on `main` with no later gate. The scenario's own
///    note is explicit that the fix is NOT to extend `_RUST_COUPLED_RI_FIXTURES`
///    but to re-examine the carve-out, which is a cross-cutting infra decision
///    and not this task's to make. (A reviewed `pg-drift-dir:allow` marker
///    exists, but it exempts the *guard*, not the harm.)
/// 2. **The directory's own contract says fixtures need not compile.**
///    `tests/prd-gate/README.md`: "fixtures here are not required to parse or
///    to pass `reify check`: several are deliberately unparseable or
///    deliberately failing … Nothing in the repo compiles this directory
///    wholesale." A zero-unresolved-calls assertion over a drawer of deliberate
///    negatives is the wrong shape — the allowlist would have to be re-curated
///    on every new fixture, which is the opposite of a gate.
///
/// The 12 fixture call sites this sweep surfaced before the root was dropped
/// are recorded, with dispositions, in
/// `docs/notes/unresolved-function-warn-sweep-2026-08-29.md`, so #6014 keeps
/// the signal without this binary keeping the coupling.
const CORPUS_ROOTS: &[&str] = &["examples", "crates/reify-compiler/stdlib"];

/// Files exempt from the zero-`UnresolvedFunction` assertion, each with a
/// mandatory reason. Keys are workspace-relative, forward-slash separated.
///
/// This is for DELIBERATE negatives only — a file that is supposed to contain
/// an unresolved call. A file that merely happens to have one is a violation to
/// fix, not an entry to add.
///
/// **Empty, deliberately.** #5371's warn-mode baseline artifact already exists
/// as the prd-gate fixture `unknown_fn_silent_accept_baseline.ri` (written for
/// #6014, citing the 5371 observation by name), and it lives under a root this
/// sweep does not walk — so it needs no exemption here. Its behaviour is pinned
/// instead by `unresolved_function_tests::the_original_line_observation_now_
/// warns`, which compiles the same `line(point3(…), point3(…))` source inline:
/// an inline assertion couples the test to the BEHAVIOUR rather than to a path.
///
/// The BASENAME above is deliberate — this comment must not spell the fixture's
/// full `tests/prd-gate/…` path. `tests/infra/test_verify_scope.sh`'s PG-DRIFT
/// half (a) derives its coupled set with a `git grep -o` over ALL tracked `*.rs`
/// and is explicitly COMMENT-INCLUSIVE, so a doc-comment mention is a reference:
/// spelling the path here obliges `verify.sh`'s `_RUST_COUPLED_RI_FIXTURES` to
/// list the fixture, or the guard reds. Measured: with the path spelled out, the
/// derived set grows to 13 while the list holds 12, and the fixture classifies
/// `RUN_RUST=0`. Naming it costs a verify-scope coupling this binary gets no
/// gain from — it never opens the file.
const DELIBERATE_NEGATIVES: &[(&str, &str)] = &[];

/// One unresolved call found in the corpus.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Violation {
    /// Workspace-relative path, forward-slash separated.
    file: String,
    /// 1-indexed line of the call span.
    line: usize,
    /// The callee name, taken from the diagnostic's message.
    callee: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} — {}()", self.file, self.line, self.callee)
    }
}

/// 1-indexed line number of byte `offset` within `source`.
///
/// Saturates at the last line for an out-of-range offset rather than panicking:
/// a diagnostic with a bad span is a different bug, and this binary's job is to
/// report callees, not to police span arithmetic.
fn line_of(source: &str, offset: u32) -> usize {
    let clamped = (offset as usize).min(source.len());
    source[..clamped].bytes().filter(|b| *b == b'\n').count() + 1
}

/// The callee named by an `UnresolvedFunction` diagnostic.
///
/// Reads the canonical `"unresolved function: <name>"` message form documented
/// on `DiagnosticCode::UnresolvedFunction`. Falls back to the whole message if
/// the prefix ever changes, so the report degrades to "less precise" rather
/// than to "silently empty".
fn callee_of(message: &str) -> String {
    message
        .strip_prefix("unresolved function: ")
        .unwrap_or(message)
        .to_string()
}

/// Path relative to the workspace root, forward-slash separated.
fn workspace_relative(path: &Path) -> String {
    path.strip_prefix(WORKSPACE_ROOT)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Recursively collect `*.ri` files under `dir`, sorted for deterministic
/// reporting. A missing root is an empty result, not a panic — see
/// `every_corpus_root_exists`, which is where that is diagnosed properly.
fn collect_ri_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// Every `.ri` file across all three corpus roots, sorted.
fn discover_corpus() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in CORPUS_ROOTS {
        collect_ri_files(&Path::new(WORKSPACE_ROOT).join(root), &mut paths);
    }
    paths.sort();
    paths
}

/// Compile one corpus file exactly the way `reify check` does, and return its
/// diagnostics — or `None` when the file cannot be parsed at all.
///
/// # Why this entry point and not another
///
/// The compile path is load-bearing here in a way it is not for most tests,
/// because the wrong one manufactures phantom violations by the hundred. A
/// callee is reported unresolved when nothing *in scope* declares it, so any
/// scope this walker fails to seed shows up as a corpus defect that is really a
/// harness defect. Both plausible alternatives fail that way, measured:
///
/// * `compile_with_stdlib` (what `examples_smoke.rs` uses) seeds the stdlib
///   prelude but cannot follow a user `import`, so every call to a fn declared
///   in a sibling module reads as unknown.
/// * `compile_project` follows user imports but does NOT seed the prelude, so
///   every call to a stdlib `pub fn` reads as unknown. Measured on this corpus:
///   117 phantom sites across 32 callees — `SPEED_OF_LIGHT()`
///   (`stdlib/units.ri:185`), `Frame3()`, `PointLoad()`, `solve_elastic_static()`
///   and the rest of the FEA/dynamics/constants surface, every one of them a
///   real `pub fn` or `structure def` in `crates/reify-compiler/stdlib/`.
///
/// [`compile_entry_with_stdlib_cfg`] is the one that does both — it is what
/// `crates/reify-cli/src/main.rs` calls for `reify check` — so this sweep
/// answers the same question a user's `reify check` would, which is exactly the
/// question #5997 needs answered before it can turn these warnings into errors.
///
/// `parse_with_stdlib` (not bare `parse`) matches: prelude-aware parsing is
/// what makes `Type.Variant` references against stdlib enums resolve as
/// `EnumAccess` rather than as errors.
fn diagnostics_for(path: &Path) -> Option<Vec<reify_core::Diagnostic>> {
    let source = std::fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();

    let parsed = parse_with_stdlib(&source, ModulePath::single(&stem));
    // A file that does not parse has no call sites to judge. `examples_smoke.rs`
    // owns the "must parse" gate; duplicating it here would double-report.
    if !parsed.errors.is_empty() {
        return None;
    }

    // Sibling user imports resolve relative to the entry file's directory,
    // mirroring the CLI. `stdlib_root` is inert on this path — every `std.*`
    // import is skipped because `load_stdlib()` already seeded the whole
    // stdlib — but is passed truthfully rather than as a sentinel so the
    // resolver is constructed identically to the CLI's.
    let resolver = ModuleResolver::new(path.parent()?, Path::new(STDLIB_ROOT));
    let compiled = compile_entry_with_stdlib_cfg(&parsed, &resolver, &CfgSet::default());
    Some(compiled.diagnostics)
}

/// Walk the corpus and return every `UnresolvedFunction` violation, sorted.
fn sweep() -> Vec<Violation> {
    let skipped: Vec<&str> = DELIBERATE_NEGATIVES.iter().map(|(p, _)| *p).collect();
    let mut violations = Vec::new();

    for path in discover_corpus() {
        let rel = workspace_relative(&path);
        if skipped.contains(&rel.as_str()) {
            continue;
        }
        let Some(diagnostics) = diagnostics_for(&path) else {
            continue;
        };
        // A file that does not compile cleanly is out of scope — its warnings
        // are downstream of whatever is actually broken.
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for d in &diagnostics {
            if d.code != Some(DiagnosticCode::UnresolvedFunction) {
                continue;
            }
            let offset = d.labels.first().map(|l| l.span.start).unwrap_or(0);
            violations.push(Violation {
                file: rel.clone(),
                line: line_of(&source, offset),
                callee: callee_of(&d.message),
            });
        }
    }

    violations.sort();
    violations
}

// ---------------------------------------------------------------------------
// the gate
// ---------------------------------------------------------------------------

/// No committed `.ri` file calls a name outside `is_known_builtin`'s closed
/// world.
///
/// This is #5997's precondition, asserted rather than assumed. The failure
/// message enumerates every `(file, line, callee)` triple so a RED run is
/// itself the violation list — see
/// `docs/notes/unresolved-function-warn-sweep-2026-08-29.md` for the
/// disposition of the names this surfaced when it first ran.
#[test]
fn corpus_has_no_unresolved_function_calls() {
    let violations = sweep();
    assert!(
        violations.is_empty(),
        "{} unresolved-function call site(s) in the committed .ri corpus.\n\n{}\n\n\
         Each is a call to a name in NO compiler classification family, NO eval \
         dispatch arm, and NO user/stdlib `fn`. Dispositions, in order of \
         preference:\n\
         \x20 1. the name is genuinely eval-dispatchable but not yet \
         family-registered -> add it to EVAL_DEFERRED_BUILTIN_NAMES in its \
         owning-module group (crates/reify-compiler/src/unresolved_function.rs);\n\
         \x20 2. the fallback's first-arg typing is VERIFIED correct for it -> \
         FIRST_ARG_TYPED_NAMES, with the eval body cited;\n\
         \x20 3. it is a typo or a dead name -> fix the .ri;\n\
         \x20 4. the file deliberately exhibits an unresolved call -> add it to \
         DELIBERATE_NEGATIVES here, with a reason.\n\
         Papering over a real typo with a manifest entry is disposition 1 \
         misapplied, and it is what the manifest's disjointness tests exist to \
         make expensive.",
        violations.len(),
        violations
            .iter()
            .map(|v| format!("  - {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ---------------------------------------------------------------------------
// guards on the sweep itself
// ---------------------------------------------------------------------------

/// The sweep must actually be sweeping something.
///
/// Without this, a typo in `CORPUS_ROOTS` or a `read_dir` that quietly returns
/// nothing would make `corpus_has_no_unresolved_function_calls` pass
/// vacuously — the classic way a corpus gate stops gating.
#[test]
fn every_corpus_root_exists_and_holds_ri_files() {
    for root in CORPUS_ROOTS {
        let dir = Path::new(WORKSPACE_ROOT).join(root);
        assert!(
            dir.is_dir(),
            "corpus root '{root}' does not exist at {} — CORPUS_ROOTS is stale",
            dir.display()
        );
        let mut found = Vec::new();
        collect_ri_files(&dir, &mut found);
        assert!(
            !found.is_empty(),
            "corpus root '{root}' contains no .ri files; the sweep over it is vacuous"
        );
    }
}

/// …and it must be compiling a substantial fraction of what it finds.
///
/// The silent "skip anything with an Error" rule is the sweep's biggest risk:
/// if a compiler change started erroring on most of the corpus, the gate would
/// go quietly green while covering nothing. The floor is deliberately loose —
/// this asserts the mechanism is alive, not a specific corpus health number,
/// which `examples_smoke.rs` owns.
#[test]
fn the_sweep_actually_compiles_most_of_the_corpus() {
    let all = discover_corpus();
    let clean = all
        .iter()
        .filter(|p| {
            diagnostics_for(p)
                .is_some_and(|ds| !ds.iter().any(|d| d.severity == Severity::Error))
        })
        .count();
    assert!(
        clean * 2 > all.len(),
        "only {clean} of {} corpus files compile without Errors — the sweep is \
         skipping more than it checks, so its green is not evidence of anything",
        all.len()
    );
}

/// Every `DELIBERATE_NEGATIVES` entry names a file that exists and carries a
/// reason. A stale entry silently exempts nothing, or worse, exempts a path
/// someone later creates for an unrelated purpose.
///
/// Vacuous while the list is empty, which is the current state; it exists so
/// that the first entry someone adds is checked, not so that today's zero is.
#[test]
fn deliberate_negatives_are_live_and_justified() {
    for (rel, reason) in DELIBERATE_NEGATIVES {
        let path = Path::new(WORKSPACE_ROOT).join(rel);
        assert!(
            path.is_file(),
            "DELIBERATE_NEGATIVES names '{rel}', which does not exist"
        );
        assert!(
            reason.len() > 30,
            "DELIBERATE_NEGATIVES entry '{rel}' needs a real reason, got {reason:?}"
        );
    }
}
