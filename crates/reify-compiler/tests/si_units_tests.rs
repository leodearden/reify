//! Tests for SI prefix and derived-unit stdlib expansion (task 334).

mod common;

use common::stdlib_param_si_value;
use reify_compiler::{CompiledUnit, compile, si_units};
use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};
use reify_core::{DimensionVector, ModulePath};

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Compile a unit declaration and look up its `dimension` field.
fn unit_dim(source: &str, unit_name: &str) -> DimensionVector {
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);
    let u = module
        .units
        .iter()
        .find(|u| u.name == unit_name)
        .unwrap_or_else(|| panic!("unit {} not found in module", unit_name));
    u.dimension
}

// ─── step-3: resolve_dimension_type recognises the new PascalCase names ──────

#[test]
fn resolve_dimension_type_recognizes_new_names() {
    // Each pair is (unit declaration, expected DimensionVector).
    let cases: &[(&str, DimensionVector)] = &[
        ("pub unit u1 : Energy = 1", DimensionVector::ENERGY),
        ("pub unit u2 : Power = 1", DimensionVector::POWER),
        ("pub unit u3 : Pressure = 1", DimensionVector::PRESSURE),
        ("pub unit u4 : Frequency = 1", DimensionVector::FREQUENCY),
        ("pub unit u5 : Voltage = 1", DimensionVector::VOLTAGE),
        ("pub unit u6 : Charge = 1", DimensionVector::CHARGE),
        (
            "pub unit u7 : Capacitance = 1",
            DimensionVector::CAPACITANCE,
        ),
        ("pub unit u8 : Resistance = 1", DimensionVector::RESISTANCE),
        (
            "pub unit u9 : Conductance = 1",
            DimensionVector::CONDUCTANCE,
        ),
        ("pub unit u10 : Inductance = 1", DimensionVector::INDUCTANCE),
        (
            "pub unit u11 : MagneticFlux = 1",
            DimensionVector::MAGNETIC_FLUX,
        ),
        (
            "pub unit u12 : MagneticFluxDensity = 1",
            DimensionVector::MAGNETIC_FLUX_DENSITY,
        ),
        (
            "pub unit u13 : LuminousFlux = 1",
            DimensionVector::LUMINOUS_FLUX,
        ),
        (
            "pub unit u14 : Illuminance = 1",
            DimensionVector::ILLUMINANCE,
        ),
        (
            "pub unit u15 : AbsorbedDose = 1",
            DimensionVector::ABSORBED_DOSE,
        ),
        (
            "pub unit u16 : AngularVelocity = 1",
            DimensionVector::ANGULAR_VELOCITY,
        ),
        (
            "pub unit u17 : DynamicViscosity = 1",
            DimensionVector::DYNAMIC_VISCOSITY,
        ),
        (
            "pub unit u18 : AmountOfSubstance = 1",
            DimensionVector::AMOUNT_OF_SUBSTANCE,
        ),
        (
            "pub unit u19 : LuminousIntensity = 1",
            DimensionVector::LUMINOUS_INTENSITY,
        ),
        (
            "pub unit u20 : SolidAngle = 1",
            DimensionVector::SOLID_ANGLE,
        ),
    ];
    for (src, expected_dim) in cases {
        // Unit name is "uN" — second word in "pub unit uN : ...".
        let name = src.split_whitespace().nth(2).unwrap();
        let dim = unit_dim(src, name);
        assert_eq!(dim, *expected_dim, "dimension mismatch for source: {}", src);
    }
}

// ─── step-5: SI_PREFIXES table ────────────────────────────────────────────────

