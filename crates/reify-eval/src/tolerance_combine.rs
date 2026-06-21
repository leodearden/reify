//! Output-occurrence × active-purpose tolerance combiner.
//!
//! See `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! → "Tolerance lives at the purpose") for the contract that drives this
//! module.
//!
//! # Recognition-shape twin
//!
//! `extract_output_tolerance_bound`, `recognize_representation_within`, and
//! `crate::tolerance_scope::extract_tolerance_bindings` all share recognition
//! gates via the `pub(crate) match_representation_within_shape` helper — three
//! callers, one gate implementation that cannot drift (retiring the TODO that
//! lived in the extractor's doc before task 4199 γ; the scope-side routing
//! landed in task 4542).
//!
//! The canonical recognition shape (either compiler IR variant):
//! `UserFunctionCall("RepresentationWithin", ..)` (synthetic/test expressions)
//! or `FunctionCall { function: ResolvedFunction { name: "RepresentationWithin",
//! .. }, .. }` (compiler-resolved stdlib calls) — both are matched identically
//! by `match_representation_within_shape`.
//!
//! In both cases: args == `[<subject typed StructureRef>, <Literal Scalar LENGTH finite>=0]`.
//! The subject (arg0) is either a bare `ValueRef(vcid):StructureRef` (bare param
//! subject, e.g. `RepresentationWithin(subject, 1mm)`) or a member-access
//! `IndexAccess { ValueRef(base):StructureRef, Literal(String(field)) }:StructureRef`
//! (e.g. `RepresentationWithin(bracket.fea_subject, 1mm)`) — both are accepted
//! by Gate 3 of `match_representation_within_shape` (widened in task #3467).

use crate::graph::ConstraintNodeData;
use reify_core::{ConstraintNodeId, Diagnostic, DimensionVector, Type, ValueCellId};
use reify_ir::value::GeometryHandleRef;
use reify_ir::{CompiledExpr, CompiledExprKind, PersistentMap, Satisfaction, Value, ValueMap};
use std::collections::BTreeMap;

/// Combine an output occurrence's tolerance bound with the active purpose's
/// tolerance bound under partial-order "tighter satisfies looser" semantics.
///
/// The two bounds are conceptually different lookups but share the same f64
/// units (SI metres):
/// - `output_bound` — from a `RepresentationWithin(subject, tol)` constraint
///   declared on the output occurrence's template (e.g. `STEPOutput`).
/// - `purpose_bound` — from
///   [`crate::Engine::active_tolerance_for`], computed by the
///   active-purpose subject prefix-scan in `tolerance_scope`.
///
/// # Combination rule
///
/// | output_bound  | purpose_bound | result                |
/// |---------------|---------------|-----------------------|
/// | `Some(o)`     | `Some(p)`     | `Some(o.min(p))`      |
/// | `Some(t)`     | `None`        | `Some(t)`             |
/// | `None`        | `Some(t)`     | `Some(t)`             |
/// | `None`        | `None`        | `None`                |
///
/// The `min`-when-both-Some rule is the same partial-order semantics as the
/// cache-side `tolerance_bucket` (lookup uses the `<=` rule) and the
/// purpose-side `tolerance_scope::merge_with_min`: tighter satisfies looser,
/// so the smaller of two demanded tolerances wins.
pub fn combine_demanded_tolerance(
    output_bound: Option<f64>,
    purpose_bound: Option<f64>,
) -> Option<f64> {
    // Mirror `tolerance_bucket::insert/lookup` and `tolerance_budget::
    // per_stage_tolerance` posture: NaN/Inf/negative tolerances would
    // propagate silently into demand callers (NaN comparisons always evaluate
    // false, so a stale NaN min could never be displaced). Panic in debug
    // builds at the call site rather than letting the bad value contaminate
    // a downstream realization. Same panic-message format across the four
    // tolerance_* modules so authoring errors surface with one voice.
    for tol in [output_bound, purpose_bound].iter().flatten() {
        debug_assert!(
            tol.is_finite() && *tol >= 0.0,
            "ToleranceCombine: tolerance must be finite and non-negative, got {tol}"
        );
    }
    match (output_bound, purpose_bound) {
        (Some(o), Some(p)) => Some(o.min(p)),
        (Some(t), None) | (None, Some(t)) => Some(t),
        (None, None) => None,
    }
}

// ── Private shared recognition helper ────────────────────────────────────────

/// Match the canonical `RepresentationWithin` shape in a single
/// [`CompiledExpr`] and return the three parsed fields, or `None` on any
/// gate failure.
///
/// ## Gates (mirroring `extract_output_tolerance_bound`'s inner gates)
///
/// * **Gate 2** — top-level `UserFunctionCall("RepresentationWithin", [arg0, arg1])`.
/// * **Gate 3** — `arg0.result_type` is `StructureRef(name)`, AND `arg0.kind`
///   is one of two accepted IR shapes:
///   - `ValueRef(vcid)` — bare param subject (e.g. `subject` where
///     `param subject : MyGeom`); `vcid` is used directly.
///   - `IndexAccess { object: ValueRef(base):StructureRef, index: Literal(String(field)) }` —
///     member-access subject (e.g. `bracket.fea_subject`); the composite
///     `ValueCellId::new(&base.entity, field)` is the subject vcid.
///     Non-`ValueRef` object or non-`String` index → `None` (silent-skip).
///   Any other `arg0.kind` → `None` (silent-skip, no diagnostic).
/// * **Gate 4a** — `arg1` is a `Literal(Scalar { dimension == LENGTH, .. })`.
/// * **Gate 4b/c** — `si_value` passes `is_valid_tolerance_si` (finite + ≥ 0.0).
///
/// ## Member-access subject (task #3467)
///
/// The compiler has no `FieldAccess` IR variant. Member access on a param bound
/// to a concrete structure lowers to `IndexAccess { object: ValueRef(base):
/// StructureRef("Bracket"), index: Literal(Value::String("fea_subject")) }` with
/// outer `result_type == StructureRef("FeaFace")` (confirmed empirically,
/// pre-1/task #3467). The composite `ValueCellId::new(&base.entity, field)`
/// enables `resolve_repr_tol_key`'s value-based path; the type-name scan
/// uses `struct_name` from the outer `result_type` and needs no change.
///
/// Returns `(subject_vcid, struct_ref_name, bound_si)` on success.
pub(crate) fn match_representation_within_shape(
    expr: &CompiledExpr,
) -> Option<(ValueCellId, String, f64)> {
    // Gate 2: top-level call to "RepresentationWithin" with exactly 2 args.
    //
    // The compiler emits two different IR variants for function calls:
    //  • `UserFunctionCall` — used for calls to user-defined functions and by
    //    synthetic/test-built expressions (e.g. the tolerance_fixtures helpers).
    //  • `FunctionCall { function: ResolvedFunction, .. }` — emitted when the
    //    compiler resolves "RepresentationWithin" as a stdlib function
    //    (qualified_name "std::RepresentationWithin").
    //
    // Both variants carry the same args vector and are matched identically
    // by this gate; only the name field location differs.
    let (function_name, args) = match &expr.kind {
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => (function_name.as_str(), args),
        CompiledExprKind::FunctionCall { function, args } => (function.name.as_str(), args),
        _ => return None,
    };
    if function_name != "RepresentationWithin" {
        return None;
    }
    if args.len() != 2 {
        return None;
    }

    // Gate 3: arg0.result_type must be StructureRef(name) (both subject shapes),
    // AND arg0.kind must be either a bare ValueRef or a member-access IndexAccess.
    //
    // Hoist the result_type check first so that both IR paths share the same
    // StructureRef guard — any non-StructureRef result_type (e.g. Scalar for
    // `bracket.thickness`) is silently skipped regardless of arg0.kind.
    let subject_arg = &args[0];
    let struct_name = match &subject_arg.result_type {
        Type::StructureRef(name) => name.clone(),
        _ => return None,
    };

    // Resolve the subject ValueCellId from the arg0 IR shape.
    //
    //  • ValueRef(vcid) — bare param subject: use vcid directly (existing path,
    //    byte-identical — e.g. `constraint RepresentationWithin(subject, 1mm)`
    //    where `param subject : MyGeom`).
    //
    //  • IndexAccess { object: ValueRef(base), index: Literal(String(field)) } —
    //    member-access subject: composite vcid = ValueCellId(base.entity, field).
    //    Models `bracket.fea_subject` (task #3467). Non-ValueRef object or
    //    non-String index → None (silent-skip; no diagnostic, policy-neutral).
    //
    //  • Any other kind → None (silent-skip).
    let vcid = match &subject_arg.kind {
        CompiledExprKind::ValueRef(id) => id.clone(),
        CompiledExprKind::IndexAccess { object, index } => {
            // Object must be a ValueRef — the base struct param.
            let base = match &object.kind {
                CompiledExprKind::ValueRef(id) => id,
                _ => return None,
            };
            // Index must be a String literal — the field name.
            let field = match &index.kind {
                CompiledExprKind::Literal(Value::String(s)) => s.as_str(),
                _ => return None,
            };
            ValueCellId::new(&base.entity, field)
        }
        _ => return None,
    };

    // Gate 4: arg1 must be a Literal(Scalar { dimension == LENGTH, si_value })
    // with finite, non-negative si_value (4b/c via is_valid_tolerance_si).
    let tol_arg = &args[1];
    let si_value = match &tol_arg.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::LENGTH => *si_value,
        _ => return None,
    };
    if !crate::tolerance_gate::is_valid_tolerance_si(si_value) {
        return None;
    }

    Some((vcid, struct_name, si_value))
}

