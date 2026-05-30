//! Pure-Rust impulse-shaping convolution for the trajectory stdlib module.
//!
//! Implements the ZV, ZVD, EI (2-hump), and cascaded impulse shapers used by
//! the `input_shape` dispatcher (task ζ).
//!
//! All frequencies are in **angular frequency** (rad/s).  The Hz→rad/s
//! conversion (`ω_n = 2π·f`) is ζ's marshalling boundary, not ε's.
//!
//! # References
//!
//! Singer, N. C., & Seering, W. P. (1990). Preshaping command inputs to reduce
//! system vibration. *Journal of Dynamic Systems, Measurement, and Control*,
//! 112(1), 76–82.
//!
//! Singhose, W. E., Seering, W. P., & Singer, N. C. (1996). Input shaping for
//! vibration reduction with specified insensitivity to modeling errors. *IROS*.
//!
//! # Dead-code suppression
//!
//! The impulse-shaper types here are fully tested but not yet wired to the
//! `eval_trajectory` dispatch layer — that marshalling is owned by ζ (task after
//! ε).  Suppress the lint rather than adding a premature marshalling layer,
//! mirroring the sibling `spline` (spline.rs:14) and `gcode_import`
//! (gcode_import.rs:35) submodules.
#![allow(dead_code)]

/// A single timed impulse: a scalar amplitude applied at a specific time offset.
#[derive(Debug, Clone, PartialEq)]
struct Impulse {
    /// Time offset (seconds, ≥ 0) at which this impulse is applied.
    time: f64,
    /// Amplitude of the impulse (dimensionless; positive by convention for
    /// ZV/ZVD/EI trains, though cascade can in principle produce signed values).
    amplitude: f64,
}

/// An ordered sequence of timed impulses representing a shaper's convolution kernel.
///
/// The impulses are stored in strictly increasing time order (up to floating-point
/// tolerance).  All factory constructors (`zv`, `zvd`, `ei`) uphold this invariant.
#[derive(Debug, Clone)]
pub(crate) struct ImpulseTrain {
    impulses: Vec<Impulse>,
}

// ── EI helpers ────────────────────────────────────────────────────────────────

/// Residual vibration of a 2-impulse ZV shaper designed for frequency
/// ω_n/ρ (with the same ζ/K), evaluated at the target frequency (ω_n, ζ).
///
/// Formula (derived from Singer-Seering §3):
///
/// ```text
/// m = K^(1-ρ)
/// V = K^ρ/(1+K) · √(1 + 2m·cos(πρ) + m²)
/// ```
///
/// Special values: V(ρ=0) = 1, V(ρ=1) = 0, V(ρ=2) = K.
fn zv_residual_at_rho(rho: f64, k: f64) -> f64 {
    let m = k.powf(1.0 - rho);
    let cos_term = (std::f64::consts::PI * rho).cos();
    k.powf(rho) / (1.0 + k) * (1.0 + 2.0 * m * cos_term + m * m).sqrt()
}

/// Bisect for ρ in `(lo, hi)` such that `zv_residual_at_rho(ρ, k) = v_target`.
///
/// `ascending`: true when V is increasing over the interval (use for ρ ∈ (1,2));
/// false when V is decreasing (ρ ∈ (0,1)).
fn bisect_rho(v_target: f64, k: f64, lo: f64, hi: f64, ascending: bool) -> f64 {
    let mut a = lo + f64::EPSILON;
    let mut b = hi - f64::EPSILON;
    for _ in 0..64 {
        let mid = (a + b) * 0.5;
        let v_mid = zv_residual_at_rho(mid, k);
        if ascending {
            if v_mid < v_target { a = mid; } else { b = mid; }
        } else {
            if v_mid > v_target { a = mid; } else { b = mid; }
        }
    }
    (a + b) * 0.5
}

// ── Internal helpers / convolution ───────────────────────────────────────────

