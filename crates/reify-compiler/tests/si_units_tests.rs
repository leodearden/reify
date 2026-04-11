//! Tests for SI prefix and derived-unit stdlib expansion (task 334).

use reify_compiler::{CompiledModule, CompiledUnit, compile, compile_with_stdlib, si_units};
use reify_types::{DimensionVector, ModulePath, Severity};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("si_units_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile(&parsed)
}

fn compile_with_stdlib_helper(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("si_units_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_stdlib(&parsed)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Compile a unit declaration and look up its `dimension` field.
fn unit_dim(source: &str, unit_name: &str) -> DimensionVector {
    let module = parse_and_compile(source);
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
        ("pub unit u7 : Capacitance = 1", DimensionVector::CAPACITANCE),
        ("pub unit u8 : Resistance = 1", DimensionVector::RESISTANCE),
        ("pub unit u9 : Conductance = 1", DimensionVector::CONDUCTANCE),
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
        ("pub unit u14 : Illuminance = 1", DimensionVector::ILLUMINANCE),
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
        assert_eq!(
            dim, *expected_dim,
            "dimension mismatch for source: {}",
            src
        );
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
