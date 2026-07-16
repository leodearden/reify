use reify_core::ValueCellId;
use reify_test_support::{compile_source, make_simple_engine};

const SRC_N4: &str = r#"structure Bolt {
    param diameter : Length = 10mm
    let is_diameter_determined = determined(diameter)
}
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}"#;

#[test]
fn scratch_dump_bolts() {
    let compiled = compile_source(SRC_N4);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    for i in 0..4 {
        let struct_name = format!("S.bolts[{i}]");
        let diam_id = ValueCellId::new(&struct_name, "diameter");
        let det_id = ValueCellId::new(&struct_name, "is_diameter_determined");
        println!(
            "bolts[{i}] diameter={:?} is_diameter_determined={:?}",
            result.values.get(&diam_id),
            result.values.get(&det_id)
        );
    }
    let snapshot = engine.snapshot().expect("snapshot");
    for i in 0..4 {
        let struct_name = format!("S.bolts[{i}]");
        let diam_id = ValueCellId::new(&struct_name, "diameter");
        let det_id = ValueCellId::new(&struct_name, "is_diameter_determined");
        println!(
            "snapshot bolts[{i}] diameter={:?} is_diameter_determined={:?}",
            snapshot.values.get(&diam_id),
            snapshot.values.get(&det_id)
        );
    }
    panic!("dump done");
}
