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

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::time::Duration;

use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// PRD-default wall-time threshold for the long-chain realization warning,
/// in milliseconds.
///
/// Per `docs/prds/v0_2/multi-kernel.md` §"Resolved design decisions" →
/// "Long-chain diagnostic" and `docs/prds/v0_2/per-purpose-tolerance.md`
/// §"Resolved design decisions" → "Long-chain diagnostic gating": the
/// dispatcher emits a warning when the realization wall time **exceeds 500
/// ms** (configurable). Strict-`>` semantics — exactly 500 ms does NOT warn,
/// matching the strict-`<` decision in
/// [`crate::tolerance_promise::is_promise_insufficient`] (task 2651) and
/// the broader "tighter satisfies looser" partial-order vocabulary across
/// the tolerance subsystem.
///
/// Override at runtime via [`LONG_CHAIN_THRESHOLD_ENV_VAR`].
pub const LONG_CHAIN_DEFAULT_THRESHOLD_MS: u64 = 500;

/// Environment variable that overrides the long-chain wall-time threshold.
///
/// Accepted values:
/// - Absent / unset → uses [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`].
/// - A decimal integer string → interpreted as milliseconds.
/// - Any other value → a `tracing::warn!` is emitted and
///   [`LONG_CHAIN_DEFAULT_THRESHOLD_MS`] is used.
///
/// Mirrors [`crate::warm_pool::BUDGET_ENV_VAR`]'s constant-named-value
/// pattern: pinning the env-var name at compile time lets tests catch a
/// typo or rename before the runtime silently ignores the user's override.
pub const LONG_CHAIN_THRESHOLD_ENV_VAR: &str = "REIFY_LONG_CHAIN_THRESHOLD_MS";

/// PRD-default minimum-conversion-stages cutoff for the long-chain realization
/// warning. The predicate uses STRICT `>` against this value, so the cutoff
/// of `2` means "≥3 conversion stages required to warn".
///
/// Per `docs/prds/v0_2/multi-kernel.md` §"Resolved design decisions" →
/// "Long-chain diagnostic": "longer than 2 stages" reads as strict in plain
/// English. Boundary cases (exactly 2 stages) do NOT warn — short-chain
/// pain is self-evident; nagging is poor ergonomics. Exposing the cutoff as
/// a const lets a future PRD revision tighten to `> 1` or relax to `> 3`
/// with a single-line change while the predicate semantics remain pinned by
/// existing tests.
pub const LONG_CHAIN_MIN_STAGES: usize = 2;

