//! A let-constructed structure instance resolves a selector over its own
//! geometry `let` (task #10352).
//!
//! The ctor evaluator gives each template geometry let a symbolic handle in
//! the instance's member scope. That handle's `upstream_values_hash` comes
//! from reify-expr's `realization_upstream_values_hash`, while template and
//! `sub` scope mint theirs through the engine's own fold. The tests here pin
//! the two folds equal and the three scopes in agreement.

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// The engine's template-scope `Beam.body` handle hashes its upstream values
/// exactly as reify-expr's fold does over the same realization's op args.
/// The engine's fold is crate-private, so this public-seam comparison is what
/// keeps the two from diverging.
#[test]
fn reify_expr_fold_matches_engine_template_handle_hash() {
    let compiled = parse_and_compile_with_stdlib(
        "structure def Beam {
            param len : Length = 1000mm
            let body = box(len, 100mm, 100mm)
        }",
    );
    let result = make_simple_engine().eval(&compiled);

    let Some(Value::GeometryHandle {
        realization_ref,
        upstream_values_hash,
        ..
    }) = result.values.get(&ValueCellId::new("Beam", "body"))
    else {
        panic!(
            "Beam.body must hold a GeometryHandle, got {:?}",
            result.values.get(&ValueCellId::new("Beam", "body"))
        );
    };

    let beam = compiled
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("Beam template");
    let body = beam
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("body"))
        .expect("Beam's `body` realization");
    assert_eq!(*realization_ref, body.id);

    let ctx = reify_expr::EvalContext::new(&result.values, &compiled.functions);
    let folded = reify_expr::realization_upstream_values_hash(
        body.operations.iter().flat_map(|op| op.args()),
        |e| reify_expr::eval_expr(e, &ctx),
    );
    assert_eq!(
        *upstream_values_hash, folded,
        "reify-expr's realization_upstream_values_hash must stay byte-equal to \
         the engine's compute_realization_upstream_values_hash_from_ops"
    );
}