#[test]
fn si_prefixes_table_has_20_entries_with_correct_factors() {
    let prefixes = si_units::SI_PREFIXES;
    assert_eq!(prefixes.len(), 20, "expected 20 power-of-1000 SI prefixes");

    // Spot-check key entries by name.
    let find = |sym: &str| -> f64 {
        prefixes
            .iter()
            .find(|(s, _)| *s == sym)
            .unwrap_or_else(|| panic!("prefix '{}' missing", sym))
            .1
    };

    assert_eq!(find("q"), 1e-30);
    assert_eq!(find("y"), 1e-24);
    assert_eq!(find("n"), 1e-9);
    assert_eq!(find("m"), 1e-3);
    assert_eq!(find("k"), 1e3);
    assert_eq!(find("M"), 1e6);
    assert_eq!(find("G"), 1e9);
    assert_eq!(find("T"), 1e12);
    assert_eq!(find("Y"), 1e24);
    assert_eq!(find("Q"), 1e30);

    // Non-power-of-1000 prefixes MUST NOT appear.
    for forbidden in &["c", "d", "da", "h"] {
        assert!(
            prefixes.iter().all(|(s, _)| *s != *forbidden),
            "prefix '{}' must not appear",
            forbidden
        );
    }
}

// ─── step-7: build_si_units_source emits base prefixed units ─────────────────

#[test]
fn build_si_units_source_contains_base_prefixed_units() {
    let src = si_units::build_si_units_source();

    // Length prefixed bases.
    for line in &[
        "pub unit km : Length =",
        "pub unit nm : Length =",
        "pub unit pm : Length =",
        "pub unit fm : Length =",
        "pub unit Tm : Length =",
        "pub unit Qm : Length =",
    ] {
        assert!(
            src.contains(line),
            "generated source missing length line: `{}`\n\nfull source:\n{}",
            line,
            src
        );
    }

    // Mass prefixed (gram base — kg is SI base and NOT emitted).
    for line in &[
        "pub unit mg : Mass =",
        "pub unit ug : Mass =",
        "pub unit ng : Mass =",
        "pub unit pg : Mass =",
        "pub unit Gg : Mass =",
        "pub unit Tg : Mass =",
    ] {
        assert!(
            src.contains(line),
            "generated source missing mass line: `{}`",
            line
        );
    }
    // kg must NOT be regenerated (it's the SI base, lives in units.ri).
    assert!(
        !src.contains("pub unit kg "),
        "generator must not emit `kg` (already SI base in units.ri)"
    );

    // Time prefixed bases.
    for line in &[
        "pub unit ks : Time =",
        "pub unit ns : Time =",
        "pub unit ps : Time =",
        "pub unit fs : Time =",
        "pub unit Ts : Time =",
        "pub unit Qs : Time =",
    ] {
        assert!(
            src.contains(line),
            "generated source missing time line: `{}`",
            line
        );
    }
}

// ─── step-9: generator output parses and compiles cleanly ────────────────────

#[test]
fn generated_source_parses_and_compiles_cleanly() {
    let src = si_units::build_si_units_source();
    let parsed = reify_syntax::parse(&src, ModulePath::new(vec!["std".into(), "si_units".into()]));
    assert!(
        parsed.errors.is_empty(),
        "generated source has parse errors: {:?}\n\nsource:\n{}",
        parsed.errors,
        src
    );
    let module = compile(&parsed);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "generated source produced compile errors: {:?}",
        errs
    );

    // Spot-check some prefixed units.
    let find_unit = |name: &str| -> &CompiledUnit {
        module
            .units
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("unit `{}` not present in compiled module", name))
    };

    let km = find_unit("km");
    assert_eq!(km.dimension, DimensionVector::LENGTH);
    assert!((km.factor - 1000.0).abs() < 1e-6);

    let nm = find_unit("nm");
    assert_eq!(nm.dimension, DimensionVector::LENGTH);
    assert!((nm.factor - 1e-9).abs() < 1e-14);

    let pg = find_unit("pg");
    assert_eq!(pg.dimension, DimensionVector::MASS);
    // pg = 1e-12 * 1e-3 = 1e-15 kg
    assert!((pg.factor - 1e-15).abs() < 1e-20);

    let gg = find_unit("Gg");
    assert_eq!(gg.dimension, DimensionVector::MASS);
    // Gg = 1e9 * 1e-3 = 1e6 kg
    assert!((gg.factor - 1e6).abs() < 1e-3);

    let ks = find_unit("ks");
    assert_eq!(ks.dimension, DimensionVector::TIME);
    assert!((ks.factor - 1000.0).abs() < 1e-6);
}

// ─── step-11: task-specified prefixed-base resolution via stdlib ─────────────

