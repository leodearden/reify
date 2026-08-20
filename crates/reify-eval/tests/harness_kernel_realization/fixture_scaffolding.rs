//! Neutral, fixture-agnostic scaffolding shared by pin tests in this harness:
//! read a `.ri` fixture, compile it against the stdlib, and optionally build
//! it with a real OCCT kernel (skipping cleanly when OCCT is absent).
//!
//! NOT a former standalone `tests/<file>.rs` and deliberately holds no
//! `#[test]` fn of its own — it is a support module, so nothing here shows up
//! as a `fixture_scaffolding::<test>` path in a nextest filterset.
//!
//! # Why this module exists (task #5982 amendment)
//!
//! The #5982 split originally left `best_practices_clearance_oracle` reaching
//! into its sibling via `use super::kernel_queries_intersects_smoke::{...}` —
//! the only `use super::<sibling module>` in any consolidated harness in this
//! repo. That made the KGQ-γ intersects pin the *owner* of generic
//! read/compile/build scaffolding for an unrelated fixture, so narrowing or
//! deleting that pin (e.g. when KGQ-γ coverage is reworked) would silently
//! break an unrelated `best_practices` pin. Hoisting the two helpers here
//! makes both consumers peers of a neutral unit instead: neither test module
//! owns the other's scaffolding, and a third consumer can join without
//! deepening the coupling.

use reify_constraints::SimpleConstraintChecker;
use reify_ir::ExportFormat;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Reads `path` and parses+compiles it against the stdlib, asserting it
/// compiles cleanly. Runs unconditionally — no OCCT dependency — so a
/// missing fixture or a grammar/compile regression fails on every runner.
///
/// Split out of `compile_and_build_with_occt` so the read/compile/
/// assert-clean scaffolding has exactly one implementation shared by every
/// pin test that uses it — including the kernel-less check-surface tests in
/// `best_practices_clearance_oracle`, which need a compiled fixture but never
/// touch OCCT — instead of the risk of two copies (one here, one inlined in a
/// kernel-less test) silently drifting under copy-paste.
///
/// `what` is the repo-relative fixture label used in failure messages, so an
/// offender is reported portably rather than as an absolute build-machine path.
pub(crate) fn read_and_compile_fixture(path: &str, what: &str) -> reify_compiler::CompiledModule {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{what} should exist at {path}: {e}"));

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "{what} should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    compiled
}

/// Compiles `path` via `read_and_compile_fixture`, then either builds it with
/// a real OCCT kernel and returns the `BuildResult`, or — when OCCT is not
/// available — emits the standard skip line (naming `what`) and returns
/// `None`, letting the caller early-return.
///
/// Shared so the real-OCCT pin tests in this harness cannot drift against each
/// other on the skip/build scaffolding — the piece most likely to diverge
/// silently under copy-paste.
pub(crate) fn compile_and_build_with_occt(
    path: &str,
    what: &str,
) -> Option<reify_eval::BuildResult> {
    let compiled = read_and_compile_fixture(path, what);

    // Skip the OCCT-dependent kernel build if OCCT is not built.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions for {what}: OCCT not available");
        return None;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}
