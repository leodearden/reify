//! Compiler-level tests for the PRELUDE `Result<T, E>` enum — task B-α #4035
//! (PRD docs/prds/v0_6/result-and-fallback.md §4.3/§8.B).
//!
//! `Result` is declared as a PRELUDE enum (`stdlib/result.ri`, registered in
//! `stdlib_loader::stdlib_sources()`) — a plain generic data-carrying enum
//! (grammar task α #4029, IR task β #4030, construction-inference task γ
//! #4031), NOT a Value intrinsic (PRD fork F4-a / D5). These tests drive
//! `Ok`/`Err` construction through the PRELUDE Result with NO inline
//! `enum Result` declaration in the test source — proving the enum is
//! prelude-resolvable without an import, and that task γ #4031's type-arg
//! inference / pinned-annotation payload check apply to a prelude enum
//! exactly as they do to a user-declared one (`enum_generic_construction_inference_tests.rs`'s
//! `RESULT_ENUM_SOURCE` fixture, which is byte-for-byte what `stdlib/result.ri`
//! declares).
//!
//! Uses `reify_test_support::compile_source_with_stdlib` (NOT the bare
//! `compile_source`) because the prelude Result only exists via
//! `compile_with_stdlib` — mirrors `option_recovery_resolution_tests.rs`.
//!
//! RED (before step-2 adds `stdlib/result.ri` + registers it in
//! `stdlib_loader::stdlib_sources()`): the prelude has no enum declaring
//! `Ok`/`Err`, so every construction below poisons with `"unknown variant
//! '...': no enum in scope declares it"`, and (a)'s `find()` over
//! `load_stdlib()` returns `None`.

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::compile_source_with_stdlib;

// ── helpers (mirror enum_generic_construction_inference_tests.rs) ──────────

/// Collect the codes of `module`'s Error-severity diagnostics (used to render
/// a helpful message when a `has_error_code` assertion fails).
fn error_codes(module: &CompiledModule) -> Vec<Option<DiagnosticCode>> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

/// True if `module` carries at least one Error-severity diagnostic carrying
/// `code`.
fn has_error_code(module: &CompiledModule, code: DiagnosticCode) -> bool {
    module
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// Locate the compiled default/init expr of the named value cell (`param` or
/// `let`) on the `Widget` structure's `TopologyTemplate`.
fn widget_value_cell<'a>(module: &'a CompiledModule, member: &str) -> &'a CompiledExpr {
    let widget = module
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should be present in module.templates");
    let cell = widget
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("Widget should declare a '{member}' value cell"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{member}' value cell should carry a compiled default_expr"))
}

/// Build a `structure def` source whose single param `r : <annotation>`
/// defaults to the given construction expression — drives the pinned-
/// annotation expected-type seam (the param-default site) against the
/// PRELUDE Result, with NO inline `enum Result` prepended.
fn result_param_source(annotation: &str, construction: &str) -> String {
    format!("structure def Widget {{\n    param r : {annotation} = {construction}\n}}\n")
}

// ── (a) std/result module is registered and loads clean ────────────────────

/// The `std/result` module must be registered in the stdlib loader and
/// compile with zero Severity::Error diagnostics.
///
/// RED: `std/result` does not exist yet — the `find` returns None.
#[test]
fn result_module_loads_clean() {
    let stdlib = reify_compiler::stdlib_loader::load_stdlib();
    let module = stdlib
        .iter()
        .find(|m| m.path.to_string() == "std/result")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/result module; available paths: {:?}",
                stdlib
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in result.ri: {:?}",
        errors
    );
}

// ── (b) unannotated Ok{value:5mm} infers cleanly against the prelude Result ─

