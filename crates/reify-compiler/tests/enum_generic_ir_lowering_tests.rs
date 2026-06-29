//! Compiler lowers EnumDecl.type_params → EnumDef.type_params (task β #4030).
//!
//! Tests the β signal: the type-param HEAD on a generic enum declaration is
//! preserved in the IR `EnumDef.type_params` field after compilation.
//!
//! Also co-asserts (as regression pins):
//! - INV-6: non-generic enums lower to `type_params.is_empty()`.
//! - Payload Type::TypeParam (already landed by task δ #3942) for field types.
//! - INV-1 same-resolver-provenance: declared type-param names drive the same
//!   `Type::TypeParam(..)` outcome in payload fields that structures/traits/fns use.

mod common;

use common::compile_with_stdlib_helper;
use reify_core::ty::Type;
use reify_ir::VariantPayload;

// Shared fixture: a generic enum compiled by tests (a), (c), and (d).
// Extracted as a const so the three consumers reference the same source
// string without duplication (suggestion 2 / reviewer_comprehensive).
const RESULT_ENUM_SOURCE: &str = "\
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
";

// ── step-3 RED ───────────────────────────────────────────────────────────────

/// (a) β signal: a generic enum's declared type-param HEAD lowers to
/// `EnumDef.type_params` = ["T", "E"].
///
/// RED until step-4: pre_pass placeholder emits `type_params: vec![]`,
/// so this assertion fails.
#[test]
fn generic_enum_type_params_lowered_to_enum_def() {
    let module = compile_with_stdlib_helper(RESULT_ENUM_SOURCE);
    let result_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Result")
        .expect("Result enum should be present in module.enum_defs");

    // (a) β signal: type_params carries the declared head ["T", "E"].
    let type_param_names: Vec<String> =
        result_def.type_params.iter().map(|p| p.name.clone()).collect();
    assert_eq!(
        type_param_names,
        vec!["T".to_string(), "E".to_string()],
        "EnumDef.type_params must reflect the declared generic head <T, E>"
    );
}

/// (b) INV-6: a non-generic enum lowers to `type_params.is_empty()`.
///
/// GREEN from step-2 (placeholder vec![] satisfies this; regression pin).
#[test]
fn non_generic_enum_has_empty_type_params() {
    let source = "enum Dir { In, Out }";
    let module = compile_with_stdlib_helper(source);
    let dir_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Dir")
        .expect("Dir enum should be present in module.enum_defs");

    assert!(
        dir_def.type_params.is_empty(),
        "INV-6: non-generic enum must have empty type_params, got: {:?}",
        dir_def.type_params
    );
}

/// (c) Payload regression pin (already landed by task δ #3942): the Ok and Err
/// variants carry `VariantPayload::Named` with `Type::TypeParam` field types.
///
/// GREEN from step-2 (enums_phase already resolves this; regression pin).
#[test]
fn generic_enum_variant_payloads_carry_type_param_types() {
    let module = compile_with_stdlib_helper(RESULT_ENUM_SOURCE);
    let result_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Result")
        .expect("Result enum should be present in module.enum_defs");

    let ok_var = result_def
        .variants
        .iter()
        .find(|v| v.name == "Ok")
        .expect("Ok variant must exist");
    let err_var = result_def
        .variants
        .iter()
        .find(|v| v.name == "Err")
        .expect("Err variant must exist");

    // Ok { value: T } -> Named([("value", Type::TypeParam("T"))])
    assert_eq!(
        ok_var.payload,
        VariantPayload::Named(vec![("value".to_string(), Type::TypeParam("T".to_string()))]),
        "Ok variant must carry Named([value: TypeParam(T)])"
    );

    // Err { error: E } -> Named([("error", Type::TypeParam("E"))])
    assert_eq!(
        err_var.payload,
        VariantPayload::Named(vec![("error".to_string(), Type::TypeParam("E".to_string()))]),
        "Err variant must carry Named([error: TypeParam(E)])"
    );
}

