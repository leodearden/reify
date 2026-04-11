//! SI prefix and derived-unit tables + `.ri` source generator.
//!
//! Provides const tables for the 20 power-of-1000 SI prefixes (quecto..quetta)
//! and the standard SI derived units (N, Pa, J, W, Hz, V, ohm, S, F, H, Wb,
//! T, lm, lx, Bq, Gy, Sv, C, eV, bar, mbar, rpm, rad_per_s, Pa_s), together
//! with a programmatic generator that emits Reify unit declarations at
//! `load_stdlib()` time.
