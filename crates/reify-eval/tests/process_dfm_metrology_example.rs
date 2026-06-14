//! B integration gate: end-to-end DFM metrology example (task 4411 ζ).
//!
//! Loads `examples/process/std_process_dfm_metrology.ri` (the shipped
//! user-observable example), builds it with an OCCT kernel, runs
//! `engine.check()`, and asserts the auto-emitted diagnostic multiset.
//!
//! # Guard
//!
//! All tests early-return (eprintln + return) when
//! `!reify_kernel_occt::OCCT_AVAILABLE`, mirroring the γ harness in
//! `process_dfm_measure.rs`.
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

fn assert_no_dfm_diagnostic(result: &reify_eval::CheckResult, substr: &str) {
    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(substr))
        .collect();
    assert!(
        matching.is_empty(),
        "expected no diagnostic containing {:?}, but got: {:#?}",
        substr,
        matching
    );
}

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

// ── step-1 / step-2: OCCT-gated overhang slice ───────────────────────────────

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
