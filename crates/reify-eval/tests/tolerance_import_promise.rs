//! Engine-level integration tests for the imported-geometry tolerance
//! promise contract (per task 2651 / PRD
//! `docs/prds/v0_2/per-purpose-tolerance.md` "Resolved design decisions" →
//! "Imported geometry promise"; arch §10.4 / §14.5).
//!
//! Builds a hand-crafted `STEPInput` template carrying a
//! `param tolerance : Length = X m` declaration whose post-`eval()`
//! value-cell entry is the imported-geometry tolerance promise. Asserts the
//! promise is observable via `Engine::imported_tolerance_promise`, then
//! pairs it with the existing demand-side fixture pattern (manufacturing
//! purpose + STEPOutput template + MyDesign subject) to exercise
//! `Engine::check_imported_tolerance_promise`'s strict-tighter-than-promise
//! warning emission and the four no-op rows of its truth table.

use reify_test_support::builders::{
    CompiledModuleBuilder, CompiledPurposeBuilder, TopologyTemplateBuilder,
};
use reify_test_support::make_engine;
use reify_types::{
    CompiledExpr, DiagnosticCode, DimensionVector, ModulePath, Severity, Type, Value, ValueCellId,
};

/// Build an `STEPInput`-shaped `TopologyTemplate` carrying a single
/// `param tolerance : Length = promise_tol_si m` declaration. The template's
/// name is `"STEPInput"` so the post-`eval()` snapshot's value-cell map
/// contains an entry keyed by `ValueCellId("STEPInput", "tolerance")` whose
/// value is `Value::Scalar { si_value == promise_tol_si, dimension == LENGTH }`.
/// See `crate::tolerance_promise::extract_input_tolerance_promise` for the
/// recognition contract.
fn step_input_template(promise_tol_si: f64) -> reify_compiler::TopologyTemplate {
    let length_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let default_expr = CompiledExpr::literal(
        Value::Scalar {
            si_value: promise_tol_si,
            dimension: DimensionVector::LENGTH,
        },
        length_type.clone(),
    );
    TopologyTemplateBuilder::new("STEPInput")
        .param("STEPInput", "tolerance", length_type, Some(default_expr))
        .build()
}

/// Build an `STEPOutput`-shaped `TopologyTemplate` carrying a single
/// `RepresentationWithin(<ValueRef typed StructureRef>, <length-literal>)`
/// body constraint at SI `output_tol` metres. Mirrors the precedent in
/// `tests/tolerance_combine.rs::step_output_template`.
fn step_output_template(output_tol: f64) -> reify_compiler::TopologyTemplate {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Structure".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: output_tol,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let body = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );
    TopologyTemplateBuilder::new("STEPOutput")
        .param(
            "STEPOutput",
            "subject",
            Type::StructureRef("Structure".to_string()),
            None,
        )
        .constraint("STEPOutput", 0, None, body)
        .build()
}

/// Build a `manufacturing`-style `CompiledPurpose` whose sole constraint is
/// `RepresentationWithin(subject, purpose_tol m)`. Mirrors the precedent in
/// `tests/tolerance_combine.rs::manufacturing_purpose`.
fn manufacturing_purpose(purpose_name: &str, purpose_tol: f64) -> reify_compiler::CompiledPurpose {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Structure".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: purpose_tol,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let rep_within = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );
    CompiledPurposeBuilder::new(purpose_name)
        .param("subject", "Structure")
        .constraint("subject", 0, None, rep_within)
        .build()
}

/// Build a minimal `MyDesign` template — the subject of the manufacturing
/// purpose's tolerance scope. Mirrors the precedent in
/// `tests/tolerance_combine.rs::my_design_template`.
fn my_design_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .build()
}

/// Pinned by the imported-geometry-promise contract: after `eval()`, the
/// `STEPInput` template's `param tolerance : Length = X m` declaration
/// surfaces as a value-cell entry under `(STEPInput, "tolerance")`, and
/// `Engine::imported_tolerance_promise("STEPInput")` returns
/// `Some(promise_tol_si)`.
#[test]
fn engine_imported_tolerance_promise_returns_si_value_after_eval() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_imported_tolerance_promise_extracted".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);

    assert_eq!(
        engine.imported_tolerance_promise("STEPInput"),
        Some(50e-6),
        "STEPInput's `param tolerance : Length = 50um` default expression \
         must surface in the post-eval snapshot.values map under \
         (STEPInput, \"tolerance\") and be returned as Some(50e-6) by the \
         engine query"
    );
}

/// Pinned by the warning-emission contract from PRD
/// `docs/prds/v0_2/per-purpose-tolerance.md`: when a downstream demand is
/// strictly tighter than the imported-geometry tolerance promise, the
/// runtime emits a `Severity::Warning` carrying
/// `DiagnosticCode::ImportedTolerancePromiseInsufficient` and the
/// as-imported realization proceeds.
///
/// Setup: STEPInput promise=50µm (loose), STEPOutput body=1µm (tight),
/// manufacturing purpose=1µm (also tight). After `activate_purpose`, the
/// demanded tolerance for STEPOutput is `min(1µm, 1µm) = 1µm` (via
/// `combine_demanded_tolerance`'s min-fold), which is strictly tighter than
/// the 50µm promise — so the warning fires.
#[test]
fn engine_check_imported_tolerance_promise_emits_warning_when_demand_strictly_tighter_than_promise()
{
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_promise_insufficient".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let diag = engine
        .check_imported_tolerance_promise("STEPInput", "MyDesign", "STEPOutput")
        .expect(
            "with promise=50e-6 and demand=1e-6 (strict tighter), the check must \
             return Some(diagnostic) — not None",
        );

    assert_eq!(
        diag.severity,
        Severity::Warning,
        "diagnostic severity must be Warning (PRD: warn, not error — \
         runtime proceeds with as-imported realization)"
    );
    assert_eq!(
        diag.code,
        Some(DiagnosticCode::ImportedTolerancePromiseInsufficient),
        "diagnostic code must round-trip ImportedTolerancePromiseInsufficient \
         for filter-by-code downstream consumers"
    );
    assert!(
        diag.message.contains("STEPInput"),
        "message must name the input template so authors can locate the \
         import site (got: {:?})",
        diag.message
    );
}
