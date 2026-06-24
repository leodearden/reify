//! Integration tests for task η 3945 (trait-static fn dispatch).
//!
//! ## Step-1 / Step-2 (producer): registration in `traits_phase`
//!
//! A trait's body-carrying static (no-`self`) assoc fn must be compiled and
//! registered as a namespaced `CompiledFunction` named `"Trait::method"` in
//! `CompiledModule.functions` at the end of `phase_traits`.
//!
//! ## Step-3 / Step-4 (consumer): `TraitStaticCall` dispatch arm
//!
//! `Trait::fn(args)` inside a structure body must lower to a
//! `CompiledExprKind::UserFunctionCall { function_name: "Trait::fn", .. }`,
//! producing no Error diagnostics.

use reify_core::{DiagnosticCode, Severity};
use reify_ir::CompiledExprKind;
use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only, warnings_only};

// ── Step-1 producer tests (RED until step-2) ────────────────────────────────

/// (a) POSITIVE registration: a module with a trait declaring a static (no-self)
/// body-carrying assoc fn compiles diagnostic-clean, and the resulting
/// `CompiledModule.functions` contains a `CompiledFunction` whose
/// `name == "Defaultable::make_default"` with 0 non-self params.
///
/// RED today: `traits_phase` never compiles trait fn bodies, so the namespaced
/// fn is absent from `ctx.functions`.
#[test]
fn static_assoc_fn_registered_in_module_functions() {
    let source = r#"
trait Defaultable {
    fn make_default() -> Real { 1.0 }
}
"#;
    let compiled = compile_source(source);

    // Must be diagnostic-clean.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors
    );

    // The namespaced function must appear in the module's function table.
    let found = compiled
        .functions
        .iter()
        .any(|f| f.name == "Defaultable::make_default" && f.params.is_empty());
    assert!(
        found,
        "expected 'Defaultable::make_default' (0 params) in module functions; \
         functions present: {:?}",
        compiled.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// (b) NEGATIVE member-ref: a static fn body that references a trait member
/// (which is not in scope during compilation of a neutral fn body) must yield
/// an `UnresolvedName` diagnostic naming the offending member.
///
/// RED today: trait fn bodies are never compiled (`compile_trait` only stores
/// the `FnDef`), so no error fires for the member reference.
#[test]
fn static_assoc_fn_body_referencing_trait_member_errors() {
    let source = r#"
trait Bad {
    fn make_bad() -> Real { diameter }
}
"#;
    let compiled = compile_source(source);
    let errors = errors_only(&compiled);

    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for unresolved 'diameter'; \
         got none. All diagnostics: {:?}",
        compiled.diagnostics
    );

    let unresolved_naming_diameter = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::UnresolvedName) && d.message.contains("diameter")
    });
    assert!(
        unresolved_naming_diameter,
        "expected an UnresolvedName diagnostic mentioning 'diameter'; \
         errors: {:?}",
        errors.iter().map(|d| (&d.code, &d.message)).collect::<Vec<_>>()
    );
}

// ── Step-3 consumer tests (RED until step-4) ─────────────────────────────────

/// (a) POSITIVE lowering: `Trait::fn()` inside a structure body lowers to a
/// `UserFunctionCall` with `function_name == "Defaultable::make_default"` and
/// produces no Error diagnostics.
///
/// RED today: the `TraitStaticCall` arm is still the "not yet supported" poison
/// placeholder, so it emits an error and returns a poison expr.
#[test]
fn trait_static_call_lowers_to_user_function_call_in_structure_body() {
    let source = r#"
trait Defaultable {
    fn make_default() -> Real { 1.0 }
}
pub structure def Spacer {
    let gap : Real = Defaultable::make_default()
}
"#;
    let compiled = compile_source(source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors
    );

    // Find the `gap` let-cell and check its compiled expr is a UserFunctionCall.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Spacer")
        .expect("Spacer template not found");
    let gap_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "gap")
        .expect("gap cell not found in Spacer");
    let expr = gap_cell
        .default_expr
        .as_ref()
        .expect("gap cell has no expression");
    match &expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "Defaultable::make_default",
                "expected 'Defaultable::make_default', got '{function_name}'"
            );
        }
        other => panic!(
            "expected UserFunctionCall for gap, got: {:?}",
            other
        ),
    }
}

