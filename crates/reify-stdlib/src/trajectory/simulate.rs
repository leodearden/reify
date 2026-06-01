//! Pure-Rust forward-pass simulator — `simulate_trajectory_core`.
//!
//! Implements the pure-Rust f64 CORE of the forward-pass simulator described
//! in `docs/prds/v0_3/trajectory-input-shaping.md` §6.1, §11 Phase 3 θ.
//!
//! # Scope / pure-Rust layer
//!
//! Like the sibling submodules (`spline.rs`, `sampling.rs`, `impulse_shaper.rs`,
//! `gcode_import.rs`) and `modal/transient.rs`, this module is a **pure-Rust
//! f64 layer** — all inputs and outputs are plain Rust types with no
//! `reify_ir::Value` dependency.
//!
//! The following are **deferred** to the downstream Value-wiring task (π
//! ComputeNode trampoline / dedicated dispatch):
//! - `eval_trajectory` match-arm dispatch wiring
//! - Value marshalling (EndEffectorTrack Value construction)
//! - FK-snapshot integration (Value-level `snapshot`/`end_effector_pose`)
//! - `.ri` accessor bodies (`end_effector_track`, `deviation_from_nominal`,
//!   `peak_deviation`) — currently stub TODO(θ) bodies in trajectory.ri
//!
//! # Dead-code suppression
//!
//! The public(crate) API here is tested at the pure-function level ahead of
//! the π consumer that will wire it to the Value layer.  Suppress the lint
//! rather than adding a premature marshalling layer.
//! # Robustness contract
//!
//! `simulate_trajectory_core` handles all degenerate inputs without panicking:
//! - **Empty modal modes**: vibration_offset is all-zero; combined == nominal.
//! - **Degenerate/short trajectory** (spline duration ≤ 0, or
//!   `to_trajectory_samples` returns `None` or < 2 samples): returns
//!   well-formed empty output vectors (`t_samples`, inner `Vec`s all empty).
//! - **Consistent output shaping**: `vibration_offset`, `nominal_pose`, and
//!   `combined_pose` outer length always equals `effector_locations.len()`;
//!   inner lengths always equal `t_samples.len()`.
//!
//! # Downstream wiring (deferred to π trampoline task)
//!
//! Value marshalling, `eval_trajectory` dispatch, FK-snapshot integration,
//! `EndEffectorTrack` Value construction, and the `.ri` accessor bodies
//! (`end_effector_track`, `deviation_from_nominal`, `peak_deviation`) are owned
//! by the downstream Value-wiring task — exactly as ζ owns impulse-shaper
//! marshalling and the modal ι trampoline owns `transient_response` marshalling.
#![allow(dead_code)]

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the modal-aware integration timestep (§6.1).
///
/// dt = min(0.5 / f_max, duration / 1000)
///
/// where `f_max` is the largest finite positive frequency in `freqs_hz`.
/// If no finite positive frequency exists (empty slice, all non-positive, or all
/// non-finite) the formula falls back to `duration / 1000`.
///
/// # Degenerate inputs
///
/// * `duration ≤ 0` or non-finite → returns a safe positive floor of `1e-6` s
///   so callers always receive a finite positive dt.
/// * Non-positive or non-finite entries in `freqs_hz` are ignored.
pub(crate) fn modal_aware_dt(freqs_hz: &[f64], duration: f64) -> f64 {
    // Guard duration.
    let safe_duration = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        // Degenerate: return a safe floor.
        return 1e-6;
    };

    // Find the max finite positive frequency.
    let f_max = freqs_hz
        .iter()
        .copied()
        .filter(|&f| f.is_finite() && f > 0.0)
        .fold(f64::NEG_INFINITY, f64::max);

    let dt_duration = safe_duration / 1000.0;
    if f_max > 0.0 {
        (0.5 / f_max).min(dt_duration)
    } else {
        dt_duration
    }
}

use crate::dynamics::rnea::{inverse_dynamics_open_chain, RneaLink};
use crate::dynamics::spatial::{SpatialTransform6, SpatialVector6};
use crate::trajectory::sampling::to_trajectory_samples;
use super::spline::MultiJointSpline;
use crate::modal::transient::{reconstruct_series, solve_modal_response};

// ── Public types ─────────────────────────────────────────────────────────────

/// Per-link descriptor for the pure-Rust mechanism model.
///
/// Carries the fixed spatial transform and inertial/subspace parameters needed
/// by RNEA + the FK chain.  For the test scenarios here the transforms are
/// pre-computed at the reference pose; for general q-varying configurations the
/// downstream Value-wiring task recomputes them from snapshot coordinates.
#[derive(Clone)]
pub(crate) struct LinkDesc {
    /// Fixed parent-to-child spatial transform `X_{p→i}`.
    pub(crate) parent_to_child: SpatialTransform6,
    /// Motion-subspace columns `S_i` (one per joint DOF).
    pub(crate) subspace: Vec<SpatialVector6>,
    /// Body mass (kg).
    pub(crate) mass: f64,
    /// Center of mass in body frame (m).
    pub(crate) com: [f64; 3],
    /// Rotational inertia about COM in body axes (kg·m²).
    pub(crate) inertia_about_com: [[f64; 3]; 3],
}

/// Pure-Rust mechanism model: an ordered list of link descriptors.
#[derive(Clone)]
pub(crate) struct MechanismModel {
    /// Links in spanning-tree topological order (parent before child).
    pub(crate) links: Vec<LinkDesc>,
}

/// Per-mode descriptor for the modal model.
#[derive(Clone)]
pub(crate) struct ModeDesc {
    /// Natural frequency (Hz).
    pub(crate) freq_hz: f64,
    /// Modal damping ratio ζ (dimensionless, ≥ 0).
    pub(crate) zeta: f64,
    /// Projection of this mode onto the generalized-force DOF space: Φᵢ vector.
    ///
    /// Length should equal the total DOF count (Σ link.subspace.len()).  A
    /// shorter vector is silently zero-padded (via `forces_to_forcing_history`'s
    /// min-length rule).
    pub(crate) force_projection: Vec<f64>,
}

/// Effector location: modal participation coefficients at this location.
#[derive(Clone)]
pub(crate) struct EffectorLocation {
    /// Per-mode participation coefficient `Φᵢ[node]` at this physical location.
    /// Length == number of modes in the `ModalModel`.
    pub(crate) mode_coeffs: Vec<f64>,
}

