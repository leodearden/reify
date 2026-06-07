//! Integration test for the production `Engine::realization_kernel_provenance()`
//! accessor (task 4248, piece 3).
//!
//! Mirrors `kernel_registry_inventory.rs::engine_with_registered_kernel_picks_occt_for_brep_box_build`:
//! - skips when OCCT is unavailable
//! - parses + compiles a single-realization BRep box
//! - runs `Engine::with_registered_kernel` → `engine.build(...)`
//! - calls the NEW production accessor `engine.realization_kernel_provenance()`
//! - asserts one entry: kernel == KernelId::Occt, repr == ReprKind::BRep,
//!   realization id is non-empty
//!
//! RED today: `realization_kernel_provenance` does not exist; the test will
//! fail to compile.

use reify_core::ModulePath;
use reify_ir::{ExportFormat, ReprKind};

#[test]
fn realization_kernel_provenance_reports_occt_brep_for_box_build() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping realization_kernel_provenance_reports_occt_brep_for_box_build: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Tiny single-realization BRep box (mirrors kernel_registry_inventory.rs).
    let source = r#"structure S {
    let b = box(10mm, 10mm, 10mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("provenance_box"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Confirm geometry was actually produced (same guard as kernel_registry_inventory).
    assert!(
        result.geometry_output.is_some(),
        "expected geometry output; diagnostics: {:?}",
        result.diagnostics
    );

    // Production accessor under test.
    let provenance = engine.realization_kernel_provenance();

    assert_eq!(
        provenance.len(),
        1,
        "expected exactly one realization entry; got {}: {:?}",
        provenance.len(),
        provenance.iter().map(|p| &p.realization).collect::<Vec<_>>()
    );

    let entry = &provenance[0];
    assert_eq!(
        entry.kernel,
        reify_core::KernelId::Occt,
        "expected terminal kernel Occt for a BRep box build, got {:?}",
        entry.kernel
    );
    assert_eq!(
        entry.repr,
        ReprKind::BRep,
        "expected repr BRep for a box build, got {:?}",
        entry.repr
    );
    assert!(
        !entry.realization.is_empty(),
        "realization id string must be non-empty"
    );
}
