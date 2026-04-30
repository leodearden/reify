//! Multi-kernel dispatcher (v0.2): pure-logic plan ranking by
//! conversion-stage count.
//!
//! Given a registry of kernels (each described by a
//! [`reify_types::CapabilityDescriptor`]), an [`Operation`] to perform, a
//! demanded output [`ReprKind`], and a set of currently-available reprs,
//! picks the kernel + (possibly empty) conversion chain that minimises the
//! number of conversion stages. PRD reference:
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions" — selection
//! by conversion-stage count alone, deterministic given the registered set.
//!
//! This module is pure logic. It does NOT yet wire dispatch into op
//! execution in `geometry_ops.rs`; that integration is task 2642's
//! responsibility.

use std::collections::{BTreeMap, HashSet};

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// A concrete plan returned by [`dispatch`]: which kernel runs the final op,
/// preceded by zero or more conversion stages.
///
/// Each conversion entry is `(kernel_name, from, to)`: the named kernel is
/// expected to convert from `from` to `to`. The conversions are ordered so
/// the final entry's `to` matches the input repr expected by `kernel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchPlan {
    /// Name of the kernel that runs the final (target) operation.
    pub kernel: String,
    /// Sequence of conversion stages to perform before invoking `kernel`.
    /// Each tuple = `(kernel_name, from_repr, to_repr)`. Empty when the
    /// demanded repr is already in `available`.
    pub conversions: Vec<(String, ReprKind, ReprKind)>,
}

/// Pick a kernel + conversion chain to perform `op` and produce `demanded`,
/// given that the inputs are currently realised in the reprs listed by
/// `available`.
///
/// Returns `None` when no chain reaches `demanded` from any repr in
/// `available` via conversions declared by registered kernels, or when no
/// registered kernel claims to support `op` on `demanded`.
///
/// **Step-6 scope: zero-conversion only.** This minimal stub returns the
/// first kernel in lexicographic order whose descriptor declares
/// `(op, demanded)` AND `demanded ∈ available`. BFS expansion across
/// conversion stages is added in step 8.
pub fn dispatch(
    registry: &BTreeMap<String, &CapabilityDescriptor>,
    op: Operation,
    demanded: ReprKind,
    available: &HashSet<ReprKind>,
) -> Option<DispatchPlan> {
    if !available.contains(&demanded) {
        return None;
    }
    for (name, descriptor) in registry.iter() {
        if descriptor.supports(op, demanded) {
            return Some(DispatchPlan {
                kernel: name.clone(),
                conversions: vec![],
            });
        }
    }
    None
}

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
