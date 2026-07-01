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

/// The single-param, same-param-twice `Pair<T>` enum used by the conflict
/// test: both fields of `Both` declare the SAME type parameter `T`, so
/// supplying two structurally-incompatible concrete types binds `T` twice.
const PAIR_ENUM_SOURCE: &str = "\
enum Pair<T> {
    Both { a: T, b: T },
}
";

/// step-3 (RED): a same-type-param-twice payload — two fields whose declared
/// type is the SAME bare type parameter `T` — supplied concrete values of two
/// different (structurally incompatible) types must be flagged as a
/// type-argument conflict (task γ #4031, PRD §5 D3 / §7.3).
///
/// This is the faithful, erasure-compatible realization of the task/PRD's
/// illustrative `Node { left: Leaf{value:1mm}, right: Leaf{value:1N} }`
/// example: that recursive form is NOT detectable under the PRD's own D1/
/// F-Mono erasure (a constructed `Leaf{..}` child has result type
/// `Type::Enum("Tree")` with no type-arg slot, so unifying the declared
/// `Tree<T>` field against it binds nothing for `T` — documented β posture,
/// type_compat.rs:563-569). The same-param-twice form exercises the identical
/// diagnostic via the stated mechanism (D3/INV-3: each payload field whose
/// declared type is `Type::TypeParam(P)` binds `P`; same-`P` fields must
/// agree).
///
/// Currently RED: `unify`'s `Err(TypeArgConflict)` is silently ignored by
/// compile_variant_construct's inference loop (step-2's `let _ = unify(...)`);
/// nothing yet routes it to a diagnostic.
#[test]
fn same_param_twice_conflict_is_flagged() {
    let source = format!(
        "{PAIR_ENUM_SOURCE}\nstructure def Widget {{\n    let p = Both {{ a: 1mm, b: 1N }}\n}}\n"
    );
    assert!(
        has_error_code(&source, DiagnosticCode::EnumTypeArgConflict),
        "Both {{ a: 1mm, b: 1N }} binds T=Length from 'a' then T=Force from 'b' — expected \
         EnumTypeArgConflict; got error codes {:?}",
        error_codes(&source)
    );
    // The construction must NOT assemble a clean value — a type-arg conflict
    // suppresses value assembly (poison/typed-placeholder), observable here as
    // at least one Error-severity diagnostic on the module.
    assert!(
        !error_codes(&source).is_empty(),
        "a same-param-twice conflict must suppress clean value assembly (at least one Error \
         diagnostic expected); got none"
    );
}

/// Build a `structure def` source whose single param `r : <annotation>` defaults
/// to the given construction expression, prepended with [`RESULT_ENUM_SOURCE`].
/// The explicit `: <annotation>` drives the pinned-annotation expected-type seam
/// (the param-default site, entity.rs) rather than the bare `let` form used by
/// the earlier (unannotated) inference tests above.
fn result_param_source(annotation: &str, construction: &str) -> String {
    format!(
        "{RESULT_ENUM_SOURCE}\nstructure def Widget {{\n    param r : {annotation} = {construction}\n}}\n"
    )
}

/// step-5 (RED) case (a): a PINNED enum annotation (`Result<Force, String>`) on
/// a `param` default must OVERRIDE payload-driven inference for the
/// type-param-aware payload-type check — `Ok { value: 5mm }` supplies a
/// Length-typed payload for the `TypeParam("T")` field, but the annotation pins
/// `T = Force`, so the substituted declared type (`Force`) disagrees with the
/// supplied `Length` and must be flagged as `DiagnosticCode::VariantPayloadType`
/// (task γ #4031, PRD §5 D... pinned-annotation check).
///
/// Currently RED: the top-level param-default site (entity.rs:1942-1947)
/// compiles its initializer with plain `compile_expr` (no `expected_type`), so
/// the pinned `Result<Force, String>` annotation never reaches
/// `compile_variant_construct` — payload-driven inference (step-2) instead
/// infers `T = Length` from the VALUE itself and checks clean, so today this
/// construction emits NOTHING.
#[test]
fn pinned_annotation_payload_mismatch_emits_variant_payload_type() {
    let source = result_param_source("Result<Force, String>", "Ok { value: 5mm }");
    assert!(
        has_error_code(&source, DiagnosticCode::VariantPayloadType),
        "param r : Result<Force, String> = Ok {{ value: 5mm }} pins T=Force, but field \
         'value' supplies Length -> expected VariantPayloadType; got error codes {:?}",
        error_codes(&source)
    );
}

