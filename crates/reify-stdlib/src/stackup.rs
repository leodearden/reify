//! Tolerance stack-up builtins: Contributor value-shape builders.
//! T1 Phase 1 — math arms in T2/T5.

use std::collections::BTreeMap;

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector};
use reify_ir::Value;

use crate::helpers::{sanitize_value, validate_dimensioned_scalar};

/// Evaluate a tolerance stack-up builtin by name.
///
/// Returns `Some(value)` if the name is a recognised stack-up function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_stackup(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "contributor" => contributor(args),
        "contributor_asym" => contributor_asym(args),
        "stackup_worst_case" => stackup_worst_case(args),
        "stackup_rss" => stackup_rss(args),
        "monte_carlo_stackup" => monte_carlo_stackup(args),
        _ => return None,
    })
}

mod rng;

// --- private helpers ---

/// Validate that `v` is a LENGTH scalar with a finite `si_value`.
fn len_scalar(v: &Value) -> Option<f64> {
    validate_dimensioned_scalar(v, DimensionVector::LENGTH)
}

/// Parse a sign value: accepts only `Value::Int(1)` or `Value::Int(-1)`.
fn parse_sign(v: &Value) -> Option<i64> {
    match v {
        Value::Int(1) => Some(1),
        Value::Int(-1) => Some(-1),
        _ => None,
    }
}

// --- stackup error classification ---

/// Internal error classification for tolerance chain validation.
///
/// Each variant corresponds to one of the §4.4 diagnostic codes returned by
/// [`parse_chain_checked`] and mapped to a [`Diagnostic`] by
/// [`chain_error_to_diagnostic`]:
/// - `EmptyChain`  → `E_StackupEmptyChain`  / `DiagnosticCode::StackupEmptyChain`
/// - `DimMismatch` → `E_StackupDimMismatch` / `DiagnosticCode::StackupDimMismatch`
/// - `BadSign`     → `E_StackupBadSign`     / `DiagnosticCode::StackupBadSign`
///
/// Note: `E_StackupBadSamples` / `DiagnosticCode::StackupBadSamples` is classified
/// separately in [`diagnose`] by directly inspecting `args[1]` for
/// `monte_carlo_stackup`, because `parse_chain_checked` only validates the chain
/// argument (not `samples`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackupError {
    /// Chain is empty (`Value::List` with zero elements) or not a list.
    EmptyChain,
    /// A contributor entry is not a `Value::Map`, or a required LENGTH field
    /// (`nominal`, `plus_tol`, `minus_tol`) is missing or not a finite LENGTH scalar.
    DimMismatch,
    /// A contributor's `sign` field is not `Value::Int(1)` or `Value::Int(-1)`.
    BadSign,
}

// --- builder functions ---