#[test]
fn task_test_prefixed_bases_resolve_via_stdlib() {
    // 1km = 1000m
    let (v, d) = stdlib_param_si_value("Length", "1km");
    assert!((v - 1000.0).abs() < 1e-9, "1km should be 1000, got {}", v);
    assert_eq!(d, DimensionVector::LENGTH);

    // 1nm = 1e-9m
    let (v, d) = stdlib_param_si_value("Length", "1nm");
    assert!((v - 1e-9).abs() < 1e-15, "1nm should be 1e-9, got {}", v);
    assert_eq!(d, DimensionVector::LENGTH);

    // 1Tm = 1e12 m
    let (v, d) = stdlib_param_si_value("Length", "1Tm");
    assert!((v - 1e12).abs() < 1e-3, "1Tm should be 1e12, got {}", v);
    assert_eq!(d, DimensionVector::LENGTH);

    // 1ks = 1000 s
    let (v, d) = stdlib_param_si_value("Time", "1ks");
    assert!((v - 1000.0).abs() < 1e-9, "1ks should be 1000, got {}", v);
    assert_eq!(d, DimensionVector::TIME);

    // 1pg = 1e-15 kg
    let (v, d) = stdlib_param_si_value("Mass", "1pg");
    assert!((v - 1e-15).abs() < 1e-20, "1pg should be 1e-15, got {}", v);
    assert_eq!(d, DimensionVector::MASS);
}

// ─── step-13: SI_DERIVED_UNITS table ─────────────────────────────────────────

#[test]
fn si_derived_units_table_defines_newtons_pascals_joules() {
    let table = si_units::SI_DERIVED_UNITS;

    // Helper: look up a derived unit by name.
    let find = |name: &str| -> &si_units::SiDerivedUnit {
        table
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("derived unit `{}` missing from SI_DERIVED_UNITS", name))
    };

    // Expected: (name, dimension, factor). Factor 1.0 means the SI unit itself.
    let cases: &[(&str, &str, f64)] = &[
        // Core SI derived units (factor 1 in SI).
        ("N", "Force", 1.0),
        ("Pa", "Pressure", 1.0),
        ("J", "Energy", 1.0),
        ("W", "Power", 1.0),
        ("V", "Voltage", 1.0),
        ("Hz", "Frequency", 1.0),
        ("ohm", "Resistance", 1.0),
        ("S", "Conductance", 1.0),
        ("F", "Capacitance", 1.0),
        ("H", "Inductance", 1.0),
        ("Wb", "MagneticFlux", 1.0),
        ("T", "MagneticFluxDensity", 1.0),
        ("lm", "LuminousFlux", 1.0),
        ("lx", "Illuminance", 1.0),
        // Bq shares s⁻¹ with Frequency (dimensionally).
        ("Bq", "Frequency", 1.0),
        ("Gy", "AbsorbedDose", 1.0),
        ("Sv", "AbsorbedDose", 1.0),
        ("C", "Charge", 1.0),
        // Non-SI factor conversions.
        ("eV", "Energy", 1.602176634e-19),
        ("bar", "Pressure", 100000.0),
        ("mbar", "Pressure", 100.0),
        ("rpm", "AngularVelocity", std::f64::consts::PI / 30.0),
        ("rad_per_s", "AngularVelocity", 1.0),
        ("Pa_s", "DynamicViscosity", 1.0),
        ("sr", "SolidAngle", 1.0),
    ];

    assert_eq!(
        table.len(),
        cases.len(),
        "SI_DERIVED_UNITS must contain exactly {} entries, found {}",
        cases.len(),
        table.len()
    );

    for (name, dim, factor) in cases {
        let entry = find(name);
        assert_eq!(
            entry.dimension, *dim,
            "derived unit `{}` has wrong dimension: expected {}, got {}",
            name, dim, entry.dimension
        );
        assert!(
            (entry.factor - factor).abs() < factor.abs().max(1.0) * 1e-12,
            "derived unit `{}` has wrong factor: expected {}, got {}",
            name,
            factor,
            entry.factor
        );
    }
}

// ─── step-15: task-specified derived units (and prefixed) via stdlib ─────────

