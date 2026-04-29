//! Tests for SolveSpaceSolver — geometric constraint solving via libslvs FFI.

use reify_constraints::SolveSpaceSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, CompiledExpr, CompiledExprKind, ConstraintSolver, ContentHash, DiagnosticCode,
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
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Point", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
                free: false,
            },
            AutoParam {
                id: y2_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Angle", 0), constraint_expr)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
        other => panic!("expected Solved for angle, got {:?}", other),
    }
}

/// Solve a parallel constraint between two lines.
///
/// line1: fixed from (0,0,0) to (0.01, 0.01, 0) — 45 degree diagonal
/// line2: from fixed (0.02, 0, 0) to auto (x2, y2, 0)
/// Constraint: parallel(line1, line2)
#[test]
fn solve_parallel_constraint() {
    let solver = SolveSpaceSolver;

    let x2_id = vcid("Line2", "ex");
    let y2_id = vcid("Line2", "ey");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });
    let l = |v: f64| {
        literal(Value::Scalar {
            si_value: v,
            dimension: DimensionVector::LENGTH,
        })
    };

    // line1: (0,0,0) → (0.01, 0.01, 0)
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero.clone()],
        Type::dimensionless_scalar(),
    );
    let pt1_end = geo_fn(
        "point3d",
        vec![l(0.01), l(0.01), zero.clone()],
        Type::dimensionless_scalar(),
    );

    // line2: (0.02, 0, 0) → (x2, y2, 0) — auto params
    let pt2_start = geo_fn(
        "point3d",
        vec![l(0.02), zero.clone(), zero.clone()],
        Type::dimensionless_scalar(),
    );
    let pt2_end = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Line2", "ex", Type::length()),
            value_ref_typed("Line2", "ey", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );

    let line1 = geo_fn(
        "line_segment",
        vec![origin, pt1_end],
        Type::dimensionless_scalar(),
    );
    let line2 = geo_fn(
        "line_segment",
        vec![pt2_start, pt2_end],
        Type::dimensionless_scalar(),
    );

    // parallel(line1, line2) — boolean constraint (top-level function call)
    let constraint_expr = geo_fn("parallel", vec![line1, line2], Type::Bool);

    let mut current = ValueMap::new();
    current.insert(
        x2_id.clone(),
        Value::Scalar {
            si_value: 0.03,
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
                free: false,
            },
            AutoParam {
                id: y2_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Parallel", 0), constraint_expr)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x2 = values.get(&x2_id).unwrap().as_f64().unwrap();
            let y2 = values.get(&y2_id).unwrap().as_f64().unwrap();

            // line1 direction: (0.01, 0.01, 0) → slope = 1
            // line2 direction: (x2 - 0.02, y2 - 0, 0)
            // Parallel means cross product ≈ 0
            let dx2 = x2 - 0.02;
            let dy2 = y2;
            let cross = 0.01 * dy2 - 0.01 * dx2;
            assert!(
                cross.abs() < 1e-6,
                "lines should be parallel: cross product = {} (x2={}, y2={})",
                cross,
                x2,
                y2,
            );
        }
        other => panic!("expected Solved for parallel, got {:?}", other),
    }
}

