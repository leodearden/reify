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
//! All three phases are delivered as **pure utility modules**, now wired into
//! the compile pipeline end-to-end. The parser surface is landed —
//! `auto_type_arg` is admitted inside `type_arg_list` via
//! `tree-sitter-reify/grammar.js:710-729` (corpus pin:
//! `tree-sitter-reify/test/corpus/auto_type_arg.txt`), and AST lowering routes
//! `auto_type_arg` CST nodes to `TypeExprKind::Auto { free, bound }` via
//! `crates/reify-syntax/src/ts_parser.rs:679-720`. The **compile-pipeline
//! resolver call-site** landed in task 3558:
//! `compile_builder::auto_type_param_phase::phase_auto_type_param_resolution`
//! (invoked from `crates/reify-compiler/src/lib.rs::compile_with_prelude_context`
//! between `phase_entities` and the bound-check pass) drains each
//! `sub x = Foo<auto: Bound>()` use-site, dispatches
//! [`resolve_auto_type_params_with_backtracking`], and writes the result into
//! `CompiledModule.auto_type_substitution` (which stays at its empty default
//! when the module declares no `auto:` type-args). End-to-end source-level
//! resolution is therefore live. Functions in this module remain unit-tested
//! against compiler-built registries in addition to the integration coverage
//! in `crates/reify-compiler/tests/auto_type_arg_lowering_tests.rs`.
//!
//! Phase D (topology trigger / re-resolution on registry change) is
//! explicitly deferred to a follow-up task.
//!
//! # Phase E (v0.2) — Backtracking
//!
//! [`resolve_auto_type_params_with_backtracking`] extends the v0.1 per-param
//! BFS orchestrator above into a depth-first search over the cross-product of
//! `auto:` candidate sets, with constraint feasibility re-checked at each leaf
//! assignment per the design decision "implement v0.2 search with full re-check
//! at each binding". Bounded by a configurable `max_depth` (default 6, sourced
//! from `reify.toml` `[auto_type_params]`); above the bound, the search falls
//! back to v0.1 BFS with a [`AutoTypeParamDepthBoundExceeded`] warning for
//! auditability.
//!
//! Driving PRD: `docs/prds/v0_2/auto-resolution-backtracking.md`. The section
//! header comment immediately above the function delineates the algorithm's
//! scope and deferrals to sibling task 2664 (BFS-failure test suite). Tasks
//! 2660 (backjumping via the "rejected because" channel), 2661 (`auto(free)`
//! cross-product NonUnique enumeration), 2662 (100k cap), and 2663 (rich
//! diagnostic format with first-param prefix illustration + free-mode
//! collection cap tightening) now land in this module.
//!
//! **Backjumping (task 2660):** `build_constraint_blame_map` builds a static
//! `HashMap<ConstraintNodeId, BTreeSet<usize>>` mapping each constraint to the
//! `params`-slice indices it references through `ValueRef(cell_id)` nodes typed
//! as `Type::TypeParam(name)`. At each infeasible leaf, `compute_deepest_blame_level`
//! takes `max` over the union of violated constraints' blame sets to derive
//! backjump target `J` — the deepest blamed param level. The `DfsControl::BackjumpTo(J)`
//! arm unwinds the recursion to level `J`, where the sibling loop resumes.
//! Constraints with no `TypeParam` references are absent from the map ("absent
//! ↔ no blame ↔ ordinary backtrack") — preserving 2659/2661 test outcomes when
//! the deferred type-substitution mechanics are not yet in place.
//!
//! Backjumping (task 2660) consumes the violated-constraint channel from the
//! leaf check via the static blame map built by `build_constraint_blame_map`.
//!
//! [`AutoTypeParamDepthBoundExceeded`]: reify_types::DiagnosticCode::AutoTypeParamDepthBoundExceeded

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type};
use reify_ir::{CompiledExprKind, CompiledFunction, ConstraintChecker, ConstraintInput};

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

/// Result returned by both auto-type-param orchestrators:
/// [`resolve_auto_type_params`] (v0.1 BFS, since task 2310) and
/// [`resolve_auto_type_params_with_backtracking`] (v0.2 DFS, since task 2659).
///
/// ## `per_param` shape — BFS orchestrator
///
/// One entry per *processed* param, in declared order. Each entry is
/// `(param_name, SelectionResult)`. All params up to **and including** the
/// first failure are recorded; params *after* the first failure are NOT
/// recorded (halt-on-first-failure rule). On a full success, all N params
/// are recorded. Length ranges from 1 (first-param failure) to N.
///
/// ## `per_param` shape — DFS orchestrator
///
/// The DFS orchestrator's Phase A enumeration loop is **hoisted** out of the
/// recursion (all params are enumerated up front before any cross-product leaf
/// is visited). This changes the shape in two ways relative to BFS:
///
/// - **Phase A halt arm (Empty or Overflow on any param)**: `per_param` has
///   length **1**, containing only the failing param's entry
///   — `(name, NoCandidate)` for Empty, `(name, Ambiguous(overflow_vec))` for
///   Overflow. Prior params that passed Phase A are NOT included: Phase B/C
///   selection has not yet occurred at the moment the loop short-circuits, so
///   there are no `Selected` entries to accumulate.
///
/// - **Cross-product Ambiguous arm (≥2 feasible cross-product assignments
///   under strict mode)**: `per_param` has length **1**, anchored on
///   `params[0]`: `[(params[0].name, Ambiguous(witnesses))]`. The Ambiguous
///   outcome is collective across the cross-product — it is not attributable
///   to any single param's Phase B/C selection — so the anchor follows the
///   same convention as the diagnostic label (anchored on `params[0]`'s
///   `use_site_span`).
///
/// - **Cross-product Selected paths** (single feasible assignment, single-param
///   case, or all-success): `per_param` has length N, with each entry
///   `(p.name, Selected(template_name))` in declared order — matches BFS's
///   success shape.
///
/// - **Cross-product all-free NonUnique arm (≥2 feasible cross-product
///   assignments when every param has `free=true`)**: `per_param` has length
///   N — the same success shape as the single-feasible (`1 =>`) arm — with
///   each entry `(p.name, Selected(lex_first_candidate))`. The lex-first
///   feasible leaf (`feasible_assignments[0]`, DFS visit order) is selected.
///   One `AutoTypeParamNonUnique` (Warning) diagnostic is attached alongside
///   the success outcome. Contrast with the strict-mode Ambiguous arm (length
///   1, `SelectionResult::Ambiguous`) which fires when any param has
///   `free=false`. (Task 2661.)
///
/// ## Depth-bound discontinuity
///
/// When `params.len() > max_depth`, `resolve_auto_type_params_with_backtracking`
/// delegates to BFS and the result has the **BFS shape** described above. This
/// means that for the same Phase A failure fixture, the `per_param` shape FLIPS
/// at the boundary:
///
/// - `n = max_depth` (DFS path): `[(failing_param, NoCandidate)]` — length 1.
/// - `n = max_depth + 1` (BFS-fallback path): `[(prior_param, Selected), (failing_param, NoCandidate)]` — length 2.
///
/// This discontinuity is intentional. It is pinned by the test
/// `dfs_phase_a_failure_at_depth_bound_boundary_documents_per_param_shape_discontinuity`
/// in `tests/auto_type_param_backtracking_tests.rs`; callers that destructure
/// `per_param` and need to handle both paths should grep for that test name to
/// find the canonical explanation.
///
/// ## `substitution`
///
/// Only the *successfully resolved* params, in declared order. Each entry is
/// `(param_name, template_name)`. A param appears here iff its
/// `SelectionResult` was `Selected`. On any failure path, only the params that
/// had already been `Selected` before the halt are included.
///
/// ## Asymmetry
///
/// The asymmetry between `per_param` and `substitution` is intentional and
/// load-bearing: `per_param` carries every recorded outcome (including the
/// first failure), while `substitution` carries only the successful
/// substitutions. Tests assert both lengths to pin declared-order halt
/// semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiParamResolutionOutcome {
    /// Per-param outcomes in declared order, stopping at or before the first
    /// failure. The exact shape depends on which orchestrator produced this
    /// value; see the struct-level doc-comment for the BFS shape, the DFS
    /// Phase A halt shape, and the DFS cross-product Ambiguous shape.
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

/// Maximum number of composite witness strings rendered in an
/// `AutoTypeParamNonUnique` (all-free NonUnique) diagnostic message.
///
/// Governs only the all-free NonUnique display path in
/// `resolve_auto_type_params_with_backtracking`. The free-mode collection cap
/// (task 2663 Scope 2) stops DFS enumeration at `NON_UNIQUE_DISPLAY_CAP + 1`
/// feasibles, so the exact total past the cap is unknown by design. The
/// elision suffix wording reflects that:
/// - When `total <= NON_UNIQUE_DISPLAY_CAP`: every collected feasible is
///   rendered and there is **no** elision suffix.
/// - When `total == NON_UNIQUE_DISPLAY_CAP + 1` (collection cap hit, exact
///   total unknown): the message gets a coarse
///   `"(more than NON_UNIQUE_DISPLAY_CAP feasibles exist; rest elided)"`
///   suffix — we know at least one feasible was elided from the collected
///   set, plus an unknown number were never collected.
pub const NON_UNIQUE_DISPLAY_CAP: usize = 16;

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