#[test]
fn task_test_derived_units_and_prefixed_resolve_via_stdlib() {
    // Each case: (param_type, literal, expected_si_value, expected_dimension).
    // Epsilon is proportional to magnitude: small factors use a tight absolute
    // check, large factors use a relative check.
    let cases: &[(&str, &str, f64, DimensionVector)] = &[
        // Force + prefixes.
        ("Force", "1N", 1.0, DimensionVector::FORCE),
        ("Force", "1kN", 1000.0, DimensionVector::FORCE),
        ("Force", "1MN", 1e6, DimensionVector::FORCE),
        // Pressure + prefixes + non-SI accepted units.
        ("Pressure", "1Pa", 1.0, DimensionVector::PRESSURE),
        ("Pressure", "1kPa", 1000.0, DimensionVector::PRESSURE),
        ("Pressure", "1MPa", 1e6, DimensionVector::PRESSURE),
        ("Pressure", "1GPa", 1e9, DimensionVector::PRESSURE),
        ("Pressure", "1bar", 100000.0, DimensionVector::PRESSURE),
        ("Pressure", "1mbar", 100.0, DimensionVector::PRESSURE),
        // Energy + prefixes + eV variants.
        ("Energy", "1J", 1.0, DimensionVector::ENERGY),
        ("Energy", "1kJ", 1000.0, DimensionVector::ENERGY),
        ("Energy", "1MJ", 1e6, DimensionVector::ENERGY),
        ("Energy", "1eV", 1.602176634e-19, DimensionVector::ENERGY),
        ("Energy", "1keV", 1.602176634e-16, DimensionVector::ENERGY),
        ("Energy", "1MeV", 1.602176634e-13, DimensionVector::ENERGY),
        // Power + prefixes.
        ("Power", "1W", 1.0, DimensionVector::POWER),
        ("Power", "1kW", 1000.0, DimensionVector::POWER),
        ("Power", "1MW", 1e6, DimensionVector::POWER),
        // Frequency + prefixes.
        ("Frequency", "1Hz", 1.0, DimensionVector::FREQUENCY),
        ("Frequency", "1kHz", 1000.0, DimensionVector::FREQUENCY),
        ("Frequency", "1MHz", 1e6, DimensionVector::FREQUENCY),
        ("Frequency", "1GHz", 1e9, DimensionVector::FREQUENCY),
        ("Frequency", "1Bq", 1.0, DimensionVector::FREQUENCY),
        // Voltage.
        ("Voltage", "1V", 1.0, DimensionVector::VOLTAGE),
        ("Voltage", "1mV", 1e-3, DimensionVector::VOLTAGE),
        ("Voltage", "1kV", 1000.0, DimensionVector::VOLTAGE),
        // Resistance.
        ("Resistance", "1ohm", 1.0, DimensionVector::RESISTANCE),
        ("Resistance", "1kohm", 1000.0, DimensionVector::RESISTANCE),
        // Capacitance.
        ("Capacitance", "1F", 1.0, DimensionVector::CAPACITANCE),
        ("Capacitance", "1uF", 1e-6, DimensionVector::CAPACITANCE),
        ("Capacitance", "1pF", 1e-12, DimensionVector::CAPACITANCE),
        // Inductance.
        ("Inductance", "1H", 1.0, DimensionVector::INDUCTANCE),
        // Magnetic flux + density.
        ("MagneticFlux", "1Wb", 1.0, DimensionVector::MAGNETIC_FLUX),
        (
            "MagneticFluxDensity",
            "1T",
            1.0,
            DimensionVector::MAGNETIC_FLUX_DENSITY,
        ),
        // Luminous flux + illuminance.
        ("LuminousFlux", "1lm", 1.0, DimensionVector::LUMINOUS_FLUX),
        ("Illuminance", "1lx", 1.0, DimensionVector::ILLUMINANCE),
        // Absorbed dose (Gy and Sv share dim).
        ("AbsorbedDose", "1Gy", 1.0, DimensionVector::ABSORBED_DOSE),
        ("AbsorbedDose", "1Sv", 1.0, DimensionVector::ABSORBED_DOSE),
        // Angular velocity.
        (
            "AngularVelocity",
            "1rpm",
            std::f64::consts::PI / 30.0,
            DimensionVector::ANGULAR_VELOCITY,
        ),
        (
            "AngularVelocity",
            "1rad_per_s",
            1.0,
            DimensionVector::ANGULAR_VELOCITY,
        ),
        // Dynamic viscosity.
        (
            "DynamicViscosity",
            "1Pa_s",
            1.0,
            DimensionVector::DYNAMIC_VISCOSITY,
        ),
        // Solid angle.
        ("SolidAngle", "1sr", 1.0, DimensionVector::SOLID_ANGLE),
        // RF/IC engineering: femtofarad, pico/femtosiemens.
        ("Capacitance", "1fF", 1e-15, DimensionVector::CAPACITANCE),
        ("Conductance", "1pS", 1e-12, DimensionVector::CONDUCTANCE),
        ("Conductance", "1fS", 1e-15, DimensionVector::CONDUCTANCE),
    ];

    for (ptype, literal, expected, expected_dim) in cases {
        let (v, d) = stdlib_param_si_value(ptype, literal);
        // Tolerance: 1e-12 relative or 1e-25 absolute (whichever larger) to
        // accommodate both large (1e9) and tiny (1.6e-19) expected values.
        let tol = (expected.abs() * 1e-12).max(1e-25);
        assert!(
            (v - expected).abs() < tol,
            "{} : {} — expected {}, got {} (tol {})",
            ptype,
            literal,
            expected,
            v,
            tol
        );
        assert_eq!(
            d, *expected_dim,
            "{} : {} — dimension mismatch",
            ptype, literal
        );
    }
}