/// Convolve two impulse trains: each output impulse has
/// `time = t_a + t_b` and `amplitude = A_a * A_b` for all pairs.
/// Impulses whose times coincide within `MERGE_EPSILON` are merged by
/// summing their amplitudes.  The result is sorted by time.
fn convolve_trains(a: &ImpulseTrain, b: &ImpulseTrain) -> ImpulseTrain {
    const MERGE_EPS: f64 = 1e-10; // seconds

    let mut raw: Vec<(f64, f64)> = Vec::with_capacity(a.impulses.len() * b.impulses.len());
    for ia in &a.impulses {
        for ib in &b.impulses {
            raw.push((ia.time + ib.time, ia.amplitude * ib.amplitude));
        }
    }

    // Sort by time (NaN-free — all times are finite and ≥ 0).
    raw.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // Merge coincident-time impulses.
    let mut merged: Vec<Impulse> = Vec::with_capacity(raw.len());
    for (t, amp) in raw {
        if let Some(last) = merged.last_mut()
            && (last.time - t).abs() < MERGE_EPS
        {
            last.amplitude += amp;
            continue;
        }
        merged.push(Impulse { time: t, amplitude: amp });
    }

    ImpulseTrain { impulses: merged }
}

/// Compute the damped natural frequency `ω_d = ω_n · √(1 − ζ²)` and the
/// exponential decay factor `K = exp(−ζ π / √(1 − ζ²))`.
///
/// For the undamped case (ζ=0): ω_d = ω_n, K = 1.
fn damped_freq_and_k(omega_n: f64, zeta: f64) -> (f64, f64) {
    // Guard against ζ≥1 (critically/over-damped): clamp to a small value so
    // ω_d stays positive. In practice the shaper domain is ζ ∈ [0, 1).
    let zeta_clamped = zeta.min(1.0 - f64::EPSILON.sqrt());
    let sqrt_term = (1.0 - zeta_clamped * zeta_clamped).sqrt();
    let omega_d = omega_n * sqrt_term;
    let k = if zeta_clamped == 0.0 {
        1.0
    } else {
        (-zeta_clamped * std::f64::consts::PI / sqrt_term).exp()
    };
    (omega_d, k)
}

impl ImpulseTrain {
    /// Construct the two-impulse **Zero-Vibration (ZV)** shaper.
    ///
    /// # Parameters
    /// - `omega_n`: natural angular frequency (rad/s), must be > 0.
    /// - `zeta`: damping ratio ζ ∈ [0, 1).
    ///
    /// # Algorithm (Singer & Seering 1990, §3)
    ///
    /// ```text
    /// ω_d = ω_n · √(1 − ζ²)
    /// K   = exp(−ζ π / √(1 − ζ²))
    /// t   = [0,  π / ω_d]
    /// A   = [1/(1+K),  K/(1+K)]
    /// ```
    pub(crate) fn zv(omega_n: f64, zeta: f64) -> ImpulseTrain {
        let (omega_d, k) = damped_freq_and_k(omega_n, zeta);
        let norm = 1.0 + k;
        ImpulseTrain {
            impulses: vec![
                Impulse { time: 0.0,                amplitude: 1.0 / norm },
                Impulse { time: std::f64::consts::PI / omega_d, amplitude: k / norm },
            ],
        }
    }

    /// Construct the three-impulse **Zero-Vibration-Derivative (ZVD)** shaper.
    ///
    /// # Parameters
    /// - `omega_n`: natural angular frequency (rad/s), must be > 0.
    /// - `zeta`: damping ratio ζ ∈ [0, 1).
    ///
    /// # Algorithm
    ///
    /// ```text
    /// ω_d = ω_n · √(1 − ζ²)
    /// K   = exp(−ζ π / √(1 − ζ²))
    /// t   = [0,  π/ω_d,  2π/ω_d]
    /// A   = [1, 2K, K²] / (1 + K)²
    /// ```
    ///
    /// Algebraically equivalent to `cascade([zv(ω,ζ), zv(ω,ζ)])` — used as a
    /// cross-check in the unit tests.
    pub(crate) fn zvd(omega_n: f64, zeta: f64) -> ImpulseTrain {
        let (omega_d, k) = damped_freq_and_k(omega_n, zeta);
        let norm = (1.0 + k) * (1.0 + k);
        let half_period = std::f64::consts::PI / omega_d;
        ImpulseTrain {
            impulses: vec![
                Impulse { time: 0.0,             amplitude: 1.0 / norm },
                Impulse { time: half_period,      amplitude: 2.0 * k / norm },
                Impulse { time: 2.0 * half_period, amplitude: k * k / norm },
            ],
        }
    }

