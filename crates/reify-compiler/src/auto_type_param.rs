//! Phases A and B of the `auto` type-parameter resolution algorithm.
//!
//! # Phase A — Candidate Enumeration
//!
//! Implements the **candidate enumeration** step described in
//! `docs/prds/auto-type-param-resolution.md` and `docs/reify-language-spec.md`
//! §3.9 (lines 474–516): walk the in-scope name table at the use site and
//! collect every concrete structure whose declared trait bounds satisfy
//! a required trait bound. The pool is capped at
//! [`MAX_AUTO_TYPE_PARAM_CANDIDATES`]; if the pool would exceed the cap, an
//! [`reify_types::DiagnosticCode::AutoTypeParamPoolOverflow`] error is emitted
//! and the (alphabetically-first) capped list is returned to the caller.
//!
//! # Phase B — Per-Candidate Feasibility Filter
//!
//! Filters the candidates produced by Phase A down to those that do not
//! provably falsify any of the parameterized template's **top-level
//! (unguarded) constraints**. Public surface:
//!
//! - [`filter_feasible_candidates`] — core filter function.
//! - [`FeasibilityResult`] — two-arm result enum (`Empty` / `Feasible`).
//! - [`RejectedCandidate`] — candidate name + the violated constraint ids.
//!
//! Feasibility predicate (architecture §2.5 monotonic-feasible):
//! `feasible(c) ≡ satisfaction != Violated`. Both `Satisfied` and
//! `Indeterminate` count as feasible; only `Violated` causes rejection.
//!
//! Explicit scope cuts for Phase B:
//! 1. **Top-level constraints only.** `template.constraints` is checked;
//!    guarded-group constraints are NOT collected here (that lives in
//!    `reify-eval`; guard-aware filtering is a follow-up task).
//! 2. **No type-substitution mechanics.** With an empty `ValueMap`, the
//!    candidate name does not yet vary constraint outcomes. A future task
//!    will substitute `Type::TypeParam(T)` → `Type::StructureRef(candidate)`
//!    and supply per-candidate resolved defaults.
//! 3. **Monotonic `Indeterminate = feasible`.** Per architecture §2.5,
//!    `Undef` cells produce `Indeterminate` — this is NOT a rejection signal.
//!    Adding values can only flip `Indeterminate → Satisfied/Violated`, never
//!    the reverse, preserving the architecture's monotonicity guarantee.
//!
//! # Common Scope
//!
//! Both phases are delivered as **pure utility modules**: the parser does not
//! yet accept `auto: TraitName` syntax inside `type_arg_list`
//! (`tree-sitter-reify/grammar.js:601-605` only permits `$.type_expr`), so
//! end-to-end source-level resolution is impossible until a follow-up parser
//! task lands the new syntax. Functions in this module are unit-tested against
//! compiler-built registries; a future task will wire them into the compile
//! pipeline once the parser/AST learn `auto:` in type-arg position.
//!
//! Phases C (selection logic / strict-vs-free) and D (topology trigger) are
//! explicitly deferred to follow-up tasks.

use std::collections::HashMap;

use reify_types::{
    ConstraintChecker, ConstraintInput, ConstraintNodeId, Diagnostic, DiagnosticCode,
    DiagnosticLabel, CompiledFunction, SourceSpan,
};

use crate::entity::satisfies_trait_bound;
use crate::types::{CompiledTrait, TopologyTemplate};

/// Maximum size of the `auto:` candidate pool.
///
/// PRD `docs/prds/auto-type-param-resolution.md` §"Phase A" mandates that
/// the pool of in-scope structures satisfying the bound is capped at 10.
/// If more candidates exist, a hard error
/// (`E_AUTO_TYPE_PARAM_POOL_OVERFLOW`) is emitted and only the first 10
/// alphabetically (by FQN) are reported.
pub const MAX_AUTO_TYPE_PARAM_CANDIDATES: usize = 10;

