//! Eval-layer wiring for the `body_mass_props` stdlib fn (RBD-β, task 3829;
//! PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! This is the `Value`/diagnostic/kernel-seam half of the modal-style split:
//! the dependency-free `f64` math (density ladder, analytic box inertia) lives
//! in `reify_stdlib::dynamics::mass_props`; this module extracts `Value`s,
//! emits diagnostics, wires the (deferred) geometry-kernel seam, and assembles
//! the `MassProperties` `Value::StructureInstance`.
//!
//! Two entry points (mirroring `geometry_ops::try_eval_*`):
//!   * [`eval_body_mass_props_core`] — pure core: given an already-resolved
//!     body `Value`, an optional explicit density arg, and an injected
//!     geometric-query closure, runs the density ladder, emits
//!     `E_DynamicsNoDensity` (hard error) when no density is resolvable, and
//!     builds the `MassProperties` instance. Kernel-free and unit-testable.
//!   * [`try_eval_body_mass_props`] — dispatch recognition for a
//!     `body_mass_props(...)` call cell: when the body is a
//!     `Value::GeometryHandle`, builds a kernel-backed `geom_query` closure
//!     (`Volume` / `CenterOfMass` / `InertiaTensor`) and routes it through
//!     `eval_body_mass_props_core` so the density ladder runs once and its
//!     resolved density feeds each KGQ query. On kernel failure or missing
//!     handle, the geometric fields degrade to `Value::Undef` with a Warning
//!     (mirrors `geometry_ops::dispatch_inertia_tensor`'s defensive contract).

use std::sync::Arc;

use reify_core::dimension::DimensionVector;
use reify_core::{ContentHash, Diagnostic, DiagnosticCode};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::dynamics::eval::{
    default_frame3, inverse_dynamics_sample, motion_trajectory_samples,
};
use reify_stdlib::dynamics::mass_props::{resolve_density, resolve_density_strict};
use reify_stdlib::dynamics::rnea::default_gravity;
use reify_stdlib::dynamics::trampoline::{InverseDynamicsCacheKey, body_solid_hashes};

use crate::arg_acceptance::{Acceptance, accept_arg, density_spec};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Sentinel `StructureTypeId` for engine-assembled (registry-free) instances.
/// Mirrors `modal_ops::degenerate_modal_result`: the nominal `type_name` is the
/// source of truth for downstream hooks (the MassProperties PSD validator keys
/// on `type_name == "MassProperties"`, not on the id).
const REGISTRY_FREE_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

/// Extract an `f64` from a numeric value cell (`Int` / `Real` / dimensioned
/// `Scalar`). Mirrors `dynamics_psd`'s `cell_f64`; non-numeric cells yield
/// `None`.
fn cell_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Real(r) => Some(*r),
        Value::Scalar { si_value, .. } => Some(*si_value),
        _ => None,
    }
}

/// Read `body.material.density` as an `f64`, if the body is a StructureInstance
/// whose `material` field is itself a StructureInstance carrying a numeric
/// `density`. Any missing link (no `material`, non-structure material, no
/// `density`, non-numeric density) yields `None` — the Material ladder rung is
/// simply skipped.
fn body_material_density(body: &Value) -> Option<f64> {
    if let Value::StructureInstance(data) = body
        && let Some(Value::StructureInstance(material)) = data.fields.get("material")
        && let Some(cell) = material.fields.get("density")
    {
        return cell_f64(cell);
    }
    None
}

/// A human-readable label for the body, used in the default-density warning.
/// Prefers an explicit `name : String` field, falling back to the structure's
/// nominal `type_name`, then a generic placeholder.
fn body_label(body: &Value) -> String {
    if let Value::StructureInstance(data) = body {
        if let Some(Value::String(name)) = data.fields.get("name") {
            return name.clone();
        }
        return data.type_name.clone();
    }
    "<body>".to_string()
}

/// Run the fn-level density priority ladder for `body_mass_props`. Returns
/// `Some(density_kg_per_m3)` when a density is resolved, or `None` when no
/// density is resolvable (degrade result to Undef) or when the explicit arg is
/// rejected/undefined.
///
/// When `density_arg` is `Some(v)`, the value is routed through
/// [`accept_arg`] against [`density_spec`]:
/// - [`Acceptance::Accepted`] → `Some(si_value)` (dimension-correct Density scalar).
/// - [`Acceptance::Undefined`] → `None` (quiet degrade; data-indeterminacy).
/// - [`Acceptance::Rejected`] → push `Diagnostic::warning(rej.message(...))` → `None`.
///
/// When `density_arg` is `None`, the no-explicit-arg path walks the Material
/// rung via [`resolve_density_strict`] (explicit→material, no water tail). If
/// neither source resolves a density — no explicit arg AND no body
/// `Material.density` (incl. no `default Material = …` in scope, which the
/// conformance checker would have injected at compile time) — emits
/// `E_DynamicsNoDensity` (`Severity::Error`) naming the three fixes and
/// returns `None` so the geometric fields degrade to `Value::Undef` (same
/// degrade shape as a rejected explicit arg, ambient-default-material C task 4498).
///
/// **Severity note:** A *rejected* explicit arg (wrong dimension) is a
/// `Severity::Warning` + `None` degrade, while *no resolvable density at all*
/// (this tail) is a `Severity::Error` + `None` degrade.  The asymmetry is
/// intentional: a dimensionally-wrong arg is a type mismatch that may be a
/// transient authoring error (a warning keeps the skeleton visible), whereas
/// a completely missing density has no fallback and is an unconditional hard
/// error per PRD §7(iii) (removing the water default must move code
/// works→loud-error, never to a different silent value).
///
/// `pub(crate)` so the modal_ops cross-path convergence test (task 4470 step-3)
/// can feed the same material Value to both the modal and dynamics resolution
/// paths without duplicating the ladder logic.
pub(crate) fn resolve_body_density(
    body: &Value,
    density_arg: Option<&Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    if let Some(v) = density_arg {
        // Explicit arg present: dimension-check it via the shared acceptance helper.
        return match accept_arg(v, &density_spec()) {
            Acceptance::Accepted(si) => Some(si),
            Acceptance::Undefined => None, // quiet degrade (data-indeterminacy)
            Acceptance::Rejected(rej) => {
                diagnostics.push(Diagnostic::warning(
                    rej.message("body_mass_props", "density"),
                ));
                None
            }
        };
    }

    // No explicit arg: walk the Material rung only (no water tail).
    let material = body_material_density(body);
    match resolve_density_strict(None, material) {
        Some((rho, _)) => Some(rho),
        None => {
            // Neither an explicit density nor a body Material density is
            // available. This is a hard error (E_DynamicsNoDensity): the
            // user must pass an explicit density, give the body a Material
            // with a density, or declare `default Material = …` in scope
            // (which the conformance checker injects at compile time).
            diagnostics.push(
                Diagnostic::error(format!(
                    "body_mass_props('{}'): no density resolvable — pass an explicit \
                     density argument, give the body a Material with a density, or \
                     declare `default Material = …` in scope",
                    body_label(body),
                ))
                .with_code(DiagnosticCode::DynamicsNoDensity),
            );
            None
        }
    }
}

/// Mass `Value` for the `MassProperties.mass : Mass` field (dimensioned scalar).
fn mass_value(mass: f64) -> Value {
    Value::Scalar {
        si_value: mass,
        dimension: DimensionVector::MASS,
    }
}

/// Centre-of-mass `Value` for `MassProperties.com : Point3<Length>` — a
/// `Value::Point` of three Length-dimensioned scalars.
fn com_value(com: [f64; 3]) -> Value {
    Value::Point(
        com.iter()
            .map(|&x| Value::Scalar {
                si_value: x,
                dimension: DimensionVector::LENGTH,
            })
            .collect(),
    )
}

/// Inertia `Value` for `MassProperties.inertia : Matrix<3,3,MomentOfInertia>` —
/// a 3×3 `Value::Matrix` of `MomentOfInertia`-dimensioned `Value::Scalar` cells
/// (kg·m²). Each cell carries `si_value` == the raw f64 from the geometric query
/// and `dimension == DimensionVector::MOMENT_OF_INERTIA`.
///
/// The PSD hook (`dynamics_psd`) and the RNEA extraction path
/// (`inertia_3x3_from_value`) both read cells via `cell_f64`, which accepts
/// `Value::Scalar{si_value,..}` and strips to `si_value` — so all downstream
/// numeric outputs (eigenvalues, RNEA τ) are byte-identical to the former
/// `Value::Real` encoding. Mirrors `com_value`'s `LENGTH` pattern.
fn inertia_value(inertia: [[f64; 3]; 3]) -> Value {
    Value::Matrix(
        inertia
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&x| Value::Scalar {
                        si_value: x,
                        dimension: DimensionVector::MOMENT_OF_INERTIA,
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Assemble a `MassProperties` `Value::StructureInstance` from its four field
/// values. The geometric fields (`mass`, `com`, `inertia`) are passed as
/// `Value`s so this single assembler serves both the concrete-geometry core and
/// the deferred-kernel dispatch path (which passes `Value::Undef` for them).
/// `origin` is a default zero-`Frame3` (task 4547 retarget — was a `Real`
/// placeholder), minted via `reify_stdlib`'s shared [`default_frame3`] so this
/// producer and `make_mass_properties` emit an identical `origin`.
///
/// Reuses the `modal_ops`/`StructureInstanceData` construction pattern (task
/// 3822 MassProperties structure_def, `dynamics.ri`).
fn assemble_mass_properties(mass: Value, com: Value, inertia: Value) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("mass".to_string(), mass),
        ("com".to_string(), com),
        ("inertia".to_string(), inertia),
        ("origin".to_string(), default_frame3()),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE_TYPE_ID,
        type_name: "MassProperties".to_string(),
        version: 1,
        fields,
    }))
}

/// Pure eval core for `body_mass_props`: resolve the density (emitting
/// `E_DynamicsNoDensity` when no density is resolvable), invoke the injected
/// geometric query, and assemble the `MassProperties` instance.
///
/// `geom_query` is the kernel seam abstracted as a closure `density -> (mass,
/// com, inertia)`; this keeps the core kernel-free and exactly unit-testable
/// (the tests inject `reify_stdlib::dynamics::mass_props::uniform_box_inertia`).
/// `try_eval_body_mass_props` builds a kernel-backed `geom_query` closure
/// (task 4237 / KGQ-λ seam) and routes it through this core so the density
/// ladder resolves once and its result feeds each KGQ query.
pub fn eval_body_mass_props_core(
    body: &Value,
    density_arg: Option<&Value>,
    geom_query: impl Fn(f64) -> (f64, [f64; 3], [[f64; 3]; 3]),
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    match resolve_body_density(body, density_arg, diagnostics) {
        Some(density) => {
            let (mass, com, inertia) = geom_query(density);
            assemble_mass_properties(mass_value(mass), com_value(com), inertia_value(inertia))
        }
        // Rejected or undefined explicit density: degrade to Undef without
        // invoking the geometry query (mirrors kernel-failure degradation shape).
        None => assemble_mass_properties(Value::Undef, Value::Undef, Value::Undef),
    }
}

/// Resolve a call-argument `CompiledExpr` to the `Value` it denotes: a
/// `ValueRef` is looked up in `values`; an inline `Literal` yields its baked
/// value. Any other expr shape (or a `ValueRef` to an absent cell) yields
/// `None` — mirroring the "unsupported arg shape → fall through" contract of
/// `geometry_ops::resolve_density_arg` / `resolve_int_value_ref`.
///
/// **Cross-link:** this function intentionally collapses both a missing-cell
/// `ValueRef` and an unsupported expr shape into a single `None`, losing the
/// distinction between the two. The density-arg resolution in
/// [`try_eval_body_mass_props`] uses an inline `match` over the same
/// `ValueRef` / `Literal` / other classification but needs a three-way
/// outcome (present value / missing cell / unsupported shape) so it can route
/// *missing cell* to a quiet data-indeterminacy degrade and *unsupported shape*
/// to a loud `Warning` degrade. If either classification is extended, keep the
/// two in sync.
fn resolve_arg_value<'a>(
    expr: &'a reify_ir::CompiledExpr,
    values: &'a reify_ir::ValueMap,
) -> Option<&'a Value> {
    match &expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => values.get(id),
        reify_ir::CompiledExprKind::Literal(v) => Some(v),
        _ => None,
    }
}

/// Extract the `GeometryHandleId` from a `Value::GeometryHandle` body.
/// Returns `None` for any other `Value` shape (e.g. a `Block` StructureInstance
/// built by the dispatch unit tests, which falls through to the deferred-Undef
/// path).
fn body_geometry_handle(body: &Value) -> Option<reify_ir::GeometryHandleId> {
    match body {
        Value::GeometryHandle { kernel_handle, .. } => {
            // TODO(#4652): step-8 converts None to genuine decline; no None
            // producer exists until eval-mint in step-4.
            Some(kernel_handle.unwrap_or(reify_ir::GeometryHandleId::INVALID))
        }
        _ => None,
    }
}

/// Issue the three density-aware KGQ queries for `body_mass_props` against a
/// known geometry handle, using the resolved `density` (kg/m³) supplied by
/// [`eval_body_mass_props_core`] so density resolution happens exactly once
/// and all three queries see the same value.
///
/// Returns `(mass, com, inertia)` on success, or a [`reify_ir::QueryError`] on
/// the first failing/malformed query. The caller is responsible for emitting
/// diagnostics and downgrading to `Value::Undef` on error.
#[allow(clippy::type_complexity)]
fn query_body_mass_props_from_kernel(
    kernel: &dyn reify_ir::GeometryKernel,
    handle: reify_ir::GeometryHandleId,
    density: f64,
) -> Result<(f64, [f64; 3], [[f64; 3]; 3]), reify_ir::QueryError> {
    // (a) Volume → mass = density × V
    let vol_reply = kernel.query(&reify_ir::GeometryQuery::Volume(handle))?;
    let volume = cell_f64(&vol_reply).ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!(
            "body_mass_props Volume reply is not numeric: {vol_reply:?}"
        ))
    })?;
    let mass = density * volume;

    // (b) CenterOfMass{handle, density} → {"x":_,"y":_,"z":_} JSON → [f64;3]
    let com_reply = kernel.query(&reify_ir::GeometryQuery::CenterOfMass { handle, density })?;
    let com =
        crate::topology_selectors::parse_xyz_value(&com_reply, "body_mass_props CenterOfMass")?;

    // (c) InertiaTensor{handle, density} → List-of-lists → [[f64;3];3]
    let inertia_reply =
        kernel.query(&reify_ir::GeometryQuery::InertiaTensor { handle, density })?;
    let inertia = crate::dynamics_psd::inertia_3x3_from_value(&inertia_reply).ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!(
            "body_mass_props InertiaTensor reply is not a 3×3 matrix: {inertia_reply:?}"
        ))
    })?;

    Ok((mass, com, inertia))
}

