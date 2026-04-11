//! SI prefix and derived-unit tables + `.ri` source generator.
//!
//! Provides const tables for the 20 power-of-1000 SI prefixes (quecto..quetta)
//! and the standard SI derived units (N, Pa, J, W, Hz, V, ohm, S, F, H, Wb,
//! T, lm, lx, Bq, Gy, Sv, C, eV, bar, mbar, rpm, rad_per_s, Pa_s), together
//! with a programmatic generator that emits Reify unit declarations at
//! `load_stdlib()` time.

/// The 20 power-of-1000 SI prefixes, from quecto (1e-30) to quetta (1e30).
///
/// Excludes centi/deci/deca/hecto — the four non-power-of-1000 prefixes.
/// This matches the 2022 BIPM revision of the SI prefix set for engineering
/// use. ASCII `u` is the conventional stand-in for `µ` (micro).
pub const SI_PREFIXES: &[(&str, f64)] = &[
    ("q", 1e-30),
    ("r", 1e-27),
    ("y", 1e-24),
    ("z", 1e-21),
    ("a", 1e-18),
    ("f", 1e-15),
    ("p", 1e-12),
    ("n", 1e-9),
    ("u", 1e-6),
    ("m", 1e-3),
    ("k", 1e3),
    ("M", 1e6),
    ("G", 1e9),
    ("T", 1e12),
    ("P", 1e15),
    ("E", 1e18),
    ("Z", 1e21),
    ("Y", 1e24),
    ("R", 1e27),
    ("Q", 1e30),
];

/// SI base units that receive the full 20-prefix set, paired with their
/// dimension name. Excludes unprefixed bases themselves — those live in
/// `stdlib/units.ri` (m, kg, s, rad, K).
///
/// Note: `g` is used as the mass prefix base (not `kg`) because prefixes
/// can't stack, and `kg` is already the SI base unit. The generator
/// multiplies gram-prefix factors by 1e-3 and skips the `k` prefix to
/// avoid re-declaring `kg`.
pub const SI_PREFIX_BASES: &[(&str, &str)] = &[
    ("m", "Length"),
    ("g", "Mass"),
    ("s", "Time"),
    ("A", "Current"),
    ("K", "Temperature"),
    ("mol", "AmountOfSubstance"),
    ("cd", "LuminousIntensity"),
    ("rad", "Angle"),
];

/// One SI (or SI-factor-derived) derived-unit entry.
///
/// - `name` — unit symbol as written in Reify source (e.g. `"Pa"`, `"ohm"`, `"eV"`).
/// - `dimension` — PascalCase dimension name resolved via `type_resolution::resolve_dimension_type`.
/// - `factor` — multiplicative conversion to SI (e.g. `1.0` for Pa, `1.602176634e-19` for eV).
/// - `prefix_combos` — which SI prefixes to auto-generate for this unit. The
///   conventional engineering subset — not every prefix × every unit, since
///   `RQ`-scale kelvin or `y`-scale siemens would only bloat the generated source.
///   Empty means the unit is emitted unprefixed only.
pub struct SiDerivedUnit {
    pub name: &'static str,
    pub dimension: &'static str,
    pub factor: f64,
    pub prefix_combos: &'static [&'static str],
}

