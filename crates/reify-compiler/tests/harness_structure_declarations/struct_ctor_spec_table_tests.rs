//! Pins for the rows of the construction-site legality table in
//! `docs/reify-language-spec.md` §4.9 that no other test pins.
//!
//! The remaining rows are pinned in `struct_ctor_field_conformance_tests.rs`
//! (this harness), `harness_traits/trait_typed_param_tests.rs`, and
//! `harness_diagnostics_robustness/param_binding_selector_coercion_tests.rs`.
//!
//! Conventions follow `struct_ctor_field_conformance_tests.rs`: inline sources
//! led by a `module test.<name>` line, counts narrowed to ctor-conformance
//! codes, and literal `Severity::Error` because the severity knob is
//! `pub(crate)`.

use reify_compiler::CompiledModule;
use reify_core::diagnostics::DiagnosticCode;
use reify_core::{Diagnostic, Severity};
use reify_test_support::{
    compile_source_with_stdlib, ctor_diagnostic_names_arg, errors_only, is_ctor_conformance_code,
};

fn ctor_conformance_diags(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

fn non_ctor_conformance_errors(module: &CompiledModule) -> Vec<&Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| !is_ctor_conformance_code(d.code))
        .collect()
}

fn assert_clean(module: &CompiledModule, what: &str) {
    let diags = ctor_conformance_diags(module);
    assert!(
        diags.is_empty(),
        "{what}: expected no ctor-conformance diagnostic, got: {diags:#?}"
    );
    let stray = non_ctor_conformance_errors(module);
    assert!(
        stray.is_empty(),
        "{what}: fixture must be otherwise well-formed, got errors: {stray:#?}"
    );
}

fn assert_single_error<'m>(
    module: &'m CompiledModule,
    code: DiagnosticCode,
    what: &str,
) -> &'m Diagnostic {
    let diags = ctor_conformance_diags(module);
    assert_eq!(
        diags.len(),
        1,
        "{what}: expected exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Error, "{what}: {:?}", diags[0]);
    assert_eq!(diags[0].code, Some(code), "{what}: {:?}", diags[0]);
    diags[0]
}

const SOURCE_LIST_OF_GEOMETRY_GIVEN_SELECTOR: &str = r#"module test.list_geometry_selector
structure def G { param geoms : List<Geometry> = undef }
structure def Plate {
    let body = box(10mm, 10mm, 2mm)
    let g1 = G(geoms: face(body, "y_max"))
    sub g2 = G(geoms: face(body, "y_max"))
}
"#;

/// Today's construction-site behaviour: a bare selector bound to a
/// `List<Geometry>` field is rejected in both the `let` and the `sub =` form,
/// although the same argument to a fn param is legal (pinned in
/// `param_binding_selector_coercion_tests.rs`). #7956 makes construction sites
/// accept it (Leo, esc-5307-3) and flips this test together with the spec text.
///
/// Load-bearing: relaxing the conformance predicate WITHOUT the
/// `resolve_selector` lowering wrap would store a raw Selector in a
/// `List<Geometry>` field, because evaluation inserts ctor args verbatim (PRD
/// D7). This pin stops that half-change from landing silently.
#[test]
fn list_of_geometry_field_rejects_a_bare_selector_at_construction_sites() {
    let module = compile_source_with_stdlib(SOURCE_LIST_OF_GEOMETRY_GIVEN_SELECTOR);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        2,
        "one rejection per construction context (let, sub =), got: {diags:#?}"
    );
    for d in diags {
        assert_eq!(d.severity, Severity::Error, "{d:?}");
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToTrait),
            "{d:?}"
        );
        for needle in ["does not match wrapper shape", "geoms", "List<Geometry>"] {
            assert!(
                d.message.contains(needle),
                "message must contain {needle:?}, got: {:?}",
                d.message
            );
        }
    }
}

const SOURCE_KIND_AGNOSTIC_SELECTOR_FIELD: &str = r#"module test.kind_agnostic_selector_field
structure def K { param anysel : Selector = undef }
structure def Plate {
    let body = box(10mm, 10mm, 2mm)
    let a = K(anysel: face(body, "x_max"))
    let b = K(anysel: edges(body))
    sub s = K(anysel: face(body, "x_max"))
}
"#;

#[test]
fn kind_agnostic_selector_field_accepts_every_selector_kind() {
    let module = compile_source_with_stdlib(SOURCE_KIND_AGNOSTIC_SELECTOR_FIELD);
    assert_clean(&module, "Selector field given face/edge selectors");
}

const SOURCE_SINGLE_KIND_FIELD_GIVEN_KIND_AGNOSTIC: &str = r#"module test.single_kind_given_any
structure def K { param face : FaceSelector = undef }
structure def Plate {
    let body = box(10mm, 10mm, 2mm)
    param anys : Selector
    let a = K(face: anys)
}
"#;

#[test]
fn single_kind_selector_field_rejects_a_kind_agnostic_selector_value() {
    let module = compile_source_with_stdlib(SOURCE_SINGLE_KIND_FIELD_GIVEN_KIND_AGNOSTIC);
    let d = assert_single_error(
        &module,
        DiagnosticCode::SelectorKindMismatch,
        "FaceSelector field given a Selector value",
    );
    assert!(
        d.message.contains("FaceSelector"),
        "message must name the required kind, got: {:?}",
        d.message
    );
}

const SOURCE_OPTION_NONE_AND_IMPLICIT_SOME: &str = r#"module test.option_none_implicit_some
structure def K {
    param maybe_len : Option<Length> = undef
    param face : Option<FaceSelector> = undef
}
structure def Plate {
    let a = K(maybe_len: 3mm)
    let b = K(maybe_len: none)
    let c = K(face: none)
    sub s = K(maybe_len: 3mm)
}
"#;

#[test]
fn option_field_accepts_none_and_an_implicit_some_bare_value() {
    let module = compile_source_with_stdlib(SOURCE_OPTION_NONE_AND_IMPLICIT_SOME);
    assert_clean(&module, "Option fields given none and a bare inner value");
}

const SOURCE_POSITIONAL_ARG_CHECKED: &str = r#"module test.positional_arg_checked
structure def W {
    param a : String = "x"
    param b : String = "y"
}
structure def Root { let w = W("ok", 42) }
"#;

#[test]
fn positional_argument_is_checked_like_a_named_one() {
    let module = compile_source_with_stdlib(SOURCE_POSITIONAL_ARG_CHECKED);
    let d = assert_single_error(
        &module,
        DiagnosticCode::ArgTypeMismatch,
        "second positional String arg given Int",
    );
    assert!(
        ctor_diagnostic_names_arg(&d.message, "b"),
        "message must name param 'b', got: {:?}",
        d.message
    );
}