/// Dispatch recognition for a `body_mass_props(body, density?)` call cell,
/// mirroring `geometry_ops::try_eval_*`.
///
/// Returns `Some(MassProperties)` when `default_expr` is a `FunctionCall` named
/// `body_mass_props` whose body argument resolves (against `values`) to a
/// `Value`; returns `None` for any other expr — a non-call shape, a different
/// function name, a missing/unresolvable body arg — so the caller leaves the
/// cell's existing value untouched (the geometry_ops `None`-means-skip contract).
///
/// The density ladder still runs on the recognised path: the optional second
/// argument (explicit `density`) and the body's `Material.density` feed
/// [`resolve_body_density`], which emits `E_DynamicsNoDensity` (hard error)
/// when no density is resolvable.
///
/// ## Kernel seam (task 4237 / KGQ-λ)
/// When `body` is a `Value::GeometryHandle`, a kernel-backed `geom_query`
/// closure is built over the three existing KGQ variants
/// (`Volume` / `CenterOfMass{density}` / `InertiaTensor{density}`) and routed
/// through [`eval_body_mass_props_core`] so density is resolved exactly once.
/// On any kernel error or malformed reply, a `Diagnostic::warning` is emitted
/// and the geometric fields downgrade to `Value::Undef` (mirrors
/// `geometry_ops::dispatch_inertia_tensor`'s defensive contract). When `body`
/// has no geometry handle (e.g. a Block StructureInstance), the deferred-Undef
/// path is preserved so callers leaving such bodies stay green.
pub fn try_eval_body_mass_props(
    default_expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    // (1) Must be a FunctionCall — anything else (e.g. a bare ValueRef) is not
    // a body_mass_props call site; leave the cell untouched.
    let (function, args) = match &default_expr.kind {
        reify_ir::CompiledExprKind::FunctionCall { function, args } => (function, args),
        _ => return None,
    };

    // (2) Must be the `body_mass_props` helper. Checked BEFORE any arg
    // resolution or diagnostic emission so unrelated calls (e.g. `volume`) are
    // silently skipped.
    if function.name != "body_mass_props" {
        return None;
    }

    // (2a) Arity guard. The compiler signature `body_mass_props(body, density?)`
    // is the primary arity gate, but the `expr.rs` name-recognition path assigns
    // the `MassProperties` result type without an arity check, so a
    // malformed-arity call can still reach this dispatch. Surface it as an
    // `E_DynamicsBodyMassPropsArity` error instead of silently returning `None`
    // (which would leave a `MassProperties`-typed cell holding the pure-eval
    // `Undef` with no diagnostic anywhere). The cell is left at its `Undef`
    // pure-eval value — no `MassProperties` is assembled for a malformed call.
    if args.is_empty() || args.len() > 2 {
        diagnostics.push(
            Diagnostic::error(format!(
                "body_mass_props expects 1 or 2 arguments (body, density?), got {}",
                args.len(),
            ))
            .with_code(DiagnosticCode::DynamicsBodyMassPropsArity),
        );
        return None;
    }

    // (3) Resolve the body argument (args[0]). A missing or unresolvable body
    // arg returns None (cell left untouched) rather than a malformed instance.
    let body = resolve_arg_value(args.first()?, values)?;

    // (4) Optional explicit density argument (args[1]).
    //
    // We distinguish three cases to close Hole 2 ("explicit arg silently
    // ignored"):
    //   - Absent (no args[1])        → density_arg = None  (run the ladder)
    //   - ValueRef to a present cell → density_arg = Some(value)
    //   - ValueRef to a MISSING cell → quiet data-indeterminacy degrade (Undef)
    //   - Literal                    → density_arg = Some(literal_value)
    //   - Any other expr shape       → loud degrade with a Warning diagnostic
    //
    // The early-return path runs BEFORE the kernel-handle/no-handle match so a
    // doomed density never enters the kernel closure.
    //
    // Cross-link: the body arg (args[0]) uses resolve_arg_value(), which
    // applies the same ValueRef / Literal / other shape classification but
    // collapses missing-cell and unsupported-shape into a single None. The
    // density arg needs the richer three-way outcome above, so it uses this
    // inline match. If the shape classification changes, keep both in sync.
    let density_arg: Option<&Value> = match args.get(1) {
        None => None, // 1-arg form: run the Material/water ladder
        Some(e) => match &e.kind {
            reify_ir::CompiledExprKind::ValueRef(id) => {
                match values.get(id) {
                    Some(v) => Some(v),
                    // Missing cell: quiet data-indeterminacy degrade.
                    None => {
                        return Some(assemble_mass_properties(
                            Value::Undef,
                            Value::Undef,
                            Value::Undef,
                        ));
                    }
                }
            }
            reify_ir::CompiledExprKind::Literal(v) => Some(v),
            // Unsupported expr shape: loud degrade with a Warning (never fall
            // through to the Material/water ladder, which would silently use a
            // completely different density than what the caller supplied).
            _ => {
                diagnostics.push(Diagnostic::warning(
                    "body_mass_props: density argument could not be resolved \
                     (unsupported expression shape); mass properties set to Undef"
                        .to_string(),
                ));
                return Some(assemble_mass_properties(
                    Value::Undef,
                    Value::Undef,
                    Value::Undef,
                ));
            }
        },
    };

    // (5)/(6) Kernel seam (task 4237 / KGQ-λ): if the body is a
    // GeometryHandle, build a kernel-backed geom_query closure and route it
    // through eval_body_mass_props_core so the density ladder runs once and
    // its result feeds each KGQ query. On error, capture via RefCell and
    // downgrade to Undef (with a Warning in step-4). No handle → deferred path.
    match body_geometry_handle(body) {
        Some(h) => {
            let err: std::cell::RefCell<Option<reify_ir::QueryError>> =
                std::cell::RefCell::new(None);
            // INVARIANT: eval_body_mass_props_core calls geom_query exactly once
            // (l.188 — single `geom_query(density)` call after density resolution).
            // If the core ever called the closure more than once, only the LAST
            // error would be captured and any prior successful triple would be
            // silently overwritten with the (0,0,0) sentinel, producing wrong mass
            // properties with no diagnostic. The debug_assert below makes that
            // contract violation fail loudly under any test run.
            let invocation_count = std::cell::Cell::new(0u32);
            let q = |d: f64| {
                invocation_count.set(invocation_count.get() + 1);
                match query_body_mass_props_from_kernel(kernel, h, d) {
                    Ok(triple) => triple,
                    Err(e) => {
                        *err.borrow_mut() = Some(e);
                        (0.0_f64, [0.0_f64; 3], [[0.0_f64; 3]; 3])
                    }
                }
            };
            let mp = eval_body_mass_props_core(body, density_arg, q, diagnostics);
            debug_assert!(
                invocation_count.get() <= 1,
                "eval_body_mass_props_core invoked geom_query {} time(s); expected \
                 at most 1 — the RefCell error-capture assumes a single call and \
                 would silently overwrite errors with (0,0,0) sentinels on a \
                 multi-call core (0 invocations is legitimate when a rejected or \
                 undefined explicit density skips the geometry query)",
                invocation_count.get()
            );
            if let Some(e) = err.borrow().as_ref() {
                // Defensive downgrade: a kernel error for any of the three
                // mass-properties queries (Volume / CenterOfMass / InertiaTensor)
                // degrades the geometric fields to Undef and emits one Warning
                // (mirrors geometry_ops::dispatch_inertia_tensor's contract).
                // The MassProperties PSD hook classifies Undef inertia as Skip,
                // so no spurious E_DynamicsInertiaNotPSD is generated.
                diagnostics.push(Diagnostic::warning(format!(
                    "body_mass_props kernel query failed — geometric fields \
                     (mass/com/inertia) set to Undef: {e}"
                )));
                Some(assemble_mass_properties(
                    Value::Undef,
                    Value::Undef,
                    Value::Undef,
                ))
            } else {
                Some(mp)
            }
        }
        None => {
            // No geometry handle: run the density ladder for its diagnostic
            // side effect, then return the deferred-Undef sentinel.
            let _density = resolve_body_density(body, density_arg, diagnostics);
            Some(assemble_mass_properties(
                Value::Undef,
                Value::Undef,
                Value::Undef,
            ))
        }
    }
}

