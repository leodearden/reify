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
    LEX_PARENT_MULTIBODY_SRC, assert_equivalent_or_allowed, build_case, build_case_keep_engine,
    build_with_kernel_stdlib, fits_build_volume_satisfaction, residue_for, seeded_build_volume_kernel,
};
use reify_core::{DiagnosticCode, Severity};
use reify_eval::BuildScheduler;
use reify_ir::Satisfaction;

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
        .filter(|d| {
            d.code == Some(DiagnosticCode::EvalUnresolved) && d.severity == Severity::Error
        })
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
        unresolved.iter().map(|d| d.message.clone()).collect::<Vec<_>>(),
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
        legacy.geometry_output, unified.geometry_output,
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
    // Indeterminate.
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