/// The result of [`enumerate_candidates`].
///
/// Three arms map to three downstream actions for callers:
/// - [`CandidateEnumeration::Empty`] → selection phase will emit
///   `E_AUTO_TYPE_PARAM_NO_CANDIDATE`. **No** diagnostic was emitted by
///   `enumerate_candidates` itself.
/// - [`CandidateEnumeration::Found`] → caller (Phase B) runs feasibility
///   filter on these candidates.
/// - [`CandidateEnumeration::Overflow`] → bail. The
///   [`reify_types::DiagnosticCode::AutoTypeParamPoolOverflow`] diagnostic
///   has **already been pushed** onto the supplied diagnostic vector
///   (caller MUST NOT run feasibility — overflow is a hard error and the
///   full pool is not available).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateEnumeration {
    /// Zero candidates satisfy the bound. Selection phase (future task)
    /// will emit `E_AUTO_TYPE_PARAM_NO_CANDIDATE`; this function emits no
    /// diagnostic on this arm.
    Empty,
    /// 1..=[`MAX_AUTO_TYPE_PARAM_CANDIDATES`] candidates, sorted
    /// alphabetically by template name (FQN). Caller (Phase B) runs
    /// feasibility filter on these.
    Found(Vec<String>),
    /// Pool overflowed (>[`MAX_AUTO_TYPE_PARAM_CANDIDATES`]).  The vector
    /// is the first `MAX` alphabetically; the
    /// [`reify_types::DiagnosticCode::AutoTypeParamPoolOverflow`]
    /// diagnostic has already been pushed onto the diagnostic vector with
    /// the same names attached via [`reify_types::Diagnostic::candidates`].
    /// Caller MUST NOT run feasibility — overflow is a hard error.
    Overflow(Vec<String>),
}

/// Phase A of `auto:` type-parameter resolution: enumerate every concrete
/// structure in the in-scope template registry whose declared trait
/// bounds satisfy **all** of the supplied `bounds`, capped at
/// [`MAX_AUTO_TYPE_PARAM_CANDIDATES`].
///
/// # Determinism
///
/// `template_registry` is a `HashMap` whose iteration order is not
/// stable. To make the result and any emitted diagnostic deterministic
/// across runs and platforms (PRD acceptance criterion 11), templates
/// are visited in alphabetically-sorted name order, and the returned
/// `Vec` is sorted alphabetically by template name (FQN). The "first 10
/// of the pool" reported on overflow is therefore exactly "first 10
/// alphabetically of the entire pool" — no excluded match could be
/// alphabetically earlier than position 10 because we iterate in sorted
/// order with early termination at `MAX+1`.
///
/// # Composite-bound semantics
///
/// `bounds` is treated as an **intersection**: a template qualifies only
/// if it satisfies *every* bound in the slice (PRD §"Phase A":
/// "Composite (`T: TraitA + TraitB`): intersection.").
/// `bounds.is_empty()` is a precondition violation guarded by a
/// `debug_assert!` — every type would match an empty bound, which is
/// meaningless for `auto:`.
///
/// # Trait-conformance predicate
///
/// Each candidate is checked via [`satisfies_trait_bound`] which walks
/// trait refinement chains transitively (with cycle protection). A
/// structure that declares conformance to a trait `B` which refines `A`
/// satisfies a required bound of `A`.
pub fn enumerate_candidates(
    bounds: &[String],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    use_site_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> CandidateEnumeration {
    debug_assert!(
        !bounds.is_empty(),
        "enumerate_candidates: bounds slice must be non-empty (every type would match an empty bound)"
    );

    // Visit templates in alphabetically-sorted name order so the result
    // is deterministic regardless of HashMap iteration order.
    let mut sorted_names: Vec<&String> = template_registry.keys().collect();
    sorted_names.sort();

    // Allow `collected` to grow to MAX+1 so the caller can detect
    // overflow; once the count exceeds MAX we stop iterating because
    // every later match (if any) would be alphabetically-after the
    // first MAX (sorted iteration ⇒ first MAX in `collected[0..MAX]`
    // are exactly the alphabetically-first MAX of the entire pool).
    let mut collected: Vec<String> = Vec::new();
    for name in sorted_names {
        let tmpl = template_registry
            .get(name.as_str())
            .expect("name was just enumerated from the same template_registry map");
        if bounds
            .iter()
            .all(|b| satisfies_trait_bound(&tmpl.trait_bounds, b, trait_registry))
        {
            collected.push((*name).clone());
            // Push first, check second: this lets us distinguish
            // "exactly MAX" from "MAX+1" for the Found vs Overflow
            // split. The (MAX+1)-th entry is `truncate`d off below;
            // we deliberately allocate one extra String to detect
            // overflow.
            if collected.len() > MAX_AUTO_TYPE_PARAM_CANDIDATES {
                // We have one more match than MAX — that's enough to
                // conclude overflow; further iteration cannot change
                // the alphabetically-first MAX.
                break;
            }
        }
    }
    // Sorted iteration above already yields sorted output; this assert
    // pins the invariant so a future refactor that changes the iteration
    // strategy fails loudly in debug builds rather than silently breaking
    // determinism.
    debug_assert!(
        collected.windows(2).all(|w| w[0] <= w[1]),
        "sorted iteration must yield sorted output"
    );

    if collected.is_empty() {
        CandidateEnumeration::Empty
    } else if collected.len() > MAX_AUTO_TYPE_PARAM_CANDIDATES {
        collected.truncate(MAX_AUTO_TYPE_PARAM_CANDIDATES);
        let joined_bounds = bounds.join(" + ");
        let names_list = collected.join(", ");
        let message = format!(
            "auto type parameter has more than {max} candidates satisfying bound '{bounds_str}'; first {max} alphabetically: {names}",
            max = MAX_AUTO_TYPE_PARAM_CANDIDATES,
            bounds_str = joined_bounds,
            names = names_list,
        );
        let label_message = format!("auto type-param bound '{}' here", joined_bounds);
        diagnostics.push(
            Diagnostic::error(message)
                .with_code(DiagnosticCode::AutoTypeParamPoolOverflow)
                .with_label(DiagnosticLabel::new(use_site_span, label_message))
                .with_candidates(collected.clone()),
        );
        CandidateEnumeration::Overflow(collected)
    } else {
        CandidateEnumeration::Found(collected)
    }
}

// ─── Phase B: per-candidate feasibility filter ────────────────────────────

/// A candidate that was rejected by Phase B's feasibility filter.
///
/// Carries the candidate's name and the `ConstraintNodeId`s of every
/// constraint whose result was [`reify_types::Satisfaction::Violated`].
/// Only violated constraints are recorded here — `Satisfied` and
/// `Indeterminate` results do not appear in this list (architecture §2.5
/// monotonic-feasible: undef does not falsify).
///
/// Future Phase C can use this record to surface an
/// `E_AUTO_TYPE_PARAM_NO_CANDIDATE` diagnostic with per-candidate
/// rejection reasons, or re-run the checker for richer diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedCandidate {
    /// The name of the candidate (FQN, as produced by Phase A).
    pub name: String,
    /// The constraint node ids whose satisfaction was `Violated` for this
    /// candidate. Non-empty by construction (a candidate that has zero
    /// violated constraints is accepted, not rejected).
    pub violated_constraints: Vec<ConstraintNodeId>,
}

