use std::fmt;
use std::ops::{Add, Neg, Sub};

use crate::ContentHash;

/// Rational number as i16/i16 for dimension exponents.
/// Uses i16 to prevent overflow when multiplying exponents (max i8 * i8 = 16,129 < i16::MAX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rational {
    num: i16,
    den: i16,
}

impl Rational {
    pub const ZERO: Rational = Rational::new(0, 1);
    pub const ONE: Rational = Rational::new(1, 1);

    pub const fn new(num: i16, den: i16) -> Self {
        assert!(den != 0, "denominator must not be zero");
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i16;
        let sign = if den < 0 { -1 } else { 1 };
        Rational {
            num: sign * num / g,
            den: sign * den / g,
        }
    }

    pub const fn num(&self) -> i16 {
        self.num
    }

    pub const fn den(&self) -> i16 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num() == 0
    }

    pub fn is_integer(self) -> bool {
        self.den() == 1 || self.num() % self.den() == 0
    }

    pub fn as_i8(self) -> Option<i8> {
        let val = if self.den() == 1 {
            self.num()
        } else if self.num() % self.den() == 0 {
            self.num() / self.den()
        } else {
            return None;
        };
        i8::try_from(val).ok()
    }
}

impl Add for Rational {
    type Output = Rational;
    fn add(self, rhs: Rational) -> Rational {
        Rational::new(
            self.num() * rhs.den() + rhs.num() * self.den(),
            self.den() * rhs.den(),
        )
    }
}

impl Sub for Rational {
    type Output = Rational;
    fn sub(self, rhs: Rational) -> Rational {
        Rational::new(
            self.num() * rhs.den() - rhs.num() * self.den(),
            self.den() * rhs.den(),
        )
    }
}

impl Neg for Rational {
    type Output = Rational;
    fn neg(self) -> Rational {
        Rational::new(-self.num(), self.den())
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den() == 1 {
            write!(f, "{}", self.num())
        } else {
            write!(f, "{}/{}", self.num(), self.den())
        }
    }
}

const fn gcd(mut a: u16, mut b: u16) -> u16 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Dimension vector: 10 rational exponents for the SI base dimensions plus Money.
///
/// Index mapping:
/// 0: Length (m)
/// 1: Mass (kg)
/// 2: Time (s)
/// 3: Electric current (A)
/// 4: Temperature (K)
/// 5: Amount of substance (mol)
/// 6: Luminous intensity (cd)
/// 7: Angle (rad, treated as dimension for engineering use)
/// 8: Solid angle (sr)
/// 9: Money (USD)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DimensionVector(pub [Rational; 10]);

impl DimensionVector {
    pub const DIMENSIONLESS: DimensionVector = DimensionVector([Rational::ZERO; 10]);
    pub const LENGTH: DimensionVector = DimensionVector::basis(0);
    pub const MASS: DimensionVector = DimensionVector::basis(1);
    pub const TIME: DimensionVector = DimensionVector::basis(2);
    pub const CURRENT: DimensionVector = DimensionVector::basis(3);
    pub const TEMPERATURE: DimensionVector = DimensionVector::basis(4);
    pub const AMOUNT_OF_SUBSTANCE: DimensionVector = DimensionVector::basis(5);
    pub const LUMINOUS_INTENSITY: DimensionVector = DimensionVector::basis(6);
    pub const ANGLE: DimensionVector = DimensionVector::basis(7);
    pub const SOLID_ANGLE: DimensionVector = DimensionVector::basis(8);
    pub const MONEY: DimensionVector = DimensionVector::basis(9);

    /// Common derived dimensions.
    pub const AREA: DimensionVector = DimensionVector::basis_n(0, 2);
    pub const VOLUME: DimensionVector = DimensionVector::basis_n(0, 3);

    // Derived SI dimensions — built at const-eval time via direct exponent arrays.
    //
    // Index layout reminder (see struct doc):
    //   0:Length  1:Mass  2:Time  3:Current  4:Temperature
    //   5:AmountOfSubstance  6:LuminousIntensity  7:Angle  8:SolidAngle  9:Money

