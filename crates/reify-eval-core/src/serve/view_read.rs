//! A read through a view (contract 6). The view is opened on its first
//! read: each item of its substitution names a place relative to the
//! opener, as the opener's own reads do, and no two name one place. Each
//! read through it is resolved relative to the opener in the same way, and
//! answered in the view's own index.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::failure::Failure;
use crate::index::{Index, StateId, Supposed};
use crate::outcome::Outcome;
use crate::path_table::{PathId, PathTable};
use crate::record::Reading;
use crate::resolve::{Target, target};
use crate::serve::read;
use crate::substitution::Substitution;
use crate::wanted::Wanted;

/// The outcome supposed at each place `substitution` names, read by the
/// opener at `opener` declared `declaration` in `index`, its own state's.
/// An item whose ref names no node's place, or a place an earlier item
/// names, is the opener's failure (contract 6: what a view substitutes has
/// exactly one path).
pub(crate) fn open(
    opener: PathId,
    declaration: &Declaration,
    substitution: &Substitution,
    paths: &mut PathTable,
    index: &Index,
) -> Result<BTreeMap<PathId, Supposed>, Failure> {
    let mut supposed = BTreeMap::new();
    for (r, outcome) in substitution.items() {
        let not_supposable = || Failure::NotSupposable { at: r.clone() };
        let Target::Node(place) = target(opener, declaration, r, paths, index) else {
            return Err(not_supposable());
        };
        if supposed.contains_key(&place) {
            return Err(not_supposable());
        }
        let erased = match outcome {
            Outcome::Value(input) => input.outcome(),
            Outcome::Failure(failure) => Outcome::Failure(failure.clone()),
            Outcome::Absent => Outcome::Absent,
        };
        let content = Content::of_supposed(outcome);
        supposed.insert(
            place,
            Supposed {
                outcome: erased,
                content,
            },
        );
    }
    Ok(supposed)
}

/// The reading of `wanted` through the view `view`, whose index is
/// `view_index`, asked by the opener at `opener` declared `declaration`,
/// whose own index is `opener_index`.
pub(crate) fn serve(
    opener: PathId,
    declaration: &Declaration,
    wanted: Arc<[Wanted]>,
    paths: &mut PathTable,
    opener_index: &Index,
    view: StateId,
    view_index: &Index,
) -> Reading {
    let mut reading = Reading::through(Arc::clone(&wanted), view);
    for item in wanted.iter() {
        let Wanted::Ref(r) = item else {
            unreachable!("a read through a view names a place, and declares no child")
        };
        let place = target(opener, declaration, r, paths, opener_index);
        read::settle(place, &mut reading, view_index);
    }
    reading
}