/// The result of [`filter_feasible_candidates`].
///
/// Two arms map to two downstream actions for Phase C (selection):
/// - [`FeasibilityResult::Empty`] → all candidates were rejected; selection
///   phase will emit `E_AUTO_TYPE_PARAM_NO_CANDIDATE` with the rejection
///   reasons from the `rejected` Vec. **No** diagnostic is emitted by
///   `filter_feasible_candidates` itself.
/// - [`FeasibilityResult::Feasible`] → at least one candidate survived;
///   `accepted.len()` drives Phase C's 0 / 1 / ≥2 dispatch.
///
/// The shape mirrors Phase A's [`CandidateEnumeration`] (Empty vs. Found)
/// intentionally. Both `accepted` and `rejected` preserve the input
/// (alphabetical) order from Phase A.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeasibilityResult {
    /// Every candidate was rejected. Selection phase will emit
    /// `E_AUTO_TYPE_PARAM_NO_CANDIDATE`; this function emits no diagnostic.
    Empty {
        /// The rejected candidates in input order, each paired with the
        /// violated constraint ids that caused rejection.
        rejected: Vec<RejectedCandidate>,
    },
    /// At least one candidate is feasible. `accepted.len()` drives Phase C.
    Feasible {
        /// Candidates that passed the feasibility filter, in input order.
        accepted: Vec<String>,
        /// Candidates that did not pass, in input order, paired with the
        /// violated constraint ids.
        rejected: Vec<RejectedCandidate>,
    },
}

