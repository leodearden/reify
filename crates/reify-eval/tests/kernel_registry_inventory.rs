//! End-to-end pin for the cross-crate v0.2 multi-kernel inventory plumbing.
//!
//! This test binary's compile closure includes `reify-kernel-occt` (declared
//! in `crates/reify-eval/Cargo.toml:25-31` as a `[dev-dependencies]` entry),
//! so when OCCT is available the adapter's `inventory::submit!` (added in
//! task 2642 step 8) fires here and the registration appears in
//! `reify_eval::collect_registry()`.
//!
//! Pin scope: the chain `KernelRegistration in reify-types` →
//! `inventory::submit! in reify-kernel-occt` → `inventory::iter +
//! collect_registry in reify-eval`. A regression in any of those layers
//! (missing module declaration, wrong `cfg` gate, type mismatch on the
//! collect target) would break this test even though each crate's own
//! unit / integration tests stay green.

use reify_types::{ExportFormat, ModulePath, Operation, ReprKind};

/// `collect_registry()` must surface the OCCT submission with a descriptor
/// that supports `(PrimitiveBox, BRep)` — a minimal proof that the
/// inventory plumbing is wired end-to-end across the three crates.
///
/// Skipped in stub mode: with `cfg(has_occt)` off, the OCCT submit
/// doesn't fire, so the registry is correctly empty and there's nothing
/// to assert.
#[test]
fn collect_registry_finds_occt_entry_with_brep_primitive_support() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        return;
    }

    let registry = reify_eval::collect_registry();

    let occt = registry.get("occt").expect(
        "collect_registry() must contain key \"occt\" once reify-kernel-occt's \
         inventory::submit! fires (gated on cfg(has_occt))",
    );

    assert!(
        occt.supports(Operation::PrimitiveBox, ReprKind::BRep),
        "the OCCT entry materialised by collect_registry() must declare \
         (PrimitiveBox, BRep) — caught a divergence between the inventory \
         submission's descriptor and the direct `register::occt_capability_descriptor()`",
    );
}

/// End-to-end pin for `Engine::with_registered_kernel`: the inventory-driven
/// constructor must read the static-collected `KernelRegistration` set once,
/// instantiate the OCCT kernel via the registered factory, and produce real
/// geometry output for a trivial BRep box build.
///
/// Pin scope (the round-trip): `inventory::submit!` (in
/// reify-kernel-occt) → `collect_registry()` (in reify-eval) →
/// `Engine::with_registered_kernel` constructor → factory invocation →
/// `OcctKernelHandle::spawn()` → `engine.build(..., ExportFormat::Stl)` →
/// non-empty `geometry_output`. A regression in any link breaks this test.
///
/// Skipped in stub mode: with `cfg(has_occt)` off the registry is empty
/// and the constructor would forward `None` for the kernel, so the build
/// path can't produce geometry — there is nothing meaningful to assert.
#[test]
fn engine_with_registered_kernel_picks_occt_for_brep_box_build() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        return;
    }

    // Tiny single-realization source: `let b = box(10mm, 10mm, 10mm)`.
    // Positional args match the existing `box(...)` call convention used
    // throughout `crates/reify-eval/tests/boolean_ops_e2e.rs`; `box`'s
    // signature is `fn box(width: Length, depth: Length, height: Length)`.
    let source = r#"structure S {
    let b = box(10mm, 10mm, 10mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("registered_kernel_box"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // The new inventory-driven constructor: no manual `OcctKernelHandle::spawn()`
    // call here — that wiring is the contract under test.
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));

    let result = engine.build(&compiled, ExportFormat::Stl);

    let output = result
        .geometry_output
        .expect("Engine::with_registered_kernel must instantiate the registered OCCT factory and produce STL output");
    assert!(
        !output.is_empty(),
        "STL geometry_output must be non-empty — empty output indicates the registered kernel \
         was not actually instantiated and execute() was never called",
    );
}
