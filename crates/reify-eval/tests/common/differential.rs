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
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::{BuildResult, BuildScheduler, Engine, EvalResult};
use reify_ir::{ExportFormat, GeometryHandleId, GeometryKernel, Satisfaction, Value};
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

/// Like [`build_under`] but RETAINS the engine so its post-build
/// [`Engine::eval_state`] — the `snapshot.graph` + `trace_map` the residue gate
/// re-plans over — stays observable. A FRESH engine per call (the cold-start
/// `eval()` runs and populates `trace_map`; a second build on the same engine
/// would hit `eval_cached`). Returns `(engine, result)`. The residue gate
/// (`residue_for`) consumes the returned engine.
pub fn build_under_keep_engine(
    source: &str,
    scheduler: BuildScheduler,
    needs_stdlib: bool,
) -> (Engine, BuildResult) {
    let compiled = if needs_stdlib {
        compile_source_with_stdlib(source)
    } else {
        compile_source(source)
    };
    let kernel: Box<dyn GeometryKernel> = Box::new(MockGeometryKernel::new());
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    let result = engine.build(&compiled, ExportFormat::Step);
    (engine, result)
}

/// Build a [`CorpusCase`] under `scheduler`, honoring its optional seeded
/// `kernel`: `None` ⇒ the default unseeded [`build_under`]; `Some(f)` ⇒ a FRESH
/// `f()` kernel (so geometry queries resolve identically under both schedulers).
/// The single corpus-sweep entry point — every gate routes through it so a case's
/// kernel seeding is applied uniformly.
pub fn build_case(case: &CorpusCase, scheduler: BuildScheduler) -> BuildResult {
    match case.kernel {
        Some(make) => {
            if case.needs_stdlib {
                build_with_kernel_stdlib(case.source, scheduler, make())
            } else {
                build_with_kernel(case.source, scheduler, make())
            }
        }
        None => build_under(case.source, scheduler, case.needs_stdlib),
    }
}

/// Like [`build_case`] but RETAINS the engine for the residue gate (mirrors
/// [`build_under_keep_engine`], honoring the case's optional seeded `kernel`).
pub fn build_case_keep_engine(case: &CorpusCase, scheduler: BuildScheduler) -> (Engine, BuildResult) {
    let compiled = if case.needs_stdlib {
        compile_source_with_stdlib(case.source)
    } else {
        compile_source(case.source)
    };
    let kernel: Box<dyn GeometryKernel> = match case.kernel {
        Some(make) => make(),
        None => Box::new(MockGeometryKernel::new()),
    };
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    let result = engine.build(&compiled, ExportFormat::Step);
    (engine, result)
}

/// The Stage-1 residue set, observed DIRECTLY: re-run the pure unified planner
/// (`run_unified_pass`) over the engine's post-build `eval_state`
/// (`snapshot.graph` + `trace_map`) and return its `residue` — the node-set
/// members the Kahn worklist never popped (cyclic nodes plus anything stranded
/// downstream of a cycle).
///
/// Re-running the pure planner POST-build is sound: the realization loop mutates
/// only node `produced_*` fields, never graph topology or `trace_map` (see
/// `engine_build.rs:2413`), so this residue equals the one the build's own
/// Stage-1 pass computed. A diagnostics-only check cannot substitute — a
/// stranded-without-SCC node is left Undef and emits NO diagnostic yet IS
/// residue, exactly the false-negative this direct observation closes.
///
/// `engine` MUST be a freshly cold-built engine (use [`build_under_keep_engine`]);
/// `eval_state()` is `None` until the first `eval()`/`build()` populates it.
pub fn residue_for(engine: &Engine) -> std::collections::HashSet<reify_eval::cache::NodeId> {
    let state = engine
        .eval_state()
        .expect("eval_state is populated after a cold build()");
    reify_eval::engine_fixpoint::run_unified_pass(&state.snapshot.graph, &state.trace_map).residue
}

