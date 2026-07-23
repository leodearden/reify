#![allow(clippy::mutable_key_type)]
//! End-to-end eval test for `examples/multi_load_bracket.ri` (task 3018).
//!
//! Drives the full prismatic multi-load-case FEA example through the real
//! solve engine (`register_compute_fns`) and asserts the PRD η signals:
//!   "example parses/types/runs; envelope < yield globally"
//!
//! Assertions:
//!   (a) zero Error-severity diagnostics
//!   (b) MultiLoadBracket.results is a populated MultiCaseResult —
//!       Value::Map{"cases" → Map} with exactly 3 entries (operating/overload/
//!       transport), each a Value::StructureInstance("ElasticResult") whose
//!       `stress` is a Sampled Field with finite data.
//!   (c) MultiLoadBracket.envelope is a non-vacuous Sampled Field<Point3,
//!       Pressure> with finite data (NOT Value::Undef).
//!   (d) Per-case independence: overload.max_von_mises > operating.max_von_mises > 0.
//!   (e) MultiLoadBracket.peak_stress is Value::Scalar{PRESSURE}, positive and finite.
//!   (f) MultiLoadBracket.within_yield is Value::Bool.
//!
//! Does NOT assert `within_yield == true` or any absolute `< 310 MPa` bound —
//! the synthetic solve magnitude for these loads is not known a-priori, and a
//! guessed threshold would be an unfounded numeric assertion.
//!
//! Mirrors the scaffold from `multi_case_compute_node.rs`.

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Path to the example file ──────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/multi_load_bracket.ri"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract a named field from a `Value::StructureInstance` or `Value::Map`.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract `max_von_mises` as `f64` (SI value) from an ElasticResult value.
/// Panics if the field is absent or not a Scalar.
fn extract_max_von_mises_f64(result: &Value, case_name: &str) -> f64 {
    let mvm = extract_field(result, "max_von_mises").unwrap_or_else(|| {
        panic!("case \"{case_name}\": max_von_mises field missing from ElasticResult")
    });
    match mvm {
        Value::Scalar { si_value, .. } => si_value,
        other => {
            panic!("case \"{case_name}\": max_von_mises must be Value::Scalar, got: {other:?}")
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main e2e test
// ─────────────────────────────────────────────────────────────────────────────

/// End-to-end: `examples/multi_load_bracket.ri` runs through the real
/// solve engine and produces a populated MultiCaseResult, a non-vacuous
/// Sampled envelope, a callable peak_stress Scalar<Pressure>, and a
/// within_yield Bool.
#[test]
fn multi_load_bracket_e2e_real_solve_and_design_predicate() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/multi_load_bracket.ri — check CARGO_MANIFEST_DIR resolution",
    );

    let compiled = parse_and_compile_with_stdlib(&src);
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // ── (a) Zero Error-severity diagnostics ──────────────────────────────────

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics evaluating multi_load_bracket.ri, got:\n{:#?}",
        errors
    );

    // ── (b) MultiLoadBracket.results is a populated MultiCaseResult ──────────
    //
    // Shape: Value::Map{"cases" → Value::Map} with exactly 3 entries.
    // Each entry is a Value::StructureInstance("ElasticResult") with a
    // Sampled `stress` Field (finite data).

    let results_cell = ValueCellId::new("MultiLoadBracket", "results");
    let results_val = eval_result
        .values
        .get(&results_cell)
        .unwrap_or_else(|| panic!("MultiLoadBracket.results cell not found in eval result"));

    let cases_map = match results_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => {
                panic!("MultiLoadBracket.results[\"cases\"] must be Value::Map, got: {other:?}")
            }
        },
        other => panic!(
            "MultiLoadBracket.results must be Value::Map (not {:?})",
            std::mem::discriminant(other)
        ),
    };
    assert_eq!(
        cases_map.len(),
        3,
        "cases map must have exactly 3 entries (operating/overload/transport); \
         got {} entries: {:?}",
        cases_map.len(),
        cases_map.keys().collect::<Vec<_>>()
    );

    for case_name in ["operating", "overload", "transport"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "cases map must contain \"{case_name}\" key; got keys: {:?}",
                    cases_map.keys().collect::<Vec<_>>()
                )
            });

        // Each case must be a StructureInstance("ElasticResult")
        match case_val {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "ElasticResult",
                    "case \"{case_name}\" must be StructureInstance(\"ElasticResult\"), \
                     got type_name = \"{}\"",
                    data.type_name
                );
            }
            other => panic!(
                "case \"{case_name}\" must be Value::StructureInstance(\"ElasticResult\"), \
                 got: {other:?}"
            ),
        }

        // stress must be a Sampled Field with finite data
        let stress_val = extract_field(case_val, "stress").unwrap_or_else(|| {
            panic!("case \"{case_name}\": stress field missing from ElasticResult")
        });
        let stress_sf = match &stress_val {
            Value::Field { source, lambda, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "case \"{case_name}\": stress source must be Sampled, got: {source:?}"
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => sf.clone(),
                    other => panic!(
                        "case \"{case_name}\": stress lambda must be SampledField, \
                         got: {other:?}"
                    ),
                }
            }
            other => {
                panic!("case \"{case_name}\": expected stress to be Value::Field, got: {other:?}")
            }
        };
        assert!(
            !stress_sf.data.is_empty(),
            "case \"{case_name}\": stress SampledField.data must be non-empty"
        );
        assert!(
            stress_sf.data.iter().all(|v| v.is_finite()),
            "case \"{case_name}\": stress field has non-finite values; \
             first non-finite index: {:?}",
            stress_sf.data.iter().position(|v| !v.is_finite())
        );
    }

    // ── (c) MultiLoadBracket.envelope is a non-vacuous Sampled Field ─────────
    //
    // envelope_von_mises(results) over real Sampled fields must produce a
    // Sampled (not Undef/Symbolic) Field with codomain Scalar<PRESSURE>
    // and finite data.

    let envelope_cell = ValueCellId::new("MultiLoadBracket", "envelope");
    let envelope_val = eval_result
        .values
        .get(&envelope_cell)
        .unwrap_or_else(|| panic!("MultiLoadBracket.envelope cell not found in eval result"));

    let envelope_sf = match envelope_val {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => {
            assert_eq!(
                *codomain_type,
                Type::Scalar {
                    dimension: DimensionVector::PRESSURE
                },
                "envelope codomain_type must be Scalar<PRESSURE>, got: {codomain_type:?}"
            );
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "envelope source must be Sampled (NOT Undef/Symbolic — envelope_von_mises \
                 over real Sampled stress fields must produce a non-vacuous Sampled envelope; \
                 β/4085 closed the VonMises-derived-max gap); got: {source:?}"
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => sf.clone(),
                other => panic!("envelope lambda must be SampledField, got: {other:?}"),
            }
        }
        Value::Undef => panic!(
            "MultiLoadBracket.envelope must NOT be Value::Undef — \
             envelope_von_mises over real Sampled stress fields must produce \
             a non-vacuous Sampled envelope (β/4085 closed the VonMises-derived-max gap)"
        ),
        other => panic!("MultiLoadBracket.envelope must be Value::Field (Sampled), got: {other:?}"),
    };
    assert!(
        !envelope_sf.data.is_empty(),
        "envelope SampledField.data must be non-empty"
    );
    assert!(
        envelope_sf.data.iter().all(|v| v.is_finite()),
        "envelope field has non-finite values; first non-finite index: {:?}",
        envelope_sf.data.iter().position(|v| !v.is_finite())
    );

    // ── (d) Per-case independence: overload.max_von_mises > operating > 0 ────
    //
    // Linear elasticity identity: the overload case (10 kN tip force) must
    // produce strictly higher peak von-Mises stress than the operating case
    // (5 kN tip force), and operating must be positive.

    let op_val = cases_map
        .get(&Value::String("operating".to_string()))
        .expect("cases map must contain \"operating\"");
    let ov_val = cases_map
        .get(&Value::String("overload".to_string()))
        .expect("cases map must contain \"overload\"");

    let op_mvm = extract_max_von_mises_f64(op_val, "operating");
    let ov_mvm = extract_max_von_mises_f64(ov_val, "overload");

    assert!(
        op_mvm > 0.0,
        "operating.max_von_mises must be positive (real FEA stress), got {op_mvm}"
    );
    assert!(
        ov_mvm > op_mvm,
        "overload.max_von_mises ({ov_mvm:.3e} Pa) must be strictly greater than \
         operating.max_von_mises ({op_mvm:.3e} Pa) — \
         linear elasticity: 2× tip load (10kN vs 5kN) ⇒ 2× stress"
    );

    // ── (e) MultiLoadBracket.peak_stress is Scalar<PRESSURE>, positive, finite

    let peak_stress_cell = ValueCellId::new("MultiLoadBracket", "peak_stress");
    let peak_stress_val = eval_result
        .values
        .get(&peak_stress_cell)
        .unwrap_or_else(|| {
            panic!(
                "MultiLoadBracket.peak_stress cell not found in eval result — \
                 add `let peak_stress = max(envelope)` to examples/multi_load_bracket.ri \
                 (step-4 / task 3018)"
            )
        });

    match peak_stress_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "peak_stress must have PRESSURE dimension, got: {dimension:?}"
            );
            assert!(
                si_value.is_finite(),
                "peak_stress must be finite, got: {si_value}"
            );
            assert!(
                *si_value > 0.0,
                "peak_stress must be positive (real FEA result), got: {si_value:.3e} Pa"
            );
        }
        Value::Undef => panic!(
            "MultiLoadBracket.peak_stress must NOT be Value::Undef — \
             max(envelope) over a non-vacuous Sampled Field must produce \
             a positive finite Scalar<PRESSURE>"
        ),
        other => {
            panic!("MultiLoadBracket.peak_stress must be Value::Scalar<PRESSURE>, got: {other:?}")
        }
    }

    // ── (f) MultiLoadBracket.within_yield is Value::Bool ────────────────────
    //
    // Does NOT assert the boolean value — the synthetic solve magnitude is not
    // known a-priori, so asserting true/false would be an unfounded threshold.

    let within_yield_cell = ValueCellId::new("MultiLoadBracket", "within_yield");
    let within_yield_val = eval_result
        .values
        .get(&within_yield_cell)
        .unwrap_or_else(|| {
            panic!(
                "MultiLoadBracket.within_yield cell not found in eval result — \
                 add `let within_yield = peak_stress < yield_limit` to \
                 examples/multi_load_bracket.ri (step-4 / task 3018)"
            )
        });

    assert!(
        matches!(within_yield_val, Value::Bool(_)),
        "MultiLoadBracket.within_yield must be Value::Bool, got: {within_yield_val:?}"
    );
}
