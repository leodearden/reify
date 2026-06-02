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
//!     `W_DynamicsDefaultDensity` on default-water fallback, and builds the
//!     `MassProperties` instance. Kernel-free and unit-testable.
//!   * [`try_eval_body_mass_props`] — dispatch recognition for a
//!     `body_mass_props(...)` call cell: resolves the body + optional density
//!     args, runs the density ladder (emitting `W_DynamicsDefaultDensity`), and
//!     assembles a `MassProperties` whose geometric fields stay the deferred
//!     `Value::Undef` sentinel until the KGQ kernel seam (task 3620) is wired.

use reify_core::dimension::DimensionVector;
use reify_core::{ContentHash, Diagnostic, DiagnosticCode};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::dynamics::eval::{inverse_dynamics_sample, motion_trajectory_samples};
use reify_stdlib::dynamics::mass_props::{DensitySource, resolve_density};
use reify_stdlib::dynamics::rnea::default_gravity;
use reify_stdlib::dynamics::trampoline::{body_solid_hashes, InverseDynamicsCacheKey};

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
        && let Some(Value::StructureInstance(material)) =
            data.fields.get("material")
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

/// Run the fn-level density priority ladder for `body_mass_props` and emit the
/// `W_DynamicsDefaultDensity` warning (once) when it falls through to the water
/// default. Returns the resolved density (kg/m³).
///
/// Shared by [`eval_body_mass_props_core`] (concrete-geometry path) and
/// `try_eval_body_mass_props` (deferred-kernel dispatch path) so the ladder and
/// the diagnostic are single-sourced regardless of whether the geometric query
/// is available.
fn resolve_body_density(
    body: &Value,
    density_arg: Option<&Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let explicit = density_arg.and_then(cell_f64);
    let material = body_material_density(body);
    let (density, source) = resolve_density(explicit, material);
    if source == DensitySource::DefaultWater {
        diagnostics.push(
            Diagnostic::warning(format!(
                "body_mass_props('{}'): no explicit density and no Material density; \
                 defaulting to 1000 kg/m³ (water)",
                body_label(body),
            ))
            .with_code(DiagnosticCode::DynamicsDefaultDensity),
        );
    }
    density
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

/// Inertia `Value` for `MassProperties.inertia : Matrix<3,3,Real>` — a 3×3
/// `Value::Matrix` of plain `Real` cells (so the existing
/// `dynamics_psd::inertia_3x3_from_value` parser and the engine PSD hook read
/// it unchanged).
fn inertia_value(inertia: [[f64; 3]; 3]) -> Value {
    Value::Matrix(
        inertia
            .iter()
            .map(|row| row.iter().map(|&x| Value::Real(x)).collect())
            .collect(),
    )
}

/// Assemble a `MassProperties` `Value::StructureInstance` from its four field
/// values. The geometric fields (`mass`, `com`, `inertia`) are passed as
/// `Value`s so this single assembler serves both the concrete-geometry core and
/// the deferred-kernel dispatch path (which passes `Value::Undef` for them).
/// `origin` is the `Real` frame placeholder matching the structure_def.
///
/// Reuses the `modal_ops`/`StructureInstanceData` construction pattern (task
/// 3822 MassProperties structure_def, `dynamics.ri`).
fn assemble_mass_properties(mass: Value, com: Value, inertia: Value) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("mass".to_string(), mass),
        ("com".to_string(), com),
        ("inertia".to_string(), inertia),
        ("origin".to_string(), Value::Real(0.0)),
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
/// `W_DynamicsDefaultDensity` on water fallback), invoke the injected geometric
/// query, and assemble the `MassProperties` instance.
///
/// `geom_query` is the kernel seam abstracted as a closure `density -> (mass,
/// com, inertia)`; this keeps the core kernel-free and exactly unit-testable
/// (the tests inject `reify_stdlib::dynamics::mass_props::uniform_box_inertia`).
/// The deferred-kernel dispatch path (`try_eval_body_mass_props`) does NOT route
/// geometry through here — it reuses [`resolve_body_density`] +
/// [`assemble_mass_properties`] with `Undef` geometric fields until task 3620
/// lands; once it does, the supervisor routes the real kernel query through this
/// core.
pub fn eval_body_mass_props_core(
    body: &Value,
    density_arg: Option<&Value>,
    geom_query: impl Fn(f64) -> (f64, [f64; 3], [[f64; 3]; 3]),
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    let density = resolve_body_density(body, density_arg, diagnostics);
    let (mass, com, inertia) = geom_query(density);
    assemble_mass_properties(mass_value(mass), com_value(com), inertia_value(inertia))
}

/// Resolve a call-argument `CompiledExpr` to the `Value` it denotes: a
/// `ValueRef` is looked up in `values`; an inline `Literal` yields its baked
/// value. Any other expr shape (or a `ValueRef` to an absent cell) yields
/// `None` — mirroring the "unsupported arg shape → fall through" contract of
/// `geometry_ops::resolve_real_scalar_arg` / `resolve_int_value_ref`.
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
/// [`resolve_body_density`], which emits `W_DynamicsDefaultDensity` when neither
/// is present.
///
/// ## Deferred kernel seam — TODO(3620 / KGQ-λ)
/// The density-aware geometric mass/center-of-mass/inertia query
/// (`moment_of_inertia(Solid, Density)`, KGQ Phase 4, task 3620) is **not wired
/// by this batch** — the task description marks this as a cross-PRD edge the
/// supervisor connects later. So the geometric fields of the assembled
/// `MassProperties` are the deferred [`Value::Undef`] sentinel. The single
/// wiring point is marked below: once 3620 lands, build a `geom_query` closure
/// over `kernel` + `body.geometry` and route it through
/// [`eval_body_mass_props_core`] so the concrete tensor replaces the `Undef`s
/// (and is then validated by the existing engine MassProperties PSD hook,
/// against [`uniform_box_inertia`](reify_stdlib::dynamics::mass_props::uniform_box_inertia)
/// as ground truth).
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

    // (4) Optional explicit density argument (args[1]); an absent or
    // unresolvable second arg simply lets the ladder fall through to the
    // Material / default-water rungs.
    let density_arg = args.get(1).and_then(|e| resolve_arg_value(e, values));

    // (5) Run the fn-level density ladder for its `W_DynamicsDefaultDensity`
    // side effect. The resolved magnitude is unused on this deferred path (no
    // geometric query consumes it yet); once the kernel seam below is wired it
    // will feed `geom_query`.
    let _density = resolve_body_density(body, density_arg, diagnostics);

    // (6) Kernel seam — TODO(3620 / KGQ-λ moment_of_inertia(Solid, Density)):
    // this is the single wiring point. When the density-aware KGQ geometry
    // query lands, replace the `Undef` geometric fields below by routing
    //   geom_query = |rho| <kernel query over body.geometry at density rho>
    // through `eval_body_mass_props_core(body, density_arg, geom_query,
    // diagnostics)`. Until then the kernel is unused and the geometric fields
    // are the deferred sentinel.
    let _ = kernel;
    Some(assemble_mass_properties(
        Value::Undef,
        Value::Undef,
        Value::Undef,
    ))
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
        assert!((got - want).abs() < 1e-12, "{what}: expected {want}, got {got}");
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
    // L66's `name`-field read is the only swept read with no direct existing
    // assertion: no prior test builds a body carrying a `name` field, and the
    // default-water tests assert only severity/code (not the warning message).
    // These two tests close that gap: they pin body_label's behaviour so a
    // mis-keyed borrow of "name" (or any typo in the type_name fallback) would
    // be caught immediately.

    #[test]
    fn body_label_uses_name_field_in_default_density_warning() {
        // Build a body carrying an explicit `name` field AND a material with no
        // density (forces the default-water path, which embeds body_label in the
        // warning message).
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

        assert_eq!(diags.len(), 1, "default-water fallback must emit exactly one diagnostic");
        assert!(
            diags[0].message.contains("WidgetA"),
            "warning message must use the body's `name` field; got: {:?}",
            diags[0].message,
        );
    }

    #[test]
    fn body_label_falls_back_to_type_name_without_name_field() {
        // body(None) has no `name` field and type_name "Block"; no density forces
        // the default-water path so body_label's type_name fallback is exercised.
        let b = body(None);
        let mut diags = Vec::new();
        eval_body_mass_props_core(&b, None, |d| uniform_box_inertia(DIMS, d), &mut diags);

        assert_eq!(diags.len(), 1, "default-water fallback must emit exactly one diagnostic");
        assert!(
            diags[0].message.contains("Block"),
            "warning message must fall back to the body's type_name 'Block'; got: {:?}",
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

        assert_eq!(used.get(), 2700.0, "geom_query must be called with the material density");
        assert_matches_geom(&result, 2700.0);
        assert!(
            diags.is_empty(),
            "Material-rung resolution must emit no diagnostics, got {diags:?}"
        );
    }

    // ── Case B: no material density -> default water + warning ───────────────

    #[test]
    fn missing_density_defaults_to_water_with_warning() {
        let b = body(None); // material present but carries no density field
        let used = std::cell::Cell::new(f64::NAN);
        let geom = |density: f64| {
            used.set(density);
            uniform_box_inertia(DIMS, density)
        };
        let mut diags = Vec::new();
        let result = eval_body_mass_props_core(&b, None, geom, &mut diags);

        assert_eq!(used.get(), 1000.0, "geom_query must be called with the 1000 kg/m³ default");
        assert_matches_geom(&result, 1000.0);
        assert_eq!(diags.len(), 1, "default-water fallback must emit exactly one diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsDefaultDensity),
            "default-water diagnostic must carry the DynamicsDefaultDensity code"
        );
    }

    // ── Case C: explicit density arg wins, no warning ────────────────────────

    #[test]
    fn explicit_density_arg_wins_with_no_warning() {
        let b = body(Some(2700.0)); // material present, but explicit arg overrides
        let used = std::cell::Cell::new(f64::NAN);
        let geom = |density: f64| {
            used.set(density);
            uniform_box_inertia(DIMS, density)
        };
        let mut diags = Vec::new();
        let explicit = Value::Real(5000.0);
        let result = eval_body_mass_props_core(&b, Some(&explicit), geom, &mut diags);

        assert_eq!(used.get(), 5000.0, "geom_query must be called with the explicit density");
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
    // the density ladder and the `W_DynamicsDefaultDensity` warning still run.

    /// Build a `<fn_name>(<args…>)` `FunctionCall` expr, each arg a `ValueRef`
    /// to the supplied cell. Mirrors the `geometry_ops` `conformance_call`
    /// content-hash construction so the synthetic expr is well-formed.
    fn call_expr(fn_name: &str, arg_cells: &[ValueCellId]) -> CompiledExpr {
        let args: Vec<CompiledExpr> = arg_cells
            .iter()
            .map(|c| CompiledExpr::value_ref(c.clone(), Type::Real))
            .collect();
        let mut content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(ContentHash::of_str(fn_name));
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

    // ── default-water fallback still emits the warning on the dispatch path ────

    #[test]
    fn dispatch_emits_default_density_warning_when_no_material_density() {
        let body_cell = ValueCellId::new("Design", "blk");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(None)); // material present, no density
        let expr = call_expr("body_mass_props", &[body_cell]);
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        let result = try_eval_body_mass_props(&expr, &values, &kernel, &mut diags)
            .expect("recognised call must return Some even on the default-density path");
        assert_deferred_mass_props(&result);
        assert_eq!(
            diags.len(),
            1,
            "default-water fallback must emit exactly one diagnostic, got {diags:?}"
        );
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::DynamicsDefaultDensity),
            "default-water diagnostic must carry the DynamicsDefaultDensity code"
        );
    }

    // ── explicit density arg (2-arg form) wins, suppresses the warning ────────

    #[test]
    fn dispatch_explicit_density_arg_suppresses_warning() {
        let body_cell = ValueCellId::new("Design", "blk");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mut values = ValueMap::new();
        values.insert(body_cell.clone(), body(None)); // no Material density…
        values.insert(rho_cell.clone(), Value::Real(5000.0)); // …but explicit arg present
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
        assert!(diags.is_empty(), "None-dispatch must not emit diagnostics, got {diags:?}");
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
        assert_eq!(diags.len(), 1, "malformed arity must emit one diagnostic, got {diags:?}");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, Some(DiagnosticCode::DynamicsBodyMassPropsArity));
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
    /// The cached trajectory-level result (`List<List<JointForce>>`).
    result: Value,
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
            + value_size_estimate(&self.result)
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

/// The malformed / closed-chain short-circuit outcome: η's `Value::Undef`
/// convention surfaced as a `Completed` with no donated warm state — keeping the
/// trampoline result bit-identical to the unregistered `inverse_dynamics_lower`
/// body-inline fallback, and donating no warm state for an input that produced no
/// real computation (design_decision #6).
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
fn completed_donating(cache: InverseDynamicsCache) -> ComputeOutcome {
    let result = cache.result.clone();
    let (state, size_bytes) = cache.into_opaque_state();
    let cost_per_byte = if size_bytes > 0 { Some(1.0 / size_bytes as f64) } else { None };
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
/// bit-identical to the body-inline `inverse_dynamics_lower` fallback. A malformed
/// trajectory, or a sample the seam rejects (a closed-chain mechanism, an
/// arity/shape mismatch), yields [`undef_outcome`] — η's exact Undef convention —
/// donating no warm state. Gravity is the constant [`default_gravity`] (PRD §12
/// q1), folded into the cache key.
pub(crate) fn run_inverse_dynamics(
    value_inputs: &[Value],
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> InverseDynamicsRun {
    // step-10 wires the cache-HIT lookup below; cooperative per-sample
    // cancellation polling is still deferred to step-12 — bind the unused handle
    // so this staged GREEN compiles warning-clean.
    let _ = cancellation;

    // Arity guard, mirroring eval_inverse_dynamics: inverse_dynamics(mechanism,
    // trajectory). The engine always supplies the two fn args, so this is defensive.
    if value_inputs.len() != 2 {
        return InverseDynamicsRun { outcome: undef_outcome(), reused: false };
    }
    let mechanism = &value_inputs[0];
    let trajectory = &value_inputs[1];

    // Cache key over the result-determining inputs (constant default gravity).
    let key = InverseDynamicsCacheKey::from_inputs(mechanism, trajectory, default_gravity());

    // ── cache HIT ──────────────────────────────────────────────────────────────
    // A prior warm state whose key matches certifies its cached result for reuse:
    // return it with reused=true WITHOUT re-running the per-sample RNEA loop. The
    // cache is re-donated (cloned, since `prior_warm_state` is borrowed) so the node
    // keeps its warm state and the next identical call can HIT again — a Completed
    // outcome with new_warm_state=None would otherwise clear it.
    if let Some(cache) = prior_warm_state.and_then(|s| s.downcast_ref::<InverseDynamicsCache>())
        && cache.key.matches(&key)
    {
        return InverseDynamicsRun { outcome: completed_donating(cache.clone()), reused: true };
    }

    // ── cache MISS ───────────────────────────────────────────────────────────────
    // Record the body-granular solid-hash invalidation record, then drive the
    // per-sample RNEA loop through the shared stdlib seam so the result is
    // single-sourced with the body-inline fallback. Any `None` (malformed
    // trajectory, closed-chain mechanism, or shape mismatch) collapses to η's Undef.
    let body_solid_hashes = body_solid_hashes(mechanism);
    let samples = match motion_trajectory_samples(trajectory) {
        Some(s) => s,
        None => return InverseDynamicsRun { outcome: undef_outcome(), reused: false },
    };
    let mut per_sample = Vec::with_capacity(samples.len());
    for sample in samples {
        match inverse_dynamics_sample(mechanism, sample) {
            Some(forces) => per_sample.push(Value::List(forces)),
            None => return InverseDynamicsRun { outcome: undef_outcome(), reused: false },
        }
    }
    let result = Value::List(per_sample);

    // Donate the freshly-computed result as warm state so a later identical call HITs.
    let cache = InverseDynamicsCache { key, body_solid_hashes, result };
    InverseDynamicsRun { outcome: completed_donating(cache), reused: false }
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

    use super::{run_inverse_dynamics, InverseDynamicsRun};
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
    /// inertia (3×3 Matrix<Real>), origin (Real) — the shape the η snapshot RNEA
    /// core parses from `body.solid`.
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
                    Value::Scalar { si_value: mass, dimension: DimensionVector::MASS },
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
                            Value::Scalar { si_value: k as f64, dimension: DimensionVector::TIME },
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
        assert_eq!(per_sample.len(), expected_samples, "one force list per sample");
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

        assert!(!run.reused, "a fresh run (no prior warm state) must not be a cache reuse");
        match &run.outcome {
            ComputeOutcome::Completed { result, new_warm_state, .. } => {
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
        assert!(!first.reused, "first run (no prior warm state) must be a MISS");
        let (first_result, warm) = match first.outcome {
            ComputeOutcome::Completed { result, new_warm_state, .. } => {
                (result, new_warm_state.expect("the MISS path must donate a warm state"))
            }
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
}