/// Locate the single `FitsBuildVolume` constraint entry's [`Satisfaction`] in a
/// [`BuildResult`] (the stdlib def's instantiation is labelled
/// `"FitsBuildVolume#0[0]"`). The 4275 boundary differential (step-15/16) reads
/// this DIRECTLY to assert UnifiedDag is DEFINITE while legacy is Indeterminate —
/// a clarity companion to the `assert_equivalent_or_allowed` reasoned check.
/// Lifted from `tests/unified_dag_geometry_executors.rs:728`. Panics with the full
/// constraint list if no such entry is present.
pub fn fits_build_volume_satisfaction(result: &BuildResult) -> Satisfaction {
    result
        .constraint_results
        .iter()
        .find(|e| {
            e.label
                .as_deref()
                .is_some_and(|l| l.contains("FitsBuildVolume"))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a FitsBuildVolume constraint result, got: {:?}",
                result.constraint_results
            )
        })
        .satisfaction
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
    /// Optional seeded-kernel factory. `None` ⇒ a default unseeded
    /// [`MockGeometryKernel`]. `Some(f)` ⇒ `f()` builds a FRESH seeded kernel per
    /// build (each build consumes its kernel), so geometry QUERIES
    /// (`volume`/`centroid`/`bbox`) reach results under BOTH schedulers — making
    /// the case a fair equivalence test rather than a comparison of two distinct
    /// unseeded-mock FAILURE modes (which is itself a scheduler-ordering artifact,
    /// not a semantic divergence — see `seeded_physical_kernel`).
    pub kernel: Option<fn() -> Box<dyn GeometryKernel>>,
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

/// One projected value cell: the cell id (the matcher's key + sort key) plus a
/// canonical, order-independent, type-discriminating equality fingerprint, and a
/// readable `Display` render (the latter is deliberately NOT part of equality).
#[derive(Debug, Clone)]
pub struct ProjectedValue {
    /// `ValueCellId` Display (`Entity.member`) — the matcher key and sort key.
    pub cell: String,
    /// `content_hash` hex — the canonical equality fingerprint. It SORTS
    /// struct/map fields, DOMAIN-SEPARATES by type tag (so `Int(1)` ≠ `Real(1.0)`),
    /// and EXCLUDES per-Engine ephemeral handles (`kernel_handle`) and the
    /// `StructureInstance.type_id` — so two semantically identical values compare
    /// equal regardless of `PersistentMap`/`HashMap` iteration order or which
    /// scheduler produced them.
    ///
    /// A raw `{:?}` render (step-2's first cut) is NEITHER canonical —
    /// `StructureInstanceData.fields` is a `PersistentMap` whose Debug iteration
    /// order leaks — NOR handle-stable (`GeometryHandle` Debug bakes in the
    /// ephemeral `kernel_handle`), so it cannot be the equality key for a safety
    /// gate. `content_hash` is exactly the cross-Engine-stable identity Reify's
    /// own incremental cache keys on, so it is the right canonical form here.
    pub canonical: String,
    /// Human-readable `Display` render — surfaced in diffs only, deliberately NOT
    /// compared (Display is lossy: `Int(1)` and `Real(1.0)` both render "1").
    pub display: String,
}

/// Equality is the canonical fingerprint at a given cell — the readable `display`
/// render is excluded so a lossy/non-canonical render can never corrupt the gate.
impl PartialEq for ProjectedValue {
    fn eq(&self, other: &Self) -> bool {
        self.cell == other.cell && self.canonical == other.canonical
    }
}

/// Deterministic canonical projection of a [`BuildResult`] over the
/// scheduler-overlap comparison surface. Structural equality of two projections
/// IS the equivalence relation the gate asserts.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedBuildResult {
    /// `values` sorted by `ValueCellId`, each a canonical [`ProjectedValue`].
    pub values: Vec<ProjectedValue>,
    /// `constraint_results` sorted by constraint id, as
    /// `(id, label, satisfaction)`.
    pub constraint_results: Vec<(String, Option<String>, Satisfaction)>,
    /// `diagnostics` in emission order, as `(code, message, severity)` triples.
    pub diagnostics: Vec<(Option<DiagnosticCode>, String, Severity)>,
    /// The raw exported geometry bytes.
    pub geometry_output: Option<Vec<u8>>,
    /// `resolved_params` sorted by cell, each a canonical [`ProjectedValue`].
    pub resolved_params: Vec<ProjectedValue>,
}

