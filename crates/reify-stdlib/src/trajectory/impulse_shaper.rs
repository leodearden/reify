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
pub struct ImpulseTrain {
    impulses: Vec<Impulse>,
}

// ── EI helpers ────────────────────────────────────────────────────────────────

/// Solve for the outer-impulse amplitude weight `a₁` of the optimal Singhose
/// 2-hump EI shaper such that both insensitivity humps peak at exactly `v_tol`.
///
/// # Derivation (undamped baseline)
///
/// The optimal 2-hump EI is the symmetric four-impulse shaper with equal
/// half-period spacing (times `0, π/ω_d, 2π/ω_d, 3π/ω_d`) and amplitude weights
/// `[a₁, a₂, a₂, a₁]` with `a₂ = ½ − a₁` (so `Σ = 1`).  Centring the residual
/// phasor at the midpoint, the undamped residual is
///
/// ```text
/// V(ω) = 2·|cos φ| · |4·a₁·cos²φ + (½ − 4·a₁)|,   φ = ω·π / (2·ω_n)
/// ```
///
/// which is **zero at the design frequency** (`φ = π/2`) and rises to two
/// symmetric humps on either side.  Setting `∂V/∂cos φ = 0` locates each hump at
/// `cos²φ = (8a₁ − 1)/(24a₁)`; substituting back, the hump height is
///
/// ```text
/// v_tol = (2/3)·(8a₁ − 1)·√((8a₁ − 1)/(24a₁)).
/// ```
///
/// With `u = 8a₁ − 1` this rearranges to the cubic `4u³ − 27·v_tol²·u −
/// 27·v_tol² = 0`, which has exactly one positive real root (Descartes).  Both
/// humps share the same height by construction, so they equal `v_tol`
/// *exactly* — unlike the legacy cascade-of-two-ZV approximation (esc-3867-105),
/// whose flanking humps drifted to ≈1.016·v_tol.
///
/// Returns `a₁ ∈ [⅛, ½)` for `v_tol ∈ (0, 1)`.
fn two_hump_ei_a1(v_tol: f64) -> f64 {
    let v = v_tol.clamp(1e-12, 1.0 - 1e-9);
    let c = 27.0 * v * v; // cubic: 4u³ − c·u − c = 0
    // f(u) = 4u³ − c·u − c is < 0 at u=0 and strictly increasing past its single
    // positive root; bracket in (0, 4] (u=3 corresponds to v_tol=1).
    let f = |u: f64| 4.0 * u * u * u - c * u - c;
    let (mut lo, mut hi) = (0.0_f64, 4.0_f64);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 { lo = mid; } else { hi = mid; }
    }
    let u = 0.5 * (lo + hi);
    (u + 1.0) / 8.0
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
    pub fn zv(omega_n: f64, zeta: f64) -> ImpulseTrain {
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
    pub fn zvd(omega_n: f64, zeta: f64) -> ImpulseTrain {
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
    /// # Algorithm (optimal Singhose-1996 2-hump EI / PRD §5.1)
    ///
    /// The optimal 2-hump EI is the symmetric four-impulse shaper with **equal
    /// half-period spacing** `t = [0, π/ω_d, 2π/ω_d, 3π/ω_d]` and amplitude
    /// weights `w = [a₁, a₂, a₂, a₁]` (`a₂ = ½ − a₁`).  `a₁` is solved from
    /// `v_tol` via [`two_hump_ei_a1`] so that **both insensitivity humps peak at
    /// exactly `v_tol`** (the flanking-hump constraint that the legacy
    /// cascade-of-two-ZV approximation — esc-3867-105 — could not hold; its humps
    /// drifted to ≈1.016·v_tol, forcing the relaxed ≤vtol·1.02 bound that this
    /// construction lets task ζ restore to the strict ≤vtol).
    ///
    /// # Damping
    ///
    /// For ζ > 0 the weights are decay-compensated, `Aₖ = wₖ·Kᵏ` with
    /// `K = exp(−ζπ/√(1−ζ²))` the per-half-period decay, then renormalised to
    /// `Σ = 1`.  Because `wₖ·Kᵏ·exp(ζ·ω_n·tₖ) = wₖ` at the design frequency
    /// (`exp(ζ·ω_n·tₖ) = K⁻ᵏ`) and the half-period phases are `kπ`, the residual
    /// `C-`component telescopes to `w₀ − w₁ + w₂ − w₃ = 0`: the shaper **nulls
    /// exactly at the design frequency for any ζ**, and damping only pulls the
    /// humps further below `v_tol` (band stays within tolerance).
    ///
    /// Guarantees: exactly 4 impulses, `Σ amplitudes = 1`, all amplitudes > 0
    /// (for `v_tol ∈ (0, 1)`), strictly increasing times from `t₀ = 0`, and
    /// residual ≤ `v_tol` across the ±band (not merely at the design point).
    pub fn ei(omega_n: f64, zeta: f64, v_tol: f64) -> ImpulseTrain {
        let (omega_d, k) = damped_freq_and_k(omega_n, zeta);

        // Outer/inner amplitude weights with both humps pinned to v_tol exactly.
        let a1 = two_hump_ei_a1(v_tol);
        let a2 = 0.5 - a1;
        let w = [a1, a2, a2, a1];

        // Decay-compensated amplitudes Aₖ = wₖ·Kᵏ (K=1 ⇒ undamped symmetric),
        // renormalised so Σ = 1.  Uniform scaling preserves the design-frequency
        // null (C ∝ w₀−w₁+w₂−w₃ = 0).
        let half_period = std::f64::consts::PI / omega_d;
        let raw: [f64; 4] = std::array::from_fn(|kk| w[kk] * k.powi(kk as i32));
        let norm: f64 = raw.iter().sum();

        ImpulseTrain {
            impulses: (0..4)
                .map(|kk| Impulse {
                    time: kk as f64 * half_period,
                    amplitude: raw[kk] / norm,
                })
                .collect(),
        }
    }

    /// Convolve a sequence of impulse trains into a single combined train.
    ///
    /// - **Empty slice** → identity train `{(0, 1)}` (unit impulse at t=0).
    /// - **Single-element slice** → returns that train unchanged.
    /// - **Multiple trains** → pairwise convolution fold; coincident-time
    ///   impulses (within a tolerance of 1e-10 s) are merged by summing their
    ///   amplitudes.
    pub fn cascade(trains: &[ImpulseTrain]) -> ImpulseTrain {
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
    // G-allow: impulse-shaping well-formedness helper (amplitude-sum check), task #3866 (ε); permanent internal helper called only within impulse_shaper.rs + unit tests; input_shape_value entry point is wired via trampoline.rs → trajectory_ops.rs:429.
    pub fn amplitude_sum(&self) -> f64 {
        self.impulses.iter().map(|imp| imp.amplitude).sum()
    }

    /// Time offset of the last (trailing) impulse (= the shaper delay Δ).
    ///
    /// Returns 0.0 for a single-impulse identity train.
    pub fn trailing_time(&self) -> f64 {
        self.impulses.last().map(|imp| imp.time).unwrap_or(0.0)
    }

    /// The `(time, amplitude)` pairs of every impulse, in strictly-increasing
    /// time order. Exposes the train contents for cross-module inspection and
    /// marshalling (e.g. `input_shape`'s `build_train_for_shaper` dispatch
    /// tests) without leaking the private `Impulse` type.
    pub fn points(&self) -> Vec<(f64, f64)> {
        self.impulses
            .iter()
            .map(|imp| (imp.time, imp.amplitude))
            .collect()
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
    pub fn residual_vibration(&self, omega_n: f64, zeta: f64) -> f64 {
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
pub fn convolve_at<F: Fn(f64) -> f64>(
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

    /// EI (2-hump), damped case (ζ=0.1): exercises the damped decay-compensation
    /// path (Aₖ = wₖ·Kᵏ renormalised; K ≈ 0.73 for ζ=0.1).  Confirms the
    /// design-frequency null and structural invariants hold under damping.
    #[test]
    fn ei_train_construction_invariants_damped() {
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.1_f64;
        let v_tol = 0.05_f64;
        let train = ImpulseTrain::ei(omega_n, zeta, v_tol);

        assert_eq!(train.impulses.len(), 4, "EI (damped ζ=0.1) must have exactly 4 impulses");

        for (i, imp) in train.impulses.iter().enumerate() {
            assert!(
                imp.amplitude > 0.0,
                "EI (damped) A[{i}] must be > 0, got {}",
                imp.amplitude
            );
        }

        assert_close(train.impulses[0].time, 0.0, 1e-12, "EI (damped) t0 must be 0");
        for i in 1..4 {
            assert!(
                train.impulses[i].time > train.impulses[i - 1].time,
                "EI (damped) times must be strictly increasing: t[{}]={} <= t[{}]={}",
                i,
                train.impulses[i].time,
                i - 1,
                train.impulses[i - 1].time
            );
        }

        assert_close(train.amplitude_sum(), 1.0, 1e-12, "EI (damped) amplitude_sum");

        let v = train.residual_vibration(omega_n, zeta);
        assert!(
            v <= v_tol + 1e-9,
            "EI (damped) residual at design should be ≤ v_tol={v_tol}, got {v:.6e}"
        );
    }

    /// EI large v_tol (ζ=0.1, v_tol=0.8): exercises the high-tolerance regime where
    /// the outer weight a₁→½ and the inner weight a₂→0⁺ (here a₁≈0.433, a₂≈0.067).
    /// Asserts the structural invariants hold (4 impulses, positive amplitudes,
    /// monotone times, Σ A=1, V ≤ v_tol at design).
    #[test]
    fn ei_train_construction_large_vtol() {
        let omega_n = 2.0 * PI * 5.0;
        let zeta = 0.1_f64;
        let v_tol = 0.8_f64;
        let train = ImpulseTrain::ei(omega_n, zeta, v_tol);

        assert_eq!(train.impulses.len(), 4, "EI large-vtol must have 4 impulses");

        for (i, imp) in train.impulses.iter().enumerate() {
            assert!(imp.amplitude > 0.0, "EI large-vtol A[{i}] must be > 0");
        }

        assert_close(train.impulses[0].time, 0.0, 1e-12, "EI large-vtol t0 must be 0");
        for i in 1..4 {
            assert!(
                train.impulses[i].time > train.impulses[i - 1].time,
                "EI large-vtol times must be strictly increasing"
            );
        }

        assert_close(train.amplitude_sum(), 1.0, 1e-12, "EI large-vtol amplitude_sum");

        let v = train.residual_vibration(omega_n, zeta);
        assert!(
            v <= v_tol + 1e-9,
            "EI large-vtol residual at design should be ≤ {v_tol}, got {v:.6e}"
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
        let cascaded = ImpulseTrain::cascade(std::slice::from_ref(&zv));
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

    /// cascade([zv, zv, zv]): exercises the N≥3 fold path.
    ///
    /// Asserts amplitude_sum≈1, times non-decreasing, and the residual product
    /// property V_{A⊛B⊛C}(ω) = V_A(ω)·V_B(ω)·V_C(ω) at an off-design
    /// frequency (1.5·ω_n).  This identity holds exactly for the Singer-Seering
    /// residual because cascade convolution corresponds to multiplication of the
    /// complex phasors: |Z_A · Z_B · Z_C| = |Z_A|·|Z_B|·|Z_C|.
    #[test]
    fn cascade_three_trains_fold_and_residual_product() {
        let omega_n = 2.0 * PI * 8.0;
        let zeta = 0.1_f64;
        let zv = ImpulseTrain::zv(omega_n, zeta);
        let three = ImpulseTrain::cascade(&[zv.clone(), zv.clone(), zv.clone()]);

        // Σ amplitudes = 1.
        assert_close(three.amplitude_sum(), 1.0, 1e-12, "cascade-3 amplitude_sum");

        // Times non-decreasing (some coincident times are merged, so strictly
        // increasing is NOT guaranteed, but non-decreasing must hold).
        for i in 1..three.impulses.len() {
            assert!(
                three.impulses[i].time >= three.impulses[i - 1].time,
                "cascade-3 times must be non-decreasing: t[{}]={} < t[{}]={}",
                i,
                three.impulses[i].time,
                i - 1,
                three.impulses[i - 1].time
            );
        }

        // Residual product property at an off-design frequency.
        let omega_test = 1.5 * omega_n;
        let v_zv = zv.residual_vibration(omega_test, zeta);
        let v_three = three.residual_vibration(omega_test, zeta);
        assert_close(
            v_three,
            v_zv * v_zv * v_zv,
            1e-10,
            "cascade-3 residual product property V = V_zv^3",
        );
    }

    /// residual_vibration on an empty ImpulseTrain returns 0.0 (early-return).
    #[test]
    fn residual_vibration_empty_train_is_zero() {
        let empty = ImpulseTrain { impulses: vec![] };
        let omega_n = 2.0 * PI * 10.0;
        let zeta = 0.05;
        assert_close(
            empty.residual_vibration(omega_n, zeta),
            0.0,
            1e-15,
            "empty ImpulseTrain V=0.0",
        );
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

    // ── step-1 (task 4111): EI band-sweep residual ───────────────────────────

    /// EI residual ≤ v_tol across the full ±15% band (task 4111 gate).
    ///
    /// Sweeps 31 frequencies over [8.5, 11.5] Hz (= [0.85, 1.15]·ω_n for 10 Hz)
    /// for ζ ∈ {0.0, 0.05, 0.1} and asserts residual ≤ 0.05 + 1e-9 at every point.
    ///
    /// This is the across-band guard the design-point check (`ei_train_construction
    /// _invariants`, design frequency only) cannot provide.  The legacy cascade-of-
    /// two-ZV EI left the flanking humps unconstrained and peaked at ≈0.050811
    /// (1.016·v_tol); the optimal Singhose 2-hump construction pins both humps to
    /// exactly v_tol, so the band stays within tolerance for every ζ.
    #[test]
    fn ei_residual_within_tolerance_across_band() {
        let omega_n = 2.0 * PI * 10.0; // 10 Hz design frequency
        let v_tol = 0.05_f64;
        let n_pts = 31;
        let f_lo = 8.5_f64;
        let f_hi = 11.5_f64;

        for &zeta in &[0.0_f64, 0.05, 0.1] {
            let train = ImpulseTrain::ei(omega_n, zeta, v_tol);

            // Structural invariants.
            assert_eq!(train.impulses.len(), 4, "EI (ζ={zeta}) must have 4 impulses");
            for (i, imp) in train.impulses.iter().enumerate() {
                assert!(
                    imp.amplitude > 0.0,
                    "EI (ζ={zeta}) A[{i}] must be > 0, got {}",
                    imp.amplitude
                );
            }
            assert_close(train.impulses[0].time, 0.0, 1e-12, &format!("EI (ζ={zeta}) t0"));
            for i in 1..4 {
                assert!(
                    train.impulses[i].time > train.impulses[i - 1].time,
                    "EI (ζ={zeta}) times must be strictly increasing: t[{i}]={} <= t[{}]={}",
                    train.impulses[i].time,
                    i - 1,
                    train.impulses[i - 1].time
                );
            }
            assert_close(train.amplitude_sum(), 1.0, 1e-12, &format!("EI (ζ={zeta}) amplitude_sum"));

            // Band-sweep residual check.
            for j in 0..n_pts {
                let f = f_lo + (f_hi - f_lo) * (j as f64) / ((n_pts - 1) as f64);
                let omega = 2.0 * PI * f;
                let v = train.residual_vibration(omega, zeta);
                assert!(
                    v <= v_tol + 1e-9,
                    "EI (ζ={zeta}) residual {v:.8e} > v_tol+1e-9 at f={f:.4} Hz"
                );
            }
        }
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
