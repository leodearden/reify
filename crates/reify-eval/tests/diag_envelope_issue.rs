#![allow(clippy::mutable_key_type)]
//! Temporary diagnostic test - do not commit
//! Writes debug output to /tmp/reify_envelope_debug.txt

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

    let mut output = String::new();

    let errors: Vec<_> = eval_result.diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    output.push_str(&format!("ERRORS: {:?}\n", errors));

    let warnings: Vec<_> = eval_result.diagnostics.iter()
        .filter(|d| d.severity != Severity::Error)
        .collect();
    output.push_str(&format!("WARNINGS/INFO count: {}\n", warnings.len()));

    let results_cell = ValueCellId::new("MultiLoadBracket", "results");
    let results_val = eval_result.values.get(&results_cell);
    output.push_str(&format!("RESULTS type: {:?}\n", results_val.map(std::mem::discriminant)));
    if let Some(Value::Map(outer)) = results_val {
        output.push_str(&format!("RESULTS outer keys: {:?}\n", outer.keys().collect::<Vec<_>>()));
        if let Some(Value::Map(inner)) = outer.get(&Value::String("cases".to_string())) {
            output.push_str(&format!("CASES count: {}\n", inner.len()));
            for (k, v) in inner {
                output.push_str(&format!("  CASE {:?}: {:?}\n", k, std::mem::discriminant(v)));
                if let Value::StructureInstance(data) = v {
                    let fields: Vec<&String> = data.fields.keys().collect();
                    output.push_str(&format!("    FIELDS: {:?}\n", fields));
                    if let Some(stress) = data.fields.get("stress") {
                        output.push_str(&format!("    STRESS type: {:?}\n", std::mem::discriminant(stress)));
                        if let Value::Field { source, codomain_type, lambda, .. } = stress {
                            output.push_str(&format!("    STRESS source: {:?}\n", source));
                            output.push_str(&format!("    STRESS codomain: {:?}\n", codomain_type));
                            if let Value::SampledField(sf) = lambda.as_ref() {
                                output.push_str(&format!("    STRESS data.len={}\n", sf.data.len()));
                                output.push_str(&format!("    STRESS axis_grids lengths: {:?}\n",
                                    sf.axis_grids.iter().map(|g| g.len()).collect::<Vec<_>>()));
                                let grid_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
                                output.push_str(&format!("    STRESS grid_count={} data_check={}\n",
                                    grid_count, sf.data.len() == grid_count * 9));
                            }
                        }
                    }
                }
            }
        }
    }

    let envelope_cell = ValueCellId::new("MultiLoadBracket", "envelope");
    let envelope_val = eval_result.values.get(&envelope_cell);
    output.push_str(&format!("ENVELOPE type: {:?}\n", envelope_val.map(std::mem::discriminant)));

    let peak_stress_cell = ValueCellId::new("MultiLoadBracket", "peak_stress");
    let peak_stress_val = eval_result.values.get(&peak_stress_cell);
    output.push_str(&format!("PEAK_STRESS type: {:?}\n", peak_stress_val.map(std::mem::discriminant)));

    // Write to file and fail
    std::fs::write("/tmp/reify_envelope_debug.txt", &output).expect("failed to write debug file");

    panic!("Diagnostic complete — check /tmp/reify_envelope_debug.txt");
}
