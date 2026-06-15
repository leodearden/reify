//! B integration gate: end-to-end DFM metrology example (task 4411 ζ).
//!
//! Loads `examples/process/std_process_dfm_metrology.ri` (the shipped
//! user-observable example), builds it with an OCCT kernel, runs
//! `engine.check()`, and asserts the auto-emitted diagnostic multiset.
//!
//! # Guard
//!
//! All tests early-return (`eprintln!` + `return`) when
//! `!reify_kernel_occt::OCCT_AVAILABLE`, mirroring the γ harness in
//! `process_dfm_measure.rs`.  A CI run on a host without the OCCT kernel
//! therefore shows all three tests as "passed" but having made no assertions.
//! **Compile coverage for the example file in that environment is provided by
//! `crates/reify-compiler/tests/examples_smoke.rs`**, which discovers
//! `examples/**/*.ri` automatically and gates on Error-severity compile
//! diagnostics — no OCCT kernel required.  The integration assertions here
//! (build + check + diagnostic multiset) are deliberately OCCT-gated because
//! they depend on realized solid geometry.
//!
//! # Diagnostic multiset expected from the example file
//!
//! 3 overhang rules (Adding, max_overhang_angle=45°, box bottoms 90°):
//!   I_DFM_OVERHANG = 1, W_DFM_OVERHANG = 1, E_DFM_OVERHANG = 1.
//! 2 draft rules (Forming, draft_angle=3°, box vertical walls 0°):
//!   W_DFM_DRAFT = 1, E_DFM_DRAFT = 1.
//! 1 undercut rule (Forming, Info severity, planar re-entrant loft):
//!   I_DFM_DRAFT = 1, E_DFM_UNDERCUT = 1.
//! 1 conformer (Adding, max_overhang_angle=90°, box): emits nothing.

// ── helpers ──────────────────────────────────────────────────────────────────

fn assert_dfm_diagnostic_count(result: &reify_eval::CheckResult, substr: &str, count: usize) {
    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(substr))
        .collect();
    assert_eq!(
        matching.len(),
        count,
        "expected {count} diagnostic(s) containing {:?}, but got {}: {:#?}",
        substr,
        matching.len(),
        matching
    );
}

fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

fn load_and_compile_example() -> reify_compiler::CompiledModule {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/process/std_process_dfm_metrology.ri"
    );
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read example file {path}: {e}"));
    reify_test_support::parse_and_compile_with_stdlib(&source)
}

// ── step-1 / step-2: OCCT-gated overhang slice ──────────────────────────────

/// Loads the shipped example, builds with OCCT, checks, and asserts that
/// exactly one diagnostic is emitted per DFMSeverity for the overhang rules:
/// `I_DFM_OVERHANG` = 1, `W_DFM_OVERHANG` = 1, `E_DFM_OVERHANG` = 1.
///
/// RED (step-1): the example file does not exist yet → panics on read.
/// GREEN (step-2): the example file's three overhang rules make this pass.
#[test]
fn example_emits_one_dfm_overhang_per_severity() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping example_emits_one_dfm_overhang_per_severity: OCCT not available");
        return;
    }

    let compiled = load_and_compile_example();
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "I_DFM_OVERHANG", 1);
    assert_dfm_diagnostic_count(&result, "W_DFM_OVERHANG", 1);
    assert_dfm_diagnostic_count(&result, "E_DFM_OVERHANG", 1);
}

// ── step-3 / step-4: OCCT-gated draft slice ──────────────────────────────────

/// Loads the shipped example, builds with OCCT, checks, and asserts that
/// exactly one `W_DFM_DRAFT` (Warning) and one `E_DFM_DRAFT` (Error) are
/// emitted — one per Forming-process DFMRule with insufficient draft.
///
/// Box vertical walls have 0° draft < the Forming process `draft_angle=3°`
/// → draft violation at each rule's declared severity.
///
/// Counts are STABLE after step-6 adds the Info-severity undercut rule: that
/// rule's co-emitted draft lands on `I_DFM_DRAFT` (dfm.rs:350-360), never on
/// W or E, so `W_DFM_DRAFT` and `E_DFM_DRAFT` remain exactly 1 each.
///
/// RED (step-3): the example has no Forming process / draft rules yet →
///   assert fails (0 ≠ 1).
/// GREEN (step-4): two draft DFMRules (Warning + Error) are added.
#[test]
fn example_emits_one_dfm_draft_per_severity() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping example_emits_one_dfm_draft_per_severity: OCCT not available");
        return;
    }

    let compiled = load_and_compile_example();
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "W_DFM_DRAFT", 1);
    assert_dfm_diagnostic_count(&result, "E_DFM_DRAFT", 1);
}

// ── step-5 / step-6: OCCT-gated undercut + conformer slice ───────────────────

/// Loads the shipped example, builds with OCCT, checks, and asserts:
///
/// (a) Undercut — exactly one `E_DFM_UNDERCUT` (always Error regardless of
///     the rule's declared severity). The undercut rule uses `DFMSeverity.Info`
///     so that its mandatory co-emitted draft diagnostic lands on `I_DFM_DRAFT`.
///
/// (b) Co-emit — exactly one `I_DFM_DRAFT` (the undercut rule's Info severity
///     routes the co-emitted draft arm — dfm.rs:350-360 — to `I_DFM_DRAFT`,
///     keeping `W_DFM_DRAFT` and `E_DFM_DRAFT` at exactly 1 each).
///
/// (c) Conformer — the total `_DFM_OVERHANG` count stays at 3 (the conformer
///     rule uses `max_overhang_angle=90°`, whose threshold sin(90°)=1 means
///     `n·ẑ < -1` is never true → no 4th overhang diagnostic).
///
/// RED (step-5): the example has no undercut rule or conformer yet →
///   `E_DFM_UNDERCUT` = 0 ≠ 1 and `I_DFM_DRAFT` = 0 ≠ 1 → test fails.
/// GREEN (step-6): undercut rule (Info) + conformer are added to the example.
#[test]
fn example_emits_undercut_and_conformer_is_silent() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping example_emits_undercut_and_conformer_is_silent: OCCT not available"
        );
        return;
    }

    let compiled = load_and_compile_example();
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    // Undercut: always Error regardless of rule severity.
    assert_dfm_diagnostic_count(&result, "E_DFM_UNDERCUT", 1);
    // Co-emit: Info-severity undercut rule also triggers the draft arm → I_DFM_DRAFT.
    assert_dfm_diagnostic_count(&result, "I_DFM_DRAFT", 1);
    // Conformer: 90° max_overhang_angle → no 4th overhang diagnostic.
    assert_dfm_diagnostic_count(&result, "_DFM_OVERHANG", 3);
}