    /// Construct the four-impulse **Extra-Insensitive (EI / 2-hump EI)** shaper.
    ///
    /// # Parameters
    /// - `omega_n`: natural angular frequency (rad/s), must be > 0.
    /// - `zeta`: damping ratio ζ ∈ [0, 1).
    /// - `v_tol`: allowable residual-vibration fraction at the edges of the
    ///   insensitivity band (must be in (0, 1]).
    ///
    /// # Algorithm (Singhose 1996 / PRD §5.1 2-hump EI)
    ///
    /// Standard four-impulse EI design: the four impulse times and amplitudes
    /// are parameterised by `v_tol`; `Σ amplitudes = 1`, all amplitudes > 0,
    /// and the residual vibration at the design frequency is ≤ `v_tol`.
    pub(crate) fn ei(omega_n: f64, zeta: f64, v_tol: f64) -> ImpulseTrain {
        // 2-hump EI: cascade ZV(ω_n/ρ₁, ζ) ⊗ ZV(ω_n/ρ₂, ζ) where
        //   ρ₁ ∈ (0,1): V(ρ₁) = √v_tol  →  high-frequency atom (short spacing)
        //   ρ₂ ∈ (1,2): V(ρ₂) = √v_tol  →  low-frequency  atom (long spacing)
        // Product property: V_cascade = V_A · V_B → √v_tol · √v_tol = v_tol.
        let (omega_d, k) = damped_freq_and_k(omega_n, zeta);

        // Target each atom's residual at √v_tol so cascade = v_tol.
        let sqrt_vtol = v_tol.sqrt().min(k); // clamp to k so ρ₂ search has a root in (1,2)
        let sqrt_vtol = sqrt_vtol.max(1e-12);

        let rho_1 = bisect_rho(sqrt_vtol, k, 0.0, 1.0, false); // V decreasing on (0,1)
        let rho_2 = bisect_rho(sqrt_vtol, k, 1.0, 2.0, true);  // V increasing on (1,2)

        // Impulse spacings for each ZV atom (same K, different ω').
        let tau_1 = std::f64::consts::PI * rho_1 / omega_d; // shorter (ρ₁ < 1)
        let tau_2 = std::f64::consts::PI * rho_2 / omega_d; // longer  (ρ₂ > 1)

        // ZV amplitudes: same K → same a=1/(1+K), b=K/(1+K) for both atoms.
        let a = 1.0 / (1.0 + k);
        let b = k  / (1.0 + k);

        // Cartesian cascade (tau_1 < tau_2 → times are strictly ordered).
        ImpulseTrain {
            impulses: vec![
                Impulse { time: 0.0,           amplitude: a * a },
                Impulse { time: tau_1,         amplitude: a * b },
                Impulse { time: tau_2,         amplitude: b * a },
                Impulse { time: tau_1 + tau_2, amplitude: b * b },
            ],
        }
    }

    /// Convolve a sequence of impulse trains into a single combined train.
    ///
    /// - **Empty slice** → identity train `{(0, 1)}` (unit impulse at t=0).
    /// - **Single-element slice** → returns that train unchanged.
    /// - **Multiple trains** → pairwise convolution fold; coincident-time
    ///   impulses (within a tolerance of 1e-10 s) are merged by summing their
    ///   amplitudes.
    pub(crate) fn cascade(trains: &[ImpulseTrain]) -> ImpulseTrain {
        // Empty → identity unit impulse at t=0.
        if trains.is_empty() {
            return ImpulseTrain {
                impulses: vec![Impulse { time: 0.0, amplitude: 1.0 }],
            };
        }
        // Fold pairwise convolution over the slice.
        trains[1..].iter().fold(trains[0].clone(), |acc, next| {
            convolve_trains(&acc, next)
        })
    }

