//! A read ask: each item served in order. A ref's target is settled at
//! once with its content where its answer is known, otherwise it awaits
//! the node it names; a child item goes to the child server.

use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::index::Index;
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::reference::Ref;
use crate::resolve::{Target, target};
use crate::serve::child;
use crate::wanted::Wanted;

pub(crate) fn serve(
    reader: PathId,
    declaration: &Declaration,
    wanted: Arc<[Wanted]>,
    paths: &mut PathTable,
    index: &Index,
) -> Reading {
    let mut reading = Reading::new(Arc::clone(&wanted));
    for item in wanted.iter() {
        match item {
            Wanted::Ref(r) => serve_ref(reader, declaration, r, paths, index, &mut reading),
            Wanted::Child(step, declared) => {
                child::serve(reader, step, declared, paths, index, &mut reading);
            }
        }
    }
    reading
}

fn serve_ref(
    reader: PathId,
    declaration: &Declaration,
    r: &Ref,
    paths: &mut PathTable,
    index: &Index,
    reading: &mut Reading,
) {
    match target(reader, declaration, r, paths, index) {
        Target::Absent => absent(reading),
        Target::Input(key) => match index.input(&key) {
            Some(input) => reading.settled(input.outcome(), Content::of_input(input)),
            None => absent(reading),
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

fn absent(reading: &mut Reading) {
    reading.settled(Outcome::Absent, Content::absent());
}
