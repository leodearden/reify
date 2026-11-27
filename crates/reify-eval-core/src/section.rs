//! What a running section may ask of the core. Nothing here waits.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::cancel::Cancel;
use crate::held::{Held, Release, ReleaseHome};
use crate::job::Report;

pub(crate) struct SectionParts {
    pub serial: u64,
    pub cancel: Cancel,
    pub reports: Sender<Report>,
    pub home: ReleaseHome,
}

pub struct Section<R> {
    parts: SectionParts,
    resource: PhantomData<fn(&mut R)>,
}

impl<R: 'static> Section<R> {
    pub(crate) fn new(parts: SectionParts) -> Self {
        Self {
            parts,
            resource: PhantomData,
        }
    }

    /// Whether nobody wants the node this section works for any more.
    pub fn cancelled(&self) -> bool {
        self.parts.cancel.is_set()
    }

    /// Tells every live request that wants this section's node, while the
    /// section still runs.
    pub fn progress(&self, note: impl Any + Send + Sync) {
        let report = Report::Progress {
            serial: self.parts.serial,
            note: Arc::new(note),
        };
        let _ = self.parts.reports.send(report);
    }

    /// Wraps a thing just made inside the resource, so that dropping it,
    /// anywhere, gives it back here.
    pub fn hold<P: Release<R>>(&self, thing: P) -> Held<R, P> {
        Held::new(thing, self.parts.home.clone())
    }
}