    /// Frequency: s⁻¹
    pub const FREQUENCY: DimensionVector = DimensionVector::from_exps(&[(2, -1)]);
    /// Force: kg·m·s⁻² (same as module-scope `FORCE` — kept in parallel for ergonomics).
    pub const FORCE: DimensionVector = DimensionVector::from_exps(&[(0, 1), (1, 1), (2, -2)]);
    /// Energy: kg·m²·s⁻²
    pub const ENERGY: DimensionVector = DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2)]);
    /// Power: kg·m²·s⁻³
    pub const POWER: DimensionVector = DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3)]);
    /// Pressure: kg·m⁻¹·s⁻²
    pub const PRESSURE: DimensionVector = DimensionVector::from_exps(&[(0, -1), (1, 1), (2, -2)]);
    /// Voltage (electric potential): kg·m²·s⁻³·A⁻¹
    pub const VOLTAGE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3), (3, -1)]);
    /// Charge: s·A
    pub const CHARGE: DimensionVector = DimensionVector::from_exps(&[(2, 1), (3, 1)]);
    /// Capacitance: kg⁻¹·m⁻²·s⁴·A²
    pub const CAPACITANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (1, -1), (2, 4), (3, 2)]);
    /// Resistance: kg·m²·s⁻³·A⁻²
    pub const RESISTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3), (3, -2)]);
    /// Conductance: kg⁻¹·m⁻²·s³·A²
    pub const CONDUCTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (1, -1), (2, 3), (3, 2)]);
    /// Inductance: kg·m²·s⁻²·A⁻²
    pub const INDUCTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (3, -2)]);
    /// Magnetic flux: kg·m²·s⁻²·A⁻¹
    pub const MAGNETIC_FLUX: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (3, -1)]);
    /// Magnetic flux density: kg·s⁻²·A⁻¹
    pub const MAGNETIC_FLUX_DENSITY: DimensionVector =
        DimensionVector::from_exps(&[(1, 1), (2, -2), (3, -1)]);
    /// Luminous flux: cd·sr
    pub const LUMINOUS_FLUX: DimensionVector = DimensionVector::from_exps(&[(6, 1), (8, 1)]);
    /// Illuminance: cd·sr·m⁻²
    pub const ILLUMINANCE: DimensionVector = DimensionVector::from_exps(&[(0, -2), (6, 1), (8, 1)]);
    /// Absorbed dose of ionising radiation: m²·s⁻² (also Sievert for equivalent dose).
    pub const ABSORBED_DOSE: DimensionVector = DimensionVector::from_exps(&[(0, 2), (2, -2)]);
    /// Angular velocity: rad·s⁻¹
    pub const ANGULAR_VELOCITY: DimensionVector = DimensionVector::from_exps(&[(2, -1), (7, 1)]);
    /// Dynamic viscosity: kg·m⁻¹·s⁻¹
    pub const DYNAMIC_VISCOSITY: DimensionVector =
        DimensionVector::from_exps(&[(0, -1), (1, 1), (2, -1)]);
    /// Moment of inertia (mass-distribution): kg·m²
    ///
    /// Dimensionally distinct from `ENERGY` (kg·m²·s⁻²) — pin the s-slot
    /// distinction in tests to prevent silent collisions if either constant
    /// is edited.
    pub const MOMENT_OF_INERTIA: DimensionVector = DimensionVector::from_exps(&[(0, 2), (1, 1)]);
    /// Mass density: kg·m⁻³
    ///
    /// Named at the Rust level as `MASS_DENSITY` to disambiguate from the
    /// pre-existing `MAGNETIC_FLUX_DENSITY` constant (a magnetic-flux base
    /// quantity, not mass density). The user-facing canonical name is
    /// `"Density"` — the natural spelling at call sites like
    /// `density: Density`.
    pub const MASS_DENSITY: DimensionVector = DimensionVector::from_exps(&[(0, -3), (1, 1)]);
    /// Acceleration: m·s⁻² (LENGTH / TIME²)
    pub const ACCELERATION: DimensionVector = DimensionVector::from_exps(&[(0, 1), (2, -2)]);
    /// Force density (force per unit volume): N/m³ = kg·m⁻²·s⁻² (FORCE / VOLUME,
    /// equivalently PRESSURE / LENGTH)
    pub const FORCE_DENSITY: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (1, 1), (2, -2)]);

    // ─── Composite-quantity aliases for stdlib material/structural traits ──────
    //
    // Added by task #3115 to tighten 11 blocked-composite param sites in the
    // stdlib (materials_thermal, materials_optical, materials_electrical,
    // materials_mechanical, structural_physical) from `: Real` to dimensioned
    // scalar types. See `docs/notes/stdlib-real-placeholder-audit.md` task-E.

    /// Thermal conductivity: W/(m·K) = kg·m·s⁻³·K⁻¹
    pub const THERMAL_CONDUCTIVITY: DimensionVector =
        DimensionVector::from_exps(&[(0, 1), (1, 1), (2, -3), (4, -1)]);
    /// Specific heat capacity: J/(kg·K) = m²·s⁻²·K⁻¹
    pub const SPECIFIC_HEAT: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (2, -2), (4, -1)]);
    /// Coefficient of thermal expansion: 1/K
    pub const THERMAL_EXPANSION: DimensionVector = DimensionVector::from_exps(&[(4, -1)]);
    /// Electric resistivity: Ω·m = kg·m³·s⁻³·A⁻²
    ///
    /// Distinct from `RESISTANCE` (Ω = kg·m²·s⁻³·A⁻²) by the Length slot
    /// (3 vs 2). Pinned in `electric_resistivity_distinct_from_resistance`.
    pub const ELECTRIC_RESISTIVITY: DimensionVector =
        DimensionVector::from_exps(&[(0, 3), (1, 1), (2, -3), (3, -2)]);
    /// Electrical conductivity: S/m = kg⁻¹·m⁻³·s³·A²
    ///
    /// Distinct from `CONDUCTANCE` (S = kg⁻¹·m⁻²·s³·A²) by the Length slot
    /// (-3 vs -2). Pinned in `electrical_conductivity_distinct_from_conductance`.
    pub const ELECTRICAL_CONDUCTIVITY: DimensionVector =
        DimensionVector::from_exps(&[(0, -3), (1, -1), (2, 3), (3, 2)]);
    /// Dielectric strength: V/m = kg·m·s⁻³·A⁻¹
    pub const DIELECTRIC_STRENGTH: DimensionVector =
        DimensionVector::from_exps(&[(0, 1), (1, 1), (2, -3), (3, -1)]);
    /// Translational stiffness: N/m = kg·s⁻² (Length cancels)
    pub const STIFFNESS: DimensionVector = DimensionVector::from_exps(&[(1, 1), (2, -2)]);
    /// Absorption coefficient: 1/m
    pub const ABSORPTION_COEFF: DimensionVector = DimensionVector::from_exps(&[(0, -1)]);
    /// Curvature: 1/m — dimensionally identical to `ABSORPTION_COEFF`.
    ///
    /// Both are `1/Length`. There is no physically honest way to make them
    /// distinct DimensionVectors; the alias is justified because the
    /// source-syntax name `Curvature` carries different engineering intent
    /// (reciprocal-length-of-arc) than `AbsorptionCoeff` (per-distance decay).
    /// Per task 3603 / GHR-α design decision: `canonical_name()` continues to
    /// return `"AbsorptionCoeff"` for the shared dim because the `Curvature`
    /// entry is placed AFTER `AbsorptionCoeff` in `NAMED_DIMENSIONS` (first-
    /// match wins). The `Curvature` alias resolves correctly in the name→dim
    /// direction via `resolve_dimension_type` (used by `param k : Curvature`).
    pub const CURVATURE: DimensionVector = DimensionVector::from_exps(&[(0, -1)]);
    /// Fracture toughness: Pa·√m = kg·m^(-1/2)·s⁻²
    ///
    /// The only fractional-exponent named alias — Length slot is Rational(-1, 2).
    /// Built via the sibling `from_rational_exps` helper.
    pub const FRACTURE_TOUGHNESS: DimensionVector =
        DimensionVector::from_rational_exps(&[(0, -1, 2), (1, 1, 1), (2, -2, 1)]);

    // ─── Compliant-joint / flexure dimensioned types (task 3849 / Phase-1) ──────
    //
    // Added for the Compliant-Joints/Flexures PRD (v0.3, task α).
    // Index layout reminder: 0=Length 1=Mass 2=Time 7=Angle(rad).

    /// Rotational stiffness: N·m/rad = kg·m²·s⁻²·rad⁻¹
    ///
    /// Spring coefficient for revolute joints (spring_force = -k·Δθ gives N·m torque).
    pub const ROTATIONAL_STIFFNESS: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (7, -1)]);
    /// Rotational damping: N·m·s/rad = kg·m²·s⁻¹·rad⁻¹
    ///
    /// Damping coefficient for revolute joints (damping_force = -c·θ̇ gives N·m torque).
    pub const ROTATIONAL_DAMPING: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -1), (7, -1)]);
    /// Translational stiffness: N/m = kg·s⁻² — dimensionally identical to `STIFFNESS`.
    ///
    /// Spring coefficient for prismatic joints (spring_force = -k·Δx gives N force).
    /// Name alias: `canonical_name()` returns `"Stiffness"` (first-match scan order;
    /// see `STIFFNESS` above). The `"TranslationalStiffness"` name resolves in the
    /// name→dim direction via `resolve_dimension_type`. Mirrors the Curvature/AbsorptionCoeff
    /// alias precedent (task 3603 / GHR-α).
    pub const TRANSLATIONAL_STIFFNESS: DimensionVector = DimensionVector::STIFFNESS;
    /// Translational damping: N·s/m = kg·s⁻¹
    ///
    /// Damping coefficient for prismatic joints (damping_force = -c·ẋ gives N force).
    pub const TRANSLATIONAL_DAMPING: DimensionVector =
        DimensionVector::from_exps(&[(1, 1), (2, -1)]);

    const fn basis(index: usize) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        v[index] = Rational::ONE;
        DimensionVector(v)
    }

    const fn basis_n(index: usize, n: i16) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        v[index] = Rational::new(n, 1);
        DimensionVector(v)
    }

    /// Build a `DimensionVector` from `(index, integer_exponent)` pairs at
    /// const-eval time. Intended for concise declaration of derived-dimension
    /// constants (e.g. `ENERGY`, `VOLTAGE`).
    ///
    /// Sibling of [`from_rational_exps`]. The two share an identical
    /// index-bounds/iteration `while` skeleton, which is duplicated *by
    /// necessity*: the helpers consume slices of different tuple arity
    /// (`&[(usize, i16)]` vs `&[(usize, i16, i16)]`), and stable const fns
    /// cannot map one slice to the other nor iterate generically over two
    /// element shapes (no const closures/iterator adapters). Factoring the
    /// loop into one helper would require either unstable features or a macro
    /// that hides the integer-vs-rational distinction at the call site — the
    /// opposite of the ratified design decision keeping that distinction
    /// explicit. The duplicated scaffolding is 4 lines; the only real
    /// divergence is the per-entry `Rational::new(e, 1)` vs
    /// `Rational::new(num, den)`. Do not re-flag.
    const fn from_exps(entries: &[(usize, i16)]) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        let mut i = 0;
        while i < entries.len() {
            let (idx, e) = entries[i];
            v[idx] = Rational::new(e, 1);
            i += 1;
        }
        DimensionVector(v)
    }

    /// Sibling helper to [`from_exps`] for declaring constants with non-integer
    /// rational exponents (e.g. `FRACTURE_TOUGHNESS` with Length=Rational(-1, 2)).
    /// Each tuple is `(index, numerator, denominator)`.
    ///
    /// The two-helper split is intentional: the integer/rational distinction is
    /// explicit at the call site, and the 30+ existing `from_exps` callers stay
    /// untouched. The shared `while`-loop skeleton is duplicated by a stable
    /// const-fn limitation, not by oversight — see [`from_exps`] for the full
    /// rationale. Do not re-flag.
    const fn from_rational_exps(entries: &[(usize, i16, i16)]) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        let mut i = 0;
        while i < entries.len() {
            let (idx, num, den) = entries[i];
            v[idx] = Rational::new(num, den);
            i += 1;
        }
        DimensionVector(v)
    }

    /// Return the canonical user-facing name for this dimension, if it matches
    /// exactly one of the named singleton constants.
    ///
    /// Performs a linear scan over [`NAMED_DIMENSIONS`], the single source-of-truth
    /// table shared with `resolve_dimension_type` in
    /// `crates/reify-compiler/src/type_resolution.rs`.
    ///
    /// Returns `Some("Money")`, `Some("Force")`, etc. for each named singleton in the table.
    ///
    /// Returns `None` for:
    /// - `DIMENSIONLESS` — intentionally excluded from the table; callers handle it specially.
    /// - Composite/derived dimensions that are not in the named set (e.g. `MONEY/MASS`).
    pub fn canonical_name(&self) -> Option<&'static str> {
        NAMED_DIMENSIONS
            .iter()
            .find(|(dim, _)| *dim == *self)
            .map(|(_, name)| *name)
    }

    pub fn is_dimensionless(&self) -> bool {
        self.0.iter().all(|r| r.is_zero())
    }

    /// Multiply dimensions (add exponents).
    pub fn mul(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = self.0[i] + other.0[i];
        }
        DimensionVector(result)
    }

    /// Divide dimensions (subtract exponents).
    pub fn div(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = self.0[i] - other.0[i];
        }
        DimensionVector(result)
    }

    /// Take the nth root (divide all exponents by n).
    ///
    /// E.g., `AREA.root(2)` produces LENGTH (halves exponent from 2 to 1).
    /// Fractional exponents are representable via `Rational`.
    pub fn root(&self, n: i8) -> DimensionVector {
        assert!(n != 0, "root degree must not be zero");
        let n = n as i16;
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num(), self.0[i].den() * n);
        }
        DimensionVector(result)
    }

    /// Raise to an integer power (multiply all exponents).
    pub fn pow(&self, n: i8) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        let n = n as i16;
        let nr = Rational::new(n, 1);
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num() * nr.num(), self.0[i].den() * nr.den());
        }
        DimensionVector(result)
    }

    /// Convert an SI value to the standard engineering display unit for this dimension.
    ///
    /// Returns `(converted_value, unit_label)`. For example, LENGTH converts
    /// metres to millimetres: `to_display_units(0.08)` → `(80.0, "mm")`.
    pub fn to_display_units(&self, si_value: f64) -> (f64, &'static str) {
        if *self == DimensionVector::LENGTH {
            (si_value * 1000.0, "mm")
        } else if *self == DimensionVector::ANGLE {
            (si_value * 180.0 / std::f64::consts::PI, "deg")
        } else if *self == DimensionVector::AREA {
            (si_value * 1e6, "mm\u{00B2}")
        } else if *self == DimensionVector::VOLUME {
            (si_value * 1e9, "mm\u{00B3}")
        } else if *self == DimensionVector::MONEY {
            (si_value, "USD")
        } else if self.is_dimensionless() {
            (si_value, "")
        } else {
            (si_value, "SI")
        }
    }

    pub fn content_hash(&self) -> ContentHash {
        let mut buf = [0u8; 40]; // 10 * 4 bytes (2 bytes per i16 field)
        for (i, r) in self.0.iter().enumerate() {
            let num_bytes = r.num().to_le_bytes();
            let den_bytes = r.den().to_le_bytes();
            buf[i * 4] = num_bytes[0];
            buf[i * 4 + 1] = num_bytes[1];
            buf[i * 4 + 2] = den_bytes[0];
            buf[i * 4 + 3] = den_bytes[1];
        }
        ContentHash::of(&buf)
    }
}