/// Project one `(id, value)` cell to its canonical [`ProjectedValue`].
fn project_value(id: impl std::fmt::Display, v: &Value) -> ProjectedValue {
    ProjectedValue {
        cell: id.to_string(),
        // `ContentHash(pub u128)` — 32 hex digits is the full fingerprint.
        canonical: format!("{:032x}", v.content_hash().0),
        display: format!("{v}"),
    }
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
    // `values` — sorted by `ValueCellId`, each a canonical [`ProjectedValue`].
    let mut values: Vec<ProjectedValue> = result
        .values
        .iter()
        .map(|(id, v)| project_value(id, v))
        .collect();
    values.sort_by(|a, b| a.cell.cmp(&b.cell).then_with(|| a.canonical.cmp(&b.canonical)));

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

    // `resolved_params` — sorted by cell, each a canonical [`ProjectedValue`].
    let mut resolved_params: Vec<ProjectedValue> = result
        .resolved_params
        .iter()
        .map(|(id, v)| project_value(id, v))
        .collect();
    resolved_params.sort_by(|a, b| a.cell.cmp(&b.cell).then_with(|| a.canonical.cmp(&b.canonical)));

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
/// Computes the field-wise diff between the two canonical projections, then
/// requires a TWO-WAY match against the reasoned allow-list:
///   * every diff item MUST be matched by a SPECIFIC [`Divergence`] (constraint
///     flips by id/label substring; one-sided diagnostics by exact code; value
///     resolves by cell substring; geometry by a scoped flag). An UNMATCHED diff
///     item is an unreasoned divergence — a real ε defect → the gate fails hard
///     (escalate `design_concern`, NEVER blanket-allow);
///   * every allow entry MUST match ≥1 diff item. An entry matching nothing is
///     stale/dead cover → the gate fails hard (keeps the committed list honest as
///     ε evolves).
///
/// With an EMPTY allow-list this reduces to "any divergence fails" — the
/// plainly-equivalent path the SEED sweep relies on.
pub fn assert_equivalent_or_allowed(case: &CorpusCase, legacy: &BuildResult, unified: &BuildResult) {
    use std::collections::BTreeMap;

    let pl = project_build_result(legacy);
    let pu = project_build_result(unified);
    let allowed = case.allowed;
    let mut used = vec![false; allowed.len()];
    let mut unmatched: Vec<String> = Vec::new();

    // (1) constraint-verdict diffs, keyed by (id, label) ← ConstraintFlips.
    let by_constraint = |p: &ProjectedBuildResult| -> BTreeMap<(String, Option<String>), Satisfaction> {
        p.constraint_results
            .iter()
            .cloned()
            .map(|(id, label, sat)| ((id, label), sat))
            .collect()
    };
    let cl = by_constraint(&pl);
    let cu = by_constraint(&pu);
    let mut ckeys: Vec<&(String, Option<String>)> = cl.keys().chain(cu.keys()).collect();
    ckeys.sort();
    ckeys.dedup();
    for k in ckeys {
        let (a, b) = (cl.get(k).copied(), cu.get(k).copied());
        if a == b {
            continue;
        }
        let (id, label) = k;
        let mut matched = false;
        for (i, d) in allowed.iter().enumerate() {
            if let Divergence::ConstraintFlips { constraint, .. } = d {
                if id.contains(constraint) || label.as_deref().is_some_and(|l| l.contains(constraint)) {
                    used[i] = true;
                    matched = true;
                }
            }
        }
        if !matched {
            unmatched.push(format!(
                "constraint `{id}`{}: {a:?} (legacy) → {b:?} (unified)",
                label.as_deref().map(|l| format!(" [{l}]")).unwrap_or_default(),
            ));
        }
    }

    // (2) diagnostics present on exactly one side (order-independent multiset
    // diff) ← DiagnosticAdded, matched by exact code (either direction).
    let one_sided = |a: &[(Option<DiagnosticCode>, String, Severity)],
                     b: &[(Option<DiagnosticCode>, String, Severity)]|
     -> Vec<(Option<DiagnosticCode>, String, Severity)> {
        let mut remaining = b.to_vec();
        let mut out = Vec::new();
        for item in a {
            if let Some(pos) = remaining.iter().position(|x| x == item) {
                remaining.remove(pos);
            } else {
                out.push(item.clone());
            }
        }
        out
    };
    let legacy_only = one_sided(&pl.diagnostics, &pu.diagnostics);
    let unified_only = one_sided(&pu.diagnostics, &pl.diagnostics);
    for (side, (code, msg, sev)) in legacy_only
        .iter()
        .map(|d| ("legacy-only", d))
        .chain(unified_only.iter().map(|d| ("unified-only", d)))
    {
        let mut matched = false;
        for (i, d) in allowed.iter().enumerate() {
            if let Divergence::DiagnosticAdded { code: ac, .. } = d {
                if Some(*ac) == *code {
                    used[i] = true;
                    matched = true;
                }
            }
        }
        if !matched {
            unmatched.push(format!("{side} diagnostic code={code:?} sev={sev:?}: {msg}"));
        }
    }

    // (3) value-cell diffs (canonical differs, or present on one side) ←
    // ValueResolves, matched by cell substring. `values` and `resolved_params`
    // share the bucket (both keyed by cell id).
    let by_cell = |vs: &[ProjectedValue]| -> BTreeMap<String, ProjectedValue> {
        vs.iter().map(|p| (p.cell.clone(), p.clone())).collect()
    };
    for (tag, lvs, uvs) in [
        ("", &pl.values, &pu.values),
        ("(param) ", &pl.resolved_params, &pu.resolved_params),
    ] {
        let (vl, vu) = (by_cell(lvs), by_cell(uvs));
        let mut cells: Vec<&String> = vl.keys().chain(vu.keys()).collect();
        cells.sort();
        cells.dedup();
        for c in cells {
            let (a, b) = (vl.get(c), vu.get(c));
            let differ = match (a, b) {
                (Some(x), Some(y)) => x.canonical != y.canonical,
                _ => true,
            };
            if !differ {
                continue;
            }
            let mut matched = false;
            for (i, d) in allowed.iter().enumerate() {
                if let Divergence::ValueResolves { cell_substr, .. } = d {
                    if c.contains(cell_substr) {
                        used[i] = true;
                        matched = true;
                    }
                }
            }
            if !matched {
                unmatched.push(format!(
                    "value `{tag}{c}`: {:?} (legacy) → {:?} (unified)",
                    a.map(|p| &p.display),
                    b.map(|p| &p.display),
                ));
            }
        }
    }

    // (4) exported geometry bytes differ ← GeometryDiffers.
    if pl.geometry_output != pu.geometry_output {
        let mut matched = false;
        for (i, d) in allowed.iter().enumerate() {
            if matches!(d, Divergence::GeometryDiffers { .. }) {
                used[i] = true;
                matched = true;
            }
        }
        if !matched {
            unmatched.push(format!(
                "geometry_output bytes differ: legacy={:?} bytes, unified={:?} bytes",
                pl.geometry_output.as_ref().map(|b| b.len()),
                pu.geometry_output.as_ref().map(|b| b.len()),
            ));
        }
    }

    // Stale/unused allow entries: an entry that matched NO diff item.
    let unused: Vec<String> = allowed
        .iter()
        .zip(used.iter())
        .filter(|(_, u)| !**u)
        .map(|(d, _)| describe_divergence(d))
        .collect();

    if unmatched.is_empty() && unused.is_empty() {
        return;
    }

    let fmt_list = |items: &[String]| -> String {
        if items.is_empty() {
            "    (none)".to_string()
        } else {
            items.iter().map(|s| format!("    • {s}")).collect::<Vec<_>>().join("\n")
        }
    };
    panic!(
        "case `{}`: differential gate FAILED.\n\
         {} UNREASONED divergence(s) — a real ε defect → escalate `design_concern`, \
         NEVER blanket-allow:\n{}\n\
         {} STALE/UNUSED allow entr(y/ies) — matched no diff item, remove or fix:\n{}",
        case.name,
        unmatched.len(),
        fmt_list(&unmatched),
        unused.len(),
        fmt_list(&unused),
    );
}

/// Assert `UnifiedDag` is DETERMINISTIC for `case`: two independent builds (fresh
/// engines) produce byte-for-byte identical exported geometry AND an identical
/// canonical projection.
///
/// The raw `geometry_output` byte check is the strong δ guarantee (realization
/// order, hence exported bytes, never drifts run-to-run). The projection check
/// additionally pins diagnostic EMISSION order and constraint-result order (both
/// compared as ordered `Vec`s), so a worklist-order regression that reordered
/// diagnostics — but happened to leave geometry bytes unchanged — would still be
/// caught.
pub fn assert_unified_byte_identical(case: &CorpusCase) {
    let first = build_case(case, BuildScheduler::UnifiedDag);
    let second = build_case(case, BuildScheduler::UnifiedDag);

    assert_eq!(
        first.geometry_output, second.geometry_output,
        "case `{}`: UnifiedDag exported geometry is NOT byte-identical across two \
         independent builds — a determinism regression (the worklist pop order must \
         be total + stable). legacy_len={:?} second_len={:?}",
        case.name,
        first.geometry_output.as_ref().map(|b| b.len()),
        second.geometry_output.as_ref().map(|b| b.len()),
    );
    assert_eq!(
        project_build_result(&first),
        project_build_result(&second),
        "case `{}`: UnifiedDag canonical projection is NOT identical across two \
         independent builds — a determinism regression in values/constraints/diagnostics",
        case.name,
    );
}

/// Render a [`Divergence`] for the stale-entry panic list.
fn describe_divergence(d: &Divergence) -> String {
    match d {
        Divergence::ConstraintFlips { constraint, reason } => {
            format!("ConstraintFlips {{ constraint: {constraint:?}, reason: {reason:?} }}")
        }
        Divergence::DiagnosticAdded { code, reason } => {
            format!("DiagnosticAdded {{ code: {code:?}, reason: {reason:?} }}")
        }
        Divergence::ValueResolves { cell_substr, reason } => {
            format!("ValueResolves {{ cell_substr: {cell_substr:?}, reason: {reason:?} }}")
        }
        Divergence::GeometryDiffers { reason } => {
            format!("GeometryDiffers {{ reason: {reason:?} }}")
        }
    }
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
        kernel: None,
    },
    CorpusCase {
        name: "boolean_union",
        source: "structure S {\n    let a = box(10mm, 10mm, 10mm)\n    let b = box(5mm, 5mm, 5mm)\n    let r = union(a, b)\n}",
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "two_sub_assembly",
        source: "pub structure A {\n    let part = box(10mm, 10mm, 10mm)\n}\npub structure B {\n    let part = box(5mm, 5mm, 5mm)\n}\npub structure C {\n    sub a = A()\n    sub b = B()\n    let result = union(self.a.part, self.b.part)\n}",
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
];

// ─────────────────────────────────────────────────────────────────────────────
// GOLDEN corpus — the five committed `tests/golden` source idioms PLUS a handful
// of language-breadth entries, each lifted verbatim from a committed `examples/`
// file via `include_str!` (so the corpus tracks the real, regression-locked user
// programs, not a hand-rolled paraphrase). Discharges the "+ tests/golden corpus"
// clause of §8-ζ.
//
// `include_str!` resolves relative to THIS file, but the `#[path]`-include leaves
// that ambiguous; `concat!(env!("CARGO_MANIFEST_DIR"), …)` pins the path to the
// crate manifest dir (`crates/reify-eval`) unambiguously regardless of where the
// module is textually included.
//
// Every entry starts with an EMPTY allow-list: these are user-facing programs the
// unified driver MUST reproduce byte-for-byte. A divergence that shows up here is
// a real ε defect surfaced by the gate → fix/escalate, never blanket-allow.
// ─────────────────────────────────────────────────────────────────────────────

/// `examples/structure-instance.ri` — SIR-α: flat + nested structure-instance
/// construction (Steel/PointLoad/Beam/NestedAssembly). Pure value evaluation, no
/// geometry. (golden: `tests/golden/structure_instance.txt`.)
const SRC_STRUCTURE_INSTANCE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/structure-instance.ri"));

/// `examples/tensegrity_t_prism.ri` — T0a/T1b: a `Tensegrity` instance, an
/// `@optimized` `form_find_free` ComputeNode, and `tensegrity_wires`. Exercises
/// list values + the optimized-trampoline path. (golden:
/// `tests/golden/tensegrity_t_prism.txt`.)
const SRC_TENSEGRITY_T_PRISM: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/tensegrity_t_prism.ri"));

/// `examples/tensegrity_membrane_patch.ri` — M0: a `Tensegrity` with a
/// `surfaces` field, a `Membrane`, and `tensegrity_surfaces`. (golden:
/// `tests/golden/tensegrity_membrane_patch.txt`.)
const SRC_TENSEGRITY_MEMBRANE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/tensegrity_membrane_patch.ri"
));