// ── Assertion recognizer ──────────────────────────────────────────────────────

/// Recognise a single `RepresentationWithin` constraint expression and return
/// its three parsed components, or `None` if the expression does not match the
/// canonical shape.
///
/// Delegates directly to [`match_representation_within_shape`] — see that
/// function for the gate definitions. This function is the public asserter
/// entry point; [`extract_output_tolerance_bound`] is the budget-extractor
/// entry point. Both share the same gate implementation so they cannot drift.
///
/// ## Return value
///
/// `Some((subject_vcid, struct_ref_name, bound_si_metres))` on a match;
/// `None` on any gate failure (silent-skip posture).
pub fn recognize_representation_within(expr: &CompiledExpr) -> Option<(ValueCellId, String, f64)> {
    match_representation_within_shape(expr)
}

// ── Pure assertion eval helper ────────────────────────────────────────────────

/// Planar quantization floor for the C4 zero-bound comparator (SI metres).
///
/// β B1 validates that a planar box's achieved deviation is ≤ 1e-5 m at unit
/// scale (~1e-6 m f32 quantization — NOT exactly 0.0). Flooring a zero
/// ("exact") bound at this ceiling makes a planar subject Satisfied while a
/// coarse curved subject (β B2: ≫ 1e-5) is Violated, WITHOUT loosening any
/// non-zero (C3) bound.
pub const PLANAR_FLOOR: f64 = 1e-5;

/// Evaluate a single `RepresentationWithin` assertion expression against the
/// current value map and the post-tessellation achieved-repr-tol map.
///
/// # Contract
///
/// * Returns `None` when `expr` is not a `RepresentationWithin` shape (so
///   callers can pass arbitrary constraint expressions through safely).
/// * Returns `Some((Indeterminate, None))` when the subject cannot be resolved
///   to a key in `achieved_repr_tol` — encodes C1 (realization not run ⇒
///   never a false `Violated`).
/// * Returns `Some((Satisfied, None))` when `achieved ≤ eff_bound`.
/// * Returns `Some((Violated, Some(diag)))` when `achieved > eff_bound`;
///   the diagnostic message follows PRD §8.3 ("sampled facet deviation"
///   semantics — the metric is a sampled lower bound, so `Violated` means the
///   **measured** deviation exceeded the bound, not that a tighter bound is
///   provably unachievable).
///
/// # Subject → key resolution (hybrid)
///
/// 1. **Value-based:** look up `vcid` in `values`. If the result is a
///    `GeometryHandle`, its `realization_ref.to_string()` is the key. If it is
///    a `StructureInstance`, scan its `fields` for any `GeometryHandle` field.
/// 2. **Type-name fallback:** scan `achieved_repr_tol` keys for the prefix
///    `"{struct_name}#realization["`. If multiple keys match, take the one with
///    the **maximum** achieved value (conservative — avoids a false `Satisfied`
///    when a module has multiple realizations with varying quality). This path
///    is hydration-independent and works for the post-tessellate `check()` call
///    whose fresh `eval()` may not re-hydrate the subject's `GeometryHandle`.
///
/// # Zero-bound comparator (C4)
///
/// `eff = if bound <= 0.0 { PLANAR_FLOOR } else { bound }`. See [`PLANAR_FLOOR`].
pub fn eval_representation_within(
    id: &ConstraintNodeId,
    expr: &CompiledExpr,
    values: &ValueMap,
    achieved_repr_tol: &BTreeMap<String, f64>,
) -> Option<(Satisfaction, Option<Diagnostic>)> {
    // Step 1: recognise the shape. None → caller should treat as non-assertion.
    let (vcid, struct_name, bound) = match_representation_within_shape(expr)?;

    // Step 2: resolve the subject to an achieved_repr_tol key.
    let key = resolve_repr_tol_key(&vcid, &struct_name, values, achieved_repr_tol);

    // Step 3: look up achieved value (absent key ⇒ Indeterminate, C1).
    let achieved_opt = key.and_then(|k| achieved_repr_tol.get(&k).copied());

    // Step 4: three-valued comparison with C4 zero-bound floor.
    match achieved_opt {
        None => Some((Satisfaction::Indeterminate, None)),
        Some(achieved) => {
            let eff = if bound <= 0.0 { PLANAR_FLOOR } else { bound };
            if achieved <= eff {
                Some((Satisfaction::Satisfied, None))
            } else {
                // PRD §8.3: "sampled facet deviation exceeds bound" — the
                // metric is a sampled lower bound on the achievable deviation,
                // so this diagnostic means the measured deviation exceeded the
                // bound, not that a tighter mesh cannot satisfy it.
                //
                // When the declared bound is zero (exact-representation
                // request), the comparator uses `eff` (PLANAR_FLOOR = 1e-5 m)
                // rather than the literal 0.0 (C4).  Report `eff` so the
                // printed threshold matches the comparison performed; include
                // a note so the user is not surprised by the non-zero value.
                let floor_note = if bound <= 0.0 {
                    " (planar floor 1e-5 m applied; zero bound interpreted as exact)"
                } else {
                    ""
                };
                let diag = Diagnostic::error(format!(
                    "RepresentationWithin: sampled facet deviation {achieved:.3e} m \
                     exceeds bound {eff:.3e} m{floor_note} for {}",
                    id.entity
                ));
                Some((Satisfaction::Violated, Some(diag)))
            }
        }
    }
}

/// Resolve the `RepresentationWithin` subject `vcid` to an
/// `achieved_repr_tol` map key (a `"{entity}#realization[{idx}]"` string).
///
/// Returns `None` when neither value-based resolution nor the type-name scan
/// finds a key — the caller interprets this as Indeterminate (C1).
fn resolve_repr_tol_key(
    vcid: &ValueCellId,
    struct_name: &str,
    values: &ValueMap,
    achieved_repr_tol: &BTreeMap<String, f64>,
) -> Option<String> {
    // — Value-based resolution (hydration-dependent path) ——————————————————
    if let Some(v) = values.get(vcid) {
        // Direct GeometryHandle.
        if let Some(ghr) = GeometryHandleRef::from_geometry_handle(v) {
            return Some(ghr.realization_ref.to_string());
        }
        // StructureInstance: scan fields for any GeometryHandle field.
        if let Value::StructureInstance(data) = v {
            for field_val in data.fields.values() {
                if let Some(ghr) = GeometryHandleRef::from_geometry_handle(field_val) {
                    return Some(ghr.realization_ref.to_string());
                }
            }
        }
    }

    // — Type-name scan fallback (hydration-independent) ————————————————————
    // Scan achieved_repr_tol keys for the prefix "{struct_name}#realization[".
    // If multiple keys match, take the one with the MAXIMUM achieved value
    // (conservative — guards against a false Satisfied when a module has
    // multiple realizations of the same type with varying quality).
    let prefix = format!("{}#realization[", struct_name);
    let mut best_key: Option<String> = None;
    let mut best_val: f64 = f64::NEG_INFINITY;
    for (k, &v) in achieved_repr_tol.iter() {
        if k.starts_with(&prefix) && v > best_val {
            best_val = v;
            best_key = Some(k.clone());
        }
    }
    best_key
}

