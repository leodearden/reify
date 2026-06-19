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
        provenance
            .iter()
            .map(|p| &p.realization)
            .collect::<Vec<_>>()
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

/// Two-build-within-one-engine test: first build seeds the realization cache;
/// second build hits the cache.  Asserts that `realization_kernel_provenance()`
/// still returns the correct kernel after the second build, pinning the
/// invariant that cache-hit paths push the terminal handle onto `step_handles`
/// (the condition `step_handles.len() > handle_start` relied on by the
/// `produced_kernel` write in `engine_build.rs`).
#[test]
fn realization_kernel_provenance_survives_cache_hit_rebuild() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping realization_kernel_provenance_survives_cache_hit_rebuild: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let source = r#"structure S {
    let b = box(10mm, 10mm, 10mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("cache_hit_provenance_box"));
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

    // First build — seeds the realization cache.
    let result1 = engine.build(&compiled, ExportFormat::Step);
    assert!(
        result1.geometry_output.is_some(),
        "first build: expected geometry output; diagnostics: {:?}",
        result1.diagnostics
    );

    // Second build on the same engine — should hit the realization cache.
    // The produced_kernel field in the graph node must still be populated
    // correctly after this build.
    let result2 = engine.build(&compiled, ExportFormat::Step);
    assert!(
        result2.geometry_output.is_some(),
        "second build (cache hit): expected geometry output; diagnostics: {:?}",
        result2.diagnostics
    );

    let provenance = engine.realization_kernel_provenance();
    assert_eq!(
        provenance.len(),
        1,
        "second build: expected exactly one realization entry; got {}: {:?}",
        provenance.len(),
        provenance
            .iter()
            .map(|p| &p.realization)
            .collect::<Vec<_>>()
    );

    let entry = &provenance[0];
    assert_eq!(
        entry.kernel,
        reify_core::KernelId::Occt,
        "second build (cache hit): expected terminal kernel Occt, got {:?}",
        entry.kernel
    );
    assert_eq!(
        entry.repr,
        ReprKind::BRep,
        "second build (cache hit): expected repr BRep, got {:?}",
        entry.repr
    );
    assert!(
        !entry.realization.is_empty(),
        "second build (cache hit): realization id string must be non-empty"
    );
}