/// `examples/materials_starter_library.ri` — SIR-β-mat: the three wave-2
/// materials + member-access field reads. (golden:
/// `tests/golden/materials_starter_library.txt`.)
const SRC_MATERIALS_LIBRARY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/materials_starter_library.ri"
));

/// `examples/spec-shape-physical.ri` — GHR-ζ: a `Bracket : Physical` with a
/// concrete `box(10mm,20mm,30mm)` geometry + a `Material`, whose `mass`/`centroid`
/// derive from geometry queries. (golden: `tests/golden/spec_shape_physical.txt`.)
const SRC_SPEC_SHAPE_PHYSICAL: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/spec-shape-physical.ri"));

/// `examples/pattern_composition.ri` — geometry breadth: `linear_pattern_2d`
/// (degenerate/grid/composed), `arbitrary_pattern`, and a `union_all` boolean
/// fold across multiple `box` primitives (multiple realizations per structure).
const SRC_PATTERN_COMPOSITION: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/pattern_composition.ri"));

/// `examples/m9_constraint_def.ri` — constraint breadth: `constraint def`s
/// (single/multi-predicate, `pub`), structures consuming them, named args out of
/// declaration order, and `where`-guarded active/inactive constraints.
const SRC_M9_CONSTRAINT_DEF: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m9_constraint_def.ri"));

