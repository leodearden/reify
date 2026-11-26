//! Which place a read names, and what a path below an answered node is
//! (contract 2.1, 7.1-7.4).

use crate::declaration::{Binding, Declaration};
use crate::index::Entry;
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
/// is `declaration`. An unset binding names nothing; a snapshot input
/// declares no children, so steps below one name nothing; a place relative
/// to the declarer names nothing on a root, which has no declarer.
pub(crate) fn target(
    reader: PathId,
    declaration: &Declaration,
    r: &Ref,
    paths: &mut PathTable,
) -> Target {
    match declaration.binding(r.binding_name()) {
        None => Target::Absent,
        Some(Binding::Input(key)) if r.steps().is_empty() => Target::Input(key.clone()),
        Some(Binding::Input(_)) => Target::Absent,
        Some(Binding::Declarer(steps)) => match paths.parent(reader) {
            None => Target::Absent,
            Some((declarer, _)) => Target::Node(
                steps
                    .iter()
                    .chain(r.steps())
                    .fold(declarer, |at, step| paths.child(at, step)),
            ),
        },
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
