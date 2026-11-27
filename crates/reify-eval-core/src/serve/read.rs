//! A read ask: each ref's target, settled at once with its content where
//! its answer is known, otherwise awaiting the node it names.

use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::index::Index;
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::reference::Ref;
use crate::resolve::{Target, target};

pub(crate) fn serve(
    reader: PathId,
    declaration: &Declaration,
    refs: Arc<[Ref]>,
    paths: &mut PathTable,
    index: &Index,
) -> Reading {
    let mut reading = Reading::new(Arc::clone(&refs));
    for r in refs.iter() {
        match target(reader, declaration, r, paths, index) {
            Target::Absent => absent(&mut reading),
            Target::Input(key) => match index.input(&key) {
                Some(input) => reading.settled(input.outcome(), Content::of_input(input)),
                None => absent(&mut reading),
            },
            Target::Constant(constant) => {
                reading.settled(constant.outcome(), Content::of_input(&constant));
            }
            Target::Node(node) => match index.entry(node) {
                Some(entry) => reading.settled(entry.outcome.clone(), entry.content),
                None => reading.awaits(node),
            },
        }
    }
    reading
}

fn absent(reading: &mut Reading) {
    reading.settled(Outcome::Absent, Content::absent());
}
