use reify_core::ValueCellId;
use reify_test_support::{compile_source, make_simple_engine};
use reify_ir::Value;

const SRC_N2: &str = r#"structure Bolt {
    param diameter : Length = 10mm
    let is_diameter_determined = determined(diameter)
}
structure S {
    param n : Int = 2
    sub bolts : List<Bolt>
    constraint bolts.count == n
}"#;

#[test]
fn scratch_dump_warm_grow() {
    let compiled = compile_source(SRC_N2);
    let mut engine = make_simple_engine();
    engine.eval(&compiled);
    let n_id = ValueCellId::new("S", "n");
    let warm_result = engine.edit_param(n_id, Value::Int(4)).expect("edit_param(n,4) ok");
    for i in 0..4 {
        let struct_name = format!("S.bolts[{i}]");
        let diam_id = ValueCellId::new(&struct_name, "diameter");
        let det_id = ValueCellId::new(&struct_name, "is_diameter_determined");
        println!(
            "WARM bolts[{i}] diameter={:?} is_diameter_determined={:?}",
            warm_result.values.get(&diam_id),
            warm_result.values.get(&det_id)
        );
    }
    panic!("dump done");
}
