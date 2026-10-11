#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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