    /// Sum of all impulse amplitudes (should equal 1.0 for any well-formed shaper).
    pub(crate) fn amplitude_sum(&self) -> f64 {
        self.impulses.iter().map(|imp| imp.amplitude).sum()
    }

    /// Time offset of the last (trailing) impulse (= the shaper delay Δ).
    ///
    /// Returns 0.0 for a single-impulse identity train.
    pub(crate) fn trailing_time(&self) -> f64 {
        self.impulses.last().map(|imp| imp.time).unwrap_or(0.0)
    }

    /// Singer-Seering percentage residual vibration V(ω_n, ζ).
    ///
    /// ```text
    /// C = Σ  A_i · exp(ζ ω_n t_i) · cos(ω_d t_i)
    /// S = Σ  A_i · exp(ζ ω_n t_i) · sin(ω_d t_i)
    /// V = exp(−ζ ω_n t_N) · √(C² + S²)
    /// ```
    ///
    /// where `t_N` is the time of the last impulse and `ω_d = ω_n √(1−ζ²)`.
    ///
    /// A single unit impulse `{(0, 1)}` produces `V = 1` (the baseline used for
    /// the ≥ 40 dB suppression check).
    pub(crate) fn residual_vibration(&self, omega_n: f64, zeta: f64) -> f64 {
        if self.impulses.is_empty() {
            return 0.0;
        }
        let zeta_c = zeta.min(1.0 - f64::EPSILON.sqrt());
        let sqrt_term = (1.0 - zeta_c * zeta_c).sqrt();
        let omega_d = omega_n * sqrt_term;

        let mut c_sum = 0.0_f64;
        let mut s_sum = 0.0_f64;
        for imp in &self.impulses {
            let factor = (zeta_c * omega_n * imp.time).exp() * imp.amplitude;
            c_sum += factor * (omega_d * imp.time).cos();
            s_sum += factor * (omega_d * imp.time).sin();
        }

        let t_n = self.impulses.last().map(|i| i.time).unwrap_or(0.0);
        (-zeta_c * omega_n * t_n).exp() * (c_sum * c_sum + s_sum * s_sum).sqrt()
    }
}

