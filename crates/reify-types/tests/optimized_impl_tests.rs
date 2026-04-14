//! Integration tests for the `OptimizedImpl` trait and its input/output types
//! (Task 273 — @optimized: plumbing).
//!
//! These tests drive the trait's shape in reify-types: it must be object-safe,
//! `Send + Sync`, and its input/output types must expose the fields the Engine's
//! dispatch path will consume.

use reify_types::{
    CompiledExpr, CompiledFunction, ConstraintDiagnostics, ConstraintNodeId, ConstraintResult,
    ContentHash, DeterminacyState, OptimizedImpl, OptimizedImplInput, OptimizedImplOutput,
    PersistentMap, Satisfaction, Type, Value, ValueCellId, ValueMap,
};

/// A minimal in-test implementation so we can exercise the trait object.
struct AlwaysSatisfied;

impl OptimizedImpl for AlwaysSatisfied {
    fn check(&self, input: &OptimizedImplInput) -> OptimizedImplOutput {
        let results = input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Satisfied,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect();
        OptimizedImplOutput { results }
    }
}

fn make_literal_expr() -> CompiledExpr {
    CompiledExpr {
        kind: reify_types::CompiledExprKind::Literal(Value::Bool(true)),
        result_type: Type::Bool,
        content_hash: ContentHash::of(b"opt_test"),
    }
}

#[test]
fn optimized_impl_trait_is_object_safe() {
    // If OptimizedImpl is object-safe, `Box<dyn OptimizedImpl>` compiles.
    let _boxed: Box<dyn OptimizedImpl> = Box::new(AlwaysSatisfied);
}

#[test]
fn box_dyn_optimized_impl_is_send_sync() {
    // Confirm the trait bounds include Send + Sync so the trait object can cross
    // thread boundaries — matches the existing ConstraintChecker pattern.
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn OptimizedImpl>();
    assert_send_sync::<Box<dyn OptimizedImpl>>();
}

#[test]
fn optimized_impl_input_exposes_expected_fields() {
    let expr = make_literal_expr();
    let constraints: Vec<(ConstraintNodeId, &CompiledExpr)> =
        vec![(ConstraintNodeId::new("S", 0), &expr)];
    let values = ValueMap::new();
    let functions: Vec<CompiledFunction> = Vec::new();
    let determinacy: Option<&PersistentMap<ValueCellId, (Value, DeterminacyState)>> = None;

    let input = OptimizedImplInput {
        constraints,
        values: &values,
        functions: &functions,
        determinacy,
    };

    // Field access — if any field is renamed or missing this test fails to compile.
    assert_eq!(input.constraints.len(), 1);
    assert!(input.values.is_empty());
    assert!(input.functions.is_empty());
    assert!(input.determinacy.is_none());
}

#[test]
fn optimized_impl_output_carries_constraint_results() {
    let result = ConstraintResult {
        id: ConstraintNodeId::new("S", 0),
        satisfaction: Satisfaction::Satisfied,
        diagnostics: ConstraintDiagnostics::default(),
    };
    let output = OptimizedImplOutput {
        results: vec![result],
    };
    assert_eq!(output.results.len(), 1);
    assert_eq!(output.results[0].satisfaction, Satisfaction::Satisfied);
}

#[test]
fn trait_object_dispatch_returns_results_for_each_constraint() {
    let expr = make_literal_expr();
    let constraints: Vec<(ConstraintNodeId, &CompiledExpr)> = vec![
        (ConstraintNodeId::new("S", 0), &expr),
        (ConstraintNodeId::new("S", 1), &expr),
    ];
    let values = ValueMap::new();
    let functions: Vec<CompiledFunction> = Vec::new();

    let input = OptimizedImplInput {
        constraints,
        values: &values,
        functions: &functions,
        determinacy: None,
    };

    let imp: Box<dyn OptimizedImpl> = Box::new(AlwaysSatisfied);
    let output = imp.check(&input);

    assert_eq!(output.results.len(), 2);
    assert_eq!(output.results[0].id, ConstraintNodeId::new("S", 0));
    assert_eq!(output.results[1].id, ConstraintNodeId::new("S", 1));
    assert!(
        output
            .results
            .iter()
            .all(|r| r.satisfaction == Satisfaction::Satisfied)
    );
}