/// Solve a coincident constraint between two points.
#[test]
fn solve_coincident_constraint() {
    let solver = SolveSpaceSolver;

    let x1_id = vcid("P1", "x");
    let y1_id = vcid("P1", "y");
    let x2_id = vcid("P2", "x");
    let y2_id = vcid("P2", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    let pt1 = geo_fn(
        "point3d",
        vec![
            value_ref_typed("P1", "x", Type::length()),
            value_ref_typed("P1", "y", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );
    let pt2 = geo_fn(
        "point3d",
        vec![
            value_ref_typed("P2", "x", Type::length()),
            value_ref_typed("P2", "y", Type::length()),
            zero,
        ],
        Type::dimensionless_scalar(),
    );

    let constraint_expr = geo_fn("coincident", vec![pt1, pt2], Type::Bool);

    let mut current = ValueMap::new();
    current.insert(
        x1_id.clone(),
        Value::Scalar {
            si_value: 0.01,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y1_id.clone(),
        Value::Scalar {
            si_value: 0.02,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        x2_id.clone(),
        Value::Scalar {
            si_value: 0.03,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y2_id.clone(),
        Value::Scalar {
            si_value: 0.04,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x1_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y1_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: x2_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y2_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Coin", 0), constraint_expr)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x1 = values.get(&x1_id).unwrap().as_f64().unwrap();
            let y1 = values.get(&y1_id).unwrap().as_f64().unwrap();
            let x2 = values.get(&x2_id).unwrap().as_f64().unwrap();
            let y2 = values.get(&y2_id).unwrap().as_f64().unwrap();
            assert!(
                (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6,
                "points should be coincident: ({}, {}) vs ({}, {})",
                x1,
                y1,
                x2,
                y2,
            );
        }
        other => panic!("expected Solved for coincident, got {:?}", other),
    }
}

/// Overconstrained system returns Infeasible.
#[test]
fn solve_overconstrained_returns_infeasible() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    let pt = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Point", "x", Type::length()),
            value_ref_typed("Point", "y", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    // distance(pt, origin) == 10mm
    let dist1 = geo_fn(
        "pt_pt_distance",
        vec![pt.clone(), origin.clone()],
        Type::length(),
    );
    let c1 = eq(dist1, literal(mm(10.0)));

    // distance(pt, origin) == 20mm — contradicts c1
    let dist2 = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
    let c2 = eq(dist2, literal(mm(20.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Over", 0), c1), (cnid("Over", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostics");
        }
        other => panic!(
            "overconstrained system should be Infeasible, got {:?}",
            other
        ),
    }
}

/// Underconstrained system still solves (finds any valid position).
#[test]
fn solve_underconstrained_solves_with_dof() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");
    let z_id = vcid("Point", "z");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    let pt = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Point", "x", Type::length()),
            value_ref_typed("Point", "y", Type::length()),
            value_ref_typed("Point", "z", Type::length()),
        ],
        Type::dimensionless_scalar(),
    );
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    let dist = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
    let constraint_expr = eq(dist, literal(mm(15.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: z_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Under", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x = values.get(&x_id).unwrap().as_f64().unwrap();
            let y = values.get(&y_id).unwrap().as_f64().unwrap();
            let z = values.get(&z_id).unwrap().as_f64().unwrap();
            let dist = (x * x + y * y + z * z).sqrt();
            assert!(
                (dist - 0.015).abs() < 1e-6,
                "distance should be ~15mm (0.015m), got {} m",
                dist,
            );
        }
        other => panic!("expected Solved for underconstrained, got {:?}", other),
    }
}

/// SolveSpaceSolver.solve() must never panic on valid input.
///
/// This is a regression guard documenting the no-panic contract.
/// Uses catch_unwind to detect any panics.
#[test]
fn solve_never_panics_on_valid_input() {
    use std::panic;

    let result = panic::catch_unwind(|| {
        let solver = SolveSpaceSolver;

        let x_id = vcid("Point", "x");
        let y_id = vcid("Point", "y");

        let zero = literal(Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        });

        let pt = geo_fn(
            "point3d",
            vec![
                value_ref_typed("Point", "x", Type::length()),
                value_ref_typed("Point", "y", Type::length()),
                zero.clone(),
            ],
            Type::dimensionless_scalar(),
        );
        let origin = geo_fn(
            "point3d",
            vec![zero.clone(), zero.clone(), zero],
            Type::dimensionless_scalar(),
        );
        let dist = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
        let constraint_expr = eq(dist, literal(mm(10.0)));

        let problem = ResolutionProblem {
            auto_params: vec![
                AutoParam {
                    id: x_id,
                    param_type: Type::length(),
                    bounds: None,
                    free: false,
                },
                AutoParam {
                    id: y_id,
                    param_type: Type::length(),
                    bounds: None,
                    free: false,
                },
            ],
            constraints: vec![(cnid("NoPanic", 0), constraint_expr)],
            current_values: ValueMap::new(),
            objective: None,
            functions: vec![].into(),
        };

        solver.solve(&problem)
    });

    assert!(
        result.is_ok(),
        "SolveSpaceSolver.solve() panicked on valid input"
    );
    // Also verify it actually solved
    match result.unwrap() {
        SolveResult::Solved { .. } => {} // expected
        other => panic!("expected Solved, got {:?}", other),
    }
}

// NOTE: Lock poisoning of the internal SLVS_LOCK mutex is untestable from
// outside the module because the static Mutex is private. The solver's
// poisoned-lock path (returning NoProgress) is verified by code inspection
// and the fact that `Mutex::lock()` returns `Result` which is matched
// in the `solve_raw` implementation. The `solve_never_panics_on_valid_input`
// test above provides coverage for the no-panic contract.

/// Unrecognized geometric pattern returns NoProgress, not a panic.
#[test]
fn solve_unrecognized_pattern_falls_through() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");

    // A complex nested expression that doesn't match any known pattern
    let unknown_fn = CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "some_exotic_constraint".to_string(),
                qualified_name: "std::geo::some_exotic_constraint".to_string(),
            },
            args: vec![value_ref_typed("Point", "x", Type::length())],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: ContentHash::of(b"exotic"),
    };
    let constraint_expr = gt(unknown_fn, literal(Value::Real(5.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id,
            param_type: Type::length(),
            bounds: None,
            free: false,
        }],
        constraints: vec![(cnid("Unknown", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::NoProgress { reason } => {
            assert!(
                reason.contains("unrecognized"),
                "reason should mention unrecognized, got: {}",
                reason,
            );
        }
        other => panic!(
            "expected NoProgress for unrecognized pattern, got {:?}",
            other
        ),
    }
}

/// A point3d with a non-numeric coordinate (e.g., boolean literal) should not
/// be recognized as a valid geometric pattern. Previously, extract_coord silently
/// returned CoordRef::Fixed(0.0) for non-numeric values, causing wrong coordinates.
#[test]
fn non_numeric_coord_returns_none() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");

    // Build a point with a boolean literal as the Y coordinate — this is non-numeric
    let pt_x = value_ref_typed("Point", "x", Type::length());
    let bool_literal = CompiledExpr {
        kind: CompiledExprKind::Literal(Value::Bool(true)),
        result_type: Type::Bool,
        content_hash: ContentHash::of(b"bool_true"),
    };
    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    // point3d(x, true, 0) — second arg is non-numeric
    let bad_point = geo_fn(
        "point3d",
        vec![pt_x, bool_literal, zero.clone()],
        Type::dimensionless_scalar(),
    );

    // origin: (0, 0, 0)
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    // distance(bad_point, origin) == 10mm
    let dist_call = geo_fn("pt_pt_distance", vec![bad_point, origin], Type::length());
    let ten_mm = literal(mm(10.0));
    let constraint_expr = eq(dist_call, ten_mm);

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id,
            param_type: Type::length(),
            bounds: None,
            free: false,
        }],
        constraints: vec![(cnid("Point", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    // With non-numeric coord, the pattern should NOT be recognized
    let result = solver.solve(&problem);
    match result {
        SolveResult::NoProgress { reason } => {
            assert!(
                reason.contains("unrecognized"),
                "reason should mention unrecognized pattern, got: {}",
                reason,
            );
        }
        other => panic!(
            "expected NoProgress for non-numeric coordinate, got {:?}",
            other
        ),
    }
}

/// solve() must return SolveResult::NoProgress when a constraint expression
/// contains a non-auto ValueRef cell_id that is absent from current_values.
///
/// Because extract_coord only accepts auto params and literals, a ValueRef that
/// is not in auto_params causes recognize_pattern to return None, which means
/// solve() returns NoProgress with "unrecognized geometric constraint pattern".
/// This exercises the NoProgress return path in solve() at the constraint-loop level.
#[test]
fn solve_returns_no_progress_for_missing_non_auto_value() {
    let solver = SolveSpaceSolver;

    const AUTO_ENTITY: &str = "Auto";
    const X_MEMBER: &str = "x";
    const FIXED_ENTITY: &str = "Fixed";
    const Y_MEMBER: &str = "y";

    // A cell_id that is NOT in auto_params — simulates an incomplete eval pass.
    // (Used as FIXED_ENTITY/Y_MEMBER in the ValueRef below; intentionally absent from auto_params)

    // Auto param for x (present in auto_params so the problem is non-trivial)
    let x_id = vcid(AUTO_ENTITY, X_MEMBER);

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    // Point with x=auto, y=non-auto ValueRef (FIXED_ENTITY/Y_MEMBER NOT in auto_params)
    let pt_x = value_ref_typed(AUTO_ENTITY, X_MEMBER, Type::length());
    let pt_y = value_ref_typed(FIXED_ENTITY, Y_MEMBER, Type::length());
    let point_a = geo_fn(
        "point3d",
        vec![pt_x, pt_y, zero.clone()],
        Type::dimensionless_scalar(),
    );

    // Origin: (0, 0, 0)
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    let dist_call = geo_fn("pt_pt_distance", vec![point_a, origin], Type::length());
    let ten_mm = literal(mm(10.0));
    let constraint_expr = eq(dist_call, ten_mm);

    // fixed_y_id is intentionally absent from both auto_params and current_values.
    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: None,
            free: false,
        }],
        constraints: vec![(cnid("Test", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::NoProgress { reason } => {
            // The non-auto ValueRef (fixed_y_id) causes pattern recognition to fail,
            // so the reason is "unrecognized geometric constraint pattern".
            assert!(
                !reason.is_empty(),
                "NoProgress reason should be non-empty, got empty string"
            );
            // Verify it's not a spurious "no constraints recognized" failure
            // — we DO have a constraint, it's just unrecognizable.
            assert!(
                reason.contains("unrecognized"),
                "reason should explain why progress was impossible, got: {}",
                reason
            );
        }
        other => panic!(
            "expected NoProgress for constraint with non-auto missing value, got {:?}",
            other
        ),
    }
}

/// Inconsistent geometric constraints must carry DiagnosticCode::ConstraintUnsatisfiable.
/// Reuses the overconstrained setup from solve_overconstrained_returns_infeasible.
#[test]
fn inconsistent_geometric_diagnostic_carries_constraint_unsatisfiable_code() {
    let solver = SolveSpaceSolver;

    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    let pt = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Point", "x", Type::length()),
            value_ref_typed("Point", "y", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    // distance(pt, origin) == 10mm
    let dist1 = geo_fn("pt_pt_distance", vec![pt.clone(), origin.clone()], Type::length());
    let c1 = eq(dist1, literal(mm(10.0)));

    // distance(pt, origin) == 20mm — contradicts c1
    let dist2 = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
    let c2 = eq(dist2, literal(mm(20.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::length(), bounds: None, free: false },
            AutoParam { id: y_id.clone(), param_type: Type::length(), bounds: None, free: false },
        ],
        constraints: vec![(cnid("Over", 0), c1), (cnid("Over", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostics");
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable)),
                "inconsistent geometric diagnostic must carry ConstraintUnsatisfiable code; got: {:?}",
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            );
        }
        other => panic!(
            "overconstrained system should be Infeasible, got {:?}",
            other
        ),
    }
}
