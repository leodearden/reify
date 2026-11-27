//! A child item of a read ask (contract 7.1): the child the reader
//! declares at a step below its own place. Its path is the reader's, one
//! step down; it is settled with its answer where that path is answered in
//! the snapshot, and otherwise awaited and listed for the executor to
//! declare.

use std::sync::Arc;

use crate::declaration::Declaration;
use crate::index::Index;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::step::Step;

pub(crate) fn serve(
    reader: PathId,
    step: &Step,
    declaration: &Arc<Declaration>,
    paths: &mut PathTable,
    index: &Index,
    reading: &mut Reading,
) {
    let child = paths.child(reader, step);
    match index.entry(child) {
        Some(entry) => reading.settled(entry.outcome.clone(), entry.content),
        None => reading.declares(child, Arc::clone(declaration)),
    }
}
