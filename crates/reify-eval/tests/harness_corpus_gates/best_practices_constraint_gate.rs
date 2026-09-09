//! Corpus gate: constraint SATISFACTION over `examples/best_practices/*.ri`
//! (task #6215).
//!
//! # Invariant asserted
//!
//! Every constraint in every `.ri` file directly under
//! `examples/best_practices/` (flat, non-recursive) reports ZERO
//! `Satisfaction::Violated`, unconditionally — and the set of constraints
//! that report `Satisfaction::Indeterminate` instead of `Satisfied` is PINNED
//! by `EXPECTED_INDETERMINATE` below, checked in both directions: a new,
//! non-listed Indeterminate fails the gate (lost coverage), and a listed
//! entry that is now Satisfied ALSO fails, as stale (the exemption must be
//! deleted rather than linger and mask recovered coverage). This is
//! deliberately NOT "every constraint is Satisfied" — the corpus is not
//! uniformly green today, and both documented exceptions are intentional
//! (see `EXPECTED_INDETERMINATE`'s doc comment).
//!
//! # Why the seeded fire tests exist
//!
//! `seeded_violated_constraint_is_reported` / `seeded_satisfied_constraint_is_reported`
//! below are the MANDATED anti-silent-accept self-tests: the live corpus
//! sweep (see `run_corpus_gate`) is expected GREEN on the measured baseline,
//! so on its own it can never prove the Violated arm actually fires — a
//! `constraint_statuses` that always reported `Satisfied` would make the
//! whole sweep vacuously green. Both sibling debug gates in this crate
//! (`no_stale_undef_invariant_gate.rs`, `snapshot_cache_divergence_gate.rs`)
//! carry the same class of seeded fire test for the same reason.
//!
//! # `EXPECTED_INDETERMINATE` is not a `SKIP_SET`
//!
//! The "Do not add a `SKIP_SET` entry to exempt one" sentence in
//! `examples/best_practices/INDEX.md`, and its counterpart in
//! `.claude/skills/reify-design/SKILL.md`, both forbid adding a
//! `SKIP_SET` entry to this corpus — but that prohibition is scoped to the
//! COMPILE gate ("a file that cannot reach a clean compile does not belong
//! here"). `EXPECTED_INDETERMINATE` exempts no file from anything: every
//! listed constraint is still fully asserted here, just against
//! `Indeterminate` instead of `Satisfied`, and bidirectionally (see above).
//! Do not read this const as a norm violation of the SKIP_SET prohibition —
//! it is a different contract entirely.
//!
//! # Deliberately un-sharded
//!
//! Unlike this crate's two sibling corpus gates (`no_stale_undef_invariant_gate.rs`,
//! `snapshot_cache_divergence_gate.rs`), which shard ~251-file sweeps across
//! 24 `#[test]` fns to stay under the verify pipeline's heartbeat-idle
//! backstop, this gate runs as a single test. `examples/best_practices/` is
//! ~7 files, and the full in-process sweep measures ~0.3s — far short of the
//! backstop, so sharding would be dead weight (`examples_smoke.rs` is
//! likewise un-sharded on purpose, for the same reason).

use reify_core::ConstraintNodeId;
use reify_ir::Satisfaction;

// ── Seeded anti-silent-accept fire tests (step 1/2) ──────────────────────────

/// Minimal template with exactly one constraint, deliberately FALSE:
/// `w = 1mm`, `constraint w > 2mm` is Violated. Feeds
/// `seeded_violated_constraint_is_reported` below — the non-vacuity proof
/// that `constraint_statuses` (and, eventually, the corpus sweep's Violated
/// arm) can actually fire. Mirrors the MANDATED anti-silent-accept
/// seeded-violation self-test both sibling debug gates carry
/// (`no_stale_undef_invariant_gate.rs`, `snapshot_cache_divergence_gate.rs`).
const SEED_VIOLATED_SOURCE: &str = r#"
structure def SeededConstraintGateDemo {
    param w : Length = 1mm

    constraint w > 2mm
}
"#;