/// Build-time mechanism-mass pre-derivation pass (task 4472, rung (b)).
///
/// For a `Value::Map` with `kind == "mechanism"`, iterates over the mechanism's
/// `bodies` list and, for each body whose `solid` is a `Value::GeometryHandle`,
/// issues the three KGQ queries (`Volume` / `CenterOfMass` / `InertiaTensor`)
/// via `query_body_mass_props_from_kernel` and writes the resulting
/// `MassProperties` StructureInstance into a NEW **additive** `derived_mass_props`
/// sibling key on that body map — the original `solid` GeometryHandle is NOT
/// replaced.
///
/// Density defaults to the water default (1000 kg/m³) — mechanism bodies are
/// `Value::Map` records (not `Value::StructureInstance`), so
/// `body_material_density` — which only matches `StructureInstance` — always
/// returns `None` here. `None` is passed to `resolve_density` explicitly so
/// the two density paths (user-facing `body_mass_props` vs this build pass)
/// cannot silently diverge if `body_material_density` is later extended. The
/// `E_DynamicsNoDensity` error is NOT emitted here — that error belongs
/// to the user-facing `body_mass_props()` call, not the internal build pass.
///
/// Returns `Some(patched_mechanism)` iff at least one body was successfully
/// patched. Returns `None` for any non-mechanism value, or when no body
/// carried a geometry handle, or when all geometry-backed bodies failed their
/// kernel queries — mirroring the `None`-means-skip post-process contract.
///
/// **Idempotency guard:** bodies that already carry a `derived_mass_props`
/// `MassProperties` from a prior pass are skipped without issuing any kernel
/// queries. This guard relies on the invariant that **any upstream geometry
/// change produces a fresh body `Value` without the stale `derived_mass_props`
/// key** — i.e. the mechanism cell is rebuilt from scratch when its geometry
/// changes, so a stale derived value cannot silently survive into the new build.
/// If this invariant ever breaks (e.g. partial incremental eval that mutates
/// body maps in place rather than rebuilding them), the guard would need to be
/// strengthened to also record and compare the source handle used at derivation
/// time.
///
/// **Failed bodies are retried on every pass:** a body whose kernel query fails
/// is left without `derived_mass_props` and is therefore NOT covered by the
/// idempotency guard. On every subsequent post-process pass (build /
/// `build_snapshot` / `tessellate_from_values`) that body will be re-queried
/// and a fresh `Diagnostic::warning` will be emitted. This is intentional: once
/// the kernel can answer (e.g. after a kernel-side bugfix or a geometry update),
/// the body self-heals on the next pass without any recovery logic.
///
/// On a kernel-query failure for an individual body, a `Diagnostic::warning` is
/// emitted and that body is left unpatched (skipped), but the pass continues
/// with the remaining bodies.
pub fn derive_mechanism_mass_props(
    value: &Value,
    kernel: &dyn reify_ir::GeometryKernel,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    // Recognise mechanism: must be a Map with kind=="mechanism".
    let mech_map = match value {
        Value::Map(m) => m,
        _ => return None,
    };
    if mech_map.get(&Value::String("kind".to_string()))
        != Some(&Value::String("mechanism".to_string()))
    {
        return None;
    }

    // Extract bodies list.
    let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        _ => return None,
    };

    // Fast path: if no body is both (a) geometry-backed and (b) not already
    // derived, there is nothing to patch. Return None early to avoid allocating
    // and deep-cloning the full bodies vector only to discard it when
    // `any_patched` is false. This matters most for mechanisms with many bodies
    // but no fresh geometry-handle body (e.g. all already derived, or all with
    // non-handle solids).
    let has_patchable = bodies.iter().any(|b| {
        let bm = match b {
            Value::Map(m) => m,
            _ => return false,
        };
        // A body is patchable iff its solid is a GeometryHandle AND it does not
        // already carry a derived_mass_props MassProperties (idempotency guard).
        matches!(
            bm.get(&Value::String("solid".to_string())),
            Some(Value::GeometryHandle { .. })
        ) && !matches!(
            bm.get(&Value::String("derived_mass_props".to_string())),
            Some(Value::StructureInstance(d)) if d.type_name == "MassProperties"
        )
    });
    if !has_patchable {
        return None;
    }

    let mut patched_bodies: Vec<Value> = Vec::with_capacity(bodies.len());
    let mut any_patched = false;

    for body_value in bodies {
        let body_map = match body_value {
            Value::Map(b) => b,
            _ => {
                patched_bodies.push(body_value.clone());
                continue;
            }
        };

        // Idempotency guard: skip bodies that already carry a valid
        // derived_mass_props MassProperties from a prior pass. Avoids
        // redundant kernel round-trips when the same mechanism flows through
        // build / build_snapshot / tessellate_from_values more than once.
        if matches!(
            body_map.get(&Value::String("derived_mass_props".to_string())),
            Some(Value::StructureInstance(d)) if d.type_name == "MassProperties"
        ) {
            patched_bodies.push(body_value.clone());
            continue;
        }

        let solid = body_map.get(&Value::String("solid".to_string()));
        let handle = match solid {
            Some(Value::GeometryHandle { kernel_handle, .. }) => {
                // TODO(#4652): step-8 converts None to genuine decline; no None
                // producer exists until eval-mint in step-4.
                kernel_handle.unwrap_or(reify_ir::GeometryHandleId::INVALID)
            }
            _ => {
                // Not a geometry handle — leave unpatched.
                patched_bodies.push(body_value.clone());
                continue;
            }
        };

        // Resolve density. Mechanism bodies are Value::Map records (not
        // StructureInstance), so body_material_density — which only matches
        // StructureInstance — always returns None for body_value here. Pass None
        // explicitly to make the unreachability clear and prevent silent divergence
        // between this pass and the user-facing body_mass_props density ladder.
        let (density, _source) = resolve_density(None, None);

        // Issue the three KGQ queries.
        //
        // NOTE: We call query_body_mass_props_from_kernel + assemble_mass_properties
        // directly here rather than routing through eval_body_mass_props_core
        // because the two functions have different error contracts:
        // - eval_body_mass_props_core always returns a Value (on kernel failure it
        //   inserts Value::Undef sentinel fields via the RefCell error-capture dance
        //   in try_eval_body_mass_props).
        // - This build pass wants to SKIP the body entirely on kernel failure (no
        //   derived_mass_props inserted), not insert an Undef-carrying MassProperties.
        // Routing through eval_body_mass_props_core would silently produce an
        // Undef-carrying MassProperties instead of skipping the body.
        match query_body_mass_props_from_kernel(kernel, handle, density) {
            Ok((mass, com, inertia)) => {
                let mp = assemble_mass_properties(
                    mass_value(mass),
                    com_value(com),
                    inertia_value(inertia),
                );
                // Write derived_mass_props additively — preserve the existing body
                // keys (including solid).
                let mut new_body: std::collections::BTreeMap<Value, Value> = body_map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                new_body.insert(Value::String("derived_mass_props".to_string()), mp);
                patched_bodies.push(Value::Map(new_body));
                any_patched = true;
            }
            Err(e) => {
                diagnostics.push(Diagnostic::warning(format!(
                    "derive_mechanism_mass_props: kernel query failed for body with handle \
                     {handle:?}: {e}; body will not carry derived_mass_props"
                )));
                patched_bodies.push(body_value.clone());
            }
        }
    }

    if !any_patched {
        return None;
    }

    // Rebuild mechanism Map with the patched bodies list.
    let mut new_mech: std::collections::BTreeMap<Value, Value> = mech_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    new_mech.insert(
        Value::String("bodies".to_string()),
        Value::List(patched_bodies),
    );
    Some(Value::Map(new_mech))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DiagnosticCode, Severity};
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use reify_stdlib::dynamics::mass_props::uniform_box_inertia;

    use reify_core::{ContentHash, Type, ValueCellId};
    use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, ValueMap};
    use reify_test_support::mocks::MockGeometryKernel;

    /// Fixed box extents for the injected geometric-query stub. Distinct so all
    /// three inertia diagonal entries differ.
    const DIMS: [f64; 3] = [0.1, 0.2, 0.3];

    /// Build a `Material` StructureInstance, optionally carrying a `density`
    /// field (`Value::Real`, matching the canonical `Material.density : Real`).
    fn material(density: Option<f64>) -> Value {
        let mut entries: Vec<(String, Value)> = Vec::new();
        if let Some(d) = density {
            entries.push(("density".to_string(), Value::Real(d)));
        }
        let fields: PersistentMap<String, Value> = entries.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Material".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a Physical-shaped body whose `material` field is a `Material` with
    /// the given (optional) density.
    fn body(material_density: Option<f64>) -> Value {
        let fields: PersistentMap<String, Value> =
            [("material".to_string(), material(material_density))]
                .into_iter()
                .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Block".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Extract an f64 from a numeric cell (`Real` / `Int` / `Scalar`),
    /// delegating to the module's `cell_f64` (shared via `use super::*`) so the
    /// numeric-extraction logic is single-sourced within this module rather than
    /// re-spelled in the tests. Panics on a non-numeric cell (the tests want a
    /// hard failure, not the `None` the production helper returns).
    fn num(v: &Value) -> f64 {
        cell_f64(v).unwrap_or_else(|| panic!("expected numeric value, got {v:?}"))
    }

    /// Extract the three component magnitudes from a `Value::Point`.
    fn point3(v: &Value) -> [f64; 3] {
        match v {
            Value::Point(comps) => {
                assert_eq!(comps.len(), 3, "com must have 3 components");
                [num(&comps[0]), num(&comps[1]), num(&comps[2])]
            }
            other => panic!("expected com to be a Value::Point, got {other:?}"),
        }
    }

    /// Pull the four MassProperties fields out of an assembled result, asserting
    /// the type_name and parsing mass / com / inertia.
    fn mass_props_fields(result: &Value) -> (f64, [f64; 3], [[f64; 3]; 3]) {
        let data = match result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a MassProperties StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "MassProperties",
            "assembled instance must be type MassProperties"
        );
        let mass = num(data.fields.get("mass").expect("mass field"));
        let com = point3(data.fields.get("com").expect("com field"));
        let inertia = crate::dynamics_psd::inertia_3x3_from_value(
            data.fields.get("inertia").expect("inertia field"),
        )
        .expect("inertia field must parse as a 3×3 matrix via inertia_3x3_from_value");
        (mass, com, inertia)
    }

    fn assert_close(got: f64, want: f64, what: &str) {
        assert!(
            (got - want).abs() < 1e-12,
            "{what}: expected {want}, got {got}"
        );
    }

    fn assert_matches_geom(result: &Value, density: f64) {
        let (mass, com, inertia) = mass_props_fields(result);
        let (exp_mass, exp_com, exp_inertia) = uniform_box_inertia(DIMS, density);
        assert_close(mass, exp_mass, "mass");
        for i in 0..3 {
            assert_close(com[i], exp_com[i], "com");
        }
        for r in 0..3 {
            for c in 0..3 {
                assert_close(inertia[r][c], exp_inertia[r][c], "inertia");
            }
        }
    }

    // ── body_label regression guard: name field and type_name fallback ──────
    //
    // `body_label`'s `name`-field read is pinned by these two tests: they verify
    // the label is embedded in the E_DynamicsNoDensity error message
    // (ambient-default-material C, task 4498) so a mis-keyed borrow of "name" or
    // a typo in the type_name fallback is caught immediately.

    #[test]
    fn body_label_uses_name_field_in_no_density_error() {
        // Build a body carrying an explicit `name` field AND a material with no
        // density (forces the E_DynamicsNoDensity path, which embeds body_label
        // in the error message).
        let fields: PersistentMap<String, Value> = [
            ("name".to_string(), Value::String("WidgetA".to_string())),
            ("material".to_string(), material(None)),
        ]
        .into_iter()
        .collect();
        let b = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Block".to_string(),
            version: 1,
            fields,
        }));
        let mut diags = Vec::new();
        eval_body_mass_props_core(&b, None, |d| uniform_box_inertia(DIMS, d), &mut diags);

        assert_eq!(
            diags.len(),
            1,
            "no-density path must emit exactly one diagnostic"
        );
        assert_eq!(
            diags[0].severity,
            Severity::Error,
            "no-density diagnostic must be Severity::Error"
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsNoDensity),
            "no-density diagnostic must carry DynamicsNoDensity code"
        );
        assert!(
            diags[0].message.contains("WidgetA"),
            "error message must use the body's `name` field; got: {:?}",
            diags[0].message,
        );
    }

    #[test]
    fn body_label_falls_back_to_type_name_in_no_density_error() {
        // body(None) has no `name` field and type_name "Block"; no density forces
        // the E_DynamicsNoDensity path so body_label's type_name fallback is
        // exercised (ambient-default-material C, task 4498).
        let b = body(None);
        let mut diags = Vec::new();
        eval_body_mass_props_core(&b, None, |d| uniform_box_inertia(DIMS, d), &mut diags);

        assert_eq!(
            diags.len(),
            1,
            "no-density path must emit exactly one diagnostic"
        );
        assert_eq!(
            diags[0].severity,
            Severity::Error,
            "no-density diagnostic must be Severity::Error"
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsNoDensity),
            "no-density diagnostic must carry DynamicsNoDensity code"
        );
        assert!(
            diags[0].message.contains("Block"),
            "error message must fall back to the body's type_name 'Block'; got: {:?}",
            diags[0].message,
        );
    }

    // ── Case A: material density, no explicit arg, no warning ────────────────

    #[test]
    fn material_density_resolves_with_no_warning() {
        let b = body(Some(2700.0));
        let used = std::cell::Cell::new(f64::NAN);
        let geom = |density: f64| {
            used.set(density);
            uniform_box_inertia(DIMS, density)
        };
        let mut diags = Vec::new();
        let result = eval_body_mass_props_core(&b, None, geom, &mut diags);

        assert_eq!(
            used.get(),
            2700.0,
            "geom_query must be called with the material density"
        );
        assert_matches_geom(&result, 2700.0);
        assert!(
            diags.is_empty(),
            "Material-rung resolution must emit no diagnostics, got {diags:?}"
        );
    }

    // ── Case C: explicit density arg wins, no diagnostic ────────────────────

    #[test]
    fn explicit_density_arg_wins_with_no_warning() {
        let b = body(Some(2700.0)); // material present, but explicit arg overrides
        let used = std::cell::Cell::new(f64::NAN);
        let geom = |density: f64| {
            used.set(density);
            uniform_box_inertia(DIMS, density)
        };
        let mut diags = Vec::new();
        let explicit = Value::Scalar {
            si_value: 5000.0,
            dimension: DimensionVector::MASS_DENSITY,
        };
        let result = eval_body_mass_props_core(&b, Some(&explicit), geom, &mut diags);

        assert_eq!(
            used.get(),
            5000.0,
            "geom_query must be called with the explicit density"
        );
        assert_matches_geom(&result, 5000.0);
        assert!(
            diags.is_empty(),
            "explicit-density resolution must emit no diagnostics, got {diags:?}"
        );
    }

    // ── try_eval_body_mass_props dispatch (step-7) ───────────────────────────
    //
    // Dispatch recognition for a `body_mass_props(...)` call cell, mirroring
    // `geometry_ops::try_eval_*`. The density-aware KGQ kernel mass/com/inertia
    // query (KGQ Phase 4 / task 3620) is NOT wired by this batch, so a
    // recognised call yields a `MassProperties` whose geometric fields
    // (`mass`/`com`/`inertia`) are the deferred `Value::Undef` sentinel — while
    // the density ladder and the `E_DynamicsNoDensity` hard-error tail run on the
    // no-density path.

    /// Build a `<fn_name>(<args…>)` `FunctionCall` expr, each arg a `ValueRef`
    /// to the supplied cell. Mirrors the `geometry_ops` `conformance_call`
    /// content-hash construction so the synthetic expr is well-formed.
    fn call_expr(fn_name: &str, arg_cells: &[ValueCellId]) -> CompiledExpr {
        let args: Vec<CompiledExpr> = arg_cells
            .iter()
            .map(|c| CompiledExpr::value_ref(c.clone(), Type::dimensionless_scalar()))
            .collect();
        let mut content_hash =
            ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL]).combine(ContentHash::of_str(fn_name));
        for a in &args {
            content_hash = content_hash.combine(a.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: fn_name.to_string(),
                    qualified_name: fn_name.to_string(),
                },
                args,
            },
            result_type: Type::StructureRef("MassProperties".to_string()),
            content_hash,
        }
    }

    /// Assert `result` is a `MassProperties` `StructureInstance` whose three
    /// geometric fields are the deferred `Value::Undef` sentinel (the unwired
    /// kernel seam) — the dispatch still produces a well-typed instance.
    fn assert_deferred_mass_props(result: &Value) {
        let data = match result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a MassProperties StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "MassProperties",
            "dispatch must assemble a MassProperties instance"
        );
        for f in ["mass", "com", "inertia"] {
            assert_eq!(
                data.fields.get(f),
                Some(&Value::Undef),
                "geometric field `{f}` must be the deferred Undef sentinel (kernel seam unwired)"
            );
        }
    }

    // ── (a) recognised call + Material density -> Some, Undef geom, no warning ─

    #[test]
    fn dispatch_recognises_body_mass_props_with_material_density() {
        let body_cell = ValueCellId::new("Design", "blk");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(Some(2700.0)));
        let expr = call_expr("body_mass_props", &[body_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("recognised body_mass_props call must return Some(MassProperties)");
        assert_deferred_mass_props(&result);
        assert!(
            diags.is_empty(),
            "Material-rung resolution must emit no diagnostics, got {diags:?}"
        );
    }

    // ── no-density hard error (E_DynamicsNoDensity) ─────────────────────────
    //
    // These tests pin the contract: a `body_mass_props` call with no resolvable
    // density (no explicit arg, no Material density) must emit a hard
    // `Severity::Error` with `DiagnosticCode::DynamicsNoDensity` naming all
    // three fixes, skip the geometry query, and return a `MassProperties` with
    // `Value::Undef` geometric fields (ambient-default-material C, task 4498).

    /// Core-level: `body(None)` + no explicit arg → exactly one Error with
    /// `DynamicsNoDensity`, message names all three fixes, geom closure is
    /// never invoked, geometric fields are all `Value::Undef`.
    #[test]
    fn eval_body_mass_props_core_no_density_emits_error_and_degrades_to_undef() {
        let b = body(None); // material present but carries no density field
        let geom_called = std::cell::Cell::new(false);
        let geom = |d: f64| {
            geom_called.set(true);
            uniform_box_inertia(DIMS, d)
        };
        let mut diags = Vec::new();
        let result = eval_body_mass_props_core(&b, None, geom, &mut diags);

        // Geometry query must NOT be called (no density → skip query).
        assert!(
            !geom_called.get(),
            "geom_query must not be called when no density is resolvable"
        );

        // The returned value must still be a MassProperties with Undef geom fields.
        assert_deferred_mass_props(&result);

        // Exactly one diagnostic, Severity::Error, code DynamicsNoDensity.
        assert_eq!(
            diags.len(),
            1,
            "no-density path must emit exactly one diagnostic, got: {diags:?}"
        );
        assert_eq!(
            diags[0].severity,
            Severity::Error,
            "no-density diagnostic must be Severity::Error, got: {:?}",
            diags[0].severity
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsNoDensity),
            "no-density diagnostic must carry DynamicsNoDensity code, got: {:?}",
            diags[0].code
        );
        // Message must name all three fixes.
        let msg = &diags[0].message;
        assert!(
            msg.contains("explicit density argument"),
            "message must mention 'explicit density argument' (explicit density hint); got: {msg:?}"
        );
        assert!(
            msg.contains("Material"),
            "message must mention 'Material' (body Material hint); got: {msg:?}"
        );
        assert!(
            msg.contains("default Material"),
            "message must mention 'default Material' (ambient default hint); got: {msg:?}"
        );
    }

    /// Core-level: body with NO material at all (not just no density) — same
    /// error contract as `body(None)`, verifying the materialless path.
    #[test]
    fn eval_body_mass_props_core_no_material_emits_error_and_degrades_to_undef() {
        // Build a body with no `material` field at all.
        let no_material_body = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Block".to_string(),
            version: 1,
            fields: PersistentMap::default(),
        }));
        let geom_called = std::cell::Cell::new(false);
        let geom = |d: f64| {
            geom_called.set(true);
            uniform_box_inertia(DIMS, d)
        };
        let mut diags = Vec::new();
        let result = eval_body_mass_props_core(&no_material_body, None, geom, &mut diags);

        assert!(
            !geom_called.get(),
            "geom_query must not be called when body has no material"
        );
        assert_deferred_mass_props(&result);
        assert_eq!(
            diags.len(),
            1,
            "no-material body must emit exactly one diagnostic"
        );
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, Some(DiagnosticCode::DynamicsNoDensity));
    }

    /// Dispatch-level: `try_eval_body_mass_props` on a 1-arg `body_mass_props(body)`
    /// with a no-density body → `Some(MassProperties)` with Undef geom fields and
    /// exactly one `DynamicsNoDensity` Error in diags.
    #[test]
    fn dispatch_no_density_body_emits_error_and_degrades_to_undef() {
        let body_cell = ValueCellId::new("Design", "blk");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(None)); // material present, no density
        let expr = call_expr("body_mass_props", &[body_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("no-density call must still return Some(MassProperties)");
        assert_deferred_mass_props(&result);
        assert_eq!(
            diags.len(),
            1,
            "no-density dispatch must emit exactly one diagnostic, got: {diags:?}"
        );
        assert_eq!(
            diags[0].severity,
            Severity::Error,
            "no-density dispatch must emit Severity::Error"
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsNoDensity),
            "no-density dispatch must carry DynamicsNoDensity code"
        );
    }

    // ── explicit density arg (2-arg form) wins, no diagnostic ───────────────

    #[test]
    fn dispatch_explicit_density_arg_suppresses_warning() {
        let body_cell = ValueCellId::new("Design", "blk");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(None)); // no Material density…
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 5000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        ); // …but explicit arg present
        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("recognised 2-arg call must return Some(MassProperties)");
        assert_deferred_mass_props(&result);
        assert!(
            diags.is_empty(),
            "explicit-density resolution must emit no diagnostics, got {diags:?}"
        );
    }

    // ── (b) unrelated fn name -> None (engine leaves the cell untouched) ───────

    #[test]
    fn dispatch_returns_none_for_unrelated_fn() {
        let body_cell = ValueCellId::new("Design", "blk");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(Some(2700.0)));
        let expr = call_expr("volume", &[body_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "unrelated fn `volume` must return None, got {result:?}"
        );
        assert!(
            diags.is_empty(),
            "None-dispatch must not emit diagnostics, got {diags:?}"
        );
    }

    // ── (b) non-call expr -> None ─────────────────────────────────────────────

    #[test]
    fn dispatch_returns_none_for_non_call_expr() {
        let body_cell = ValueCellId::new("Design", "blk");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(Some(2700.0)));
        // A bare `ValueRef`, not a `FunctionCall`.
        let expr = CompiledExpr::value_ref(body_cell, Type::StructureRef("Block".to_string()));
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "non-call expr must return None, got {result:?}"
        );
    }

    // ── arity guard: malformed-arity body_mass_props -> error, None ───────────
    //
    // The compiler signature `body_mass_props(body, density?)` is the primary
    // arity gate, but the `expr.rs` name-recognition path assigns the result
    // type without an arity check, so a 0-arg or 3+-arg call can reach this
    // dispatch. It must surface an `E_DynamicsBodyMassPropsArity` error rather
    // than silently returning `None` (which would leave a MassProperties-typed
    // cell at `Undef` with no diagnostic).

    #[test]
    fn dispatch_zero_args_emits_arity_error_and_returns_none() {
        let expr = call_expr("body_mass_props", &[]);
        let values = ValueMap::new();
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "zero-arg body_mass_props() must return None (cell left at Undef), got {result:?}"
        );
        assert_eq!(
            diags.len(),
            1,
            "malformed arity must emit exactly one diagnostic, got {diags:?}"
        );
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsBodyMassPropsArity),
            "arity diagnostic must carry the DynamicsBodyMassPropsArity code"
        );
    }

    #[test]
    fn dispatch_too_many_args_emits_arity_error_and_returns_none() {
        let a = ValueCellId::new("Design", "a");
        let b = ValueCellId::new("Design", "b");
        let c = ValueCellId::new("Design", "c");
        // The guard fires before arg resolution, so the cells need not be in
        // `values`.
        let expr = call_expr("body_mass_props", &[a, b, c]);
        let values = ValueMap::new();
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "3-arg body_mass_props(...) must return None, got {result:?}"
        );
        assert_eq!(
            diags.len(),
            1,
            "malformed arity must emit one diagnostic, got {diags:?}"
        );
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsBodyMassPropsArity)
        );
    }

    // ── step-3: kernel failure downgrades to Undef + Warning ─────────────────

    #[test]
    fn try_eval_body_mass_props_kernel_failure_downgrades_to_undef_with_warning() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        // Body holds a GeometryHandle for handle 9; bare kernel has no injected
        // results so the first query (Volume) returns Err immediately.
        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(9)),
            },
        );
        // Explicit density means no E_DynamicsNoDensity error, so the only
        // warning we see is the kernel-failure downgrade warning.
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );

        // No injected results → every query returns Err.
        let kernel = MockGeometryKernel::new();

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("kernel failure must still return Some(MassProperties)");

        // All three geometric fields must be the deferred Undef sentinel.
        assert_deferred_mass_props(&result);

        // Must emit at least one Warning (the defensive kernel-failure downgrade).
        // RED after step-2 because step-2 silently discards the error without
        // emitting a diagnostic; step-4 adds the warning.
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "kernel failure must emit at least one Warning diagnostic, got: {diags:?}"
        );
    }

    // ── step-1: GeometryHandle body routes kernel queries into geometric fields ─

    #[test]
    fn try_eval_body_mass_props_routes_kernel_query_into_geometric_fields() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        // Body cell holds a GeometryHandle with kernel_handle 7.
        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(7)),
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );

        // Injected inertia: distinct diagonal so all three diagonal entries differ.
        let injected_inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        let kernel = MockGeometryKernel::new()
            .with_volume_result(GeometryHandleId(7), Value::Real(3.0))
            .with_center_of_mass_result(
                GeometryHandleId(7),
                2000.0,
                Value::String("{\"x\":0.01,\"y\":0.02,\"z\":0.03}".to_string()),
            )
            .with_inertia_tensor_result(GeometryHandleId(7), 2000.0, injected_inertia);

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("recognised body_mass_props with GeometryHandle must return Some");

        // mass = density × volume = 2000.0 × 3.0 = 6000.0 kg
        let (mass, com, inertia) = mass_props_fields(&result);
        assert_close(mass, 6000.0, "mass");
        assert_close(com[0], 0.01, "com[0]");
        assert_close(com[1], 0.02, "com[1]");
        assert_close(com[2], 0.03, "com[2]");
        assert_close(inertia[0][0], 1.0, "inertia[0][0]");
        assert_close(inertia[1][1], 2.0, "inertia[1][1]");
        assert_close(inertia[2][2], 3.0, "inertia[2][2]");
        assert_close(inertia[0][1], 0.0, "inertia[0][1]");
        assert_close(inertia[1][0], 0.0, "inertia[1][0]");
        // Explicit density must emit no E_DynamicsNoDensity error.
        assert!(
            diags
                .iter()
                .all(|d| d.code != Some(DiagnosticCode::DynamicsNoDensity)),
            "explicit density must not emit E_DynamicsNoDensity, got: {diags:?}"
        );
    }

    // ── malformed-reply defensive paths in query_body_mass_props_from_kernel ──
    //
    // The three ok_or_else / parse_xyz_value / inertia_3x3_from_value branches
    // in query_body_mass_props_from_kernel are exercised below. Each should
    // propagate the error through the closure capture, triggering the Undef
    // downgrade and the kernel-failure Warning — the same contract as a missing
    // kernel result.

    /// Partial kernel failure: Volume query succeeds but CenterOfMass is not
    /// injected (returns Err). The error must propagate out of the closure,
    /// downgrading all geometric fields to Undef and emitting a Warning.
    #[test]
    fn try_eval_body_mass_props_partial_failure_after_volume_downgrades_to_undef() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(11)),
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );

        // Volume injected and succeeds; CenterOfMass is NOT injected → Err on
        // the second query, exercising the partial-failure path.
        let kernel =
            MockGeometryKernel::new().with_volume_result(GeometryHandleId(11), Value::Real(5.0));

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("partial kernel failure must still return Some(MassProperties)");

        assert_deferred_mass_props(&result);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "partial kernel failure must emit at least one Warning, got: {diags:?}"
        );
    }

    /// Malformed Volume reply: the mock returns `Value::String("not-a-number")`
    /// for the Volume query. `cell_f64` returns `None`, which `ok_or_else`
    /// converts to a `QueryError`. All geometric fields must downgrade to Undef
    /// and exactly one Warning must be emitted.
    #[test]
    fn try_eval_body_mass_props_malformed_volume_reply_downgrades_to_undef() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(13)),
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );

        // Volume reply is non-numeric: cell_f64 returns None → ok_or_else →
        // QueryError → defensive Undef + Warning.
        let kernel = MockGeometryKernel::new().with_volume_result(
            GeometryHandleId(13),
            Value::String("not-a-number".to_string()),
        );

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("malformed Volume reply must still return Some(MassProperties)");

        assert_deferred_mass_props(&result);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "malformed Volume reply must emit at least one Warning, got: {diags:?}"
        );
    }

    // ── step-3 (task 4494): inertia cells are MomentOfInertia-dimensioned scalars ─
    //
    // Kernel-independent pin: eval_body_mass_props_core must populate the inertia
    // field of the assembled MassProperties with Value::Scalar{MOMENT_OF_INERTIA}
    // cells, not plain Value::Real. Fails RED until step-4 (task 4494) changes
    // inertia_value to emit dimensioned scalars.

    /// The populated inertia matrix must be a Value::Matrix of
    /// Value::Scalar{dimension == MOMENT_OF_INERTIA} cells with si_value equal
    /// (within 1e-12) to the corresponding uniform_box_inertia entry.
    /// This runs on every CI runner regardless of OCCT availability.
    #[test]
    fn eval_body_mass_props_core_inertia_cells_are_moment_of_inertia_scalars() {
        let b = body(Some(2700.0));
        let mut diags = Vec::new();
        let result =
            eval_body_mass_props_core(&b, None, |d| uniform_box_inertia(DIMS, d), &mut diags);

        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected MassProperties StructureInstance, got {other:?}"),
        };
        let inertia_rows = match data.fields.get("inertia").expect("inertia field") {
            Value::Matrix(rows) => rows,
            other => panic!("inertia field must be Value::Matrix, got {other:?}"),
        };
        assert_eq!(inertia_rows.len(), 3, "inertia must have 3 rows");

        let (_, _, expected_inertia) = uniform_box_inertia(DIMS, 2700.0);
        for r in 0..3 {
            assert_eq!(
                inertia_rows[r].len(),
                3,
                "each inertia row must have 3 cols"
            );
            for c in 0..3 {
                match &inertia_rows[r][c] {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::MOMENT_OF_INERTIA,
                            "inertia[{r}][{c}] must be MOMENT_OF_INERTIA-dimensioned, got {dimension:?}"
                        );
                        assert!(
                            (si_value - expected_inertia[r][c]).abs() < 1e-12,
                            "inertia[{r}][{c}] si_value: expected {}, got {}",
                            expected_inertia[r][c],
                            si_value
                        );
                    }
                    other => panic!(
                        "inertia[{r}][{c}] must be Value::Scalar{{MOMENT_OF_INERTIA}}, got {other:?}"
                    ),
                }
            }
        }
    }

    /// Malformed CenterOfMass JSON: Volume succeeds but the CoM reply is not a
    /// valid `{"x":_,"y":_,"z":_}` JSON string. `parse_xyz_value` fails,
    /// propagating a `QueryError` that triggers the Undef downgrade + Warning.
    #[test]
    fn try_eval_body_mass_props_malformed_com_json_downgrades_to_undef() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(15)),
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );

        // Volume is valid; CenterOfMass reply is malformed JSON → parse_xyz_value
        // fails → QueryError → defensive Undef + Warning.
        let kernel = MockGeometryKernel::new()
            .with_volume_result(GeometryHandleId(15), Value::Real(5.0))
            .with_center_of_mass_result(
                GeometryHandleId(15),
                2000.0,
                Value::String("not-valid-json".to_string()),
            );

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("malformed CoM JSON must still return Some(MassProperties)");

        assert_deferred_mass_props(&result);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "malformed CenterOfMass JSON must emit at least one Warning, got: {diags:?}"
        );
    }

    // ── step-1 RED: Pressure-as-density must be rejected (task 4491) ─────────
    //
    // HOLE 1: Today cell_f64 strips 2.0e11 from a Pressure scalar and feeds it
    // to geom_query, silently using Pressure-magnitude as if it were kg/m³.
    // After step-2, accept_arg rejects it, geom_query is never called, all three
    // fields degrade to Value::Undef, and a "expects Density, got Pressure"
    // Warning is emitted.

    /// 1a — Core layer: eval_body_mass_props_core with an explicit Pressure arg.
    ///
    /// RED today: cell_f64 extracts si_value 2.0e11 and calls geom_query,
    /// producing non-Undef mass/com/inertia and no "expects Density" diagnostic.
    /// GREEN after step-2: accept_arg rejects the Pressure scalar, geom_query is
    /// never invoked, all three fields are Undef, one Warning carries both
    /// "expects Density" and "Pressure".
    #[test]
    fn eval_body_mass_props_core_rejects_pressure_scalar_as_density() {
        let b = body(Some(2700.0));
        let geom_called = std::cell::Cell::new(false);
        let geom = |d: f64| {
            geom_called.set(true);
            uniform_box_inertia(DIMS, d)
        };
        let mut diags = Vec::new();
        let pressure_arg = Value::Scalar {
            si_value: 2.0e11,
            dimension: DimensionVector::PRESSURE,
        };
        let result = eval_body_mass_props_core(&b, Some(&pressure_arg), geom, &mut diags);

        // geom_query must NOT be called (Pressure arg aborts before kernel).
        assert!(
            !geom_called.get(),
            "geom_query must not be called when explicit density arg is a Pressure scalar"
        );

        // All three geometric fields must be Undef.
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected MassProperties StructureInstance, got {other:?}"),
        };
        for f in ["mass", "com", "inertia"] {
            assert_eq!(
                data.fields.get(f),
                Some(&Value::Undef),
                "geometric field `{f}` must be Undef when the density arg is rejected"
            );
        }

        // At least one Warning with "expects Density" AND "Pressure".
        let rejection_diag = diags
            .iter()
            .find(|d| d.message.contains("expects Density") && d.message.contains("Pressure"));
        assert!(
            rejection_diag.is_some(),
            "Pressure-as-density must emit a Warning containing 'expects Density' and \
             'Pressure'; got: {diags:?}"
        );
    }

    // ── step-3 RED: unsupported density arg shape must not be silently ignored ──
    //
    // HOLE 2: Today resolve_arg_value returns None for a FunctionCall arg shape,
    // causing density_arg to become None and the ladder to silently fall through
    // to the Material rung (no diagnostic). After step-4, an unsupported shape
    // emits a Warning (NOT E_DynamicsNoDensity) and the result is Undef.

    /// Build a body_mass_props FunctionCall whose args[1] is itself a FunctionCall
    /// (an unsupported arg shape that resolve_arg_value cannot resolve).
    fn call_expr_with_fn_arg(
        fn_name: &str,
        body_arg: ValueCellId,
        inner_fn_name: &str,
        inner_arg: ValueCellId,
    ) -> CompiledExpr {
        let body_ref = CompiledExpr::value_ref(body_arg, Type::dimensionless_scalar());

        // Build the inner FunctionCall that will be used as args[1].
        let inner_arg_ref =
            CompiledExpr::value_ref(inner_arg.clone(), Type::dimensionless_scalar());
        let inner_ch = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(ContentHash::of_str(inner_fn_name))
            .combine(inner_arg_ref.content_hash);
        let inner_call = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: inner_fn_name.to_string(),
                    qualified_name: inner_fn_name.to_string(),
                },
                args: vec![inner_arg_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: inner_ch,
        };

        // Build the outer body_mass_props(body_ref, inner_call) FunctionCall.
        let outer_ch = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(ContentHash::of_str(fn_name))
            .combine(body_ref.content_hash)
            .combine(inner_call.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: fn_name.to_string(),
                    qualified_name: fn_name.to_string(),
                },
                args: vec![body_ref, inner_call],
            },
            result_type: Type::StructureRef("MassProperties".to_string()),
            content_hash: outer_ch,
        }
    }

    /// step-3 RED — unsupported density arg shape (FunctionCall as args[1]).
    ///
    /// RED today: resolve_arg_value(FunctionCall) → None → density_arg = None →
    /// Material-rung ladder runs silently (density=2700, no diagnostic). GREEN
    /// after step-4: an unsupported shape emits a Warning (NOT E_DynamicsNoDensity)
    /// and the result's geometric fields are Undef.
    #[test]
    fn dispatch_unsupported_density_arg_shape_degrades_to_undef_with_warning() {
        let body_cell = ValueCellId::new("Design", "body");
        let other_cell = ValueCellId::new("Design", "x");
        let mut values = ValueMap::new();
        // Body has a material density so if the unsupported arg were silently
        // ignored the ladder would use 2700.0 (no E_DynamicsNoDensity error).
        values.insert(body_cell.clone(), body(Some(2700.0)));
        // other_cell exists but is not used as a density arg directly.
        values.insert(other_cell.clone(), Value::Real(42.0));

        // The outer body_mass_props call's args[1] is a FunctionCall ("some_fn")
        // — an unsupported shape that resolve_arg_value cannot resolve.
        let expr = call_expr_with_fn_arg("body_mass_props", body_cell, "some_fn", other_cell);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags).expect(
            "unsupported-shape arg must still return Some(MassProperties) degraded to Undef",
        );

        // All three geometric fields must be Undef.
        assert_deferred_mass_props(&result);

        // At least one Warning must be emitted, and it must NOT carry
        // E_DynamicsNoDensity (that code is reserved for the no-arg path with no material).
        let non_no_density_warnings: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Severity::Warning && d.code != Some(DiagnosticCode::DynamicsNoDensity)
            })
            .collect();
        assert!(
            !non_no_density_warnings.is_empty(),
            "unsupported density arg shape must emit a Warning that is NOT \
             E_DynamicsNoDensity; got: {diags:?}"
        );
    }

    // ── Amendment: silent-degrade paths must emit zero diagnostics ───────────
    //
    // Both the Acceptance::Undefined path in eval_body_mass_props_core and the
    // missing-cell ValueRef path in try_eval_body_mass_props are specified as
    // "quiet degrades" (data-indeterminacy — no Warning emitted). These tests
    // pin that contract so an accidental diagnostic push on either path is
    // caught immediately.

    /// Feeding `Value::Undef` as the explicit density arg degrades all geometric
    /// fields to Undef silently — geom_query is never called, zero diagnostics.
    ///
    /// Exercises the `Acceptance::Undefined → None` (quiet-degrade) branch in
    /// `resolve_body_density`.
    #[test]
    fn eval_body_mass_props_core_undef_density_arg_degrades_silently() {
        let b = body(Some(2700.0));
        let geom_called = std::cell::Cell::new(false);
        let geom = |d: f64| {
            geom_called.set(true);
            uniform_box_inertia(DIMS, d)
        };
        let mut diags = Vec::new();
        let result = eval_body_mass_props_core(&b, Some(&Value::Undef), geom, &mut diags);

        // geom_query must NOT be called (Undef density skips the geometry query).
        assert!(
            !geom_called.get(),
            "geom_query must not be called when explicit density arg is Value::Undef"
        );

        // All three geometric fields must be the deferred Undef sentinel.
        assert_deferred_mass_props(&result);

        // No diagnostic must be emitted (data-indeterminacy → quiet degrade).
        assert!(
            diags.is_empty(),
            "Undef explicit density must emit zero diagnostics (quiet degrade); got: {diags:?}"
        );
    }

    /// A `ValueRef` in `args[1]` pointing to a missing cell degrades all
    /// geometric fields to Undef silently — zero diagnostics (no Warning, no
    /// `E_DynamicsNoDensity`).
    ///
    /// Exercises the missing-cell quiet-degrade branch in the inline density-arg
    /// `match` inside `try_eval_body_mass_props` (hole-2 resolution, step-4).
    #[test]
    fn dispatch_missing_density_cell_degrades_to_undef_silently() {
        let body_cell = ValueCellId::new("Design", "blk");
        let rho_cell = ValueCellId::new("Design", "rho"); // deliberately NOT inserted
        let mut values = ValueMap::new();
        // Body has a material density so if the missing-cell were silently treated
        // as "no explicit arg" the ladder would use 2700.0 (non-Undef fields) — a
        // regression that these asserts would catch.
        values.insert(body_cell.clone(), body(Some(2700.0)));
        // rho_cell is intentionally absent from `values`.

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags).expect(
            "missing density cell must still return Some(MassProperties) degraded to Undef",
        );

        // All three geometric fields must be the deferred Undef sentinel.
        assert_deferred_mass_props(&result);

        // No diagnostic must be emitted (data-indeterminacy → quiet degrade).
        assert!(
            diags.is_empty(),
            "missing-cell density arg must emit zero diagnostics (quiet degrade); got: {diags:?}"
        );
    }

    /// 1b — Dispatch end-to-end: try_eval_body_mass_props with a Pressure rho cell.
    ///
    /// RED today: resolve_arg_value returns the Pressure scalar; cell_f64 strips
    /// it to 2.0e11 and the kernel Volume query produces non-Undef mass; no
    /// "expects Density" diagnostic is emitted. GREEN after step-2: the dispatch
    /// routes the Pressure scalar through accept_arg (Rejected), emits a Warning,
    /// and returns Some(MassProperties{Undef,Undef,Undef}).
    #[test]
    fn dispatch_pressure_scalar_as_density_is_rejected_and_degrades_to_undef() {
        use reify_core::RealizationNodeId;
        use reify_ir::GeometryHandleId;

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("Design", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(GeometryHandleId(99)),
            },
        );
        // Wrong dimension: Pressure, not Density.
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2.0e11,
                dimension: DimensionVector::PRESSURE,
            },
        );

        // Kernel has a volume result so that if the Pressure scalar were wrongly
        // accepted the mass field would be 2.0e11 × 3.0 = 6.0e11 — not Undef.
        let kernel =
            MockGeometryKernel::new().with_volume_result(GeometryHandleId(99), Value::Real(3.0));

        let expr = call_expr("body_mass_props", &[body_cell, rho_cell]);
        let mut diags = Vec::new();
        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("Pressure-as-density must still return Some(MassProperties) degraded to Undef");

        // All three geometric fields must be Undef (rejected density → degrade).
        assert_deferred_mass_props(&result);

        // Must emit at least one Warning containing "expects Density".
        let rejection_diag = diags.iter().find(|d| d.message.contains("expects Density"));
        assert!(
            rejection_diag.is_some(),
            "Pressure-as-density must emit a Warning containing 'expects Density'; got: {diags:?}"
        );
    }
}

