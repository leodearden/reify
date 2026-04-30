//! Multi-kernel dispatcher (v0.2): pure-logic plan ranking by
//! conversion-stage count.
//!
//! Stub for task 2641 step 5 — only the test scaffolding is in place.
//! Subsequent steps implement the BFS algorithm.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use reify_types::{CapabilityDescriptor, Operation, ReprKind};

    use super::{DispatchPlan, dispatch};

    /// Trivial happy path: one kernel that supports the demanded op directly on
    /// a repr already in `available`. Plan must be `(kernel, no conversions)`.
    /// This locks the zero-conversion code path before BFS expansion is added.
    #[test]
    fn dispatch_zero_conversion_returns_plan_with_kernel_only() {
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::BRep, &available);
        assert_eq!(
            plan,
            Some(DispatchPlan {
                kernel: "occt".to_string(),
                conversions: vec![],
            }),
            "zero-conversion path: occt supports (BooleanUnion, BRep) and BRep is available",
        );
    }
}