/// Same template as `SEED_VIOLATED_SOURCE`, with the one constraint made TRUE
/// instead: `w = 1mm`, `constraint w > 0mm` is Satisfied. Feeds
/// `seeded_satisfied_constraint_is_reported` below — the discrimination
/// proof: a helper that reported `Violated` for every constraint regardless
/// of truth would still pass the seeded-violation test above, so this
/// companion is required too.
const SEED_SATISFIED_SOURCE: &str = r#"
structure def SeededConstraintGateDemo {
    param w : Length = 1mm

    constraint w > 0mm
}
"#;

/// MANDATED anti-silent-accept fire test: a deliberately-false constraint
/// MUST be reported as `Satisfaction::Violated`. A `constraint_statuses` that
/// always returned `Satisfaction::Satisfied` — or an empty vec — would make
/// the eventual corpus sweep's zero-Violated assertion vacuously green.
#[test]
fn seeded_violated_constraint_is_reported() {
    let statuses = constraint_statuses(SEED_VIOLATED_SOURCE);
    assert!(
        !statuses.is_empty(),
        "expected at least one constraint result from the seeded source, got zero"
    );
    assert!(
        statuses.iter().any(|(_, s)| *s == Satisfaction::Violated),
        "expected the seeded false constraint (w=1mm, w > 2mm) to report \
         Satisfaction::Violated, got {statuses:?}"
    );
}

/// Discrimination companion to `seeded_violated_constraint_is_reported`
/// above: the SAME template with its one constraint made true must report
/// `Satisfaction::Satisfied`, never `Violated`. Without this test, a
/// `constraint_statuses` that reported `Violated` unconditionally would still
/// pass the test above.
#[test]
fn seeded_satisfied_constraint_is_reported() {
    let statuses = constraint_statuses(SEED_SATISFIED_SOURCE);
    assert!(
        !statuses.is_empty(),
        "expected at least one constraint result from the seeded source, got zero"
    );
    assert!(
        statuses.iter().any(|(_, s)| *s == Satisfaction::Satisfied),
        "expected the seeded true constraint (w=1mm, w > 0mm) to report \
         Satisfaction::Satisfied, got {statuses:?}"
    );
    assert!(
        !statuses.iter().any(|(_, s)| *s == Satisfaction::Violated),
        "expected zero Violated results for the seeded true constraint, got {statuses:?}"
    );
}

// ── constraint_statuses: the shared check surface (step 2) ──────────────────

/// Runs `source` through the exact pure value-eval check surface `reify
/// check` uses — `check_source_with_stdlib` is `parse_and_compile_with_stdlib`
/// followed by `make_simple_engine().check(&compiled)`, i.e.
/// `SimpleConstraintChecker` with NO geometry kernel — and extracts each
/// constraint's id and satisfaction, preserving `constraint_results`' order.
///
/// # Panics
/// Panics on a parse or compile error (via `check_source_with_stdlib`). Every
/// caller in this file that walks real corpus files prints the file path to
/// stderr first, so such a panic stays attributable to a file — see
/// `run_corpus_gate`. Re-asserting the zero-Error compile contract itself is
/// deliberately out of scope: `examples_smoke.rs` is the designated compile
/// gate for this corpus.
fn constraint_statuses(source: &str) -> Vec<(reify_core::ConstraintNodeId, Satisfaction)> {
    let result = reify_test_support::check_source_with_stdlib(source);
    result
        .constraint_results
        .into_iter()
        .map(|entry| (entry.id, entry.satisfaction))
        .collect()
}

// ── audit_file: pure per-file failure-taxonomy comparison (steps 3/4) ───────
//
// The tests below unit-test the FULL failure taxonomy of audit_file against
// synthetic inputs only (no filesystem, no real corpus file) — the live
// corpus is green today and can never exercise the Violated,
// UnexpectedIndeterminate, or StaleExpectedIndeterminate arms on its own, so
// these are the only proof each arm actually fires.