/// Modal model: a collection of SDOF modal oscillators.
#[derive(Clone)]
pub(crate) struct ModalModel {
    /// Ordered list of modes (ascending frequency recommended).
    pub(crate) modes: Vec<ModeDesc>,
}

/// Pure-Rust output of `simulate_trajectory_core`.
///
/// All inner `Vec`s are indexed `[location_idx][time_idx]`.
#[derive(Debug, Clone)]
pub(crate) struct EndEffectorTrackData {
    /// Uniform time grid from the modal-aware sampler (seconds).
    pub(crate) t_samples: Vec<f64>,
    /// Nominal (zero-vibration) end-effector pose per location and time.
    pub(crate) nominal_pose: Vec<Vec<Pose3>>,
    /// Vibration displacement offset `[dx, dy, dz]` per location and time.
    pub(crate) vibration_offset: Vec<Vec<[f64; 3]>>,
    /// Combined pose (`nominal + vibration`) per location and time.
    pub(crate) combined_pose: Vec<Vec<Pose3>>,
}

// ── Minimal pure-Rust end-effector pose ──────────────────────────────────────

/// Minimal pure-Rust end-effector pose: position + orientation quaternion.
///
/// Orientation is stored as a `(w, x, y, z)` unit quaternion matching the
/// `SpatialTransform6`/`Frame3` convention.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pose3 {
    /// Position in world frame [x, y, z] (metres).
    pub(crate) position: [f64; 3],
    /// Orientation as `(w, x, y, z)` unit quaternion.  Represents the
    /// rotation FROM the world frame TO this body's local frame, i.e. the
    /// same direction as `E` in `SpatialTransform6`.
    pub(crate) quaternion: [f64; 4],
}

// ── Pure-Rust nominal forward-kinematics ─────────────────────────────────────