/// Emit an [`AutoTypeParamNoCandidate`] diagnostic for the v0.2 cross-product
/// `0 =>` arm: the DFS exhausted every cross-product leaf with no feasibles.
///
/// This emits the **rich multi-param message form** documented in the
/// `AutoTypeParamNoCandidate` doc-comment (parallel to `AutoTypeParamAmbiguous`'s
/// multi-param shape). The message reports:
/// - parameter names in declared order (`[T, U, …]`)
/// - per-parameter candidate counts (`T=N, U=M, …`)
/// - cross-product size (`N × M × …`)
/// - depth context (`depth: n (max_depth = m)`)
/// - first-param prefix illustration — see "Prefix illustration" below.
///
/// # Prefix illustration (NOT conflict localization)
///
/// Backjumping (task 2660) guarantees that when DFS exits with
/// `feasible_assignments.is_empty()`, the entire cross-product is infeasible
/// (every skipped sub-tree shares the violated constraints with the leaf that
/// triggered the backjump). Therefore EVERY level-1 prefix has an
/// all-infeasible descendant sub-tree, and **no specific level-1 prefix is
/// "the cause"**. The orchestrator does not inspect rejected leaves for the
/// violated constraint(s) responsible for a particular prefix — that
/// localization work is intentionally deferred (see `AutoTypeParamNoCandidate`
/// doc-comment).
///
/// What this helper *does* render is a **fixed-shape illustration**: the
/// lex-first level-1 prefix used as a concrete labeling anchor so the user
/// can see one (param, FQN) pairing alongside the sub-tree size. It is NOT a
/// conflict diagnosis. The rendered prefix is identical for every fully-
/// infeasible cross-product whose first parameter has the same lex-first
/// candidate, regardless of which constraint actually failed.
///
/// Concretely, the illustration is:
///
/// ```text
/// (params[0].name, per_param_candidates[0][0])
/// ```
///
/// with sub-tree size `cross_product_size / per_param_candidates[0].len()`.
/// This is purely derivable from the orchestrator's existing inputs; no
/// extension to `dfs_search` is needed. The message wording explicitly notes
/// that the entire cross-product is infeasible and no specific conflict was
/// localized so users understand the illustration is not a help signal.
///
/// # Label and candidates conventions
///
/// - Label anchors on `params[0].use_site_span` (same convention as v0.1 BFS
///   strict-Ambiguous and the post-2659 cross-product Ambiguous — every
///   auto-type-param multi-param diagnostic anchors on the first param).
/// - `Diagnostic::candidates` carries the prefix illustration's FQN list in
///   declared parameter order (length 1 for level-1 prefix — the FQN-only
///   invariant pinned in
///   `crates/reify-types/src/diagnostics.rs::Diagnostic::candidates` is
///   preserved; the `T=fqn` rendering with param-name pairing lives in the
///   human-readable message only).
///
/// [`AutoTypeParamNoCandidate`]: reify_types::DiagnosticCode::AutoTypeParamNoCandidate
fn emit_no_feasible_cross_product_diagnostic(
    params: &[AutoTypeParam],
    per_param_candidates: &[Vec<String>],
    cross_product_size: usize,
    max_depth: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    let per_param_counts: Vec<String> = params
        .iter()
        .zip(per_param_candidates.iter())
        .map(|(p, candidates)| format!("{}={}", p.name, candidates.len()))
        .collect();
    let depth = params.len();

    // First-param prefix illustration: the lex-first level-1 prefix.
    //
    // This is NOT conflict localization. Backjumping (task 2660) guarantees
    // that when DFS exits with zero feasibles, the entire cross-product is
    // infeasible — EVERY level-1 prefix has an all-infeasible descendant
    // sub-tree, and no specific prefix is "the cause". The compiler does not
    // inspect rejected leaves for the violated constraint(s); the rendered
    // prefix is identical for every fully-infeasible cross-product whose
    // first parameter has the same lex-first candidate, regardless of which
    // constraint actually failed.
    //
    // What this code computes is a fixed-shape labeling anchor: the lex-first
    // level-1 prefix `(params[0].name, per_param_candidates[0][0])` with
    // sub-tree size `cross_product_size / per_param_candidates[0].len()`.
    // The message wording explicitly notes that the entire cross-product is
    // infeasible and no specific conflict was localized so users do not
    // mistake this illustration for help-channel output.
    //
    // (This branch is only entered when params.len() >= 2 — the single-param
    // case short-circuits via the `params.len() == 1` branch in
    // `resolve_auto_type_params_with_backtracking` and never reaches the
    // `0 =>` arm. Both per_param_candidates and per_param_candidates[0] are
    // therefore non-empty.)
    let prefix_param_name = &params[0].name;
    let prefix_fqn = &per_param_candidates[0][0];
    let prefix_subtree_size = cross_product_size / per_param_candidates[0].len();

    // `Diagnostic::candidates` carries the prefix illustration's FQN list in
    // declared parameter order. The level-1 prefix produces a length-1 list
    // with the lex-first FQN. The FQN-only invariant pinned at
    // `crates/reify-types/src/diagnostics.rs::Diagnostic::candidates`
    // (lines 884-903) is preserved: the bare FQN goes through the structured
    // field; the human-readable `T=fqn` rendering with param-name pairing
    // lives in the message only. Mirrors the
    // `AutoTypeParamAmbiguous` multi-param coherent-assignment convention
    // (`diagnostics.rs:510-521`).
    let prefix_fqns = vec![prefix_fqn.clone()];

    let (_joined_bounds, label_message) = render_auto_type_param_label(&params[0].bounds);
    let message = format!(
        "auto type-parameter cross-product search found no feasible assignment for parameters [{names}]: \
         candidates per parameter: {counts}; \
         cross-product size: {size}; \
         depth: {depth} (max_depth = {max_depth}); \
         first-param prefix illustration: {prefix_param_name}={prefix_fqn} \
         (lex-first level-1 prefix; sub-tree size {prefix_subtree_size}; \
         entire cross-product is infeasible — no specific conflict localized)",
        names = param_names.join(", "),
        counts = per_param_counts.join(", "),
        size = cross_product_size,
    );

    // Single label anchored on `params[0].use_site_span` — the
    // **first-param anchoring convention** shared by every multi-param
    // auto-type-param diagnostic (v0.1 BFS strict-Ambiguous, post-2659
    // cross-product Ambiguous, this v0.2 cross-product no-feasible). The
    // label-message string is rendered via `render_auto_type_param_label`
    // (single source of truth for diagnostic-label wording across all
    // emission sites). Pinned by
    // `dfs_zero_feasible_diagnostic_anchored_on_first_param_use_site_span`.
    diagnostics.push(
        Diagnostic::error(message)
            .with_code(DiagnosticCode::AutoTypeParamNoCandidate)
            .with_label(DiagnosticLabel::new(params[0].use_site_span, label_message))
            .with_candidates(prefix_fqns),
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
/// Public backward-compat wrapper around [`filter_feasible_candidates_seeded`].
///
/// External callers that do not have access to a `template_registry` (or
/// operate without per-candidate ValueMap seeding — e.g. the
/// `reify-eval` determinism test that calls this directly) should use this
/// function. Internal callers should prefer [`filter_feasible_candidates_seeded`]
/// so that per-candidate literal defaults are seeded into the `check()` call's
/// `ValueMap` (hoist reversion — task 4434 γ, re-homed from task 3637).
pub fn filter_feasible_candidates(
    candidates: &[String],
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
) -> FeasibilityResult {
    // Empty registry + empty param_name → no seeding → empty ValueMap per
    // candidate (identical to the pre-reversion behaviour).
    let empty_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
    filter_feasible_candidates_seeded(
        candidates,
        parameterized_template,
        &empty_registry,
        "",
        constraint_checker,
        functions,
    )
}

/// Per-candidate feasibility filter with ValueMap seeding (hoist reversion —
/// task 4434 γ, re-homed from task 3637).
///
/// Identical to [`filter_feasible_candidates`] except that `build_constraints_template`
/// and per-candidate `ValueMap` setup now live **inside** the per-candidate loop,
/// matching the `NOTE(substitution-pass-trigger)` reversion intent.
///
/// For each candidate, the `checker.check()` call receives a `ValueMap` seeded
/// from that candidate's literal defaults via `seed_candidate_value_map`.  The
/// key is derived from `param_type_member(parameterized_template, param_name)`,
/// so constraints of the form `<param_member>.<field> < K` can resolve
/// `<param_member>.<field>` from the seeded map.
///
/// When `template_registry` does not contain a candidate's template (or
/// `param_type_member` returns `None` for `param_name`), the per-candidate
/// ValueMap falls back to empty — identical to the pre-reversion behaviour.
/// This preserves stub-path no-op semantics for callers using
/// `CompileTimeIndeterminateChecker` (PRD §11.2).
///
/// # Note for external callers
///
/// This function is `pub(crate)`.  External callers (e.g. `reify-eval` tests
/// that exercise Phase B in isolation) should use the public
/// [`filter_feasible_candidates`] wrapper (which passes an empty registry and
/// produces the pre-reversion empty-ValueMap behaviour).
///
/// Audit: docs/architecture-audit/findings/auto-resolution-backtracking.md
/// M-005/M-013 — resolved by this reversion (task 4434 γ).
pub(crate) fn filter_feasible_candidates_seeded(
    candidates: &[String],
    parameterized_template: &TopologyTemplate,
    template_registry: &HashMap<String, &TopologyTemplate>,
    param_name: &str,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
) -> FeasibilityResult {
    use reify_ir::Satisfaction;

    debug_assert!(
        !candidates.is_empty(),
        "filter_feasible_candidates_seeded: candidates slice must be non-empty (Phase A's Found arm guarantees ≥1 candidate)"
    );

    let mut accepted: Vec<String> = Vec::new();
    let mut rejected: Vec<RejectedCandidate> = Vec::new();

    // `build_constraints_template` is hoisted out of the loop: today
    // `template.constraints` is byte-identical across all candidates, so
    // cloning ConstraintNodeId strings once is sufficient.
    // NOTE(substitution-pass-trigger): once `Type::TypeParam(T) →
    // Type::StructureRef(candidate)` substitution lands, the constraints
    // expression graph will specialize per-candidate and this build must move
    // inside the loop.  The per-candidate ValueMap seeding DOES belong inside.
    let constraints_template = build_constraints_template(parameterized_template);

    for candidate in candidates {
        let candidate_values =
            if let Some(param_member) = param_type_member(parameterized_template, param_name) {
                if let Some(&candidate_template) = template_registry.get(candidate.as_str()) {
                    seed_candidate_value_map(candidate_template, param_member)
                } else {
                    reify_ir::ValueMap::new()
                }
            } else {
                reify_ir::ValueMap::new()
            };

        let input = ConstraintInput {
            constraints: Cow::Borrowed(&constraints_template),
            values: &candidate_values,
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

// ─── β helper: per-candidate ValueMap setup (PRD §5.2) ──────────────────────

/// Build a per-candidate [`reify_ir::ValueMap`] seeded with the candidate
/// template's **direct literal defaults**, keyed under the supplied
/// `param_member` name.
///
/// # Purpose
///
/// This is the per-candidate ValueMap setup primitive for PRD §5.2 β.  When
/// a parameterized template declares `param seal : T` and a constraint reads
/// `seal.thickness`, the constraint checker needs a `ValueMap` entry for
/// `seal.thickness` populated with the candidate's resolved literal default so
/// the checker can evaluate the constraint against a concrete value instead of
/// `Undef`.
///
/// # Seeding rule (one-level, literal-only)
///
/// For each `ValueCellDecl` in `candidate_template.value_cells`:
/// - If `default_expr` is `Some(expr)` **and** `expr.kind` is
///   `CompiledExprKind::Literal(v)`, insert
///   `(ValueCellId::new(param_member, cell.id.member), v.clone())`.
/// - Otherwise (no default, or a non-literal / computed default) skip the
///   cell — it is left `Undef` in the caller's ValueMap.
///
/// Only direct literal constants are extracted.  Nested member chains and
/// computed defaults (e.g. expressions involving other cells) are deferred to
/// PRD §14.3; `reify-compiler` cannot run the evaluator, so non-literal
/// defaults cannot be reduced to a `Value` at this layer.
///
/// # Key convention
///
/// Keys use `ValueCellId::new(param_member, field)` so that a constraint
/// `seal.thickness < bore_radius` in the parameterized template resolves
/// against the seeded `seal.thickness` entry.  The `param_member` argument
/// is the **name of the `auto:` type-parameter's value cell** in the
/// parameterized template (e.g. `"seal"` for `param seal : T`), not the
/// candidate structure's own name.
///
/// # γ handoff
///
/// This helper is `pub` and unwired by design.  γ wires it into the DFS
/// per-leaf / Phase-B loop — specifically at the three
/// `NOTE(substitution-pass-trigger)` sites in this file — as part of the
/// same code region as γ's BFS-soundness joint-recheck.  β deliberately
/// does NOT touch those sites so γ can build on β sequentially without
/// collision.
pub fn seed_candidate_value_map(
    candidate_template: &crate::types::TopologyTemplate,
    param_member: &str,
) -> reify_ir::ValueMap {
    use reify_core::ValueCellId;
    use reify_ir::{CompiledExprKind, ValueMap};

    let mut map = ValueMap::new();
    for cell in &candidate_template.value_cells {
        if let Some(expr) = &cell.default_expr
            && let CompiledExprKind::Literal(v) = &expr.kind
        {
            map.insert(ValueCellId::new(param_member, &cell.id.member), v.clone());
        }
    }
    map
}

// ─── δ: candidate constructibility check + zero-arg ctor synthesis ──────────

/// The result of [`check_candidate_constructible`].
///
/// Two arms mirror the two downstream actions in the monomorph-build pass:
/// - [`CtorSynthesisResult::Ctor`] — all Param cells have `default_expr`; returns
///   the synthesized zero-arg [`reify_ir::CompiledExpr`].
/// - [`CtorSynthesisResult::NotConstructible`] — at least one Param cell has
///   `default_expr = None`; carries the name of the first such required param so
///   the caller can name it in the diagnostic.
pub(crate) enum CtorSynthesisResult {
    /// All Param cells are defaulted; returns the zero-arg StructureInstanceCtor.
    Ctor(reify_ir::CompiledExpr),
    /// At least one Param cell is required (no default). The String names the
    /// first such required param (for inclusion in the diagnostic message).
    NotConstructible(String),
}

/// Check whether `candidate` is zero-arg constructible (every `Param` cell has
/// a `default_expr`) and, if so, synthesize a `CompiledExpr::structure_instance_ctor`
/// for it.
///
/// # Constructibility rule
///
/// A candidate is constructible iff every `ValueCellKind::Param` cell in its
/// `value_cells` carries a `default_expr`.  A required param (one whose
/// `default_expr` is `None`) cannot be filled by the synthesized zero-arg ctor,
/// so synthesizing the ctor would produce a silent `Undef` field — a fake-
/// completion trap (design decision 2 in the plan).
///
/// # Synthesized ctor shape
///
/// Mirrors the canonical lowering in `crates/reify-compiler/src/expr.rs:1629-1678`:
/// - `type_id  = StructureTypeId(0)` — ephemeral placeholder, stable at compile time.
/// - `type_name = candidate.name.clone()`.
/// - `version  = candidate.version()` — from the `@version(N)` annotation, or 1.
/// - `ordered_args = []` — zero-arg: the caller supplies no positional args.
/// - `defaults` = every `ValueCellKind::Param` cell with a `default_expr`.
/// - `lets`    = every `ValueCellKind::Let` cell (excluding `__count_`-prefixed)
///   with a `default_expr`.  Mirrors the `__count_` exclusion comment in
///   `expr.rs:1659`: those are compiler-internal collection-count cells whose
///   RHS may reference sub-component values unavailable at ctor time.
/// - `result_type = Type::StructureRef(candidate.name.clone())`.
///
/// Used by `auto_type_param_phase.rs` in the monomorph-build pass to fill the
/// `default_expr` of type-param cells in the monomorphized clone.
pub(crate) fn check_candidate_constructible(
    candidate: &TopologyTemplate,
) -> CtorSynthesisResult {
    use reify_core::Type;
    use reify_ir::{CompiledExpr, StructureTypeId};

    use crate::types::ValueCellKind;

    // Constructibility check: the first required Param is the blocking param.
    let first_required = candidate
        .value_cells
        .iter()
        .find(|cell| cell.kind == ValueCellKind::Param && cell.default_expr.is_none());

    if let Some(required) = first_required {
        return CtorSynthesisResult::NotConstructible(required.id.member.clone());
    }

    // All Params are defaulted — synthesize the zero-arg ctor.
    // Collect defaults: every Param cell (they all have default_expr at this point).
    let defaults: Vec<(String, CompiledExpr)> = candidate
        .value_cells
        .iter()
        .filter(|cell| cell.kind == ValueCellKind::Param)
        .filter_map(|cell| {
            cell.default_expr
                .as_ref()
                .map(|e| (cell.id.member.clone(), e.clone()))
        })
        .collect();

    // Collect lets: non-`__count_`-prefixed Let cells with a default_expr.
    // Mirrors the canonical lowering at expr.rs:1654-1669.
    let lets: Vec<(String, CompiledExpr)> = candidate
        .value_cells
        .iter()
        .filter(|cell| {
            cell.kind == ValueCellKind::Let && !cell.id.member.starts_with("__count_")
        })
        .filter_map(|cell| {
            cell.default_expr
                .as_ref()
                .map(|e| (cell.id.member.clone(), e.clone()))
        })
        .collect();

    CtorSynthesisResult::Ctor(CompiledExpr::structure_instance_ctor(
        StructureTypeId(0),
        candidate.name.clone(),
        candidate.version(),
        vec![], // zero ordered_args
        defaults,
        lets,
        Type::StructureRef(candidate.name.clone()),
    ))
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
                let rejected_names: Vec<String> = rejected.iter().map(|r| r.name.clone()).collect();
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
                // Phase B: feasibility filter (hoist-reversion wiring — task 4434 γ).
                // Use `filter_feasible_candidates_seeded` so per-candidate ValueMaps
                // are seeded from the candidate template's literal defaults.
                let feasibility = filter_feasible_candidates_seeded(
                    &candidates,
                    parameterized_template,
                    template_registry,
                    &param.name,
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

        // Asymmetry contract: `per_param` accumulates EVERY
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
                // Selected path: record in BOTH substitution
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
// Canonical algorithm description, design decisions, and the out-of-scope
// task list live on `resolve_auto_type_params_with_backtracking`'s rustdoc
// below. The module-level rustdoc (top of file, "Phase E (v0.2) —
// Backtracking" section) carries a one-line pointer to that function.

/// Find the value-cell member in `parameterized_template` that holds the given
/// `type_param_name` (the cell whose `cell_type == Type::TypeParam(type_param_name)`).
///
/// Returns the `member` component of that cell's `ValueCellId`, or `None` if no
/// such cell exists (e.g. the type-param has no value-cell in the template, or the
/// template carries no value cells at all).
///
/// Used by the joint-recheck in `emit_fallback_warning_and_delegate_to_bfs` and
/// by the hoist-reversion NOTE sites (task 4434 γ, task 3637 re-homed) to derive
/// `param_member` for `seed_candidate_value_map`.
fn param_type_member<'t>(
    parameterized_template: &'t TopologyTemplate,
    type_param_name: &str,
) -> Option<&'t str> {
    parameterized_template
        .value_cells
        .iter()
        .find(|cell| matches!(&cell.cell_type, Type::TypeParam(n) if n == type_param_name))
        .map(|cell| cell.id.member.as_str())
}

/// Push a `Severity::Warning` diagnostic with the given `code` + `message`
/// (anchored on `params[0].use_site_span` with a label rendered by
/// `render_auto_type_param_label`) and delegate to v0.1 BFS
/// (`resolve_auto_type_params`). Used by the depth-bound and cross-product-cap
/// guard branches in `resolve_auto_type_params_with_backtracking` to emit a
/// "search-space-too-large" warning and delegate back to BFS.
///
/// γ (task 4434) — PRD §6.2 joint-recheck: BFS is run **first**; only if the
/// resulting assignment is jointly feasible (or incomplete) is the Warning
/// emitted. If BFS returns a COMPLETE assignment that is jointly INFEASIBLE
/// under the full seeded ValueMap, the Warning is suppressed and a hard
/// `AutoTypeParamBoundedInfeasible` Error is emitted instead — the function
/// returns an outcome with an **empty substitution**.
///
/// Centralizes the shared invariants of the two fallback branches (label
/// anchor, severity, BFS tail-call arg list) so that adding a new guard or
/// changing the fallback shape is a one-line edit. The per-branch message
/// content (depth-bound vs cap) and `DiagnosticCode` choice remain caller
/// concerns — the helper takes them as arguments rather than baking either
/// branch's wording into shared code.
#[allow(clippy::too_many_arguments)]
fn emit_fallback_warning_and_delegate_to_bfs(
    code: DiagnosticCode,
    message: String,
    params: &[AutoTypeParam],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> MultiParamResolutionOutcome {
    // γ PRD §6.2: run BFS first so the joint recheck can inspect the full
    // assignment before deciding Warning vs hard Error.
    let outcome = resolve_auto_type_params(
        params,
        template_registry,
        trait_registry,
        parameterized_template,
        constraint_checker,
        functions,
        diagnostics,
    );

    // Joint-recheck: only when BFS returned a COMPLETE assignment (one entry
    // per param).  BFS-incomplete outcomes (NoCandidate/Ambiguous) already
    // carry their own diagnostics and there is no assignment to recheck.
    if outcome.substitution.len() == params.len() {
        // Build the full-A ValueMap by merging one per-param seed.
        // `seed_candidate_value_map(candidate_template, param_member)` keys
        // entries under `param_member.field`, matching the parameterized
        // template's constraint expressions that reference `<param_member>.<field>`.
        let mut full_value_map = reify_ir::ValueMap::new();
        for (param_name, candidate_name) in &outcome.substitution {
            if let Some(param_member) = param_type_member(parameterized_template, param_name)
                && let Some(&candidate_template) =
                    template_registry.get(candidate_name.as_str())
            {
                let seed = seed_candidate_value_map(candidate_template, param_member);
                for (k, v) in seed.iter() {
                    // Distinct TypeParam names map to distinct value cells today
                    // (invariant: each param's member differs).  A collision here
                    // would silently overwrite an already-seeded key and produce a
                    // wrong joint ValueMap.
                    debug_assert!(
                        !full_value_map.contains(k),
                        "joint-recheck merge: key {k:?} already present; \
                         two type-params resolved to the same value-cell member — \
                         distinct-member invariant violated",
                    );
                    full_value_map.insert(k.clone(), v.clone());
                }
            }
        }

        // Single joint check (O(1), one `checker.check()` call).
        let constraints_template = build_constraints_template(parameterized_template);
        let verdict = check_constraints_leaf(
            &constraints_template,
            constraint_checker,
            functions,
            &full_value_map,
        );

        if !verdict.feasible {
            // BFS assignment is jointly infeasible (Violated constraint(s)
            // found in the full-A ValueMap).  Emit `AutoTypeParamBoundedInfeasible`
            // Error INSTEAD of the Warning and return empty substitution — PRD
            // §6.2 step 4: "Produce NO substitution for the declaration."
            let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
            let bound_clause = if code == DiagnosticCode::AutoTypeParamDepthBoundExceeded {
                "depth bound exceeded"
            } else {
                "cross-product cap exceeded"
            };
            let assignment_str: Vec<String> = outcome
                .substitution
                .iter()
                .map(|(p, c)| format!("{p}={c}"))
                .collect();
            let violated_str: Vec<String> = verdict
                .violated_constraints
                .iter()
                .map(|id| id.to_string())
                .collect();
            let err_message = format!(
                "auto type-parameter BFS fallback assignment is jointly infeasible: \
                 parameters [{names}] {bound_clause}; \
                 BFS assignment [{assignment}] violates constraint(s) [{violated}] \
                 under joint check. No substitution produced.",
                names = param_names.join(", "),
                bound_clause = bound_clause,
                assignment = assignment_str.join(", "),
                violated = violated_str.join(", "),
            );
            let (_, label_message) = render_auto_type_param_label(&params[0].bounds);
            diagnostics.push(
                Diagnostic::error(err_message)
                    .with_code(DiagnosticCode::AutoTypeParamBoundedInfeasible)
                    .with_label(DiagnosticLabel::new(params[0].use_site_span, label_message)),
            );
            return MultiParamResolutionOutcome {
                per_param: outcome.per_param,
                substitution: vec![],
            };
        }
    }

    // Feasible (or BFS incomplete) → push the existing depth-bound/cap Warning
    // and return the BFS outcome unchanged.  This is the stub-path no-op
    // guarantee of PRD §11.2: `CompileTimeIndeterminateChecker` never returns
    // Violated, so γ is transparent on the production compile path.
    let (_, label_message) = render_auto_type_param_label(&params[0].bounds);
    diagnostics.push(
        Diagnostic::warning(message)
            .with_code(code)
            .with_label(DiagnosticLabel::new(params[0].use_site_span, label_message)),
    );
    outcome
}

/// DFS over the cross-product of `auto:` candidate sets with a depth bound.
///
/// Driving PRD: `docs/prds/v0_2/auto-resolution-backtracking.md`.
///
/// Extends v0.1's per-param BFS (`resolve_auto_type_params` above) into a
/// depth-first search over the cross-product of `auto:` candidate sets. At
/// each leaf assignment of the cross-product, `filter_feasible_candidates`
/// is re-invoked (full re-check per the PRD design decision "implement v0.2
/// search with full re-check at each binding") to determine feasibility; an
/// infeasible leaf triggers backtracking to the next sibling at the deepest
/// open level.
///
/// Above the depth bound `params.len() > max_depth`, the function emits
/// `AutoTypeParamDepthBoundExceeded` (Severity::Warning) and delegates back
/// to `resolve_auto_type_params` (BFS). Above the cross-product cap
/// `cross_product_size > max_cross_product_size`, the function emits
/// `AutoTypeParamCrossProductSizeExceeded` (Severity::Warning) and
/// delegates back to `resolve_auto_type_params` (BFS). The two guards are
/// independent — depth caps the parameter count, the cap caps the search
/// space *given* the parameter count. Both fallbacks are functionally
/// correct (BFS is sound, just less complete than DFS over cross-product)
/// so the user has a working compile — the warnings are for auditability.
///
/// Both `max_depth` and `max_cross_product_size` are taken as scalars (not a
/// `&AutoTypeParamsConfig`) per the design decision: algorithm correctness
/// does not depend on where the values were sourced, and this keeps the
/// algorithm crate independent of `reify-config`. The eventual call-site
/// reads `Manifest::auto_type_params().max_depth` and
/// `.max_cross_product_size` and passes them in directly.
///
/// # `per_param` shape
///
/// The DFS-specific shapes (Phase A halt, cross-product Ambiguous) and the
/// depth-bound discontinuity are documented on [`MultiParamResolutionOutcome`].
/// Callers that destructure `per_param` should consult that doc-comment for
/// the authoritative per-orchestrator contract.
///
/// # Out of scope (sibling tasks layered on top of this foundation)
///
/// - Comprehensive v0.1 BFS-failure scenario coverage — task 2664.
///
/// Task 2660 (backjumping via the "rejected because" channel), task 2661
/// (`auto(free)` cross-product NonUnique enumeration), task 2662
/// (cross-product hard cap with BFS fallback), and task 2663 (rich
/// search-failure diagnostic format with first-param prefix illustration +
/// free-mode collection cap tightening) now land in this module.
/// - Type-substitution mechanics
///   (`Type::TypeParam(T)` → `Type::StructureRef(candidate)`) — separately
///   deferred per the PRD's "Constraint-feasibility incremental binding
///   deferred" decision.
//
// `#[allow(clippy::too_many_arguments)]`: this signature mirrors v0.1's
// `resolve_auto_type_params` (already at clippy's 7-arg ceiling) plus the
// scalars `max_depth` and `max_cross_product_size` that the algorithm needs
// but cannot derive. Bundling these into a context struct would obscure the
// intentional parallel with the BFS orchestrator's signature; the ambient
// convention across this crate (35+ call sites) is to allow the lint on
// orchestration entry points where the parameter list is the API contract
// itself.
#[allow(clippy::too_many_arguments)]
pub fn resolve_auto_type_params_with_backtracking(
    params: &[AutoTypeParam],
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    parameterized_template: &TopologyTemplate,
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    max_depth: usize,
    max_cross_product_size: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> MultiParamResolutionOutcome {
    // Vacuous success: empty params slice is a valid no-op (parity with
    // v0.1 BFS's `resolve_auto_type_params`). In particular, `0` is never
    // greater than `max_depth` (which `reify-config` already rejects when
    // `max_depth == 0`), so the depth-bound branch does not fire here.
    if params.is_empty() {
        return MultiParamResolutionOutcome {
            per_param: vec![],
            substitution: vec![],
        };
    }

    // Depth-bound guard: above the bound, fall back to v0.1 BFS with a
    // Warning diagnostic. BFS is sound (just less complete than DFS over
    // the cross-product), so the user has a working compile — the warning
    // is for auditability so they know the v0.2 search was bypassed.
    //
    // Canonical message form pinned in the DiagnosticCode doc-comment:
    // see `DiagnosticCode::AutoTypeParamDepthBoundExceeded` in
    // `crates/reify-types/src/diagnostics.rs`.
    //
    // NOTE(substitution-pass-trigger) — RESOLVED by task 4434 γ (re-homed from task 3637):
    // The BFS-fallback soundness hole (BFS picking a per-param-feasible combination that is
    // jointly INFEASIBLE at the cross-product once `Type::TypeParam(T) →
    // Type::StructureRef(candidate)` substitution diverges cell types) is closed by γ's
    // joint-recheck in `emit_fallback_warning_and_delegate_to_bfs`: after BFS returns a
    // complete assignment A, a single `check_constraints_leaf` call with the full-A seeded
    // ValueMap rejects A if Violated and emits `AutoTypeParamBoundedInfeasible` Error with
    // empty substitution.  The BFS branch no longer needs to be revisited for this hazard.
    // Remaining NOTE clause in the format! strings below is kept as a user-visible audit
    // trail but no longer reflects an open soundness concern.
    // Audit: docs/architecture-audit/findings/auto-resolution-backtracking.md M-005/M-006/M-013.

    // strict `>`: params.len()==max_depth still runs DFS; only params.len()>max_depth falls back.
    if params.len() > max_depth {
        let message = format!(
            "auto type-parameter search exceeded depth bound: {n} auto-type-params declared, \
             max_depth = {m}; falling back to per-parameter BFS (v0.1 algorithm). \
             NOTE: BFS-fallback soundness is contingent on Type::TypeParam \u{2192} Type::StructureRef \
             substitution remaining deferred; once the substitution pass lands, this fallback may \
             silently pick wrong substitutions.",
            n = params.len(),
            m = max_depth,
        );
        return emit_fallback_warning_and_delegate_to_bfs(
            DiagnosticCode::AutoTypeParamDepthBoundExceeded,
            message,
            params,
            template_registry,
            trait_registry,
            parameterized_template,
            constraint_checker,
            functions,
            diagnostics,
        );
    }

    // Build the parameterized template's constraint list ONCE here so the
    // DFS leaf-feasibility predicate borrows a single owned `Vec` rather than
    // rebuilding it on every leaf. With max_depth=6 and ~10 candidates per
    // param the worst case is 10^6 leaves; the per-leaf rebuild was a
    // measurable hot-path allocation pin.
    //
    // NOTE(substitution-pass-trigger) — PARTIALLY REVERTED by task 4434 γ (re-homed from
    // task 3637): The `leaf_values` hoist is reverted — per-leaf ValueMaps are now seeded
    // inside `dfs_search`'s leaf branch via `seed_candidate_value_map` (driven by the
    // per-param member mapping computed below).  The `constraints_template` hoist remains
    // sound because `template.constraints` is still unchanged across DFS leaves (the
    // expression graph does not yet specialize per-candidate).  When `Type::TypeParam(T) →
    // Type::StructureRef(candidate)` substitution lands and per-candidate cell types
    // diverge, `build_constraints_template` must also move inside the DFS leaf — at that
    // point this comment becomes the remaining NOTE.
    // Audit: docs/architecture-audit/findings/auto-resolution-backtracking.md M-005/M-013.
    let constraints_template = build_constraints_template(parameterized_template);
    // Pre-compute param→member mapping for `dfs_search`'s per-leaf ValueMap seeding.
    // `param_type_member` scans `parameterized_template.value_cells` for the cell whose
    // `cell_type == Type::TypeParam(param.name)` and returns its member name.  `None`
    // means no value cell carries that type param — the leaf falls back to an empty map
    // for that param (stub-path no-op semantics, PRD §11.2).
    let param_members: Vec<Option<String>> = params
        .iter()
        .map(|p| param_type_member(parameterized_template, &p.name).map(|s| s.to_owned()))
        .collect();

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
                // Mirrors `resolve_auto_type_params`'s identical empty-pool arm
                // (the BFS orchestrator above): both call
                // `emit_no_candidate_zero_rejections` for the zero-rejections
                // message form (bound mentioned, no "rejected by constraint"
                // suffix — that suffix only fires from Phase C's all-rejected
                // path) and produce `per_param=[(name, NoCandidate)]` with
                // empty substitution. The DFS analog returns immediately
                // because no later params have been enumerated yet (Phase A
                // enumeration is up-front), so the outcome's
                // per_param/substitution shape is identical to BFS's by
                // construction.
                emit_no_candidate_zero_rejections(&param.bounds, param.use_site_span, diagnostics);
                return MultiParamResolutionOutcome {
                    per_param: vec![(param.name.clone(), SelectionResult::NoCandidate)],
                    substitution: vec![],
                };
            }
            CandidateEnumeration::Overflow(overflow_vec) => {
                // Phase A already pushed the overflow diagnostic. Mirror
                // BFS's "Overflow → Ambiguous" mapping in `per_param` so the
                // outer-shape contract is identical — see the matching arm
                // in `resolve_auto_type_params` (the v0.1 BFS orchestrator)
                // which produces `SelectionResult::Ambiguous(overflow_vec)`
                // and breaks the per-param loop. The DFS analog returns
                // immediately because no later params have been enumerated
                // yet (Phase A enumeration is up-front), so the outcome's
                // per_param/substitution shape is identical to BFS's by
                // construction. No extra diagnostic emission needed —
                // `enumerate_candidates` already pushed
                // `AutoTypeParamPoolOverflow`.
                return MultiParamResolutionOutcome {
                    per_param: vec![(param.name.clone(), SelectionResult::Ambiguous(overflow_vec))],
                    substitution: vec![],
                };
            }
            CandidateEnumeration::Found(candidates) => {
                per_param_candidates.push(candidates);
            }
        }
    }

    // Cross-product cap guard (task 2662): above the cap, fall back to v0.1
    // BFS with a Warning diagnostic. Fires only after Phase A enumeration
    // completes successfully — Phase A failures (Empty/Overflow) early-return
    // before this branch and preserve the per_param/substitution shape pinned
    // by their respective tests, so the cap check never sees a partial
    // `per_param_candidates`.
    //
    // Saturating fold: `checked_mul` returns `None` on overflow, mapped to
    // `usize::MAX`. With `MAX_AUTO_TYPE_PARAM_CANDIDATES = 10` and
    // `max_depth = 6` the worst-case product is 10^6 = 1,000,000 (well below
    // `usize::MAX` on every supported platform), but a future relaxation of
    // either bound — or pathological per-param counts — could overflow.
    // Saturating to `usize::MAX` ensures the cap check fires deterministically:
    // any saturating result is by definition `> max_cross_product_size`
    // (which `reify-config` validation forces to be a sane finite value).
    //
    // strict `>`: cross_product_size==max_cross_product_size still runs DFS;
    // only cross_product_size>max_cross_product_size falls back. Equal-to-cap
    // is a representable, deterministic search space and there's no
    // algorithmic reason to refuse it.
    //
    // Canonical message form pinned in the DiagnosticCode doc-comment:
    // see `DiagnosticCode::AutoTypeParamCrossProductSizeExceeded` in
    // `crates/reify-types/src/diagnostics.rs`.
    let cross_product_size: usize = per_param_candidates
        .iter()
        .map(|v| v.len())
        .fold(1usize, |acc, n| acc.saturating_mul(n));
    if cross_product_size > max_cross_product_size {
        let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let candidate_counts: Vec<usize> = per_param_candidates.iter().map(|v| v.len()).collect();
        let message = format!(
            "auto type-parameter cross-product search exceeded size cap: \
             {n} auto-type-params declared ({names}) with per-param candidate counts {counts:?} \
             yielding cross-product size {size}, max_cross_product_size = {cap}; \
             falling back to per-parameter BFS (v0.1 algorithm). \
             NOTE: BFS-fallback soundness is contingent on Type::TypeParam \u{2192} Type::StructureRef \
             substitution remaining deferred; once the substitution pass lands, this fallback may \
             silently pick wrong substitutions.",
            n = params.len(),
            names = param_names.join(", "),
            counts = candidate_counts,
            size = cross_product_size,
            cap = max_cross_product_size,
        );
        return emit_fallback_warning_and_delegate_to_bfs(
            DiagnosticCode::AutoTypeParamCrossProductSizeExceeded,
            message,
            params,
            template_registry,
            trait_registry,
            parameterized_template,
            constraint_checker,
            functions,
            diagnostics,
        );
    }

    // Single-param degenerate path: with exactly one param, the cross-product
    // collapses to a flat enumeration of that param's candidates and the
    // recursion is degenerate. Route through the existing Phase B / Phase C
    // helpers verbatim — the result is identical to BFS's single-param
    // pipeline.
    if params.len() == 1 {
        let param = &params[0];
        let candidates = &per_param_candidates[0];
        // Hoist-reversion wiring (task 4434 γ, re-homed from task 3637):
        // use the seeded version so per-candidate ValueMaps are populated.
        let feasibility = filter_feasible_candidates_seeded(
            candidates,
            parameterized_template,
            template_registry,
            &param.name,
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
    // U inner, …).
    //
    // Strict-vs-free dispatch (task 2661): if any param is strict
    // (`free=false`), the search stops as soon as 2 feasibles are collected
    // (early-exit; max_feasible_to_collect=2). Free-mode (`every param free=true`)
    // collects up to `NON_UNIQUE_DISPLAY_CAP + 1` feasibles (task 2663) — one
    // more than the display cap so we can detect "more than the cap" without
    // enumerating the full cross-product. The lex-first pick is
    // `feasible_assignments[0]` (DFS visits in declared-order × lex-within-param
    // order by construction).
    //
    // Cost note: free-mode now visits AT MOST `NON_UNIQUE_DISPLAY_CAP + 1`
    // feasible leaves before early-terminating (task 2663 Scope 2 cap
    // tightening); previously it visited K^N leaves in the worst case.
    // Strict-mode is cheaper in the common case because it terminates
    // as soon as 2 feasibles are found; only in the all-infeasible worst case
    // does strict-mode also visit O(K^N) leaves.
    //
    // Task 2662's cross-product hard cap (`max_cross_product_size`) fires
    // BEFORE this point — see the cap-check branch immediately after Phase A
    // enumeration above. By construction, control reaches this multi-param
    // dispatch only when `cross_product_size <= max_cross_product_size`, so
    // the worst-case leaf count here is bounded by the cap (default 100,000).
    let any_strict = params.iter().any(|p| !p.free);
    let max_feasible_to_collect: usize = if any_strict {
        2
    } else {
        NON_UNIQUE_DISPLAY_CAP + 1
    };

    // Build the static blame map ONCE before recursion. Each constraint's
    // expression tree is walked to find `ValueRef(cell_id)` nodes whose cell
    // is typed `Type::TypeParam(name)`. The map drives backjumping: when an
    // infeasible leaf's violated constraints all blame param J, the search
    // skips the entire sub-tree and resumes at J (Gaschnig backjumping).
    // When the map has no entry for any violated constraint, the leaf returns
    // `DfsControl::Continue` — identical to ordinary backtracking.
    let blame_map = build_constraint_blame_map(parameterized_template, params);

    let mut current: Vec<String> = Vec::with_capacity(params.len());
    let mut feasible_assignments: Vec<Vec<String>> = Vec::new();
    // Return value discarded: at level 0 the DFS has already accumulated every
    // feasible assignment into `feasible_assignments`. `Continue` and
    // `EarlyTerminate` are observationally equivalent for a caller that reads
    // only `feasible_assignments`, and `BackjumpTo(0)` is consumed by the
    // `j == level` arm at level 0 — it cannot escape to this call site.
    let _ = dfs_search(
        0,
        &per_param_candidates,
        &mut current,
        &mut feasible_assignments,
        &constraints_template,
        template_registry,
        &param_members,
        constraint_checker,
        functions,
        max_feasible_to_collect,
        &blame_map,
    );

    match feasible_assignments.len() {
        0 => {
            // Zero feasible cross-product assignments. Emit the v0.2 rich
            // diagnostic (task 2663): parameter list, per-param candidate
            // counts, cross-product size, depth context, and smallest
            // infeasibility witness. See `emit_no_feasible_cross_product_diagnostic`
            // for the witness algorithm and message form.
            emit_no_feasible_cross_product_diagnostic(
                params,
                &per_param_candidates,
                cross_product_size,
                max_depth,
                diagnostics,
            );
            MultiParamResolutionOutcome {
                per_param: vec![(params[0].name.clone(), SelectionResult::NoCandidate)],
                substitution: vec![],
            }
        }
        1 => {
            // Exactly one feasible cross-product assignment. Two paths reach
            // here:
            // - free-mode: DFS visited all leaves but found exactly one
            //   feasible, lex-first by construction.
            // - strict-mode: the search exhausted with a single feasible
            //   leaf — uniquely determined, no Ambiguous needed.
            // Both paths produce a full per_param/substitution Vec mapping
            // each param to its bound candidate, in declared order.
            let first_feasible = feasible_assignments
                .into_iter()
                .next()
                .expect("len==1 by match arm");
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
            MultiParamResolutionOutcome {
                per_param,
                substitution,
            }
        }
        _ => {
            // ≥2 feasible cross-product assignments. Dispatch on `any_strict`:
            // - strict mode (any param free=false): Ambiguous Error (unchanged
            //   from task 2659 — the user requested unique resolution on at
            //   least one slot, so a non-unique cross-product cannot be
            //   auto-resolved).
            // - all-free mode (every param free=true): NonUnique Warning +
            //   lex-first success shape (task 2661). The runtime picks
            //   `feasible_assignments[0]` — the lexicographically-first
            //   feasible leaf by DFS visit order (declared-order ×
            //   lex-within-param) — and emits a Warning for auditability.
            if any_strict {
                // Strict-mode Ambiguous path (task 2659). max_feasible_to_collect=2
                // guarantees exactly 2 entries are present in the strict arm.
                debug_assert!(
                    feasible_assignments.len() >= 2,
                    "strict-mode ≥2 arm: must have collected at least 2 feasibles; \
                     the sibling NonUnique branch (not any_strict) handles the all-free case"
                );
                // Compact per-leaf witness summaries: "T=ORingSeal,U=AirCooled".
                // (Task 2663 added a rich smallest-infeasibility-witness format for
                // the `0 =>` no-feasibles arm via `emit_no_feasible_cross_product_diagnostic`;
                // the ≥2-feasibles strict-Ambiguous arm here continues to render the
                // composite per-leaf witnesses via `render_witnesses`.)
                let witnesses = render_witnesses(params, &feasible_assignments);
                // Diagnostic: emit one AutoTypeParamAmbiguous (Error). The label
                // anchors on params[0].use_site_span — same convention as v0.1
                // BFS strict-Ambiguous on the first-failing param. Mirrors the
                // canonical "consider an explicit substitution like '<lex_first>'
                // instead of 'auto:'" suffix from v0.1's per-param Ambiguous.
                let (_joined_bounds, label_message) =
                    render_auto_type_param_label(&params[0].bounds);
                let witnesses_join = witnesses.join("; ");
                let lex_first_witness = witnesses[0].clone();
                let message = format!(
                    "auto type-parameters have multiple feasible cross-product assignments: {witnesses_join}; consider an explicit substitution like '{lex_first_witness}' instead of 'auto:'",
                );
                // FQN-only invariant: `Diagnostic.candidates` carries bare FQNs (see
                // `crates/reify-types/src/diagnostics.rs::Diagnostic::candidates`).
                // The lex-first feasible cross-product leaf (declared-order ×
                // lex-within-param order; see `dfs_search` doc-comment) supplies the
                // FQN list — the exact substitution suggestion the message body
                // advertises. Composite witness summaries (`T=...,U=...`) remain in
                // the human-readable `message` field only; routing them through
                // `candidates` would violate the contract shared by every other
                // auto-type-param emission site (Phase A overflow, Phase C
                // strict-Ambiguous, Phase C all-rejected) and break LSP `convert.rs`
                // consumers that flatten `data.candidates` as a bare-FQN list.
                // (Task 2860.)
                diagnostics.push(
                    Diagnostic::error(message)
                        .with_code(DiagnosticCode::AutoTypeParamAmbiguous)
                        .with_label(DiagnosticLabel::new(params[0].use_site_span, label_message))
                        .with_candidates(feasible_assignments[0].clone()),
                );
                MultiParamResolutionOutcome {
                    per_param: vec![(
                        params[0].name.clone(),
                        SelectionResult::Ambiguous(witnesses),
                    )],
                    substitution: vec![],
                }
            } else {
                // All-free NonUnique path (task 2661). Every param has free=true,
                // so we collected up to NON_UNIQUE_DISPLAY_CAP + 1 feasible leaves
                // (task 2663 Scope 2 cap tightening — `max_feasible_to_collect`).
                // Emit AutoTypeParamNonUnique (Warning) and return the lex-first
                // feasible as a full length-N success shape — mirroring the `1 =>`
                // arm (single-feasible success) but with an attached Warning.
                //
                // At most NON_UNIQUE_DISPLAY_CAP witness strings are rendered.
                // When the collection cap is hit (i.e. `total > NON_UNIQUE_DISPLAY_CAP`,
                // which by construction means `total == NON_UNIQUE_DISPLAY_CAP + 1`),
                // the exact total past the cap is unknown so the elision message
                // shifts to a coarse
                // "(more than NON_UNIQUE_DISPLAY_CAP feasibles exist; rest elided)"
                // form rather than the prior exact "(N more elided)" form. The
                // wording makes the uncertainty explicit — we know at least one
                // feasible was elided from the collected set, plus an unknown
                // number were never collected.
                // See module-level NON_UNIQUE_DISPLAY_CAP for the rendering invariant.
                let total = feasible_assignments.len();
                let display_count = total.min(NON_UNIQUE_DISPLAY_CAP);

                // Build composite witness strings for the displayed portion only.
                // Format: "T=ORingSeal,U=AirCooled" — mirrors strict-Ambiguous.
                let displayed_witnesses =
                    render_witnesses(params, &feasible_assignments[..display_count]);

                let (_joined_bounds, label_message) =
                    render_auto_type_param_label(&params[0].bounds);
                let witnesses_join = displayed_witnesses.join("; ");
                let lex_first_witness = displayed_witnesses[0].clone();
                let message = if total > NON_UNIQUE_DISPLAY_CAP {
                    format!(
                        "auto(free) type-parameters have multiple feasible cross-product assignments: {witnesses_join}; (more than {NON_UNIQUE_DISPLAY_CAP} feasibles exist; rest elided); selected lexicographically-first '{lex_first_witness}'",
                    )
                } else {
                    format!(
                        "auto(free) type-parameters have multiple feasible cross-product assignments: {witnesses_join}; selected lexicographically-first '{lex_first_witness}'",
                    )
                };
                // FQN-only invariant (task 2860): candidates carries the lex-first
                // feasible leaf's bare FQN list — parallel to strict-Ambiguous above.
                diagnostics.push(
                    Diagnostic::warning(message)
                        .with_code(DiagnosticCode::AutoTypeParamNonUnique)
                        .with_label(DiagnosticLabel::new(params[0].use_site_span, label_message))
                        .with_candidates(feasible_assignments[0].clone()),
                );
                // Success shape (mirrors `1 =>` arm): full length-N per_param and
                // substitution, each param mapped to its lex-first candidate.
                let first_feasible = &feasible_assignments[0];
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
                MultiParamResolutionOutcome {
                    per_param,
                    substitution,
                }
            }
        }
    }
}

// ─── DFS rendering helpers (v0.2) ────────────────────────────────────────

/// Render composite witness strings for a slice of cross-product leaf assignments.
///
/// Each leaf is rendered as `"T=ORingSeal,U=AirCooled"` — param names zipped with
/// their selected candidate names and joined by `=`, then comma-joined across params.
///
/// Used by both the strict-Ambiguous and all-free NonUnique emission sites so both
/// share a single edit point. Task 2663 added the rich no-feasibles cross-product
/// diagnostic via `emit_no_feasible_cross_product_diagnostic` (used by the `0 =>`
/// arm and not this helper); per-leaf composite witnesses for the ≥2 feasibles
/// arms continue to flow through this helper.
fn render_witnesses(params: &[AutoTypeParam], leaves: &[Vec<String>]) -> Vec<String> {
    leaves
        .iter()
        .map(|leaf| {
            params
                .iter()
                .zip(leaf.iter())
                .map(|(p, c)| format!("{}={}", p.name, c))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect()
}

// ─── Static blame extraction (task 2660) ─────────────────────────────────────

/// Recursively collect every `Type::TypeParam(name)` string buried in a type.
///
/// Used by [`build_constraint_blame_map`] to extract param-name references from
/// a cell's declared `cell_type`. The set `out` accumulates names so the caller
/// can map them to param indices in a single pass.
///
/// Handles every composite type arm that can nest a `TypeParam`:
/// - Single-inner wrappers: `List`, `Set`, `Option`, `Complex`, `Range`
/// - Two-inner wrappers: `Map`, `Field`
/// - Multi-inner wrappers: `Function` (params + return_type), `Union` (arms)
/// - Quantity-slot structs (single `quantity` inner): `Point`, `Vector`, `Tensor`, `Matrix`
///
/// Leaf arms with no nested types (`Bool`, `Int`, `Real`, `String`, `Scalar`,
/// `Enum`, `StructureRef`, `TraitObject`, `Geometry`, `Orientation`, `Frame`,
/// `Transform`, `Plane`, `Axis`, `BoundingBox`, `Error`) are no-ops.
fn collect_type_param_names_from_type(t: &Type, out: &mut BTreeSet<String>) {
    match t {
        Type::TypeParam(name) => {
            out.insert(name.clone());
        }
        Type::List(inner)
        | Type::Set(inner)
        | Type::Option(inner)
        | Type::Complex(inner)
        | Type::Range(inner) => {
            collect_type_param_names_from_type(inner, out);
        }
        Type::Map(k, v) => {
            collect_type_param_names_from_type(k, out);
            collect_type_param_names_from_type(v, out);
        }
        Type::Function {
            params,
            return_type,
        } => {
            for p in params {
                collect_type_param_names_from_type(p, out);
            }
            collect_type_param_names_from_type(return_type, out);
        }
        Type::Field { domain, codomain } => {
            collect_type_param_names_from_type(domain, out);
            collect_type_param_names_from_type(codomain, out);
        }
        Type::Point { quantity, .. }
        | Type::Vector { quantity, .. }
        | Type::Tensor { quantity, .. }
        | Type::Matrix { quantity, .. } => {
            collect_type_param_names_from_type(quantity, out);
        }
        // Union arms can themselves contain TypeParam — recurse into each.
        // Note: `is_representable_cell_type` currently rejects Union as a
        // cell_type, so this arm is latent rather than immediately exercised.
        // It is included here so that if Union is ever admitted as a cell_type
        // (e.g., for match-block-decl narrowing), blame extraction stays correct
        // automatically rather than silently falling back to ordinary backtracking.
        Type::Union(arms) => {
            for arm in arms {
                collect_type_param_names_from_type(arm, out);
            }
        }
        // All other arms are terminal (contain no nested Type) — no-ops:
        // Bool, Int, Real, String, Scalar, Enum, StructureRef, TraitObject,
        // Geometry, Orientation, Frame, Transform, Plane, Axis, BoundingBox, Error.
        _ => {}
    }
}

/// Build a static blame map from constraint IDs to the set of `params` indices
/// that each constraint's expression tree references through `ValueRef` cells
/// typed as `Type::TypeParam(name)`.
///
/// # Algorithm
///
/// For each constraint in `template.constraints`:
/// 1. Walk `constraint.expr` via [`CompiledExpr::walk`].
/// 2. At every `ValueRef(cell_id)` node, look up the cell's declared type in
///    `template.value_cells`.
/// 3. Collect every `Type::TypeParam(name)` buried in that type (recursively,
///    via [`collect_type_param_names_from_type`]).
/// 4. Map collected names to `params` indices; drop names not in scope.
/// 5. If the resulting index set is non-empty, insert an entry into the map.
///
/// # "Absent ↔ no blame ↔ ordinary backtrack" contract
///
/// Constraints whose final index set is **empty** (no referenced cell is typed
/// as a `Type::TypeParam`, or all referenced `TypeParam` names are out of scope
/// for the current `params` slice) are **NOT inserted** into the map.
///
/// Callers that look up a constraint ID in the map and find `None` must treat
/// it as an empty blame set — equivalent to "this constraint cannot drive a
/// backjump." This is the sentinel that lets [`compute_deepest_blame_level`]
/// fall back to ordinary backtracking when violated constraints carry no
/// in-scope type-param blame. Do **not** use `unwrap_or_default()` on the
/// map lookup without understanding this contract: an absent entry means
/// "no blame", not "forgot to insert".
///
/// See PRD section *"Backjumping reuses existing 'rejected because' channel"*
/// for the rationale.
pub(crate) fn build_constraint_blame_map(
    template: &TopologyTemplate,
    params: &[AutoTypeParam],
) -> HashMap<ConstraintNodeId, BTreeSet<usize>> {
    // Build a lookup from ValueCellId → cell_type.
    let cell_type_map: HashMap<_, &Type> = template
        .value_cells
        .iter()
        .map(|decl| (&decl.id, &decl.cell_type))
        .collect();

    // Build a lookup from param name → index in `params`.
    let param_index_map: HashMap<&str, usize> = params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.as_str(), i))
        .collect();

    let mut result: HashMap<ConstraintNodeId, BTreeSet<usize>> = HashMap::new();

    for constraint in &template.constraints {
        let mut name_set: BTreeSet<String> = BTreeSet::new();
        constraint.expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(cell_id) = &node.kind
                && let Some(cell_type) = cell_type_map.get(cell_id)
            {
                collect_type_param_names_from_type(cell_type, &mut name_set);
            }
        });
        let indices: BTreeSet<usize> = name_set
            .iter()
            .filter_map(|name| param_index_map.get(name.as_str()).copied())
            .collect();
        if !indices.is_empty() {
            result.insert(constraint.id.clone(), indices);
        }
    }

    result
}

// ─── Leaf-check verdict and DFS control types (task 2660) ────────────────────

/// The result of a single DFS leaf feasibility check.
///
/// Returned by [`check_constraints_leaf`]; carries both the feasibility
/// decision (so the DFS can push a feasible leaf into `feasible_assignments`)
/// and the violated constraint IDs (so [`compute_deepest_blame_level`] can
/// derive the backjump target without a second `checker.check()` call).
///
/// The "single check() call per leaf" invariant is preserved:
/// [`check_constraints_leaf`] calls `checker.check()` exactly once and
/// partitions the results into the two fields below. The
/// [`reify_test_support::MockConstraintChecker::with_call_queue`] FIFO model
/// therefore still gives one queue-pop per leaf.
struct LeafVerdict {
    /// `true` iff no constraint in this call returned `Satisfaction::Violated`.
    /// (Both `Satisfied` and `Indeterminate` count as feasible — arch §2.5.)
    feasible: bool,
    /// The `ConstraintNodeId`s of every violated constraint in this call.
    /// Empty when `feasible == true`.
    violated_constraints: Vec<ConstraintNodeId>,
}

/// Control-flow token returned by [`dfs_search`] to its caller.
///
/// Three arms cover the three outcomes the DFS needs to communicate:
///
/// | Arm | Meaning |
/// |-----|---------|
/// | `Continue` | This sub-tree completed normally; try the next sibling. |
/// | `EarlyTerminate` | The requested `max_feasible_to_collect` count was reached; unwind immediately. |
/// | `BackjumpTo(j)` | An infeasible leaf was blamed on param `j`; unwind to level `j`. |
///
/// At level `K`, after receiving a `BackjumpTo(j)` from a recursive call:
/// - `j < K` → propagate (the backjump target is above this level).
/// - `j == K` → consume: pop the current candidate and continue the sibling
///   loop (equivalent to ordinary backtrack at `K`).
/// - `j > K` → unreachable (`unreachable!()`): the inner level at `K+1` would
///   have consumed `j == K+1` rather than propagating it, so `j > K` cannot
///   reach level `K` by induction.
///
/// See PRD *"Backjumping reuses existing 'rejected because' channel"* and the
/// design-decision "3-arm DfsControl enum" for the rationale.
enum DfsControl {
    Continue,
    EarlyTerminate,
    BackjumpTo(usize),
}

/// Build the flat `(id, &expr)` pair list from a [`TopologyTemplate`]'s
/// constraints.
///
/// Collects each [`CompiledConstraint`] into a `(ConstraintNodeId, &CompiledExpr)`
/// pair, borrowing the expression in-place (no clone of the expr).
///
/// # Deferred-substitution note
///
/// This helper is intentionally hoisted outside any per-candidate or per-leaf
/// loop because the template's constraints do not change across candidates
/// under the current (pre-substitution) mechanics.
///
/// NOTE: When the deferred type-substitution pass lands (substituting
/// `Type::TypeParam(T)` → `Type::StructureRef(candidate)`), the build will
/// need to move back inside the per-candidate / per-leaf loop with
/// per-candidate `ValueMap` setup. At that point this helper must be migrated
/// or removed together with the `filter_feasible_candidates` and
/// `resolve_auto_type_params_with_backtracking` callers.
///
/// NOTE(substitution-pass-trigger) — PARTIALLY REVERTED by task 4434 γ (re-homed from
/// task 3637): the `leaf_values` / per-candidate `ValueMap` portion of the hoist reversion
/// has landed — `dfs_search` now seeds a per-leaf `ValueMap` from the selected candidates'
/// literal defaults, and `filter_feasible_candidates_seeded` seeds per-candidate ValueMaps.
/// `build_constraints_template` itself is still hoisted (called once before the DFS loop)
/// because `template.constraints` is byte-identical across leaves today.  When
/// `Type::TypeParam(T) → Type::StructureRef(candidate)` substitution lands and per-candidate
/// cell types diverge, this build must also move inside the per-candidate / per-leaf loop.
/// Audit: docs/architecture-audit/findings/auto-resolution-backtracking.md M-005/M-013.
fn build_constraints_template(
    template: &TopologyTemplate,
) -> Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> {
    template
        .constraints
        .iter()
        .map(|c| (c.id.clone(), &c.expr))
        .collect()
}

/// Single-call leaf check that surfaces both feasibility and violated IDs.
///
/// Calls `checker.check(&input)` **exactly once**. Partitions the results
/// into:
/// - `violated_constraints`: the IDs of every `Satisfaction::Violated` result.
/// - `feasible`: `true` iff `violated_constraints` is empty.
///
/// This is the hot-path entry point for the DFS leaf branch. The returned
/// [`LeafVerdict`] is consumed by two callers:
/// - The DFS pushes a feasible leaf into `feasible_assignments`.
/// - [`compute_deepest_blame_level`] looks up the violated IDs in the static
///   blame map to derive the backjump target.
///
/// The "single check() call per leaf" invariant is preserved: the
/// [`reify_test_support::MockConstraintChecker::with_call_queue`] FIFO model
/// therefore gives exactly one queue-pop per leaf — the same property that
/// `dfs_leaf_invokes_constraint_checker_exactly_once_per_leaf_with_multi_constraint_template_two_params`
/// pins.
///
/// Inherits arch §2.5's monotonic-feasible rule: `Indeterminate` counts as
/// feasible; only `Violated` falsifies.
///
/// Passes `constraints_template` through as `Cow::Borrowed`, so no per-leaf
/// clone occurs (see task 2900).
fn check_constraints_leaf(
    constraints_template: &[(ConstraintNodeId, &reify_ir::CompiledExpr)],
    checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    values: &reify_ir::ValueMap,
) -> LeafVerdict {
    use reify_ir::ConstraintInput;
    let input = ConstraintInput {
        constraints: Cow::Borrowed(constraints_template),
        values,
        functions,
        determinacy: None,
    };
    let results = checker.check(&input);
    let violated_constraints: Vec<ConstraintNodeId> = results
        .into_iter()
        .filter(|r| r.satisfaction == reify_ir::Satisfaction::Violated)
        .map(|r| r.id)
        .collect();
    let feasible = violated_constraints.is_empty();
    LeafVerdict {
        feasible,
        violated_constraints,
    }
}

/// Compute the deepest (max-index) blamed param level from a set of violated
/// constraint IDs and the static blame map.
///
/// # Graph-based / Gaschnig backjumping semantics
///
/// Returns `max` over the **union** of every violated constraint's referenced-
/// param-index set — NOT max over any single constraint's blame, and NOT min
/// over the union.
///
/// Concretely, this is:
/// ```ignore
/// let union: BTreeSet<usize> = violated
///     .iter()
///     .filter_map(|id| blame_map.get(id))
///     .flatten()
///     .copied()
///     .collect();
/// union.iter().max().copied()
/// ```
///
/// **Do not** use `violated.iter().filter_map(|id| blame_map.get(id).and_then(|s| s.iter().max())).max()`
/// — that takes the max over each individual constraint's blame set before
/// combining, which gives the wrong answer when two constraints blame different
/// params. Example: c0 blames {T(0)}, c1 blames {U(1)}; max-per-constraint
/// gives max(0, 1) = 1 in this particular case but the formula is semantically
/// wrong because it does not first form the union {0,1}.
///
/// Rationale (soundness): every variable in the union is referenced by at
/// least one violated constraint. Changing any candidate strictly above the
/// deepest blamed index `J` cannot satisfy the violated constraints; the
/// search can safely jump to `J`.
///
/// When the union is empty (`violated` is empty, or no violated constraint
/// is in the blame map), returns `None` — the recursion falls back to
/// ordinary backtracking.
///
/// See PRD section *"Backjumping reuses existing 'rejected because' channel"*
/// and the design-decision "Aggregate deepest blame as J = max(union …)".
fn compute_deepest_blame_level(
    violated: &[ConstraintNodeId],
    blame_map: &HashMap<ConstraintNodeId, BTreeSet<usize>>,
) -> Option<usize> {
    // Take the max over the union of all blamed param-index sets.
    // One-pass iterator chain avoids the intermediate BTreeSet allocation.
    violated
        .iter()
        .filter_map(|id| blame_map.get(id))
        .flat_map(|s| s.iter())
        .copied()
        .max()
}

/// Recursive DFS over the cross-product of per-param Phase A candidate vectors,
/// with graph-based / Gaschnig backjumping via the static blame map.
///
/// Visits leaves in declared-order × lexicographic-within-param order: at
/// `level`, iterates `per_param_candidates[level]` in the order Phase A
/// produced (alphabetical FQN), pushes the candidate onto `current`,
/// recurses, then pops. At the leaf (`level == per_param_candidates.len()`),
/// calls [`check_constraints_leaf`]; if feasible, pushes the leaf into
/// `feasible_assignments`.
///
/// # Return value — `DfsControl`
///
/// `DfsControl::Continue` — this sub-tree completed normally; the caller should
/// try its next sibling.
///
/// `DfsControl::EarlyTerminate` — `max_feasible_to_collect` was reached; unwind
/// immediately without collecting more.
///
/// `DfsControl::BackjumpTo(j)` — an infeasible leaf blamed param `j`; the caller
/// at level `j` should pop its current candidate and try the next one, while
/// callers at levels `k > j` should propagate this value upward.
///
/// # Backjumping control flow (at level K, after a recursive call returns)
///
/// - `Continue` → pop, try next sibling at K.
/// - `EarlyTerminate` → pop, propagate.
/// - `BackjumpTo(j)` where `j < K` → pop, propagate (continue unwinding).
/// - `BackjumpTo(j)` where `j == K` → pop, continue sibling loop (equivalent
///   to ordinary backtrack at K — the target reached its home level).
/// - `BackjumpTo(j)` where `j > K` → `unreachable!()`: the inner level
///   at `K+1` would have consumed `j == K+1` or propagated `j < K+1`. `j > K`
///   is unreachable by induction.
///
/// # Blame-map absent ↔ ordinary backtrack
///
/// When the blame map contains no entry for any violated constraint (empty
/// blame, e.g. when the constraint is a `Bool(true)` literal with no
/// `ValueRef`), `compute_deepest_blame_level` returns `None`. The leaf then
/// returns `DfsControl::Continue`, which degenerates to ordinary backtracking —
/// the 2659/2661 test outcomes are preserved without behavioral change.
//
// `#[allow(clippy::too_many_arguments)]`: this recursive helper threads
// recursion state (`level`, `current`, `feasible_assignments`) alongside
// shared search context (`per_param_candidates`, `constraints_template`,
// `constraint_checker`, `functions`, `max_feasible_to_collect`, `blame_map`).
// Wrapping the shared context in a struct would force every recursive call
// site to deref through the wrapper for what is effectively a flat parameter
// pack; the ambient crate convention (see `resolve_auto_type_params_with_backtracking`
// above and the 35+ other sites) is to allow the lint here.
#[allow(clippy::too_many_arguments)]
fn dfs_search(
    level: usize,
    per_param_candidates: &[Vec<String>],
    current: &mut Vec<String>,
    feasible_assignments: &mut Vec<Vec<String>>,
    constraints_template: &[(ConstraintNodeId, &reify_ir::CompiledExpr)],
    template_registry: &HashMap<String, &TopologyTemplate>,
    param_members: &[Option<String>],
    constraint_checker: &dyn ConstraintChecker,
    functions: &[CompiledFunction],
    max_feasible_to_collect: usize,
    blame_map: &HashMap<ConstraintNodeId, BTreeSet<usize>>,
) -> DfsControl {
    if level == per_param_candidates.len() {
        // Leaf branch: build a per-leaf ValueMap seeded from each param's selected
        // candidate's literal defaults (hoist reversion — task 4434 γ, re-homed from
        // task 3637).  `param_members[i]` is the member name of the value cell in the
        // parameterized template that carries `Type::TypeParam(params[i].name)`.
        // When a member is `None` (no matching value cell) or the candidate template is
        // absent from the registry, that param's contribution to the map is empty —
        // stub-path no-op semantics (PRD §11.2).
        let mut leaf_values = reify_ir::ValueMap::new();
        for (i, candidate) in current.iter().enumerate() {
            if let Some(Some(member)) = param_members.get(i)
                && let Some(&candidate_template) = template_registry.get(candidate.as_str())
            {
                let seeded = seed_candidate_value_map(candidate_template, member);
                for (k, v) in seeded.iter() {
                    leaf_values.insert(k.clone(), v.clone());
                }
            }
        }
        // Call the constraint checker ONCE for this leaf.
        // The `check_constraints_leaf` helper surfaces both feasibility and
        // the violated constraint IDs in a single `checker.check()` invocation.
        //
        // Backtracking is driven by the same `Satisfaction::Violated`
        // discriminator as v0.1 BFS; `Indeterminate` counts as feasible
        // per arch §2.5's monotonic-feasible rule.
        let verdict = check_constraints_leaf(
            constraints_template,
            constraint_checker,
            functions,
            &leaf_values,
        );
        if verdict.feasible {
            feasible_assignments.push(current.clone());
            // Early-terminate once the requested feasible count is reached:
            // free-mode (max=NON_UNIQUE_DISPLAY_CAP+1, task 2663) stops one
            // past the display cap so we can detect "more than the cap";
            // strict-mode (max=2) stops once Ambiguous is provable.
            if feasible_assignments.len() >= max_feasible_to_collect {
                return DfsControl::EarlyTerminate;
            }
            return DfsControl::Continue;
        }
        // Infeasible leaf: try to compute a backjump target from the blame map.
        // When `compute_deepest_blame_level` returns `Some(j)`, the search
        // skips every remaining assignment in the entire (current[0..=j-1], *)
        // sub-tree by propagating `BackjumpTo(j)` up to level j.
        // When it returns `None` (empty blame), fall back to `Continue` —
        // identical to ordinary backtracking.
        if let Some(j) = compute_deepest_blame_level(&verdict.violated_constraints, blame_map) {
            return DfsControl::BackjumpTo(j);
        }
        return DfsControl::Continue;
    }

    for candidate in &per_param_candidates[level] {
        current.push(candidate.clone());
        let control = dfs_search(
            level + 1,
            per_param_candidates,
            current,
            feasible_assignments,
            constraints_template,
            template_registry,
            param_members,
            constraint_checker,
            functions,
            max_feasible_to_collect,
            blame_map,
        );
        current.pop();
        match control {
            DfsControl::Continue => {
                // Normal completion of the child sub-tree; try the next sibling
                // at this level.
            }
            DfsControl::EarlyTerminate => {
                // Requested feasible count reached; propagate unwind immediately.
                return DfsControl::EarlyTerminate;
            }
            DfsControl::BackjumpTo(j) => {
                if j < level {
                    // Backjump target is above this level; propagate upward.
                    return DfsControl::BackjumpTo(j);
                } else if j == level {
                    // Backjump target reached this level: pop the current
                    // candidate (already done above) and continue the sibling
                    // loop — equivalent to ordinary backtrack at this level.
                    // Nothing to return; fall through to the next iteration.
                } else {
                    // j > level: unreachable by induction (see function-level doc).
                    unreachable!(
                        "DfsControl::BackjumpTo({j}) arrived at level {level}: \
                         j > level is unreachable; inner level would have consumed j==level"
                    );
                }
            }
        }
    }
    DfsControl::Continue
}

// ─── Unit tests for private helpers ──────────────────────────────────────────

#[cfg(test)]
mod helper_tests {
    use super::{
        AutoTypeParam, build_constraint_blame_map, build_constraints_template,
        check_constraints_leaf, dfs_search,
    };
    use crate::TopologyTemplate;
    use reify_test_support::MockConstraintChecker;
    use reify_core::{ConstraintNodeId, Type};
    use reify_ir::{CompiledFunction, Satisfaction, Value};

    fn literal_expr() -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(Value::Bool(true), Type::Bool)
    }

    /// Construct a bare-bones [`crate::TopologyTemplate`] for testing, enumerating every
    /// field in one place.
    ///
    /// All non-varying fields default to their empty/private/false/`None` equivalents.
    /// This is the single source of truth for the exhaustive-field-enumeration pattern:
    /// when `TopologyTemplate` gains a new field, add it here (default or empty is fine)
    /// so tests pinning other contracts continue to compile unchanged.
    ///
    /// Note: does NOT use `TopologyTemplateBuilder` from `reify_test_support` — that
    /// builder links against the compiled `reify_compiler` library artifact, causing a
    /// "two versions of the same crate" mismatch inside `#[cfg(test)]`. Direct
    /// construction from `crate::` types keeps both sides in the same compilation unit.
    fn make_topology_template(
        name: &str,
        value_cells: Vec<crate::ValueCellDecl>,
        constraints: Vec<crate::CompiledConstraint>,
        content_hash_seed: &[u8],
    ) -> crate::TopologyTemplate {
        use reify_core::ContentHash;
        crate::TopologyTemplate {
            name: name.into(),
            doc: None,
            entity_kind: crate::EntityKind::Structure,
            visibility: crate::Visibility::Private,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells,
            constraints,
            realizations: vec![],
            sub_components: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: Default::default(),
            objective: None,
            meta: Default::default(),
            content_hash: ContentHash::of(content_hash_seed),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// Empty `constraints_template` slice → vacuously no violations → `feasible == true`.
    #[test]
    fn check_constraints_leaf_returns_feasible_for_empty_constraints() {
        let checker = MockConstraintChecker::new();
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        let result = check_constraints_leaf(&[], &checker, functions, &values);
        assert!(
            result.feasible,
            "empty constraints slice must be feasible (vacuously no violations)"
        );
    }

    /// Single constraint, checker returns `Satisfied` → `feasible == true`.
    #[test]
    fn check_constraints_leaf_returns_feasible_when_all_satisfied() {
        let expr = literal_expr();
        let id = ConstraintNodeId::new("C0", 0);
        let constraints: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> =
            vec![(id.clone(), &expr)];
        let checker = MockConstraintChecker::new(); // default: Satisfied
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        let result = check_constraints_leaf(&constraints, &checker, functions, &values);
        assert!(
            result.feasible,
            "all-Satisfied constraints must be feasible (no violations)"
        );
    }

    /// Single constraint, checker returns `Indeterminate` → `feasible == true`
    /// (architecture §2.5: Indeterminate counts as feasible, does not falsify).
    #[test]
    fn check_constraints_leaf_returns_feasible_when_all_indeterminate_per_arch_2_5() {
        let expr = literal_expr();
        let id = ConstraintNodeId::new("C0", 0);
        let constraints: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> =
            vec![(id.clone(), &expr)];
        let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        let result = check_constraints_leaf(&constraints, &checker, functions, &values);
        assert!(
            result.feasible,
            "Indeterminate constraints must be feasible (undef does not falsify, arch §2.5)"
        );
    }

    /// Two constraints, checker returns `Violated` for all → `feasible == false`.
    #[test]
    fn check_constraints_leaf_returns_infeasible_when_any_violated() {
        let expr = literal_expr();
        let id0 = ConstraintNodeId::new("C0", 0);
        let id1 = ConstraintNodeId::new("C1", 1);
        let constraints: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> =
            vec![(id0.clone(), &expr), (id1.clone(), &expr)];
        let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        let result = check_constraints_leaf(&constraints, &checker, functions, &values);
        assert!(
            !result.feasible,
            "all-Violated constraints must not be feasible (any one Violated falsifies)"
        );
    }

    /// Two constraints with distinct ids "C0" and "C1"; C0 → Satisfied, C1 → Violated →
    /// `feasible == false` (any one Violated falsifies).
    #[test]
    fn check_constraints_leaf_returns_infeasible_for_mixed_satisfied_and_violated() {
        let expr = literal_expr();
        let id0 = ConstraintNodeId::new("C0", 0);
        let id1 = ConstraintNodeId::new("C1", 1);
        let constraints: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> =
            vec![(id0.clone(), &expr), (id1.clone(), &expr)];
        // C0 → Satisfied (default), C1 → Violated
        let checker = MockConstraintChecker::new()
            .with_default(Satisfaction::Satisfied)
            .with_result(id1.clone(), Satisfaction::Violated);
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        let result = check_constraints_leaf(&constraints, &checker, functions, &values);
        assert!(
            !result.feasible,
            "mixed Satisfied+Violated must not be feasible (any one Violated falsifies)"
        );
    }

    /// `build_constraints_template` maps each `CompiledConstraint` in the
    /// `TopologyTemplate` to a `(ConstraintNodeId, &CompiledExpr)` pair,
    /// preserving order and borrowing (not cloning) the expr.
    #[test]
    fn build_constraints_template_returns_pairs_for_each_template_constraint() {
        use reify_core::SourceSpan;

        let expr0 = literal_expr();
        let expr1 = reify_ir::CompiledExpr::literal(Value::Bool(false), Type::Bool);

        let template = make_topology_template(
            "Bearing",
            vec![],
            vec![
                crate::CompiledConstraint {
                    id: ConstraintNodeId::new("Bearing", 0),
                    label: None,
                    expr: expr0,
                    span: SourceSpan::new(0, 0),
                    domain: None,
                    optimized_target: None,
                },
                crate::CompiledConstraint {
                    id: ConstraintNodeId::new("Bearing", 1),
                    label: None,
                    expr: expr1,
                    span: SourceSpan::new(0, 0),
                    domain: None,
                    optimized_target: None,
                },
            ],
            b"test-bearing",
        );

        let pairs = build_constraints_template(&template);

        // (a) one pair per constraint
        assert_eq!(
            pairs.len(),
            2,
            "must return one pair per template constraint"
        );

        // (b) order preserved — entry 0 matches template.constraints[0]
        assert_eq!(
            pairs[0].0, template.constraints[0].id,
            "entry 0 id must match template.constraints[0].id"
        );
        assert_eq!(
            pairs[1].0, template.constraints[1].id,
            "entry 1 id must match template.constraints[1].id"
        );

        // (c) borrow, not clone — raw-pointer equality pins the no-copy contract
        assert!(
            std::ptr::eq(pairs[0].1, &template.constraints[0].expr),
            "entry 0 expr pointer must equal &template.constraints[0].expr (no clone)"
        );
        assert!(
            std::ptr::eq(pairs[1].1, &template.constraints[1].expr),
            "entry 1 expr pointer must equal &template.constraints[1].expr (no clone)"
        );
    }

    /// Pins the no-per-leaf-clone invariant: `check_constraints_leaf` must pass
    /// `Cow::Borrowed` to the checker, not `Cow::Owned` (task 2900, step 5/6).
    ///
    /// Uses `BorrowAssertingChecker` — a tiny in-test impl that records whether
    /// `input.constraints` was `Cow::Borrowed` at call time. The test fires
    /// `false` as long as step-4's temporary `Cow::Owned(constraints_template.to_vec())`
    /// is still in place; step-6's switch to `Cow::Borrowed` makes it `true`.
    #[test]
    fn check_constraints_leaf_passes_constraints_as_cow_borrowed() {
        use reify_ir::{ConstraintChecker, ConstraintInput, ConstraintResult};
        use std::sync::atomic::{AtomicBool, Ordering};

        struct BorrowAssertingChecker {
            saw_borrowed: AtomicBool,
        }

        impl ConstraintChecker for BorrowAssertingChecker {
            fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
                let is_borrowed = matches!(input.constraints, std::borrow::Cow::Borrowed(_));
                self.saw_borrowed.store(is_borrowed, Ordering::SeqCst);
                Vec::new()
            }
        }

        let expr0 = literal_expr();
        let expr1 = reify_ir::CompiledExpr::literal(Value::Bool(false), Type::Bool);
        let constraints: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> = vec![
            (ConstraintNodeId::new("C0", 0), &expr0),
            (ConstraintNodeId::new("C1", 1), &expr1),
        ];

        let checker = BorrowAssertingChecker {
            saw_borrowed: AtomicBool::new(false),
        };
        let functions: &[CompiledFunction] = &[];
        let values = reify_ir::ValueMap::new();

        check_constraints_leaf(&constraints, &checker, functions, &values);

        assert!(
            checker.saw_borrowed.load(Ordering::SeqCst),
            "check_constraints_leaf must pass Cow::Borrowed to the checker (no per-leaf clone)"
        );
    }

    /// By induction, `j > level` is unreachable for well-formed blame_maps.
    /// This test synthesizes a malformed blame_map (out-of-range param index 2
    /// in a 2-param search) so the leaf returns `BackjumpTo(2)`, which routes to
    /// the `j > level` arm at level 1.
    ///
    /// The expected substring `"internal error: entered unreachable code"` is
    /// emitted by `unreachable!()` but NOT by `debug_assert!(false, …)` (which
    /// starts with `"assertion failed: false"`), so this test fails before the
    /// impl step and passes after — proper failing-first TDD.
    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn dfs_search_panics_with_unreachable_when_blame_index_exceeds_param_count() {
        use std::collections::{BTreeSet, HashMap};

        let expr = literal_expr();
        let id = ConstraintNodeId::new("C0", 0);

        // 2-param search: only indices 0 and 1 are valid.
        let per_param_candidates = vec![vec!["A".into()], vec!["B".into()]];
        let constraints_template: Vec<(ConstraintNodeId, &reify_ir::CompiledExpr)> =
            vec![(id.clone(), &expr)];

        // Force the leaf to be infeasible so compute_deepest_blame_level is called.
        let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
        let functions: &[CompiledFunction] = &[];

        // Malformed blame_map: blame "C0" on out-of-range index 2.
        // By induction the production `compute_blame_map` cannot emit index 2 in a
        // 2-param search, but dfs_search does not validate blame_map, so:
        //   level-2 leaf returns BackjumpTo(2)
        //   level-1 caller sees j=2 > level=1 → enters the `j > level` arm.
        let mut blame_set = BTreeSet::new();
        blame_set.insert(2usize);
        let mut blame_map: HashMap<ConstraintNodeId, BTreeSet<usize>> = HashMap::new();
        blame_map.insert(id.clone(), blame_set);

        let mut current: Vec<String> = Vec::new();
        let mut feasible_assignments: Vec<Vec<String>> = Vec::new();
        let empty_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let empty_param_members: Vec<Option<String>> = Vec::new();
        dfs_search(
            0,
            &per_param_candidates,
            &mut current,
            &mut feasible_assignments,
            &constraints_template,
            &empty_registry,
            &empty_param_members,
            &checker,
            functions,
            usize::MAX,
            &blame_map,
        );
    }

    // ─── build_constraint_blame_map unit tests ────────────────────────────────

    /// `build_constraint_blame_map` must return one entry per constraint that
    /// references at least one in-scope `TypeParam`-typed cell. The entry maps
    /// the `ConstraintNodeId` to the `BTreeSet<usize>` of referenced param indices.
    ///
    /// Setup: two cells (`field_t : TypeParam("T")`, `field_u : TypeParam("U")`),
    /// one `BinOp(Eq)` constraint whose `ValueRef`s address both cells.
    /// `params = [T(idx=0), U(idx=1)]` → blame set = `{0, 1}`.
    ///
    /// Pins the "at least one TypeParam ref → entry present" half of the contract.
    /// The "no ref → absent" half is pinned by
    /// `build_constraint_blame_map_excludes_out_of_scope_type_params_and_no_typeparam_constraints`.
    ///
    #[test]
    fn build_constraint_blame_map_returns_param_indices_referenced_by_constraint_expression() {
        use reify_core::{SourceSpan, ValueCellId};
        use reify_ir::BinOp;
        use std::collections::BTreeSet;

        let field_t = ValueCellId::new("Coupling", "field_t");
        let field_u = ValueCellId::new("Coupling", "field_u");
        let expr = reify_ir::CompiledExpr::binop(
            BinOp::Eq,
            reify_ir::CompiledExpr::value_ref(field_t.clone(), Type::TypeParam("T".into())),
            reify_ir::CompiledExpr::value_ref(field_u.clone(), Type::TypeParam("U".into())),
            Type::Bool,
        );

        let template = make_topology_template(
            "Coupling",
            vec![
                crate::ValueCellDecl {
                    id: field_t.clone(),
                    kind: crate::ValueCellKind::Param,
                    visibility: crate::Visibility::Private,
                    is_aux: false,
                    cell_type: Type::TypeParam("T".into()),
                    default_expr: None,
                    solver_hints: vec![],
                    span: SourceSpan::new(0, 0),
                },
                crate::ValueCellDecl {
                    id: field_u.clone(),
                    kind: crate::ValueCellKind::Param,
                    visibility: crate::Visibility::Private,
                    is_aux: false,
                    cell_type: Type::TypeParam("U".into()),
                    default_expr: None,
                    solver_hints: vec![],
                    span: SourceSpan::new(0, 0),
                },
            ],
            vec![crate::CompiledConstraint {
                id: ConstraintNodeId::new("Coupling", 0),
                label: None,
                expr,
                span: SourceSpan::new(0, 0),
                domain: None,
                optimized_target: None,
            }],
            b"test-coupling-blame",
        );

        let params = vec![
            AutoTypeParam {
                name: "T".to_string(),
                bounds: vec![],
                free: true,
                use_site_span: SourceSpan::empty(0),
            },
            AutoTypeParam {
                name: "U".to_string(),
                bounds: vec![],
                free: true,
                use_site_span: SourceSpan::empty(0),
            },
        ];

        let map = build_constraint_blame_map(&template, &params);

        assert_eq!(
            map.len(),
            1,
            "expect exactly one entry (one constraint with TypeParam refs); got: {:?}",
            map
        );
        let cid = ConstraintNodeId::new("Coupling", 0);
        assert_eq!(
            map.get(&cid).cloned().unwrap_or_default(),
            BTreeSet::from([0_usize, 1_usize]),
            "constraint referencing both T(idx=0) and U(idx=1) cells must map to {{0, 1}}"
        );
    }

    /// `build_constraint_blame_map` must NOT insert an entry for constraints whose
    /// blame set would be empty. Two sub-cases:
    ///
    /// (a) A cell typed `Type::TypeParam("Z")` where `Z` is NOT in `params=[T,U]`
    ///     contributes nothing — the constraint that only ValueRefs that cell must
    ///     be absent from the result map.
    ///
    /// (b) A constraint whose expression is `CompiledExpr::literal(Value::Bool(true),
    ///     Type::Bool)` (no ValueRef, no TypeParam anywhere) is also absent.
    ///
    /// Setup: three cells (`field_t:T`, `field_u:U`, `field_z:Z`), two constraints:
    /// - c0: `ValueRef(field_z)` only  → blame={} (Z ∉ params) → absent
    /// - c1: `Bool(true)` literal      → blame={} (no ValueRef)  → absent
    ///
    /// Pins the "empty blame → absent" invariant the DFS recursion relies on:
    /// `compute_deepest_blame_level` returns `None` for absent constraints and falls
    /// back to ordinary backtracking, so an accidental `map.insert(id, BTreeSet::new())`
    /// would incorrectly block backjumping even when no TypeParam blame exists.
    #[test]
    fn build_constraint_blame_map_excludes_out_of_scope_type_params_and_no_typeparam_constraints() {
        use reify_core::{SourceSpan, ValueCellId};

        let field_t = ValueCellId::new("Coupling", "field_t");
        let field_u = ValueCellId::new("Coupling", "field_u");
        let field_z = ValueCellId::new("Coupling", "field_z");

        // c0: ValueRef of field_z (typed TypeParam("Z"), out-of-scope)
        let expr_c0 =
            reify_ir::CompiledExpr::value_ref(field_z.clone(), Type::TypeParam("Z".into()));
        // c1: literal Bool(true) — no ValueRef, no TypeParam
        let expr_c1 = reify_ir::CompiledExpr::literal(Value::Bool(true), Type::Bool);

        let template = make_topology_template(
            "Coupling",
            vec![
                crate::ValueCellDecl {
                    id: field_t.clone(),
                    kind: crate::ValueCellKind::Param,
                    visibility: crate::Visibility::Private,
                    is_aux: false,
                    cell_type: Type::TypeParam("T".into()),
                    default_expr: None,
                    solver_hints: vec![],
                    span: SourceSpan::new(0, 0),
                },
                crate::ValueCellDecl {
                    id: field_u.clone(),
                    kind: crate::ValueCellKind::Param,
                    visibility: crate::Visibility::Private,
                    is_aux: false,
                    cell_type: Type::TypeParam("U".into()),
                    default_expr: None,
                    solver_hints: vec![],
                    span: SourceSpan::new(0, 0),
                },
                crate::ValueCellDecl {
                    id: field_z.clone(),
                    kind: crate::ValueCellKind::Param,
                    visibility: crate::Visibility::Private,
                    is_aux: false,
                    cell_type: Type::TypeParam("Z".into()),
                    default_expr: None,
                    solver_hints: vec![],
                    span: SourceSpan::new(0, 0),
                },
            ],
            vec![
                crate::CompiledConstraint {
                    id: ConstraintNodeId::new("Coupling", 0),
                    label: None,
                    expr: expr_c0,
                    span: SourceSpan::new(0, 0),
                    domain: None,
                    optimized_target: None,
                },
                crate::CompiledConstraint {
                    id: ConstraintNodeId::new("Coupling", 1),
                    label: None,
                    expr: expr_c1,
                    span: SourceSpan::new(0, 0),
                    domain: None,
                    optimized_target: None,
                },
            ],
            b"test-coupling-exclusion",
        );

        let params = vec![
            AutoTypeParam {
                name: "T".to_string(),
                bounds: vec![],
                free: true,
                use_site_span: SourceSpan::empty(0),
            },
            AutoTypeParam {
                name: "U".to_string(),
                bounds: vec![],
                free: true,
                use_site_span: SourceSpan::empty(0),
            },
            // Z is intentionally NOT in params — it must be treated as out-of-scope
        ];

        let map = build_constraint_blame_map(&template, &params);

        assert!(
            map.is_empty(),
            "constraints with empty blame sets must not appear in the map \
             (empty map expected); got: {:?}",
            map
        );
    }
}