/// A single per-constraint audit failure, naming the offending file and
/// constraint. `id`'s `Display` impl (`reify_core::identity`) renders as
/// `{entity}#constraint[{index}]` — the exact spelling `reify check` prints —
/// so a failure is directly reproducible by copy-pasting into the CLI.
#[derive(Debug, Clone, PartialEq)]
enum GateFailure {
    /// A constraint reported `Satisfaction::Violated` — a real regression.
    /// Fires regardless of `expected_indeterminate`: a Violated is never
    /// excusable by the allowlist.
    Violated { file: String, id: ConstraintNodeId },
    /// A constraint reported `Satisfaction::Indeterminate` but its index is
    /// NOT in `expected_indeterminate` — lost coverage (a regression turned
    /// a checked constraint's inputs Undef).
    UnexpectedIndeterminate { file: String, id: ConstraintNodeId },
    /// A constraint's index IS in `expected_indeterminate`, but it now
    /// reports `Satisfaction::Satisfied` — the exemption is stale and must
    /// be deleted from `EXPECTED_INDETERMINATE` instead of lingering and
    /// masking the recovered coverage.
    StaleExpectedIndeterminate { file: String, id: ConstraintNodeId },
}

/// Pure, total, side-effect-free comparison: comparing one file's actual
/// per-constraint `Satisfaction` results against its pinned
/// `expected_indeterminate` index set. No I/O, no panics — this keeps the
/// full failure taxonomy unit-testable without touching the filesystem.
///
/// Precedence, evaluated per entry in `actual`:
///   1. `Violated` -> `GateFailure::Violated`, ALWAYS, regardless of
///      `expected_indeterminate`.
///   2. `Indeterminate` && index not in `expected_indeterminate` ->
///      `GateFailure::UnexpectedIndeterminate`.
///   3. `Satisfied` && index IS in `expected_indeterminate` ->
///      `GateFailure::StaleExpectedIndeterminate`.
///   4. otherwise (`Satisfied` and not expected, or `Indeterminate` and
///      expected) -> nothing.
fn audit_file(
    file: &str,
    expected_indeterminate: &[u32],
    actual: &[(ConstraintNodeId, Satisfaction)],
) -> Vec<GateFailure> {
    let mut failures = Vec::new();
    for (id, satisfaction) in actual {
        match satisfaction {
            Satisfaction::Violated => failures.push(GateFailure::Violated {
                file: file.to_string(),
                id: id.clone(),
            }),
            Satisfaction::Indeterminate if !expected_indeterminate.contains(&id.index) => {
                failures.push(GateFailure::UnexpectedIndeterminate {
                    file: file.to_string(),
                    id: id.clone(),
                });
            }
            Satisfaction::Satisfied if expected_indeterminate.contains(&id.index) => {
                failures.push(GateFailure::StaleExpectedIndeterminate {
                    file: file.to_string(),
                    id: id.clone(),
                });
            }
            _ => {}
        }
    }
    failures
}

/// All actual results Satisfied, empty expected-indeterminate set: nothing to
/// report.
#[test]
fn audit_clean_file_reports_nothing() {
    let actual = vec![
        (ConstraintNodeId::new("Clean", 0), Satisfaction::Satisfied),
        (ConstraintNodeId::new("Clean", 1), Satisfaction::Satisfied),
    ];
    let failures = audit_file("clean.ri", &[], &actual);
    assert!(
        failures.is_empty(),
        "expected zero failures for an all-Satisfied file with an empty \
         expected-indeterminate set, got {failures:?}"
    );
}

/// A Violated constraint must be reported even when its index is (wrongly)
/// listed in `expected_indeterminate` — a Violated is NEVER excusable by the
/// allowlist. This is the arm that catches the ticket's named regression
/// class (a drifted magnitude flipping a constraint to VIOLATED).
#[test]
fn audit_reports_violated_constraint() {
    let id = ConstraintNodeId::new("Bad", 0);
    let actual = vec![(id.clone(), Satisfaction::Violated)];

    let failures = audit_file("bad.ri", &[0], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::Violated {
            file: "bad.ri".to_string(),
            id,
        }],
        "a Violated constraint must be reported even when its index is listed \
         in expected_indeterminate — got {failures:?}"
    );
}