/// (b) UNKNOWN static fn: calling `C::make()` where there is no trait `C`
/// emits exactly one Error whose message does NOT contain "not yet supported"
/// but instead describes the unknown-static-fn situation.
///
/// RED today: the placeholder arm always emits "not yet supported".
#[test]
fn unknown_trait_static_call_emits_unknown_fn_diagnostic() {
    let source = r#"pub structure def A { let s : Real = C::make() }"#;
    let compiled = compile_source(source);
    let errors = errors_only(&compiled);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Error for unknown trait-static call; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The message must NOT contain "not yet supported" — that was the placeholder text.
    assert!(
        !errors[0].message.contains("not yet supported"),
        "placeholder 'not yet supported' text must be gone; got: {:?}",
        errors[0].message
    );

    // The message must reference the unknown call site.
    assert!(
        errors[0].message.contains("C") || errors[0].message.contains("make"),
        "diagnostic should name the trait or method; got: {:?}",
        errors[0].message
    );
}

// ── Stdlib-typed positive test (uses Length/mm) ───────────────────────────────

/// Positive compilation with a Length-returning static fn using stdlib types.
/// Mirrors the e2e example file (examples/trait_assoc_fn_static.ri).
///
/// RED until step-4 (dispatch arm).
#[test]
fn static_assoc_fn_with_stdlib_length_type_compiles_clean() {
    let source = r#"
trait Defaultable {
    fn make_default() -> Length { 10mm }
}
pub structure def Spacer {
    let gap : Length = Defaultable::make_default()
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics with Length type; got: {:?}",
        errors
    );
}

// ── Amendment: additional branch-coverage tests (reviewer suggestions) ────────

/// Calling a self-receiver (instance) method as a static call on a known trait
/// must produce the 'requires a receiver' diagnostic (not 'unknown trait').
///
/// This exercises the `scope.trait_members` refinement path where the trait is
/// known and the member name is found (but the fn was not registered as static
/// because it has a self param).
#[test]
fn instance_method_called_statically_emits_receiver_required_diagnostic() {
    let source = r#"
trait Shape {
    fn area(self) -> Real { 1.0 }
}
pub structure def Box {
    let s : Real = Shape::area()
}
"#;
    let compiled = compile_source(source);
    let errors = errors_only(&compiled);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Error; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    assert!(
        errors[0].message.contains("requires a receiver"),
        "expected 'requires a receiver' in message; got: {:?}",
        errors[0].message
    );

    // Must mention the trait and method to aid diagnosis.
    assert!(
        errors[0].message.contains("Shape") && errors[0].message.contains("area"),
        "expected trait 'Shape' and method 'area' in message; got: {:?}",
        errors[0].message
    );
}

/// Calling a non-existent method on a known trait must produce the
/// 'has no static function' diagnostic (not 'unknown trait').
///
/// This exercises the `scope.trait_members` refinement path where the trait is
/// known but the method name is absent from its members.
#[test]
fn known_trait_unknown_method_emits_no_static_function_diagnostic() {
    let source = r#"
trait Known {
    fn make_default() -> Real { 1.0 }
}
pub structure def Box {
    let s : Real = Known::nonexistent()
}
"#;
    let compiled = compile_source(source);
    let errors = errors_only(&compiled);

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Error; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    assert!(
        errors[0].message.contains("has no static function"),
        "expected 'has no static function' in message; got: {:?}",
        errors[0].message
    );

    // Must name both the trait and the missing method.
    assert!(
        errors[0].message.contains("Known") && errors[0].message.contains("nonexistent"),
        "expected 'Known' and 'nonexistent' in message; got: {:?}",
        errors[0].message
    );
}

