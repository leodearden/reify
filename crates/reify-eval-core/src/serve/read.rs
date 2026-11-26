//! A read ask: each ref's target, settled at once where its answer is
//! known, otherwise awaiting the node it names.

use crate::declaration::Declaration;
use crate::index::Index;
use crate::input::Input;
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::reference::Ref;
use crate::resolve::{Target, target};

pub(crate) fn serve(
    reader: PathId,
    declaration: &Declaration,
    refs: &[Ref],
    paths: &mut PathTable,
    index: &Index,
) -> Reading {
    let mut reading = Reading::default();
    for r in refs {
        match target(reader, declaration, r, paths, index) {
            Target::Absent => reading.settled(Outcome::Absent),
            Target::Input(key) => {
                reading.settled(index.input(&key).map_or(Outcome::Absent, Input::outcome))
            }
            Target::Node(node) => match index.entry(node) {
                Some(entry) => reading.settled(entry.outcome.clone()),
                None => reading.awaits(node),
            },
        }
    }
    reading
}
