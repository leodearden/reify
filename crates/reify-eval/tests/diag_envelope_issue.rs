#![allow(clippy::mutable_key_type)]
//! Temporary diagnostic test - do not commit

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/multi_load_bracket.ri"
);

#[test]
fn diagnose_envelope_issue() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect("read failed");
    let compiled = parse_and_compile_with_stdlib(&src);
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result.diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    eprintln!("ERRORS: {:?}", errors);

    let results_cell = ValueCellId::new("MultiLoadBracket", "results");
    let results_val = eval_result.values.get(&results_cell);
    eprintln!("RESULTS type: {:?}", results_val.map(std::mem::discriminant));
    if let Some(Value::Map(outer)) = results_val {
        eprintln!("RESULTS outer keys: {:?}", outer.keys().collect::<Vec<_>>());
        if let Some(Value::Map(inner)) = outer.get(&Value::String("cases".to_string())) {
            eprintln!("CASES count: {}", inner.len());
            for (k, v) in inner {
                eprintln!("  CASE {:?}: {:?}", k, std::mem::discriminant(v));
                if let Value::StructureInstance(data) = v {
                    let fields: Vec<&String> = data.fields.keys().collect();
                    eprintln!("    FIELDS: {:?}", fields);
                    if let Some(Value::Field { source, lambda, .. }) = data.fields.get("stress") {
                        eprintln!("    STRESS source: {:?}", source);
                        if let Value::SampledField(sf) = lambda.as_ref() {
                            eprintln!("    STRESS data.len={} axis_grids={:?}", 
                                sf.data.len(),
                                sf.axis_grids.iter().map(|g| g.len()).collect::<Vec<_>>());
                        }
                    }
                }
            }
        }
    }

    let envelope_cell = ValueCellId::new("MultiLoadBracket", "envelope");
    let envelope_val = eval_result.values.get(&envelope_cell);
    eprintln!("ENVELOPE type: {:?}", envelope_val.map(std::mem::discriminant));
    panic!("diagnostic complete");
}
