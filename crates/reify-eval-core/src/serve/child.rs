//! A child item of a read ask (contract 7.1): the child the reader
//! declares at a step below its own place. Within one run a step declared
//! again with an equal declaration names the same child, and with a
//! different one is a conflict. The child's path is the reader's, one step
//! down; it is settled with its answer where that path is answered in the
//! snapshot, and otherwise awaited and listed for the executor to declare.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::declaration::Declaration;
use crate::index::Index;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::step::Step;

/// A run declaring a different declaration at `step` than it already
/// declared there.
pub(crate) struct Conflict {
    pub step: Step,
}

/// The children a run has declared, by step: those its answered reads
/// declared, then those of the request being served, in order.
pub(crate) struct Declared<'a> {
    run: &'a BTreeMap<Step, Arc<Declaration>>,
    request: BTreeMap<Step, Arc<Declaration>>,
}

impl<'a> Declared<'a> {
    pub fn new(run: &'a BTreeMap<Step, Arc<Declaration>>) -> Self {
        Self {
            run,
            request: BTreeMap::new(),
        }
    }

    /// Notes that `step` is declared `declaration`: a conflict where the
    /// run declared another declaration there.
    fn note(&mut self, step: &Step, declaration: &Arc<Declaration>) -> Result<(), Conflict> {
        match self.request.get(step).or_else(|| self.run.get(step)) {
            Some(earlier) if **earlier != **declaration => Err(Conflict { step: step.clone() }),
            Some(_) => Ok(()),
            None => {
                self.request.insert(step.clone(), Arc::clone(declaration));
                Ok(())
            }
        }
    }
}

pub(crate) fn serve(
    reader: PathId,
    (step, declaration): (&Step, &Arc<Declaration>),
    declared: &mut Declared<'_>,
    paths: &mut PathTable,
    index: &Index,
    reading: &mut Reading,
) -> Result<(), Conflict> {
    declared.note(step, declaration)?;
    let child = paths.child(reader, step);
    match index.entry(child) {
        Some(entry) => reading.settled(entry.outcome.clone(), entry.content),
        None => reading.declares(child, Arc::clone(declaration)),
    }
    Ok(())
}