/// Strict-`>` predicate for the long-chain realization warning gate.
///
/// Returns `true` iff BOTH gates pass:
///   - `plan.conversions.len() > LONG_CHAIN_MIN_STAGES` (≥3 stages)
///   - `elapsed > threshold` (strictly exceeds the wall-time budget)
///
/// Mirrors the strict-`<` decision in
/// [`crate::tolerance_promise::is_promise_insufficient`] (task 2651) — the
/// "tighter satisfies looser" / "exactly-at-the-line satisfies the
/// constraint" partial-order vocabulary used throughout the tolerance
/// subsystem. Boundary cases (exactly 2 stages, exactly the threshold) do
/// NOT warn: short-chain pain is self-evident and a sub-threshold long
/// chain is not user-visible budget pressure, so suppressing those cases
/// is intentional ergonomics (per `docs/prds/v0_2/multi-kernel.md`
/// §"Long-chain diagnostic" and `docs/prds/v0_2/per-purpose-tolerance.md`
/// §"Long-chain diagnostic gating").
///
/// # Truth table
///
/// | stages | elapsed vs threshold | result | reason                         |
/// |--------|----------------------|--------|--------------------------------|
/// | 0      | any                  | false  | chain not long                 |
/// | 1      | any                  | false  | chain not long                 |
/// | 2      | any                  | false  | boundary; strict `>` on stages |
/// | 3+     | < threshold          | false  | elapsed gate fails             |
/// | 3+     | == threshold         | false  | boundary; strict `>` on time   |
/// | 3+     | > threshold          | true   | both gates pass                |
///
/// Decoupling the predicate from [`long_chain_diagnostic`] lets a hot
/// realization loop check the gate without paying the diagnostic-construction
/// cost (mirrors the [`crate::tolerance_promise::is_promise_insufficient`] /
/// [`crate::tolerance_promise::imported_tolerance_promise_diagnostic`]
/// predicate-plus-builder split established by task 2651).
pub fn is_long_chain_realization(
    plan: &DispatchPlan,
    elapsed: Duration,
    threshold: Duration,
) -> bool {
    plan.conversions.len() > LONG_CHAIN_MIN_STAGES && elapsed > threshold
}

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
/// `descriptor.supports(op, demanded)`. By the input==output invariant on
/// [`CapabilityDescriptor::supports`] (see its doc), the `current_repr ==
/// demanded` check on the popped state verifies both the kernel's expected
/// input repr and its produced output repr in one comparison.
/// [`Operation::Convert { from }`] entries are the only shape where the
/// second tuple element diverges from the input repr; those are handled
/// exclusively by the expansion step below. We probe
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

    // Seed in deterministic [`ReprKind`] order. The caller hands us a
    // `&HashSet<ReprKind>` whose iteration order is salted by the process's
    // hashing key — iterating it directly would let the multi-seed final-stage
    // probe pick a different kernel across runs even when the registered set
    // is identical, breaking the PRD's "Selection deterministic given pinned
    // runtime configuration" contract. `BTreeSet` traversal is ordered by
    // `Ord` (BRep < Mesh < Sdf < Voxel per the enum declaration order); BFS
    // by stage-count is preserved because all available reprs sit at distance
    // 0 regardless of seed order.
    let seeds: BTreeSet<ReprKind> = available.iter().copied().collect();
    for r in seeds {
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
        //
        // TODO(perf): O(K · S) per popped state where K=#kernels, S=avg
        // supports size. v0.2 has ~50 entries × 4 kernels so this is fine,
        // but if a future kernel grows a large supports table, pre-index
        // conversion edges into a `BTreeMap<ReprKind, Vec<(kernel_name,
        // ReprKind)>>` keyed by `from` to avoid re-scanning the full
        // supports vec at each pop.
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

    use super::{
        DispatchPlan, LONG_CHAIN_DEFAULT_THRESHOLD_MS, LONG_CHAIN_MIN_STAGES,
        LONG_CHAIN_THRESHOLD_ENV_VAR, dispatch, is_long_chain_realization,
        long_chain_diagnostic,
    };
    use std::time::Duration;

    /// Pins the three module-level long-chain constants by literal value:
    /// the PRD-default threshold (500 ms wall, per
    /// `docs/prds/v0_2/per-purpose-tolerance.md` §"Long-chain diagnostic
    /// gating"), the min-stages cutoff (`>` 2 ⇒ ≥3), and the env-var name
    /// `REIFY_LONG_CHAIN_THRESHOLD_MS`. A typo or rename loudly fails this
    /// test — mirrors `warm_pool::budget_env_var_name` (warm_pool.rs:830).
    #[test]
    fn long_chain_constants_are_pinned() {
        assert_eq!(LONG_CHAIN_DEFAULT_THRESHOLD_MS, 500);
        assert_eq!(LONG_CHAIN_THRESHOLD_ENV_VAR, "REIFY_LONG_CHAIN_THRESHOLD_MS");
        assert_eq!(LONG_CHAIN_MIN_STAGES, 2);
    }

    /// Negative-path coverage for [`is_long_chain_realization`]: each branch
    /// where one or both gates fail must return `false`. Pins the
    /// strict-`>` boundary semantics on BOTH gates independently — a future
    /// `>=` flip on either gate would silently invert PRD-prose intent
    /// ("longer than 2 stages" / "exceeds 500 ms").
    #[test]
    fn is_long_chain_realization_returns_false_when_chain_short() {
        let threshold = Duration::from_millis(500);

        // (a) Zero conversions + huge elapsed → chain not long ⇒ false.
        let plan_zero = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![],
        };
        assert!(
            !is_long_chain_realization(&plan_zero, Duration::from_secs(60), threshold),
            "0 conversion stages must NOT trip the long-chain warning even with huge elapsed",
        );

        // (b) Exactly 2 conversions + huge elapsed → boundary on the
        //     stage-count gate (strict `>` LONG_CHAIN_MIN_STAGES) ⇒ false.
        let plan_two = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                ("occt".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("manifold".to_string(), ReprKind::Mesh, ReprKind::Sdf),
            ],
        };
        assert!(
            !is_long_chain_realization(&plan_two, Duration::from_secs(60), threshold),
            "exactly 2 conversion stages must NOT warn (strict > on LONG_CHAIN_MIN_STAGES)",
        );

        // (c) 3+ conversions + zero elapsed → elapsed gate fails ⇒ false.
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                ("kernel_a".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("kernel_b".to_string(), ReprKind::Mesh, ReprKind::Sdf),
                ("kernel_c".to_string(), ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert!(
            !is_long_chain_realization(&plan_three, Duration::ZERO, threshold),
            "elapsed = 0 must NOT warn even with 3 stages — both gates must hold",
        );

        // (d) 3 conversions + elapsed exactly == threshold → strict-`>`
        //     boundary on the elapsed gate ⇒ false.
        assert!(
            !is_long_chain_realization(&plan_three, threshold, threshold),
            "elapsed exactly equal to threshold must NOT warn (strict > on threshold)",
        );
    }

    /// Positive-path coverage for [`is_long_chain_realization`]: when both
    /// gates strictly pass, the predicate returns `true`. Independent from
    /// the negative-path test (`is_long_chain_realization_returns_false_…`)
    /// so a regression that breaks one direction (e.g. inverts the
    /// predicate, or drops one of the two `&&` gates) doesn't mask the
    /// other.
    #[test]
    fn is_long_chain_realization_returns_true_when_both_gates_pass() {
        let threshold = Duration::from_millis(500);

        // Just-over the boundary on both gates: 3 stages > 2, 501 > 500.
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                ("kernel_a".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("kernel_b".to_string(), ReprKind::Mesh, ReprKind::Sdf),
                ("kernel_c".to_string(), ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert!(
            is_long_chain_realization(&plan_three, Duration::from_millis(501), threshold),
            "3 stages + 501ms > 500ms threshold: both gates strictly pass ⇒ true",
        );

        // Larger margin on both gates: 4 stages, elapsed 2s.
        let plan_four = DispatchPlan {
            kernel: "kernel_e".to_string(),
            conversions: vec![
                ("kernel_a".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("kernel_b".to_string(), ReprKind::Mesh, ReprKind::Sdf),
                ("kernel_c".to_string(), ReprKind::Sdf, ReprKind::Voxel),
                ("kernel_d".to_string(), ReprKind::Voxel, ReprKind::Mesh),
            ],
        };
        assert!(
            is_long_chain_realization(&plan_four, Duration::from_secs(2), threshold),
            "4 stages + 2s elapsed >> 500ms threshold: both gates pass ⇒ true",
        );
    }

    /// Pins the `Option<Diagnostic>` return shape's negative branch: when
    /// the predicate gate is false, the builder must return `None`. The
    /// gate must short-circuit BEFORE any `Diagnostic` is constructed —
    /// otherwise an Engine layer that sees `Some(diag)` and forwards
    /// downstream would log spurious warnings on every short-chain call.
    #[test]
    fn long_chain_diagnostic_returns_none_when_predicate_false() {
        let threshold = Duration::from_millis(500);

        // Stage-count gate fails: 2 conversions (boundary), even though
        // elapsed >> threshold.
        let plan_two = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![
                ("occt".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("manifold".to_string(), ReprKind::Mesh, ReprKind::Sdf),
            ],
        };
        assert_eq!(
            long_chain_diagnostic(&plan_two, Duration::from_secs(60), threshold),
            None,
            "2 conversion stages must NOT emit a diagnostic (stage gate fails)",
        );

        // Elapsed gate fails: 3 conversions but elapsed == threshold (boundary).
        let plan_three = DispatchPlan {
            kernel: "kernel_d".to_string(),
            conversions: vec![
                ("kernel_a".to_string(), ReprKind::BRep, ReprKind::Mesh),
                ("kernel_b".to_string(), ReprKind::Mesh, ReprKind::Sdf),
                ("kernel_c".to_string(), ReprKind::Sdf, ReprKind::Voxel),
            ],
        };
        assert_eq!(
            long_chain_diagnostic(&plan_three, threshold, threshold),
            None,
            "elapsed exactly == threshold must NOT emit (elapsed gate fails)",
        );
    }

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
        // Pin the final-stage kernel and the conversion-stage content so a
        // regression that flips the chosen kernel (e.g. by reversing the
        // BTreeMap probe direction) breaks loudly instead of slipping past
        // the length-only assertion. With both kernels listing
        // `(BooleanUnion, Mesh)`, lexicographic tie-break selects "alpha";
        // with both listing `(Convert{BRep}→Mesh)` reachable in one step,
        // "alpha" is also the BTreeMap-first kernel that names the
        // conversion edge.
        assert_eq!(
            plan.kernel, "alpha",
            "lexicographic tie-break must pick 'alpha' over 'beta', got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            ("alpha".to_string(), ReprKind::BRep, ReprKind::Mesh),
            "the 1-stage conversion must be (alpha, BRep, Mesh), got {:?}",
            plan.conversions[0],
        );
    }

    /// Two-stage chain as winner: the only path from `{BRep}` to
    /// `(BooleanUnion, Mesh)` is BRep→Sdf (via alpha) then Sdf→Mesh (via
    /// beta), because no kernel declares `(Convert{BRep}, Mesh)`. Locks BFS
    /// multi-stage expansion as the *accepted-path winner*, not just the
    /// rejected-path loser as in `dispatch_prefers_shorter_chain`.
    #[test]
    fn dispatch_two_stage_chain_is_shortest() {
        // alpha: converts BRep → Sdf only. No direct BRep→Mesh anywhere.
        let alpha = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert { from: ReprKind::BRep },
                ReprKind::Sdf,
            )],
        };
        // beta: converts Sdf → Mesh only.
        let beta = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert { from: ReprKind::Sdf },
                ReprKind::Mesh,
            )],
        };
        // manifold: runs BooleanUnion on Mesh. No conversion edges declared.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("alpha".to_string(), &alpha);
        registry.insert("beta".to_string(), &beta);
        registry.insert("manifold".to_string(), &manifold);

        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);

        let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available)
            .expect("a 2-stage chain BRep→Sdf→Mesh→Union must be findable");

        assert_eq!(
            plan.conversions.len(),
            2,
            "exactly two conversion stages (BRep→Sdf, Sdf→Mesh) are required, got {plan:?}",
        );
        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            ("alpha".to_string(), ReprKind::BRep, ReprKind::Sdf),
            "first conversion stage must be (alpha, BRep, Sdf), got {:?}",
            plan.conversions[0],
        );
        assert_eq!(
            plan.conversions[1],
            ("beta".to_string(), ReprKind::Sdf, ReprKind::Mesh),
            "second conversion stage must be (beta, Sdf, Mesh), got {:?}",
            plan.conversions[1],
        );
    }

    /// Locks the `seeds: BTreeSet<ReprKind>` seeding step, which canonicalises
    /// the hash-randomised `HashSet<ReprKind>` input into `Ord`-sorted order
    /// before the BFS frontier is populated.
    ///
    /// Registry shape: kappa converts BRep→Mesh, lambda converts Sdf→Mesh, and
    /// manifold runs BooleanUnion on Mesh. With both BRep and Sdf available,
    /// the `seeds` BTreeSet ensures BRep < Sdf in frontier order, so kappa is
    /// always chosen over lambda. Without the `seeds: BTreeSet<ReprKind>`
    /// seeding step, the outcome would depend on hash-randomised HashSet
    /// iteration, making CI output non-deterministic across hash-seed
    /// perturbations.
    #[test]
    fn dispatch_seeding_order_is_deterministic() {
        // kappa: converts BRep → Mesh in one step.
        let kappa = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert { from: ReprKind::BRep },
                ReprKind::Mesh,
            )],
        };
        // lambda: converts Sdf → Mesh in one step.
        let lambda = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert { from: ReprKind::Sdf },
                ReprKind::Mesh,
            )],
        };
        // manifold: runs BooleanUnion on Mesh.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("kappa".to_string(), &kappa);
        registry.insert("lambda".to_string(), &lambda);
        registry.insert("manifold".to_string(), &manifold);

        // Both reprs available. The `seeds: BTreeSet<ReprKind>` seeding step
        // guarantees BRep < Sdf traversal order so kappa always wins over
        // lambda, irrespective of the HashSet's per-process hash randomisation.
        let mut available: HashSet<ReprKind> = HashSet::new();
        available.insert(ReprKind::BRep);
        available.insert(ReprKind::Sdf);

        let plan = dispatch(&registry, Operation::BooleanUnion, ReprKind::Mesh, &available)
            .expect("kappa (BRep→Mesh) path must be findable");

        assert_eq!(
            plan.kernel, "manifold",
            "the final-stage Mesh BooleanUnion must run on manifold, got {plan:?}",
        );
        assert_eq!(
            plan.conversions.len(),
            1,
            "exactly one conversion stage (BRep→Mesh via kappa) expected, got {plan:?}",
        );
        assert_eq!(
            plan.conversions[0],
            ("kappa".to_string(), ReprKind::BRep, ReprKind::Mesh),
            "conversion stage must be (kappa, BRep, Mesh) — BRep < Sdf in BTreeSet order, got {:?}",
            plan.conversions[0],
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