// ─── step-17: every derived unit has the correct DimensionVector ─────────────

#[test]
fn all_derived_units_have_correct_dimension_vectors() {
    // Targeted per-unit check: cover every name in the task's derived list.
    // This duplicates some coverage from step-15, but isolates any wrong
    // dimension assignment in `SI_DERIVED_UNITS` or `type_resolution.rs`.
    let cases: &[(&str, &str, DimensionVector)] = &[
        ("Force", "1N", DimensionVector::FORCE),
        ("Energy", "1J", DimensionVector::ENERGY),
        ("Power", "1W", DimensionVector::POWER),
        ("Pressure", "1Pa", DimensionVector::PRESSURE),
        ("Voltage", "1V", DimensionVector::VOLTAGE),
        ("Resistance", "1ohm", DimensionVector::RESISTANCE),
        ("Conductance", "1S", DimensionVector::CONDUCTANCE),
        ("Capacitance", "1F", DimensionVector::CAPACITANCE),
        ("Inductance", "1H", DimensionVector::INDUCTANCE),
        ("MagneticFlux", "1Wb", DimensionVector::MAGNETIC_FLUX),
        (
            "MagneticFluxDensity",
            "1T",
            DimensionVector::MAGNETIC_FLUX_DENSITY,
        ),
        ("Frequency", "1Hz", DimensionVector::FREQUENCY),
        ("AngularVelocity", "1rpm", DimensionVector::ANGULAR_VELOCITY),
        (
            "AngularVelocity",
            "1rad_per_s",
            DimensionVector::ANGULAR_VELOCITY,
        ),
        (
            "DynamicViscosity",
            "1Pa_s",
            DimensionVector::DYNAMIC_VISCOSITY,
        ),
        ("LuminousFlux", "1lm", DimensionVector::LUMINOUS_FLUX),
        ("Illuminance", "1lx", DimensionVector::ILLUMINANCE),
        ("Frequency", "1Bq", DimensionVector::FREQUENCY),
        ("AbsorbedDose", "1Gy", DimensionVector::ABSORBED_DOSE),
        ("AbsorbedDose", "1Sv", DimensionVector::ABSORBED_DOSE),
        ("Energy", "1eV", DimensionVector::ENERGY),
        ("Pressure", "1bar", DimensionVector::PRESSURE),
        ("Pressure", "1mbar", DimensionVector::PRESSURE),
        ("Charge", "1C", DimensionVector::CHARGE),
        ("SolidAngle", "1sr", DimensionVector::SOLID_ANGLE),
    ];

    for (ptype, literal, expected_dim) in cases {
        let (_, d) = stdlib_param_si_value(ptype, literal);
        assert_eq!(
            d, *expected_dim,
            "derived unit `{}` has wrong dimension (param type {})",
            literal, ptype
        );
    }
}

