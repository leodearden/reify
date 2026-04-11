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
