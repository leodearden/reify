//! Integration test to verify all new M3 resolution types are exported from the crate root.

#[test]
fn all_resolution_types_exported() {
    // DeterminacyState::Auto variant
    let _auto = reify_types::DeterminacyState::Auto;

    // ResolutionNodeId
    let _rid = reify_types::ResolutionNodeId::new("Bracket", 0);

    // ConstraintDomain
    let _cd = reify_types::ConstraintDomain::Dimensional;

    // AutoParam
    let _ap = reify_types::AutoParam {
        id: reify_types::ValueCellId::new("Bracket", "width"),
        param_type: reify_types::Type::length(),
        bounds: None,
    };

    // OptimizationObjective
    let expr = reify_types::CompiledExpr {
        kind: reify_types::CompiledExprKind::Literal(reify_types::Value::Real(1.0)),
        result_type: reify_types::Type::Real,
        content_hash: reify_types::ContentHash::of(b"test"),
    };
    let _obj = reify_types::OptimizationObjective::Minimize(expr.clone());

    // ResolutionProblem
    let _rp = reify_types::ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: reify_types::ValueMap::new(),
        objective: None,
    };

    // SolveResult
    let _sr = reify_types::SolveResult::NoProgress {
        reason: "test".to_string(),
    };

    // ConstraintSolver trait — verify it exists as a trait object type
    fn _assert_trait_object(_: &dyn reify_types::ConstraintSolver) {}
}