/// `examples/m5_guarded_enum.ri` — control-flow breadth: an `enum`, a
/// `where`-guarded param group (`where shape == Shape.Round { … } else { … }`),
/// and a `match` expression.
const SRC_M5_GUARDED_ENUM: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m5_guarded_enum.ri"));

/// `examples/cost_aggregation.ri` — aggregation breadth: `Costed : Buy` line
/// items and a dimension-preserving `[…].sum : Scalar<Money>` total over a
/// two-`sub` BOM assembly.
const SRC_COST_AGGREGATION: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/cost_aggregation.ri"));

/// A FRESH [`MockGeometryKernel`] seeded so `examples/spec-shape-physical.ri`'s
/// geometry QUERIES resolve identically under BOTH schedulers. `Bracket.geometry =
/// box(10mm,20mm,30mm)` realizes to `GeometryHandleId(1)`; the `Physical` trait
/// then computes `mass = volume(geometry) * material.density` and `centroid =
/// centroid(geometry)`, dispatching `volume`/`centroid` queries against that
/// handle. With an UNSEEDED mock those queries fail, and the two schedulers
/// surface that failure differently (UnifiedDag dispatches post-geometry per PRD
/// §3.3 and warns; legacy does not) — a scheduler-ordering ARTIFACT of the
/// unseeded mock, not a semantic divergence. Seeding the replies makes the case a
/// genuine equivalence test: both schedulers consume the SAME query results.
///
/// `volume` returns a raw `Value::Real` (kernel contract; the stdlib `volume()`
/// wrapper dimensionalizes it) and `centroid` returns an `{"x","y","z"}` JSON
/// string (the same shape the selector-vocabulary mocks use). Concrete values
/// match the analytic box (vol = 0.01·0.02·0.03 m³; centroid at the corner-origin
/// box's centre) but accuracy is immaterial here — equivalence only requires both
/// schedulers to see identical replies.
pub fn seeded_physical_kernel() -> Box<dyn GeometryKernel> {
    let centroid_json = Value::String(r#"{"x":0.005,"y":0.01,"z":0.015}"#.to_string());
    let kernel = MockGeometryKernel::new()
        .with_volume_result(GeometryHandleId(1), Value::Real(0.01 * 0.02 * 0.03))
        .with_centroid_result(GeometryHandleId(1), centroid_json);
    Box::new(kernel)
}

/// The committed golden idioms + language-breadth entries. Every entry must be
/// equivalent-or-reasoned under both schedulers, 2× byte-identical, and (acyclic)
/// residue==∅. All currently carry EMPTY allow-lists (plain equivalence expected).
pub const GOLDEN_CORPUS: &[CorpusCase] = &[
    // ── the five committed `tests/golden` idioms ──
    CorpusCase {
        name: "golden:structure_instance",
        source: SRC_STRUCTURE_INSTANCE,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "golden:tensegrity_t_prism",
        source: SRC_TENSEGRITY_T_PRISM,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "golden:tensegrity_membrane_patch",
        source: SRC_TENSEGRITY_MEMBRANE,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "golden:materials_starter_library",
        source: SRC_MATERIALS_LIBRARY,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "golden:spec_shape_physical",
        source: SRC_SPEC_SHAPE_PHYSICAL,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: Some(seeded_physical_kernel),
    },
    // ── language-breadth entries (transform / pattern / constraint / guard / cost) ──
    CorpusCase {
        name: "breadth:pattern_composition",
        source: SRC_PATTERN_COMPOSITION,
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "breadth:m9_constraint_def",
        source: SRC_M9_CONSTRAINT_DEF,
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "breadth:m5_guarded_enum",
        source: SRC_M5_GUARDED_ENUM,
        needs_stdlib: false,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
    CorpusCase {
        name: "breadth:cost_aggregation",
        source: SRC_COST_AGGREGATION,
        needs_stdlib: true,
        allowed: &[],
        expects_cycle: false,
        kernel: None,
    },
];

// ─────────────────────────────────────────────────────────────────────────────
// Shared boundary fixtures — geometry-backed-constraint cases that need a SEEDED
// kernel (so a constraint reaches a DEFINITE verdict without OCCT). Reused by the
// allow-list matcher test (corpus binary, step-3) AND the 4275 boundary
// differential (boundary binary, step-16). They are NOT in `SEED_CORPUS` (which
// is the unseeded plainly-equivalent sweep).
// ─────────────────────────────────────────────────────────────────────────────

/// The 4275 SINGLE-instance cross-`let` source: a `let proc = FdmPrinter()`
/// structure instance whose `Adding`-trait `build_volume` geometry member feeds a
/// geometry-backed `FitsBuildVolume` constraint
/// (`fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`).
///
/// Under `UnifiedDag`, ε folds the cross-`let` `bounding_box(proc.build_volume)`
/// leaf POST-geometry (PRD §3.3) → the constraint reaches a DEFINITE verdict;
/// `LegacyMultiPass` leaves the inline geometry-query leaf unresolved → freezes it
/// `Indeterminate`. `FdmPrinter` MUST be declared before `SmallPart` (declaration
/// order is topological for the cross-`let` snapshot seed — same forward-ref
/// limitation as `cross_sub_geometry_e2e.rs`).
///
/// COUNT == 1 deliberately: the multi-instance same-def form is declined to
/// `Indeterminate` (#4628, def-name-keyed snapshot cannot disambiguate instances)
/// and must NOT be used as a definite-verdict case. Lifted from
/// `tests/unified_dag_geometry_executors.rs:442`.
pub const CROSS_LET_4275_SRC: &str = r#"
import std.process

structure def FdmPrinter : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 10USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 45deg
}

structure SmallPart {
    let proc = FdmPrinter()
    let part = box(50mm, 50mm, 50mm)
    constraint FitsBuildVolume(proc: proc, part: part)
}
"#;

/// The auto + geometry-backed-constraint boundary idiom (§6, step-12): an `auto`
/// param drives a box dimension realized into geometry, and an inline
/// `fits_build_volume(bounding_box(part), bounding_box(envelope))` constraint
/// transitively reads that auto realization.
///
/// `param part : Solid = box(w, …)` reads the `auto` cell `AutoWidget.w`, and the
/// constraint reads `part` through `bounding_box(part)`. So the constraint's
/// transitive geometry-backed read closure
/// (`Constraint.realization_reads ∋ AutoWidget#part-realization`, and that
/// realization reads the auto cell `w`) reaches an auto value cell → under
/// `UnifiedDag` the transitive-auto-read guard
/// (`engine_fixpoint::unresolved_diagnostics`) DECLINES the constraint and emits
/// one `E_EVAL_UNRESOLVED` naming it; `LegacyMultiPass` degrades to Indeterminate
/// without that diagnostic.
///
/// SHAPE NOTE — `param part : Solid` (NOT `let part = box(…)`): the
/// constraint→realization edge δ's guard walks is `geometry_cell`, populated by
/// `EvaluationGraph::from_templates` ONLY for a value cell whose `cell_type ==
/// Type::Geometry` (a `Solid` cell IS `Type::Geometry`) whose member name matches
/// the realization's name. A `let part = box(…)` inferred-type binding leaves
/// `geometry_cell == None`, so `collect_constraint_realization_reads` finds no
/// backing realization and the guard would NOT fire. Same constraint as
/// `unified_dag_auto_reaching_constraint_is_declined`
/// (`tests/unified_dag_geometry_executors.rs`), but in the
/// `fits_build_volume(bounding_box(part), bounding_box(envelope))` form §6 names.
pub const AUTO_GEOMETRY_CONSTRAINT_SRC: &str = r#"
import std.process

structure AutoWidget {
    param w        : Length = auto
    param part     : Solid  = box(w, 50mm, 50mm)
    param envelope : Solid  = box(200mm, 200mm, 200mm)
    constraint fits_build_volume(bounding_box(part), bounding_box(envelope))
}
"#;

/// The lexicographic-parent multi-body assembly idiom (§6, step-14): a parent
/// `structure Assembly` whose name sorts BEFORE its child `sub` names (`m`, `z`)
/// and before the child structure defs (`Mbody`, `Zbody`), composing two distinct
/// child bodies via `union` at the parent.
///
/// The point is to stress the unified Kahn worklist's `DebugOrd` pop order: by raw
/// name sort the parent's union realization `Assembly.result` sorts FIRST (`'A'` <
/// `'M'` < `'Z'`), yet it depends on the later-sorting child body realizations
/// `Mbody.body` / `Zbody.body`, so it MUST pop LAST. The §3.1
/// realization→realization edges enforce that topological order over the raw sort
/// — the inverse of the SEED `two_sub_assembly` (parent `C` sorts AFTER children
/// `A`/`B`, so naive sort order is already correct there). Both schedulers must
/// produce byte-identical multi-body export + equivalent values/constraints, with
/// residue==∅.
///
/// `Mbody` / `Zbody` are declared before `Assembly` (forward-ref limitation: a
/// `sub` references a structure declared earlier).
pub const LEX_PARENT_MULTIBODY_SRC: &str = r#"pub structure Mbody {
    let body = box(10mm, 10mm, 10mm)
}
pub structure Zbody {
    let body = box(20mm, 20mm, 20mm)
}
pub structure Assembly {
    sub m = Mbody()
    sub z = Zbody()
    let result = union(self.m.body, self.z.body)
}"#;