/// An Indeterminate constraint whose index is NOT in the expected set must be
/// reported as `UnexpectedIndeterminate`. This is the anti-coverage-erosion
/// arm: without it, a regression that flips a constraint's inputs Undef
/// (Satisfied -> Indeterminate) would pass the gate silently.
#[test]
fn audit_reports_unexpected_indeterminate() {
    let id = ConstraintNodeId::new("Drifted", 0);
    let actual = vec![(id.clone(), Satisfaction::Indeterminate)];

    let failures = audit_file("drifted.ri", &[], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::UnexpectedIndeterminate {
            file: "drifted.ri".to_string(),
            id,
        }],
        "an Indeterminate constraint whose index is not in expected_indeterminate \
         must be reported as UnexpectedIndeterminate — got {failures:?}"
    );
}

/// An index IS in the expected set, but its actual status is now Satisfied:
/// the exemption is stale and must be reported, so the dead entry gets
/// deleted from `EXPECTED_INDETERMINATE` instead of lingering and masking
/// the recovered coverage. The live corpus is green and can never exercise
/// this arm, so this synthetic test is the only proof it fires.
#[test]
fn audit_reports_stale_expected_indeterminate() {
    let id = ConstraintNodeId::new("Recovered", 0);
    let actual = vec![(id.clone(), Satisfaction::Satisfied)];

    let failures = audit_file("recovered.ri", &[0], &actual);
    assert_eq!(
        failures,
        vec![GateFailure::StaleExpectedIndeterminate {
            file: "recovered.ri".to_string(),
            id,
        }],
        "a Satisfied constraint whose index is listed in expected_indeterminate \
         must be reported as StaleExpectedIndeterminate — got {failures:?}"
    );
}

// ── corpus_files: flat corpus discovery (steps 5/6) ──────────────────────────

/// Absolute path to `examples/best_practices/`, resolved at compile time from
/// this crate's manifest directory (two levels up) — matches
/// `examples_smoke.rs:13`'s `EXAMPLES_DIR` and both sibling gates'
/// `corpus_files`.
const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/best_practices");

/// Basenames of the `*.ri` files directly inside `examples/best_practices/`,
/// sorted for deterministic reporting.
///
/// Deliberately a FLAT (non-recursive) read — this fn's flatness is
/// load-bearing, not incidental: the corpus is a single flat drawer of idiom
/// exemplars by design (`examples_smoke.rs::corpus_ri_files()`, line 636's
/// comment), and a nested subdirectory appearing here is a structural change
/// that should be reviewed rather than silently absorbed by switching this to
/// a recursive walk.
fn corpus_files() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(CORPUS_DIR);
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "best_practices_constraint_gate: cannot read directory '{}': {e}",
            dir.display()
        )
    });
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ri"))
        .collect();
    files.sort();
    files
}

/// Renders `path` as a repo-relative `examples/best_practices/<name>` string
/// for failure messages, so offenders are reported portably rather than as
/// absolute build-machine paths.
fn corpus_relative(path: &std::path::Path) -> String {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("examples/best_practices/{name}"),
        None => path.display().to_string(),
    }
}

/// Guards a not-yet-existing `corpus_files()`: at least 6 `.ri` files (the
/// count measured on this branch — a floor that catches a silently-emptied
/// or mis-resolved directory, the same class of guard as
/// `harness_compilation_surface/examples_smoke.rs`'s
/// `total >= MIN_DISCOVERED_RI_FILES`), every returned path a `.ri` file
/// sitting directly inside a `best_practices` directory (FLAT — a nested
/// subdirectory must NOT be swept, matching
/// `harness_compilation_surface/examples_smoke.rs::corpus_ri_files()`, whose
/// comment records that a nested subdirectory here is a structural change
/// that should be reviewed rather than silently absorbed), sorted
/// (deterministic failure output), and containing the known exemplars
/// `bolt_circle.ri` and `clearance_oracle.ri` by basename (proving path
/// resolution actually reached the real directory rather than returning an
/// empty vec that would make every later assertion vacuous).
#[test]
fn corpus_discovery_finds_the_flat_best_practices_drawer() {
    let files = corpus_files();

    assert!(
        files.len() >= 6,
        "expected at least 6 .ri files under examples/best_practices/, got {}: {files:?}",
        files.len()
    );

    for path in &files {
        assert_eq!(
            path.extension().and_then(|e| e.to_str()),
            Some("ri"),
            "expected every corpus_files() entry to be a .ri file, got {}",
            path.display()
        );
        assert_eq!(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("best_practices"),
            "expected {} to sit directly inside a 'best_practices' directory — \
             the walk must be FLAT, a nested subdirectory must not be swept",
            path.display()
        );
    }

    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(
        files, sorted,
        "expected corpus_files() to return a sorted list for deterministic failure output"
    );

    let basenames: Vec<&str> = files
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
        .collect();
    assert!(
        basenames.contains(&"bolt_circle.ri"),
        "expected corpus_files() to contain bolt_circle.ri, got {basenames:?}"
    );
    assert!(
        basenames.contains(&"clearance_oracle.ri"),
        "expected corpus_files() to contain clearance_oracle.ri, got {basenames:?}"
    );
}

