//! INV-2 erasure boundary regression pin for generic data-carrying enums
//! (task ε #4033, step-5; PRD docs/prds/v0_6/generic-data-carrying-enums.md §8).
//!
//! Pins the D1/INV-2 invariant: a generic enum's `Value::Enum` payload carries
//! NO type-arg tag, so eval is byte-identical to a non-generic enum with the
//! same concrete payload, and `match` payload-binding eval binds the concrete
//! value with zero type-arg awareness.
//!
//! These tests PASS immediately (erasure landed in DCE ζ #3946 + generics β +
//! construction inference γ #4031 + typed binders δ #4032) — regression pins
//! mirroring crates/reify-expr/tests/generic_fn_erasure_tests.rs (the
//! generic-FN INV-2 pin, task 4233 δ). If either ever goes RED, it signals an
//! erasure regression in a dependency, not a gap in this crate.
//!
//! Uses the same compile_source + cell_expr + eval_expr pattern as that file.

use reify_core::DimensionVector;
use reify_ir::{Value, ValueMap};
use reify_test_support::compile_source;

/// Locate the `default_expr` of a named value cell in the first template.
fn cell_expr<'a>(
    module: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = &module.templates[0];
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default_expr"))
}

// ── test (a): erased payload is byte-identical to a non-generic analogue ────

/// INV-2: `Ok { value: 5mm }` (generic `Result<T,E>`) and `OkL { value: 5mm }`
/// (non-generic `OkLen`) evaluate to `Value::Enum` whose PAYLOADS are
/// byte-identical — the erased value model carries no type-arg tag. Encodes
/// the PRD's own INV-2 example directly: "`Ok { value: 5mm }` ... produce
/// structurally-identical Value::Enum payloads" to a hypothetical non-generic
/// `OkLength { value: 5mm }`.
#[test]
fn generic_enum_value_payload_identical_to_non_generic() {
    let source = "enum Result<T, E> { Ok { value: T }, Err { error: E } } \
                  enum OkLen { OkL { value: Length } } \
                  structure S { let g = Ok { value: 5mm }  let m = OkL { value: 5mm } }";
    let module = compile_source(source);

    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    let g_val = reify_expr::eval_expr(cell_expr(&module, "g"), &ctx);
    let (g_type_name, g_variant, g_payload) = match &g_val {
        Value::Enum {
            type_name,
            variant,
            payload,
        } => (type_name.clone(), variant.clone(), payload.clone()),
        other => panic!(
            "expected Value::Enum for g (generic Ok{{value:5mm}}), got {:?}",
            other
        ),
    };
    assert_eq!(g_type_name, "Result", "g should be a Result enum value");
    assert_eq!(g_variant, "Ok", "g should be the Ok variant");

    let m_val = reify_expr::eval_expr(cell_expr(&module, "m"), &ctx);
    let (m_type_name, m_variant, m_payload) = match &m_val {
        Value::Enum {
            type_name,
            variant,
            payload,
        } => (type_name.clone(), variant.clone(), payload.clone()),
        other => panic!(
            "expected Value::Enum for m (non-generic OkL{{value:5mm}}), got {:?}",
            other
        ),
    };
    assert_eq!(m_type_name, "OkLen", "m should be an OkLen enum value");
    assert_eq!(m_variant, "OkL", "m should be the OkL variant");

    // INV-2: the two enums differ (name/variant, asserted above) but their
    // PAYLOADS — the part a generic construction's type args could have
    // tainted — must be byte-identical.
    assert_eq!(
        g_payload, m_payload,
        "INV-2: generic Ok{{value:5mm}} and non-generic OkL{{value:5mm}} payloads \
         must be byte-identical (erased value model carries no type-arg tag)"
    );
}

// ── test (b): match-payload-binding eval is type-arg-agnostic ───────────────

/// INV-2: `match` payload-binding eval binds the concrete `Value` from a
/// generic enum's payload with zero awareness of the (erased) type argument.
/// `match (Ok { value: 5mm }) { Ok { value: v } => v, Err { error: m } => 6mm }`
/// selects the Ok arm and returns the bound 5mm scalar — the VariantBind eval
/// path (ζ #3946) has no type-arg branch to be agnostic *about*.
#[test]
fn match_eval_on_generic_enum_is_type_arg_agnostic() {
    let source = "enum Result<T, E> { Ok { value: T }, Err { error: E } } \
                  structure S { let bore = match (Ok { value: 5mm }) { \
                  Ok { value: v } => v, Err { error: m } => 6mm } }";
    let module = compile_source(source);

    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    let bore_val = reify_expr::eval_expr(cell_expr(&module, "bore"), &ctx);
    match bore_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected 0.005 m (5mm) from the Ok arm's bound payload, got {si_value}"
            );
            assert_eq!(
                dimension,
                DimensionVector::LENGTH,
                "bound payload should carry LENGTH dimension"
            );
        }
        other => panic!("expected Value::Scalar for bore, got {:?}", other),
    }
}
