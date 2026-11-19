//! The Direction operator table: which operators accept a `Direction`, the
//! static type each accepted form has, and what the accepted forms evaluate to
//! (PRD `docs/prds/v0_6/direction-typing.md` §5 C5/C6, B16/B18-B20, task γ
//! #9635).

use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::Value;
use reify_test_support::{
    cell_value, compile_source_with_stdlib, direction_val, errors_only, get_let_expr,
};

const STRUCTURE: &str = "DirOps";

/// A structure holding `d = dir(0, 0, 1)`, `e = dir(1, 0, 0)` and `body`.
fn direction_source(body: &str) -> String {
    format!(
        "structure def {STRUCTURE} {{\n    let d = dir(0, 0, 1)\n    let e = dir(1, 0, 0)\n    {body}\n}}\n"
    )
}

/// Each accepted C6 form, bound as a `let`, with the static type it must have.
fn accepted_forms() -> Vec<(&'static str, &'static str, Type)> {
    let dimensionless_vector = || Type::vec3(Type::dimensionless_scalar());
    vec![
        ("length_scaled", "2mm * d", Type::vec3(Type::length())),
        ("real_scaled", "d * 2.0", dimensionless_vector()),
        ("int_scaled", "2 * d", dimensionless_vector()),
        ("cosine", "dot(d, e)", Type::dimensionless_scalar()),
        ("normal", "cross(d, e)", dimensionless_vector()),
        ("antiparallel", "-d", Type::Direction),
        ("normalized", "normalize(d)", Type::Direction),
        ("normal_direction", "dir(cross(d, e))", Type::Direction),
        ("as_vector", "vector(d)", dimensionless_vector()),
        (
            "vector_sum",
            "vector(d) + vector(e)",
            dimensionless_vector(),
        ),
    ]
}

fn eval_source_with_stdlib(source: &str) -> reify_eval::EvalResult {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_test_support::make_engine();
    let result = engine.eval(&compiled);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "eval-phase errors: {errors:?}");
    result
}

#[test]
fn accepted_forms_have_the_c6_static_types() {
    let body = accepted_forms()
        .iter()
        .map(|(name, expr, _)| format!("let {name} = {expr}"))
        .collect::<Vec<_>>()
        .join("\n    ");
    let module = compile_source_with_stdlib(&direction_source(&body));
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "accepted forms must check clean: {errors:?}"
    );

    for (name, expr, expected) in accepted_forms() {
        assert_eq!(
            get_let_expr(&module, name).result_type,
            expected,
            "`{expr}` has the wrong static type"
        );
    }
}

#[test]
fn vector_is_a_known_builtin() {
    let module = compile_source_with_stdlib(&direction_source("let v = vector(d)"));
    let unresolved: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedFunction))
        .collect();
    assert!(unresolved.is_empty(), "vector must resolve: {unresolved:?}");
}

/// A structure with an `Axis` param `a` and a single-relation `relate` block.
fn relate_source(relation: &str) -> String {
    format!(
        "structure def RelateDir {{\n    param a : Axis = axis_x(point3(0mm, 0mm, 0mm))\n    relate {{ {relation} }}\n}}\n"
    )
}

#[test]
fn parallel_of_an_axis_direction_and_a_direction_checks_clean() {
    let module = compile_source_with_stdlib(&relate_source("parallel(a.dir, dir(0, 0, 1))"));
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "B16 first form must check clean: {errors:?}"
    );
}

#[test]
fn parallel_of_a_converted_vector_has_no_datum_projection() {
    let module =
        compile_source_with_stdlib(&relate_source("parallel(vector(a.dir), dir(0, 0, 1))"));
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::DatumProjectionUnavailable)),
        "a Vector3 operand has no Direction projection (B16): {errors:?}"
    );
}

