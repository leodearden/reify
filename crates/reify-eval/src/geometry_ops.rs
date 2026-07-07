// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

use std::collections::{BTreeMap, HashMap, HashSet};

use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, GeometryHandleId, GeometryKernel, KernelHandle, ValueMap};

use crate::eval_ctx_with_meta;

/// Minimum meaningful distance in meters (1 picometer).
///
/// Distances with `|v| < DEGENERATE_LENGTH_M` cannot produce a well-defined
/// solid — any kernel attempting to extrude / sweep at sub-picometer lengths
/// is likely to return an opaque error. Named constants (not bare literals)
/// also let future refactors relocate the tolerance without a regex sweep.
/// Boundary semantics are pinned by
/// `build_extrude_distance_{just_below,at}_threshold_*` tests.
pub(crate) const DEGENERATE_LENGTH_M: f64 = 1e-12;

/// Minimum meaningful angle in radians (sub-picoradian).
///
/// Revolve angles with `|a| < DEGENERATE_ANGLE_RAD` cannot produce a
/// well-defined revolved solid. Boundary semantics are pinned by
/// `build_revolve_angle_*_threshold_*` tests.
pub(crate) const DEGENERATE_ANGLE_RAD: f64 = 1e-12;

/// Generic geometry epsilon for axis-magnitude / direction-vector checks
/// (e.g. rejecting near-zero revolve axes).
pub(crate) const GEOMETRY_EPSILON: f64 = 1e-12;

/// Routing outcome returned by [`gate_query_capability`].
///
/// Maps directly to the downstream dispatcher choice:
/// - `Occt` → invoke the OCCT BRep kernel
/// - `Manifold` → invoke the Manifold Mesh kernel
/// - `Unsupported` → fail closed; the caller maps this to `None` so the cell
///   retains `Value::Undef` (the existing fall-through-is-preservation
///   contract invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // used in #[cfg(test)] and by downstream dispatcher tasks (KGQ-ο/π/ρ)
pub(crate) enum CapabilityRoute {
    /// Route to the OCCT BRep kernel.
    Occt,
    /// Route to the Manifold Mesh kernel.
    Manifold,
    /// Query is unsupported for this repr; fail closed.
    ///
    /// The gate has already pushed a `Diagnostic::error` carrying
    /// [`reify_types::DiagnosticCode::QueryNotSupportedOnRepr`] onto the
    /// diagnostics vec.
    Unsupported,
}

/// Capability-gating decision + diagnostic helper (PRD §5.4).
///
/// Maps `(produced_repr, query.capability_kind())` to a [`CapabilityRoute`]:
///
/// | repr          | capability    | route       | diagnostic |
/// |---------------|---------------|-------------|------------|
/// | BRep          | BRepOnly      | Occt        | —          |
/// | BRep          | BRepAndMesh   | Occt        | —          |
/// | BRep          | MeshOnly      | Unsupported | Error      |
/// | Mesh          | MeshOnly      | Manifold    | —          |
/// | Mesh          | BRepAndMesh   | Manifold    | —          |
/// | Mesh          | BRepOnly      | Unsupported | Error      |
/// | Sdf/Voxel/VolumeMesh | any    | Unsupported | Error      |
///
/// # Fail-closed contract
///
/// Every `Unsupported` branch pushes exactly one
/// `Diagnostic::error(...).with_code(DiagnosticCode::QueryNotSupportedOnRepr)`
/// onto `diagnostics`, then returns `Unsupported`. The caller must map
/// `Unsupported` → `None` → `Value::Undef` (the existing fall-through-
/// is-preservation contract). This function never panics.
///
/// # Message text
///
/// The 'requires' clause is derived from the query's capability kind:
/// - `BRepOnly` → `'<name>' requires BRep representation; this geometry is realized as <repr>`
/// - `MeshOnly` → `'<name>' requires Mesh representation; this geometry is realized as <repr>`
/// - `BRepAndMesh` → `'<name>' requires BRep or Mesh representation; this geometry is realized as <repr>`
///
/// `query_display_name` is the user-written `.ri` helper name (e.g.
/// `"curvature"`, `"edge_length"`) — thread it like existing `&function.name`
/// callers. `produced_repr` is rendered via `{:?}` (`"Mesh"`, `"BRep"`, …).
///
/// # Exhaustiveness
///
/// The inner `match produced_repr` covers all five [`reify_types::ReprKind`]
/// variants explicitly (no `_` wildcard) so a future repr addition is a
/// compile error at this site.
/// Core `(QueryCapability, ReprKind)` → `CapabilityRoute` decision + diagnostic
/// push (task #4812, P0β refactor).
///
/// Shared by the geometry-query path ([`gate_query_capability`]) and the new
/// region-selector path (`resolve_selector_to_list`).  Pushes exactly one
/// `Diagnostic::error(...).with_code(QueryNotSupportedOnRepr)` on
/// `Unsupported`; the caller maps `Unsupported` → `Value::Undef`.
#[allow(dead_code)] // used by gate_query_capability + region path; KGQ-ο/π/ρ will use directly
pub(crate) fn route_capability(
    capability: reify_ir::QueryCapability,
    produced_repr: reify_ir::ReprKind,
    query_display_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CapabilityRoute {
    use reify_core::DiagnosticCode;
    use reify_ir::{QueryCapability, ReprKind};

    let requires_clause = match capability {
        QueryCapability::BRepOnly => "requires BRep representation",
        QueryCapability::MeshOnly => "requires Mesh representation",
        QueryCapability::BRepAndMesh => "requires BRep or Mesh representation",
    };

    let unsupported = |diagnostics: &mut Vec<Diagnostic>| {
        diagnostics.push(
            Diagnostic::error(format!(
                "'{query_display_name}' {requires_clause}; \
                 this geometry is realized as {produced_repr:?}"
            ))
            .with_code(DiagnosticCode::QueryNotSupportedOnRepr),
        );
        CapabilityRoute::Unsupported
    };

    match produced_repr {
        ReprKind::BRep => match capability {
            QueryCapability::BRepOnly | QueryCapability::BRepAndMesh => CapabilityRoute::Occt,
            QueryCapability::MeshOnly => unsupported(diagnostics),
        },
        ReprKind::Mesh => match capability {
            QueryCapability::MeshOnly | QueryCapability::BRepAndMesh => CapabilityRoute::Manifold,
            QueryCapability::BRepOnly => unsupported(diagnostics),
        },
        ReprKind::Sdf => unsupported(diagnostics),
        ReprKind::Voxel => unsupported(diagnostics),
        ReprKind::VolumeMesh => unsupported(diagnostics),
    }
}

#[allow(dead_code)] // used in #[cfg(test)] and by downstream dispatcher tasks (KGQ-ο/π/ρ)
pub(crate) fn gate_query_capability(
    query: &reify_ir::GeometryQuery,
    produced_repr: reify_ir::ReprKind,
    query_display_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CapabilityRoute {
    route_capability(query.capability_kind(), produced_repr, query_display_name, diagnostics)
}

/// Look up a named argument in `args`, evaluate it, and return the resulting
/// `Value`.  If the argument is absent, push a `Warning` diagnostic and return
/// `None`.  Callers that need a finite `f64` should use [`eval_named_arg_f64`],
/// which also emits a `Warning` when the value is non-numeric or non-finite.
///
/// Fail-fast / anti-cascade contract: the caller is expected to propagate the
/// `None` via `.ok_or_else(...)?` so `compile_geometry_op` short-circuits with
/// a single Error before any downstream type-coercion check can fire. This
/// produces exactly one Warning + one Error per missing arg — no
/// "expected Geometry, found Undef" cascade. That invariant is regression-locked
/// by `build_primitive_missing_arg_emits_exactly_one_compile_warning` in
/// `tests/geometry_error_handling.rs`.
pub(crate) fn eval_named_arg(
    name: &str,
    kind_label: impl std::fmt::Display,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match args.iter().find(|(n, _)| n == name) {
        Some((_, expr)) => Some(reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, functions, meta_map),
        )),
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "missing required geometry argument '{}' for {}",
                name, kind_label
            )));
            None
        }
    }
}

/// Look up a named argument, evaluate it, and convert to a finite `f64`.
/// Returns `None` with a diagnostic when the argument is absent (delegated
/// to [`eval_named_arg`]) or when the argument is present but evaluates to a
/// non-numeric or non-finite value (NaN, ±Infinity, or a non-`f64` type such
/// as `String` or `Bool`).  In the latter case a `Warning` diagnostic is
/// pushed with the message `"argument '{name}' for {kind} evaluated to
/// non-numeric/non-finite value"`.
///
/// Non-numeric / non-finite path coverage is locked by
/// `eval_named_arg_f64_{undef,nan,infinity}_value_returns_none_with_warning`.
pub(crate) fn eval_named_arg_f64(
    name: &str,
    kind_label: impl std::fmt::Display + Copy,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    let value = eval_named_arg(
        name,
        kind_label,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )?;
    match value.as_f64() {
        Some(v) if v.is_finite() => Some(v),
        _ => {
            diagnostics.push(Diagnostic::warning(format!(
                "argument '{}' for {} evaluated to non-numeric/non-finite value",
                name, kind_label
            )));
            None
        }
    }
}

/// Evaluate all args in a variadic curve constructor to f64 values.
///
/// Returns `None` if any arg evaluates to a non-finite value, pushing a
/// warning diagnostic for each bad arg.  Used by InterpCurve, BezierCurve,
/// and NurbsCurve to avoid duplicating the same eval-and-collect loop.
pub(crate) fn eval_all_args_to_f64(
    label: &str,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<f64>> {
    args.iter()
        .map(|(name, expr)| {
            let v = reify_expr::eval_expr(expr, &eval_ctx_with_meta(values, functions, meta_map));
            match v.as_f64() {
                Some(f) if f.is_finite() => Some(f),
                _ => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{} arg '{}' is non-finite",
                        label, name
                    )));
                    None
                }
            }
        })
        .collect()
}

/// Canonicalize sub-handle `kernel_handle` ids into the canonical edge/face
/// order: ascending `kernel_handle` id, deduplicated. Single source of truth
/// for the "canonical order + dedup" step, shared by `resolve_subhandle_list`
/// (which layers the cross-solid membership gate on top) and the
/// `compile_geometry_op` `ModifyKind::Fillet` eval arm, so the two never drift
/// on ordering/dedup (task 3205 reviewer note). Ascending id matches
/// `extract_edges`' TopExp mint order, so a curated subset lines up with the
/// kernel's edge map.
fn canonical_subhandle_ids(
    ids: impl IntoIterator<Item = GeometryHandleId>,
) -> Vec<GeometryHandleId> {
    // `BTreeSet` gives dedup (by id) + ascending canonical order in a single
    // structure — `GeometryHandleId` is `Ord`.
    ids.into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Lower a `List<Geometry>` of KGQ topology sub-handles to a canonical
/// `Vec<GeometryHandleId>` (task 3205 — the curated edge/face SELECTION SEAM).
///
/// This helper is **kernel-free** (pure `Value` → `Value`): it never touches
/// the geometry kernel, so it is callable from BOTH [`compile_geometry_op`]
/// (the legacy eval lowering site, which has no kernel parameter and runs in a
/// phase where the parent shape is not yet realized) AND the unified
/// build-DAG driver (engine-unified-build-dag task η).
///
/// The cross-solid gate is `realization_ref` equality: every KGQ sub-handle
/// inherits its parent solid's `realization_ref` unchanged (KGQ-η PRD §4
/// invariant i, see [`crate::topology_selectors::make_sub_handle`]), so a
/// handle minted from a different solid carries a different `realization_ref`
/// and is rejected. The hash domain for these sub-handles is
/// [`crate::topology_selectors::compose_sub_handle_hash`] /
/// [`crate::topology_selectors::SubKind`]; this resolver reads only the
/// already-built `realization_ref` + `kernel_handle`, so it needs no rehash.
///
/// Contract:
///   - `arg` MUST be a `Value::List`; any other shape is a hard `Err`.
///   - `parent` MUST be a `Value::GeometryHandle`; its `realization_ref` is the
///     membership key.
///   - every element MUST be a `Value::GeometryHandle` whose `realization_ref`
///     equals the parent's — an element from a different solid is `Err`
///     (cross-solid).
///   - the resulting ids are **deduped** by `kernel_handle` and returned in
///     **ascending canonical order** (matching `extract_edges`' TopExp mint
///     order, so a curated subset lines up with the kernel's edge map).
///   - an **empty** input list is a legitimate `Ok(vec![])`. The anti-zero-
///     edges (`E_EMPTY_SELECTION`) guard — which distinguishes "selector
///     present but resolved to nothing" from "no selector at all" — is the
///     eval arm's job, NOT this structural resolver's.
// `#[allow(dead_code)]`: forward-looking selection-resolver seam. The legacy
// eval arm (`compile_geometry_op`'s `ModifyKind::Fillet`) cannot call the full
// resolver because the parent solid's `Value::GeometryHandle` is not realized in
// phase P2 (it enters `values` only in P3 — see the task-3205 plan), so that arm
// shares only the structural `canonical_subhandle_ids` canonicalization and skips
// the cross-solid membership gate. The full cross-solid resolver is consumed by
// engine-unified-build-dag η/ε, whose in-loop driver has the realized parent
// handle. Exercised now by the `resolve_subhandle_list_*` unit tests below.
// TODO(#4727): Stage-4 cutover (#4362) landed — UnifiedDag is now the default —
// but `resolve_subhandle_list` still has no production caller (the in-loop driver
// has not been wired to invoke it). Drop this `#[allow(dead_code)]` once Stage-5
// (#4727) wires the caller or removes this dead path entirely.
#[allow(dead_code)]
pub(crate) fn resolve_subhandle_list(
    arg: &reify_ir::Value,
    parent: &reify_ir::Value,
) -> Result<Vec<GeometryHandleId>, String> {
    let parent_ref = match parent {
        reify_ir::Value::GeometryHandle {
            realization_ref, ..
        } => realization_ref,
        other => {
            return Err(format!(
                "resolve_subhandle_list: parent must be a Geometry handle, got {:?}",
                other
            ));
        }
    };

    let elems = match arg {
        reify_ir::Value::List(elems) => elems,
        other => {
            return Err(format!(
                "resolve_subhandle_list: edge selector must be a List<Geometry>, got {:?}",
                other
            ));
        }
    };

    // Validate every element (cross-solid membership gate), collecting raw
    // kernel_handles; `canonical_subhandle_ids` then dedups + sorts them into
    // canonical order — the SAME canonicalization the eval Fillet arm uses, so
    // the two cannot drift on ordering/dedup.
    let mut ids: Vec<GeometryHandleId> = Vec::with_capacity(elems.len());
    for (i, elem) in elems.iter().enumerate() {
        match elem {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                kernel_handle,
                ..
            } => {
                if realization_ref != parent_ref {
                    return Err(format!(
                        "resolve_subhandle_list: edge[{}] belongs to a different solid \
                         ({} != parent {}) — cross-solid edge selection is rejected",
                        i, realization_ref, parent_ref
                    ));
                }
                let Some(kh) = *kernel_handle else {
                    return Err(format!(
                        "resolve_subhandle_list: edge[{}] is a symbolic (unrealized) handle \
                         — edge selection requires a realized geometry handle",
                        i
                    ));
                };
                ids.push(kh);
            }
            other => {
                return Err(format!(
                    "resolve_subhandle_list: edge[{}] must be a Geometry sub-handle, got {:?}",
                    i, other
                ));
            }
        }
    }

    Ok(canonical_subhandle_ids(ids))
}

/// Op-specific user-facing wording for the shared legacy-P2 curated-edge
/// resolver [`resolve_curated_edges_p2`]. The resolution POLICY is identical
/// across the three local-feature ops (Fillet, Chamfer, ChamferAsymmetric);
/// only the call-form strings in the diagnostics differ, so each eval arm
/// supplies its own labels while the logic lives in exactly one place.
#[derive(Clone, Copy)]
struct CuratedEdgeLabels {
    /// Full call signature, e.g. `"fillet(solid, edges, radius)"`. Names the
    /// call form in the reject-non-handle, empty-selection, and
    /// unresolved-selector diagnostics.
    call_form: &'static str,
    /// Bare verb for the "refusing to silently {verb} all edges" phrasing
    /// (`"fillet"` / `"chamfer"` — asymmetric chamfer also uses `"chamfer"`).
    verb: &'static str,
    /// Short op name: the prefix of the returned `Err` strings and the
    /// "at the point this {short} runs" phrasing.
    short: &'static str,
    /// User-actionable tail for the unresolved-selector (legacy-P2 `Undef`)
    /// `Err`. The 2-arg fallback hint differs per op; `chamfer_asymmetric` has
    /// no 2-arg form, so its tail only points at the η/ε follow-up.
    unresolved_hint: &'static str,
}

impl CuratedEdgeLabels {
    const FILLET: Self = Self {
        call_form: "fillet(solid, edges, radius)",
        verb: "fillet",
        short: "fillet",
        unresolved_hint: "Use 2-arg fillet(solid, radius) to fillet all edges, or \
                          wait for curated edge selection (engine-unified-build-dag \
                          tasks 4360/4358).",
    };
    const CHAMFER: Self = Self {
        call_form: "chamfer(solid, edges, distance)",
        verb: "chamfer",
        short: "chamfer",
        unresolved_hint: "Use 2-arg chamfer(solid, distance) to chamfer all edges, or \
                          wait for curated edge selection (engine-unified-build-dag \
                          tasks 4360/4358).",
    };
    const CHAMFER_ASYMMETRIC: Self = Self {
        call_form: "chamfer_asymmetric(solid, edges, d1, d2)",
        verb: "chamfer",
        short: "chamfer_asymmetric",
        unresolved_hint: "Wait for curated edge selection (engine-unified-build-dag \
                          tasks 4360/4358).",
    };
}

/// Resolve a PRESENT (3-arg/4-arg) curated edge selector to canonical
/// `GeometryHandleId`s in the legacy-P2 eval arm (`compile_geometry_op`).
///
/// Single shared implementation behind the Fillet, Chamfer, and
/// ChamferAsymmetric eval arms — extracted (task 4185 reviewer note) so the
/// reject-non-handle policy, the [`canonical_subhandle_ids`] canonicalization,
/// the anti-zero-edges `EmptyEdgeSelection` guard, and the legacy-`Undef`
/// staging `Err` are defined ONCE and structurally cannot drift between the
/// three ops (they previously shared the logic only by copy-paste + comment).
///
/// Caller contract: `edges_val` is the ALREADY-evaluated selector value, and
/// the caller has already confirmed an `edges` arg was present (the absent =
/// all-edges back-compat path stays in the arm). Policy:
///   - `Value::List`: every element MUST be a `Value::GeometryHandle` — a
///     non-handle element is a hard `Err` (never a silent drop, mirroring
///     [`resolve_subhandle_list`]'s strictness); the ids are deduped +
///     ascending-canonical via [`canonical_subhandle_ids`]. An EMPTY resolved
///     set pushes an `EmptyEdgeSelection` diagnostic and returns `Err`
///     (anti-zero-edges, task-3295 trap).
///   - any non-`List` value is the legacy-pipeline `Undef` state (the selector
///     resolves in P4, after this P2 arm): NOT an empty selection, so NO
///     `EmptyEdgeSelection`; returns a user-actionable `Err`.
///
/// Kernel-free, and (like the legacy arms) shares only the cross-solid-gate-LESS
/// canonicalization documented on [`resolve_subhandle_list`] — the P2 arm cannot
/// run the membership gate because the parent solid handle is not yet realized.
fn resolve_curated_edges_p2(
    edges_val: &reify_ir::Value,
    labels: CuratedEdgeLabels,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<GeometryHandleId>, String> {
    let elems = match edges_val {
        reify_ir::Value::List(elems) => elems,
        // The selector did not resolve to a List — on the legacy pipeline it is
        // `Undef` (the edges selector resolves in P4, after this P2 arm). This
        // is NOT an empty selection, so do NOT emit `EmptyEdgeSelection` (that
        // would false-positive on every legacy 3-arg/4-arg call); return a
        // USER-ACTIONABLE `Err` so the cell stays Undef and η resolves it
        // in-loop. Removed once engine-unified-build-dag η/ε (tasks 4360/4358)
        // make curated selection reachable end-to-end.
        other => {
            return Err(format!(
                "{}: curated edge selection is not yet available on the current \
                 build pipeline — the edge selector cannot be resolved at the \
                 point this {} runs. {} [edge selector evaluated to {:?}]",
                labels.call_form, labels.short, labels.unresolved_hint, other
            ));
        }
    };
    // Extract each sub-handle's kernel_handle, ERRORING on any element that is
    // NOT a Geometry sub-handle so a partially-malformed selector (some handles,
    // some non-handles) surfaces an error rather than silently operating on only
    // the surviving subset (the latent trap the task-3205 reviewer flagged: a
    // `filter_map` here would drop the bad elements and only an ALL-dropped list
    // would trip EmptyEdgeSelection).
    let mut raw_ids: Vec<GeometryHandleId> = Vec::with_capacity(elems.len());
    for (i, e) in elems.iter().enumerate() {
        match e {
            reify_ir::Value::GeometryHandle { kernel_handle, .. } => {
                let Some(kh) = *kernel_handle else {
                    return Err(format!(
                        "{}: edge selector element [{}] is a symbolic (unrealized) handle \
                         — edge selection requires a realized geometry handle",
                        labels.call_form, i
                    ));
                };
                raw_ids.push(kh);
            }
            other => {
                return Err(format!(
                    "{}: edge selector element [{}] is not a Geometry sub-handle \
                     (got {:?}) — the edge selector must be a List of edge handles",
                    labels.call_form, i, other
                ));
            }
        }
    }
    let resolved = canonical_subhandle_ids(raw_ids);
    // ANTI-ZERO-EDGES: a present selector that resolves to ZERO edges must NEVER
    // silently fall through to the all-edges path (the task-3295 fake-done trap).
    // Emit a blocking E_EMPTY_SELECTION and return Err.
    if resolved.is_empty() {
        diagnostics.push(
            Diagnostic::error(format!(
                "{}: edge selector resolved to zero edges — refusing to silently {} all edges",
                labels.call_form, labels.verb
            ))
            .with_code(reify_core::DiagnosticCode::EmptyEdgeSelection),
        );
        return Err(format!(
            "{}: edge selector resolved to zero edges",
            labels.short
        ));
    }
    Ok(resolved)
}

/// Validate and convert a pattern count from f64 to usize.
///
/// Rejects non-positive values, non-integers, and values exceeding
/// a reasonable upper bound. Returns `Err` with a diagnostic when
/// the count is invalid.
fn validate_pattern_count(
    raw: f64,
    arg_name: &str,
    kind_label: impl std::fmt::Display,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<usize, String> {
    if raw < 1.0 {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} is less than 1 (must be a positive integer)",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: less than 1".to_string());
    }
    if raw != raw.floor() {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} is not an integer",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: not an integer".to_string());
    }
    if raw > 100_000.0 {
        diagnostics.push(Diagnostic::warning(format!(
            "pattern {} dropped: {}={} exceeds upper bound of 100000",
            kind_label, arg_name, raw
        )));
        return Err("invalid pattern count: exceeds upper bound".to_string());
    }
    Ok(raw as usize)
}

/// Extract three SI-valued `f64` components from a [`reify_ir::Value::Point`]
/// or [`reify_ir::Value::Vector`] with exactly 3 numeric, finite components.
///
/// Returns `None` if:
/// - the value is not a `Point` or `Vector`;
/// - it does not have exactly 3 components;
/// - any component does not yield a finite `f64` via [`reify_ir::Value::as_f64`].
///
/// Both `Point` (with LENGTH-dimensioned `Scalar` components — SI metres) and
/// `Vector` (with dimensionless `Real` components) pass through correctly
/// because `Value::as_f64` extracts `si_value` from `Scalar` and the raw
/// float from `Real`.
fn point3_components(value: &reify_ir::Value) -> Option<[f64; 3]> {
    let comps = match value {
        reify_ir::Value::Point(c) | reify_ir::Value::Vector(c) if c.len() == 3 => c,
        _ => return None,
    };
    let a = comps[0].as_f64().filter(|v| v.is_finite())?;
    let b = comps[1].as_f64().filter(|v| v.is_finite())?;
    let c = comps[2].as_f64().filter(|v| v.is_finite())?;
    Some([a, b, c])
}

/// Normalize a 3-component direction vector to unit length.
///
/// Returns `Err` when the vector magnitude is below [`GEOMETRY_EPSILON`]
/// (zero or near-zero), preventing a degenerate `[0,0,0]` normal from
/// propagating silently to the kernel.  The caller maps `Err(String)` to a
/// `Diagnostic::error` via the standard `Err(String)` → diagnostic idiom
/// (see `engine_build.rs`).
fn unit_vector3(v: [f64; 3]) -> Result<[f64; 3], String> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag < GEOMETRY_EPSILON {
        return Err(format!(
            "zero-magnitude vector [{:.6e}, {:.6e}, {:.6e}] cannot be normalized \
             to a unit direction",
            v[0], v[1], v[2]
        ));
    }
    Ok([v[0] / mag, v[1] / mag, v[2] / mag])
}

/// Decode a [`Value::Plane`] into `(origin, unit_normal)` — a pair of SI
/// metre triples returned as `([f64; 3], [f64; 3])`.
///
/// The normal is normalized to unit length.  Non-unit normals are accepted and
/// normalized silently (the plane equation is invariant to normal scale).
/// Zero-magnitude normals are always rejected.
///
/// # Returns
/// - `Ok((origin, unit_normal))` — origin in metres, normal dimensionless unit vector.
/// - `Err(message)` — for any of:
///   - wrong value variant (not `Value::Plane`), including `Value::Undef`;
///   - origin or normal with non-numeric / non-finite components;
///   - zero-magnitude normal.
///
/// # Visibility
/// `pub(crate)` — co-located with the mirror/circular_pattern eval consumers
/// and available to sibling modules in `reify-eval`.  Widened to `pub` only
/// when a cross-crate consumer lands (task 3465, design open).
pub(crate) fn decode_plane(value: &reify_ir::Value) -> Result<([f64; 3], [f64; 3]), String> {
    let (origin_val, normal_val) = match value {
        reify_ir::Value::Plane { origin, normal } => (origin.as_ref(), normal.as_ref()),
        other => {
            return Err(format!("expected a Plane value, got {}", other));
        }
    };
    let origin_arr = point3_components(origin_val).ok_or_else(|| {
        "Plane origin is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let normal_raw = point3_components(normal_val).ok_or_else(|| {
        "Plane normal is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let unit_normal =
        unit_vector3(normal_raw).map_err(|e| format!("Plane has a degenerate normal: {e}"))?;
    Ok((origin_arr, unit_normal))
}

/// Decode a [`Value::Axis`] into `(origin, unit_direction)` — a pair of SI
/// metre triples returned as `([f64; 3], [f64; 3])`.
///
/// The direction vector is normalized to unit length.  Non-unit directions are
/// accepted and normalized silently.  Zero-magnitude directions are rejected.
///
/// # Returns
/// - `Ok((origin, unit_direction))` — origin in metres, direction a
///   dimensionless unit vector.
/// - `Err(message)` — for any of:
///   - wrong value variant (not `Value::Axis`), including `Value::Undef`;
///   - origin or direction with non-numeric / non-finite components;
///   - zero-magnitude direction.
///
/// Reuses the private helpers [`point3_components`] and [`unit_vector3`] from
/// [`decode_plane`] — the single canonical decode surface for Axis values
/// (task η, design decision A).
///
/// # Visibility
/// `pub(crate)` — widened to `pub` only when a cross-crate consumer lands
/// (task 3465, design open).
// G-allow: same-file caller only; audit counts cross-file refs
pub(crate) fn decode_axis(value: &reify_ir::Value) -> Result<([f64; 3], [f64; 3]), String> {
    let (origin_val, dir_val) = match value {
        reify_ir::Value::Axis { origin, direction } => (origin.as_ref(), direction.as_ref()),
        other => {
            return Err(format!("expected an Axis value, got {}", other));
        }
    };
    let origin_arr = point3_components(origin_val)
        .ok_or_else(|| "Axis origin is not a valid 3-component numeric Point/Vector".to_string())?;
    let dir_raw = point3_components(dir_val).ok_or_else(|| {
        "Axis direction is not a valid 3-component numeric Point/Vector".to_string()
    })?;
    let unit_dir =
        unit_vector3(dir_raw).map_err(|e| format!("Axis has a degenerate direction: {e}"))?;
    Ok((origin_arr, unit_dir))
}

/// Convert a bare-numeric angle [`reify_ir::Value`] to radians, emitting a
/// deprecation warning diagnostic.
///
/// CAD convention: a bare `Real` or `Int` angle (no unit suffix in source) is
/// interpreted as degrees and converted to radians.  Values that already carry
/// an `ANGLE` dimension (from `deg` / `rad` suffixes) pass through unchanged.
///
/// Extracted to a shared free function to prevent verbatim duplication between
/// the value-form and scalar-form branches of the `circular_pattern` eval arm.
fn resolve_bare_angle(raw: reify_ir::Value, diagnostics: &mut Vec<Diagnostic>) -> reify_ir::Value {
    let as_deg: Option<f64> = match &raw {
        reify_ir::Value::Real(v) => Some(*v),
        reify_ir::Value::Int(i) => Some(*i as f64),
        _ => None,
    };
    if let Some(deg) = as_deg {
        let rad = deg * std::f64::consts::PI / 180.0;
        diagnostics.push(Diagnostic::warning(format!(
            "circular_pattern: bare numeric angle `{}` interpreted as {}°; \
             use `{}deg` or `{:.6}rad` for explicit units",
            deg, deg, deg, rad
        )));
        reify_ir::Value::angle(rad)
    } else {
        raw
    }
}

/// Translate a compiled geometry operation into a runtime `GeometryOp` by
/// evaluating its argument expressions against the current value environment.
///
/// # Failure semantics and the silent-defaults convention
///
/// Returns `Err(reason)` — rather than `Ok` with a fabricated default — when
/// evaluation is incomplete: a required argument is absent, a value is
/// non-finite, a `GeomRef` cannot be resolved, or an arm-level validation
/// guard fires (e.g. negative scale factor, degenerate extrude distance,
/// zero-length revolve axis).
///
/// This is the intentional, convention-aligned alternative to silent defaults
/// (see `review/briefing.yaml` line 9 and project norm
/// `feedback_silent_defaults_pattern`, which forbids patterns like
/// `unwrap_or(Value::Undef)` or `unwrap_or(0.0)` that silently fabricate a
/// plausible-but-wrong value).  An `Err` propagates "evaluation is
/// incomplete" to the caller without inventing geometry the user never asked for.
///
/// ## Warning-then-propagate discipline
///
/// The error is never *silent* at its origin point.  Before each `Err`
/// escapes, a `Warning`-severity `Diagnostic` is pushed (by the helpers
/// [`eval_named_arg`] / [`eval_named_arg_f64`] for missing or non-finite
/// args, or by the arm-level validation guards for semantic failures).  The
/// `Err(String)` is a short *summary* the caller uses for its one
/// `Error`-severity diagnostic; the `Warning` carries the full, per-argument
/// explanation.
///
/// # Ordering invariant for `functions`
///
/// `functions` is the slice of [`CompiledFunction`]s from the module.  The
/// evaluator passes the *full* module-level slice so that any expression
/// inside an op's args can reference user-defined functions by index.
/// Forward references within the same structure are resolved during
/// compilation (name → index), so the slice must preserve declaration order
/// to keep indices valid.  Callers that construct a partial functions slice
/// (e.g. for testing) must ensure indices in compiled expressions stay
/// in-bounds or the lookup will silently return `Value::Undef`.
pub(crate) fn compile_geometry_op(
    op: &reify_compiler::CompiledGeometryOp,
    values: &ValueMap,
    step_handles: &[GeometryHandleId],
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

    // Helper: resolve a GeomRef to a handle.
    //
    // GeomRef::Step(idx) — looks up in the per-realization step_handles slice.
    // GeomRef::Sub(name) — looks up in named_steps (name → handle built by the
    //   engine as each named realization completes).  On miss returns Err; no
    //   Warning diagnostic is emitted here — the caller (execute_realization_ops)
    //   emits a single Error-severity diagnostic per failed op.  This follows the
    //   "no Warning at origin, single Error at caller" convention documented in
    //   the `compile_geometry_op` doc-comment above.
    let resolve_geom_ref = |r: &GeomRef,
                            step_handles: &[GeometryHandleId]|
     -> Result<GeometryHandleId, String> {
        match r {
            GeomRef::Step(idx) => step_handles
                .get(*idx)
                .copied()
                .filter(|h| *h != GeometryHandleId::INVALID)
                .ok_or_else(|| {
                    format!(
                        "unresolvable GeomRef::Step({}) — index out of bounds or INVALID handle",
                        idx
                    )
                }),
            // GeomRef::Sub(name) — look up the handle in the caller-supplied
            // named_steps map.  The map is populated by the engine as each
            // named realization completes (see execute_realization_ops).
            //
            // No Warning diagnostic at origin: on miss this arm returns
            // Err(String) and emits NO diagnostic.  The caller
            // (execute_realization_ops) converts the Err into a single
            // Error-severity diagnostic per failed op, consistent with the
            // "no Warning at origin, single Error at caller" convention
            // documented in the `compile_geometry_op` doc-comment.
            // Pinned by compile_geometry_op_sub_ref_unknown_name_returns_err_no_warning.
            // GeomRef::Sub resolves via named_steps[name].  Two namespaces share
            // this arm — both are keyed by bare `name`, but their population sites differ:
            //
            //   • Bare key `"b"` (no '.'): same-structure sibling realization.
            //     `named_steps["b"]` is populated by the `b` realization's executor
            //     (engine_build.rs) before `f`'s executor runs.  The Kahn schedule
            //     (engine_fixpoint.rs) guarantees `b` precedes `f` via the explicit
            //     sibling→sibling realization edge added by task #4668 step-4
            //     (`resolve_sibling_ref` in deps.rs).  Emitted by the compiler's
            //     sibling pre-check (task #4668 step-2, geometry.rs).
            //
            //   • Compound key `"sub.member"` (contains '.'): cross-component
            //     reference (`self.<sub>.<member>`).  `named_steps["sub.member"]` is
            //     seeded by the child template's realization executor via the compound
            //     key injection path in engine_build.rs.
            //
            // Identifiers in the DSL never contain '.', so the two namespaces are
            // disjoint by construction — no collision is possible.
            //
            // Bare keys (0 dots) are same-structure siblings; compound keys (1 dot)
            // are cross-sub references.  Keys with 2+ dots, or leading/trailing
            // dots, cannot originate from the compiler (DSL identifiers contain
            // no '.'; compound keys are constructed as "sub"+"."+"member" by
            // `try_resolve_cross_sub_geom_ref`).
            GeomRef::Sub(name) => {
                debug_assert!(
                    name.matches('.').count() <= 1
                        && !name.starts_with('.')
                        && !name.ends_with('.'),
                    "GeomRef::Sub key '{}' is malformed: must be bare (0 dots, \
                     sibling realization) or compound (exactly 1 dot 'sub.member', \
                     cross-sub reference)",
                    name
                );
                named_steps
                    .get(name)
                    .map(|kh| kh.id)
                    .filter(|h| *h != GeometryHandleId::INVALID)
                    .ok_or_else(|| {
                        format!(
                            "unresolvable GeomRef::Sub('{}') — no such named sub-reference in scope",
                            name
                        )
                    })
            }
        }
    };

    match op {
        CompiledGeometryOp::Primitive { kind, args } => lookup_primitive(*kind)
            .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
            (kind, args, values, functions, meta_map, diagnostics),
        CompiledGeometryOp::Boolean { op, left, right } => {
            // Fail-fast: `?` on `left` short-circuits before `right` is resolved,
            // so at most one "unresolvable GeomRef::Step" Error surfaces per
            // Boolean op. Pinned by
            // `build_boolean_{union,difference,intersection}_unresolved_*_no_kernel_error`
            // in `tests/geometry_error_handling.rs`.
            let left_id = resolve_geom_ref(left, step_handles)?;
            let right_id = resolve_geom_ref(right, step_handles)?;
            match op {
                BooleanOp::Union => Ok(reify_ir::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Ok(reify_ir::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Ok(reify_ir::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            lookup_modify(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, target_id, step_handles, args, values, functions, meta_map, diagnostics)
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            lookup_transform(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, target_id, args, values, functions, meta_map, diagnostics)
        }
        CompiledGeometryOp::Pattern { kind, target, args } => {
            let target_id = resolve_geom_ref(target, step_handles)?;
            lookup_pattern(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, target_id, args, values, functions, meta_map, diagnostics)
        }
        CompiledGeometryOp::Sweep {
            kind,
            profiles,
            args,
        } => {
            lookup_sweep(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, profiles, step_handles, named_steps, args, values, functions, meta_map, diagnostics)
        }
        CompiledGeometryOp::Curve { kind, args } => {
            lookup_curve(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, args, values, functions, meta_map, diagnostics)
        }
        CompiledGeometryOp::Profile { kind, args } => {
            lookup_profile(*kind)
                .ok_or_else(|| format!("no registered compiler for {:?}", kind))?
                (kind, args, values, functions, meta_map, diagnostics)
        }
        // u_degree, v_degree) and constructs reify_ir::GeometryOp::NurbsSurface.
        CompiledGeometryOp::Surface { kind, args } => {
            use reify_compiler::SurfaceKind;
            match kind {
                SurfaceKind::Nurbs => {
                    // Evaluate all 6 named args to Values (sequential, each borrow
                    // of `diagnostics` ends before the next call).
                    let cp_val = eval_named_arg(
                        "control_points", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing control_points argument".to_string())?;
                    let w_val = eval_named_arg(
                        "weights", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing weights argument".to_string())?;
                    let uk_val = eval_named_arg(
                        "u_knots", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing u_knots argument".to_string())?;
                    let vk_val = eval_named_arg(
                        "v_knots", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing v_knots argument".to_string())?;
                    let ud_val = eval_named_arg(
                        "u_degree", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing u_degree argument".to_string())?;
                    let vd_val = eval_named_arg(
                        "v_degree", kind, args, values, functions, meta_map, diagnostics,
                    )
                    .ok_or_else(|| "nurbs_surface: missing v_degree argument".to_string())?;

                    // Decode control_points: Value::List(rows) → Vec<Vec<[f64; 3]>>.
                    // Each inner element is decoded via point3_components (SI metres).
                    let cp_rows = match cp_val {
                        reify_ir::Value::List(rows) => rows,
                        other => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: control_points must be a List of rows, got {:?}",
                                other
                            )));
                            return Err(
                                "nurbs_surface: control_points is not a List".to_string()
                            );
                        }
                    };
                    let control_points: Vec<Vec<[f64; 3]>> = cp_rows
                        .iter()
                        .enumerate()
                        .map(|(ri, rv)| -> Result<Vec<[f64; 3]>, String> {
                            match rv {
                                reify_ir::Value::List(pts) => pts
                                    .iter()
                                    .enumerate()
                                    .map(|(ci, pt)| {
                                        point3_components(pt).ok_or_else(|| {
                                            format!(
                                                "nurbs_surface: control_points[{}][{}] must be \
                                                 a Point3<Length>, got {:?}",
                                                ri, ci, pt
                                            )
                                        })
                                    })
                                    .collect(),
                                other => Err(format!(
                                    "nurbs_surface: control_points row {} must be a List of \
                                     points, got {:?}",
                                    ri, other
                                )),
                            }
                        })
                        .collect::<Result<_, _>>()?;

                    // Validate grid shape (non-empty + rectangular).
                    let n_u = control_points.len();
                    if n_u == 0 {
                        diagnostics.push(Diagnostic::error(
                            "nurbs_surface: control_points grid must be non-empty".to_string(),
                        ));
                        return Err(
                            "nurbs_surface: control_points grid has zero rows".to_string()
                        );
                    }
                    let n_v = control_points[0].len();
                    if n_v == 0 {
                        diagnostics.push(Diagnostic::error(
                            "nurbs_surface: control_points rows must be non-empty".to_string(),
                        ));
                        return Err(
                            "nurbs_surface: control_points grid has zero columns".to_string()
                        );
                    }
                    for (i, row) in control_points.iter().enumerate() {
                        if row.len() != n_v {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: control_points row {} has {} points, expected \
                                 {} (grid must be rectangular)",
                                i,
                                row.len(),
                                n_v
                            )));
                            return Err(
                                "nurbs_surface: non-rectangular control_points".to_string()
                            );
                        }
                    }

                    // Decode weights: Value::List(rows) → Vec<Vec<f64>>.
                    let w_rows = match w_val {
                        reify_ir::Value::List(rows) => rows,
                        other => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: weights must be a List of rows, got {:?}",
                                other
                            )));
                            return Err("nurbs_surface: weights is not a List".to_string());
                        }
                    };
                    let weights: Vec<Vec<f64>> = w_rows
                        .iter()
                        .enumerate()
                        .map(|(ri, rv)| -> Result<Vec<f64>, String> {
                            match rv {
                                reify_ir::Value::List(wts) => wts
                                    .iter()
                                    .enumerate()
                                    .map(|(ci, w)| {
                                        w.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                                            format!(
                                                "nurbs_surface: weights[{}][{}] must be \
                                                 a finite scalar, got {:?}",
                                                ri, ci, w
                                            )
                                        })
                                    })
                                    .collect(),
                                other => Err(format!(
                                    "nurbs_surface: weights row {} must be a List, got {:?}",
                                    ri, other
                                )),
                            }
                        })
                        .collect::<Result<_, _>>()?;

                    // Validate weights shape matches control_points shape.
                    if weights.len() != n_u {
                        diagnostics.push(Diagnostic::error(format!(
                            "nurbs_surface: weights has {} rows, expected {} (must match \
                             control_points)",
                            weights.len(),
                            n_u
                        )));
                        return Err("nurbs_surface: weights row count mismatch".to_string());
                    }
                    for (i, row) in weights.iter().enumerate() {
                        if row.len() != n_v {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: weights row {} has {} elements, expected {} \
                                 (must match control_points)",
                                i,
                                row.len(),
                                n_v
                            )));
                            return Err("nurbs_surface: weights shape mismatch".to_string());
                        }
                    }

                    // Decode u_knots: Value::List → Vec<f64>.
                    let u_knots: Vec<f64> = match uk_val {
                        reify_ir::Value::List(ks) => ks
                            .iter()
                            .enumerate()
                            .map(|(i, k)| {
                                k.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                                    format!(
                                        "nurbs_surface: u_knots[{}] must be a finite scalar, \
                                         got {:?}",
                                        i, k
                                    )
                                })
                            })
                            .collect::<Result<_, _>>()?,
                        other => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: u_knots must be a List, got {:?}",
                                other
                            )));
                            return Err("nurbs_surface: u_knots is not a List".to_string());
                        }
                    };

                    // Decode v_knots: Value::List → Vec<f64>.
                    let v_knots: Vec<f64> = match vk_val {
                        reify_ir::Value::List(ks) => ks
                            .iter()
                            .enumerate()
                            .map(|(i, k)| {
                                k.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
                                    format!(
                                        "nurbs_surface: v_knots[{}] must be a finite scalar, \
                                         got {:?}",
                                        i, k
                                    )
                                })
                            })
                            .collect::<Result<_, _>>()?,
                        other => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: v_knots must be a List, got {:?}",
                                other
                            )));
                            return Err("nurbs_surface: v_knots is not a List".to_string());
                        }
                    };

                    // Decode u_degree: Value::Int (>= 1) → usize.
                    let u_degree = match ud_val {
                        reify_ir::Value::Int(d) if d >= 1 => d as usize,
                        reify_ir::Value::Int(d) => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: u_degree must be >= 1, got {}",
                                d
                            )));
                            return Err(format!("nurbs_surface: u_degree {} is invalid", d));
                        }
                        other => {
                            diagnostics.push(Diagnostic::error(
                                "nurbs_surface: u_degree must be an integer".to_string(),
                            ));
                            return Err(format!("nurbs_surface: u_degree is {:?}", other));
                        }
                    };

                    // Decode v_degree: Value::Int (>= 1) → usize.
                    let v_degree = match vd_val {
                        reify_ir::Value::Int(d) if d >= 1 => d as usize,
                        reify_ir::Value::Int(d) => {
                            diagnostics.push(Diagnostic::error(format!(
                                "nurbs_surface: v_degree must be >= 1, got {}",
                                d
                            )));
                            return Err(format!("nurbs_surface: v_degree {} is invalid", d));
                        }
                        other => {
                            diagnostics.push(Diagnostic::error(
                                "nurbs_surface: v_degree must be an integer".to_string(),
                            ));
                            return Err(format!("nurbs_surface: v_degree is {:?}", other));
                        }
                    };

                    Ok(reify_ir::GeometryOp::NurbsSurface {
                        control_points,
                        weights,
                        u_knots,
                        v_knots,
                        u_degree,
                        v_degree,
                    })
                }
            }
        }
        // isosurface(grid, iso?, adaptive?) → GeometryOp::Surface { grid,
        // iso_level, adaptive } — marching-cubes extraction from a Voxel-repr
        // grid operand. `iso`/`adaptive` are optional: absence is the normal,
        // expected shape (mirroring the `edges`/`faces`/`third` optional-arg
        // convention — e.g. `modify_offset_curve`'s `third_expr` lookup above),
        // so they are read directly rather than through `eval_named_arg`'s
        // "missing required argument" Warning path. Defaults: iso_level=0.0
        // exactly, adaptive=false.
        CompiledGeometryOp::Isosurface { grid, args } => {
            let grid_id = resolve_geom_ref(grid, step_handles)?;

            let iso_level = match args.iter().find(|(n, _)| n == "iso").map(|(_, e)| e) {
                None => 0.0,
                Some(expr) => {
                    let v = reify_expr::eval_expr(
                        expr,
                        &eval_ctx_with_meta(values, functions, meta_map),
                    );
                    v.as_f64().unwrap_or_else(|| {
                        diagnostics.push(Diagnostic::warning(
                            "isosurface: 'iso' argument evaluated to a non-numeric \
                             value — defaulting to 0.0"
                                .to_string(),
                        ));
                        0.0
                    })
                }
            };

            let adaptive = match args.iter().find(|(n, _)| n == "adaptive").map(|(_, e)| e) {
                None => false,
                Some(expr) => {
                    let v = reify_expr::eval_expr(
                        expr,
                        &eval_ctx_with_meta(values, functions, meta_map),
                    );
                    match v {
                        reify_ir::Value::Bool(b) => b,
                        _ => {
                            diagnostics.push(Diagnostic::warning(
                                "isosurface: 'adaptive' argument evaluated to a \
                                 non-Bool value — defaulting to false"
                                    .to_string(),
                            ));
                            false
                        }
                    }
                }
            };

            Ok(reify_ir::GeometryOp::Surface {
                grid: grid_id,
                iso_level,
                adaptive,
            })
        }
    }
}

// ── L5 Axis-3 fn-table dispatch ─────────────────────────────────────────────
//
// Free fn versions of each compile_geometry_op match arm, fn-type aliases
// for each family, static dispatch tables, and lookup helpers.
// Wired into compile_geometry_op via lookup_X dispatch (step-4).

// ── resolve_geom_ref_impl (free-fn version) ──────────────────────────────────

fn resolve_geom_ref_impl(
    r: &reify_compiler::GeomRef,
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
) -> Result<GeometryHandleId, String> {
    match r {
        reify_compiler::GeomRef::Step(idx) => step_handles
            .get(*idx)
            .copied()
            .filter(|h| *h != GeometryHandleId::INVALID)
            .ok_or_else(|| {
                format!(
                    "unresolvable GeomRef::Step({}) — index out of bounds or INVALID handle",
                    idx
                )
            }),
        reify_compiler::GeomRef::Sub(name) => {
            debug_assert!(
                name.matches('.').count() <= 1
                    && !name.starts_with('.')
                    && !name.ends_with('.'),
                "GeomRef::Sub key '{}' is malformed: must be bare (0 dots, \
                 sibling realization) or compound (exactly 1 dot 'sub.member', \
                 cross-sub reference)",
                name
            );
            named_steps
                .get(name)
                .map(|kh| kh.id)
                .filter(|h| *h != GeometryHandleId::INVALID)
                .ok_or_else(|| {
                    format!(
                        "unresolvable GeomRef::Sub('{}') — no such named sub-reference in scope",
                        name
                    )
                })
        }
    }
}

// ── fn-type aliases ──────────────────────────────────────────────────────────

type PrimitiveCompileFn = fn(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type ModifyCompileFn = fn(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type TransformCompileFn = fn(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type PatternCompileFn = fn(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type SweepCompileFn = fn(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type CurveCompileFn = fn(
    kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

type ProfileCompileFn = fn(
    kind: &reify_compiler::ProfileKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String>;

// ── Primitive fns ─────────────────────────────────────────────────────────────

fn prim_box(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Box {
        width: eval_arg("width")?,
        height: eval_arg("height")?,
        depth: eval_arg("depth")?,
    })
}

fn prim_cylinder(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Cylinder {
        radius: eval_arg("radius")?,
        height: eval_arg("height")?,
    })
}

fn prim_sphere(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Sphere {
        radius: eval_arg("radius")?,
    })
}

fn prim_tube(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Tube {
        outer_r: eval_arg("outer_r")?,
        inner_r: eval_arg("inner_r")?,
        height: eval_arg("height")?,
    })
}

fn prim_cone(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Cone {
        bottom_radius: eval_arg("bottom_radius")?,
        top_radius: eval_arg("top_radius")?,
        height: eval_arg("height")?,
    })
}

fn prim_wedge(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Wedge {
        width: eval_arg("width")?,
        depth: eval_arg("depth")?,
        height: eval_arg("height")?,
        top_width: eval_arg("top_width")?,
    })
}

fn prim_torus(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::Torus {
        major_radius: eval_arg("major_radius")?,
        minor_radius: eval_arg("minor_radius")?,
    })
}

fn prim_half_space(
    kind: &reify_compiler::PrimitiveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::HalfSpace {
        px: eval_arg("px")?,
        py: eval_arg("py")?,
        pz: eval_arg("pz")?,
        nx: eval_arg("nx")?,
        ny: eval_arg("ny")?,
        nz: eval_arg("nz")?,
    })
}

// ── Modify fns ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn modify_fillet(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let radius = eval_arg("radius")?;
    let edges_expr = args.iter().find(|(n, _)| n == "edges").map(|(_, e)| e);
    match edges_expr {
        None => Ok(reify_ir::GeometryOp::Fillet {
            target: target_id,
            edges: vec![],
            radius,
        }),
        Some(expr) => {
            let edges_val = reify_expr::eval_expr(
                expr,
                &eval_ctx_with_meta(values, functions, meta_map),
            );
            let edges = resolve_curated_edges_p2(
                &edges_val,
                CuratedEdgeLabels::FILLET,
                diagnostics,
            )?;
            Ok(reify_ir::GeometryOp::Fillet {
                target: target_id,
                edges,
                radius,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn modify_chamfer(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let distance = eval_arg("distance")?;
    let edges_expr = args.iter().find(|(n, _)| n == "edges").map(|(_, e)| e);
    match edges_expr {
        None => Ok(reify_ir::GeometryOp::Chamfer {
            target: target_id,
            edges: vec![],
            distance,
        }),
        Some(expr) => {
            let edges_val = reify_expr::eval_expr(
                expr,
                &eval_ctx_with_meta(values, functions, meta_map),
            );
            let edges = resolve_curated_edges_p2(
                &edges_val,
                CuratedEdgeLabels::CHAMFER,
                diagnostics,
            )?;
            Ok(reify_ir::GeometryOp::Chamfer {
                target: target_id,
                edges,
                distance,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn modify_chamfer_asymmetric(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let d1 = eval_arg("d1")?;
    let d2 = eval_arg("d2")?;
    let edges_expr = args.iter().find(|(n, _)| n == "edges").map(|(_, e)| e);
    match edges_expr {
        None => Ok(reify_ir::GeometryOp::ChamferAsymmetric {
            target: target_id,
            edges: vec![],
            d1,
            d2,
        }),
        Some(expr) => {
            let edges_val = reify_expr::eval_expr(
                expr,
                &eval_ctx_with_meta(values, functions, meta_map),
            );
            let edges = resolve_curated_edges_p2(
                &edges_val,
                CuratedEdgeLabels::CHAMFER_ASYMMETRIC,
                diagnostics,
            )?;
            Ok(reify_ir::GeometryOp::ChamferAsymmetric {
                target: target_id,
                edges,
                d1,
                d2,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn modify_shell(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let thickness = eval_arg("thickness")?;
    let open_faces_expr =
        args.iter().find(|(n, _)| n == "open_faces").map(|(_, e)| e);
    if let Some(expr) = open_faces_expr {
        let faces_val = reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, functions, meta_map),
        );
        match &faces_val {
            reify_ir::Value::List(elems) => {
                let mut raw_ids: Vec<GeometryHandleId> =
                    Vec::with_capacity(elems.len());
                for (i, e) in elems.iter().enumerate() {
                    match e {
                        reify_ir::Value::GeometryHandle {
                            kernel_handle,
                            ..
                        } => {
                            let Some(kh) = *kernel_handle else {
                                return Err(format!(
                                    "shell_open(solid, thickness, open_faces): \
                                     face selector element [{}] is a symbolic \
                                     (unrealized) handle — face selection \
                                     requires a realized geometry handle",
                                    i
                                ));
                            };
                            raw_ids.push(kh);
                        }
                        other => {
                            return Err(format!(
                                "shell_open(solid, thickness, open_faces): \
                                 face selector element [{}] is not a Geometry \
                                 sub-handle (got {:?}) — the open_faces \
                                 selector must be a List of face handles",
                                i, other
                            ));
                        }
                    }
                }
                let resolved = canonical_subhandle_ids(raw_ids);
                if resolved.is_empty() {
                    diagnostics.push(
                        Diagnostic::error(
                            "shell_open(solid, thickness, open_faces): \
                             face selector resolved to zero faces — \
                             refusing to silently shell all faces",
                        )
                        .with_code(
                            reify_core::DiagnosticCode::EmptyEdgeSelection,
                        ),
                    );
                    return Err(
                        "shell_open: face selector resolved to zero faces"
                            .to_string(),
                    );
                }
                return Ok(reify_ir::GeometryOp::Shell {
                    target: target_id,
                    thickness,
                    faces_to_remove: vec![],
                    open_face_handles: resolved,
                });
            }
            other => {
                return Err(format!(
                    "shell_open(solid, thickness, open_faces): curated \
                     face selection is not yet available on the current \
                     build pipeline — the face selector cannot be resolved \
                     at the point this shell_open runs. Use numeric \
                     shell(solid, thickness, face_N) to remove specific \
                     faces by index, or wait for curated face selection \
                     (engine-unified-build-dag tasks 4360/4358). \
                     [face selector evaluated to {:?}]",
                    other
                ));
            }
        }
    }
    let mut faces_to_remove: Vec<usize> = Vec::new();
    for (name, expr) in args.iter().filter(|(n, _)| n.starts_with("face_")) {
        let val = reify_expr::eval_expr(
            expr,
            &eval_ctx_with_meta(values, functions, meta_map),
        );
        match val.as_f64() {
            None => {
                diagnostics.push(Diagnostic::warning(format!(
                    "Shell face index '{}' is non-numeric — skipped",
                    name
                )));
            }
            Some(f) if !f.is_finite() => {
                diagnostics.push(Diagnostic::warning(format!(
                    "Shell face index '{}' is non-finite ({}) — skipped",
                    name, f
                )));
            }
            Some(f) if f < 0.0 => {
                diagnostics.push(Diagnostic::warning(format!(
                    "Shell face index '{}' is negative ({}) — skipped",
                    name, f
                )));
            }
            Some(f) if f != f.floor() => {
                diagnostics.push(Diagnostic::warning(format!(
                    "Shell face index '{}' is not an integer ({}) — skipped",
                    name, f
                )));
            }
            Some(f) if f > 1_000_000.0 => {
                diagnostics.push(Diagnostic::warning(format!(
                    "Shell face index '{}' exceeds upper bound of 1000000 ({}) — skipped",
                    name, f
                )));
            }
            Some(f) => {
                faces_to_remove.push(f as usize);
            }
        }
    }
    Ok(reify_ir::GeometryOp::Shell {
        target: target_id,
        thickness,
        faces_to_remove,
        open_face_handles: vec![],
    })
}

#[allow(clippy::too_many_arguments)]
fn modify_draft(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let angle = eval_arg("angle")?;
    // plane is resolved via step_handles.last() (a pre-existing approximation —
    // plane_xy yields a Value::Plane, not a sub-op; the full plane-handle plumbing
    // fix is out of scope for δ). Filter INVALID so a preceding compile failure
    // (sentinel) propagates as Err rather than forwarding INVALID to the kernel.
    let plane_id = step_handles
        .last()
        .copied()
        .filter(|h| *h != GeometryHandleId::INVALID)
        .ok_or_else(|| "no valid plane handle available for Draft".to_string())?;
    let faces_expr = args.iter().find(|(n, _)| n == "faces").map(|(_, e)| e);
    match faces_expr {
        None => Ok(reify_ir::GeometryOp::Draft {
            target: target_id,
            faces: vec![],
            angle,
            plane: plane_id,
        }),
        Some(expr) => {
            let faces_val = reify_expr::eval_expr(
                expr,
                &eval_ctx_with_meta(values, functions, meta_map),
            );
            match &faces_val {
                reify_ir::Value::List(elems) => {
                    let mut raw_ids: Vec<GeometryHandleId> =
                        Vec::with_capacity(elems.len());
                    for (i, e) in elems.iter().enumerate() {
                        match e {
                            reify_ir::Value::GeometryHandle {
                                kernel_handle,
                                ..
                            } => {
                                let Some(kh) = *kernel_handle else {
                                    return Err(format!(
                                        "draft(solid, faces, angle, neutral_plane): \
                                         face selector element [{}] is a symbolic \
                                         (unrealized) handle — face selection \
                                         requires a realized geometry handle",
                                        i
                                    ));
                                };
                                raw_ids.push(kh);
                            }
                            other => {
                                return Err(format!(
                                    "draft(solid, faces, angle, neutral_plane): \
                                     face selector element [{}] is not a Geometry \
                                     sub-handle (got {:?}) — the face selector \
                                     must be a List of face handles",
                                    i, other
                                ));
                            }
                        }
                    }
                    let resolved = canonical_subhandle_ids(raw_ids);
                    if resolved.is_empty() {
                        diagnostics.push(
                            Diagnostic::error(
                                "draft(solid, faces, angle, neutral_plane): \
                                 face selector resolved to zero faces — refusing \
                                 to silently draft all faces",
                            )
                            .with_code(
                                reify_core::DiagnosticCode::EmptyEdgeSelection,
                            ),
                        );
                        return Err("draft: face selector resolved to zero faces"
                            .to_string());
                    }
                    Ok(reify_ir::GeometryOp::Draft {
                        target: target_id,
                        faces: resolved,
                        angle,
                        plane: plane_id,
                    })
                }
                other => Err(format!(
                    "draft(solid, faces, angle, neutral_plane): curated \
                     face selection is not yet available on the current \
                     build pipeline — the face selector cannot be resolved \
                     at the point this draft runs. Use 3-arg \
                     draft(solid, angle, neutral_plane) to draft all \
                     faces, or wait for curated face selection \
                     (engine-unified-build-dag tasks 4360/4358). \
                     [face selector evaluated to {:?}]",
                    other
                )),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn modify_thicken(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let offset = eval_arg("offset")?;
    Ok(reify_ir::GeometryOp::Thicken {
        target: target_id,
        offset,
    })
}

#[allow(clippy::too_many_arguments)]
fn modify_zone_slab(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let width = eval_arg("width")?;
    Ok(reify_ir::GeometryOp::ZoneSlab {
        target: target_id,
        width,
    })
}

#[allow(clippy::too_many_arguments)]
fn modify_offset_solid(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let distance = eval_arg("distance")?;
    Ok(reify_ir::GeometryOp::OffsetSolid {
        target: target_id,
        distance,
    })
}

#[allow(clippy::too_many_arguments)]
fn modify_offset_curve(
    kind: &reify_compiler::ModifyKind,
    target_id: GeometryHandleId,
    _step_handles: &[GeometryHandleId],
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    let distance = eval_arg("distance")?;
    let third_expr = args.iter().find(|(n, _)| n == "third").map(|(_, e)| e);
    let (reference, direction) = match third_expr {
        None => (None, None),
        Some(expr) => {
            if let Some((_, _, kernel_handle)) =
                resolve_parent_geometry_handle_arg(expr, values)
            {
                (Some(kernel_handle), None)
            } else {
                let v = reify_expr::eval_expr(
                    expr,
                    &eval_ctx_with_meta(values, functions, meta_map),
                );
                match point3_components(&v) {
                    Some(dir) => (None, Some(dir)),
                    None => {
                        diagnostics.push(Diagnostic::warning(
                            "offset_curve: 3rd argument is neither a reference \
                             Surface (bound geometry handle) nor a direction \
                             vec3 — building a planar offset and ignoring it"
                                .to_string(),
                        ));
                        (None, None)
                    }
                }
            }
        }
    };
    Ok(reify_ir::GeometryOp::OffsetCurve {
        target: target_id,
        distance,
        reference,
        direction,
    })
}

// ── Transform fns ─────────────────────────────────────────────────────────────

fn transform_translate(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
    };
    Ok(reify_ir::GeometryOp::Translate {
        target: target_id,
        dx: f64_arg("dx")?,
        dy: f64_arg("dy")?,
        dz: f64_arg("dz")?,
    })
}

fn transform_rotate(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    if args.iter().any(|(n, _)| n == "orientation") {
        let v = eval_named_arg("orientation", kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| "rotate: 'orientation' arg is missing".to_string())?;
        match decode_orientation_to_axis_angle(&v) {
            Some((axis, angle_rad)) => Ok(reify_ir::GeometryOp::Rotate {
                target: target_id,
                axis,
                angle_rad,
            }),
            None => {
                diagnostics.push(Diagnostic::warning(
                    "rotate dropped: 'orientation' arg is not a valid Orientation<3>"
                        .to_string(),
                ));
                Err("rotate: 'orientation' arg is not a valid Orientation<3>".into())
            }
        }
    } else {
        let mut f64_arg = |name: &str| -> Result<f64, String> {
            eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
                .ok_or_else(|| {
                    format!("missing or non-finite argument '{}' for {}", name, kind)
                })
        };
        Ok(reify_ir::GeometryOp::Rotate {
            target: target_id,
            axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
            angle_rad: f64_arg("angle")?,
        })
    }
}

fn transform_scale(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
    };
    let factor = f64_arg("factor")?;
    if factor < 0.0 {
        diagnostics.push(Diagnostic::warning(format!(
            "scale dropped: factor={} is negative (must be positive)",
            factor
        )));
        return Err("scale factor is negative".into());
    }
    if factor == 0.0 {
        diagnostics.push(Diagnostic::warning(
            "scale dropped: factor=0 produces degenerate \
             (zero-volume) geometry (must be > 0)"
                .to_string(),
        ));
        return Err("scale factor is zero (degenerate)".into());
    }
    Ok(reify_ir::GeometryOp::Scale {
        target: target_id,
        factor,
    })
}

fn transform_rotate_around(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
    };
    Ok(reify_ir::GeometryOp::RotateAround {
        target: target_id,
        point: [f64_arg("px")?, f64_arg("py")?, f64_arg("pz")?],
        axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
        angle_rad: f64_arg("angle")?,
    })
}

fn transform_apply(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    match eval_named_arg(
        "transform",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    ) {
        Some(v) => match decompose_transform_to_arrays(&v) {
            Some((rotation, translation)) => {
                Ok(reify_ir::GeometryOp::ApplyTransform {
                    target: target_id,
                    rotation,
                    translation,
                })
            }
            None => {
                diagnostics.push(Diagnostic::warning(
                    "apply_transform dropped: 'transform' arg is not a valid Transform<3>"
                        .to_string(),
                ));
                Err(
                    "apply_transform: 'transform' arg is not a valid Transform<3>"
                        .into(),
                )
            }
        },
        None => {
            Err("apply_transform: 'transform' arg is missing".into())
        }
    }
}

/// Sarrus / cofactor-expansion determinant of a row-major 3×3 matrix.
///
/// Mirrors `reify_stdlib::matrix::mat3_det` (the production formula backing
/// the `determinant` builtin's `AffineMap` arm), which is `pub(crate)` to
/// that crate — this local copy keeps `transform_affine_apply`'s singular-map
/// guard inside reify-eval, same rationale as `decompose_transform_to_arrays`.
/// `affine_apply_linear_det_matches_stdlib_determinant_builtin` (below) is a
/// cross-check test pinning the two formulas to agree, since they cannot
/// share a single source of truth across the crate boundary.
fn affine_apply_linear_det(m: [[f64; 3]; 3]) -> f64 {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
}

fn transform_affine_apply(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    match eval_named_arg(
        "map",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    ) {
        Some(reify_ir::Value::AffineMap { linear, translation }) => {
            // Epsilon (not exact `== 0.0`) guard: a near-singular linear part
            // (e.g. det ~ 1e-300, or a matrix degenerate only up to
            // floating-point round-off) is nonzero and would otherwise slip
            // past an exact-equality check straight through to OCCT's
            // gp_GTrsf/BRepBuilderAPI_GTransform, surfacing as a raw
            // OperationFailed error (or worse, invalid/non-manifold geometry)
            // instead of this graceful diagnostic drop.
            const SINGULAR_DET_EPSILON: f64 = 1e-12;
            if affine_apply_linear_det(linear).abs() < SINGULAR_DET_EPSILON {
                diagnostics.push(Diagnostic::warning(
                    "affine_apply dropped: linear part is singular (det=0)".to_string(),
                ));
                return Err("affine_apply: linear part is singular (det=0)".into());
            }
            Ok(reify_ir::GeometryOp::AffineApply {
                target: target_id,
                linear,
                translation,
            })
        }
        Some(_) => {
            diagnostics.push(Diagnostic::warning(
                "affine_apply dropped: 'map' arg is not a valid AffineMap".to_string(),
            ));
            Err("affine_apply: 'map' arg is not a valid AffineMap".into())
        }
        None => Err("affine_apply: 'map' arg is missing".into()),
    }
}

/// Lower the per-axis (non-rigid) `scale(geometry, factors: Vector3<Real>)`
/// overload to `GeometryOp::ScaleNonUniform`. Mirrors `transform_affine_apply`:
/// evaluate the `factors` arg, decode it to `[sx, sy, sz]`, and reject
/// non-finite or zero components with a graceful "scale dropped" diagnostic
/// (Err) rather than letting them reach the kernel. Negative components
/// (reflections) are valid and pass through unchanged.
fn transform_scale_non_uniform(
    kind: &reify_compiler::TransformKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let value = eval_named_arg(
        "factors",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| "scale: 'factors' arg is missing".to_string())?;

    let components = match &value {
        reify_ir::Value::Vector(items) if items.len() == 3 => {
            match (
                vec3_component_si(&items[0]),
                vec3_component_si(&items[1]),
                vec3_component_si(&items[2]),
            ) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            }
        }
        _ => None,
    };

    let [sx, sy, sz] = match components {
        Some(c) => c,
        None => {
            diagnostics.push(Diagnostic::warning(
                "scale dropped: 'factors' arg is not a valid Vec3 of dimensionless reals"
                    .to_string(),
            ));
            return Err("scale: 'factors' arg is not a valid Vec3".into());
        }
    };

    if !sx.is_finite() || !sy.is_finite() || !sz.is_finite() || sx == 0.0 || sy == 0.0 || sz == 0.0
    {
        diagnostics.push(Diagnostic::warning(format!(
            "scale dropped: factors=({sx}, {sy}, {sz}) must be finite and non-zero \
             (produces degenerate zero-volume geometry otherwise)"
        )));
        return Err(format!(
            "scale factors must be finite and non-zero, got ({sx}, {sy}, {sz})"
        ));
    }

    Ok(reify_ir::GeometryOp::ScaleNonUniform {
        target: target_id,
        sx,
        sy,
        sz,
    })
}

// ── Pattern fns ───────────────────────────────────────────────────────────────

fn pattern_linear(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    let direction = [f64_arg("dx")?, f64_arg("dy")?, f64_arg("dz")?];
    let count_raw = f64_arg("count")?;
    let count = validate_pattern_count(count_raw, "count", kind, diagnostics)?;
    let spacing = eval_named_arg(
        "spacing",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'spacing' for {}", kind))?;
    Ok(reify_ir::GeometryOp::LinearPattern {
        target: target_id,
        direction,
        count,
        spacing,
    })
}

fn pattern_circular(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    if args.iter().any(|(n, _)| n == "axis") {
        let axis_val = eval_named_arg(
            "axis",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| format!("missing required argument 'axis' for {}", kind))?;
        let (axis_origin, axis_dir) = decode_axis(&axis_val)
            .map_err(|e| format!("circular_pattern: {}", e))?;
        let count_raw = eval_named_arg_f64(
            "count",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument 'count' for {}", kind)
        })?;
        let count = validate_pattern_count(count_raw, "count", kind, diagnostics)?;
        let raw_angle = eval_named_arg(
            "angle",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| format!("missing required argument 'angle' for {}", kind))?;
        let angle = resolve_bare_angle(raw_angle, diagnostics);
        Ok(reify_ir::GeometryOp::CircularPattern {
            target: target_id,
            axis_origin,
            axis_dir,
            count,
            angle,
        })
    } else {
        let mut f64_arg = |name: &str| -> Result<f64, String> {
            eval_named_arg_f64(
                name,
                kind,
                args,
                values,
                functions,
                meta_map,
                diagnostics,
            )
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
        };
        let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
        let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
        let count_raw = f64_arg("count")?;
        let count = validate_pattern_count(count_raw, "count", kind, diagnostics)?;
        let raw_angle = eval_named_arg(
            "angle",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| format!("missing required argument 'angle' for {}", kind))?;
        let angle = resolve_bare_angle(raw_angle, diagnostics);
        Ok(reify_ir::GeometryOp::CircularPattern {
            target: target_id,
            axis_origin,
            axis_dir,
            count,
            angle,
        })
    }
}

fn pattern_mirror(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    if args.iter().any(|(n, _)| n == "plane") {
        let plane_val = eval_named_arg(
            "plane",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| format!("missing required argument 'plane' for {}", kind))?;
        let (plane_origin, plane_normal) =
            decode_plane(&plane_val).map_err(|e| format!("mirror: {}", e))?;
        Ok(reify_ir::GeometryOp::Mirror {
            target: target_id,
            plane_origin,
            plane_normal,
        })
    } else {
        let mut f64_arg = |name: &str| -> Result<f64, String> {
            eval_named_arg_f64(
                name,
                kind,
                args,
                values,
                functions,
                meta_map,
                diagnostics,
            )
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
        };
        Ok(reify_ir::GeometryOp::Mirror {
            target: target_id,
            plane_origin: [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?],
            plane_normal: [f64_arg("nx")?, f64_arg("ny")?, f64_arg("nz")?],
        })
    }
}

fn pattern_linear2d(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    let direction1 = [f64_arg("dx1")?, f64_arg("dy1")?, f64_arg("dz1")?];
    let count1_raw = f64_arg("count1")?;
    let count1 = validate_pattern_count(count1_raw, "count1", kind, diagnostics)?;
    let spacing1 = eval_named_arg(
        "spacing1",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'spacing1' for {}", kind))?;
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    let direction2 = [f64_arg("dx2")?, f64_arg("dy2")?, f64_arg("dz2")?];
    let count2_raw = f64_arg("count2")?;
    let count2 = validate_pattern_count(count2_raw, "count2", kind, diagnostics)?;
    let spacing2 = eval_named_arg(
        "spacing2",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'spacing2' for {}", kind))?;
    Ok(reify_ir::GeometryOp::LinearPattern2D {
        target: target_id,
        direction1,
        count1,
        spacing1,
        direction2,
        count2,
        spacing2,
    })
}

fn pattern_arbitrary(
    kind: &reify_compiler::PatternKind,
    target_id: GeometryHandleId,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    // List form: arbitrary_pattern(target, transforms: List<Transform<3>>).
    // Each element decodes via decompose_transform_to_arrays (same decode
    // transform_apply uses for a single Transform<3>); any invalid element,
    // non-List value, or empty list is a graceful drop (Warning + Err),
    // mirroring transform_apply's convention rather than fabricating a
    // default transform.
    if args.iter().any(|(name, _)| name == "transform_list") {
        let value = eval_named_arg(
            "transform_list",
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| "arbitrary_pattern: 'transform_list' arg is missing".to_string())?;
        let reify_ir::Value::List(elements) = &value else {
            diagnostics.push(Diagnostic::warning(
                "arbitrary_pattern dropped: 'transform_list' arg is not a List<Transform<3>>"
                    .to_string(),
            ));
            return Err(
                "arbitrary_pattern: 'transform_list' arg is not a List<Transform<3>>".into(),
            );
        };
        if elements.is_empty() {
            diagnostics.push(Diagnostic::warning(
                "arbitrary_pattern dropped: 'transform_list' is empty".to_string(),
            ));
            return Err("arbitrary_pattern: 'transform_list' is empty".into());
        }
        let mut transforms = Vec::with_capacity(elements.len());
        for element in elements {
            match decompose_transform_to_arrays(element) {
                Some(decoded) => transforms.push(decoded),
                None => {
                    diagnostics.push(Diagnostic::warning(
                        "arbitrary_pattern dropped: 'transform_list' element is not a valid \
                         Transform<3>"
                            .to_string(),
                    ));
                    return Err(
                        "arbitrary_pattern: 'transform_list' element is not a valid Transform<3>"
                            .into(),
                    );
                }
            }
        }
        return Ok(reify_ir::GeometryOp::ArbitraryPattern {
            target: target_id,
            transforms,
        });
    }

    let mut transforms = Vec::new();
    let mut idx = 0;
    loop {
        let dx_name = format!("t{}_dx", idx);
        if !args.iter().any(|(name, _)| name == &dx_name) {
            break;
        }
        let mut f64_arg = |name: &str| -> Result<f64, String> {
            eval_named_arg_f64(
                name,
                kind,
                args,
                values,
                functions,
                meta_map,
                diagnostics,
            )
            .ok_or_else(|| {
                format!("missing or non-finite argument '{}' for {}", name, kind)
            })
        };
        let dx = f64_arg(&format!("t{}_dx", idx))?;
        let dy = f64_arg(&format!("t{}_dy", idx))?;
        let dz = f64_arg(&format!("t{}_dz", idx))?;
        // Scalar-triple form: translation-only, so the rotation quaternion is
        // identity. Mirrors `ApplyTransform`'s scalar-first `[qw,qx,qy,qz]`.
        transforms.push(([1.0, 0.0, 0.0, 0.0], [dx, dy, dz]));
        idx += 1;
    }
    if transforms.is_empty() {
        return Err("ArbitraryPattern has no transforms".into());
    }
    Ok(reify_ir::GeometryOp::ArbitraryPattern {
        target: target_id,
        transforms,
    })
}

// ── Sweep fns ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn sweep_loft(
    _kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    _args: &[(String, reify_ir::CompiledExpr)],
    _values: &ValueMap,
    _functions: &[CompiledFunction],
    _meta_map: &HashMap<String, HashMap<String, String>>,
    _diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let resolved: Result<Vec<GeometryHandleId>, String> = profiles
        .iter()
        .map(|r| resolve_geom_ref_impl(r, step_handles, named_steps))
        .collect();
    Ok(reify_ir::GeometryOp::Loft {
        profiles: resolved?,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_extrude(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let distance = eval_named_arg(
        "distance",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'distance' for {}", kind))?;
    match distance.as_f64() {
        Some(v) if v.is_finite() && v.abs() >= DEGENERATE_LENGTH_M => {}
        Some(v) => {
            diagnostics.push(Diagnostic::warning(format!(
                "extrude dropped: distance={} is degenerate \
                 (|distance| must be finite and >= 1e-12 m)",
                v
            )));
            return Err(format!("extrude distance is degenerate: {}", v));
        }
        None => return Err("extrude distance is non-numeric".into()),
    }
    Ok(reify_ir::GeometryOp::Extrude {
        profile: profile_handle,
        distance,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_revolve(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    let axis_dir = [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?];
    let mag = axis_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
    if !mag.is_finite() || mag < GEOMETRY_EPSILON {
        diagnostics.push(Diagnostic::warning(format!(
            "revolve dropped: rotation axis [{}, {}, {}] has \
             degenerate magnitude={} (must be finite and >= 1e-12)",
            axis_dir[0], axis_dir[1], axis_dir[2], mag
        )));
        return Err(format!("revolve axis has degenerate magnitude: {}", mag));
    }
    let angle_rad = f64_arg("angle")?;
    if angle_rad.abs() < DEGENERATE_ANGLE_RAD {
        diagnostics.push(Diagnostic::warning(format!(
            "revolve dropped: angle={} rad is degenerate \
             (|angle| must be >= 1e-12 rad)",
            angle_rad
        )));
        return Err(format!("revolve angle is degenerate: {} rad", angle_rad));
    }
    let axis_origin = [f64_arg("ox")?, f64_arg("oy")?, f64_arg("oz")?];
    Ok(reify_ir::GeometryOp::Revolve {
        profile: profile_handle,
        axis_origin,
        axis_dir,
        angle_rad,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_sweep(
    _kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    _args: &[(String, reify_ir::CompiledExpr)],
    _values: &ValueMap,
    _functions: &[CompiledFunction],
    _meta_map: &HashMap<String, HashMap<String, String>>,
    _diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let path_handle = resolve_geom_ref_impl(
        profiles
            .get(1)
            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    Ok(reify_ir::GeometryOp::Sweep {
        profile: profile_handle,
        path: path_handle,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_extrude_symmetric(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let distance = eval_named_arg(
        "distance",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'distance' for {}", kind))?;
    match distance.as_f64() {
        Some(v) if v.is_finite() && v.abs() >= 2.0 * DEGENERATE_LENGTH_M => {}
        Some(v) => {
            diagnostics.push(Diagnostic::warning(format!(
                "extrude_symmetric dropped: distance={} is \
                 degenerate (|distance/2| must be finite and >= 1e-12 m \
                 per-side; i.e. |distance| >= 2e-12 m, half-distance floor)",
                v
            )));
            return Err(format!("extrude_symmetric distance is degenerate: {}", v));
        }
        None => return Err("extrude_symmetric distance is non-numeric".into()),
    }
    Ok(reify_ir::GeometryOp::ExtrudeSymmetric {
        profile: profile_handle,
        distance,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_extrude_infinite(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing or non-finite argument '{}' for {}", name, kind))
    };
    let dx = f64_arg("dx")?;
    let dy = f64_arg("dy")?;
    let dz = f64_arg("dz")?;
    let mag = (dx * dx + dy * dy + dz * dz).sqrt();
    if !mag.is_finite() || mag < GEOMETRY_EPSILON {
        diagnostics.push(Diagnostic::warning(format!(
            "extrude_infinite dropped: axis [{}, {}, {}] has \
             degenerate magnitude={} (must be finite and >= 1e-12)",
            dx, dy, dz, mag
        )));
        return Err(format!(
            "extrude_infinite axis has degenerate magnitude: {}",
            mag
        ));
    }
    let direction_str = eval_named_arg(
        "direction",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| "missing required argument 'direction' for extrude_infinite".to_string())?;
    let direction_s = match &direction_str {
        reify_ir::Value::String(s) => s.as_str().to_owned(),
        other => {
            return Err(format!(
                "extrude_infinite direction must be a string, got: {:?}",
                other
            ))
        }
    };
    // Fold direction into (axis, both):
    // "positive" → keep axis as-is, both=false
    // "negative" → negate axis, both=false
    // "both"     → keep axis as-is, both=true
    let (axis, both) = match direction_s.as_str() {
        "positive" => ([dx, dy, dz], false),
        "negative" => ([-dx, -dy, -dz], false),
        "both" => ([dx, dy, dz], true),
        other => {
            diagnostics.push(Diagnostic::error(format!(
                "extrude_infinite: invalid direction {:?}; \
                 must be one of \"positive\", \"negative\", or \"both\"",
                other
            )));
            return Err(format!("extrude_infinite: invalid direction {:?}", other));
        }
    };
    Ok(reify_ir::GeometryOp::ExtrudeInfinite {
        profile: profile_handle,
        axis,
        both,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_sweep_guided(
    _kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    _args: &[(String, reify_ir::CompiledExpr)],
    _values: &ValueMap,
    _functions: &[CompiledFunction],
    _meta_map: &HashMap<String, HashMap<String, String>>,
    _diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let profile_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no profile GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let path_handle = resolve_geom_ref_impl(
        profiles
            .get(1)
            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let guide_handle = resolve_geom_ref_impl(
        profiles
            .get(2)
            .ok_or_else(|| "no guide GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    Ok(reify_ir::GeometryOp::SweepGuided {
        profile: profile_handle,
        path: path_handle,
        guide: guide_handle,
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_loft_guided(
    _kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    _args: &[(String, reify_ir::CompiledExpr)],
    _values: &ValueMap,
    _functions: &[CompiledFunction],
    _meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    if profiles.len() < 3 {
        diagnostics.push(Diagnostic::warning(format!(
            "loft_guided dropped: expected at least 2 \
             profile refs + 1 guide ref (3 total), got {}",
            profiles.len()
        )));
        return Err(format!(
            "loft_guided requires at least 3 refs, got {}",
            profiles.len()
        ));
    }
    let guide_ref = profiles
        .last()
        .ok_or_else(|| "no guide GeomRef supplied".to_string())?;
    let profile_refs = &profiles[..profiles.len() - 1];
    let resolved_profiles: Result<Vec<GeometryHandleId>, String> = profile_refs
        .iter()
        .map(|r| resolve_geom_ref_impl(r, step_handles, named_steps))
        .collect();
    let resolved_profiles = resolved_profiles?;
    let resolved_guide = resolve_geom_ref_impl(guide_ref, step_handles, named_steps)?;
    Ok(reify_ir::GeometryOp::LoftGuided {
        profiles: resolved_profiles,
        guides: vec![resolved_guide],
    })
}

#[allow(clippy::too_many_arguments)]
fn sweep_pipe(
    kind: &reify_compiler::SweepKind,
    profiles: &[reify_compiler::GeomRef],
    step_handles: &[GeometryHandleId],
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let path_handle = resolve_geom_ref_impl(
        profiles
            .first()
            .ok_or_else(|| "no path GeomRef supplied".to_string())?,
        step_handles,
        named_steps,
    )?;
    let radius = eval_named_arg(
        "radius",
        kind,
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| format!("missing required argument 'radius' for {}", kind))?;
    Ok(reify_ir::GeometryOp::Pipe {
        path: path_handle,
        radius,
    })
}

// ── Curve fns ─────────────────────────────────────────────────────────────────

fn curve_line_segment(
    kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    Ok(reify_ir::GeometryOp::LineSegment {
        x1: f64_arg("x1")?,
        y1: f64_arg("y1")?,
        z1: f64_arg("z1")?,
        x2: f64_arg("x2")?,
        y2: f64_arg("y2")?,
        z2: f64_arg("z2")?,
    })
}

fn curve_arc(
    kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    Ok(reify_ir::GeometryOp::Arc {
        center: [f64_arg("cx")?, f64_arg("cy")?, f64_arg("cz")?],
        radius: f64_arg("radius")?,
        start_angle: f64_arg("start_angle")?,
        end_angle: f64_arg("end_angle")?,
        axis: [f64_arg("ax")?, f64_arg("ay")?, f64_arg("az")?],
    })
}

fn curve_helix(
    kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut f64_arg = |name: &str| -> Result<f64, String> {
        eval_named_arg_f64(
            name,
            kind,
            args,
            values,
            functions,
            meta_map,
            diagnostics,
        )
        .ok_or_else(|| {
            format!("missing or non-finite argument '{}' for {}", name, kind)
        })
    };
    Ok(reify_ir::GeometryOp::Helix {
        radius: f64_arg("radius")?,
        pitch: f64_arg("pitch")?,
        height: f64_arg("height")?,
    })
}

fn curve_interp_curve(
    _kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let coords = eval_all_args_to_f64(
        "interp",
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| "failed to evaluate all interp args to f64".to_string())?;
    let points: Vec<[f64; 3]> =
        coords.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    Ok(reify_ir::GeometryOp::InterpCurve { points })
}

fn curve_bezier_curve(
    _kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let coords = eval_all_args_to_f64(
        "bezier",
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| "failed to evaluate all bezier args to f64".to_string())?;
    let control_points: Vec<[f64; 3]> =
        coords.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    Ok(reify_ir::GeometryOp::BezierCurve { control_points })
}

fn curve_nurbs_curve(
    _kind: &reify_compiler::CurveKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let vals = eval_all_args_to_f64(
        "nurbs",
        args,
        values,
        functions,
        meta_map,
        diagnostics,
    )
    .ok_or_else(|| "failed to evaluate all nurbs args to f64".to_string())?;
    if vals.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "nurbs() requires at least degree and n_points arguments".to_string(),
        ));
        return Err("nurbs() requires at least degree and n_points".into());
    }
    if vals[0] < 1.0 || vals[0] != vals[0].trunc() || vals[0] > 25.0 {
        diagnostics.push(Diagnostic::error(format!(
            "nurbs() degree must be a positive integer (1..25), got {}",
            vals[0]
        )));
        return Err(format!("nurbs() degree invalid: {}", vals[0]));
    }
    let degree = vals[0] as usize;
    if vals[1] < 2.0 || vals[1] != vals[1].trunc() || vals[1] > (vals.len() as f64)
    {
        diagnostics.push(Diagnostic::error(
            format!(
                "nurbs() n_points must be a positive integer >= 2 and consistent with argument count, got {}",
                vals[1]
            ),
        ));
        return Err(format!("nurbs() n_points invalid: {}", vals[1]));
    }
    let n_points = vals[1] as usize;
    let expected_min = 2 + n_points * 3 + n_points;
    if vals.len() < expected_min {
        diagnostics.push(Diagnostic::error(format!(
            "nurbs() got fewer arguments than expected for {} control points",
            n_points,
        )));
        return Err(format!(
            "nurbs() too few arguments for {} control points",
            n_points
        ));
    }
    let pole_start = 2;
    let pole_end = pole_start + n_points * 3;
    let weight_end = pole_end + n_points;
    let control_points: Vec<[f64; 3]> = vals[pole_start..pole_end]
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let weights: Vec<f64> = vals[pole_end..weight_end].to_vec();
    let knots: Vec<f64> = vals[weight_end..].to_vec();
    if knots.is_empty() {
        diagnostics.push(Diagnostic::error(
            "nurbs() requires at least 1 knot value".to_string(),
        ));
        return Err("nurbs() requires at least 1 knot value".into());
    }
    let expected_knots = n_points + degree + 1;
    if knots.len() != expected_knots {
        diagnostics.push(Diagnostic::error(format!(
            "nurbs() expected {} knots (n_points + degree + 1 = {} + {} + 1), got {}",
            expected_knots, n_points, degree, knots.len(),
        )));
        return Err(format!(
            "nurbs() wrong knot count: expected {}, got {}",
            expected_knots,
            knots.len()
        ));
    }
    Ok(reify_ir::GeometryOp::NurbsCurve {
        control_points,
        weights,
        knots,
        degree,
    })
}

// ── Profile fns ───────────────────────────────────────────────────────────────

fn profile_rectangle(
    kind: &reify_compiler::ProfileKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::RectangleProfile {
        width: eval_arg("width")?,
        height: eval_arg("height")?,
    })
}

fn profile_circle(
    kind: &reify_compiler::ProfileKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::CircleProfile {
        radius: eval_arg("radius")?,
    })
}

fn profile_polygon(
    _kind: &reify_compiler::ProfileKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let coords =
        eval_all_args_to_f64("polygon", args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| {
                "polygon() has invalid (non-numeric or non-finite) coordinates"
                    .to_string()
            })?;
    let points: Vec<[f64; 2]> = coords.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    Ok(reify_ir::GeometryOp::PolygonProfile { points })
}

fn profile_ellipse(
    kind: &reify_compiler::ProfileKind,
    args: &[(String, reify_ir::CompiledExpr)],
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    let mut eval_arg = |name: &str| -> Result<reify_ir::Value, String> {
        eval_named_arg(name, kind, args, values, functions, meta_map, diagnostics)
            .ok_or_else(|| format!("missing required argument '{}' for {}", name, kind))
    };
    Ok(reify_ir::GeometryOp::EllipseProfile {
        semi_major: eval_arg("semi_major")?,
        semi_minor: eval_arg("semi_minor")?,
    })
}

// ── Static dispatch tables ────────────────────────────────────────────────────

static PRIMITIVE_COMPILERS: &[(reify_compiler::PrimitiveKind, PrimitiveCompileFn)] = &[
    (reify_compiler::PrimitiveKind::Box, prim_box),
    (reify_compiler::PrimitiveKind::Cylinder, prim_cylinder),
    (reify_compiler::PrimitiveKind::Sphere, prim_sphere),
    (reify_compiler::PrimitiveKind::Tube, prim_tube),
    (reify_compiler::PrimitiveKind::Cone, prim_cone),
    (reify_compiler::PrimitiveKind::Wedge, prim_wedge),
    (reify_compiler::PrimitiveKind::Torus, prim_torus),
    (reify_compiler::PrimitiveKind::HalfSpace, prim_half_space),
];

static MODIFY_COMPILERS: &[(reify_compiler::ModifyKind, ModifyCompileFn)] = &[
    (reify_compiler::ModifyKind::Fillet, modify_fillet),
    (reify_compiler::ModifyKind::Chamfer, modify_chamfer),
    (reify_compiler::ModifyKind::ChamferAsymmetric, modify_chamfer_asymmetric),
    (reify_compiler::ModifyKind::Shell, modify_shell),
    (reify_compiler::ModifyKind::Draft, modify_draft),
    (reify_compiler::ModifyKind::Thicken, modify_thicken),
    (reify_compiler::ModifyKind::ZoneSlab, modify_zone_slab),
    (reify_compiler::ModifyKind::OffsetSolid, modify_offset_solid),
    (reify_compiler::ModifyKind::OffsetCurve, modify_offset_curve),
];

static TRANSFORM_COMPILERS: &[(reify_compiler::TransformKind, TransformCompileFn)] = &[
    (reify_compiler::TransformKind::Translate, transform_translate),
    (reify_compiler::TransformKind::Rotate, transform_rotate),
    (reify_compiler::TransformKind::Scale, transform_scale),
    (reify_compiler::TransformKind::RotateAround, transform_rotate_around),
    (reify_compiler::TransformKind::ApplyTransform, transform_apply),
    (reify_compiler::TransformKind::AffineApply, transform_affine_apply),
    (reify_compiler::TransformKind::ScaleNonUniform, transform_scale_non_uniform),
];

static PATTERN_COMPILERS: &[(reify_compiler::PatternKind, PatternCompileFn)] = &[
    (reify_compiler::PatternKind::Linear, pattern_linear),
    (reify_compiler::PatternKind::Circular, pattern_circular),
    (reify_compiler::PatternKind::Mirror, pattern_mirror),
    (reify_compiler::PatternKind::Linear2D, pattern_linear2d),
    (reify_compiler::PatternKind::Arbitrary, pattern_arbitrary),
];

static SWEEP_COMPILERS: &[(reify_compiler::SweepKind, SweepCompileFn)] = &[
    (reify_compiler::SweepKind::Loft, sweep_loft),
    (reify_compiler::SweepKind::Extrude, sweep_extrude),
    (reify_compiler::SweepKind::Revolve, sweep_revolve),
    (reify_compiler::SweepKind::Sweep, sweep_sweep),
    (reify_compiler::SweepKind::ExtrudeSymmetric, sweep_extrude_symmetric),
    (reify_compiler::SweepKind::ExtrudeInfinite, sweep_extrude_infinite),
    (reify_compiler::SweepKind::SweepGuided, sweep_sweep_guided),
    (reify_compiler::SweepKind::LoftGuided, sweep_loft_guided),
    (reify_compiler::SweepKind::Pipe, sweep_pipe),
];

static CURVE_COMPILERS: &[(reify_compiler::CurveKind, CurveCompileFn)] = &[
    (reify_compiler::CurveKind::LineSegment, curve_line_segment),
    (reify_compiler::CurveKind::Arc, curve_arc),
    (reify_compiler::CurveKind::Helix, curve_helix),
    (reify_compiler::CurveKind::InterpCurve, curve_interp_curve),
    (reify_compiler::CurveKind::BezierCurve, curve_bezier_curve),
    (reify_compiler::CurveKind::NurbsCurve, curve_nurbs_curve),
];

static PROFILE_COMPILERS: &[(reify_compiler::ProfileKind, ProfileCompileFn)] = &[
    (reify_compiler::ProfileKind::Rectangle, profile_rectangle),
    (reify_compiler::ProfileKind::Circle, profile_circle),
    (reify_compiler::ProfileKind::Polygon, profile_polygon),
    (reify_compiler::ProfileKind::Ellipse, profile_ellipse),
];

// ── Lookup helpers ────────────────────────────────────────────────────────────

fn lookup_primitive(kind: reify_compiler::PrimitiveKind) -> Option<PrimitiveCompileFn> {
    PRIMITIVE_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_modify(kind: reify_compiler::ModifyKind) -> Option<ModifyCompileFn> {
    MODIFY_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_transform(kind: reify_compiler::TransformKind) -> Option<TransformCompileFn> {
    TRANSFORM_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_pattern(kind: reify_compiler::PatternKind) -> Option<PatternCompileFn> {
    PATTERN_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_sweep(kind: reify_compiler::SweepKind) -> Option<SweepCompileFn> {
    SWEEP_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_curve(kind: reify_compiler::CurveKind) -> Option<CurveCompileFn> {
    CURVE_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

fn lookup_profile(kind: reify_compiler::ProfileKind) -> Option<ProfileCompileFn> {
    PROFILE_COMPILERS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

// ── Conformance-query dispatch (task 2320) ──────────────────────────────────
//
// `try_eval_conformance_query` is the kernel-aware eval-time dispatch for the
// stdlib helpers `is_watertight`, `is_manifold`, and `is_orientable`.
//
// Architecture: the helpers cannot be evaluated by the pure-value
// `eval_expr` / `eval_builtin` path because (a) `Type::Geometry` has no
// corresponding `Value` variant, and (b) the kernel — and therefore
// `GeometryHandleId`s — only exists behind `Engine.geometry_kernel`. The
// kernel-aware dispatch must live in the build / check pipeline where the
// engine has both the kernel and the realisation's per-name
// `GeometryHandleId` map (`named_steps`). This free function is invoked
// from `engine_build.rs` after `execute_realization_ops` has populated
// `named_steps` for a template, and patches the resulting `Value::Bool(_)`
// into the per-cell `ValueMap`.
//
// Helper-name → marker-trait pairing for the user-assertion escape hatch:
//   `is_watertight` ↔ `"Watertight"`
//   `is_manifold`   ↔ `"Manifold"`
//   `is_orientable` ↔ `"Orientable"`
// Note the asymmetry: `is_watertight` short-circuits **only** on
// `"Watertight"` — declaring `Closed` or `Manifold` (which `Watertight`
// refines per `geometry_traits.ri`) is not sufficient. Trait-DAG
// propagation is intentionally not done here; the simple name-equivalence
// rule mirrors task 2321's per-bound `W_TRAIT_USER_ASSERTED` warning.
//
// Returns:
//   `Some(Value::Bool(_))` when the dispatch produces a definite answer
//                          (kernel reply OR user-assertion override).
//   `Some(Value::Undef)`   when the kernel returned a non-`Bool` (defensive
//                          downgrade with a Warning diagnostic).
//   `None`                 when the expression is not a recognised
//                          conformance-query helper, or the arg shape is
//                          unsupported (literal, non-`ValueRef`,
//                          unresolvable cell-member name).  Callers fall
//                          through to the cell's compiled default.
pub(crate) fn try_eval_conformance_query(
    expr: &reify_ir::CompiledExpr,
    template_trait_bounds: &[String],
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Early-return ordering audit (task 2320 step-8): the kernel is the last
    // step. Each guard below either short-circuits via `return None` (for
    // unsupported shapes) or short-circuits via `return Some(Bool(true))`
    // (for the user-assertion escape hatch). Pinned by the
    // `try_eval_conformance_query_*_returns_none_no_kernel_call` and
    // `try_eval_conformance_query_user_assertion_*` tests.
    //
    //   1. CompiledExprKind::FunctionCall          (cheapest — pattern match)
    //   2. recognised helper name                  (string compare)
    //   3. user-assertion escape hatch             (Vec::any string compare)
    //   4. single-arg ValueRef shape               (pattern match)
    //   5. named_steps cell-member lookup          (HashMap::get)
    //   6. kernel.query(...)                       (the actual round-trip)

    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be one of the three recognised helper names. The pairing
    // with the matching marker trait is fixed.
    let marker_trait = match function.name.as_str() {
        "is_watertight" => "Watertight",
        "is_manifold" => "Manifold",
        "is_orientable" => "Orientable",
        // θ conformance predicates (task #4171)
        "is_closed" => "Closed",
        "is_connected" => "Connected",
        "is_bounded" => "Bounded",
        _ => return None,
    };

    // (3) Escape hatch: if the enclosing structure declared the matching
    // marker trait, skip the kernel query entirely and return Bool(true).
    // This is intentionally checked *before* arg-shape resolution so the
    // user-assertion semantic holds even when the arg is otherwise
    // unresolvable.
    if template_trait_bounds.iter().any(|t| t == marker_trait) {
        return Some(reify_ir::Value::Bool(true));
    }

    // (4) Arg shape: we only resolve `is_watertight(<entity>.<member>)`
    // where `<member>` is a let-bound geometry name in `named_steps`.
    // Anything else (literals, nested expressions, cross-template idents)
    // falls through to `None` so the cell stays at its compiled default
    // (`Value::Undef`) — verified by the integration test
    // `is_watertight_with_literal_int_arg_falls_through_to_undef` in
    // `tests/conformance_runtime.rs` (task 2320 step-13/14).
    if args.len() != 1 {
        return None;
    }
    let cell_id = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        // Defensive fall-through (task 2320 step-14): literals, nested
        // expressions, and any non-`ValueRef` shape bail to `None` *before*
        // any `named_steps` lookup or `kernel.query(...)` round-trip — so
        // ill-formed conformance-query call sites degrade gracefully rather
        // than panicking the build.
        _ => return None,
    };

    // (5) Resolve the cell-member name to a kernel handle. Absent →
    // `None` (and the kernel is never consulted).
    let handle = match named_steps.get(&cell_id.member) {
        Some(kh) => kh.id,
        None => return None,
    };

    // (6) All guards passed: build the matching kernel query and dispatch.
    let query = match function.name.as_str() {
        "is_watertight" => reify_ir::GeometryQuery::IsWatertight(handle),
        "is_manifold" => reify_ir::GeometryQuery::IsManifold(handle),
        "is_orientable" => reify_ir::GeometryQuery::IsOrientable(handle),
        // θ conformance predicates (task #4171)
        "is_closed" => reify_ir::GeometryQuery::IsClosed(handle),
        "is_connected" => reify_ir::GeometryQuery::IsConnected(handle),
        "is_bounded" => reify_ir::GeometryQuery::IsBounded(handle),
        // Unreachable — the earlier match already filtered to these six names.
        _ => return None,
    };

    match kernel.query(&query) {
        Ok(reify_ir::Value::Bool(b)) => Some(reify_ir::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel returned non-Bool value {:?}; treating as undefined",
                function.name, cell_id.member, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({}) kernel query failed: {}",
                function.name, cell_id.member, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ── Whole-handle geometry-query dispatch (task 3608, GHR-ζ) ─────────────────
//
// `try_eval_geometry_query` is the kernel-aware eval-time dispatch for the
// stdlib whole-handle geometry queries `volume` / `area` / `centroid` /
// `bounding_box` on a `Value::GeometryHandle` (PRD
// `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6). Sibling to
// `try_eval_conformance_query` / `try_eval_topology_selector`, dispatched from
// `Engine::post_process_geometry_queries`.

/// Tessellation deflection forwarded to `GeometryQuery::MaxDeviation.tolerance`
/// when the `max_deviation(actual, nominal)` callable is evaluated.
///
/// Mirrors `Engine::DEFAULT_TESSELLATION_TOLERANCE` (engine_build.rs:3165 =
/// 0.0001 m). Kept local to confine ζ's eval footprint to geometry_ops.rs and
/// avoid locking the hot engine_build.rs for a const reference (ζ / C4, task
/// 4479). The test `max_deviation_tessellation_tolerance_pins_engine_default_value`
/// pins this value and documents the update procedure (see the test comment).
const MAX_DEVIATION_TESSELLATION_TOLERANCE_M: f64 = 0.0001;
//
// `length` / `perimeter` are deliberately NOT handled here: they are already
// delivered via the edge/face topology-selector path (`dispatch_edge_length` /
// `dispatch_perimeter`), and `GeometryQuery` has no whole-handle
// Length/Perimeter variant — routing them here would double-dispatch.
//
// Kernel reply contract (`reify-kernel-occt/src/lib.rs:2519`): Volume /
// SurfaceArea return `Value::Real` (SI m³ / m²); Centroid / BoundingBox return
// the canonical JSON `Value::String` wire format.
//
// Returns:
//   `Some(Value::Scalar { dimension: VOLUME/AREA, .. })` for volume / area,
//   `Some(Value::Point([length, length, length]))` for centroid,
//   `Some(Value::BoundingBox { min, max })` (two `Point3<Length>`) for bounding_box,
//   `Some(Value::Undef)` (with a Warning) when a handle arg resolves but the
//        kernel errors or replies with an unexpected type (PRD §4
//        defensive-downgrade contract),
//   `Some(_)` for the NESTED case — the folded value of the enclosing
//        expression (e.g. `Scalar<Mass>` for `mass = volume(g) * density`),
//   `None` when the expr neither IS, nor CONTAINS, a recognised whole-handle
//        geometry-query call (or, for the direct case, its single arg is
//        unresolvable) — the caller leaves the cell at its compiled default
//        (`Value::Undef`).
//
// Two shapes are handled (task 3608: step-2 = direct, step-10 = nested):
//   (a) DIRECT — the cell's `default_expr` IS a geometry-query call, e.g.
//       `centroid = centroid(geometry)`. Dispatched straight to the kernel.
//   (b) NESTED — the `default_expr` CONTAINS a geometry-query call inside a
//       larger expression, e.g. `mass = volume(geometry) * material.density`
//       (a `BinOp` whose left leaf is `volume(...)`). Every geometry-query
//       leaf is rewritten to a `Literal` of its dispatched Value, then the
//       enclosing expression is recomputed with the standard pure evaluator
//       (`reify_expr::eval_expr`): `Scalar<Volume> * Scalar<Density>`
//       recombines to `Scalar<Mass>` via the existing units arithmetic, and
//       `material.density` resolves against the already-evaluated `material`
//       StructureInstance cell in `values` (the eval pass that produced
//       `values` runs before this post-process — engine_build.rs:1802). The
//       frozen Physical spec shape (GHR-α) computes `mass` this way, so the
//       nested fold is what produces the terminal user-observable.
//
// Cross-cell factoring: `try_eval_geometry_query` itself does NOT re-evaluate
// dependent cells. If the geometry-query call is NOT lexically in the cell's
// own `default_expr` — e.g.:
//       let v = volume(geometry)       // (a) DIRECT — folds to Scalar<Volume>
//       let m = v * material.density   // BinOp of ValueRef(v) — NO query leaf
// then `m`'s expr contains no geometry-query `FunctionCall`, so
// `expr_contains_geometry_query` is `false`, this pass returns `None` for `m`.
// This post-process inserts ONLY into geometry-query cells. However, the
// subsequent `post_process_derived_lets` pass in `engine_build.rs` (task 4229)
// performs a fixpoint re-eval of Undef Let cells, which resolves cross-cell
// factoring: after `v` folds to `Scalar<Volume>`, `post_process_derived_lets`
// re-evaluates `m` and folds it to the correct value. This is pinned by
// `cross_cell_factored_dependent_folds_via_fixpoint`
// (tests/geometry_query_kernel_dispatch.rs).
//
// GHR-ζ does NOT route through `gate_query_capability` (task 3623): consistent
// with the existing selector-dispatch siblings, and all GHR-ζ fixtures realize
// as BRep so the gate would route `Occt` anyway. Wiring the gate is the KGQ
// dispatcher family's scope.
pub(crate) fn try_eval_geometry_query(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // ── Case (ζ): DIRECT 2-arg — `max_deviation(actual, nominal)`. Kept
    //    SEPARATE from the 1-arg `is_geometry_query_call` invariant (ζ / C4,
    //    task 4479). Returns `None` (cell keeps compiled default Value::Undef)
    //    when either arg is unresolvable (non-ValueRef literal or missing
    //    named_steps entry). This is a **deliberate** design choice matching
    //    the 2-arg sibling convention (min_clearance, distance, contains) —
    //    literal-arg calls are rejected by the type checker before reaching
    //    eval, so a silent None here avoids spurious Warning diagnostics on
    //    type-unsafe call patterns the compiler already surfaces. If usability
    //    concerns outweigh sibling consistency in a future revision, change
    //    the `?` operators to explicit `else { emit Warning; return
    //    Some(Value::Undef); }` guards. Scope: direct-call only;
    //    nested-arithmetic fold is out of scope (matches the
    //    min_clearance/kinematic-sibling convention).
    if let reify_ir::CompiledExprKind::FunctionCall { function, args } = &expr.kind
        && function.name == "max_deviation"
        && args.len() == 2
    {
        let actual = resolve_geometry_handle_arg(&args[0], named_steps)?;
        let nominal = resolve_geometry_handle_arg(&args[1], named_steps)?;
        let query = reify_ir::GeometryQuery::MaxDeviation {
            actual,
            nominal,
            tolerance: MAX_DEVIATION_TESSELLATION_TOLERANCE_M,
        };
        return dispatch_scalar_query(
            kernel,
            query,
            reify_core::DimensionVector::LENGTH,
            "max_deviation",
            diagnostics,
        );
    }

    // ── Case (a): DIRECT — the expr itself is a whole-handle geometry-query
    //    call. Dispatch and return the typed Value. Returns `None` when the
    //    single arg is unresolvable, so the cell keeps its compiled default —
    //    preserving the pre-step-10 (direct-only) contract exactly.
    if is_geometry_query_call(expr) {
        let (function, args) = match &expr.kind {
            reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
            _ => unreachable!("is_geometry_query_call guarantees a FunctionCall node"),
        };
        return dispatch_geometry_query_call(
            function.name.as_str(),
            args,
            named_steps,
            kernel,
            diagnostics,
        );
    }

    // ── Case (b): NESTED — fold a geometry-query call buried inside a larger
    //    expression. Skip cells with no geometry-query call anywhere (they keep
    //    their compiled default / belong to another pass).
    if !expr_contains_geometry_query(expr) {
        return None;
    }
    // Rewrite each geometry-query leaf to a Literal of its dispatched Value, then
    // recompute the enclosing expression with the standard pure evaluator.
    let rewritten = rewrite_geometry_queries(expr, named_steps, kernel, diagnostics);
    Some(reify_expr::eval_expr(
        &rewritten,
        &eval_ctx_with_meta(values, functions, meta_map),
    ))
}

// ── feature(geometry) accessor dispatch (task 4830, P3α) ────────────────────
//
// `feature(geometry) : Feature` (PRD D1) is unlike volume/area/centroid: it
// issues NO kernel query, and it needs the `TopologyAttributeTable` for
// sub-shape resolution — neither of which `try_eval_geometry_query` above
// threads. It is therefore dispatched separately, from a DEDICATED
// post-process pass (`Engine::post_process_feature_accessor`), rather than
// folded into the generic geometry-query pass.

/// Project a resolved geometry handle to its owning `Value::Feature`.
///
/// Resolution order: `table.lookup(handle_id)` → `attr.feature_id` (sub-shape
/// mode — e.g. a fillet face resolves to the fillet feature); else
/// `FeatureId::from(&realization_ref)` (whole-body mode — the table seeds
/// only sub-shapes, so a whole-body handle falls through to its realization
/// feature, which per PRD D3 always resolves).
///
/// `resolved` is `None` when the accessor's argument did not resolve to a
/// realized `Value::GeometryHandle` (see `resolve_parent_geometry_handle_arg`);
/// the caller maps `None` → the cell's compiled default `Value::Undef`.
pub(crate) fn project_handle_to_feature(
    resolved: Option<(reify_core::identity::RealizationNodeId, GeometryHandleId)>,
    table: &reify_ir::TopologyAttributeTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    use reify_core::DiagnosticCode;

    let Some((realization_ref, handle_id)) = resolved else {
        // Fail-closed (OQ#2): the argument did not resolve to a realized
        // `Value::GeometryHandle`. Mirrors `route_capability`'s construction
        // (geometry_ops.rs:118-127), reusing P0 β's code rather than adding a
        // dedicated variant. The caller maps `None` → the cell's compiled
        // default `Value::Undef`.
        diagnostics.push(
            Diagnostic::error(
                "'feature' requires a realized geometry handle; \
                 the argument did not resolve to one"
                    .to_string(),
            )
            .with_code(DiagnosticCode::QueryNotSupportedOnRepr),
        );
        return None;
    };
    let feature_id = table
        .lookup(handle_id)
        .map(|attr| attr.feature_id.clone())
        .unwrap_or_else(|| reify_ir::FeatureId::from(&realization_ref));
    Some(reify_ir::Value::Feature(feature_id))
}

/// Eval-time dispatch for the explicit projection `feature(geometry) :
/// Feature` (PRD D1, P3α). Recognises a 1-arg `feature(...)`
/// `CompiledExprKind::FunctionCall`, resolves the let-bound arg via
/// `resolve_parent_geometry_handle_arg`, and projects the resolved handle via
/// [`project_handle_to_feature`]. Returns `None` for any other expr shape —
/// the caller leaves the cell at its compiled default.
pub(crate) fn try_eval_feature_accessor(
    expr: &reify_ir::CompiledExpr,
    values: &ValueMap,
    table: &reify_ir::TopologyAttributeTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let reify_ir::CompiledExprKind::FunctionCall { function, args } = &expr.kind else {
        return None;
    };
    if function.name != "feature" || args.len() != 1 {
        return None;
    }
    let resolved = resolve_parent_geometry_handle_arg(&args[0], values)
        .map(|(realization_ref, _upstream_values_hash, handle_id)| (realization_ref, handle_id));
    project_handle_to_feature(resolved, table, diagnostics)
}

/// `true` iff `expr` is a recognised whole-handle geometry-query call —
/// `volume` / `area` / `centroid` / `bounding_box` with exactly one arg. The
/// single source of truth for the recognised-name set, used to gate the
/// direct-dispatch path and to locate fold leaves in the nested path.
/// `length` / `perimeter` are intentionally excluded (topology-selector path —
/// see the module note above `try_eval_geometry_query`).
pub(crate) fn is_geometry_query_call(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if args.len() == 1
                && matches!(
                    function.name.as_str(),
                    "volume" | "area" | "centroid" | "bounding_box"
                )
    )
}

/// `true` iff `expr` is a top-level `FunctionCall` to a **geometry consumer**
/// — a builtin that requires a realized kernel to produce a result and therefore
/// cannot be resolved on the pure value-eval surface (kernel-less
/// `Engine::eval` / `eval_cached`).
///
/// ## Consumer vs constructor partition
///
/// **Consumers** (returns `true`):
/// - The `is_geometry_query_call` family: `volume`, `area`, `centroid`,
///   `bounding_box` (scalar/point queries that require a kernel handle).
/// - The kernel-bearing `TopologySelectorHelper` consumers enumerated in the
///   name map at `geometry_ops.rs:5314-5352`: `adjacent_faces`, `normal`,
///   `closest_point`, `shared_edges`, `length`, `perimeter`, `curvature`,
///   `center_of_mass`, `moment_of_inertia`, `distance`, `contains`,
///   `intersects`, `geo_equiv`, `is_on`, `angle_between_surfaces`.
///
/// **Not consumers** (returns `false`):
/// - GEOMETRY_FUNCTION_NAMES constructors (`box`, `cylinder`, `sphere`, …).
/// - The 9 R2b kernel-free leaf selector ctors (`faces`, `edges`,
///   `mid_surface`, `vertices`, `faces_by_area`, `faces_by_normal`,
///   `edges_by_length`, `edges_parallel_to`, `edges_at_height`) — these mint
///   a symbolic `Value::Selector` without a kernel.
/// - Composition/named-leaf ctors: `union`, `intersect`, `difference`, `face`,
///   `edge`, `solid_body`, `vertex`, `mid_surface`.
/// - List/selection helpers: `single`.
/// - Non-`FunctionCall` expression nodes (`Literal`, `ValueRef`, …).
///
/// ## Maintenance contract
///
/// When a new kernel-bearing helper is added to `TopologySelectorHelper`,
/// add its name here.  When a new kernel-free leaf ctor is added (like R2b's
/// 9 names), verify it is NOT listed here.  The consumer/constructor partition
/// is the single source of truth — the classifier tests in `geometry_ops.rs`
/// exercise the boundary.
///
/// Called by `engine_eval::detect_unresolved_geometry_consumers` (task 4651
/// R1a) to locate typed-consumption sites that remain `Value::Undef` after
/// kernel-less eval.
pub(crate) fn is_geometry_consumer_call(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::FunctionCall { function, .. }
            if matches!(
                function.name.as_str(),
                // ── is_geometry_query_call family (1-arg scalar/point queries) ─
                "volume" | "area" | "centroid" | "bounding_box"
                // ── kernel-bearing TopologySelectorHelper consumers ─────────────
                | "adjacent_faces"
                | "normal"
                | "closest_point"
                | "shared_edges"
                // task #4759 — relational-walk v2 selectors
                | "siblings_of_face"
                | "ancestor_faces_of_edge"
                | "length"
                | "perimeter"
                | "curvature"
                | "center_of_mass"
                | "moment_of_inertia"
                | "distance"
                | "contains"
                | "intersects"
                | "geo_equiv"
                | "is_on"
                | "angle_between_surfaces"
                // task 3614 (KGQ-ε): dispatched via the same build()-only
                // TopologySelectorHelper::Angle path as `angle_between_surfaces`
                // (try_eval_topology_selector) even though its own computation
                // is pure-math (acos/clamp/dot) — a pre-existing gap in this
                // allow-list (task 4952 α), not a deliberate exclusion: no
                // classifier test asserted `angle` == false, and its own
                // pinning test (`kernel_queries_angle_smoke.rs`) resolves it
                // via `engine.build()`, not `engine.eval()`.
                | "angle"
            )
    )
}

/// `true` iff any node in `expr`'s tree is a geometry-query call (per
/// [`is_geometry_query_call`]). Drives the nested-fold gate: only expressions
/// that actually contain a query are rewritten + re-evaluated. Uses the
/// canonical `CompiledExpr::walk` traversal so new expr variants are covered
/// automatically.
fn expr_contains_geometry_query(expr: &reify_ir::CompiledExpr) -> bool {
    let mut found = false;
    expr.walk(&mut |node| {
        if is_geometry_query_call(node) {
            found = true;
        }
    });
    found
}

/// Dispatch a single recognised geometry-query call (its `function_name` + one
/// handle `args`) to the kernel and convert the reply to a typed Value. Shared
/// by the direct path (returned straight to the caller) and the nested-fold
/// rewrite (wrapped in a `Literal`).
///
/// Returns `None` when the single arg is unresolvable (literal, non-`ValueRef`,
/// missing `named_steps` entry) or the name is unrecognised; `Some(Value::Undef)`
/// (with a Warning) on a kernel error or unexpected reply type (PRD §4
/// defensive downgrade — see `dispatch_scalar_query` / `dispatch_bounding_box`).
fn dispatch_geometry_query_call(
    function_name: &str,
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Exactly one handle arg, resolved via `named_steps` (hydrated + revalidated
    // by `post_process_geometry_handle_cells` before this pass). Unresolvable
    // (literal, non-`ValueRef`, missing entry) → `None`.
    if args.len() != 1 {
        return None;
    }
    let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
    match function_name {
        "volume" => dispatch_scalar_query(
            kernel,
            reify_ir::GeometryQuery::Volume(handle),
            reify_core::DimensionVector::VOLUME,
            "volume",
            diagnostics,
        ),
        "area" => dispatch_scalar_query(
            kernel,
            reify_ir::GeometryQuery::SurfaceArea(handle),
            reify_core::DimensionVector::AREA,
            "area",
            diagnostics,
        ),
        // Centroid returns the canonical JSON-Point3 (`{"x":_,"y":_,"z":_}`)
        // wire format; `dispatch_point3_length_reply` decodes it to a
        // `Point3<Length>` (shared with closest_point / center_of_mass), with the
        // same Warning + Undef defensive downgrade on kernel error / malformed reply.
        "centroid" => dispatch_point3_length_reply(
            kernel,
            &reify_ir::GeometryQuery::Centroid(handle),
            "centroid",
            diagnostics,
        ),
        // BoundingBox returns the 6-field JSON (`{"xmin":_,..,"zmax":_}`);
        // `dispatch_bounding_box` decodes it (reusing `parse_bbox_axis_extents`
        // per axis) into `Value::BoundingBox` of two `Point3<Length>` corners.
        "bounding_box" => dispatch_bounding_box(kernel, handle, diagnostics),
        // Unrecognised name — `is_geometry_query_call` gates the callers, so this
        // is unreachable in practice; return `None` defensively.
        _ => None,
    }
}

/// Rewrite every geometry-query leaf in `expr` to a `Literal` of its
/// kernel-dispatched Value, returning a fresh expression the standard pure
/// evaluator can fold. Used only by the nested case of
/// [`try_eval_geometry_query`].
///
/// A geometry-query leaf whose handle arg is unresolvable folds to
/// `Literal(Value::Undef)`, so the enclosing arithmetic propagates `Undef`
/// (strict Undef propagation in `eval_expr`).
///
/// Recursion descends through `BinOp` / `UnOp` — the arithmetic wrappers the
/// frozen Physical spec shape uses (`mass = volume(geometry) *
/// material.density`). Every other node kind is cloned unchanged: an off-path
/// subtree (no nested query, e.g. the `material.density` operand) is reproduced
/// exactly, and a query nested inside an un-handled wrapper kind stays unfolded
/// so `eval_expr` yields `Undef` — the same outcome as the cell's compiled
/// default (a conservative downgrade, never a wrong value). Extend this match
/// if a future trait nests a geometry query inside a richer wrapper.
///
/// CROSS-SCHEDULER REACH (task 4358 ε amendment): the non-query `FunctionCall`-
/// args recursion arm below is NOT UnifiedDag-only — this function is shared
/// geometry-fold code reached on BOTH scheduler paths. On `LegacyMultiPass` it
/// runs inside `post_process_geometry_queries` → `try_eval_geometry_query`
/// case (b) for every VALUE CELL whose `default_expr` is a non-query
/// `FunctionCall` wrapping a geometry-query leaf (e.g.
/// `let fits = fits_build_volume(bounding_box(part), bounding_box(envelope))`).
/// Before ε added this arm the outer call fell through the `_` arm un-folded, so
/// its inner leaves never folded and `eval_expr` (kernel-less) yielded `Undef`;
/// with the arm the leaves fold first and `eval_expr` computes a concrete value.
/// This is therefore a shared CORRECTNESS fix (Undef/error → real value), and the
/// one documented exception to ε's "LegacyMultiPass stays byte-identical" claim —
/// limited to that specific non-query-wrapper cell shape. Pinned on the legacy
/// path by `tests/unified_dag_geometry_executors.rs::
/// legacy_multipass_folds_nonquery_functioncall_value_cell`.
///
/// PERFORMANCE: every geometry-query leaf is dispatched independently, so an
/// expression repeating an identical call (e.g. `volume(g) + volume(g)`) issues
/// one kernel round-trip per occurrence, and the enclosing
/// `post_process_geometry_queries` re-runs this rewrite on every build path
/// (including cache-hit builds). For the frozen Physical spec shape each query
/// cell holds a single call, so this is one cheap round-trip and negligible. If
/// these expressions grow, memoize dispatch results per
/// `(function_name, GeometryHandleId)` within a single rewrite so repeated
/// leaves reuse one round-trip — deliberately NOT done here as it is
/// unobservable at the current single-query scope.
pub(crate) fn rewrite_geometry_queries(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::CompiledExpr {
    match &expr.kind {
        // Geometry-query leaf → Literal of its dispatched Value.
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if is_geometry_query_call(expr) =>
        {
            let value = dispatch_geometry_query_call(
                function.name.as_str(),
                args,
                named_steps,
                kernel,
                diagnostics,
            )
            .unwrap_or(reify_ir::Value::Undef);
            reify_ir::CompiledExpr::literal(value, expr.result_type.clone())
        }
        // Non-query outer FunctionCall (task 4358 ε): recurse into each argument
        // so inner geometry-query leaves fold, but leave the outer call's
        // identity (function + arity + result type) intact. The leaf arm above
        // (guarded by `is_geometry_query_call`) wins for recognised query calls;
        // this arm handles every OTHER FunctionCall — e.g. the constraint shape
        // `fits_build_volume(bounding_box(..), bounding_box(..))` — so its inner
        // query leaves resolve instead of being left un-folded (→ Undef) by the
        // `_` fallthrough. Reached on BOTH scheduler paths (it also folds legacy
        // non-query-wrapper VALUE cells via `post_process_geometry_queries`) — see
        // the CROSS-SCHEDULER REACH note in this function's doc comment.
        reify_ir::CompiledExprKind::FunctionCall { function, args } => {
            let rewritten_args: Vec<reify_ir::CompiledExpr> = args
                .iter()
                .map(|a| rewrite_geometry_queries(a, named_steps, kernel, diagnostics))
                .collect();
            // No public `function_call` constructor: rebuild manually with a
            // fresh content hash mirroring `compile_expr`'s combine order
            // (qualified_name + each arg hash), per expr.rs `map_value_refs`.
            let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(reify_core::ContentHash::of_str(&function.qualified_name));
            for a in &rewritten_args {
                content_hash = content_hash.combine(a.content_hash);
            }
            reify_ir::CompiledExpr {
                kind: reify_ir::CompiledExprKind::FunctionCall {
                    function: function.clone(),
                    args: rewritten_args,
                },
                result_type: expr.result_type.clone(),
                content_hash,
            }
        }
        reify_ir::CompiledExprKind::BinOp { op, left, right } => reify_ir::CompiledExpr::binop(
            *op,
            rewrite_geometry_queries(left, named_steps, kernel, diagnostics),
            rewrite_geometry_queries(right, named_steps, kernel, diagnostics),
            expr.result_type.clone(),
        ),
        reify_ir::CompiledExprKind::UnOp { op, operand } => reify_ir::CompiledExpr::unop(
            *op,
            rewrite_geometry_queries(operand, named_steps, kernel, diagnostics),
            expr.result_type.clone(),
        ),
        // Off-path subtree or un-handled wrapper kind — clone unchanged (see the
        // function doc for why this is conservative, never wrong).
        _ => expr.clone(),
    }
}

/// Issue a scalar-returning kernel query (`Volume` / `SurfaceArea` /
/// `MaxDeviation`) and wrap the `Value::Real` (or, defensively,
/// `Value::Scalar`) reply through the `Value::from_real_scalar` chokepoint: a
/// dimensioned result becomes `Value::Scalar { si_value, dimension }`, while a
/// dimensionless result collapses to `Value::Real` (Invariant V — no code path
/// constructs a `Value::Scalar { dimension.is_dimensionless() }`).
///
/// Returns `Some(Value::Undef)` + one Warning on:
/// - a kernel error,
/// - an unexpected reply type (PRD §4 defensive downgrade),
/// - a **non-finite or negative** kernel value — a degenerate result (NaN /
///   ±Inf) or a negative measurement (impossible for volume / area /
///   deviation) propagating as a valid `Scalar` would silently corrupt
///   downstream arithmetic; surfacing it as Undef + Warning matches PRD §4.
///
/// Mirrors `dispatch_edge_length`.
fn dispatch_scalar_query(
    kernel: &dyn reify_ir::GeometryKernel,
    query: reify_ir::GeometryQuery,
    dimension: reify_core::DimensionVector,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(&query) {
        // Both reply shapes — a bare `Real` and the defensive `Scalar` — carry a
        // single magnitude that must be finite and non-negative to stand for a
        // volume / area / deviation. Validate them identically (a NaN / ±Inf /
        // negative magnitude in EITHER shape is downgraded to Undef + Warning),
        // then collapse through the `from_real_scalar` chokepoint (dimensionless
        // → Value::Real, dimensioned → Value::Scalar; Invariant V).
        Ok(reify_ir::Value::Real(v)) | Ok(reify_ir::Value::Scalar { si_value: v, .. })
            if v.is_finite() && v >= 0.0 =>
        {
            Some(reify_ir::Value::from_real_scalar(v, dimension))
        }
        Ok(reify_ir::Value::Real(v)) | Ok(reify_ir::Value::Scalar { si_value: v, .. }) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel returned a non-finite or negative value ({v}); \
                 cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel reply has unexpected type (expected Real, \
                 got {unexpected:?}); cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}(...) kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `BoundingBox` query and decode the canonical 6-field JSON reply
/// (`{"xmin":_,"ymin":_,"zmin":_,"xmax":_,"ymax":_,"zmax":_}`) into
/// `Value::BoundingBox { min, max }` — two `Point3<Length>` corners. Reuses
/// `topology_selectors::parse_bbox_axis_extents` once per axis (the same parser
/// the extremal selectors use) rather than introducing a new 6-field decoder.
///
/// Returns `Some(Value::Undef)` + one Warning on a kernel error or malformed
/// reply (PRD §4 defensive downgrade), mirroring the volume/area/centroid arms.
/// The `?`-chain unifies the kernel `query` error and the per-axis parse error
/// (both `reify_ir::QueryError`) into the single `Err` arm.
fn dispatch_bounding_box(
    kernel: &dyn reify_ir::GeometryKernel,
    handle: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    fn decode(
        kernel: &dyn reify_ir::GeometryKernel,
        handle: GeometryHandleId,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        let reply = kernel.query(&reify_ir::GeometryQuery::BoundingBox(handle))?;
        let (xmin, xmax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'x')?;
        let (ymin, ymax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'y')?;
        let (zmin, zmax) = crate::topology_selectors::parse_bbox_axis_extents(&reply, b'z')?;
        Ok(reify_ir::Value::BoundingBox {
            min: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(xmin),
                reify_ir::Value::length(ymin),
                reify_ir::Value::length(zmin),
            ])),
            max: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(xmax),
                reify_ir::Value::length(ymax),
                reify_ir::Value::length(zmax),
            ])),
        })
    }

    match decode(kernel, handle) {
        Ok(value) => Some(value),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "bounding_box(...) kernel query/parse failed: {err}; cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ── Kinematic-query dispatch (task 2531) ────────────────────────────────────
//
// `try_eval_kinematic_query` is the kernel-aware eval-time dispatch for the
// stdlib helpers `interferes`, `interferes_with`, `min_clearance` (kinematic-
// constraints PRD task 8). Sibling to `try_eval_conformance_query`.
//
// Each helper consumes a Snapshot let-cell (the FK-evaluated Map produced by
// `snapshot()`) plus, for the binary forms, two Int let-cells holding body
// ids. The Snapshot's body records carry a `solid: Value::String("name")`
// field — the helper resolves each `name` to a `GeometryHandleId` via the
// per-template `named_steps` map populated by `execute_realization_ops`.
// All three helpers share OCCT's `BRepExtrema_DistShapeShape` primitive
// (`GeometryQuery::Distance`) — `interferes_with` is just `Distance ≤ 0`.
//
// FK placement via ApplyTransform (task 3906 T8): the Snapshot's per-body
// `world_transform` IS applied to the OCCT shape before the distance probe via
// the shared `GeometryOp::ApplyTransform` primitive — the same path T5 static
// `at` placement uses (`decompose_transform_to_arrays` + `surface_subtree`
// identity short-circuit). Each non-identity body transform is applied ONCE
// (O(N) `ApplyTransform` ops) before the O(N²) pairwise `Distance` probes,
// so posed handle ids are reused across all pairs. Identity / missing /
// undecomposable `world_transform` falls back to the raw source handle with
// no kernel op — preserving `fixed()`-joint fixtures unchanged.
//
// Self-pair exclusion: `interferes` iterates pairs as `i < j` upper-triangular
// — excluding both `(a, a)` self-pairs and the duplicate `(b, a)` ordering.
// Same-chain-segment exclusion (parent/child immediate joints sharing a face)
// is not done here — task 8 acceptance only requires self-pair exclusion.
//
// Returns:
//   `Some(Value::List(_))`   for `interferes` — list of pair Maps
//                            `{ "a": Int, "b": Int }`. Empty when no pair
//                            satisfies `Distance ≤ 0`.
//   `Some(Value::Bool(_))`   for `interferes_with`.
//   `Some(Value::Scalar { dimension: LENGTH, .. })` for `min_clearance`.
//   `Some(Value::Undef)`     when arg shapes pass but a runtime resolution
//                            fails (unresolvable `solid` name in
//                            `named_steps`, kernel error, missing body id,
//                            etc.) — defensive downgrade with Warning.
//   `None`                   when the expression is not a recognised
//                            kinematic-query helper, or the arg shape is
//                            unsupported (literal, non-`ValueRef`, missing
//                            snapshot in `values`). Callers fall through to
//                            the cell's compiled default (`Value::Undef`).
pub(crate) fn try_eval_kinematic_query(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (1b) Swept flat_map branch: flat_map(snaps, |s| [kin_helper(s, a, b)]).
    // Checked before the helper-name match (step 2) so the `flat_map` name
    // does not fall through to the `_ => return None` arm. Non-kinematic
    // flat_map lambdas (e.g. `center_of_mass`) return None from
    // `try_eval_swept_kinematic_query`, preserving the pure-eval value.
    if function.name == "flat_map" {
        return try_eval_swept_kinematic_query(
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            pose_cache,
        );
    }

    // (2) Must be one of the three recognised helper names.
    let helper = match function.name.as_str() {
        "interferes" => KinematicHelper::Interferes,
        "interferes_with" => KinematicHelper::InterferesWith,
        "min_clearance" => KinematicHelper::MinClearance,
        _ => return None,
    };

    // (3) Per-helper arity guard.
    let expected_args = helper.arity();
    if args.len() != expected_args {
        return None;
    }

    // (4) args[0] must be a `ValueRef` to a let-cell holding the Snapshot
    // Map. Literal / inline-call shapes fall through to None so the cell
    // stays at its compiled default (`Value::Undef`) — mirrors the
    // `try_eval_conformance_query` arg-shape contract.
    let snapshot_cell = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let snapshot_value = values.get(snapshot_cell)?;

    // For the binary forms, args[1] / args[2] are the `Value::Int` body ids
    // (evaluate-then-accept, task ε: an inline integer literal now works, and a
    // defined-but-wrong value emits a Warning rather than falling through
    // silently). Pulled out so the unary `interferes` arm doesn't pay for it.
    let body_id_args = if expected_args == 3 {
        let a = resolve_int_value_ref(&args[1], values, &function.name, "body_a", diagnostics)?;
        let b = resolve_int_value_ref(&args[2], values, &function.name, "body_b", diagnostics)?;
        Some((a, b))
    } else {
        None
    };

    // (5–7) Delegate the per-snapshot core (extract bodies → build id→handle
    // with FK ApplyTransform → dispatch per-helper) to
    // `eval_kinematic_on_snapshot`. The swept flat_map branch calls the same
    // function for each element of the snapshot list (task 3844).
    eval_kinematic_on_snapshot(
        helper,
        &function.name,
        snapshot_value,
        body_id_args,
        named_steps,
        kernel,
        diagnostics,
        pose_cache,
    )
}

/// Per-snapshot kinematic dispatch: resolve bodies from a Snapshot Map, apply
/// FK world_transforms via `GeometryOp::ApplyTransform`, and run the
/// per-helper kernel probe (`interferes`, `interferes_with`, `min_clearance`).
///
/// Extracted from `try_eval_kinematic_query` so the swept flat_map branch
/// (`try_eval_swept_kinematic_query`) can invoke the same per-snapshot logic
/// for each element of a snapshot list (task 3844, KCC-epsilon).
///
/// `fn_name` is used only in Warning diagnostics; pass the stdlib function
/// name (e.g. `"min_clearance"`) for readable messages.
///
/// Returns:
///   `Some(Value)` on success — List / Bool / length Scalar per helper.
///   `Some(Value::Undef)` when the snapshot Map is malformed or a kernel
///     operation fails — the caller receives the per-snapshot Undef rather
///     than collapsing the entire swept result.
#[allow(clippy::too_many_arguments)]
fn eval_kinematic_on_snapshot(
    helper: KinematicHelper,
    fn_name: &str,
    snapshot_value: &reify_ir::Value,
    body_id_args: Option<(i64, i64)>,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // (5) Read the Snapshot's bodies list. Returns Some(Value::Undef) (not
    // None) when the cell value isn't a well-formed Snapshot — the stdlib
    // stub already validated this on the value-eval pass, so reaching here
    // with a non-Snapshot indicates a stale / mismatched cell rather than
    // a parser-time shape; surfacing Undef is more visible than silently
    // falling back to the compiled default.
    let bodies = match extract_snapshot_bodies(snapshot_value) {
        Some(b) => b,
        None => return Some(reify_ir::Value::Undef),
    };

    // (6) Build (id → handle) by resolving each body's `solid` String against
    // `named_steps`. Bodies whose `solid` doesn't appear in `named_steps`
    // (e.g. a snapshot of a mechanism whose source let-name was never
    // realised because the structure has no realization for it) are
    // skipped — the helper still works for the realised subset.
    let mut id_to_handle: Vec<(i64, GeometryHandleId)> = Vec::with_capacity(bodies.len());
    for body in bodies {
        let body_map = match body {
            reify_ir::Value::Map(m) => m,
            _ => return Some(reify_ir::Value::Undef),
        };
        let id = match body_map.get(&reify_ir::Value::String("id".to_string())) {
            Some(reify_ir::Value::Int(n)) => *n,
            _ => return Some(reify_ir::Value::Undef),
        };
        // Binary helpers only probe two specific body ids — skip all others to
        // avoid O(N-2) wasted ApplyTransform ops per query (e.g. a 50-body
        // snapshot calling min_clearance(s, a, b) would otherwise pose 48
        // irrelevant bodies). The unary `interferes` helper needs all bodies.
        if let Some((qid_a, qid_b)) = body_id_args
            && id != qid_a
            && id != qid_b
        {
            continue;
        }
        let solid_name = match body_map.get(&reify_ir::Value::String("solid".to_string())) {
            Some(reify_ir::Value::String(s)) => s,
            // Non-string `solid` (e.g. a stale `Value::Undef` from a body whose
            // source-let was a geometry call) is not resolvable here — skip the
            // body silently rather than collapsing the entire query to Undef.
            _ => continue,
        };
        if let Some(handle) = named_steps.get(solid_name) {
            let raw_id = handle.id;
            // Apply the body's FK world_transform (if present and decomposable)
            // via the shared ApplyTransform primitive so the distance probe
            // operates on FK-posed geometry. Identity/missing world_transform
            // falls back to the raw handle (no kernel op).
            let posed_id = if let Some(wt) =
                body_map.get(&reify_ir::Value::String("world_transform".to_string()))
            {
                match decompose_transform_to_arrays(wt) {
                    Some((rotation, translation))
                        if rotation != [1.0, 0.0, 0.0, 0.0] || translation != [0.0, 0.0, 0.0] =>
                    {
                        // Cache posed handles for the duration of the build
                        // pass: a typical structure calls interferes/interferes_with/min_clearance on
                        // the same snapshot, so without a cache each non-identity
                        // body is re-posed once per query (3× the kernel ops
                        // for the same geometry). The key is (source handle,
                        // rotation bits, translation bits) — bit-exact to avoid
                        // float equality pitfalls across calls.
                        let cache_key = (
                            raw_id,
                            rotation.map(f64::to_bits),
                            translation.map(f64::to_bits),
                        );
                        if let Some(&cached_id) = pose_cache.get(&cache_key) {
                            cached_id
                        } else {
                            match kernel.execute(&reify_ir::GeometryOp::ApplyTransform {
                                target: raw_id,
                                rotation,
                                translation,
                            }) {
                                Ok(posed) => {
                                    pose_cache.insert(cache_key, posed.id);
                                    posed.id
                                }
                                Err(e) => {
                                    // A partial pose would mix FK-posed and
                                    // unposed handles in the same pairwise
                                    // probe, yielding a geometrically
                                    // meaningless result. Collapse the whole
                                    // query to Undef (consistent with the
                                    // kernel_distance error arm) so the failure
                                    // is visible rather than silently wrong.
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "{fn_name}: ApplyTransform failed for body '{solid_name}': {e}",
                                    )));
                                    return Some(reify_ir::Value::Undef);
                                }
                            }
                        }
                    }
                    _ => raw_id,
                }
            } else {
                raw_id
            };
            id_to_handle.push((id, posed_id));
        }
        // Bodies whose solid name isn't in named_steps: skipped (see comment
        // above the loop).
    }

    // (7) Dispatch per-helper.
    match helper {
        KinematicHelper::Interferes => {
            let mut pairs = Vec::new();
            for i in 0..id_to_handle.len() {
                for j in (i + 1)..id_to_handle.len() {
                    let (id_a, handle_a) = id_to_handle[i];
                    let (id_b, handle_b) = id_to_handle[j];
                    match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                        Some(d) if d <= 0.0 => {
                            pairs.push(make_pair_map(id_a, id_b));
                        }
                        Some(_) => {}
                        // Kernel error already emitted a Warning diagnostic
                        // — collapse the whole query to Undef so the cell
                        // exposes the failure rather than a partial list.
                        None => return Some(reify_ir::Value::Undef),
                    }
                }
            }
            Some(reify_ir::Value::List(pairs))
        }
        KinematicHelper::InterferesWith => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair: per the PRD acceptance, "a single body's interference
            // with itself is not reported". Returning Bool(false) here is a
            // defensive fallback — typical user-code uses distinct ids.
            if id_a == id_b {
                return Some(reify_ir::Value::Bool(false));
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                Some(d) => Some(reify_ir::Value::Bool(d <= 0.0)),
                None => Some(reify_ir::Value::Undef),
            }
        }
        KinematicHelper::MinClearance => {
            let (id_a, id_b) = body_id_args.expect("3-arg form populated body_id_args");
            // Self-pair clearance is undefined — surfacing 0.0 would lie about
            // a degenerate input. Returning Undef pushes the user toward
            // distinct ids; pinned by the smoke-test self-pair arm.
            if id_a == id_b {
                return Some(reify_ir::Value::Undef);
            }
            let handle_a = match handle_for_id(&id_to_handle, id_a) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            let handle_b = match handle_for_id(&id_to_handle, id_b) {
                Some(h) => h,
                None => return Some(reify_ir::Value::Undef),
            };
            match kernel_distance(kernel, handle_a, handle_b, diagnostics, fn_name) {
                Some(d) => Some(reify_ir::Value::length(d)),
                None => Some(reify_ir::Value::Undef),
            }
        }
    }
}

/// Swept kinematic-query dispatch for `flat_map(snaps, |s| [kin_helper(s, a, b)])`.
///
/// Called by `try_eval_kinematic_query` when the outer function name is
/// `flat_map`. Validates that:
///   - `args[0]` is a `ValueRef` to a `Value::List` of Snapshot Maps.
///   - `args[1]` is a `Lambda { param_ids: [s_id], body: ListLiteral([inner]) }`.
///   - `inner` is a binary kinematic helper call (`interferes_with` or
///     `min_clearance`) with `args[0] == ValueRef(s_id)` (the lambda param)
///     and `args[1..]` resolving to `Int` body ids in `values`.
///
/// On match: runs `eval_kinematic_on_snapshot` for each snapshot and returns
/// `Some(Value::List(results))` — one result per snapshot (Undef on per-
/// snapshot failure, rather than collapsing the whole list).
///
/// On any mismatch (non-kinematic inner, wrong shape, non-Int captures):
/// returns `None` so the cell keeps the pure-eval value (e.g.
/// `center_of_mass` swept cells computed by the regular eval pass).
///
/// The unary `interferes` swept form is intentionally not supported: it would
/// concatenate pair-lists ambiguously. Falls through to None.
fn try_eval_swept_kinematic_query(
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    pose_cache: &mut HashMap<(GeometryHandleId, [u64; 4], [u64; 3]), GeometryHandleId>,
) -> Option<reify_ir::Value> {
    // flat_map must have exactly 2 args: (list_arg, lambda_arg).
    if args.len() != 2 {
        return None;
    }

    // args[0] must be a ValueRef to a list of Snapshots.
    let list_id = match &args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let snapshots = match values.get(list_id) {
        Some(reify_ir::Value::List(snaps)) => snaps,
        _ => return None,
    };

    // args[1] must be a Lambda with exactly one parameter (the snapshot `s`).
    let (s_param_id, body) = match &args[1].kind {
        reify_ir::CompiledExprKind::Lambda {
            param_ids, body, ..
        } if param_ids.len() == 1 => (&param_ids[0], body.as_ref()),
        _ => return None,
    };

    // Lambda body must be ListLiteral([inner]) — a single-element list.
    let inner = match &body.kind {
        reify_ir::CompiledExprKind::ListLiteral(elems) if elems.len() == 1 => &elems[0],
        _ => return None,
    };

    // inner must be a binary kinematic helper call with 3 args.
    let (inner_fn, inner_args) = match &inner.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args.as_slice()),
        _ => return None,
    };
    let helper = match inner_fn.name.as_str() {
        "interferes_with" => KinematicHelper::InterferesWith,
        "min_clearance" => KinematicHelper::MinClearance,
        // Unary `interferes` and non-kinematic names (e.g. center_of_mass)
        // → fall through so the pure-eval value is preserved.
        _ => return None,
    };
    if inner_args.len() != 3 {
        return None;
    }

    // inner_args[0] must be ValueRef to the lambda parameter (the snapshot `s`).
    let arg0_ref = match &inner_args[0].kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    if arg0_ref != s_param_id {
        return None;
    }

    // inner_args[1] and [2] are the Int body ids (evaluate-then-accept, task ε:
    // an inline integer literal now works, and a defined-but-wrong value emits a
    // Warning rather than falling through silently).
    let id_a = resolve_int_value_ref(
        &inner_args[1],
        values,
        &inner_fn.name,
        "body_a",
        diagnostics,
    )?;
    let id_b = resolve_int_value_ref(
        &inner_args[2],
        values,
        &inner_fn.name,
        "body_b",
        diagnostics,
    )?;
    let body_id_args = Some((id_a, id_b));

    // For each snapshot in the list run the per-snapshot dispatch core and
    // collect results. Per-snapshot failures (None) become Value::Undef so
    // the list length is always equal to the snapshot count.
    let fn_name = inner_fn.name.as_str();
    let mut out: Vec<reify_ir::Value> = Vec::with_capacity(snapshots.len());
    for snap in snapshots {
        let result = eval_kinematic_on_snapshot(
            helper,
            fn_name,
            snap,
            body_id_args,
            named_steps,
            kernel,
            diagnostics,
            pose_cache,
        );
        out.push(result.unwrap_or(reify_ir::Value::Undef));
    }
    Some(reify_ir::Value::List(out))
}

#[derive(Clone, Copy)]
enum KinematicHelper {
    Interferes,
    InterferesWith,
    MinClearance,
}

impl KinematicHelper {
    fn arity(self) -> usize {
        match self {
            KinematicHelper::Interferes => 1,
            KinematicHelper::InterferesWith | KinematicHelper::MinClearance => 3,
        }
    }
}

/// Resolve a kinematic body-id arg (the `id_a` / `id_b` positionals of
/// `interferes_with` / `min_clearance`) to its `i64` value, emitting a
/// `Severity::Warning` when the caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::Int` cell (the common `let id_a = …` form) reads the cell (now an
/// owned clone; see [`eval_arg_value`]) — functionally identical to the prior
/// `values.get(id)` path — while an inline integer expression now EVALUATES
/// rather than falling through to a silent `None`. The
/// γ-style "non-`ValueRef` shape → silent fall-through" contract is gone.
///
/// | evaluated arg value                              | return    | diagnostic?     |
/// |--------------------------------------------------|-----------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg) | `None`    | no — quiet      |
/// | `Value::Int(n)`                                  | `Some(n)` | no              |
/// | any other defined value (Real, Scalar, …)        | `None`    | yes — 1 Warning |
fn resolve_int_value_ref(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<i64> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    match value {
        // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg
        // the local ctx can't evaluate) returns None with no diagnostic —
        // behaviourally identical to the prior `values.get(id)` fall-through.
        reify_ir::Value::Undef => None,
        reify_ir::Value::Int(n) => Some(n),
        // Defined-but-wrong (non-Int): emit exactly one Warning naming
        // builtin/arg/Int/got (byte-uniform wording with the density / point /
        // vec3 / range paths).
        other => {
            diagnostics.push(Diagnostic::warning(
                ArgRejection {
                    got: int_got_label(&other),
                    expected: "Int",
                    migration_hint: None,
                }
                .message(builtin, arg_name),
            ));
            None
        }
    }
}

/// Dimension-qualified label for a `Value::Scalar`, mirroring
/// `arg_acceptance::value_short_label` so the `got` payload of task ε's
/// non-scalar resolvers (int / point / vec3 / range / string) reports the
/// SAME dimension-qualified Scalar wording as the density / scalar-dim paths
/// that route through `accept_arg` — e.g. `"MASS_DENSITY Scalar"`,
/// `"dimensionless Scalar"`, `"dimensioned Scalar"`. `value_short_label` is
/// module-private to `arg_acceptance` (owned by task δ, not modified here), so
/// the Scalar arm is replicated rather than shared.
fn scalar_got_label(dimension: &reify_core::DimensionVector) -> String {
    if dimension.is_dimensionless() {
        "dimensionless Scalar".to_string()
    } else if let Some(name) = dimension.canonical_name() {
        format!("{name} Scalar")
    } else {
        "dimensioned Scalar".to_string()
    }
}

/// Short human-readable label for a `Value` that failed Int classification,
/// used as the `got` field of the rejection diagnostic (task ε).
fn int_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::String(_) => "String".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        _ => "non-Int value".to_string(),
    }
}

/// Extract the `bodies` list from a Snapshot Map, validating
/// `kind="snapshot"`. Mirrors `reify_stdlib::snapshot::snapshot_bodies` —
/// duplicated here because the stdlib helper is module-private.
fn extract_snapshot_bodies(snap: &reify_ir::Value) -> Option<Vec<reify_ir::Value>> {
    let map = match snap {
        reify_ir::Value::Map(m) => m,
        _ => return None,
    };
    if map.get(&reify_ir::Value::String("kind".to_string()))
        != Some(&reify_ir::Value::String("snapshot".to_string()))
    {
        return None;
    }
    match map.get(&reify_ir::Value::String("bodies".to_string())) {
        Some(reify_ir::Value::List(b)) => Some(b.clone()),
        _ => None,
    }
}

fn handle_for_id(pairs: &[(i64, GeometryHandleId)], id: i64) -> Option<GeometryHandleId> {
    pairs.iter().find(|(i, _)| *i == id).map(|(_, h)| *h)
}

/// Build the `{ "a": Int, "b": Int }` pair Map returned by `interferes`.
/// Alphabetical key order matches `BTreeMap` iteration so that List
/// equality used in the smoke tests is stable across iterations.
fn make_pair_map(id_a: i64, id_b: i64) -> reify_ir::Value {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        reify_ir::Value::String("a".to_string()),
        reify_ir::Value::Int(id_a),
    );
    m.insert(
        reify_ir::Value::String("b".to_string()),
        reify_ir::Value::Int(id_b),
    );
    reify_ir::Value::Map(m)
}

/// Issue a `GeometryQuery::Distance` against the kernel and reduce to a raw
/// SI metres f64. Returns `None` (and emits a Warning diagnostic) on kernel
/// error or when the kernel returns a non-numeric `Value` — caller maps
/// `None` to a defensive `Value::Undef`.
///
/// `pub(crate)` so `Engine::distance_between_placed` (engine_build.rs) can
/// reuse the same error-handling convention (T7 task 3905).
pub(crate) fn kernel_distance(
    kernel: &dyn reify_ir::GeometryKernel,
    from: GeometryHandleId,
    to: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
    helper_name: &str,
) -> Option<f64> {
    let query = reify_ir::GeometryQuery::Distance { from, to };
    match kernel.query(&query) {
        Ok(reify_ir::Value::Real(d)) => Some(d),
        // Some kernels (e.g. test-support `MockGeometryKernel::with_distance_result`)
        // store the value as a length-dimensioned `Scalar` instead of a raw
        // `Real`. Read the SI value either way so the dispatch stays kernel-
        // agnostic; the dimension itself is unused (the helpers' return-side
        // dimension is fixed by the helper, not the kernel reply).
        Ok(reify_ir::Value::Scalar { si_value, .. }) => Some(si_value),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel Distance({:?}, {:?}) returned non-numeric value {:?}; treating as undefined",
                helper_name, from, to, other
            )));
            None
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel Distance({:?}, {:?}) failed: {}",
                helper_name, from, to, err
            )));
            None
        }
    }
}

// ── Topology-selector dispatch (tasks 2324, 2699) ────────────────────────────
//
// `try_eval_topology_selector` is the kernel-aware eval-time dispatch for the
// topology-selector helper family (PRD `docs/prds/topology-selectors.md`
// §3.9). Sibling to `try_eval_conformance_query` and
// `try_eval_kinematic_query` — same arg-shape / fall-through contract.
//
// ── Which names get eval dispatch here (task 2324) ──────────────────────────
//
// The per-name `match` at step (2) below is the SOURCE OF TRUTH for which
// helpers get a kernel-routed `Value` payload — NOT the compile-time recogniser
// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` in `reify_compiler::units` (which is the
// broader classification list).
//
// Currently dispatched:
//   `closest_point(point, geometry)` → `GeometryQuery::ClosestPointOnShape`
//   `is_on(point, geometry)`         → `GeometryQuery::PointOnShape`
//   `angle_between_surfaces(a, b)`   → `GeometryQuery::SurfaceAngle`
//   `angle(a, b)`                    → pure-math acos (task 3614, KGQ-ε)
//   `contains(solid, point)`         → `GeometryQuery::Contains` (task 3611, KGQ-β)
//   `geo_equiv(left, right, tol)`   → `GeometryQuery::GeoEquiv`  (task 3613, KGQ-δ)
//
// ── Which names are compile-time typed but NOT eval-dispatched (task 2699) ──
//
// Task 2699 added 11 names to `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` and
// `topology_selector_result_type`, wiring their compile-time cell types.
// They fall through the `_ => return None` arm at step (2) below, so the
// cell stays at the `Value::Undef` set by the regular eval path.
// `value_type_kind_matches` accepts `Value::Undef` for any type
// (`reify_eval::lib:196`), so the cell typechecks until task 2691 wires
// the actual dispatch arms here:
//   `edges` / `faces`                       → List<Geometry>  (task 2691)
//   `edges_by_length` / `faces_by_area`     → List<Geometry>  (task 2691)
//   `faces_by_normal` / `edges_parallel_to` → List<Geometry>  (task 2691)
//   `edges_at_height`                       → List<Geometry>  (task 2691)
//   `adjacent_faces` / `shared_edges`       → List<Geometry>  (task 2691)
//   `center_of_mass`                        → Point3<Length>  (task 2691)
//   `moment_of_inertia`                     → Tensor<2,3,MI>  (task 2691)
//
// Arg-shape contract (applies to all dispatched names):
//   - Both args must be `ValueRef`s — literal / inline-call shapes fall
//     through to `None` so the cell stays at its compiled default
//     (`Value::Undef`). Pinned by the
//     `try_eval_topology_selector_*_literal_args_falls_through_to_none`
//     unit tests.
//   - For `closest_point` / `is_on`: args[0] must resolve in `values` to a
//     `Value::Point` of three Length-dimensioned scalars; args[1] must
//     resolve in `named_steps` to a `GeometryHandleId` (let-bound geometry).
//   - For `angle_between_surfaces`: both args must resolve in `named_steps`
//     to a `GeometryHandleId`.
//
// Returns:
//   `Some(Value::Point(vec![length, length, length]))` for `closest_point`
//                          (parsed from the kernel's JSON-Point3 reply).
//   `Some(Value::Bool(_))` for `is_on`.
//   `Some(Value::Scalar { dimension: ANGLE, .. })` for
//                          `angle_between_surfaces` and `angle`.
//   `Some(Value::Undef)`   on a kernel error or a malformed kernel reply
//                          (defensive downgrade with a Warning diagnostic);
//                          also for `angle` with zero-length / non-finite
//                          input (Warning emitted, no kernel call).
//   `None`                 when the expression is not a recognised
//                          topology-selector helper, or the arg shape is
//                          unsupported. Callers fall through to the cell's
//                          compiled default.
/// Resolve a selector constructor's parent-solid argument (`arg[0]`) to a
/// [`reify_ir::value::GeometryHandleRef`] target, accepting **both** symbolic
/// (`kernel_handle=None`, eval path) and realized (`kernel_handle=Some`, build path)
/// handles (task 4118 γ, widened by R2b task #4653).
///
/// Falls through to `None` (cell stays at `Value::Undef`) for any non-`ValueRef`
/// expr, a missing cell, or a cell that holds a non-`GeometryHandle` value
/// (PRD invariant #2: never partially-construct a selector target).
///
/// **Used exclusively by [`try_build_kernel_free_leaf_selector`]** so that the
/// widening (accepting `None`) cannot leak into build-path callers that require a
/// realized handle.  All other call sites use the realized-only
/// [`resolve_selector_target`], which wraps this function and additionally requires
/// `kernel_handle.is_some()`.
fn resolve_symbolic_selector_target(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::value::GeometryHandleRef> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    let value = values.get(cell_id)?;
    reify_ir::value::GeometryHandleRef::from_geometry_handle(value)
}

/// Realized-only variant of [`resolve_symbolic_selector_target`]: requires
/// `kernel_handle.is_some()` and returns `None` for a symbolic handle
/// (`kernel_handle=None`).
///
/// Used by [`try_eval_feature_datum_projection`], the build-path named-leaf ctor
/// [`eval_named_leaf_selector_ctor`], and the two non-kernel-free build-path
/// topology-selector helpers (`AdjacentFaces`/`SharedEdges`) — all of which need
/// a live kernel handle to proceed.  Preserves the pre-R2b contract for every
/// call site except the new kernel-free leaf path.
fn resolve_selector_target(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::value::GeometryHandleRef> {
    let ghr = resolve_symbolic_selector_target(expr, values)?;
    ghr.kernel_handle?;
    Some(ghr)
}

/// Resolve a `Feature`-typed argument expr to its [`reify_ir::FeatureId`]
/// (task 4831, P3β). Mirrors [`resolve_symbolic_selector_target`]: only a
/// `ValueRef` to a cell holding `Value::Feature(fid)` resolves; any other
/// expr shape, a missing cell, or a cell holding a non-`Feature` value
/// returns `None` (PRD invariant #2: never partially-construct a selector).
///
/// Used exclusively by [`try_build_kernel_free_leaf_selector`]'s
/// `CreatedByFeature`/`SplitByFeature` arms to resolve the `f : Feature`
/// argument — the sibling of `resolve_symbolic_selector_target`'s `solid`
/// argument resolution.
fn resolve_feature_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::FeatureId> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(cell_id)? {
        reify_ir::Value::Feature(fid) => Some(fid.clone()),
        _ => None,
    }
}

/// Shared kernel-free leaf selector constructor, called by BOTH the eval-path
/// [`try_eval_symbolic_topology_selector`] and the build-path
/// [`try_eval_topology_selector`] for the 9 kernel-free leaf constructors
/// (task #4653 R2b, suggestion 2 — eliminates drift risk between the two paths).
///
/// Uses [`resolve_symbolic_selector_target`] (accepts `kernel_handle=None`) so
/// that eval-path symbolic targets flow through unchanged, while build-path targets
/// (always realized, `Some`) are handled identically to before.  Arity is assumed
/// valid — callers must check `helper.expected_arity()` before dispatching.
///
/// Returns `None` for every `TopologySelectorHelper` variant that is NOT one of
/// the 9 kernel-free leaf ctors (e.g. `ClosestPoint`, `AdjacentFaces`,
/// composition `Union`/`Intersect`/`Difference`, named-leaf `Face`/`Edge`).
fn try_build_kernel_free_leaf_selector(
    helper: TopologySelectorHelper,
    args: &[reify_ir::CompiledExpr],
    values: &reify_ir::ValueMap,
    function_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match helper {
        // ── arity-1 All-leaf ctors ──────────────────────────────────────────
        TopologySelectorHelper::Edges => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::All,
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::Faces => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::All,
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::MidSurface => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByRole(reify_ir::Role::MidSurfaceFace),
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::Vertices => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Vertex,
                target,
                reify_ir::value::LeafQuery::All,
                function_name,
                diagnostics,
            )
        }
        // ── arity-2 predicate-leaf ctors ───────────────────────────────────
        TopologySelectorHelper::EdgesByLength => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let (min_m, max_m) = resolve_range_dim_arg(
                &args[1],
                values,
                reify_core::DimensionVector::LENGTH,
                function_name,
                "length_range",
                diagnostics,
            )?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByLength { min_m, max_m },
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::FacesByArea => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let (min_m2, max_m2) = resolve_range_dim_arg(
                &args[1],
                values,
                reify_core::DimensionVector::AREA,
                function_name,
                "area_range",
                diagnostics,
            )?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByArea { min_m2, max_m2 },
                function_name,
                diagnostics,
            )
        }
        // ── arity-3 predicate-leaf ctors ───────────────────────────────────
        TopologySelectorHelper::FacesByNormal => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let dir =
                resolve_vec3_arg(&args[1], values, function_name, "dir", diagnostics)?;
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByNormal { dir, tol_rad },
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesParallelTo => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let axis =
                resolve_vec3_arg(&args[1], values, function_name, "axis", diagnostics)?;
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByParallel { axis, tol_rad },
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesAtHeight => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let z_m =
                resolve_length_scalar_arg(&args[1], values, function_name, "z", diagnostics)?;
            let tol_m =
                resolve_length_scalar_arg(&args[2], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByHeight { z_m, tol_m },
                function_name,
                diagnostics,
            )
        }
        // ── Task 3523: selector_vocabulary_v2 leaf-predicate ctors ──────────
        // arity-3 perpendicular ctors (solid, dir, tol) — reuse the faces_by_normal
        // arg-parse path verbatim; ByPerpendicular is kind-agnostic so the kind is
        // fixed by the constructor name (Face vs Edge).
        TopologySelectorHelper::FacesPerpendicularTo => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let axis =
                resolve_vec3_arg(&args[1], values, function_name, "axis", diagnostics)?;
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByPerpendicular { axis, tol_rad },
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesPerpendicularTo => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let axis =
                resolve_vec3_arg(&args[1], values, function_name, "axis", diagnostics)?;
            let tol_rad =
                resolve_angle_scalar_arg(&args[2], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByPerpendicular { axis, tol_rad },
                function_name,
                diagnostics,
            )
        }
        // arity-2 surface/curve-kind ctors (solid, name) — string literal → enum.
        TopologySelectorHelper::FacesBySurfaceKind => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let kind =
                resolve_face_surface_kind_arg(&args[1], values, function_name, diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::BySurfaceKind(kind),
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::EdgesByCurveKind => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let kind =
                resolve_edge_curve_kind_arg(&args[1], values, function_name, diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Edge,
                target,
                reify_ir::value::LeafQuery::ByCurveKind(kind),
                function_name,
                diagnostics,
            )
        }
        // arity-4 extremal ctors (solid, axis, sense, tol) — registered Face-kind.
        TopologySelectorHelper::ExtremalByBbox => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let axis_index =
                resolve_axis_index_arg(&args[1], values, function_name, diagnostics)?;
            let max =
                resolve_extremal_sense_arg(&args[2], values, function_name, diagnostics)?;
            let tol_m =
                resolve_length_scalar_arg(&args[3], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByExtremalBbox { axis_index, max, tol_m },
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::ExtremalByCentroid => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let axis_index =
                resolve_axis_index_arg(&args[1], values, function_name, diagnostics)?;
            let max =
                resolve_extremal_sense_arg(&args[2], values, function_name, diagnostics)?;
            let tol_m =
                resolve_length_scalar_arg(&args[3], values, function_name, "tol", diagnostics)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::ByExtremalCentroid { axis_index, max, tol_m },
                function_name,
                diagnostics,
            )
        }
        // ── arity-2 provenance leaf ctors (task 4831, P3β) ──────────────────
        // args[0]: parent solid (resolve_symbolic_selector_target, as above).
        // args[1]: the feature f : Feature (resolve_feature_arg — the sibling
        // arg resolver for a Value::Feature(fid) ValueRef).
        TopologySelectorHelper::CreatedByFeature => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let fid = resolve_feature_arg(&args[1], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::CreatedByFeature(fid),
                function_name,
                diagnostics,
            )
        }
        TopologySelectorHelper::SplitByFeature => {
            let target = resolve_symbolic_selector_target(&args[0], values)?;
            let fid = resolve_feature_arg(&args[1], values)?;
            build_leaf_selector(
                reify_core::ty::SelectorKind::Face,
                target,
                reify_ir::value::LeafQuery::SplitByFeature(fid),
                function_name,
                diagnostics,
            )
        }
        // Non-kernel-free helpers (kernel-bearing, composition, named-leaf):
        // return None so the cell stays at Value::Undef.
        _ => None,
    }
}

/// Package a kernel-FREE leaf `Value::Selector` (task 4118 γ): the 7
/// predicate/all selector constructors evaluate to a typed
/// `Value::Selector(kind)` pairing the parent solid handle (`target`) with a
/// `LeafQuery` describing the predicate. NO kernel query is issued here — the
/// `Selector → List<Geometry>` resolution is deferred to the compiler-inserted
/// `ResolveSelector` coercion node, executed by `topology_selectors::resolve`
/// (K2/BT7: zero kernel queries during construction).
///
/// `kind` and `query.required_kind()` are statically matched at every call site
/// below, so the K1 kind-closure check in `SelectorValue::leaf` never fails in
/// practice; the defensive `Err` arm emits a Warning and leaves the cell at
/// `Undef` rather than silently dropping it.
fn build_leaf_selector(
    kind: reify_core::ty::SelectorKind,
    target: reify_ir::value::GeometryHandleRef,
    query: reify_ir::value::LeafQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match reify_ir::value::SelectorValue::leaf(kind, target, query) {
        Ok(sv) => Some(reify_ir::Value::Selector(sv)),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name}: selector kind-closure violation ({err:?}); cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Single source of truth for the kernel-free symbolic-eval selector-ctor
/// name→helper map. A `Some(helper)` result means the named ctor is wired into
/// the `eval()`/`eval_cached()` surface (via [`try_eval_symbolic_topology_selector`])
/// and is EXPECTED to mint a `Value::Selector` there without a kernel; `None`
/// means the name is kernel-bearing / composition / named-leaf / unknown and
/// resolves only on the `build()` path.
///
/// Both the dispatcher ([`try_eval_symbolic_topology_selector`]) and clause-8's
/// "still in the α net" guard ([`is_symbolic_eval_wired_selector_ctor`]) read
/// this ONE map, so the EVAL_WIRED set can never drift between them — adding a
/// new kernel-free leaf ctor here automatically (a) lets it resolve on eval and
/// (b) keeps it OUT of the build-only exemption, with no second edit.
///
/// Module-private (not `pub(crate)`): both callers live in this module, and the
/// private `TopologySelectorHelper` return type must not leak past it
/// (`private_interfaces` lint, `-D warnings` in the gate).
fn symbolic_eval_helper_for_name(name: &str) -> Option<TopologySelectorHelper> {
    Some(match name {
        "faces" => TopologySelectorHelper::Faces,
        "edges" => TopologySelectorHelper::Edges,
        "mid_surface" => TopologySelectorHelper::MidSurface,
        // task 4368: 0-D All-leaf ctor — mirrors faces/edges with Vertex kind.
        "vertices" => TopologySelectorHelper::Vertices,
        "edges_by_length" => TopologySelectorHelper::EdgesByLength,
        "faces_by_area" => TopologySelectorHelper::FacesByArea,
        "faces_by_normal" => TopologySelectorHelper::FacesByNormal,
        "edges_parallel_to" => TopologySelectorHelper::EdgesParallelTo,
        "edges_at_height" => TopologySelectorHelper::EdgesAtHeight,
        // task 3523: selector_vocabulary_v2 leaf-predicate ctors (kernel-free).
        "faces_perpendicular_to" => TopologySelectorHelper::FacesPerpendicularTo,
        "edges_perpendicular_to" => TopologySelectorHelper::EdgesPerpendicularTo,
        "faces_by_surface_kind" => TopologySelectorHelper::FacesBySurfaceKind,
        "edges_by_curve_kind" => TopologySelectorHelper::EdgesByCurveKind,
        "extremal_by_bbox" => TopologySelectorHelper::ExtremalByBbox,
        "extremal_by_centroid" => TopologySelectorHelper::ExtremalByCentroid,
        // task 4831 (P3β): feature-provenance leaf ctors (kernel-free).
        "created_by_feature" => TopologySelectorHelper::CreatedByFeature,
        "split_by_feature" => TopologySelectorHelper::SplitByFeature,
        _ => return None,
    })
}

/// `true` iff `expr` is a `FunctionCall` to a selector constructor that the
/// kernel-free symbolic-eval pass ([`symbolic_eval_helper_for_name`] /
/// [`try_eval_symbolic_topology_selector`]) resolves — i.e. it is EXPECTED to
/// mint a `Value::Selector` on the `eval()`/`eval_cached()` surface.
///
/// Such a call is NOT build-only: clause 8 uses this as the guard that keeps
/// the α stale-Undef net TIGHT. A well-formed leaf ctor that is still `Undef`
/// post-eval with all deps resolved is a genuine bug (wrong arity, or a
/// scheduling miss where its target was not minted in time), which must remain
/// a violation rather than be silently exempted.
pub(crate) fn is_symbolic_eval_wired_selector_ctor(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::FunctionCall { function, .. }
            if symbolic_eval_helper_for_name(&function.name).is_some()
    )
}

/// `true` iff `expr` is a `FunctionCall` that *consumes* a geometry- or
/// selector-typed value — at least one argument of `Type::Geometry` /
/// `Type::Selector` / `Type::AnySelector`.
///
/// This is the STRUCTURAL rule at the heart of clause 8's build-only
/// classifier (task γ / #4954): a `FunctionCall` that consumes geometry/
/// selector but is NOT [`is_symbolic_eval_wired_selector_ctor`] can only
/// resolve against a realized kernel on the `build()`/`tessellate()` path.
/// Keyed on argument *types* (which the compiler has already resolved), it
/// CLOSES the open-ended geometry-consumer class the former clause-8 name
/// lists kept missing — the `face`/`edge`/`solid_body`/`vertex` named-leaf
/// ctors, composition `union`/`intersect`/`difference`, GD&T `max_deviation`,
/// `split`, `feature`, the conformance queries, and any future geometry-arg
/// consumer — with no per-name upkeep.
///
/// It does NOT cover build-only queries that consume ABSTRACTED handles
/// rather than a geometry-typed arg — the kernel geometry-query family's
/// `angle` (over direction vectors) and the kinematic/dynamics queries (over
/// mechanism snapshots + body-ids). Those remain registry-keyed in
/// [`is_build_only_dispatch_call`]; they are a bounded, stable set, not the
/// growing class this structural rule exists to absorb.
///
/// Deliberately `FunctionCall`-only: geometry/selector-receiver `MethodCall`s
/// (the feature→datum projections) are governed by the narrow, receiver-type-
/// gated [`is_feature_datum_projection_call`], so this rule does not broaden
/// method-call exemptions beyond that documented set.
pub(crate) fn consumes_geometry_or_selector(expr: &reify_ir::CompiledExpr) -> bool {
    fn is_geometry_or_selector(t: &reify_core::Type) -> bool {
        matches!(
            t,
            reify_core::Type::Geometry
                | reify_core::Type::Selector(_)
                | reify_core::Type::AnySelector
        )
    }
    match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { args, .. } => {
            args.iter().any(|a| is_geometry_or_selector(&a.result_type))
        }
        _ => false,
    }
}

/// Kernel-FREE eval-path dispatch for the leaf selector constructors over a
/// SYMBOLIC target (R2b, task #4653).
///
/// Maps the function name to a [`TopologySelectorHelper`] via
/// [`symbolic_eval_helper_for_name`], runs the per-helper arity check (via
/// `helper.expected_arity()`), and delegates construction to the shared
/// [`try_build_kernel_free_leaf_selector`] helper. Returns `None` for every
/// kernel-bearing / composition / named-leaf ctor and for any
/// non-`FunctionCall` expr shape — the cell stays at `Value::Undef` (R1a /
/// deferred to R3).
pub(crate) fn try_eval_symbolic_topology_selector(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Must be a FunctionCall — anything else is not a selector constructor.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // Kernel-free leaf ctor name→helper (single source of truth). All other
    // helpers (kernel-bearing, composition, named-leaf, unknown) → None.
    let helper = symbolic_eval_helper_for_name(&function.name)?;

    // Per-helper arity check (same contract as try_eval_topology_selector).
    let expected_arity = helper.expected_arity();
    if args.len() != expected_arity {
        return None;
    }

    try_build_kernel_free_leaf_selector(helper, args, values, &function.name, diagnostics)
}


/// Kernel-free pass that mints `Value::Selector` cells for topology-selector
/// expressions over symbolic `GeometryHandle` targets (task #4653 R2b step-6).
///
/// Called immediately AFTER [`Engine::mint_symbolic_geometry_handles_into_values`]
/// at every eval-path entry point (eval / eval_cached / engine_edit) so the
/// symbolic body handle is already present in `values` when the selector cell
/// resolves its target via [`try_eval_symbolic_topology_selector`].
///
/// Walks every `template.value_cells` in `module.templates`.  For each cell
/// whose `default_expr` is a recognised kernel-free leaf selector constructor
/// (`faces`, `edges`, `mid_surface`, `vertices`, `faces_by_normal`,
/// `faces_by_area`, `edges_by_length`, `edges_parallel_to`, `edges_at_height`)
/// and whose current value is `Undef` or absent, dispatches
/// [`try_eval_symbolic_topology_selector`] and writes the returned
/// `Value::Selector` into `values`.
///
/// Cells already holding a non-`Undef` value (e.g. realized by the build
/// path), or whose expr is a kernel-bearing / unrecognised constructor, are
/// left untouched (`try_eval_symbolic_topology_selector` returns `None`).
///
/// **Collect-then-write** avoids a split-borrow conflict: the immutable
/// `&values` consumed by `try_eval_symbolic_topology_selector` is released
/// before the `&mut values` write-back.  This mirrors
/// `mint_symbolic_geometry_handles_into_values`.
///
/// **Single-pass sufficiency**: topology selectors do not chain through value
/// cells (build-path invariant, `engine_build.rs` `post_process_topology_selectors`),
/// so no entry in this loop depends on another selector cell being patched
/// first — one pass suffices.
///
/// **Return value (R3f, task #4946)**: the set of `ValueCellId`s this call
/// actually flipped Undef → non-Undef — i.e. every entry in `entries`, since
/// the collect loop above already skips cells holding a non-Undef value.
/// Callers feed this into [`crate::Engine::re_eval_consumers_of_in_walk_mints`]
/// (or its `_from_graph` sibling) so a same-pass consumer that read one of
/// these cells BEFORE this post-walk mint ran gets re-checked — closing the
/// gap for selector targets (e.g. a geometry LET) that have no value cell of
/// their own and so never resolve via the in-walk mint retry.
pub(crate) fn mint_symbolic_topology_selectors_into_values(
    module: &reify_compiler::CompiledModule,
    values: &mut reify_ir::ValueMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashSet<reify_core::identity::ValueCellId> {
    use reify_core::identity::ValueCellId;
    use reify_ir::Value;

    // Phase 1: collect (read pass — holds only `&values`).
    let mut entries: Vec<(ValueCellId, Value)> = Vec::new();
    for template in &module.templates {
        for cell in &template.value_cells {
            let Some(default_expr) = &cell.default_expr else {
                continue;
            };
            // Skip cells already holding a non-Undef value (e.g. realized by
            // the build path or an earlier sibling pass).
            match values.get(&cell.id) {
                Some(v) if !matches!(v, Value::Undef) => continue,
                _ => {}
            }
            if let Some(value) =
                try_eval_symbolic_topology_selector(default_expr, values, diagnostics)
            {
                entries.push((cell.id.clone(), value));
            }
        }
    }
    // Phase 2: write-back (requires `&mut values`; &values borrow already dropped).
    let mut flipped = HashSet::with_capacity(entries.len());
    for (cell_id, value) in entries {
        flipped.insert(cell_id.clone());
        values.insert(cell_id, value);
    }
    flipped
}

/// Kernel-bearing evaluation of the compiler-inserted `ResolveSelector`
/// coercion node and `IndexAccess` over a selector (task 4118 γ, step-6).
///
/// `ResolveSelector { selector }` → reconstruct the inner `Value::Selector`
/// (PREFERRED: inline from a nested selector `FunctionCall`, sidestepping
/// value-cell ordering; else a `ValueRef` to an already-patched selector cell),
/// call the single `topology_selectors::resolve` executor, and wrap the
/// canonical-order handle ids as a `Value::List` of `Value::GeometryHandle`
/// sub-handles via `make_sub_handle`.
///
/// `IndexAccess { object: ResolveSelector{..} | <selector FunctionCall>, index }`
/// → resolve the selector to its list then return the indexed element (the
/// `faces(s)[i]` curvature shape).
///
/// Returns `None` for any other expr shape (the geometry_ops `None`-means-skip
/// contract: the cell is left for a sibling pass / the pure eval path).
pub(crate) fn try_eval_resolve_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    table: &reify_ir::TopologyAttributeTable,
    realized_reprs: &HashMap<reify_core::identity::RealizationNodeId, reify_ir::ReprKind>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match &expr.kind {
        reify_ir::CompiledExprKind::ResolveSelector { selector } => {
            resolve_selector_to_list(selector, named_steps, values, kernel, table, realized_reprs, diagnostics)
        }
        reify_ir::CompiledExprKind::IndexAccess { object, index } => {
            // Only handle IndexAccess whose object is a selector / ResolveSelector;
            // ordinary collection indexing is owned by the pure eval_expr path.
            let inner_selector = match &object.kind {
                reify_ir::CompiledExprKind::ResolveSelector { selector } => selector.as_ref(),
                reify_ir::CompiledExprKind::FunctionCall { .. } => object.as_ref(),
                _ => return None,
            };
            match resolve_selector_to_list(
                inner_selector,
                named_steps,
                values,
                kernel,
                table,
                realized_reprs,
                diagnostics,
            )? {
                reify_ir::Value::List(elems) => {
                    let idx = resolve_index_usize(index, values)?;
                    match elems.get(idx) {
                        Some(v) => Some(v.clone()),
                        None => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "selector index {idx} out of bounds (len {}); cell left at Undef",
                                elems.len()
                            )));
                            Some(reify_ir::Value::Undef)
                        }
                    }
                }
                // resolve_selector_to_list downgraded to Undef (kernel error) —
                // propagate so the cell is visibly degraded rather than skipped.
                other => Some(other),
            }
        }
        // `single(<selector>)` (task 4118 γ): the single()/list-helper coercion
        // site (compiler step-10) wraps the selector argument in a
        // `ResolveSelector`, so a `single(faces_by_normal(...))` cell compiles to
        // `FunctionCall { "single", [ResolveSelector{..}] }`. The pure eval path
        // cannot resolve the inner `ResolveSelector` (no kernel), so resolve it
        // HERE and unwrap the unique element — yielding the `Geometry` handle that
        // `single`'s `single(List<Geometry>) → Geometry` contract promises. This
        // is the runtime half of the single()/list-helper coercion (the golden
        // `top = single(faces_by_normal(b, +Z, 1deg))` shape).
        //
        // LOCKSTEP with the compiler: the set of coercing list-helpers is named
        // by `reify_compiler::coerce::COERCING_LIST_HELPERS` (currently just
        // `single`). This arm is the runtime counterpart that constant's doc
        // requires — it is intentionally hard-pinned to `"single"` (not the whole
        // set) because the unwrap-the-unique-element logic below is `single`'s
        // specific `single(List<Geometry>) → Geometry` semantics. If a new
        // coercing helper is ever added to `COERCING_LIST_HELPERS`, it needs its
        // OWN arm here implementing that helper's semantics (e.g. `first` → index
        // 0), not a widening of this `== "single"` guard.
        reify_ir::CompiledExprKind::FunctionCall { function, args }
            if function.name == "single" && args.len() == 1 =>
        {
            let selector_expr = match &args[0].kind {
                reify_ir::CompiledExprKind::ResolveSelector { selector } => selector.as_ref(),
                // Defensive: a bare selector FunctionCall (un-coerced) — still ours.
                reify_ir::CompiledExprKind::FunctionCall { .. } => &args[0],
                // Any other arg shape (a real List, a ValueRef to a List, …) is
                // owned by the pure eval_expr path — skip.
                _ => return None,
            };
            // Shared helper: unwrap a 1-element list, or push a Warning and return
            // Undef for any other cardinality. Extracted to avoid duplicating the
            // cardinality contract and warning message across the selector-value path
            // and the relational-selector fallback below — a single edit point for
            // future changes to either (#4873 amendment).
            let unwrap_single_list =
                |mut elems: Vec<reify_ir::Value>,
                 diags: &mut Vec<Diagnostic>|
                 -> Option<reify_ir::Value> {
                    if elems.len() == 1 {
                        Some(elems.remove(0))
                    } else {
                        diags.push(Diagnostic::warning(format!(
                            "single(...) expected exactly 1 element, got {}; cell left at Undef",
                            elems.len()
                        )));
                        Some(reify_ir::Value::Undef)
                    }
                };
            match resolve_selector_to_list(
                selector_expr,
                named_steps,
                values,
                kernel,
                table,
                realized_reprs,
                diagnostics,
            ) {
                Some(reify_ir::Value::List(elems)) => unwrap_single_list(elems, diagnostics),
                // resolve_selector_to_list downgraded to Undef (kernel error) —
                // propagate so the cell is visibly degraded rather than skipped.
                Some(other) => Some(other),
                // resolve_selector_to_list returned None: the arg is not a
                // Value::Selector (relational selectors — shared_edges, siblings_of_face,
                // ancestor_faces_of_edge, adjacent_faces — evaluate to Value::List via
                // try_eval_topology_selector, NOT Value::Selector, so
                // reconstruct_selector_value returns None). Fallback: resolve the
                // selector_expr via the topology-selector path and unwrap the unique
                // element (task #4873 — the deferred EDGE half of #4857).
                None => match try_eval_topology_selector(
                    selector_expr,
                    named_steps,
                    values,
                    kernel,
                    diagnostics,
                ) {
                    Some(reify_ir::Value::List(elems)) => unwrap_single_list(elems, diagnostics),
                    // Topology selector resolved to a non-List value or None —
                    // propagate unchanged (non-List) or fall through to pure eval (None).
                    other => other,
                },
            }
        }
        _ => None,
    }
}

/// Feature → datum projection member names (geometric-relations ε): the four
/// projections a realized feature's trait bundle carries. LOCKSTEP with the
/// compiler typing table (`datum_projection.rs` `Type::Geometry`/`Selector` arm)
/// — these are exactly the members `datum_projection_result_type` resolves for a
/// feature receiver. Used to gate [`try_eval_feature_datum_projection`] so it
/// only intercepts feature→datum projection `MethodCall`s, leaving β's pure
/// datum→datum projections (and any other method call) to the pure eval path.
const FEATURE_DATUM_PROJECTION_MEMBERS: [&str; 4] = ["axis", "plane", "point", "dir"];

/// `true` iff `expr` is a feature→datum projection `MethodCall` — the
/// geometric-relations ε `feature.axis`/`.plane`/`.point`/`.dir` family that
/// [`try_eval_feature_datum_projection`] resolves. These are, like
/// [`is_geometry_consumer_call`]'s builtins, resolvable ONLY via
/// `engine_build.rs`'s `post_process_feature_datum_projections` — the pure
/// `eval_datum_projection` (`reify-expr/src/lib.rs`) has no arm for a
/// `Value::GeometryHandle`/`Value::Selector` receiver, so a feature→datum
/// projection cell is `Value::Undef` after plain `eval()`/`eval_cached()` by
/// design (task γ / #4954 boundary row: giving the receiver a first-class
/// value cell means the checker's clause-4 "missing producer" exemption no
/// longer fires once that receiver is minted to a symbolic placeholder, so
/// this build()-only class needs its own clause-8 classifier entry — mirrors
/// `is_geometry_consumer_call`'s existing partition).
///
/// Gated on the RECEIVER's static type (`object.result_type`), mirroring the
/// compiler's own lowering decision (`datum_projection.rs`'s
/// `datum_projection_result_type` `Type::Geometry | Type::Selector(_) |
/// Type::AnySelector` arm) — NOT on method name alone: `dir` is ALSO a valid
/// β pure datum→datum projection member (`Axis.dir`), resolved entirely by
/// `eval_datum_projection` at plain eval time, so it must stay a plain
/// (non-build-only) dependency when its receiver is a datum, not a feature.
pub(crate) fn is_feature_datum_projection_call(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::MethodCall { object, method, args }
            if args.is_empty()
                && FEATURE_DATUM_PROJECTION_MEMBERS.contains(&method.as_str())
                && matches!(
                    object.result_type,
                    reify_core::Type::Geometry
                        | reify_core::Type::Selector(_)
                        | reify_core::Type::AnySelector
                )
    )
}

/// Kernel-backed evaluation of a feature → datum projection (`feature.axis` /
/// `.plane` / `.point` / `.dir`), geometric-relations ε (design §7.2). The
/// compiler lowers such a projection to a `MethodCall { object: <feature>,
/// method: <proj>, args: [] }` whose object is a realized `Value::GeometryHandle`
/// cell; the pure `eval_datum_projection` cannot evaluate it (it reaches the
/// kernel, the construction history, and the dedup primitive), so it is resolved
/// HERE, mirroring the `ResolveSelector` coercion in
/// [`try_eval_resolve_selector`].
///
/// Resolves the receiver to its feature handle, builds the deduplicated
/// [`feature_datum_bundle`](crate::feature_datum::feature_datum_bundle) from the
/// analytic ∪ construction-history union (the recovered [`SweptKind`] history is
/// looked up in `swept_kinds` by the feature handle), and refines it to the
/// requested projection via
/// [`feature_datum_projection`](crate::feature_datum::feature_datum_projection):
/// a unique datum is returned as its `Value`, a zero/many group emits a
/// select-a-subfeature [`DiagnosticCode::FeatureDatumAmbiguous`] error and yields
/// `Value::Undef`.
///
/// Returns `None` (skip — leave the cell for the pure eval path) when the expr is
/// not a feature→datum projection `MethodCall`, or when its receiver is a β
/// *datum* receiver (e.g. `axis.dir`, owned by `eval_datum_projection`) that does
/// not resolve to a realized `Value::GeometryHandle`.
///
/// A receiver that statically types as a topology *selector*
/// (`Type::Selector(_)` / `Type::AnySelector`) is accepted at compile time
/// (design §2.2 types a selection's feature→datum projection) but its
/// selector→sub-handle resolution is not yet wired on the eval side; rather than
/// leaving the cell a silent `Value::Undef`, it emits a select-a-subfeature
/// [`DiagnosticCode::FeatureDatumAmbiguous`] error and yields `Value::Undef`.
pub(crate) fn try_eval_feature_datum_projection(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    swept_kinds: &crate::sweep_classifier::SweptKindTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let (object, member) = match &expr.kind {
        reify_ir::CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } if args.is_empty() && FEATURE_DATUM_PROJECTION_MEMBERS.contains(&method.as_str()) => {
            (object.as_ref(), method.as_str())
        }
        _ => return None,
    };

    // Resolve the receiver to a realized feature handle. Only a feature receiver
    // backed by a realized `Value::GeometryHandle` cell is wired end-to-end; a β
    // datum receiver (`Axis`/…) does not resolve here, so we return None and the
    // pure `eval_datum_projection` path handles it.
    let handle = match resolve_selector_target(object, values) {
        Some(target) => target.kernel_handle,
        None => {
            // The receiver did not resolve to a realized geometry handle. Check
            // whether the receiver cell holds a hydrated `Value::Selector` (the
            // common post-hydration case where the topology-selector pass has
            // already written the cell before this feature-datum pass runs).
            if matches!(
                object.result_type,
                reify_core::ty::Type::Selector(_) | reify_core::ty::Type::AnySelector
            ) {
                // Try to read the hydrated Value::Selector from the values map.
                let maybe_sv = match &object.kind {
                    reify_ir::CompiledExprKind::ValueRef(id) => match values.get(id) {
                        Some(reify_ir::Value::Selector(sv)) => Some(sv.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(sv) = maybe_sv {
                    return Some(eval_selector_feature_datum(
                        &sv,
                        member,
                        kernel,
                        swept_kinds,
                        diagnostics,
                    ));
                }
                // No hydrated Value::Selector in the cell: static-type fallback —
                // emit an explicit select-a-subfeature diagnostic instead of
                // leaving the cell a silent `Value::Undef`.
                diagnostics.push(
                    Diagnostic::error(format!(
                        "feature→datum projection '.{member}' over a topology selector \
                         requires a resolved selector; select a single sub-feature \
                         (e.g. `single(...)`) or project from the realized feature instead"
                    ))
                    .with_code(reify_core::DiagnosticCode::FeatureDatumAmbiguous),
                );
                return Some(reify_ir::Value::Undef);
            }
            // Not a selector receiver — β datum such as `axis.dir`, or a
            // not-yet-hydrated cell. Return None and let the pure
            // `eval_datum_projection` path own it.
            return None;
        }
    };

    let Some(handle_id) = handle else {
        return Some(reify_ir::Value::Undef);
    };
    let history = swept_kinds.lookup(handle_id);
    let bundle = crate::feature_datum::feature_datum_bundle(handle_id, kernel, history);
    Some(crate::feature_datum::feature_datum_projection(
        &bundle,
        member,
        diagnostics,
    ))
}

/// Resolve a hydrated `Value::Selector` to its sub-handle ids via
/// [`crate::topology_selectors::resolve`], build a per-handle
/// [`crate::feature_datum::FeatureDatumBundle`] for each, union the four groups
/// across all handles, re-dedup the union at the confusion-floor tolerance
/// (so coaxial/coplanar/coincident datums from different sub-handles collapse to
/// one), and finally project via [`crate::feature_datum::feature_datum_projection`]
/// — the same select-one-or-diagnose refinement the `GeometryHandle` arm uses.
///
/// On `topology_selectors::resolve` returning `Err`, pushes a `Severity::Warning`
/// and returns `Value::Undef` (mirroring `try_eval_resolve_selector` @3471-3476).
///
/// Called from `try_eval_feature_datum_projection` in the `None` branch of
/// `resolve_selector_target` when a hydrated `Value::Selector` cell is present.
fn eval_selector_feature_datum(
    sv: &reify_ir::value::SelectorValue,
    member: &str,
    kernel: &mut dyn reify_ir::GeometryKernel,
    swept_kinds: &crate::sweep_classifier::SweptKindTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    // (a) Resolve the selector to a list of sub-handle ids.
    let ids = match crate::topology_selectors::resolve(sv, kernel, diagnostics) {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "feature→datum projection over selector: kernel error resolving selector: \
                 {err}; cell left at Undef"
            )));
            return reify_ir::Value::Undef;
        }
    };

    // (b) Union per-handle FeatureDatumBundles into one combined bundle.
    let mut combined = crate::feature_datum::FeatureDatumBundle::default();
    for id in ids {
        let b = crate::feature_datum::feature_datum_bundle(id, kernel, swept_kinds.lookup(id));
        combined.axes.extend(b.axes);
        combined.planes.extend(b.planes);
        combined.points.extend(b.points);
        combined.directions.extend(b.directions);
    }

    // (c) Re-dedup each group at the confusion-floor tolerance so N coaxial /
    // coplanar / coincident sub-handle datums collapse to one.
    //
    // V1 DESIGN NOTE — floor-only tolerance (deliberate):
    // `dedup_tolerance(0.0, 0.0)` uses the geometric confusion floor with no
    // per-sub-shape local modelling tolerance added.  The `Datum` carrier that
    // flows through `feature_datum_bundle` does not retain each sub-shape's
    // local lin_tol, so we cannot fold per-handle tolerances here without a
    // Datum API change.  For clean analytic primitives (all local tols at the
    // floor) this is equivalent to `max(local_tols)`, making it correct for the
    // v1 target.  A coarse/imprecise sub-shape whose local tol exceeds the
    // floor could in theory yield a spurious FeatureDatumAmbiguous where the
    // single-GeometryHandle arm would merge; that narrowing is accepted as a v1
    // limitation and documented here so future readers do not mistake it for a
    // bug.  Threading per-handle lin_tol into the cross-handle re-dedup (e.g.
    // fold the max of per-handle bundle lin_tols) would fix it at the cost of
    // a `FeatureDatumBundle::lin_tol` field — left to a follow-up if coarse
    // models are encountered in practice.
    let tol = crate::feature_datum::dedup_tolerance(0.0, 0.0);
    combined.axes = crate::feature_datum::dedup_datums(combined.axes, tol);
    combined.planes = crate::feature_datum::dedup_datums(combined.planes, tol);
    combined.points = crate::feature_datum::dedup_datums(combined.points, tol);
    combined.directions = crate::feature_datum::dedup_datums(combined.directions, tol);

    // (d) Project: unique → datum Value; zero/many → FeatureDatumAmbiguous + Undef.
    crate::feature_datum::feature_datum_projection(&combined, member, diagnostics)
}

// ── geometric-relations η: intrinsic `self`-datum projections (task 4387) ─────

/// Intrinsic `self`-datum projection member names (geometric-relations η). A
/// structure's `self` is its own identity frame (design §6), so these project
/// the structure's intrinsic identity-frame constants. LOCKSTEP with the
/// compiler typing table (`datum_projection.rs` `Type::StructureRef(_)` arm) and
/// the `expr.rs` self-datum lowering — exactly the members
/// `datum_projection_result_type` resolves on a `StructureRef` receiver.
const SELF_DATUM_PROJECTION_MEMBERS: [&str; 8] = [
    "origin", "frame", "x", "y", "z", "xy_plane", "yz_plane", "zx_plane",
];

/// The world origin as a length-dimensioned `Point3` (all components 0 m).
fn self_datum_origin_point() -> reify_ir::Value {
    reify_ir::Value::Point(vec![
        reify_ir::Value::length(0.0),
        reify_ir::Value::length(0.0),
        reify_ir::Value::length(0.0),
    ])
}

/// A dimensionless 3-vector plane normal — matches `frame_xy_plane`'s
/// `Vector([Real; 3])` plane-normal convention in reify-expr.
fn self_datum_normal(x: f64, y: f64, z: f64) -> reify_ir::Value {
    reify_ir::Value::Vector(vec![
        reify_ir::Value::Real(x),
        reify_ir::Value::Real(y),
        reify_ir::Value::Real(z),
    ])
}

/// Kernel-free evaluation of an intrinsic `self`-datum projection
/// (`self.origin` / `.frame` / `.x` / `.y` / `.z` / `.xy_plane` / `.yz_plane` /
/// `.zx_plane`), geometric-relations η (design §6).
///
/// The compiler (η step-8) lowers `self.<datum>` to a
/// `MethodCall { object: ValueRef(__self : StructureRef), method, args: [] }`.
/// There is no `__self` value cell at eval time (mirroring the structural-query
/// `self.children`/`self.members` path — see [`crate::structural_query`]), so
/// the pure `eval_expr` short-circuits the `__self` receiver to `Undef`. This
/// intercepts such a node by its STATIC shape — a `StructureRef`-typed receiver
/// plus a self-datum member name — and returns the structure's intrinsic
/// identity-frame constant:
///   - `origin`   → `Point3<Length>` at the world origin
///   - `frame`    → identity `Frame` (world origin, identity quaternion)
///   - `x`/`y`/`z`→ unit-axis `Direction`s
///   - `*_plane`  → principal `Plane`s through the origin (axis normals)
///
/// The `StructureRef` receiver gate distinguishes self-datums from β's datum→
/// datum projections (`axis.origin`, `frame.x`, …), whose receivers are
/// `Axis`/`Frame`/… typed and which the pure `eval_datum_projection` owns.
///
/// Returns `None` for any node that is not a self-datum projection.
pub(crate) fn try_eval_self_datum_projection(
    expr: &reify_ir::CompiledExpr,
) -> Option<reify_ir::Value> {
    let (object, member) = match &expr.kind {
        reify_ir::CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } if args.is_empty() && SELF_DATUM_PROJECTION_MEMBERS.contains(&method.as_str()) => {
            (object.as_ref(), method.as_str())
        }
        _ => return None,
    };
    if !matches!(object.result_type, reify_core::ty::Type::StructureRef(_)) {
        return None;
    }
    Some(match member {
        "origin" => self_datum_origin_point(),
        "frame" => reify_ir::Value::Frame {
            origin: Box::new(self_datum_origin_point()),
            basis: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
        },
        "x" => reify_ir::Value::Direction {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        "y" => reify_ir::Value::Direction {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        },
        "z" => reify_ir::Value::Direction {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
        "xy_plane" => reify_ir::Value::Plane {
            origin: Box::new(self_datum_origin_point()),
            normal: Box::new(self_datum_normal(0.0, 0.0, 1.0)),
        },
        "yz_plane" => reify_ir::Value::Plane {
            origin: Box::new(self_datum_origin_point()),
            normal: Box::new(self_datum_normal(1.0, 0.0, 0.0)),
        },
        "zx_plane" => reify_ir::Value::Plane {
            origin: Box::new(self_datum_origin_point()),
            normal: Box::new(self_datum_normal(0.0, 1.0, 0.0)),
        },
        // SELF_DATUM_PROJECTION_MEMBERS is the exhaustive gate above; defensive.
        _ => return None,
    })
}

/// Returns `true` if `expr` contains any intrinsic `self`-datum projection node.
///
/// Gates the clone+rewrite+re-eval pass so cells without self-datums are not
/// re-evaluated. Uses the canonical [`reify_ir::CompiledExpr::walk`] traversal
/// (visits self + every sub-expression), so it needs no hand-rolled recursion.
pub(crate) fn contains_self_datum_projection(expr: &reify_ir::CompiledExpr) -> bool {
    let mut found = false;
    expr.walk(&mut |e| {
        if try_eval_self_datum_projection(e).is_some() {
            found = true;
        }
    });
    found
}

/// Rewrite every intrinsic `self`-datum projection node within `expr` in-place
/// to a [`reify_ir::CompiledExpr::literal`] holding its intrinsic constant
/// (preserving the projection's `result_type`), so the pure `eval_expr`
/// evaluates the containing cell — including NESTED operands such as
/// `midplane(self.xy_plane, self.zx_plane)`.
///
/// Mirrors [`crate::structural_query`]'s `expand_structural_query` `&mut`
/// tree-walk (there is no generic `walk_mut`): each `MethodCall` on a
/// `StructureRef` receiver naming a self-datum is replaced by its constant,
/// otherwise the walk recurses into sub-expressions.
pub(crate) fn rewrite_self_datum_projections(expr: &mut reify_ir::CompiledExpr) {
    use reify_ir::CompiledExprKind as K;
    // Replace this node if it is itself a self-datum projection.
    if let Some(value) = try_eval_self_datum_projection(expr) {
        *expr = reify_ir::CompiledExpr::literal(value, expr.result_type.clone());
        return;
    }
    match &mut expr.kind {
        K::Literal(_)
        | K::ValueRef(_)
        | K::CrossSubGeometryRef(_)
        | K::OptionNone
        | K::MetaAccess { .. }
        | K::DeterminacyPredicate { .. }
        | K::PurposeReflectiveAggregation { .. } => {}
        K::BinOp { left, right, .. } => {
            rewrite_self_datum_projections(left);
            rewrite_self_datum_projections(right);
        }
        K::UnOp { operand, .. } => rewrite_self_datum_projections(operand),
        K::FunctionCall { args, .. } | K::UserFunctionCall { args, .. } => {
            for arg in args {
                rewrite_self_datum_projections(arg);
            }
        }
        K::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_self_datum_projections(condition);
            rewrite_self_datum_projections(then_branch);
            rewrite_self_datum_projections(else_branch);
        }
        K::Match { discriminant, arms } => {
            rewrite_self_datum_projections(discriminant);
            for arm in arms {
                rewrite_self_datum_projections(&mut arm.body);
            }
        }
        K::Lambda { body, .. } => rewrite_self_datum_projections(body),
        K::ListLiteral(elements)
        | K::SetLiteral(elements)
        | K::ReflectiveCellList(elements) => {
            for elem in elements {
                rewrite_self_datum_projections(elem);
            }
        }
        K::MapLiteral(entries) => {
            for (key, val) in entries {
                rewrite_self_datum_projections(key);
                rewrite_self_datum_projections(val);
            }
        }
        K::IndexAccess { object, index } => {
            rewrite_self_datum_projections(object);
            rewrite_self_datum_projections(index);
        }
        K::MethodCall { object, args, .. } => {
            rewrite_self_datum_projections(object);
            for arg in args {
                rewrite_self_datum_projections(arg);
            }
        }
        K::Quantifier {
            collection,
            predicate,
            ..
        } => {
            rewrite_self_datum_projections(collection);
            rewrite_self_datum_projections(predicate);
        }
        K::OptionSome(inner) => rewrite_self_datum_projections(inner),
        K::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                rewrite_self_datum_projections(lo);
            }
            if let Some(hi) = upper {
                rewrite_self_datum_projections(hi);
            }
        }
        K::AdHocSelector { base, args, .. } => {
            rewrite_self_datum_projections(base);
            for arg in args {
                rewrite_self_datum_projections(arg);
            }
        }
        K::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, arg) in ordered_args {
                rewrite_self_datum_projections(arg);
            }
            for (_, def) in defaults {
                rewrite_self_datum_projections(def);
            }
        }
        K::ResolveSelector { selector } => rewrite_self_datum_projections(selector),
    }
}

/// Reconstruct a `SelectorValue` from a single compiled arg expression.
///
/// PREFERRED path: inline reconstruction from a nested selector FunctionCall
/// (no value-cell ordering dependency) via a recursive `try_eval_topology_selector`
/// call.  Fallback: a `ValueRef` pointing to an already-patched `Value::Selector`
/// cell in the `values` map.
///
/// Returns `None` for any other expr shape (the cell is not yet hydrated or does
/// not represent a selector) — the composition arm then returns `None`, leaving
/// the cell at `Value::Undef` for a subsequent pass.
///
/// Factored from the step-1 inline reconstruction in `resolve_selector_to_list`
/// (task 4119 δ, step-6) so the composition arms in `try_eval_topology_selector`
/// can reuse the same logic.
fn reconstruct_selector_value(
    arg: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::value::SelectorValue> {
    match &arg.kind {
        reify_ir::CompiledExprKind::FunctionCall { .. } => {
            match try_eval_topology_selector(arg, named_steps, values, kernel, diagnostics)? {
                reify_ir::Value::Selector(sv) => Some(sv),
                // FunctionCall resolved to a non-selector (e.g. adjacent_faces
                // → List, or Undef) — not ours to wrap.
                _ => None,
            }
        }
        reify_ir::CompiledExprKind::ValueRef(id) => match values.get(id) {
            Some(reify_ir::Value::Selector(sv)) => Some(sv.clone()),
            // Cell not yet patched to a selector / not a selector — skip.
            _ => None,
        },
        _ => None,
    }
}

/// Build a variadic selector composition (`union` or `intersect`) from a slice of
/// compiled args by reconstructing each child `SelectorValue` then calling the
/// provided constructor.  Parameterised by `constructor` so Union and Intersect
/// share the same collect+construct+error path and only differ in the fn they pass.
///
/// Returns `None` if any child cannot be reconstructed (cell stays Undef).
/// Returns `Some(Value::Undef)` + a Warning on `SelectorError` (defensive backstop
/// — compile-time `E_SELECTOR_KIND_MISMATCH` should have fired first).
fn eval_variadic_composition(
    op_name: &str,
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
    constructor: fn(
        Vec<reify_ir::value::SelectorValue>,
    ) -> Result<reify_ir::value::SelectorValue, reify_ir::value::SelectorError>,
) -> Option<reify_ir::Value> {
    let children: Vec<reify_ir::value::SelectorValue> = args
        .iter()
        .map(|arg| reconstruct_selector_value(arg, named_steps, values, kernel, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    match constructor(children) {
        Ok(sv) => Some(reify_ir::Value::Selector(sv)),
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{op_name}: selector kind-closure violation ({err:?}); cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Resolve the target [`reify_ir::value::GeometryHandleRef`] for the named-leaf
/// selector ctors (`face`, `edge`, `solid_body`, `vertex`), accepting EITHER a
/// realized `Value::GeometryHandle` OR a hydrated `Value::Selector` first arg
/// (task #4583).
///
/// - **Primary path**: [`resolve_selector_target`] — realized
///   `Value::GeometryHandle` cell; the pre-4583 path, kept primary.
/// - **Fallback path**: when `resolve_selector_target` returns `None`, attempt
///   [`reconstruct_selector_value`] + [`first_leaf_target`]`.cloned()` to extract
///   the input selector's parent-geometry target. This lets
///   `face(mid_surface(body), "region_0")` root its Named leaf at `body`'s GHR.
///
/// A multi-leaf union/intersect first arg narrows to [`first_leaf_target`] — a
/// documented v1 limitation; the single-leaf `mid_surface` consumer is the
/// in-scope case. Preserves the typed `Value::Selector` model with zero kernel
/// queries at construction (K2/BT7): the fallback never eagerly resolves handles.
///
/// Does NOT widen the shared [`resolve_selector_target`] (which feature-datum
/// projection + `AdjacentFaces`/`SharedEdges` require to return a realized handle).
fn resolve_named_leaf_target(
    arg: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::value::GeometryHandleRef> {
    // Primary: realized Value::GeometryHandle (pre-4583 path, kept primary).
    if let Some(ghr) = resolve_selector_target(arg, values) {
        return Some(ghr);
    }
    // Fallback: hydrated Value::Selector — extract the parent-geometry target
    // via first_leaf_target (left-most leaf walk), keeping the result a GHR
    // rather than an eager handle list (K2/BT7; preserves typed Selector model).
    let sv = reconstruct_selector_value(arg, named_steps, values, kernel, diagnostics)?;
    first_leaf_target(&sv).cloned()
}

/// Build a named-leaf selector (`face`, `edge`, `solid_body`, or `vertex`) from two
/// compiled args: `args[0]` is the geometry target (resolved via
/// [`resolve_named_leaf_target`], which accepts either a realized
/// `Value::GeometryHandle` or a hydrated `Value::Selector` — task #4583) and
/// `args[1]` is the tag string (extracted via [`resolve_string_literal_arg`]).
/// Parameterised by `kind` so all four named-leaf ctors share the same path.
fn eval_named_leaf_selector_ctor(
    kind: reify_core::ty::SelectorKind,
    args: &[reify_ir::CompiledExpr],
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    function_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let target =
        resolve_named_leaf_target(&args[0], named_steps, values, kernel, diagnostics)?;
    // Evaluate-then-accept (task ε): the named-leaf ctor threads the real
    // `values`, so the tag arg now resolves a `ValueRef → String` cell
    // (`face(body, label_var)`) in addition to an inline string literal; a
    // defined-but-wrong tag emits a Warning instead of falling through silently.
    let name = resolve_string_literal_arg(&args[1], values, function_name, "name", diagnostics)?;
    build_leaf_selector(
        kind,
        target,
        reify_ir::value::LeafQuery::Named(name),
        function_name,
        diagnostics,
    )
}

/// Reconstruct the `Value::Selector` denoted by `selector_expr`, resolve it via
/// `topology_selectors::resolve`, and wrap the canonical-order handle ids as a
/// `Value::List` of `Value::GeometryHandle` sub-handles. Shared by the
/// `ResolveSelector` and `IndexAccess`-over-selector arms of
/// [`try_eval_resolve_selector`].
///
/// Returns `None` when the inner expr is not a selector we can reconstruct (so
/// the caller skips the cell); `Some(Value::Undef)` + a Warning when `resolve()`
/// fails at the kernel.
///
/// NOTE (sub-handle indexing): the resolved ids are enumerated by FILTERED
/// position, so a predicate leaf's `[i]` does not preserve the parent's canonical
/// TopExp index. For the call-site-transparent shapes in scope here — `All`-leaf
/// indexing (`faces(b)[i]`, filtered == canonical) and single-element
/// `single(predicate(...))` — filtered position equals the intended element.
/// Canonical-index recovery for multi-element predicate `[i]` is a follow-up.
pub(crate) fn resolve_selector_to_list(
    selector_expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    table: &reify_ir::TopologyAttributeTable,
    realized_reprs: &HashMap<reify_core::identity::RealizationNodeId, reify_ir::ReprKind>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Obtain the Value::Selector via the shared helper (task 4119 δ, step-6).
    let sv = reconstruct_selector_value(selector_expr, named_steps, values, kernel, diagnostics)?;

    // (2) Parent identity for sub-handle hashing: the (first) leaf target.
    let target = first_leaf_target(&sv)?;
    let sub_kind = match sv.kind {
        reify_core::ty::SelectorKind::Face => crate::topology_selectors::SubKind::Face,
        reify_core::ty::SelectorKind::Edge => crate::topology_selectors::SubKind::Edge,
        reify_core::ty::SelectorKind::Body => crate::topology_selectors::SubKind::Solid,
        // Vertex sub-handles use the 0x04 domain byte (task 4368).
        reify_core::ty::SelectorKind::Vertex => crate::topology_selectors::SubKind::Vertex,
    };
    let parent_rr = target.realization_ref.clone();
    let parent_hash = target.upstream_values_hash;

    // (2b) Fail-closed capability gate (task #4812, P0β): if the selector has a
    // gated capability (region_query_capability returns Some) AND the body's
    // realized repr is known (present in realized_reprs), route through
    // route_capability. On Unsupported the gate already pushed a structured
    // QueryNotSupportedOnRepr Error — return Undef immediately without calling
    // the kernel. Named selectors return None from region_query_capability and
    // are un-gated (PRD §7). Unknown repr (absent from map) skips the gate
    // (fail-open: preserves today's behavior for symbolic/unrealized handles).
    //
    // P0β scope note (composite selectors): the gate keys on the FIRST-LEAF
    // target only (via `first_leaf_target` above, mirrored by
    // `region_query_capability`'s left-most walk). A Union/Intersect over
    // operands realized as different reprs is therefore gated on the first
    // leaf's repr alone — later operands with a non-supporting repr (e.g.
    // Sdf) would pass the gate and reach the kernel, where they fall back to
    // today's generic-error path. Full per-leaf gating for composite
    // mixed-repr selectors is out of scope for P0β (task #4812); the
    // foundational single-leaf signal covers the common single-body case.
    if let Some(cap) = crate::topology_selectors::region_query_capability(&sv)
        && let Some(&repr) = realized_reprs.get(&target.realization_ref)
    {
        let display = crate::topology_selectors::region_selector_display_name(&sv);
        if route_capability(cap, repr, &display, diagnostics) == CapabilityRoute::Unsupported {
            return Some(reify_ir::Value::Undef);
        }
    }

    // (3) Resolve via the single executor — the kernel-bearing query happens HERE,
    // not at construction (K2/BT7). `resolve_with_attributes` is the
    // table-threaded twin of `resolve` (task 4536): a `ByRole` leaf (e.g.
    // `mid_surface(body)`) filters the realized body's `TopologyAttributeTable`,
    // while every other leaf/composite behaves exactly as `resolve`.
    match crate::topology_selectors::resolve_with_attributes(&sv, kernel, table, diagnostics) {
        Ok(ids) => {
            // task 4536: an attribute-role leaf (e.g. `mid_surface(body)`) that
            // matched NO entities means NO body in this design recorded that role
            // — the threaded table is build-global, so this is a per-DESIGN, not
            // a per-body, statement (see the SCOPE note on the `ByRole` arm in
            // topology_selectors.rs). The contract is `Value::Undef` + a
            // diagnostic in that case, NOT a silent empty list. Generic empty
            // selections (a `faces_by_area` window with no match, a ByRole leaf
            // nested in a 4119 composite, …) keep returning an empty
            // `Value::List`.
            if ids.is_empty()
                && let Some(role) = selector_is_attribute_role_leaf(&sv)
            {
                // Role-GENERIC wording: phrased in terms of the matched `role`
                // (not a hardcoded "mid-surface"), because
                // `selector_is_attribute_role_leaf` admits ANY ByRole leaf, and
                // as a per-DESIGN claim ("no body in this design"), because the
                // build-global table spans every body in the build.
                diagnostics.push(Diagnostic::warning(format!(
                    "topology-attribute selector matched no entities with role \
                     {role:?}; no body in this design carries a {role:?} \
                     attribute; result undefined"
                )));
                return Some(reify_ir::Value::Undef);
            }
            // Task 4831 (P3β / PRD §3 D3 sub-case b): a provenance leaf
            // (`created_by_feature`/`split_by_feature`) that matched NO faces
            // means no recorded construction history names the queried
            // feature (e.g. imported geometry, or a feature that created/
            // split nothing) — same "never a silent empty" contract as the
            // ByRole branch above, sibling predicate since provenance leaves
            // carry a FeatureId, not a Role. Non-BRep reprs are already
            // fail-closed upstream by `region_query_capability` => BRepOnly
            // (step-4) via the `route_capability` gate above, so the two
            // branches are mutually exclusive — exactly one diagnostic fires.
            if ids.is_empty()
                && let Some(fid) = selector_is_provenance_leaf(&sv)
            {
                diagnostics.push(Diagnostic::warning(format!(
                    "feature-provenance selector matched no faces for feature \
                     {fid}; result undefined"
                )));
                return Some(reify_ir::Value::Undef);
            }
            let elements = ids
                .into_iter()
                .enumerate()
                .map(|(i, id)| {
                    crate::topology_selectors::make_sub_handle(
                        &parent_rr,
                        &parent_hash,
                        sub_kind,
                        i as u32,
                        id,
                    )
                })
                .collect();
            Some(reify_ir::Value::List(elements))
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "resolve_selector: kernel error resolving selector: {err}; cell left at Undef"
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// First `Leaf` target reached by a left-most walk of the selector tree — the
/// parent solid handle used for sub-handle identity. The 7 re-typed constructors
/// only build `Leaf` nodes; composites walk to their first child for robustness.
fn first_leaf_target(
    sv: &reify_ir::value::SelectorValue,
) -> Option<&reify_ir::value::GeometryHandleRef> {
    fn walk(node: &reify_ir::value::SelectorNode) -> Option<&reify_ir::value::GeometryHandleRef> {
        match node {
            reify_ir::value::SelectorNode::Leaf { target, .. } => Some(target),
            reify_ir::value::SelectorNode::Union(children)
            | reify_ir::value::SelectorNode::Intersect(children) => {
                children.first().and_then(|c| walk(&c.node))
            }
            reify_ir::value::SelectorNode::Difference(a, _) => walk(&a.node),
        }
    }
    walk(&sv.node)
}

/// Returns `Some(role)` iff `sv` is a single `ByRole(role)` leaf — i.e. a
/// `mid_surface(body)`-style attribute-role selector (task 4536).
///
/// Used by [`resolve_selector_to_list`] to distinguish a genuinely-empty role
/// match (no body in this design carries the matched role → the contract is
/// `Value::Undef` + a diagnostic) from a generic empty selection (e.g. a
/// `faces_by_area` window matching nothing → an empty `Value::List`). Composite
/// selectors (`Union`/`Intersect`/`Difference`) and every other leaf query
/// return `None`, so a ByRole leaf nested inside a 4119 composition still
/// follows the generic empty-list path rather than collapsing the whole
/// composition to `Undef`.
///
/// Role-GENERIC: returns whatever [`reify_ir::Role`] the leaf carries, not just
/// `MidSurfaceFace`. The empty→`Undef` contract it gates is per-DESIGN (the
/// `ByRole` resolution table is build-global), NOT per-body — see the SCOPE
/// note on the `ByRole` arm in `topology_selectors.rs`.
fn selector_is_attribute_role_leaf(sv: &reify_ir::value::SelectorValue) -> Option<reify_ir::Role> {
    match &sv.node {
        reify_ir::value::SelectorNode::Leaf {
            query: reify_ir::value::LeafQuery::ByRole(role),
            ..
        } => Some(*role),
        _ => None,
    }
}

/// Returns `Some(&fid)` iff `sv` is a single `CreatedByFeature(fid)` or
/// `SplitByFeature(fid)` leaf (task 4831, P3β / PRD §3 D3 sub-case b).
///
/// Sibling to [`selector_is_attribute_role_leaf`]: provenance leaves carry a
/// [`reify_ir::FeatureId`], not a [`reify_ir::Role`], so they need their own
/// predicate to gate the same "never a silent empty" empty→`Undef` contract
/// in [`resolve_selector_to_list`]. Composite selectors
/// (`Union`/`Intersect`/`Difference`) and every other leaf query return
/// `None`, so a provenance leaf nested inside a 4119 composition still
/// follows the generic empty-list path rather than collapsing the whole
/// composition to `Undef` — same non-collapsing discipline as the ByRole
/// sibling.
fn selector_is_provenance_leaf(
    sv: &reify_ir::value::SelectorValue,
) -> Option<&reify_ir::FeatureId> {
    match &sv.node {
        reify_ir::value::SelectorNode::Leaf {
            query:
                reify_ir::value::LeafQuery::CreatedByFeature(fid)
                | reify_ir::value::LeafQuery::SplitByFeature(fid),
            ..
        } => Some(fid),
        _ => None,
    }
}

/// Resolve an `IndexAccess` index expr to a `usize`. Accepts an `Int` literal or
/// a `ValueRef` to an `Int` cell; returns `None` for anything else or a negative
/// index (the caller then leaves the cell untouched).
fn resolve_index_usize(
    index: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<usize> {
    let v = match &index.kind {
        reify_ir::CompiledExprKind::Literal(v) => v,
        reify_ir::CompiledExprKind::ValueRef(id) => values.get(id)?,
        _ => return None,
    };
    match v {
        reify_ir::Value::Int(i) if *i >= 0 => Some(*i as usize),
        _ => None,
    }
}

pub(crate) fn try_eval_topology_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    values: &reify_ir::ValueMap,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Must be a FunctionCall — anything else is unsupported.
    let (function, args) = match &expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be one of the recognised helper names.
    let helper = match function.name.as_str() {
        "closest_point" => TopologySelectorHelper::ClosestPoint,
        "is_on" => TopologySelectorHelper::IsOn,
        "angle_between_surfaces" => TopologySelectorHelper::AngleBetweenSurfaces,
        "edges" => TopologySelectorHelper::Edges,
        "faces" => TopologySelectorHelper::Faces,
        // task 4536 — role-addressed mid-surface leaf ctor
        "mid_surface" => TopologySelectorHelper::MidSurface,
        "center_of_mass" => TopologySelectorHelper::CenterOfMass,
        "moment_of_inertia" => TopologySelectorHelper::MomentOfInertia,
        "edges_by_length" => TopologySelectorHelper::EdgesByLength,
        "faces_by_area" => TopologySelectorHelper::FacesByArea,
        "faces_by_normal" => TopologySelectorHelper::FacesByNormal,
        "edges_parallel_to" => TopologySelectorHelper::EdgesParallelTo,
        "edges_at_height" => TopologySelectorHelper::EdgesAtHeight,
        "adjacent_faces" => TopologySelectorHelper::AdjacentFaces,
        "shared_edges" => TopologySelectorHelper::SharedEdges,
        // task #4759 — relational-walk v2 selectors
        "siblings_of_face" => TopologySelectorHelper::SiblingsOfFace,
        "ancestor_faces_of_edge" => TopologySelectorHelper::AncestorFacesOfEdge,
        "angle" => TopologySelectorHelper::Angle,
        "contains" => TopologySelectorHelper::Contains,
        "geo_equiv" => TopologySelectorHelper::GeoEquiv,
        "normal" => TopologySelectorHelper::Normal,
        "curvature" => TopologySelectorHelper::Curvature,
        "length" => TopologySelectorHelper::Length,
        "perimeter" => TopologySelectorHelper::Perimeter,
        "distance" => TopologySelectorHelper::Distance,
        "intersects" => TopologySelectorHelper::Intersects,
        "split" => TopologySelectorHelper::Split,
        // task 4119 δ — selector-composition algebra
        "union" => TopologySelectorHelper::Union,
        "intersect" => TopologySelectorHelper::Intersect,
        "difference" => TopologySelectorHelper::Difference,
        // task 4119 δ — Named-leaf constructors (PRD §11.1)
        "face" => TopologySelectorHelper::Face,
        "edge" => TopologySelectorHelper::Edge,
        "solid_body" => TopologySelectorHelper::SolidBody,
        // task 4368 — 0-D vertex selector ctors
        "vertices" => TopologySelectorHelper::Vertices,
        "vertex" => TopologySelectorHelper::Vertex,
        // task 3523: selector_vocabulary_v2 leaf-predicate ctors (kernel-free).
        "faces_perpendicular_to" => TopologySelectorHelper::FacesPerpendicularTo,
        "edges_perpendicular_to" => TopologySelectorHelper::EdgesPerpendicularTo,
        "faces_by_surface_kind" => TopologySelectorHelper::FacesBySurfaceKind,
        "edges_by_curve_kind" => TopologySelectorHelper::EdgesByCurveKind,
        "extremal_by_bbox" => TopologySelectorHelper::ExtremalByBbox,
        "extremal_by_centroid" => TopologySelectorHelper::ExtremalByCentroid,
        // task 4831 (P3β): feature-provenance leaf ctors (kernel-free).
        "created_by_feature" => TopologySelectorHelper::CreatedByFeature,
        "split_by_feature" => TopologySelectorHelper::SplitByFeature,
        _ => return None,
    };

    // (3) Per-helper arity check. Each new selector in task 3560 carries its
    // own arity contract; the legacy 2-arg trio (closest_point, is_on,
    // angle_between_surfaces) shares the arity-2 branch.
    // task 4119 δ: union/intersect are variadic (≥ 2); difference is binary (== 2).
    match helper {
        TopologySelectorHelper::Union | TopologySelectorHelper::Intersect => {
            if args.len() < 2 {
                return None;
            }
        }
        _ => {
            let expected_arity = helper.expected_arity();
            if args.len() != expected_arity {
                return None;
            }
        }
    }

    match helper {
        TopologySelectorHelper::ClosestPoint | TopologySelectorHelper::IsOn => {
            // args[0]: point ValueRef → values map → Value::Point of three Length scalars.
            let point =
                resolve_point3_length_arg(&args[0], values, &function.name, "point", diagnostics)?;
            // args[1]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[1], named_steps)?;

            match helper {
                TopologySelectorHelper::ClosestPoint => {
                    let query = reify_ir::GeometryQuery::ClosestPointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                    };
                    dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics)
                }
                TopologySelectorHelper::IsOn => {
                    // Use `reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (= OCCT's
                    // `Precision::Confusion()`, ~1e-7) as the default tolerance for the
                    // v0.1 2-arg `is_on(point, geometry)` form.  The constant is the
                    // single source of truth shared between this dispatcher and
                    // `OcctKernel::point_on_shape`
                    // (`crates/reify-kernel-occt/src/lib.rs`).  A future explicit-
                    // tolerance overload `is_on(point, geometry, tol)` will plumb the
                    // user-supplied tolerance through here.
                    let query = reify_ir::GeometryQuery::PointOnShape {
                        handle,
                        px: point[0],
                        py: point[1],
                        pz: point[2],
                        tolerance: reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
                    };
                    dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
                }
                // Enumerate the complement explicitly (rather than `_`) so that
                // adding a new `TopologySelectorHelper` variant and grouping it
                // into the outer `ClosestPoint | IsOn` or-pattern forces the
                // compiler to error here instead of silently funnelling into
                // `unreachable!()`.
                TopologySelectorHelper::AngleBetweenSurfaces
                | TopologySelectorHelper::Edges
                | TopologySelectorHelper::Faces
                | TopologySelectorHelper::MidSurface
                | TopologySelectorHelper::CenterOfMass
                | TopologySelectorHelper::MomentOfInertia
                | TopologySelectorHelper::EdgesByLength
                | TopologySelectorHelper::FacesByArea
                | TopologySelectorHelper::FacesByNormal
                | TopologySelectorHelper::EdgesParallelTo
                | TopologySelectorHelper::EdgesAtHeight
                | TopologySelectorHelper::AdjacentFaces
                | TopologySelectorHelper::SharedEdges
                // task #4759 — relational-walk v2 selectors
                | TopologySelectorHelper::SiblingsOfFace
                | TopologySelectorHelper::AncestorFacesOfEdge
                | TopologySelectorHelper::Angle
                | TopologySelectorHelper::Contains
                | TopologySelectorHelper::GeoEquiv
                | TopologySelectorHelper::Normal
                | TopologySelectorHelper::Curvature
                | TopologySelectorHelper::Length
                | TopologySelectorHelper::Perimeter
                | TopologySelectorHelper::Distance
                | TopologySelectorHelper::Intersects
                | TopologySelectorHelper::Split
                // task 4119 δ — composition + Named-leaf ctors
                | TopologySelectorHelper::Union
                | TopologySelectorHelper::Intersect
                | TopologySelectorHelper::Difference
                | TopologySelectorHelper::Face
                | TopologySelectorHelper::Edge
                | TopologySelectorHelper::SolidBody
                // task 4368 — 0-D vertex selector ctors
                | TopologySelectorHelper::Vertices
                | TopologySelectorHelper::Vertex
                // task 3523 — selector_vocabulary_v2 leaf-predicate ctors
                | TopologySelectorHelper::FacesPerpendicularTo
                | TopologySelectorHelper::EdgesPerpendicularTo
                | TopologySelectorHelper::FacesBySurfaceKind
                | TopologySelectorHelper::EdgesByCurveKind
                | TopologySelectorHelper::ExtremalByBbox
                | TopologySelectorHelper::ExtremalByCentroid
                // task 4831 (P3β) — provenance leaf ctors
                | TopologySelectorHelper::CreatedByFeature
                | TopologySelectorHelper::SplitByFeature => {
                    unreachable!("ClosestPoint/IsOn outer match guarantees this")
                }
            }
        }
        TopologySelectorHelper::AngleBetweenSurfaces => {
            // Both args: geometry ValueRefs → named_steps map → GeometryHandleId.
            let face_a = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let face_b = resolve_geometry_handle_arg(&args[1], named_steps)?;
            let query = reify_ir::GeometryQuery::SurfaceAngle { face_a, face_b };
            dispatch_surface_angle(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Contains => {
            // args[0]: solid geometry ValueRef → named_steps map → GeometryHandleId.
            // args[1]: point ValueRef → values map → Value::Point of three Length scalars.
            // Arg order is solid-then-point (mirror of is_on: point-then-geometry).
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            // Use `reify_ir::DEFAULT_CONTAINS_TOLERANCE_M` (= OCCT's
            // `Precision::Confusion()`, ~1e-7) as the default tolerance for the
            // v0.1 2-arg `contains(solid, point)` form, matching the is_on
            // precedent per §5.2. A future explicit-tolerance
            // `contains(solid, point, tol)` overload will plumb the
            // user-supplied tolerance through here.
            let query = reify_ir::GeometryQuery::Contains {
                handle,
                px: point[0],
                py: point[1],
                pz: point[2],
                tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            };
            // Reuse the Bool-unwrap helper from `is_on`: dispatches
            // `kernel.query(&query)` and unwraps `Value::Bool`, downgrading
            // non-Bool / Err replies to `Some(Value::Undef)` + Warning per §4.
            dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::GeoEquiv => {
            // geo_equiv(left, right, tol) → Bool (task 3613, KGQ-δ, PRD §5.1).
            // True iff BOTH topology equivalence (canonical TopExp::MapShapes
            // per-kind counts match) AND sampled-vertex tolerance (N=8 uniform
            // parameter points per face/edge; |p_a - p_b| < tol) hold.
            //
            // FUTURE: geo_equiv_strict(a, b, tol) — symmetric Hausdorff distance
            // variant deferred to v0.4 (PRD §5.1, Open Question §10).
            //
            // args[0]: left geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: right geometry ValueRef → named_steps → GeometryHandleId.
            // args[2]: tolerance ValueRef → values → Value::length(m) → SI metres.
            let left = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let right = resolve_geometry_handle_arg(&args[1], named_steps)?;
            let tolerance = resolve_length_scalar_arg(
                &args[2],
                values,
                &function.name,
                "tolerance",
                diagnostics,
            )?;
            let query = reify_ir::GeometryQuery::GeoEquiv {
                left,
                right,
                tolerance,
            };
            // Reuse the Bool-unwrap helper: dispatches kernel.query(&query) and
            // unwraps Value::Bool, downgrading non-Bool / Err replies to
            // Some(Value::Undef) + Warning (function.name = "geo_equiv").
            dispatch_point_on_shape(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Angle => {
            // Both args: value-flow Vec3 ValueRefs → values map → [f64; 3].
            // Pure-math: acos(clamp(dot(a,b)/(|a||b|), -1, 1)). No kernel call.
            //
            // The dot-product and L2-norm are hand-rolled on [f64; 3] rather than
            // reusing `crates/reify-stdlib/src/linalg.rs` because that crate
            // operates on `Value` tensors, not bare [f64; 3] slices.  If the
            // degenerate-input semantics here ever diverge from linalg.rs's
            // `magnitude`/`dot` handling, align them explicitly.  See also the
            // unit tests for `angle` in this module (task 3614, KGQ-ε).
            let a = resolve_vec3_arg(&args[0], values, &function.name, "a", diagnostics)?;
            let b = resolve_vec3_arg(&args[1], values, &function.name, "b", diagnostics)?;
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let na = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            let nb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
            // Primary degenerate guard: zero-length or explicitly non-finite
            // (NaN/inf component → NaN magnitude, overflow → inf magnitude).
            if na == 0.0 || nb == 0.0 || !na.is_finite() || !nb.is_finite() {
                diagnostics.push(Diagnostic::warning(format!(
                    "angle: degenerate input — zero-length or non-finite vector \
                     (|a|={na}, |b|={nb}); cell left at Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
            let cos_theta = (dot / (na * nb)).clamp(-1.0, 1.0);
            // Secondary degenerate guard: catch NaN from subnormal magnitude
            // underflow (na*nb underflows to 0.0 while both na and nb
            // individually passed the guard above — a rare but possible case
            // with extremely small component values).  clamp() propagates NaN
            // unchanged in IEEE 754, so this must be tested after clamping.
            if !cos_theta.is_finite() {
                diagnostics.push(Diagnostic::warning(format!(
                    "angle: computed cosine is non-finite \
                     (|a|={na}, |b|={nb}, dot={dot}); \
                     possible subnormal magnitude underflow; cell left at Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
            let theta = cos_theta.acos();
            Some(reify_ir::Value::angle(theta))
        }
        TopologySelectorHelper::Normal => {
            // `normal(surface, point) -> Vector3<Dimensionless>` (task 3615, KGQ-ζ).
            // Arg order mirrors `contains`: Surface=args[0], Point3<Length>=args[1].
            // args[0]: surface geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: point ValueRef → values → Value::Point of three Length scalars → [f64;3] SI metres.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            let query = reify_ir::GeometryQuery::FaceNormalAt {
                handle,
                px: point[0],
                py: point[1],
                pz: point[2],
            };
            dispatch_normal_vector3(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::Curvature => {
            // `curvature(shape, point) -> Scalar<Curvature>|Matrix<2,2,Curvature>` (task 3621, KGQ-μ).
            // Arg order: Shape=args[0], Point3<Length>=args[1].
            // args[0]: geometry ValueRef → named_steps → GeometryHandleId.
            // args[1]: point ValueRef → values → [f64;3] SI metres (px, py, pz).
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let point =
                resolve_point3_length_arg(&args[1], values, &function.name, "point", diagnostics)?;
            dispatch_curvature(kernel, handle, point, &function.name, diagnostics)
        }
        TopologySelectorHelper::Length => {
            // `length(curve) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: edge sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, kernel_handle) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_edge_length(kernel, kernel_handle, &function.name, diagnostics)
        }
        TopologySelectorHelper::Perimeter => {
            // `perimeter(surface) -> Scalar<Length>` (task 3622, KGQ-ν).
            // arg[0]: face sub-handle ValueRef → values → kernel_handle.
            // Falls through (None) when arg is not a hydrated Value::GeometryHandle
            // (PRD invariant #2).
            let (_, _, face_kh) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            dispatch_perimeter(kernel, face_kh, &function.name, diagnostics)
        }
        TopologySelectorHelper::Distance => {
            // `distance(a, b) -> Scalar<Length>` (task 3610, KGQ-α, PRD §9).
            //
            // Resolve each arg as Shape (named_steps) else Point (values).
            // Non-ValueRef args → None on that resolution, fall-through to None.
            //
            // 2×2 dispatch matrix:
            //   (Shape, Point) / (Point, Shape) → ClosestPointOnShape + Euclidean
            //   (Shape, Shape)                  → GeometryQuery::Distance{from,to}
            //   (Point, Point)                  → pure Euclidean, no kernel call
            //
            // Kernel-error-downgrade contract (invariant #3): on Err or malformed
            // ClosestPointOnShape reply, `dispatch_point3_length_reply` returns
            // `Some(Value::Undef)` with exactly one Warning diagnostic (not None),
            // so the cell is visibly degraded rather than silently preserved.

            // Extract the SI-metre f64 from a length-typed Value component.
            // Returns None for non-numeric variants (dead-code guard: in practice
            // dispatch_point3_length_reply always yields Value::Scalar{LENGTH}
            // components; returning None rather than NAN makes misbehaviour
            // visible so the caller can downgrade to Undef + Warning).
            let extract_si = |v: &reify_ir::Value| -> Option<f64> {
                match v {
                    reify_ir::Value::Scalar { si_value, .. } => Some(*si_value),
                    reify_ir::Value::Real(r) => Some(*r),
                    _ => None,
                }
            };

            // Euclidean distance between two SI-metre 3-D points.
            let euclidean_3d = |a: [f64; 3], b: [f64; 3]| -> f64 {
                let dx = a[0] - b[0];
                let dy = a[1] - b[1];
                let dz = a[2] - b[2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            };

            let arg0_shape = resolve_geometry_handle_arg(&args[0], named_steps);
            // The point resolver is the SECOND probe (shape first, else point):
            // when it is reached the arg is already known not to be a resolvable
            // shape, so a defined-but-wrong value is genuinely neither shape nor
            // point — per PRD §7.3 "never silent" it emits one Warning naming
            // `distance` / the positional arg / Point<Length>. A shape arg never
            // reaches the point probe (arg0_shape is Some → probe skipped), so
            // the common Shape×Shape / Shape×Point forms stay diagnostic-free; an
            // Undef arg still degrades quietly.
            let arg0_point = if arg0_shape.is_none() {
                resolve_point3_length_arg(&args[0], values, &function.name, "a", diagnostics)
            } else {
                None
            };
            let arg1_shape = resolve_geometry_handle_arg(&args[1], named_steps);
            let arg1_point = if arg1_shape.is_none() {
                resolve_point3_length_arg(&args[1], values, &function.name, "b", diagnostics)
            } else {
                None
            };

            // Normalise Shape×Point and Point×Shape to a single (handle, point)
            // pair — both cases are symmetric and share one ClosestPointOnShape
            // block, eliminating the 40-line duplication that was a maintenance
            // hazard (reviewer note: each arm was byte-for-byte identical).
            let shape_point_pair = match (arg0_shape, arg0_point, arg1_shape, arg1_point) {
                (Some(h), None, None, Some(p)) | (None, Some(p), Some(h), None) => Some((h, p)),
                _ => None,
            };

            if let Some((handle, point)) = shape_point_pair {
                // Shape × Point / Point × Shape: issue ClosestPointOnShape on the
                // shape then compute Euclidean distance from the query point.
                //
                // `dispatch_point3_length_reply` handles Err/malformed with
                // Some(Value::Undef) + one Warning (invariant #3). On success it
                // returns Some(Value::Point([length, length, length])).
                let query = reify_ir::GeometryQuery::ClosestPointOnShape {
                    handle,
                    px: point[0],
                    py: point[1],
                    pz: point[2],
                };
                match dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics) {
                    Some(reify_ir::Value::Point(comps)) if comps.len() == 3 => {
                        let cx = extract_si(&comps[0]);
                        let cy = extract_si(&comps[1]);
                        let cz = extract_si(&comps[2]);
                        match (cx, cy, cz) {
                            (Some(cx), Some(cy), Some(cz)) => {
                                Some(reify_ir::Value::length(euclidean_3d(point, [cx, cy, cz])))
                            }
                            // Non-numeric component — unexpected but guarded;
                            // downgrade visibly rather than silently emitting NaN
                            // (invariant #3, reviewer note on robustness).
                            _ => {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "{}: ClosestPointOnShape reply contained a \
                                     non-numeric component; treating distance as undefined",
                                    &function.name
                                )));
                                Some(reify_ir::Value::Undef)
                            }
                        }
                    }
                    // Undef reply (error already warned by dispatch helper) →
                    // propagate.
                    Some(reify_ir::Value::Undef) => Some(reify_ir::Value::Undef),
                    // None from dispatch_point3_length_reply (shouldn't happen) → None.
                    _ => None,
                }
            } else {
                match (arg0_shape, arg0_point, arg1_shape, arg1_point) {
                    (Some(from), None, Some(to), None) => {
                        // Shape × Shape: issue GeometryQuery::Distance{from,to} via
                        // kernel_distance. Returns None on Err/non-numeric (already
                        // warned); map None → Some(Value::Undef) per invariant #3.
                        // Exactly one kernel query (invariant #4).
                        match kernel_distance(kernel, from, to, diagnostics, &function.name) {
                            Some(d) => Some(reify_ir::Value::length(d)),
                            None => Some(reify_ir::Value::Undef),
                        }
                    }
                    (None, Some(pa), None, Some(pb)) => {
                        // Point × Point: pure Euclidean, no kernel call (invariant
                        // #4: 0 queries).
                        Some(reify_ir::Value::length(euclidean_3d(pa, pb)))
                    }
                    // Non-ValueRef / unresolvable args — fall through to None
                    // (invariants #1/#2).
                    _ => None,
                }
            }
        }
        TopologySelectorHelper::Intersects => {
            // `intersects(a, b) -> Bool` (task 3612, KGQ-γ, PRD §9).
            //
            // Routes through GeometryQuery::Distance{from,to} via kernel_distance,
            // classifying d <= 0.0 → Bool(true) (shapes touching or overlapping)
            // and d > 0.0 → Bool(false) (shapes apart).
            //
            // This reproduces the shipped shapes_intersect adapter semantics
            // (reify-kernel-occt/src/lib.rs:770: "Ok(true) iff min BREP distance
            // ≤ 0.0") and the kinematic interferes_with precedent
            // (geometry_ops.rs:1601: `Some(d) => Bool(d <= 0.0)`).
            //
            // NOTE: d=0.0 (touching / face-coincident) → Bool(true) here.  The
            // Manifold-side queries::intersects returns false for the same case
            // (CSG boolean yields empty mesh for zero shared volume) — a known
            // parity divergence to be resolved by KGQ-ο (Phase 5).
            //
            // Both args must be Shape ValueRefs. Non-ValueRef/non-geometry args
            // return None from resolve_geometry_handle_arg → fall through to None
            // (invariants #1/#2). Kernel Err/non-numeric already emitted one
            // Warning and returns None → mapped to Some(Undef) (invariant #3).
            // Exactly one kernel query (invariant #4).
            let from = resolve_geometry_handle_arg(&args[0], named_steps)?;
            let to = resolve_geometry_handle_arg(&args[1], named_steps)?;
            match kernel_distance(kernel, from, to, diagnostics, &function.name) {
                Some(d) => Some(reify_ir::Value::Bool(d <= 0.0)),
                None => Some(reify_ir::Value::Undef),
            }
        }
        TopologySelectorHelper::Split => {
            // `split(solid, plane) -> List<Geometry>` (task 4190, PRD ζ).
            //
            // args[0]: solid ValueRef → values map → full parent GeometryHandle.
            //   Resolved via `resolve_parent_geometry_handle_arg` so we get the
            //   parent's realization_ref + upstream_values_hash for sub-handle
            //   construction (PRD §4).  Falls through to None when the arg cell
            //   is not yet a hydrated Value::GeometryHandle (PRD invariant #2:
            //   never partially construct a sub-handle).
            // args[1]: plane ValueRef → values map → Value::Plane.
            //   Decoded via `decode_plane` → (plane_origin, plane_normal [f64;3]).
            //   Falls through to None when args[1] is not a Value::Plane (wrong
            //   variant, unresolved, Undef, etc.) — same fall-through contract.
            // Dispatch: `GeometryKernel::execute_split(&GeometryOp::Split{..})`.
            //   On Ok(ids): build Value::List via make_sub_handle(SubKind::Solid).
            //   On Err: emit Warning diagnostic, return Some(Value::Undef).
            let (parent_rr, parent_hash, parent_kernel_handle) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;

            // Resolve and decode the plane arg.
            let plane_cell_id = match &args[1].kind {
                reify_ir::CompiledExprKind::ValueRef(id) => id,
                _ => return None,
            };
            let plane_val = values.get(plane_cell_id)?;
            let (plane_origin, plane_normal) = match decode_plane(plane_val) {
                Ok(pair) => pair,
                Err(_) => return None,
            };

            let op = reify_ir::GeometryOp::Split {
                target: parent_kernel_handle,
                plane_origin,
                plane_normal,
            };
            match kernel.execute_split(&op) {
                Ok(piece_ids) => {
                    let elements = piece_ids
                        .into_iter()
                        .enumerate()
                        .map(|(i, piece_kernel_id)| {
                            crate::topology_selectors::make_sub_handle(
                                &parent_rr,
                                &parent_hash,
                                crate::topology_selectors::SubKind::Solid,
                                i as u32,
                                piece_kernel_id,
                            )
                        })
                        .collect();
                    Some(reify_ir::Value::List(elements))
                }
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{}({:?}): kernel error: {}",
                        function.name, parent_kernel_handle, err
                    )));
                    Some(reify_ir::Value::Undef)
                }
            }
        }
        // Task 4118 (γ): `edges(solid)` / `faces(solid)` and the other 7
        // kernel-free leaf ctors delegate to the shared helper, which uses
        // `resolve_symbolic_selector_target` (accepts both symbolic and realized
        // handles).  In the build path all targets are realized (Some), so the
        // behaviour is identical to the previous per-arm `resolve_selector_target`
        // calls.  See `try_build_kernel_free_leaf_selector` for the full list.
        TopologySelectorHelper::Edges
        | TopologySelectorHelper::Faces
        | TopologySelectorHelper::MidSurface
        | TopologySelectorHelper::EdgesByLength
        | TopologySelectorHelper::FacesByArea
        | TopologySelectorHelper::FacesByNormal
        | TopologySelectorHelper::EdgesParallelTo
        | TopologySelectorHelper::EdgesAtHeight
        | TopologySelectorHelper::Vertices
        // task 3523: the 6 v2 leaf ctors are likewise kernel-free at construction.
        | TopologySelectorHelper::FacesPerpendicularTo
        | TopologySelectorHelper::EdgesPerpendicularTo
        | TopologySelectorHelper::FacesBySurfaceKind
        | TopologySelectorHelper::EdgesByCurveKind
        | TopologySelectorHelper::ExtremalByBbox
        | TopologySelectorHelper::ExtremalByCentroid
        // task 4831 (P3β): the two feature-provenance leaf ctors are likewise
        // kernel-free at construction — same shared-helper delegation.
        | TopologySelectorHelper::CreatedByFeature
        | TopologySelectorHelper::SplitByFeature => {
            try_build_kernel_free_leaf_selector(helper, args, values, &function.name, diagnostics)
        }
        TopologySelectorHelper::CenterOfMass => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Value::Scalar{MASS_DENSITY}.
            // Uses resolve_density_arg (same as MomentOfInertia) — Contract A
            // (task 4486 γ): only a dimensioned Density is accepted; bare Real
            // and dimensionless Scalar now emit a Severity::Warning.
            let density = resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::CenterOfMass { handle, density };
            dispatch_point3_length_reply(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::MomentOfInertia => {
            // args[0]: geometry ValueRef → named_steps map → GeometryHandleId.
            let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
            // args[1]: density ValueRef → values map → Value::Scalar{MASS_DENSITY}.
            // Uses resolve_density_arg — Contract A (task 4486 γ): only a
            // dimensioned Density is accepted; bare Real and dimensionless
            // Scalar now emit a Severity::Warning.
            let density = resolve_density_arg(&args[1], values, &function.name, diagnostics)?;
            let query = reify_ir::GeometryQuery::InertiaTensor { handle, density };
            dispatch_inertia_tensor(kernel, &query, &function.name, diagnostics)
        }
        TopologySelectorHelper::AdjacentFaces => {
            // args[0]: parent solid ValueRef → values map → full Value::GeometryHandle.
            // Must resolve from `values` (not `named_steps`) so we get the parent's
            // realization_ref + upstream_values_hash for sub-handle construction (PRD §4).
            // Falls through to None when the arg cell is not a hydrated Value::GeometryHandle
            // (PRD invariant #2: never partially construct a sub-handle).
            let (parent_rr, parent_hash, parent_kh) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: face sub-handle ValueRef → values map → kernel_handle only.
            // Real face sub-handles (e.g. from faces(b) / single(faces_by_normal(...)))
            // live in `values` as Value::GeometryHandle, not in named_steps (design §4).
            let (_, _, face_kh) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            // `adjacent_to_face` recovers the 0-based face index via
            // `extract_faces(parent)`, dispatches `GeometryQuery::AdjacentFaces`,
            // and maps the reply indices back to face handles.
            // Result saved before dispatch to avoid a double-mutable-borrow on kernel.
            // Output: List<Value::GeometryHandle> sub-handles per PRD §4 (KGQ-κ).
            let filter_result =
                crate::selector_vocabulary_v2::adjacent_to_face(kernel, parent_kh, face_kh);
            dispatch_filtered_subhandles(
                kernel,
                parent_kh,
                crate::topology_selectors::SubKind::Face,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::SharedEdges => {
            // args[0]: face_a ValueRef → values map → kernel_handle only.
            // Face sub-handles live in `values` as Value::GeometryHandle.
            // Falls through to None when not hydrated (PRD invariant #2).
            let (_, _, face_a) = resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: face_b ValueRef → values map → kernel_handle only.
            let (_, _, face_b) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            dispatch_shared_edges(kernel, face_a, face_b, &function.name, diagnostics, values)
        }
        // ── task #4759: relational-walk v2 selectors ─────────────────────────
        TopologySelectorHelper::SiblingsOfFace => {
            // args[0]: parent solid ValueRef → values map → full Value::GeometryHandle.
            // Must resolve from `values` (not `named_steps`) to obtain the parent's
            // realization_ref + upstream_values_hash for sub-handle construction.
            // Falls through to None when the arg cell is not a hydrated Value::GeometryHandle
            // (PRD invariant #2: never partially construct a sub-handle).
            let (parent_rr, parent_hash, parent_kh) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: face sub-handle ValueRef → values map → kernel_handle only.
            let (_, _, face_kh) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            // `siblings_of_face` = extract_faces(parent) minus face; pure composition,
            // zero kernel queries beyond extract_faces.
            let filter_result =
                crate::selector_vocabulary_v2::siblings_of_face(kernel, parent_kh, face_kh);
            dispatch_filtered_subhandles(
                kernel,
                parent_kh,
                crate::topology_selectors::SubKind::Face,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        TopologySelectorHelper::AncestorFacesOfEdge => {
            // args[0]: parent solid ValueRef → values map → full Value::GeometryHandle.
            let (parent_rr, parent_hash, parent_kh) =
                resolve_parent_geometry_handle_arg(&args[0], values)?;
            // args[1]: edge sub-handle ValueRef → values map → kernel_handle only.
            let (_, _, edge_kh) = resolve_parent_geometry_handle_arg(&args[1], values)?;
            // `ancestor_faces_of_edge` → GeometryQuery::AncestorFacesOfEdge;
            // for a closed manifold solid, every edge bounds exactly 2 faces.
            let filter_result =
                crate::selector_vocabulary_v2::ancestor_faces_of_edge(kernel, parent_kh, edge_kh);
            dispatch_filtered_subhandles(
                kernel,
                parent_kh,
                crate::topology_selectors::SubKind::Face,
                &parent_rr,
                &parent_hash,
                filter_result,
                &function.name,
                diagnostics,
            )
        }
        // ── task 4119 δ: selector-composition algebra ────────────────────────
        // union(a, b, …) / intersect(a, b, …) / difference(a, b) build a
        // kernel-FREE composite `Value::Selector(kind)` whose tree is
        // Union/Intersect/Difference of the child SelectorValues.  Child
        // selectors are reconstructed via `reconstruct_selector_value` (either
        // an inline nested selector FunctionCall or a ValueRef to an already-
        // patched selector cell).  The K1 kind-closure check is delegated to
        // the `SelectorValue::{union,intersect,difference}` constructors; on
        // `SelectorError::KindMismatch` (defensive backstop — compile-time
        // E_SELECTOR_KIND_MISMATCH should have fired first) a Warning is emitted
        // and `Some(Value::Undef)` is returned, mirroring `build_leaf_selector`.
        // Zero kernel queries at construction time (K2/BT7).
        TopologySelectorHelper::Union => eval_variadic_composition(
            "union",
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            reify_ir::value::SelectorValue::union,
        ),
        TopologySelectorHelper::Intersect => eval_variadic_composition(
            "intersect",
            args,
            named_steps,
            values,
            kernel,
            diagnostics,
            reify_ir::value::SelectorValue::intersect,
        ),
        TopologySelectorHelper::Difference => {
            // args[0] and args[1] guaranteed by the == 2 arity gate.
            let a = reconstruct_selector_value(&args[0], named_steps, values, kernel, diagnostics)?;
            let b = reconstruct_selector_value(&args[1], named_steps, values, kernel, diagnostics)?;
            match reify_ir::value::SelectorValue::difference(a, b) {
                Ok(sv) => Some(reify_ir::Value::Selector(sv)),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "difference: selector kind-closure violation ({err:?}); cell left at Undef"
                    )));
                    Some(reify_ir::Value::Undef)
                }
            }
        }
        // ── task 4119 δ: Named-leaf constructors ─────────────────────────────
        // face(geometry, name) / edge(geometry, name) / solid_body(geometry, name)
        // resolve the parent GeometryHandleRef from args[0] (via
        // resolve_named_leaf_target, which accepts EITHER a realized
        // Value::GeometryHandle OR a hydrated Value::Selector — task #4583) and
        // the name string from args[1] (via resolve_string_literal_arg).  Both must
        // succeed; either falling through yields None (cell left at Undef — PRD
        // invariant #2). Zero kernel queries at construction time (K2/BT7); Named
        // resolution is the D8 interim (W_TOPOLOGY_TAG_STALE + [] until
        // persistent-naming-v2, tasks 2302/2570).
        TopologySelectorHelper::Face => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Face,
            args,
            named_steps,
            values,
            kernel,
            &function.name,
            diagnostics,
        ),
        TopologySelectorHelper::Edge => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Edge,
            args,
            named_steps,
            values,
            kernel,
            &function.name,
            diagnostics,
        ),
        TopologySelectorHelper::SolidBody => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Body,
            args,
            named_steps,
            values,
            kernel,
            &function.name,
            diagnostics,
        ),
        // task 4368: 0-D vertex selector ctors — `Vertices` (arity-1 All-leaf)
        // is handled by the or-pattern above; `Vertex` (arity-2 Named-leaf) stays here.
        TopologySelectorHelper::Vertex => eval_named_leaf_selector_ctor(
            reify_core::ty::SelectorKind::Vertex,
            args,
            named_steps,
            values,
            kernel,
            &function.name,
            diagnostics,
        ),
    }
}

/// Dispatch the `shared_edges(face_a, face_b)` selector per design-doc §4.3.
///
/// Pipeline:
///   1. Derive each face's parent solid via `selector_vocabulary_v2::owner_body_of`
///      (which issues `GeometryQuery::OwnerBody` and decodes the `Value::Int`
///      reply). On query error → warning + `Value::Undef`.
///   2. If the two parents differ → push a "different parent solids" warning
///      and return `Value::List(vec![])` (silent degrade — empty list is
///      structurally valid as a `List<Geometry>` cell while the warning
///      surfaces the user-actionable issue).
///   3. Recover each face's 0-based index in the parent via
///      `extract_faces(parent)` + `position`. On extract error OR a face not
///      appearing in `extract_faces` → warning + `Value::Undef`.
///   4. Dispatch `GeometryQuery::SharedEdges { shape, face_a, face_b }`. On
///      query error or non-`Value::List` reply → warning + `Value::Undef`.
///   5. Map the reply integer indices back to edge handles via
///      `extract_edges(parent)`. Skip indices that fall outside the edge
///      enumeration (defensive against a kernel bug rather than a hard
///      failure mode — see design-doc §4.3 for the rationale).
///   6. Recover the parent solid's `(realization_ref, upstream_values_hash)` via
///      `resolve_owner_solid_handle(values, parent_a)`. Falls through to `None`
///      when the parent solid is not hydrated in `values` (PRD invariant #2).
///   7. Return `Value::List(Vec<Value::GeometryHandle>)` edge sub-handles per PRD §4 (KGQ-κ).
fn dispatch_shared_edges(
    kernel: &mut dyn reify_ir::GeometryKernel,
    face_a: GeometryHandleId,
    face_b: GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
    values: &reify_ir::ValueMap,
) -> Option<reify_ir::Value> {
    // (1) Derive parents via OwnerBody.
    let parent_a = match crate::selector_vocabulary_v2::owner_body_of(kernel, face_a) {
        Ok(p) => p,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} OwnerBody({:?}) failed: {}",
                helper_name, face_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let parent_b = match crate::selector_vocabulary_v2::owner_body_of(kernel, face_b) {
        Ok(p) => p,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} OwnerBody({:?}) failed: {}",
                helper_name, face_b, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (2) Cross-solid guard rail: empty list + warning when faces span
    //     different parents (design-doc §4.3).
    if parent_a != parent_b {
        diagnostics.push(Diagnostic::warning(format!(
            "{}: faces have different parent solids ({:?} vs {:?}); returning empty list",
            helper_name, parent_a, parent_b
        )));
        return Some(reify_ir::Value::List(Vec::new()));
    }

    // (3) Recover 0-based face indices via extract_faces(parent).
    let faces = match kernel.extract_faces(parent_a) {
        Ok(f) => f,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} extract_faces({:?}) failed: {}",
                helper_name, parent_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let idx_a = match faces.iter().position(|h| *h == face_a) {
        Some(i) => i,
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: face_a {:?} is not a child of parent {:?} (was extract_faces called?)",
                helper_name, face_a, parent_a
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let idx_b = match faces.iter().position(|h| *h == face_b) {
        Some(i) => i,
        None => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: face_b {:?} is not a child of parent {:?} (was extract_faces called?)",
                helper_name, face_b, parent_a
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (4) Dispatch SharedEdges query.
    let reply = match kernel.query(&reify_ir::GeometryQuery::SharedEdges {
        shape: parent_a,
        face_a: idx_a,
        face_b: idx_b,
    }) {
        Ok(v) => v,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} SharedEdges query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let int_indices = match reply {
        reify_ir::Value::List(items) => items,
        other => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: expected Value::List from SharedEdges, got {:?}",
                helper_name, other
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // (5) Map reply indices back to edge handles via extract_edges(parent).
    let edges = match kernel.extract_edges(parent_a) {
        Ok(e) => e,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} extract_edges({:?}) failed: {}",
                helper_name, parent_a, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(int_indices.len());
    for item in int_indices {
        let idx = match item {
            reify_ir::Value::Int(i) => i,
            other => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: expected Value::Int element in SharedEdges list, got {:?}",
                    helper_name, other
                )));
                return Some(reify_ir::Value::Undef);
            }
        };
        let usize_idx: usize = match idx.try_into() {
            Ok(u) => u,
            Err(_) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: SharedEdges returned negative index {}",
                    helper_name, idx
                )));
                return Some(reify_ir::Value::Undef);
            }
        };
        // Defensive: silently skip out-of-range indices rather than failing
        // hard — surfaces a malformed kernel reply as a smaller-than-expected
        // list rather than total cell collapse.
        if let Some(h) = edges.get(usize_idx) {
            out.push(*h);
        }
    }

    // (6) Recover the parent solid's realization_ref + upstream_values_hash from
    //     `values`. Edge sub-handles must compose from the parent solid's hash
    //     (PRD §4 cache coherence), not from a face sub-handle hash.
    //     Falls through to None when the parent solid cell is absent (e.g. unnamed
    //     inline solid), per PRD invariant #2 (never partial-construct sub-handles).
    let (parent_rr, parent_hash) = resolve_owner_solid_handle(values, parent_a)?;

    // (7) Emit List<Value::GeometryHandle> edge sub-handles via dispatch_filtered_subhandles,
    //     which re-extracts extract_edges(parent) to map retained ids → TopExp indices and
    //     builds make_sub_handle entries per PRD §4 iii/iv.
    dispatch_filtered_subhandles(
        kernel,
        parent_a,
        crate::topology_selectors::SubKind::Edge,
        &parent_rr,
        &parent_hash,
        Ok(out),
        helper_name,
        diagnostics,
    )
}

/// Run a pre-computed filtered-selector result and emit a `Value::List` of
/// `Value::GeometryHandle` sub-handles whose `upstream_values_hash` encodes the
/// canonical TopExp index of each retained sub-shape (PRD §4 iii/iv).
///
/// After the filter returns the retained `Vec<GeometryHandleId>`, we
/// re-extract the canonical sub-shape list and map each retained id to its
/// 0-based position, so `faces_by_normal(box,+z,1°)[0]` hashes identically to
/// `faces(box)[k]` for the same physical face.  Relies on PRD §4 intra-session
/// handle persistence (extract_* yields stable ids within a session).
///
/// Defensively warns + skips any retained id absent from the canonical list
/// rather than crashing — surfaces a malformed kernel state as a
/// shorter-than-expected list rather than total cell collapse.
#[allow(clippy::too_many_arguments)]
fn dispatch_filtered_subhandles(
    kernel: &mut dyn reify_ir::GeometryKernel,
    parent_kernel_handle: GeometryHandleId,
    sub_kind: crate::topology_selectors::SubKind,
    parent_rr: &reify_core::identity::RealizationNodeId,
    parent_hash: &[u8; 32],
    filter_result: Result<Vec<GeometryHandleId>, reify_ir::QueryError>,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let retained = match filter_result {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // Re-extract the canonical list to recover each retained id's TopExp index.
    // The filter helper already called extract_* once; this second call is safe
    // because PRD §4 intra-session handle persistence guarantees stable ids+order.
    let canonical = match sub_kind {
        crate::topology_selectors::SubKind::Edge => kernel.extract_edges(parent_kernel_handle),
        crate::topology_selectors::SubKind::Face => kernel.extract_faces(parent_kernel_handle),
        // SubKind::Vertex is unreachable here: Vertex selectors only carry
        // LeafQuery::All or LeafQuery::Named (K1 rejects every predicate query
        // — ByLength, ByArea, ByNormal, etc. — for Vertex kind via
        // required_kind()).  dispatch_filtered_subhandles is called ONLY from
        // predicate-filter dispatch arms (edges_by_length, faces_by_normal,
        // etc.); the All/Named resolution path for vertices goes through
        // dispatch_extract_subshapes instead.
        crate::topology_selectors::SubKind::Vertex => {
            unreachable!(
                "dispatch_filtered_subhandles called with SubKind::Vertex — \
                 Vertex selectors carry only All/Named leaves (no predicate \
                 filters exist for 0-D topology); predicate callers are \
                 Face/Edge-only"
            )
        }
        // SubKind::Solid is only used by the Split dispatch arm, which calls
        // execute_split directly — it never reaches dispatch_filtered_subhandles.
        crate::topology_selectors::SubKind::Solid => {
            unreachable!(
                "dispatch_filtered_subhandles called with SubKind::Solid — \
                 split pieces are handled by the Split arm via execute_split, \
                 not through the filter-subhandle path"
            )
        }
    };
    let canonical = match canonical {
        Ok(ids) => ids,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}: canonical re-extract failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    let canonical_index_map: std::collections::HashMap<GeometryHandleId, usize> = canonical
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let mut elements: Vec<reify_ir::Value> = Vec::with_capacity(retained.len());
    for retained_id in retained {
        match canonical_index_map.get(&retained_id) {
            Some(&canonical_index) => {
                elements.push(crate::topology_selectors::make_sub_handle(
                    parent_rr,
                    parent_hash,
                    sub_kind,
                    canonical_index as u32,
                    retained_id,
                ));
            }
            None => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{}: retained handle {:?} absent from canonical list; skipping",
                    helper_name, retained_id
                )));
            }
        }
    }
    Some(reify_ir::Value::List(elements))
}

#[derive(Clone, Copy)]
enum TopologySelectorHelper {
    ClosestPoint,
    IsOn,
    AngleBetweenSurfaces,
    /// `edges(geometry) -> List<Geometry>` — extract the unique edges of a
    /// shape (task 3560).
    Edges,
    /// `faces(geometry) -> List<Geometry>` — extract the unique faces of a
    /// shape (task 3560).
    Faces,
    /// `mid_surface(geometry) -> Selector(Face)` — role-addressed leaf ctor
    /// (task 4536). Builds a kind-typed `Value::Selector(Face)` LEAF carrying
    /// `LeafQuery::ByRole(Role::MidSurfaceFace)`; resolution filters the realized
    /// body's `TopologyAttributeTable` for the shell-extract synthetic
    /// mid-surface faces (which are NOT enumerable via `extract_faces`). Arity 1,
    /// kernel-FREE at construction (K2/BT7). Composes with 4119's
    /// union/intersect as a first-class kind-typed leaf.
    MidSurface,
    /// `created_by_feature(solid, f) -> Selector(Face)` — provenance-addressed
    /// leaf ctor (task 4831, P3β). Builds a kind-typed `Value::Selector(Face)`
    /// LEAF carrying `LeafQuery::CreatedByFeature(fid)`; resolution filters the
    /// realized body's `TopologyAttributeTable` for face-role entries whose
    /// `feature_id` equals `fid` (see `reify_ir::value::LeafQuery::CreatedByFeature`
    /// for the full D2 contract). Arity 2 (solid, f : Feature), kernel-FREE at
    /// construction — mirrors `MidSurface`. Composes with 4119's union/intersect
    /// as a first-class kind-typed leaf.
    CreatedByFeature,
    /// `split_by_feature(solid, f) -> Selector(Face)` — the `mod_history`-gated
    /// sibling of [`Self::CreatedByFeature`] (task 4831, P3β). Carries
    /// `LeafQuery::SplitByFeature(fid)`; resolution filters for face-role
    /// entries whose `mod_history` records a split by `fid`. Same arity /
    /// kernel-free contract as `CreatedByFeature`.
    SplitByFeature,
    /// `center_of_mass(geometry, density) -> Point3<Length>` — uniform-density
    /// center of mass (task 3560).
    CenterOfMass,
    /// `moment_of_inertia(geometry, density) -> Tensor<2,3,MomentOfInertia>` —
    /// mass-weighted 3×3 inertia tensor about the centroid (task 3560).
    MomentOfInertia,
    /// `edges_by_length(geometry, Range<Length>) -> List<Geometry>` — edges
    /// whose length falls in the range (task 3560).
    EdgesByLength,
    /// `faces_by_area(geometry, Range<Area>) -> List<Geometry>` — faces whose
    /// surface area falls in the range (task 3560).
    FacesByArea,
    /// `faces_by_normal(geometry, Vec3, Angle) -> List<Geometry>` — faces
    /// whose outward normal is within an angular tolerance of a target
    /// direction (task 3560).
    FacesByNormal,
    /// `edges_parallel_to(geometry, Vec3, Angle) -> List<Geometry>` — edges
    /// whose midpoint tangent is (anti-)parallel to an axis within an
    /// angular tolerance (task 3560).
    EdgesParallelTo,
    /// `edges_at_height(geometry, Length, Length) -> List<Geometry>` — edges
    /// lying entirely within a tolerance of a horizontal `z = z0` plane
    /// (task 3560).
    EdgesAtHeight,
    /// `adjacent_faces(parent, face) -> List<Geometry>` — faces of `parent`
    /// that share at least one edge with `face` (task 3560).
    AdjacentFaces,
    /// `shared_edges(face_a, face_b) -> List<Geometry>` — edges of the
    /// common parent solid that lie on the boundary of BOTH faces (task 3560).
    /// Derives the parent via `OwnerBody` on both args; silently degrades to
    /// an empty list (with a warning) when the two faces live on different
    /// parent solids (design-doc §4.3).
    SharedEdges,
    /// `siblings_of_face(parent, face) -> List<Geometry>` — all faces of `parent`
    /// except `face` itself (task #4759). Pure composition: extract_faces(parent)
    /// filtered to exclude the target. Zero kernel queries beyond extract_faces.
    SiblingsOfFace,
    /// `ancestor_faces_of_edge(parent, edge) -> List<Geometry>` — the 1 or 2
    /// faces of `parent` that own `edge` on their boundary (task #4759). Backed
    /// by `GeometryQuery::AncestorFacesOfEdge`; for a closed manifold solid every
    /// edge bounds exactly 2 faces.
    AncestorFacesOfEdge,
    /// `angle(a, b) -> Angle` — angle between two 3-D vectors (task 3614,
    /// PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-ε).
    /// Pure-math: `acos(clamp(dot(a,b)/(|a||b|), -1, 1))`. No kernel call.
    /// Args are value-flow `Vector<3>` resolved from `values`; zero-length
    /// or non-finite input emits a Warning and returns `Some(Value::Undef)`.
    Angle,
    /// `contains(solid, point) -> Bool` — test whether `point` is inside or
    /// on the boundary of the closed solid `solid` (task 3611, KGQ-β, PRD §9).
    /// Backed by `GeometryQuery::Contains` → `BRepClass3d_SolidClassifier`
    /// (IN || ON → true, OUT → false). Arg order is solid-then-point, mirroring
    /// `is_on` with args swapped. Default tolerance from
    /// `DEFAULT_CONTAINS_TOLERANCE_M` per §5.2.
    Contains,
    /// `geo_equiv(left, right, tol) -> Bool` — topology hash + N=8 parameter
    /// sample per §5.1 (task 3613, KGQ-δ, PRD §9). True iff BOTH topology
    /// (per-kind shape count) AND sampled-vertex tolerance hold.
    /// Uses `QueryCapability::BRepAndMesh`; sample count from
    /// `DEFAULT_GEO_EQUIV_SAMPLE_COUNT` (§5.2). Tolerance is an explicit
    /// user-supplied Length arg (no default constant per §5.2).
    ///
    /// FUTURE: `geo_equiv_strict(a, b, tol) -> Bool` — symmetric Hausdorff
    /// distance variant deferred to v0.4 (PRD §5.1, Open Question §10).
    GeoEquiv,
    /// `normal(surface, point) -> Vector3<Dimensionless>` — at-point outward
    /// unit surface normal (task 3615, KGQ-ζ, PRD §9). Projects the Cartesian
    /// `point` onto the face's parametric surface via `ShapeAnalysis_Surface::
    /// ValueOfUV` then returns the orientation-aware outward normal at the
    /// projected (u,v) — the REVERSED-flip convention shared with `FaceNormal`
    /// and `surface_normal_at`. Backed by `GeometryQuery::FaceNormalAt` →
    /// `surface_normal_at_point` in crates/reify-kernel-occt/src/lib.rs.
    /// Arg order: Surface=args[0] (named_steps), Point3<Length>=args[1] (values).
    Normal,
    /// `curvature(shape, point) -> Scalar<Curvature>|Matrix<2,2,Curvature>` —
    /// at-point curvature (task 3621, KGQ-μ, PRD §9). For a surface (face),
    /// returns a 2×2 `Value::Matrix` of principal curvatures [[κ_max,0],[0,κ_min]]
    /// as `Value::Scalar{ dimension: 1/Length }` cells; for a curve (edge),
    /// returns `Value::Scalar{ si_value: κ, dimension: 1/Length }`. Dispatches
    /// `GeometryQuery::SurfaceCurvatureAt` first; on error retries as
    /// `GeometryQuery::CurveCurvatureAt`. Backed by `OcctKernel::curvature_at`
    /// (surface) and `OcctKernel::curve_curvature_at` (curve).
    /// Arg order: Shape=args[0] (named_steps), Point3<Length>=args[1] (values).
    Curvature,
    /// `length(curve) -> Scalar<Length>` — arc length of a single-edge
    /// sub-handle (task 3622, KGQ-ν, PRD §9 Phase 4). Backed by
    /// `GeometryQuery::EdgeLength`. Arg: Curve=args[0] (values sub-handle).
    /// Multi-edge Curve composition deferred per PRD Open Question §10.6.
    Length,
    /// `perimeter(surface) -> Scalar<Length>` — sum of all boundary-edge
    /// lengths of a face sub-handle (task 3622, KGQ-ν, PRD §9 Phase 4).
    /// Composes `extract_edges(face)` + per-edge `EdgeLength`. No new FFI.
    /// Arg: Surface=args[0] (values sub-handle).
    Perimeter,
    /// `distance(a, b) -> Scalar<Length>` — Euclidean distance between two
    /// geometry objects (task 3610, KGQ-α, PRD §9).
    ///
    /// Dispatches a 2×2 arg-kind matrix:
    /// - Shape × Shape → `GeometryQuery::Distance{from,to}` via `kernel_distance`.
    /// - Shape × Point / Point × Shape → `GeometryQuery::ClosestPointOnShape`
    ///   on the shape + Euclidean to the query point.
    /// - Point × Point → pure Euclidean, no kernel call.
    ///
    /// Each arg is resolved as Shape (named_steps via `resolve_geometry_handle_arg`)
    /// else Point (`resolve_point3_length_arg` from values). Non-ValueRef args fall
    /// through to `None` (PRD §4 invariant #1 / #2). Kernel errors downgrade to
    /// `Some(Value::Undef)` + one Warning (invariant #3). At most one kernel
    /// query per call (invariant #4).
    Distance,
    /// `intersects(a, b) -> Bool` — test whether two geometry objects intersect
    /// (task 3612, KGQ-γ, PRD §9).
    ///
    /// Routes through `GeometryQuery::Distance{from,to}` via `kernel_distance`,
    /// classifying `d <= 0.0` → `Bool(true)` and `d > 0.0` → `Bool(false)`.
    /// This reproduces the shipped `shapes_intersect` adapter semantics
    /// (`reify-kernel-occt/src/lib.rs:770`: "Ok(true) iff min BREP distance ≤ 0.0")
    /// and the kinematic `interferes_with` precedent (`geometry_ops.rs:1601`).
    ///
    /// Both args must be Shape ValueRefs (resolved via `resolve_geometry_handle_arg`
    /// from `named_steps`). Non-ValueRef/non-geometry args fall through to `None`
    /// (PRD §4 invariants #1/#2). Kernel Err/non-numeric already emits one Warning
    /// and returns `None` → mapped to `Some(Undef)` (invariant #3). Exactly one
    /// kernel query (invariant #4).
    ///
    /// NOTE: A dedicated `GeometryQuery::Intersects` variant + `ManifoldKernel::query()`
    /// wiring + `#kernel(manifold)` cross-kernel parity gate is KGQ-ο (Phase 5).
    /// This task ships the eval dispatch arm only; the Manifold standalone function
    /// ships alongside in `crates/reify-kernel-manifold/src/queries.rs`.
    ///
    /// KNOWN PARITY DIVERGENCE (KGQ-ο concern): The Manifold-side
    /// `queries::intersects` uses strict CSG non-emptiness rather than `d ≤ 0.0`.
    /// Two solids sharing only a coincident face (BRep distance = 0.0, zero shared
    /// volume) return `true` here but `false` in the Manifold function (empty
    /// CSG intersection mesh).  KGQ-ο must resolve canonical boundary semantics
    /// before enabling the parity gate.  See also the "Known parity divergence"
    /// section in `crates/reify-kernel-manifold/src/queries.rs::intersects`.
    Intersects,
    /// `split(solid, plane) -> List<Geometry>` — split a solid into pieces by
    /// an unbounded planar cutting tool (task 4190, PRD ζ).
    ///
    /// Backed by `GeometryKernel::execute_split` →
    /// `BRepAlgoAPI_Splitter` (OCCT kernel). A non-intersecting plane yields a
    /// length-1 list containing the original solid unchanged.
    ///
    /// args[0]: solid `Value::GeometryHandle` (resolved from `values` via
    ///   `resolve_parent_geometry_handle_arg`, providing the parent
    ///   realization_ref + hash for sub-handle construction).
    /// args[1]: cutting plane `Value::Plane` (resolved from `values`, decoded
    ///   via `decode_plane` into (origin, unit_normal)).
    ///
    /// Each result piece is stored as a `Value::GeometryHandle` sub-handle via
    /// `make_sub_handle` with `SubKind::Solid` (0x03) — domain-separated from
    /// edge (0x01) and face (0x02) hashes.  On kernel error emits a Warning
    /// diagnostic and returns `Some(Value::Undef)`.  Non-Plane args[1] or
    /// unhydrated args[0] fall through to `None`.
    Split,
    /// `union(a, b, …) -> Selector(k)` — variadic same-kind selector union (task
    /// 4119 δ).  All operands must already be `Value::Selector(k)` of the SAME
    /// kind (K1); reconstructed via `reconstruct_selector_value` and combined via
    /// `SelectorValue::union`.  Arity ≥ 2 (variadic; bypasses the fixed
    /// `expected_arity` gate — see arity guard in `try_eval_topology_selector`).
    /// On `SelectorError::KindMismatch` from the value-layer: Warning + Undef
    /// (defensive backstop; compile-time E_SELECTOR_KIND_MISMATCH should have
    /// already fired).
    Union,
    /// `intersect(a, b, …) -> Selector(k)` — variadic same-kind selector
    /// intersection (task 4119 δ).  Mirrors `Union` in construction;
    /// `SelectorValue::intersect` enforces K1. Arity ≥ 2.
    Intersect,
    /// `difference(a, b) -> Selector(k)` — binary same-kind selector difference
    /// (task 4119 δ).  Arity exactly 2; `SelectorValue::difference` enforces K1.
    Difference,
    /// `face(geometry, name) -> Selector(Face)` — Named-leaf FaceSelector ctor
    /// (task 4119 δ, PRD §11.1).  Arity 2: args[0] = parent geometry ValueRef,
    /// args[1] = name string Literal.  Builds `LeafQuery::Named(name)` with
    /// `SelectorKind::Face`.  Resolution is the D8 interim (W_TOPOLOGY_TAG_STALE
    /// + [] for any name until persistent-naming-v2 lands).
    Face,
    /// `edge(geometry, name) -> Selector(Edge)` — Named-leaf EdgeSelector ctor
    /// (task 4119 δ, PRD §11.1).  Arity 2: args[0] = parent geometry ValueRef,
    /// args[1] = name string Literal.  Builds `LeafQuery::Named(name)` with
    /// `SelectorKind::Edge`.
    Edge,
    /// `solid_body(geometry, name) -> Selector(Body)` — Named-leaf BodySelector
    /// ctor (task 4119 δ, PRD §11.1).  Arity 2.  `body(...)` is the RBD ctor
    /// (StructureRef("Mechanism")) — `solid_body` is the verified-free alternative.
    SolidBody,
    /// `vertices(geometry) -> Selector(Vertex)` — All-leaf VertexSelector ctor
    /// (task 4368).  Arity 1.  Builds a kernel-FREE `Value::Selector(Vertex)` with
    /// `LeafQuery::All` over the parent geometry handle (K2/BT7).  Mirrors
    /// `Faces` / `Edges` / `MidSurface` but for 0-manifold vertices.
    Vertices,
    /// `vertex(geometry, name) -> Selector(Vertex)` — Named-leaf VertexSelector
    /// ctor (task 4368).  Arity 2: args[0] = parent geometry ValueRef, args[1] =
    /// name string Literal.  Builds `LeafQuery::Named(name)` with
    /// `SelectorKind::Vertex`.  Zero kernel queries at construction time (K2/BT7).
    Vertex,
    // ── Task 3523: selector_vocabulary_v2 leaf-predicate ctors ───────────────
    /// `faces_perpendicular_to(geometry, Vec3, Angle) -> Selector(Face)` — faces
    /// whose normal is within `tol` of perpendicular to the axis. Arity 3,
    /// kernel-FREE; builds `LeafQuery::ByPerpendicular` (Face kind).
    FacesPerpendicularTo,
    /// `edges_perpendicular_to(geometry, Vec3, Angle) -> Selector(Edge)` — edges
    /// whose tangent is within `tol` of perpendicular to the axis. Arity 3,
    /// kernel-FREE; builds `LeafQuery::ByPerpendicular` (Edge kind).
    EdgesPerpendicularTo,
    /// `faces_by_surface_kind(geometry, name) -> Selector(Face)` — faces whose
    /// underlying surface is of the named kind ("Plane"/"Cylinder"/…). Arity 2,
    /// kernel-FREE; builds `LeafQuery::BySurfaceKind` (Face kind).
    FacesBySurfaceKind,
    /// `edges_by_curve_kind(geometry, name) -> Selector(Edge)` — edges whose
    /// underlying curve is of the named kind ("Line"/"Circle"/…). Arity 2,
    /// kernel-FREE; builds `LeafQuery::ByCurveKind` (Edge kind).
    EdgesByCurveKind,
    /// `extremal_by_bbox(geometry, axis, sense, Length) -> Selector(Face)` —
    /// face(s) extreme along `axis` ("X"/"Y"/"Z") with `sense` ("Max"/"Min") by
    /// AABB bound. Arity 4, kernel-FREE; builds `LeafQuery::ByExtremalBbox`.
    ExtremalByBbox,
    /// `extremal_by_centroid(geometry, axis, sense, Length) -> Selector(Face)` —
    /// face(s) extreme along `axis` with `sense` by centroid coordinate. Arity 4,
    /// kernel-FREE; builds `LeafQuery::ByExtremalCentroid`.
    ExtremalByCentroid,
}

impl TopologySelectorHelper {
    /// The exact number of arguments this helper takes. Used by the
    /// per-helper arity gate in `try_eval_topology_selector` before any
    /// arg-shape resolution runs — non-matching arities fall through to
    /// `None` so the cell stays at the `Value::Undef` left by `eval_expr`.
    fn expected_arity(self) -> usize {
        match self {
            TopologySelectorHelper::ClosestPoint
            | TopologySelectorHelper::IsOn
            | TopologySelectorHelper::AngleBetweenSurfaces
            | TopologySelectorHelper::CenterOfMass
            | TopologySelectorHelper::MomentOfInertia
            | TopologySelectorHelper::EdgesByLength
            | TopologySelectorHelper::FacesByArea
            | TopologySelectorHelper::AdjacentFaces
            | TopologySelectorHelper::SharedEdges
            // task #4759 — relational-walk v2 selectors (arity 2: parent + target)
            | TopologySelectorHelper::SiblingsOfFace
            | TopologySelectorHelper::AncestorFacesOfEdge
            | TopologySelectorHelper::Angle
            | TopologySelectorHelper::Contains
            | TopologySelectorHelper::Normal
            | TopologySelectorHelper::Curvature
            | TopologySelectorHelper::Distance
            | TopologySelectorHelper::Intersects
            | TopologySelectorHelper::Split
            // task 4119 δ: difference is binary; Union/Intersect are variadic
            // (≥ 2) but list 2 here as their minimum arity. The arity gate in
            // try_eval_topology_selector special-cases them to use a ≥2 check
            // rather than the exact equality check, so this value is not used
            // for the Union/Intersect path.
            | TopologySelectorHelper::Difference
            | TopologySelectorHelper::Union
            | TopologySelectorHelper::Intersect
            // task 4119 δ: Named-leaf ctors are arity 2 (geometry, name).
            | TopologySelectorHelper::Face
            | TopologySelectorHelper::Edge
            | TopologySelectorHelper::SolidBody
            // task 4368: Named-leaf vertex ctor is arity 2 (geometry, name).
            | TopologySelectorHelper::Vertex
            // task 3523: surface/curve-kind selectors are arity 2 (geometry, name).
            | TopologySelectorHelper::FacesBySurfaceKind
            | TopologySelectorHelper::EdgesByCurveKind
            // task 4831 (P3β): provenance leaf ctors are arity 2 (solid, f).
            | TopologySelectorHelper::CreatedByFeature
            | TopologySelectorHelper::SplitByFeature => 2,
            TopologySelectorHelper::Edges
            | TopologySelectorHelper::Faces
            | TopologySelectorHelper::MidSurface
            | TopologySelectorHelper::Length
            | TopologySelectorHelper::Perimeter
            // task 4368: All-leaf vertex ctor is arity 1 (geometry).
            | TopologySelectorHelper::Vertices => 1,
            TopologySelectorHelper::FacesByNormal
            | TopologySelectorHelper::EdgesParallelTo
            | TopologySelectorHelper::EdgesAtHeight
            | TopologySelectorHelper::GeoEquiv
            // task 3523: perpendicular selectors share the (solid, dir, tol) shape.
            | TopologySelectorHelper::FacesPerpendicularTo
            | TopologySelectorHelper::EdgesPerpendicularTo => 3,
            // task 3523: extremal selectors are arity 4 (solid, axis, sense, tol).
            TopologySelectorHelper::ExtremalByBbox
            | TopologySelectorHelper::ExtremalByCentroid => 4,
        }
    }
}

/// Issue `extract_edges` (or `extract_faces`, per `sub_kind`) for the given
/// parent kernel handle and return a `Value::List` of `Value::GeometryHandle`
/// sub-handles (PRD §4). Each element carries:
/// - `realization_ref` — cloned from the parent (unchanged per PRD §4).
/// - `upstream_values_hash` — `compose_sub_handle_hash(parent_hash, sub_kind, index)`.
/// - `kernel_handle` — the kernel id returned by `extract_edges`/`extract_faces`.
///
/// Returns `Some(Value::Undef)` (with a Warning diagnostic) on kernel error —
/// preserving the same defensive-downgrade contract as the sibling dispatchers
/// (`dispatch_point3_length_reply`, `dispatch_point_on_shape`, etc.).
// Task 4118 (γ): the `edges`/`faces` All-leaf construction path is now
// kernel-FREE (see `build_leaf_selector`), so eager sub-shape extraction has no
// caller at construction time. Retained (allow dead_code) — the kernel-bearing
// `ResolveSelector` resolution path (step-6) re-realizes selectors and may reuse
// this eager-extraction shape; remove if it stays unused.
#[allow(dead_code)]
fn dispatch_extract_subshapes(
    kernel: &mut dyn reify_ir::GeometryKernel,
    parent_kernel_handle: GeometryHandleId,
    sub_kind: crate::topology_selectors::SubKind,
    parent_realization_ref: &reify_core::identity::RealizationNodeId,
    parent_hash: &[u8; 32],
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let result = match sub_kind {
        crate::topology_selectors::SubKind::Edge => kernel.extract_edges(parent_kernel_handle),
        crate::topology_selectors::SubKind::Face => kernel.extract_faces(parent_kernel_handle),
        crate::topology_selectors::SubKind::Vertex => kernel.extract_vertices(parent_kernel_handle),
        // SubKind::Solid is only used by the Split dispatch arm, which calls
        // execute_split directly and does NOT go through dispatch_extract_subshapes.
        crate::topology_selectors::SubKind::Solid => {
            unreachable!(
                "dispatch_extract_subshapes called with SubKind::Solid — \
                 split pieces are produced via execute_split in the Split arm, \
                 not through the extract-subshapes path"
            )
        }
    };
    match result {
        Ok(sub_ids) => {
            let elements = sub_ids
                .into_iter()
                .enumerate()
                .map(|(i, sub_kernel_id)| {
                    crate::topology_selectors::make_sub_handle(
                        parent_realization_ref,
                        parent_hash,
                        sub_kind,
                        i as u32,
                        sub_kernel_id,
                    )
                })
                .collect();
            Some(reify_ir::Value::List(elements))
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{}({:?}): kernel error: {}",
                helper_name, parent_kernel_handle, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Short label for a `Value` that failed `Point<Length>` classification, used
/// as the `got` field of the rejection diagnostic (task ε). A `Value::Point` is
/// distinguished by wrong arity vs. carrying a wrong-dimension / non-Scalar
/// component, so the Warning names what actually went wrong; any other value is
/// labelled by its kind.
fn point3_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Point(items) if items.len() != 3 => {
            format!("Point of {} components", items.len())
        }
        reify_ir::Value::Point(_) => "Point with a non-Length or non-Scalar component".to_string(),
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        _ => "non-Point value".to_string(),
    }
}

/// Resolve a 3-component point arg to its `[f64; 3]` SI-metre components,
/// emitting a `Severity::Warning` when the caller passes a defined-but-wrong
/// value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::Point` cell (the common let-bound `let p = point3(x, y, z)` form)
/// reads the cell (now an owned clone; see [`eval_arg_value`]) — functionally
/// identical to the prior `values.get(id)` path — while an inline point
/// expression now EVALUATES rather than falling through to a silent `None`. The value must be a `Value::Point` of exactly three
/// LENGTH-dimensioned `Value::Scalar` components: the cell type is fixed at
/// `Type::Point<Length>` by the compile-time wiring in `expr.rs`, so a
/// well-formed Scalar component MUST carry `DimensionVector::LENGTH` — a
/// wrong-dimensioned Scalar slipping through would be silently reinterpreted as
/// metres at the kernel boundary, so a `debug_assert` surfaces the violation in
/// tests; in release we fall through to the rejection path rather than feed the
/// kernel garbage.
///
/// | evaluated arg value                                   | return       | diagnostic?     |
/// |-------------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)      | `None`       | no — quiet      |
/// | `Value::Point` of 3 LENGTH `Scalar`s                  | `Some([..])` | no              |
/// | non-Point, wrong arity, or non-LENGTH/non-Scalar comp | `None`       | yes — 1 Warning |
fn resolve_point3_length_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 3]> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic — behaviourally
    // identical to the prior `values.get(id)?` fall-through for a missing cell.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    // A Value::Point of exactly three LENGTH-dimensioned Scalars resolves to its
    // SI-metre components (the `debug_assert` + LENGTH check is preserved from
    // the prior ValueRef path).
    let as_point3 = |v: &reify_ir::Value| -> Option<[f64; 3]> {
        let components = match v {
            reify_ir::Value::Point(items) if items.len() == 3 => items,
            _ => return None,
        };
        let mut out = [0.0_f64; 3];
        for (i, comp) in components.iter().enumerate() {
            match comp {
                reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    debug_assert!(
                        *dimension == reify_core::DimensionVector::LENGTH,
                        "resolve_point3_length_arg: expected LENGTH-dimensioned Scalar, \
                         got dimension {:?} (si_value={}); cell type is Point<Length> per \
                         compile-time wiring in expr.rs",
                        dimension,
                        si_value
                    );
                    if *dimension != reify_core::DimensionVector::LENGTH {
                        return None;
                    }
                    out[i] = *si_value;
                }
                _ => return None,
            }
        }
        Some(out)
    };
    if let Some(components) = as_point3(&value) {
        return Some(components);
    }

    // Defined-but-wrong (non-Point, wrong arity, or non-LENGTH/non-Scalar
    // component): emit exactly one Warning naming builtin/arg/Point<Length>/got
    // (byte-uniform wording with the density / vec3 / range paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: point3_got_label(&value),
            expected: "Point<Length>",
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Evaluate an argument `CompiledExpr` against the `ValueMap` with a LOCAL
/// context (no user-defined functions, no meta block) — the evaluate-then-accept
/// mechanism shared by the task ε (4492) owned-arg resolvers.
///
/// A `ValueRef` resolves via `get_or_undef`, preserving quiet degradation for a
/// missing or `Value::Undef` cell. The resulting `Value` is *behaviourally*
/// identical to what the prior `values.get(id)` shape-match produced, with one
/// nuance: `eval_expr` returns an OWNED `Value` (`get_or_undef` clones the cell)
/// where the prior path borrowed, so a Point/Vector/Range cell now incurs one
/// `Vec<Scalar>` clone per resolve. The cost is negligible against the kernel
/// round-trips these dispatchers perform. Inline literals, field-access, and
/// range/vector/arithmetic constructors now EVALUATE rather than falling through
/// to a silent `None`.
///
/// User-defined-function-call / meta-block args in these positions evaluate to
/// `Value::Undef` → quiet `None`, consistent with the degradation contract: the
/// selector/kinematic/ad-hoc dispatch fns that own these args do not carry
/// `functions`/`meta_map` (only `compile_geometry_op` does), so the local
/// `EvalContext::new(values, &[])` is faithful to PRD decision 10's load-bearing
/// intent ("evaluate the arg expr against the `ValueMap`"). See task ε design
/// decision 1.
///
/// The local `EvalContext` carries no diagnostics sink (`diagnostics: None`), so
/// any RUNTIME diagnostic `eval_expr` might emit while evaluating an inline arg
/// expression (e.g. a field-OOB or undef-builtin warning) is intentionally
/// dropped here — the v0.1 arg shapes these resolvers accept (scalars, points,
/// vec3, ranges, strings, ints) do not trigger such diagnostics. A future arg
/// form that did would need a `with_runtime_diagnostics` sink drained into the
/// caller's `diagnostics` vec.
fn eval_arg_value(expr: &reify_ir::CompiledExpr, values: &reify_ir::ValueMap) -> reify_ir::Value {
    reify_expr::eval_expr(expr, &reify_expr::EvalContext::new(values, &[]))
}

/// Resolve the `density` argument of `center_of_mass` and `moment_of_inertia`
/// to a raw `f64` (SI kg/m³), emitting a `Severity::Warning` when the caller
/// passes a defined-but-wrong type.
///
/// Contract A (task 4486 γ) + evaluate-then-accept (task 4492 ε): the arg expr
/// is EVALUATED against `values` (via [`eval_arg_value`]) and the resulting
/// `Value` classified by [`crate::arg_acceptance::accept_arg`] with
/// [`crate::arg_acceptance::density_spec`]. Inline / computed density
/// expressions (e.g. `moment_of_inertia(b, 7850kg/m^3)`) now WORK — the γ
/// "must be bound to a let / not yet supported" fall-through is gone.
///
/// | evaluated arg value                           | return       | diagnostic pushed?        |
/// |-----------------------------------------------|--------------|---------------------------|
/// | `Value::Undef` (missing/Undef cell, or a      | `None`       | no — quiet degradation    |
/// |   user-fn/meta arg the local ctx can't eval)  |              |                           |
/// | `Value::Scalar{MASS_DENSITY,v}` (inline lit,  | `Some(v)`    | no                        |
/// |   `ValueRef`, field-access, or arithmetic)    |              |                           |
/// | bare `Value::Real`, dimensionless/wrong-dim   | `None`       | yes — `Severity::Warning` |
/// |   `Value::Scalar`, or any non-numeric `Value` |              | naming `density` + hint   |
fn resolve_density_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    use crate::arg_acceptance::{Acceptance, accept_arg, density_spec};

    let value = eval_arg_value(expr, values);

    match accept_arg(&value, &density_spec()) {
        Acceptance::Accepted(si) => Some(si),
        Acceptance::Undefined => None,
        Acceptance::Rejected(rej) => {
            diagnostics.push(Diagnostic::warning(rej.message(helper_name, "density")));
            None
        }
    }
}

/// Read a single Vec3 component: a `Value::Real` or a dimensionless
/// `Value::Scalar`. Returns `None` for any dimensioned Scalar or non-numeric
/// payload — the direction/axis args of `faces_by_normal` / `edges_parallel_to`
/// are pure unit-vector numerics in v0.1.
fn vec3_component_si(value: &reify_ir::Value) -> Option<f64> {
    match value {
        reify_ir::Value::Real(v) => Some(*v),
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == reify_core::DimensionVector::DIMENSIONLESS => Some(*si_value),
        _ => None,
    }
}

/// Short human-readable label for a `Value` that failed Vec3 classification,
/// used as the `got` field of the rejection diagnostic (task ε). Vec3-aware: a
/// `Value::Vector` of the wrong arity or carrying a dimensioned / non-numeric
/// component is distinguished from a plainly non-Vector value, so the Warning
/// names what actually went wrong.
fn vec3_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Vector(items) if items.len() != 3 => {
            format!("Vector of {} components", items.len())
        }
        reify_ir::Value::Vector(_) => {
            "Vector with a dimensioned or non-numeric component".to_string()
        }
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Point(_) => "Point".to_string(),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        _ => "non-Vec3 value".to_string(),
    }
}

/// Resolve a 3-component vector arg to its `[f64; 3]` SI components, emitting a
/// `Severity::Warning` when the caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. Inline
/// `vec3(...)` constructor calls now WORK — `eval_expr` lowers the `vec3(...)`
/// `FunctionCall` to a `Value::Vector` via `reify_stdlib::eval_builtin`, so the
/// γ "Literal/`ValueRef` shape-match only → silent fall-through" behaviour is
/// gone. Each component must still be a `Value::Real` or a dimensionless
/// `Value::Scalar` (per [`vec3_component_si`]); the vector must have exactly
/// three components — the direction/axis args of `faces_by_normal` /
/// `edges_parallel_to` / `angle` are pure unit-vector numerics in v0.1.
///
/// | evaluated arg value                                | return       | diagnostic?     |
/// |----------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)   | `None`       | no — quiet      |
/// | `Value::Vector` of 3 `Real`/dimensionless `Scalar` | `Some([..])` | no              |
/// | non-Vector, wrong length, or dimensioned component | `None`       | yes — 1 Warning |
fn resolve_vec3_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 3]> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic — behaviourally
    // identical to the γ fall-through for missing cells.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    let as_vec3 = |v: &reify_ir::Value| -> Option<[f64; 3]> {
        match v {
            reify_ir::Value::Vector(items) if items.len() == 3 => Some([
                vec3_component_si(&items[0])?,
                vec3_component_si(&items[1])?,
                vec3_component_si(&items[2])?,
            ]),
            _ => None,
        }
    };
    if let Some(components) = as_vec3(&value) {
        return Some(components);
    }

    // Defined-but-wrong: emit exactly one Warning naming builtin/arg/Vec3/got
    // (byte-uniform wording with the density / scalar-bound paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: vec3_got_label(&value),
            expected: "Vec3",
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Shared evaluate-then-accept core for the SCALAR-dimension owned args
/// (task ε): EVALUATE `expr` against `values` (via [`eval_arg_value`]) and
/// classify the resulting `Value` against an inline
/// [`crate::arg_acceptance::ArgSpec`] of `expected_dim`. `Value::Undef`
/// degrades quietly to `None`; a defined-but-wrong value pushes exactly one
/// `Severity::Warning` (built from the rejection + `builtin`/`arg_name` labels,
/// byte-uniform with the density path) and returns `None`.
fn resolve_scalar_dim_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
    type_name: &'static str,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    use crate::arg_acceptance::{Acceptance, ArgSpec, accept_arg};

    let value = eval_arg_value(expr, values);
    let spec = ArgSpec {
        type_name,
        dimension: expected_dim,
        migration_hint: None,
    };
    match accept_arg(&value, &spec) {
        Acceptance::Accepted(si) => Some(si),
        Acceptance::Undefined => None,
        Acceptance::Rejected(rej) => {
            diagnostics.push(Diagnostic::warning(rej.message(builtin, arg_name)));
            None
        }
    }
}

/// Resolve an ANGLE-dimensioned scalar arg to its SI value (radians).
/// EVALUATES the arg expr (task ε): an inline dimensioned-angle literal, a
/// `ValueRef → ANGLE Scalar` (let-bound `let tol = 1deg`), or an angle-typed
/// arithmetic expression all WORK. A `Value::Undef` (missing cell, etc.)
/// degrades quietly; a defined-but-wrong value (wrong dimension, non-Scalar)
/// pushes exactly one `Severity::Warning` naming `builtin`/`arg_name`. Pins the
/// ANGLE dimension for the angular-tolerance args of `faces_by_normal` /
/// `edges_parallel_to`.
fn resolve_angle_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    resolve_scalar_dim_arg(
        expr,
        values,
        reify_core::DimensionVector::ANGLE,
        "Angle",
        builtin,
        arg_name,
        diagnostics,
    )
}

/// Resolve a LENGTH-dimensioned scalar arg to its SI value (metres).
/// EVALUATES the arg expr (task ε): an inline dimensioned-length literal, a
/// `ValueRef → LENGTH Scalar` (let-bound `let z = 0mm`), or a length-typed
/// arithmetic expression all WORK. A `Value::Undef` degrades quietly; a
/// defined-but-wrong value pushes exactly one `Severity::Warning` naming
/// `builtin`/`arg_name`. Pins the LENGTH dimension for the z-plane / tolerance
/// args of `edges_at_height` and the tolerance arg of `geo_equiv`.
fn resolve_length_scalar_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    resolve_scalar_dim_arg(
        expr,
        values,
        reify_core::DimensionVector::LENGTH,
        "Length",
        builtin,
        arg_name,
        diagnostics,
    )
}

/// Read a `Value::Scalar` whose `dimension` is `expected_dim` and return its
/// SI value. `None` for any other shape (wrong dimension, non-Scalar).
fn scalar_si_with_dim(
    value: &reify_ir::Value,
    expected_dim: reify_core::DimensionVector,
) -> Option<f64> {
    match value {
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } if *dimension == expected_dim => Some(*si_value),
        _ => None,
    }
}

/// Human-readable expected-type label for a `Range<dim>` rejection diagnostic
/// (task ε). The two real callers pin `LENGTH` (`edges_by_length`) and `AREA`
/// (`faces_by_area`); any other dimension degrades to a bare `"Range"`.
fn range_expected_label(expected_dim: reify_core::DimensionVector) -> &'static str {
    if expected_dim == reify_core::DimensionVector::LENGTH {
        "Range<Length>"
    } else if expected_dim == reify_core::DimensionVector::AREA {
        "Range<Area>"
    } else {
        "Range"
    }
}

/// Short label for a `Value` that failed `Range<dim>` classification, used as
/// the `got` field of the rejection diagnostic (task ε). A `Value::Range` is
/// distinguished as half-open (one bound `None`) vs. carrying a wrong-dimension
/// / non-Scalar bound, so the Warning names what actually went wrong; any other
/// value is labelled by its kind.
fn range_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Range { lower, upper, .. } if lower.is_none() || upper.is_none() => {
            "half-open Range".to_string()
        }
        reify_ir::Value::Range { .. } => {
            "Range with a wrong-dimension or non-Scalar bound".to_string()
        }
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        _ => "non-Range value".to_string(),
    }
}

/// Resolve a `Range<Quantity>` arg to its `(lower_si, upper_si)` SI bounds,
/// both dimensioned `expected_dim`, emitting a `Severity::Warning` when the
/// caller passes a defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. `eval_expr`
/// lowers an inline `RangeConstructor` — including one with computed bounds
/// such as `0mm..(20mm + 30mm)` — to a `Value::Range`, and a `ValueRef →
/// Value::Range` (the common let-bound `let r = 0mm..50mm` form) reads the
/// cell; so the former Literal/ValueRef/RangeConstructor shape-match COLLAPSES
/// into one `Value::Range` classification, and the γ "inline computed bound →
/// silent fall-through" behaviour is gone. Both bounds must be present (a
/// half-open range is rejected — the v0.1 filtered selectors require a closed
/// `[lo, hi]` window) and dimensioned `expected_dim`.
///
/// | evaluated arg value                                 | return       | diagnostic?     |
/// |-----------------------------------------------------|--------------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg)    | `None`       | no — quiet      |
/// | closed `Value::Range` of two `expected_dim` Scalars | `Some((..))` | no              |
/// | non-Range, half-open, or wrong-dimension bound      | `None`       | yes — 1 Warning |
fn resolve_range_dim_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    expected_dim: reify_core::DimensionVector,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(f64, f64)> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg the
    // local ctx can't evaluate) returns None with no diagnostic.
    if matches!(value, reify_ir::Value::Undef) {
        return None;
    }

    // A closed Range of two `expected_dim` Scalars resolves to its SI bounds.
    if let reify_ir::Value::Range {
        lower: Some(lo),
        upper: Some(hi),
        ..
    } = &value
        && let (Some(lo_si), Some(hi_si)) = (
            scalar_si_with_dim(lo, expected_dim),
            scalar_si_with_dim(hi, expected_dim),
        )
    {
        return Some((lo_si, hi_si));
    }

    // Defined-but-wrong (non-Range, half-open, or wrong-dimension bound): emit
    // exactly one Warning naming builtin/arg/Range<dim>/got (byte-uniform
    // wording with the density / vec3 / scalar-bound paths).
    diagnostics.push(Diagnostic::warning(
        ArgRejection {
            got: range_got_label(&value),
            expected: range_expected_label(expected_dim),
            migration_hint: None,
        }
        .message(builtin, arg_name),
    ));
    None
}

/// Scan `values` for the `Value::GeometryHandle` whose `kernel_handle ==
/// parent_body_kh` and return its `(realization_ref, upstream_values_hash)`.
///
/// Used by `dispatch_shared_edges` to recover the parent solid's hash for edge
/// sub-handle construction (PRD §4 cache coherence): edge sub-handles must
/// compose from the parent solid's `upstream_values_hash`, not from a face
/// sub-handle's hash. The parent solid cell is hydrated into `values` by
/// `post_process_geometry_handle_cells` (engine_build.rs:3693-3700).
///
/// Returns `None` when no matching cell is found (e.g. unnamed inline solid),
/// causing the caller to fall through per PRD invariant #2 (never
/// partial-construct sub-handles from a non-hydrated geometry cell).
///
/// # Uniqueness assumption
/// Kernel handles are unique per shape within a session (PRD §4 intra-session
/// handle persistence), so at most one `Value::GeometryHandle` in `values`
/// carries any given `kernel_handle`. The linear scan returns on the first
/// match; that match is expected to be the only one.
fn resolve_owner_solid_handle(
    values: &reify_ir::ValueMap,
    parent_body_kh: GeometryHandleId,
) -> Option<(reify_core::identity::RealizationNodeId, [u8; 32])> {
    for (_, value) in values.iter() {
        if let reify_ir::Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } = value
            && *kernel_handle == Some(parent_body_kh)
        {
            return Some((realization_ref.clone(), *upstream_values_hash));
        }
    }
    None
}

/// Resolve a geometry-handle arg to a `GeometryHandleId` via `named_steps`.
///
/// Matches three structural shapes — never evaluating the arg (the ordering
/// invariant esc-4358-124: a geometry-query leaf must reduce to a `Literal`
/// structurally, before any `eval_expr` pass):
///
/// * `CompiledExprKind::ValueRef(id)` / `CrossSubGeometryRef(id)` — the
///   established OR-pattern convention (reify-ir/src/expr.rs). The `self.<sub>`
///   cross-sub `proc.build_volume` arg resolves whether it lowered to a
///   forward-declared scoped `ValueRef` or a genuine-realization
///   `CrossSubGeometryRef`. A cross-sub handle carries a scoped
///   `<parent>.<sub>` entity stamp, and `seed_cross_sub_named_steps` keys it in
///   `named_steps` by the composed `"<sub>.<member>"` key (engine_build.rs), so
///   a dotted entity looks up that composed key; a plain same-template ref
///   (dot-free entity) keeps the bare-member lookup.
/// * `CompiledExprKind::IndexAccess { object: ValueRef(proc), index:
///   Literal("build_volume") }` — the cross-`let` structure-instance member
///   access shape: `let proc = FdmPrinter()` is a `StructureRef`-typed value
///   cell (NOT a `sub`) whose `.member` projection lowers to `IndexAccess` via
///   SIR-α field projection (reify-compiler/src/expr.rs). Compose the same
///   `"<binding>.<member>"` key that the cross-`let` seeding in
///   `check_constraints_post_geometry` stamps — `<binding>` is the object
///   ValueRef's bare member (the `proc` binding name), `<member>` is the
///   string-literal index (task 4358 ε step-10).
///
/// Returns `None` for any other expr shape or a missing `named_steps` entry —
/// caller maps to the "unsupported arg shape → fall through" behaviour.
fn resolve_geometry_handle_arg(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
) -> Option<GeometryHandleId> {
    let key = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id)
        | reify_ir::CompiledExprKind::CrossSubGeometryRef(id) => {
            // `rsplit_once('.')` is exactly "if entity contains '.', take the
            // last segment as the sub name": `Some((_, sub))` ⟺ dotted entity,
            // `sub` = everything after the final '.'
            // (== `entity.rsplit('.').next().unwrap()`).
            match id.entity.rsplit_once('.') {
                Some((_, sub)) => format!("{}.{}", sub, id.member),
                None => id.member.clone(),
            }
        }
        reify_ir::CompiledExprKind::IndexAccess { object, index } => {
            let (reify_ir::CompiledExprKind::ValueRef(obj_id)
            | reify_ir::CompiledExprKind::CrossSubGeometryRef(obj_id)) = &object.kind
            else {
                return None;
            };
            let reify_ir::CompiledExprKind::Literal(reify_ir::Value::String(member)) = &index.kind
            else {
                return None;
            };
            format!("{}.{}", obj_id.member, member)
        }
        _ => return None,
    };
    named_steps.get(&key).map(|kh| kh.id)
}

/// Resolve a `CompiledExprKind::ValueRef` arg to the full parent
/// `Value::GeometryHandle` fields: `(realization_ref, upstream_values_hash,
/// kernel_handle)`. Returns `None` for any non-`ValueRef` shape, a missing
/// cell, or a cell that is not a `Value::GeometryHandle` — the caller falls
/// through, leaving the selector cell at its compiled default (`Value::Undef`).
///
/// PRD §4 invariant #2: sub-handles must never be partially constructed from
/// a non-hydrated geometry cell. This gate enforces that contract at the
/// dispatch boundary.
fn resolve_parent_geometry_handle_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
) -> Option<(
    reify_core::identity::RealizationNodeId,
    [u8; 32],
    GeometryHandleId,
)> {
    let cell_id = match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => id,
        _ => return None,
    };
    match values.get(cell_id)? {
        reify_ir::Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => kernel_handle.map(|kh| (realization_ref.clone(), *upstream_values_hash, kh)),
        _ => None,
    }
}

/// Issue a query whose kernel reply is the canonical JSON-Point3
/// (`{"x":_,"y":_,"z":_}`) wire format and unwrap to a
/// `Value::Point(vec![length, length, length])`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// malformed reply. Shared by `closest_point` (`ClosestPointOnShape`),
/// `center_of_mass` (`CenterOfMass`), and the whole-handle `centroid`
/// (`Centroid`, task 3608) — all return the identical JSON-Point3 encoding per
/// the `GeometryQuery` doc, so a single decode path serves them.
///
/// Takes `&dyn` (not `&mut dyn`): `GeometryKernel::query` is `&self`, so an
/// immutable borrow suffices, and `&mut dyn` call sites reborrow to `&dyn`
/// automatically.
fn dispatch_point3_length_reply(
    kernel: &dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, helper_name) {
            Ok([x, y, z]) => Some(reify_ir::Value::Point(vec![
                reify_ir::Value::length(x),
                reify_ir::Value::length(y),
                reify_ir::Value::length(z),
            ])),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel reply parse failed: {}",
                    helper_name, err
                )));
                Some(reify_ir::Value::Undef)
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `FaceNormalAt` query and unwrap the kernel's JSON-Point3 reply
/// (`{"x":_,"y":_,"z":_}`) into a dimensionless `Value::Vector([Real, Real,
/// Real])` — the canonical unit normal representation (matches `vec3()` output
/// and `resolve_vec3_arg` expectations).
///
/// Models the same defensive-downgrade contract as `dispatch_point3_length_reply`:
/// - Kernel `Err` → one Warning ("`normal` kernel query failed: …") + `Some(Value::Undef)`
/// - Malformed reply (non-String or bad JSON) → one Warning ("`normal` kernel reply parse
///   failed: …") + `Some(Value::Undef)`
///
/// Powers `TopologySelectorHelper::Normal` (task 3615, KGQ-ζ).
fn dispatch_normal_vector3(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, helper_name) {
            Ok([x, y, z]) => Some(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(x),
                reify_ir::Value::Real(y),
                reify_ir::Value::Real(z),
            ])),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel reply parse failed: {}",
                    helper_name, err
                )));
                Some(reify_ir::Value::Undef)
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Curvature dimension constant: 1/Length = Length^-1 (m⁻¹ in SI).
/// Used by `dispatch_curvature` to tag scalar and matrix cells.
/// The curvature dimension is LENGTH^-1. index 0 = LENGTH in DimensionVector.
const CURVATURE_DIM: reify_core::dimension::DimensionVector = {
    let mut d = [reify_core::dimension::Rational::ZERO; 10];
    d[0] = reify_core::dimension::Rational::new(-1, 1);
    reify_core::dimension::DimensionVector(d)
};

/// Dispatch a `length(curve)` query for `TopologySelectorHelper::Length`
/// (task 3622, KGQ-ν).
///
/// Issues `GeometryQuery::EdgeLength(handle)` and wraps the reply as
/// `Value::length(metres)`. Returns `Some(Value::Undef)` + one Warning on
/// Err or an unexpected kernel reply type (PRD §4 defensive-downgrade contract).
fn dispatch_edge_length(
    kernel: &mut dyn reify_ir::GeometryKernel,
    handle: reify_ir::GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(&reify_ir::GeometryQuery::EdgeLength(handle)) {
        Ok(reify_ir::Value::Real(l)) => Some(reify_ir::Value::length(l)),
        Ok(reify_ir::Value::Scalar { si_value, .. }) => Some(reify_ir::Value::length(si_value)),
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply has unexpected type (expected Real, got {unexpected:?}); \
                 cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Dispatch a `perimeter(surface)` query for `TopologySelectorHelper::Perimeter`
/// (task 3622, KGQ-ν).
///
/// Composes `kernel.extract_edges(face_kh)` + per-edge `EdgeLength`. On any
/// extract error or per-edge non-Real reply, emits exactly one Warning and
/// returns `Some(Value::Undef)` (PRD §4 defensive-downgrade contract).
fn dispatch_perimeter(
    kernel: &mut dyn reify_ir::GeometryKernel,
    face_kh: reify_ir::GeometryHandleId,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let edges = match kernel.extract_edges(face_kh) {
        Ok(e) => e,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} extract_edges failed: {err}",
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    // Degenerate face: no boundary edges → silent 0.0 would mask a real kernel
    // problem; downgrade like the other failure modes instead.
    if edges.is_empty() {
        diagnostics.push(Diagnostic::warning(format!(
            "{helper_name} extract_edges returned no boundary edges for face \
             {face_kh:?}; degenerate geometry; cell left at Undef",
        )));
        return Some(reify_ir::Value::Undef);
    }
    let mut total_m = 0.0_f64;
    for edge_id in &edges {
        match kernel.query(&reify_ir::GeometryQuery::EdgeLength(*edge_id)) {
            Ok(reify_ir::Value::Real(l)) => total_m += l,
            Ok(reify_ir::Value::Scalar { si_value, .. }) => total_m += si_value,
            Ok(unexpected) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} EdgeLength for edge {edge_id:?} has unexpected type \
                     (expected Real, got {unexpected:?}); cell left at Undef",
                )));
                return Some(reify_ir::Value::Undef);
            }
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} EdgeLength for edge {edge_id:?} failed: {err}",
                )));
                return Some(reify_ir::Value::Undef);
            }
        }
    }
    Some(reify_ir::Value::length(total_m))
}

/// Dispatch a `curvature(shape, point)` query for `TopologySelectorHelper::Curvature`
/// (task 3621, KGQ-μ).
///
/// Strategy: try `SurfaceCurvatureAt{handle, u=px, v=py}` first. If the kernel
/// returns Ok, decode the `[[κ_max,0],[0,κ_min]]` nested-List wire value into a
/// `Value::Matrix` of `Value::Scalar{si_value, dimension: 1/Length}` cells. If
/// the kernel returns Err, retry as `CurveCurvatureAt{handle,px,py,pz}` and
/// return `Value::Scalar{si_value: κ, dimension: 1/Length}`. If both fail, emit
/// exactly one Warning naming `helper_name` and return `Some(Value::Undef)`.
///
/// The surface wire note: the kernel encodes the principal-curvature matrix as a
/// diagonal `[[kappa_max, 0.0], [0.0, kappa_min]]` (InertiaTensor wire convention)
/// so trace/2 = mean curvature H and det = Gaussian curvature K.
fn dispatch_curvature(
    kernel: &mut dyn reify_ir::GeometryKernel,
    handle: reify_ir::GeometryHandleId,
    point: [f64; 3],
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // Try surface first: (px, py) → (u, v) per design decision §3.
    let surface_query = reify_ir::GeometryQuery::SurfaceCurvatureAt {
        handle,
        u: point[0],
        v: point[1],
    };
    if let Ok(value) = kernel.query(&surface_query) {
        return Some(parse_curvature_matrix_reply(
            &value,
            helper_name,
            diagnostics,
        ));
    }
    // Err(_): fall through to curve form

    // Retry as curve: full 3D world point.
    let curve_query = reify_ir::GeometryQuery::CurveCurvatureAt {
        handle,
        px: point[0],
        py: point[1],
        pz: point[2],
    };
    match kernel.query(&curve_query) {
        Ok(reify_ir::Value::Real(kappa)) => Some(reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        }),
        Ok(unexpected) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply has unexpected type (expected Real for curve curvature, \
                 got {unexpected:?}); cell left at Undef",
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel query failed: {err}",
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Decode the kernel's `[[κ_max, 0.0], [0.0, κ_min]]` nested-List reply into a
/// `Value::Matrix` of `Value::Scalar{si_value, dimension: 1/Length}` cells.
///
/// On any parse failure emits a Warning and returns `Value::Undef` (same
/// defensive-downgrade contract as `dispatch_normal_vector3`).
fn parse_curvature_matrix_reply(
    value: &reify_ir::Value,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let rows = match value {
        reify_ir::Value::List(rows) if rows.len() == 2 => rows,
        other => {
            diagnostics.push(Diagnostic::warning(format!(
                "{helper_name} kernel reply parse failed: expected 2-row List, got {other:?}",
            )));
            return reify_ir::Value::Undef;
        }
    };
    let mut matrix_rows: Vec<Vec<reify_ir::Value>> = Vec::with_capacity(2);
    for (i, row) in rows.iter().enumerate() {
        let cells = match row {
            reify_ir::Value::List(cells) if cells.len() == 2 => cells,
            other => {
                diagnostics.push(Diagnostic::warning(format!(
                    "{helper_name} kernel reply parse failed: row {i} is not a 2-element List, \
                     got {other:?}",
                )));
                return reify_ir::Value::Undef;
            }
        };
        let mut matrix_row: Vec<reify_ir::Value> = Vec::with_capacity(2);
        for (j, cell) in cells.iter().enumerate() {
            let si_value = match cell {
                reify_ir::Value::Real(v) => *v,
                other => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "{helper_name} kernel reply parse failed: cell [{i}][{j}] is not Real, \
                         got {other:?}",
                    )));
                    return reify_ir::Value::Undef;
                }
            };
            matrix_row.push(reify_ir::Value::Scalar {
                si_value,
                dimension: CURVATURE_DIM,
            });
        }
        matrix_rows.push(matrix_row);
    }
    reify_ir::Value::Matrix(matrix_rows)
}

/// Issue an `InertiaTensor` query and re-wrap the kernel's row-of-row
/// `Value::List` reply into a nested `Value::Tensor(rows_of_tensors)` where
/// each element is a `Value::Scalar { si_value, dimension: MOMENT_OF_INERTIA }`.
///
/// The kernel returns raw dimensionless `Value::Real` cell values
/// (`[[m11,m12,m13],[m21,m22,m23],[m31,m32,m33]]`) because
/// `GeometryQuery::InertiaTensor` predates the dimensioned-Scalar wrap; the
/// eval-side owns the MomentOfInertia (kg·m²) tagging so the result matches
/// the compile-time `Tensor<2,3,MomentOfInertia>` cell type from
/// `topology_selector_result_type`.
///
/// Returns `Some(Value::Undef)` (with a Warning diagnostic) on a kernel
/// error or any malformed shape (non-List reply, non-List row, non-numeric
/// element). Same defensive-downgrade contract as
/// `dispatch_point3_length_reply`.
fn dispatch_inertia_tensor(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    let malformed = |diagnostics: &mut Vec<Diagnostic>, detail: String| {
        diagnostics.push(Diagnostic::warning(format!(
            "{} kernel reply malformed: {}",
            helper_name, detail
        )));
        Some(reify_ir::Value::Undef)
    };
    let reply = match kernel.query(query) {
        Ok(v) => v,
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            return Some(reify_ir::Value::Undef);
        }
    };
    let rows = match &reply {
        reify_ir::Value::List(rows) => rows,
        other => {
            return malformed(
                diagnostics,
                format!("expected Value::List, got {:?}", other),
            );
        }
    };
    let mut tensor_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let cols = match row {
            reify_ir::Value::List(cols) => cols,
            other => {
                return malformed(
                    diagnostics,
                    format!("expected Value::List row, got {:?}", other),
                );
            }
        };
        let mut tensor_cols = Vec::with_capacity(cols.len());
        for col in cols {
            // The kernel emits dimensionless Value::Real; accept a
            // dimensionless Scalar too so the dispatch stays kernel-
            // implementation agnostic (mirrors kernel_distance's
            // Real|Scalar leniency).
            let si = match col {
                reify_ir::Value::Real(v) => *v,
                reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                } if *dimension == reify_core::DimensionVector::DIMENSIONLESS
                    || *dimension == reify_core::DimensionVector::MOMENT_OF_INERTIA =>
                {
                    *si_value
                }
                other => {
                    return malformed(
                        diagnostics,
                        format!("expected numeric tensor element, got {:?}", other),
                    );
                }
            };
            tensor_cols.push(reify_ir::Value::Scalar {
                si_value: si,
                dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
            });
        }
        tensor_rows.push(reify_ir::Value::Tensor(tensor_cols));
    }
    Some(reify_ir::Value::Tensor(tensor_rows))
}

/// Issue a `PointOnShape` query and unwrap to a `Value::Bool(_)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-Bool reply.
fn dispatch_point_on_shape(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(reify_ir::Value::Bool(b)) => Some(reify_ir::Value::Bool(b)),
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-Bool value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

/// Issue a `SurfaceAngle` query and unwrap to a `Value::angle(rad)`. Returns
/// `Some(Value::Undef)` (with a Warning diagnostic) on a kernel error or a
/// non-numeric reply.
fn dispatch_surface_angle(
    kernel: &mut dyn reify_ir::GeometryKernel,
    query: &reify_ir::GeometryQuery,
    helper_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    match kernel.query(query) {
        Ok(reify_ir::Value::Real(rad)) => Some(reify_ir::Value::angle(rad)),
        // Some mock kernels store the angle as an angle-dimensioned Scalar
        // — accept either form so the dispatch is kernel-implementation
        // agnostic (mirrors `kernel_distance`'s Real|Scalar leniency).
        // Bind `dimension` (not `..`) so a wrong-dimensioned Scalar (e.g.
        // LENGTH) is caught rather than silently reinterpreted as radians.
        // Mirrors `resolve_point3_length_arg`'s tightened LENGTH check
        // introduced in commit 8c464177db (task 2324): debug_assert FIRST,
        // then if-fall-through in release.
        //
        // DIMENSIONLESS is accepted alongside ANGLE as a deliberate
        // compatibility trade-off: some mock kernels return raw f64 values
        // without attaching a dimension tag (see
        // `MockGeometryKernel::with_surface_angle_result`). A production kernel
        // returning DIMENSIONLESS for an angle would itself violate the type
        // contract — this leniency is intentional test-support compatibility,
        // not because DIMENSIONLESS is a valid angle dimension in real kernels.
        Ok(reify_ir::Value::Scalar {
            si_value,
            dimension,
        }) => {
            debug_assert!(
                dimension == reify_core::DimensionVector::ANGLE
                    || dimension == reify_core::DimensionVector::DIMENSIONLESS,
                "dispatch_surface_angle: expected ANGLE- or DIMENSIONLESS-dimensioned Scalar, \
                 got dimension {:?} (si_value={}); kernel cell type is Type::angle() per \
                 compile-time wiring",
                dimension,
                si_value
            );
            if dimension != reify_core::DimensionVector::ANGLE
                && dimension != reify_core::DimensionVector::DIMENSIONLESS
            {
                diagnostics.push(Diagnostic::warning(format!(
                    "{} kernel returned wrong-dimensioned Scalar \
                     (dimension={}, si_value={}); treating as undefined",
                    helper_name, dimension, si_value
                )));
                return Some(reify_ir::Value::Undef);
            }
            Some(reify_ir::Value::angle(si_value))
        }
        Ok(other) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel returned non-numeric value {:?}; treating as undefined",
                helper_name, other
            )));
            Some(reify_ir::Value::Undef)
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{} kernel query failed: {}",
                helper_name, err
            )));
            Some(reify_ir::Value::Undef)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ad-hoc selector eval dispatch (task 3463)
//
// Layer 2 of the two-layer @face/@edge evaluation split.  Layer 1 (pure-expr
// @point) lives in `reify-expr/src/lib.rs`.  Here we wire the kernel-aware
// @face/@edge path, mirroring `try_eval_topology_selector` in structure.
//
// Why `pub` (not `pub(crate)`): integration tests in `tests/` are separate
// crates and cannot see `pub(crate)`.  The function is also re-exported from
// `lib.rs` for the same reason, following the `resolve_unique_by_attribute`
// precedent.
// ─────────────────────────────────────────────────────────────────────────────

/// Sub-shape kind that the kernel-aware `@face` / `@edge` dispatch path
/// is willing to accept — a strict subset of `reify_types::SelectorKind`
/// with `Point` excluded by construction.
///
/// Layer 1 (`eval_expr`) resolves `@point` selectors directly from
/// literal coordinates and never reaches this module; Layer 2's
/// `try_eval_ad_hoc_selector` converts the incoming `SelectorKind` via
/// `FrameSubShapeKind::from_selector_kind` and `?`-propagates `None` for
/// `SelectorKind::Point`, so every downstream `match` in this module
/// only needs to handle `Face` and `Edge`. Replaces three previous
/// `unreachable!("Point arm ...")` arms with compile-time exhaustiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameSubShapeKind {
    Face,
    Edge,
}

impl FrameSubShapeKind {
    /// Narrow a `SelectorKind` to a kernel-aware sub-shape kind.
    /// Returns `None` for `SelectorKind::Point` so the caller can `?`-
    /// propagate the early-return-None invariant established by Layer-1.
    fn from_selector_kind(k: &reify_ir::SelectorKind) -> Option<Self> {
        match k {
            reify_ir::SelectorKind::Face => Some(FrameSubShapeKind::Face),
            reify_ir::SelectorKind::Edge => Some(FrameSubShapeKind::Edge),
            reify_ir::SelectorKind::Point => None,
        }
    }
}

/// Dispatch a `CompiledExprKind::AdHocSelector` expression through the engine
/// attribute table and geometry kernel, returning the resolved `Value::Frame`
/// (or `Some(Value::Undef)` on a diagnostic failure, or `None` if the
/// expression is not an `AdHocSelector` or the arg shapes are unsupported).
///
/// Called by `Engine::post_process_ad_hoc_selectors` after `eval_expr` has set
/// Face/Edge cells to `Value::Undef`.  Layer-1 (`@point`) was already resolved
/// by `eval_expr` directly, so `Point` arms here return `None` immediately.
///
/// # Arg-shape contract
/// - `expr.kind` must be `AdHocSelector` — anything else yields `None`.
/// - `base` must be a `Literal(Value::String(name))` — other shapes yield `None`
///   (cell stays at Undef).
/// - `args[0]` must be a `Literal(Value::String(label))` — same fall-through.
/// - `name` must exist in `named_steps` — miss yields `None`.
///
/// # Returns
/// - `Some(Value::Frame { origin, basis })` on success.
/// - `Some(Value::Undef)` on any diagnostic failure (resolver emits its own
///   `TopologyAttributeStale` Warning; kernel errors get a new Warning here).
/// - `None` for non-AdHocSelector, Point-kind, or unsupported arg shapes.
pub fn try_eval_ad_hoc_selector(
    expr: &reify_ir::CompiledExpr,
    named_steps: &HashMap<String, KernelHandle>,
    kernel: &mut dyn reify_ir::GeometryKernel,
    table: &reify_ir::TopologyAttributeTable,
    selector_span: reify_core::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // (1) Must be an AdHocSelector — anything else is not applicable.
    let (base, selector_kind, args) = match &expr.kind {
        reify_ir::CompiledExprKind::AdHocSelector {
            base,
            selector_kind,
            args,
        } => (base.as_ref(), selector_kind, args),
        _ => return None,
    };

    // (2) Convert to the narrowed FrameSubShapeKind — Point maps to None and
    //     `?` early-returns here, keeping Point out of all downstream matches.
    //     Layer-1 (eval_expr) already resolved @point from literal coordinates.
    let frame_sub_shape_kind = FrameSubShapeKind::from_selector_kind(selector_kind)?;

    // (3+4) Extract the base name and the face/edge label via evaluate-then-accept
    //       (task ε). `try_eval_ad_hoc_selector` carries no `ValueMap` — its callers
    //       (`post_process_ad_hoc_selectors` in engine_build.rs + the ad_hoc_selector
    //       smoke tests) are outside task ε's module scope — so base/label evaluate
    //       against a LOCAL empty context. Ad-hoc base/label compile to string
    //       literals (reify_compiler expr.rs AdHocSelector), which evaluate
    //       identically against any context; a stray ValueRef degrades to quiet
    //       Undef exactly as before (no regression), while a defined-but-wrong value
    //       now emits a Warning. See resolve_string_literal_arg's doc-comment.
    let ad_hoc_values = reify_ir::ValueMap::new();
    let builtin = match &frame_sub_shape_kind {
        FrameSubShapeKind::Face => "@face",
        FrameSubShapeKind::Edge => "@edge",
    };
    let name = resolve_string_literal_arg(base, &ad_hoc_values, builtin, "base", diagnostics)?;

    let label = match args.first() {
        Some(a) => resolve_string_literal_arg(a, &ad_hoc_values, builtin, "label", diagnostics)?,
        None => return None,
    };

    // (5) Look up the base name in named_steps → GeometryHandleId.
    let handle = match named_steps.get(name.as_str()) {
        Some(kh) => kh.id,
        None => return None,
    };

    // (6) Extract sub-shape handles from the kernel.
    //     Exhaustive over Face/Edge — no Point arm needed (filtered above).
    let candidates: Vec<GeometryHandleId> = match frame_sub_shape_kind {
        FrameSubShapeKind::Face => match kernel.extract_faces(handle) {
            Ok(faces) => faces,
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@face(\"{label}\"): extract_faces({handle:?}) failed: {err}"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
        FrameSubShapeKind::Edge => match kernel.extract_edges(handle) {
            Ok(edges) => edges,
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@edge(\"{label}\"): extract_edges({handle:?}) failed: {err}"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
    };

    // (7) Build AttributeQuery with dual user_label + canonical-name role translation.
    let query = crate::topology_attribute_resolver::AttributeQuery {
        user_label: Some(label.clone()),
        role_and_index: cap_kind_translation(&label),
        feature_id: None,
    };

    // (8) Resolve via the attribute table.
    let resolution = crate::topology_attribute_resolver::resolve_unique_by_attribute(
        table,
        &candidates,
        &query,
        selector_span,
        diagnostics,
    );

    // (9) On Resolved: query kernel for Frame; on any other outcome: Some(Undef).
    //     The resolver already pushed its TopologyAttributeStale / AmbiguousAfterSplit
    //     Warning, so we only need to patch the cell value here.
    match resolution {
        crate::topology_attribute_resolver::AttributeResolution::Resolved(target_id) => {
            construct_frame_from_kernel(target_id, frame_sub_shape_kind, kernel, diagnostics)
        }
        _ => Some(reify_ir::Value::Undef),
    }
}

/// Resolve a selector name/label string arg (the `face`/`edge`/`solid_body`
/// builtin name and the ad-hoc `@face`/`@edge` base/label) to an OWNED
/// `String`, emitting a `Severity::Warning` when the caller passes a
/// defined-but-wrong value.
///
/// Evaluate-then-accept (task ε): the arg expr is EVALUATED against `values`
/// (via [`eval_arg_value`]) and the resulting `Value` classified. A `ValueRef →
/// Value::String` cell now resolves (the named-leaf `face(body, label_var)`
/// form), while an inline `Literal(Value::String)` evaluates to itself —
/// functionally identical to the prior `Literal`-match. The return type changed
/// from `Option<&str>` to `Option<String>` because the evaluated `Value` is
/// owned by the local eval, not borrowed from `expr`.
///
/// | evaluated arg value                              | return    | diagnostic?     |
/// |--------------------------------------------------|-----------|-----------------|
/// | `Value::Undef` (missing/Undef cell, user-fn arg) | `None`    | no — quiet      |
/// | `Value::String(s)`                               | `Some(s)` | no              |
/// | any other defined value (Int, Real, …)           | `None`    | yes — 1 Warning |
///
/// NOTE (ad-hoc context): `try_eval_ad_hoc_selector` carries no `ValueMap` in
/// its signature — it is a public API whose callers (`post_process_ad_hoc_selectors`
/// in `engine_build.rs` and the `ad_hoc_selector_smoke_tests`) are outside task
/// ε's module scope — so it evaluates base/label against a LOCAL empty
/// `ValueMap`. Ad-hoc base/label compile to string literals in practice (see
/// `reify_compiler` `expr.rs` `AdHocSelector`), which evaluate identically
/// against any context; a stray `ValueRef` there degrades to quiet `Undef`
/// exactly as before (no regression). The named-leaf caller
/// (`eval_named_leaf_selector_ctor`) threads the real `values`, so
/// `face(body, label_var)` resolves a `ValueRef → String` cell.
fn resolve_string_literal_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    arg_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    use crate::arg_acceptance::ArgRejection;

    let value = eval_arg_value(expr, values);

    match value {
        // Quiet degradation: an Undef value (missing cell, or a user-fn/meta arg
        // the local ctx can't evaluate) returns None with no diagnostic —
        // behaviourally identical to the prior non-`Literal(String)` fall-through.
        reify_ir::Value::Undef => None,
        reify_ir::Value::String(s) => Some(s),
        // Defined-but-wrong (non-String): emit exactly one Warning naming
        // builtin/arg/String/got (byte-uniform wording with the density / point /
        // vec3 / range / int paths).
        other => {
            diagnostics.push(Diagnostic::warning(
                ArgRejection {
                    got: string_got_label(&other),
                    expected: "String",
                    migration_hint: None,
                }
                .message(builtin, arg_name),
            ));
            None
        }
    }
}

/// Resolve a `faces_by_surface_kind` kind arg: a String literal naming a
/// canonical [`reify_ir::FaceSurfaceKind`] ("Plane"/"Cylinder"/…). Returns
/// `None` (with one Warning) for a non-String value or an unrecognised name,
/// leaving the cell at `Value::Undef`. (task 3523)
fn resolve_face_surface_kind_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::FaceSurfaceKind> {
    let name = resolve_string_literal_arg(expr, values, builtin, "kind", diagnostics)?;
    match reify_ir::FaceSurfaceKind::try_from_str(&name) {
        Ok(k) => Some(k),
        Err(_) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{builtin}: unrecognised surface kind \"{name}\" (expected one of \
                 Plane/Cylinder/Cone/Sphere/Torus/BezierSurface/BSplineSurface/\
                 OffsetSurface/Other); cell left at Undef"
            )));
            None
        }
    }
}

/// Resolve an `edges_by_curve_kind` kind arg: a String literal naming a
/// canonical [`reify_ir::EdgeCurveKind`] ("Line"/"Circle"/…). Returns `None`
/// (with one Warning) for a non-String value or an unrecognised name. (task 3523)
fn resolve_edge_curve_kind_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::EdgeCurveKind> {
    let name = resolve_string_literal_arg(expr, values, builtin, "kind", diagnostics)?;
    match reify_ir::EdgeCurveKind::try_from_str(&name) {
        Ok(k) => Some(k),
        Err(_) => {
            diagnostics.push(Diagnostic::warning(format!(
                "{builtin}: unrecognised curve kind \"{name}\" (expected one of \
                 Line/Circle/Ellipse/Hyperbola/Parabola/BezierCurve/BSplineCurve/\
                 OffsetCurve/Other); cell left at Undef"
            )));
            None
        }
    }
}

/// Resolve an extremal-selector `axis` arg: a String literal "X"/"Y"/"Z" mapped
/// to the axis index 0/1/2 (matching `LeafQuery::ByExtremal*.axis_index`, which
/// `resolve_leaf` maps back to `selector_vocabulary_v2::Axis`). Case-sensitive
/// canonical names, mirroring `FaceSurfaceKind::try_from_str`. (task 3523)
fn resolve_axis_index_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u8> {
    let name = resolve_string_literal_arg(expr, values, builtin, "axis", diagnostics)?;
    match name.as_str() {
        "X" => Some(0),
        "Y" => Some(1),
        "Z" => Some(2),
        _ => {
            diagnostics.push(Diagnostic::warning(format!(
                "{builtin}: unrecognised axis \"{name}\" (expected \"X\", \"Y\", or \
                 \"Z\"); cell left at Undef"
            )));
            None
        }
    }
}

/// Resolve an extremal-selector `sense` arg: a String literal "Max"/"Min" mapped
/// to `max: bool` (true = Max). Case-sensitive. (task 3523)
fn resolve_extremal_sense_arg(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    builtin: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<bool> {
    let name = resolve_string_literal_arg(expr, values, builtin, "sense", diagnostics)?;
    match name.as_str() {
        "Max" => Some(true),
        "Min" => Some(false),
        _ => {
            diagnostics.push(Diagnostic::warning(format!(
                "{builtin}: unrecognised sense \"{name}\" (expected \"Max\" or \
                 \"Min\"); cell left at Undef"
            )));
            None
        }
    }
}

/// Short human-readable label for a `Value` that failed String classification,
/// used as the `got` field of the rejection diagnostic (task ε).
fn string_got_label(value: &reify_ir::Value) -> String {
    match value {
        reify_ir::Value::Int(_) => "Int".to_string(),
        reify_ir::Value::Real(_) => "Real".to_string(),
        reify_ir::Value::Scalar { dimension, .. } => scalar_got_label(dimension),
        reify_ir::Value::Bool(_) => "Bool".to_string(),
        reify_ir::Value::Vector(_) => "Vector".to_string(),
        reify_ir::Value::Point(_) => "Point".to_string(),
        _ => "non-String value".to_string(),
    }
}

/// Translate a canonical face/edge label into a `(Role, local_index)` pair for
/// the `AttributeQuery::role_and_index` field.
///
/// The translation covers canonical labels wired by the Cylinder seeder
/// (`seed_primitive_attributes`), making `@face("top")` / `@face("bottom")`
/// / `@face("start")` / `@face("end")` / `@face("side")` work against
/// Cylinder and Extrude / Revolve primitives without requiring a
/// `name = "..."` source annotation on the face (which is deferred per the
/// PRD).
///
/// **`"side"` note:** `Role::Side` with `local_index = 0` is the entry
/// seeded for the single lateral face of a Cylinder. For primitives with
/// multiple side faces (future Boolean / Loft results) this match selects
/// index 0 only; if the resolver finds multiple `Role::Side` entries it will
/// return `AmbiguousAfterSplit`, surfacing a `TopologyAttributeStale` Warning
/// and leaving the cell at `Value::Undef` — the same graceful-degradation
/// path as all other Unresolved outcomes.
///
/// Any unrecognised label returns `None` — the query then relies entirely on
/// `user_label` and will Unresolve if no `user_label` entry exists in the table.
// G-allow: task #3463 (done) cap/role vocabulary table; consumer is try_eval_ad_hoc_selector @face/@edge dispatch (same-file, task #3463, done) + ad_hoc_selector smoke tests
pub(crate) fn cap_kind_translation(label: &str) -> Option<(reify_ir::Role, u32)> {
    use reify_ir::{CapKind, Role};
    match label {
        "top" => Some((Role::Cap(CapKind::Top), 0)),
        "bottom" => Some((Role::Cap(CapKind::Bottom), 0)),
        "start" => Some((Role::Cap(CapKind::Start), 0)),
        "end" => Some((Role::Cap(CapKind::End), 0)),
        "side" => Some((Role::Side, 0)),
        _ => None,
    }
}

/// Query the kernel for centroid and normal/tangent of `target_id`, then
/// construct a `Value::Frame { origin, basis }`.
///
/// The `sub_shape_kind` parameter selects the kernel query:
/// - `FrameSubShapeKind::Face` → `GeometryQuery::FaceNormal` (face normal maps
///   to the frame's **+Z** axis — standard CAD convention for planar features).
/// - `FrameSubShapeKind::Edge` → `GeometryQuery::EdgeTangent` (edge tangent
///   maps to the frame's **+Z** axis; downstream consumers that expect the
///   tangent along **+X** should apply a 90° R_Y pre-rotation).
///
/// `FrameSubShapeKind` excludes `Point` by construction, so this function
/// never needs to handle the Point case — the type system enforces the
/// invariant that was previously guarded by `unreachable!()` arms.
///
/// On centroid failure: push a Warning and return `Some(Value::Undef)`.
/// On normal/tangent failure: push a Warning and use identity basis, so the
/// Frame still has a meaningful origin.
fn construct_frame_from_kernel(
    target_id: GeometryHandleId,
    sub_shape_kind: FrameSubShapeKind,
    kernel: &mut dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Value> {
    // ── Origin via Centroid ───────────────────────────────────────────────
    // `GeometryQuery::Centroid` is unified — works for faces, edges, AND solids.
    let origin = match kernel.query(&reify_ir::GeometryQuery::Centroid(target_id)) {
        Ok(value) => match crate::topology_selectors::parse_xyz_value(&value, "Centroid") {
            Ok([x, y, z]) => reify_ir::Value::Point(vec![
                reify_ir::Value::length(x),
                reify_ir::Value::length(y),
                reify_ir::Value::length(z),
            ]),
            Err(err) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "@face/@edge centroid parse failed: {err}; cell left as Undef"
                )));
                return Some(reify_ir::Value::Undef);
            }
        },
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "@face/@edge centroid query failed: {err}; cell left as Undef"
            )));
            return Some(reify_ir::Value::Undef);
        }
    };

    // ── Basis via FaceNormal (face) or EdgeTangent (edge) ─────────────────
    // Exhaustive over Face/Edge — Point is excluded by the FrameSubShapeKind
    // type, so no unreachable!() arm is needed here.
    let basis_query = match sub_shape_kind {
        FrameSubShapeKind::Face => reify_ir::GeometryQuery::FaceNormal(target_id),
        FrameSubShapeKind::Edge => reify_ir::GeometryQuery::EdgeTangent(target_id),
    };
    let query_label = match sub_shape_kind {
        FrameSubShapeKind::Face => "FaceNormal",
        FrameSubShapeKind::Edge => "EdgeTangent",
    };

    let basis = match kernel.query(&basis_query) {
        Ok(value) => {
            match crate::topology_selectors::parse_xyz_value(&value, query_label) {
                Ok([nx, ny, nz]) => quaternion_from_z_to_axis(nx, ny, nz),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "@face/@edge {query_label} parse failed: {err}; using identity basis"
                    )));
                    // Degrade gracefully: return a Frame with correct origin, identity basis.
                    reify_ir::Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }
                }
            }
        }
        Err(err) => {
            diagnostics.push(Diagnostic::warning(format!(
                "@face/@edge {query_label} query failed: {err}; using identity basis"
            )));
            // Degrade gracefully: origin was obtained, identity basis.
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
    };

    Some(reify_ir::Value::Frame {
        origin: Box::new(origin),
        basis: Box::new(basis),
    })
}

/// Compute the shortest-arc unit quaternion that rotates the +Z axis `(0, 0, 1)`
/// to the given (approximately unit) axis vector `(nx, ny, nz)`.
///
/// Formula: for unit vectors `a` and `b`,
///   `q_unnorm = (1 + dot(a,b),  cross(a,b))`
/// where `a = +Z = (0,0,1)`, so:
///   `dot(+Z, b) = nz`
///   `cross(+Z, b) = (-ny, nx, 0)`
///   `q_unnorm = (1 + nz, -ny, nx, 0)`
///
/// Special case: `b ≈ -Z` makes `q_unnorm ≈ (0,0,0,0)` — degenerate.
/// Fall back to a 180° rotation around the +X axis.
///
/// **Numerical note:** for an approximately unit input, `len_sq = (1 + nz)² + nx² + ny²`.
/// Since `nx² + ny² = 1 − nz² = (1 − nz)(1 + nz)`, this simplifies to
///   `len_sq = 2·(1 + nz)`.
/// So `len_sq < 1e-12` fires for `nz < −1 + 5e-13` — roughly half a femto-unit from
/// `−Z`. The margin is intentional: it is well above the rounding noise accumulated
/// by the multiply-and-add chain that produces `len_sq`, yet small enough that the
/// fallback only activates for genuinely degenerate inputs. **Do not tighten the
/// threshold further**: reducing it below ~`1e-13` would shrink the safety margin
/// into f64 rounding noise and allow near-degenerate inputs to produce NaN-carrying
/// quaternions.
fn quaternion_from_z_to_axis(nx: f64, ny: f64, nz: f64) -> reify_ir::Value {
    let w_unnorm = 1.0 + nz;
    // Use `0.0 - ny` instead of `-ny` to avoid producing -0.0 when ny = 0.0.
    // In IEEE 754, `0.0 - 0.0 = +0.0` (round-to-nearest), whereas the unary
    // negation `-0.0 = -0.0`.  The bit-exact `PartialEq` on `Value::Orientation`
    // would treat -0.0 and +0.0 as unequal, causing spurious test failures when
    // the input has a zero component.
    let x_unnorm = 0.0 - ny;
    let y_unnorm = nx;
    // z component of cross(+Z, b) is always 0.

    let len_sq = w_unnorm * w_unnorm + x_unnorm * x_unnorm + y_unnorm * y_unnorm;

    if len_sq < 1e-12 {
        // (nx, ny, nz) ≈ -Z: degenerate case. Rotate 180° around +X.
        return reify_ir::Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
    }

    let len = len_sq.sqrt();
    reify_ir::Value::Orientation {
        w: w_unnorm / len,
        x: x_unnorm / len,
        y: y_unnorm / len,
        z: 0.0,
    }
}

/// Evaluate an `at <pose>` sub-component placement expression into a rigid
/// child→parent [`reify_ir::Value::Transform`].
///
/// # Convention (resolves PRD §11 Q1)
///
/// A `Transform { rotation: Q, translation: t }` maps a child-local point `p`
/// to parent-space via `Q·p + t`.  Carrying the child's identity origin-frame
/// onto target `Frame { origin: o, basis: R }` (target in parent coords) forces:
///
/// - child-origin 0 → o  ⇒  t = o (origin components copied as-is, dimension preserved)
/// - child-axes   I → R  ⇒  Q = R (basis copied; no normalization — frame3 guarantees unit basis)
///
/// Hence `Frame { origin: o, basis: R }` → `Transform { rotation: R, translation: o_as_vector }`.
///
/// | `pose` result                       | outcome                                               |
/// |-------------------------------------|-------------------------------------------------------|
/// | `None`                              | identity (Orientation(1,0,0,0), Vector[len 0,0,0])    |
/// | `Some(_)` → `Value::Transform`      | pass through unchanged                                |
/// | `Some(_)` → `Value::Frame`          | lowered per the convention above                      |
/// | anything else (incl. `Value::Undef`)| one `Diagnostic::error`; returns `Value::Undef`       |
#[allow(dead_code)] // used in #[cfg(test)]; consumed by T5 (full-tree composition)
pub(crate) fn eval_sub_pose(
    pose: Option<&reify_ir::CompiledExpr>,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let Some(expr) = pose else {
        return identity_pose_transform();
    };

    let value = reify_expr::eval_expr(expr, &eval_ctx_with_meta(values, functions, meta_map));
    match value {
        reify_ir::Value::Transform { .. } => value,
        reify_ir::Value::Frame { origin, basis } => {
            frame_to_pose_transform(*origin, *basis, diagnostics)
        }
        _ => {
            // This arm includes Value::Undef (expression evaluation failed upstream).
            // We emit one error here intentionally: it gives the caller a call-site
            // anchor showing *where* the failed pose affected sub-component placement,
            // complementing whatever diagnostic the upstream expression already emitted.
            // This behavior is pinned by `eval_sub_pose_undef_expr_returns_undef_with_diagnostic`.
            diagnostics.push(Diagnostic::error(
                "`at` pose expression must evaluate to a Transform or Frame",
            ));
            reify_ir::Value::Undef
        }
    }
}

/// The identity child→parent [`reify_ir::Value::Transform`] — `Orientation(1,0,0,0)`
/// + zero-LENGTH `Vector` translation.
///
/// Shared by [`eval_sub_pose`]'s `None` arm (a sub with no `at` clause) and
/// [`eval_auto_sub_pose`]'s fallback (an `at auto` sub with no solved Frame). Same
/// shape as `compose_pose_chain(&[])` but allocation-only (no builtin dispatch).
fn identity_pose_transform() -> reify_ir::Value {
    reify_ir::Value::Transform {
        rotation: Box::new(reify_ir::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(reify_ir::Value::Vector(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ])),
    }
}

/// Lower a [`reify_ir::Value::Frame`] `{ origin, basis }` to a child→parent
/// [`reify_ir::Value::Transform`] per the §11-Q1 convention (origin → translation,
/// basis → rotation; see [`eval_sub_pose`]).
///
/// The single source of truth for the Frame→Transform lowering, shared by
/// [`eval_sub_pose`] (a concrete `at <frame>` pose) and [`eval_auto_sub_pose`]
/// (geometric-relations ζ's solver-written `at auto` pose). A malformed Frame —
/// non-`Point` origin, wrong arity, non-LENGTH / non-finite component, or
/// non-`Orientation` basis — pushes one `Diagnostic::error` and returns
/// `Value::Undef` (the established `eval_sub_pose` contract its unit tests pin).
fn frame_to_pose_transform(
    origin: reify_ir::Value,
    basis: reify_ir::Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let components = match origin {
        reify_ir::Value::Point(c) => c,
        other => {
            diagnostics.push(Diagnostic::error(format!(
                "`at` pose Frame origin must be a Point; got {:?}",
                other
            )));
            return reify_ir::Value::Undef;
        }
    };
    if components.len() != 3
        || !components.iter().all(|c| {
            if let reify_ir::Value::Scalar {
                si_value,
                dimension,
            } = c
            {
                *dimension == reify_core::DimensionVector::LENGTH && si_value.is_finite()
            } else {
                false
            }
        })
    {
        diagnostics.push(Diagnostic::error(
            "`at` pose Frame origin must be a 3-component LENGTH-dimensioned Point with finite coordinates",
        ));
        return reify_ir::Value::Undef;
    }
    if !matches!(basis, reify_ir::Value::Orientation { .. }) {
        diagnostics.push(Diagnostic::error(
            // The check is structural: Value::Orientation variant only.
            // frame3 guarantees unit basis; non-unit quaternions arriving
            // via other construction paths are caught by downstream
            // Transform composition rather than here (keeps lowering exact).
            "`at` pose Frame basis must be a Value::Orientation",
        ));
        return reify_ir::Value::Undef;
    }
    reify_ir::Value::Transform {
        rotation: Box::new(basis),
        translation: Box::new(reify_ir::Value::Vector(components)),
    }
}

/// Evaluate an `at auto` sub-component's placement (geometric-relations ζ, task 4386).
///
/// The per-scope relate-solve (ζ step-18, run in the build pass) writes each `at
/// auto` sub's solved 6-DOF assembly `Frame` into the value map under
/// [`crate::relate_solve::auto_pose_cell`]`(scope, sub)`. This is the surfacing
/// walk's auto arm: it reads that Frame back and lowers it to a child→parent
/// [`reify_ir::Value::Transform`] via [`frame_to_pose_transform`] (the SAME
/// convention as a concrete `at <frame>` pose), so the existing
/// `compose_pose_chain`→`GeometryOp::ApplyTransform` placement (task 3901) seats
/// the sub unchanged.
///
/// Returns the identity Transform when no solved Frame is present: either the
/// scope had no solvable relate-solve, or the driving-set solve failed (Infeasible
/// / non-convergent) and the build pass ALREADY emitted an `Error` diagnostic that
/// fails the build. In that case the sub degrades to identity rather than this arm
/// emitting a second, duplicate error.
pub(crate) fn eval_auto_sub_pose(
    scope: &str,
    sub: &str,
    values: &ValueMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Value {
    let cell = crate::relate_solve::auto_pose_cell(scope, sub);
    match values.get(&cell).cloned() {
        Some(reify_ir::Value::Frame { origin, basis }) => {
            frame_to_pose_transform(*origin, *basis, diagnostics)
        }
        // The solve may already produce a Transform directly; pass it through.
        Some(t @ reify_ir::Value::Transform { .. }) => t,
        // No solved Frame (no relations, or a failed solve that already errored).
        _ => identity_pose_transform(),
    }
}

/// Resolve whether the `let` binding backing a realization was declared `aux`
/// (PRD §2.2), i.e. "no external geometric effect" → surfaced hidden.
///
/// The compiler threads `LetDecl.is_aux` directly onto `RealizationDecl.is_aux`
/// at lowering (geometry lets are lowered as realizations only — they create no
/// `ValueCellDecl`, so the flag cannot be recovered via `value_cells`). Geometry
/// params and the guarded-group path carry no source `aux` modifier and so are
/// always `false`.
///
/// Used by `tessellate_from_values` to derive `MeshSurface.default_visible`
/// on the flat (no-composition) path; the Phase-B containment walk additionally
/// ORs in any `aux` ancestor sub (T5 steps 4/6).
///
/// This is intentionally a thin wrapper over the public `RealizationDecl.is_aux`
/// field: its value is *documentary*, not behavioral. It gives the surfacing
/// call site (`surface_subtree`) a self-describing name for the visibility
/// intent and a single anchor for the non-obvious compiler-threading rationale
/// above (the field exists only because escalation esc-3903-220 added it — the
/// `aux` modifier on a geometry `let` would otherwise be dropped at lowering).
/// Keeping it avoids re-deriving that context at the use site; inlining would
/// either lose the doc or scatter it into a call-site comment.
pub(crate) fn realization_is_aux(realization: &reify_compiler::RealizationDecl) -> bool {
    realization.is_aux
}

/// Decompose a `Value::Transform` into raw quaternion + SI-metre translation
/// arrays for building a kernel-agnostic `reify_ir::GeometryOp::ApplyTransform`
/// (T5 step-8/10).
///
/// Accepts `Transform { rotation: Orientation { w, x, y, z }, translation:
/// Vector([s0, s1, s2]) }` where each translation component is a finite LENGTH
/// or dimensionless `Scalar`; returns `Some(([w,x,y,z], [tx,ty,tz]))` with the
/// translation read straight off `Scalar.si_value` (SI metres). Returns `None`
/// for any other shape — non-`Transform`, non-`Orientation` rotation, a
/// translation that is not a 3-component `Vector`, or a component that is not a
/// LENGTH/dimensionless finite `Scalar`. Each component is checked independently,
/// so a mixed-dimension translation (e.g. one ANGLE among LENGTHs) is rejected.
///
/// `reify_stdlib`'s own `decompose_transform` is private, so this local
/// pattern-match keeps the change inside reify-eval while feeding the IR op's
/// raw float arrays (the IR is kernel-agnostic by design).
pub(crate) fn decompose_transform_to_arrays(v: &reify_ir::Value) -> Option<([f64; 4], [f64; 3])> {
    let reify_ir::Value::Transform {
        rotation,
        translation,
    } = v
    else {
        return None;
    };
    let reify_ir::Value::Orientation { w, x, y, z } = rotation.as_ref() else {
        return None;
    };
    let reify_ir::Value::Vector(components) = translation.as_ref() else {
        return None;
    };
    if components.len() != 3 {
        return None;
    }
    let mut t = [0.0_f64; 3];
    for (i, c) in components.iter().enumerate() {
        let reify_ir::Value::Scalar {
            si_value,
            dimension,
        } = c
        else {
            return None;
        };
        let dim_ok = *dimension == reify_core::DimensionVector::LENGTH
            || *dimension == reify_core::DimensionVector::DIMENSIONLESS;
        if !dim_ok || !si_value.is_finite() {
            return None;
        }
        t[i] = *si_value;
    }
    Some(([*w, *x, *y, *z], t))
}

/// Decode a `Value::Orientation` quaternion into an `(axis, angle_rad)` pair
/// suitable for `GeometryOp::Rotate`.
///
/// Replicates the `orient_to_axis_angle` math from `reify-stdlib`
/// (orientation.rs:440-471) as a local helper feeding raw float arrays into the
/// IR op.  The stdlib function is `pub(crate)` and returns a `Value::Map` —
/// awkward to consume here; same rationale as `decompose_transform_to_arrays`.
///
/// Identity/near-identity quaternions (|v| < 1e-12) decode to the canonical
/// no-op `([1.0, 0.0, 0.0], 0.0)` — never `([0.0, 0.0, 0.0], 0.0)`, because
/// the kernel Rotate handler rejects zero-length axes.
pub(crate) fn decode_orientation_to_axis_angle(
    v: &reify_ir::Value,
) -> Option<([f64; 3], f64)> {
    let reify_ir::Value::Orientation { w, x, y, z } = v else {
        return None;
    };
    if !reify_ir::quaternion_is_finite(*w, *x, *y, *z) {
        return None;
    }
    let v_norm = (x * x + y * y + z * z).sqrt();
    if v_norm < 1e-12 {
        return Some(([1.0, 0.0, 0.0], 0.0));
    }
    let angle = 2.0 * v_norm.atan2(*w);
    let axis = [x / v_norm, y / v_norm, z / v_norm];
    Some((axis, angle))
}

/// Left-fold a chain of pose `Value::Transform`s into a single world transform
/// via the quaternion-correct `transform_compose` builtin (T5 step-8/10).
///
/// Seeds with the identity Transform and folds `transform_compose(acc, next)`
/// left-to-right, so the result is `pose_0 ∘ pose_1 ∘ … ∘ pose_n` (mirrors the
/// proven left-fold in `reify_stdlib::loop_closure::chain_transform`). An empty
/// chain returns the identity Transform unchanged. Reuses the already-tested
/// stdlib builtin rather than hand-rolling quaternion math; `reify-eval` already
/// depends on `reify-stdlib`.
pub(crate) fn compose_pose_chain(poses: &[reify_ir::Value]) -> reify_ir::Value {
    let identity = reify_ir::Value::Transform {
        rotation: Box::new(reify_ir::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(reify_ir::Value::Vector(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ])),
    };
    poses.iter().fold(identity, |acc, next| {
        reify_stdlib::eval_builtin("transform_compose", &[acc, next.clone()])
    })
}

/// Indices into `module.templates` of the *root* templates for surfacing: those
/// whose `name` is NOT the `structure_name` of any NON-collection sub anywhere
/// in the module (T5 step-4).
///
/// `CompiledModule.templates` is a flat `Vec` with no root marker, and the
/// pre-T5 evaluator surfaced *every* template standalone — so a contained child
/// appeared at un-placed local coords. The containment walk surfaces each
/// non-root descendant exactly once at its composed world pose, so only roots
/// seed the walk; contained templates are suppressed standalone.
///
/// Collection subs are deliberately excluded from the "contained" set: their
/// per-element placement is out of scope (PRD §10), so a template used *only* as
/// a `List<T>` sub still surfaces standalone as a root (unchanged behavior).
pub(crate) fn root_template_indices(module: &reify_compiler::CompiledModule) -> Vec<usize> {
    let contained: std::collections::HashSet<&str> = module
        .templates
        .iter()
        .flat_map(|t| t.sub_components.iter())
        .filter(|sub| !sub.is_collection)
        .map(|sub| sub.structure_name.as_str())
        .collect();
    module
        .templates
        .iter()
        .enumerate()
        .filter(|(_, t)| !contained.contains(t.name.as_str()))
        .map(|(idx, _)| idx)
        .collect()
}

/// Indices of every template reachable from `seeds` by following NON-collection
/// subs, inclusive of the seeds themselves (T5 amendment — cycle-loss guard).
///
/// The Phase-B driver surfaces from each root, but a template that is excluded
/// from the root set (because some sub names it) yet is reachable from NO root
/// can only sit inside a non-collection containment cycle with no acyclic entry
/// point — a self-recursive `sub child : Self`, or a mutual `A -> B -> A`. Pre-T5
/// every template surfaced standalone, so dropping such a template is a silent
/// geometry-loss regression. The driver computes `reachable_template_indices(…,
/// roots)` and surfaces any *unreached* template as a fallback root, so its
/// geometry is preserved (the per-template `surface_subtree` walk stays bounded
/// by the `depth > templates.len()` cycle guard).
///
/// `structure_name -> template index` is resolved by `position` (mirroring
/// `surface_subtree` / `root_template_indices`); collection subs are skipped to
/// match the root-set's containment definition. In an *acyclic* module every
/// non-root is reachable from some root, so this returns the full index set and
/// the fallback loop is a no-op — zero behavior change off the cyclic path.
pub(crate) fn reachable_template_indices(
    module: &reify_compiler::CompiledModule,
    seeds: &[usize],
) -> std::collections::HashSet<usize> {
    let mut reached: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut stack: Vec<usize> = seeds.to_vec();
    while let Some(idx) = stack.pop() {
        if !reached.insert(idx) {
            continue;
        }
        for sub in &module.templates[idx].sub_components {
            if sub.is_collection {
                continue;
            }
            // Push every non-collection child; the `reached.insert` guard above
            // dedups on pop, so re-pushing an already-reached index is harmless.
            if let Some(child) = module
                .templates
                .iter()
                .position(|t| t.name == sub.structure_name)
            {
                stack.push(child);
            }
        }
    }
    reached
}

/// Pre-filter callback type for the placed-realization walk.
///
/// Called before `ApplyTransform` with `(t_idx, r_idx, entity_path)`.
/// Returns `false` to skip both the transform and visitor call for that realization.
/// Use `None` for the default "include all" behavior.
pub(crate) type PlacedPreFilter<'a> = Option<&'a dyn Fn(usize, usize, &str) -> bool>;

/// Placed-product body collected by the T7 export walk (`surface_export_bodies`).
///
/// The `entity_path` is the composed PRD §11.2 path; `handle_id` is the
/// `GeometryHandleId` of the placed BRep (world transform already baked in via
/// `ApplyTransform`); `default_visible` follows the same OR-of-aux rule as
/// `surface_subtree` (`false` iff aux or under-aux-sub).
#[derive(Debug)]
pub(crate) struct ExportBody {
    pub entity_path: String,
    pub handle_id: GeometryHandleId,
    pub default_visible: bool,
}

/// Shared inner walk for T5 (tessellate) and T7 (export) containment-tree surfacing.
///
/// Implements the common cycle guard, placement decomposition, `ApplyTransform`
/// application, `default_visible` derivation, and `entity_path` formatting shared
/// by `surface_subtree` and `surface_export_bodies`.  The terminal action per
/// realization is delegated to `visit_realization`.
///
/// # Visitor
///
/// `visit_realization(kernel, placed_id, entity_path, default_visible, t_idx, r_idx, diagnostics)`
/// — called once per realization that produced a handle, after the composed world
/// transform is applied:
/// - `kernel`: mutable borrow of the default kernel (over the visit call; released before
///   recursion so the sub loop can re-borrow).
/// - `placed_id`: the `GeometryHandleId` after `ApplyTransform` (or the source handle for
///   identity/undecomposable poses — no extra kernel op).
/// - `entity_path`: PRD §11.2 composed path (owned `String`).
/// - `default_visible`: `false` iff aux or under-aux-sub (same OR rule as the callers).
/// - `t_idx`, `r_idx`: template and realization indices (for budget lookup in the
///   tessellate visitor).
/// - `diagnostics`: the walk's diagnostic accumulator; the visitor may push to it.
///
/// Callers (`surface_subtree`, `surface_export_bodies`) preserve their public signatures
/// unchanged — they are thin wrappers that capture their output collection in the closure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_placed_realizations<V>(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    aux_ancestor: bool,
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    // task-4147: per-instance handle-row override for constructor-arg subs.
    //
    // When `Some(row)`, the realization loop reads handles from `row[r_idx]`
    // instead of `terminal_handles[t_idx][r_idx]`.  `row` is aligned with
    // `terminal_handles[t_idx]` (same length, same r_idx semantics), produced
    // by `crate::engine_build::realize_sub_override_handles` for each sub
    // with `!sub.args.is_empty()`.
    //
    // Pass `None` at roots (roots are never overridden subs) and for arg-free
    // subs (which reuse the Phase-A shared handle, no re-realization needed).
    handle_row_override: Option<&[Option<KernelHandle>]>,
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    // Optional short-circuit filter (T7 amendment, suggestion 3).
    //
    // Called BEFORE `ApplyTransform` with `(t_idx, r_idx, entity_path)`.
    // If the filter returns `false`, BOTH the transform and the visitor call are
    // skipped for that realization — no new kernel handle is minted.
    //
    // Pass `None` for the default "include all" behavior (zero overhead on the
    // tessellation hot path).  Pass `Some(f)` in `distance_between_placed` to
    // skip transforms for realizations not matching the two target paths, avoiding
    // accumulation of transient placed handles in the kernel store on repeated calls.
    pre_filter: PlacedPreFilter<'_>,
    visit_realization: &mut V,
) where
    V: FnMut(
        &mut dyn GeometryKernel,
        GeometryHandleId,
        String,
        bool,
        usize,
        usize,
        &mut Vec<Diagnostic>,
    ),
{
    if depth > module.templates.len() {
        return;
    }
    let template = &module.templates[t_idx];

    // Decompose the inherited world transform once for this template.
    // `Some(non-identity)` → apply via ApplyTransform before visiting;
    // identity / non-decomposable short-circuits to the source handle (no kernel op).
    let placement: Option<([f64; 4], [f64; 3])> =
        match decompose_transform_to_arrays(composed_world) {
            Some((rotation, translation))
                if rotation != [1.0, 0.0, 0.0, 0.0] || translation != [0.0, 0.0, 0.0] =>
            {
                Some((rotation, translation))
            }
            _ => None,
        };

    // task-4147: use per-instance override row when present; fall back to the
    // Phase-A shared row for arg-free subs (or at roots).
    let handles_row: &[Option<KernelHandle>] =
        handle_row_override.unwrap_or(&terminal_handles[t_idx]);

    for (r_idx, realization) in template.realizations.iter().enumerate() {
        let Some(handle) = handles_row[r_idx] else {
            continue;
        };
        // Compute entity_path BEFORE ApplyTransform so `pre_filter` can gate the
        // transform on the path (avoids minting transient handles for unwanted bodies).
        let entity_path = format!("{}#realization[{}]", path_prefix, realization.id.index);
        // Short-circuit: if the caller supplied a pre-filter and this realization is
        // not wanted, skip both the transform and the visitor — no kernel op issued.
        if pre_filter.is_some_and(|f| !f(t_idx, r_idx, &entity_path)) {
            continue;
        }
        let default_kernel = geometry_kernels
            .get_mut(default_kernel_name)
            .expect("default kernel must remain in the map across the surfacing walk");
        let placed_id = match placement {
            Some((rotation, translation)) => {
                match default_kernel.execute(&reify_ir::GeometryOp::ApplyTransform {
                    target: handle.id,
                    rotation,
                    translation,
                }) {
                    Ok(transformed) => transformed.id,
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!(
                            "transform application error: {}",
                            e
                        )));
                        continue;
                    }
                }
            }
            None => handle.id,
        };
        let default_visible = !(aux_ancestor || realization_is_aux(realization));
        // Pass a mutable borrow of the kernel to the visitor; the borrow is
        // released when visit_realization returns, before the sub-component
        // loop re-borrows geometry_kernels for the recursive call.
        // Explicit deref-coercion: `default_kernel` is `&mut Box<dyn GeometryKernel>`;
        // `&mut **default_kernel` gives the `&mut dyn GeometryKernel` the visitor expects.
        visit_realization(
            &mut **default_kernel,
            placed_id,
            entity_path,
            default_visible,
            t_idx,
            r_idx,
            diagnostics,
        );
    }

    // Recurse into each non-collection sub, composing the sub's `at` pose.
    for sub in &template.sub_components {
        if sub.is_collection {
            continue;
        }
        let Some(child_idx) = module
            .templates
            .iter()
            .position(|t| t.name == sub.structure_name)
        else {
            continue;
        };
        let child_prefix = format!("{}.{}", path_prefix, sub.name);
        // Geometric-relations ζ (task 4386): an `at auto` sub is placed by the
        // per-scope relate-solve, whose solved Frame the build pass wrote into
        // `values` (keyed by `auto_pose_cell`). Read it back via the auto arm;
        // every other sub evaluates its concrete `at <pose>` (or identity) as before.
        let sub_pose = if sub.auto_pose.is_some() {
            eval_auto_sub_pose(&template.name, &sub.name, values, diagnostics)
        } else {
            eval_sub_pose(sub.pose.as_ref(), values, functions, meta_map, diagnostics)
        };
        let child_world = compose_pose_chain(&[composed_world.clone(), sub_pose]);

        // task-4147: for constructor-arg subs, re-realize the child's handles
        // against the per-instance override value scope BEFORE the recursive
        // call (sequencing avoids overlapping `&mut geometry_kernels` borrows).
        let child_override_row: Option<Vec<Option<KernelHandle>>> = if !sub.args.is_empty() {
            Some(crate::engine_build::realize_sub_override_handles(
                &template.name,
                sub,
                &module.templates[child_idx],
                geometry_kernels,
                default_kernel_name,
                values,
                functions,
                meta_map,
                diagnostics,
            ))
        } else {
            None
        };

        walk_placed_realizations(
            module,
            child_idx,
            &child_prefix,
            aux_ancestor || sub.is_aux,
            &child_world,
            depth + 1,
            terminal_handles,
            child_override_row.as_deref(),
            geometry_kernels,
            default_kernel_name,
            values,
            functions,
            meta_map,
            diagnostics,
            pre_filter,
            visit_realization,
        );
    }
}

/// T7 (task 3905, robustness fix esc-3905-277): for each template, the set of
/// realization indices to **exclude** from the export walk — every
/// geometry-producing realization except the template's *final* one.
///
/// Rationale: a boolean (or modify / sweep / …) whose operands are bound to named
/// lets — e.g. `let a = box(...); let b = box(...); let r = union(a, b)` — compiles
/// to one realization per let, BUT the compiler *inlines* each operand's
/// construction into the consuming realization (`r`'s ops are `[Box, Box,
/// Boolean(Step0, Step1)]`, referencing its operands by intra-realization
/// `GeomRef::Step`, not by cross-realization `GeomRef::Sub`). The `a`/`b`
/// realizations are therefore standalone duplicates of geometry already contained
/// in `r`. Surfacing all three (the pre-fix behavior, filtered only by `aux`)
/// shipped a STEP file with the two consumed input boxes PLUS their union — three
/// overlapping solids.
///
/// The pre-T7 export took `*step_handles.last()` — the terminal handle of the LAST
/// geometry-producing realization in declaration order, i.e. the un-consumed
/// result. This restores that "final realization per template" semantics while
/// preserving T7's multi-body-via-sub-components behavior: each *sub-component* is a
/// distinct template surfaced by the containment walk, so two product subs still
/// yield two bodies — only redundant *intra-template* intermediate lets are pruned.
///
/// `final` is the highest `r_idx` for which `terminal_handles[t][r]` is `Some`
/// (matching `step_handles.last()`); realizations that produced no handle are
/// already skipped by the walk, so including them in the skip set is harmless.
pub(crate) fn non_final_realization_indices(
    module: &reify_compiler::CompiledModule,
    terminal_handles: &[Vec<Option<KernelHandle>>],
) -> Vec<HashSet<usize>> {
    module
        .templates
        .iter()
        .enumerate()
        .map(|(t_idx, template)| {
            // Index of the final geometry-producing realization (highest r_idx
            // with a recorded terminal handle) — equals `step_handles.last()`.
            let final_idx = terminal_handles.get(t_idx).and_then(|handles| {
                handles
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(r_idx, h)| h.is_some().then_some(r_idx))
            });
            // Skip every realization that is not the final one.
            (0..template.realizations.len())
                .filter(|r_idx| Some(*r_idx) != final_idx)
                .collect()
        })
        .collect()
}

/// T7 export walk (task 3905): collect placed-product BRep handles for STEP export.
///
/// Thin wrapper over `walk_placed_realizations` that pushes each placed realization
/// as an `ExportBody` (handle_id + entity_path + default_visible) without tessellating.
///
/// `skip` (indexed by `t_idx`) lists realization indices to exclude — every
/// non-final intra-template realization (see [`non_final_realization_indices`]) —
/// so only the un-consumed final product body of each template is exported.
///
/// The `default_visible` flag uses the same OR-of-aux derivation as `surface_subtree` —
/// `default_visible == false` ⟺ aux or under-aux-sub ⟺ excluded from export by the
/// caller.  Source handles remain valid after `ApplyTransform` (T3 non-destructive).
/// Identity/undecomposable world transforms short-circuit to the source handle (no kernel op).
///
/// Cycle guard: same `depth > module.templates.len()` bound as `walk_placed_realizations`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_export_bodies(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    // True when any ancestor sub was declared `aux` (inherited down the walk).
    aux_ancestor: bool,
    // Composed world transform inherited from root, accrued down the walk.
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    // T7 robustness fix (esc-3905-277): per-template non-final realization
    // indices. A realization at (t, r) with `skip[t].contains(&r)` is a redundant
    // intra-template intermediate let (its geometry is inlined into the template's
    // final realization) and is excluded from export.
    skip: &[HashSet<usize>],
    // Optional entity-path pre-filter (T7 amendment, suggestion 3).
    // Passed through to `walk_placed_realizations`; checked BEFORE `ApplyTransform`.
    // `None` = include all non-skipped bodies (default for `build()`).
    // `Some(f)` = also skip bodies whose path doesn't satisfy `f` (used by
    // `distance_between_placed` to avoid minting transient handles for non-target paths).
    pre_filter: PlacedPreFilter<'_>,
    export_bodies: &mut Vec<ExportBody>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Combine the caller-supplied path pre-filter (gates ApplyTransform before calling
    // the walk) with the intra-template skip set (excludes boolean operand lets).
    // The combined closure is used as the walk's pre_filter so that BOTH checks happen
    // BEFORE the kernel operation is issued, saving a transient handle per skipped body.
    let combined_filter: &dyn Fn(usize, usize, &str) -> bool = &|t: usize, r: usize, path: &str| {
        !skip.get(t).is_some_and(|set| set.contains(&r)) && pre_filter.is_none_or(|f| f(t, r, path))
    };
    walk_placed_realizations(
        module,
        t_idx,
        path_prefix,
        aux_ancestor,
        composed_world,
        depth,
        terminal_handles,
        None, // handle_row_override: roots are never overridden subs
        geometry_kernels,
        default_kernel_name,
        values,
        functions,
        meta_map,
        diagnostics,
        Some(combined_filter),
        &mut |_kernel, placed_id, entity_path, default_visible, _t, _r, _diag| {
            // The combined pre_filter already excluded non-final / non-target
            // realizations before the transform, so every body reaching this
            // visitor should be collected unconditionally.
            export_bodies.push(ExportBody {
                entity_path,
                handle_id: placed_id,
                default_visible,
            });
        },
    );
}

/// Phase-B containment-tree surfacing (T5 steps 4/6/10).
///
/// Thin wrapper over `walk_placed_realizations` that tessellates each placed
/// realization into a `MeshSurface`.
///
/// Depth-first walk from a root template: surface the current template's
/// realizations under `path_prefix` (the dotted entity-path prefix that precedes
/// the `#realization[i]` suffix), then recurse into each NON-collection sub with
/// `path_prefix` extended by `.<sub-name>`. The realization's terminal handle is
/// looked up positionally from `terminal_handles[t_idx][r_idx]` (recorded by
/// Phase A); `None` (no geometry produced) is skipped. The default kernel is
/// re-borrowed by name to tessellate, mirroring the pre-T5 terminal-handle path.
///
/// Entity-path scheme (PRD §11.2 `parent.sub#realization[i]`): for a ROOT,
/// `path_prefix` is the template name, so the surface path equals
/// `realization.id.to_string()` (`<entity>#realization[<index>]`) — bit-identical
/// to pre-T5. For a DESCENDANT, `path_prefix` is `<root>.<sub>…`, giving e.g.
/// `Assembly.c#realization[0]`.
///
/// `depth` bounds the recursion: a simple path in an acyclic sub-graph visits at
/// most `templates.len()` distinct templates, so a depth past that implies a
/// non-collection sub cycle (e.g. a recursive structure reached via a root). We
/// stop there to avoid unbounded recursion — runtime recursion unfolding is a
/// separate concern (`unfold.rs`) and out of this surfacing path's scope.
///
/// step-10 composes each sub's `at` pose down the walk (`eval_sub_pose` +
/// `compose_pose_chain`) and applies the resulting world transform via
/// `GeometryOp::ApplyTransform` on the default kernel before tessellation;
/// identity / un-placed poses short-circuit and tessellate the handle directly.
/// Step-6 threads `aux` inheritance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_subtree(
    module: &reify_compiler::CompiledModule,
    t_idx: usize,
    path_prefix: &str,
    // True when any ancestor sub on the path to this template was declared
    // `aux` (PRD §3 rule 2: an aux sub means the whole contained subtree has no
    // external geometric effect). Inherited down the walk; ORed with each
    // realization's own `aux` to derive `default_visible`. `false` at roots.
    aux_ancestor: bool,
    // T5 step-10: the composed world transform inherited from the root down to
    // this template (`pose_root ∘ … ∘ pose_parent`). Identity at roots. When
    // non-identity, applied to each realization's terminal geometry (via
    // `GeometryOp::ApplyTransform` on the default kernel) before tessellation so
    // the descendant surfaces at its composed world pose.
    composed_world: &reify_ir::Value,
    depth: usize,
    terminal_handles: &[Vec<Option<KernelHandle>>],
    // `&mut` so a non-identity `composed_world` can `execute` an ApplyTransform
    // on the default kernel before tessellating; the pre-step-10 walk only read
    // the kernel to tessellate.
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    tessellation_budgets: &[Vec<f64>],
    // T5 step-10: parent value / function / meta context for evaluating each
    // sub's `at` pose via `eval_sub_pose`.
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    meshes: &mut Vec<crate::MeshSurface>,
    diagnostics: &mut Vec<Diagnostic>,
    // Determinacy β (task 4198): when `true`, call
    // `kernel.measure_mesh_deviation` for each successfully tessellated
    // occurrence and insert the result into `achieved_repr_tol`. `false`
    // by default — zero overhead when γ assertions are not active.
    capture_repr_tol: bool,
    // Determinacy β (task 4198): per-build map from realized-occurrence name
    // ("{entity}#realization[{index}]") to sampled max facet-chord deviation
    // in SI metres. Populated here (the unique site holding kernel + placed_id
    // + fresh mesh + entity_path simultaneously) when `capture_repr_tol` is
    // true. Recording is skip-on-None and skip-on-empty-mesh so the map never
    // contains misleading 0.0 entries (honest absence = missing key, B3).
    achieved_repr_tol: &mut BTreeMap<String, f64>,
) {
    walk_placed_realizations(
        module,
        t_idx,
        path_prefix,
        aux_ancestor,
        composed_world,
        depth,
        terminal_handles,
        None, // handle_row_override: roots are never overridden subs
        geometry_kernels,
        default_kernel_name,
        values,
        functions,
        meta_map,
        diagnostics,
        // Tessellation surfaces all bodies (no path filter needed); pass None to
        // avoid any pre-filter overhead on the hot path.
        None,
        &mut |kernel, placed_id, entity_path, default_visible, t, r, diag| {
            let budget = tessellation_budgets[t][r];
            match kernel.tessellate(placed_id, budget) {
                Ok(mesh) => {
                    // Determinacy β (task 4198): record the sampled max
                    // facet-chord deviation BEFORE moving entity_path into
                    // MeshSurface. Gated on `capture_repr_tol` so the hot
                    // path pays zero overhead (no BRepExtrema projection,
                    // no actor round-trip) when γ assertions are not active.
                    // Only record for non-empty meshes — an empty mesh yields
                    // honest absence (missing key), never 0.0.
                    // measure_mesh_deviation returns None for non-OCCT kernels
                    // (default-absent trait method, B3). Anti-circularity: the
                    // metric takes no tolerance argument and measures actual
                    // facet-chord error, NOT the configured deflection budget.
                    if capture_repr_tol
                        && !mesh.indices.is_empty()
                        && let Some(dev) = kernel.measure_mesh_deviation(placed_id, &mesh)
                    {
                        achieved_repr_tol.insert(entity_path.clone(), dev);
                    }
                    // step-6: hide iff any `aux` ancestor sub OR this realization's
                    // own `aux` let. aux bodies are still tessellated and shipped —
                    // only hidden by default.
                    meshes.push(crate::MeshSurface {
                        entity_path,
                        mesh,
                        default_visible,
                    });
                }
                Err(e) => {
                    diag.push(Diagnostic::error(format!("tessellation error: {}", e)));
                }
            }
        },
    );
}

#[cfg(test)]
mod tests;
