//! Which realizations an edit made stale, and which `RealizationCache`
//! families that obliges the edit to evict (selective-realization-eviction
//! PRD, `docs/prds/v0_6/selective-realization-eviction.md`).
//!
//! Pure functions over the evaluation graph: no engine state is read or
//! written here. `engine_edit` runs them at its two compare sites and applies
//! the result to the cache.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use reify_core::{ComputeNodeId, ConstraintNodeId, RealizationNodeId, ValueCellId};

    use super::stale_realization_entities;
    use crate::cache::NodeId;

    fn rid(entity: &str) -> RealizationNodeId {
        RealizationNodeId::new(entity, 0)
    }

    fn entities(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn includes_the_entity_of_every_changed_seed() {
        let changed = HashSet::from([rid("PartA"), rid("PartB")]);

        let stale = stale_realization_entities(&changed, [], &HashSet::new());

        assert_eq!(stale, entities(&["PartA", "PartB"]));
    }

    /// `Asm` is a `GeomRef::Sub` consumer of the seed `PartA`: its own
    /// input-cone fold never moves, so only the transitive dirty cone reaches
    /// it. `dirty::compute_dirty_cone_with_realizations` never puts the SEEDS
    /// in its result, so the seed's entity must come from `changed`.
    #[test]
    fn includes_the_entity_of_every_realization_reached_through_the_dirty_cone() {
        let changed = HashSet::from([rid("PartA")]);
        let cone = HashSet::from([NodeId::Realization(rid("Asm"))]);

        let stale = stale_realization_entities(&changed, [], &cone);

        assert_eq!(stale, entities(&["PartA", "Asm"]));
    }

    /// A removed realization (collection shrink, recompile removal) leaves
    /// its entity's cached terminal behind, where it could now stand in for a
    /// different realization.
    #[test]
    fn includes_the_entity_of_every_removed_realization() {
        let removed = [rid("Dropped")];

        let stale = stale_realization_entities(&HashSet::new(), &removed, &HashSet::new());

        assert_eq!(stale, entities(&["Dropped"]));
    }

    #[test]
    fn ignores_value_compute_and_constraint_members_of_the_cone() {
        let cone = HashSet::from([
            NodeId::Value(ValueCellId::new("PartA", "w")),
            NodeId::Compute(ComputeNodeId::new("Fea", 0)),
            NodeId::Constraint(ConstraintNodeId::new("PartA", 0)),
        ]);

        let stale = stale_realization_entities(&HashSet::new(), [], &cone);

        assert!(stale.is_empty(), "only realization members name a family: {stale:?}");
    }

    /// PRD §6 zero-eviction row: an edit that moves no realization evicts
    /// nothing.
    #[test]
    fn is_empty_for_a_no_realization_edit() {
        let stale = stale_realization_entities(&HashSet::new(), [], &HashSet::new());

        assert!(stale.is_empty());
    }
}
