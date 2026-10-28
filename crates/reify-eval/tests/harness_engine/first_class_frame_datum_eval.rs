//! Frame datum projections evaluate on every receiver path and through every
//! engine entry point (task #6100).
//!
//! `examples/first_class_frame_datums.ri` projects a let-bound frame, a rotated
//! frame built from let-bound `self` directions, and `self.frame` itself. Each
//! test drives one entry point — `Engine::eval`, a fresh `Engine::eval_cached`,
//! and the post-solve let re-run — and every projected cell must hold its exact
//! value, in SI metres.

use reify_constraints::DimensionalSolver;
use reify_core::{ValueCellId, VersionId};
use reify_eval::{Engine, SolverPosture};
use reify_ir::{ResolutionProfile, Value, ValueMap};
use reify_test_support::{
    MockConstraintChecker, collect_errors, compile_source_with_stdlib, make_engine,
};

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/first_class_frame_datums.ri"
);

const ENTITY: &str = "FirstClassFrameDatums";

const EPS: f64 = 1e-12;

fn compiled_example() -> reify_compiler::CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("could not read {EXAMPLE_PATH}: {e}"));
    let module = compile_source_with_stdlib(&source);
    let errors = collect_errors(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/first_class_frame_datums.ri should compile without errors: {errors:#?}"
    );
    module
}

fn cell<'a>(values: &'a ValueMap, entity: &str, member: &str) -> &'a Value {
    values
        .get(&ValueCellId::new(entity, member))
        .unwrap_or_else(|| panic!("{entity}.{member} has no value"))
}

fn components(v: &Value, what: &str) -> [f64; 3] {
    match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => [0, 1, 2].map(|i| {
            c[i].as_f64()
                .unwrap_or_else(|| panic!("{what}: component {i} is not numeric in {v:?}"))
        }),
        other => panic!("{what}: expected a 3-component Point or Vector, got {other:?}"),
    }
}

fn assert_close(actual: [f64; 3], expected: [f64; 3], what: &str) {
    assert!(
        (0..3).all(|i| (actual[i] - expected[i]).abs() < EPS),
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

fn assert_point(v: &Value, expected: [f64; 3], what: &str) {
    assert!(
        matches!(v, Value::Point(_)),
        "{what}: expected a Point, got {v:?}"
    );
    assert_close(components(v, what), expected, what);
}

fn assert_direction(v: &Value, expected: [f64; 3], what: &str) {
    match v {
        Value::Direction { x, y, z } => assert_close([*x, *y, *z], expected, what),
        other => panic!("{what}: expected a Direction, got {other:?}"),
    }
}

fn assert_plane(v: &Value, origin: [f64; 3], normal: [f64; 3], what: &str) {
    match v {
        Value::Plane {
            origin: o,
            normal: n,
        } => {
            assert_point(o, origin, &format!("{what} origin"));
            assert!(
                matches!(**n, Value::Vector(_)),
                "{what}: the normal must be a Vector, got {n:?}"
            );
            assert_close(components(n, what), normal, &format!("{what} normal"));
        }
        other => panic!("{what}: expected a Plane, got {other:?}"),
    }
}

/// Every projected cell of the example holds its exact value.
fn assert_every_projection(values: &ValueMap) {
    let get = |member: &str| cell(values, ENTITY, member);

    let f_origin = [0.004, 0.005, 0.006];
    assert_point(get("f_o"), f_origin, "f_o");
    assert_direction(get("f_x"), [1.0, 0.0, 0.0], "f_x");
    assert_direction(get("f_z"), [0.0, 0.0, 1.0], "f_z");
    assert_plane(get("f_xy"), f_origin, [0.0, 0.0, 1.0], "f_xy");
    assert_plane(get("f_yz"), f_origin, [1.0, 0.0, 0.0], "f_yz");
    assert_plane(get("f_zx"), f_origin, [0.0, 1.0, 0.0], "f_zx");

    let r_origin = [0.001, 0.002, 0.003];
    assert_direction(get("r_x"), [0.0, 1.0, 0.0], "r_x");
    assert_direction(get("r_y"), [0.0, 0.0, 1.0], "r_y");
    assert_direction(get("r_z"), [1.0, 0.0, 0.0], "r_z");
    assert_plane(get("r_xy"), r_origin, [1.0, 0.0, 0.0], "r_xy");
    assert_plane(get("r_yz"), r_origin, [0.0, 1.0, 0.0], "r_yz");
    assert_plane(get("r_zx"), r_origin, [0.0, 0.0, 1.0], "r_zx");

    let s_origin = [0.0, 0.0, 0.0];
    assert_point(get("s_o"), s_origin, "s_o");
    assert_plane(get("s_yz"), s_origin, [1.0, 0.0, 0.0], "s_yz");
    assert_plane(get("sf_yz"), s_origin, [1.0, 0.0, 0.0], "sf_yz");
    assert_plane(get("s_zx"), s_origin, [0.0, 1.0, 0.0], "s_zx");
    assert_plane(get("sf_zx"), s_origin, [0.0, 1.0, 0.0], "sf_zx");
}

#[test]
fn eval_projects_every_member_of_a_let_bound_frame() {
    let module = compiled_example();
    let result = make_engine().eval(&module);
    assert_every_projection(&result.values);
}

/// A fresh `eval_cached` with no prior `eval()` (task #6130's entry point).
#[test]
fn eval_cached_projects_every_member_of_a_let_bound_frame() {
    let module = compiled_example();
    let result = make_engine().eval_cached(&module, VersionId(1));
    assert_every_projection(&result.eval_result.values);
}

/// A solved `auto` param sends the lets through the post-solve re-run; a
/// `self.<datum>` cell and a consumer of one must both survive it.
const POST_SOLVE_SRC: &str = r#"
    structure S {
        param w : Length = auto
        constraint w == 5mm
        let p = self.xy_plane
        let f = frame_at(point3(4mm, 5mm, 6mm), self.x, self.z)
        let fo = f.origin
    }
"#;

#[test]
fn self_datum_consumers_survive_the_post_solve_let_rerun() {
    let module = compile_source_with_stdlib(POST_SOLVE_SRC);
    let errors = collect_errors(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "POST_SOLVE_SRC should compile: {errors:#?}"
    );

    let mut engine = Engine::new(
        SolverPosture::Solve(ResolutionProfile::FRESH),
        Box::new(MockConstraintChecker::new()),
        None,
    )
    .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&module);

    match cell(&result.values, "S", "w") {
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 0.005).abs() < 1e-9,
            "premise: w must be solved to 5mm so the post-solve re-run runs, got {si_value}"
        ),
        other => panic!("premise: w must be solved to a Length, got {other:?}"),
    }
    assert_plane(
        cell(&result.values, "S", "p"),
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        "p",
    );
    assert_point(cell(&result.values, "S", "fo"), [0.004, 0.005, 0.006], "fo");
}