// ─── step-19: unknown unit names still yield a parse / resolve error ─────────

#[test]
fn unknown_unit_still_produces_parse_error() {
    // Regression: expanding the SI-unit surface must not silently accept
    // arbitrary identifiers. An unknown unit after a number literal should
    // yield an Error-severity diagnostic mentioning the unit name.
    let source = "structure def S { param x : Length = 5xyz123 }";
    let module = compile_source_with_stdlib(source);
    let errs = errors_only(&module);
    assert!(
        !errs.is_empty(),
        "expected at least one Error diagnostic for unknown unit `xyz123`, got none"
    );
    let has_unknown_unit_msg = errs
        .iter()
        .any(|d| d.message.contains("unknown unit") && d.message.contains("xyz123"));
    assert!(
        has_unknown_unit_msg,
        "expected error mentioning `unknown unit` and `xyz123`, got diagnostics: {:?}",
        errs
    );
}

// ─── step-21: existing hand-written units.ri entries still resolve ──────────

#[test]
#[allow(non_snake_case)]
fn existing_units_ri_still_has_m_kg_s_rad_deg_degC_degF_imperial() {
    // Guard: trimming units.ri to remove SI-generator duplicates must not
    // accidentally drop any non-SI / affine / SI-base entry. Values below
    // are the canonical SI conversions from the hand-written stdlib.

    // Pure SI bases (factor 1.0).
    let cases_si_base: &[(&str, &str, f64, DimensionVector)] = &[
        ("Length", "1m", 1.0, DimensionVector::LENGTH),
        ("Mass", "1kg", 1.0, DimensionVector::MASS),
        ("Time", "1s", 1.0, DimensionVector::TIME),
        ("Angle", "1rad", 1.0, DimensionVector::ANGLE),
    ];
    for (ptype, literal, expected, expected_dim) in cases_si_base {
        let (v, d) = stdlib_param_si_value(ptype, literal);
        assert!(
            (v - expected).abs() < 1e-12,
            "{} : {} expected {}, got {}",
            ptype,
            literal,
            expected,
            v
        );
        assert_eq!(d, *expected_dim);
    }

    // Angle — deg = π/180 rad.
    let (v, d) = stdlib_param_si_value("Angle", "1deg");
    assert!(
        (v - std::f64::consts::PI / 180.0).abs() < 1e-12,
        "1deg should be π/180, got {}",
        v
    );
    assert_eq!(d, DimensionVector::ANGLE);

    // Temperature affine path — 32degF is still the freezing point of water.
    let (v, _) = stdlib_param_si_value("Temperature", "32degF");
    assert!(
        (v - 273.15).abs() < 1e-6,
        "32degF should be 273.15 K, got {}",
        v
    );

    // cm is NOT auto-generated (centi is not in the power-of-1000 set), so
    // it must still live in the hand-written units.ri.
    let (v, d) = stdlib_param_si_value("Length", "1cm");
    assert!((v - 0.01).abs() < 1e-12, "1cm should be 0.01, got {}", v);
    assert_eq!(d, DimensionVector::LENGTH);

    // Imperial length.
    let imperial_lengths: &[(&str, f64)] =
        &[("1in", 0.0254), ("1ft", 0.3048), ("1thou", 0.0000254)];
    for (literal, expected) in imperial_lengths {
        let (v, d) = stdlib_param_si_value("Length", literal);
        assert!(
            (v - expected).abs() < 1e-12,
            "{} expected {}, got {}",
            literal,
            expected,
            v
        );
        assert_eq!(d, DimensionVector::LENGTH);
    }

    // Imperial mass.
    let (v, _) = stdlib_param_si_value("Mass", "1lb");
    assert!((v - 0.45359237).abs() < 1e-12, "1lb wrong: {}", v);
    let (v, _) = stdlib_param_si_value("Mass", "1oz");
    assert!((v - 0.028349523125).abs() < 1e-12, "1oz wrong: {}", v);

    // Non-SI time.
    let (v, _) = stdlib_param_si_value("Time", "1min");
    assert!((v - 60.0).abs() < 1e-12, "1min wrong: {}", v);
    let (v, _) = stdlib_param_si_value("Time", "1h");
    assert!((v - 3600.0).abs() < 1e-12, "1h wrong: {}", v);

    // g must still be available as a standalone mass unit (gram) — the
    // generator uses it only as a prefix base, never as standalone, so
    // units.ri carries it.
    let (v, _) = stdlib_param_si_value("Mass", "1g");
    assert!((v - 0.001).abs() < 1e-12, "1g wrong: {}", v);
}

