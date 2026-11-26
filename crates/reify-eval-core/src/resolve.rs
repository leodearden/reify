//! Which place a read names, and what a path below an answered node is
//! (contract 2.1, 7.1-7.4).

use crate::declaration::{Binding, Declaration};
use crate::index::{Entry, Index};
use crate::input::InputKey;
use crate::kinds::Kinds;
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::reference::Ref;
use crate::step::Step;

/// What a ref names: nothing, a snapshot input, or a node of the reader's
/// snapshot.
pub(crate) enum Target {
    Absent,
    Input(InputKey),
    Node(PathId),
}

/// The place `r` names when read by the node at `reader` whose declaration
/// is `declaration`. A passed binding moves to the declarer, whose
/// declaration the index holds (its answer declared the child), carrying
/// the steps still to take; the walk ends at an input or a place. An unset
/// binding names nothing; a snapshot input, attached or not, declares no
/// children, so steps below one name nothing; a binding relative to the
/// declarer names nothing on a root, which has no declarer.
pub(crate) fn target<'a>(
    reader: PathId,
    declaration: &'a Declaration,
    r: &'a Ref,
    paths: &mut PathTable,
    index: &'a Index,
) -> Target {
    let (mut node, mut declaration, mut name) = (reader, declaration, r.binding_name());
    let mut steps = r.steps().to_vec();
    loop {
        let declarer = paths.parent(node).map(|(declarer, _)| declarer);
        match (declaration.binding(name), declarer) {
            (Some(Binding::Input(key)), _) if steps.is_empty() => {
                return Target::Input(key.clone());
            }
            (Some(Binding::Attached(attached)), _) if steps.is_empty() => {
                return Target::Input(InputKey::attached(paths.path(node), attached.clone()));
            }
            (None | Some(Binding::Input(_) | Binding::Attached(_)), _)
            | (Some(Binding::Declarer(_) | Binding::Passed(_)), None) => return Target::Absent,
            (Some(Binding::Declarer(place)), Some(declarer)) => {
                let below = place.iter().chain(&steps);
                return Target::Node(below.fold(declarer, |at, step| paths.child(at, step)));
            }
            (Some(Binding::Passed(passed)), Some(declarer)) => {
                declaration = index
                    .entry(declarer)
                    .and_then(|entry| entry.declaration.as_deref())
                    .expect("a declarer's body ran, and its answer is indexed");
                steps = passed.steps().iter().chain(&steps).cloned().collect();
                name = passed.binding_name();
                node = declarer;
            }
        }
    }
}

/// What the child at `step` below a node answered `entry` is: the
/// declaration its kind gives for that step; absent where it gives none or
/// the node is absent; the node's failure where it failed (design decision).
pub(crate) fn declaration_of(step: &Step, entry: &Entry, kinds: &Kinds) -> Outcome<Declaration> {
    match (&entry.outcome, &entry.declaration) {
        (Outcome::Failure(failure), _) => Outcome::Failure(failure.clone()),
        (Outcome::Value(value), Some(node)) => kinds
            .get(node.kind())
            .and_then(|kind| kind.declares(&**value, step))
            .map_or(Outcome::Absent, Outcome::of),
        (Outcome::Value(_), None) | (Outcome::Absent, _) => Outcome::Absent,
    }
}
