//! Compiler tests for the E_AUTO_NOT_AT_BINDING_SITE semantic gate (task 3808, δ;
//! extended to TraitStaticCall and AdHocSelector in task 4143).
//!
//! Verifies that `ExprKind::Auto` in a FUNCTION-call argument position, a
//! TRAIT-STATIC-CALL argument position, or an AD-HOC-SELECTOR argument position
//! emits `DiagnosticCode::AutoNotAtBindingSite`, while `auto` in a
//! STRUCTURE-construction named-arg position is silently accepted (structure ctors
//! are binding sites).
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
//! Step 3 (RED): Tests (f)/(g)/(h) add TraitStaticCall positions. Until step 4
//! wires up the gate for the `ExprKind::TraitStaticCall` arm via the shared
//! `reject_auto_in_arg_list` helper, the `auto` arg silently compiles to Undef
//! and zero gate errors are produced.
//!
//! Step 4 (GREEN): The shared helper is added, the FunctionCall arm refactored to
//! call it, and the TraitStaticCall arm gains the gate. Tests (f)/(g)/(h) pass.
//!
//! Step 5 (RED): Tests (i)/(j)/(k) add AdHocSelector positions. Until step 6
//! wires up the gate for the `ExprKind::AdHocSelector` arm, the `auto` arg
//! silently compiles to Undef via the catch-all arm — zero gate errors.
//!
//! Step 6 (GREEN): The AdHocSelector arm gains the gate via the same shared
//! helper. Tests (i)/(j)/(k) pass.
//!
//! ## Source convention
//!
//! The stdlib does not define `clamp`; the tests provide their own single-param
//! user function to avoid any ambiguity with stdlib overloads.  The structure
//! `Bolt` is defined inline to keep tests self-contained.
//!
//! The `Defaultable` trait fixture declares `make_default(x: Real) -> Real { x }`;
//! using a different name from the zero-param variant in trait_assoc_fn_static_tests
//! avoids any name-collision ambiguity.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// User-defined one-param function used in function-call tests.
/// No stdlib `clamp` exists; using a custom name avoids any overload ambiguity.
const CLAMP_FN: &str = "fn clamp(x: Length) -> Length = x";

/// Structure with a single `param` used in the structure-construction guard test.
const BOLT_STRUCTURE: &str = "structure Bolt { param length : Length = 10mm }";

/// Trait with a one-param static function used in TraitStaticCall tests.
/// Distinct from the zero-param `make_default()` in trait_assoc_fn_static_tests.rs.
const DEFAULTABLE_TRAIT: &str =
    "trait Defaultable { fn make_default(x: Real) -> Real { x } }";

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (a) `clamp(x: auto)` — strict auto in a function-call argument.
///
/// Must emit **exactly one** diagnostic with `code == AutoNotAtBindingSite`,
/// `severity == Error`, and a message that contains the callee name `"clamp"`.
/// Total error count must also be 1 (the poison return suppresses cascading
/// type errors — anti-cascade contract from task-448/1912/1921).
#[test]
fn function_call_strict_auto_emits_auto_not_at_binding_site() {
    let source = format!("{CLAMP_FN}  structure S {{ let y = clamp(x: auto) }}");
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for `clamp(x: auto)`;\
         \n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for `clamp(x: auto)`;\
         \n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
    assert!(
        first.message.contains("clamp"),
        "expected error message to name the callee 'clamp'; got: {:?}",
        first.message
    );
}

/// (b) `clamp(x: auto(free))` — free auto in a function-call argument.
///
/// Both strict and free `auto` are invalid at a function-call argument site.
/// Must emit **exactly one** `AutoNotAtBindingSite` error with `severity == Error`
/// and message containing `"clamp"`.  Total error count must also be 1
/// (anti-cascade contract — same as test (a)).
#[test]
fn function_call_free_auto_emits_auto_not_at_binding_site() {
    let source = format!("{CLAMP_FN}  structure S {{ let y = clamp(x: auto(free)) }}");
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for `clamp(x: auto(free))`;\
         \n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for `clamp(x: auto(free))`;\
         \n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
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

/// (e) Multi-auto-arg: `two_auto(a: auto, b: auto)` — only the FIRST offending arg
/// is reported (anti-cascade contract).
///
/// The gate uses `.find()` to locate the first `ExprKind::Auto` and emits a single
/// poison literal; the second `auto` is never reached.  Asserting `len() == 1`
/// locks in that invariant so a future change to iterate over all offending args
/// (breaking the anti-cascade contract) would be caught here.
///
/// The diagnostic label span is also verified to point at the first `auto` arg —
/// i.e. its start offset is less than the byte offset of the second `auto` in the
/// source string.
#[test]
fn function_call_multi_auto_reports_only_first_arg() {
    let source = "fn two_auto(a: Length, b: Length) -> Length = a  \
         structure S { let y = two_auto(a: auto, b: auto) }"
        .to_string();
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for two-auto call (anti-cascade);\
         \n  gate errors: {:?}",
        gate_errors
    );

    // Verify the diagnostic label points at the FIRST `auto` arg (before `b: auto`).
    let second_auto_offset =
        source.rfind("b: auto").expect("source must contain 'b: auto'") + "b: ".len();
    let label_start = gate_errors[0]
        .labels
        .first()
        .expect("gate diagnostic must carry a label")
        .span
        .start as usize;
    assert!(
        label_start < second_auto_offset,
        "label should point at the first `auto` arg (offset < {});\
         \n  label span start: {}, second `auto` starts at: {}",
        second_auto_offset,
        label_start,
        second_auto_offset,
    );
}

// ── TraitStaticCall gate tests (step 3/4) ────────────────────────────────────