/// Extract the tightest `RepresentationWithin` tolerance bound declared on
/// the named output occurrence's template, by scanning the runtime
/// constraint graph.
///
/// Output occurrences (e.g. `STEPOutput`, see arch §14.5) carry a body
/// constraint shaped like `RepresentationWithin(subject, 1um)`. When such an
/// occurrence is sub-instantiated, the constraint stays under its
/// *template-name* entity scope in the runtime graph (subs duplicate value
/// cells under scoped entity-refs but do NOT scope-duplicate constraints —
/// see `crate::graph::EvaluationGraph::from_templates`). So the extractor
/// scans `constraints` for entries with `id.entity == output_template_name`
/// regardless of how many times the occurrence was instantiated.
///
/// # Recognition gates
///
/// Mirrors [`crate::tolerance_scope::extract_tolerance_bindings`] (with the
/// purpose-param-membership check dropped — output occurrences have a fixed
/// `param subject : Structure` binding pattern at the template level rather
/// than per-purpose param identity):
///
/// 1. **Entity filter:** `id.entity == output_template_name`. Exact match —
///    no dot-boundary trickery (that semantic belongs to the descendants
///    prefix-scan, not template-name lookup).
/// 2. **Outer shape:** top-level `UserFunctionCall("RepresentationWithin",
///    [arg0, arg1])`.
/// 3. **Subject (arg0):** `ValueRef` whose `result_type` is `StructureRef(_)`.
/// 4. **Tolerance literal (arg1):** `Literal(Value::Scalar { dimension ==
///    LENGTH, si_value })` where `si_value.is_finite() && si_value >= 0.0`.
///    Negative finite literals are silently skipped to keep this extractor
///    in lockstep with [`combine_demanded_tolerance`]'s debug-assert
///    `is_finite() && >= 0.0` invariant — without the gate, a negative bound
///    would survive extraction, then panic the engine in debug builds and
///    silently win an `o.min(p)` race in release builds.
///
/// # Min-fold across multiple matches
///
/// A template author may declare multiple `RepresentationWithin` constraints
/// (e.g. inner vs outer subjects, or a guarded-group split). The extractor
/// folds via `min` under partial-order "tighter satisfies looser" semantics,
/// matching [`crate::tolerance_scope::merge_with_min`] and the cache-side
/// `tolerance_bucket` `<=` rule.
///
/// # Silent-skip posture
///
/// Non-matching shapes / non-finite literals / non-LENGTH dimensions /
/// unrelated entities are silently skipped (no panic). This matches the
/// "activate dormant infrastructure" posture from task 2647 — extraction is
/// policy-neutral; downstream consumers can layer diagnostics if a missing
/// or malformed bound is suspicious.
///
/// Returns `None` when no matching constraint exists.
///
/// # Shared recognition (drift retired)
///
/// Gates 2-4 are delegated to [`match_representation_within_shape`].
/// `crate::tolerance_scope::extract_tolerance_bindings` is now a third caller
/// of that helper (landed as task 4542), so the two extractors cannot drift.
pub fn extract_output_tolerance_bound(
    constraints: &PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    output_template_name: &str,
) -> Option<f64> {
    // Silent-skip audit (locked by `extract_output_tolerance_bound_skips_
    // non_finite_non_length_and_unrelated_entity`):
    //   Gate 1 (entity filter)        skips unrelated-entity entries
    //   Gates 2-4 (shape match)       delegated to
    //                                 `match_representation_within_shape` —
    //                                 see that function for the per-gate
    //                                 breakdown (UFC name+arity, ValueRef
    //                                 :StructureRef, Literal Scalar LENGTH
    //                                 finite≥0). Every non-match returns None
    //                                 → `continue` here.
    // No `panic!`, `expect`, or `unwrap` is reachable — malformed graphs
    // are silently skipped.
    let mut tightest: Option<f64> = None;
    for (id, data) in constraints.iter() {
        // Gate 1: entity exact-match filter.
        if id.entity != output_template_name {
            continue;
        }

        // Gates 2-4: shared recognition shape (UFC + ValueRef:StructureRef +
        // Literal Scalar LENGTH finite≥0). Only the bound (si_value) is needed
        // here — subject vcid and StructureRef name are discarded (C2: public
        // signature and behavior are byte-identical to the pre-factoring impl).
        let Some((_vcid, _struct_name, si_value)) = match_representation_within_shape(&data.expr)
        else {
            continue;
        };

        // Min-fold under partial-order "tighter satisfies looser" semantics.
        tightest = Some(match tightest {
            Some(cur) => cur.min(si_value),
            None => si_value,
        });
    }
    tightest
}

// ── Output-occurrence conformance (io-export δ) ───────────────────────────────

/// Returns `true` iff any name in `trait_bounds` equals or transitively refines
/// the `"Output"` trait, walking [`reify_compiler::CompiledTrait::refinements`]
/// over a name→trait map built from `trait_defs`.
///
/// This is the io-export δ export-driver's trait-conformance gate: an occurrence
/// template is a driver-eligible Output sink iff its `entity_kind == Occurrence`
/// (checked by the caller) **and** `conforms_to_output(template.trait_bounds,
/// module.trait_defs)`. Recognizing by *transitive trait-bound conformance* —
/// not a `trait_bounds.contains("Output")` name match — means user-defined
/// Output occurrences are driven too: `occurrence def Foo : MyExport` where
/// `trait MyExport : Output` conforms even though `"Output"` never appears
/// directly in `Foo`'s bounds.
///
/// # Why re-implemented here
///
/// reify-compiler's `satisfies_trait_bound` / `trait_satisfies` are
/// `pub(crate)` and unreachable from reify-eval; making them `pub` would touch
/// an out-of-scope crate. This small local closure keeps the change inside the
/// three touched crates and is co-located with the other Output recognizers
/// (`extract_output_tolerance_bound`, `match_representation_within_shape`) so
/// the driver and the tolerance pipeline share one Output-recognition module.
///
/// # Cycle safety
///
/// A `visited` set bounds the refinement walk, so a malformed refinement cycle
/// (`trait A : B`, `trait B : A`) terminates with `false` instead of looping
/// forever. The `name == "Output"` check fires at pop time, before the visited
/// guard, so a bound that *equals* `"Output"` is recognized even when `"Output"`
/// also appears as an interior node of the lattice.
pub fn conforms_to_output(
    trait_bounds: &[String],
    trait_defs: &[reify_compiler::CompiledTrait],
) -> bool {
    use std::collections::{HashMap, HashSet};

    // name → refinement (parent-trait) names. Built once per call; a module's
    // trait_defs is small (its declared traits), so the allocation is cheap.
    let by_name: HashMap<&str, &[String]> = trait_defs
        .iter()
        .map(|t| (t.name.as_str(), t.refinements.as_slice()))
        .collect();

    let mut visited: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = trait_bounds.iter().map(String::as_str).collect();

    while let Some(name) = stack.pop() {
        if name == "Output" {
            return true;
        }
        // Cycle guard: skip a trait whose refinements were already enqueued.
        if !visited.insert(name) {
            continue;
        }
        if let Some(refinements) = by_name.get(name) {
            stack.extend(refinements.iter().map(String::as_str));
        }
    }
    false
}

/// Where a recognized Output occurrence sends its geometry — resolved from the
/// occurrence's `format` field value by [`extract_output_export_spec`].
#[derive(Debug, Clone, PartialEq)]
pub enum OutputTarget {
    /// Serialize a file in the given format (`OutputFormat.STEP/STL/ThreeMF`).
    File(reify_ir::ExportFormat),
    /// A `DisplayOutput` (`OutputFormat.Display`) — recognized as a conforming
    /// Output but file emission is deferred (the viewport drive is a sibling
    /// PRD). `build_outputs` surfaces an info diagnostic
    /// ([`crate::I_DISPLAY_OUTPUT_DEFERRED`]) and emits no file.
    DisplayDeferred,
}

/// The per-instance export spec read off a realized Output occurrence's
/// [`reify_ir::value::StructureInstanceData`] fields.
///
/// Sibling to [`extract_output_tolerance_bound`] in this Output-recognition
/// module: that extractor is template/constraint-scoped (tolerance only),
/// whereas this reader is per-instance (the `format`/`path`/`resolution` that
/// the io-export δ driver consumes). Co-locating them satisfies the task's
/// "driver and tolerance pipeline share one Output-recognition path" directive
/// without changing the existing extractor's signature.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputExportSpec {
    /// The resolved target: a file format or display-deferred.
    pub format: OutputTarget,
    /// The destination path, verbatim from the occurrence's `path` field.
    /// Empty for a `DisplayDeferred` occurrence that declares no path.
    pub path: String,
    /// STL tessellation tolerance (SI metres) from the occurrence's
    /// `resolution` Length field, if present. Informational in δ — the demanded
    /// tolerance is threaded through `build()`'s realization pass, not
    /// re-tessellated per occurrence in v1; ε/γ consume this.
    pub tess_tol: Option<f64>,
    /// STEP schema selected by the occurrence's `version` field
    /// (`STEPVersion.AP203/AP214/AP242`), defaulting to
    /// [`reify_ir::StepSchema::Ap214`] when absent. Only meaningful for a
    /// `File(Step)` target; the ε driver threads it into the OCCT writer via
    /// [`reify_ir::ExportOptions`]. Non-STEP targets carry the default.
    pub step_schema: reify_ir::StepSchema,
}

