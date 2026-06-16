//! ζ (task 4359) differential safety-gate — the corpus SWEEP.
//!
//! Builds a representative corpus under BOTH `BuildScheduler::{LegacyMultiPass,
//! UnifiedDag}` and asserts `BuildResult` equivalence on the overlap domain,
//! gated by a per-case REASONED allow-list (`assert_equivalent_or_allowed`). It
//! also runs unified 2× → byte-identical (determinism) and adds the Stage-1
//! `residue == ∅` gate on every acyclic legacy-passing case (PRD
//! `docs/prds/v0_6/engine-unified-build-dag.md` §6, decomposition §8-ζ).
//!
//! The §6 boundary cases a plain legacy-vs-unified diff cannot surface live in
//! the sibling binary `unified_dag_boundary_cases.rs`.
//!
//! The shared harness is `#[path]`-included (NOT via `tests/common/mod.rs`) so
//! this safety-gate lands with zero edits to existing shared test files.

#[path = "common/differential.rs"]
mod differential;

use differential::{
    CROSS_LET_4275_SRC, CorpusCase, Divergence, SEED_CORPUS, assert_equivalent_or_allowed,
    build_under, build_with_kernel_stdlib, project_build_result, seeded_build_volume_kernel,
};
use reify_core::DiagnosticCode;
use reify_eval::BuildScheduler;

// ─────────────────────────────────────────────────────────────────────────────
// step-1 (RED): the core ζ contract — on the equivalence overlap, UnifiedDag's
// BuildResult projection is byte-equal to LegacyMultiPass's.
// ─────────────────────────────────────────────────────────────────────────────

/// PRIMARY (must-pass) — iterate the SEED corpus (plainly-equivalent programs,
/// EMPTY allow-lists), build each under BOTH `LegacyMultiPass` and `UnifiedDag`
/// on fresh engines, and assert the two canonical projections are equal. This
/// pins the core equivalence guarantee the ι default-flip relies on: on the
/// scheduler-overlap domain, the unified driver is observationally identical to
/// the legacy multi-pass build.
///
/// RED until step-2: `project_build_result` is not implemented yet (the type
/// exists but the projection fn does not), so this fails to compile.
#[test]
fn differential_corpus_is_equivalent_on_overlap() {
    for case in SEED_CORPUS {
        assert!(
            case.allowed.is_empty(),
            "seed case `{}` must carry an EMPTY allow-list (it is plainly equivalent); \
             reasoned-divergence cases belong in their own steps",
            case.name,
        );
        let legacy = build_under(case.source, BuildScheduler::LegacyMultiPass, case.needs_stdlib);
        let unified = build_under(case.source, BuildScheduler::UnifiedDag, case.needs_stdlib);
        assert_eq!(
            project_build_result(&unified),
            project_build_result(&legacy),
            "UnifiedDag must be projection-equivalent to LegacyMultiPass on the \
             equivalence-overlap seed case `{}`",
            case.name,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3 (RED): the reasoned, PER-CASE allow-list admits ONLY a divergence that
// is matched by a SPECIFIC reasoned entry — never a blanket toggle, never an
// unused entry. This is what makes the corpus a trustworthy terminal oracle for
// the ι default-flip: an UNREASONED divergence is a real ε defect (hard failure
// → escalate), and a STALE entry that no longer matches anything is dead cover
// (also a hard failure).
// ─────────────────────────────────────────────────────────────────────────────

/// Human reasons for the 4275 single-instance cross-`let` divergences, shared by
/// the reasoned (a) and stale (c) allow-lists below so the wording stays in lockstep.
const R_FLIP: &str = "4275 single-instance `let proc = FdmPrinter()`: UnifiedDag folds the \
     cross-`let` bounding_box(proc.build_volume) leaf post-geometry (PRD §3.3) → a DEFINITE \
     verdict; LegacyMultiPass freezes the pre-geometry Indeterminate";
const R_DIAG: &str = "the legacy-only `constraint … indeterminate` warning vanishes once \
     UnifiedDag resolves the constraint to a definite verdict (a consequence of the flip, not \
     an independent divergence)";

/// The two REASONED divergences the 4275 single-instance case legitimately exhibits.
const REASONED_4275: &[Divergence] = &[
    Divergence::ConstraintFlips { constraint: "FitsBuildVolume", reason: R_FLIP },
    Divergence::DiagnosticAdded { code: DiagnosticCode::ConstraintIndeterminate, reason: R_DIAG },
];

/// The two real reasoned entries PLUS a bogus `GeometryDiffers` that matches NO
/// diff item (the 4275 geometry_output is byte-equal under both schedulers) — so
/// the ONLY reason this allow-list fails the gate is the stale/unused entry.
const STALE_4275: &[Divergence] = &[
    Divergence::ConstraintFlips { constraint: "FitsBuildVolume", reason: R_FLIP },
    Divergence::DiagnosticAdded { code: DiagnosticCode::ConstraintIndeterminate, reason: R_DIAG },
    Divergence::GeometryDiffers {
        reason: "no geometry divergence exists for the 4275 case — a stale/unused allow entry \
                 must be rejected, never left as dead cover",
    },
];

fn case_4275(name: &'static str, allowed: &'static [Divergence]) -> CorpusCase {
    CorpusCase { name, source: CROSS_LET_4275_SRC, needs_stdlib: true, allowed, expects_cycle: false }
}

/// RED until step-4: `assert_equivalent_or_allowed` today (step-2) is plain
/// projection equality that IGNORES `case.allowed`, so sub-test (a) — a genuinely
/// divergent case that SHOULD be admitted by its reasoned allow-list — panics. The
/// structured per-[`Divergence`] matcher (step-4) makes (a) pass while keeping (b)
/// and (c) rejecting.
///
/// The 4275 single-instance cross-`let` form diverges in exactly TWO reasoned
/// ways: (1) the `FitsBuildVolume` constraint flips Indeterminate (legacy) →
/// DEFINITE (unified); (2) the legacy-only `ConstraintIndeterminate` warning
/// disappears under unified. The `SmallPart.proc` StructureInstance value is
/// IDENTICAL under both schedulers — only its inner field-map iteration order
/// differs in a `{:?}` render — so the canonical (content-hash) projection
/// step-4 lands collapses it to NON-divergent (it must NOT need an allow entry).
#[test]
fn allow_list_admits_only_reasoned_divergence() {
    // One build pair, reused across (a)/(b)/(c) — each differs only in `allowed`.
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

    // (a) the two REASONED divergences are admitted → MUST NOT panic.
    assert_equivalent_or_allowed(&case_4275("cross_let_4275_reasoned", REASONED_4275), &legacy, &unified);

    // (b) the SAME divergence with an EMPTY allow-list is REJECTED — an unreasoned
    // divergence is never silently accepted.
    let caught_empty = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_equivalent_or_allowed(&case_4275("cross_let_4275_empty", &[]), &legacy, &unified);
    }));
    assert!(
        caught_empty.is_err(),
        "an EMPTY allow-list MUST reject the 4275 divergence — an unreasoned divergence is a \
         real ε defect, never silently accepted",
    );

    // (c) an allow-list whose extra `GeometryDiffers` entry matches NO diff item is
    // REJECTED — stale/blanket entries are dead cover.
    let caught_stale = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_equivalent_or_allowed(&case_4275("cross_let_4275_stale", STALE_4275), &legacy, &unified);
    }));
    assert!(
        caught_stale.is_err(),
        "an allow entry that matches no diff item (the stale `GeometryDiffers`) MUST be rejected \
         — the committed allow-list stays honest as ε evolves",
    );
}
