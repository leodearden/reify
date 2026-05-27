//! End-to-end integration test: bracket.ri source → STEP export.
//!
//! Verifies the full pipeline: parse → compile → Engine (with real constraint
//! checker and OCCT kernel) → build → valid STEP output.
//!
//! NOTE: OCCT's STEP writer uses global state that is not thread-safe.
//! These tests must run single-threaded (cargo test -- --test-threads=1)
//! or be structured to avoid concurrent OCCT access.

use reify_core::ModulePath;
use reify_ir::{ExportFormat, Satisfaction};

fn run_bracket_e2e(source: &str) {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Verify the compiler produced realizations
    assert_eq!(compiled.templates.len(), 1);
    let template = &compiled.templates[0];
    assert!(
        !template.realizations.is_empty(),
        "compiler should produce realization declarations from geometry lets"
    );
    assert_eq!(template.realizations[0].operations.len(), 1);

    // Build with real constraint checker and OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // All constraints should be satisfied
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }

    // Geometry output should be present and valid STEP
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    let step_str = String::from_utf8(output).expect("STEP should be valid UTF-8");
    assert!(
        step_str.contains("ISO-10303-21"),
        "STEP output should contain ISO-10303-21 header"
    );
}

/// The M1 exit criterion: parse bracket source, compile, build with OCCT, get valid STEP.
/// Tests both the in-memory fixture and the examples/bracket.ri file.
#[test]
fn bracket_source_to_step_e2e() {
    // Test with the in-memory fixture
    run_bracket_e2e(reify_test_support::bracket_source());

    // Test with the actual file
    let file_source = std::fs::read_to_string("../../examples/bracket.ri")
        .expect("examples/bracket.ri should exist");
    run_bracket_e2e(&file_source);
}
