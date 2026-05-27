//! `worst_case` Lambda-dispatch tests at the documented usage shape.
//!
//! Pins the `worst_case(mcr, |e| e["displacement"])` round-trip — the
//! ElasticResult-shaped per-case Maps with a `"displacement"` key, exactly
//! matching the doc-block contract on `eval_worst_case_dispatch` and on
//! `crates/reify-compiler/stdlib/fea_multi_case.ri`.
//!
//! Why this lives at the Rust level rather than in the compiler-driven
//! E2E smoke tests: today the Reify parser does not let users annotate
//! lambda parameters with the `ElasticResult` struct type, so the smoke
//! tests in `crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs`
//! pass per-case Sampled scalar Fields directly through an identity
//! lambda (the "lambda-syntax caveat" called out in fea_multi_case.ri's
//! doc block). That covers the dispatch arm end-to-end but does NOT pin
//! the documented `e["displacement"]` field-extraction shape that the
//! `.ri` doc promises. This file closes that gap by hand-constructing a
//! `Value::Lambda` whose body does exactly the documented `IndexAccess`
//! lookup, so the documented shape stays pinned independent of when
//! richer lambda-parameter typing lands in the parser.

#![allow(clippy::mutable_key_type)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction, SampledField, SampledGridKind, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a FunctionCall expression for stdlib functions (mirrors the helper
/// in `field_reductions_tests.rs`).
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    let hash = ContentHash::of(name.as_bytes());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: hash,
    }
}

/// Build a 1-D Sampled `Value::Field { source: Sampled, .. }` over the
/// `[0.0, 1.0, 2.0]` axis with the given `data` buffer and `Length`
/// scalar codomain (the documented `displacement` field codomain on
/// the `ElasticResult` struct).
fn make_displacement_field(name: &str, data: Vec<f64>) -> Value {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![2.0],
        spacing: vec![1.0],
        axis_grids: vec![vec![0.0, 1.0, 2.0]],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: Type::Real,
        codomain_type: length,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Build an ElasticResult-shaped `Value::Map` with the `"displacement"`
/// key bound to the given Field. Mirrors the per-case shape that
/// `crates/reify-stdlib/src/fea.rs::extract_per_case_sampled_field`
/// reads.
fn make_elastic_result_map(displacement: Value) -> Value {
    let mut m: BTreeMap<Value, Value> = BTreeMap::new();
    m.insert(Value::String("displacement".to_string()), displacement);
    Value::Map(m)
}

/// Build a MultiCaseResult-shaped outer `Value::Map` from a list of
/// `(case_name, ElasticResult-Map)` pairs. The outer Map wraps an inner
/// "cases" Map whose keys are `Value::String` (so BTreeMap iteration is
/// lexicographic on UTF-8 bytes — the tie-break invariant).
fn make_multi_case_result(cases: &[(&str, Value)]) -> Value {
    let mut inner: BTreeMap<Value, Value> = BTreeMap::new();
    for (name, er) in cases {
        inner.insert(Value::String((*name).to_string()), er.clone());
    }
    let mut outer: BTreeMap<Value, Value> = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(inner));
    Value::Map(outer)
}

/// Construct the `Value::Lambda` for the documented `|e| e["displacement"]`
/// usage shape.
///
/// Lambda body: `IndexAccess { object: ValueRef(e), index: literal("displacement") }`.
/// Captures: empty (the lambda body only reads the parameter `e`).
fn make_displacement_extractor_lambda() -> Value {
    let e_id = ValueCellId::new("$lambda0.S", "e");
    // `e["displacement"]` — IndexAccess on a Map value. The result type
    // is the documented `Field<Real, Length>` displacement codomain; in
    // practice `eval_expr` does not consult `result_type` for IndexAccess
    // at runtime, so the choice here is purely documentary.
    let body = CompiledExpr::index_access(
        CompiledExpr::value_ref(
            e_id.clone(),
            // Documentary Map<String, Real> approximating the runtime shape
            // Map<String, Field<Real, Length>> (the value-side Field codomain
            // is dropped because IndexAccess does not consult `result_type` at
            // runtime; the precise per-case shape is noted in the IndexAccess
            // result_type comment below).
            Type::Map(Box::new(Type::String), Box::new(Type::Real)),
        ),
        CompiledExpr::literal(Value::String("displacement".to_string()), Type::String),
        // Documentary result type: actual runtime shape is Field<Real, Length>
        // (the displacement Field codomain). IndexAccess does not consult
        // `result_type` at runtime, so this is a documentary annotation only.
        Type::Real,
    );
    Value::Lambda {
        params: vec![("e".to_string(), e_id)],
        body: Box::new(body),
        captures: ValueMap::new(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Documented-shape round-trip: `worst_case(mcr, |e| e["displacement"])`
/// extracts the `displacement` Field from each ElasticResult, runs
/// `field_reductions::compute_max` per case, and returns the case name
/// with the largest max. The lambda body is exactly the IndexAccess
/// chain promised by `crates/reify-compiler/stdlib/fea_multi_case.ri`.
#[test]
fn worst_case_with_displacement_extractor_lambda_returns_dominant_case_name() {
    // Three cases with distinct max displacements (50 / 200 / 100 over a
    // shared 3-grid axis). Per-case max = trailing element by construction.
    let case_a = make_elastic_result_map(make_displacement_field("a", vec![10.0, 30.0, 50.0]));
    let case_b = make_elastic_result_map(make_displacement_field("b", vec![100.0, 150.0, 200.0]));
    let case_c = make_elastic_result_map(make_displacement_field("c", vec![20.0, 60.0, 100.0]));
    let mcr = make_multi_case_result(&[
        ("operating", case_a),
        ("overload", case_b),
        ("transport", case_c),
    ]);

    let lambda = make_displacement_extractor_lambda();

    let expr = make_function_call(
        "worst_case",
        vec![
            CompiledExpr::literal(mcr, Type::Real),
            CompiledExpr::literal(lambda, Type::Real),
        ],
        Type::String,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::String("overload".to_string()),
        "worst_case with documented |e| e[\"displacement\"] shape must return \
         the case name with the largest per-case displacement max"
    );
}

/// Tie-break invariant under the documented IndexAccess usage shape:
/// when two cases share the largest max, the lex-smaller case name wins
/// (BTreeMap lexicographic iteration + strict `>` running-best
/// comparison; pinned in the dispatch arm doc-comment).
#[test]
fn worst_case_with_displacement_extractor_lambda_tie_breaks_lex_smaller_case() {
    // "alpha" and "beta" both peak at 100; "gamma" peaks at 50.
    let case_alpha =
        make_elastic_result_map(make_displacement_field("alpha", vec![10.0, 50.0, 100.0]));
    let case_beta =
        make_elastic_result_map(make_displacement_field("beta", vec![10.0, 50.0, 100.0]));
    let case_gamma =
        make_elastic_result_map(make_displacement_field("gamma", vec![10.0, 30.0, 50.0]));
    let mcr = make_multi_case_result(&[
        ("alpha", case_alpha),
        ("beta", case_beta),
        ("gamma", case_gamma),
    ]);

    let lambda = make_displacement_extractor_lambda();

    let expr = make_function_call(
        "worst_case",
        vec![
            CompiledExpr::literal(mcr, Type::Real),
            CompiledExpr::literal(lambda, Type::Real),
        ],
        Type::String,
    );

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::String("alpha".to_string()),
        "tied per-case maxes must resolve to the lex-smaller case name \
         (BTreeMap iteration + strict `>` running-best)"
    );
}
