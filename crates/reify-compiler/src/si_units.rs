//! SI prefix and derived-unit tables + `.ri` source generator.
//!
//! Provides const tables for the 20 power-of-1000 SI prefixes (quecto..quetta)
//! and the standard SI derived units (N, Pa, J, W, Hz, V, ohm, S, F, H, Wb,
//! T, lm, lx, Bq, Gy, Sv, C, eV, bar, mbar, rpm, rad_per_s, Pa_s, sr), together
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

/// Discriminates which SI prefixes should be emitted for a unit or base.
///
/// - `All` — emit all 20 SI prefixes. Use for bases with no practical upper/lower
///   limit (e.g. metre, gram, second, ampere, mole).
/// - `Only(list)` — emit only the listed prefix symbols. Use for restricted bases
///   (e.g. kelvin → `&["n", "u", "m"]`) or for derived units where `&[]` means
///   "emit the unprefixed form only, no prefixed variants".
///
/// The empty-slice convention is therefore universal: `Only(&[])` unambiguously
/// means "no prefixed variants", which eliminates the prior inversion where `&[]`
/// on `SiPrefixBase` meant "all" and `&[]` on `SiDerivedUnit` meant "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixSet {
    /// Emit all 20 SI prefixes.
    All,
    /// Emit only the listed prefix symbols (empty slice ⇒ no prefixed variants).
    Only(&'static [&'static str]),
}

impl PrefixSet {
    /// Returns `true` if `sym` should be emitted for this `PrefixSet`.
    ///
    /// - `All` → always `true` (debug-asserts that `sym` is a known `SI_PREFIXES` symbol).
    /// - `Only(list)` → `list.contains(&sym)`.
    pub fn includes(&self, sym: &str) -> bool {
        match self {
            PrefixSet::All => {
                debug_assert!(
                    SI_PREFIXES.iter().any(|(s, _)| *s == sym),
                    "unknown prefix symbol: `{sym}`"
                );
                true
            }
            PrefixSet::Only(list) => list.contains(&sym),
        }
    }
}

/// One SI base-unit entry for the prefix-expansion generator.
///
/// - `name` — base symbol (e.g. `"m"`, `"g"`, `"K"`).
/// - `dimension` — PascalCase dimension name.
/// - `prefix_combos` — which SI prefixes to generate for this base.
///   `PrefixSet::All` means "emit all 20 prefixes" (unrestricted bases like m, g, s).
///   `PrefixSet::Only(list)` restricts generation to the listed prefixes, omitting
///   nonsensical large/small-scale combinations (e.g. quettakelvin, quectokelvin).
pub struct SiPrefixBase {
    pub name: &'static str,
    pub dimension: &'static str,
    pub prefix_combos: PrefixSet,
}

/// SI base units used by the prefix-expansion generator. Excludes unprefixed
/// bases themselves — those live in `stdlib/units.ri` (m, kg, s, rad, K).
///
/// Note: `g` is the mass prefix base (not `kg`) because SI prefixes can't
/// stack. The generator multiplies gram-prefix factors by 1e-3 and skips `k`
/// to avoid re-declaring `kg`.
///
/// `prefix_combos` is the engineering-relevant subset per base:
///   - `PrefixSet::All` — emit all 20 SI prefixes (m, g, s, A, mol have no
///     practical upper/lower limit in engineering use).
///   - `PrefixSet::Only(list)` — restrict to listed prefixes only
///     (K, cd, rad have practical ranges; larger/smaller combos are nonsensical).
pub const SI_PREFIX_BASES: &[SiPrefixBase] = &[
    SiPrefixBase {
        name: "m",
        dimension: "Length",
        prefix_combos: PrefixSet::All,
    },
    SiPrefixBase {
        name: "g",
        dimension: "Mass",
        prefix_combos: PrefixSet::All,
    },
    SiPrefixBase {
        name: "s",
        dimension: "Time",
        prefix_combos: PrefixSet::All,
    },
    SiPrefixBase {
        name: "A",
        dimension: "Current",
        prefix_combos: PrefixSet::All,
    },
    // Kelvin: cryogenics/quantum use nK/uK/mK; QK/qK are nonsensical.
    SiPrefixBase {
        name: "K",
        dimension: "Temperature",
        prefix_combos: PrefixSet::Only(&["n", "u", "m"]),
    },
    SiPrefixBase {
        name: "mol",
        dimension: "AmountOfSubstance",
        prefix_combos: PrefixSet::All,
    },
    // Candela: mcd/ucd used in photometry; sub-micro or super-kilo are unused.
    SiPrefixBase {
        name: "cd",
        dimension: "LuminousIntensity",
        prefix_combos: PrefixSet::Only(&["m", "u"]),
    },
    // Radian: mrad/urad/nrad for optics/precision; Qrad/qrad nonsensical.
    SiPrefixBase {
        name: "rad",
        dimension: "Angle",
        prefix_combos: PrefixSet::Only(&["m", "u", "n"]),
    },
];

