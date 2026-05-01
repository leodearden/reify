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

use reify_test_support::builders::TopologyTemplateBuilder;
use reify_types::{CompiledExpr, DimensionVector, Type, Value};

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

/// Pinned by the imported-geometry-promise contract: after `eval()`, the
/// `STEPInput` template's `param tolerance : Length = X m` declaration
/// surfaces as a value-cell entry under `(STEPInput, "tolerance")`, and
/// `Engine::imported_tolerance_promise("STEPInput")` returns
/// `Some(promise_tol_si)`.
#[test]
fn engine_imported_tolerance_promise_returns_si_value_after_eval() {
    use reify_test_support::builders::CompiledModuleBuilder;
    use reify_test_support::make_engine;
    use reify_types::ModulePath;

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
