//! Pure `f64` mass-properties helpers for the `body_mass_props` stdlib fn
//! (RBD-β, task 3829; PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! Dependency-free `f64` math — no `reify_ir::Value`, no diagnostics sink, no
//! geometry kernel. Mirrors the modal split (`reify-stdlib/src/modal/
//! free_vibration.rs` holds pure scalar math; `reify-eval/src/dynamics_ops.rs`
//! owns the `Value`/diagnostic/kernel wiring that calls into this module).
//!
//! Two responsibilities:
//!   * [`resolve_density_strict`] — the shared explicit→material rung-walk,
//!     returning `None` when neither source is present (strict / no-water tail).
//!   * [`resolve_density`] — thin wrapper over `resolve_density_strict` that
//!     adds the `DEFAULT_DENSITY_KG_M3` water tail used by `body_mass_props`
//!     (explicit arg > body `Material` density > default water).
//!   * `uniform_box_inertia` (added in step-4) — the closed-form analytic
//!     ground-truth mass/com/inertia for a uniform-density box, the value the
//!     deferred KGQ kernel query (task 3620) will later be cross-checked against.

/// Default mass density (kg/m³) used when `body_mass_props` can resolve no
/// other density: the density of water at ~4 °C.
///
/// PRD `docs/prds/v0_3/rigid-body-dynamics.md` §5.4 specifies water (1000
/// kg/m³) as the bottom rung of the density ladder used by the internal
/// mechanism build pass (`derive_mechanism_mass_props`). The user-facing
/// `body_mass_props` no longer falls back to this value — a missing density
/// now emits `E_DynamicsNoDensity` (task 4498). This constant is kept for
/// the internal mechanism build pass, which operates on `Value::Map` bodies
/// that cannot carry a `Material`.
pub const DEFAULT_DENSITY_KG_M3: f64 = 1000.0;

/// Which rung of the [`resolve_density`] priority ladder supplied the density.
///
/// Returned alongside the resolved density so the eval layer can identify the
/// source. The pure layer itself stays diagnostic-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DensitySource {
    /// The caller passed an explicit `density` argument to `body_mass_props`.
    Explicit,
    /// No explicit argument; the body's `Material.density` was used.
    Material,
    /// Neither was available; the [`DEFAULT_DENSITY_KG_M3`] water default was
    /// used. This rung is now only consumed by the internal mechanism build pass
    /// (`derive_mechanism_mass_props`); the user-facing `body_mass_props` emits
    /// `E_DynamicsNoDensity` instead (task 4498).
    DefaultWater,
}

/// Walk the explicit→material priority ladder and return the winning rung, or
/// `None` if neither source is present (strict / no-water tail).
///
/// This is the **single canonical definition** of the density rung-walk.
/// Both `body_mass_props` (via [`resolve_density`]) and modal FEA (via
/// `extract_density_or_degenerate`) delegate here so the ladder is defined in
/// exactly one place; callers differ only in how they handle the `None` tail:
///
/// * [`resolve_density`] maps `None → (DEFAULT_DENSITY_KG_M3, DefaultWater)`.
/// * `modal_ops::extract_density_or_degenerate` maps `None → E_ModalNoMassMatrix`
///   (eigenfrequencies scale with √(1/ρ); a silent ρ=1000 would yield
///   plausible-but-wrong physics).
///
/// Like [`resolve_density`] this is pure `f64` selection — no validation of
/// the magnitude (a non-positive or `NaN` density is returned verbatim).
pub fn resolve_density_strict(
    explicit: Option<f64>,
    material: Option<f64>,
) -> Option<(f64, DensitySource)> {
    if let Some(rho) = explicit {
        Some((rho, DensitySource::Explicit))
    } else {
        material.map(|rho| (rho, DensitySource::Material))
    }
}

