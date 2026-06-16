//! Shared, self-contained differential harness for the ζ (task 4359) safety-gate.
//!
//! `#[path]`-included by the two ζ test binaries
//! (`unified_dag_differential_corpus.rs`, `unified_dag_boundary_cases.rs`) so the
//! harness is reused with ZERO edits to the existing `tests/common/mod.rs`
//! (lowest blast radius for a safety-gate landing alongside the sibling
//! unified-dag tasks). It deliberately does NOT live in `common/mod.rs`.
//!
//! Both schedulers are driven through the deterministic
//! `Engine::set_build_scheduler` test seam (a
//! `#[cfg(any(test, feature = "test-instrumentation"))]` setter reached via the
//! self-dev-dep with `test-instrumentation` enabled — see
//! `crates/reify-eval/Cargo.toml`), so the gate runs unconditionally in the
//! default CI build, independent of the `unified-dag` cargo feature and without
//! mutating process env (parallel-safe).
//!
//! Helpers/types are consumed incrementally as the ζ steps land their RED tests;
//! `#![allow(dead_code)]` keeps the partially-wired harness green under
//! `-D warnings` until every item has a caller.
#![allow(dead_code)]

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildResult, BuildScheduler, Engine};
use reify_ir::{ExportFormat, GeometryKernel, Satisfaction};
use reify_test_support::{MockGeometryKernel, compile_source, compile_source_with_stdlib};

// ─────────────────────────────────────────────────────────────────────────────
// Build drivers — a FRESH engine per call (the cold-start `eval()` path runs and
// populates `eval_state.trace_map`, which the residue gate consumes; a second
// build on the same engine would hit `eval_cached`). Lifted verbatim from the
// δ/ε pattern (tests/unified_dag_geometry_executors.rs:50-85,
// tests/unified_dag_cycle_contract.rs:35-48).
// ─────────────────────────────────────────────────────────────────────────────

/// Compile `source` (through the stdlib prelude iff `needs_stdlib`), build it on
/// a FRESH engine under `scheduler` with a default (unseeded)
/// [`MockGeometryKernel`], and return the full [`BuildResult`]. The corpus-sweep
/// driver — used by every gate that does not need seeded kernel replies.
pub fn build_under(source: &str, scheduler: BuildScheduler, needs_stdlib: bool) -> BuildResult {
    let kernel = Box::new(MockGeometryKernel::new());
    if needs_stdlib {
        build_with_kernel_stdlib(source, scheduler, kernel)
    } else {
        build_with_kernel(source, scheduler, kernel)
    }
}