/// The multi-realization export idiom (§6, step-18): a single structure with ≥2
/// box realizations folded through two `union` realizations to a terminal body.
/// Five realizations (`a`, `b`, `c`, `ab`, `result`) exercise the unified schedule
/// over a non-trivial realization DAG; both schedulers MUST export byte-identical
/// geometry + equivalent values/constraints, with residue==∅.
pub const MULTI_REALIZATION_SRC: &str = r#"pub structure MultiBody {
    let a = box(10mm, 10mm, 10mm)
    let b = box(20mm, 20mm, 20mm)
    let c = box(30mm, 30mm, 30mm)
    let ab = union(a, b)
    let result = union(ab, c)
}"#;

/// The warm determinacy-predicate idiom (§6, step-18): a `Real` param `k` driving
/// a numeric `let scaled` and a boolean determinacy-predicate `let within = k <=
/// 3.0`. The warm-path test builds this then `edit_param`s `k` and asserts the
/// re-evaluated values are identical regardless of the engine's `BuildScheduler`
/// — `build_scheduler` is read ONLY in cold `build()`; `edit_param` never consults
/// it. Mirrors the proven `Real`-param + `Value::Real` warm-edit shape from
/// `tests/field_eval_tests.rs`.
pub const WARM_PREDICATE_SRC: &str = r#"structure WarmPredicate {
    param k    : Real = 2.0
    let scaled = k * 10.0
    let within = k <= 3.0
}"#;

