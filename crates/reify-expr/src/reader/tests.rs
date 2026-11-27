use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

use super::CellReader;

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("Probe", member)
}

fn store() -> ValueMap {
    let mut store = ValueMap::new();
    store.insert(cell("x"), Value::Int(5));
    store.insert(cell("y"), Value::Int(6));
    store
}

fn entries(map: &ValueMap) -> Vec<(ValueCellId, Value)> {
    let mut entries: Vec<_> = map.iter().map(|(id, v)| (id.clone(), v.clone())).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn a_value_map_reader_reads_a_stored_cell_and_undef_for_any_other() {
    let store = store();
    let reader: &dyn CellReader = &store;
    assert_eq!(reader.read(&cell("x")), Value::Int(5));
    assert_eq!(reader.read(&cell("absent")), Value::Undef);
}

#[test]
fn a_value_map_reader_is_its_own_map() {
    let store = store();
    let reader: &dyn CellReader = &store;
    assert_eq!(entries(reader.as_value_map()), entries(&store));
}