/// (f) `Defaultable::make_default(x: auto)` — strict auto in a trait-static-call arg.
///
/// Must emit **exactly one** `AutoNotAtBindingSite` error with `severity == Error`
/// and a message that names the position (e.g. contains `"make_default"`).
/// Total error count must also be 1 (anti-cascade contract).
///
/// RED until step 4: today the `auto` arg silently compiles to Undef via the
/// catch-all `ExprKind::Auto` arm (expr.rs:2773) — zero gate errors produced.
#[test]
fn trait_static_call_strict_auto_emits_auto_not_at_binding_site() {
    let source = format!(
        "{DEFAULTABLE_TRAIT}  structure S {{ let y = Defaultable::make_default(x: auto) }}"
    );
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for \
         `Defaultable::make_default(x: auto)`;\n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for \
         `Defaultable::make_default(x: auto)`;\n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
    assert!(
        first.message.contains("make_default"),
        "expected error message to name the callee 'make_default'; got: {:?}",
        first.message
    );
}

/// (g) `Defaultable::make_default(x: auto(free))` — free auto in a trait-static-call arg.
///
/// Both strict and free `auto` are invalid at a trait-static-call argument site.
/// Must emit **exactly one** `AutoNotAtBindingSite` error with `severity == Error`
/// and message containing `"make_default"`.  Total error count must also be 1
/// (anti-cascade contract — same as test (f)).
///
/// RED until step 4: same silent-accept defect as (f).
#[test]
fn trait_static_call_free_auto_emits_auto_not_at_binding_site() {
    let source = format!(
        "{DEFAULTABLE_TRAIT}  structure S {{ let y = Defaultable::make_default(x: auto(free)) }}"
    );
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for \
         `Defaultable::make_default(x: auto(free))`;\n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for \
         `Defaultable::make_default(x: auto(free))`;\n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
    assert!(
        first.message.contains("make_default"),
        "expected error message to name the callee 'make_default'; got: {:?}",
        first.message
    );
}

/// (h) Non-auto control: `Defaultable::make_default(x: 1.0)` — no `auto` arg.
///
/// Must not emit any `AutoNotAtBindingSite` diagnostic.
#[test]
fn non_auto_trait_static_call_produces_no_gate_error() {
    let source = format!(
        "{DEFAULTABLE_TRAIT}  structure S {{ let y = Defaultable::make_default(x: 1.0) }}"
    );
    let module = compile_source_with_stdlib(&source);

    let gate_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        gate_errors.is_empty(),
        "unexpected AutoNotAtBindingSite diagnostic for plain \
         `Defaultable::make_default(x: 1.0)`;\n  gate diagnostics: {:?}",
        gate_errors
    );
}

// ── AdHocSelector gate tests (step 5/6) ──────────────────────────────────────

/// (i) `p @ face(x: auto)` — strict auto in an ad-hoc selector argument.
///
/// The `let p = 5mm` binding keeps `is_direct_port=false`, so the geometry-
/// availability check is skipped and base resolution succeeds.  The fixture is
/// fully valid except for the `auto` arg.
///
/// Must emit **exactly one** `AutoNotAtBindingSite` error with `severity == Error`
/// and a message that names the position (e.g. contains `"@face"`).
/// Total error count must also be 1 (anti-cascade contract).
///
/// RED until step 6: today the `auto` arg silently compiles to Undef via the
/// catch-all `ExprKind::Auto` arm — zero gate errors produced.
#[test]
fn ad_hoc_selector_strict_auto_emits_auto_not_at_binding_site() {
    let source =
        "structure S { let p = 5mm  let y = p @ face(x: auto) }".to_string();
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for `p @ face(x: auto)`;\
         \n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for `p @ face(x: auto)`;\
         \n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
    assert!(
        first.message.contains("@face"),
        "expected error message to name the selector '@face'; got: {:?}",
        first.message
    );
}

/// (j) `p @ face(x: auto(free))` — free auto in an ad-hoc selector argument.
///
/// Both strict and free `auto` are invalid at an ad-hoc selector argument site.
/// Must emit **exactly one** `AutoNotAtBindingSite` error with `severity == Error`
/// and message containing `"@face"`.  Total error count must also be 1
/// (anti-cascade contract — same as test (i)).
///
/// RED until step 6: same silent-accept defect as (i).
#[test]
fn ad_hoc_selector_free_auto_emits_auto_not_at_binding_site() {
    let source =
        "structure S { let p = 5mm  let y = p @ face(x: auto(free)) }".to_string();
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    let gate_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert_eq!(
        gate_errors.len(),
        1,
        "expected exactly one AutoNotAtBindingSite error for `p @ face(x: auto(free))`;\
         \n  gate errors: {:?}",
        gate_errors
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one total error (no cascading errors) for \
         `p @ face(x: auto(free))`;\n  all errors: {:?}",
        errors
    );

    let first = gate_errors[0];
    assert_eq!(
        first.severity,
        Severity::Error,
        "expected Error severity; got: {:?}",
        first.severity
    );
    assert!(
        first.message.contains("@face"),
        "expected error message to name the selector '@face'; got: {:?}",
        first.message
    );
}

/// (k) Non-auto control: `p @ face("top")` — ordinary ad-hoc selector without `auto`.
///
/// Must not emit any `AutoNotAtBindingSite` diagnostic.
#[test]
fn non_auto_ad_hoc_selector_produces_no_gate_error() {
    let source =
        "structure S { let p = 5mm  let y = p @ face(\"top\") }".to_string();
    let module = compile_source_with_stdlib(&source);

    let gate_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoNotAtBindingSite))
        .collect();

    assert!(
        gate_errors.is_empty(),
        "unexpected AutoNotAtBindingSite diagnostic for plain `p @ face(\"top\")`;\
         \n  gate diagnostics: {:?}",
        gate_errors
    );
}
