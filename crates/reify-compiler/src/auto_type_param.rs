//! Phases A, B, and C of the `auto` type-parameter resolution algorithm.
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
//! # Phase C — Selection (strict-vs-free dispatch + lexicographic tiebreak)
//!
//! Pure dispatcher over Phase B's [`FeasibilityResult`] that produces one of
//! three outcomes per PRD §"Phase C". Public surface:
//!
//! - [`select_candidate`] — core selection function.
//! - [`SelectionResult`] — three-arm result enum (`Selected` / `NoCandidate`
//!   / `Ambiguous`).
//!
//! Dispatch table (`accepted.len()` × `free`):
//!
//! | feasible | strict (`free=false`)                  | free (`free=true`)                          |
//! |----------|----------------------------------------|---------------------------------------------|
//! | 0        | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error  | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error      |
//! | 1        | `Selected(name)`, no diagnostic         | `Selected(name)`, no diagnostic             |
//! | ≥2       | `E_AUTO_TYPE_PARAM_AMBIGUOUS` error      | `W_AUTO_TYPE_PARAM_NON_UNIQUE` warning      |
//!
//! Single-feasible (`accepted.len() == 1`) is always selected directly
//! regardless of `free` — there is nothing to disambiguate, so emitting
//! `W_AUTO_TYPE_PARAM_NON_UNIQUE` would be both noise and a contract
//! violation. The lex-first tiebreak under `auto(free)` uses `accepted[0]`
//! because Phase B preserves Phase A's alphabetical input order.
//!
//! # Multi-Param Orchestration
//!
//! [`resolve_auto_type_params`] composes Phase A → B → C for each of a list of
//! `auto:` type-parameters, iterating in **declared order** (PRD task 4 /
//! acceptance criterion 6: "`Coupling<auto: A, auto: B>` — A resolves first;
//! B's candidate pool is computed against the resolved A").
//!
//! Key behaviors:
//!
//! - **Declared order** — params are visited in the slice order supplied by the
//!   caller, which must match source declaration order. The substitution Vec and
//!   `per_param` Vec both accumulate in that order.
//! - **Halt-on-first-failure** — when any param fails (`CandidateEnumeration::Overflow`,
//!   `SelectionResult::NoCandidate`, or `SelectionResult::Ambiguous`), the
//!   orchestrator records that param's outcome in `per_param` and STOPS. No later
//!   param is enumerated, feasibility-checked, or selected, and no additional
//!   diagnostics are emitted. This is the v0.1 "no cross-param backtracking" rule.
//! - **Substitution Vec** — `resolve_auto_type_params` returns a
//!   `Vec<(String, String)>` (`param_name → template_name`) in declared order.
//!   For v0.1 this Vec is recorded but NOT yet consumed by Phase A's `bounds`
//!   slice or Phase B's `ValueMap` (deferred substitution work; see Phase B
//!   scope cut 2). The wiring is in place so a future task can read the map
//!   without a signature change.
//! - **Per-param `free` flag** — each [`AutoTypeParam`] carries its own `free`
//!   flag; the orchestrator passes `param.free` to Phase C independently for
//!   each param. A strict param and a free param in the same call may produce
//!   different Phase C arms (error vs. warning+selected).
//!
//! **Why "Multi-Param Orchestration" not "Phase D"?** The existing module
//! doc-comment reserves "Phase D" for the SchemaNode topology-trigger work
//! (re-resolution on registry change; task 2388). Calling this section "Phase D"
//! would clash with that reservation. "Multi-Param Orchestration" maps directly
//! to PRD task 4's label without invalidating the existing Phase D reservation.
//!
//! # Common Scope
//!
//! All three phases are delivered as **pure utility modules**: the parser
//! does not yet accept `auto: TraitName` syntax inside `type_arg_list`
//! (`tree-sitter-reify/grammar.js:601-605` only permits `$.type_expr`), so
//! end-to-end source-level resolution is impossible until a follow-up parser
//! task lands the new syntax. Functions in this module are unit-tested against
//! compiler-built registries; a future task will wire them into the compile
//! pipeline once the parser/AST learn `auto:` in type-arg position.
//!
//! Phase D (topology trigger / re-resolution on registry change) is
//! explicitly deferred to a follow-up task.

use std::collections::HashMap;

use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintInput, ConstraintNodeId, Diagnostic,
    DiagnosticCode, DiagnosticLabel, SourceSpan,
};

use crate::entity::satisfies_trait_bound;
use crate::types::{CompiledTrait, TopologyTemplate};

// ─── Multi-Param Orchestration types ─────────────────────────────────────────

