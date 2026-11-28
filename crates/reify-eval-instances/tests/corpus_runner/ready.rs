//! The entries of the corpus that are checked: those whose every need is
//! built and that are not disputed (`EntryState::of`, the one rule).

use crate::bookkeeping::{Built, Disputed, EntryState};
use crate::corpus_tree::{self, LoadedEntry, Problem};

/// Every ready entry of the corpus, and every problem met loading it and
/// its bookkeeping.
pub fn ready_entries() -> (Vec<LoadedEntry>, Vec<Problem>) {
    let root = corpus_tree::corpus_root();
    let mut problems = Vec::new();
    let (built, found) = Built::parse(&corpus_tree::bookkeeping_text(
        &root,
        corpus_tree::BUILT,
        &mut problems,
    ));
    problems.extend(found);
    let (disputed, found) = Disputed::parse(&corpus_tree::bookkeeping_text(
        &root,
        corpus_tree::DISPUTED,
        &mut problems,
    ));
    problems.extend(found);
    let (entries, found) = corpus_tree::load_corpus(&root);
    problems.extend(found);

    let ready = entries
        .into_iter()
        .filter(|entry| {
            matches!(
                EntryState::of(&entry.record.needs, &built, disputed.lists(&entry.id)),
                EntryState::Ready
            )
        })
        .collect();
    (ready, problems)
}