/// Compute the nominal end-effector pose (position + orientation) by composing
/// a chain of `parent_to_child` [`SpatialTransform6`] transforms root→effector.
///
/// Each `SpatialTransform6` has block form `X(r, E) = [[E, 0]; [−r̃·E, E]]`
/// (Featherstone Eq. 2.24) where `r` is the child-frame origin expressed in the
/// parent frame and `E` maps parent-frame motion vectors to child-frame.
///
/// The FK algorithm:
/// 1. Start: accumulated rotation `R = I₃` (world frame), position `p = 0`.
/// 2. For each link transform `X_i`:
///    a. Extract `E_i` from the top-left 3×3.
///    b. Recover `r_i` from `r̃_i = −BL_i · E_i^T` where `BL_i` is bottom-left.
///    c. `p += R^T · r_i`   (r_i is in parent frame; R^T converts to world).
///    d. `R = E_i · R`      (update accumulated world→child rotation).
/// 3. Return `Pose3 { position: p, quaternion: rotation_matrix_to_quat(R^T) }`.
///
/// The chain must be in topological order (parent before child) with the
/// effector link last.  An empty chain returns the identity pose.
pub(crate) fn nominal_fk_chain(link_chain: &[SpatialTransform6]) -> Pose3 {
    // R maps from world frame to current accumulated frame (same direction as E).
    let mut r_acc = [[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut p_world = [0.0_f64; 3];

    for x in link_chain {
        let m = x.as_matrix();

        // Extract E (top-left 3×3).
        let e = [
            [m[0], m[1], m[2]],
            [m[6], m[7], m[8]],
            [m[12], m[13], m[14]],
        ];

        // Extract BL (bottom-left 3×3): indices [row*6+col] for rows 3..6, cols 0..3.
        let bl = [
            [m[18], m[19], m[20]],
            [m[24], m[25], m[26]],
            [m[30], m[31], m[32]],
        ];

        // Recover r̃ = -BL · E^T, then unpack r from the skew matrix.
        // r̃[2][1] = x, r̃[0][2] = y, r̃[1][0] = z
        let et = [[e[0][0], e[1][0], e[2][0]],
                  [e[0][1], e[1][1], e[2][1]],
                  [e[0][2], e[1][2], e[2][2]]];
        let neg_bl = [[-bl[0][0], -bl[0][1], -bl[0][2]],
                      [-bl[1][0], -bl[1][1], -bl[1][2]],
                      [-bl[2][0], -bl[2][1], -bl[2][2]]];
        // r̃ = neg_bl · E^T (= -BL · E^T)
        let r_tilde = mat3x3_mul(neg_bl, et);

        // Unpack r from skew matrix: r̃ = [[0,-z,y];[z,0,-x];[-y,x,0]]
        let r_i = [r_tilde[2][1], r_tilde[0][2], r_tilde[1][0]];

        // p_world += R_acc^T · r_i  (convert r_i from parent frame to world frame)
        let r_acc_t = [[r_acc[0][0], r_acc[1][0], r_acc[2][0]],
                       [r_acc[0][1], r_acc[1][1], r_acc[2][1]],
                       [r_acc[0][2], r_acc[1][2], r_acc[2][2]]];
        p_world[0] += r_acc_t[0][0] * r_i[0] + r_acc_t[0][1] * r_i[1] + r_acc_t[0][2] * r_i[2];
        p_world[1] += r_acc_t[1][0] * r_i[0] + r_acc_t[1][1] * r_i[1] + r_acc_t[1][2] * r_i[2];
        p_world[2] += r_acc_t[2][0] * r_i[0] + r_acc_t[2][1] * r_i[1] + r_acc_t[2][2] * r_i[2];

        // R_acc = E_i · R_acc
        r_acc = mat3x3_mul(e, r_acc);
    }

    // Orientation: R_acc maps world→effector; for the pose we return the
    // rotation in the canonical "frame's orientation in world" sense.
    let q = rotation_matrix_to_quat(r_acc);
    Pose3 { position: p_world, quaternion: q }
}

/// 3×3 matrix product (nested-array row-major).
#[inline]
fn mat3x3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

/// Convert a rotation matrix `E` (maps world → local) to a `(w, x, y, z)`
/// unit quaternion (Shepperd method, numerically stable).
///
/// The returned quaternion satisfies the same convention as `Frame3::rotation`.
fn rotation_matrix_to_quat(e: [[f64; 3]; 3]) -> [f64; 4] {
    let trace = e[0][0] + e[1][1] + e[2][2];
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        let w = 0.25 / s;
        let x = (e[2][1] - e[1][2]) * s;
        let y = (e[0][2] - e[2][0]) * s;
        let z = (e[1][0] - e[0][1]) * s;
        [w, x, y, z]
    } else if e[0][0] > e[1][1] && e[0][0] > e[2][2] {
        let s = 2.0 * (1.0 + e[0][0] - e[1][1] - e[2][2]).sqrt();
        let w = (e[2][1] - e[1][2]) / s;
        let x = 0.25 * s;
        let y = (e[0][1] + e[1][0]) / s;
        let z = (e[0][2] + e[2][0]) / s;
        [w, x, y, z]
    } else if e[1][1] > e[2][2] {
        let s = 2.0 * (1.0 + e[1][1] - e[0][0] - e[2][2]).sqrt();
        let w = (e[0][2] - e[2][0]) / s;
        let x = (e[0][1] + e[1][0]) / s;
        let y = 0.25 * s;
        let z = (e[1][2] + e[2][1]) / s;
        [w, x, y, z]
    } else {
        let s = 2.0 * (1.0 + e[2][2] - e[0][0] - e[1][1]).sqrt();
        let w = (e[1][0] - e[0][1]) / s;
        let x = (e[0][2] + e[2][0]) / s;
        let y = (e[1][2] + e[2][1]) / s;
        let z = 0.25 * s;
        [w, x, y, z]
    }
}

/// Superpose modal responses to produce per-location physical vibration offsets.
///
/// For each mode `i`:
/// 1. Compute ωᵢ = 2π·freq_hz_i.
/// 2. Call `solve_modal_response(ωᵢ, ζᵢ, times, &forcing_i, 0.0, 0.0)` to get
///    the modal coordinate series ξᵢ(tⱼ).
/// 3. For each location, call `reconstruct_series(coeffs, &mode_coords)` where
///    `coeffs[i] = location_coeffs[loc][i]` to get the physical displacement.
///
/// # Arguments
/// * `times` — uniform time grid `[t₀, t₁, …]`.
/// * `modes` — per-mode `(omega_rad_s, zeta, modal_forcing_series)`.
/// * `location_coeffs` — `[n_locations][n_modes]` per-location participation
///   coefficients (Φᵢ[node] at each effector location).
///
/// # Returns
/// `[n_locations][n_times]` physical vibration displacement time series.
pub(crate) fn superpose_modes(
    times: &[f64],
    modes: &[(f64, f64, Vec<f64>)],  // (omega, zeta, forcing)
    location_coeffs: &[Vec<f64>],
) -> Vec<Vec<f64>> {
    // Solve each mode's modal coordinate series.
    let mode_coords: Vec<Vec<f64>> = modes
        .iter()
        .map(|(omega, zeta, forcing)| {
            solve_modal_response(*omega, *zeta, times, forcing, 0.0, 0.0).coords
        })
        .collect();

    // For each location, reconstruct the physical displacement by superposition.
    location_coeffs
        .iter()
        .map(|coeffs| reconstruct_series(coeffs, &mode_coords))
        .collect()
}

/// Project per-sample generalized forces onto modal shapes to produce per-mode
/// scalar modal forcing series: f_i(t_j) = Φᵢᵀ · F(t_j).
///
/// # Arguments
/// * `forces` — `[n_times][n_dofs]` — per-sample generalized force vectors.
/// * `projections` — `[n_modes][n_dofs]` — per-mode projection (modal shape)
///   coefficients.  Each entry is the projection of this mode onto the
///   generalized-force DOF space.
///
/// # Returns
/// `[n_modes][n_times]` — per-mode scalar modal forcing series.
///
/// # Graceful handling of mismatched DOF counts
/// The dot product at each time step uses the common DOF length
/// `min(forces[j].len(), projections[i].len())` — surplus entries are silently
/// ignored rather than indexing out of bounds.
pub(crate) fn forces_to_forcing_history(
    forces: &[Vec<f64>],
    projections: &[Vec<f64>],
) -> Vec<Vec<f64>> {
    let n_modes = projections.len();
    let n_times = forces.len();
    let mut result: Vec<Vec<f64>> = Vec::with_capacity(n_modes);
    for shape in projections {
        let mut series = Vec::with_capacity(n_times);
        for f_at_t in forces {
            let common_len = f_at_t.len().min(shape.len());
            let dot: f64 = f_at_t[..common_len]
                .iter()
                .zip(shape[..common_len].iter())
                .map(|(a, b)| a * b)
                .sum();
            series.push(dot);
        }
        result.push(series);
    }
    result
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Pure-Rust forward-pass simulator core (PRD §6.1, Phase 3 θ).
///
/// Pipeline:
/// 1. `dt = modal_aware_dt(modal freqs, spline.duration())`
/// 2. `traj = to_trajectory_samples(spline, dt)` (or empty if spline is degenerate)
/// 3. Per sample: build `RneaLink` chain from `(q, q̇, q̈)`, call
///    `inverse_dynamics_open_chain(links, [0,0,0])` → `τ(t_j)`.
///    (gravity = 0 so the static-pose mandate holds by construction.)
/// 4. `forcing = forces_to_forcing_history(τ-history, mode projections)`
/// 5. `vibration = superpose_modes(times, modes, location_coeffs)`
///    — maps each location's scalar superposition to `[dx, dy, dz]` by treating
///    the scalar as a displacement along the first effector axis (Z by default).
///    The downstream Value-wiring task applies proper physical-space mapping.
/// 6. `nominal = nominal_fk_chain(link_chain)` per sample per location
/// 7. `combined = nominal + vibration`
///
/// Returns `EndEffectorTrackData` with consistent `[location][time]` indexing.
///
/// # Degenerate inputs
/// - Empty/degenerate spline (duration ≤ 0 or < 2 valid samples): returns
///   well-formed empty output vectors — no panic, no index-out-of-bounds.
/// - Empty modal modes: vibration_offset is all-zero, combined == nominal.
/// - Location count is respected in all output inner-vector lengths.
///
/// Return an empty [`EndEffectorTrackData`] shaped for `n_loc` effector locations.
///
/// All inner `Vec`s are empty and `t_samples` is empty.  This is the canonical
/// degenerate-input output (duration ≤ 0, or `to_trajectory_samples` returning
/// `None` / < 2 samples): callers can safely index `[loc]` without panicking,
/// and `t_samples.is_empty()` signals "no data available".
fn empty_track_data(n_loc: usize) -> EndEffectorTrackData {
    EndEffectorTrackData {
        t_samples: Vec::new(),
        nominal_pose: vec![Vec::new(); n_loc],
        vibration_offset: vec![Vec::new(); n_loc],
        combined_pose: vec![Vec::new(); n_loc],
    }
}

pub(crate) fn simulate_trajectory_core(
    spline: &MultiJointSpline,
    mechanism: &MechanismModel,
    modal: &ModalModel,
    effector_locations: &[EffectorLocation],
) -> EndEffectorTrackData {
    let n_loc = effector_locations.len();
    let freqs_hz: Vec<f64> = modal.modes.iter().map(|m| m.freq_hz).collect();

    // ── (1) Modal-aware timestep ────────────────────────────────────────────
    let dt = modal_aware_dt(&freqs_hz, spline.duration());

    // ── (2) Sample the profile ──────────────────────────────────────────────
    let traj_opt = to_trajectory_samples(spline, dt);
    let traj = match traj_opt {
        Some(t) if t.samples.len() >= 2 => t,
        _ => return empty_track_data(n_loc),
    };
    let times = traj.times();
    let n_times = times.len();

    // ── (3) Per-sample generalized forces (RNEA, gravity = 0) ──────────────
    // Build the fixed FK link chain once (same for all samples — fixed transforms).
    let fk_chain: Vec<SpatialTransform6> = mechanism
        .links
        .iter()
        .map(|l| l.parent_to_child)
        .collect();

    // Precompute per-link DOF-start offsets once (O(n_links)) rather than
    // re-summing a prefix on every link on every sample (O(n_links²·n_samples)).
    let dof_starts: Vec<usize> = {
        let mut starts = Vec::with_capacity(mechanism.links.len());
        let mut offset = 0usize;
        for l in &mechanism.links {
            starts.push(offset);
            offset += l.subspace.len();
        }
        starts
    };

    let mut tau_history: Vec<Vec<f64>> = Vec::with_capacity(n_times);
    for sample in &traj.samples {
        let rnea_links: Vec<RneaLink> = mechanism
            .links
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let n_dof = l.subspace.len();
                // For fixed transforms the parent_to_child doesn't vary with q.
                // q̇ and q̈ come from the trajectory sample (sliced per-link DOF).
                let dof_start = dof_starts[i];
                let q_dot = sample.vels.get(dof_start..dof_start + n_dof)
                    .map(|s| s.to_vec())
                    .unwrap_or_else(|| vec![0.0; n_dof]);
                let q_ddot = sample.accels.get(dof_start..dof_start + n_dof)
                    .map(|s| s.to_vec())
                    .unwrap_or_else(|| vec![0.0; n_dof]);
                RneaLink {
                    parent: if i == 0 { None } else { Some(i - 1) },
                    parent_to_child: l.parent_to_child,
                    subspace: l.subspace.clone(),
                    mass: l.mass,
                    com: l.com,
                    inertia_about_com: l.inertia_about_com,
                    q_dot,
                    q_ddot,
                }
            })
            .collect();

        let tau = inverse_dynamics_open_chain(&rnea_links, [0.0, 0.0, 0.0]);
        // Flatten per-link tau into a single per-DOF vector.
        let flat_tau: Vec<f64> = tau.into_iter().flatten().collect();
        tau_history.push(flat_tau);
    }

    // ── (4) Modal forcing history ────────────────────────────────────────────
    let projections: Vec<Vec<f64>> = modal.modes
        .iter()
        .map(|m| m.force_projection.clone())
        .collect();
    let modal_forcing: Vec<Vec<f64>> = forces_to_forcing_history(&tau_history, &projections);

    // ── (5) Modal superposition → vibration offsets ──────────────────────────
    // Prepare per-mode (omega, zeta, forcing) tuples.
    let mode_tuples: Vec<(f64, f64, Vec<f64>)> = modal.modes
        .iter()
        .zip(modal_forcing.iter())
        .map(|(m, forcing)| {
            let omega = 2.0 * std::f64::consts::PI * m.freq_hz;
            (omega, m.zeta, forcing.clone())
        })
        .collect();

    let location_coeffs: Vec<Vec<f64>> = effector_locations
        .iter()
        .map(|loc| loc.mode_coeffs.clone())
        .collect();

    // scalar_vib[loc][t]: scalar vibration displacement per location per time.
    let scalar_vib = if mode_tuples.is_empty() {
        vec![vec![0.0_f64; n_times]; n_loc]
    } else {
        superpose_modes(&times, &mode_tuples, &location_coeffs)
    };

    // Convert scalar → [dx, dy, dz] treating scalar as Z-axis displacement.
    // (Physical-space mapping is finalised in the downstream Value-wiring task.)
    let vibration_offset: Vec<Vec<[f64; 3]>> = scalar_vib
        .iter()
        .map(|series| series.iter().map(|&s| [0.0, 0.0, s]).collect())
        .collect();

    // ── (6) Nominal FK per location (same chain for all samples) ────────────
    // For fixed transforms the nominal pose is time-invariant; we compute it
    // once and replicate — keeping the per-sample indexing for generality.
    let nominal_pose_single = nominal_fk_chain(&fk_chain);
    let nominal_pose: Vec<Vec<Pose3>> = (0..n_loc)
        .map(|_| vec![nominal_pose_single.clone(); n_times])
        .collect();

    // ── (7) Combined = nominal + vibration ───────────────────────────────────
    let combined_pose: Vec<Vec<Pose3>> = (0..n_loc)
        .map(|loc| {
            (0..n_times)
                .map(|t| {
                    let nom = &nominal_pose[loc][t];
                    let [dx, dy, dz] = vibration_offset[loc][t];
                    Pose3 {
                        position: [
                            nom.position[0] + dx,
                            nom.position[1] + dy,
                            nom.position[2] + dz,
                        ],
                        quaternion: nom.quaternion,
                    }
                })
                .collect()
        })
        .collect();

    EndEffectorTrackData {
        t_samples: times,
        nominal_pose,
        vibration_offset,
        combined_pose,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── step-9: RED (mandated signal #1) — static → zero vibration ──────────

    use super::super::spline::{BoundaryCondition, CubicSpline, MultiJointSpline};

    /// Build a constant-pose (all-equal-waypoints) 1-joint clamped cubic spline on [0, T].
    fn constant_pose_spline(duration: f64, q_val: f64) -> MultiJointSpline {
        let knots = vec![0.0, duration / 2.0, duration];
        let vals = vec![q_val, q_val, q_val];
        let bc = BoundaryCondition::Clamped { start_vel: 0.0, end_vel: 0.0 };
        let s = CubicSpline::fit(&knots, &vals, &bc).expect("cubic fit");
        MultiJointSpline::new_cubic(vec![s]).expect("multi-joint")
    }

    /// (a) Static profile → vibration_offset all ≤1e-12, combined==nominal.
    /// (b) t_samples: first=0, last=duration.
    /// (c) nominal_pose constant across time.
    #[test]
    fn static_profile_zero_vibration() {
        let duration = 0.5_f64;
        let spline = constant_pose_spline(duration, 0.0);

        // 1-link mechanism: identity transform, 1 kg point mass, 1 DOF (prismatic X).
        let link = LinkDesc {
            parent_to_child: SpatialTransform6::from_frame3(
                &crate::dynamics::spatial::Frame3::identity()
            ),
            subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0; 3],
            inertia_about_com: [[0.0; 3]; 3],
        };
        let mech = MechanismModel { links: vec![link] };

        // 1 mode (5 Hz, ζ=0.05), unit force projection, unit mode coefficient at location.
        let modal = ModalModel {
            modes: vec![ModeDesc {
                freq_hz: 5.0,
                zeta: 0.05,
                force_projection: vec![1.0],
            }],
        };
        let effector_locs = vec![EffectorLocation { mode_coeffs: vec![1.0] }];

        let result = simulate_trajectory_core(&spline, &mech, &modal, &effector_locs);

        // t_samples: first=0, last=duration.
        assert!(!result.t_samples.is_empty(), "t_samples non-empty");
        assert!((result.t_samples[0]).abs() < 1e-12, "first sample t=0");
        assert!(
            (result.t_samples.last().unwrap() - duration).abs() < 1e-12,
            "last sample t=duration"
        );

        // Vibration all zero.
        let n_loc = result.vibration_offset.len();
        assert_eq!(n_loc, 1, "1 location");
        for (loc_idx, vib_series) in result.vibration_offset.iter().enumerate() {
            for (t_idx, &[dx, dy, dz]) in vib_series.iter().enumerate() {
                let mag = dx.abs().max(dy.abs()).max(dz.abs());
                assert!(
                    mag <= 1e-12,
                    "loc {loc_idx} t_idx {t_idx}: vibration mag {mag:.2e} > 1e-12"
                );
            }
        }

        // combined_pose == nominal_pose at every sample.
        for loc in 0..n_loc {
            let n_t = result.nominal_pose[loc].len();
            assert_eq!(result.combined_pose[loc].len(), n_t, "combined and nominal same length");
            for i in 0..n_t {
                let nom = &result.nominal_pose[loc][i];
                let comb = &result.combined_pose[loc][i];
                for k in 0..3 {
                    assert!(
                        (comb.position[k] - nom.position[k]).abs() <= 1e-12,
                        "loc {loc} t {i} pos[{k}]: combined={} nominal={}",
                        comb.position[k], nom.position[k]
                    );
                }
            }
        }

        // nominal_pose constant across time.
        if result.nominal_pose[0].len() > 1 {
            let p0 = result.nominal_pose[0][0].position;
            for (i, pose) in result.nominal_pose[0].iter().enumerate() {
                for (k, &p0_k) in p0.iter().enumerate() {
                    assert!(
                        (pose.position[k] - p0_k).abs() <= 1e-12,
                        "nominal_pose[0][{i}].position[{k}] not constant"
                    );
                }
            }
        }
    }

    // ─── step-11: RED — step on single-mode oscillator + edge cases ──────────

    /// Build a constant-acceleration 1-joint spline on [0, duration].
    /// q(t) = 0.5·a·t²  →  q̇=a·t,  q̈=a everywhere.
    fn constant_accel_spline(duration: f64, accel: f64) -> MultiJointSpline {
        // Use 3 knots so the cubic fit is over-constrained and exact for q(t)=0.5·a·t².
        let t1 = duration / 2.0;
        let t2 = duration;
        let knots = vec![0.0, t1, t2];
        let vals = vec![0.0, 0.5 * accel * t1 * t1, 0.5 * accel * t2 * t2];
        let bc = BoundaryCondition::Clamped {
            start_vel: 0.0,
            end_vel: accel * duration,
        };
        let s = CubicSpline::fit(&knots, &vals, &bc).expect("accel spline fit");
        MultiJointSpline::new_cubic(vec![s]).expect("multi-joint accel")
    }

    /// End-to-end step on single-mode oscillator.
    /// 1-DOF prismatic mechanism, mass m, constant acceleration a → τ = m·a (step).
    /// Modal forcing = τ (unit projection). Vibration (Z-axis) matches analytic step response.
    #[test]
    fn step_on_single_mode_oscillator_matches_analytic() {
        let mass = 2.0_f64;
        let accel = 3.0_f64;      // m/s²; τ = mass·accel = 6.0 N (step)
        let p0 = mass * accel;    // step modal forcing magnitude

        let freq_hz = 5.0_f64;
        let zeta = 0.05_f64;
        let omega = 2.0 * std::f64::consts::PI * freq_hz;
        let duration = 0.4_f64;

        let spline = constant_accel_spline(duration, accel);

        // 1-link mechanism: identity transform, prismatic X (subspace [0,0,0,1,0,0]).
        let link = LinkDesc {
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
            subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
            mass,
            com: [0.0; 3],
            inertia_about_com: [[0.0; 3]; 3],
        };
        let mech = MechanismModel { links: vec![link] };

        // 1 mode, unit force projection, unit location coefficient.
        let modal = ModalModel {
            modes: vec![ModeDesc {
                freq_hz,
                zeta,
                force_projection: vec![1.0],
            }],
        };
        let effector_locs = vec![EffectorLocation { mode_coeffs: vec![1.0] }];

        let result = simulate_trajectory_core(&spline, &mech, &modal, &effector_locs);

        assert!(!result.t_samples.is_empty(), "non-empty output");
        assert_eq!(result.vibration_offset.len(), 1, "1 location");

        for (t_idx, (&t, &[_dx, _dy, dz])) in result
            .t_samples
            .iter()
            .zip(result.vibration_offset[0].iter())
            .enumerate()
        {
            let want = analytic_step_response(p0, omega, zeta, t);
            let diff = (dz - want).abs();
            assert!(
                diff < 1e-9,
                "t={t:.4} step {t_idx}: vib_z={dz:.6e}, want={want:.6e}, diff={diff:.2e}"
            );
        }

        // combined = nominal + vibration.
        for (t_idx, (&[_dx, _dy, dz], comb)) in result.vibration_offset[0]
            .iter()
            .zip(result.combined_pose[0].iter())
            .enumerate()
        {
            let nom_z = result.nominal_pose[0][t_idx].position[2];
            assert!(
                (comb.position[2] - (nom_z + dz)).abs() < 1e-14,
                "t_idx {t_idx}: combined.z != nominal.z + vib.z"
            );
        }
    }

    /// Edge case (a): empty modal modes → vibration all zero, combined==nominal, no panic.
    #[test]
    fn simulate_empty_modes_zero_vibration() {
        let spline = constant_pose_spline(0.3, 0.5);
        let link = LinkDesc {
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
            subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0; 3],
            inertia_about_com: [[0.0; 3]; 3],
        };
        let mech = MechanismModel { links: vec![link] };
        let modal = ModalModel { modes: vec![] };
        let effector_locs = vec![EffectorLocation { mode_coeffs: vec![] }];

        let result = simulate_trajectory_core(&spline, &mech, &modal, &effector_locs);
        assert!(!result.t_samples.is_empty(), "should produce samples");
        for &[dx, dy, dz] in &result.vibration_offset[0] {
            assert_eq!([dx, dy, dz], [0.0, 0.0, 0.0], "vibration must be zero");
        }
        for (nom, comb) in result.nominal_pose[0].iter().zip(result.combined_pose[0].iter()) {
            assert_eq!(nom.position, comb.position, "combined must equal nominal");
        }
    }

    /// Edge case (b): output shape is consistent across all locations.
    ///
    /// Renamed from the misleading `simulate_degenerate_spline_no_panic` — this
    /// test exercises multi-location output-shape consistency on a normal spline,
    /// not the degenerate early-return path (see `simulate_degenerate_spline_no_panic`
    /// below for the dedicated degenerate-path test).
    #[test]
    fn multi_location_output_shape() {
        let spline = constant_pose_spline(0.2, 0.0);
        let link = LinkDesc {
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
            subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
            mass: 1.0,
            com: [0.0; 3],
            inertia_about_com: [[0.0; 3]; 3],
        };
        let mech = MechanismModel { links: vec![link] };
        let modal = ModalModel {
            modes: vec![ModeDesc { freq_hz: 5.0, zeta: 0.05, force_projection: vec![1.0] }],
        };
        // 3 locations — output inner-vector lengths must all equal n_times.
        let effector_locs = vec![
            EffectorLocation { mode_coeffs: vec![1.0] },
            EffectorLocation { mode_coeffs: vec![0.5] },
            EffectorLocation { mode_coeffs: vec![0.0] },
        ];
        let result = simulate_trajectory_core(&spline, &mech, &modal, &effector_locs);
        let n_t = result.t_samples.len();
        assert_eq!(result.nominal_pose.len(), 3, "3 locations in nominal_pose");
        assert_eq!(result.vibration_offset.len(), 3, "3 locations in vibration_offset");
        assert_eq!(result.combined_pose.len(), 3, "3 locations in combined_pose");
        for loc in 0..3 {
            assert_eq!(result.nominal_pose[loc].len(), n_t, "loc {loc} nominal length");
            assert_eq!(result.vibration_offset[loc].len(), n_t, "loc {loc} vib length");
            assert_eq!(result.combined_pose[loc].len(), n_t, "loc {loc} combined length");
        }
    }

    /// Degenerate path: `empty_track_data` produces well-formed empty output.
    ///
    /// `simulate_trajectory_core` calls `empty_track_data(n_loc)` when
    /// `to_trajectory_samples` returns `None` or fewer than 2 samples.
    /// The `MultiJointSpline` public API always produces `duration > 0`, so the
    /// branch cannot be exercised end-to-end without a mock; instead the
    /// `empty_track_data` helper is extracted and tested directly to pin the
    /// robustness contract:
    ///
    /// - `t_samples` is empty
    /// - outer length == `n_loc` for nominal/vibration/combined
    /// - every inner Vec is empty
    ///
    /// This test replaces the incorrectly-named predecessor that only tested
    /// multi-location output shape on a normal spline.
    #[test]
    fn simulate_degenerate_spline_no_panic() {
        for n_loc in [0usize, 1, 3] {
            let out = empty_track_data(n_loc);
            assert!(out.t_samples.is_empty(), "n_loc={n_loc}: t_samples must be empty");
            assert_eq!(
                out.nominal_pose.len(), n_loc,
                "n_loc={n_loc}: nominal_pose outer len"
            );
            assert_eq!(
                out.vibration_offset.len(), n_loc,
                "n_loc={n_loc}: vibration_offset outer len"
            );
            assert_eq!(
                out.combined_pose.len(), n_loc,
                "n_loc={n_loc}: combined_pose outer len"
            );
            for loc in 0..n_loc {
                assert!(
                    out.nominal_pose[loc].is_empty(),
                    "n_loc={n_loc} loc={loc}: nominal inner empty"
                );
                assert!(
                    out.vibration_offset[loc].is_empty(),
                    "n_loc={n_loc} loc={loc}: vib inner empty"
                );
                assert!(
                    out.combined_pose[loc].is_empty(),
                    "n_loc={n_loc} loc={loc}: combined inner empty"
                );
            }
        }
    }

    // ─── step-7: RED — nominal_fk_pose ───────────────────────────────────────

    use crate::dynamics::spatial::{Frame3, SpatialTransform6};

    /// (a) Single revolute link at angle θ with fixed link offset r.
    ///     Effector position == closed-form rotated offset.
    #[test]
    fn nominal_fk_pose_single_revolute_link() {
        // Z-axis revolute joint at angle θ = π/4.
        // Link offset: 1.0 m along world X (in parent frame), zero rotation.
        // After joint rotation R_z(θ):
        //   effector position = R_z(θ) · [1, 0, 0] = [cos θ, sin θ, 0]
        let theta = std::f64::consts::FRAC_PI_4; // 45°
        let c = theta.cos();
        let s = theta.sin();

        // parent_to_child: pure Z-rotation R_z(θ) first, then offset [1,0,0] in parent.
        // Convention from rnea.rs: rot(E) · xlt(r) = from_frame3({E_id,0}).compose(from_frame3({I,r}))
        // But for FK we want: X = rot_z(θ) · xlt([1,0,0])
        // Build it as: pure rotation (θ around Z) composed with pure translation [1,0,0]
        // (The plan says "compose the per-link SpatialTransform6 chain root→effector")
        // We need the child's origin in world frame after applying this transform.
        // Let's construct the link chain as one link: its transform maps origin to
        // the effector's world position after rotation by θ.

        // Quaternion for R_z(θ): w=cos(θ/2), z=sin(θ/2), x=y=0
        let qw = (theta / 2.0).cos();
        let qz = (theta / 2.0).sin();
        let rot_frame = Frame3::new([qw, 0.0, 0.0, qz], [0.0; 3]);
        let trans_frame = Frame3::new([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

        // X_{p→child} = rot · trans (rnea convention: rot(E)·xlt(r))
        let x_link = SpatialTransform6::from_frame3(&rot_frame)
            .compose(&SpatialTransform6::from_frame3(&trans_frame));
        let link_chain = vec![x_link];

        let pose = nominal_fk_chain(&link_chain);

        // Expected effector position: R_z(θ) · [1, 0, 0] = [c, s, 0]
        let tol = 1e-12;
        assert!((pose.position[0] - c).abs() < tol, "x: {} vs {}", pose.position[0], c);
        assert!((pose.position[1] - s).abs() < tol, "y: {} vs {}", pose.position[1], s);
        assert!((pose.position[2]).abs() < tol, "z: {}", pose.position[2]);
    }

    /// (b) Constant joint config over multiple calls → identical nominal pose.
    #[test]
    fn nominal_fk_pose_constant_config_identical() {
        let theta = 0.3_f64;
        let qw = (theta / 2.0).cos();
        let qz = (theta / 2.0).sin();
        let rot_frame = Frame3::new([qw, 0.0, 0.0, qz], [0.0; 3]);
        let trans_frame = Frame3::new([1.0, 0.0, 0.0, 0.0], [0.5, 0.0, 0.0]);
        let x_link = SpatialTransform6::from_frame3(&rot_frame)
            .compose(&SpatialTransform6::from_frame3(&trans_frame));
        let link_chain = vec![x_link];

        let pose_a = nominal_fk_chain(&link_chain);
        let pose_b = nominal_fk_chain(&link_chain);
        assert_eq!(pose_a.position, pose_b.position, "constant config → identical pose");
    }

    // ─── step-5: RED — superpose_modes ───────────────────────────────────────

    /// Local analytic SDOF step-response oracle (matches transient.rs tests).
    ///
    /// ξ(t) = (p₀/ω²)·[1 − e^{−ζωt}·(cos(ωD·t) + (ζω/ωD)·sin(ωD·t))]
    fn analytic_step_response(p0: f64, omega: f64, zeta: f64, t: f64) -> f64 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        (p0 / (omega * omega))
            * (1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin()))
    }

    /// (a) SINGLE-MODE STEP: constant modal forcing p0, unit location coefficient
    ///     → reconstructed displacement matches analytic SDOF step response within 1e-9.
    #[test]
    fn superpose_modes_single_mode_step_matches_analytic() {
        let omega = 2.0 * std::f64::consts::PI * 5.0; // 5 Hz
        let zeta = 0.05_f64;
        let p0 = 1.0_f64;
        let n = 200;
        let t_end = 0.5_f64;
        let dt = t_end / (n - 1) as f64;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
        // Constant step forcing.
        let forcing_series: Vec<f64> = vec![p0; n];

        // One mode, one location with unit coefficient.
        let modes = vec![(omega, zeta, forcing_series.clone())];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0]]; // 1 location × 1 mode

        let result = superpose_modes(&times, &modes, &location_coeffs);

        assert_eq!(result.len(), 1, "1 location");
        assert_eq!(result[0].len(), n, "n time samples");
        for (j, (&got, &t)) in result[0].iter().zip(times.iter()).enumerate() {
            let want = analytic_step_response(p0, omega, zeta, t);
            let diff = (got - want).abs();
            assert!(
                diff < 1e-9,
                "t={t:.4} step {j}: got {got:.6e}, want {want:.6e}, diff {diff:.2e}"
            );
        }
    }

    /// (b) ZERO forcing from rest → all-zero displacement.
    #[test]
    fn superpose_modes_zero_forcing_zero_displacement() {
        let omega = 2.0 * std::f64::consts::PI * 10.0;
        let zeta = 0.02_f64;
        let n = 50;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * 0.01).collect();
        let forcing_series: Vec<f64> = vec![0.0; n];
        let modes = vec![(omega, zeta, forcing_series)];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0]];

        let result = superpose_modes(&times, &modes, &location_coeffs);
        assert_eq!(result.len(), 1);
        for &v in &result[0] {
            assert!(v.abs() < 1e-15, "expected zero, got {v:.2e}");
        }
    }

    /// (c) Two modes superpose additively (sum of single-mode responses).
    #[test]
    fn superpose_modes_two_modes_additive() {
        let omega1 = 2.0 * std::f64::consts::PI * 5.0;
        let omega2 = 2.0 * std::f64::consts::PI * 15.0;
        let zeta = 0.05_f64;
        let p0 = 1.0_f64;
        let n = 100;
        let dt = 0.005_f64;
        let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
        let f1: Vec<f64> = vec![p0; n];
        let f2: Vec<f64> = vec![p0 * 2.0; n];

        // Superpose both modes at unit location coefficient.
        let modes = vec![
            (omega1, zeta, f1.clone()),
            (omega2, zeta, f2.clone()),
        ];
        let location_coeffs: Vec<Vec<f64>> = vec![vec![1.0, 1.0]]; // 1 location × 2 modes

        let result = superpose_modes(&times, &modes, &location_coeffs);

        // Compare against sum of individual single-mode results.
        let modes_a = vec![(omega1, zeta, f1.clone())];
        let lc_a: Vec<Vec<f64>> = vec![vec![1.0]];
        let r1 = superpose_modes(&times, &modes_a, &lc_a);

        let modes_b = vec![(omega2, zeta, f2.clone())];
        let lc_b: Vec<Vec<f64>> = vec![vec![1.0]];
        let r2 = superpose_modes(&times, &modes_b, &lc_b);

        for j in 0..n {
            let want = r1[0][j] + r2[0][j];
            let got = result[0][j];
            assert!(
                (got - want).abs() < 1e-13,
                "step {j}: got {got:.6e}, want {want:.6e}"
            );
        }
    }

    // ─── step-3: RED — forces_to_forcing_history ──────────────────────────────

    /// (a) All-zero force input → every modal forcing series is all-zero.
    #[test]
    fn forces_to_forcing_history_zero_input() {
        // 3 time steps, 2 DOFs, 2 modes
        let forces: Vec<Vec<f64>> = vec![
            vec![0.0, 0.0],
            vec![0.0, 0.0],
            vec![0.0, 0.0],
        ];
        let projections: Vec<Vec<f64>> = vec![
            vec![1.0, 0.5],  // mode 0 shape
            vec![0.3, 0.7],  // mode 1 shape
        ];
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 2, "n_modes");
        for (i, series) in result.iter().enumerate() {
            assert_eq!(series.len(), 3, "mode {i}: n_times");
            for &v in series {
                assert_eq!(v, 0.0, "mode {i}: expected zero for zero input");
            }
        }
    }

    /// (b) Known single-DOF force with known projection coefficient.
    #[test]
    fn forces_to_forcing_history_known_projection() {
        // 1 DOF, 2 time steps: forces = [[2.0], [4.0]]
        // 1 mode, coefficient c=3.0 → modal forcing = [6.0, 12.0]
        let forces: Vec<Vec<f64>> = vec![vec![2.0], vec![4.0]];
        let projections: Vec<Vec<f64>> = vec![vec![3.0]];
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 1, "n_modes");
        assert_eq!(result[0].len(), 2, "n_times");
        assert!((result[0][0] - 6.0).abs() < 1e-14, "t0: expected 6.0, got {}", result[0][0]);
        assert!((result[0][1] - 12.0).abs() < 1e-14, "t1: expected 12.0, got {}", result[0][1]);
    }

    /// (c) Output shape = [n_modes][n_times].
    #[test]
    fn forces_to_forcing_history_output_shape() {
        let n_times = 7;
        let n_dofs = 3;
        let n_modes = 4;
        let forces: Vec<Vec<f64>> = (0..n_times).map(|_| vec![1.0; n_dofs]).collect();
        let projections: Vec<Vec<f64>> = (0..n_modes).map(|i| vec![i as f64 * 0.1; n_dofs]).collect();
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), n_modes, "outer dim = n_modes");
        for (i, series) in result.iter().enumerate() {
            assert_eq!(series.len(), n_times, "mode {i}: inner dim = n_times");
        }
    }

    /// (d) DOF/shape length mismatch → truncate to common length, no panic.
    #[test]
    fn forces_to_forcing_history_dof_mismatch_no_panic() {
        // Force has 3 DOFs, mode shape has 2 → common = 2; result is defined, no panic.
        let forces: Vec<Vec<f64>> = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
        ];
        let projections: Vec<Vec<f64>> = vec![vec![1.0, 1.0]]; // only 2 entries
        let result = forces_to_forcing_history(&forces, &projections);
        assert_eq!(result.len(), 1, "n_modes");
        assert_eq!(result[0].len(), 2, "n_times");
        // mode 0, t=0: dot([1.0, 2.0], [1.0, 1.0]) = 3.0
        assert!((result[0][0] - 3.0).abs() < 1e-14, "t0 mismatch: {}", result[0][0]);
    }

    // ─── step-1: RED — modal_aware_dt ─────────────────────────────────────────

    /// (a) 0.5/f_max governs (high f_max, long duration).
    #[test]
    fn modal_aware_dt_governed_by_frequency() {
        // f_max=100 Hz → 0.5/100=0.005 s; duration/1000 = 10.0/1000 = 0.01 s
        // → min is 0.005
        let dt = modal_aware_dt(&[50.0, 100.0], 10.0);
        assert!((dt - 0.005).abs() < 1e-14, "expected 0.005, got {dt}");
    }

    /// (b) duration/1000 governs (low f_max, short duration).
    #[test]
    fn modal_aware_dt_governed_by_duration() {
        // f_max=1 Hz → 0.5/1=0.5 s; duration/1000 = 0.1/1000 = 0.0001 s
        // → min is 0.0001
        let dt = modal_aware_dt(&[0.5, 1.0], 0.1);
        assert!((dt - 0.0001).abs() < 1e-18, "expected 0.0001, got {dt}");
    }

    /// (c) single-mode input.
    #[test]
    fn modal_aware_dt_single_mode() {
        // f_max=20 Hz → 0.5/20=0.025 s; duration/1000 = 5.0/1000=0.005 s
        // → min is 0.005
        let dt = modal_aware_dt(&[20.0], 5.0);
        assert!((dt - 0.005).abs() < 1e-16, "expected 0.005, got {dt}");
    }

    /// (d) empty modes → falls back to duration/1000.
    #[test]
    fn modal_aware_dt_empty_modes_falls_back_to_duration() {
        let dt = modal_aware_dt(&[], 2.0);
        assert!((dt - 0.002).abs() < 1e-16, "expected 0.002, got {dt}");
    }

    /// (e) non-positive/non-finite frequencies are ignored; result stays finite > 0.
    #[test]
    fn modal_aware_dt_ignores_bad_frequencies() {
        // Only valid freq is 10.0 Hz → 0.5/10=0.05; duration/1000=1.0/1000=0.001
        // → min is 0.001
        let dt = modal_aware_dt(&[0.0, -5.0, f64::NAN, f64::INFINITY, 10.0], 1.0);
        assert!(dt > 0.0 && dt.is_finite(), "dt must be finite and positive, got {dt}");
        assert!((dt - 0.001).abs() < 1e-15, "expected 0.001, got {dt}");
    }
}
