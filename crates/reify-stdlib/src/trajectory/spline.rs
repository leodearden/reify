//! Pure-Rust spline math for the trajectory stdlib module.
//!
//! Implements interpolating cubic and quintic B-splines used by
//! `piecewise_polynomial` / `evaluate_profile*` / `profile_duration`.
//!
//! This module has no `reify_types` dependency — all inputs and outputs are
//! plain `f64` / `Vec<f64>`.  Value marshalling lives in `mod.rs`.
//!
//! # Dead-code suppression
//!
//! The spline types here are fully tested but not yet wired to `eval_trajectory`
//! (Value marshalling is a later phase — γ/η/θ per the β PRD scope boundary).
//! Suppress the lint rather than adding a premature marshalling layer.
#![allow(dead_code)]

/// Boundary condition for cubic interpolating splines.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoundaryCondition {
    /// Natural: second derivatives at endpoints are zero.
    Natural,
    /// Clamped: first derivatives at endpoints are prescribed.
    Clamped { start_vel: f64, end_vel: f64 },
    /// Periodic: function and derivatives wrap around (C2 continuity across seam).
    Periodic,
}

/// A single-joint cubic interpolating spline.
///
/// Represented as piecewise degree-3 polynomials stored as per-segment
/// coefficients: for segment i in [t[i], t[i+1]], the value at t is
///   a[i] + b[i]*s + c[i]*s^2 + d[i]*s^3  where s = t - t[i]
#[derive(Debug, Clone)]
pub(crate) struct CubicSpline {
    /// Knot times (strictly increasing), length n.
    knots: Vec<f64>,
    /// Coefficient a[i] = value at knot i, length n-1.
    a: Vec<f64>,
    /// Coefficient b[i] (first deriv at knot i), length n-1.
    b: Vec<f64>,
    /// Coefficient c[i] (half second deriv at knot i), length n-1.
    c: Vec<f64>,
    /// Coefficient d[i], length n-1.
    d: Vec<f64>,
}

impl CubicSpline {
    /// Fit a cubic interpolating spline through (knots[i], values[i]).
    ///
    /// Returns `None` if:
    /// - fewer than 2 knots
    /// - knots not strictly increasing
    /// - knots.len() != values.len()
    pub(crate) fn fit(
        knots: &[f64],
        values: &[f64],
        bc: &BoundaryCondition,
    ) -> Option<Self> {
        let n = knots.len();
        if n < 2 || n != values.len() {
            return None;
        }
        // Reject non-finite inputs early so NaN/Inf can never propagate into
        // coefficients and produce nonsensical eval results downstream.
        if !knots.iter().all(|k| k.is_finite()) || !values.iter().all(|v| v.is_finite()) {
            return None;
        }
        // Check strictly increasing
        for i in 0..n - 1 {
            if knots[i + 1] <= knots[i] {
                return None;
            }
        }
        // For Clamped BC, also reject non-finite velocity prescriptions.
        if let BoundaryCondition::Clamped { start_vel, end_vel } = bc
            && (!start_vel.is_finite() || !end_vel.is_finite())
        {
            return None;
        }

        match bc {
            BoundaryCondition::Natural => Self::fit_natural(knots, values),
            BoundaryCondition::Clamped { start_vel, end_vel } => {
                Self::fit_clamped(knots, values, *start_vel, *end_vel)
            }
            BoundaryCondition::Periodic => Self::fit_periodic(knots, values),
        }
    }

    /// Natural cubic spline: second derivatives at endpoints = 0.
    fn fit_natural(knots: &[f64], values: &[f64]) -> Option<Self> {
        let n = knots.len();
        let m = n - 1; // number of segments

        if n == 2 {
            // Linear spline for 2 points
            let h = knots[1] - knots[0];
            let slope = (values[1] - values[0]) / h;
            return Some(CubicSpline {
                knots: knots.to_vec(),
                a: vec![values[0]],
                b: vec![slope],
                c: vec![0.0],
                d: vec![0.0],
            });
        }

        // Build and solve tridiagonal system for second derivatives M[i].
        // Natural: M[0] = M[n-1] = 0.
        // For i = 1..n-2:
        //   h[i-1]*M[i-1] + 2*(h[i-1]+h[i])*M[i] + h[i]*M[i+1] = 6*((values[i+1]-values[i])/h[i] - (values[i]-values[i-1])/h[i-1])
        let h: Vec<f64> = (0..m).map(|i| knots[i + 1] - knots[i]).collect();

        let inner = n - 2; // number of interior knots
        if inner == 0 {
            unreachable!("handled by n==2 case above");
        }

        let mut diag = vec![0.0_f64; inner];
        let mut upper = vec![0.0_f64; inner - 1];
        let mut lower = vec![0.0_f64; inner - 1];
        let mut rhs = vec![0.0_f64; inner];

        for i in 0..inner {
            let ki = i + 1; // knot index in full array
            diag[i] = 2.0 * (h[ki - 1] + h[ki]);
            let rhs_val = 6.0
                * ((values[ki + 1] - values[ki]) / h[ki]
                    - (values[ki] - values[ki - 1]) / h[ki - 1]);
            rhs[i] = rhs_val;
            if i + 1 < inner {
                upper[i] = h[ki];
                lower[i] = h[ki];
            }
        }

        let m_inner = solve_tridiagonal(&lower, &diag, &upper, &rhs)?;

        // Full second derivative array (M[0]=0, M[n-1]=0)
        let mut m_vals = vec![0.0_f64; n];
        m_vals[1..(inner + 1)].copy_from_slice(&m_inner[..inner]);

        Self::from_second_derivatives(knots, values, &h, &m_vals)
    }

