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

use std::collections::{BTreeMap, HashSet, VecDeque};

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
/// **Algorithm.** BFS over reachable [`ReprKind`] states. The frontier is
/// seeded with `{(r, vec![]) | r ∈ available}`. At each pop, the current
/// repr is the *input* repr available to the final-stage op. We probe
/// every registered kernel for `(op, demanded)`: if any kernel supports
/// the demanded `(op, output_repr)` pair AND the popped state's repr
/// equals `demanded`, we return immediately. Otherwise we expand by every
/// kernel-declared conversion `(Convert{from: popped_repr}, to)`,
/// enqueuing `(to, chain ++ (kernel_name, popped_repr, to))` for any `to`
/// not yet visited. BFS termination is guaranteed because the visited set
/// is keyed on [`ReprKind`] (4 variants → at most 4 expansions).
///
/// **Determinism.** The registry is a [`BTreeMap`] so kernel iteration is
/// lexicographic on name. Two ties at equal stage-count and equal final
/// kernel choice are broken by the lexicographic order in which we enqueue
/// expansions. Selection is therefore deterministic given a fixed
/// `registry` (PRD `docs/prds/v0_2/multi-kernel.md`: "Selection
/// deterministic given pinned runtime configuration").
///
/// **`None` returns** in three branches: (a) no path from `available` to
/// `demanded` exists via declared conversions; (b) no registered kernel
/// claims to support `op` on `demanded`; (c) the registry is empty.
pub fn dispatch(
    registry: &BTreeMap<String, &CapabilityDescriptor>,
    op: Operation,
    demanded: ReprKind,
    available: &HashSet<ReprKind>,
) -> Option<DispatchPlan> {
    // BFS state: (currently-realised repr, conversion chain so far).
    let mut frontier: VecDeque<(ReprKind, Vec<(String, ReprKind, ReprKind)>)> =
        VecDeque::new();
    let mut visited: HashSet<ReprKind> = HashSet::new();

    // Seed with every available repr in arbitrary HashSet order. BFS by
    // stage-count is preserved because all available reprs sit at distance 0.
    for &r in available {
        frontier.push_back((r, vec![]));
        visited.insert(r);
    }

    while let Some((current_repr, chain)) = frontier.pop_front() {
        // Final-stage probe: does any kernel support (op, demanded), AND is
        // the current repr equal to `demanded` (so the kernel can consume
        // what we have / will have)? Iterate in BTreeMap order for
        // lexicographic determinism.
        if current_repr == demanded {
            for (name, descriptor) in registry.iter() {
                if descriptor.supports(op, demanded) {
                    return Some(DispatchPlan {
                        kernel: name.clone(),
                        conversions: chain,
                    });
                }
            }
        }

        // Expansion step: for every kernel-declared conversion
        // (Convert{from: current_repr}, to), enqueue (to, chain + entry).
        for (kernel_name, descriptor) in registry.iter() {
            for &(decl_op, decl_to) in descriptor.supports.iter() {
                if let Operation::Convert { from } = decl_op {
                    if from == current_repr && !visited.contains(&decl_to) {
                        visited.insert(decl_to);
                        let mut new_chain = chain.clone();
                        new_chain.push((kernel_name.clone(), current_repr, decl_to));
                        frontier.push_back((decl_to, new_chain));
                    }
                }
            }
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

    /// Single-conversion chain: input is BRep but the requesting op is a Mesh
    /// boolean. The plan must invoke occt's BRep→Mesh tessellation, then run
    /// manifold's BooleanUnion on the resulting Mesh.
    ///
    /// This locks BFS's first expansion step — discovering reachable reprs by
    /// applying any kernel's `Convert{from: ...}` entry.
    #[test]
    fn dispatch_single_conversion_chain() {
        // occt only knows how to tessellate BRep into Mesh.
        let occt = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert { from: ReprKind::BRep },
                ReprKind::Mesh,
            )],
        };
        // manifold only knows how to perform BooleanUnion on Mesh.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available)
            .expect("a single-stage chain BRep→Mesh→Union must be findable");

        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold"
        );
        assert_eq!(
            plan.conversions.len(),
            1,
            "exactly one conversion stage (BRep→Mesh) is required, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            ("occt".to_string(), ReprKind::BRep, ReprKind::Mesh),
            "the conversion stage must be (occt, BRep, Mesh), got {:?}",
            plan.conversions[0],
        );
    }
}
