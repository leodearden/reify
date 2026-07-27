//! ζ (task 4359) differential safety-gate — the §6 expanded BOUNDARY cases.
//!
//! These are the cases a plain legacy-vs-unified `BuildResult` diff CANNOT
//! surface (because legacy degrades identically, or the property is about a
//! scheduler-internal ordering / a directly-asserted unified-only diagnostic):
//!   * auto + geometry-backed constraint → `EvalUnresolved` (unified) /
//!     Indeterminate (legacy);
//!   * cross-sub multi-body assembly with a lexicographically-early parent →
//!     byte-equivalent multi-body export under both schedulers;
//!   * the 4275 single-instance `let proc = FdmPrinter()` definite-verdict form;
//!   * multi-realization export equivalence + a warm scheduler-agnostic
//!     regression guard (warm stays scheduler-agnostic until θ #4361).
//!
//! The corpus SWEEP (equivalence-or-reasoned, 2× byte-identical, residue==∅)
//! lives in the sibling binary `unified_dag_differential_corpus.rs`.
//!
//! The shared harness is `#[path]`-included (NOT via `tests/common/mod.rs`) so
//! this safety-gate lands with zero edits to existing shared test files.

#[path = "common/differential.rs"]
mod differential;

use differential::{
    AUTO_GEOMETRY_CONSTRAINT_SRC, CROSS_LET_4275_SRC, CorpusCase, Divergence,
    LEX_PARENT_MULTIBODY_SRC, MULTI_ENTITY_EXPORT_SRC, MULTI_REALIZATION_SRC,
    WARM_AUTO_CONST_LET_SRC, WARM_PREDICATE_K5_SRC, WARM_PREDICATE_SRC,
    assert_equivalent_or_allowed, build_case, build_case_keep_engine, build_snapshot_export_matches_build,
    build_under, build_with_kernel_stdlib, cold_eval_with_solver,
    fits_build_volume_satisfaction, project_build_result, project_eval_values,
    residue_for, seeded_build_volume_kernel, warm_eval_after_edit, warm_eval_cached_with_solver,
};
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::BuildScheduler;
use reify_ir::{Satisfaction, Value};

/// The TWO reasoned divergences the 4275 single-instance cross-`let` case
/// legitimately exhibits under the seeded kernel (step-16). Mirrors the corpus
/// binary's `REASONED_4275` (the same divergence proven by
/// `allow_list_admits_only_reasoned_divergence`): (1) the `FitsBuildVolume`
/// constraint flips Indeterminate (legacy) → DEFINITE (unified); (2) the
/// legacy-only `ConstraintIndeterminate` warning vanishes once unified resolves
/// the verdict (a CONSEQUENCE of the flip, not an independent divergence). Every
/// other field is byte-equal, so the gate admits exactly these and nothing else.
const REASONED_4275_BOUNDARY: &[Divergence] = &[
    Divergence::ConstraintFlips {
        constraint: "FitsBuildVolume",
        reason: "4275 single-instance `let proc = FdmPrinter()`: UnifiedDag folds the cross-`let` \
                 bounding_box(proc.build_volume) leaf post-geometry (PRD §3.3) → a DEFINITE \
                 verdict; LegacyMultiPass freezes the pre-geometry Indeterminate",
    },
    Divergence::DiagnosticAdded {
        code: DiagnosticCode::ConstraintIndeterminate,
        reason: "the legacy-only `constraint … indeterminate` warning vanishes once UnifiedDag \
                 resolves the constraint to a definite verdict (a consequence of the flip, not an \
                 independent divergence)",
    },
];

// ─────────────────────────────────────────────────────────────────────────────
// step-11 (RED): auto + geometry-backed constraint → `EvalUnresolved`.
//
// THE §6 BOUNDARY a plain legacy-vs-unified diff CANNOT surface: legacy degrades
// to Indeterminate and unified DECLINES the same constraint, so neither produces
// a definite verdict — a `BuildResult` projection diff sees no constraint flip.
// The distinguishing signal is unified-ONLY: the δ/ε transitive-auto-read guard
// fires `E_EVAL_UNRESOLVED` (the constraint's geometry-backed read closure reaches
// an `auto` cell), which legacy never emits. We therefore assert the diagnostic
// presence/absence DIRECTLY rather than through `assert_equivalent_or_allowed`.
// ─────────────────────────────────────────────────────────────────────────────

