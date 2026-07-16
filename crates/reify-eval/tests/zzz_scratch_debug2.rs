use reify_core::ValueCellId;
use reify_test_support::{compile_source, make_simple_engine};

const SRC_PLAIN: &str = r#"structure S {
    param diameter : Length = 10mm
    let is_diameter_determined = determined(diameter)
}"#;

#[test]
fn scratch_dump_plain() {
    let compiled = compile_source(SRC_PLAIN);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let det_id = ValueCellId::new("S", "is_diameter_determined");
    println!("PLAIN is_diameter_determined={:?}", result.values.get(&det_id));
    panic!("dump done");
}