// ── inverse_dynamics ComputeNode trampoline (task RBD-ι) ─────────────────────

/// Warm-state payload donated by the `inverse_dynamics` trampoline (task RBD-ι):
/// the cache key the result was computed for, the per-body solid-content-hash
/// invalidation record, and the cached `List<List<JointForce>>` result itself.
///
/// Recovered on the next invocation via [`OpaqueState::downcast_ref`] and reused
/// only when the incoming request's [`InverseDynamicsCacheKey`] matches (a cache
/// HIT, step-10). Mirrors `modal_ops::ModalAnalysisCache` (the modal-κ split),
/// except the cached payload is the finished trajectory result: the dynamics
/// solve has no separable "assembly" half, and the per-sample
/// MassProperties-reuse optimisation is deferred (design_decision #4).
#[derive(Clone)]
pub(crate) struct InverseDynamicsCache {
    /// The (mechanism, trajectory, gravity)-hash key the cached `result` certifies;
    /// read by the cache-HIT lookup (`key.matches`) on the next invocation.
    key: InverseDynamicsCacheKey,
    /// Per-body `solid` content hashes (in `bodies` order), recorded so the warm
    /// state observes "the MassProperties only changed when a body solid changed"
    /// at body granularity (PRD §7.7). The HIT decision itself is the full-key
    /// [`InverseDynamicsCacheKey::matches`]; this record is its body-granular
    /// companion (and the input a future per-body MassProperties-reuse
    /// optimisation would key on).
    body_solid_hashes: Vec<ContentHash>,
    /// The cached trajectory-level result (`List<List<JointForce>>`), held behind
    /// an [`Arc`] so the mandatory cache-HIT re-donation (see
    /// [`run_inverse_dynamics`]) is an O(1) refcount bump rather than a second
    /// deep clone of the whole `List<List<JointForce>>` tree. Only the
    /// output-value-cell copy pays an unavoidable deep clone (the engine cell
    /// owns a plain `Value`); the re-donated warm-state copy shares this `Arc`.
    result: Arc<Value>,
}