/// Evaluate the shaped command at time `t` by convolving `train` against `f`
/// clamped to the domain `[0, t_domain]`.
///
/// ```text
/// shaped(t) = Σ  A_i · f_clamped(t − t_i)
/// ```
///
/// where `f_clamped(τ) = f(τ.clamp(0, t_domain))`.
///
/// # Shaped duration
///
/// The output remains valid for `t ∈ [0, t_domain + train.trailing_time()]`.
/// After that the final value is frozen (all samples beyond `t_domain` clamp to
/// `f(t_domain)` and `Σ A_i = 1`).
pub(crate) fn convolve_at<F: Fn(f64) -> f64>(
    train: &ImpulseTrain,
    f: &F,
    t_domain: f64,
    t: f64,
) -> f64 {
    train.impulses.iter().map(|imp| {
        let tau = (t - imp.time).clamp(0.0, t_domain);
        imp.amplitude * f(tau)
    }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Assert two f64 values are within `eps` of each other.
    fn assert_close(a: f64, b: f64, eps: f64, label: &str) {
        assert!(
            (a - b).abs() < eps,
            "{label}: expected {a:.6e} ≈ {b:.6e} (tolerance {eps:.0e})"
        );
    }

    // ── step-11: convolve_at ─────────────────────────────────────────────────

    /// convolve_at: endpoint clamping, shaped-duration, start/final-value preservation,
    /// and interior sample match.
    #[test]
    fn convolve_at_properties() {
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.05;
        let train = ImpulseTrain::zv(omega_n, zeta);
        let delta = train.trailing_time();
        let t_domain = 0.5_f64; // half-second ramp domain

        // Linear ramp f(t) = t, clamped to [0, t_domain].
        let ramp = |t: f64| t.clamp(0.0, t_domain);

        // (a) endpoint clamping: at t < t₁ (= 0), f_clamped(t-t_i) = f(0).
        //     For t=0: both impulses contribute f_clamped(0-0)=0 and f_clamped(0-delta)<0→clamped to 0.
        //     shaped(0) = A₀·f(0) + A₁·f(clamp(0-delta)) = A₀·0 + A₁·0 = 0 = f(0).
        let shaped_at_0 = convolve_at(&train, &ramp, t_domain, 0.0);
        assert_close(shaped_at_0, ramp(0.0), 1e-12, "start preservation");

        // (b) final-value preservation: shaped(t_domain+Δ) = f(t_domain) because Σ Aᵢ=1
        //     and all arguments t - t_i ≥ t_domain → clamp to t_domain.
        let t_final = t_domain + delta;
        let shaped_at_final = convolve_at(&train, &ramp, t_domain, t_final);
        assert_close(shaped_at_final, ramp(t_domain), 1e-12, "final-value preservation");

        // (c) shaped_duration = t_domain + trailing_time().
        let expected_duration = t_domain + delta;
        assert_close(
            expected_duration,
            t_domain + train.trailing_time(),
            1e-12,
            "shaped duration",
        );

        // (d) interior sample: for the identity train {(0,1)}, shaped(t) == f(t).
        let identity = ImpulseTrain {
            impulses: vec![Impulse { time: 0.0, amplitude: 1.0 }],
        };
        let t_mid = t_domain * 0.6;
        assert_close(
            convolve_at(&identity, &ramp, t_domain, t_mid),
            ramp(t_mid),
            1e-12,
            "identity convolve equals f",
        );

        // (e) manual interior sample for ZV train: at t = t_domain / 2.
        let t_eval = t_domain / 2.0;
        let expected_manual = train.impulses.iter().map(|imp| {
            imp.amplitude * ramp((t_eval - imp.time).clamp(0.0, t_domain))
        }).sum::<f64>();
        assert_close(
            convolve_at(&train, &ramp, t_domain, t_eval),
            expected_manual,
            1e-14,
            "ZV interior sample",
        );
    }

    // ── step-9: EI train construction ────────────────────────────────────────

    /// EI (2-hump): exactly 4 impulses; amplitude_sum≈1; all amplitudes > 0;
    /// times strictly increasing from t1=0; residual_vibration(design) ≤ v_tol + 1e-9.
    #[test]
    fn ei_train_construction_invariants() {
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.0_f64;
        let v_tol = 0.05_f64;
        let train = ImpulseTrain::ei(omega_n, zeta, v_tol);

        assert_eq!(train.impulses.len(), 4, "EI must have exactly 4 impulses");

        // All amplitudes strictly positive.
        for (i, imp) in train.impulses.iter().enumerate() {
            assert!(imp.amplitude > 0.0, "EI A[{i}] must be > 0, got {}", imp.amplitude);
        }

        // Times strictly increasing.
        assert_close(train.impulses[0].time, 0.0, 1e-12, "EI t0 must be 0");
        for i in 1..4 {
            assert!(
                train.impulses[i].time > train.impulses[i - 1].time,
                "EI times must be strictly increasing: t[{}]={} <= t[{}]={}",
                i, train.impulses[i].time, i - 1, train.impulses[i - 1].time
            );
        }

        // Σ amplitudes = 1.
        assert_close(train.amplitude_sum(), 1.0, 1e-12, "EI amplitude_sum");

        // Residual ≤ v_tol at design frequency.
        let v = train.residual_vibration(omega_n, zeta);
        assert!(
            v <= v_tol + 1e-9,
            "EI residual at design should be ≤ v_tol={v_tol}, got {v:.6e}"
        );
    }

    // ── step-7: cascade ──────────────────────────────────────────────────────

    /// cascade([]) → identity {(0,1)}.
    #[test]
    fn cascade_empty_is_identity() {
        let id = ImpulseTrain::cascade(&[]);
        assert_eq!(id.impulses.len(), 1, "empty cascade must be unit impulse");
        assert_close(id.impulses[0].time, 0.0, 1e-12, "identity t0");
        assert_close(id.impulses[0].amplitude, 1.0, 1e-12, "identity A0");
        assert_close(id.amplitude_sum(), 1.0, 1e-12, "identity amplitude_sum");
    }

    /// cascade([train]) == that train (single-element identity).
    #[test]
    fn cascade_single_is_identity() {
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.1;
        let zv = ImpulseTrain::zv(omega_n, zeta);
        let cascaded = ImpulseTrain::cascade(&[zv.clone()]);
        assert_eq!(cascaded.impulses.len(), zv.impulses.len());
        for (c, z) in cascaded.impulses.iter().zip(&zv.impulses) {
            assert_close(c.time, z.time, 1e-12, "single cascade time");
            assert_close(c.amplitude, z.amplitude, 1e-12, "single cascade amp");
        }
    }

    /// cascade([zv, zv]) == zvd (ZVD ≡ ZV⊛ZV identity, within 1e-12).
    #[test]
    fn cascade_zv_zv_equals_zvd() {
        let omega_n = 2.0 * PI * 8.0;
        let zeta = 0.1_f64;
        let zv = ImpulseTrain::zv(omega_n, zeta);
        let zvd = ImpulseTrain::zvd(omega_n, zeta);
        let cascaded = ImpulseTrain::cascade(&[zv.clone(), zv.clone()]);

        assert_eq!(
            cascaded.impulses.len(),
            zvd.impulses.len(),
            "cascade(zv,zv) must have same number of impulses as zvd"
        );
        for (c, z) in cascaded.impulses.iter().zip(&zvd.impulses) {
            assert_close(c.time, z.time, 1e-12, "cascade(zv,zv) time vs zvd");
            assert_close(c.amplitude, z.amplitude, 1e-12, "cascade(zv,zv) amp vs zvd");
        }
        assert_close(cascaded.amplitude_sum(), 1.0, 1e-12, "cascade(zv,zv) amplitude_sum");
    }

    // ── step-5: ZVD train construction ──────────────────────────────────────

    /// ZVD: exactly 3 impulses; times [0, π/ω_d, 2π/ω_d]; amplitudes [1,2K,K²]/(1+K)²;
    /// amplitude_sum≈1; residual_vibration at design ≤1e-12.
    #[test]
    fn zvd_train_construction_and_zero_residual() {
        let omega_n = 2.0 * PI * 8.0;
        let zeta = 0.1_f64;

        let omega_d = omega_n * (1.0 - zeta * zeta).sqrt();
        let k = (-zeta * PI / (1.0 - zeta * zeta).sqrt()).exp();
        let norm = (1.0 + k) * (1.0 + k);

        let train = ImpulseTrain::zvd(omega_n, zeta);

        assert_eq!(train.impulses.len(), 3, "ZVD must have 3 impulses");
        assert_close(train.impulses[0].time, 0.0, 1e-12, "ZVD t0");
        assert_close(train.impulses[1].time, PI / omega_d, 1e-12, "ZVD t1");
        assert_close(train.impulses[2].time, 2.0 * PI / omega_d, 1e-12, "ZVD t2");
        assert_close(train.impulses[0].amplitude, 1.0 / norm, 1e-12, "ZVD A0");
        assert_close(train.impulses[1].amplitude, 2.0 * k / norm, 1e-12, "ZVD A1");
        assert_close(train.impulses[2].amplitude, k * k / norm, 1e-12, "ZVD A2");
        assert_close(train.amplitude_sum(), 1.0, 1e-12, "ZVD amplitude_sum");

        let v = train.residual_vibration(omega_n, zeta);
        assert!(
            v.abs() <= 1e-12,
            "ZVD residual at design should be ≈0, got {v:.3e}"
        );
    }

    // ── step-3: residual_vibration ───────────────────────────────────────────

    /// A unit impulse at t=0 has V=1 (baseline).
    #[test]
    fn residual_vibration_unit_impulse_is_one() {
        let unit = ImpulseTrain {
            impulses: vec![Impulse { time: 0.0, amplitude: 1.0 }],
        };
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.05;
        assert_close(unit.residual_vibration(omega_n, zeta), 1.0, 1e-12, "unit impulse V=1");
    }

    /// ZV train at design (ω_n, ζ) has V≈0 (≤1e-12) — proven telescoping identity.
    #[test]
    fn residual_vibration_zv_at_design_is_zero() {
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.05;
        let train = ImpulseTrain::zv(omega_n, zeta);
        let v = train.residual_vibration(omega_n, zeta);
        assert!(
            v.abs() <= 1e-12,
            "ZV residual at design should be ≈0, got {v:.3e}"
        );
    }

    /// ≥40 dB suppression: V_zv ≤ 0.01·V_unshaped at design frequency.
    #[test]
    fn residual_vibration_zv_40db_suppression() {
        let omega_n = 2.0 * PI * 10.0; // 10 Hz
        let zeta = 0.05;
        let unit = ImpulseTrain {
            impulses: vec![Impulse { time: 0.0, amplitude: 1.0 }],
        };
        let v_unshaped = unit.residual_vibration(omega_n, zeta);
        let v_shaped = ImpulseTrain::zv(omega_n, zeta).residual_vibration(omega_n, zeta);
        // V_unshaped = 1.0 by the identity above; V_shaped ≈ 0 → ratio ≤ 0.01
        assert!(
            v_shaped <= 0.01 * v_unshaped,
            "ZV must suppress ≥40 dB; V_shaped={v_shaped:.3e}, 0.01*V_unshaped={:.3e}",
            0.01 * v_unshaped
        );
    }

    // ── step-1: ZV train construction ────────────────────────────────────────

    /// ZV undamped (ζ=0): amplitudes [0.5, 0.5] at times [0, π/ω_n].
    #[test]
    fn zv_undamped_amplitudes_and_times() {
        let omega_n = 2.0 * PI * 10.0; // 10 Hz → rad/s
        let train = ImpulseTrain::zv(omega_n, 0.0);

        assert_eq!(train.impulses.len(), 2, "ZV must have exactly 2 impulses");

        // Times: [0, π/ω_n] for ζ=0 (ω_d = ω_n)
        assert_close(train.impulses[0].time, 0.0, 1e-12, "ZV undamped t0");
        assert_close(
            train.impulses[1].time,
            PI / omega_n,
            1e-12,
            "ZV undamped t1",
        );

        // Amplitudes: [0.5, 0.5] for ζ=0 (K=1)
        assert_close(train.impulses[0].amplitude, 0.5, 1e-12, "ZV undamped A0");
        assert_close(train.impulses[1].amplitude, 0.5, 1e-12, "ZV undamped A1");

        // Σ amplitudes = 1
        assert_close(train.amplitude_sum(), 1.0, 1e-12, "ZV undamped amplitude_sum");

        // trailing_time == last impulse time
        assert_close(
            train.trailing_time(),
            PI / omega_n,
            1e-12,
            "ZV undamped trailing_time",
        );
    }

    /// ZV damped (ζ=0.1): verify K and ω_d formulas.
    #[test]
    fn zv_damped_amplitudes_and_times() {
        let omega_n = 2.0 * PI * 5.0; // 5 Hz
        let zeta = 0.1_f64;

        let omega_d = omega_n * (1.0 - zeta * zeta).sqrt();
        let k = (-zeta * PI / (1.0 - zeta * zeta).sqrt()).exp();
        let a0 = 1.0 / (1.0 + k);
        let a1 = k / (1.0 + k);
        let t1 = PI / omega_d;

        let train = ImpulseTrain::zv(omega_n, zeta);

        assert_eq!(train.impulses.len(), 2);
        assert_close(train.impulses[0].time, 0.0, 1e-12, "ZV damped t0");
        assert_close(train.impulses[1].time, t1, 1e-12, "ZV damped t1");
        assert_close(train.impulses[0].amplitude, a0, 1e-12, "ZV damped A0");
        assert_close(train.impulses[1].amplitude, a1, 1e-12, "ZV damped A1");
        assert_close(train.amplitude_sum(), 1.0, 1e-12, "ZV damped amplitude_sum");
        assert_close(train.trailing_time(), t1, 1e-12, "ZV damped trailing_time");
    }
}
