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
use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::dynamics::mass_props::{DensitySource, resolve_density};

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
            data.fields.get(&"material".to_string())
        && let Some(cell) = material.fields.get(&"density".to_string())
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
        if let Some(Value::String(name)) = data.fields.get(&"name".to_string()) {
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

    /// Extract an f64 from a numeric cell (`Real` / `Int` / `Scalar`).
    fn num(v: &Value) -> f64 {
        match v {
            Value::Real(r) => *r,
            Value::Int(n) => *n as f64,
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("expected numeric value, got {other:?}"),
        }
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
        let mass = num(data.fields.get(&"mass".to_string()).expect("mass field"));
        let com = point3(data.fields.get(&"com".to_string()).expect("com field"));
        let inertia = crate::dynamics_psd::inertia_3x3_from_value(
            data.fields.get(&"inertia".to_string()).expect("inertia field"),
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
                data.fields.get(&f.to_string()),
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
}
