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

use differential::{SEED_CORPUS, build_under, project_build_result};
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