/// A FRESH [`MockGeometryKernel`] seeded with valid bbox replies for the first
/// four realized handles, so `fits_build_volume` is decidable EITHER way (⇒ a
/// DEFINITE verdict, never undecidable — proving the unified fold, not mere
/// decidability). Boxed for direct use with [`build_with_kernel_stdlib`]; a fresh
/// kernel per call (each build consumes its kernel). Lifted from
/// `tests/unified_dag_geometry_executors.rs:468-480`.
pub fn seeded_build_volume_kernel() -> Box<dyn GeometryKernel> {
    let bbox = |hi: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\
              \"xmax\":{hi},\"ymax\":{hi},\"zmax\":{hi}}}"
        ))
    };
    let mut k = MockGeometryKernel::new();
    for i in 1..=4u64 {
        k = k.with_bbox_result(GeometryHandleId(i), bbox(if i == 1 { 0.20 } else { 0.05 }));
    }
    Box::new(k)
}

// ─────────────────────────────────────────────────────────────────────────────
// Warm-path helpers (§6, step-18). `build_scheduler` is consulted ONLY in cold
// `build()` (engine_build.rs:2420/3008); `eval_cached` / `edit_param` /
// `edit_source` / `build_snapshot` do NOT read it, so a warm re-evaluation is
// scheduler-agnostic until θ (#4361) routes warm Resolution back-prop through the
// driver. θ must re-home the warm corpus rows from "scheduler-agnostic regression
// guard" to "warm == cold" assertions when it lands.
// ─────────────────────────────────────────────────────────────────────────────