/// One SI (or SI-factor-derived) derived-unit entry.
///
/// - `name` — unit symbol as written in Reify source (e.g. `"Pa"`, `"ohm"`, `"eV"`).
/// - `dimension` — PascalCase dimension name resolved via `type_resolution::resolve_dimension_type`.
/// - `factor` — multiplicative conversion to SI (e.g. `1.0` for Pa, `1.602176634e-19` for eV).
/// - `prefix_combos` — which SI prefixes to auto-generate for this unit.
///   Always `PrefixSet::Only(list)` — derived units never use `PrefixSet::All`
///   since no derived unit needs all 20 SI prefixes in engineering practice.
///   `PrefixSet::Only(&[])` means the unit is emitted in unprefixed form only
///   (no prefixed variants).
pub struct SiDerivedUnit {
    pub name: &'static str,
    pub dimension: &'static str,
    pub factor: f64,
    pub prefix_combos: PrefixSet,
}

/// The 25 SI and engineering-derived units (19 standard SI plus `eV`, `bar`,
/// `mbar`, `rpm`, `rad_per_s`, `Pa_s`).
///
/// Ordering matches the task spec. `prefix_combos` is `PrefixSet::Only(list)`
/// for all entries; the generator expands each into `<prefix><name>` declarations
/// with `prefix_factor * base_factor`. `PrefixSet::Only(&[])` means unprefixed only.
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
        prefix_combos: PrefixSet::Only(&["k", "M", "G"]),
    },
    SiDerivedUnit {
        name: "Pa",
        dimension: "Pressure",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["k", "M", "G"]),
    },
    SiDerivedUnit {
        name: "J",
        dimension: "Energy",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["m", "k", "M", "G"]),
    },
    SiDerivedUnit {
        name: "W",
        dimension: "Power",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["u", "m", "k", "M", "G"]),
    },
    SiDerivedUnit {
        name: "V",
        dimension: "Voltage",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["u", "m", "k"]),
    },
    SiDerivedUnit {
        name: "Hz",
        dimension: "Frequency",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["k", "M", "G", "T"]),
    },
    SiDerivedUnit {
        name: "ohm",
        dimension: "Resistance",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["m", "k", "M", "G"]),
    },
    SiDerivedUnit {
        name: "S",
        dimension: "Conductance",
        factor: 1.0,
        // nS/uS/mS common in electronics; pS/fS used in RF design and
        // leakage characterisation.
        prefix_combos: PrefixSet::Only(&["f", "p", "n", "u", "m"]),
    },
    SiDerivedUnit {
        name: "F",
        dimension: "Capacitance",
        factor: 1.0,
        // fF (femtofarad) standard in RF/IC design (parasitic caps, MEMS).
        prefix_combos: PrefixSet::Only(&["f", "p", "n", "u", "m"]),
    },
    SiDerivedUnit {
        name: "H",
        dimension: "Inductance",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["u", "m"]),
    },
    SiDerivedUnit {
        name: "Wb",
        dimension: "MagneticFlux",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["u", "m"]),
    },
    SiDerivedUnit {
        name: "T",
        dimension: "MagneticFluxDensity",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["u", "m"]),
    },
    SiDerivedUnit {
        name: "lm",
        dimension: "LuminousFlux",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "lx",
        dimension: "Illuminance",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "Bq",
        dimension: "Frequency",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "Gy",
        dimension: "AbsorbedDose",
        factor: 1.0,
        // uGy/mGy are standard in radiation dosimetry (medical imaging).
        prefix_combos: PrefixSet::Only(&["u", "m"]),
    },
    SiDerivedUnit {
        name: "Sv",
        dimension: "AbsorbedDose",
        factor: 1.0,
        // uSv/mSv are the common units in health physics and occupational dose.
        prefix_combos: PrefixSet::Only(&["u", "m"]),
    },
    SiDerivedUnit {
        name: "C",
        dimension: "Charge",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&["p", "n", "u", "m"]),
    },
    // Non-SI factor conversions.
    SiDerivedUnit {
        name: "eV",
        dimension: "Energy",
        factor: 1.602176634e-19,
        prefix_combos: PrefixSet::Only(&["k", "M", "G", "T"]),
    },
    SiDerivedUnit {
        name: "bar",
        dimension: "Pressure",
        factor: 100000.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "mbar",
        dimension: "Pressure",
        factor: 100.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    // rpm = 2π/60 rad·s⁻¹ = π/30.
    SiDerivedUnit {
        name: "rpm",
        dimension: "AngularVelocity",
        factor: std::f64::consts::PI / 30.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "rad_per_s",
        dimension: "AngularVelocity",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    SiDerivedUnit {
        name: "Pa_s",
        dimension: "DynamicViscosity",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
    },
    // sr (steradian) — SI unit of solid angle. Dimensionless like radian;
    // prefixed steradians (msr, usr) are not conventional, so no prefixed variants.
    SiDerivedUnit {
        name: "sr",
        dimension: "SolidAngle",
        factor: 1.0,
        prefix_combos: PrefixSet::Only(&[]),
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
    for base_entry in SI_PREFIX_BASES {
        let base = base_entry.name;
        let dim = base_entry.dimension;
        // Gram is special: kg is the SI base, so each gram-prefix factor
        // must be multiplied by 1e-3 (since g = 0.001 kg), and the `k`
        // prefix must be skipped to avoid re-declaring kg.
        let base_factor_exp: i32 = if base == "g" { -3 } else { 0 };

        for (prefix, prefix_factor) in SI_PREFIXES {
            // Skip `k` + `g` → would produce kg (already SI base).
            if base == "g" && *prefix == "k" {
                continue;
            }
            // Only emit this prefix if the base's PrefixSet includes it.
            if !base_entry.prefix_combos.includes(prefix) {
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

        // Prefixed variants — iterate SI_PREFIXES and emit each prefix that
        // u.prefix_combos.includes(sym). This mirrors the base-unit loop and
        // correctly handles both PrefixSet::All and PrefixSet::Only without
        // silently producing zero results if a unit ever carries PrefixSet::All.
        // Note: emits prefixed variants in SI_PREFIXES order (ascending magnitude),
        // regardless of the order symbols appear in PrefixSet::Only.
        for (prefix, prefix_factor) in SI_PREFIXES {
            if !u.prefix_combos.includes(prefix) {
                continue;
            }
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
    assert!(
        x.is_finite() && x > 0.0,
        "format_f64_as_decimal expects positive finite input, got {}",
        x
    );

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
                s.chars().all(|c| c.is_ascii_digit() || c == '.'),
                "format output `{}` contains invalid chars",
                s
            );
            let parsed: f64 = s
                .parse()
                .unwrap_or_else(|e| panic!("failed to re-parse `{}`: {}", s, e));
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

    /// Suggestion 5: extend round-trip coverage to the extreme SI prefix range
    /// (quecto = 1e-30, exa = 1e18, quetta = 1e30). Exercises the high-precision
    /// code path where `log >= 17` would clamp precision to 0 — these values
    /// are powers of ten so their decimal representation is exact.
    #[test]
    fn format_f64_round_trips_extreme_si_range() {
        let extreme_cases = [
            1e-30_f64, // quecto prefix (qm etc.)
            1e18_f64,  // exa prefix (Em etc.)
            1e30_f64,  // quetta prefix (Qm etc.)
        ];
        for original in extreme_cases {
            let s = format_f64_as_decimal(original);
            assert!(
                s.chars().all(|c| c.is_ascii_digit() || c == '.'),
                "output `{}` contains chars outside \\d+(.\\d+)? for input {}",
                s,
                original
            );
            let parsed: f64 = s.parse().unwrap_or_else(|e| {
                panic!("failed to re-parse `{}` (from {}): {}", s, original, e)
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

    /// Suggestion 4: defensive check that every `prefix_combos` entry in
    /// `SI_DERIVED_UNITS` references a symbol that actually exists in
    /// `SI_PREFIXES`. A typo here would only surface at `load_stdlib()` time
    /// (inside `OnceLock::get_or_init`) crashing every downstream user.
    ///
    /// Uses `match` with an explicit `PrefixSet::All` arm so the compiler
    /// enforces exhaustiveness if a new variant is ever added.
    #[test]
    fn all_derived_unit_prefix_combos_are_valid_si_prefix_symbols() {
        let known: std::collections::HashSet<&str> =
            SI_PREFIXES.iter().map(|(sym, _)| *sym).collect();
        for unit in SI_DERIVED_UNITS {
            match unit.prefix_combos {
                PrefixSet::All => {} // All SI_PREFIXES symbols are valid by construction.
                PrefixSet::Only(list) => {
                    for prefix in list {
                        assert!(
                            known.contains(prefix),
                            "SI_DERIVED_UNITS entry `{}` has unknown prefix `{}` in prefix_combos",
                            unit.name,
                            prefix
                        );
                    }
                }
            }
        }
    }

    /// Regression guard: `SI_PREFIXES` entries must be in strictly ascending
    /// order of magnitude (factor). The ordering doc-comment on the
    /// derived-unit prefix loop states that prefixed variants are emitted in
    /// `SI_PREFIXES` declaration order, so this test locks that invariant.
    /// A future reordering of `SI_PREFIXES` would be caught here.
    #[test]
    fn si_prefixes_are_in_ascending_magnitude_order() {
        for window in SI_PREFIXES.windows(2) {
            let (sym_a, factor_a) = window[0];
            let (sym_b, factor_b) = window[1];
            assert!(
                factor_a < factor_b,
                "SI_PREFIXES out of order: `{}` ({}) must be < `{}` ({})",
                sym_a,
                factor_a,
                sym_b,
                factor_b
            );
        }
    }

    /// `PrefixSet::All.includes()` should panic (debug_assert) when the caller
    /// passes a symbol that doesn't exist in `SI_PREFIXES`. In production this
    /// would be a silent logic error; the debug_assert catches it early.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unknown prefix symbol")]
    fn debug_assert_rejects_unknown_prefix_in_all() {
        // "bogus" is not in SI_PREFIXES — the debug_assert in the All arm
        // must fire. Without the guard this call returns `true` silently.
        let _ = PrefixSet::All.includes("bogus");
    }

    /// Defensive check that every `prefix_combos` entry in `SI_PREFIX_BASES`
    /// references a symbol that actually exists in `SI_PREFIXES`. A typo here
    /// would cause the generator to silently skip that prefix at `load_stdlib()`
    /// time, producing a subtly incomplete unit set.
    ///
    /// Uses `match` with an explicit `PrefixSet::All` arm so the compiler
    /// enforces exhaustiveness if a new variant is ever added.
    #[test]
    fn all_prefix_base_prefix_combos_are_valid_si_prefix_symbols() {
        let known: std::collections::HashSet<&str> =
            SI_PREFIXES.iter().map(|(sym, _)| *sym).collect();
        for base in SI_PREFIX_BASES {
            match base.prefix_combos {
                PrefixSet::All => {} // All SI_PREFIXES symbols are valid by construction.
                PrefixSet::Only(list) => {
                    for prefix in list {
                        assert!(
                            known.contains(prefix),
                            "SI_PREFIX_BASES entry `{}` has unknown prefix `{}` in prefix_combos",
                            base.name,
                            prefix
                        );
                    }
                }
            }
        }
    }
}
