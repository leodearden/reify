//! Task η integration-gate: multi-domain example + eval/diagnostic assertions
//! (PRD docs/prds/v0_6/ports-breadth-expansion.md §7 η).
//!
//! Exercises the shared example file `examples/stdlib/ports_breadth.ri` through
//! the full parse → compile_with_stdlib → eval pipeline and asserts three η
//! signals in TDD order:
//!
//!   a. `thread_spec_derived_lets_eval_from_example` — ThreadSpec ISO 68-1
//!      derived lets (M6×1) resolve to standards-table values.
//!   b. `asymmetric_located_port_warning_fires` — a MechanicalPort (located) ↔
//!      ThermalPort (non-located) bare connect emits the asymmetric-LocatedPort
//!      warning (connect.rs:344).
//!   c. `hydraulic_port_multidomain_conforms` — HydroConformer's port p
//!      type_name == "HydraulicPort" (FluidPort+MechanicalPort diamond resolves,
//!      zero Error diagnostics).
//!
//! EVAL-TIME RE-DECLARATION NOTE: compile_with_stdlib provides stdlib
//! definitions as a compilation prelude but only user templates appear in the
//! output CompiledModule. The eval engine resolves sub/structure constructions
//! exclusively from user templates, so every structure constructed at eval time
//! (ThreadSpec, Frame3) must be locally re-declared in the example .ri file.
//! This is the established m8_tolerancing.ri / ports_mechanical_thread_eval.rs
//! workaround (PRD §8 "Eval-time sub re-declaration").

use reify_compiler::{CompiledModule, EntityKind};
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::make_simple_engine;

// ── File path ─────────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/stdlib/ports_breadth.ri"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read `examples/stdlib/ports_breadth.ri`, parse, compile with stdlib
/// (asserting no Severity::Error at parse + compile), eval with
/// `SimpleConstraintChecker` (asserting no eval errors), and return the full
/// `EvalResult`. Mirrors `eval_ri_file` in m8_3_stdlib_integration.rs.
fn eval_breadth() -> reify_eval::EvalResult {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));

    let parsed = reify_syntax::parse(&source, ModulePath::single("ports_breadth"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in ports_breadth.ri: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in ports_breadth.ri: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in ports_breadth.ri: {:?}",
        eval_errors
    );

    result
}

/// Read `examples/stdlib/ports_breadth.ri`, parse, compile with stdlib, and
/// return the `CompiledModule` WITHOUT asserting on warnings — so warning
/// diagnostics are inspectable by the caller. Panics only on Severity::Error at
/// parse or compile stages.
fn compile_breadth() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));

    let parsed = reify_syntax::parse(&source, ModulePath::single("ports_breadth"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in ports_breadth.ri: {:?}",
        parsed.errors
    );

    reify_compiler::compile_with_stdlib(&parsed)
}

/// Fetch a Scalar cell from the eval result and assert its si_value (abs tol
/// 1e-9) and dimension. Mirrors `assert_scalar_cell` in
/// m8_3_stdlib_integration.rs / ports_mechanical_thread_eval.rs.
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

// ─── step-1/2 (task η signal a): ThreadSpec derived-let eval readout ─────────

/// PRD task-η signal (a): ThreadSpec derived lets (M6×1) resolve to ISO 68-1
/// standards-table values via the locally-re-declared ThreadSpec + ThreadAssembly
/// in ports_breadth.ri:
///   minor_diameter = 6 − 1·1.0825 = 4.9175mm = 0.0049175m
///   pitch_diameter = 6 − 1·0.6495 = 5.3505mm = 0.0053505m
///   tap_drill      = 6 − 1        = 5mm      = 0.005m
///
/// RED (step-1): ports_breadth.ri does not exist → read_to_string panics.
#[test]
fn thread_spec_derived_lets_eval_from_example() {
    let result = eval_breadth();

    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "minor_diameter",
        0.0049175,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "pitch_diameter",
        0.0053505,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "tap_drill",
        0.005,
        DimensionVector::LENGTH,
    );
}

// ─── step-3/4 (task η signal b): asymmetric LocatedPort warning ───────────────

/// PRD task-η signal (b): connecting a stdlib MechanicalPort (LocatedPort) to a
/// ThermalPort (non-located) via a bare port name emits the asymmetric-LocatedPort
/// warning (connect.rs:344).
///
/// Mirrors connect_compile_tests.rs::asymmetric_located_port_emits_warning but
/// exercises stdlib-derived trait-typed ports rather than inline ad-hoc traits —
/// the integration point the PRD §0 highlights ("previously unreachable from
/// stdlib-derived ports").
///
/// RED (step-3): ports_breadth.ri has no asymmetric connect yet → no warning.
#[test]
fn asymmetric_located_port_warning_fires() {
    let compiled = compile_breadth();

    let located_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("asymmetric")
                && d.message.contains("LocatedPort")
        })
        .collect();

    assert!(
        !located_warnings.is_empty(),
        "expected a warning about asymmetric LocatedPort connection in ports_breadth.ri \
         (MechanicalPort [located] ↔ ThermalPort [non-located]), got diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ─── step-5/6 (task η signal c): HydraulicPort multi-domain conformance ───────

/// PRD task-η signal (c): HydroConformer's port p resolves to type_name
/// "HydraulicPort" (FluidPort + MechanicalPort multi-domain diamond), and the
/// whole file compiles with zero Severity::Error diagnostics.
///
/// Mirrors ports_stdlib_compile.rs::hydraulic_port_concrete_conformer_multidomain_compiles
/// + the m8_3 ports port type_name assertion idiom.
///
/// RED (step-5): HydroConformer does not exist in ports_breadth.ri yet →
/// the find on compiled.templates panics / asserts.
#[test]
fn hydraulic_port_multidomain_conforms() {
    let compiled = compile_breadth();

    // (i) Zero Error diagnostics for the whole file.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "ports_breadth.ri should have zero Error diagnostics; got: {:?}",
        errors
    );

    // (ii) HydroConformer is declared as a Structure template.
    let hydro = compiled
        .templates
        .iter()
        .find(|t| t.name == "HydroConformer" && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "HydroConformer (EntityKind::Structure) not found in ports_breadth.ri; \
                 templates: {:?}",
                compiled
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        });

    // (iii) Port p type_name == "HydraulicPort".
    let port_p = hydro
        .ports
        .iter()
        .find(|p| p.name == "p")
        .expect("HydroConformer should have a port named 'p'");
    assert_eq!(
        port_p.type_name, "HydraulicPort",
        "HydroConformer.p port type_name should be 'HydraulicPort' (FluidPort+MechanicalPort \
         multi-domain diamond), got '{}'",
        port_p.type_name
    );
}