/// The `TraitStaticCall` dispatch arm checks `matched_fn.annotations` for
/// `@deprecated` and emits a Warning if present.  This test verifies that the
/// guard does NOT fire when the trait fn carries no annotation — i.e. no
/// spurious deprecation warning is emitted for a clean (non-annotated) call.
///
/// Paired with `trait_static_fn_call_emits_deprecation_warning` (below) which
/// verifies the positive warning path for an `@deprecated`-annotated trait fn.
#[test]
fn trait_static_fn_call_emits_no_spurious_deprecation_warning() {
    let source = r#"
trait Factory {
    fn make_item() -> Real { 1.0 }
}
pub structure def Box {
    let s : Real = Factory::make_item()
}
"#;
    let compiled = compile_source(source);

    // Must compile with no errors.
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors
    );

    // No spurious deprecation warnings from the dispatch arm.
    let warns = warnings_only(&compiled);
    let deprecation_warns: Vec<_> = warns
        .iter()
        .filter(|d| d.message.contains("deprecated"))
        .collect();
    assert!(
        deprecation_warns.is_empty(),
        "expected no spurious deprecation warnings for non-annotated trait static fn; \
         got: {:?}",
        deprecation_warns
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Calling an `@deprecated`-annotated trait static fn emits exactly one
/// deprecation Warning whose message contains both "deprecated" and the
/// custom deprecation message string.
///
/// This is the positive counterpart to
/// `trait_static_fn_call_emits_no_spurious_deprecation_warning`.
///
/// RED after the grammar-only change (step-2): the source now parses and the
/// fn registers, but `lower_trait_members` drops the annotation so
/// `FnDef.annotations` is empty → `CompiledFunction.annotations` empty →
/// no warning emitted.
/// GREEN after the `lower_trait_members` annotation-attach change (step-4).
#[test]
fn trait_static_fn_call_emits_deprecation_warning() {
    let source = r#"
trait Factory {
    @deprecated("use make_new")
    fn make_old() -> Real { 1.0 }
}
pub structure def Box {
    let s : Real = Factory::make_old()
}
"#;
    let compiled = compile_source(source);

    // Must compile with no errors.
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors
    );

    // Must emit exactly one deprecation warning containing the custom message.
    let warns = warnings_only(&compiled);
    let deprecation_warns: Vec<_> = warns
        .iter()
        .filter(|d| d.message.contains("deprecated") && d.message.contains("use make_new"))
        .collect();
    assert_eq!(
        deprecation_warns.len(),
        1,
        "expected exactly one deprecation warning containing 'deprecated' and \
         'use make_new'; got {} warnings: {:?}",
        deprecation_warns.len(),
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A body-less `@deprecated` fn *signature* in a trait propagates through
/// `lower_trait_members` identically to a `function_definition` — the same
/// drain-and-attach path runs for both CST node kinds. However, the traits
/// phase skips bodyless fns at registration time (`fn_def.body.is_none()`
/// guard in traits_phase.rs), so no `CompiledFunction` is registered and no
/// call site can trigger a deprecation warning. This test pins the
/// "no unexpected errors or warnings from the definition alone" semantics.
///
/// The counterpart `trait_static_fn_call_emits_deprecation_warning` tests
/// the full warning path for body-carrying static fns.
#[test]
fn trait_fn_signature_with_deprecated_annotation_compiles_cleanly() {
    let source = r#"
trait LegacyApi {
    @deprecated("use LegacyApi::new_op")
    fn old_op() -> Real
}
"#;
    let compiled = compile_source(source);

    // The @deprecated annotation on a fn signature must not produce any errors.
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for trait with @deprecated fn signature; got: {:?}",
        errors
    );

    // No call site exists, so no deprecation warning should fire.
    let warns = warnings_only(&compiled);
    let dep_warns: Vec<_> = warns
        .iter()
        .filter(|d| d.message.contains("deprecated"))
        .collect();
    assert!(
        dep_warns.is_empty(),
        "expected no deprecation warnings for unused @deprecated fn signature (no call site); \
         got: {:?}",
        dep_warns
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