/// The 24 standard SI derived units plus non-SI accepted engineering units
/// (`eV`, `bar`, `mbar`, `rpm`, `rad_per_s`, `Pa_s`).
///
/// Ordering matches the task spec. `prefix_combos` holds only the
/// commonly-used engineering prefixes per unit; the generator expands each
/// into `<prefix><name>` declarations with `prefix_factor * base_factor`.
///
/// Design notes:
/// - `Bq` is dimensionally `s⁻¹` (same as `Hz`) — distinct symbol, same dim.
/// - `Sv` and `Gy` both share the absorbed-dose dimension (`m²·s⁻²`).
/// - `rpm = 2π/60 rad/s` — computed literally as `PI / 30` (see test).
/// - `bar = 100000 Pa`, `mbar = 100 Pa` — non-SI but widely used.
/// - `Pa_s`, `rad_per_s` use underscore names to avoid the `Pas` / `rads`
///   prefix-ambiguity (`Pas` could parse as peta-second).
pub const SI_DERIVED_UNITS: &[SiDerivedUnit] = &[
    // Core SI derived units (factor = 1.0 in SI).
    SiDerivedUnit {
        name: "N",
        dimension: "Force",
        factor: 1.0,
        prefix_combos: &["k", "M", "G"],
    },
    SiDerivedUnit {
        name: "Pa",
        dimension: "Pressure",
        factor: 1.0,
        prefix_combos: &["k", "M", "G"],
    },
    SiDerivedUnit {
        name: "J",
        dimension: "Energy",
        factor: 1.0,
        prefix_combos: &["m", "k", "M", "G"],
    },
    SiDerivedUnit {
        name: "W",
        dimension: "Power",
        factor: 1.0,
        prefix_combos: &["u", "m", "k", "M", "G"],
    },
    SiDerivedUnit {
        name: "V",
        dimension: "Voltage",
        factor: 1.0,
        prefix_combos: &["u", "m", "k"],
    },
    SiDerivedUnit {
        name: "Hz",
        dimension: "Frequency",
        factor: 1.0,
        prefix_combos: &["k", "M", "G", "T"],
    },
    SiDerivedUnit {
        name: "ohm",
        dimension: "Resistance",
        factor: 1.0,
        prefix_combos: &["m", "k", "M", "G"],
    },
    SiDerivedUnit {
        name: "S",
        dimension: "Conductance",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "F",
        dimension: "Capacitance",
        factor: 1.0,
        prefix_combos: &["p", "n", "u", "m"],
    },
    SiDerivedUnit {
        name: "H",
        dimension: "Inductance",
        factor: 1.0,
        prefix_combos: &["u", "m"],
    },
    SiDerivedUnit {
        name: "Wb",
        dimension: "MagneticFlux",
        factor: 1.0,
        prefix_combos: &["u", "m"],
    },
    SiDerivedUnit {
        name: "T",
        dimension: "MagneticFluxDensity",
        factor: 1.0,
        prefix_combos: &["u", "m"],
    },
    SiDerivedUnit {
        name: "lm",
        dimension: "LuminousFlux",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "lx",
        dimension: "Illuminance",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "Bq",
        dimension: "Frequency",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "Gy",
        dimension: "AbsorbedDose",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "Sv",
        dimension: "AbsorbedDose",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "C",
        dimension: "Charge",
        factor: 1.0,
        prefix_combos: &["p", "n", "u", "m"],
    },
    // Non-SI factor conversions.
    SiDerivedUnit {
        name: "eV",
        dimension: "Energy",
        factor: 1.602176634e-19,
        prefix_combos: &["k", "M", "G", "T"],
    },
    SiDerivedUnit {
        name: "bar",
        dimension: "Pressure",
        factor: 100000.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "mbar",
        dimension: "Pressure",
        factor: 100.0,
        prefix_combos: &[],
    },
    // rpm = 2π/60 rad·s⁻¹ = π/30.
    SiDerivedUnit {
        name: "rpm",
        dimension: "AngularVelocity",
        factor: std::f64::consts::PI / 30.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "rad_per_s",
        dimension: "AngularVelocity",
        factor: 1.0,
        prefix_combos: &[],
    },
    SiDerivedUnit {
        name: "Pa_s",
        dimension: "DynamicViscosity",
        factor: 1.0,
        prefix_combos: &[],
    },
];

/// Build the `.ri` source text for all SI units.
///
/// The generated source is compiled at `load_stdlib()` time as a synthetic
/// `std.si_units` module prepended to the stdlib chain. The text contains:
///   1. Prefixed base units (20 prefixes × 8 bases, minus `kg` special-case)
///   2. SI derived units with their SI conversion factor
///   3. Prefixed derived units (per-unit engineering prefix subsets)
///
/// All factors are emitted as plain decimal literals (tree-sitter's
/// `number_literal` grammar does not accept scientific notation).
pub fn build_si_units_source() -> String {
    let mut out = String::new();
    out.push_str("// Generated by crates/reify-compiler/src/si_units.rs\n");
    out.push_str("// Do not edit by hand.\n\n");

    // Section 1: prefixed base units.
    out.push_str("// ── SI-prefixed base units ────────────────────────────\n");
    for (base, dim) in SI_PREFIX_BASES {
        // Gram is special: kg is the SI base, so each gram-prefix factor
        // must be multiplied by 1e-3 (since g = 0.001 kg), and the `k`
        // prefix must be skipped to avoid re-declaring kg.
        let base_factor_exp: i32 = if *base == "g" { -3 } else { 0 };

        for (prefix, prefix_factor) in SI_PREFIXES {
            // Skip `k` + `g` → would produce kg (already SI base).
            if *base == "g" && *prefix == "k" {
                continue;
            }
            let prefix_exp = prefix_factor.log10().round() as i32;
            let total_exp = prefix_exp + base_factor_exp;
            let factor_str = format_power_of_ten(total_exp);
            out.push_str(&format!(
                "pub unit {}{} : {} = {}\n",
                prefix, base, dim, factor_str
            ));
        }
        out.push('\n');
    }

    // Section 2+3: SI derived units and their prefixed variants.
    //
    // Emit each derived unit followed immediately by its prefixed variants,
    // so related declarations stay grouped in the generated source. The
    // registry doesn't care about ordering beyond "declared before use".
    out.push_str("// ── SI derived units (N, Pa, J, W, Hz, V, …) ───────────\n");
    for u in SI_DERIVED_UNITS {
        // Unprefixed form first.
        let factor_str = format_f64_as_decimal(u.factor);
        out.push_str(&format!(
            "pub unit {} : {} = {}\n",
            u.name, u.dimension, factor_str
        ));

        // Prefixed variants — resolve each prefix symbol's factor from
        // SI_PREFIXES, multiply with the unit's base factor, and emit.
        for prefix in u.prefix_combos {
            let prefix_factor = SI_PREFIXES
                .iter()
                .find(|(sym, _)| *sym == *prefix)
                .unwrap_or_else(|| {
                    panic!(
                        "SI_DERIVED_UNITS entry `{}` references unknown prefix `{}`",
                        u.name, prefix
                    )
                })
                .1;
            let combined = prefix_factor * u.factor;
            let combined_str = format_f64_as_decimal(combined);
            out.push_str(&format!(
                "pub unit {}{} : {} = {}\n",
                prefix, u.name, u.dimension, combined_str
            ));
        }
        out.push('\n');
    }

    out
}

