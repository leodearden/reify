use std::collections::{BTreeSet, HashMap};

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

use super::{CellRead, CellReader, PendingReader, RoundReader};

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("Probe", member)
}

/// A store with some cells here and some not here yet.
#[derive(Default)]
struct StubStore {
    here: HashMap<ValueCellId, Value>,
    not_here_yet: BTreeSet<ValueCellId>,
}

impl PendingReader for StubStore {
    fn read(&self, id: &ValueCellId) -> CellRead {
        if self.not_here_yet.contains(id) {
            return CellRead::NotHereYet;
        }
        CellRead::Here(self.here.get(id).cloned().unwrap_or(Value::Undef))
    }
}

/// `x = 5` and `u = Undef` here, `p` not here yet.
fn stub_store() -> StubStore {
    let mut store = StubStore::default();
    store.here.insert(cell("x"), Value::Int(5));
    store.here.insert(cell("u"), Value::Undef);
    store.not_here_yet.insert(cell("p"));
    store
}

#[test]
fn a_round_reader_reads_a_cell_that_is_here_as_its_value_without_counting() {
    let store = stub_store();
    let round = RoundReader::new(&store);
    assert_eq!(round.read(&cell("x")), Value::Int(5));
    assert_eq!(round.read(&cell("u")), Value::Undef);
    assert_eq!(round.not_here_yet_count(), 0);
}

#[test]
fn a_round_reader_reads_a_cell_not_here_yet_as_undef_and_counts_each_read() {
    let store = stub_store();
    let round = RoundReader::new(&store);
    assert_eq!(round.read(&cell("p")), Value::Undef);
    assert_eq!(round.not_here_yet_count(), 1);
    round.read(&cell("x"));
    assert_eq!(round.not_here_yet_count(), 1);
    assert_eq!(round.read(&cell("p")), Value::Undef);
    assert_eq!(round.not_here_yet_count(), 2);
}

#[test]
fn a_round_reader_may_answer_not_here_yet_and_is_no_map() {
    let store = stub_store();
    let round = RoundReader::new(&store);
    assert!(round.may_answer_not_here_yet());
    assert!(round.as_value_map().is_empty());
}

#[test]
fn a_value_map_reader_never_answers_not_here_yet() {
    let mut store = ValueMap::new();
    store.insert(cell("x"), Value::Int(5));
    let reader: &dyn CellReader = &store;
    reader.read(&cell("absent"));
    assert!(!reader.may_answer_not_here_yet());
    assert_eq!(reader.not_here_yet_count(), 0);
}