/// Read the [`OutputExportSpec`] off a realized Output-occurrence instance
/// value, or `None` if `instance` is not a recognizable Output spec.
///
/// # Gates
///
/// 1. **Instance** — `instance` must be a [`Value::StructureInstance`].
/// 2. **Format** — its `format` field must be a
///    `Value::Enum { type_name: "OutputFormat", variant }`; the variant maps
///    `STEP→File(Step)`, `STL→File(Stl)`, `ThreeMF→File(ThreeMF)`,
///    `Display→DisplayDeferred`. Any other (absent / non-enum / unknown
///    variant) → `None`.
/// 3. **Path** — a `Value::String` `path` field is required for *file* targets
///    (absent or non-String → `None`); for `DisplayDeferred` a missing/non-String
///    path is tolerated (a viewport sink has no file path) and yields an empty
///    `path`.
/// 4. **Resolution** — a `Value::Scalar` `resolution` field with `LENGTH`
///    dimension becomes `tess_tol` (its SI value); absent/non-LENGTH → `None`.
/// 5. **Version** — a `Value::Enum { type_name: "STEPVersion", variant }`
///    `version` field maps `AP203→Ap203`, `AP214→Ap214`, `AP242→Ap242`;
///    absent / non-enum / unknown variant → [`reify_ir::StepSchema::Ap214`]
///    (the DSL default `version : STEPVersion = STEPVersion.AP214`).
///
/// Keying the target on the resolved `format` *value* (not the instance type
/// name) is robust to user-defined Output occurrences that set
/// `format : OutputFormat`.
pub fn extract_output_export_spec(instance: &Value) -> Option<OutputExportSpec> {
    // Gate 1: must be a StructureInstance.
    let Value::StructureInstance(data) = instance else {
        return None;
    };
    let fields = &data.fields;

    // Gate 2: `format` must be an OutputFormat enum; map variant → target.
    let format = match fields.get("format") {
        Some(Value::Enum { type_name, variant }) if type_name == "OutputFormat" => {
            match variant.as_str() {
                "STEP" => OutputTarget::File(reify_ir::ExportFormat::Step),
                "STL" => OutputTarget::File(reify_ir::ExportFormat::Stl),
                "ThreeMF" => OutputTarget::File(reify_ir::ExportFormat::ThreeMF),
                "Display" => OutputTarget::DisplayDeferred,
                _ => return None,
            }
        }
        _ => return None,
    };

    // Gate 3: path — required-as-String for file targets; optional for display.
    let path = match (fields.get("path"), &format) {
        (Some(Value::String(s)), _) => s.clone(),
        (_, OutputTarget::DisplayDeferred) => String::new(),
        _ => return None,
    };

    // Gate 4: `resolution` Length → tess_tol (SI metres); else None.
    let tess_tol = match fields.get("resolution") {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::LENGTH => Some(*si_value),
        _ => None,
    };

    // Gate 5: `version` STEPVersion enum → STEP schema; absent/unknown → default.
    let step_schema = match fields.get("version") {
        Some(Value::Enum { type_name, variant }) if type_name == "STEPVersion" => {
            match variant.as_str() {
                "AP203" => reify_ir::StepSchema::Ap203,
                "AP214" => reify_ir::StepSchema::Ap214,
                "AP242" => reify_ir::StepSchema::Ap242,
                _ => reify_ir::StepSchema::default(),
            }
        }
        _ => reify_ir::StepSchema::default(),
    };

    Some(OutputExportSpec {
        format,
        path,
        tess_tol,
        step_schema,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ConstraintNodeData;
    use reify_core::{ConstraintNodeId, ContentHash, DimensionVector, Type, ValueCellId};
    use reify_ir::{CompiledExpr, PersistentMap, Value};

    /// Build a `(ConstraintNodeId, ConstraintNodeData)` pair carrying the
    /// canonical `RepresentationWithin(<ValueRef typed StructureRef>,
    /// <Literal Scalar(LENGTH)>)` shape that `extract_output_tolerance_bound`
    /// is expected to recognise. Mirrors the
    /// `tolerance_scope::tests::representation_within_constraint` fixture
    /// but produces a graph-side `ConstraintNodeData` instead of a
    /// `CompiledPurpose` constraint.
    fn representation_within_constraint_node(
        entity: &str,
        index: u32,
        si_value: f64,
        dimension: DimensionVector,
    ) -> (ConstraintNodeId, ConstraintNodeData) {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Structure".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value,
                dimension,
            },
            Type::Scalar { dimension },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        let id = ConstraintNodeId::new(entity, index);
        let data = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr,
            content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
            optimized_target: None,
        };
        (id, data)
    }

    /// Build a `(ConstraintNodeId, ConstraintNodeData)` pair whose outer
    /// `CompiledExpr` is the caller-supplied `expr` instead of the canonical
    /// `RepresentationWithin(...)` shape. Used by the silent-skip audit
    /// fixtures that exercise outer-shape mismatches (non-`UserFunctionCall`
    /// outer kind, wrong arity, wrong arg-type) — the matcher must `continue`
    /// past these without observing them.
    fn constraint_node_with(
        entity: &str,
        index: u32,
        expr: CompiledExpr,
    ) -> (ConstraintNodeId, ConstraintNodeData) {
        let id = ConstraintNodeId::new(entity, index);
        let data = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr,
            content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
            optimized_target: None,
        };
        (id, data)
    }

    #[test]
    fn extract_output_tolerance_bound_skips_non_finite_non_length_and_unrelated_entity() {
        let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
            PersistentMap::default();

        // (a) NaN tolerance literal under STEPOutput — must be silently
        // skipped (no panic). NaN comparisons always evaluate false, so a
        // stale NaN min could never be displaced.
        let (id_a, data_a) = representation_within_constraint_node(
            "STEPOutput",
            0,
            f64::NAN,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_a, data_a);

        // (b) INFINITY tolerance literal under STEPOutput — silently skipped.
        let (id_b, data_b) = representation_within_constraint_node(
            "STEPOutput",
            1,
            f64::INFINITY,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_b, data_b);

        // (c) NEG_INFINITY tolerance literal under STEPOutput — silently
        // skipped.
        let (id_c, data_c) = representation_within_constraint_node(
            "STEPOutput",
            2,
            f64::NEG_INFINITY,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_c, data_c);

        // (d) DIMENSIONLESS Scalar literal under STEPOutput — silently
        // skipped by the LENGTH match-guard.
        let (id_d, data_d) = representation_within_constraint_node(
            "STEPOutput",
            3,
            1.0,
            DimensionVector::DIMENSIONLESS,
        );
        constraints.insert(id_d, data_d);

        // (e) Valid finite LENGTH RepresentationWithin under "OtherTemplate"
        // — silently skipped by the entity exact-match filter (even though
        // its tolerance is tighter than (f) below).
        let (id_e, data_e) = representation_within_constraint_node(
            "OtherTemplate",
            0,
            1e-6,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_e, data_e);

        // (g) Negative finite LENGTH literal under STEPOutput — silently
        // skipped by Gate 4c (>= 0.0). Without this gate, a negative bound
        // would survive extraction, then panic the combiner's debug-assert
        // in debug builds and win an `o.min(p)` race in release builds.
        let (id_g, data_g) =
            representation_within_constraint_node("STEPOutput", 5, -1e-3, DimensionVector::LENGTH);
        constraints.insert(id_g, data_g);

        // (h) Non-`UserFunctionCall` outer kind under STEPOutput — silently
        // skipped by Gate 2 (the `match &data.expr.kind { … _ => continue }`
        // arm). Pins that any non-UFC top-level (e.g. a top-level `Literal`)
        // never reaches the inner shape check.
        let (id_h, data_h) = constraint_node_with(
            "STEPOutput",
            6,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        );
        constraints.insert(id_h, data_h);

        // (i) `UserFunctionCall("RepresentationWithin", [single_arg])` (arity
        // 1) under STEPOutput — silently skipped by Gate 2's arity check.
        // Pins that the arity-mismatch branch is reachable independently of
        // the outer-shape branch.
        let (id_i, data_i) = constraint_node_with(
            "STEPOutput",
            7,
            CompiledExpr::user_function_call(
                "RepresentationWithin".to_string(),
                vec![CompiledExpr::value_ref(
                    ValueCellId::new("subject", "self"),
                    Type::StructureRef("Structure".to_string()),
                )],
                Type::Bool,
            ),
        );
        constraints.insert(id_i, data_i);

        // (j) `RepresentationWithin(<ValueRef typed Real>, <length-literal>)`
        // under STEPOutput — silently skipped by Gate 3 (arg0 result_type
        // must be `StructureRef(_)`). Pins the type-tag gate independently
        // of the ValueRef-kind gate.
        let (id_j, data_j) = constraint_node_with(
            "STEPOutput",
            8,
            CompiledExpr::user_function_call(
                "RepresentationWithin".to_string(),
                vec![
                    CompiledExpr::value_ref(
                        ValueCellId::new("subject", "self"),
                        Type::dimensionless_scalar(),
                    ),
                    CompiledExpr::literal(
                        Value::Scalar {
                            si_value: 0.5e-6,
                            dimension: DimensionVector::LENGTH,
                        },
                        Type::Scalar {
                            dimension: DimensionVector::LENGTH,
                        },
                    ),
                ],
                Type::Bool,
            ),
        );
        constraints.insert(id_j, data_j);

        // (f) The one valid finite LENGTH RepresentationWithin under
        // STEPOutput — survives all gates.
        let (id_f, data_f) =
            representation_within_constraint_node("STEPOutput", 4, 5e-6, DimensionVector::LENGTH);
        constraints.insert(id_f, data_f);

        assert_eq!(
            extract_output_tolerance_bound(&constraints, "STEPOutput"),
            Some(5e-6),
            "only the valid finite non-negative LENGTH constraint under \
             STEPOutput must survive — all other entries (NaN, ±Inf, \
             negative-finite, dimensionless, unrelated entity, non-UFC outer, \
             wrong-arity, non-StructureRef arg0) must be silently skipped"
        );
    }

    #[test]
    fn extract_output_tolerance_bound_returns_min_under_matching_entity() {
        let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
            PersistentMap::default();

        // Two matching RepresentationWithin entries under "STEPOutput" — must
        // be folded via min so the tighter (1e-6) wins.
        let (id_a, data_a) =
            representation_within_constraint_node("STEPOutput", 0, 50e-6, DimensionVector::LENGTH);
        constraints.insert(id_a, data_a);
        let (id_b, data_b) =
            representation_within_constraint_node("STEPOutput", 1, 1e-6, DimensionVector::LENGTH);
        constraints.insert(id_b, data_b);

        // An unrelated constraint under a different entity — must be skipped
        // by the entity exact-match filter.
        let (id_c, data_c) = representation_within_constraint_node(
            "OtherEntity",
            0,
            0.5e-6,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_c, data_c);

        assert_eq!(
            extract_output_tolerance_bound(&constraints, "STEPOutput"),
            Some(1e-6),
            "min-fold across the two matching entries must yield the tighter (1e-6); \
             OtherEntity's tighter 0.5e-6 must be filtered out by entity match"
        );
    }

    #[test]
    fn combine_returns_min_when_both_some() {
        assert_eq!(
            combine_demanded_tolerance(Some(50e-6), Some(1e-6)),
            Some(1e-6),
            "tighter purpose-bound (1e-6) wins over looser output-bound (50e-6)"
        );
        assert_eq!(
            combine_demanded_tolerance(Some(1e-6), Some(50e-6)),
            Some(1e-6),
            "tighter output-bound (1e-6) wins over looser purpose-bound (50e-6) — symmetric"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_nan_output_bound() {
        combine_demanded_tolerance(Some(f64::NAN), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_nan_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(f64::NAN));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_infinite_output_bound() {
        combine_demanded_tolerance(Some(f64::INFINITY), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_infinite_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(f64::INFINITY));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_negative_output_bound() {
        combine_demanded_tolerance(Some(-1.0e-3), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_negative_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(-1.0e-3));
    }

    #[test]
    fn combine_passes_through_lone_some_or_returns_none_when_both_none() {
        assert_eq!(
            combine_demanded_tolerance(Some(1e-6), None),
            Some(1e-6),
            "lone output-bound passes through when purpose-bound is None"
        );
        assert_eq!(
            combine_demanded_tolerance(None, Some(1e-6)),
            Some(1e-6),
            "lone purpose-bound passes through when output-bound is None"
        );
        assert_eq!(
            combine_demanded_tolerance(None, None),
            None,
            "both None must return None — no demand contributor exists"
        );
    }

    // ── step-1 (task 4199 γ): recognize_representation_within unit tests ───────
    //
    // These tests are RED until step-2 implements `recognize_representation_within`.

    /// Build the canonical `RepresentationWithin(<ValueRef typed
    /// StructureRef("Curved")>, <Scalar{1e-6, LENGTH}>)` expression used
    /// across multiple step-1 test cases.
    fn canonical_repr_within_expr() -> CompiledExpr {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        )
    }

    /// Canonical RepresentationWithin expression → Some((subject_vcid, "Curved", 1e-6)).
    ///
    /// This is the recognition-shape positive case: a well-formed
    /// `RepresentationWithin(ValueRef(subject.self):StructureRef("Curved"),
    /// Scalar{1e-6, LENGTH})` must yield the tuple
    /// `(ValueCellId("subject","self"), "Curved", 1e-6)`.
    #[test]
    fn recognize_repr_within_returns_some_for_canonical_shape() {
        let expr = canonical_repr_within_expr();
        let result = recognize_representation_within(&expr);
        assert!(
            result.is_some(),
            "canonical RepresentationWithin must be recognized as Some"
        );
        let (vcid, struct_name, bound) = result.unwrap();
        assert_eq!(
            vcid,
            ValueCellId::new("subject", "self"),
            "subject ValueCellId must match the ValueRef in arg0"
        );
        assert_eq!(
            struct_name, "Curved",
            "StructureRef inner name must be extracted from arg0.result_type"
        );
        assert!(
            (bound - 1e-6).abs() < 1e-15,
            "bound must match the Scalar si_value in arg1; got {bound}"
        );
    }

    /// Non-RepresentationWithin function name → None (silent-skip gate 2 name check).
    #[test]
    fn recognize_repr_within_returns_none_for_wrong_function_name() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationBetween".to_string(), // wrong name
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "wrong function name must return None (gate 2 name check)"
        );
    }

    /// Arity ≠ 2 (single arg) → None (silent-skip gate 2 arity check).
    #[test]
    fn recognize_repr_within_returns_none_for_wrong_arity() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg], // arity 1 — wrong
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "arity ≠ 2 must return None (gate 2 arity check)"
        );
    }

    /// Arg0 is a Literal (not a ValueRef) → None (gate 3 ValueRef check).
    #[test]
    fn recognize_repr_within_returns_none_for_non_value_ref_arg0() {
        // Use a Bool literal as arg0 — not a ValueRef.
        let literal_arg = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![literal_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-ValueRef arg0 must return None (gate 3)"
        );
    }

    /// Arg0 is a ValueRef but with non-StructureRef result_type (Real) → None (gate 3).
    #[test]
    fn recognize_repr_within_returns_none_for_non_structure_ref_result_type() {
        // ValueRef with result_type = Real, not StructureRef.
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::dimensionless_scalar(), // wrong result_type
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-StructureRef result_type on arg0 must return None (gate 3)"
        );
    }

    /// Arg1 has a DIMENSIONLESS dimension (not LENGTH) → None (gate 4a).
    #[test]
    fn recognize_repr_within_returns_none_for_non_length_dimension() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::DIMENSIONLESS, // wrong dimension
            },
            Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-LENGTH dimension in arg1 must return None (gate 4a)"
        );
    }

    /// Arg1 is a NaN tolerance literal → None (gate 4b).
    #[test]
    fn recognize_repr_within_returns_none_for_nan_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "NaN tolerance must return None (gate 4b)"
        );
    }

    /// Arg1 is +Infinity tolerance literal → None (gate 4b).
    #[test]
    fn recognize_repr_within_returns_none_for_infinite_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: f64::INFINITY,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "+Infinity tolerance must return None (gate 4b)"
        );
    }

    /// Arg1 is a negative (−1 mm) finite LENGTH tolerance literal → None (gate 4c).
    #[test]
    fn recognize_repr_within_returns_none_for_negative_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: -1e-3, // negative finite
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "negative finite tolerance must return None (gate 4c)"
        );
    }

    /// Non-UserFunctionCall outer kind (a Bool literal) → None.
    ///
    /// Pins that the matcher short-circuits on the outer kind, never reaching
    /// the inner gates, so callers can pass arbitrary constraint expressions
    /// through `recognize_representation_within` safely.
    #[test]
    fn recognize_repr_within_returns_none_for_non_ufc_outer_kind() {
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-UserFunctionCall outer kind must return None"
        );
    }

    /// Zero bound (0.0) with LENGTH dimension → Some — zero is a valid non-negative
    /// finite bound (the C4 PLANAR_FLOOR comparator handles it, not the gate).
    ///
    /// Mirrors the `>= 0.0` (not `> 0.0`) posture of `is_valid_tolerance_si`.
    #[test]
    fn recognize_repr_within_returns_some_for_zero_bound() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        let result = recognize_representation_within(&expr);
        assert!(
            result.is_some(),
            "zero bound (0.0 m) is a valid non-negative finite LENGTH bound and \
             must be recognized — the PLANAR_FLOOR comparator handles zero bounds, \
             not the gate"
        );
        let (_, _, bound) = result.unwrap();
        assert_eq!(bound, 0.0, "zero bound must be preserved exactly");
    }

    // ── step-3 (task 4199 γ): eval_representation_within unit tests ───────────
    //
    // These tests verify the pure assertion eval helper using synthetic ValueMap
    // and synthetic BTreeMap<String, f64> achieved_repr_tol inputs.
    //
    // Note: the implementation was added in step-2's commit alongside the
    // recognizer; tests here provide regression coverage.

    use reify_core::RealizationNodeId;
    use reify_ir::GeometryHandleId;

    /// Build a canonical `RepresentationWithin(ValueRef(subject.self):StructureRef(name), bound_m)`
    /// expression for step-3 eval tests.
    fn eval_test_expr(struct_name: &str, bound_m: f64) -> CompiledExpr {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef(struct_name.to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: bound_m,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        )
    }

    /// Build a `Value::GeometryHandle` for `{entity}#realization[{idx}]`.
    fn geometry_handle_value(entity: &str, idx: u32) -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new(entity, idx),
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId::INVALID,
        }
    }

    /// (a) achieved < bound → Satisfied.
    ///
    /// The type-name scan fallback resolves the subject to
    /// `"Curved#realization[0]"` (struct_name == "Curved") and compares
    /// achieved (1e-7) < bound (1e-6) → Satisfied.
    #[test]
    fn eval_repr_within_satisfied_when_achieved_less_than_bound() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new();
        let mut achieved = BTreeMap::new();
        achieved.insert("Curved#realization[0]".to_string(), 1e-7);

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(
            result.is_some(),
            "must return Some for a RepresentationWithin expr"
        );
        let (sat, diag) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Satisfied,
            "achieved (1e-7) < bound (1e-6) → Satisfied"
        );
        assert!(diag.is_none(), "no diagnostic on Satisfied");
    }

    /// (b) achieved > bound → Violated with a diagnostic.
    #[test]
    fn eval_repr_within_violated_when_achieved_exceeds_bound() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new();
        let mut achieved = BTreeMap::new();
        achieved.insert("Curved#realization[0]".to_string(), 5e-3); // 5mm ≫ 1µm

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, diag) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Violated,
            "achieved (5e-3) > bound (1e-6) → Violated"
        );
        assert!(diag.is_some(), "Violated must carry a diagnostic");
        let msg = diag.unwrap().message;
        assert!(
            msg.contains("RepresentationWithin"),
            "diagnostic must mention RepresentationWithin; got: {msg}"
        );
        assert!(
            msg.contains("exceeds bound"),
            "diagnostic must mention 'exceeds bound'; got: {msg}"
        );
    }

    /// (c) Key ABSENT in map → Indeterminate (C1: realization not run → never a
    /// false Violated).
    #[test]
    fn eval_repr_within_indeterminate_when_key_absent() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new();
        let achieved: BTreeMap<String, f64> = BTreeMap::new(); // empty

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, diag) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Indeterminate,
            "absent key → Indeterminate (C1: realization not run)"
        );
        assert!(diag.is_none(), "no diagnostic on Indeterminate");
    }

    /// (d) Subject unresolvable (no value in ValueMap, no matching key in
    /// achieved_repr_tol for the struct_name) → Indeterminate.
    #[test]
    fn eval_repr_within_indeterminate_when_subject_unresolvable() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new(); // no entry for subject.self
        // achieved_repr_tol has entries for a DIFFERENT name — no prefix match
        let mut achieved = BTreeMap::new();
        achieved.insert("Other#realization[0]".to_string(), 5e-3);

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, _) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Indeterminate,
            "unresolvable subject (no value, no matching key) → Indeterminate"
        );
    }

    /// (e) C4 zero-bound floor — part 1: bound=0.0 with achieved ≤ PLANAR_FLOOR
    /// (1e-5 m) → Satisfied.
    #[test]
    fn eval_repr_within_zero_bound_satisfied_below_planar_floor() {
        let id = ConstraintNodeId::new("Planar", 0);
        let expr = eval_test_expr("Planar", 0.0); // exact / zero bound
        let values = ValueMap::new();
        let mut achieved = BTreeMap::new();
        // Planar box achieved ~1e-6 (β B1 validates ≤ 1e-5 m for planar).
        achieved.insert("Planar#realization[0]".to_string(), 1e-6);

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, _) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Satisfied,
            "bound=0.0 with achieved (1e-6) ≤ PLANAR_FLOOR (1e-5) → Satisfied (C4)"
        );
    }

    /// (e) C4 zero-bound floor — part 2: bound=0.0 with achieved ≫ PLANAR_FLOOR
    /// → Violated (coarse curved geometry, cf. β B2).
    #[test]
    fn eval_repr_within_zero_bound_violated_above_planar_floor() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 0.0); // exact / zero bound
        let values = ValueMap::new();
        let mut achieved = BTreeMap::new();
        // Coarse curved geometry achieved ~50mm >> 1e-5 m.
        achieved.insert("Curved#realization[0]".to_string(), 50e-3);

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, diag) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Violated,
            "bound=0.0 with achieved (50e-3) ≫ PLANAR_FLOOR (1e-5) → Violated (C4)"
        );
        assert!(diag.is_some(), "Violated must carry a diagnostic");
    }

    /// (f) Value-based resolution: a `Value::GeometryHandle{realization_ref=
    /// "Curved#realization[0]"}` subject value keys directly into
    /// `achieved_repr_tol["Curved#realization[0]"]`.
    #[test]
    fn eval_repr_within_value_based_resolution_via_geometry_handle() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);

        // Place a GeometryHandle value at subject.self.
        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("subject", "self"),
            geometry_handle_value("Curved", 0),
        );

        // achieved_repr_tol key has a non-standard name to verify value-based
        // resolution was used (type-name scan would also match "Curved#realization[0]"
        // here, but the value-based path should be tried first).
        let mut achieved = BTreeMap::new();
        achieved.insert("Curved#realization[0]".to_string(), 5e-7); // < 1e-6

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, _) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Satisfied,
            "GeometryHandle value resolves to 'Curved#realization[0]'; achieved (5e-7) < bound (1e-6) → Satisfied"
        );
    }

    /// (g) Type-name scan fallback: subject value is absent (Undef) but
    /// `achieved_repr_tol` contains `"Curved#realization[0]"` and the
    /// StructureRef name is `"Curved"` → resolves via type-name scan.
    #[test]
    fn eval_repr_within_type_name_scan_fallback_when_value_absent() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new(); // no entry — subject stays Undef

        let mut achieved = BTreeMap::new();
        achieved.insert("Curved#realization[0]".to_string(), 5e-7); // < 1e-6

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, _) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Satisfied,
            "type-name scan resolves 'Curved#realization[0]' for struct_name 'Curved'; \
             achieved (5e-7) < bound (1e-6) → Satisfied"
        );
    }

    /// (g) multi-key scan takes MAX achieved (conservative) — multiple
    /// `"Curved#realization[N]"` keys; the highest (1e-2) must be used so the
    /// assertion correctly reports Violated even if some realizations are fine.
    #[test]
    fn eval_repr_within_type_name_scan_uses_max_achieved_on_multiple_keys() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = eval_test_expr("Curved", 1e-6);
        let values = ValueMap::new();

        let mut achieved = BTreeMap::new();
        achieved.insert("Curved#realization[0]".to_string(), 5e-7); // fine — Satisfied alone
        achieved.insert("Curved#realization[1]".to_string(), 1e-2); // coarse — Violated

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(result.is_some());
        let (sat, _) = result.unwrap();
        assert_eq!(
            sat,
            Satisfaction::Violated,
            "max-achieved (1e-2) wins over the finer (5e-7) — conservative, no false Satisfied"
        );
    }

    /// Non-RepresentationWithin expr → None (pass-through posture).
    #[test]
    fn eval_repr_within_returns_none_for_non_repr_within_expr() {
        let id = ConstraintNodeId::new("Curved", 0);
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let values = ValueMap::new();
        let achieved: BTreeMap<String, f64> = BTreeMap::new();

        let result = eval_representation_within(&id, &expr, &values, &achieved);
        assert!(
            result.is_none(),
            "non-RepresentationWithin expr must return None (pass-through)"
        );
    }

    // ── step-1 (task #3467): member-access recognition unit tests ────────────
    //
    // These tests exercise `recognize_representation_within` (which delegates
    // to `match_representation_within_shape`) with arg0 shaped as an
    // `IndexAccess { ValueRef(base):StructureRef, Literal(String(field)) }`.
    //
    // RED until step-3 widens Gate 3 of `match_representation_within_shape`.

    /// Build a `RepresentationWithin(IndexAccess(ValueRef(base):base_type,
    /// Literal(String(field))):result_struct, Scalar{si_value,LENGTH})` expression.
    ///
    /// Used to construct member-access subjects of varying types for the
    /// step-1 recognition gate tests.
    fn index_access_repr_within_expr(
        base_entity: &str,
        base_member: &str,
        base_struct: &str,
        field: &str,
        result_struct: Option<&str>, // None → non-StructureRef (e.g. Scalar)
        si_value: f64,
    ) -> CompiledExpr {
        let object = CompiledExpr::value_ref(
            ValueCellId::new(base_entity, base_member),
            Type::StructureRef(base_struct.to_string()),
        );
        let index = CompiledExpr::literal(Value::String(field.to_string()), Type::String);
        let arg0_result_type = match result_struct {
            Some(name) => Type::StructureRef(name.to_string()),
            None => Type::dimensionless_scalar(), // models bracket.thickness (non-struct)
        };
        let arg0 = CompiledExpr::index_access(object, index, arg0_result_type);
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![arg0, tol_arg],
            Type::Bool,
        )
    }

    /// POSITIVE (a): IndexAccess member-access subject with StructureRef result_type
    /// → `recognize_representation_within` returns the composite ValueCellId.
    ///
    /// Models `bracket.fea_subject` where bracket is `ValueRef("bracket","self"):
    /// StructureRef("Bracket")` and the index is `Literal(String("fea_subject"))`.
    /// The outer CompiledExpr.result_type is `StructureRef("FeaFace")`.
    ///
    /// Expected: `Some((ValueCellId("bracket","fea_subject"), "FeaFace", 1e-3))`.
    ///
    /// RED until step-3 widens Gate 3.
    #[test]
    fn recognize_repr_within_returns_some_for_index_access_subject() {
        let expr = index_access_repr_within_expr(
            "bracket",
            "self",       // base member
            "Bracket",    // base struct type
            "fea_subject",
            Some("FeaFace"), // result struct type
            1e-3,         // 1mm bound
        );
        let result = recognize_representation_within(&expr);
        assert!(
            result.is_some(),
            "IndexAccess member-access subject with StructureRef result_type \
             must be recognized as Some — RED until step-3 widens Gate 3"
        );
        let (vcid, struct_name, bound) = result.unwrap();
        assert_eq!(
            vcid,
            ValueCellId::new("bracket", "fea_subject"),
            "composite vcid must be (base.entity, field) = ('bracket','fea_subject')"
        );
        assert_eq!(
            struct_name, "FeaFace",
            "struct_name must be extracted from arg0.result_type StructureRef"
        );
        assert!(
            (bound - 1e-3).abs() < 1e-15,
            "bound must match the tolerance literal; got {bound}"
        );
    }

    /// NEGATIVE (b): IndexAccess subject with non-StructureRef result_type (Scalar)
    /// models `bracket.thickness` — the outer result_type is dimensionless scalar,
    /// not a StructureRef → must return None (silent-skip).
    ///
    /// Still RED until step-3 (Gate 3 must check result_type BEFORE matching
    /// IndexAccess, so a non-StructureRef IndexAccess returns None).
    #[test]
    fn recognize_repr_within_returns_none_for_index_access_non_struct_result_type() {
        let expr = index_access_repr_within_expr(
            "bracket",
            "self",
            "Bracket",
            "thickness",
            None, // non-StructureRef result type
            1e-3,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "IndexAccess with non-StructureRef result_type must return None (silent-skip)"
        );
    }

    /// NEGATIVE (c): IndexAccess with a non-String index literal (Integer) → None.
    ///
    /// `index = Literal(Int(0))` does not represent a named field access;
    /// must be silently skipped.
    #[test]
    fn recognize_repr_within_returns_none_for_index_access_non_string_index() {
        // Build manually: index is Int, not String.
        let object = CompiledExpr::value_ref(
            ValueCellId::new("bracket", "self"),
            Type::StructureRef("Bracket".to_string()),
        );
        let index_int = CompiledExpr::literal(Value::Int(0), Type::Int);
        let arg0 = CompiledExpr::index_access(
            object,
            index_int,
            Type::StructureRef("FeaFace".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-3,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![arg0, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "IndexAccess with non-String index must return None (silent-skip)"
        );
    }

    /// NEGATIVE (d): IndexAccess whose `object` is not a ValueRef (e.g. a Literal)
    /// → must return None (silent-skip, non-ValueRef object not supported).
    #[test]
    fn recognize_repr_within_returns_none_for_index_access_non_value_ref_object() {
        // Build manually: object is a Literal(Bool), not a ValueRef.
        let object_literal = CompiledExpr::literal(
            Value::Bool(true),
            Type::Bool,
        );
        let index = CompiledExpr::literal(Value::String("fea_subject".to_string()), Type::String);
        let arg0 = CompiledExpr::index_access(
            object_literal,
            index,
            Type::StructureRef("FeaFace".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-3,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![arg0, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "IndexAccess with non-ValueRef object must return None (silent-skip)"
        );
    }

    // ── conforms_to_output (io-export δ step-1) ──────────────────────────────

    /// Build a minimal `CompiledTrait` with the given name and refinement
    /// (parent-trait) names. All other fields default to empty — only `name`
    /// and `refinements` drive the transitive Output-conformance walk.
    fn trait_def(name: &str, refinements: &[&str]) -> reify_compiler::CompiledTrait {
        reify_compiler::CompiledTrait {
            name: name.to_string(),
            is_pub: true,
            doc: None,
            type_params: Vec::new(),
            refinements: refinements.iter().map(|s| s.to_string()).collect(),
            required_members: Vec::new(),
            defaults: Vec::new(),
            content_hash: ContentHash::of_str(name),
            annotations: Vec::new(),
            pragmas: Vec::new(),
        }
    }

    /// Direct and transitive Output conformance is recognized; supertraits,
    /// sibling traits, and empty bounds are rejected. Mirrors the stdlib io.ri
    /// trait lattice: `Output : Sink`, `Input : Source`, with a synthetic
    /// user-defined `MyExport : Output` for the transitive case.
    #[test]
    fn conforms_to_output_recognizes_direct_transitive_and_rejects_others() {
        let trait_defs = vec![
            trait_def("Source", &[]),
            trait_def("Sink", &[]),
            trait_def("Output", &["Sink"]),
            trait_def("Input", &["Source"]),
            trait_def("MyExport", &["Output"]),
        ];

        // Direct: a bound equal to "Output".
        assert!(
            conforms_to_output(&["Output".to_string()], &trait_defs),
            "a direct [\"Output\"] bound must conform"
        );

        // Transitive: MyExport refines Output (user-defined Output occurrence).
        assert!(
            conforms_to_output(&["MyExport".to_string()], &trait_defs),
            "a bound that transitively refines Output (MyExport : Output) must conform"
        );

        // Sink is a SUPERTRAIT of Output, not Output — must NOT conform.
        assert!(
            !conforms_to_output(&["Sink".to_string()], &trait_defs),
            "Sink is a supertrait of Output, not Output itself — must not conform"
        );

        // Input refines Source, never reaches Output — must NOT conform.
        assert!(
            !conforms_to_output(&["Input".to_string()], &trait_defs),
            "Input refines Source, never Output — must not conform"
        );

        // No bounds at all (a plain Structure) — must NOT conform.
        assert!(
            !conforms_to_output(&[], &trait_defs),
            "empty trait_bounds must not conform"
        );
    }

    /// A refinement cycle must terminate (no infinite loop) and yield the
    /// correct answer: `false` when the cycle never reaches Output, `true`
    /// when a node on the cycle also refines Output.
    #[test]
    fn conforms_to_output_handles_refinement_cycle_without_infinite_loop() {
        // A → B → A cycle that never reaches Output → false (and terminates).
        let cyclic = vec![trait_def("A", &["B"]), trait_def("B", &["A"])];
        assert!(
            !conforms_to_output(&["A".to_string()], &cyclic),
            "an A⇄B cycle that never reaches Output must return false without looping"
        );

        // A → B → {A, Output}: the cycle co-exists with a path to Output → true.
        let cyclic_with_output = vec![
            trait_def("A", &["B"]),
            trait_def("B", &["A", "Output"]),
            trait_def("Output", &["Sink"]),
            trait_def("Sink", &[]),
        ];
        assert!(
            conforms_to_output(&["A".to_string()], &cyclic_with_output),
            "a cycle that also refines Output (B : A, Output) must still conform"
        );
    }

    // ── extract_output_export_spec (io-export δ step-3) ──────────────────────

    /// Build a `Value::StructureInstance` named `type_name` with the given
    /// fields. `type_id`/`version` are dummies — `extract_output_export_spec`
    /// keys on the `format` *field value*, not the instance's type identity.
    fn struct_instance(type_name: &str, fields: &[(&str, Value)]) -> Value {
        let mut map: PersistentMap<String, Value> = PersistentMap::default();
        for (k, v) in fields {
            map.insert((*k).to_string(), v.clone());
        }
        Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
            type_id: reify_ir::StructureTypeId(0),
            type_name: type_name.to_string(),
            version: 0,
            fields: map,
        }))
    }

    /// An `OutputFormat::<variant>` enum value (the shape of an occurrence's
    /// `format` field).
    fn out_fmt(variant: &str) -> Value {
        Value::Enum {
            type_name: "OutputFormat".to_string(),
            variant: variant.to_string(),
        }
    }

    /// File-format occurrences map `format`→`OutputTarget::File`, read a String
    /// `path`, and turn a `resolution` Length into `tess_tol` (SI metres).
    #[test]
    fn extract_output_export_spec_reads_file_formats_path_and_resolution() {
        // STLOutput: STL + path + 0.2mm resolution.
        let stl = struct_instance(
            "STLOutput",
            &[
                ("format", out_fmt("STL")),
                ("path", Value::String("o.stl".to_string())),
                ("resolution", Value::length(2e-4)),
            ],
        );
        assert_eq!(
            extract_output_export_spec(&stl),
            Some(OutputExportSpec {
                format: OutputTarget::File(reify_ir::ExportFormat::Stl),
                path: "o.stl".to_string(),
                tess_tol: Some(2e-4),
                step_schema: reify_ir::StepSchema::Ap214,
            }),
            "STLOutput → File(Stl), path \"o.stl\", tess_tol 2e-4"
        );

        // STEPOutput: STEP + path, no resolution → tess_tol None.
        let step = struct_instance(
            "STEPOutput",
            &[
                ("format", out_fmt("STEP")),
                ("path", Value::String("o2.step".to_string())),
            ],
        );
        assert_eq!(
            extract_output_export_spec(&step),
            Some(OutputExportSpec {
                format: OutputTarget::File(reify_ir::ExportFormat::Step),
                path: "o2.step".to_string(),
                tess_tol: None,
                step_schema: reify_ir::StepSchema::Ap214,
            }),
            "STEPOutput → File(Step), path \"o2.step\", tess_tol None"
        );

        // ThreeMFOutput: ThreeMF + path.
        let mf = struct_instance(
            "ThreeMFOutput",
            &[
                ("format", out_fmt("ThreeMF")),
                ("path", Value::String("o.3mf".to_string())),
            ],
        );
        let spec = extract_output_export_spec(&mf).expect("ThreeMF spec must be Some");
        assert_eq!(
            spec.format,
            OutputTarget::File(reify_ir::ExportFormat::ThreeMF),
            "ThreeMFOutput → File(ThreeMF)"
        );
        assert_eq!(spec.path, "o.3mf");
    }

    /// A `DisplayOutput` (format == Display, no `path` field) is recognized as a
    /// deferred target — a `Some(DisplayDeferred)`, NOT a `None`.
    #[test]
    fn extract_output_export_spec_recognizes_display_as_deferred() {
        let disp = struct_instance("DisplayOutput", &[("format", out_fmt("Display"))]);
        let spec = extract_output_export_spec(&disp).expect("Display spec must be Some");
        assert_eq!(
            spec.format,
            OutputTarget::DisplayDeferred,
            "DisplayOutput → DisplayDeferred (recognized, file emission deferred)"
        );
    }

    /// A file-format occurrence with a missing or non-String `path`, and any
    /// non-`StructureInstance` value, yield `None`.
    #[test]
    fn extract_output_export_spec_rejects_missing_path_nonstring_path_and_non_instance() {
        // File format with NO path field → None.
        let no_path = struct_instance("STLOutput", &[("format", out_fmt("STL"))]);
        assert_eq!(
            extract_output_export_spec(&no_path),
            None,
            "a file Output with no path must not yield a spec"
        );

        // File format with a non-String path → None.
        let bad_path = struct_instance(
            "STLOutput",
            &[("format", out_fmt("STL")), ("path", Value::Int(7))],
        );
        assert_eq!(
            extract_output_export_spec(&bad_path),
            None,
            "a non-String path must not yield a spec"
        );

        // Non-StructureInstance value → None (pass-through posture).
        assert_eq!(
            extract_output_export_spec(&Value::Bool(true)),
            None,
            "a non-StructureInstance value must not yield a spec"
        );
    }

    /// A `STEPVersion::<variant>` enum value (the shape of a STEPOutput
    /// occurrence's `version` field).
    fn step_ver(variant: &str) -> Value {
        Value::Enum {
            type_name: "STEPVersion".to_string(),
            variant: variant.to_string(),
        }
    }

    /// The `version` field of a STEPOutput occurrence selects the STEP schema:
    /// `STEPVersion.AP203`→`Ap203`, `AP242`→`Ap242`, and an absent `version`
    /// defaults to `Ap214` (matching the DSL default
    /// `version : STEPVersion = STEPVersion.AP214`). The reader keys on the
    /// enum's `STEPVersion` type name and variant, mirroring how `format`
    /// reads the `OutputFormat` enum.
    #[test]
    fn extract_output_export_spec_reads_step_version_into_schema() {
        use reify_ir::StepSchema;

        // version = STEPVersion.AP203 → Ap203.
        let step_203 = struct_instance(
            "STEPOutput",
            &[
                ("format", out_fmt("STEP")),
                ("path", Value::String("a.step".to_string())),
                ("version", step_ver("AP203")),
            ],
        );
        assert_eq!(
            extract_output_export_spec(&step_203).map(|s| s.step_schema),
            Some(StepSchema::Ap203),
            "version STEPVersion.AP203 → StepSchema::Ap203"
        );

        // version = STEPVersion.AP242 → Ap242.
        let step_242 = struct_instance(
            "STEPOutput",
            &[
                ("format", out_fmt("STEP")),
                ("path", Value::String("b.step".to_string())),
                ("version", step_ver("AP242")),
            ],
        );
        assert_eq!(
            extract_output_export_spec(&step_242).map(|s| s.step_schema),
            Some(StepSchema::Ap242),
            "version STEPVersion.AP242 → StepSchema::Ap242"
        );

        // No `version` field → default Ap214 (the DSL default).
        let step_default = struct_instance(
            "STEPOutput",
            &[
                ("format", out_fmt("STEP")),
                ("path", Value::String("c.step".to_string())),
            ],
        );
        assert_eq!(
            extract_output_export_spec(&step_default).map(|s| s.step_schema),
            Some(StepSchema::Ap214),
            "absent version → StepSchema::Ap214 (DSL default)"
        );
    }
}