/// Input record for a single `auto:` type-parameter to be resolved by
/// [`resolve_auto_type_params`].
///
/// One `AutoTypeParam` is produced per `auto: TraitName` clause in the source.
/// The fields mirror the per-param information that Phase A/B/C consume:
/// - `name`: the type-parameter name (e.g., `"T"`, `"U"`).
/// - `bounds`: the list of required trait names (intersection semantics, same
///   as Phase A's `bounds` parameter).
/// - `free`: the strict-vs-free flag (same as Phase C's `free` parameter).
/// - `use_site_span`: the source span of the `auto:` clause, used for
///   diagnostic labels by Phase A and Phase C.
///
/// The order of params in the slice passed to `resolve_auto_type_params`
/// determines resolution order (declared order, PRD criterion 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoTypeParam {
    /// The type-parameter name (e.g., `"T"`).
    pub name: String,
    /// Required trait bound(s) — intersection semantics, matches Phase A's
    /// `bounds` parameter.
    pub bounds: Vec<String>,
    /// Strict (`false`) vs. free (`true`) resolution flag — matches Phase C's
    /// `free` parameter.
    pub free: bool,
    /// Span of the `auto:` clause at the use site — used for diagnostic labels.
    pub use_site_span: SourceSpan,
}

/// Result of [`resolve_auto_type_params`].
///
/// - `per_param` — one entry per *processed* param, in declared order. Each
///   entry is `(param_name, SelectionResult)`. All params up to and including
///   the first failure are recorded here; params *after* the first failure are
///   NOT recorded (halt-on-first-failure rule).
/// - `substitution` — only the *successfully resolved* params, in declared
///   order. Each entry is `(param_name, template_name)`. A param appears here
///   iff its `SelectionResult` was `Selected`.
///
/// The asymmetry between `per_param` and `substitution` is intentional and
/// load-bearing: `per_param` carries every outcome (success and the first
/// failure), while `substitution` carries only the successful substitutions.
/// Tests assert both lengths to pin declared-order halt semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiParamResolutionOutcome {
    /// Per-param outcomes in declared order, stopping at the first failure.
    pub per_param: Vec<(String, SelectionResult)>,
    /// Successfully resolved substitutions `(param_name, template_name)`, in
    /// declared order.
    pub substitution: Vec<(String, String)>,
}

/// Maximum size of the `auto:` candidate pool.
///
/// PRD `docs/prds/auto-type-param-resolution.md` §"Phase A" mandates that
/// the pool of in-scope structures satisfying the bound is capped at 10.
/// If more candidates exist, a hard error
/// (`E_AUTO_TYPE_PARAM_POOL_OVERFLOW`) is emitted and only the first 10
/// alphabetically (by FQN) are reported.
pub const MAX_AUTO_TYPE_PARAM_CANDIDATES: usize = 10;

/// Render the `bounds` slice for diagnostic display and produce the
/// matching label-message string in one step.
///
/// Used by every diagnostic-emitting site in this module (Phase A's
/// overflow path and all three Phase C arms) so the rendering convention
/// (`bounds.join(" + ")` for the bound list, `"auto type-param bound
/// '<bounds>' here"` for the label) lives in exactly one place. Returns
/// `(joined_bounds, label_message)` so callers that need the joined-bounds
/// string a second time (for the main message body) can reuse it without
/// re-joining.
fn render_auto_type_param_label(bounds: &[String]) -> (String, String) {
    let joined_bounds = bounds.join(" + ");
    let label_message = format!("auto type-param bound '{}' here", joined_bounds);
    (joined_bounds, label_message)
}