/// FORCE dimension: kg·m·s⁻²
pub const FORCE: DimensionVector = {
    let mut v = [Rational::ZERO; 10];
    v[0] = Rational::ONE; // length
    v[1] = Rational::ONE; // mass
    v[2] = Rational::new(-2, 1); // time^-2
    DimensionVector(v)
};

/// Single source-of-truth mapping from named singleton `DimensionVector` constants to their
/// canonical PascalCase user-facing names.
///
/// This table drives both:
/// - [`DimensionVector::canonical_name`] — dimension → name direction (linear scan forward).
/// - `resolve_dimension_type` in `crates/reify-compiler/src/type_resolution.rs` — name → dimension
///   direction (linear scan backward).
///
/// **`DIMENSIONLESS` is intentionally excluded.** `canonical_name` returns `None` for
/// `DIMENSIONLESS` via the search-miss path (the existing contract), while `resolve_dimension_type`
/// special-cases `"Dimensionless" => DimensionVector::DIMENSIONLESS` as a separate fallback arm.
///
/// The slice contains 34 entries, one per named singleton, in the same order as the
/// original `canonical_name` match arms (LENGTH .. FORCE_DENSITY).
pub static NAMED_DIMENSIONS: &[(DimensionVector, &str)] = &[
    (DimensionVector::LENGTH, "Length"),
    (DimensionVector::MASS, "Mass"),
    (DimensionVector::TIME, "Time"),
    (DimensionVector::CURRENT, "Current"),
    (DimensionVector::TEMPERATURE, "Temperature"),
    (DimensionVector::AMOUNT_OF_SUBSTANCE, "AmountOfSubstance"),
    (DimensionVector::LUMINOUS_INTENSITY, "LuminousIntensity"),
    (DimensionVector::ANGLE, "Angle"),
    (DimensionVector::SOLID_ANGLE, "SolidAngle"),
    (DimensionVector::MONEY, "Money"),
    (DimensionVector::AREA, "Area"),
    (DimensionVector::VOLUME, "Volume"),
    (DimensionVector::FREQUENCY, "Frequency"),
    (DimensionVector::FORCE, "Force"),
    (DimensionVector::ENERGY, "Energy"),
    (DimensionVector::POWER, "Power"),
    (DimensionVector::PRESSURE, "Pressure"),
    (DimensionVector::VOLTAGE, "Voltage"),
    (DimensionVector::CHARGE, "Charge"),
    (DimensionVector::CAPACITANCE, "Capacitance"),
    (DimensionVector::RESISTANCE, "Resistance"),
    (DimensionVector::CONDUCTANCE, "Conductance"),
    (DimensionVector::INDUCTANCE, "Inductance"),
    (DimensionVector::MAGNETIC_FLUX, "MagneticFlux"),
    (
        DimensionVector::MAGNETIC_FLUX_DENSITY,
        "MagneticFluxDensity",
    ),
    (DimensionVector::LUMINOUS_FLUX, "LuminousFlux"),
    (DimensionVector::ILLUMINANCE, "Illuminance"),
    (DimensionVector::ABSORBED_DOSE, "AbsorbedDose"),
    (DimensionVector::ANGULAR_VELOCITY, "AngularVelocity"),
    (DimensionVector::DYNAMIC_VISCOSITY, "DynamicViscosity"),
    (DimensionVector::MOMENT_OF_INERTIA, "MomentOfInertia"),
    (DimensionVector::MASS_DENSITY, "Density"),
    (DimensionVector::ACCELERATION, "Acceleration"),
    (DimensionVector::FORCE_DENSITY, "ForceDensity"),
    // ── Composite-quantity aliases added by task #3115 (see task-E in the audit) ──
    (DimensionVector::THERMAL_CONDUCTIVITY, "ThermalConductivity"),
    (DimensionVector::SPECIFIC_HEAT, "SpecificHeat"),
    (DimensionVector::THERMAL_EXPANSION, "ThermalExpansion"),
    (DimensionVector::ELECTRIC_RESISTIVITY, "ElectricResistivity"),
    (
        DimensionVector::ELECTRICAL_CONDUCTIVITY,
        "ElectricalConductivity",
    ),
    (DimensionVector::DIELECTRIC_STRENGTH, "DielectricStrength"),
    (DimensionVector::STIFFNESS, "Stiffness"),
    // Task 3849 / flexure Phase-1: TranslationalStiffness is dimensionally identical
    // to STIFFNESS (N/m = kg·s⁻²). Placed AFTER "Stiffness" so first-match
    // canonical_name() continues to return "Stiffness" for the shared dim
    // (preserves materials golden behaviour). The name→dim direction
    // (resolve_dimension_type / resolve_type_name) finds this entry when source
    // syntax says `TranslationalStiffness`. Mirrors the Curvature/AbsorptionCoeff
    // alias precedent (task 3603 / GHR-α).
    (DimensionVector::TRANSLATIONAL_STIFFNESS, "TranslationalStiffness"),
    (DimensionVector::ROTATIONAL_STIFFNESS, "RotationalStiffness"),
    (DimensionVector::ROTATIONAL_DAMPING, "RotationalDamping"),
    (DimensionVector::TRANSLATIONAL_DAMPING, "TranslationalDamping"),
    (DimensionVector::ABSORPTION_COEFF, "AbsorptionCoeff"),
    // Task 3603 / GHR-α: `Curvature` is dimensionally identical to
    // `AbsorptionCoeff` (both `1/Length`). The entry is placed AFTER
    // `AbsorptionCoeff` so the first-match linear scan in `canonical_name`
    // continues to return `"AbsorptionCoeff"` for the shared dim (preserves
    // existing `materials_optical` golden behavior). The name→dim direction
    // (`resolve_dimension_type`) finds this entry directly when source syntax
    // says `Curvature`.
    (DimensionVector::CURVATURE, "Curvature"),
    (DimensionVector::FRACTURE_TOUGHNESS, "FractureToughness"),
];