/// Resolve the mass density for `body_mass_props` via the fn-level priority
/// ladder (PRD §5.4): an explicit `density` argument wins; failing that, the
/// body's `Material` density; failing that, the [`DEFAULT_DENSITY_KG_M3`] water
/// default.
///
/// Returns the chosen density (kg/m³) paired with the [`DensitySource`] rung it
/// came from, so the caller knows whether a default-density warning is due.
/// This is pure `f64` selection — no validation of the magnitude (a non-positive
/// or `NaN` density is returned verbatim; physical validity of the resulting
/// inertia is enforced downstream by the existing MassProperties PSD hook).
///
/// Implemented as a thin wrapper over [`resolve_density_strict`] so the
/// explicit→material ladder is defined in exactly one place.
pub fn resolve_density(explicit: Option<f64>, material: Option<f64>) -> (f64, DensitySource) {
    resolve_density_strict(explicit, material)
        .unwrap_or((DEFAULT_DENSITY_KG_M3, DensitySource::DefaultWater))
}

/// Closed-form mass/center-of-mass/inertia of a uniform-density axis-aligned
/// box with edge lengths `dims = [a, b, c]` (metres) and the given `density`
/// (kg/m³), expressed in a **corner-origin** body frame (one corner at the
/// frame origin, edges along +x/+y/+z).
///
/// Returns `(mass, com, inertia)` where:
///   * `mass = ρ·a·b·c`,
///   * `com = [a/2, b/2, c/2]` (the box's geometric centre relative to the
///     corner origin),
///   * `inertia` is the 3×3 tensor **about the centre of mass** — a diagonal
///     matrix `m/12 · diag(b²+c², a²+c², a²+b²)` with zero products of inertia
///     (the principal axes of a box align with its edges).
///
/// This is the analytic ground truth referenced by the RBD PRD
/// (`docs/prds/v0_3/rigid-body-dynamics.md` §10 Phase 1 β): the value the
/// density-aware KGQ kernel query (task 3620 / KGQ-λ `moment_of_inertia`) will
/// later be cross-checked against once it is wired into `body_mass_props`. It
/// is `pub` so that future supervisor wiring and its cross-validation test can
/// reuse the exact same closed form.
#[allow(dead_code)] // G-allow: test-only analytic ground-truth closed form; KGQ wiring into body_mass_props landed via #3829 (done) + #4237 dynamics_ops seam (done); fn is permanent test-only helper, zero production callers by design
pub fn uniform_box_inertia(dims: [f64; 3], density: f64) -> (f64, [f64; 3], [[f64; 3]; 3]) {
    let [a, b, c] = dims;
    let mass = density * a * b * c;
    let com = [a / 2.0, b / 2.0, c / 2.0];
    let coeff = mass / 12.0;
    let inertia = [
        [coeff * (b * b + c * c), 0.0, 0.0],
        [0.0, coeff * (a * a + c * c), 0.0],
        [0.0, 0.0, coeff * (a * a + b * b)],
    ];
    (mass, com, inertia)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::dimension::DimensionVector;
    use reify_ir::{StructureInstanceData, Value};

    // ── MassProperties owner: the one Value shape (S15 η, task 8697) ─────────

    const ASYMMETRIC_INERTIA: [[f64; 3]; 3] = [
        [0.10, 0.01, 0.02],
        [0.03, 0.20, 0.04],
        [0.05, 0.06, 0.30],
    ];

    fn moi(x: f64) -> Value {
        Value::Scalar {
            si_value: x,
            dimension: DimensionVector::MOMENT_OF_INERTIA,
        }
    }

    fn instance_data(v: &Value) -> &StructureInstanceData {
        match v {
            Value::StructureInstance(d) => d,
            other => panic!("expected a StructureInstance, got {other:?}"),
        }
    }

    fn field<'a>(v: &'a Value, name: &str) -> &'a Value {
        instance_data(v)
            .fields
            .get(name)
            .unwrap_or_else(|| panic!("missing field {name} in {v:?}"))
    }

    fn sample() -> MassProperties {
        MassProperties {
            mass: 2.5,
            com: [0.1, -0.2, 0.3],
            inertia: ASYMMETRIC_INERTIA,
        }
    }

    #[test]
    fn to_value_emits_the_declared_shape() {
        let v = sample().to_value();
        let data = instance_data(&v);
        assert_eq!(data.type_name, "MassProperties");
        let mut keys: Vec<&str> = data.fields.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["com", "inertia", "mass", "origin"]);

        assert_eq!(
            field(&v, "mass"),
            &Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::MASS,
            }
        );
        assert_eq!(
            field(&v, "com"),
            &Value::Point(vec![
                Value::length(0.1),
                Value::length(-0.2),
                Value::length(0.3),
            ])
        );
        let Value::Matrix(rows) = field(&v, "inertia") else {
            panic!("inertia must be a Value::Matrix, got {:?}", field(&v, "inertia"))
        };
        assert_eq!(rows.len(), 3, "inertia must have 3 rows");
        for (r, row) in rows.iter().enumerate() {
            let want: Vec<Value> = ASYMMETRIC_INERTIA[r].iter().map(|&x| moi(x)).collect();
            assert_eq!(row, &want, "inertia row {r} must be MOMENT_OF_INERTIA scalars");
        }

        // origin: the default zero Frame3 — a position (LENGTH zeros) plus three
        // directions (dimensionless zeros, `Value::Real` per Invariant V). The
        // all-zero basis is the production default, not a placeholder to
        // "improve" into an identity basis.
        let origin = field(&v, "origin");
        assert_eq!(instance_data(origin).type_name, "Frame3");
        let components = |member: &str| -> Vec<Value> {
            match field(origin, member) {
                Value::Vector(c) if c.len() == 3 => c.clone(),
                other => panic!("Frame3.{member} must be a 3-component Vector, got {other:?}"),
            }
        };
        assert_eq!(components("origin"), vec![Value::length(0.0); 3]);
        for member in ["x_axis", "y_axis", "z_axis"] {
            for (i, c) in components(member).into_iter().enumerate() {
                assert_eq!(c, Value::Real(0.0), "Frame3.{member}[{i}]");
                assert!(c.dimension().is_dimensionless(), "Frame3.{member}[{i}]");
            }
        }
    }

    #[test]
    fn from_value_inverts_to_value() {
        let mp = sample();
        assert_eq!(MassProperties::from_value(&mp.to_value()), Some(mp));
    }

    #[test]
    fn from_value_accepts_the_legacy_spellings() {
        let mp = sample();
        let bare = crate::helpers::mint_registry_free_instance(
            "MassProperties",
            vec![
                ("mass".to_string(), Value::Real(mp.mass)),
                (
                    "com".to_string(),
                    Value::List(mp.com.iter().map(|&c| Value::length(c)).collect()),
                ),
                (
                    "inertia".to_string(),
                    Value::Matrix(
                        mp.inertia
                            .iter()
                            .map(|row| row.iter().map(|&x| Value::Real(x)).collect())
                            .collect(),
                    ),
                ),
            ],
        );
        assert_eq!(
            MassProperties::from_value(&bare),
            MassProperties::from_value(&mp.to_value()),
            "bare Real mass/inertia cells and a List com read like the dimensioned twin"
        );
        assert_eq!(MassProperties::from_value(&bare), Some(mp));
    }

    #[test]
    fn from_value_rejects_non_mass_properties() {
        assert_eq!(MassProperties::from_value(&Value::Real(1.0)), None);
        let other = crate::helpers::mint_registry_free_instance(
            "Block",
            vec![
                ("mass".to_string(), Value::Real(1.0)),
                ("com".to_string(), Value::List(vec![Value::Real(0.0); 3])),
                ("inertia".to_string(), Value::Matrix(vec![vec![Value::Real(0.0); 3]; 3])),
            ],
        );
        assert_eq!(MassProperties::from_value(&other), None);
    }

    #[test]
    fn unresolved_value_is_honest_undef() {
        let v = MassProperties::unresolved_value();
        assert_eq!(instance_data(&v).type_name, "MassProperties");
        for name in ["mass", "com", "inertia"] {
            assert_eq!(field(&v, name), &Value::Undef, "{name} must be the honest Undef");
        }
        assert_eq!(field(&v, "origin"), field(&sample().to_value(), "origin"));
        assert_eq!(MassProperties::from_value(&v), None);
    }

    #[test]
    fn read_inertia_reads_one_shape() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let cells = |f: fn(f64) -> Value| -> Vec<Vec<Value>> {
            identity
                .iter()
                .map(|row| row.iter().map(|&x| f(x)).collect())
                .collect()
        };
        let list_of = |rows: Vec<Vec<Value>>, wrap: fn(Vec<Value>) -> Value| {
            Value::List(rows.into_iter().map(wrap).collect())
        };

        let read = |v: &Value| MassProperties::read_inertia(v);
        assert_eq!(read(&Value::Matrix(cells(moi))), Ok(Some(identity)));
        assert_eq!(
            read(&list_of(cells(|x| Value::Int(x as i64)), Value::List)),
            Ok(Some(identity)),
            "List-of-List of Int"
        );
        assert_eq!(
            read(&list_of(cells(Value::Real), Value::List)),
            Ok(Some(identity)),
            "List-of-List of Real: the OCCT InertiaTensor reply shape"
        );
        assert_eq!(
            read(&list_of(cells(moi), Value::Vector)),
            Ok(Some(identity)),
            "List of Vector rows"
        );

        let mut two_by_three = cells(moi);
        two_by_three.pop();
        assert_eq!(read(&Value::Matrix(two_by_three)), Ok(None), "2×3 is shape-malformed");
        let mut one_undef = cells(moi);
        one_undef[1][2] = Value::Undef;
        assert_eq!(read(&Value::Matrix(one_undef)), Ok(None), "an Undef cell is unresolved");

        assert!(
            read(&Value::Matrix(cells(Value::length))).is_err(),
            "LENGTH cells are refused"
        );
        let mut one_bool = cells(moi);
        one_bool[0][0] = Value::Bool(true);
        assert!(read(&Value::Matrix(one_bool)).is_err(), "a Bool cell is refused");
    }

    // ── resolve_density priority ladder ──────────────────────────────────────

    #[test]
    fn explicit_density_wins_over_material() {
        // (a) explicit Some(2700) beats material Some(7850): the explicit
        // `density` arg is the highest fn-level ladder rung.
        let (rho, src) = resolve_density(Some(2700.0), Some(7850.0));
        assert_eq!(rho, 2700.0, "explicit density must be used verbatim");
        assert_eq!(src, DensitySource::Explicit);
    }

    #[test]
    fn material_density_used_when_no_explicit() {
        // (b) explicit None, material Some(7850) -> Material rung.
        let (rho, src) = resolve_density(None, Some(7850.0));
        assert_eq!(rho, 7850.0, "material density must be used when no explicit arg");
        assert_eq!(src, DensitySource::Material);
    }

    #[test]
    fn defaults_to_water_when_neither_present() {
        // (c) explicit None, material None -> default water 1000 kg/m³.
        let (rho, src) = resolve_density(None, None);
        assert_eq!(rho, 1000.0, "must fall back to the 1000 kg/m³ water default");
        assert_eq!(src, DensitySource::DefaultWater);
    }

    // ── resolve_density_strict — strict (None) tail ──────────────────────────

    #[test]
    fn strict_explicit_density_wins_over_material() {
        // (a) explicit Some(2700) beats material Some(7850): the explicit
        // `density` arg is the highest ladder rung.
        let result = resolve_density_strict(Some(2700.0), Some(7850.0));
        assert_eq!(
            result,
            Some((2700.0, DensitySource::Explicit)),
            "explicit rung must win verbatim"
        );
    }

    #[test]
    fn strict_material_density_used_when_no_explicit() {
        // (b) explicit None, material Some(7850) -> Material rung.
        let result = resolve_density_strict(None, Some(7850.0));
        assert_eq!(
            result,
            Some((7850.0, DensitySource::Material)),
            "material rung must be used when no explicit arg"
        );
    }

    #[test]
    fn strict_returns_none_when_neither_present() {
        // (c) explicit None, material None -> STRICT tail: no water fallback.
        let result = resolve_density_strict(None, None);
        assert_eq!(result, None, "strict tail must return None, not water");
    }

    #[test]
    fn strict_shared_rung_walk_invariant() {
        // (d) Invariant: for the explicit and material rungs,
        //     resolve_density_strict(e, m) == Some(resolve_density(e, m)).
        //     At the empty tail the two functions diverge by design:
        //     resolve_density(None,None) == (1000.0, DefaultWater)
        //     resolve_density_strict(None,None) == None.

        // explicit rung — both agree
        let strict_e = resolve_density_strict(Some(2700.0), Some(7850.0));
        let water_e = resolve_density(Some(2700.0), Some(7850.0));
        assert_eq!(
            strict_e,
            Some(water_e),
            "on the explicit rung strict and water wrappers must agree"
        );

        // material rung — both agree
        let strict_m = resolve_density_strict(None, Some(7850.0));
        let water_m = resolve_density(None, Some(7850.0));
        assert_eq!(
            strict_m,
            Some(water_m),
            "on the material rung strict and water wrappers must agree"
        );

        // empty tail — intentional divergence
        let strict_none = resolve_density_strict(None, None);
        let (water_rho, water_src) = resolve_density(None, None);
        assert_eq!(strict_none, None, "strict tail must be None");
        assert_eq!(
            water_rho, 1000.0,
            "water wrapper must fall back to 1000 kg/m³"
        );
        assert_eq!(
            water_src,
            DensitySource::DefaultWater,
            "water wrapper must report DefaultWater"
        );
    }

    // ── uniform_box_inertia analytic ground truth ────────────────────────────

    #[test]
    fn uniform_box_inertia_matches_hand_computed_values() {
        // Distinct extents so all three inertia diagonal entries differ.
        // a=0.1, b=0.2, c=0.3 m; ρ=1000 kg/m³.
        //
        // Hand-computed expected values (independent of the impl, so this pins
        // real numbers rather than impl==impl):
        //   mass = ρ·a·b·c = 1000·0.1·0.2·0.3 = 6.0 kg
        //   com  = [a/2, b/2, c/2] = [0.05, 0.10, 0.15] (corner-origin box)
        //   Ixx  = m/12·(b²+c²) = 6/12·(0.04+0.09) = 0.5·0.13 = 0.065
        //   Iyy  = m/12·(a²+c²) = 6/12·(0.01+0.09) = 0.5·0.10 = 0.05
        //   Izz  = m/12·(a²+b²) = 6/12·(0.01+0.04) = 0.5·0.05 = 0.025
        //   all products of inertia = 0
        let (mass, com, inertia) = uniform_box_inertia([0.1, 0.2, 0.3], 1000.0);

        assert!((mass - 6.0).abs() < 1e-12, "mass should be 6.0 kg, got {mass}");

        let expected_com = [0.05, 0.10, 0.15];
        for (i, (got, want)) in com.iter().zip(expected_com.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-12,
                "com[{i}] should be {want}, got {got}"
            );
        }

        let expected_diag = [0.065, 0.05, 0.025];
        for (r, row) in inertia.iter().enumerate() {
            for (c, &got) in row.iter().enumerate() {
                let want = if r == c { expected_diag[r] } else { 0.0 };
                assert!(
                    (got - want).abs() < 1e-12,
                    "inertia[{r}][{c}] should be {want}, got {got}"
                );
            }
        }
    }
}
