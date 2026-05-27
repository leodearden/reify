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

use reify_test_support::CountingSubscriberBuilder;
use reify_core::ModulePath;
use reify_ir::{ExportFormat, Operation, ReprKind};
use std::sync::atomic::Ordering;

/// `collect_registry()` must surface the OCCT submission with a descriptor
/// that supports `(PrimitiveBox, BRep)` — a minimal proof that the
/// inventory plumbing is wired end-to-end across the three crates.
///
/// Skipped in stub mode: with `cfg(has_occt)` off, the OCCT submit
/// doesn't fire, so the registry is correctly empty and there's nothing
/// to assert. The skip is announced via `eprintln!` so stub-mode CI
/// produces an observable signal — silent no-op early-returns would let a
/// regression that drops the OCCT submit (e.g. an accidental cfg gate
/// widening) hide in green logs on dev machines where OCCT *is* available
/// while CI silently skips.
#[test]
fn collect_registry_finds_occt_entry_with_brep_primitive_support() {
    // Always-runnable shape pin (independent of OCCT availability): the
    // function returns the documented type and is callable without panic.
    // This catches regressions to `collect_registry`'s signature or its
    // inner inventory walk even in stub-mode CI builds, where the OCCT-
    // specific assertions below are skipped.
    let _shape_pin: std::collections::BTreeMap<String, reify_ir::CapabilityDescriptor> =
        reify_eval::collect_registry();

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping collect_registry_finds_occt_entry_with_brep_primitive_support: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
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
        eprintln!(
            "skipping engine_with_registered_kernel_picks_occt_for_brep_box_build: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
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
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // The new inventory-driven constructor: no manual `OcctKernelHandle::spawn()`
    // call here — that wiring is the contract under test.
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));

    // STEP rather than STL: OCCT's `OcctKernelHandle::export` returns
    // `unsupported export format: Stl` for `ExportFormat::Stl` (see
    // `export_unsupported_format_returns_error` in
    // `crates/reify-kernel-occt/src/handle.rs:1301`). STEP is the only
    // BRep-native export format OCCT supports; the round-trip pin needs
    // a format that actually round-trips, so we use STEP and rely on the
    // `> 0 bytes` assertion below to confirm the registered factory was
    // invoked and the kernel produced real output.
    let result = engine.build(&compiled, ExportFormat::Step);

    let output = result.geometry_output.unwrap_or_else(|| {
        panic!(
            "Engine::with_registered_kernel must instantiate the registered OCCT factory \
             and produce STEP output. Diagnostics: {:?}\nrealizations on first template: {}",
            result.diagnostics,
            compiled.templates[0].realizations.len(),
        )
    });
    assert!(
        !output.is_empty(),
        "STEP geometry_output must be non-empty — empty output indicates the registered kernel \
         was not actually instantiated and execute() was never called",
    );
}

/// Integration pin for the tracing emission from `Engine::with_registered_kernel`:
/// the constructor must fire exactly one selection event (INFO when multiple
/// kernels are registered, DEBUG when only one is) per call at the
/// `reify_eval::kernel_registry` target.
///
/// Asserts `info_count + debug_count == 1` rather than pinning the specific
/// level, so that a v0.3+ build registering a second kernel adapter (which
/// causes INFO to fire instead of DEBUG) does not trigger a spurious failure
/// here. The unit tests in `kernel_registry.rs` already cover the branch logic
/// exhaustively with synthetic inputs.
///
/// Skipped in stub mode: with `cfg(has_occt)` off the registry is empty and
/// `with_registered_kernel` forwards `None` for the kernel — the selection
/// helper is not reached, so there is nothing to assert. The skip is announced
/// via `eprintln!` so stub-mode CI produces an observable signal.
#[test]
fn engine_with_registered_kernel_emits_one_selection_event() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping engine_with_registered_kernel_emits_one_selection_event: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::INFO)
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_eval::kernel_registry")
        .build();
    let info_count = counters[&tracing::Level::INFO].clone();
    let debug_count = counters[&tracing::Level::DEBUG].clone();

    tracing::subscriber::with_default(subscriber, || {
        let checker = reify_constraints::SimpleConstraintChecker;
        let _engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    });

    let info_c = info_count.load(Ordering::Acquire);
    let debug_c = debug_count.load(Ordering::Acquire);
    assert_eq!(
        info_c + debug_c,
        1,
        "Engine::with_registered_kernel must emit exactly one selection event at \
         reify_eval::kernel_registry per construction (INFO when registry().len() > 1, \
         DEBUG when == 1). Got: info={info_c}, debug={debug_c}",
    );
}