#[test]
fn accepted_forms_evaluate_end_to_end() {
    let result = eval_source_with_stdlib(&direction_source(
        "let antiparallel = -dir(0, 0, 1)
    let length_scaled = 2mm * dir(0, 0, 1)
    let cosine = dot(d, e)
    let normal = cross(e, dir(0, 1, 0))
    let normalized = normalize(d)
    let normal_direction = dir(cross(e, dir(0, 1, 0)))
    let vector_sum = vector(d) + vector(e)",
    ));
    let value = |member: &str| cell_value(&result, STRUCTURE, member);
    let real_vector = |c: [f64; 3]| Value::Vector(c.map(Value::Real).to_vec());

    let antiparallel = value("antiparallel");
    assert_eq!(antiparallel, direction_val(0.0, 0.0, -1.0));
    assert_eq!(format!("{antiparallel}"), "dir(0, 0, -1)");
    assert_eq!(
        value("length_scaled"),
        Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.002)
        ])
    );
    assert_eq!(value("cosine"), Value::Real(0.0));
    assert_eq!(value("normal"), real_vector([0.0, 0.0, 1.0]));
    assert_eq!(value("normalized"), direction_val(0.0, 0.0, 1.0));
    assert_eq!(value("normal_direction"), direction_val(0.0, 0.0, 1.0));
    assert_eq!(value("vector_sum"), real_vector([1.0, 0.0, 1.0]));
}

// ── C6 rejection table ──────────────────────────────────────────────────────

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading the production constant here would make it a tautology.
const VECTOR_HINT: &str = "write vector(d)";

/// The single Error that `let r = {expr}` draws, beside `d` and `e`.
fn sole_error(expr: &str) -> reify_core::Diagnostic {
    let module = compile_source_with_stdlib(&direction_source(&format!("let r = {expr}")));
    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "`{expr}` must draw exactly one Error: {errors:?}"
    );
    errors[0].clone()
}

#[test]
fn rejected_operator_forms_end_with_the_vector_hint() {
    for expr in [
        "d + e",
        "d - e",
        "d + vector(e)",
        "vector(e) - d",
        "d + 1",
        "d * e",
        "d / 2",
        "2 / d",
    ] {
        let error = sole_error(expr);
        assert_eq!(
            error.code,
            Some(DiagnosticCode::ArithOperandKind),
            "`{expr}`: {error:?}"
        );
        assert!(
            error.message.ends_with(VECTOR_HINT),
            "`{expr}` must carry the vector(d) fix hint: {error:?}"
        );
    }
}

#[test]
fn rejected_math_forms_end_with_the_vector_hint() {
    for (name, expr) in [
        ("magnitude", "magnitude(d)"),
        ("dot", "dot(d, vec3(0.0, 0.0, 1.0))"),
        ("dot", "dot(vec3(0.0, 0.0, 1.0), d)"),
        ("cross", "cross(d, vec3(0.0, 0.0, 1.0))"),
        ("cross", "cross(vec3(0.0, 0.0, 1.0), d)"),
        ("abs", "abs(d)"),
        ("outer", "outer(d, e)"),
    ] {
        let error = sole_error(expr);
        assert_eq!(
            error.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "`{expr}`: {error:?}"
        );
        assert!(
            error.message.starts_with(name),
            "`{expr}` must name {name}: {error:?}"
        );
        assert!(
            error.message.ends_with(VECTOR_HINT),
            "`{expr}` must carry the vector(d) fix hint: {error:?}"
        );
    }
}

#[test]
fn vector_of_a_non_direction_is_rejected() {
    let error = sole_error("vector(vec3(1, 0, 0))");
    assert_eq!(
        error.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{error:?}"
    );
    assert!(
        error.message.contains("Direction"),
        "the error must name the Direction it expects: {error:?}"
    );
}

#[test]
fn non_direction_arithmetic_carries_no_vector_hint() {
    let error = sole_error("vec3(1mm, 0mm, 0mm) * vec3(1mm, 0mm, 0mm)");
    assert_eq!(
        error.code,
        Some(DiagnosticCode::ArithOperandKind),
        "{error:?}"
    );
    assert!(
        !error.message.contains(VECTOR_HINT),
        "the vector(d) hint is Direction-scoped: {error:?}"
    );
}