// ── EXPECTED_INDETERMINATE: pinned allowlist well-formedness (steps 7/8) ────

/// The pinned set of `(basename, constraint index)` pairs that are expected
/// to report `Satisfaction::Indeterminate` — never `Satisfied` — under the
/// pure value-eval check surface this gate (and `reify check`) runs on. Each
/// entry carries a mandatory reason citing the in-file documentation that
/// explains why the Indeterminate is intentional, so a reviewer can confirm
/// the exemption against the exemplar's own prose rather than trusting this
/// const alone.
///
/// # This is NOT a `SKIP_SET`
///
/// The "Do not add a `SKIP_SET` entry to exempt one" sentence in
/// `examples/best_practices/INDEX.md`, and its counterpart in
/// `.claude/skills/reify-design/SKILL.md`, forbid adding a `SKIP_SET`
/// entry to this corpus — but that prohibition is scoped to the COMPILE gate
/// ("a file that cannot reach a clean compile does not belong here"). This
/// const exempts NO file from anything: every listed constraint is still
/// fully asserted by `run_corpus_gate`, just against `Indeterminate` instead
/// of `Satisfied` — and bidirectionally, via `audit_file`: a listed entry
/// that becomes `Satisfied` FAILS as stale (see
/// `GateFailure::StaleExpectedIndeterminate`), so a resolved exemption can
/// never linger as dead weight that masks recovered coverage.
///
/// Seeded with exactly the three entries measured on this branch, verified
/// two independent ways (`./target/release/reify check` per file, and an
/// in-process `check_source_with_stdlib` probe — both agree).
const EXPECTED_INDETERMINATE: &[(&str, u32, &str)] = &[
    (
        "clearance_oracle.ri",
        0,
        "`constraint not fouls` — `intersects` is a geometry-consumer builtin: it needs \
         a realized kernel and resolves only on the build()/tessellate() path, not the \
         pure value-eval surface this gate (and `reify check`) runs on. Documented by \
         the EVAL/BUILD ONLY bullet in clearance_oracle.ri, which states verbatim \
         \"THAT IS EXPECTED, NOT A FAILURE\", and echoed by the `clearance_oracle.ri` \
         row of examples/best_practices/INDEX.md.",
    ),
    (
        "clearance_oracle.ri",
        1,
        "`constraint gap > min_gap` — same class as constraint[0] above, via the \
         geometry-consumer builtin `distance`. Documented by the EVAL/BUILD ONLY \
         bullet in clearance_oracle.ri and echoed by the `clearance_oracle.ri` row of \
         examples/best_practices/INDEX.md.",
    ),
    (
        "discrete_choice.ri",
        0,
        "`constraint side * side == 1` — `DiscreteChoice.side` (an `auto(free)` param) is \
         undefined on this check surface. discrete_choice.ri:49-60 documents the sharp \
         edge: the solved root is only approximately +-1 (-0.9999999999999973, not \
         -1.0), and `==` is exact float equality — accepted at solve time by the \
         solver's own residual tolerance, which is why `reify eval` resolves this \
         constraint cleanly even though this check surface cannot.",
    ),
];