/// [CORE SIGNAL] `Ok { value: 5mm }` with NO inline `enum Result` and NO
/// pinned annotation must check clean — the PRELUDE Result is in scope, and
/// task γ #4031's payload-driven inference binds `T = Length` from the
/// supplied `5mm`. The `r` value cell's compiled default_expr must be a
/// clean `Literal(Value::Enum { type_name: "Result", variant: "Ok", .. })`
/// carrying the `value` payload field — proving construction actually
/// assembled a value rather than merely suppressing an error.
///
/// RED: the prelude has no `Result` enum → `Ok` is an unresolved variant name
/// → poisoned to `Value::Undef` with an "unknown variant" Error diagnostic.
#[test]
fn prelude_ok_construction_infers_cleanly_and_builds_enum_value() {
    let source = "structure def Widget {\n    let r = Ok { value: 5mm }\n}\n";
    let module = compile_source_with_stdlib(source);

    assert!(
        error_codes(&module).is_empty(),
        "Ok {{ value: 5mm }} against the PRELUDE Result (no inline enum Result declared) \
         should produce ZERO Error diagnostics; got {:?}",
        error_codes(&module)
    );

    let expr = widget_value_cell(&module, "r");
    assert_eq!(
        expr.result_type,
        Type::Enum("Result".to_string()),
        "'r' should be typed Type::Enum(\"Result\"), got {:?}",
        expr.result_type
    );
    match &expr.kind {
        CompiledExprKind::Literal(Value::Enum {
            type_name,
            variant,
            payload,
        }) => {
            assert_eq!(type_name, "Result", "enum type_name");
            assert_eq!(variant, "Ok", "constructed variant");
            assert_eq!(
                payload.len(),
                1,
                "Ok payload should carry exactly the 'value' field, got {:?}",
                payload
            );
            assert_eq!(payload[0].0, "value", "payload field name");
            match &payload[0].1 {
                Value::Scalar { si_value, dimension } => {
                    assert!(
                        (*si_value - 0.005).abs() < 1e-9,
                        "5mm should carry si_value 0.005 (meters), got {si_value}"
                    );
                    assert_eq!(
                        *dimension,
                        reify_core::DimensionVector::LENGTH,
                        "5mm payload should carry the Length dimension, got {:?}",
                        dimension
                    );
                }
                other => panic!(
                    "expected Value::Scalar for the 'value' payload field, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected Literal(Value::Enum {{ Ok }}), got {:?}", other),
    }
}

// ── (c) unannotated Err{error:"bad"} checks clean against the prelude Result ─

/// `Err { error: "bad" }` with NO inline `enum Result` must check clean and
/// compile to a `Literal(Value::Enum { type_name: "Result", variant: "Err",
/// .. })` carrying the `error` payload field.
///
/// RED: the prelude has no `Result` enum → `Err` is an unresolved variant
/// name → poisoned with an "unknown variant" Error diagnostic.
#[test]
fn prelude_err_construction_checks_clean_and_builds_enum_value() {
    let source = "structure def Widget {\n    let e = Err { error: \"bad\" }\n}\n";
    let module = compile_source_with_stdlib(source);

    assert!(
        error_codes(&module).is_empty(),
        "Err {{ error: \"bad\" }} against the PRELUDE Result (no inline enum Result declared) \
         should produce ZERO Error diagnostics; got {:?}",
        error_codes(&module)
    );

    let expr = widget_value_cell(&module, "e");
    assert_eq!(
        expr.result_type,
        Type::Enum("Result".to_string()),
        "'e' should be typed Type::Enum(\"Result\"), got {:?}",
        expr.result_type
    );
    match &expr.kind {
        CompiledExprKind::Literal(Value::Enum {
            type_name,
            variant,
            payload,
        }) => {
            assert_eq!(type_name, "Result", "enum type_name");
            assert_eq!(variant, "Err", "constructed variant");
            assert_eq!(
                payload.len(),
                1,
                "Err payload should carry exactly the 'error' field, got {:?}",
                payload
            );
            assert_eq!(payload[0].0, "error", "payload field name");
            assert_eq!(
                payload[0].1,
                Value::String("bad".to_string()),
                "'error' payload should carry the String value \"bad\", got {:?}",
                payload[0].1
            );
        }
        other => panic!("expected Literal(Value::Enum {{ Err }}), got {:?}", other),
    }
}

// ── (d) pinned annotation mismatch against the prelude Result ──────────────

/// [CORE SIGNAL] A PINNED enum annotation (`Result<Force, String>`) on a
/// `param` default must OVERRIDE payload-driven inference for the
/// type-param-aware payload-type check, exactly as it does for a
/// user-declared `Result` (task γ #4031). `Ok { value: 5mm }` supplies a
/// Length-typed payload for the `TypeParam("T")` field, but the annotation
/// pins `T = Force`, so the substituted declared type (`Force`) disagrees
/// with the supplied `Length` and must be flagged as
/// `DiagnosticCode::VariantPayloadType`.
///
/// RED: the prelude has no `Result` enum → `Ok` is an unresolved variant name
/// → poisoned with an "unknown variant" Error, not `VariantPayloadType`.
#[test]
fn pinned_annotation_mismatch_against_prelude_result_emits_variant_payload_type() {
    let source = result_param_source("Result<Force, String>", "Ok { value: 5mm }");
    let module = compile_source_with_stdlib(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "param r : Result<Force, String> = Ok {{ value: 5mm }} against the PRELUDE Result \
         pins T=Force, but field 'value' supplies Length -> expected VariantPayloadType; got \
         error codes {:?}",
        error_codes(&module)
    );
}

// ── (e) pinned annotation match against the prelude Result (regression) ────

/// The same pinned-annotation seam as (d), but with the supplied payload
/// MATCHING the pinned arg (`Result<Length, String>` + `value: 5mm`) — must
/// check clean (ZERO Error diagnostics). Regression guard alongside (d) so a
/// fix that unconditionally flags every pinned `Result<..>` construction
/// (rather than only a genuine mismatch) is caught.
///
/// RED: the prelude has no `Result` enum → `Ok` is an unresolved variant name
/// → poisoned with an "unknown variant" Error diagnostic.
#[test]
fn pinned_annotation_match_against_prelude_result_checks_clean() {
    let source = result_param_source("Result<Length, String>", "Ok { value: 5mm }");
    let module = compile_source_with_stdlib(&source);
    assert!(
        error_codes(&module).is_empty(),
        "param r : Result<Length, String> = Ok {{ value: 5mm }} against the PRELUDE Result \
         pins T=Length, matching the supplied Length payload -> expected ZERO Error \
         diagnostics; got {:?}",
        error_codes(&module)
    );
}

// ── (f) pinned annotation mismatch against the prelude Result (E axis) ─────

/// The pinned-annotation payload-type check exercised on the SECOND type
/// param (`E`), symmetric to (d)'s `T` case: `Result<Length, Force>` pins
/// `E = Force`, but `Err { error: "bad" }` supplies a `String`-typed payload
/// for the `TypeParam("E")` field, so the substituted declared type
/// (`Force`) disagrees with the supplied `String` and must be flagged as
/// `DiagnosticCode::VariantPayloadType`. Confirms the second type param
/// substitutes correctly through the prelude enum too.
///
/// RED: the prelude has no `Result` enum → `Err` is an unresolved variant
/// name → poisoned with an "unknown variant" Error, not `VariantPayloadType`.
#[test]
fn pinned_annotation_mismatch_against_prelude_result_emits_variant_payload_type_on_err_axis() {
    let source = result_param_source("Result<Length, Force>", "Err { error: \"bad\" }");
    let module = compile_source_with_stdlib(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "param r : Result<Length, Force> = Err {{ error: \"bad\" }} against the PRELUDE Result \
         pins E=Force, but field 'error' supplies String -> expected VariantPayloadType; got \
         error codes {:?}",
        error_codes(&module)
    );
}

// ── (g) a LOCAL `enum Result` coexists with the PRELUDE Result ─────────────

/// [DESIGN COHERENCE] `resolution_enums` is built as `prelude ++ local`
/// (`enums_phase::build_resolution_enums_from_cache`), with NO dedup and NO
/// duplicate-enum diagnostic — `shadow_lint` is Warning-only and explicitly
/// skips `Declaration::Enum` (see its module doc), and `reserved_name_lint`
/// only fires against BUILTIN type names (`resolve_type_name`), which
/// `"Result"` is not. Variant-name resolution in `compile_variant_construct`
/// is a first-match linear scan over that concatenated list
/// (`enum_defs.iter().find_map(...)`), so when a LOCAL module ALSO declares
/// `enum Result<T, E>`, any variant name the PRELUDE already defines (`Ok`,
/// `Err`) resolves against the PRELUDE first — the local declaration is
/// shadowed BY the prelude, not the other way around.
///
/// This test pins that (perhaps surprising) resolution order as a
/// deliberate, reviewed regression guard rather than a silent internal
/// collision. The local `Result` below gives its `Ok`/`Err` variants
/// DIFFERENT field names (`payload`/`failure` instead of `value`/`error`)
/// specifically so the two enums are distinguishable at the construction
/// site:
///
/// 1. Compiling the local `enum Result` alongside the stdlib prelude, and
///    constructing `Ok { value: 5mm }` (the PRELUDE's field name), produces
///    ZERO Error diagnostics — no duplicate-enum / shadow error, and the
///    construction resolves against *some* `Result` enum cleanly.
/// 2. The constructed value's payload field name is `"value"` (the
///    PRELUDE's), not `"payload"` (the local declaration's) — confirming
///    the PRELUDE Result won first-match resolution, not the local one.
#[test]
fn local_result_enum_coexists_with_prelude_result_and_prelude_wins_first_match() {
    let source = "enum Result<T, E> {\n    Ok { payload: T },\n    Err { failure: E },\n}\n\n\
                  structure def Widget {\n    let r = Ok { value: 5mm }\n}\n";
    let module = compile_source_with_stdlib(source);

    assert!(
        error_codes(&module).is_empty(),
        "a local `enum Result` alongside the PRELUDE Result, constructed via the PRELUDE's \
         field name ('value'), should produce ZERO Error diagnostics -- the two same-named \
         enums must coexist without a duplicate-enum/shadow error; got {:?}",
        error_codes(&module)
    );

    let expr = widget_value_cell(&module, "r");
    match &expr.kind {
        CompiledExprKind::Literal(Value::Enum { payload, .. }) => {
            assert_eq!(
                payload[0].0, "value",
                "the PRELUDE Result's 'Ok {{ value }}' variant should win first-match \
                 resolution over the local 'Ok {{ payload }}' variant (resolution_enums = \
                 prelude ++ local), got payload field {:?}",
                payload[0].0
            );
        }
        other => panic!("expected Literal(Value::Enum {{ Ok }}), got {:?}", other),
    }
}