fn contributor(args: &[Value]) -> Value {
    if !matches!(args.len(), 2 | 3) {
        return Value::Undef;
    }
    let nominal_si = match len_scalar(&args[0]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let tol_si = match len_scalar(&args[1]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let sign: i64 = if args.len() == 3 {
        match parse_sign(&args[2]) {
            Some(s) => s,
            None => return Value::Undef,
        }
    } else {
        1
    };
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let tol = Value::Scalar { si_value: tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, tol.clone(), tol, sign, "Normal")
}

fn contributor_asym(args: &[Value]) -> Value {
    if !matches!(args.len(), 3..=5) {
        return Value::Undef;
    }
    let nominal_si = match len_scalar(&args[0]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let plus_tol_si = match len_scalar(&args[1]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let minus_tol_si = match len_scalar(&args[2]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let sign: i64 = if args.len() >= 4 {
        match parse_sign(&args[3]) {
            Some(s) => s,
            None => return Value::Undef,
        }
    } else {
        1
    };
    let dist_variant: &str = if args.len() == 5 {
        match parse_distribution(&args[4]) {
            Some(v) => v,
            None => return Value::Undef,
        }
    } else {
        "Normal"
    };
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let plus_tol = Value::Scalar { si_value: plus_tol_si, dimension: DimensionVector::LENGTH };
    let minus_tol = Value::Scalar { si_value: minus_tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, plus_tol, minus_tol, sign, dist_variant)
}

fn parse_distribution(v: &Value) -> Option<&str> {
    match v {
        Value::Enum { type_name, variant } if type_name == "Distribution" => {
            match variant.as_str() {
                s @ ("Normal" | "Uniform" | "Triangular") => Some(s),
                _ => None,
            }
        }
        _ => None,
    }
}

// --- stackup math helpers ---

/// Build a LENGTH scalar result value, collapsing NaN/inf to [`Value::Undef`] via
/// [`sanitize_value`].
fn len_result(si: f64) -> Value {
    sanitize_value(Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH })
}

/// Distribution variant for a contributor's random deviation.
#[derive(Clone, Copy, Debug)]
enum Distribution {
    Normal,
    Uniform,
    Triangular,
}

/// Extracted numeric data from a single contributor map entry.
struct ContributorData {
    nominal: f64,
    plus_tol: f64,
    minus_tol: f64,
    sign: i64,
    distribution: Distribution,
}

/// Parse a chain of contributors from a `Value`, returning a typed `StackupError`
/// when any validation step fails.
///
/// Error mapping:
/// - `v` is not a `Value::List`, or the list is empty → `StackupError::EmptyChain`
/// - any element is not a `Value::Map`, or a required LENGTH field is invalid → `StackupError::DimMismatch`
/// - any contributor's `sign` field is not `Value::Int(+1/-1)` → `StackupError::BadSign`
fn parse_chain_checked(v: &Value) -> Result<Vec<ContributorData>, StackupError> {
    let items = match v {
        Value::List(items) if !items.is_empty() => items,
        _ => return Err(StackupError::EmptyChain),
    };
    let mut chain = Vec::with_capacity(items.len());
    for item in items {
        let map = match item {
            Value::Map(m) => m,
            _ => return Err(StackupError::DimMismatch),
        };
        let nominal = len_scalar(
            map.get(&Value::String("nominal".into()))
                .ok_or(StackupError::DimMismatch)?,
        )
        .ok_or(StackupError::DimMismatch)?;
        let plus_tol = len_scalar(
            map.get(&Value::String("plus_tol".into()))
                .ok_or(StackupError::DimMismatch)?,
        )
        .ok_or(StackupError::DimMismatch)?;
        let minus_tol = len_scalar(
            map.get(&Value::String("minus_tol".into()))
                .ok_or(StackupError::DimMismatch)?,
        )
        .ok_or(StackupError::DimMismatch)?;
        let sign = parse_sign(
            map.get(&Value::String("sign".into()))
                .ok_or(StackupError::BadSign)?,
        )
        .ok_or(StackupError::BadSign)?;
        let distribution = match map.get(&Value::String("distribution".into())) {
            Some(Value::Enum { type_name, variant }) if type_name.as_str() == "Distribution" => {
                match variant.as_str() {
                    "Normal" => Distribution::Normal,
                    "Uniform" => Distribution::Uniform,
                    "Triangular" => Distribution::Triangular,
                    _ => return Err(StackupError::DimMismatch), // unrecognized variant
                }
            }
            None => Distribution::Normal, // default when key absent
            _ => return Err(StackupError::DimMismatch), // invalid key type
        };
        chain.push(ContributorData { nominal, plus_tol, minus_tol, sign, distribution });
    }
    Ok(chain)
}

/// Parse a chain of contributors from a `Value`.
///
/// Delegates to [`parse_chain_checked`]; returns `None` on any error so the
/// builtins retain their existing `Value::Undef` error behaviour unchanged.
fn parse_chain(v: &Value) -> Option<Vec<ContributorData>> {
    parse_chain_checked(v).ok()
}

/// Maps a chain-validation error to its corresponding [`Diagnostic`].
///
/// Called by [`diagnose`] for all three stackup math builtins — this single
/// source of truth eliminates the otherwise-duplicated EmptyChain/DimMismatch/
/// BadSign message strings.
fn chain_error_to_diagnostic(e: StackupError) -> Option<Diagnostic> {
    match e {
        StackupError::EmptyChain => Some(
            Diagnostic::error("E_StackupEmptyChain: tolerance chain must be non-empty")
                .with_code(DiagnosticCode::StackupEmptyChain),
        ),
        StackupError::DimMismatch => Some(
            Diagnostic::error(
                "E_StackupDimMismatch: contributor field must be a finite LENGTH scalar",
            )
            .with_code(DiagnosticCode::StackupDimMismatch),
        ),
        StackupError::BadSign => Some(
            Diagnostic::error(
                "E_StackupBadSign: contributor sign must be Int(+1) or Int(-1)",
            )
            .with_code(DiagnosticCode::StackupBadSign),
        ),
    }
}

/// Pure classifier: given the name and args of a stdlib call that returned
/// `Value::Undef`, determine whether this was a recognised stackup builtin
/// error and, if so, which `Diagnostic` (with `Severity::Error`) to emit.
///
/// Returns `None` for:
/// - unrecognised function names (non-stackup builtins, user functions, etc.)
/// - valid input to a stackup builtin (no error to report)
///
/// Returns `Some(Diagnostic)` for:
/// - `E_StackupEmptyChain` — empty or non-list chain arg
/// - `E_StackupDimMismatch` — contributor not a Map, or required field not a
///   finite LENGTH scalar
/// - `E_StackupBadSign` — contributor sign not Int(+1/-1)
/// - `E_StackupBadSamples` — `monte_carlo_stackup` samples ≤ 0 (not a positive Int)
///
/// **Not diagnosed** (no §4.4 code; `Value::Undef` propagates silently):
/// - `monte_carlo_stackup` `seed` arg is not `Value::Int`
/// - `monte_carlo_stackup` `sigma_level` is non-positive, NaN, or dimensioned
/// - `monte_carlo_stackup` `spec_min`/`spec_max` are asymmetrically present or inverted
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    // Only the stackup math builtins can produce these diagnostics.
    // contributor / contributor_asym return Undef too, but they're builder
    // functions — argument validation there is the caller's responsibility
    // and has no PRD §4.4 diagnostic code.
    match name {
        "stackup_worst_case" | "stackup_rss" => {
            if args.is_empty() {
                return None; // arity error handled elsewhere
            }
            parse_chain_checked(&args[0]).err().and_then(chain_error_to_diagnostic)
        }
        "monte_carlo_stackup" => {
            if args.len() < 3 {
                return None; // arity error handled elsewhere
            }
            // Check chain first; if invalid, surface the chain error.
            if let Err(e) = parse_chain_checked(&args[0]) {
                return chain_error_to_diagnostic(e);
            }
            // Chain is valid. Check samples arg.
            match &args[1] {
                Value::Int(n) if *n > 0 => None, // valid
                _ => Some(
                    Diagnostic::error("E_StackupBadSamples: samples must be a positive integer")
                        .with_code(DiagnosticCode::StackupBadSamples),
                ),
            }
        }
        _ => None, // not a stackup math builtin
    }
}

/// Compute worst-case (extreme-value) stack-up bounds for a contributor chain.
///
/// The bound computation is **sign-aware**: for a gap G = Σ sign_i·X_i, each
/// contributor's plus/minus tolerance is routed to the correct side depending on
/// its sign:
///
/// - `sign=+1`: `plus_tol` widens the maximum; `minus_tol` widens the minimum.
/// - `sign=-1`: the contributor is subtracted, so `minus_tol` widens the maximum
///   (subtracting a smaller value increases G) and `plus_tol` widens the minimum.
///
/// For symmetric tolerances (`plus_tol == minus_tol`) the result is identical to a
/// sign-agnostic sum, so all symmetric-chain tests are unaffected.
fn stackup_worst_case(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let chain = match parse_chain(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let mut gap_nominal = 0.0_f64;
    // sum_max: tolerance budget that widens the gap toward its maximum.
    // sum_min: tolerance budget that widens the gap toward its minimum.
    let mut sum_max = 0.0_f64;
    let mut sum_min = 0.0_f64;
    for c in &chain {
        gap_nominal += c.sign as f64 * c.nominal;
        if c.sign > 0 {
            sum_max += c.plus_tol;
            sum_min += c.minus_tol;
        } else {
            // sign = -1: subtracted contributor — its minus_tol raises the gap (max side)
            //            and its plus_tol lowers the gap (min side).
            sum_max += c.minus_tol;
            sum_min += c.plus_tol;
        }
    }
    let wc_max = gap_nominal + sum_max;
    let wc_min = gap_nominal - sum_min;
    // worst_case_band = half-width of the worst-case interval = (sum_max + sum_min) / 2
    let wc_band = (sum_max + sum_min) / 2.0;
    let mut m = BTreeMap::new();
    m.insert(Value::String("nominal_gap".into()),     len_result(gap_nominal));
    m.insert(Value::String("worst_case_band".into()), len_result(wc_band));
    m.insert(Value::String("worst_case_max".into()),  len_result(wc_max));
    m.insert(Value::String("worst_case_min".into()),  len_result(wc_min));
    Value::Map(m)
}

/// Parse a sigma_level argument: must be a positive, finite, dimensionless numeric.
///
/// Accepted variants: `Value::Int(n)`, `Value::Real(r)`, or a dimensionless
/// `Value::Scalar`. Dimensioned scalars (e.g. LENGTH), <= 0, and non-finite
/// values all return `None`.
fn parse_sigma_level(v: &Value) -> Option<f64> {
    let sigma = match v {
        Value::Int(n) => *n as f64,
        Value::Real(r) => *r,
        Value::Scalar { si_value, dimension }
            if *dimension == DimensionVector::DIMENSIONLESS =>
        {
            *si_value
        }
        _ => return None,
    };
    if sigma.is_finite() && sigma > 0.0 {
        Some(sigma)
    } else {
        None
    }
}

fn stackup_rss(args: &[Value]) -> Value {
    if !(1..=2).contains(&args.len()) {
        return Value::Undef;
    }
    let chain = match parse_chain(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let sigma_level: f64 = if args.len() == 2 {
        match parse_sigma_level(&args[1]) {
            Some(s) => s,
            None => return Value::Undef,
        }
    } else {
        3.0 // PRD §3 default: +/-3σ mechanical convention
    };
    let mut gap_nominal = 0.0_f64;
    let mut sum_t_sq = 0.0_f64;
    for c in &chain {
        gap_nominal += c.sign as f64 * c.nominal;
        let t_i = (c.plus_tol + c.minus_tol) / 2.0; // half-band per contributor
        sum_t_sq += (t_i / sigma_level).powi(2);
    }
    // rss_sigma = sqrt(Σ(t_i/σ)²); rss_band = σ·rss_sigma = sqrt(Σt_i²) — sigma-invariant
    let rss_sigma = sum_t_sq.sqrt();
    let rss_band = sigma_level * rss_sigma;
    let rss_min = gap_nominal - rss_band;
    let rss_max = gap_nominal + rss_band;
    let mut m = BTreeMap::new();
    m.insert(Value::String("nominal_gap".into()), len_result(gap_nominal));
    m.insert(Value::String("rss_band".into()),    len_result(rss_band));
    m.insert(Value::String("rss_max".into()),     len_result(rss_max));
    m.insert(Value::String("rss_min".into()),     len_result(rss_min));
    m.insert(Value::String("rss_sigma".into()),   len_result(rss_sigma));
    m.insert(Value::String("sigma_level".into()), Value::Real(sigma_level));
    Value::Map(m)
}

/// Parse an optional length bound: `Value::Undef` → absent (`Some(None)`),
/// a valid finite LENGTH scalar → present (`Some(Some(si_value))`),
/// anything else → invalid (`None`).
fn parse_optional_length(v: &Value) -> Option<Option<f64>> {
    match v {
        Value::Undef => Some(None),
        _ => len_scalar(v).map(Some),
    }
}

/// Parse spec bounds `(spec_min, spec_max)` from two arg slots.
///
/// Returns `Ok(Some((min, max)))` when both are present and `min <= max`,
/// `Ok(None)` when both are absent (Undef), or `Err(())` when only one is
/// present, either slot is an invalid (non-length, non-Undef) value, or
/// `spec_min > spec_max` (inverted bounds are a user error, not a valid
/// empty range — return Undef rather than silently yielding yield_fraction=0).
fn parse_spec_bounds(min_arg: &Value, max_arg: &Value) -> Result<Option<(f64, f64)>, ()> {
    match (parse_optional_length(min_arg), parse_optional_length(max_arg)) {
        (Some(Some(lo)), Some(Some(hi))) => {
            if lo <= hi { Ok(Some((lo, hi))) } else { Err(()) }
        }
        (Some(None), Some(None)) => Ok(None),
        (None, _) | (_, None) => Err(()), // invalid spec value
        _ => Err(()), // asymmetric present/absent
    }
}

/// Stub implementation for Monte Carlo stack-up (step-2: validation + key scaffold).
/// Real sampling math is added in step-4.
fn monte_carlo_stackup(args: &[Value]) -> Value {
    // (a) arity guard: 3..=6 args only
    if !(3..=6).contains(&args.len()) {
        return Value::Undef;
    }

    // (b) parse chain
    let chain = match parse_chain(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };

    // (c) parse samples: must be Value::Int with n > 0
    let n_samples: usize = match &args[1] {
        Value::Int(n) if *n > 0 => *n as usize,
        _ => return Value::Undef,
    };

    // (d) parse seed: must be Value::Int (any i64)
    let seed_i64: i64 = match &args[2] {
        Value::Int(s) => *s,
        _ => return Value::Undef,
    };
    let seed_u64: u64 = seed_i64 as u64;

    // (e)+(f) parse spec bounds and sigma_level based on arity
    let (spec_bounds, sigma_level): (Option<(f64, f64)>, f64) = match args.len() {
        3 => (None, 3.0),
        4 => {
            // args[3] = sigma_level (Undef → default 3.0)
            let sl = if matches!(args[3], Value::Undef) {
                3.0
            } else {
                match parse_sigma_level(&args[3]) {
                    Some(s) => s,
                    None => return Value::Undef,
                }
            };
            (None, sl)
        }
        5 => {
            // args[3] = spec_min, args[4] = spec_max; default sigma_level=3.0
            match parse_spec_bounds(&args[3], &args[4]) {
                Ok(bounds) => (bounds, 3.0),
                Err(()) => return Value::Undef,
            }
        }
        6 => {
            // args[3] = spec_min, args[4] = spec_max, args[5] = sigma_level
            let bounds = match parse_spec_bounds(&args[3], &args[4]) {
                Ok(b) => b,
                Err(()) => return Value::Undef,
            };
            let sl = if matches!(args[5], Value::Undef) {
                3.0
            } else {
                match parse_sigma_level(&args[5]) {
                    Some(s) => s,
                    None => return Value::Undef,
                }
            };
            (bounds, sl)
        }
        _ => unreachable!(), // arity guard above
    };

    // --- Core Monte Carlo sampling ---

    // Compute nominal gap and per-contributor half-bands
    let mut gap_nominal = 0.0_f64;
    for c in &chain {
        gap_nominal += c.sign as f64 * c.nominal;
    }

    // Allocate per-trial gap vector, initialised to the nominal gap
    let mut gaps: Vec<f64> = vec![gap_nominal; n_samples];

    // Outer: contributor index ascending; inner: draw index ascending.
    // Per-contributor sub-seed via SplitMix64-XOR to avoid Box–Muller spare
    // leakage across contributor boundaries (design decision §2 in plan).
    let mut any_sampled = false;
    for (i, c) in chain.iter().enumerate() {
        let t_i = (c.plus_tol + c.minus_tol) / 2.0; // symmetric half-band
        if t_i == 0.0 {
            continue; // zero tolerance → no contribution to variance
        }
        any_sampled = true;
        let sub_seed = seed_u64 ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
        let mut rng = rng::Xoshiro256StarStar::from_seed(sub_seed);
        let sign_f = c.sign as f64;
        for gap in &mut gaps {
            let deviation: f64 = match c.distribution {
                Distribution::Normal     => rng.sample_normal(t_i / sigma_level),
                Distribution::Uniform    => rng.sample_uniform_sym(t_i),
                Distribution::Triangular => rng.sample_triangular_sym(t_i),
            };
            *gap += sign_f * deviation;
        }
    }

    // INV-5: degenerate short-circuit — all tolerances are zero, no sampling occurred.
    // Return exact values to avoid FP accumulation errors in mean computation.
    if !any_sampled {
        let yf = spec_bounds.map(|(spec_min, spec_max)| {
            if gap_nominal >= spec_min && gap_nominal <= spec_max { 1.0_f64 } else { 0.0_f64 }
        });
        return build_mc_map(
            gap_nominal,
            len_result(gap_nominal), // mc_mean  = nominal_gap (exact)
            len_result(0.0),         // mc_sigma = 0.0 (exact)
            len_result(gap_nominal), // mc_min   = nominal_gap (exact)
            len_result(gap_nominal), // mc_max   = nominal_gap (exact)
            len_result(gap_nominal), // mc_p_low  = nominal_gap (exact)
            len_result(gap_nominal), // mc_p_high = nominal_gap (exact)
            n_samples,
            seed_i64,
            yf,
        );
    }

    // Compute statistics
    let n = n_samples as f64;
    let mc_mean: f64 = gaps.iter().sum::<f64>() / n;

    let mc_sigma: f64 = if n_samples > 1 {
        let ss: f64 = gaps.iter().map(|&g| (g - mc_mean).powi(2)).sum();
        (ss / (n - 1.0)).sqrt()
    } else {
        0.0
    };

    let mc_min: f64 = gaps.iter().cloned().fold(f64::INFINITY,     f64::min);
    let mc_max: f64 = gaps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // R-7 linear interpolation quantile on a sorted copy
    let mut sorted = gaps.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    debug_assert!(sorted.iter().all(|v| v.is_finite()),
        "MC: gap sample unexpectedly non-finite — PRNG or chain design error");

    let quantile = |p: f64| -> f64 {
        let h   = (n_samples as f64 - 1.0) * p;
        let lo  = h.floor() as usize;
        let hi  = h.ceil()  as usize;
        sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
    };

    const P_LOW:  f64 = 0.00135;  // ≈ Φ(−3)
    const P_HIGH: f64 = 0.99865;  // ≈ Φ(+3)
    let mc_p_low  = quantile(P_LOW);
    let mc_p_high = quantile(P_HIGH);

    // Optional yield fraction
    let yield_fraction: Option<f64> = spec_bounds.map(|(spec_min, spec_max)| {
        let n_within = gaps.iter().filter(|&&g| g >= spec_min && g <= spec_max).count();
        n_within as f64 / n_samples as f64
    });

    build_mc_map(
        gap_nominal,
        len_result(mc_mean),
        len_result(mc_sigma),
        len_result(mc_min),
        len_result(mc_max),
        len_result(mc_p_low),
        len_result(mc_p_high),
        n_samples,
        seed_i64,
        yield_fraction,
    )
}

/// Assemble the Monte Carlo result `Value::Map`.
#[allow(clippy::too_many_arguments)]
fn build_mc_map(
    gap_nominal: f64,
    mc_mean: Value,
    mc_sigma: Value,
    mc_min: Value,
    mc_max: Value,
    mc_p_low: Value,
    mc_p_high: Value,
    samples: usize,
    seed: i64,
    yield_fraction: Option<f64>,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("mc_max".into()),     mc_max);
    m.insert(Value::String("mc_mean".into()),    mc_mean);
    m.insert(Value::String("mc_min".into()),     mc_min);
    m.insert(Value::String("mc_p_high".into()),  mc_p_high);
    m.insert(Value::String("mc_p_low".into()),   mc_p_low);
    m.insert(Value::String("mc_sigma".into()),   mc_sigma);
    m.insert(Value::String("nominal_gap".into()), len_result(gap_nominal));
    m.insert(Value::String("samples".into()),    Value::Int(samples as i64));
    m.insert(Value::String("seed".into()),       Value::Int(seed));
    if let Some(yf) = yield_fraction {
        m.insert(Value::String("mc_yield_fraction".into()), Value::Real(yf));
    }
    Value::Map(m)
}

fn make_contributor_map(
    nominal: Value,
    plus_tol: Value,
    minus_tol: Value,
    sign: i64,
    dist_variant: &str,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("nominal".into()), nominal);
    m.insert(Value::String("plus_tol".into()), plus_tol);
    m.insert(Value::String("minus_tol".into()), minus_tol);
    m.insert(Value::String("sign".into()), Value::Int(sign));
    m.insert(
        Value::String("distribution".into()),
        Value::Enum { type_name: "Distribution".into(), variant: dist_variant.into() },
    );
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    fn len(si: f64) -> Value {
        Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH }
    }

    fn expect_map(v: Option<Value>) -> std::collections::BTreeMap<Value, Value> {
        match v {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        }
    }

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_stackup("foo", &[]).is_none());
    }

    #[test]
    fn eval_builtin_contributor_returns_map() {
        let m = match crate::eval_builtin("contributor", &[len(0.010), len(0.0001)]) {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
    }

    #[test]
    fn eval_builtin_unknown_stackup_name_returns_undef() {
        assert!(crate::eval_builtin("stackup_xyz_unknown", &[]).is_undef());
    }

    #[test]
    fn eval_builtin_zero_arg_stackup_math_returns_undef() {
        // stackup_worst_case and stackup_rss are fully implemented; 0-arg calls hit the arity
        // guard and still return Undef.  monte_carlo_stackup is the only remaining stub.
        assert!(crate::eval_builtin("stackup_worst_case", &[]).is_undef());
        assert!(crate::eval_builtin("stackup_rss", &[]).is_undef());
        assert!(crate::eval_builtin("monte_carlo_stackup", &[]).is_undef());
    }

    #[test]
    fn contributor_asym_validation_returns_undef() {
        let nom = len(0.010);
        let pt = len(0.0001);
        let mt = len(0.00005);

        // (a) arity: 0/1/2/6 args
        assert!(eval_stackup("contributor_asym", &[]).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", std::slice::from_ref(&nom)).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone()]).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
            nom.clone(),
        ]).unwrap().is_undef());
        // (b) nominal wrong dim
        let force = Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE };
        assert!(eval_stackup("contributor_asym", &[force, pt.clone(), mt.clone()]).unwrap().is_undef());
        // (c) plus_tol is Value::Int (not Scalar)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), Value::Int(1), mt.clone()]).unwrap().is_undef());
        // (d) plus_tol has NaN si_value
        let nan = Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH };
        assert!(eval_stackup("contributor_asym", &[nom.clone(), nan, mt.clone()]).unwrap().is_undef());
        // (e) sign=Int(0)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone(), mt.clone(), Value::Int(0)]).unwrap().is_undef());
        // (f) sign=Real(1.0)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone(), mt.clone(), Value::Real(1.0)]).unwrap().is_undef());
        // (g) distribution is String (not Enum)
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1), Value::String("Normal".into()),
        ]).unwrap().is_undef());
        // (h) distribution Enum with wrong type_name
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Material".into(), variant: "Steel".into() },
        ]).unwrap().is_undef());
        // (i) distribution Enum with unrecognised variant
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Distribution".into(), variant: "Lognormal".into() },
        ]).unwrap().is_undef());
    }

    #[test]
    fn contributor_asym_4arg_accepts_explicit_sign() {
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(-1),
        ]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(-1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    #[test]
    fn contributor_asym_5arg_accepts_distribution_uniform() {
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Uniform".into() };
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(1), dist,
        ]));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Uniform".into() }
        );
    }

    #[test]
    fn contributor_asym_5arg_accepts_distribution_triangular() {
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Triangular".into() };
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(1), dist,
        ]));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Triangular".into() }
        );
    }

    #[test]
    fn contributor_asym_3arg_returns_map_with_asymmetric_tols() {
        let nominal = len(0.010);   // 10mm
        let plus_tol = len(0.0001); // 0.1mm
        let minus_tol = len(0.00005); // 0.05mm
        let m = expect_map(eval_stackup("contributor_asym", &[nominal, plus_tol, minus_tol]));

        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.00005));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    #[test]
    fn contributor_validation_returns_undef() {
        let nom = len(0.010);
        let tol = len(0.0001);

        // (a) zero args
        assert!(eval_stackup("contributor", &[]).unwrap().is_undef());
        // (b) one arg
        assert!(eval_stackup("contributor", std::slice::from_ref(&nom)).unwrap().is_undef());
        // (c) four args
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(1), tol.clone()]).unwrap().is_undef());
        // (d) nominal is Value::Real (not Scalar)
        assert!(eval_stackup("contributor", &[Value::Real(0.010), tol.clone()]).unwrap().is_undef());
        // (e) nominal is FORCE scalar (wrong dim)
        let force = Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE };
        assert!(eval_stackup("contributor", &[force, tol.clone()]).unwrap().is_undef());
        // (f) nominal has NaN si_value
        let nan = Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH };
        assert!(eval_stackup("contributor", &[nan, tol.clone()]).unwrap().is_undef());
        // (g) tol is ANGLE scalar (wrong dim)
        let angle = Value::Scalar { si_value: 0.1, dimension: DimensionVector::ANGLE };
        assert!(eval_stackup("contributor", &[nom.clone(), angle]).unwrap().is_undef());
        // (h) tol is Value::Int
        assert!(eval_stackup("contributor", &[nom.clone(), Value::Int(1)]).unwrap().is_undef());
        // (i) sign is Int(0)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(0)]).unwrap().is_undef());
        // (j) sign is Int(2)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(2)]).unwrap().is_undef());
        // (k) sign is Real(1.0) (not Int)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Real(1.0)]).unwrap().is_undef());
    }

    #[test]
    fn contributor_3arg_accepts_explicit_sign_negative() {
        let m = expect_map(eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(-1));
    }

    #[test]
    fn contributor_3arg_accepts_explicit_sign_positive() {
        let m = expect_map(eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(1)]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
    }

    #[test]
    fn contributor_2arg_returns_map_with_default_sign_and_distribution() {
        let nominal = len(0.010); // 10mm
        let tol = len(0.0001);    // 0.1mm
        let m = expect_map(eval_stackup("contributor", &[nominal, tol]));

        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    // ─── shared helpers for step-1 and step-3 tests ──────────────────────────

    /// Extract the SI value from a LENGTH scalar; panic otherwise (test-only).
    fn scalar_si(v: &Value) -> f64 {
        match v {
            Value::Scalar { si_value, dimension } if *dimension == DimensionVector::LENGTH => {
                *si_value
            }
            other => panic!("expected LENGTH scalar, got {:?}", other),
        }
    }

    /// Assert `actual` is within `rel_tol` (relative) of `expected`.
    fn assert_rel_close(actual: f64, expected: f64, rel_tol: f64, label: &str) {
        let eps = rel_tol * expected.abs().max(1e-30_f64);
        assert!(
            (actual - expected).abs() <= eps,
            "{}: actual={:.6e} expected={:.6e} diff={:.3e} eps={:.3e}",
            label,
            actual,
            expected,
            (actual - expected).abs(),
            eps
        );
    }

    /// Golden 3-contributor chain (shared by worst_case and rss tests):
    /// c1(nominal=10mm, tol=0.1mm, +1), c2(5mm, 0.05mm, -1), c3(3mm, 0.2mm, +1).
    /// gap_nominal = 0.010 - 0.005 + 0.003 = 0.008 m.
    fn golden_chain() -> Value {
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(1)]).unwrap();
        let c2 =
            eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(-1)]).unwrap();
        let c3 = eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(1)]).unwrap();
        Value::List(vec![c1, c2, c3])
    }

    // ─── monte_carlo_stackup step-1 tests ───────────────────────────────────

    /// Helper: accepts None (arm absent) or Some(Undef) (arm present, validation failed).
    fn is_undef_or_none(v: Option<Value>) -> bool {
        match v {
            None => true,
            Some(val) => val.is_undef(),
        }
    }

    #[test]
    fn monte_carlo_arity_and_validation_returns_undef() {
        let chain = golden_chain();
        let n = Value::Int(10); // valid samples
        let s = Value::Int(42); // valid seed

        // 0-arg, 1-arg, 2-arg (missing seed)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup", &[])));
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup", std::slice::from_ref(&chain))));
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup", &[chain.clone(), n.clone()])));

        // 7-arg over-arity
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup", &[
            chain.clone(), n.clone(), s.clone(),
            len(0.001), len(0.009), Value::Real(3.0), Value::Undef,
        ])));

        // chain arg not a non-empty List
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[Value::Int(1), n.clone(), s.clone()])));
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[Value::List(vec![]), n.clone(), s.clone()])));

        // samples = Int(0) (E_StackupBadSamples)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), Value::Int(0), s.clone()])));
        // samples = Int(-5)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), Value::Int(-5), s.clone()])));
        // samples = Real(100.0) (non-Int)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), Value::Real(100.0), s.clone()])));

        // seed = Real(42.0) (non-Int)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), Value::Real(42.0)])));
        // seed = Scalar (dimensioned)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), len(0.042)])));

        // spec_min/spec_max asymmetric: one Length, other Undef (5-arg form)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), len(0.001), Value::Undef])));
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), Value::Undef, len(0.009)])));

        // sigma_level non-positive (4-arg form: args[3] = sigma_level)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), Value::Int(-1)])));
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), Value::Int(0)])));
        // sigma_level NaN
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), Value::Real(f64::NAN)])));
        // sigma_level dimensioned (LENGTH scalar)
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
            &[chain.clone(), n.clone(), s.clone(), len(0.003)])));
    }

    #[test]
    fn monte_carlo_returns_map_for_minimum_valid_args() {
        // Happy-path placeholder: returns Some(Map) for minimum valid 3-arg call.
        // Content checked in step-3. This test is RED until step-2 adds the arm.
        let result = eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(10), Value::Int(42),
        ]);
        match result {
            Some(Value::Map(_)) => {}
            other => panic!("expected Some(Map), got {:?}", other),
        }
    }

    // ─── monte_carlo_stackup step-3 tests (RED until step-4) ───────────────

    #[test]
    fn monte_carlo_map_has_expected_keys_no_spec() {
        // 3-arg call → exactly 9 keys, NO mc_yield_fraction
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(1000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let expected_keys: std::collections::BTreeSet<Value> = [
            "nominal_gap", "mc_mean", "mc_sigma", "mc_min", "mc_max",
            "mc_p_low", "mc_p_high", "samples", "seed",
        ]
        .iter()
        .map(|k| Value::String((*k).into()))
        .collect();
        let actual_keys: std::collections::BTreeSet<Value> = m.keys().cloned().collect();
        assert_eq!(actual_keys, expected_keys,
            "key mismatch: extra={:?} missing={:?}",
            actual_keys.difference(&expected_keys).collect::<Vec<_>>(),
            expected_keys.difference(&actual_keys).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn monte_carlo_samples_seed_int_keys() {
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(1000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        assert_eq!(m[&Value::String("samples".into())], Value::Int(1000));
        assert_eq!(m[&Value::String("seed".into())],    Value::Int(42));
    }

    #[test]
    fn monte_carlo_nominal_gap_matches_chain() {
        // golden_chain: gap_nominal = 0.010 - 0.005 + 0.003 = 0.008 m
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(100), Value::Int(99),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let ng = scalar_si(&m[&Value::String("nominal_gap".into())]);
        assert_rel_close(ng, 0.008, 1e-12, "nominal_gap");
    }

    #[test]
    fn monte_carlo_length_keys_are_length_scalars() {
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(500), Value::Int(7),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        for key in &["mc_mean", "mc_sigma", "mc_min", "mc_max", "mc_p_low", "mc_p_high"] {
            let v = &m[&Value::String((*key).into())];
            match v {
                Value::Scalar { si_value, dimension }
                    if *dimension == DimensionVector::LENGTH && si_value.is_finite() => {}
                other => panic!("key {key}: expected finite LENGTH Scalar, got {:?}", other),
            }
        }
    }

    #[test]
    fn monte_carlo_min_le_p_low_le_p_high_le_max() {
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(2000), Value::Int(13),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mc_min   = scalar_si(&m[&Value::String("mc_min".into())]);
        let mc_p_low = scalar_si(&m[&Value::String("mc_p_low".into())]);
        let mc_p_high= scalar_si(&m[&Value::String("mc_p_high".into())]);
        let mc_max   = scalar_si(&m[&Value::String("mc_max".into())]);
        assert!(mc_min <= mc_p_low,  "mc_min({mc_min}) > mc_p_low({mc_p_low})");
        assert!(mc_p_low <= mc_p_high,"mc_p_low({mc_p_low}) > mc_p_high({mc_p_high})");
        assert!(mc_p_high <= mc_max, "mc_p_high({mc_p_high}) > mc_max({mc_max})");
    }

    #[test]
    fn monte_carlo_normal_only_mean_near_nominal_within_se() {
        // golden_chain: 3 Normal contributors, gap_nominal = 0.008 m
        // rss_sigma = sqrt((1e-4/3)^2 + (5e-5/3)^2 + (2e-4/3)^2)
        //           = sqrt(1.11111e-9 + 2.77778e-10 + 4.44444e-9) / 3 × 3  -- let's compute
        // t1=1e-4, t2=5e-5, t3=2e-4; rss_sigma = sqrt(t1²+t2²+t3²)/sigma_level
        // = sqrt(1e-8 + 2.5e-9 + 4e-8) / 3 = sqrt(5.25e-8) / 3
        // SE(mean) = rss_sigma / sqrt(N)
        // At N=10_000, 5×SE is 5×(sqrt(5.25e-8)/3)/100 ≈ 1.28e-6  — well within 1e-4
        let n = 10_000_usize;
        let rss_sigma = (5.25e-8_f64).sqrt() / 3.0;
        let five_se = 5.0 * rss_sigma / (n as f64).sqrt();
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(n as i64), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mc_mean = scalar_si(&m[&Value::String("mc_mean".into())]);
        assert!(
            (mc_mean - 0.008_f64).abs() <= five_se,
            "mc_mean {mc_mean:.6e} not within 5×SE={five_se:.2e} of nominal 0.008"
        );
    }

    // ─── monte_carlo_stackup step-5 tests (RED until step-6 confirms) ──────

    #[test]
    fn monte_carlo_same_seed_bit_identical() {
        // INV-3: two calls with the same args must produce bit-identical f64 results.
        let run = || match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(5000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let m1 = run();
        let m2 = run();

        for key in &["nominal_gap", "mc_mean", "mc_sigma", "mc_min", "mc_max",
                     "mc_p_low", "mc_p_high"] {
            let k = Value::String((*key).into());
            let a = scalar_si(&m1[&k]);
            let b = scalar_si(&m2[&k]);
            assert_eq!(a.to_bits(), b.to_bits(),
                "key {key}: a={a} b={b} — not bit-identical");
        }
        assert_eq!(m1[&Value::String("samples".into())], m2[&Value::String("samples".into())]);
        assert_eq!(m1[&Value::String("seed".into())],    m2[&Value::String("seed".into())]);
    }

    #[test]
    fn monte_carlo_different_seed_differs() {
        // Different seed must produce different mc_sigma and mc_mean.
        let run = |seed: i64| match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(5000), Value::Int(seed),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let m42 = run(42);
        let m43 = run(43);
        let sigma42 = scalar_si(&m42[&Value::String("mc_sigma".into())]);
        let sigma43 = scalar_si(&m43[&Value::String("mc_sigma".into())]);
        let mean42  = scalar_si(&m42[&Value::String("mc_mean".into())]);
        let mean43  = scalar_si(&m43[&Value::String("mc_mean".into())]);
        assert_ne!(sigma42.to_bits(), sigma43.to_bits(),
            "mc_sigma should differ for different seeds");
        assert_ne!(mean42.to_bits(), mean43.to_bits(),
            "mc_mean should differ for different seeds");
    }

    // ─── monte_carlo_stackup step-7 tests ───────────────────────────────────

    #[test]
    fn monte_carlo_sigma_within_2pct_of_rss_at_n100k() {
        // PRD §3.3 convergence: SE(σ̂) ≈ 0.224% at N=100k; 2% bound ≈ 9×SE → robust.
        // golden_chain: t1=1e-4, t2=5e-5, t3=2e-4, sigma_level=3
        // rss_sigma = sqrt((t1/3)² + (t2/3)² + (t3/3)²)
        let rss = match eval_stackup("stackup_rss", &[golden_chain()]) {
            Some(Value::Map(m)) => m,
            other => panic!("stackup_rss failed: {:?}", other),
        };
        let rss_sigma = scalar_si(&rss[&Value::String("rss_sigma".into())]);

        let mc = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(100_000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("monte_carlo failed: {:?}", other),
        };
        let mc_sigma = scalar_si(&mc[&Value::String("mc_sigma".into())]);

        let rel_err = (mc_sigma - rss_sigma).abs() / rss_sigma;
        assert!(rel_err <= 0.02,
            "mc_sigma {mc_sigma:.6e} vs rss_sigma {rss_sigma:.6e}: rel_err {:.4}% > 2%",
            rel_err * 100.0);
    }

    #[test]
    fn monte_carlo_sigma_bit_exact_regression_pin_n100k_seed42() {
        // Regression golden for mc_sigma at (golden_chain, N=100_000, seed=42).
        // Pinned in step-8 after measuring the actual value.
        // Placeholder constant 0 will be replaced with the real bits in step-8.
        let mc = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(100_000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mc_sigma = scalar_si(&mc[&Value::String("mc_sigma".into())]);
        // Print actual bits for step-8 pinning (remove after capturing)
        // Pinned in step-8: golden mc_sigma for (golden_chain, N=100_000, seed=42)
        // Value: 7.629323289651385e-5 m  (measured on first correct run)
        let expected_bits: u64 = 0x3F13_FFF3_C2C2_E856;
        assert_eq!(mc_sigma.to_bits(), expected_bits,
            "mc_sigma={mc_sigma:.10e} bits=0x{:016X} expected=0x{expected_bits:016X}",
            mc_sigma.to_bits());
    }

    // ─── monte_carlo_stackup step-9 tests (yield_fraction) ─────────────────

    #[test]
    fn monte_carlo_yield_fraction_absent_without_spec() {
        // 3-arg call: no mc_yield_fraction key.
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(1000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        assert!(!m.contains_key(&Value::String("mc_yield_fraction".into())),
            "3-arg call must NOT contain mc_yield_fraction");
    }

    #[test]
    fn monte_carlo_yield_fraction_present_with_5arg() {
        // 5-arg call: mc_yield_fraction present as Value::Real (dimensionless).
        let m = match eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(1000), Value::Int(42),
            len(0.001), len(0.015), // spec_min=1mm, spec_max=15mm
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        assert!(m.contains_key(&Value::String("mc_yield_fraction".into())),
            "5-arg call MUST contain mc_yield_fraction");
        match &m[&Value::String("mc_yield_fraction".into())] {
            Value::Real(_) => {}
            other => panic!("mc_yield_fraction must be Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn monte_carlo_yield_fraction_one_sigma_band_normal() {
        // Single Normal contributor: nominal=10mm, tol=3mm → sigma_gap=1mm at sigma_level=3.
        // spec=[9mm, 11mm] = ±1σ; expect yield ≈ Φ(1)−Φ(−1) = 0.6826894921370859.
        // SE(p̂) = sqrt(p(1-p)/N) ≈ 1.47e-3 at N=100k; |empirical − 0.6827| ≤ 0.01 (≈ 7×SE).
        let c = eval_stackup("contributor", &[len(0.010), len(0.003), Value::Int(1)]).unwrap();
        let one_c_chain = Value::List(vec![c]);
        let m = match eval_stackup("monte_carlo_stackup", &[
            one_c_chain, Value::Int(100_000), Value::Int(42),
            len(0.009), len(0.011),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let yf = match &m[&Value::String("mc_yield_fraction".into())] {
            Value::Real(r) => *r,
            other => panic!("expected Real, got {:?}", other),
        };
        let expected = 0.6826894921370859_f64; // Φ(1)−Φ(−1)
        assert!((yf - expected).abs() <= 0.01,
            "yield_fraction {yf:.6} not within 0.01 of {expected:.6}");
    }

    #[test]
    fn monte_carlo_yield_fraction_full_band_normal() {
        // spec=[6mm, 14mm] = ±4σ for same chain; expect yield ≥ 0.999.
        let c = eval_stackup("contributor", &[len(0.010), len(0.003), Value::Int(1)]).unwrap();
        let one_c_chain = Value::List(vec![c]);
        let m = match eval_stackup("monte_carlo_stackup", &[
            one_c_chain, Value::Int(100_000), Value::Int(42),
            len(0.006), len(0.014),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let yf = match &m[&Value::String("mc_yield_fraction".into())] {
            Value::Real(r) => *r,
            other => panic!("expected Real, got {:?}", other),
        };
        assert!(yf >= 0.999, "yield ≥ 0.999 for ±4σ spec, got {yf}");
    }

    // ─── monte_carlo_stackup step-11 tests (INV-4 sign-flip, INV-5 zero-tol) ─

    #[test]
    fn monte_carlo_inv4_sign_flip_negates_mean_band_unchanged() {
        // INV-4: flip all signs → mc_mean negates; mc_sigma unchanged (bit-exact
        // since sampling stream depends only on (seed, contributor_index), not sign).
        let orig_chain = golden_chain(); // c1(+1), c2(-1), c3(+1) → gap=+0.008
        let c1f = eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]).unwrap();
        let c2f = eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(1)]).unwrap();
        let c3f = eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(-1)]).unwrap();
        let flip_chain = Value::List(vec![c1f, c2f, c3f]);

        let n = 10_000_i64;
        let seed = 42_i64;

        let run = |chain| match eval_stackup("monte_carlo_stackup", &[
            chain, Value::Int(n), Value::Int(seed),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mo = run(orig_chain);
        let mf = run(flip_chain);

        let mean_orig  = scalar_si(&mo[&Value::String("mc_mean".into())]);
        let mean_flip  = scalar_si(&mf[&Value::String("mc_mean".into())]);
        let sigma_orig = scalar_si(&mo[&Value::String("mc_sigma".into())]);
        let sigma_flip = scalar_si(&mf[&Value::String("mc_sigma".into())]);

        // mc_mean must negate (within 5·SE of 0.008)
        let rss_sigma = (5.25e-8_f64).sqrt() / 3.0;
        let five_se = 5.0 * rss_sigma / (n as f64).sqrt();
        assert!((mean_orig + mean_flip).abs() <= 2.0 * five_se,
            "mc_mean + mc_mean_flipped should ≈ 0; got {} + {} = {}",
            mean_orig, mean_flip, mean_orig + mean_flip);

        // mc_sigma bit-identical (sign only multiplied post-draw)
        assert_eq!(sigma_orig.to_bits(), sigma_flip.to_bits(),
            "mc_sigma should be bit-identical after sign flip: {} vs {}", sigma_orig, sigma_flip);
    }

    #[test]
    fn monte_carlo_inv5_zero_tol_sigma_is_zero() {
        // INV-5: all tolerances = 0 → mc_sigma = 0 exactly, mc_mean = nominal_gap exactly,
        // mc_min = mc_max = nominal_gap exactly (no random deviations).
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0), Value::Int(1)]).unwrap();
        let c2 = eval_stackup("contributor", &[len(0.005), len(0.0), Value::Int(-1)]).unwrap();
        let zero_tol_chain = Value::List(vec![c1, c2]);
        let m = match eval_stackup("monte_carlo_stackup", &[
            zero_tol_chain, Value::Int(500), Value::Int(7),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let ng     = scalar_si(&m[&Value::String("nominal_gap".into())]);
        let mean   = scalar_si(&m[&Value::String("mc_mean".into())]);
        let sigma  = scalar_si(&m[&Value::String("mc_sigma".into())]);
        let mc_min = scalar_si(&m[&Value::String("mc_min".into())]);
        let mc_max = scalar_si(&m[&Value::String("mc_max".into())]);
        assert_eq!(sigma, 0.0,  "zero-tol: mc_sigma must be 0.0, got {sigma}");
        assert_eq!(mean,  ng,   "zero-tol: mc_mean must == nominal_gap, got {mean} vs {ng}");
        assert_eq!(mc_min, ng,  "zero-tol: mc_min must == nominal_gap, got {mc_min} vs {ng}");
        assert_eq!(mc_max, ng,  "zero-tol: mc_max must == nominal_gap, got {mc_max} vs {ng}");
    }

    // ─── monte_carlo_stackup amendment tests ────────────────────────────────

    #[test]
    fn monte_carlo_inverted_spec_bounds_returns_undef() {
        // spec_min > spec_max → Undef (reviewer suggestion #2: robustness guard).
        // Previously this silently produced yield_fraction=0; now it is rejected.
        assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup", &[
            golden_chain(), Value::Int(100), Value::Int(42),
            len(0.010), len(0.001), // spec_min=10mm > spec_max=1mm (inverted)
        ])), "inverted spec bounds must return Undef");
    }

    #[test]
    fn monte_carlo_uniform_distribution_sigma_near_theoretical() {
        // Single Uniform contributor: sample_uniform_sym(t_i) draws from Uniform[-t_i, t_i].
        // Variance = t_i²/3  →  sigma_gap = t_i/sqrt(3).
        // Exercises the Distribution::Uniform branch in the MC sampling loop.
        let t = 0.01_f64;
        let expected_sigma = t / 3.0_f64.sqrt();
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Uniform".into() };
        let c = eval_stackup("contributor_asym", &[
            len(0.010), len(t), len(t), Value::Int(1), dist,
        ]).unwrap();
        let chain = Value::List(vec![c]);
        let m = match eval_stackup("monte_carlo_stackup", &[
            chain, Value::Int(100_000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mc_sigma = scalar_si(&m[&Value::String("mc_sigma".into())]);
        let rel_err = (mc_sigma - expected_sigma).abs() / expected_sigma;
        assert!(rel_err <= 0.02,
            "Uniform mc_sigma {mc_sigma:.6e} vs expected {expected_sigma:.6e}: \
             rel_err {:.4}% > 2%", rel_err * 100.0);
    }

    #[test]
    fn monte_carlo_triangular_distribution_sigma_near_theoretical() {
        // Single Triangular contributor: sample_triangular_sym(t_i) draws from Triangular[-t_i, t_i].
        // Variance = t_i²/6  →  sigma_gap = t_i/sqrt(6).
        // Exercises the Distribution::Triangular branch in the MC sampling loop.
        let t = 0.01_f64;
        let expected_sigma = t / 6.0_f64.sqrt();
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Triangular".into() };
        let c = eval_stackup("contributor_asym", &[
            len(0.010), len(t), len(t), Value::Int(1), dist,
        ]).unwrap();
        let chain = Value::List(vec![c]);
        let m = match eval_stackup("monte_carlo_stackup", &[
            chain, Value::Int(100_000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let mc_sigma = scalar_si(&m[&Value::String("mc_sigma".into())]);
        let rel_err = (mc_sigma - expected_sigma).abs() / expected_sigma;
        assert!(rel_err <= 0.02,
            "Triangular mc_sigma {mc_sigma:.6e} vs expected {expected_sigma:.6e}: \
             rel_err {:.4}% > 2%", rel_err * 100.0);
    }

    #[test]
    fn monte_carlo_invalid_distribution_in_chain_returns_undef() {
        // parse_chain returns None for unrecognized/invalid distribution values,
        // which propagates to Undef from monte_carlo_stackup.
        // (a) Unrecognized Distribution variant
        {
            let mut m = std::collections::BTreeMap::new();
            m.insert(Value::String("nominal".into()),      len(0.010));
            m.insert(Value::String("plus_tol".into()),     len(0.001));
            m.insert(Value::String("minus_tol".into()),    len(0.001));
            m.insert(Value::String("sign".into()),         Value::Int(1));
            m.insert(Value::String("distribution".into()),
                Value::Enum { type_name: "Distribution".into(), variant: "Lognormal".into() });
            let chain = Value::List(vec![Value::Map(m)]);
            assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
                &[chain, Value::Int(10), Value::Int(42)])),
                "unrecognized Distribution variant must return Undef");
        }
        // (b) Invalid distribution key type (String instead of Enum)
        {
            let mut m = std::collections::BTreeMap::new();
            m.insert(Value::String("nominal".into()),      len(0.010));
            m.insert(Value::String("plus_tol".into()),     len(0.001));
            m.insert(Value::String("minus_tol".into()),    len(0.001));
            m.insert(Value::String("sign".into()),         Value::Int(1));
            m.insert(Value::String("distribution".into()),
                Value::String("Normal".into())); // must be Enum, not String
            let chain = Value::List(vec![Value::Map(m)]);
            assert!(is_undef_or_none(eval_stackup("monte_carlo_stackup",
                &[chain, Value::Int(10), Value::Int(42)])),
                "invalid distribution key type (String) must return Undef");
        }
    }

    #[test]
    fn monte_carlo_absent_distribution_key_defaults_to_normal() {
        // A contributor map without a "distribution" key should default to Normal
        // in parse_chain, yielding bit-identical mc_sigma vs an explicit Normal chain.
        let mut m_no_dist = std::collections::BTreeMap::new();
        m_no_dist.insert(Value::String("nominal".into()),   len(0.010));
        m_no_dist.insert(Value::String("plus_tol".into()),  len(0.001));
        m_no_dist.insert(Value::String("minus_tol".into()), len(0.001));
        m_no_dist.insert(Value::String("sign".into()),      Value::Int(1));
        // NOTE: "distribution" key deliberately omitted → parse_chain defaults to Normal
        let chain_no_dist = Value::List(vec![Value::Map(m_no_dist)]);

        let c_normal =
            eval_stackup("contributor", &[len(0.010), len(0.001), Value::Int(1)]).unwrap();
        let chain_normal = Value::List(vec![c_normal]);

        let run = |chain| match eval_stackup("monte_carlo_stackup", &[
            chain, Value::Int(5000), Value::Int(42),
        ]) {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        };
        let m_absent = run(chain_no_dist);
        let m_normal = run(chain_normal);

        let sigma_absent = scalar_si(&m_absent[&Value::String("mc_sigma".into())]);
        let sigma_normal = scalar_si(&m_normal[&Value::String("mc_sigma".into())]);
        assert_eq!(sigma_absent.to_bits(), sigma_normal.to_bits(),
            "absent distribution key should produce Normal results (bit-identical): \
             absent={sigma_absent:.8e} normal={sigma_normal:.8e}");
    }

    // ─── stackup_worst_case tests (step-1 RED; GREEN after step-2 impl) ──────

    #[test]
    fn worst_case_happy_path_golden_chain() {
        // GOLDEN chain hand-calc (SI meters):
        //   gap_nominal     = 0.010 - 0.005 + 0.003       = 0.008       m
        //   sum_plus        = 0.0001 + 0.00005 + 0.0002    = 0.00035     m
        //   sum_minus       = 0.0001 + 0.00005 + 0.0002    = 0.00035     m (symmetric)
        //   worst_case_max  = 0.008 + 0.00035              = 0.00835     m
        //   worst_case_min  = 0.008 - 0.00035              = 0.00765     m
        //   worst_case_band = (0.00035 + 0.00035) / 2      = 3.5e-4      m
        let m = expect_map(eval_stackup("stackup_worst_case", &[golden_chain()]));
        assert_eq!(m.len(), 4, "result map must have exactly 4 keys");
        let tol = 1e-12_f64;
        assert_rel_close(
            scalar_si(&m[&Value::String("nominal_gap".into())]),
            0.008,
            tol,
            "nominal_gap",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_max".into())]),
            0.00835,
            tol,
            "worst_case_max",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_min".into())]),
            0.00765,
            tol,
            "worst_case_min",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_band".into())]),
            3.5e-4,
            tol,
            "worst_case_band",
        );
    }

    #[test]
    fn worst_case_inv4_sign_flip_negates_nominal_gap_band_unchanged() {
        // INV-4: flip all signs → nominal_gap negates, worst_case_band unchanged.
        //   Flipped: c1(-1), c2(+1), c3(-1)
        //   gap_nominal = -0.010 + 0.005 - 0.003 = -0.008 m
        //   worst_case_band remains 3.5e-4 m (sum_plus / sum_minus unchanged)
        let c1 =
            eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]).unwrap();
        let c2 =
            eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(1)]).unwrap();
        let c3 =
            eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(-1)]).unwrap();
        let flipped = Value::List(vec![c1, c2, c3]);
        let m = expect_map(eval_stackup("stackup_worst_case", &[flipped]));
        let tol = 1e-12_f64;
        assert_rel_close(
            scalar_si(&m[&Value::String("nominal_gap".into())]),
            -0.008,
            tol,
            "nominal_gap flipped",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_band".into())]),
            3.5e-4,
            tol,
            "band unchanged after flip",
        );
    }

    #[test]
    fn worst_case_inv5_zero_tol_band_is_zero() {
        // INV-5: all tolerances = 0 → band=0, max==min==nominal_gap.
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0), Value::Int(1)]).unwrap();
        let c2 = eval_stackup("contributor", &[len(0.005), len(0.0), Value::Int(-1)]).unwrap();
        let zero_tol_chain = Value::List(vec![c1, c2]);
        let m = expect_map(eval_stackup("stackup_worst_case", &[zero_tol_chain]));
        let gap  = scalar_si(&m[&Value::String("nominal_gap".into())]);
        let band = scalar_si(&m[&Value::String("worst_case_band".into())]);
        let max  = scalar_si(&m[&Value::String("worst_case_max".into())]);
        let min  = scalar_si(&m[&Value::String("worst_case_min".into())]);
        assert_eq!(band, 0.0, "zero-tol: band must be 0.0");
        assert_eq!(max, gap,  "zero-tol: max == nominal_gap");
        assert_eq!(min, gap,  "zero-tol: min == nominal_gap");
    }

    #[test]
    fn worst_case_inv5_empty_chain_returns_undef() {
        assert!(
            eval_stackup("stackup_worst_case", &[Value::List(vec![])]).unwrap().is_undef(),
            "empty chain must return Undef"
        );
    }

    #[test]
    fn worst_case_inv6_non_length_field_returns_undef() {
        // A contributor map whose `nominal` is a FORCE scalar must yield Undef.
        use std::collections::BTreeMap;
        let mut bad_m: BTreeMap<Value, Value> = BTreeMap::new();
        bad_m.insert(
            Value::String("nominal".into()),
            Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE },
        );
        bad_m.insert(Value::String("plus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("minus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("sign".into()), Value::Int(1));
        bad_m.insert(
            Value::String("distribution".into()),
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
        );
        let bad_chain = Value::List(vec![Value::Map(bad_m)]);
        assert!(
            eval_stackup("stackup_worst_case", &[bad_chain]).unwrap().is_undef(),
            "non-LENGTH nominal must return Undef"
        );
    }

    #[test]
    fn worst_case_malformed_inputs_return_undef() {
        use std::collections::BTreeMap;
        let nom = len(0.010);
        let tol = len(0.0001);

        // (a) args[0] is not a List
        assert!(
            eval_stackup("stackup_worst_case", std::slice::from_ref(&nom)).unwrap().is_undef(),
            "non-List arg[0] must be Undef"
        );
        assert!(
            eval_stackup("stackup_worst_case", &[Value::Int(1)]).unwrap().is_undef(),
            "Int arg[0] must be Undef"
        );

        // (b) List element is not a Map
        let not_map = Value::List(vec![nom.clone()]);
        assert!(
            eval_stackup("stackup_worst_case", &[not_map]).unwrap().is_undef(),
            "non-Map element must be Undef"
        );

        // (c) Contributor map missing `sign` key
        let mut no_sign: BTreeMap<Value, Value> = BTreeMap::new();
        no_sign.insert(Value::String("nominal".into()), nom.clone());
        no_sign.insert(Value::String("plus_tol".into()), tol.clone());
        no_sign.insert(Value::String("minus_tol".into()), tol.clone());
        let missing_sign_chain = Value::List(vec![Value::Map(no_sign)]);
        assert!(
            eval_stackup("stackup_worst_case", &[missing_sign_chain]).unwrap().is_undef(),
            "missing sign key must be Undef"
        );

        // (d) sign = Int(0) is invalid
        let mut zero_sign: BTreeMap<Value, Value> = BTreeMap::new();
        zero_sign.insert(Value::String("nominal".into()), nom.clone());
        zero_sign.insert(Value::String("plus_tol".into()), tol.clone());
        zero_sign.insert(Value::String("minus_tol".into()), tol.clone());
        zero_sign.insert(Value::String("sign".into()), Value::Int(0));
        let zero_sign_chain = Value::List(vec![Value::Map(zero_sign)]);
        assert!(
            eval_stackup("stackup_worst_case", &[zero_sign_chain]).unwrap().is_undef(),
            "sign=0 must be Undef"
        );
    }

    #[test]
    fn worst_case_arity_returns_undef() {
        // 0 args → Undef
        assert!(
            eval_stackup("stackup_worst_case", &[]).unwrap().is_undef(),
            "0 args must be Undef"
        );
        // 2 args → Undef
        let chain = golden_chain();
        assert!(
            eval_stackup("stackup_worst_case", &[chain.clone(), chain]).unwrap().is_undef(),
            "2 args must be Undef"
        );
    }

    // ─── stackup_rss tests (step-3 RED; GREEN after step-4 impl) ─────────────

    #[test]
    fn rss_happy_path_golden_chain_default_sigma() {
        // GOLDEN chain hand-calc (SI meters, default sigma_level=3):
        //   t1 = (0.0001 + 0.0001) / 2 = 1e-4   m  (symmetric)
        //   t2 = (0.00005 + 0.00005) / 2 = 5e-5  m
        //   t3 = (0.0002 + 0.0002) / 2 = 2e-4    m
        //   Sum t^2 = (1e-4)^2 + (5e-5)^2 + (2e-4)^2 = 1e-8 + 2.5e-9 + 4e-8 = 5.25e-8 m^2
        //   rss_band  = sqrt(5.25e-8) ≈ 2.291288e-4  m  (sigma-invariant)
        //   rss_sigma = rss_band / 3  ≈ 7.637628e-5  m
        //   nominal_gap = 0.008 m
        //   rss_min   = 0.008 - rss_band ≈ 7.770871e-3  m
        //   rss_max   = 0.008 + rss_band ≈ 8.229129e-3  m
        //   sigma_level = Value::Real(3.0)
        let m = expect_map(eval_stackup("stackup_rss", &[golden_chain()]));
        assert_eq!(m.len(), 6, "result map must have exactly 6 keys");
        let tol = 1e-12_f64;
        let rss_band_expected = (5.25e-8_f64).sqrt();
        let rss_sigma_expected = rss_band_expected / 3.0;
        assert_rel_close(
            scalar_si(&m[&Value::String("nominal_gap".into())]),
            0.008,
            tol,
            "nominal_gap",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("rss_band".into())]),
            rss_band_expected,
            tol,
            "rss_band",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("rss_sigma".into())]),
            rss_sigma_expected,
            tol,
            "rss_sigma",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("rss_min".into())]),
            0.008 - rss_band_expected,
            tol,
            "rss_min",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("rss_max".into())]),
            0.008 + rss_band_expected,
            tol,
            "rss_max",
        );
        assert_eq!(
            m[&Value::String("sigma_level".into())],
            Value::Real(3.0),
            "sigma_level must be Value::Real(3.0)"
        );
    }

    #[test]
    fn rss_inv2_exactness_sigma3() {
        // INV-2: rss_sigma equals the closed-form formula to 1e-12 relative.
        let m = expect_map(eval_stackup("stackup_rss", &[golden_chain()]));
        let expected_sigma = ((1e-4_f64 / 3.0).powi(2)
            + (5e-5_f64 / 3.0).powi(2)
            + (2e-4_f64 / 3.0).powi(2))
        .sqrt();
        assert_rel_close(
            scalar_si(&m[&Value::String("rss_sigma".into())]),
            expected_sigma,
            1e-12,
            "rss_sigma exactness (INV-2)",
        );
    }

    #[test]
    fn rss_sigma_flow_through_int_arg() {
        // Sigma flow-through: pass Value::Int(6).
        //   rss_sigma(sigma=6) = rss_band / 6  (halves relative to sigma=3)
        //   rss_band INVARIANT: rss_band(sigma=6) == rss_band(sigma=3)  ← regression guard
        //   sigma_level stored as Value::Real(6.0)
        let chain = golden_chain();
        let m6 = expect_map(eval_stackup("stackup_rss", &[chain.clone(), Value::Int(6)]));
        let m3 = expect_map(eval_stackup("stackup_rss", &[chain]));
        let tol = 1e-12_f64;
        let rss_band3  = scalar_si(&m3[&Value::String("rss_band".into())]);
        let rss_sigma3 = scalar_si(&m3[&Value::String("rss_sigma".into())]);
        let rss_band6  = scalar_si(&m6[&Value::String("rss_band".into())]);
        let rss_sigma6 = scalar_si(&m6[&Value::String("rss_sigma".into())]);
        // rss_band is sigma-invariant (PRD 3.2 identity)
        assert_rel_close(rss_band6, rss_band3, tol, "rss_band invariant under sigma=6 (regression guard)");
        // rss_sigma halves when sigma doubles
        assert_rel_close(rss_sigma6, rss_sigma3 / 2.0, tol, "rss_sigma halves at sigma=6");
        assert_eq!(
            m6[&Value::String("sigma_level".into())],
            Value::Real(6.0),
            "sigma_level stored as Real(6.0)"
        );
    }

    #[test]
    fn rss_sigma_flow_through_real_arg() {
        // Same as the Int(6) test but pass Value::Real(6.0).
        let chain = golden_chain();
        let m6 = expect_map(eval_stackup("stackup_rss", &[chain.clone(), Value::Real(6.0)]));
        let m3 = expect_map(eval_stackup("stackup_rss", &[chain]));
        let tol = 1e-12_f64;
        let rss_band3  = scalar_si(&m3[&Value::String("rss_band".into())]);
        let rss_sigma3 = scalar_si(&m3[&Value::String("rss_sigma".into())]);
        let rss_band6  = scalar_si(&m6[&Value::String("rss_band".into())]);
        let rss_sigma6 = scalar_si(&m6[&Value::String("rss_sigma".into())]);
        assert_rel_close(rss_band6, rss_band3, tol, "rss_band invariant (Real arg)");
        assert_rel_close(rss_sigma6, rss_sigma3 / 2.0, tol, "rss_sigma halves (Real arg)");
        assert_eq!(m6[&Value::String("sigma_level".into())], Value::Real(6.0));
    }

    #[test]
    fn rss_inv1_worst_case_band_ge_rss_band() {
        // INV-1: worst_case_band >= rss_band (L1 >= L2 inequality).
        // 3.5e-4 >= 2.291288e-4 (Golden chain values).
        let wc = expect_map(eval_stackup("stackup_worst_case", &[golden_chain()]));
        let rss = expect_map(eval_stackup("stackup_rss", &[golden_chain()]));
        let wc_band  = scalar_si(&wc[&Value::String("worst_case_band".into())]);
        let rss_band = scalar_si(&rss[&Value::String("rss_band".into())]);
        assert!(
            wc_band >= rss_band,
            "INV-1 violated: worst_case_band={:.6e} < rss_band={:.6e}",
            wc_band, rss_band
        );
    }

    #[test]
    fn rss_inv4_sign_flip_invariants() {
        // INV-4: flip all signs; rss_sigma and rss_band unchanged; nominal_gap negates.
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]).unwrap();
        let c2 = eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(1)]).unwrap();
        let c3 = eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(-1)]).unwrap();
        let flipped = Value::List(vec![c1, c2, c3]);
        let mf = expect_map(eval_stackup("stackup_rss", &[flipped]));
        let m  = expect_map(eval_stackup("stackup_rss", &[golden_chain()]));
        let tol = 1e-12_f64;
        assert_rel_close(
            scalar_si(&mf[&Value::String("rss_band".into())]),
            scalar_si(&m[&Value::String("rss_band".into())]),
            tol,
            "rss_band invariant under sign flip",
        );
        assert_rel_close(
            scalar_si(&mf[&Value::String("rss_sigma".into())]),
            scalar_si(&m[&Value::String("rss_sigma".into())]),
            tol,
            "rss_sigma invariant under sign flip",
        );
        assert_rel_close(
            scalar_si(&mf[&Value::String("nominal_gap".into())]),
            -0.008,
            tol,
            "nominal_gap negated after flip",
        );
    }

    #[test]
    fn rss_inv5_zero_tol_sigma_and_band_are_zero() {
        // INV-5: all tolerances = 0 → rss_sigma=0, rss_band=0, rss_min==rss_max==nominal_gap.
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0), Value::Int(1)]).unwrap();
        let c2 = eval_stackup("contributor", &[len(0.005), len(0.0), Value::Int(-1)]).unwrap();
        let zero_tol_chain = Value::List(vec![c1, c2]);
        let m = expect_map(eval_stackup("stackup_rss", &[zero_tol_chain]));
        let gap   = scalar_si(&m[&Value::String("nominal_gap".into())]);
        let band  = scalar_si(&m[&Value::String("rss_band".into())]);
        let sigma = scalar_si(&m[&Value::String("rss_sigma".into())]);
        let min   = scalar_si(&m[&Value::String("rss_min".into())]);
        let max   = scalar_si(&m[&Value::String("rss_max".into())]);
        assert_eq!(band,  0.0, "zero-tol: rss_band must be 0");
        assert_eq!(sigma, 0.0, "zero-tol: rss_sigma must be 0");
        assert_eq!(min, gap, "zero-tol: rss_min == nominal_gap");
        assert_eq!(max, gap, "zero-tol: rss_max == nominal_gap");
    }

    #[test]
    fn rss_validation_returns_undef() {
        // (a) empty chain → Undef
        assert!(
            eval_stackup("stackup_rss", &[Value::List(vec![])]).unwrap().is_undef(),
            "empty chain must be Undef"
        );

        // (b) non-Length contributor field → Undef
        use std::collections::BTreeMap;
        let mut bad_m: BTreeMap<Value, Value> = BTreeMap::new();
        bad_m.insert(
            Value::String("nominal".into()),
            Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE },
        );
        bad_m.insert(Value::String("plus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("minus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("sign".into()), Value::Int(1));
        bad_m.insert(
            Value::String("distribution".into()),
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
        );
        let bad_chain = Value::List(vec![Value::Map(bad_m)]);
        assert!(
            eval_stackup("stackup_rss", &[bad_chain]).unwrap().is_undef(),
            "non-LENGTH contributor field must be Undef"
        );

        // (c) sigma_level <= 0 → Undef
        let chain = golden_chain();
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), Value::Int(0)]).unwrap().is_undef(),
            "sigma_level=Int(0) must be Undef"
        );
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), Value::Real(-3.0)]).unwrap().is_undef(),
            "sigma_level=Real(-3.0) must be Undef"
        );

        // (d) non-finite sigma_level → Undef
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), Value::Real(f64::NAN)]).unwrap().is_undef(),
            "sigma_level=NaN must be Undef"
        );
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), Value::Real(f64::INFINITY)])
                .unwrap()
                .is_undef(),
            "sigma_level=Inf must be Undef"
        );

        // (e) DIMENSIONED sigma_level (LENGTH scalar 6mm) → Undef
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), len(0.006)]).unwrap().is_undef(),
            "dimensioned sigma_level must be Undef"
        );

        // (f) 3 args → Undef
        assert!(
            eval_stackup("stackup_rss", &[chain.clone(), Value::Real(3.0), Value::Real(0.0)])
                .unwrap()
                .is_undef(),
            "3 args must be Undef"
        );

        // (g) 0 args → Undef
        assert!(
            eval_stackup("stackup_rss", &[]).unwrap().is_undef(),
            "0 args must be Undef"
        );
    }

    // ─── asymmetric-tolerance tests (Suggestions 1 & 2) ─────────────────────

    #[test]
    fn worst_case_asym_contributor_sign_aware_bounds() {
        // Verifies sign-aware worst-case bound computation for asymmetric tolerances.
        //
        // Chain (SI meters):
        //   c1: nominal=10mm, plus_tol=0.2mm, minus_tol=0.1mm, sign=+1
        //   c2: nominal=5mm,  plus_tol=0.3mm, minus_tol=0.15mm, sign=-1
        //
        // gap_nominal = +0.010 + (-1)*0.005 = 0.005 m
        //
        // Sign-aware routing:
        //   c1 (sign=+1): plus_tol  → max side, minus_tol  → min side
        //   c2 (sign=-1): minus_tol → max side, plus_tol   → min side
        //
        // sum_max  = 0.0002  + 0.00015 = 0.00035 m
        // sum_min  = 0.0001  + 0.0003  = 0.00040 m
        // wc_max   = 0.005   + 0.00035 = 0.00535 m
        // wc_min   = 0.005   - 0.00040 = 0.00460 m
        // wc_band  = (0.00035 + 0.00040) / 2 = 3.75e-4 m
        let c1 = eval_stackup(
            "contributor_asym",
            &[len(0.010), len(0.0002), len(0.0001), Value::Int(1)],
        )
        .unwrap();
        let c2 = eval_stackup(
            "contributor_asym",
            &[len(0.005), len(0.0003), len(0.00015), Value::Int(-1)],
        )
        .unwrap();
        let chain = Value::List(vec![c1, c2]);
        let m = expect_map(eval_stackup("stackup_worst_case", &[chain]));
        let tol = 1e-12_f64;
        assert_rel_close(scalar_si(&m[&Value::String("nominal_gap".into())]),     0.005,   tol, "nominal_gap");
        assert_rel_close(scalar_si(&m[&Value::String("worst_case_max".into())]),  0.00535, tol, "wc_max");
        assert_rel_close(scalar_si(&m[&Value::String("worst_case_min".into())]),  0.0046,  tol, "wc_min");
        assert_rel_close(scalar_si(&m[&Value::String("worst_case_band".into())]), 3.75e-4, tol, "wc_band");
    }

    #[test]
    fn rss_asym_contributor_t_i_averaging() {
        // Verifies t_i = (plus_tol + minus_tol) / 2 half-band averaging for asymmetric
        // tolerances with a sign=-1 contributor.
        //
        // Chain (SI meters):
        //   c1: nominal=10mm, plus_tol=0.2mm, minus_tol=0.1mm, sign=+1
        //   c2: nominal=5mm,  plus_tol=0.3mm, minus_tol=0.15mm, sign=-1
        //
        // gap_nominal = 0.005 m
        // t1 = (0.0002 + 0.0001) / 2 = 0.00015  m
        // t2 = (0.0003 + 0.00015) / 2 = 0.000225 m
        // Sum t^2 = (0.00015)^2 + (0.000225)^2 = 2.25e-8 + 5.0625e-8 = 7.3125e-8 m^2
        // rss_band  = sqrt(7.3125e-8) m  (sigma-invariant)
        // rss_sigma = rss_band / 3
        let c1 = eval_stackup(
            "contributor_asym",
            &[len(0.010), len(0.0002), len(0.0001), Value::Int(1)],
        )
        .unwrap();
        let c2 = eval_stackup(
            "contributor_asym",
            &[len(0.005), len(0.0003), len(0.00015), Value::Int(-1)],
        )
        .unwrap();
        let chain = Value::List(vec![c1, c2]);
        let m = expect_map(eval_stackup("stackup_rss", &[chain]));
        let tol = 1e-12_f64;
        let rss_band_expected = (7.3125e-8_f64).sqrt();
        let rss_sigma_expected = rss_band_expected / 3.0;
        assert_rel_close(scalar_si(&m[&Value::String("nominal_gap".into())]),  0.005,               tol, "nominal_gap");
        assert_rel_close(scalar_si(&m[&Value::String("rss_band".into())]),     rss_band_expected,   tol, "rss_band");
        assert_rel_close(scalar_si(&m[&Value::String("rss_sigma".into())]),    rss_sigma_expected,  tol, "rss_sigma");
        assert_rel_close(scalar_si(&m[&Value::String("rss_min".into())]),      0.005 - rss_band_expected, tol, "rss_min");
        assert_rel_close(scalar_si(&m[&Value::String("rss_max".into())]),      0.005 + rss_band_expected, tol, "rss_max");
    }

    // ─── diagnose() classifier tests (step-3 RED; GREEN after step-4) ────────

    use reify_core::{Diagnostic as RDiagnostic, DiagnosticCode, Severity};

    /// (a) empty chain → StackupEmptyChain, message contains "E_StackupEmptyChain",
    ///     severity == Error.
    #[test]
    fn diagnose_empty_chain_returns_stackup_empty_chain() {
        let d: RDiagnostic = diagnose("stackup_rss", &[Value::List(vec![])])
            .expect("diagnose must return Some for empty chain");
        assert_eq!(d.severity, Severity::Error, "expected Error severity");
        assert_eq!(d.code, Some(DiagnosticCode::StackupEmptyChain));
        assert!(d.message.contains("E_StackupEmptyChain"),
            "message must contain 'E_StackupEmptyChain'; got: {:?}", d.message);
    }

    /// (b) non-Map item in chain → StackupDimMismatch.
    #[test]
    fn diagnose_non_map_item_returns_stackup_dim_mismatch() {
        // A chain whose only element is a plain Int (not a Map).
        let chain = Value::List(vec![Value::Int(42)]);
        let d = diagnose("stackup_rss", &[chain])
            .expect("diagnose must return Some for non-Map contributor");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::StackupDimMismatch));
        assert!(d.message.contains("E_StackupDimMismatch"),
            "message must contain 'E_StackupDimMismatch'; got: {:?}", d.message);
    }

    /// (b) non-Length nominal field → StackupDimMismatch.
    #[test]
    fn diagnose_non_length_nominal_returns_stackup_dim_mismatch() {
        // Build a map with nominal = Value::Real (not a LENGTH scalar).
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("nominal".into()),   Value::Real(0.010)); // not LENGTH
        m.insert(Value::String("plus_tol".into()),  len(0.0001));
        m.insert(Value::String("minus_tol".into()), len(0.0001));
        m.insert(Value::String("sign".into()),      Value::Int(1));
        let chain = Value::List(vec![Value::Map(m)]);
        let d = diagnose("stackup_worst_case", &[chain])
            .expect("diagnose must return Some for non-Length nominal");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::StackupDimMismatch));
    }

    /// (c) sign == Int(2) → StackupBadSign.
    #[test]
    fn diagnose_sign_int_2_returns_stackup_bad_sign() {
        let c = make_bad_sign_contributor(Value::Int(2));
        let chain = Value::List(vec![c]);
        let d = diagnose("stackup_rss", &[chain])
            .expect("diagnose must return Some for sign=Int(2)");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::StackupBadSign));
        assert!(d.message.contains("E_StackupBadSign"),
            "message must contain 'E_StackupBadSign'; got: {:?}", d.message);
    }

    /// (c) sign == Int(0) → StackupBadSign.
    #[test]
    fn diagnose_sign_int_0_returns_stackup_bad_sign() {
        let c = make_bad_sign_contributor(Value::Int(0));
        let chain = Value::List(vec![c]);
        let d = diagnose("stackup_rss", &[chain])
            .expect("diagnose must return Some for sign=Int(0)");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::StackupBadSign));
    }

    /// Helper: a contributor Map with valid Length fields but an arbitrary sign.
    fn make_bad_sign_contributor(sign: Value) -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("nominal".into()),   len(0.010));
        m.insert(Value::String("plus_tol".into()),  len(0.0001));
        m.insert(Value::String("minus_tol".into()), len(0.0001));
        m.insert(Value::String("sign".into()),      sign);
        Value::Map(m)
    }

    /// (d) samples == Int(0) for monte_carlo_stackup → StackupBadSamples.
    #[test]
    fn diagnose_monte_carlo_zero_samples_returns_stackup_bad_samples() {
        let d = diagnose("monte_carlo_stackup", &[golden_chain(), Value::Int(0), Value::Int(42)])
            .expect("diagnose must return Some for samples=0");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::StackupBadSamples));
        assert!(d.message.contains("E_StackupBadSamples"),
            "message must contain 'E_StackupBadSamples'; got: {:?}", d.message);
    }

    /// (e) valid non-empty chain → None (no error).
    #[test]
    fn diagnose_valid_chain_returns_none() {
        assert!(
            diagnose("stackup_rss", &[golden_chain()]).is_none(),
            "valid chain must not produce a diagnostic"
        );
    }

    /// (f) non-stackup function name → None.
    #[test]
    fn diagnose_unknown_function_returns_none() {
        assert!(
            diagnose("not_a_stackup_fn", &[]).is_none(),
            "unknown function must not produce a diagnostic"
        );
    }

    /// Empty chain works for all three stackup math builtins.
    #[test]
    fn diagnose_empty_chain_works_for_all_math_builtins() {
        let empty = Value::List(vec![]);
        for name in &["stackup_worst_case", "stackup_rss", "monte_carlo_stackup"] {
            let args: &[Value] = if *name == "monte_carlo_stackup" {
                &[empty.clone(), Value::Int(10), Value::Int(42)]
            } else {
                std::slice::from_ref(&empty)
            };
            let d = diagnose(name, args)
                .unwrap_or_else(|| panic!("diagnose({name}, empty_chain) must return Some"));
            assert_eq!(d.code, Some(DiagnosticCode::StackupEmptyChain),
                "{name}: expected StackupEmptyChain");
        }
    }
}
