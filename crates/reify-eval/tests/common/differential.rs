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
