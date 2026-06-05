//! Task β behavioural eval signals for `std.ports.mechanical` (PRD
//! docs/prds/v0_6/ports-breadth-expansion.md task β).
//!
//! Two user-observable signals, both via INLINE source that LOCALLY re-declares
//! the relevant definitions. `compile_with_stdlib` does not export stdlib
//! structure templates into the user `CompiledModule`, so the eval engine cannot
//! resolve a `sub spec = ThreadSpec(...)` against the stdlib def — documented in
//! examples/m8_tolerancing.ri:7-29 and PRD §8 ("Eval-time sub re-declaration").
//! The established workaround is local re-declaration (m8_tolerancing.ri
//! re-declares DimensionalTolerance with its computed lets). The stdlib surface
//! itself (ThreadSpec's four lets + RHS; the dof==1 constraints) is pinned
//! directly against the loaded module by
//! reify-compiler/tests/ports_stdlib_compile.rs; here we exercise the runtime
//! readouts that surface produces:
//!
//!   1. thread_spec_derived_lets_eval — ThreadSpec's four ISO 68-1 derived lets
//!      resolve to the M6×1 standards-table values (this file).
//!   2. linear_guide_port_dof_constraint_violation — the dof==1 invariant rejects
//!      a degrees_of_freedom == 2 conformer at check() time (added in step-15).

use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::make_simple_engine;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse inline `source`, compile with stdlib (asserting no Severity::Error at
/// parse + compile), eval with `SimpleConstraintChecker` (asserting no eval
/// errors), and return the full `EvalResult`. Mirrors `eval_ri_file` in
/// m8_3_stdlib_integration.rs but for inline source rather than a .ri file.
fn eval_inline(source: &str, module_name: &str) -> reify_eval::EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    result
}

/// Fetch a Scalar cell from the eval result and assert its si_value (abs tol
/// 1e-9) and dimension. Mirrors `assert_scalar_cell` in
/// m8_3_stdlib_integration.rs.
fn assert_scalar_cell(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
    expected_si: f64,
    expected_dim: DimensionVector,
) {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, member));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - expected_si).abs() < 1e-9,
                "{}.{}: expected si_value ≈{}, got {}",
                entity,
                member,
                expected_si,
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "{}.{}: wrong dimension",
                entity, member
            );
        }
        other => panic!(
            "{}.{} should be Value::Scalar, got {:?}",
            entity, member, other
        ),
    }
}

// ─── step-13/14 (task β): ThreadSpec derived-let eval readout ─────────────────

/// Inline fixture: locally re-declares the three thread enums + a ThreadSpec
/// structure mirroring the stdlib def, plus a Holder whose `sub spec` is an M6×1
/// thread (nominal Ø 6mm, pitch 1mm, ISO metric, class 6g/6H).
///
/// step-13 (RED) ships this WITHOUT the four derived lets, so the
/// minor_diameter/pitch_diameter/tap_drill cells are absent from the eval result
/// → assert_scalar_cell panics on the first missing cell. step-14 (GREEN) adds
/// the four lets so the M6×1 readouts resolve to the ISO standards-table values.
///
/// `thread_form : Option<Geometry>` (a none-valued carrier slot) is intentionally
/// omitted from this fixture — it is irrelevant to the derived-let signal and the
/// full stdlib shape is pinned by ports_stdlib_compile.rs::thread_spec_structure_surface.
const THREAD_SPEC_FIXTURE: &str = r#"
enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
enum ThreadClass { Class_6g6H, Class_4g6H }
enum ThreadTighteningDirection { Clockwise, Counterclockwise }

structure def ThreadSpec {
    param system : ThreadSystem
    param nominal_diameter : Length
    param pitch : Length
    param thread_class : ThreadClass
    param tightening : ThreadTighteningDirection = ThreadTighteningDirection.Clockwise
}

structure def Holder {
    sub spec = ThreadSpec(
        system: ThreadSystem.ISO_Metric,
        nominal_diameter: 6mm,
        pitch: 1mm,
        thread_class: ThreadClass.Class_6g6H
    )
}
"#;

/// PRD task-β `reify eval` signal: the ThreadSpec derived lets resolve to the
/// M6×1 ISO standards-table values (60°-flank ISO 68-1 identities):
///   minor_diameter = 6 − 1·1.0825 = 4.9175mm = 0.0049175m  (D − 1.25H, H=0.866P)
///   pitch_diameter = 6 − 1·0.6495 = 5.3505mm = 0.0053505m  (D − 0.75H)
///   tap_drill      = 6 − 1        = 5mm      = 0.005m       (~75%-thread rule)
///
/// RED (step-13): the fixture omits the derived lets → the cells are absent.
#[test]
fn thread_spec_derived_lets_eval() {
    let result = eval_inline(THREAD_SPEC_FIXTURE, "ports_mechanical_thread_eval");

    assert_scalar_cell(
        &result,
        "Holder.spec",
        "minor_diameter",
        0.0049175,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "Holder.spec",
        "pitch_diameter",
        0.0053505,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "Holder.spec",
        "tap_drill",
        0.005,
        DimensionVector::LENGTH,
    );
}