impl InverseDynamicsCache {
    /// Coarse estimate of the retained size of this cache in bytes: the flat key,
    /// the per-body solid-hash record, and the cached result `Value` tree. Drives
    /// both the [`OpaqueState`] LRU size hint and the donated `cost_per_byte`
    /// (mirrors `ModalAnalysisCache::estimated_size_bytes`). Always
    /// ≥ `size_of::<InverseDynamicsCacheKey>() > 0`, so the `cost_per_byte`
    /// reciprocal is well-defined.
    fn estimated_size_bytes(&self) -> usize {
        std::mem::size_of::<InverseDynamicsCacheKey>()
            + self.body_solid_hashes.len() * std::mem::size_of::<ContentHash>()
            + value_size_estimate(self.result.as_ref())
    }

    /// Wrap this cache in an [`OpaqueState`] for donation to the warm-state pool,
    /// sized by [`estimated_size_bytes`](Self::estimated_size_bytes). Returns that
    /// `size_bytes` alongside the state so the caller derives `cost_per_byte` from
    /// the same single measurement. Mirrors `ModalAnalysisCache::into_opaque_state`.
    fn into_opaque_state(self) -> (OpaqueState, usize) {
        let size = self.estimated_size_bytes();
        (OpaqueState::new(self, size), size)
    }
}

/// Result of the in-crate core [`run_inverse_dynamics`]: the engine-facing
/// [`ComputeOutcome`] plus a white-box `reused` flag the in-crate cache-HIT tests
/// assert against (the public `ComputeFn` returns only the outcome). Mirrors
/// `modal_ops::ModalTrampolineRun`.
pub(crate) struct InverseDynamicsRun {
    /// The compute outcome the public trampoline returns.
    pub(crate) outcome: ComputeOutcome,
    /// `true` iff this run reused a cached [`InverseDynamicsCache`] result rather
    /// than recomputing the per-sample RNEA loop. Observable only in-crate (the
    /// cache-HIT tests); the public `ComputeFn` discards it, hence `allow(dead_code)`.
    #[allow(dead_code)]
    pub(crate) reused: bool,
}