impl fmt::Display for DimensionVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = ["m", "kg", "s", "A", "K", "mol", "cd", "rad", "sr", "USD"];
        // Emit positive-exponent slots first, then negative-exponent slots,
        // preserving index order within each group. This produces conventional
        // notation like "USD·kg^-1" rather than "kg^-1·USD".
        let mut numerator = Vec::new();
        let mut denominator = Vec::new();
        for (i, r) in self.0.iter().enumerate() {
            if !r.is_zero() {
                let part = if *r == Rational::ONE {
                    names[i].to_string()
                } else {
                    format!("{}^{}", names[i], r)
                };
                if r.num() > 0 {
                    numerator.push(part);
                } else {
                    denominator.push(part);
                }
            }
        }
        let parts: Vec<String> = numerator.into_iter().chain(denominator).collect();
        if parts.is_empty() {
            write!(f, "dimensionless")
        } else {
            write!(f, "{}", parts.join("·"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_getters() {
        let r = Rational::new(3, 4);
        assert_eq!(r.num(), 3);
        assert_eq!(r.den(), 4);
    }

    #[test]
    fn rational_const_new_normalization() {
        const R: Rational = Rational::new(6, 4);
        assert_eq!(R.num(), 3);
        assert_eq!(R.den(), 2);
    }

    #[test]
    fn rational_normalization_via_getters() {
        let r = Rational::new(2, 4);
        assert_eq!(r.num(), 1);
        assert_eq!(r.den(), 2);
    }

    #[test]
    fn rational_arithmetic() {
        assert_eq!(
            Rational::new(1, 2) + Rational::new(1, 3),
            Rational::new(5, 6)
        );
        assert_eq!(Rational::new(1, 1) - Rational::new(1, 1), Rational::ZERO);
        assert_eq!(-Rational::new(3, 4), Rational::new(-3, 4));
    }

    #[test]
    fn rational_normalization() {
        assert_eq!(Rational::new(2, 4), Rational::new(1, 2));
        assert_eq!(Rational::new(-2, -4), Rational::new(1, 2));
        assert_eq!(Rational::new(2, -4), Rational::new(-1, 2));
    }

    #[test]
    fn dimension_mul() {
        let area = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
        assert_eq!(area, DimensionVector::AREA);
    }

    #[test]
    fn dimension_div() {
        let velocity = DimensionVector::LENGTH.div(&DimensionVector({
            let mut v = [Rational::ZERO; 10];
            v[2] = Rational::ONE; // time
            v
        }));
        assert_eq!(velocity.0[0], Rational::ONE); // m
        assert_eq!(velocity.0[2], Rational::new(-1, 1)); // s^-1
    }

    #[test]
    fn dimension_pow() {
        let volume = DimensionVector::LENGTH.pow(3);
        assert_eq!(volume, DimensionVector::VOLUME);
    }

    #[test]
    fn dimensionless_check() {
        assert!(DimensionVector::DIMENSIONLESS.is_dimensionless());
        assert!(!DimensionVector::LENGTH.is_dimensionless());
    }

    #[test]
    fn force_dimension() {
        // F = m·a = kg·m·s⁻²
        let f = DimensionVector::MASS
            .mul(&DimensionVector::LENGTH)
            .mul(&DimensionVector::TIME.pow(-2));
        assert_eq!(f, FORCE);
    }

    #[test]
    fn all_new_derived_dimensions_have_correct_exponents() {
        // Helper: build a DimensionVector from (index, exponent) pairs.
        let make = |entries: &[(usize, i16)]| {
            let mut v = [Rational::ZERO; 10];
            for (i, e) in entries {
                v[*i] = Rational::new(*e, 1);
            }
            DimensionVector(v)
        };

        // Base-dimension constants that must exist.
        assert_eq!(DimensionVector::AMOUNT_OF_SUBSTANCE, make(&[(5, 1)]));
        assert_eq!(DimensionVector::LUMINOUS_INTENSITY, make(&[(6, 1)]));
        assert_eq!(DimensionVector::SOLID_ANGLE, make(&[(8, 1)]));

        // Derived-dimension constants.
        // Frequency: s⁻¹
        assert_eq!(DimensionVector::FREQUENCY, make(&[(2, -1)]));
        // Energy: kg·m²·s⁻²
        assert_eq!(DimensionVector::ENERGY, make(&[(0, 2), (1, 1), (2, -2)]));
        // Power: kg·m²·s⁻³
        assert_eq!(DimensionVector::POWER, make(&[(0, 2), (1, 1), (2, -3)]));
        // Pressure: kg·m⁻¹·s⁻²
        assert_eq!(DimensionVector::PRESSURE, make(&[(0, -1), (1, 1), (2, -2)]));
        // Voltage: kg·m²·s⁻³·A⁻¹
        assert_eq!(
            DimensionVector::VOLTAGE,
            make(&[(0, 2), (1, 1), (2, -3), (3, -1)])
        );
        // Charge: s·A
        assert_eq!(DimensionVector::CHARGE, make(&[(2, 1), (3, 1)]));
        // Capacitance: kg⁻¹·m⁻²·s⁴·A²
        assert_eq!(
            DimensionVector::CAPACITANCE,
            make(&[(0, -2), (1, -1), (2, 4), (3, 2)])
        );
        // Resistance: kg·m²·s⁻³·A⁻²
        assert_eq!(
            DimensionVector::RESISTANCE,
            make(&[(0, 2), (1, 1), (2, -3), (3, -2)])
        );
        // Conductance: kg⁻¹·m⁻²·s³·A²
        assert_eq!(
            DimensionVector::CONDUCTANCE,
            make(&[(0, -2), (1, -1), (2, 3), (3, 2)])
        );
        // Inductance: kg·m²·s⁻²·A⁻²
        assert_eq!(
            DimensionVector::INDUCTANCE,
            make(&[(0, 2), (1, 1), (2, -2), (3, -2)])
        );
        // Magnetic flux: kg·m²·s⁻²·A⁻¹
        assert_eq!(
            DimensionVector::MAGNETIC_FLUX,
            make(&[(0, 2), (1, 1), (2, -2), (3, -1)])
        );
        // Magnetic flux density: kg·s⁻²·A⁻¹
        assert_eq!(
            DimensionVector::MAGNETIC_FLUX_DENSITY,
            make(&[(1, 1), (2, -2), (3, -1)])
        );
        // Luminous flux: cd·sr
        assert_eq!(DimensionVector::LUMINOUS_FLUX, make(&[(6, 1), (8, 1)]));
        // Illuminance: cd·sr·m⁻²
        assert_eq!(
            DimensionVector::ILLUMINANCE,
            make(&[(0, -2), (6, 1), (8, 1)])
        );
        // Absorbed dose: m²·s⁻²
        assert_eq!(DimensionVector::ABSORBED_DOSE, make(&[(0, 2), (2, -2)]));
        // Angular velocity: rad·s⁻¹
        assert_eq!(DimensionVector::ANGULAR_VELOCITY, make(&[(2, -1), (7, 1)]));
        // Dynamic viscosity: kg·m⁻¹·s⁻¹
        assert_eq!(
            DimensionVector::DYNAMIC_VISCOSITY,
            make(&[(0, -1), (1, 1), (2, -1)])
        );
    }

    #[test]
    fn content_hash_determinism() {
        assert_eq!(
            DimensionVector::LENGTH.content_hash(),
            DimensionVector::LENGTH.content_hash()
        );
    }

    #[test]
    fn root_area_to_length() {
        // root(2) of AREA [2,0,...] → LENGTH [1,0,...]
        let result = DimensionVector::AREA.root(2);
        assert_eq!(result, DimensionVector::LENGTH);
    }

    #[test]
    fn root_length_4_to_length_2() {
        // root(2) of LENGTH^4 → LENGTH^2
        let len4 = DimensionVector::LENGTH.pow(4);
        let result = len4.root(2);
        assert_eq!(result, DimensionVector::AREA); // LENGTH^2 == AREA
    }

    #[test]
    fn root_length_to_fractional_exponent() {
        // root(2) of LENGTH → LENGTH^(1/2)
        let result = DimensionVector::LENGTH.root(2);
        assert_eq!(result.0[0], Rational::new(1, 2));
        // all other exponents should be zero
        for i in 1..10 {
            assert_eq!(result.0[i], Rational::ZERO);
        }
    }

    #[test]
    fn root_dimensionless_stays_dimensionless() {
        let result = DimensionVector::DIMENSIONLESS.root(2);
        assert_eq!(result, DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn pow_overflow_does_not_silently_wrap() {
        // LENGTH^64 raised to power 2 should give exponent 128.
        // With i8, 64*2=128 overflows i8::MAX (127).
        let len64 = DimensionVector::basis_n(0, 64);
        let result = len64.pow(2);
        assert_eq!(result.0[0], Rational::new(128, 1));
    }

    #[test]
    fn root_overflow_does_not_silently_wrap() {
        // Rational exponent {1, 64} with root(3) → {1, 192}.
        // With i8, den 64*3=192 overflows i8::MAX.
        let mut v = [Rational::ZERO; 10];
        v[0] = Rational::new(1, 64);
        let dv = DimensionVector(v);
        let result = dv.root(3);
        assert_eq!(result.0[0], Rational::new(1, 192));
    }

    #[test]
    fn rational_add_beyond_i8_range() {
        // 100 + 100 = 200, which overflows i8::MAX (127).
        let a = Rational::new(100, 1);
        let b = Rational::new(100, 1);
        assert_eq!(a + b, Rational::new(200, 1));
    }

    #[test]
    fn rational_neg_at_i8_boundary() {
        // Negating -128 in i8 overflows (no positive 128 in i8).
        // With i16, -(-128) = 128 works fine.
        let r = Rational::new(-128, 1);
        assert_eq!(-r, Rational::new(128, 1));
    }

    #[test]
    fn rational_sub_beyond_i8_range() {
        // 50 - (-100) = 150, but cross-multiplication in Sub produces
        // 50*1 - (-100)*1 = 150 which overflows i8::MAX.
        let a = Rational::new(50, 1);
        let b = Rational::new(-100, 1);
        assert_eq!(a - b, Rational::new(150, 1));
    }

    #[test]
    fn pow_large_exponent_round_trip() {
        // LENGTH^100 then root(100) should recover LENGTH.
        let powered = DimensionVector::LENGTH.pow(100);
        assert_eq!(powered.0[0], Rational::new(100, 1));
        let rooted = powered.root(100);
        assert_eq!(rooted, DimensionVector::LENGTH);
    }

    #[test]
    fn content_hash_determinism_with_wide_values() {
        // DimensionVector with exponents > 127 must hash deterministically.
        let dv = DimensionVector::basis_n(0, 200);
        let h1 = dv.content_hash();
        let h2 = dv.content_hash();
        assert_eq!(h1, h2);
        // Hash should differ from a different wide exponent.
        let dv2 = DimensionVector::basis_n(0, 201);
        assert_ne!(h1, dv2.content_hash());
    }

    #[test]
    fn rational_display_wide_values() {
        // Display formatting must show correct values beyond old i8 range.
        let r = Rational::new(200, 1);
        assert_eq!(format!("{}", r), "200");
        let r2 = Rational::new(300, 2);
        assert_eq!(format!("{}", r2), "150");
    }

    #[test]
    fn pressure_display_sorts_positive_before_negative_exponents() {
        // PRESSURE = kg·m⁻¹·s⁻² (index order: m=-1, kg=1, s=-2).
        // The Display impl emits positive-exponent slots first (kg), then
        // negative-exponent slots in index order (m^-1, s^-2).  This test
        // locks in that ordering for a pre-existing dimension so any future
        // revert to raw index order is caught immediately.
        assert_eq!(
            format!("{}", DimensionVector::PRESSURE),
            "kg\u{00B7}m^-1\u{00B7}s^-2"
        );
    }

    #[test]
    fn money_display_outputs_usd() {
        assert_eq!(format!("{}", DimensionVector::MONEY), "USD");
    }

    #[test]
    fn money_display_with_squared_exponent() {
        assert_eq!(format!("{}", DimensionVector::MONEY.pow(2)), "USD^2");
    }

    #[test]
    fn money_per_mass_display_renders_compositely() {
        // CostPerMass: MONEY / MASS → "USD·kg^-1"
        assert_eq!(
            format!("{}", DimensionVector::MONEY.div(&DimensionVector::MASS)),
            "USD\u{00B7}kg^-1"
        );
    }

    #[test]
    fn money_dimensionless_after_self_cancel_displays_dimensionless() {
        assert_eq!(
            format!("{}", DimensionVector::MONEY.div(&DimensionVector::MONEY)),
            "dimensionless"
        );
    }

    #[test]
    fn money_mul_with_mass_keeps_both_slots() {
        let result = DimensionVector::MONEY.mul(&DimensionVector::MASS);
        assert_eq!(result.0[9], Rational::ONE, "slot 9 (Money) should be ONE");
        assert_eq!(result.0[1], Rational::ONE, "slot 1 (Mass) should be ONE");
    }

    #[test]
    fn money_div_by_mass_produces_cost_per_mass() {
        // CostPerMass: MONEY/MASS → slot 9 = +1, slot 1 = -1, all others = 0
        let make = |entries: &[(usize, i16)]| {
            let mut v = [Rational::ZERO; 10];
            for (i, e) in entries {
                v[*i] = Rational::new(*e, 1);
            }
            DimensionVector(v)
        };
        assert_eq!(
            DimensionVector::MONEY.div(&DimensionVector::MASS),
            make(&[(1, -1), (9, 1)])
        );
    }

    #[test]
    fn money_pow_2_doubles_slot_9() {
        assert_eq!(DimensionVector::MONEY.pow(2).0[9], Rational::new(2, 1));
    }

    #[test]
    fn money_root_2_halves_slot_9() {
        assert_eq!(DimensionVector::MONEY.root(2).0[9], Rational::new(1, 2));
    }

    #[test]
    fn money_div_by_money_is_dimensionless() {
        assert!(
            DimensionVector::MONEY
                .div(&DimensionVector::MONEY)
                .is_dimensionless()
        );
    }

    #[test]
    fn money_does_not_leak_into_unrelated_arithmetic() {
        // Slot 9 must stay zero when Money is not involved.
        let result = DimensionVector::LENGTH.mul(&DimensionVector::MASS);
        assert_eq!(
            result.0[9],
            Rational::ZERO,
            "slot 9 should be zero for non-Money arithmetic"
        );
    }

    #[test]
    fn money_content_hash_is_deterministic() {
        assert_eq!(
            DimensionVector::MONEY.content_hash(),
            DimensionVector::MONEY.content_hash()
        );
    }

    #[test]
    fn money_content_hash_differs_from_other_base_dimensions() {
        let money_hash = DimensionVector::MONEY.content_hash();
        assert_ne!(money_hash, DimensionVector::LENGTH.content_hash());
        assert_ne!(money_hash, DimensionVector::MASS.content_hash());
        assert_ne!(money_hash, DimensionVector::TIME.content_hash());
        assert_ne!(money_hash, DimensionVector::CURRENT.content_hash());
        assert_ne!(money_hash, DimensionVector::TEMPERATURE.content_hash());
        assert_ne!(
            money_hash,
            DimensionVector::AMOUNT_OF_SUBSTANCE.content_hash()
        );
        assert_ne!(
            money_hash,
            DimensionVector::LUMINOUS_INTENSITY.content_hash()
        );
        assert_ne!(money_hash, DimensionVector::ANGLE.content_hash());
        assert_ne!(money_hash, DimensionVector::SOLID_ANGLE.content_hash());
        assert_ne!(money_hash, DimensionVector::DIMENSIONLESS.content_hash());
        assert_ne!(money_hash, DimensionVector::MONEY.pow(2).content_hash());
        assert_ne!(
            money_hash,
            DimensionVector::MONEY
                .div(&DimensionVector::MASS)
                .content_hash()
        );
    }

    // --- canonical_name tests (step-1) ---

    #[test]
    fn canonical_name_money_returns_money() {
        assert_eq!(DimensionVector::MONEY.canonical_name(), Some("Money"));
    }

    #[test]
    fn canonical_name_force_returns_force() {
        assert_eq!(DimensionVector::FORCE.canonical_name(), Some("Force"));
    }

    #[test]
    fn canonical_name_length_returns_length() {
        assert_eq!(DimensionVector::LENGTH.canonical_name(), Some("Length"));
    }

    #[test]
    fn canonical_name_mass_returns_mass() {
        assert_eq!(DimensionVector::MASS.canonical_name(), Some("Mass"));
    }

    #[test]
    fn canonical_name_composite_returns_none() {
        // MONEY / MASS is a composite dimension — no single canonical name.
        assert_eq!(
            DimensionVector::MONEY
                .div(&DimensionVector::MASS)
                .canonical_name(),
            None
        );
    }

    #[test]
    fn canonical_name_dimensionless_returns_none() {
        assert_eq!(DimensionVector::DIMENSIONLESS.canonical_name(), None);
    }

    /// Full coverage: every named singleton resolves via `canonical_name` to
    /// SOME registered name for its dim.
    ///
    /// The test derives its loop from [`super::NAMED_DIMENSIONS`] — the single source-of-truth
    /// table shared with `resolve_dimension_type`. Adding a new named dimension only requires
    /// updating `NAMED_DIMENSIONS`; this test and both consuming functions automatically stay in
    /// sync.
    ///
    /// Refactored for task 3603 / GHR-α to admit dim aliases (e.g. `Curvature`
    /// and `AbsorptionCoeff` both map to `1/Length`). The strict per-row
    /// `canonical_name == Some(expected_name)` assertion is replaced by
    /// "canonical_name returns some name registered for that dim"; the
    /// first-match scan-order property is verified separately by the
    /// `materials_optical` golden tests.
    #[test]
    fn canonical_name_covers_all_named_singletons() {
        for &(dim, _) in super::NAMED_DIMENSIONS {
            let canon = dim.canonical_name();
            assert!(
                canon.is_some(),
                "canonical_name returned None for registered dim {:?}",
                dim,
            );
            let canon_name = canon.unwrap();
            let canon_is_registered_for_dim = super::NAMED_DIMENSIONS
                .iter()
                .any(|(d, n)| *d == dim && *n == canon_name);
            assert!(
                canon_is_registered_for_dim,
                "canonical_name({:?}) returned {:?}, which is not registered for that dim",
                dim,
                canon_name,
            );
        }
        // DIMENSIONLESS is intentionally not named (self-explanatory to users).
        assert_eq!(DimensionVector::DIMENSIONLESS.canonical_name(), None);
    }

    #[test]
    fn content_hash_buffer_covers_slot_9() {
        // Verify that slot-9 bytes actually feed into the digest.
        let b7 = DimensionVector::basis_n(9, 7);
        let b8 = DimensionVector::basis_n(9, 8);
        let b0_7 = DimensionVector::basis_n(0, 7);
        // Different exponent in same slot → different hash
        assert_ne!(b7.content_hash(), b8.content_hash());
        // Same exponent in different slot → different hash
        assert_ne!(b7.content_hash(), b0_7.content_hash());
    }

    #[test]
    fn to_display_units_recognises_money() {
        let (value, unit) = DimensionVector::MONEY.to_display_units(25.0);
        assert_eq!(value, 25.0, "Money value should pass through unchanged");
        assert_eq!(
            unit, "USD",
            "Money unit label should be USD, not SI fallback"
        );
        assert_ne!(unit, "SI", "Money must NOT fall through to the SI fallback");
    }

    #[test]
    fn to_display_units_keeps_si_fallback_for_unknown_composed_dim() {
        // A composed Money/Length dimension is not the canonical MONEY singleton.
        // Guard: Money must NOT leak into a composite label (the real regression
        // risk). We do not pin the exact fallback string so future improvements
        // (e.g. assigning "USD/m" to this dimension) don't falsely fail.
        let cost_per_length = DimensionVector::MONEY.div(&DimensionVector::LENGTH);
        let (_, unit) = cost_per_length.to_display_units(1.0);
        assert_ne!(
            unit, "USD",
            "bare 'USD' label must not appear for a composed Money/Length dimension"
        );
    }

    #[test]
    fn moment_of_inertia_has_kg_m_squared_exponents() {
        let mi = DimensionVector::MOMENT_OF_INERTIA;
        assert_eq!(mi, DimensionVector::from_exps(&[(0, 2), (1, 1)]));
        assert_eq!(mi.canonical_name(), Some("MomentOfInertia"));
    }

    #[test]
    fn mass_density_has_kg_per_m_cubed_exponents() {
        let d = DimensionVector::MASS_DENSITY;
        assert_eq!(d, DimensionVector::from_exps(&[(0, -3), (1, 1)]));
        assert_eq!(d.canonical_name(), Some("Density"));
    }

    #[test]
    fn mass_density_is_distinct_from_magnetic_flux_density() {
        // MASS_DENSITY is kg·m⁻³ (mechanics); MAGNETIC_FLUX_DENSITY is kg·s⁻²·A⁻¹
        // (electromagnetism). They share the word "Density" colloquially but are
        // dimensionally unrelated; pin the distinction so future edits cannot
        // silently collapse them.
        assert_ne!(
            DimensionVector::MASS_DENSITY,
            DimensionVector::MAGNETIC_FLUX_DENSITY
        );
    }

    #[test]
    fn moment_of_inertia_is_distinct_from_energy() {
        // ENERGY is kg·m²·s⁻²; MOMENT_OF_INERTIA is kg·m². The s-slot distinction
        // is the whole reason MOMENT_OF_INERTIA needs its own constant.
        assert_ne!(DimensionVector::MOMENT_OF_INERTIA, DimensionVector::ENERGY);
    }

    #[test]
    fn money_constant_populates_slot_9() {
        // Slot 9 must be ONE; all other slots must be ZERO.
        assert_eq!(DimensionVector::MONEY.0[9], Rational::ONE);
        for i in 0..9 {
            assert_eq!(
                DimensionVector::MONEY.0[i],
                Rational::ZERO,
                "slot {} should be zero",
                i
            );
        }
        // MONEY is not DIMENSIONLESS and not SOLID_ANGLE.
        assert_ne!(DimensionVector::MONEY, DimensionVector::DIMENSIONLESS);
        assert_ne!(DimensionVector::MONEY, DimensionVector::SOLID_ANGLE);
    }

    /// Verify `NAMED_DIMENSIONS` is a complete, self-consistent table.
    ///
    /// (a) The table must be non-empty.
    /// (b) Every entry's `name` forward-resolves to its `dim` — i.e. scanning the
    ///     table by name finds the row's own dim.
    /// (c) For every entry, `dim.canonical_name()` returns SOME name that is
    ///     itself a registered entry's name for that dim. When multiple entries
    ///     share the same dim (e.g. `ABSORPTION_COEFF` and `CURVATURE` are both
    ///     `1/Length`), `canonical_name` returns the first one in scan order —
    ///     this test accepts ANY registered name for the dim, not the row's own
    ///     name, so dim-aliasing is permitted.
    /// (d) All entry names are unique (no two rows register the same name).
    /// (e) `DIMENSIONLESS.canonical_name()` is still `None` (intentionally excluded).
    ///
    /// Refactored for task 3603 / GHR-α: the original per-row
    /// `assert_eq!(dim.canonical_name(), Some(expected_name))` could not admit
    /// dimension aliases (Curvature and AbsorptionCoeff share `1/Length`). The
    /// new three-part check preserves the consistency contract while allowing
    /// aliases.
    #[test]
    fn named_dimensions_table_is_complete_and_consistent() {
        assert!(
            !super::NAMED_DIMENSIONS.is_empty(),
            "NAMED_DIMENSIONS must not be empty"
        );

        // (b) name → dim forward-resolution: each entry's name must locate its row.
        for &(dim, name) in super::NAMED_DIMENSIONS {
            let found = super::NAMED_DIMENSIONS
                .iter()
                .find(|(_, n)| *n == name)
                .map(|(d, _)| *d);
            assert_eq!(
                found,
                Some(dim),
                "forward name→dim lookup failed for {:?}: first match by name should return {:?}",
                name,
                dim,
            );
        }

        // (c) canonical_name(dim) returns SOME name that is registered for that dim.
        // Aliases are allowed: when two rows share a dim, the first scan-match wins.
        for &(dim, _) in super::NAMED_DIMENSIONS {
            let canon = dim.canonical_name();
            assert!(
                canon.is_some(),
                "canonical_name returned None for registered dim {:?}",
                dim,
            );
            let canon_name = canon.unwrap();
            let canon_is_registered_for_dim = super::NAMED_DIMENSIONS
                .iter()
                .any(|(d, n)| *d == dim && *n == canon_name);
            assert!(
                canon_is_registered_for_dim,
                "canonical_name({:?}) returned {:?}, which is not a registered name for that dim",
                dim,
                canon_name,
            );
        }

        // (d) All entry names are unique.
        let mut names: Vec<&'static str> =
            super::NAMED_DIMENSIONS.iter().map(|(_, n)| *n).collect();
        names.sort();
        let total = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            total,
            "NAMED_DIMENSIONS contains duplicate names — every entry must have a unique name",
        );

        // (e) DIMENSIONLESS is excluded.
        assert_eq!(
            DimensionVector::DIMENSIONLESS.canonical_name(),
            None,
            "DIMENSIONLESS must remain absent from NAMED_DIMENSIONS (canonical_name returns None)"
        );
    }

    /// Task 3603 / GHR-α: the `Curvature` named dimension must be registered
    /// in `NAMED_DIMENSIONS` mapping to `DimensionVector::CURVATURE`. The
    /// entry is required so that `resolve_dimension_type` (and any other
    /// name→dim scanner) can resolve `param k : Curvature` source syntax.
    #[test]
    fn curvature_name_is_registered_in_named_dimensions() {
        let found = super::NAMED_DIMENSIONS
            .iter()
            .any(|(dim, name)| *name == "Curvature" && *dim == DimensionVector::CURVATURE);
        assert!(
            found,
            "NAMED_DIMENSIONS must contain (DimensionVector::CURVATURE, \"Curvature\")"
        );
    }

    /// Mirrors the `resolve_dimension_type` name→dim direction: scanning the
    /// table by name `"Curvature"` must return `DimensionVector::CURVATURE`.
    /// Documents the direction-of-use distinction from `canonical_name()`
    /// (dim→name); see GHR-α design decision for why both must work.
    #[test]
    fn named_dimensions_curvature_resolves_via_forward_lookup() {
        let resolved = super::NAMED_DIMENSIONS
            .iter()
            .find(|(_, name)| *name == "Curvature")
            .map(|(dim, _)| *dim);
        assert_eq!(
            resolved,
            Some(DimensionVector::CURVATURE),
            "forward lookup name→dim for \"Curvature\" should resolve to CURVATURE"
        );
    }

    // ─── Money-compound and Torque-vs-Energy regression guards ───────────────

    /// `MONEY × FORCE` must pin every slot to its expected exponent:
    /// slot 0 (Length) = +1, slot 1 (Mass) = +1, slot 2 (Time) = −2,
    /// slot 9 (Money) = +1, and all remaining slots
    /// (Current, Temperature, Substance, Luminosity, Angle, SolidAngle) = 0.
    ///
    /// Asserting every slot — not only Angle slot 7 — ensures that any
    /// 10-slot exponent-buffer bug that bled into ANY adjacent slot would be
    /// caught, not only a bleed into Angle.
    #[test]
    fn money_compound_with_force_pins_all_slots() {
        let result = DimensionVector::MONEY.mul(&DimensionVector::FORCE);
        assert_eq!(
            result.0[0],
            Rational::ONE,
            "slot 0 (Length) should be ONE for MONEY × FORCE"
        );
        assert_eq!(
            result.0[1],
            Rational::ONE,
            "slot 1 (Mass) should be ONE for MONEY × FORCE"
        );
        assert_eq!(
            result.0[2],
            Rational::new(-2, 1),
            "slot 2 (Time) should be -2 for MONEY × FORCE"
        );
        assert_eq!(
            result.0[9],
            Rational::ONE,
            "slot 9 (Money) should be ONE for MONEY × FORCE"
        );
        for i in [3usize, 4, 5, 6, 7, 8] {
            assert_eq!(
                result.0[i],
                Rational::ZERO,
                "slot {} should be ZERO for MONEY × FORCE",
                i
            );
        }
    }

    /// Torque (Force·Length/Angle = kg·m²·s⁻²·rad⁻¹) and Energy (Force·Length =
    /// kg·m²·s⁻²) must be distinct dimensions.  The only difference is slot 7
    /// (Angle): torque has −1, energy has 0.
    ///
    /// This is the core regression guard: a 10-slot exponent vector that silently
    /// conflates Angle with another slot would make these two dimensions equal,
    /// breaking all engineering models that distinguish "rotational energy" from
    /// "translational energy".
    #[test]
    fn torque_dim_differs_from_energy_dim_via_angle_slot() {
        let torque = DimensionVector::FORCE
            .mul(&DimensionVector::LENGTH)
            .div(&DimensionVector::ANGLE);
        let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);

        assert_ne!(
            torque, energy,
            "Torque and Energy must be distinct dimensions"
        );
        assert_eq!(
            torque.0[7],
            Rational::new(-1, 1),
            "Torque slot 7 (Angle) should be -1"
        );
        assert_eq!(
            energy.0[7],
            Rational::ZERO,
            "Energy slot 7 (Angle) should be ZERO"
        );
    }

    /// Multiplying both Torque and Energy by MONEY produces `Money·Torque` and
    /// `Money·Energy`. Each compound dimension must:
    ///   (a) carry slot 9 (Money) = +1, and
    ///   (b) retain its original Angle-slot exponent (−1 vs 0 respectively),
    ///       so the two compound dimensions remain distinct.
    ///
    /// Guards against any future slot-propagation bug that might collapse the
    /// Angle-slot distinction once a Money factor is mixed in.
    #[test]
    fn torque_with_money_factor_remains_distinct_from_energy_with_money_factor() {
        let torque = DimensionVector::FORCE
            .mul(&DimensionVector::LENGTH)
            .div(&DimensionVector::ANGLE);
        let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);

        let cost_per_torque = DimensionVector::MONEY.mul(&torque);
        let cost_per_energy = DimensionVector::MONEY.mul(&energy);

        assert_ne!(
            cost_per_torque, cost_per_energy,
            "Money·Torque and Money·Energy must remain distinct"
        );
        assert_eq!(
            cost_per_torque.0[9],
            Rational::ONE,
            "Money·Torque slot 9 should be ONE"
        );
        assert_eq!(
            cost_per_energy.0[9],
            Rational::ONE,
            "Money·Energy slot 9 should be ONE"
        );
        assert_eq!(
            cost_per_torque.0[7],
            Rational::new(-1, 1),
            "Money·Torque slot 7 (Angle) should be -1"
        );
        assert_eq!(
            cost_per_energy.0[7],
            Rational::ZERO,
            "Money·Energy slot 7 (Angle) should be ZERO"
        );
    }

    /// The content hashes of Torque and Energy must differ so that any future
    /// change to the hash-buffer layout cannot silently conflate the two dimensions.
    ///
    /// Complements `torque_dim_differs_from_energy_dim_via_angle_slot` at the
    /// hash layer: if the content_hash incorrectly encodes slot 7, then dimension
    /// type-checking built on hashes would mis-identify torque as energy even
    /// though `DimensionVector::eq` would still be correct.
    #[test]
    fn torque_dim_content_hash_differs_from_energy_dim_content_hash() {
        let torque = DimensionVector::FORCE
            .mul(&DimensionVector::LENGTH)
            .div(&DimensionVector::ANGLE);
        let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);
        assert_ne!(
            torque.content_hash(),
            energy.content_hash(),
            "Torque and Energy content hashes must differ"
        );
    }

    #[test]
    fn acceleration_canonical_name_is_acceleration() {
        let a = DimensionVector::ACCELERATION;
        assert_eq!(a.canonical_name(), Some("Acceleration"));
    }

    #[test]
    fn acceleration_equals_length_div_time_squared() {
        assert_eq!(
            DimensionVector::ACCELERATION,
            DimensionVector::LENGTH.div(&DimensionVector::TIME.pow(2))
        );
    }

    #[test]
    fn acceleration_is_distinct_from_length() {
        assert_ne!(DimensionVector::ACCELERATION, DimensionVector::LENGTH);
    }

    #[test]
    fn force_density_canonical_name_is_force_density() {
        let fd = DimensionVector::FORCE_DENSITY;
        assert_eq!(fd.canonical_name(), Some("ForceDensity"));
    }

    #[test]
    fn force_density_equals_force_div_volume() {
        assert_eq!(
            DimensionVector::FORCE_DENSITY,
            DimensionVector::FORCE.div(&DimensionVector::VOLUME)
        );
    }

    #[test]
    fn force_density_is_distinct_from_pressure() {
        // PRESSURE has slot 0 = -1; FORCE_DENSITY has slot 0 = -2.
        // They share the same mass and time exponents; pin the length-slot
        // distinction so future edits cannot silently collapse the two.
        assert_ne!(DimensionVector::FORCE_DENSITY, DimensionVector::PRESSURE);
    }

    /// `from_rational_exps` is the sibling helper to `from_exps` for declaring
    /// `DimensionVector` constants whose exponents are non-integer rationals
    /// (e.g. `FRACTURE_TOUGHNESS` with Length=Rational(-1, 2)).
    ///
    /// Test asserts the helper is usable at const-eval time (the call site that
    /// matters is inside `pub const FRACTURE_TOUGHNESS: DimensionVector = …`),
    /// and that the slot vector contains the precise (num, den) rationals at
    /// the indexed positions while every other slot is `Rational::ZERO`.
    #[test]
    fn from_rational_exps_builds_fractional_exponent_vector() {
        const V: DimensionVector =
            DimensionVector::from_rational_exps(&[(0, -1, 2), (1, 1, 1), (2, -2, 1)]);
        assert_eq!(V.0[0], Rational::new(-1, 2));
        assert_eq!(V.0[1], Rational::ONE);
        assert_eq!(V.0[2], Rational::new(-2, 1));
        for i in 3..10 {
            assert_eq!(V.0[i], Rational::ZERO, "slot {} should be zero", i);
        }
    }

    // ─── Step-3: per-alias unit tests for the 9 new named-dimension aliases ───
    //
    // Each test (a) verifies the exponent vector by *composing* the alias from
    // already-established named/base SI dimensions (POWER, ENERGY, RESISTANCE,
    // …) via the mul/div/pow/root algebra — NOT by re-stating the literal
    // `from_exps(&[…])` used in the constant's own definition, so the assertion
    // independently checks the physics rather than acting as a pure
    // change-detector; and (b) pins the canonical PascalCase name.
    // Sibling-distinctness regression-guards pin the slots that distinguish
    // look-alike pairs (Resistivity vs Resistance, Conductivity vs
    // Conductance) and the only fractional case (FractureToughness Length slot).

    #[test]
    fn thermal_conductivity_dimension_exponents() {
        // W/(m·K) = POWER / (LENGTH · TEMPERATURE)
        let expected =
            DimensionVector::POWER.div(&DimensionVector::LENGTH.mul(&DimensionVector::TEMPERATURE));
        assert_eq!(DimensionVector::THERMAL_CONDUCTIVITY, expected);
        assert_eq!(
            DimensionVector::THERMAL_CONDUCTIVITY.canonical_name(),
            Some("ThermalConductivity")
        );
    }

    #[test]
    fn specific_heat_dimension_exponents() {
        // J/(kg·K) = ENERGY / (MASS · TEMPERATURE)
        let expected =
            DimensionVector::ENERGY.div(&DimensionVector::MASS.mul(&DimensionVector::TEMPERATURE));
        assert_eq!(DimensionVector::SPECIFIC_HEAT, expected);
        assert_eq!(
            DimensionVector::SPECIFIC_HEAT.canonical_name(),
            Some("SpecificHeat")
        );
    }

    #[test]
    fn thermal_expansion_dimension_exponents() {
        // 1/K = reciprocal of TEMPERATURE
        let expected = DimensionVector::TEMPERATURE.pow(-1);
        assert_eq!(DimensionVector::THERMAL_EXPANSION, expected);
        assert_eq!(
            DimensionVector::THERMAL_EXPANSION.canonical_name(),
            Some("ThermalExpansion")
        );
    }

    #[test]
    fn electric_resistivity_dimension_exponents() {
        // Ω·m = RESISTANCE · LENGTH
        let expected = DimensionVector::RESISTANCE.mul(&DimensionVector::LENGTH);
        assert_eq!(DimensionVector::ELECTRIC_RESISTIVITY, expected);
        assert_eq!(
            DimensionVector::ELECTRIC_RESISTIVITY.canonical_name(),
            Some("ElectricResistivity")
        );
    }

    #[test]
    fn electrical_conductivity_dimension_exponents() {
        // S/m = CONDUCTANCE / LENGTH
        let expected = DimensionVector::CONDUCTANCE.div(&DimensionVector::LENGTH);
        assert_eq!(DimensionVector::ELECTRICAL_CONDUCTIVITY, expected);
        assert_eq!(
            DimensionVector::ELECTRICAL_CONDUCTIVITY.canonical_name(),
            Some("ElectricalConductivity")
        );
    }

    #[test]
    fn dielectric_strength_dimension_exponents() {
        // V/m = VOLTAGE / LENGTH
        let expected = DimensionVector::VOLTAGE.div(&DimensionVector::LENGTH);
        assert_eq!(DimensionVector::DIELECTRIC_STRENGTH, expected);
        assert_eq!(
            DimensionVector::DIELECTRIC_STRENGTH.canonical_name(),
            Some("DielectricStrength")
        );
    }

    #[test]
    fn stiffness_dimension_exponents() {
        // N/m = FORCE / LENGTH (Length cancels → kg·s⁻²)
        let expected = DimensionVector::FORCE.div(&DimensionVector::LENGTH);
        assert_eq!(DimensionVector::STIFFNESS, expected);
        assert_eq!(
            DimensionVector::STIFFNESS.canonical_name(),
            Some("Stiffness")
        );
    }

    #[test]
    fn absorption_coeff_dimension_exponents() {
        // 1/m = reciprocal of LENGTH
        let expected = DimensionVector::LENGTH.pow(-1);
        assert_eq!(DimensionVector::ABSORPTION_COEFF, expected);
        assert_eq!(
            DimensionVector::ABSORPTION_COEFF.canonical_name(),
            Some("AbsorptionCoeff")
        );
    }

    #[test]
    fn fracture_toughness_dimension_exponents() {
        // Pa·√m = PRESSURE · √LENGTH — the only fractional-exponent alias;
        // derived via LENGTH.root(2) so the test exercises the rational path
        // independently of the constant's own `from_rational_exps` literal.
        let expected = DimensionVector::PRESSURE.mul(&DimensionVector::LENGTH.root(2));
        assert_eq!(DimensionVector::FRACTURE_TOUGHNESS, expected);
        assert_eq!(
            DimensionVector::FRACTURE_TOUGHNESS.canonical_name(),
            Some("FractureToughness")
        );
    }

    /// ELECTRIC_RESISTIVITY (Ω·m) and RESISTANCE (Ω) differ only in the
    /// Length slot (3 vs 2). Pin the distinction so future edits cannot
    /// silently collapse them.
    #[test]
    fn electric_resistivity_distinct_from_resistance() {
        assert_ne!(
            DimensionVector::ELECTRIC_RESISTIVITY,
            DimensionVector::RESISTANCE
        );
        assert_eq!(
            DimensionVector::ELECTRIC_RESISTIVITY.0[0],
            Rational::new(3, 1),
            "ElectricResistivity Length slot should be 3 (Ω·m)"
        );
        assert_eq!(
            DimensionVector::RESISTANCE.0[0],
            Rational::new(2, 1),
            "Resistance Length slot should be 2 (Ω)"
        );
    }

    /// ELECTRICAL_CONDUCTIVITY (S/m) and CONDUCTANCE (S) differ only in the
    /// Length slot (-3 vs -2). Pin the distinction.
    #[test]
    fn electrical_conductivity_distinct_from_conductance() {
        assert_ne!(
            DimensionVector::ELECTRICAL_CONDUCTIVITY,
            DimensionVector::CONDUCTANCE
        );
        assert_eq!(
            DimensionVector::ELECTRICAL_CONDUCTIVITY.0[0],
            Rational::new(-3, 1),
            "ElectricalConductivity Length slot should be -3 (S/m)"
        );
        assert_eq!(
            DimensionVector::CONDUCTANCE.0[0],
            Rational::new(-2, 1),
            "Conductance Length slot should be -2 (S)"
        );
    }

    /// FRACTURE_TOUGHNESS is the only alias with a fractional Length exponent.
    /// Pin the (-1, 2) value so any silent integer-collapse is caught.
    #[test]
    fn fracture_toughness_has_fractional_length_exponent() {
        assert_eq!(
            DimensionVector::FRACTURE_TOUGHNESS.0[0],
            Rational::new(-1, 2),
            "FractureToughness Length slot should be Rational(-1, 2)"
        );
    }

    /// CURVATURE is 1/Length — dimensionally identical to ABSORPTION_COEFF.
    /// Pin the Length-slot exponent to Rational(-1, 1) and confirm all other
    /// slots are Rational::ZERO. Added for task 3603 / GHR-α (PRD §8 Phase 1)
    /// so the stdlib geometry-query registration `curvature → Scalar<Curvature>`
    /// has a well-defined dimensional alias.
    #[test]
    fn curvature_constant_is_length_inverse() {
        // 1/m = reciprocal of LENGTH — matches ABSORPTION_COEFF by construction.
        assert_eq!(
            DimensionVector::CURVATURE,
            DimensionVector::from_exps(&[(0, -1)])
        );
        // Length slot is exactly Rational(-1, 1).
        assert_eq!(
            DimensionVector::CURVATURE.0[0],
            Rational::new(-1, 1),
            "Curvature Length slot should be Rational(-1, 1)"
        );
        // All other slots are Rational::ZERO.
        for i in 1..10 {
            assert_eq!(
                DimensionVector::CURVATURE.0[i],
                Rational::ZERO,
                "Curvature slot {} should be Rational::ZERO",
                i
            );
        }
    }

    // ─── Step-1 (task 3849): flexure dimensioned-type const existence + exponents ──
    //
    // Four new pub const DimensionVectors for compliant-joint/flexure types (task α,
    // Phase-1 of docs/prds/v0_3/compliant-joints-flexures.md):
    //
    //   ROTATIONAL_STIFFNESS  = N·m/rad   = kg·m²·s⁻²·rad⁻¹  (index: 0=+2,1=+1,2=-2,7=-1)
    //   ROTATIONAL_DAMPING    = N·m·s/rad = kg·m²·s⁻¹·rad⁻¹  (index: 0=+2,1=+1,2=-1,7=-1)
    //   TRANSLATIONAL_STIFFNESS = N/m     = kg·s⁻²             (index: 1=+1,2=-2; == STIFFNESS)
    //   TRANSLATIONAL_DAMPING = N·s/m     = kg·s⁻¹             (index: 1=+1,2=-1)

    #[test]
    fn rotational_stiffness_has_correct_exponents() {
        // N·m/rad = kg·m²·s⁻²·rad⁻¹
        assert_eq!(
            DimensionVector::ROTATIONAL_STIFFNESS,
            DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (7, -1)])
        );
    }

    #[test]
    fn rotational_damping_has_correct_exponents() {
        // N·m·s/rad = kg·m²·s⁻¹·rad⁻¹
        assert_eq!(
            DimensionVector::ROTATIONAL_DAMPING,
            DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -1), (7, -1)])
        );
    }

    #[test]
    fn translational_stiffness_equals_stiffness_alias() {
        // N/m = kg·s⁻² — identical to the existing STIFFNESS constant.
        assert_eq!(
            DimensionVector::TRANSLATIONAL_STIFFNESS,
            DimensionVector::from_exps(&[(1, 1), (2, -2)])
        );
        assert_eq!(
            DimensionVector::TRANSLATIONAL_STIFFNESS,
            DimensionVector::STIFFNESS,
            "TRANSLATIONAL_STIFFNESS must equal STIFFNESS (same N/m physics)"
        );
    }

    #[test]
    fn translational_damping_has_correct_exponents() {
        // N·s/m = kg·s⁻¹
        assert_eq!(
            DimensionVector::TRANSLATIONAL_DAMPING,
            DimensionVector::from_exps(&[(1, 1), (2, -1)])
        );
    }

    #[test]
    fn flexure_dims_are_mutually_distinct_except_documented_alias() {
        // ROTATIONAL_STIFFNESS ≠ ROTATIONAL_DAMPING (time exponent differs: -2 vs -1)
        assert_ne!(
            DimensionVector::ROTATIONAL_STIFFNESS,
            DimensionVector::ROTATIONAL_DAMPING
        );
        // ROTATIONAL_STIFFNESS ≠ TRANSLATIONAL_STIFFNESS (length and angle slots differ)
        assert_ne!(
            DimensionVector::ROTATIONAL_STIFFNESS,
            DimensionVector::TRANSLATIONAL_STIFFNESS
        );
        // ROTATIONAL_STIFFNESS ≠ TRANSLATIONAL_DAMPING
        assert_ne!(
            DimensionVector::ROTATIONAL_STIFFNESS,
            DimensionVector::TRANSLATIONAL_DAMPING
        );
        // ROTATIONAL_DAMPING ≠ TRANSLATIONAL_STIFFNESS
        assert_ne!(
            DimensionVector::ROTATIONAL_DAMPING,
            DimensionVector::TRANSLATIONAL_STIFFNESS
        );
        // ROTATIONAL_DAMPING ≠ TRANSLATIONAL_DAMPING
        assert_ne!(
            DimensionVector::ROTATIONAL_DAMPING,
            DimensionVector::TRANSLATIONAL_DAMPING
        );
        // TRANSLATIONAL_STIFFNESS == STIFFNESS (documented alias — NOT an error)
        assert_eq!(
            DimensionVector::TRANSLATIONAL_STIFFNESS,
            DimensionVector::STIFFNESS
        );
        // TRANSLATIONAL_DAMPING ≠ TRANSLATIONAL_STIFFNESS
        assert_ne!(
            DimensionVector::TRANSLATIONAL_DAMPING,
            DimensionVector::TRANSLATIONAL_STIFFNESS
        );
    }

    #[test]
    fn flexure_dims_distinct_from_energy_and_dynamic_viscosity() {
        // ROTATIONAL_STIFFNESS (kg·m²·s⁻²·rad⁻¹) ≠ ENERGY (kg·m²·s⁻²) — differs by angle slot.
        assert_ne!(DimensionVector::ROTATIONAL_STIFFNESS, DimensionVector::ENERGY);
        // ROTATIONAL_DAMPING (kg·m²·s⁻¹·rad⁻¹) ≠ ENERGY (kg·m²·s⁻²) — time AND angle differ.
        assert_ne!(DimensionVector::ROTATIONAL_DAMPING, DimensionVector::ENERGY);
        // TRANSLATIONAL_DAMPING (kg·s⁻¹) ≠ DYNAMIC_VISCOSITY (kg·m⁻¹·s⁻¹) — length slot differs.
        assert_ne!(
            DimensionVector::TRANSLATIONAL_DAMPING,
            DimensionVector::DYNAMIC_VISCOSITY
        );
    }

    // ─── Step-3 (task 3849): NAMED_DIMENSIONS table registration for flexure types ──
    //
    // Table-driven: asserts both directions (dim→entry and name→dim) for all four
    // flexure types in one pass.  The canonical_name direction (dim→first-match name)
    // is tested separately below because it also covers the Stiffness/TranslationalStiffness
    // alias ordering.

    #[test]
    fn flexure_dims_registered_and_resolve_by_name() {
        let cases: &[(DimensionVector, &str)] = &[
            (DimensionVector::ROTATIONAL_STIFFNESS, "RotationalStiffness"),
            (DimensionVector::ROTATIONAL_DAMPING, "RotationalDamping"),
            (DimensionVector::TRANSLATIONAL_STIFFNESS, "TranslationalStiffness"),
            (DimensionVector::TRANSLATIONAL_DAMPING, "TranslationalDamping"),
        ];
        for &(dim, name) in cases {
            // (a) forward: (dim, name) entry exists in the table
            let registered = super::NAMED_DIMENSIONS
                .iter()
                .any(|(d, n)| *n == name && *d == dim);
            assert!(
                registered,
                "NAMED_DIMENSIONS must contain an entry with dim={dim:?} and name=\"{name}\""
            );
            // (b) name→dim: the name resolves to the expected dim
            let found_dim = super::NAMED_DIMENSIONS
                .iter()
                .find(|(_, n)| *n == name)
                .map(|(d, _)| *d);
            assert_eq!(
                found_dim,
                Some(dim),
                "NAMED_DIMENSIONS name→dim lookup for \"{name}\" must return {dim:?}"
            );
        }
    }

    #[test]
    fn flexure_dims_have_correct_canonical_names() {
        // RotationalStiffness and RotationalDamping are distinct dims — canonical_name is unambiguous.
        assert_eq!(
            DimensionVector::ROTATIONAL_STIFFNESS.canonical_name(),
            Some("RotationalStiffness")
        );
        assert_eq!(
            DimensionVector::ROTATIONAL_DAMPING.canonical_name(),
            Some("RotationalDamping")
        );
        // TranslationalDamping is a distinct dim — canonical_name is unambiguous.
        assert_eq!(
            DimensionVector::TRANSLATIONAL_DAMPING.canonical_name(),
            Some("TranslationalDamping")
        );
        // STIFFNESS canonical name must STILL be "Stiffness" (TranslationalStiffness alias
        // placed AFTER Stiffness in table — first-match unchanged).
        assert_eq!(
            DimensionVector::STIFFNESS.canonical_name(),
            Some("Stiffness"),
            "STIFFNESS canonical_name must remain 'Stiffness' after TranslationalStiffness alias is added"
        );
    }
}