/// Compile `source` (NO stdlib prelude), build it on a FRESH engine under
/// `scheduler` with the supplied `kernel`, and return the full [`BuildResult`].
/// For boundary cases that seed `with_bbox_result` / `with_volume_result` replies
/// and use only core geometry builtins (`box` / `edges_at_height` / `fillet`).
pub fn build_with_kernel(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

/// Like [`build_with_kernel`] but compiles `source` through the stdlib prelude
/// ([`compile_source_with_stdlib`]) so prelude names — DFM builtins
/// (`fits_build_volume`), geometry types (`Solid`), user `constraint def`s —
/// resolve. The geometry-backed-constraint boundary cases (4275, auto+geometry)
/// need this because `fits_build_volume` lives in the `std.process` prelude.
pub fn build_with_kernel_stdlib(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source_with_stdlib(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

// ─────────────────────────────────────────────────────────────────────────────
// Corpus data types + the reasoned, PER-CASE allow-list (no blanket patterns).
// ─────────────────────────────────────────────────────────────────────────────

/// One corpus entry: a `.ri` source plus the PER-CASE, REASONED allow-list of
/// the legitimate legacy-vs-unified divergences it is permitted to exhibit.
pub struct CorpusCase {
    /// Stable human label, surfaced in every panic message.
    pub name: &'static str,
    /// The `.ri` program built under both schedulers.
    pub source: &'static str,
    /// Compile through the stdlib prelude (`fits_build_volume`, `Solid`, …).
    pub needs_stdlib: bool,
    /// The reasoned divergences this case is permitted to exhibit. EMPTY ⇒ the
    /// two projections must be byte-equal. Every entry MUST match a real diff
    /// item (a stale/blanket entry fails the gate) and every diff item MUST
    /// match an entry (an unreasoned divergence is a real ε defect → fails the
    /// gate, then escalate `design_concern` — never blanket-allow).
    pub allowed: &'static [Divergence],
    /// True iff the source contains a genuine eval cycle (so the residue==∅ and
    /// no-`EvalCycle` gates are skipped for it).
    pub expects_cycle: bool,
}

/// One reasoned, per-case divergence between the LegacyMultiPass and UnifiedDag
/// projections. Each variant matches a SPECIFIC diff item (never a blanket
/// pattern) and carries a human `reason`.
pub enum Divergence {
    /// A constraint's verdict changed (e.g. Indeterminate→Satisfied/Violated).
    /// Matched by a substring of the constraint's id/label.
    ConstraintFlips {
        constraint: &'static str,
        reason: &'static str,
    },
    /// A diagnostic carrying `code` is present under UnifiedDag but not legacy
    /// (e.g. `EvalCycle` / `EvalUnresolved`). Matched by exact `DiagnosticCode`.
    DiagnosticAdded {
        code: DiagnosticCode,
        reason: &'static str,
    },
    /// A value cell resolved differently (e.g. Undef→definite). Matched by a
    /// substring of the cell id (`Entity.member`).
    ValueResolves {
        cell_substr: &'static str,
        reason: &'static str,
    },
    /// The exported geometry bytes differ. A scoped per-case flag.
    GeometryDiffers { reason: &'static str },
}

/// Deterministic canonical projection of a [`BuildResult`] over the
/// scheduler-overlap comparison surface. Structural equality of two projections
/// IS the equivalence relation the gate asserts.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedBuildResult {
    /// `values` sorted by `ValueCellId`, each rendered to a stable
    /// `(Entity.member, Debug(value))` pair.
    pub values: Vec<(String, String)>,
    /// `constraint_results` sorted by constraint id, as
    /// `(id, label, satisfaction)`.
    pub constraint_results: Vec<(String, Option<String>, Satisfaction)>,
    /// `diagnostics` in emission order, as `(code, message, severity)` triples.
    pub diagnostics: Vec<(Option<DiagnosticCode>, String, Severity)>,
    /// The raw exported geometry bytes.
    pub geometry_output: Option<Vec<u8>>,
    /// `resolved_params` sorted by cell, as `(Entity.member, Debug(value))`.
    pub resolved_params: Vec<(String, String)>,
}

/// Project a [`BuildResult`] onto its deterministic canonical
/// [`ProjectedBuildResult`] over the scheduler-overlap comparison surface.
///
/// Determinism is the whole point — every field that rides a `HashMap`/persistent
/// map is rendered to a sorted `Vec` so iteration order can never leak into the
/// comparison:
/// - `values` / `resolved_params`: sorted by `ValueCellId` (Display `Entity.member`),
///   each value rendered with a stable `{:?}`;
/// - `constraint_results`: sorted by constraint-id Display then label, as
///   `(id, label, satisfaction)`;
/// - `diagnostics`: kept in EMISSION order (the δ driver pins one total order for
///   its diagnostic vector — see `engine_fixpoint::run_unified_pass` docs — so the
///   order itself is part of the contract and must NOT be re-sorted);
/// - `geometry_output`: the raw bytes verbatim.
///
/// Structural equality of two projections IS the equivalence relation the ζ gate
/// asserts.
pub fn project_build_result(result: &BuildResult) -> ProjectedBuildResult {
    // `values` — sorted by `ValueCellId`, each as `(Entity.member, Debug(value))`.
    let mut values: Vec<(String, String)> = result
        .values
        .iter()
        .map(|(id, v)| (id.to_string(), format!("{v:?}")))
        .collect();
    values.sort();

    // `constraint_results` — sorted by constraint-id Display, then label, as
    // `(id, label, satisfaction)`. `sort_by` (not `sort`) so we need no `Ord` on
    // `Satisfaction`; the (id, label) key is the deterministic discriminator.
    let mut constraint_results: Vec<(String, Option<String>, Satisfaction)> = result
        .constraint_results
        .iter()
        .map(|e| (e.id.to_string(), e.label.clone(), e.satisfaction))
        .collect();
    constraint_results.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // `diagnostics` — EMISSION order (NOT sorted): the unified driver guarantees a
    // single total order for its diagnostic vector, and the δ cycle-contract test
    // already asserts that order is byte-preserving vs legacy on acyclic modules.
    let diagnostics: Vec<(Option<DiagnosticCode>, String, Severity)> = result
        .diagnostics
        .iter()
        .map(|d| (d.code, d.message.clone(), d.severity))
        .collect();

    let geometry_output = result.geometry_output.clone();

    // `resolved_params` — sorted by cell, as `(Entity.member, Debug(value))`.
    let mut resolved_params: Vec<(String, String)> = result
        .resolved_params
        .iter()
        .map(|(id, v)| (id.to_string(), format!("{v:?}")))
        .collect();
    resolved_params.sort();

    ProjectedBuildResult {
        values,
        constraint_results,
        diagnostics,
        geometry_output,
        resolved_params,
    }
}

/// Assert UnifiedDag is equivalent to LegacyMultiPass for `case`, admitting ONLY
/// the per-case reasoned divergences in `case.allowed`.
///
/// step-2 implements the EMPTY-allow-list path: plain projection equality with a
/// rich panic diff. step-4 extends this with the structured per-[`Divergence`]
/// matcher that admits a reasoned non-empty allow-list while rejecting unused
/// allow entries and unreasoned diff items (a real ε defect → escalate
/// `design_concern`, never blanket-allow).
pub fn assert_equivalent_or_allowed(case: &CorpusCase, legacy: &BuildResult, unified: &BuildResult) {
    let projected_legacy = project_build_result(legacy);
    let projected_unified = project_build_result(unified);

    assert_eq!(
        projected_unified, projected_legacy,
        "case `{}`: UnifiedDag projection diverged from LegacyMultiPass with no \
         reasoned allow-list to admit it.\n  legacy  = {projected_legacy:#?}\n  \
         unified = {projected_unified:#?}",
        case.name,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// SEED corpus — plainly-equivalent programs (empty allow-lists). Expanded with
// the `tests/golden` idioms + language-breadth entries at step-10.
// ─────────────────────────────────────────────────────────────────────────────

/// Plainly-equivalent seed programs: every one must project byte-equal under
/// both schedulers (empty `allowed`), carry empty residue, and be 2×
/// byte-identical. The box primitive / boolean union / two-sub assembly are
/// lifted from `unified_dag_cycle_contract.rs`'s acyclic source and the
/// `let r = union(a, b)` idiom proven at `multi_handle_engine_dispatch.rs:442`.
pub const SEED_CORPUS: &[CorpusCase] = &[
    CorpusCase {
        name: "box_primitive",
        source: "pub structure S {\n    let part = box(10mm, 10mm, 10mm)\n}",
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
    },
    CorpusCase {
        name: "boolean_union",
        source: "structure S {\n    let a = box(10mm, 10mm, 10mm)\n    let b = box(5mm, 5mm, 5mm)\n    let r = union(a, b)\n}",
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
    },
    CorpusCase {
        name: "two_sub_assembly",
        source: "pub structure A {\n    let part = box(10mm, 10mm, 10mm)\n}\npub structure B {\n    let part = box(5mm, 5mm, 5mm)\n}\npub structure C {\n    sub a = A()\n    sub b = B()\n    let result = union(self.a.part, self.b.part)\n}",
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
    },
];