/// Coarse heap-size estimate of a `Value` tree: a per-node `size_of::<Value>()`
/// plus the out-of-line payload of strings, lists, and structure-instance fields.
/// Feeds [`InverseDynamicsCache::estimated_size_bytes`]; only a monotone proxy for
/// "how expensive is this result to retain" is needed, not byte-exactness, so the
/// catch-all covers the scalar leaves (`Real`/`Int`/`Scalar`/…) that carry no
/// out-of-line heap payload. `List` + `StructureInstance` fully cover the
/// `List<List<JointForce>>` result this cache holds.
fn value_size_estimate(v: &Value) -> usize {
    let base = std::mem::size_of::<Value>();
    match v {
        Value::String(s) => base + s.len(),
        Value::List(items) => base + items.iter().map(value_size_estimate).sum::<usize>(),
        Value::StructureInstance(d) => {
            base + d.type_name.len()
                + d.fields
                    .iter()
                    .map(|(k, val)| k.len() + value_size_estimate(val))
                    .sum::<usize>()
        }
        _ => base,
    }
}

/// The malformed-input short-circuit outcome: η's `Value::Undef` convention
/// surfaced as a `Completed` with no donated warm state — keeping the
/// trampoline result bit-identical to the unregistered `inverse_dynamics_lower`
/// body-inline fallback, and donating no warm state for an input that produced no
/// real computation (design_decision #6). (Closed-chain mechanisms no longer
/// short-circuit here: since task 4146 the shared per-sample seam routes them
/// through the closed-chain KKT bridge; `None` from the seam now means a
/// malformed input or a failed loop solve / singular KKT.)
fn undef_outcome() -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
    }
}

/// Build the `Completed` outcome that donates `cache` as the node's warm state:
/// the cache's `result` is returned to the output value cell and the cache itself
/// is donated to the warm-state pool, sized via
/// [`InverseDynamicsCache::into_opaque_state`] with `cost_per_byte` the reciprocal
/// of that size (a bigger cached result is pricier to retain). Shared by the
/// cache-HIT and cache-MISS paths of [`run_inverse_dynamics`] so both donate
/// identically (mirrors the modal trampoline's single donation tail).
///
/// Performs exactly ONE deep clone of the `List<List<JointForce>>` tree — the
/// copy handed to the output value cell (the engine cell owns a plain `Value`).
/// The warm-state copy re-uses the same `Arc<Value>` allocation (moved in via
/// [`into_opaque_state`](InverseDynamicsCache::into_opaque_state)), so the
/// mandatory re-donation costs an `Arc` refcount bump, not a second O(n) clone.
fn completed_donating(cache: InverseDynamicsCache) -> ComputeOutcome {
    let result = cache.result.as_ref().clone();
    let (state, size_bytes) = cache.into_opaque_state();
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    ComputeOutcome::Completed {
        result,
        new_warm_state: Some(state),
        cost_per_byte,
        diagnostics: Vec::new(),
    }
}

/// In-crate core behind [`solve_inverse_dynamics_trampoline`]: run RNEA inverse
/// dynamics over a whole `MotionTrajectory`, with the task-ι warm-state cache.
/// Returns an [`InverseDynamicsRun`] so in-crate tests can observe whether the
/// cached result was reused; the public trampoline takes only `.outcome`.
///
/// `@optimized("dynamics::inverse_dynamics")` core for `fn inverse_dynamics`.
/// Receives the two flat `value_inputs` matching the fn signature:
///
/// ```text
/// [0] mechanism  : Mechanism        (Value::Map — kind, bodies[].solid, joints)
/// [1] trajectory : MotionTrajectory (StructureInstance — samples[] of q/q̇/q̈)
/// ```
///
/// Drives the per-sample loop through the reify-stdlib seam
/// ([`motion_trajectory_samples`] + [`inverse_dynamics_sample`]) so the result is
/// bit-identical to the body-inline `inverse_dynamics_lower` fallback — including
/// closed-chain mechanisms, which the seam routes through the closed-chain KKT
/// bridge (task 4146). A malformed trajectory, or a sample the seam rejects (an
/// arity/shape mismatch, a failed loop solve / singular KKT), yields
/// [`undef_outcome`] — η's exact Undef convention — donating no warm state.
/// Gravity is the constant [`default_gravity`] (PRD §12 q1), folded into the
/// cache key.
///
/// Honours cooperative cancellation (PRD §6/§9.1): polls `cancellation` on entry
/// and at every per-sample boundary, returning [`ComputeOutcome::Cancelled`] (no
/// result, no warm state) within one sample interval of a fired cancel.
pub(crate) fn run_inverse_dynamics(
    value_inputs: &[Value],
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> InverseDynamicsRun {
    // ── (0) entry cancellation checkpoint ────────────────────────────────────
    // Coarse cooperative cancellation (CN-contract §2 / PRD §6/§9.1): poll on
    // entry, then again at every per-sample boundary (the MISS loop below). An
    // entry poll alone cannot meet §9.1's "abort within one sample interval" — the
    // per-sample poll is what bounds the abort latency. A fired cancel returns
    // ComputeOutcome::Cancelled (reused = false), donating no warm state and
    // discarding any partial per-sample output, mirroring run_modal_analysis.
    if cancellation.is_cancelled() {
        return InverseDynamicsRun {
            outcome: ComputeOutcome::Cancelled,
            reused: false,
        };
    }

    // Arity guard, mirroring eval_inverse_dynamics: inverse_dynamics(mechanism,
    // trajectory). The engine always supplies the two fn args, so this is defensive.
    if value_inputs.len() != 2 {
        return InverseDynamicsRun {
            outcome: undef_outcome(),
            reused: false,
        };
    }
    let mechanism = &value_inputs[0];
    let trajectory = &value_inputs[1];

    // Cache key over the result-determining inputs (constant default gravity).
    let key = InverseDynamicsCacheKey::from_inputs(mechanism, trajectory, default_gravity());

    // ── cache HIT ──────────────────────────────────────────────────────────────
    // A prior warm state whose key matches certifies its cached result for reuse:
    // return it with reused=true WITHOUT re-running the per-sample RNEA loop. The
    // cache must be re-donated so the node keeps its warm state and the next
    // identical call can HIT again: the engine takes the prior warm state out of
    // the pool at dispatch start (`get_warm_state`, take-semantics) and only
    // restores it on Cancelled/Failed — a Completed outcome with new_warm_state=None
    // would leave the node with no warm state at all. Re-donation is cheap here:
    // `prior_warm_state` is borrowed, but `cache.clone()` only bumps the result's
    // `Arc` refcount (+ copies the small body-solid-hash Vec), and the lone deep
    // clone of the `List<List<JointForce>>` tree is the one `completed_donating`
    // makes for the output value cell.
    if let Some(cache) = prior_warm_state.and_then(|s| s.downcast_ref::<InverseDynamicsCache>())
        && cache.key.matches(&key)
    {
        return InverseDynamicsRun {
            outcome: completed_donating(cache.clone()),
            reused: true,
        };
    }

    // ── cache MISS ───────────────────────────────────────────────────────────────
    // Record the body-granular solid-hash invalidation record, then drive the
    // per-sample RNEA loop through the shared stdlib seam so the result is
    // single-sourced with the body-inline fallback (closed-chain mechanisms route
    // through the seam's KKT bridge, task 4146). Any `None` (malformed trajectory,
    // shape mismatch, or failed loop solve) collapses to η's Undef.
    //
    // NOTE (speculative-generality, kept per design_decision #4): `body_solid_hashes`
    // is RECORDED but not yet CONSUMED by any HIT/MISS decision — that verdict is
    // `InverseDynamicsCacheKey::matches`, and the key's `mech_hash`
    // (`mechanism.content_hash()`) already folds in every body's `solid`, so a
    // changed body solid already forces a MISS today. The per-body record exists
    // for the deferred per-body MassProperties-reuse optimisation (design_decision
    // #4; η's hoist note, dynamics/eval.rs), which will key on it. Until that lands
    // its only runtime effect is its byte contribution to `estimated_size_bytes`;
    // the O(bodies) walk is negligible beside the per-sample RNEA loop it precedes.
    let body_solid_hashes = body_solid_hashes(mechanism);
    let samples = match motion_trajectory_samples(trajectory) {
        Some(s) => s,
        None => {
            return InverseDynamicsRun {
                outcome: undef_outcome(),
                reused: false,
            };
        }
    };
    let mut per_sample = Vec::with_capacity(samples.len());
    for sample in samples {
        // Per-sample cancellation checkpoint (PRD §9.1, "abort within one sample
        // interval"): poll before each sample's RNEA solve so a cancel fired
        // mid-trajectory is observed at the next sample boundary. The partial
        // `per_sample` is dropped — a Cancelled outcome returns no result Value and
        // donates no warm state.
        if cancellation.is_cancelled() {
            return InverseDynamicsRun {
                outcome: ComputeOutcome::Cancelled,
                reused: false,
            };
        }
        match inverse_dynamics_sample(mechanism, sample) {
            Some(forces) => per_sample.push(Value::List(forces)),
            None => {
                return InverseDynamicsRun {
                    outcome: undef_outcome(),
                    reused: false,
                };
            }
        }
    }
    let result = Value::List(per_sample);

    // Donate the freshly-computed result as warm state so a later identical call
    // HITs. The result is wrapped in an `Arc` so the eventual HIT re-donation
    // shares this allocation instead of deep-cloning the trajectory again; on this
    // MISS path `completed_donating` still deep-clones it exactly once for the
    // output value cell (the freshly-built `Value::List` would otherwise be moved
    // whole into the cache, leaving nothing for the cell).
    let cache = InverseDynamicsCache {
        key,
        body_solid_hashes,
        result: Arc::new(result),
    };
    InverseDynamicsRun {
        outcome: completed_donating(cache),
        reused: false,
    }
}

/// `@optimized("dynamics::inverse_dynamics")` public `ComputeFn` for `fn
/// inverse_dynamics` (registered in `compute_targets::mod`, step-14). A thin
/// wrapper over the in-crate core [`run_inverse_dynamics`]: it forwards the prior
/// warm state and the cancellation handle and surfaces only the [`ComputeOutcome`].
/// Warm-state donation/recovery (the cached result) and cooperative cancellation
/// live in the core; the core's white-box `reused` flag is for in-crate
/// amortization tests only. Mirrors `solve_modal_analysis_trampoline`.
pub fn solve_inverse_dynamics_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    run_inverse_dynamics(value_inputs, prior_warm_state, cancellation).outcome
}

#[cfg(test)]
mod inverse_dynamics_trampoline_tests {
    use reify_core::dimension::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use reify_stdlib::eval_builtin;

    use super::{InverseDynamicsRun, run_inverse_dynamics};
    use crate::{CancellationHandle, ComputeOutcome};

    /// Static single-pendulum ground truth: τ = m·g·L·sin(30°)
    /// = 1·9.81·0.1·0.5 = 0.4905 N·m (validated at <1e-6 by `rnea.rs` and
    /// `dynamics::eval.rs`).
    const STATIC_TORQUE: f64 = 0.4905;