// ── PrefixSet enum semantics ─────────────────────────────────────────────────

#[test]
fn prefix_set_all_includes_any_symbol() {
    // PrefixSet::All must return true for every known SI prefix symbol.
    assert!(si_units::PrefixSet::All.includes("k"));
    assert!(si_units::PrefixSet::All.includes("M"));
    assert!(si_units::PrefixSet::All.includes("n"));
    assert!(si_units::PrefixSet::All.includes("q"));
    assert!(si_units::PrefixSet::All.includes("Q"));
}

#[test]
fn prefix_set_only_includes_listed_symbol() {
    let ps = si_units::PrefixSet::Only(&["k", "M"]);
    assert!(ps.includes("k"));
    assert!(ps.includes("M"));
}

#[test]
fn prefix_set_only_excludes_unlisted_symbol() {
    let ps = si_units::PrefixSet::Only(&["k", "M"]);
    assert!(!ps.includes("G"));
    assert!(!ps.includes("n"));
}

#[test]
fn prefix_set_only_empty_excludes_all() {
    let ps = si_units::PrefixSet::Only(&[]);
    assert!(!ps.includes("k"));
    assert!(!ps.includes("m"));
    assert!(!ps.includes("n"));
}

// ── PrefixSet field on SiPrefixBase ──────────────────────────────────────────

#[test]
fn si_prefix_bases_use_prefix_set_enum() {
    let find_base = |name: &str| -> &si_units::SiPrefixBase {
        si_units::SI_PREFIX_BASES
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("base `{}` missing from SI_PREFIX_BASES", name))
    };

    // Unrestricted bases use PrefixSet::All.
    for name in &["m", "g", "s", "A", "mol"] {
        let base = find_base(name);
        assert_eq!(
            base.prefix_combos,
            si_units::PrefixSet::All,
            "base `{}` should have PrefixSet::All, got {:?}",
            name,
            base.prefix_combos
        );
    }

    // Restricted bases use PrefixSet::Only with correct prefix lists.
    let k = find_base("K");
    assert_eq!(
        k.prefix_combos,
        si_units::PrefixSet::Only(&["n", "u", "m"]),
        "K should have PrefixSet::Only(&[\"n\", \"u\", \"m\"]), got {:?}",
        k.prefix_combos
    );

    let cd = find_base("cd");
    assert_eq!(
        cd.prefix_combos,
        si_units::PrefixSet::Only(&["m", "u"]),
        "cd should have PrefixSet::Only(&[\"m\", \"u\"]), got {:?}",
        cd.prefix_combos
    );

    let rad = find_base("rad");
    assert_eq!(
        rad.prefix_combos,
        si_units::PrefixSet::Only(&["m", "u", "n"]),
        "rad should have PrefixSet::Only(&[\"m\", \"u\", \"n\"]), got {:?}",
        rad.prefix_combos
    );
}

// ─── S4: SI_PREFIX_BASES restricted prefix filtering ─────────────────────────

