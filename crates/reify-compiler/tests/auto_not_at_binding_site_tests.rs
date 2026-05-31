//! Compiler tests for the E_AUTO_NOT_AT_BINDING_SITE semantic gate (task 3808, δ).
//!
//! Verifies that `ExprKind::Auto` in a FUNCTION-call argument position emits
//! `DiagnosticCode::AutoNotAtBindingSite`, while `auto` in a STRUCTURE-construction
//! named-arg position is silently accepted (structure ctors are binding sites).
//!
//! ## The RED → GREEN arc
//!
//! Step 1 (RED): These tests compile source that uses `auto` in a function-call
//! argument (`clamp(x: auto)`).  Until step 2 wires up the gate in
//! `crates/reify-compiler/src/expr.rs`, no `AutoNotAtBindingSite` diagnostic is
//! emitted — so the function-rejection assertions fail, confirming the tests are
//! genuinely RED.
//!
//! Step 2 (GREEN): The gate at the top of the `ExprKind::FunctionCall` arm emits
//! `DiagnosticCode::AutoNotAtBindingSite` for non-structure callees, while
//! structure constructors fall through unchanged.  After that, all four assertions
//! below pass.
//!
//! ## Source convention
//!
//! The stdlib does not define `clamp`; the tests provide their own single-param
//! user function to avoid any ambiguity with stdlib overloads.  The structure
//! `Bolt` is defined inline to keep tests self-contained.

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// User-defined one-param function used in function-call tests.
/// No stdlib `clamp` exists; using a custom name avoids any overload ambiguity.
const CLAMP_FN: &str = "fn clamp(x: Length) -> Length = x";

/// Structure with a single `param` used in the structure-construction guard test.
const BOLT_STRUCTURE: &str = "structure Bolt { param length : Length = 10mm }";

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (a) `clamp(x: auto)` — strict auto in a function-call argument.
///
/// Must emit exactly one diagnostic with `code == AutoNotAtBindingSite`,
/// `severity == Error`, and a message that contains the callee name `"clamp"`.
#[test]
fn function_call_strict_auto_emits_auto_not_at_binding_site() {
    let source = format!("{CLAMP_FN}  structure S {{ let y = clamp(x: auto) }}");
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        !gate_errors.is_empty(),
        "expected at least one AutoNotAtBindingSite error for `clamp(x: auto)`;\
         \n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert!(
        first.message.contains("clamp"),
        "expected error message to name the callee 'clamp'; got: {:?}",
        first.message
    );
}

/// (b) `clamp(x: auto(free))` — free auto in a function-call argument.
///
/// Both strict and free `auto` are invalid at a function-call argument site.
/// Must emit exactly one `AutoNotAtBindingSite` error with message containing "clamp".
#[test]
fn function_call_free_auto_emits_auto_not_at_binding_site() {
    let source = format!("{CLAMP_FN}  structure S {{ let y = clamp(x: auto(free)) }}");
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        !gate_errors.is_empty(),
        "expected at least one AutoNotAtBindingSite error for `clamp(x: auto(free))`;\
         \n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert!(
        first.message.contains("clamp"),
        "expected error message to name the callee 'clamp'; got: {:?}",
        first.message
    );
}

/// (c) Boundary guard: `Bolt(length: auto)` — structure-construction named arg.
///
/// Structure construction is a BINDING SITE for `auto`; the gate must NOT fire.
/// The test asserts the ABSENCE of any `AutoNotAtBindingSite` diagnostic,
/// staying robust to unrelated ε-deferred diagnostics on unresolved determinacy.
#[test]
fn structure_construction_auto_is_not_rejected() {
    let source = format!(
        "{BOLT_STRUCTURE}  structure S {{ let y = Bolt(length: auto) }}"
    );
    let module = compile_source_with_stdlib(&source);

    let gate_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        gate_errors.is_empty(),
        "unexpected AutoNotAtBindingSite diagnostic for structure construction \
         `Bolt(length: auto)` — structure ctors are binding sites, not call sites;\
         \n  gate diagnostics: {:?}",
        gate_errors
    );
}

/// (d) Non-auto control: `clamp(x: 5mm)` — ordinary function call without `auto`.
///
/// Must not emit any `AutoNotAtBindingSite` diagnostic.
#[test]
fn non_auto_function_call_produces_no_gate_error() {
    let source = format!("{CLAMP_FN}  structure S {{ let y = clamp(x: 5mm) }}");
    let module = compile_source_with_stdlib(&source);

    let gate_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        gate_errors.is_empty(),
        "unexpected AutoNotAtBindingSite diagnostic for plain `clamp(x: 5mm)`;\
         \n  gate diagnostics: {:?}",
        gate_errors
    );
}
