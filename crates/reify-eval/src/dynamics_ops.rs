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
