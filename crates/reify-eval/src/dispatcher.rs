//! Multi-kernel dispatcher (v0.2): pure-logic plan ranking by
//! conversion-stage count.
//!
//! # What this module does
//!
//! Given a registry of kernels (each described by a
//! [`reify_types::CapabilityDescriptor`]), an [`reify_types::Operation`] to
//! perform, a demanded output [`reify_types::ReprKind`], and a set of
//! currently-available reprs, [`dispatch`] picks the kernel +
//! (possibly empty) conversion chain that minimises the number of
//! conversion stages. The result is a [`DispatchPlan`] naming the final
//! kernel and the ordered conversion stages to perform first.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions":
//! - `CapabilityDescriptor { supports: Vec<(Operation, ReprKind)> }` —
//!   feasibility table only, no `cost_hint` or `error_factor`.
//! - Dispatcher ranks candidate (kernel, conversion-chain) plans by
//!   conversion-stage count alone.
//! - Selection deterministic given the registered set of kernels.
//!
//! # Determinism contract
//!
//! 1. Plans are ranked strictly by conversion-stage count (BFS over
//!    reachable [`reify_types::ReprKind`] states; first hit wins).
//! 2. Ties at equal stage-count are broken lexicographically on kernel
//!    name. The `registry` parameter is a [`std::collections::BTreeMap`]
//!    so kernel iteration order is lexicographic and stable across calls.
//! 3. The BFS visited set is keyed on [`reify_types::ReprKind`] (4
//!    variants), so the algorithm terminates after at most 4 expansions.
//!
//! # Scope boundary (task 2641)
//!
//! This module is pure logic. It does NOT yet wire dispatch into op
//! execution in `geometry_ops.rs`; the kernel-registry mechanism + OCCT
//! adapter migration that consumes [`dispatch`] is task 2642's
//! responsibility. Subsequent kernel adapter tasks (2643 Manifold, 2644
//! Fidget, 2645 OpenVDB) consume the [`reify_types::CapabilityDescriptor`]
//! type defined alongside [`reify_types::Operation`] in the
//! `reify-types` crate.