/// Build the auto + geometry-backed-constraint idiom under BOTH schedulers (with a
/// SEEDED bbox kernel, so the constraint would otherwise be decidable — proving
/// the decline is the guard firing, not mere undecidability) and assert:
///   (a) under `UnifiedDag`, the diagnostics carry a `Severity::Error`
///       `DiagnosticCode::EvalUnresolved` NAMING the offending constraint, with NO
///       `EvalCycle` (the module is acyclic — no false-positive cycle) and no hang;
///   (b) under `LegacyMultiPass`, there is NO `EvalUnresolved` (it degrades to
///       Indeterminate identically — which is exactly why a plain diff is blind to
///       this boundary).
///
/// RED until step-12: `AUTO_GEOMETRY_CONSTRAINT_SRC` (the source idiom that drives
/// the transitive-auto-read guard) is not authored yet, so this fails to compile.
#[test]
fn auto_plus_geometry_constraint_emits_eval_unresolved() {
    let unified = build_with_kernel_stdlib(
        AUTO_GEOMETRY_CONSTRAINT_SRC,
        BuildScheduler::UnifiedDag,
        seeded_build_volume_kernel(),
    );
    let legacy = build_with_kernel_stdlib(
        AUTO_GEOMETRY_CONSTRAINT_SRC,
        BuildScheduler::LegacyMultiPass,
        seeded_build_volume_kernel(),
    );

    let codes = |r: &reify_eval::BuildResult| {
        r.diagnostics
            .iter()
            .map(|d| (d.code, d.severity, d.message.clone()))
            .collect::<Vec<_>>()
    };

    // (a) UnifiedDag: the guard declines the auto-reaching constraint and names it.
    let unresolved: Vec<_> = unified
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error)
        .collect();
    assert!(
        !unresolved.is_empty(),
        "UnifiedDag must surface a Severity::Error E_EVAL_UNRESOLVED for the auto-reaching \
         geometry-backed constraint (the transitive-auto-read guard firing); got {:?}",
        codes(&unified),
    );
    assert!(
        unresolved
            .iter()
            .any(|d| d.message.contains("unresolved constraint:")),
        "the E_EVAL_UNRESOLVED diagnostic must NAME the offending constraint; got {:?}",
        unresolved
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>(),
    );
    assert!(
        !unified
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::EvalCycle)),
        "the auto+geometry module is ACYCLIC — UnifiedDag must NOT surface a false-positive \
         EvalCycle; got {:?}",
        codes(&unified),
    );

    // (b) LegacyMultiPass: NO EvalUnresolved — it degrades to Indeterminate
    // identically, which is why a plain legacy-vs-unified diff cannot surface this
    // boundary (both sides decline; only the unified-only diagnostic distinguishes).
    assert!(
        !legacy
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::EvalUnresolved)),
        "LegacyMultiPass must NOT carry EvalUnresolved (it degrades to Indeterminate); got {:?}",
        codes(&legacy),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-13 (RED): cross-sub multi-body assembly with a LEXICOGRAPHICALLY-EARLY
// parent. The parent structure's name sorts BEFORE its child `sub` names, so the
// unified Kahn worklist's DebugOrd pop order is stressed: the parent's union
// realization must still pop AFTER its children's body realizations. The §3.1
// realization→realization edges enforce the topological order over the raw name
// sort, so both schedulers MUST produce byte-identical multi-body export +
// equivalent values/constraints, with residue==∅. A divergence here is a real
// worklist-ordering bug → escalate `design_concern`, never blanket-allow.
// ─────────────────────────────────────────────────────────────────────────────

