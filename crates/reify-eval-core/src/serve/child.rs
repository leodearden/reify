//! A child item of a read ask (contract 7.1): the child the reader
//! declares at a step below its own place, in conflict where the run or
//! the request already declared a different one there. The child's path is
//! the reader's, one step down; it is settled with its answer where that
//! path is answered in the snapshot, and otherwise awaited and listed for
//! the executor to declare.

use std::sync::Arc;

use crate::declaration::Declaration;
use crate::declared_children::{Conflict, DeclaredChildren};
use crate::index::Index;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::step::Step;

/// Serves the child `declaration` at `step` below `reader`, whose run has
/// declared `by_run`.
pub(crate) fn serve(
    reader: PathId,
    step: &Step,
    declaration: &Arc<Declaration>,
    by_run: &DeclaredChildren,
    paths: &mut PathTable,
    index: &Index,
    reading: &mut Reading,
) -> Result<(), Conflict> {
    by_run.is_new(step, declaration)?;
    reading.declare(step, declaration)?;
    let child = paths.child(reader, step);
    match index.entry(child) {
        Some(entry) => reading.settled(entry.outcome.clone(), entry.content),
        None => reading.awaits_child(child, Arc::clone(declaration)),
    }
    Ok(())
}