/// Format a power of 10 as a plain decimal string.
///
/// - `n >= 0` → `"1" + n zeros` (e.g., 3 → "1000", 0 → "1")
/// - `n < 0` → `"0." + (|n|-1) zeros + "1"` (e.g., -3 → "0.001")
///
/// Used instead of `f64` formatting because the tree-sitter grammar
/// (`/\d+(\.\d+)?/`) does not accept scientific notation, and f64
/// can't faithfully represent 1e-30 as a plain decimal anyway.
fn format_power_of_ten(n: i32) -> String {
    if n == 0 {
        "1".to_string()
    } else if n > 0 {
        let mut s = String::with_capacity(n as usize + 1);
        s.push('1');
        for _ in 0..n {
            s.push('0');
        }
        s
    } else {
        // e.g. n = -3 → "0.001"
        let zeros = (-n - 1) as usize;
        let mut s = String::with_capacity(zeros + 3);
        s.push_str("0.");
        for _ in 0..zeros {
            s.push('0');
        }
        s.push('1');
        s
    }
}

/// Format an arbitrary positive f64 as a plain decimal literal.
///
/// Tree-sitter's `number_literal` grammar `/\d+(\.\d+)?/` does not accept
/// scientific notation, so Rust's default `{}`/`{:?}` formatting (which
/// switches to `e`-notation for very small or very large values) cannot
/// be used directly. This function picks a precision proportional to the
/// value's decimal exponent so the full 17-significant-digit round-trip
/// precision of f64 is preserved, then trims trailing zeros.
///
/// Guarantees:
/// - Input must be `> 0` and finite (SI factors are always positive).
/// - Output is a string of the form `\d+` or `\d+\.\d+` (matches grammar).
/// - Round-tripping through `str::parse::<f64>` recovers the original f64.
fn format_f64_as_decimal(x: f64) -> String {
    assert!(x.is_finite() && x > 0.0, "format_f64_as_decimal expects positive finite input, got {}", x);

    // Choose precision: 17 significant digits of f64 round-trip precision,
    // expressed relative to the value's decimal magnitude.
    let log = x.log10().floor() as i32;
    let precision: usize = if log >= 0 {
        // Integer part has (log+1) digits — subtract from 17.
        (17 - log - 1).max(0) as usize
    } else {
        // Fractional value: need |log| leading zeros + 17 significant digits.
        ((-log) as usize) + 17
    };

    let mut s = format!("{:.*}", precision, x);

    // Trim trailing zeros after the decimal point, then the point itself
    // if it's now the last character. Leaves whole numbers as bare digits.
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_power_of_ten_cases() {
        assert_eq!(format_power_of_ten(0), "1");
        assert_eq!(format_power_of_ten(1), "10");
        assert_eq!(format_power_of_ten(3), "1000");
        assert_eq!(format_power_of_ten(6), "1000000");
        assert_eq!(format_power_of_ten(-1), "0.1");
        assert_eq!(format_power_of_ten(-3), "0.001");
        assert_eq!(format_power_of_ten(-9), "0.000000001");
    }

    #[test]
    fn format_f64_round_trips_for_si_factors() {
        // Whole numbers — no decimal point.
        assert_eq!(format_f64_as_decimal(1.0), "1");
        assert_eq!(format_f64_as_decimal(1000.0), "1000");
        assert_eq!(format_f64_as_decimal(1e6), "1000000");
        assert_eq!(format_f64_as_decimal(100000.0), "100000");
        assert_eq!(format_f64_as_decimal(100.0), "100");

        // Round-trip property: parsing the output must recover the input.
        let cases = [
            1.0,
            0.001,
            1e-6,
            1e-9,
            1e-12,
            1.602176634e-19,
            std::f64::consts::PI / 30.0,
            0.45359237,
            1.602176634e-16, // keV
            1.602176634e-13, // MeV
        ];
        for original in cases {
            let s = format_f64_as_decimal(original);
            // Shape must match grammar: one or more digits, optional .digits.
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_digit() || c == '.'),
                "format output `{}` contains invalid chars",
                s
            );
            let parsed: f64 = s.parse().unwrap_or_else(|e| {
                panic!("failed to re-parse `{}`: {}", s, e)
            });
            assert_eq!(
                parsed.to_bits(),
                original.to_bits(),
                "round-trip failed for {}: got `{}` → {}",
                original,
                s,
                parsed
            );
        }
    }
}