/// Once SI_PREFIX_BASES supports per-base prefix_combos filtering, the generator
/// must only emit the allowed prefixes for restricted bases (K, cd, rad) while
/// still emitting all 20 prefixes for unrestricted bases (m, g, s, A, mol).
#[test]
fn si_prefix_bases_restricted_entries_only_generate_specified_prefixes() {
    let src = si_units::build_si_units_source();

    // Nonsensical combinations must be ABSENT after filtering.
    for absent in &[
        "pub unit QK",
        "pub unit qK",
        "pub unit qcd",
        "pub unit Qcd",
        "pub unit Qrad",
        "pub unit qrad",
    ] {
        assert!(
            !src.contains(absent),
            "generated source must NOT contain `{}` (nonsensical prefix combo)\n\nfull source:\n{}",
            absent,
            src
        );
    }

    // Restricted bases must still emit their allowed prefixes.
    for present in &[
        "pub unit mK : Temperature",
        "pub unit uK : Temperature",
        "pub unit nK : Temperature",
        "pub unit mcd : LuminousIntensity",
        "pub unit ucd : LuminousIntensity",
        "pub unit mrad : Angle",
        "pub unit urad : Angle",
        "pub unit nrad : Angle",
    ] {
        assert!(
            src.contains(present),
            "generated source missing expected restricted-prefix line: `{}`\n\nfull source:\n{}",
            present,
            src
        );
    }

    // Unrestricted bases must still get all 20 prefixes.
    for present in &["pub unit Qm : Length", "pub unit qm : Length"] {
        assert!(
            src.contains(present),
            "generated source missing unrestricted-base line: `{}`\n\nfull source:\n{}",
            present,
            src
        );
    }

    // Exact-count assertions: restricted bases must emit ONLY their allowed
    // prefixes — a stale or over-inclusive filter would still pass the
    // presence checks above but fail these.
    let k_count = si_units::SI_PREFIXES
        .iter()
        .filter(|(p, _)| src.contains(&format!("pub unit {}K ", p)))
        .count();
    assert_eq!(
        k_count, 3,
        "K must emit exactly 3 prefixed units (n, u, m); got {}",
        k_count
    );

    let cd_count = si_units::SI_PREFIXES
        .iter()
        .filter(|(p, _)| src.contains(&format!("pub unit {}cd ", p)))
        .count();
    assert_eq!(
        cd_count, 2,
        "cd must emit exactly 2 prefixed units (m, u); got {}",
        cd_count
    );

    let rad_count = si_units::SI_PREFIXES
        .iter()
        .filter(|(p, _)| src.contains(&format!("pub unit {}rad ", p)))
        .count();
    assert_eq!(
        rad_count, 3,
        "rad must emit exactly 3 prefixed units (m, u, n); got {}",
        rad_count
    );
}

// ── PrefixSet field on SiDerivedUnit ─────────────────────────────────────────

#[test]
fn si_derived_units_use_prefix_set_only() {
    // (1) Every entry in SI_DERIVED_UNITS must use PrefixSet::Only (never All).
    for unit in si_units::SI_DERIVED_UNITS {
        assert!(
            matches!(unit.prefix_combos, si_units::PrefixSet::Only(_)),
            "derived unit `{}` must use PrefixSet::Only, got {:?}",
            unit.name,
            unit.prefix_combos
        );
    }

    // (2) Units with no prefixed variants must have PrefixSet::Only(&[]).
    for name in &[
        "lm",
        "lx",
        "Bq",
        "bar",
        "mbar",
        "rpm",
        "rad_per_s",
        "Pa_s",
        "sr",
    ] {
        let unit = si_units::SI_DERIVED_UNITS
            .iter()
            .find(|u| u.name == *name)
            .unwrap_or_else(|| panic!("derived unit `{}` missing", name));
        assert_eq!(
            unit.prefix_combos,
            si_units::PrefixSet::Only(&[]),
            "unit `{}` should have PrefixSet::Only(&[]), got {:?}",
            name,
            unit.prefix_combos
        );
    }

    // (3) Spot-check: N has PrefixSet::Only(&["k", "M", "G"]).
    let n = si_units::SI_DERIVED_UNITS
        .iter()
        .find(|u| u.name == "N")
        .expect("N missing from SI_DERIVED_UNITS");
    assert_eq!(
        n.prefix_combos,
        si_units::PrefixSet::Only(&["k", "M", "G"]),
        "N should have PrefixSet::Only(&[\"k\", \"M\", \"G\"]), got {:?}",
        n.prefix_combos
    );
}