use std::collections::{BTreeMap, HashSet, VecDeque};

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Ordered sequence of conversion stages: each entry is
/// `(kernel_name, from_repr, to_repr)`. Factored as a type alias to keep the
/// internal BFS frontier type below clippy's `type_complexity` threshold and
/// to give the conversion-chain shape a single named home.
type ConversionChain = Vec<(String, ReprKind, ReprKind)>;

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
    pub conversions: ConversionChain,
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
/// **Tie-break.** Ties at equal stage-count are broken lexicographically on
/// kernel name; the registry is a [`BTreeMap`] so kernel iteration is
/// deterministic across BTreeMap iteration order (lexicographic). Selection
/// is therefore deterministic given a fixed `registry` (PRD
/// `docs/prds/v0_2/multi-kernel.md`: "Selection deterministic given pinned
/// runtime configuration"). Ties at equal stage-count and equal final
/// kernel choice fall through to the order in which we enqueue conversion
/// expansions, which is itself a BTreeMap-order traversal.
///
/// **`None` returns** in three branches:
///   - (a) no conversion path from any repr in `available` reaches
///     `demanded` (the BFS visited set covers all 4 [`ReprKind`] variants
///     without producing the demanded one);
///   - (b) no registered kernel claims `(op, demanded)` in its supports
///     table — even when the demanded repr IS reachable;
///   - (c) the registry is empty (or `available` is empty AND no
///     conversion can synthesise a repr ex nihilo, which by construction
///     cannot happen since [`Operation::Convert { from }`] always
///     requires an input repr).
pub fn dispatch(
    registry: &BTreeMap<String, &CapabilityDescriptor>,
    op: Operation,
    demanded: ReprKind,
    available: &HashSet<ReprKind>,
) -> Option<DispatchPlan> {
    // BFS state: (currently-realised repr, conversion chain so far).
    let mut frontier: VecDeque<(ReprKind, ConversionChain)> = VecDeque::new();
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
                if let Operation::Convert { from } = decl_op
                    && from == current_repr
                    && !visited.contains(&decl_to)
                {
                    visited.insert(decl_to);
                    let mut new_chain = chain.clone();
                    new_chain.push((kernel_name.clone(), current_repr, decl_to));
                    frontier.push_back((decl_to, new_chain));
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

    /// Two competing chains lead to (BooleanUnion, Mesh): a 1-stage path via
    /// alpha (BRep→Mesh→Union) and a 2-stage path via beta (BRep→Sdf→Mesh→
    /// Union). BFS by stage-count must pick the shorter one. Locks the
    /// "rank by conversion-stage count alone" PRD requirement.
    #[test]
    fn dispatch_prefers_shorter_chain() {
        let alpha = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (Operation::Convert { from: ReprKind::BRep }, ReprKind::Mesh),
            ],
        };
        let beta = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (Operation::Convert { from: ReprKind::BRep }, ReprKind::Sdf),
                (Operation::Convert { from: ReprKind::Sdf }, ReprKind::Mesh),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("alpha".to_string(), &alpha);
        registry.insert("beta".to_string(), &beta);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available)
            .expect("a 1-stage chain via alpha must be findable");

        assert_eq!(
            plan.conversions.len(),
            1,
            "BFS must pick the 1-stage chain, not the 2-stage chain via Sdf — got {plan:?}",
        );
    }

    /// Two kernels both directly support the demanded (op, repr) with zero
    /// conversions. The lexicographically smaller kernel name wins.
    ///
    /// Five repeated calls confirm determinism — a HashMap-based registry
    /// would otherwise return a random kernel each call. Locks the PRD's
    /// "Selection deterministic given pinned runtime configuration".
    #[test]
    fn dispatch_tie_break_lexicographic_kernel_name() {
        let alpha = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("alpha".to_string(), &alpha);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::Mesh);

        // Repeat 5x: every call must return the same kernel name.
        for trial in 0..5 {
            let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available)
                .expect("both kernels can answer the demand directly");
            assert_eq!(
                plan.kernel, "alpha",
                "trial {trial}: lexicographically smaller name 'alpha' must win over 'manifold', got {plan:?}",
            );
            assert!(
                plan.conversions.is_empty(),
                "trial {trial}: zero-conversion path expected, got {plan:?}",
            );
        }
    }

    /// Three None-return branches must all hold:
    ///   (a) no conversion path from any available repr to the demanded repr;
    ///   (b) op never declared on any reachable repr;
    ///   (c) registry empty.
    #[test]
    fn dispatch_returns_none_when_no_chain_exists() {
        // (a) occt only supports BRep ops, no conversion to Mesh; Mesh demand
        //     is unreachable.
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);
        assert_eq!(
            dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available),
            None,
            "(a) demanded repr Mesh unreachable from {{BRep}} via no conversions ⇒ None",
        );

        // (b) Demand-repr matches kernel's declared support repr (BRep), but
        //     `available` is empty AND no conversion exists to bring any repr
        //     into scope. Frontier seeded empty ⇒ never enters the probe.
        let empty_available: HashSet<ReprKind> = HashSet::new();
        assert_eq!(
            dispatch(&registry, Operation::BooleanUnion, ReprKind::BRep, &empty_available),
            None,
            "(b) demanded BRep is in occt's supports table but `available` is empty ⇒ None",
        );

        // (c) Op not in any descriptor (registry has only Convert + a single
        //     boolean) — query a Modify op and expect None.
        assert_eq!(
            dispatch(&registry, Operation::ModifyFillet, ReprKind::BRep, &available),
            None,
            "(c) ModifyFillet not in any kernel's supports ⇒ None",
        );

        // Edge case: empty registry. Frontier is seeded but nothing matches.
        let empty_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        assert_eq!(
            dispatch(&empty_registry, Operation::BooleanUnion, ReprKind::Mesh, &available),
            None,
            "edge: empty registry ⇒ None",
        );
    }

    /// Integration: a small registry mirroring the v0.2 planned setup —
    /// "occt" owns BRep primitives + BRep→Mesh tessellation, "manifold" owns
    /// Mesh booleans. This locks the contract shape that downstream tasks
    /// (2642 kernel-registry wiring, 2643 manifold adapter) will consume:
    ///
    ///   1. `BooleanUnion → Mesh` from `available = {BRep}` → "manifold" via
    ///      one conversion stage performed by "occt" (BRep→Mesh).
    ///   2. `PrimitiveBox → BRep` from `available = {BRep}` → "occt" with
    ///      zero conversions. Primitives are passed `available = {demanded}`
    ///      because they produce the demanded repr from non-geometric inputs
    ///      (size/dimension scalars), so the BFS treats the demanded repr as
    ///      "trivially in scope" with no conversion required.
    ///
    /// No new dispatcher logic is exercised here beyond what step-7's
    /// single-conversion test and step-9's shortest-chain test already lock;
    /// this test exists so future kernel-registry refactors break loudly if
    /// the v0.2 occt+manifold contract regresses.
    #[test]
    fn dispatch_uses_capability_descriptor_for_v02_kernels() {
        // occt: BRep primitives (Box/Cylinder/Sphere) + BRep→Mesh tessellation.
        let occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::PrimitiveCylinder, ReprKind::BRep),
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (Operation::Convert { from: ReprKind::BRep }, ReprKind::Mesh),
            ],
        };
        // manifold: Mesh booleans (Union/Difference/Intersection).
        let manifold = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (Operation::BooleanDifference, ReprKind::Mesh),
                (Operation::BooleanIntersection, ReprKind::Mesh),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &occt);
        registry.insert("manifold".to_string(), &manifold);

        // Scenario 1: BooleanUnion demanded as Mesh, inputs realised as BRep.
        // Plan must invoke occt's BRep→Mesh conversion, then manifold's union.
        let mut available_brep: HashSet<ReprKind> = HashSet::new();
        available_brep.insert(ReprKind::BRep);
        let plan_union = dispatch(
            &registry,
            Operation::BooleanUnion,
            ReprKind::Mesh,
            &available_brep,
        )
        .expect("v0.2 occt+manifold mix must satisfy BRep→Mesh→Union");
        assert_eq!(
            plan_union.kernel, "manifold",
            "Mesh BooleanUnion must run on manifold per v0.2 capability split",
        );
        assert_eq!(
            plan_union.conversions.len(),
            1,
            "BRep→Mesh requires exactly one conversion stage, got {plan_union:?}",
        );
        assert_eq!(
            plan_union.conversions[0],
            ("occt".to_string(), ReprKind::BRep, ReprKind::Mesh),
            "the conversion stage must be (occt, BRep, Mesh), got {:?}",
            plan_union.conversions[0],
        );

        // Scenario 2: PrimitiveBox demanded as BRep. Primitives pass
        // `available = {demanded}` since they produce the demanded repr
        // without consuming a geometric input. Plan picks occt directly.
        let plan_box = dispatch(
            &registry,
            Operation::PrimitiveBox,
            ReprKind::BRep,
            &available_brep,
        )
        .expect("v0.2 occt+manifold mix must satisfy PrimitiveBox→BRep");
        assert_eq!(
            plan_box.kernel, "occt",
            "BRep PrimitiveBox must run on occt per v0.2 capability split",
        );
        assert!(
            plan_box.conversions.is_empty(),
            "PrimitiveBox→BRep requires zero conversions, got {plan_box:?}",
        );
    }
}
