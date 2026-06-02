//! DSL-down eval test: shell_channels surfacing (task #4067, step-1 RED → step-2 GREEN).
//!
//! Evaluates `examples/fea_shell_channels.ri` and asserts:
//!   - No Error diagnostics (clean compile + eval).
//!   - `result.shell_channels.top/.mid/.bottom` are non-Undef `Value::Field`s.
//!   - `result.shell_channels.mid == result.stress` (I-2 identity).
//!
//! Note on I-4 (von_mises field path): Reify field lambda bodies only support
//! scalar-valued expressions; Tensor-typed codomains cannot be produced from
//! DSL-authored lambdas. The fixture therefore uses Real->Real fields, so
//! von_mises(shell_channels.top) returns Undef (not a tensor field).
//! I-4 is covered by field_analysis_tests.rs via Rust-constructed analytical
//! tensor fields — no additional DSL fixture coverage is needed here.
//!
//! RED until step-2 adds `param shell_channels : ShellStress` to `ElasticResult`
//! in `crates/reify-compiler/stdlib/solver_elastic.ri`.

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

fn shell_channels_source() -> &'static str {
    include_str!("../../../examples/fea_shell_channels.ri")
}

/// Extract a named field from a StructureInstance (one level).
fn extract_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(key.to_string())).cloned(),
        _ => None,
    }
}

/// Extract a nested field from a StructureInstance (two levels: outer.inner).
fn extract_nested(val: &Value, outer_key: &str, inner_key: &str) -> Option<Value> {
    let outer_val = extract_field(val, outer_key)?;
    extract_field(&outer_val, inner_key)
}

// ── step-1(a): top / mid / bottom are non-Undef Fields ──────────────────────

/// DSL-down gate: ElasticResult.shell_channels carries non-Undef top/mid/bottom.
///
/// Fails in RED state (before step-2) because ElasticResult has no `shell_channels`
/// param yet — accessing it returns Undef and `extract_nested` returns None.
#[test]
fn shell_channels_top_mid_bottom_are_non_undef_fields() {
    let source = shell_channels_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from fea_shell_channels.ri, got: {:?}",
        errors
    );

    // (b) result cell is a StructureInstance.
    let result_cell = ValueCellId::new("FeaShellChannels", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaShellChannels.result not found in eval result"));

    // (c) result.shell_channels.top / .mid / .bottom must be non-Undef Value::Fields.
    for channel in &["top", "mid", "bottom"] {
        let field_val =
            extract_nested(result_val, "shell_channels", channel).unwrap_or_else(|| {
                panic!(
                    "result.shell_channels.{channel} not found in ElasticResult; \
                     this is the expected RED failure until step-2 adds \
                     `param shell_channels : ShellStress` to ElasticResult in \
                     solver_elastic.ri"
                )
            });
        assert!(
            !matches!(&field_val, Value::Undef),
            "result.shell_channels.{channel} should be non-Undef (Value::Field), got Undef"
        );
        assert!(
            matches!(&field_val, Value::Field { .. }),
            "result.shell_channels.{channel} should be Value::Field, got: {:?}",
            field_val
        );
    }
}

// ── step-1(b): I-2 — shell_channels.mid == stress ──────────────────────────

/// I-2: result.shell_channels.mid must equal result.stress (same value by identity).
///
/// Fails in RED state because shell_channels is absent from ElasticResult.
#[test]
fn shell_channels_mid_equals_stress() {
    let source = shell_channels_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    let result_cell = ValueCellId::new("FeaShellChannels", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaShellChannels.result not found"));

    let stress = extract_field(result_val, "stress")
        .unwrap_or_else(|| panic!("ElasticResult.stress field not found"));
    let mid = extract_nested(result_val, "shell_channels", "mid").unwrap_or_else(|| {
        panic!(
            "result.shell_channels.mid not found; this is the expected RED failure \
                 until step-2 adds `param shell_channels : ShellStress`"
        )
    });

    assert_eq!(
        stress, mid,
        "I-2: result.shell_channels.mid must equal result.stress \
         (both bound to the same `mid_f` analytical field in the fixture)"
    );
}
