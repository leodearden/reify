//! Kinematic-query cells (`interferes` / `interferes_with` / `min_clearance`,
//! including the swept `flat_map` form) resolve at sub-INSTANCE scope
//! (`Parent.<sub>.<member>`) on every build-phase surface, against that
//! instance's own geometry.
//!
//! Gated on `OCCT_AVAILABLE`; skipped on builds without OCCT.

use reify_compiler::compile_with_stdlib;
use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::{ExportFormat, Satisfaction, Value, ValueMap};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

// `Parent` owns a step named `cube_a` 500mm away from everything, so a
// sub-instance that resolved its snapshot bodies against the parent's own
// steps instead of its own would read a clearance of roughly 450mm. The
// overridden `f` closes its gap until the cubes overlap, so its Bool and pair
// cells differ from the template's.
const INSTANCE_SCOPE_SOURCE: &str = r#"
structure def Child {
    param gap : Length = 30mm

    let cube_a = box(20mm, 20mm, 20mm)
    let cube_b = translate(box(20mm, 20mm, 20mm), gap, 0mm, 0mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let m2 = body(m1, "cube_b", fixed())
    let s = snapshot(m2, [])

    let id_a = body_id_of(m2, "cube_a")
    let id_b = body_id_of(m2, "cube_b")

    let pairs = interferes(s)
    let collide_ab = interferes_with(s, id_a, id_b)
    let clearance_ab = min_clearance(s, id_a, id_b)

    let j = prismatic(vec3(1, 0, 0), 0mm .. 300mm)
    let n1 = body(mechanism(), "cube_a", j)
    let n2 = body(n1, "cube_b", fixed())
    let ida = body_id_of(n2, "cube_a")
    let idb = body_id_of(n2, "cube_b")
    let snaps = sweep(n2, j, 0mm .. 20mm, 3)
    let swept = flat_map(snaps, |p| [min_clearance(p, ida, idb)])
}

structure def Parent {
    let cube_a = translate(box(20mm, 20mm, 20mm), 500mm, 0mm, 0mm)

    sub d = Child()
    sub e = Child(gap: 50mm)
    sub f = Child(gap: 10mm)

    let margin = self.d.clearance_ab - 5mm

    constraint self.d.clearance_ab > 5mm
    constraint self.e.clearance_ab > 25mm
    constraint self.d.clearance_ab > 50mm
}
"#;

const KINEMATIC_MEMBERS: [&str; 4] = ["clearance_ab", "collide_ab", "pairs", "swept"];

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(
        source,
        ModulePath::single("mechanism_interference_instance_scope_e2e"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {errors:#?}");
    compiled
}

fn engine_with_occt() -> reify_eval::Engine {
    reify_eval::Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(OcctKernelHandle::spawn())),
    )
}

fn cell<'a>(values: &'a ValueMap, entity: &str, member: &str) -> &'a Value {
    values
        .get(&ValueCellId::new(entity, member))
        .unwrap_or_else(|| panic!("{entity}.{member} not found in result values"))
}

fn read_si_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("{label}: expected numeric, got {other:?}"),
    }
}

fn assert_close(actual: &Value, expected: f64, tolerance: f64, label: &str) {
    let actual = read_si_f64(actual, label);
    assert!(
        (actual - expected).abs() < tolerance,
        "{label}: expected ~{expected}, got {actual}"
    );
}

fn swept_metres(v: &Value, label: &str) -> Vec<f64> {
    match v {
        Value::List(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| read_si_f64(item, &format!("{label}[{i}]")))
            .collect(),
        other => panic!("{label}: expected a list, got {other:?}"),
    }
}

/// `actual` matches the fully defined `expected`: numbers to within 1e-9
/// (SI), lists element by element, anything else exactly.
fn assert_value_matches(actual: &Value, expected: &Value, label: &str) {
    match expected {
        Value::Undef => panic!("{label}: the expected value must be defined"),
        Value::Real(_) | Value::Scalar { .. } => {
            assert_close(actual, read_si_f64(expected, label), 1e-9, label);
        }
        Value::List(expected_items) => {
            let Value::List(actual_items) = actual else {
                panic!("{label}: expected a list, got {actual:?}");
            };
            assert_eq!(actual_items.len(), expected_items.len(), "{label} length");
            for (i, (a, e)) in actual_items.iter().zip(expected_items).enumerate() {
                assert_value_matches(a, e, &format!("{label}[{i}]"));
            }
        }
        _ => assert_eq!(actual, expected, "{label}"),
    }
}

fn assert_instance_matches_template(values: &ValueMap, member: &str) {
    assert_value_matches(
        cell(values, "Parent.d", member),
        cell(values, "Child", member),
        &format!("Parent.d.{member}"),
    );
}

fn satisfaction_of(results: &[reify_eval::ConstraintCheckEntry], id: &str) -> Satisfaction {
    results
        .iter()
        .find(|entry| entry.id.to_string() == id)
        .unwrap_or_else(|| panic!("{id} not found in constraint results"))
        .satisfaction
}

#[test]
fn build_resolves_sub_instance_kinematic_cells_to_template_values() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(INSTANCE_SCOPE_SOURCE);
    let result = engine_with_occt().build(&compiled, ExportFormat::Step);

    for member in KINEMATIC_MEMBERS {
        assert_instance_matches_template(&result.values, member);
    }
}

