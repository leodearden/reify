//! Tests for SolveSpaceSolver — geometric constraint solving via libslvs FFI.

use reify_constraints::SolveSpaceSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, CompiledExpr, CompiledExprKind, ConstraintSolver, ContentHash,
    DimensionVector, ResolutionProblem, ResolvedFunction, SolveResult, Type, Value, ValueMap,
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

/// Solve an angle constraint between two lines.
///
/// Two lines sharing the origin, with the endpoint of line2 being auto params.
/// Constraint: angle(line1, line2) == 90 degrees (perpendicular via angle).
#[test]
fn solve_angle_constraint() {
    let solver = SolveSpaceSolver;

    // line1: from origin (0,0,0) to (0.01, 0, 0) — fixed along X axis
    // line2: from origin (0,0,0) to (x2, y2, 0) — auto params
    let x2_id = vcid("Line2", "x");
    let y2_id = vcid("Line2", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });
    let ten_mm = literal(Value::Scalar {
        si_value: 0.01,
        dimension: DimensionVector::LENGTH,
    });

    // Fixed points for line1
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero.clone()],
        Type::dimensionless_scalar(),
    );
    let pt_x_axis = geo_fn(
        "point3d",
        vec![ten_mm, zero.clone(), zero.clone()],
        Type::dimensionless_scalar(),
    );

    // Auto point for line2 endpoint
    let pt2_x = value_ref_typed("Line2", "x", Type::length());
    let pt2_y = value_ref_typed("Line2", "y", Type::length());
    let pt2 = geo_fn(
        "point3d",
        vec![pt2_x, pt2_y, zero.clone()],
        Type::dimensionless_scalar(),
    );

    // line1 and line2 as line_segment expressions
    let line1 = geo_fn(
        "line_segment",
        vec![origin.clone(), pt_x_axis],
        Type::dimensionless_scalar(),
    );
    let line2 = geo_fn(
        "line_segment",
        vec![origin, pt2],
        Type::dimensionless_scalar(),
    );

    // angle(line1, line2) == 90deg (in radians: pi/2)
    let angle_call = geo_fn("angle", vec![line1, line2], Type::dimensionless_scalar());
    let ninety_deg = literal(deg(90.0));
    let constraint_expr = eq(angle_call, ninety_deg);

    let mut current = ValueMap::new();
    // Provide initial guess for auto params (avoid degenerate start)
    current.insert(
        x2_id.clone(),
        Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y2_id.clone(),
        Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x2_id.clone(),
                param_type: Type::length(),
                bounds: None,
            },
            AutoParam {
                id: y2_id.clone(),
                param_type: Type::length(),
                bounds: None,
            },
        ],
        constraints: vec![(cnid("Angle", 0), constraint_expr)],
        current_values: current,
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let x2 = values.get(&x2_id).unwrap().as_f64().unwrap();
            let y2 = values.get(&y2_id).unwrap().as_f64().unwrap();

            // line1 direction: (1, 0, 0)
            // line2 direction: (x2, y2, 0) (from origin)
            // For 90 degrees, dot product should be ~0 → x2 ≈ 0
            let dot = x2; // dot product of (1,0,0) · (x2,y2,0) = x2
            let line2_len = (x2 * x2 + y2 * y2).sqrt();
            // cos(90) = 0, so dot / line2_len should be ~0
            if line2_len > 1e-10 {
                let cos_angle = dot / line2_len;
                assert!(
                    cos_angle.abs() < 0.01,
                    "cos(angle) should be ~0 for 90 degrees, got {} (x2={}, y2={})",
                    cos_angle,
                    x2,
                    y2,
                );
            }
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}
