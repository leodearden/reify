//! Tests for SolveSpaceSolver — geometric constraint solving via libslvs FFI.

use reify_constraints::SolveSpaceSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, CompiledExpr, CompiledExprKind, ConstraintSolver, ContentHash, DimensionVector,
    ResolutionProblem, ResolvedFunction, SolveResult, Type, Value, ValueMap,
};

// --- Helpers ---

/// Build a geometry function call expression (e.g., std::geo::pt_pt_distance).
fn geo_fn(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::geo::{}", name),
            },
            args,
        },
        result_type,
        content_hash: ContentHash::of(format!("geo_{}", name).as_bytes()),
    }
}

/// SolveSpaceSolver must be Send + Sync (required by ConstraintSolver trait).
#[test]
fn solvespace_solver_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SolveSpaceSolver>();
}

/// SolveSpaceSolver can be boxed as a trait object for ConstraintSolver.
#[test]
fn solvespace_solver_as_trait_object() {
    let solver = SolveSpaceSolver;
    let _boxed: Box<dyn ConstraintSolver> = Box::new(solver);
}

/// Solve a simple point-to-point distance constraint.
///
/// Two auto params (x, y) for a point, constrained to be 10mm from origin.
/// Expression: eq(pt_pt_distance(point(x, y, 0), point(0, 0, 0)), 10mm)
#[test]
fn solve_simple_point_distance_constraint() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");

    // Build expression: pt_pt_distance(point(x, y, 0), origin) == 10mm
    let pt_x = value_ref_typed("Point", "x", Type::length());
    let pt_y = value_ref_typed("Point", "y", Type::length());
    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    // First point: (x, y, 0) — auto params
    let point_a = geo_fn(
        "point3d",
        vec![pt_x, pt_y, zero.clone()],
        Type::dimensionless_scalar(), // placeholder result type for points
    );

    // Origin: (0, 0, 0)
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    // distance(point_a, origin)
    let dist_call = geo_fn("pt_pt_distance", vec![point_a, origin], Type::length());

    // distance == 10mm
    let ten_mm = literal(mm(10.0));
    let constraint_expr = eq(dist_call, ten_mm);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
            },
        ],
        constraints: vec![(cnid("Point", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let x_val = values.get(&x_id).unwrap().as_f64().unwrap();
            let y_val = values.get(&y_id).unwrap().as_f64().unwrap();
            let actual_dist = (x_val * x_val + y_val * y_val).sqrt();
            // 10mm = 0.01m in SI
            assert!(
                (actual_dist - 0.01).abs() < 1e-6,
                "distance should be ~10mm (0.01m), got {} m (x={}, y={})",
                actual_dist,
                x_val,
                y_val,
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}