#[test]
fn build_resolves_an_overridden_sub_instance_against_its_own_geometry() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(INSTANCE_SCOPE_SOURCE);
    let result = engine_with_occt().build(&compiled, ExportFormat::Step);

    assert_close(
        cell(&result.values, "Child", "clearance_ab"),
        0.010,
        1e-6,
        "Child.clearance_ab",
    );
    assert_close(
        cell(&result.values, "Parent.e", "clearance_ab"),
        0.030,
        1e-6,
        "Parent.e.clearance_ab",
    );
    assert_eq!(
        cell(&result.values, "Parent.e", "collide_ab"),
        &Value::Bool(false),
        "Parent.e.collide_ab"
    );

    // The sweep slides cube_a 0, 10 and 20mm toward cube_b, which sits 50mm
    // out in `e` (the template's 30mm gap closes to contact instead).
    let swept = swept_metres(cell(&result.values, "Parent.e", "swept"), "Parent.e.swept");
    assert_eq!(swept.len(), 3, "Parent.e.swept length: {swept:?}");
    for (i, (actual, expected)) in swept.iter().zip([0.030, 0.020, 0.010]).enumerate() {
        assert!(
            (actual - expected).abs() < 1e-6,
            "Parent.e.swept[{i}]: expected ~{expected}, got {actual}"
        );
    }
}

#[test]
fn build_resolves_a_colliding_override_against_its_own_geometry() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(INSTANCE_SCOPE_SOURCE);
    let result = engine_with_occt().build(&compiled, ExportFormat::Step);

    assert_eq!(
        cell(&result.values, "Child", "collide_ab"),
        &Value::Bool(false),
        "precondition: Child.collide_ab"
    );
    assert_eq!(
        cell(&result.values, "Parent.f", "collide_ab"),
        &Value::Bool(true),
        "Parent.f.collide_ab"
    );
    assert_close(
        cell(&result.values, "Parent.f", "clearance_ab"),
        0.0,
        1e-9,
        "Parent.f.clearance_ab",
    );
    match cell(&result.values, "Parent.f", "pairs") {
        Value::List(pairs) => assert_eq!(pairs.len(), 1, "Parent.f.pairs: {pairs:?}"),
        other => panic!("Parent.f.pairs: expected a list, got {other:?}"),
    }
}

#[test]
fn build_parent_constraints_and_lets_over_sub_kinematic_cells_are_determinate() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(INSTANCE_SCOPE_SOURCE);
    let result = engine_with_occt().build(&compiled, ExportFormat::Step);

    let results = &result.constraint_results;
    assert_eq!(
        satisfaction_of(results, "Parent#constraint[0]"),
        Satisfaction::Satisfied
    );
    assert_eq!(
        satisfaction_of(results, "Parent#constraint[1]"),
        Satisfaction::Satisfied
    );
    assert_eq!(
        satisfaction_of(results, "Parent#constraint[2]"),
        Satisfaction::Violated
    );
    assert_close(
        cell(&result.values, "Parent", "margin"),
        0.005,
        1e-6,
        "Parent.margin",
    );
}

// Every `BadIdChild` instance re-runs the template's `min_clearance` call
// with the same Real body id, so each would repeat the template's warning.
const BAD_BODY_ID_SOURCE: &str = r#"
structure def BadIdChild {
    let cube_a = box(20mm, 20mm, 20mm)
    let cube_b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let m2 = body(m1, "cube_b", fixed())
    let s = snapshot(m2, [])

    let id_b = body_id_of(m2, "cube_b")
    let clearance_bad = min_clearance(s, 1.5, id_b)
}

structure def BadIdParent {
    sub d = BadIdChild()
    sub e = BadIdChild()
}
"#;

#[test]
fn build_reports_a_template_level_kinematic_warning_once_not_per_instance() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(BAD_BODY_ID_SOURCE);
    let result = engine_with_occt().build(&compiled, ExportFormat::Step);

    let body_id_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("min_clearance") && d.message.contains("body_a"))
        .collect();
    assert_eq!(
        body_id_warnings.len(),
        1,
        "the bad body id is reported once, not once per sub instance: {body_id_warnings:#?}"
    );
}

#[test]
fn tessellate_and_build_snapshot_agree_with_build_on_sub_instance_cells() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_no_errors(INSTANCE_SCOPE_SOURCE);

    let tessellated = engine_with_occt().tessellate_realizations(&compiled);
    assert_close(
        cell(&tessellated.values, "Parent.d", "clearance_ab"),
        0.010,
        1e-6,
        "tessellate: Parent.d.clearance_ab",
    );
    assert_eq!(
        cell(&tessellated.values, "Parent.d", "collide_ab"),
        &Value::Bool(false),
        "tessellate: Parent.d.collide_ab"
    );

    let mut engine = engine_with_occt();
    let _eval_result = engine.eval(&compiled);
    let snapshot_build = engine
        .build_snapshot(&compiled, ExportFormat::Step)
        .expect("build_snapshot after eval must produce a result");
    assert_close(
        cell(&snapshot_build.values, "Parent.d", "clearance_ab"),
        0.010,
        1e-6,
        "build_snapshot: Parent.d.clearance_ab",
    );
    assert_eq!(
        cell(&snapshot_build.values, "Parent.d", "collide_ab"),
        &Value::Bool(false),
        "build_snapshot: Parent.d.collide_ab"
    );
}