/// Guards a not-yet-existing `EXPECTED_INDETERMINATE`: every entry names a
/// file that actually exists in `corpus_files()` (mirrors
/// `examples_smoke.rs::skip_set_entries_exist_under_examples_dir`, line 248 —
/// an entry left behind after its exemplar is renamed or deleted must fail
/// loudly instead of silently never matching), no duplicate `(file, index)`
/// pairs, and every entry's reason string is non-empty — the
/// auditable-justification contract `SKIP_SET`'s `(&str, &str)` tuple shape
/// encodes (`examples_smoke.rs:20-22`).
#[test]
fn expected_indeterminate_entries_are_well_formed() {
    let files = corpus_files();
    let basenames: Vec<&str> = files
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
        .collect();

    let mut seen: std::collections::HashSet<(&str, u32)> = std::collections::HashSet::new();
    for &(file, index, reason) in EXPECTED_INDETERMINATE {
        assert!(
            basenames.contains(&file),
            "EXPECTED_INDETERMINATE entry ({file:?}, {index}) names a file that does not \
             exist under examples/best_practices/ — got corpus basenames {basenames:?}"
        );
        assert!(
            seen.insert((file, index)),
            "duplicate EXPECTED_INDETERMINATE entry for ({file:?}, {index}) — each \
             (file, constraint index) pair must appear at most once"
        );
        assert!(
            !reason.is_empty(),
            "EXPECTED_INDETERMINATE entry ({file:?}, {index}) has an empty reason string \
             — every exemption must carry a mandatory, auditable justification"
        );
    }
}

// ── run_corpus_gate: headline end-to-end corpus sweep (steps 9/10) ──────────

/// Break-glass: `REIFY_BEST_PRACTICES_CONSTRAINT_BYPASS=1` downgrades the
/// corpus assertion to a warn (prints offenders, does not fail) — mirrors
/// `REIFY_MAIN_GATE_BYPASS` (CLAUDE.md) and this crate's sibling gate
/// (`snapshot_cache_divergence_gate.rs::audit_bypassed`), so a future corpus
/// change can never wedge the merge queue.
fn audit_bypassed() -> bool {
    std::env::var("REIFY_BEST_PRACTICES_CONSTRAINT_BYPASS").is_ok_and(|v| v == "1")
}

/// Renders one `GateFailure` as a single human-readable line, using `id`'s
/// `Display` impl so the offender is directly reproducible by copy-pasting
/// into `reify check`.
fn describe_failure(failure: &GateFailure) -> String {
    match failure {
        GateFailure::Violated { file, id } => format!("VIOLATED: {file}: {id}"),
        GateFailure::UnexpectedIndeterminate { file, id } => {
            format!("UNEXPECTED INDETERMINATE: {file}: {id}")
        }
        GateFailure::StaleExpectedIndeterminate { file, id } => {
            format!("STALE EXPECTED_INDETERMINATE: {file}: {id}")
        }
    }
}

