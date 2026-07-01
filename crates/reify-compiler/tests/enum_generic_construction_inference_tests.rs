//! Type-argument inference + payload-vs-param type checking at generic-variant
//! construction (task γ #4031).
//!
//! Extends DCE δ's named-field variant construction (`variant_construct.rs`,
//! task 3942) with type-arg inference over generic enums (`EnumDef.type_params`,
//! task β #4030):
//!   - Payload-driven inference: each supplied field whose declared type is a
//!     bare `Type::TypeParam(P)` binds `P` from the value's concrete type; the
//!     substituted declared type then drives the (existing) payload-type check.
//!   - Same-param-twice conflict: two fields binding the same `P` to different
//!     concrete types emits `DiagnosticCode::EnumTypeArgConflict`.
//!   - Pinned-annotation check: a `param x : Enum<Args> = Variant { .. }` site
//!     pins the type args positionally, overriding payload-driven inference for
//!     the type-param-aware `DiagnosticCode::VariantPayloadType` check.
//!
//! Diagnostic assertions match on `Diagnostic.code` (typed `DiagnosticCode`)
//! rather than message substrings, per the codebase convention
//! (reify-core/src/diagnostics.rs) — mirrors `variant_construction_check_tests.rs`.

mod common;

use common::compile_with_stdlib_helper;
use reify_core::{DiagnosticCode, Severity};

/// The two-param `Result<T, E>` enum shared across inference tests (identical
/// fixture to `enum_generic_ir_lowering_tests.rs` / `generic_enum_pattern_binder_tests.rs`).
const RESULT_ENUM_SOURCE: &str = "\
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
";

/// Compile `source` and collect the codes of its Error-severity diagnostics
/// (used to render a helpful message when a `has_error_code` assertion fails).
fn error_codes(source: &str) -> Vec<Option<DiagnosticCode>> {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

/// True if compiling `source` yields at least one Error-severity diagnostic
/// carrying `code`.
fn has_error_code(source: &str, code: DiagnosticCode) -> bool {
    compile_with_stdlib_helper(source)
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// step-1 (RED): a generic-variant construction supplying a bare type-param
/// payload field, with NO pinned enum annotation in scope, must check clean —
/// payload-driven inference binds `T = Length` from the supplied `5mm` and the
/// (substituted) payload-type check passes.
///
/// Currently FAILS: `compile_variant_construct` calls
/// `type_compatible(declared_ty=Type::TypeParam("T"), value.result_type=Scalar<Length>)`,
/// which returns `false` (`TypeParam` hits the `_ => false` arm of
/// `implicitly_converts_to`), spuriously emitting `VariantPayloadType` for a
/// type-param field that no inference step has substituted yet.
#[test]
fn bare_type_param_payload_infers_cleanly_without_annotation() {
    let source = format!(
        "{RESULT_ENUM_SOURCE}\nstructure def Widget {{\n    let r = Ok {{ value: 5mm }}\n}}\n"
    );
    assert!(
        !has_error_code(&source, DiagnosticCode::VariantPayloadType),
        "Ok {{ value: 5mm }} with declared field type TypeParam(\"T\") should infer T=Length \
         and check clean — expected NO VariantPayloadType; got error codes {:?}",
        error_codes(&source)
    );
    assert!(
        error_codes(&source).is_empty(),
        "Ok {{ value: 5mm }} should produce ZERO Error diagnostics; got {:?}",
        error_codes(&source)
    );
}