    /// Mint a registry-free `Value::StructureInstance` (mirrors the eval-side
    /// `mint_instance`).
    fn instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// Pull a named field from a `StructureInstance`, asserting `type_name`.
    fn field<'a>(v: &'a Value, type_name: &str, member: &str) -> &'a Value {
        match v {
            Value::StructureInstance(d) if d.type_name == type_name => d
                .fields
                .get(member)
                .unwrap_or_else(|| panic!("{type_name} missing field `{member}`")),
            other => panic!("expected a {type_name} StructureInstance, got {other:?}"),
        }
    }

    /// A `MassProperties` instance: mass (Mass-scalar), com (Point3<Length>),
    /// inertia (3×3 Matrix of **`Value::Real`**), origin (Real).
    ///
    /// The inertia cells are intentionally `Value::Real` (the legacy / user-authored
    /// encoding) rather than `Value::Scalar{MOMENT_OF_INERTIA}` (production path).
    /// This exercises the backward-compatible code path: `cell_f64` accepts both
    /// `Value::Real(x)` and `Value::Scalar { si_value: x, .. }`, so RNEA extraction
    /// is dimension-agnostic and produces identical numerics for both encodings.
    /// See `make_mass_properties` / `mass_properties_fixture` in `reify-stdlib` for
    /// the production-faithful dimensioned shape.
    fn mass_properties(mass: f64, com: [f64; 3], inertia: [[f64; 3]; 3]) -> Value {
        let com_point = Value::Point(com.iter().map(|&c| Value::length(c)).collect());
        let inertia_matrix = Value::Matrix(
            inertia
                .iter()
                .map(|row| row.iter().map(|&x| Value::Real(x)).collect())
                .collect(),
        );
        instance(
            "MassProperties",
            vec![
                (
                    "mass".to_string(),
                    Value::Scalar {
                        si_value: mass,
                        dimension: DimensionVector::MASS,
                    },
                ),
                ("com".to_string(), com_point),
                ("inertia".to_string(), inertia_matrix),
                ("origin".to_string(), Value::Real(0.0)),
            ],
        )
    }

    /// The single-pendulum mechanism (1 kg point mass at com=[0,0,−0.1] on a
    /// revolute joint about +y), built via the `eval_builtin` mechanism/body/
    /// joint builders so the Map shape (kind, bodies.solid, joint_parents) is the
    /// real one the trampoline reads.
    fn pendulum_mechanism() -> Value {
        use std::f64::consts::PI;
        let mp = mass_properties(1.0, [0.0, 0.0, -0.1], [[0.0; 3]; 3]);
        let axis_y = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(-PI))),
            upper: Some(Box::new(Value::angle(PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis_y, range]);
        let mech = eval_builtin("mechanism", &[]);
        eval_builtin("body", &[mech, mp, joint])
    }

    /// A motionless single-pendulum `MotionTrajectory` of `n` samples, all at
    /// θ = −30° with q̇ = q̈ = 0, so every sample's RNEA torque is the
    /// static-gravity 0.4905 N·m.
    fn motionless_trajectory(n: usize) -> Value {
        let theta = -std::f64::consts::PI / 6.0;
        let samples: Vec<Value> = (0..n)
            .map(|k| {
                instance(
                    "TrajectorySample",
                    vec![
                        (
                            "t".to_string(),
                            Value::Scalar {
                                si_value: k as f64,
                                dimension: DimensionVector::TIME,
                            },
                        ),
                        ("values".to_string(), Value::List(vec![Value::Real(theta)])),
                        ("vels".to_string(), Value::List(vec![Value::Real(0.0)])),
                        ("accels".to_string(), Value::List(vec![Value::Real(0.0)])),
                    ],
                )
            })
            .collect();
        instance(
            "MotionTrajectory",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                ("samples".to_string(), Value::List(samples)),
            ],
        )
    }

    /// Assert `result` is a `List<List<JointForce>>` of `expected_samples` outer
    /// entries, each a length-1 inner list whose `JointForce.value` is a
    /// `ScalarTorque` of magnitude ≈ 0.4905 N·m (<1e-6).
    fn assert_static_pendulum_result(result: &Value, expected_samples: usize) {
        let per_sample = match result {
            Value::List(s) => s,
            other => panic!("expected List<List<JointForce>>, got {other:?}"),
        };
        assert_eq!(
            per_sample.len(),
            expected_samples,
            "one force list per sample"
        );
        for (i, sample_forces) in per_sample.iter().enumerate() {
            let forces = match sample_forces {
                Value::List(f) => f,
                other => panic!("sample {i}: expected List<JointForce>, got {other:?}"),
            };
            assert_eq!(forces.len(), 1, "sample {i}: one joint ⇒ one JointForce");
            let value = field(&forces[0], "JointForce", "value");
            let magnitude = match field(value, "ScalarTorque", "magnitude") {
                Value::Real(m) => *m,
                other => panic!("magnitude must be a Real, got {other:?}"),
            };
            assert!(
                (magnitude - STATIC_TORQUE).abs() < 1e-6,
                "sample {i}: expected {STATIC_TORQUE} N·m, got {magnitude}"
            );
        }
    }

    // ── step-7 RED: run_inverse_dynamics MISS path ──────────────────────────────

    /// A fresh run (prior_warm_state = None) on the motionless single-pendulum
    /// trajectory completes with `reused = false`, donates a warm-state cache,
    /// and returns every sample's static-gravity torque (0.4905 N·m).
    #[test]
    fn run_inverse_dynamics_miss_returns_static_torque_and_donates_warm_state() {
        let mech = pendulum_mechanism();
        let traj = motionless_trajectory(2);

        let run: InverseDynamicsRun =
            run_inverse_dynamics(&[mech, traj], None, &CancellationHandle::new());

        assert!(
            !run.reused,
            "a fresh run (no prior warm state) must not be a cache reuse"
        );
        match &run.outcome {
            ComputeOutcome::Completed {
                result,
                new_warm_state,
                ..
            } => {
                assert!(
                    new_warm_state.is_some(),
                    "the MISS path must donate a warm-state cache"
                );
                assert_static_pendulum_result(result, 2);
            }
            other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
        }
    }

    // ── step-9 RED: run_inverse_dynamics cache HIT ──────────────────────────────

    /// A second run with identical `[mechanism, trajectory]` inputs, fed the
    /// warm-state cache the first (MISS) run donated, is a cache HIT: it reports
    /// `reused = true` and returns a result equal to the first run's — without
    /// recomputing the per-sample RNEA loop. RED until the step-10 HIT path reads
    /// `prior_warm_state` (step-8 always recomputes, so `reused` stays false).
    #[test]
    fn run_inverse_dynamics_hit_reuses_donated_warm_state() {
        let inputs = [pendulum_mechanism(), motionless_trajectory(2)];
        let handle = CancellationHandle::new();

        // First run: cache MISS, donates a warm-state cache.
        let first = run_inverse_dynamics(&inputs, None, &handle);
        assert!(
            !first.reused,
            "first run (no prior warm state) must be a MISS"
        );
        let (first_result, warm) = match first.outcome {
            ComputeOutcome::Completed {
                result,
                new_warm_state,
                ..
            } => (
                result,
                new_warm_state.expect("the MISS path must donate a warm state"),
            ),
            other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
        };

        // Second run: identical inputs + the donated warm state ⇒ cache HIT.
        let second = run_inverse_dynamics(&inputs, Some(&warm), &handle);
        assert!(
            second.reused,
            "identical inputs + a matching warm state must be a cache HIT (reused=true)"
        );
        match second.outcome {
            ComputeOutcome::Completed { result, .. } => assert_eq!(
                result, first_result,
                "the cache HIT must return the cached result unchanged"
            ),
            other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
        }
    }

    // ── step-11 RED: run_inverse_dynamics cooperative cancellation ──────────────

    /// An already-cancelled handle short-circuits to `ComputeOutcome::Cancelled`
    /// (no partial result `Value` is produced), even with a multi-sample
    /// trajectory. RED until step-12 polls the handle: step-10 binds `cancellation`
    /// with `let _` and always runs to Completed.
    #[test]
    fn run_inverse_dynamics_pre_cancelled_yields_cancelled() {
        let inputs = [pendulum_mechanism(), motionless_trajectory(4)];
        let handle = CancellationHandle::new();
        handle.cancel();

        let run = run_inverse_dynamics(&inputs, None, &handle);
        assert!(!run.reused, "a cancelled run is not a cache reuse");
        assert!(
            matches!(run.outcome, ComputeOutcome::Cancelled),
            "a pre-cancelled handle must yield Cancelled (no partial result), got {:?}",
            run.outcome
        );
    }

    /// A cancel fired while the per-sample loop is active is observed at a sample
    /// boundary and yields `ComputeOutcome::Cancelled`. Together with
    /// [`run_inverse_dynamics_pre_cancelled_yields_cancelled`] (which pins the
    /// on-entry checkpoint) this exercises the per-sample polling granularity
    /// (PRD §9.1, "abort within 1 sample interval") that an entry-only check could
    /// not deliver.
    ///
    /// The observation is made robust instead of racing wall-clock time. Per-sample
    /// RNEA for a 1-joint pendulum is so cheap that a fixed (sample-count, delay)
    /// pair can let the loop finish before the canceller thread is even scheduled;
    /// the old version asserted on that race and so could flake on a fast/loaded CI
    /// box. Here a too-fast completion is NOT a failure — it means "this trajectory
    /// was too short for this machine", so we retry with a 4× longer one. The
    /// canceller sleeps ~1 ms so the main-thread solve is provably past its on-entry
    /// poll and deep inside the loop when the cancel lands, and the length grows
    /// until it outlasts that delay: a correct per-sample poll converges to
    /// Cancelled (in practice on the first iteration), whereas an implementation
    /// that dropped the per-sample poll would keep Completing and trip the
    /// escalation ceiling. (`CancellationHandle` is a plain `Arc<AtomicBool>` with
    /// no per-poll hook, so a single-threaded "flip after N samples" cannot be
    /// injected; the escalating retry is the deterministic-failure, non-flaky
    /// alternative the reviewer asked for.)
    #[test]
    fn run_inverse_dynamics_cancelled_mid_loop_yields_cancelled() {
        // Start well above the ~1 ms cancel delay (cheap 1-joint RNEA × samples) and
        // escalate only if the solve still beat the canceller. The ceiling is a
        // safety net — a correct per-sample poll converges on the first iteration.
        const MAX_SAMPLES: usize = 5_000_000;
        let mut samples = 20_000usize;
        loop {
            let inputs = [pendulum_mechanism(), motionless_trajectory(samples)];
            let handle = CancellationHandle::new();
            let canceller_handle = handle.clone();
            let canceller = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1));
                canceller_handle.cancel();
            });

            let run = run_inverse_dynamics(&inputs, None, &handle);
            canceller.join().expect("canceller thread must not panic");

            match run.outcome {
                ComputeOutcome::Cancelled => {
                    assert!(!run.reused, "a cancelled run is not a cache reuse");
                    return;
                }
                // Loop finished before the cancel was observed: too short for this
                // machine. Lengthen and retry — a too-fast completion is never a
                // failure (that was the old flake).
                ComputeOutcome::Completed { .. } => {
                    let next = samples.saturating_mul(4);
                    assert!(
                        next <= MAX_SAMPLES,
                        "cancellation was never observed even at {samples} samples \
                         (next {next} > {MAX_SAMPLES} ceiling); the per-sample \
                         cancellation poll may be missing",
                    );
                    samples = next;
                }
                other => panic!("expected Cancelled or Completed, got {other:?}"),
            }
        }
    }
}

// ── task-4472 step-5 RED: derive_mechanism_mass_props ────────────────────────
//
// Tests for the build-time mechanism-mass derivation pass.  The function
// `derive_mechanism_mass_props` does not exist yet; all tests below are RED.
//
// The function under test:
//   pub fn derive_mechanism_mass_props(
//       value: &Value,
//       kernel: &dyn GeometryKernel,
//       diagnostics: &mut Vec<Diagnostic>,
//   ) -> Option<Value>
//
// Invariants exercised:
//   (a) mechanism with a GeometryHandle body → Some(patched) with additive
//       `derived_mass_props` and original `solid` still present.
//   (b) body with a MassProperties solid → unpatched; non-GeometryHandle body
//       → unpatched; mechanism with no patchable body → None.
//   (c) non-mechanism Value → None.
//   (d) kernel failure for a geometry body → body skipped, Warning diagnostic.

#[cfg(test)]
mod derive_mechanism_mass_props_tests {
    use std::collections::BTreeMap;

    use reify_core::{RealizationNodeId, Severity};
    use reify_ir::{
        GeometryHandleId, PersistentMap, StructureInstanceData, StructureTypeId, Value,
    };
    use reify_test_support::mocks::MockGeometryKernel;

    use super::derive_mechanism_mass_props;

    /// Fixed kernel handle for the GeometryHandle body in derivation tests.
    const HANDLE_ID: GeometryHandleId = GeometryHandleId(42);