/// step-5 (RED) case (b): the same pinned-annotation seam, but with the
/// supplied payload MATCHING the pinned arg (`Result<Length, String>` +
/// `value: 5mm`) — must check clean (ZERO Error diagnostics). A regression pin
/// alongside case (a) so a fix that unconditionally flags every pinned
/// `Result<..>` construction (rather than only a genuine mismatch) is caught.
#[test]
fn pinned_annotation_payload_match_checks_clean() {
    let source = result_param_source("Result<Length, String>", "Ok { value: 5mm }");
    assert!(
        error_codes(&source).is_empty(),
        "param r : Result<Length, String> = Ok {{ value: 5mm }} pins T=Length, matching the \
         supplied Length payload -> expected ZERO Error diagnostics; got {:?}",
        error_codes(&source)
    );
}

/// The non-generic `Shape` enum used by the step-7(a) INV-6 regression tests:
/// γ's inference/substitution machinery must be a complete no-op when no
/// declared payload field type carries a `Type::TypeParam` leaf — `subst`
/// stays empty and `substitute_type_params` is the identity, so the
/// payload-type check is byte-for-byte the pre-γ (DCE δ, task 3942) check.
const SHAPE_ENUM_SOURCE: &str = "\
enum Shape {
    Circle { radius: Length },
    Point,
}
";

/// step-7(a) (regression): INV-6 — a non-generic enum's VALID construction
/// still checks clean under γ's extended payload-type check.
#[test]
fn non_generic_enum_valid_construction_stays_clean() {
    let source = format!(
        "{SHAPE_ENUM_SOURCE}\nstructure def Widget {{\n    let c = Circle {{ radius: 5mm }}\n}}\n"
    );
    assert!(
        error_codes(&source).is_empty(),
        "Circle {{ radius: 5mm }} on a non-generic enum should produce ZERO Error \
         diagnostics (INV-6 — γ must not touch non-generic enums); got {:?}",
        error_codes(&source)
    );
}

/// step-7(a) (regression): INV-6 — a non-generic enum's GENUINE payload-type
/// mismatch is still flagged (bit-for-bit unchanged from pre-γ). `radius` is
/// declared `Length`; `true` (Bool) mismatches.
#[test]
fn non_generic_enum_payload_mismatch_still_flagged() {
    let source = format!(
        "{SHAPE_ENUM_SOURCE}\nstructure def Widget {{\n    let c = Circle {{ radius: true }}\n}}\n"
    );
    assert!(
        has_error_code(&source, DiagnosticCode::VariantPayloadType),
        "Circle {{ radius: true }} supplies a Bool for Length field 'radius' -> expected \
         VariantPayloadType (INV-6: non-generic enums must still be checked); got error \
         codes {:?}",
        error_codes(&source)
    );
}

/// The recursive `Tree<T>` enum used by the step-7(b) tolerance test — the
/// SAME fixture spelling as `enum_type_param_lowering_tests.rs`'s
/// `tree_t_recursive_node_left_is_parameterized_type` (task α/β grammar/
/// lowering). `Node`'s `left`/`right` fields declare `Tree<T>`, resolved to
/// `Type::Applied { "Tree", [TypeParam("T")] }` (task β #4030).
const TREE_ENUM_SOURCE: &str = "\
enum Tree<T> {
    Leaf { value: T },
    Node { left: Tree<T>, right: Tree<T> },
}
";

/// step-7(b): recursive/applied payload-field tolerance. Constructing a
/// `Node` whose `left`/`right` fields are themselves `Leaf { .. }`
/// constructions must NOT spuriously emit `VariantPayloadType` — under D1/
/// F-Mono erasure a constructed `Leaf { value: 1mm }` child has result type
/// `Type::Enum("Tree")` (no type-arg slot on the value), so the declared
/// field type `Type::Applied { "Tree", [TypeParam("T")] }` must tolerate the
/// erased same-base `Type::Enum("Tree")` supplied value with NO pinned
/// annotation in scope (task γ #4031; guards against a false-positive that
/// would break sibling task ε's integration example).
#[test]
fn recursive_applied_field_accepts_erased_enum_child() {
    let source = format!(
        "{TREE_ENUM_SOURCE}\nstructure def Widget {{\n    let t = Node {{ left: Leaf {{ value: 1mm }}, right: Leaf {{ value: 1mm }} }}\n}}\n"
    );
    assert!(
        error_codes(&source).is_empty(),
        "Node {{ left: Leaf {{ value: 1mm }}, right: Leaf {{ value: 1mm }} }} should produce \
         ZERO Error diagnostics — a recursive Tree<T> field must tolerate an erased \
         same-base Type::Enum(\"Tree\") child; got {:?}",
        error_codes(&source)
    );
}
