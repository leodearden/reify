//! Version-id discipline gate (task ε, PRD
//! `docs/prds/v0_6/engine-build-hardening.md` §8 / D7) — makes INV-BUILD-2
//! (`docs/invariants.md`: "Version/snapshot IDs are allocated and read
//! through exactly one API each") self-enforcing.
//!
//! Scans `crates/reify-eval/src/**/*.rs` for raw `self.next_version_id` /
//! `self.next_snapshot_id` arithmetic or reads outside the allocator
//! (`Engine::allocate_snapshot_version`, engine_admin.rs) and the two named
//! readers (`Engine::last_allocated_version`, engine_admin.rs;
//! `Engine::current_eval_version`, engine_build.rs) — the only fns where
//! bumping/reading the raw counters IS the API's own definition. A line
//! carrying an explicit `// version-id-gate: allow — <reason>` escape
//! comment is exempted (same grammar as the `ptodo:allow` precedent).
//!
//! Step-1 (RED): anti-vacuous unit self-tests over the not-yet-implemented
//! `scan_source` matcher — mirrors the seeded-violation self-test pattern in
//! `no_stale_undef_invariant_gate.rs::seeded_stale_undef_violation_is_reported`.
//! `scan_source`/`Violation` do not exist yet, so this file does not
//! compile: that failure to compile IS the RED signal for this step.

/// One raw, non-exempt `self.next_version_id` / `self.next_snapshot_id` use
/// found outside the allocator/reader API family.
#[derive(Debug)]
struct Violation {
    /// The label passed to `scan_source` — the scanned file's display path
    /// (real-tree scans) or bare basename (unit self-tests below).
    file: String,
    /// 1-based line number within the scanned source.
    line: usize,
    /// The raw (not comment-stripped) source line, trimmed.
    text: String,
}

// ── Step-1 self-tests (RED: `scan_source` doesn't exist yet — this whole
//    file fails to compile until step-2 implements it) ─────────────────────

/// (a) A synthetic raw bump inside a non-allowlisted fn body is reported —
/// the permanent "demonstrably RED if reintroduced" proof, mirroring
/// `seeded_stale_undef_violation_is_reported`.
#[test]
fn seeded_synthetic_violation_is_reported() {
    let source = "\
impl Engine {
    pub fn some_other_method(&mut self) {
        self.next_version_id += 1;
    }
}
";
    let violations = scan_source("engine_eval.rs", source);
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one violation for a raw bump reintroduced in a \
         non-allowlisted fn, got {violations:?}"
    );
    assert_eq!(violations[0].file, "engine_eval.rs");
    assert!(
        violations[0].text.contains("next_version_id"),
        "violation text should quote the offending line, got {:?}",
        violations[0].text
    );
}

/// (b) The SAME token inside each allowlisted fn's own body is not
/// reported — these three fns ARE the allocate/read API family.
#[test]
fn allowlisted_fn_bodies_report_zero_violations() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "engine_admin.rs",
            "allocate_snapshot_version",
            "self.next_snapshot_id += 1;\n        self.next_version_id += 1;",
        ),
        (
            "engine_admin.rs",
            "last_allocated_version",
            "self.next_version_id.saturating_sub(1)",
        ),
        (
            "engine_build.rs",
            "current_eval_version",
            "self.next_version_id",
        ),
    ];
    for (label, fn_name, body) in cases {
        let source = format!(
            "impl Engine {{\n    pub(crate) fn {fn_name}(&mut self) -> VersionId {{\n        {body}\n    }}\n}}\n"
        );
        let violations = scan_source(label, &source);
        assert!(
            violations.is_empty(),
            "expected zero violations inside allowlisted fn {fn_name} \
             ({label}), got {violations:?}"
        );
    }
}

/// (c) A line carrying the `// version-id-gate: allow — <reason>` escape
/// comment is exempted even inside a non-allowlisted fn.
#[test]
fn allow_comment_escape_hatch_suppresses_violation() {
    let source = "\
impl Engine {
    fn restore_from_setup(&mut self, setup: &Setup) {
        self.next_snapshot_id = setup.snapshot_id.0; // version-id-gate: allow — setup restore, not allocation
        self.next_version_id = setup.version.0; // version-id-gate: allow — setup restore, not allocation
    }
}
";
    let violations = scan_source("concurrent.rs", source);
    assert!(
        violations.is_empty(),
        "expected the allow-comment escape hatch to suppress both lines, \
         got {violations:?}"
    );
}

/// (d) A comment-only (`//` or `///`) mention of the token is not matched —
/// the gate matches code tokens, not prose.
#[test]
fn comment_only_mentions_are_not_matched() {
    let source = "\
impl Engine {
    /// Must not read `self.next_version_id` directly — use the allocator.
    // self.next_snapshot_id is documented here too.
    fn some_fn(&mut self) {}
}
";
    let violations = scan_source("engine_build.rs", source);
    assert!(
        violations.is_empty(),
        "expected doc/comment-only mentions to be stripped before \
         matching, got {violations:?}"
    );
}

/// (e) A bare (non-`self.`-scoped) constructor initialiser and an
/// `engine.`-scoped test-style read are not matched — the gate is scoped to
/// the Engine's own `&mut self` allocation idiom, not every appearance of
/// the token.
#[test]
fn non_self_scoped_uses_are_not_matched() {
    let source = "\
impl Engine {
    fn new() -> Self {
        Self {
            next_snapshot_id: 0,
            next_version_id: 0,
        }
    }
}

fn read_counters(engine: &Engine) -> (u64, u64) {
    (engine.next_snapshot_id, engine.next_version_id)
}
";
    let violations = scan_source("engine_admin.rs", source);
    assert!(
        violations.is_empty(),
        "expected non-`self.`-scoped uses (constructor initialiser, \
         `engine.`-bound test read) to be ignored, got {violations:?}"
    );
}
