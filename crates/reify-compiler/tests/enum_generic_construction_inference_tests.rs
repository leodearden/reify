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
use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};

/// The two-param `Result<T, E>` enum shared across inference tests (identical
/// fixture to `enum_generic_ir_lowering_tests.rs` / `generic_enum_pattern_binder_tests.rs`).
const RESULT_ENUM_SOURCE: &str = "\
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
";

/// Collect the codes of `module`'s Error-severity diagnostics (used to render
/// a helpful message when a `has_error_code` assertion fails).
///
/// Takes an already-compiled `&CompiledModule` rather than a `source: &str`
/// (and re-compiling internally) so that a test asserting both code-presence
/// AND the failure-message diagnostic list compiles `source` exactly ONCE via
/// `compile_with_stdlib_helper` — amendment (reviewer suggestion: avoid the
/// 2-3x recompile per test the source-taking variant caused).
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
/// `let`) on the `Widget` structure's `TopologyTemplate`. Mirrors the
/// `outline_default` helper in `variant_construction_check_tests.rs` — used
/// to inspect whether a construction actually assembled a clean `Value::Enum`
/// or was suppressed to the `Value::Undef` poison/typed-placeholder, rather
/// than merely inferring suppression from the presence of *some* Error
/// diagnostic.
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        !has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "Ok {{ value: 5mm }} with declared field type TypeParam(\"T\") should infer T=Length \
         and check clean — expected NO VariantPayloadType; got error codes {:?}",
        error_codes(&module)
    );
    assert!(
        error_codes(&module).is_empty(),
        "Ok {{ value: 5mm }} should produce ZERO Error diagnostics; got {:?}",
        error_codes(&module)
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::EnumTypeArgConflict),
        "Both {{ a: 1mm, b: 1N }} binds T=Length from 'a' then T=Force from 'b' — expected \
         EnumTypeArgConflict; got error codes {:?}",
        error_codes(&module)
    );
    // The construction must NOT assemble a clean value — a type-arg conflict
    // suppresses value assembly. Inspect the 'p' value cell's compiled
    // default_expr directly to confirm it is the Value::Undef poison/
    // typed-placeholder (compile_variant_construct's checks_start guard)
    // rather than a clean Literal(Value::Enum).
    match &widget_value_cell(&module, "p").kind {
        CompiledExprKind::Literal(Value::Undef) => {}
        other => panic!(
            "a same-param-twice conflict must suppress clean value assembly — expected \
             Literal(Value::Undef), got {:?}",
            other
        ),
    }
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "param r : Result<Force, String> = Ok {{ value: 5mm }} pins T=Force, but field \
         'value' supplies Length -> expected VariantPayloadType; got error codes {:?}",
        error_codes(&module)
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        error_codes(&module).is_empty(),
        "param r : Result<Length, String> = Ok {{ value: 5mm }} pins T=Length, matching the \
         supplied Length payload -> expected ZERO Error diagnostics; got {:?}",
        error_codes(&module)
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        error_codes(&module).is_empty(),
        "Circle {{ radius: 5mm }} on a non-generic enum should produce ZERO Error \
         diagnostics (INV-6 — γ must not touch non-generic enums); got {:?}",
        error_codes(&module)
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "Circle {{ radius: true }} supplies a Bool for Length field 'radius' -> expected \
         VariantPayloadType (INV-6: non-generic enums must still be checked); got error \
         codes {:?}",
        error_codes(&module)
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
    let module = compile_with_stdlib_helper(&source);
    assert!(
        error_codes(&module).is_empty(),
        "Node {{ left: Leaf {{ value: 1mm }}, right: Leaf {{ value: 1mm }} }} should produce \
         ZERO Error diagnostics — a recursive Tree<T> field must tolerate an erased \
         same-base Type::Enum(\"Tree\") child; got {:?}",
        error_codes(&module)
    );
}

/// step-8: pinned-recursive tolerance. With a PINNED annotation in scope
/// (`param t : Tree<Length> = ...`), the `Node` fields' declared type
/// `Type::Applied { "Tree", [TypeParam("T")] }` substitutes to the CONCRETE
/// `Type::Applied { "Tree", [Length] }` — unlike step-7(b)'s unpinned case,
/// this substituted type no longer carries a `TypeParam` leaf, so it reaches
/// the `enum_payload_compatible` check rather than the unbound-type-param
/// skip. Must still tolerate the erased same-base `Type::Enum("Tree")`
/// supplied by the constructed `Leaf { .. }` children (task γ #4031 step-8).
#[test]
fn pinned_recursive_applied_field_accepts_matching_erased_child() {
    let source = format!(
        "{TREE_ENUM_SOURCE}\nstructure def Widget {{\n    param t : Tree<Length> = Node {{ left: Leaf {{ value: 1mm }}, right: Leaf {{ value: 1mm }} }}\n}}\n"
    );
    let module = compile_with_stdlib_helper(&source);
    let codes = error_codes(&module);
    assert!(
        codes.is_empty(),
        "param t : Tree<Length> = Node {{ left: Leaf {{ .. }}, right: Leaf {{ .. }} }} should \
         produce ZERO Error diagnostics (pinned Tree<Length> substitutes Node's fields to \
         concrete Type::Applied{{\"Tree\",[Length]}}, which must tolerate the erased same-base \
         Type::Enum(\"Tree\") supplied by each Leaf child); got {:?}",
        codes
    );
}

/// step-8 (sanity): `enum_payload_compatible`'s same-base tolerance must NOT
/// paper over a genuine cross-enum-base mismatch — a `Circle { .. }` (from
/// the unrelated `Shape` enum) supplied where a `Tree<Length>`-pinned `Node`
/// field expects a `Tree` value must still be flagged.
#[test]
fn pinned_recursive_applied_field_flags_cross_enum_mismatch() {
    let source = format!(
        "{TREE_ENUM_SOURCE}\n{SHAPE_ENUM_SOURCE}\nstructure def Widget {{\n    param t : Tree<Length> = Node {{ left: Circle {{ radius: 1mm }}, right: Leaf {{ value: 1mm }} }}\n}}\n"
    );
    let module = compile_with_stdlib_helper(&source);
    assert!(
        has_error_code(&module, DiagnosticCode::VariantPayloadType),
        "Node {{ left: Circle {{ .. }}, .. }} supplies a Shape/Circle value for a Tree<Length> \
         field 'left' -> expected VariantPayloadType (enum_payload_compatible must not tolerate \
         a differing enum base); got error codes {:?}",
        error_codes(&module)
    );
}
