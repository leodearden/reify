//! What every committed-ratchet ledger shares: the set algebra — live debt
//! the ledger already holds is kept, live debt it lacks is new, and a
//! committed row no live debt matches is stale — and the rule that only a
//! git-tracked ledger file is read ([`committed_rows`]). It knows nothing
//! about a ledger file's row grammar or about how a detector finds its debt.
//! [`regenerator`] is the driver each lane's `*-baseline-gen` binary shares.

pub mod regenerator;

use crate::AuditContext;
use std::collections::BTreeSet;

/// The rows of the committed ledger file at `path`, read with the lane's own
/// `parse` — none when the file is absent from `tracked`, unreadable or
/// empty. An untracked copy is inert, so a stray local ledger never absorbs
/// debt.
pub fn committed_rows<R: Ord>(
    ctx: &AuditContext<'_>,
    tracked: &[String],
    path: &str,
    parse: fn(&str) -> BTreeSet<R>,
) -> BTreeSet<R> {
    if !tracked.iter().any(|tracked_path| tracked_path == path) {
        return BTreeSet::new();
    }
    ctx.read_relative(path)
        .map(|content| parse(&content))
        .unwrap_or_default()
}

/// Live debt (what a detector finds today) against committed debt (the
/// ledger file). The three views are disjoint: `kept ∪ new_debt == live` and
/// `kept ∪ stale == committed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger<R: Ord + Clone> {
    pub live: BTreeSet<R>,
    pub committed: BTreeSet<R>,
}

impl<R: Ord + Clone> Ledger<R> {
    /// Live debt the ledger already accounts for — what a shrink-only
    /// regeneration writes.
    pub fn kept(&self) -> BTreeSet<R> {
        self.live.intersection(&self.committed).cloned().collect()
    }

    /// Live debt the ledger lacks — reported by the ratchet.
    pub fn new_debt(&self) -> BTreeSet<R> {
        self.live.difference(&self.committed).cloned().collect()
    }

    /// Committed rows no live debt matches — removing one would change no
    /// other finding, so each is reported as dead weight.
    pub fn stale(&self) -> BTreeSet<R> {
        self.committed.difference(&self.live).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
        items.into_iter().map(str::to_string).collect()
    }

    #[test]
    fn the_ledger_partitions_live_and_committed_debt() {
        let ledger = Ledger {
            live: rows(["kept", "new"]),
            committed: rows(["kept", "stale"]),
        };

        assert_eq!(ledger.kept(), rows(["kept"]));
        assert_eq!(ledger.new_debt(), rows(["new"]));
        assert_eq!(ledger.stale(), rows(["stale"]));

        let (kept, new_debt, stale) = (ledger.kept(), ledger.new_debt(), ledger.stale());
        assert!(kept.is_disjoint(&new_debt) && kept.is_disjoint(&stale));
        assert!(new_debt.is_disjoint(&stale));
        assert_eq!(&kept | &new_debt, ledger.live);
        assert_eq!(&kept | &stale, ledger.committed);
    }

    #[test]
    fn an_empty_ledger_has_three_empty_views() {
        let ledger: Ledger<String> = Ledger {
            live: BTreeSet::new(),
            committed: BTreeSet::new(),
        };
        assert!(ledger.kept().is_empty());
        assert!(ledger.new_debt().is_empty());
        assert!(ledger.stale().is_empty());
    }
}
