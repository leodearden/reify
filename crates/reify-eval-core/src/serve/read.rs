//! A read: the outcome a binding of the reader's declaration names.

use crate::declaration::{Binding, Declaration};
use crate::input::Input;
use crate::outcome::{Erased, Outcome};
use crate::reference::Ref;
use crate::snapshot::Snapshot;

pub(crate) fn answer(declaration: &Declaration, snapshot: &Snapshot, r: &Ref) -> Outcome<Erased> {
    match declaration.binding(r.binding_name()) {
        None => Outcome::Absent,
        Some(Binding::Input(key)) => snapshot.input(key).map_or(Outcome::Absent, Input::outcome),
    }
}