/// Emit an [`AutoTypeParamNoCandidate`] diagnostic for the zero-rejection-summary
/// case: no candidates at all in the pool (Phase A's empty-pool path) or all
/// candidates rejected but `rejected` is empty in a release build (Phase C's
/// release-build fallback).
///
/// This is the zero-rejection form of the message — "auto type parameter has no
/// feasible candidates for bound '{bounds}'" — with no rejection detail appended.
/// Both call sites previously duplicated this builder chain verbatim; centralising
/// it here prevents silent wording drift if the message is ever updated.
///
/// In the Phase-C release fallback, `rejected.is_empty()` is implied by
/// `rejection_summary.is_empty()` (the join cannot produce an empty string from a
/// non-empty rejection list), so emitting `with_candidates(Vec::new())` here matches
/// the previous behaviour.
///
/// [`AutoTypeParamNoCandidate`]: reify_types::DiagnosticCode::AutoTypeParamNoCandidate
fn emit_no_candidate_zero_rejections(
    bounds: &[String],
    use_site_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (joined_bounds, label_message) = render_auto_type_param_label(bounds);
    let message = format!(
        "auto type parameter has no feasible candidates for bound '{}'",
        joined_bounds,
    );
    diagnostics.push(
        Diagnostic::error(message)
            .with_code(DiagnosticCode::AutoTypeParamNoCandidate)
            .with_label(DiagnosticLabel::new(use_site_span, label_message))
            .with_candidates(Vec::<String>::new()),
    );
}

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
        let (joined_bounds, label_message) = render_auto_type_param_label(bounds);
        let names_list = collected.join(", ");
        let message = format!(
            "auto type parameter has more than {max} candidates satisfying bound '{bounds_str}'; first {max} alphabetically: {names}",
            max = MAX_AUTO_TYPE_PARAM_CANDIDATES,
            bounds_str = joined_bounds,
            names = names_list,
        );
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
/// # Preconditions
///
/// Callers MUST supply a **non-empty** `candidates` slice. Phase A's
/// [`CandidateEnumeration::Found`] arm guarantees 1..=[`MAX_AUTO_TYPE_PARAM_CANDIDATES`]
/// entries, so in normal usage this is always satisfied. `candidates.is_empty()`
/// is a precondition violation guarded by a `debug_assert!` — an empty slice
/// has no semantic meaning for the selection phase, and silently absorbing it
/// would mask Phase A wiring bugs.
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

    debug_assert!(
        !candidates.is_empty(),
        "filter_feasible_candidates: candidates slice must be non-empty (Phase A's Found arm guarantees ≥1 candidate)"
    );

    let empty_values = ValueMap::new();

    // Build the template's constraint list once — it does not change across
    // candidates in Phase B.  When the deferred type-substitution pass lands
    // (substituting Type::TypeParam(T) → Type::StructureRef(candidate)), this
    // will need to move back inside the loop with per-candidate ValueMap setup.
    let constraints_template: Vec<(ConstraintNodeId, &reify_types::CompiledExpr)> =
        parameterized_template
            .constraints
            .iter()
            .map(|c| (c.id.clone(), &c.expr))
            .collect();

    let mut accepted: Vec<String> = Vec::new();
    let mut rejected: Vec<RejectedCandidate> = Vec::new();

    for candidate in candidates {
        let input = ConstraintInput {
            constraints: constraints_template.clone(),
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

// ─── Phase C: selection (strict-vs-free dispatch + lexicographic tiebreak) ──

/// The result of [`select_candidate`].
///
/// Three arms map to three downstream actions for the caller (the
/// type-resolution pipeline):
/// - [`SelectionResult::Selected`] → the resolution succeeded; `name` is the
///   chosen candidate FQN. Either a sole feasible candidate (no diagnostic)
///   or — under `auto(free)` with ≥2 feasible candidates — the
///   lexicographically-first candidate (with `W_AUTO_TYPE_PARAM_NON_UNIQUE`
///   warning attached to the diagnostics vec).
/// - [`SelectionResult::NoCandidate`] → 0 candidates survived Phase B's
///   feasibility filter; an `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error has
///   already been pushed onto the diagnostics vec.
/// - [`SelectionResult::Ambiguous`] → ≥2 feasible candidates under strict
///   (`free = false`); an `E_AUTO_TYPE_PARAM_AMBIGUOUS` error has already
///   been pushed. The Vec carries the feasible candidate FQNs in input
///   order (alphabetical) so the caller can surface them to its own
///   diagnostic / error-recovery layer.
///
/// The shape mirrors the three-arm dispatch from PRD §"Phase C":
/// 0 / 1 / ≥2-feasible × strict / free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionResult {
    /// Resolution succeeded: a single candidate was chosen. Carries the
    /// chosen FQN.
    Selected(String),
    /// Zero feasible candidates. The corresponding `E_AUTO_TYPE_PARAM_NO_CANDIDATE`
    /// diagnostic has already been pushed onto the diagnostics vec.
    NoCandidate,
    /// Two or more feasible candidates under strict resolution. The
    /// `E_AUTO_TYPE_PARAM_AMBIGUOUS` diagnostic has already been pushed
    /// onto the diagnostics vec. The Vec carries the feasible candidate
    /// FQNs in input order.
    Ambiguous(Vec<String>),
}

/// Phase C of `auto:` type-parameter resolution: dispatch on
/// `accepted.len()` × `free` to produce one of three outcomes.
///
/// PRD `docs/prds/auto-type-param-resolution.md` §"Phase C":
///
/// | feasible | strict (`free=false`)                  | free (`free=true`)                          |
/// |----------|----------------------------------------|---------------------------------------------|
/// | 0        | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error  | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error      |
/// | 1        | `Selected(name)`, no diagnostic         | `Selected(name)`, no diagnostic             |
/// | ≥2       | `E_AUTO_TYPE_PARAM_AMBIGUOUS` error      | `W_AUTO_TYPE_PARAM_NON_UNIQUE` warning      |
///
/// The `feasibility` argument is consumed by value: this function extracts
/// owned `Vec<String>` (`accepted`) and `Vec<RejectedCandidate>` (`rejected`)
/// for the diagnostic-content and `SelectionResult` arms. By-value matches
/// the natural ownership flow (Phase B → Phase C → consumer) and avoids
/// clones.
///
/// `bounds` is the trait-bound list from the `auto: TraitName + ...` syntax
/// (intersection semantics, mirrors Phase A's same-named param). Used only
/// for diagnostic-message rendering (`bounds.join(" + ")`).
///
/// `use_site_span` is attached as the (single) label span on every
/// diagnostic this function emits.
pub fn select_candidate(
    feasibility: FeasibilityResult,
    bounds: &[String],
    free: bool,
    use_site_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> SelectionResult {
    // Mirror Phase A's `enumerate_candidates` precondition: an empty bound
    // slice has no semantic meaning for `auto:` and would render `''` in
    // diagnostic messages. Phase A guards the same invariant; pinning it
    // here too prevents callers that bypass Phase A from silently producing
    // malformed messages.
    debug_assert!(
        !bounds.is_empty(),
        "select_candidate: bounds slice must be non-empty (every type would match an empty bound)"
    );
    match feasibility {
        FeasibilityResult::Empty { rejected } => {
            debug_assert!(
                !rejected.is_empty(),
                "FeasibilityResult::Empty must carry at least one rejected candidate; \
                 otherwise Phase A's empty-pool path would have fired first"
            );
            // Per-rejection prose: each candidate paired with its violated
            // constraint ids. v0.1 encodes per-rejection details as a string;
            // the structured `candidates` field carries just the FQNs (parity
            // with AutoTypeParamPoolOverflow). LSP integration (task 2389)
            // will surface richer per-rejection structure later.
            let rejection_summary = rejected
                .iter()
                .map(|r| {
                    let ids = r
                        .violated_constraints
                        .iter()
                        .map(|c| format!("{}#{}", c.entity, c.index))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("'{}' rejected by constraint {}", r.name, ids)
                })
                .collect::<Vec<_>>()
                .join(", ");
            // In debug builds the assert above fires before this point when
            // `rejected` is empty. In release builds (where `debug_assert!`
            // is a no-op), the zero-rejection path delegates to the shared
            // helper so both the Phase A empty-pool path and this release-build
            // fallback produce the same base message without duplication.
            if rejection_summary.is_empty() {
                emit_no_candidate_zero_rejections(bounds, use_site_span, diagnostics);
            } else {
                let (joined_bounds, label_message) = render_auto_type_param_label(bounds);
                let rejected_names: Vec<String> =
                    rejected.iter().map(|r| r.name.clone()).collect();
                let message = format!(
                    "auto type parameter has no feasible candidates for bound '{bounds_str}': {summary}",
                    bounds_str = joined_bounds,
                    summary = rejection_summary,
                );
                diagnostics.push(
                    Diagnostic::error(message)
                        .with_code(DiagnosticCode::AutoTypeParamNoCandidate)
                        .with_label(DiagnosticLabel::new(use_site_span, label_message))
                        .with_candidates(rejected_names),
                );
            }
            SelectionResult::NoCandidate
        }
        FeasibilityResult::Feasible { accepted, .. } => {
            // Single-feasible: there is nothing to disambiguate, so emit no
            // diagnostic and return Selected(name). This branch DOES NOT
            // consult `free` — under PRD §"Phase C", a sole feasible
            // candidate is always selected directly regardless of strict
            // vs. free; emitting `W_AUTO_TYPE_PARAM_NON_UNIQUE` here would
            // be both noise and a contract violation.
            if accepted.len() == 1 {
                let name = accepted
                    .into_iter()
                    .next()
                    .expect("length-1 by guard above");
                return SelectionResult::Selected(name);
            }
            // ≥2 feasible: dispatch on `free` flag.
            debug_assert!(
                accepted.len() >= 2,
                "FeasibilityResult::Feasible.accepted is non-empty by construction; \
                 length-1 path returned above; only length-≥2 reaches here"
            );
            // The lex-first contract for the AMBIGUOUS / NON_UNIQUE arms
            // depends on `accepted[0]` being the alphabetically-first FQN.
            // Phase B preserves Phase A's sortedness, but `select_candidate`
            // accepts a `FeasibilityResult` from any caller (the tests
            // construct one by hand). Pin the invariant here so a caller
            // that bypasses Phase A/B with an unsorted accepted vec fails
            // loudly in debug builds rather than silently breaking the
            // public "lexicographically-first" contract.
            debug_assert!(
                accepted.windows(2).all(|w| w[0] <= w[1]),
                "select_candidate: accepted must be in alphabetical order — Phase B preserves Phase A's sortedness"
            );
            if !free {
                // Strict (`free=false`) + ≥2 feasible → AMBIGUOUS error.
                let (joined_bounds, label_message) = render_auto_type_param_label(bounds);
                let candidates_join = accepted.join(", ");
                let lex_first = &accepted[0];
                let message = format!(
                    "auto type parameter has multiple feasible candidates for bound '{bounds_str}': {names}; consider an explicit substitution like '{lex_first}' instead of 'auto:'",
                    bounds_str = joined_bounds,
                    names = candidates_join,
                    lex_first = lex_first,
                );
                diagnostics.push(
                    Diagnostic::error(message)
                        .with_code(DiagnosticCode::AutoTypeParamAmbiguous)
                        .with_label(DiagnosticLabel::new(use_site_span, label_message))
                        .with_candidates(accepted.clone()),
                );
                return SelectionResult::Ambiguous(accepted);
            }
            // Free (`free=true`) + ≥2 feasible → NON_UNIQUE warning;
            // select the lexicographically-first candidate. Phase B preserves
            // Phase A's alphabetical input order, so `accepted[0]` IS the
            // lex-first FQN.
            let (joined_bounds, label_message) = render_auto_type_param_label(bounds);
            let candidates_join = accepted.join(", ");
            let lex_first = accepted[0].clone();
            let message = format!(
                "auto(free) type parameter has multiple feasible candidates for bound '{bounds_str}': {names}; selected lexicographically-first '{lex_first}'",
                bounds_str = joined_bounds,
                names = candidates_join,
                lex_first = lex_first,
            );
            diagnostics.push(
                Diagnostic::warning(message)
                    .with_code(DiagnosticCode::AutoTypeParamNonUnique)
                    .with_label(DiagnosticLabel::new(use_site_span, label_message))
                    .with_candidates(accepted),
            );
            SelectionResult::Selected(lex_first)
        }
    }
}

// ─── Multi-Param Orchestration ────────────────────────────────────────────────

/// Resolve multiple `auto:` type-parameters in **declared order**, composing
/// Phase A → B → C for each param and halting on the first failure.
///
/// # Declared-order semantics (PRD criterion 6)
///
/// Params are iterated in the order supplied by the caller — declared order.
/// Each `SelectionResult::Selected(name)` is recorded into `substitution` in
/// that order. The v0.1 substitution map is plumbed through but NOT yet
/// consumed by Phase A's `bounds` slice or Phase B's `ValueMap`; the deferred
/// type-substitution mechanics (substituting `Type::TypeParam(T)` →
/// `Type::StructureRef(candidate)`) will read this vec without a signature
/// change when that follow-up task lands.
///
/// # Halt-on-first-failure
///
/// When any param fails — `CandidateEnumeration::Overflow`, `SelectionResult::NoCandidate`,
/// or `SelectionResult::Ambiguous` — the orchestrator records that param's
/// outcome in `per_param` and **stops**. Later params are not enumerated,
/// not feasibility-checked, and emit no diagnostics. This is the v0.1
/// "no cross-param backtracking" rule.
///
/// ## Overflow → Ambiguous mapping
///
/// `CandidateEnumeration::Overflow` carries the first
/// `MAX_AUTO_TYPE_PARAM_CANDIDATES` alphabetically and is modelled as
/// `SelectionResult::Ambiguous(overflow_vec)` in `per_param`. Both overflow
/// and strict-ambiguous share the same "≥2 candidates, can't pick one without
/// user input" shape. Phase A has already pushed the overflow diagnostic before
/// the orchestrator runs, so no second diagnostic is emitted here.
///
/// # Per-param `free` flag
///
/// Each `AutoTypeParam` carries its own `free` flag (strict vs. free
/// resolution). The orchestrator passes `param.free` to Phase C independently
/// for each param — it does NOT use a single `free` value for all params.
///
/// # Empty params
///
/// An empty `params` slice is a vacuous success: the function returns an
/// empty `MultiParamResolutionOutcome` immediately without emitting any
/// diagnostic. This is NOT a precondition violation — a definition with zero
/// `auto:` type-params has no orchestration work to do.
///
/// # Naming note
///
/// This section is named "Multi-Param Orchestration" (not "Phase D") because
/// the existing module doc-comment reserves "Phase D" for the SchemaNode
/// topology-trigger work (task 2388). The PRD refers to this functionality as
/// task 4 / PRD criterion 6.
pub fn resolve_auto_type_params(
    params: &[AutoTypeParam],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> MultiParamResolutionOutcome {
    // Vacuous success: empty params slice is a valid no-op (not a bug).
    if params.is_empty() {
        return MultiParamResolutionOutcome {
            per_param: vec![],
            substitution: vec![],
        };
    }

    let mut per_param: Vec<(String, SelectionResult)> = Vec::new();
    let mut substitution: Vec<(String, String)> = Vec::new();

    // declared order — see PRD criterion 6.
    // After a Selected outcome the loop FALLS THROUGH to the next param
    // (no early return); per_param and substitution both accumulate in
    // declared order.
    for param in params {
        // Shadows enumerate_candidates' own non-empty-bounds assert with
        // orchestrator-level wording so failures point at the caller of
        // resolve_auto_type_params rather than its delegate.
        debug_assert!(
            !param.bounds.is_empty(),
            "resolve_auto_type_params: param.bounds must be non-empty (every type would match an empty bound)"
        );
        // Phase A: enumerate candidates.
        let enumeration = enumerate_candidates(
            &param.bounds,
            template_registry,
            trait_registry,
            param.use_site_span,
            diagnostics,
        );

        // Phases A → B → C: dispatch on the enumeration outcome.
        // `CandidateEnumeration::Empty` is handled directly here, removing the
        // need for a synthetic `FeasibilityResult::Empty` and the downstream
        // `matches!` heuristic that re-discovered the empty-pool condition by
        // inspecting the synthetic value's `rejected.is_empty()`.
        let selection = match enumeration {
            CandidateEnumeration::Empty => {
                // Phase A found zero in-scope structures satisfying the bound.
                // Emit NoCandidate directly — no Phase B or C call needed.
                emit_no_candidate_zero_rejections(&param.bounds, param.use_site_span, diagnostics);
                SelectionResult::NoCandidate
            }
            CandidateEnumeration::Overflow(overflow_vec) => {
                // Phase A already pushed the overflow diagnostic.
                // Model overflow as Ambiguous (same "≥2 candidates, can't
                // pick one without user input" shape); the outer match below
                // handles the push-and-break for all failure modes uniformly.
                SelectionResult::Ambiguous(overflow_vec)
            }
            CandidateEnumeration::Found(candidates) => {
                // Phase B: feasibility filter.
                let feasibility = filter_feasible_candidates(
                    &candidates,
                    parameterized_template,
                    constraint_checker,
                    functions,
                );
                // Phase C: selection.
                select_candidate(
                    feasibility,
                    &param.bounds,
                    param.free,
                    param.use_site_span,
                    diagnostics,
                )
            }
        };

        // Asymmetry contract (step-14): `per_param` accumulates EVERY
        // processed param (success or first failure); `substitution` carries
        // ONLY `Selected` entries. A caller that needs all outcomes inspects
        // `per_param`; a caller that needs only resolved names inspects
        // `substitution`. The two vecs have the same declared-order prefix for
        // all successful params, then `per_param` gains the single failure
        // entry while `substitution` stops. This asymmetry is load-bearing:
        // `substitution` is the map consumed by type-substitution mechanics;
        // feeding it a NoCandidate or Ambiguous entry would corrupt substitution.
        match selection {
            SelectionResult::Selected(ref name) => {
                // Selected path (step-4 contract): record in BOTH substitution
                // (for later type-substitution consumers) AND per_param (for
                // callers inspecting per-param outcomes), then continue the
                // loop — do NOT break or return.
                substitution.push((param.name.clone(), name.clone()));
                per_param.push((param.name.clone(), selection));
            }
            SelectionResult::NoCandidate | SelectionResult::Ambiguous(_) => {
                // All three failure modes (Overflow → Ambiguous, Phase A
                // empty-pool → NoCandidate, Phase C all-rejected → NoCandidate)
                // converge here. The appropriate diagnostic was already pushed
                // by the emitting site. Record the failure in per_param and
                // halt — no later param is enumerated, feasibility-checked, or
                // selected (halt-on-first-failure, v0.1 rule). The failure
                // entry is intentionally NOT pushed into substitution (see
                // asymmetry contract comment above).
                per_param.push((param.name.clone(), selection));
                break;
            }
        }
    }

    MultiParamResolutionOutcome {
        per_param,
        substitution,
    }
}

// ─── v0.2 — Backtracking: DFS over cross-product with depth bound ─────────
//
// Driving PRD: docs/prds/v0_2/auto-resolution-backtracking.md.
//
// `resolve_auto_type_params_with_backtracking` extends v0.1's per-param BFS
// (`resolve_auto_type_params` above) into a depth-first search over the
// cross-product of `auto:` candidate sets. At each leaf assignment of the
// cross-product, `filter_feasible_candidates` is re-invoked (full re-check
// per the PRD design decision "implement v0.2 search with full re-check at
// each binding") to determine feasibility; an infeasible leaf triggers
// backtracking to the next sibling at the deepest open level.
//
// Above the depth bound `params.len() > max_depth`, the function emits
// `AutoTypeParamDepthBoundExceeded` (Severity::Warning) and delegates back
// to `resolve_auto_type_params` (BFS). The fallback is functionally correct
// (BFS is sound, just less complete than DFS over cross-product) so the
// user has a working compile — the warning is for auditability.
//
// # Out of scope (sibling tasks layered on top of this foundation)
//
// - Backjumping via the "rejected because" channel — task 2660.
// - `auto(free)` report-all cross-product enumeration with the
//   `AutoTypeParamNonUnique` warning — task 2661.
// - Cross-product hard cap of 100k assignments — task 2662.
// - Rich diagnostic format with smallest infeasibility witness — task 2663.
// - Comprehensive v0.1 BFS-failure scenario coverage — task 2664.
// - Type-substitution mechanics
//   (`Type::TypeParam(T)` → `Type::StructureRef(candidate)`) — separately
//   deferred per the PRD's "Constraint-feasibility incremental binding
//   deferred" decision.

/// DFS over the cross-product of `auto:` candidate sets with a depth bound.
///
/// See the section header comment above for context, design decisions, and
/// out-of-scope deferrals to sibling tasks.
///
/// `max_depth` is taken as a scalar (not a `&AutoTypeParamsConfig`) per the
/// design decision: algorithm correctness does not depend on where the value
/// was sourced, and this keeps the algorithm crate independent of
/// `reify-config`. The eventual call-site reads
/// `Manifest::auto_type_params().max_depth` and passes it in directly.
pub fn resolve_auto_type_params_with_backtracking(
    params: &[AutoTypeParam],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    max_depth: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> MultiParamResolutionOutcome {
    // Vacuous success: empty params slice is a valid no-op (parity with
    // v0.1 BFS's `resolve_auto_type_params`). In particular, `0` is never
    // greater than `max_depth` (which `reify-config` already rejects when
    // `max_depth == 0`), so the depth-bound branch does not fire here.
    if params.is_empty() {
        let _ = max_depth;
        return MultiParamResolutionOutcome {
            per_param: vec![],
            substitution: vec![],
        };
    }

    // Phase A enumeration runs ONCE per param up front (before recursion),
    // producing a `Vec<Vec<String>>` of per-param candidate vectors. This
    // hoists Phase A out of the DFS body — Phase A depends only on the
    // trait-bound + template registry, not on already-bound params (the
    // deferred substitution mechanics are the only piece that would couple
    // them; that refactor point is documented in the design decision).
    //
    // Phase A failure modes (`Empty` / `Overflow`) on any param halt the
    // whole search before recursion begins, with the same per_param /
    // substitution shape as v0.1 BFS's halt-on-first-failure rule.
    let mut per_param_candidates: Vec<Vec<String>> = Vec::with_capacity(params.len());
    for param in params {
        // Shadow `enumerate_candidates`' own non-empty-bounds assert with
        // orchestrator-level wording so failures point at the caller of
        // `resolve_auto_type_params_with_backtracking` rather than its delegate.
        debug_assert!(
            !param.bounds.is_empty(),
            "resolve_auto_type_params_with_backtracking: param.bounds must be non-empty (every type would match an empty bound)"
        );
        let enumeration = enumerate_candidates(
            &param.bounds,
            template_registry,
            trait_registry,
            param.use_site_span,
            diagnostics,
        );
        match enumeration {
            CandidateEnumeration::Empty => {
                // Phase A found zero in-scope structures satisfying the bound.
                // Mirrors `resolve_auto_type_params`'s identical handling at
                // the empty-pool arm: emit NoCandidate directly with the
                // zero-rejections message form.
                emit_no_candidate_zero_rejections(
                    &param.bounds,
                    param.use_site_span,
                    diagnostics,
                );
                return MultiParamResolutionOutcome {
                    per_param: vec![(param.name.clone(), SelectionResult::NoCandidate)],
                    substitution: vec![],
                };
            }
            CandidateEnumeration::Overflow(overflow_vec) => {
                // Phase A already pushed the overflow diagnostic. Mirror
                // BFS's "Overflow → Ambiguous" mapping in `per_param` so the
                // outer-shape contract is identical.
                return MultiParamResolutionOutcome {
                    per_param: vec![(
                        param.name.clone(),
                        SelectionResult::Ambiguous(overflow_vec),
                    )],
                    substitution: vec![],
                };
            }
            CandidateEnumeration::Found(candidates) => {
                per_param_candidates.push(candidates);
            }
        }
    }

    // Single-param degenerate path: with exactly one param, the cross-product
    // collapses to a flat enumeration of that param's candidates and the
    // recursion is degenerate. Route through the existing Phase B / Phase C
    // helpers verbatim — the result is identical to BFS's single-param
    // pipeline. Multi-param recursion is wired in step-20 of task 2659.
    if params.len() == 1 {
        let param = &params[0];
        let candidates = &per_param_candidates[0];
        let feasibility = filter_feasible_candidates(
            candidates,
            parameterized_template,
            constraint_checker,
            functions,
        );
        let selection = select_candidate(
            feasibility,
            &param.bounds,
            param.free,
            param.use_site_span,
            diagnostics,
        );
        let mut per_param = Vec::with_capacity(1);
        let mut substitution = Vec::with_capacity(1);
        match selection {
            SelectionResult::Selected(ref name) => {
                substitution.push((param.name.clone(), name.clone()));
                per_param.push((param.name.clone(), selection));
            }
            SelectionResult::NoCandidate | SelectionResult::Ambiguous(_) => {
                per_param.push((param.name.clone(), selection));
            }
        }
        return MultiParamResolutionOutcome {
            per_param,
            substitution,
        };
    }

    // Multi-param DFS over the cross-product. The recursive helper visits
    // leaves in declared-order × lexicographic-within-param order (T outer,
    // U inner, …). For step-20 we run in "stop at first feasible" mode —
    // free-mode behavior. Strict-vs-free dispatch lands in step-24, which
    // will continue searching past the first feasible leaf to detect ≥2
    // (Ambiguous).
    let mut current: Vec<String> = Vec::with_capacity(params.len());
    let mut feasible_assignments: Vec<Vec<String>> = Vec::new();
    dfs_search(
        0,
        &per_param_candidates,
        &mut current,
        &mut feasible_assignments,
        parameterized_template,
        constraint_checker,
        functions,
        /* stop_after_first_feasible: */ true,
    );

    if let Some(first_feasible) = feasible_assignments.into_iter().next() {
        // Build per_param + substitution from the first feasible cross-product
        // assignment, in declared order. Each entry pairs `params[i].name`
        // with the candidate selected at level `i`. The asymmetry contract
        // mirrors v0.1 BFS: per_param carries Selected entries, substitution
        // carries (param_name, candidate) pairs — both in declared order.
        let per_param: Vec<(String, SelectionResult)> = params
            .iter()
            .zip(first_feasible.iter())
            .map(|(p, name)| (p.name.clone(), SelectionResult::Selected(name.clone())))
            .collect();
        let substitution: Vec<(String, String)> = params
            .iter()
            .zip(first_feasible.iter())
            .map(|(p, name)| (p.name.clone(), name.clone()))
            .collect();
        return MultiParamResolutionOutcome {
            per_param,
            substitution,
        };
    }

    // No feasible cross-product assignment. Strict-vs-free dispatch and the
    // NoCandidate / Ambiguous diagnostic encoding land in step-24. Until
    // then, this branch is unreachable from any current test (step-19 and
    // step-21 both have at least one feasible leaf by construction).
    unimplemented!(
        "resolve_auto_type_params_with_backtracking: zero-feasible cross-product \
         encoding (NoCandidate diagnostic + per_param shape) lands in step-24 \
         of task 2659."
    )
}

// ─── DFS recursion helpers (v0.2) ────────────────────────────────────────

/// True iff the current cross-product leaf assignment is feasible.
///
/// Synthesizes a single-element placeholder vec for `filter_feasible_candidates`
/// so exactly one `constraint_checker.check()` call fires per leaf. With
/// deferred substitution mechanics (see the Phase B scope cut), the
/// candidate name does not affect the verdict — it serves only as a stable
/// label if a future task (e.g. 2663's rich diagnostic format) lifts the
/// leaf verdict into per-leaf diagnostic structure.
///
/// The "single check per leaf" shape is what makes the
/// [`reify_test_support::MockConstraintChecker::with_call_queue`] FIFO model
/// useful for backtracking tests: one queue pop drives one leaf verdict.
///
/// Inherits Phase B's monotonic-feasible rule (architecture §2.5):
/// `Indeterminate` counts as feasible — only `Satisfaction::Violated` on the
/// single-candidate input maps to [`FeasibilityResult::Empty`].
fn dfs_leaf_feasible(
    current: &[String],
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
) -> bool {
    let leaf_label = current.join(",");
    let placeholder = vec![leaf_label];
    let feasibility = filter_feasible_candidates(
        &placeholder,
        parameterized_template,
        constraint_checker,
        functions,
    );
    matches!(feasibility, FeasibilityResult::Feasible { .. })
}

/// Recursive DFS over the cross-product of per-param Phase A candidate vectors.
///
/// Visits leaves in declared-order × lexicographic-within-param order: at
/// `level`, iterates `per_param_candidates[level]` in the order Phase A
/// produced (alphabetical FQN), pushes the candidate onto `current`,
/// recurses, then pops. At the leaf (`level == per_param_candidates.len()`),
/// calls [`dfs_leaf_feasible`]; if feasible, pushes the leaf into
/// `feasible_assignments`.
///
/// Returns `true` when the caller should early-terminate (unwind out of the
/// recursion immediately). The leaf branch returns `stop_after_first_feasible`
/// when it records a feasible leaf. With `stop_after_first_feasible = true`
/// (free-mode in step-20), the first feasible leaf halts the search; with
/// `stop_after_first_feasible = false` (strict-mode in step-24), the search
/// continues past the first feasible leaf so the caller can detect ≥2
/// (Ambiguous).
fn dfs_search(
    level: usize,
    per_param_candidates: &[Vec<String>],
    current: &mut Vec<String>,
    feasible_assignments: &mut Vec<Vec<String>>,
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    stop_after_first_feasible: bool,
) -> bool {
    if level == per_param_candidates.len() {
        if dfs_leaf_feasible(current, parameterized_template, constraint_checker, functions) {
            feasible_assignments.push(current.clone());
            return stop_after_first_feasible;
        }
        return false;
    }
    for candidate in &per_param_candidates[level] {
        current.push(candidate.clone());
        let early = dfs_search(
            level + 1,
            per_param_candidates,
            current,
            feasible_assignments,
            parameterized_template,
            constraint_checker,
            functions,
            stop_after_first_feasible,
        );
        current.pop();
        if early {
            return true;
        }
    }
    false
}