/// The thin driver wiring `corpus_files` (discovery), `constraint_statuses`
/// (per-file check surface), and `audit_file` (pure per-file comparison
/// against `EXPECTED_INDETERMINATE`) into the headline corpus sweep. Returns
/// every `GateFailure` found across `examples/best_practices/`.
///
/// Deliberately does NOT re-assert the zero-Error compile contract —
/// `examples_smoke.rs` owns that gate and duplicating it here would be
/// lockstep duplication. `constraint_statuses` (via
/// `check_source_with_stdlib`) panics on a parse/compile error, so this fn
/// `eprintln!`s each file's repo-relative path immediately BEFORE checking
/// it, keeping such a panic attributable to a file without re-asserting the
/// compile contract itself.
///
/// When `REIFY_BEST_PRACTICES_CONSTRAINT_BYPASS=1` is set, the offender
/// report is still printed but this returns an empty vec, downgrading
/// `best_practices_corpus_satisfies_every_constraint` to a warn.
fn run_corpus_gate() -> Vec<GateFailure> {
    let files = corpus_files();
    let mut failures: Vec<GateFailure> = Vec::new();
    let mut constraints_checked = 0usize;

    for path in &files {
        let display = corpus_relative(path);
        eprintln!("  checking {display}");

        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("best_practices_constraint_gate: reading {display}: {e}"));
        let statuses = constraint_statuses(&source);
        constraints_checked += statuses.len();

        let basename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let expected: Vec<u32> = EXPECTED_INDETERMINATE
            .iter()
            .filter(|(file, _, _)| *file == basename)
            .map(|(_, index, _)| *index)
            .collect();

        failures.extend(audit_file(&display, &expected, &statuses));
    }

    eprintln!(
        "best_practices_constraint_gate: {} file(s) swept, {} constraint(s) checked, {} \
         EXPECTED_INDETERMINATE exemption(s) applied:",
        files.len(),
        constraints_checked,
        EXPECTED_INDETERMINATE.len(),
    );
    for (file, index, reason) in EXPECTED_INDETERMINATE {
        eprintln!("    {file}#constraint[{index}]: {reason}");
    }

    if failures.is_empty() {
        return failures;
    }

    if audit_bypassed() {
        eprintln!(
            "[REIFY_BEST_PRACTICES_CONSTRAINT_BYPASS] {} failure(s) DOWNGRADED to warn:",
            failures.len()
        );
        for failure in &failures {
            eprintln!("    {}", describe_failure(failure));
        }
        eprintln!(
            "(unset REIFY_BEST_PRACTICES_CONSTRAINT_BYPASS to re-enforce this gate as a hard \
             failure)"
        );
        return Vec::new();
    }

    failures
}

/// The headline gate: sweeps every file `corpus_files()` returns, checking
/// each constraint via `constraint_statuses` and auditing it via
/// `audit_file` against `EXPECTED_INDETERMINATE`. Asserts ZERO
/// `GateFailure`s on the live corpus.
///
/// This is expected GREEN on the measured baseline (7 files, 31 constraints:
/// 28 Satisfied / 3 Indeterminate / 0 Violated, with all 3 Indeterminate
/// listed in `EXPECTED_INDETERMINATE` above).
#[test]
fn best_practices_corpus_satisfies_every_constraint() {
    let failures = run_corpus_gate();
    if failures.is_empty() {
        return;
    }

    let mut violated = Vec::new();
    let mut unexpected_indeterminate = Vec::new();
    let mut stale = Vec::new();
    for failure in &failures {
        match failure {
            GateFailure::Violated { file, id } => violated.push(format!("    {file}: {id}")),
            GateFailure::UnexpectedIndeterminate { file, id } => {
                unexpected_indeterminate.push(format!("    {file}: {id}"))
            }
            GateFailure::StaleExpectedIndeterminate { file, id } => {
                stale.push(format!("    {file}: {id}"))
            }
        }
    }

    let mut report = String::new();
    if !violated.is_empty() {
        report.push_str(&format!(
            "  VIOLATED ({} constraint(s)) — a real regression. Reproduce with \
             `reify check examples/best_practices/<file>`:\n{}\n",
            violated.len(),
            violated.join("\n")
        ));
    }
    if !unexpected_indeterminate.is_empty() {
        report.push_str(&format!(
            "  UNEXPECTED INDETERMINATE ({} constraint(s)) — a constraint's inputs went \
             undefined (lost coverage). Reproduce with \
             `reify check examples/best_practices/<file>`. If this Indeterminate is \
             genuinely intentional, add a reasoned entry to EXPECTED_INDETERMINATE; \
             otherwise it is a regression to fix:\n{}\n",
            unexpected_indeterminate.len(),
            unexpected_indeterminate.join("\n")
        ));
    }
    if !stale.is_empty() {
        report.push_str(&format!(
            "  STALE EXPECTED_INDETERMINATE ({} entr(ies)) — now Satisfied. DELETE the \
             corresponding entry from EXPECTED_INDETERMINATE so it stops masking the \
             recovered coverage:\n{}\n",
            stale.len(),
            stale.join("\n")
        ));
    }

    panic!(
        "best_practices constraint gate: {} failure(s) across examples/best_practices/:\n{}",
        failures.len(),
        report
    );
}