/// Cold-build `source` on a FRESH engine under `scheduler`, then drive the WARM
/// `edit_param(cell, new_value)` path and return its [`EvalResult`]. The cold
/// `build()` is the ONLY path that reads `build_scheduler`; the subsequent
/// `edit_param` does not — so the returned warm result is expected to be identical
/// across schedulers (the regression the warm corpus row guards).
pub fn warm_eval_after_edit(
    source: &str,
    scheduler: BuildScheduler,
    needs_stdlib: bool,
    cell: ValueCellId,
    new_value: Value,
) -> EvalResult {
    let compiled = if needs_stdlib {
        compile_source_with_stdlib(source)
    } else {
        compile_source(source)
    };
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(scheduler);
    // Cold build — populates eval_state and is the sole build_scheduler reader.
    engine.build(&compiled, ExportFormat::Step);
    // WARM path — edit_param re-evaluates WITHOUT consulting build_scheduler.
    engine
        .edit_param(cell, new_value)
        .expect("edit_param must succeed on the warm path")
}

/// Project an [`EvalResult`]'s `values` to the same deterministic, canonical,
/// order-independent [`ProjectedValue`] vec `project_build_result` uses (sorted by
/// cell id then content-hash). The warm corpus row compares two of these across
/// schedulers; structural equality IS the scheduler-agnostic warm guarantee.
pub fn project_eval_values(r: &EvalResult) -> Vec<ProjectedValue> {
    let mut values: Vec<ProjectedValue> =
        r.values.iter().map(|(id, v)| project_value(id, v)).collect();
    values.sort_by(|a, b| a.cell.cmp(&b.cell).then_with(|| a.canonical.cmp(&b.canonical)));
    values
}
