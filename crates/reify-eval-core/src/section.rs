//! What a running section may ask of the core. Nothing here waits.

use std::marker::PhantomData;

use crate::held::{Held, Release, ReleaseHome};

pub(crate) struct SectionParts {
    pub home: ReleaseHome,
}

pub struct Section<R> {
    parts: SectionParts,
    resource: PhantomData<fn(&mut R)>,
}

impl<R: 'static> Section<R> {
    pub(crate) fn new(parts: SectionParts) -> Self {
        Self { parts, resource: PhantomData }
    }

    /// Wraps a thing just made inside the resource, so that dropping it,
    /// anywhere, gives it back here.
    pub fn hold<P: Release<R>>(&self, thing: P) -> Held<R, P> {
        Held::new(thing, self.parts.home.clone())
    }
}
