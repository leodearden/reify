//! A read ask: each item served in order. A ref's target is settled at
//! once with its content where its answer is known, otherwise it awaits
//! the node it names; a child item goes to the child server, which may
//! find the request in conflict with what the run declared.

use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::declared_children::{Conflict, DeclaredChildren};
use crate::index::Index;
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::reference::Ref;
use crate::resolve::{Target, target};
use crate::serve::child;
use crate::wanted::Wanted;

/// The reading of `wanted`, asked by the node at `reader` declared
/// `declaration`, whose run has declared `by_run`.
pub(crate) fn serve(
    reader: PathId,
    declaration: &Declaration,
    wanted: Arc<[Wanted]>,
    by_run: &DeclaredChildren,
    paths: &mut PathTable,
    index: &Index,
) -> Result<Reading, Conflict> {
    let mut reading = Reading::new(Arc::clone(&wanted));
    for item in wanted.iter() {
        match item {
            Wanted::Ref(r) => serve_ref(reader, declaration, r, paths, index, &mut reading),
            Wanted::Child(step, child) => {
                child::serve(reader, step, child, by_run, paths, index, &mut reading)?;
            }
        }
    }
    Ok(reading)
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