/// (d) INV-1 same-resolver-provenance: the declared type-param NAMES in
/// `EnumDef.type_params` ("T","E") exactly match the names appearing in the
/// payload `Type::TypeParam(..)` fields, demonstrating that the enum's
/// declared type params drive the same resolver outcome that
/// structure/trait/fn generics produce.
///
/// RED until step-4 (type_params is empty, so the name-coincidence loop
/// cannot find any params to cross-check against payload fields).
#[test]
fn inv1_type_param_names_coincide_with_payload_type_param_names() {
    let module = compile_with_stdlib_helper(RESULT_ENUM_SOURCE);
    let result_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Result")
        .expect("Result enum should be present in module.enum_defs");

    // Collect declared type-param names from the head.
    let declared_names: Vec<&str> =
        result_def.type_params.iter().map(|p| p.name.as_str()).collect();
    assert!(
        !declared_names.is_empty(),
        "INV-1 pre-condition: type_params must be non-empty (fails RED until step-4)"
    );

    // Collect all Type::TypeParam names that appear in variant payloads.
    let mut payload_tp_names: Vec<String> = vec![];
    for variant in &result_def.variants {
        if let VariantPayload::Named(fields) = &variant.payload {
            for (_fname, ftype) in fields {
                if let Type::TypeParam(tp_name) = ftype {
                    payload_tp_names.push(tp_name.clone());
                }
            }
        }
    }

    // Every payload TypeParam name must appear in the declared head.
    for tp_name in &payload_tp_names {
        assert!(
            declared_names.contains(&tp_name.as_str()),
            "INV-1: payload Type::TypeParam({:?}) is not in declared type_params {:?}",
            tp_name,
            declared_names
        );
    }

    // The declared head must be a subset of the payload TypeParam names
    // (both "T" and "E" must appear in at least one payload field).
    for name in &declared_names {
        assert!(
            payload_tp_names.iter().any(|n| n == *name),
            "INV-1: declared type param {:?} does not appear in any payload field type {:?}",
            name,
            payload_tp_names
        );
    }
}

/// Bounds and default on a generic enum's type param survive the lowering
/// path through `convert_type_params` into `EnumDef.type_params`.
///
/// Tests that `convert_type_params` preserves not just the name but also
/// the declared `bounds` (`Vec<TraitBound>`) and optional `default` (`Type`)
/// for enums, matching the behaviour already exercised for structures/traits/fns
/// (INV-1: same converter, same full output).
///
/// Uses `T: Tagged = Int`: the bound name "Tagged" should survive in
/// `bounds[0].trait_ref.name`; the default `Int` should resolve to
/// `Type::Int` via `resolve_type_name`.
///
/// GREEN from step-4 (convert_type_params is the shared converter for all
/// declaration kinds and already handles bounds + Named-type defaults).
#[test]
fn generic_enum_type_param_bounds_and_default_survive_lowering() {
    // A user-defined trait used as a bound, plus a defaulted type param.
    // Bounds are stored on the definition (not checked at the definition site),
    // so this compiles cleanly regardless of whether any concrete type
    // satisfies the bound — the constraint is only enforced at the use site.
    let source = "\
trait Tagged {}
enum Wrapper<T: Tagged = Int> {
    Item { value: T },
}
";
    let module = compile_with_stdlib_helper(source);
    let wrapper_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Wrapper")
        .expect("Wrapper enum should be present in module.enum_defs");

    assert_eq!(
        wrapper_def.type_params.len(),
        1,
        "Wrapper<T: Tagged = Int> must have exactly one type param"
    );
    let tp = &wrapper_def.type_params[0];
    assert_eq!(tp.name, "T", "type param name must be 'T'");

    // Bound: `T: Tagged` -> bounds == [TraitBound { trait_ref: TraitRef { name: "Tagged", .. } }]
    assert_eq!(
        tp.bounds.len(),
        1,
        "type param T must carry exactly one bound (Tagged)"
    );
    assert_eq!(
        tp.bounds[0].trait_ref.name, "Tagged",
        "type param bound must be the declared trait name 'Tagged'"
    );

    // Default: `T = Int` -> default == Some(Type::Int)
    assert_eq!(
        tp.default,
        Some(Type::Int),
        "type param default 'Int' must lower to Type::Int via resolve_type_name"
    );
}
