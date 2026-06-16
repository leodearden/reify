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
    AUTO_GEOMETRY_CONSTRAINT_SRC, CorpusCase, LEX_PARENT_MULTIBODY_SRC,
    assert_equivalent_or_allowed, build_case, build_case_keep_engine, build_with_kernel_stdlib,
    residue_for, seeded_build_volume_kernel,
};
use reify_core::{DiagnosticCode, Severity};
use reify_eval::BuildScheduler;

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