/// RED until step-14: `LEX_PARENT_MULTIBODY_SRC` (the lexicographic-parent
/// assembly source) is not authored yet, so this fails to compile.
#[test]
fn cross_sub_multi_body_assembly_exports_equivalently() {
    let case = CorpusCase {
        name: "lex_parent_multibody",
        source: LEX_PARENT_MULTIBODY_SRC,
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    };

    let legacy = build_case(&case, BuildScheduler::LegacyMultiPass);
    let unified = build_case(&case, BuildScheduler::UnifiedDag);

    // (1) full projection equivalence (values + constraints + diagnostics +
    // geometry) admitting ZERO divergence (empty allow-list).
    assert_equivalent_or_allowed(&case, &legacy, &unified);

    // (2) explicit byte-equivalence of the exported multi-body geometry — the §3.1
    // edges keep the unified pop order correct despite the lexicographically-early
    // parent.
    assert_eq!(
        legacy.geometry_output,
        unified.geometry_output,
        "lexicographic-parent multi-body assembly: exported geometry MUST be byte-identical \
         across schedulers (legacy_len={:?}, unified_len={:?})",
        legacy.geometry_output.as_ref().map(|b| b.len()),
        unified.geometry_output.as_ref().map(|b| b.len()),
    );

    // (3) Stage-1 residue==∅ (the assembly is acyclic).
    let (engine, _) = build_case_keep_engine(&case, BuildScheduler::UnifiedDag);
    let residue = residue_for(&engine);
    assert!(
        residue.is_empty(),
        "lexicographic-parent assembly: Stage-1 residue MUST be ∅ under UnifiedDag; \
         got {} unpopped node(s): {residue:?}",
        residue.len(),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-15 (RED): the 4275 single-instance `let proc = FdmPrinter()` definite
// differential. UnifiedDag folds the cross-`let` bounding_box(proc.build_volume)
// leaf POST-geometry (PRD §3.3) → a DEFINITE verdict; LegacyMultiPass freezes the
// pre-geometry Indeterminate. This is a REASONED divergence (not an equivalence),
// expressed through `assert_equivalent_or_allowed` carrying a
// `Divergence::ConstraintFlips`. COUNT == 1 deliberately: the multi-instance
// same-def form is declined to Indeterminate (ε #4628, def-name-keyed snapshot
// cannot disambiguate instances) and must NOT be used as a definite-verdict case.
// ─────────────────────────────────────────────────────────────────────────────

/// RED until step-16: `fits_build_volume_satisfaction` (the harness verdict
/// extractor) and the reasoned `REASONED_4275_BOUNDARY` allow-list are not
/// authored yet, so this fails to compile.
#[test]
fn cross_sub_4275_let_bound_form_is_definite_differential() {
    let legacy = build_with_kernel_stdlib(
        CROSS_LET_4275_SRC,
        BuildScheduler::LegacyMultiPass,
        seeded_build_volume_kernel(),
    );
    let unified = build_with_kernel_stdlib(
        CROSS_LET_4275_SRC,
        BuildScheduler::UnifiedDag,
        seeded_build_volume_kernel(),
    );

    // (1) DIRECT: UnifiedDag reaches a DEFINITE verdict (Satisfied OR Violated,
    // NEVER a fixed polarity — the OCCT verdict-FLIP e2e is η's); legacy is frozen
    // Indeterminate. This DEFINITE check doubles as the HANDLE-REACH canary for
    // `seeded_build_volume_kernel`'s hardcoded GeometryHandleId(1..=4): a definite
    // verdict is only reachable if the seeded bbox replies actually reached the
    // constraint, so a future handle-numbering change fails LOUDLY here.
    let unified_sat = fits_build_volume_satisfaction(&unified);
    let legacy_sat = fits_build_volume_satisfaction(&legacy);
    assert_ne!(
        unified_sat,
        Satisfaction::Indeterminate,
        "UnifiedDag must fold the 4275 single-instance cross-`let` \
         bounding_box(proc.build_volume) leaf to a DEFINITE verdict (legacy_sat={legacy_sat:?}); \
         constraint_results={:?}",
        unified.constraint_results,
    );
    assert_eq!(
        legacy_sat,
        Satisfaction::Indeterminate,
        "LegacyMultiPass must freeze the pre-geometry Indeterminate; constraint_results={:?}",
        legacy.constraint_results,
    );

    // (2) REASONED: the SAME divergence expressed through the per-case allow-list —
    // the gate ADMITS exactly the `FitsBuildVolume` ConstraintFlips (+ the
    // consequent vanished `ConstraintIndeterminate` warning) and nothing else.
    let case = CorpusCase {
        name: "cross_sub_4275_let_bound_definite",
        source: CROSS_LET_4275_SRC,
        needs_stdlib: true,
        allowed: REASONED_4275_BOUNDARY,
        expects_cycle: false,
        kernel: Some(seeded_build_volume_kernel),
    };
    assert_equivalent_or_allowed(&case, &legacy, &unified);
}

// ─────────────────────────────────────────────────────────────────────────────
// step-17 (RED): the remaining §6 rows.
//   (a) `multi_realization_export_equivalent` — a cold build of a ≥2-realization
//       module exports byte-equivalent geometry under both schedulers (+ value /
//       constraint equivalence + residue==∅);
//   (b) `warm_determinacy_predicate_let_is_scheduler_agnostic` — build, then drive
//       a WARM path (`edit_param`) over a determinacy-predicate `let`, and assert
//       the warm result is identical whether the engine was set to Legacy or
//       Unified. `build_scheduler` is read ONLY in cold `build()`
//       (engine_build.rs:2420/3008); `eval_cached` / `edit_param` / `edit_source`
//       / `build_snapshot` do NOT consult it, so warm is scheduler-agnostic.
//   (c) `reserved_warm_auto_plus_const_let_theta` — an `#[ignore]`d θ-reserved row.
//
// θ RE-HOME NOTE: when θ (#4361) routes warm Resolution back-prop
// (`let y = auto_x + N`) through the unified driver, it MUST re-home rows (b)/(c)
// from "scheduler-agnostic regression guard" to "warm == cold" assertions — at
// which point (b) stops being a pure agnosticism guard and (c) becomes a real warm
// back-prop differential.
// ─────────────────────────────────────────────────────────────────────────────

/// (a) RED until step-18: `MULTI_REALIZATION_SRC` is not authored yet, so this
/// fails to compile.
#[test]
fn multi_realization_export_equivalent() {
    let case = CorpusCase {
        name: "multi_realization_export",
        source: MULTI_REALIZATION_SRC,
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    };
    let legacy = build_case(&case, BuildScheduler::LegacyMultiPass);
    let unified = build_case(&case, BuildScheduler::UnifiedDag);

    // value / constraint / diagnostic / geometry equivalence (empty allow-list).
    assert_equivalent_or_allowed(&case, &legacy, &unified);
    // explicit byte-equivalence of the exported bodies.
    assert_eq!(
        legacy.geometry_output,
        unified.geometry_output,
        "multi-realization module: exported bodies MUST be byte-identical across schedulers \
         (legacy_len={:?}, unified_len={:?})",
        legacy.geometry_output.as_ref().map(|b| b.len()),
        unified.geometry_output.as_ref().map(|b| b.len()),
    );
    // Stage-1 residue==∅ (acyclic).
    let (engine, _) = build_case_keep_engine(&case, BuildScheduler::UnifiedDag);
    let residue = residue_for(&engine);
    assert!(
        residue.is_empty(),
        "multi-realization module: Stage-1 residue MUST be ∅ under UnifiedDag; got {} node(s): {residue:?}",
        residue.len(),
    );
}

/// (b) θ (#4361) re-home: warm==cold assertion for the determinacy-predicate `let`.
///
/// Old (ζ): assert that warm `edit_param` re-eval is byte-identical across
/// schedulers (scheduler-agnostic regression guard).
/// New (θ): ALSO assert that the warm result equals the cold build at the same
/// input point (k=5.0).  θ routes warm Resolution back-prop through the unified
/// driver; warm paths now produce the same values as a fresh cold build.
/// Both schedulers are checked: the scheduler-agnostic invariant is PRESERVED and
/// augmented with the warm==cold invariant.
#[test]
fn warm_determinacy_predicate_let_is_scheduler_agnostic() {
    // Warm edit_param results under each scheduler (k: 2.0 → 5.0).
    // Construct each ValueCellId fresh (no Clone assumption on ValueCellId).
    let warm_legacy = warm_eval_after_edit(
        WARM_PREDICATE_SRC,
        BuildScheduler::LegacyMultiPass,
        false,
        ValueCellId::new("WarmPredicate", "k"),
        Value::Real(5.0),
    );
    let warm_unified = warm_eval_after_edit(
        WARM_PREDICATE_SRC,
        BuildScheduler::UnifiedDag,
        false,
        ValueCellId::new("WarmPredicate", "k"),
        Value::Real(5.0),
    );

    // (1) Scheduler-agnostic guard (preserved from ζ): legacy warm == unified warm.
    assert_eq!(
        project_eval_values(&warm_legacy),
        project_eval_values(&warm_unified),
        "warm==warm: edit_param re-eval of determinacy-predicate `let` MUST be \
         scheduler-agnostic (build_scheduler is read only in cold build())",
    );

    // (2) θ warm==cold: both warm results must equal a cold build at k=5.0
    // (WARM_PREDICATE_K5_SRC is WARM_PREDICATE_SRC with the default changed to 5.0).
    let cold_legacy  = build_under(WARM_PREDICATE_K5_SRC, BuildScheduler::LegacyMultiPass, false);
    let cold_unified = build_under(WARM_PREDICATE_K5_SRC, BuildScheduler::UnifiedDag,      false);
    assert_eq!(
        project_eval_values(&warm_legacy),
        project_build_result(&cold_legacy).values,
        "warm==cold (LegacyMultiPass): edit_param(k→5.0) values must match cold build at k=5.0",
    );
    assert_eq!(
        project_eval_values(&warm_unified),
        project_build_result(&cold_unified).values,
        "warm==cold (UnifiedDag): edit_param(k→5.0) values must match cold build at k=5.0",
    );
}

/// (c) θ (#4361) un-ignoring: warm `let y = auto_x + N` back-prop (warm==cold).
///
/// eval_cached's `SolveResult::Solved` arm (engine_eval.rs, θ step-4) is now
/// implemented — it back-props solved autos as Determined and re-evaluates
/// downstream lets.  This row asserts that the warm eval_cached result equals the
/// cold eval() result (warm==cold) under BOTH schedulers.
///
/// Uses WARM_AUTO_CONST_LET_SRC (`param x: Length = auto; constraint x == 10mm;
/// let y = x + 5mm`) + DimensionalSolver via the θ solver warm helper.
#[test]
fn reserved_warm_auto_plus_const_let_theta() {
    // warm == cold under LegacyMultiPass
    let (_, warm_legacy) =
        warm_eval_cached_with_solver(WARM_AUTO_CONST_LET_SRC, BuildScheduler::LegacyMultiPass);
    let cold_legacy = cold_eval_with_solver(WARM_AUTO_CONST_LET_SRC, BuildScheduler::LegacyMultiPass);
    assert_eq!(
        project_eval_values(&warm_legacy.eval_result),
        project_eval_values(&cold_legacy),
        "warm==cold (LegacyMultiPass): eval_cached must match cold eval() for WARM_AUTO_CONST_LET_SRC \
         (x=10mm, y=15mm, both Determined)",
    );

    // warm == cold under UnifiedDag
    let (_, warm_unified) =
        warm_eval_cached_with_solver(WARM_AUTO_CONST_LET_SRC, BuildScheduler::UnifiedDag);
    let cold_unified = cold_eval_with_solver(WARM_AUTO_CONST_LET_SRC, BuildScheduler::UnifiedDag);
    assert_eq!(
        project_eval_values(&warm_unified.eval_result),
        project_eval_values(&cold_unified),
        "warm==cold (UnifiedDag): eval_cached must match cold eval() for WARM_AUTO_CONST_LET_SRC \
         (x=10mm, y=15mm, both Determined)",
    );
}

/// (d) θ (#4361) regression guard: build_snapshot's exported product set equals
/// build()'s under BOTH schedulers.
///
/// `MULTI_ENTITY_EXPORT_SRC` (two `pub structure`s, one `box` each) was the
/// canonical case for the step-1 RED / step-2 GREEN export-bug fix.  This row
/// guards the fix stays correct across schedulers: under both `LegacyMultiPass`
/// and `UnifiedDag`, `build_snapshot` must call `make_compound` with the same
/// arity as `build()` and export the resulting compound handle.
#[test]
fn build_snapshot_multi_entity_export_matches_build() {
    build_snapshot_export_matches_build(
        MULTI_ENTITY_EXPORT_SRC,
        BuildScheduler::LegacyMultiPass,
    );
    build_snapshot_export_matches_build(
        MULTI_ENTITY_EXPORT_SRC,
        BuildScheduler::UnifiedDag,
    );
}