    /// Build segment coefficients from second derivatives M.
    fn from_second_derivatives(
        knots: &[f64],
        values: &[f64],
        h: &[f64],
        m: &[f64],
    ) -> Option<Self> {
        let n = knots.len();
        let segs = n - 1;
        let mut a = Vec::with_capacity(segs);
        let mut b = Vec::with_capacity(segs);
        let mut c = Vec::with_capacity(segs);
        let mut d = Vec::with_capacity(segs);
        for i in 0..segs {
            let hi = h[i];
            a.push(values[i]);
            b.push((values[i + 1] - values[i]) / hi - hi * (2.0 * m[i] + m[i + 1]) / 6.0);
            c.push(m[i] / 2.0);
            d.push((m[i + 1] - m[i]) / (6.0 * hi));
        }
        Some(CubicSpline {
            knots: knots.to_vec(),
            a,
            b,
            c,
            d,
        })
    }

    /// Find the segment index for a given t (clamped to valid range).
    fn segment(&self, t: f64) -> usize {
        let n = self.knots.len();
        let segs = n - 1;
        if t <= self.knots[0] {
            return 0;
        }
        if t >= self.knots[n - 1] {
            return segs - 1;
        }
        // Binary search
        let mut lo = 0usize;
        let mut hi = segs - 1;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.knots[mid] <= t {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    /// Evaluate the spline at t.
    pub(crate) fn eval(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        self.a[i] + s * (self.b[i] + s * (self.c[i] + s * self.d[i]))
    }

    /// Clamped cubic spline: prescribed first derivatives at endpoints.
    fn fit_clamped(
        knots: &[f64],
        values: &[f64],
        start_vel: f64,
        end_vel: f64,
    ) -> Option<Self> {
        let n = knots.len();
        let m = n - 1;
        let h: Vec<f64> = (0..m).map(|i| knots[i + 1] - knots[i]).collect();

        if n == 2 {
            // One segment, fully determined by endpoint values and slopes via cubic Hermite
            let h0 = h[0];
            let v0 = values[0];
            let v1 = values[1];
            let d0 = start_vel;
            let d1 = end_vel;
            // Hermite basis: a=v0, b=d0, c=(3(v1-v0)/h - 2d0 - d1)/h, d=(2(v0-v1)/h + d0 + d1)/h^2
            let a = v0;
            let b = d0;
            let c = (3.0 * (v1 - v0) / h0 - 2.0 * d0 - d1) / h0;
            let d = (2.0 * (v0 - v1) / h0 + d0 + d1) / (h0 * h0);
            return Some(CubicSpline {
                knots: knots.to_vec(),
                a: vec![a],
                b: vec![b],
                c: vec![c],
                d: vec![d],
            });
        }

        // System size = n (including endpoints with clamped BC)
        // M[0] determined by: h[0]*M[0]/3 + h[0]*M[1]/6 = (values[1]-values[0])/h[0] - start_vel
        // M[n-1] determined by: h[n-2]*M[n-2]/6 + h[n-2]*M[n-1]/3 = end_vel - (values[n-1]-values[n-2])/h[n-2]
        // Interior equations same as natural.

        let size = n;
        let mut diag = vec![0.0_f64; size];
        let mut upper = vec![0.0_f64; size - 1];
        let mut lower = vec![0.0_f64; size - 1];
        let mut rhs = vec![0.0_f64; size];

        // First row (i=0): clamped BC
        diag[0] = h[0] / 3.0;
        upper[0] = h[0] / 6.0;
        rhs[0] = (values[1] - values[0]) / h[0] - start_vel;

        // Last row (i=n-1): clamped BC
        diag[n - 1] = h[m - 1] / 3.0;
        lower[n - 2] = h[m - 1] / 6.0;
        rhs[n - 1] = end_vel - (values[n - 1] - values[n - 2]) / h[m - 1];

        // Interior rows i=1..n-2
        for i in 1..n - 1 {
            diag[i] = 2.0 * (h[i - 1] + h[i]);
            rhs[i] = 6.0
                * ((values[i + 1] - values[i]) / h[i]
                    - (values[i] - values[i - 1]) / h[i - 1]);
            if i < n - 1 {
                upper[i] = h[i];
            }
            if i > 0 {
                lower[i - 1] = h[i - 1];
            }
        }

        let m_vals = solve_tridiagonal(&lower[..n - 1], &diag, &upper[..n - 1], &rhs)?;

        Self::from_second_derivatives(knots, values, &h, &m_vals)
    }

    /// Periodic cubic spline: C2 at the wrap seam.
    fn fit_periodic(knots: &[f64], values: &[f64]) -> Option<Self> {
        let n = knots.len();
        let m = n - 1;
        let h: Vec<f64> = (0..m).map(|i| knots[i + 1] - knots[i]).collect();

        // For periodic splines we require values[0] == values[n-1] (caller should
        // ensure close-loop). The system is cyclic tridiagonal for M[0]..M[n-2]
        // (n-1 unknowns, with M[n-1] = M[0]).
        //
        // For i = 0..n-2 (treating indices modulo n-1):
        //   h[(i-1) mod (n-1)] * M[(i-1) mod (n-1)]
        //   + 2*(h[(i-1) mod (n-1)] + h[i]) * M[i]
        //   + h[i] * M[(i+1) mod (n-1)]
        //   = 6*((values[i+1]-values[i])/h[i] - (values[i]-values[i-1])/h[(i-1) mod (n-1)])

        let p = n - 1; // number of unknowns M[0]..M[p-1]
        if p < 2 {
            return None;
        }

        // Build cyclic system
        let mut diag = vec![0.0_f64; p];
        let mut upper = vec![0.0_f64; p - 1]; // upper[i] = coeff of M[i+1] in row i, i=0..p-2
        let mut lower = vec![0.0_f64; p - 1]; // lower[i] = coeff of M[i] in row i+1, i=0..p-2
        let mut rhs = vec![0.0_f64; p];

        // Corner entries for the cyclic part (coupling M[0] and M[p-1])
        let mut corner_ul = 0.0_f64; // top-right: coeff of M[p-1] in row 0
        let mut corner_ll = 0.0_f64; // bottom-left: coeff of M[0] in row p-1

        for i in 0..p {
            let im1 = if i == 0 { p - 1 } else { i - 1 };
            let ip1 = if i == p - 1 { 0 } else { i + 1 };
            // value[i+1] for the segment starting at i (wrap: i+1 mod n, but since
            // values[n-1]=values[0] for periodic we use values[(i+1) mod (n-1) + ???])
            // Actually we use the original values array:
            //   segment i: knots[i]..knots[i+1] with values[i] and values[i+1]
            //   for i < p=n-1 this is fine since the arrays have length n.
            let v_next = if i + 1 < n { values[i + 1] } else { values[0] };
            let v_curr = values[i];
            let v_prev = if i > 0 { values[i - 1] } else { values[n - 2] };
            let h_prev = h[im1];
            let h_curr = h[i];

            diag[i] = 2.0 * (h_prev + h_curr);
            rhs[i] = 6.0 * ((v_next - v_curr) / h_curr - (v_curr - v_prev) / h_prev);

            if i + 1 < p {
                upper[i] = h_curr;
                lower[i] = h_curr;
            } else {
                // row p-1: M[ip1=0] entry → corner
                corner_ll = h_curr;
            }
            if i == 0 {
                // row 0: M[im1=p-1] entry → corner
                corner_ul = h_prev;
            }
            let _ = ip1; // used only for documentation
        }

        let m_inner = solve_cyclic_tridiagonal(&lower, &diag, &upper, corner_ll, corner_ul, &rhs)?;

        // Full second derivative array: M[i] for i=0..p-1, M[n-1]=M[0]
        let mut m_vals = vec![0.0_f64; n];
        m_vals[..p].copy_from_slice(&m_inner[..p]);
        m_vals[n - 1] = m_inner[0];

        Self::from_second_derivatives(knots, values, &h, &m_vals)
    }

    /// Evaluate the first derivative at t.
    pub(crate) fn eval_dot(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        self.b[i] + s * (2.0 * self.c[i] + s * 3.0 * self.d[i])
    }

    /// Evaluate the second derivative at t.
    pub(crate) fn eval_ddot(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        2.0 * self.c[i] + s * 6.0 * self.d[i]
    }

    /// Return the total duration (last knot - first knot).
    pub(crate) fn duration(&self) -> f64 {
        let n = self.knots.len();
        self.knots[n - 1] - self.knots[0]
    }
}

/// A single-joint quintic Hermite spline.
///
/// Each segment is a degree-5 polynomial determined by endpoint
/// value/velocity/acceleration. Per-segment coefficients stored as:
///   p(s) = a + b*s + c*s^2 + d*s^3 + e*s^4 + f*s^5  where s = t - t[i]
#[derive(Debug, Clone)]
pub(crate) struct QuinticSpline {
    knots: Vec<f64>,
    a: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
    d: Vec<f64>,
    e: Vec<f64>,
    f: Vec<f64>,
}

/// Per-knot data for quintic Hermite fit.
#[derive(Debug, Clone)]
pub(crate) struct KnotData {
    pub t: f64,
    pub value: f64,
    pub vel: f64,
    pub accel: f64,
}

impl QuinticSpline {
    /// Fit a quintic Hermite spline through the given knot data.
    ///
    /// Returns `None` if fewer than 2 knots or not strictly increasing.
    pub(crate) fn fit(knots: &[KnotData]) -> Option<Self> {
        let n = knots.len();
        if n < 2 {
            return None;
        }
        // Reject non-finite inputs early.
        if knots.iter().any(|k| {
            !k.t.is_finite() || !k.value.is_finite() || !k.vel.is_finite() || !k.accel.is_finite()
        }) {
            return None;
        }
        for i in 0..n - 1 {
            if knots[i + 1].t <= knots[i].t {
                return None;
            }
        }

        let segs = n - 1;
        let mut times = Vec::with_capacity(n);
        let mut a = Vec::with_capacity(segs);
        let mut b = Vec::with_capacity(segs);
        let mut c = Vec::with_capacity(segs);
        let mut d = Vec::with_capacity(segs);
        let mut e = Vec::with_capacity(segs);
        let mut f = Vec::with_capacity(segs);

        for k in knots {
            times.push(k.t);
        }

        for i in 0..segs {
            let h = knots[i + 1].t - knots[i].t;
            // Boundary values
            let p0 = knots[i].value;
            let v0 = knots[i].vel;
            let a0 = knots[i].accel;
            let p1 = knots[i + 1].value;
            let v1 = knots[i + 1].vel;
            let a1 = knots[i + 1].accel;

            // Hermite coefficients for degree-5 polynomial on [0, h]
            // p(0)=p0, p'(0)=v0, p''(0)=a0, p(h)=p1, p'(h)=v1, p''(h)=a1
            // a = p0
            // b = v0
            // c = a0/2
            // Then solve for d, e, f from the 3 endpoint conditions at h:
            //   p(h)  = p0 + v0*h + a0/2*h^2 + d*h^3 + e*h^4 + f*h^5 = p1
            //   p'(h) = v0 + a0*h + 3d*h^2 + 4e*h^3 + 5f*h^4 = v1
            //   p''(h)= a0 + 6d*h + 12e*h^2 + 20f*h^3 = a1
            //
            // Let:
            //   dp = p1 - p0 - v0*h - a0/2*h^2
            //   dv = v1 - v0 - a0*h
            //   da = a1 - a0
            //
            // System:
            //   h^3    h^4    h^5   | dp
            //   3h^2   4h^3   5h^4  | dv
            //   6h     12h^2  20h^3 | da

            let dp = p1 - p0 - v0 * h - 0.5 * a0 * h * h;
            let dv = v1 - v0 - a0 * h;
            let da = a1 - a0;

            let h2 = h * h;
            let h3 = h2 * h;
            let h4 = h3 * h;
            let h5 = h4 * h;

            // Solve 3x3 system via direct elimination
            // Row 1: h3*d + h4*e + h5*f = dp
            // Row 2: 3h2*d + 4h3*e + 5h4*f = dv
            // Row 3: 6h*d + 12h2*e + 20h3*f = da

            // Eliminate d from rows 2 and 3:
            // Row2' = Row2 - 3/h * Row1: (4h3 - 3h3)*e + (5h4 - 3h4)*f = dv - 3*dp/h
            //   => h3*e + 2h4*f = dv - 3*dp/h
            // Row3' = Row3 - 6/h2 * Row1: (12h2 - 6h2)*e + (20h3 - 6h3)*f = da - 6*dp/h2
            //   => 6h2*e + 14h3*f = da - 6*dp/h2

            let r1 = dv - 3.0 * dp / h;
            let r2 = da - 6.0 * dp / h2;
            // Row1': h3*e + 2h4*f = r1
            // Row2': 6h2*e + 14h3*f = r2
            // Eliminate e from Row2': 6h2*(h3*e + 2h4*f)/h3 = 6*(r1)/h
            // Row2'' = Row2' - 6/h * Row1':
            //   (14h3 - 12h3)*f = r2 - 6*r1/h
            //   => 2h3*f = r2 - 6*r1/h

            let fi = (r2 - 6.0 * r1 / h) / (2.0 * h3);
            let ei = (r1 - 2.0 * h4 * fi) / h3;
            let di = (dp - h4 * ei - h5 * fi) / h3;

            a.push(p0);
            b.push(v0);
            c.push(0.5 * a0);
            d.push(di);
            e.push(ei);
            f.push(fi);
        }

        Some(QuinticSpline {
            knots: times,
            a,
            b,
            c,
            d,
            e,
            f,
        })
    }

    /// Find the segment index for a given t (clamped to valid range).
    fn segment(&self, t: f64) -> usize {
        let n = self.knots.len();
        let segs = n - 1;
        if t <= self.knots[0] {
            return 0;
        }
        if t >= self.knots[n - 1] {
            return segs - 1;
        }
        let mut lo = 0usize;
        let mut hi = segs - 1;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.knots[mid] <= t {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    /// Evaluate the spline at t.
    pub(crate) fn eval(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        self.a[i]
            + s * (self.b[i]
                + s * (self.c[i] + s * (self.d[i] + s * (self.e[i] + s * self.f[i]))))
    }

    /// Evaluate the first derivative at t.
    pub(crate) fn eval_dot(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        self.b[i]
            + s * (2.0 * self.c[i]
                + s * (3.0 * self.d[i] + s * (4.0 * self.e[i] + s * 5.0 * self.f[i])))
    }

    /// Evaluate the second derivative at t.
    pub(crate) fn eval_ddot(&self, t: f64) -> f64 {
        let i = self.segment(t);
        let s = t - self.knots[i];
        2.0 * self.c[i]
            + s * (6.0 * self.d[i] + s * (12.0 * self.e[i] + s * 20.0 * self.f[i]))
    }

    /// Return the total duration.
    pub(crate) fn duration(&self) -> f64 {
        let n = self.knots.len();
        self.knots[n - 1] - self.knots[0]
    }
}

/// A multi-joint spline wrapping one scalar spline per joint.
pub(crate) enum MultiJointSpline {
    Cubic(Vec<CubicSpline>),
    Quintic(Vec<QuinticSpline>),
}

impl MultiJointSpline {
    /// Construct a multi-joint cubic spline, validating that all joints share
    /// the same first and last knot time.
    ///
    /// Returns `None` if `joints` is empty or if any joint's knot range differs
    /// from the first joint's.  Mismatched ranges cause `duration()` to return
    /// only the first joint's span and per-joint eval to clamp independently —
    /// silently masking the inconsistency — so we catch it here instead.
    pub(crate) fn new_cubic(joints: Vec<CubicSpline>) -> Option<Self> {
        if joints.is_empty() {
            return None;
        }
        let t_start = joints[0].knots[0];
        let t_end = *joints[0].knots.last().unwrap();
        for j in &joints[1..] {
            if j.knots[0] != t_start || *j.knots.last().unwrap() != t_end {
                return None;
            }
        }
        Some(MultiJointSpline::Cubic(joints))
    }

    /// Construct a multi-joint quintic spline, validating that all joints share
    /// the same first and last knot time.
    ///
    /// Returns `None` if `joints` is empty or if any joint's knot range differs
    /// from the first joint's.
    pub(crate) fn new_quintic(joints: Vec<QuinticSpline>) -> Option<Self> {
        if joints.is_empty() {
            return None;
        }
        let t_start = joints[0].knots[0];
        let t_end = *joints[0].knots.last().unwrap();
        for j in &joints[1..] {
            if j.knots[0] != t_start || *j.knots.last().unwrap() != t_end {
                return None;
            }
        }
        Some(MultiJointSpline::Quintic(joints))
    }

    /// Evaluate position at t, returning one value per joint.
    pub(crate) fn eval(&self, t: f64) -> Vec<f64> {
        match self {
            MultiJointSpline::Cubic(splines) => splines.iter().map(|s| s.eval(t)).collect(),
            MultiJointSpline::Quintic(splines) => splines.iter().map(|s| s.eval(t)).collect(),
        }
    }

    /// Evaluate first derivative at t.
    pub(crate) fn eval_dot(&self, t: f64) -> Vec<f64> {
        match self {
            MultiJointSpline::Cubic(splines) => splines.iter().map(|s| s.eval_dot(t)).collect(),
            MultiJointSpline::Quintic(splines) => {
                splines.iter().map(|s| s.eval_dot(t)).collect()
            }
        }
    }

    /// Evaluate second derivative at t.
    pub(crate) fn eval_ddot(&self, t: f64) -> Vec<f64> {
        match self {
            MultiJointSpline::Cubic(splines) => splines.iter().map(|s| s.eval_ddot(t)).collect(),
            MultiJointSpline::Quintic(splines) => {
                splines.iter().map(|s| s.eval_ddot(t)).collect()
            }
        }
    }

    /// Return total duration (same for all joints).
    pub(crate) fn duration(&self) -> f64 {
        match self {
            MultiJointSpline::Cubic(splines) => {
                splines.first().map(|s| s.duration()).unwrap_or(0.0)
            }
            MultiJointSpline::Quintic(splines) => {
                splines.first().map(|s| s.duration()).unwrap_or(0.0)
            }
        }
    }
}

// ── Linear algebra helpers ────────────────────────────────────────────────────

/// Absolute singularity threshold for the Thomas algorithm pivot test.
///
/// Spline second-derivative systems are strictly diagonally dominant for any
/// set of distinct knots (all diagonal entries ≥ 2·min(h) > 0; off-diagonal
/// entries ≤ max(h)).  The pivot can only approach zero if knot spacing is
/// sub-`f64::EPSILON` — i.e., if the caller violated the strict-monotonicity
/// precondition that `CubicSpline::fit` already enforces.  An absolute
/// threshold of `EPSILON * 1e6` (~2.2e-10) is therefore conservative for any
/// physically meaningful knot spacing (sub-millisecond times would require
/// h < 2.2e-10, which is below f64's representable spacing at t > 1 s).
const SINGULAR_PIVOT: f64 = f64::EPSILON * 1_000_000.0;

/// Solve a tridiagonal system Ax = rhs using the Thomas algorithm.
///
/// - `lower`: sub-diagonal (length n-1), lower[i] is the coefficient in row i+1 col i
/// - `diag`: main diagonal (length n)
/// - `upper`: super-diagonal (length n-1), upper[i] is the coefficient in row i col i+1
///
/// Returns `None` if any pivot is zero (singular system).
fn solve_tridiagonal(lower: &[f64], diag: &[f64], upper: &[f64], rhs: &[f64]) -> Option<Vec<f64>> {
    let n = diag.len();
    assert_eq!(lower.len(), n - 1);
    assert_eq!(upper.len(), n - 1);
    assert_eq!(rhs.len(), n);

    let mut c_prime = vec![0.0_f64; n - 1];
    let mut d_prime = vec![0.0_f64; n];

    // Forward sweep
    let pivot = diag[0];
    if pivot.abs() < SINGULAR_PIVOT {
        return None;
    }
    c_prime[0] = upper[0] / pivot;
    d_prime[0] = rhs[0] / pivot;

    for i in 1..n {
        let m = lower[i - 1] * c_prime[i - 1];
        let denom = diag[i] - m;
        if denom.abs() < SINGULAR_PIVOT {
            return None;
        }
        d_prime[i] = (rhs[i] - lower[i - 1] * d_prime[i - 1]) / denom;
        if i < n - 1 {
            c_prime[i] = upper[i] / denom;
        }
    }

    // Back substitution
    let mut x = vec![0.0_f64; n];
    x[n - 1] = d_prime[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = d_prime[i] - c_prime[i] * x[i + 1];
    }

    Some(x)
}

/// Solve a cyclic tridiagonal system (Sherman-Morrison approach).
///
/// The system has the standard tridiagonal entries plus corner entries:
/// A[0, p-1] = corner_ul  (top-right)
/// A[p-1, 0] = corner_ll  (bottom-left)
///
/// Uses Sherman-Morrison: solve two tridiagonal systems, combine.
fn solve_cyclic_tridiagonal(
    lower: &[f64],  // length p-1
    diag: &[f64],   // length p
    upper: &[f64],  // length p-1
    corner_ll: f64, // A[p-1, 0]
    corner_ul: f64, // A[0, p-1]
    rhs: &[f64],    // length p
) -> Option<Vec<f64>> {
    let p = diag.len();
    if p < 3 {
        return None;
    }

    // gamma chosen to avoid amplifying diag[0]
    let gamma = -diag[0];

    // Modified diagonal for the two sub-problems
    let mut diag_mod = diag.to_vec();
    diag_mod[0] -= gamma;
    diag_mod[p - 1] -= corner_ll * corner_ul / gamma;

    // Solve A' * u = rhs
    let u = solve_tridiagonal(lower, &diag_mod, upper, rhs)?;

    // Build vector v (perturbation)
    let mut v_vec = vec![0.0_f64; p];
    v_vec[0] = 1.0;
    v_vec[p - 1] = corner_ll / gamma;

    // Solve A' * z = v
    let z = solve_tridiagonal(lower, &diag_mod, upper, &v_vec)?;

    // Sherman-Morrison correction
    // x = u - (u·v / (1 + z·v)) * z
    // where v = (gamma, 0, ..., 0, corner_ul)
    let uv = gamma * u[0] + corner_ul * u[p - 1];
    let zv = gamma * z[0] + corner_ul * z[p - 1];
    let factor = uv / (1.0 + zv);

    let x: Vec<f64> = u.iter().zip(z.iter()).map(|(ui, zi)| ui - factor * zi).collect();
    Some(x)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn cubic_p(t: f64) -> f64 {
        1.0 + 2.0 * t - 0.5 * t * t + 0.3 * t * t * t
    }
    fn cubic_dp(t: f64) -> f64 {
        2.0 - t + 0.9 * t * t
    }
    #[allow(dead_code)]
    fn cubic_ddp(t: f64) -> f64 {
        -1.0 + 1.8 * t
    }

    // ── Step-1: natural cubic — corrected contract ────────────────────────────

    /// Natural cubic spline satisfies:
    /// (i)  at-knot interpolation within TOL (by construction), and
    /// (ii) endpoint second-derivative == 0 within TOL (the defining BC invariant).
    ///
    /// Off-knot exact reproduction is NOT asserted — it is mathematically
    /// impossible for Natural BC when the source data has non-zero endpoint
    /// curvature (see plan analysis / design_decisions).
    #[test]
    fn cubic_natural_spline_interpolates_at_knots_and_satisfies_natural_bc() {
        let ts = [0.0, 1.0, 2.5, 4.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let spline = CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural)
            .expect("fit should succeed");

        // (i) at-knot interpolation
        for &t in &ts {
            let got = spline.eval(t);
            assert!(
                (got - cubic_p(t)).abs() < TOL,
                "eval at knot t={t}: got {got}, want {}, diff {}",
                cubic_p(t),
                (got - cubic_p(t)).abs()
            );
        }

        // (ii) natural BC invariant: M[0] = M[N] = 0
        let ddot_start = spline.eval_ddot(ts[0]);
        assert!(
            ddot_start.abs() < TOL,
            "natural BC: eval_ddot(t_0)={ddot_start}, want 0"
        );
        let ddot_end = spline.eval_ddot(ts[3]);
        assert!(
            ddot_end.abs() < TOL,
            "natural BC: eval_ddot(t_N)={ddot_end}, want 0"
        );
    }

    #[test]
    fn cubic_spline_duration_equals_last_minus_first_knot() {
        let ts = [0.5, 1.0, 2.5, 4.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let spline = CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural)
            .expect("fit should succeed");
        assert!(
            (spline.duration() - 3.5).abs() < TOL,
            "duration: got {}, want 3.5",
            spline.duration()
        );
    }

    #[test]
    fn cubic_fit_returns_none_for_single_knot() {
        assert!(
            CubicSpline::fit(&[1.0], &[1.0], &BoundaryCondition::Natural).is_none(),
            "single knot should return None"
        );
    }

    #[test]
    fn cubic_fit_returns_none_for_non_increasing_knots() {
        assert!(
            CubicSpline::fit(&[0.0, 1.0, 0.5], &[1.0, 2.0, 3.0], &BoundaryCondition::Natural)
                .is_none(),
            "non-increasing knots should return None"
        );
    }

    const TOL_PERIODIC: f64 = 1e-10;

    // ── Step-9: multi-joint vector spline ────────────────────────────────────

    /// Corrected natural-BC multi-joint cubic test: per-joint at-knot
    /// interpolation + per-joint endpoint zero-ddot.  Off-knot exact
    /// reproduction and derivative reproduction are NOT asserted (impossible
    /// for Natural BC — same reasoning as the single-joint step-1 correction).
    #[test]
    fn multi_joint_cubic_natural_per_joint_interpolation_and_endpoint_zero_ddot() {
        fn p1(t: f64) -> f64 { 2.0 - t + t * t - 0.1 * t * t * t }
        // dp1 and ddp1 documented for future reference but not asserted here
        // fn dp1(t: f64) -> f64 { -1.0 + 2.0 * t - 0.3 * t * t }
        // fn ddp1(t: f64) -> f64 { 2.0 - 0.6 * t }

        let ts = [0.0, 1.0, 2.5, 4.0];
        let bc = BoundaryCondition::Natural;

        let vs0: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let s0 = CubicSpline::fit(&ts, &vs0, &bc).expect("joint-0 fit should succeed");

        let vs1: Vec<f64> = ts.iter().map(|&t| p1(t)).collect();
        let s1 = CubicSpline::fit(&ts, &vs1, &bc).expect("joint-1 fit should succeed");

        let mjs = MultiJointSpline::Cubic(vec![s0, s1]);

        // Per-joint at-knot interpolation (by construction)
        for &t in &ts {
            let vals = mjs.eval(t);
            assert_eq!(vals.len(), 2);
            assert!(
                (vals[0] - cubic_p(t)).abs() < TOL,
                "j0 knot t={t}: got {}, want {}",
                vals[0], cubic_p(t)
            );
            assert!(
                (vals[1] - p1(t)).abs() < TOL,
                "j1 knot t={t}: got {}, want {}",
                vals[1], p1(t)
            );
        }

        // Per-joint endpoint zero-ddot (natural BC invariant)
        for &t_end in &[ts[0], ts[3]] {
            let ddots = mjs.eval_ddot(t_end);
            assert_eq!(ddots.len(), 2);
            assert!(
                ddots[0].abs() < TOL,
                "j0 natural BC eval_ddot at t={t_end}: {}",
                ddots[0]
            );
            assert!(
                ddots[1].abs() < TOL,
                "j1 natural BC eval_ddot at t={t_end}: {}",
                ddots[1]
            );
        }
    }

    /// Quintic Hermite multi-joint: exact per-joint reproduction is valid.
    #[test]
    fn multi_joint_quintic_per_joint_exact_reproduction() {
        fn q1(t: f64) -> f64 {
            3.0 + t - t * t + 0.5 * t * t * t + 0.2 * t * t * t * t - 0.05 * t * t * t * t * t
        }
        fn dq1(t: f64) -> f64 {
            1.0 - 2.0 * t + 1.5 * t * t + 0.8 * t * t * t - 0.25 * t * t * t * t
        }
        fn ddq1(t: f64) -> f64 {
            -2.0 + 3.0 * t + 2.4 * t * t - 1.0 * t * t * t
        }

        let ts = [0.0, 1.0, 2.5];
        let knots0: Vec<KnotData> = ts
            .iter()
            .map(|&t| KnotData { t, value: quintic_q(t), vel: quintic_dq(t), accel: quintic_ddq(t) })
            .collect();
        let knots1: Vec<KnotData> = ts
            .iter()
            .map(|&t| KnotData { t, value: q1(t), vel: dq1(t), accel: ddq1(t) })
            .collect();

        let s0 = QuinticSpline::fit(&knots0).expect("quintic fit j0");
        let s1 = QuinticSpline::fit(&knots1).expect("quintic fit j1");
        let mjs = MultiJointSpline::Quintic(vec![s0, s1]);

        for &t in &[0.0, 0.3, 1.0, 1.5, 2.0, 2.5] {
            let vals = mjs.eval(t);
            assert!(
                (vals[0] - quintic_q(t)).abs() < TOL,
                "quintic j0 val at t={t}: got {}, want {}",
                vals[0], quintic_q(t)
            );
            assert!(
                (vals[1] - q1(t)).abs() < TOL,
                "quintic j1 val at t={t}: got {}, want {}",
                vals[1], q1(t)
            );

            let dots = mjs.eval_dot(t);
            assert!(
                (dots[0] - quintic_dq(t)).abs() < TOL,
                "quintic j0 dot at t={t}: got {}, want {}",
                dots[0], quintic_dq(t)
            );
            assert!(
                (dots[1] - dq1(t)).abs() < TOL,
                "quintic j1 dot at t={t}: got {}, want {}",
                dots[1], dq1(t)
            );

            let ddots = mjs.eval_ddot(t);
            assert!(
                (ddots[0] - quintic_ddq(t)).abs() < TOL,
                "quintic j0 ddot at t={t}: got {}, want {}",
                ddots[0], quintic_ddq(t)
            );
            assert!(
                (ddots[1] - ddq1(t)).abs() < TOL,
                "quintic j1 ddot at t={t}: got {}, want {}",
                ddots[1], ddq1(t)
            );
        }
    }

    // ── Step-7: quintic Hermite — exact reproduction of general quintic ───────

    fn quintic_q(t: f64) -> f64 {
        1.0 + t + t * t + t * t * t - 0.5 * t * t * t * t + 0.1 * t * t * t * t * t
    }
    fn quintic_dq(t: f64) -> f64 {
        1.0 + 2.0 * t + 3.0 * t * t - 2.0 * t * t * t + 0.5 * t * t * t * t
    }
    fn quintic_ddq(t: f64) -> f64 {
        2.0 + 6.0 * t - 6.0 * t * t + 2.0 * t * t * t
    }

    /// Quintic Hermite uniquely determines a degree-5 poly per segment from
    /// endpoint value/vel/accel; exact reproduction IS valid here.
    #[test]
    fn quintic_spline_reproduces_general_quintic_exactly() {
        let ts = [0.0, 1.0, 2.5];
        let knots: Vec<KnotData> = ts
            .iter()
            .map(|&t| KnotData {
                t,
                value: quintic_q(t),
                vel: quintic_dq(t),
                accel: quintic_ddq(t),
            })
            .collect();
        let spline = QuinticSpline::fit(&knots).expect("quintic fit should succeed");

        for &t in &[0.0, 0.3, 1.0, 1.2, 2.0, 2.5] {
            let got = spline.eval(t);
            let want = quintic_q(t);
            assert!(
                (got - want).abs() < TOL,
                "quintic eval at t={t}: got {got}, want {want}, diff {}",
                (got - want).abs()
            );
            let got_dot = spline.eval_dot(t);
            let want_dot = quintic_dq(t);
            assert!(
                (got_dot - want_dot).abs() < TOL,
                "quintic eval_dot at t={t}: got {got_dot}, want {want_dot}, diff {}",
                (got_dot - want_dot).abs()
            );
            let got_ddot = spline.eval_ddot(t);
            let want_ddot = quintic_ddq(t);
            assert!(
                (got_ddot - want_ddot).abs() < TOL,
                "quintic eval_ddot at t={t}: got {got_ddot}, want {want_ddot}, diff {}",
                (got_ddot - want_ddot).abs()
            );
        }
    }

    #[test]
    fn quintic_fit_returns_none_for_single_knot() {
        let k = vec![KnotData { t: 0.0, value: 0.0, vel: 0.0, accel: 0.0 }];
        assert!(QuinticSpline::fit(&k).is_none(), "single knot should return None");
    }

    // ── Step-5: periodic cubic — C1 continuity at wrap seam ──────────────────

    #[test]
    fn cubic_periodic_spline_first_derivative_continuous_at_seam() {
        let period = 4.0_f64;
        let ts = [0.0, 1.0, 2.0, 3.0, 4.0];
        let vs: Vec<f64> = ts
            .iter()
            .map(|&t| (2.0 * std::f64::consts::PI * t / period).sin())
            .collect();
        let n = ts.len();

        // Sanity: sin over full period has equal endpoints
        assert!(
            (vs[0] - vs[n - 1]).abs() < 1e-15,
            "closure precondition failed: vs[0]={} vs[n-1]={}",
            vs[0],
            vs[n - 1]
        );

        let spline = CubicSpline::fit(&ts, &vs, &BoundaryCondition::Periodic)
            .expect("periodic fit should succeed");

        // C1 continuity at seam: eval_dot(period - eps) ≈ eval_dot(0 + eps)
        let eps = 1e-8;
        let dot_end = spline.eval_dot(period - eps);
        let dot_start = spline.eval_dot(eps);
        assert!(
            (dot_end - dot_start).abs() < TOL_PERIODIC,
            "periodic C1 at seam: dot_end={dot_end}, dot_start={dot_start}, diff={}",
            (dot_end - dot_start).abs()
        );
    }

    // ── Robustness: NaN/Inf rejection ────────────────────────────────────────

    #[test]
    fn cubic_fit_returns_none_for_nan_value() {
        let ts = [0.0, 1.0, 2.0];
        let vs = [1.0, f64::NAN, 3.0];
        assert!(
            CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural).is_none(),
            "NaN in values should return None"
        );
    }

    #[test]
    fn cubic_fit_returns_none_for_inf_knot() {
        let ts = [0.0, f64::INFINITY, 2.0];
        let vs = [1.0, 2.0, 3.0];
        assert!(
            CubicSpline::fit(&ts, &vs, &BoundaryCondition::Natural).is_none(),
            "Inf in knots should return None"
        );
    }

    #[test]
    fn cubic_clamped_fit_returns_none_for_nan_velocity() {
        let ts = [0.0, 1.0, 2.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        assert!(
            CubicSpline::fit(
                &ts,
                &vs,
                &BoundaryCondition::Clamped { start_vel: f64::NAN, end_vel: 1.0 }
            )
            .is_none(),
            "NaN start_vel should return None"
        );
    }

    #[test]
    fn quintic_fit_returns_none_for_nan_accel() {
        let knots = vec![
            KnotData { t: 0.0, value: 1.0, vel: 0.0, accel: f64::NAN },
            KnotData { t: 1.0, value: 2.0, vel: 0.0, accel: 0.0 },
        ];
        assert!(
            QuinticSpline::fit(&knots).is_none(),
            "NaN accel should return None"
        );
    }

    // ── Robustness: MultiJointSpline mismatched-knots rejection ──────────────

    #[test]
    fn multi_joint_new_cubic_rejects_mismatched_end_knot() {
        let ts0 = [0.0, 1.0, 2.5, 4.0];
        let vs0: Vec<f64> = ts0.iter().map(|&t| cubic_p(t)).collect();
        let s0 = CubicSpline::fit(&ts0, &vs0, &BoundaryCondition::Natural).unwrap();

        // Different end knot
        let ts1 = [0.0, 1.0, 2.5, 5.0];
        let vs1: Vec<f64> = ts1.iter().map(|&t| cubic_p(t)).collect();
        let s1 = CubicSpline::fit(&ts1, &vs1, &BoundaryCondition::Natural).unwrap();

        assert!(
            MultiJointSpline::new_cubic(vec![s0, s1]).is_none(),
            "mismatched end knots should return None"
        );
    }

    #[test]
    fn multi_joint_new_cubic_accepts_matching_knots() {
        let ts = [0.0, 1.0, 2.5, 4.0];
        let vs0: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let vs1: Vec<f64> = ts.iter().map(|&t| cubic_p(t) + 1.0).collect();
        let s0 = CubicSpline::fit(&ts, &vs0, &BoundaryCondition::Natural).unwrap();
        let s1 = CubicSpline::fit(&ts, &vs1, &BoundaryCondition::Natural).unwrap();
        assert!(
            MultiJointSpline::new_cubic(vec![s0, s1]).is_some(),
            "matching knots should succeed"
        );
    }

    #[test]
    fn multi_joint_new_quintic_rejects_mismatched_end_knot() {
        let ts0 = [0.0, 1.0, 2.5];
        let knots0: Vec<KnotData> = ts0
            .iter()
            .map(|&t| KnotData { t, value: quintic_q(t), vel: quintic_dq(t), accel: quintic_ddq(t) })
            .collect();
        let s0 = QuinticSpline::fit(&knots0).unwrap();

        // Different end knot
        let ts1 = [0.0, 1.0, 3.0];
        let knots1: Vec<KnotData> = ts1
            .iter()
            .map(|&t| KnotData { t, value: quintic_q(t), vel: quintic_dq(t), accel: quintic_ddq(t) })
            .collect();
        let s1 = QuinticSpline::fit(&knots1).unwrap();

        assert!(
            MultiJointSpline::new_quintic(vec![s0, s1]).is_none(),
            "mismatched quintic end knots should return None"
        );
    }

    #[test]
    fn multi_joint_new_empty_returns_none() {
        assert!(
            MultiJointSpline::new_cubic(vec![]).is_none(),
            "empty cubic joints should return None"
        );
        assert!(
            MultiJointSpline::new_quintic(vec![]).is_none(),
            "empty quintic joints should return None"
        );
    }

    // ── Step-3: clamped cubic — exact reproduction of general cubic ───────────

    /// With clamped BC (endpoint slopes = exact cubic derivatives), the unique
    /// solution is the original cubic polynomial.  This assertion IS
    /// mathematically valid (4 knots × 4 interp + 4 C1/C2 + 2 clamped slopes
    /// = 12 conditions for 12 unknowns).
    #[test]
    fn cubic_clamped_spline_reproduces_general_cubic_exactly() {
        let ts = [0.0, 1.0, 2.5, 4.0];
        let vs: Vec<f64> = ts.iter().map(|&t| cubic_p(t)).collect();
        let spline = CubicSpline::fit(
            &ts,
            &vs,
            &BoundaryCondition::Clamped {
                start_vel: cubic_dp(ts[0]),
                end_vel: cubic_dp(ts[ts.len() - 1]),
            },
        )
        .expect("clamped fit should succeed");

        for &t in &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.7, 4.0] {
            let got = spline.eval(t);
            let want = cubic_p(t);
            assert!(
                (got - want).abs() < TOL,
                "clamped eval at t={t}: got {got}, want {want}, diff {}",
                (got - want).abs()
            );
            let got_dot = spline.eval_dot(t);
            let want_dot = cubic_dp(t);
            assert!(
                (got_dot - want_dot).abs() < TOL,
                "clamped eval_dot at t={t}: got {got_dot}, want {want_dot}, diff {}",
                (got_dot - want_dot).abs()
            );
            let got_ddot = spline.eval_ddot(t);
            let want_ddot = cubic_ddp(t);
            assert!(
                (got_ddot - want_ddot).abs() < TOL,
                "clamped eval_ddot at t={t}: got {got_ddot}, want {want_ddot}, diff {}",
                (got_ddot - want_ddot).abs()
            );
        }
    }
}