    /// Build a minimal mechanism `Value::Map` containing a single body.
    ///
    /// The mechanism map has: kind="mechanism", bodies=[body_map].
    /// The body map has: id=0, solid=`solid_value`. All other mechanism
    /// fields (joint_parents, loop_closures, next_id) are omitted — the
    /// derivation pass only reads `kind` and `bodies[*].solid`, so this
    /// minimal layout is sufficient.
    fn one_body_mechanism(solid_value: Value) -> Value {
        let mut body = BTreeMap::new();
        body.insert(Value::String("id".to_string()), Value::Int(0));
        body.insert(Value::String("solid".to_string()), solid_value);

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body)]),
        );
        Value::Map(mech)
    }

    /// Build a `Value::GeometryHandle` for `HANDLE_ID`.
    fn geometry_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Design", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(HANDLE_ID),
        }
    }

    /// Build a MockGeometryKernel with Volume / CenterOfMass / InertiaTensor
    /// replies for `HANDLE_ID` at the water-default density (1000.0 kg/m³).
    ///
    /// Injected values: volume=6.0, com={x:0.1,y:0.2,z:0.3},
    /// inertia=diagonal(1,2,3). Mass = 1000×6 = 6000 kg.
    fn mock_kernel() -> MockGeometryKernel {
        let inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        MockGeometryKernel::new()
            .with_volume_result(HANDLE_ID, Value::Real(6.0))
            .with_center_of_mass_result(
                HANDLE_ID,
                1000.0,
                Value::String("{\"x\":0.1,\"y\":0.2,\"z\":0.3}".to_string()),
            )
            .with_inertia_tensor_result(HANDLE_ID, 1000.0, inertia)
    }

    /// Helper: extract `derived_mass_props` from the first body of a patched
    /// mechanism value, asserting the structure along the way.
    fn first_body_derived_mass_props(patched: &Value) -> &Value {
        let mech_map = match patched {
            Value::Map(m) => m,
            other => panic!("patched must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("patched mechanism missing bodies"),
        };
        assert_eq!(bodies.len(), 1, "expected exactly one body");
        let body_map = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("body must be a Map, got {other:?}"),
        };
        body_map
            .get(&Value::String("derived_mass_props".to_string()))
            .unwrap_or_else(|| panic!("first body missing derived_mass_props key"))
    }

    // ── (a) GeometryHandle body → Some(patched) with additive derived_mass_props ─

    /// A mechanism body with solid = GeometryHandle and the water-default density
    /// (no explicit material on the body) must yield Some(patched) where the first
    /// body gains `derived_mass_props` with mass = 1000×volume = 6000.0 kg, and
    /// the original `solid` GeometryHandle is still present in the patched body.
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_patches_geometry_handle_body() {
        let mech = one_body_mechanism(geometry_handle());
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);
        let patched = result.expect("must return Some(patched) for geometry-backed body");

        // Derived mass properties must be present and be a MassProperties instance.
        let mp = first_body_derived_mass_props(&patched);
        let data = match mp {
            Value::StructureInstance(d) => d,
            other => panic!("derived_mass_props must be a StructureInstance, got {other:?}"),
        };
        assert_eq!(data.type_name, "MassProperties");

        // mass = density × volume = 1000 × 6 = 6000.0
        let mass_field = data.fields.get("mass").expect("mass field");
        let mass_f64 = match mass_field {
            Value::Real(r) => *r,
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("mass must be numeric, got {other:?}"),
        };
        assert!(
            (mass_f64 - 6000.0).abs() < 1e-9,
            "mass = density×volume = 1000×6 = 6000.0; got {mass_f64}"
        );

        // No diagnostic should be emitted on the success path.
        // (The internal build pass uses the water-default density silently;
        // E_DynamicsNoDensity belongs to the user-facing body_mass_props() call only.)
        assert!(
            diags.is_empty(),
            "no diagnostic expected on success; got: {diags:?}"
        );
    }

    /// The original `solid` GeometryHandle must be preserved in the patched body
    /// — the derived pass writes ADDITIVELY and must not replace body.solid.
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_preserves_solid_in_patched_body() {
        let handle = geometry_handle();
        let mech = one_body_mechanism(handle.clone());
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let patched = derive_mechanism_mass_props(&mech, &kernel, &mut diags)
            .expect("must return Some for geometry-backed body");

        let mech_map = match &patched {
            Value::Map(m) => m,
            other => panic!("patched must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("missing bodies"),
        };
        let body_map = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("body must be a Map, got {other:?}"),
        };
        let solid = body_map
            .get(&Value::String("solid".to_string()))
            .expect("solid must still be present after additive write");
        assert_eq!(
            solid, &handle,
            "solid must equal the original GeometryHandle"
        );
    }

    // ── (b) unpatched bodies ─────────────────────────────────────────────────

    /// A mechanism body whose `solid` is already a MassProperties StructureInstance
    /// must NOT be patched (no `derived_mass_props` inserted). This is the case
    /// where rung (a) would already resolve — no need for the build pass to add a
    /// redundant derived field.
    ///
    /// A mechanism with NO geometry-backed body → None (nothing to patch).
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_skips_mass_properties_solid_body() {
        // Build a MassProperties StructureInstance as the body's solid.
        let mp_solid = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "MassProperties".to_string(),
            version: 1,
            fields: [("mass".to_string(), Value::Real(1.0))]
                .into_iter()
                .collect::<PersistentMap<_, _>>(),
        }));
        let mech = one_body_mechanism(mp_solid);
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        // A mechanism with only a MassProperties solid (no GeometryHandle) means
        // no body was patched → None.
        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "must return None when no body has a GeometryHandle solid; got {result:?}"
        );
        assert!(diags.is_empty(), "no diagnostics expected; got: {diags:?}");
    }

    /// A mechanism body whose `solid` is a non-handle, non-MassProperties value
    /// (e.g. a plain String) must NOT be patched, and since no body is patchable
    /// the function returns None.
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_skips_non_handle_solid_body() {
        let mech = one_body_mechanism(Value::String("placeholder".to_string()));
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);
        assert!(
            result.is_none(),
            "must return None for non-GeometryHandle body solid; got {result:?}"
        );
        assert!(diags.is_empty(), "no diagnostics expected; got: {diags:?}");
    }

    // ── (c) non-mechanism Value → None ───────────────────────────────────────

    /// Passing a non-mechanism Value (a plain List, or a Map without
    /// kind="mechanism") must return None — the pass only touches mechanism cells.
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_returns_none_for_non_mechanism() {
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        // Plain list.
        assert!(
            derive_mechanism_mass_props(&Value::List(vec![]), &kernel, &mut diags).is_none(),
            "Value::List must return None"
        );
        // Map without kind="mechanism".
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("snapshot".to_string()),
        );
        assert!(
            derive_mechanism_mass_props(&Value::Map(m), &kernel, &mut diags).is_none(),
            "Map with kind != 'mechanism' must return None"
        );
        // A non-Map Value.
        assert!(
            derive_mechanism_mass_props(&Value::Int(99), &kernel, &mut diags).is_none(),
            "Value::Int must return None"
        );
        assert!(
            diags.is_empty(),
            "no diagnostics for non-mechanism; got: {diags:?}"
        );
    }

    // ── (d) kernel failure → body skipped, Warning diagnostic ────────────────

    /// When the kernel query fails for a geometry-backed body (e.g. no Volume
    /// reply injected), the body must be skipped (no `derived_mass_props` written),
    /// a `Warning` diagnostic must be emitted, and since no body was successfully
    /// patched the function returns None.
    ///
    /// RED: `derive_mechanism_mass_props` does not exist yet.
    #[test]
    fn derive_mechanism_mass_props_emits_warning_and_skips_on_kernel_failure() {
        let mech = one_body_mechanism(geometry_handle());
        // Bare kernel — no replies injected, so Volume query will fail.
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);

        // No body was successfully patched → None.
        assert!(
            result.is_none(),
            "must return None when kernel fails for all bodies; got {result:?}"
        );
        // A Warning diagnostic must be emitted.
        assert!(
            !diags.is_empty(),
            "a Warning diagnostic must be emitted on kernel failure"
        );
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "emitted diagnostic must have Warning severity; got: {diags:?}"
        );
    }

    // ── multi-body partial-success ────────────────────────────────────────────

    /// A second handle id for the partial-failure two-body tests — distinct from
    /// HANDLE_ID (42) so injected replies can target each handle independently.
    const HANDLE_ID2: GeometryHandleId = GeometryHandleId(43);

    /// Build a mechanism `Value::Map` containing TWO bodies (body[0] with
    /// `solid=solid0`, body[1] with `solid=solid1`). Used to exercise the
    /// multi-body partial-success path where some bodies are patched and some
    /// are skipped.
    fn two_body_mechanism(solid0: Value, solid1: Value) -> Value {
        let mut body0 = BTreeMap::new();
        body0.insert(Value::String("id".to_string()), Value::Int(0));
        body0.insert(Value::String("solid".to_string()), solid0);

        let mut body1 = BTreeMap::new();
        body1.insert(Value::String("id".to_string()), Value::Int(1));
        body1.insert(Value::String("solid".to_string()), solid1);

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body0), Value::Map(body1)]),
        );
        Value::Map(mech)
    }

    /// Extract the body map at `index` from a patched mechanism, asserting the
    /// structure along the way.
    fn body_at(patched: &Value, index: usize) -> &std::collections::BTreeMap<Value, Value> {
        let mech_map = match patched {
            Value::Map(m) => m,
            other => panic!("patched must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("patched mechanism missing bodies"),
        };
        match &bodies[index] {
            Value::Map(b) => b,
            other => panic!("body[{index}] must be a Map, got {other:?}"),
        }
    }

    /// Two-body mechanism: body[0] has a working geometry handle (HANDLE_ID),
    /// body[1] has a non-handle solid (String placeholder).
    ///
    /// Expected: Some(patched), body[0] gains derived_mass_props, body[1] does
    /// NOT gain derived_mass_props, and body order (id fields) is preserved.
    /// No Warning should be emitted for the non-handle body.
    #[test]
    fn derive_mechanism_mass_props_two_body_first_patched_second_non_handle() {
        let mech = two_body_mechanism(geometry_handle(), Value::String("placeholder".to_string()));
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);
        let patched = result.expect("must return Some when at least one body is patched");

        // Body order is preserved (2 bodies total).
        let mech_map = match &patched {
            Value::Map(m) => m,
            other => panic!("patched must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("missing bodies"),
        };
        assert_eq!(bodies.len(), 2, "body order must be preserved (2 bodies)");

        // body[0]: id=0 (order preserved) and carries derived_mass_props.
        let b0 = body_at(&patched, 0);
        assert_eq!(
            b0.get(&Value::String("id".to_string())),
            Some(&Value::Int(0))
        );
        assert!(
            b0.contains_key(&Value::String("derived_mass_props".to_string())),
            "body[0] must carry derived_mass_props; keys: {:?}",
            b0.keys().collect::<Vec<_>>()
        );

        // body[1]: id=1 (order preserved) and does NOT carry derived_mass_props.
        let b1 = body_at(&patched, 1);
        assert_eq!(
            b1.get(&Value::String("id".to_string())),
            Some(&Value::Int(1))
        );
        assert!(
            !b1.contains_key(&Value::String("derived_mass_props".to_string())),
            "body[1] (non-handle) must not carry derived_mass_props"
        );

        // No diagnostic for the non-handle skip path.
        assert!(
            diags.is_empty(),
            "non-handle skip must not emit diagnostics; got: {diags:?}"
        );
    }

    /// Two-body mechanism: body[0] has a working geometry handle (HANDLE_ID,
    /// replies injected), body[1] has a geometry handle (HANDLE_ID2, no replies
    /// injected → kernel failure).
    ///
    /// Expected: Some(patched) because body[0] succeeded; body[0] gains
    /// derived_mass_props; body[1] does NOT gain derived_mass_props; a Warning
    /// is emitted for body[1]'s kernel failure; body order is preserved.
    #[test]
    fn derive_mechanism_mass_props_two_body_first_patched_second_kernel_failure() {
        let handle0 = geometry_handle(); // HANDLE_ID=42, replies injected by mock_kernel()
        let handle1 = Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Design", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(HANDLE_ID2), // no replies → kernel failure
        };
        let mech = two_body_mechanism(handle0, handle1);
        // mock_kernel() has replies only for HANDLE_ID=42; HANDLE_ID2=43 has none.
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech, &kernel, &mut diags);
        let patched = result.expect("must return Some when body[0] succeeded");

        // Body order preserved (2 bodies).
        let mech_map = match &patched {
            Value::Map(m) => m,
            other => panic!("patched must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("missing bodies"),
        };
        assert_eq!(bodies.len(), 2, "body order must be preserved (2 bodies)");

        // body[0]: patched.
        let b0 = body_at(&patched, 0);
        assert_eq!(
            b0.get(&Value::String("id".to_string())),
            Some(&Value::Int(0))
        );
        assert!(
            b0.contains_key(&Value::String("derived_mass_props".to_string())),
            "body[0] must carry derived_mass_props"
        );

        // body[1]: skipped (kernel failure).
        let b1 = body_at(&patched, 1);
        assert_eq!(
            b1.get(&Value::String("id".to_string())),
            Some(&Value::Int(1))
        );
        assert!(
            !b1.contains_key(&Value::String("derived_mass_props".to_string())),
            "body[1] (kernel failure) must not carry derived_mass_props"
        );

        // A Warning must be emitted for body[1]'s kernel failure.
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "kernel failure on body[1] must emit a Warning; got: {diags:?}"
        );
    }

    // ── idempotency guard ────────────────────────────────────────────────────

    /// A mechanism body that already carries a `derived_mass_props` MassProperties
    /// from a prior pass must be skipped without issuing any kernel queries.
    /// When NO body needed re-derivation (all already derived), the function
    /// returns None — no-change, no kernel round-trips.
    #[test]
    fn derive_mechanism_mass_props_skips_already_derived_body() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        // Build a body that already has derived_mass_props (MassProperties instance).
        let existing_mp = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "MassProperties".to_string(),
            version: 1,
            fields: [("mass".to_string(), Value::Real(6000.0))]
                .into_iter()
                .collect::<PersistentMap<_, _>>(),
        }));
        let mut body = BTreeMap::new();
        body.insert(Value::String("id".to_string()), Value::Int(0));
        body.insert(Value::String("solid".to_string()), geometry_handle());
        body.insert(Value::String("derived_mass_props".to_string()), existing_mp);

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body)]),
        );
        let mech_value = Value::Map(mech);

        // The bare kernel has no injected replies; if the guard were absent, the
        // kernel query would fail and a Warning would be emitted.
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech_value, &kernel, &mut diags);

        // All bodies were already derived → None (no-change, no kernel round-trip).
        assert!(
            result.is_none(),
            "already-derived mechanism must return None (no re-query); got {result:?}"
        );
        // No Warning must be emitted — the guard skipped the kernel call entirely.
        assert!(
            diags.is_empty(),
            "idempotency guard must not emit any diagnostics; got: {diags:?}"
        );
    }

    /// Two-body mixed idempotency test: body[0] already carries a sentinel
    /// `derived_mass_props` (mass=9999.0, a distinct value distinguishable from
    /// the freshly-derived mass=6000.0), body[1] is a fresh geometry body
    /// (HANDLE_ID, replies injected via `mock_kernel()`).
    ///
    /// Expected invariants:
    /// - body[0] idempotency guard fires → no kernel call → `derived_mass_props`
    ///   remains byte-identical to the input sentinel (mass=9999.0).
    /// - body[1] is freshly derived → gains `derived_mass_props` (mass=6000.0).
    /// - No Warning is emitted (guard skipped body[0], body[1] succeeded).
    ///
    /// body[0]'s solid is HANDLE_ID2 (43) which has no kernel replies; if the
    /// guard were absent, querying body[0] would emit a Warning and the
    /// `diags.is_empty()` assertion below would fail — verifying that the guard
    /// does not re-query already-derived bodies even in the mixed-body case.
    #[test]
    fn derive_mechanism_mass_props_mixed_idempotency_and_fresh_derivation() {
        // body[0]: already-derived sentinel; solid = HANDLE_ID2 (no replies → fails if queried).
        let sentinel_mp = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "MassProperties".to_string(),
            version: 1,
            fields: [("mass".to_string(), Value::Real(9999.0))]
                .into_iter()
                .collect::<PersistentMap<_, _>>(),
        }));
        let handle_no_replies = Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Design", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(HANDLE_ID2), // no replies in mock_kernel() → would warn if queried
        };
        let mut body0 = BTreeMap::new();
        body0.insert(Value::String("id".to_string()), Value::Int(0));
        body0.insert(Value::String("solid".to_string()), handle_no_replies);
        body0.insert(
            Value::String("derived_mass_props".to_string()),
            sentinel_mp.clone(),
        );

        // body[1]: fresh geometry body, HANDLE_ID (replies injected by mock_kernel()).
        let mut body1 = BTreeMap::new();
        body1.insert(Value::String("id".to_string()), Value::Int(1));
        body1.insert(Value::String("solid".to_string()), geometry_handle());

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body0), Value::Map(body1)]),
        );
        let mech_value = Value::Map(mech);

        // mock_kernel() has replies for HANDLE_ID=42 only; HANDLE_ID2=43 has none.
        let kernel = mock_kernel();
        let mut diags = Vec::new();

        let result = derive_mechanism_mass_props(&mech_value, &kernel, &mut diags);
        let patched = result.expect("must return Some (body[1] was freshly derived)");

        // body[0]: guard fired → derived_mass_props must be byte-identical to sentinel.
        let b0 = body_at(&patched, 0);
        let b0_mp = b0
            .get(&Value::String("derived_mass_props".to_string()))
            .expect("body[0] must still carry derived_mass_props after guard");
        assert_eq!(
            b0_mp, &sentinel_mp,
            "body[0] derived_mass_props must be byte-identical to the input sentinel \
             (guard must not re-derive when derived_mass_props is already present)"
        );

        // body[1]: freshly derived → must carry derived_mass_props with mass=6000.0.
        let b1 = body_at(&patched, 1);
        assert!(
            b1.contains_key(&Value::String("derived_mass_props".to_string())),
            "body[1] must carry derived_mass_props after fresh derivation"
        );
        let b1_data = match b1
            .get(&Value::String("derived_mass_props".to_string()))
            .unwrap()
        {
            Value::StructureInstance(d) => d,
            other => panic!("body[1] derived_mass_props must be StructureInstance, got {other:?}"),
        };
        let mass_f64 = match b1_data.fields.get("mass").expect("mass field") {
            Value::Real(r) => *r,
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("mass must be numeric, got {other:?}"),
        };
        assert!(
            (mass_f64 - 6000.0).abs() < 1e-9,
            "body[1] mass = 1000 × 6 = 6000.0; got {mass_f64}"
        );

        // No Warning emitted: guard skipped body[0] cleanly, body[1] succeeded.
        assert!(
            diags.is_empty(),
            "no diagnostic expected in mixed idempotency case; got: {diags:?}"
        );
    }
}
