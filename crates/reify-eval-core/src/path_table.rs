//! Every path the core has met, interned once per core: a `PathId` names a
//! path, and knows its parent and the step below it. Paths are never
//! removed.

use std::collections::BTreeMap;

use crate::path::{Path, Root};
use crate::step::Step;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PathId(usize);

enum Place {
    Root(Root),
    Below { parent: PathId, step: Step },
}

#[derive(Default)]
pub(crate) struct PathTable {
    places: Vec<Place>,
    /// How many steps below its root each path is, by its id.
    depths: Vec<usize>,
    roots: BTreeMap<Root, PathId>,
    children: BTreeMap<(PathId, Step), PathId>,
}

impl PathTable {
    pub fn intern(&mut self, path: &Path) -> PathId {
        let root = self.root(path.root_name());
        path.steps()
            .iter()
            .fold(root, |parent, step| self.child(parent, step))
    }

    pub fn root(&mut self, root: &Root) -> PathId {
        if let Some(id) = self.roots.get(root) {
            return *id;
        }
        let id = self.push(Place::Root(root.clone()));
        self.roots.insert(root.clone(), id);
        id
    }

    /// The path one `step` below `parent`.
    pub fn child(&mut self, parent: PathId, step: &Step) -> PathId {
        let key = (parent, step.clone());
        if let Some(id) = self.children.get(&key) {
            return *id;
        }
        let step = key.1.clone();
        let id = self.push(Place::Below { parent, step });
        self.children.insert(key, id);
        id
    }

    /// The parent of `id` and the step from it down to `id`; `None` for a
    /// root.
    pub fn parent(&self, id: PathId) -> Option<(PathId, &Step)> {
        match &self.places[id.0] {
            Place::Root(_) => None,
            Place::Below { parent, step } => Some((*parent, step)),
        }
    }

    /// The root `id` is, if it is one.
    pub fn as_root(&self, id: PathId) -> Option<&Root> {
        match &self.places[id.0] {
            Place::Root(root) => Some(root),
            Place::Below { .. } => None,
        }
    }

    /// How many steps below its root `id` is: 0 for a root.
    pub fn depth(&self, id: PathId) -> usize {
        self.depths[id.0]
    }

    pub fn path(&self, id: PathId) -> Path {
        let mut steps = Vec::new();
        let mut at = id;
        loop {
            match &self.places[at.0] {
                Place::Root(root) => {
                    steps.reverse();
                    return Path::from_parts(root.clone(), steps);
                }
                Place::Below { parent, step } => {
                    steps.push(step.clone());
                    at = *parent;
                }
            }
        }
    }

    fn push(&mut self, place: Place) -> PathId {
        let depth = match &place {
            Place::Root(_) => 0,
            Place::Below { parent, .. } => self.depth(*parent) + 1,
        };
        self.places.push(place);
        self.depths.push(depth);
        PathId(self.places.len() - 1)
    }
}
