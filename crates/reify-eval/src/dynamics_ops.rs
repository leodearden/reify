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
//!   * `try_eval_body_mass_props` (added in step-8) — dispatch recognition for
//!     a `body_mass_props(...)` call cell, wiring the (currently unwired) KGQ
//!     kernel seam (task 3620) before delegating to the core.

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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DiagnosticCode, Severity};
    use reify_core::dimension::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use reify_stdlib::dynamics::mass_props::uniform_box_inertia;

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
}