/// Phase B of `auto:` type-parameter resolution: filter the candidates
/// produced by Phase A's [`enumerate_candidates`] down to those that do
/// not provably falsify any of the parameterized template's top-level
/// constraints.
///
/// # Feasibility predicate
///
/// For each candidate a [`ConstraintInput`] is built with:
/// - `constraints`: the template's top-level (unguarded) constraints,
///   one entry per [`crate::types::CompiledConstraint`].
/// - `values`: an empty [`reify_types::ValueMap`] (every cell `Undef`).
/// - `functions`: the supplied compiled functions.
/// - `determinacy`: `None`.
///
/// Then `constraint_checker.check(&input)` is called. A candidate is
/// **accepted** iff every result has
/// `satisfaction != `[`reify_types::Satisfaction::Violated`]`
/// (i.e., both `Satisfied` and `Indeterminate` count as feasible —
/// architecture §2.5: "undef does not falsify"). A candidate is
/// **rejected** when at least one result is `Violated`; the violated
/// constraint node ids are recorded in the [`RejectedCandidate`] entry.
///
/// # Scope (explicit deferrals)
///
/// - **Top-level constraints only.** `template.constraints` is checked;
///   guarded-group constraints (`template.guarded_groups[*].constraints`)
///   are NOT visited here. Guard-aware filtering belongs to the eval-side
///   pipeline and is deferred to a follow-up task.
/// - **No type-substitution mechanics.** With an empty `ValueMap`, the
///   candidate name does not yet vary constraint outcomes. A future task
///   will substitute `Type::TypeParam(T)` → `Type::StructureRef(candidate)`
///   in value-cell types and supply per-candidate resolved defaults.
/// - **No parser/AST integration.** Same as Phase A — pure utility function.
///
/// # Determinism
///
/// Input order is preserved in both `accepted` and `rejected`; callers are
/// expected to supply candidates in alphabetical order (as Phase A does),
/// so the output vectors inherit that alphabetical ordering.
///
/// # Satisfaction → feasibility mapping
///
/// The three-arm `Satisfaction` enum maps to two feasibility outcomes:
///
/// | `Satisfaction` arm   | Feasibility verdict | Why                                   |
/// |----------------------|---------------------|---------------------------------------|
/// | `Satisfied`          | **Feasible**        | Constraint is provably satisfied      |
/// | `Indeterminate`      | **Feasible**        | Undef does not falsify (arch §2.5)    |
/// | `Violated`           | **Infeasible**      | Constraint is provably falsified      |
///
/// Only `Violated` appears in [`RejectedCandidate::violated_constraints`];
/// `Satisfied` and `Indeterminate` ids are never recorded there.
pub fn filter_feasible_candidates(
    candidates: &[String],
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
) -> FeasibilityResult {
    use reify_types::{Satisfaction, ValueMap};

    if candidates.is_empty() {
        return FeasibilityResult::Empty {
            rejected: Vec::new(),
        };
    }

    let empty_values = ValueMap::new();
    let mut accepted: Vec<String> = Vec::new();
    let mut rejected: Vec<RejectedCandidate> = Vec::new();

    for candidate in candidates {
        // Build the constraint input for this candidate. Phase B uses an
        // empty ValueMap (every cell Undef). The constraint checker will
        // return Indeterminate for any constraint that depends on Undef
        // cells, which counts as feasible per architecture §2.5.
        let constraints: Vec<(ConstraintNodeId, &reify_types::CompiledExpr)> =
            parameterized_template
                .constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr))
                .collect();

        let input = ConstraintInput {
            constraints,
            values: &empty_values,
            functions,
            determinacy: None,
        };

        let results = constraint_checker.check(&input);

        // Collect only the ids that are Violated — Satisfied and
        // Indeterminate both pass the feasibility predicate.
        let violated: Vec<ConstraintNodeId> = results
            .into_iter()
            .filter(|r| r.satisfaction == Satisfaction::Violated)
            .map(|r| r.id)
            .collect();

        if violated.is_empty() {
            accepted.push(candidate.clone());
        } else {
            rejected.push(RejectedCandidate {
                name: candidate.clone(),
                violated_constraints: violated,
            });
        }
    }

    if accepted.is_empty() {
        FeasibilityResult::Empty { rejected }
    } else {
        FeasibilityResult::Feasible { accepted, rejected }
    }
}
