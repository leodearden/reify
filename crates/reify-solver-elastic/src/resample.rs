//! Grid-based resampling of P1-tet nodal fields onto a regular 3D grid.
//!
//! Implements [`GridSpec`] and [`resample_nodal_to_grid`] — the primitives
//! used by the `elastic_static` and `buckling` trampolines to produce a
//! `Regular3D Sampled Value::Field` from the FEA nodal solution.
//!
//! # Design decisions
//!
//! - Grid resolution mirrors the solve mesh: `GridSpec.counts = (nx,ny,nz)`
//!   element counts → grid nodes = counts+1, spanning body bounds.
//! - Containment uses a [`TetSpatialIndex`] BVH — O(grid·log elems) per call.
//!   The index is built once per `resample_*` call (O(n·log n)) then queried
//!   O(log n) per grid point.  The instrumented entry points return
//!   [`ResampleStats`] with the exact `point_in_tet_p1` evaluation count for
//!   deterministic complexity assertions in tests.
//! - Out-of-solid grid points carry `f64::NAN` for all stride components.
//! - Data ordering: row-major axis-0(x) outermost → axis-2(z) innermost,
//!   with stride components contiguous per grid point.
//! - [`classify_grid_misses`] / [`nearest_miss_margin`] are the **grid-miss
//!   instrument** (task #6154): a diagnostic sibling of the production path in
//!   the same spirit as [`ResampleStats`]. A raw NaN count cannot distinguish a
//!   mesh that fails to tile its own AABB (a *coverage* defect) from grid points
//!   that sit a round-off outside a face it does tile (a *tolerance* effect) —
//!   both write the same sentinel. The classifier buckets misses by grid INDEX
//!   extremity (never by a second geometric epsilon, so it cannot itself be a
//!   source of the near-boundary error it measures), and `nearest_miss_margin`
//!   reports *how far* outside in the same absolute-barycentric units that
//!   `point_in_tet_p1`'s `tol` is expressed in — making the measured margin
//!   directly comparable to the `tol` a caller passed.

use std::sync::atomic::AtomicBool;

use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

use crate::interpolation::{TetSpatialIndex, barycentric_p1};

/// Per-call resample statistics returned by the instrumented entry points.
///
/// The `point_in_tet_tests` count is fully deterministic — independent of CPU
/// speed or optimization level — and is suitable for exact complexity assertions
/// in tests (e.g. asserting BVH ≥4× fewer evaluations than the linear scan on
/// the same mesh).
#[derive(Debug, Clone, Default)]
pub struct ResampleStats {
    /// Total number of `point_in_tet_p1` evaluations across all grid points
    /// during the BVH traversal.
    pub point_in_tet_tests: u64,
}

/// Element-count grid specification for a regular 3D axis-aligned grid.
///
/// `counts[i]` is the number of **element** intervals along axis i; the
/// grid has `counts[i]+1` nodes along axis i.  The physical extent of the
/// grid is `[bounds_min[i], bounds_max[i]]` with uniform spacing
/// `spacing[i] = (bounds_max[i] - bounds_min[i]) / counts[i]`.
#[derive(Debug, Clone, Copy)]
pub struct GridSpec {
    /// Lower bound of the grid along each axis (SI units).
    pub bounds_min: [f64; 3],
    /// Upper bound of the grid along each axis (SI units).
    pub bounds_max: [f64; 3],
    /// Number of element intervals along each axis (grid nodes = counts+1).
    pub counts: [usize; 3],
}

/// Resample a nodal field defined on a P1-tet mesh onto a regular 3D grid.
///
/// # Arguments
///
/// - `nodes`: node coordinates (length n_nodes), `nodes[n] = [x, y, z]`.
/// - `elems`: element connectivity (length n_elems), `elems[e] = [n0,n1,n2,n3]`.
/// - `nodal_values`: flat array of length `n_nodes × stride`.  Node `n`'s
///   values are `nodal_values[n*stride .. n*stride+stride]`.
/// - `stride`: number of scalar components per node (3 for displacement, 9 for stress).
/// - `grid`: specifies bounds, element counts (→ node counts = counts+1), and spacing.
/// - `name`: field name embedded in the returned [`SampledField`].
/// - `tol`: absolute barycentric tolerance for point-in-tet containment.
///   A value of `1e-9` accepts points within round-off of an element face.
///
/// # Returns
///
/// A [`SampledField`] with:
/// - `kind = Regular3D`
/// - `interpolation = Linear`
/// - `data.len() == (nx+1)*(ny+1)*(nz+1)*stride`
/// - Row-major ordering: axis-0 (x) outermost, axis-2 (z) innermost;
///   the `stride` components are contiguous per grid point.
/// - Grid points outside all elements carry `f64::NAN` for all `stride` components.
///
/// Prefer [`resample_multi_nodal_to_grid`] when sampling two or more fields
/// (displacement + stress) on the same geometry — it halves the point-location cost.
pub fn resample_nodal_to_grid(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    nodal_values: &[f64],
    stride: usize,
    grid: &GridSpec,
    name: &str,
    tol: f64,
) -> SampledField {
    resample_nodal_to_grid_instrumented(nodes, elems, nodal_values, stride, grid, name, tol).0
}

/// Like [`resample_nodal_to_grid`] but also returns [`ResampleStats`] with
/// the `point_in_tet_p1` evaluation count.
///
/// Used in tests to assert O(grid·log elems) complexity via deterministic
/// count comparisons.  The public [`resample_nodal_to_grid`] is a thin
/// wrapper that calls this and discards the stats.
pub fn resample_nodal_to_grid_instrumented(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    nodal_values: &[f64],
    stride: usize,
    grid: &GridSpec,
    name: &str,
    tol: f64,
) -> (SampledField, ResampleStats) {
    let [nx, ny, nz] = grid.counts;
    // Spacing per axis: (max-min)/count
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    // Build per-axis grid coordinates via linspace_inclusive.
    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_nodal_to_grid_instrumented: linspace_inclusive failed — check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    // Build the BVH once for all grid points — O(n·log n).
    let idx = TetSpatialIndex::build(nodes, elems, tol);

    let mut data = Vec::with_capacity(n_grid * stride);
    let mut total_tests: u64 = 0;

    // Row-major iteration: axis-0(x) outermost → axis-2(z) innermost.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                // BVH locate: returns (min-index containing element, point_in_tet_p1 count).
                let (elem_opt, tests) = idx.locate_counted(nodes, elems, p, tol);
                total_tests += tests as u64;

                match elem_opt {
                    Some(e) => {
                        let conn = &elems[e];
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        // Recompute barycentric weights for the located element.
                        // Intentional: `point_in_tet_p1` (called inside `locate_counted`)
                        // already computed identical barycentric coords and discarded them.
                        // The recompute preserves the unchanged `point_in_tet_p1` signature
                        // and guarantees bit-identical output via the same arithmetic as the
                        // original linear-scan path.
                        let bary = barycentric_p1(&phys4, p);
                        for c in 0..stride {
                            let val = bary[0] * nodal_values[conn[0] * stride + c]
                                + bary[1] * nodal_values[conn[1] * stride + c]
                                + bary[2] * nodal_values[conn[2] * stride + c]
                                + bary[3] * nodal_values[conn[3] * stride + c];
                            data.push(val);
                        }
                    }
                    None => {
                        // Out-of-solid sentinel: NaN per stride component.
                        for _ in 0..stride {
                            data.push(f64::NAN);
                        }
                    }
                }
            }
        }
    }

    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: grid.bounds_min.to_vec(),
        bounds_max: grid.bounds_max.to_vec(),
        spacing,
        axis_grids,
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    let stats = ResampleStats { point_in_tet_tests: total_tests };
    (sf, stats)
}

/// Resample **multiple** nodal fields onto the same Regular3D grid in a single
/// geometry pass.
///
/// Semantically equivalent to calling [`resample_nodal_to_grid`] once per entry
/// in `fields`, but the containing-tet + barycentric-weight computation is done
/// **once per grid point** instead of once per *(grid point × field)*.  For the
/// buckling trampoline (~13 k grid points × 61 k tets × 2 fields) this reduces
/// the O(grid·elems) point-location cost — the dominant non-CG step.
///
/// # Arguments
///
/// - `fields`: slice of `(&[f64], usize, &str)` tuples — each is
///   `(nodal_values, stride, name)` for one field.
///   `nodal_values` must have length `n_nodes × stride`.
///
/// All fields share the same `nodes`, `elems`, `grid`, and `tol`.
/// Returns one [`SampledField`] per input entry, in the same order.
pub fn resample_multi_nodal_to_grid(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    fields: &[(&[f64], usize, &str)],
    grid: &GridSpec,
    tol: f64,
) -> Vec<SampledField> {
    resample_multi_nodal_to_grid_instrumented(nodes, elems, fields, grid, tol).0
}

/// Like [`resample_multi_nodal_to_grid`] but also returns [`ResampleStats`].
///
/// The BVH index is built once and shared across all fields; `point_in_tet_tests`
/// counts locate evaluations per grid point (independent of field count).
/// The public [`resample_multi_nodal_to_grid`] is a thin wrapper that calls
/// this and discards the stats.
pub fn resample_multi_nodal_to_grid_instrumented(
    nodes: &[[f64; 3]],
    elems: &[[usize; 4]],
    fields: &[(&[f64], usize, &str)],
    grid: &GridSpec,
    tol: f64,
) -> (Vec<SampledField>, ResampleStats) {
    let [nx, ny, nz] = grid.counts;
    let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
    let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
    let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
    let spacing = vec![sx, sy, sz];

    let axis_grids: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            let sp = spacing[i];
            reify_ir::sampled::linspace_inclusive(grid.bounds_min[i], grid.bounds_max[i], sp)
                .expect("resample_multi_nodal_to_grid_instrumented: linspace_inclusive failed — \
                         check that bounds_min < bounds_max and counts > 0")
        })
        .collect();

    let nx1 = axis_grids[0].len();
    let ny1 = axis_grids[1].len();
    let nz1 = axis_grids[2].len();
    let n_grid = nx1 * ny1 * nz1;

    // Build the BVH once — shared across all fields and all grid points.
    let idx = TetSpatialIndex::build(nodes, elems, tol);

    // Pre-allocate one output buffer per field.
    let mut data_bufs: Vec<Vec<f64>> = fields
        .iter()
        .map(|(_, stride, _)| Vec::with_capacity(n_grid * stride))
        .collect();

    let mut total_tests: u64 = 0;

    // Single geometry pass: locate once per grid point, apply to all fields.
    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let p = [axis_grids[0][ix], axis_grids[1][iy], axis_grids[2][iz]];

                let (elem_opt, tests) = idx.locate_counted(nodes, elems, p, tol);
                total_tests += tests as u64;

                match elem_opt {
                    Some(e) => {
                        let conn = &elems[e];
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        // Recompute barycentric weights (intentional recompute — see
                        // `resample_nodal_to_grid_instrumented` for the full rationale).
                        let bary = barycentric_p1(&phys4, p);
                        // Grid point is inside this tet — interpolate every field.
                        for (fi, (nodal_vals, stride, _)) in fields.iter().enumerate() {
                            for c in 0..*stride {
                                let val = bary[0] * nodal_vals[conn[0] * stride + c]
                                    + bary[1] * nodal_vals[conn[1] * stride + c]
                                    + bary[2] * nodal_vals[conn[2] * stride + c]
                                    + bary[3] * nodal_vals[conn[3] * stride + c];
                                data_bufs[fi].push(val);
                            }
                        }
                    }
                    None => {
                        // Out-of-solid sentinel: NaN for every stride component of every field.
                        for (fi, (_, stride, _)) in fields.iter().enumerate() {
                            for _ in 0..*stride {
                                data_bufs[fi].push(f64::NAN);
                            }
                        }
                    }
                }
            }
        }
    }

    // Assemble one SampledField per input field, sharing the same grid metadata.
    let sampled_fields: Vec<SampledField> = fields
        .iter()
        .zip(data_bufs)
        .map(|((_, _, name), data)| SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: grid.bounds_min.to_vec(),
            bounds_max: grid.bounds_max.to_vec(),
            spacing: spacing.clone(),
            axis_grids: axis_grids.clone(),
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        })
        .collect();

    let stats = ResampleStats { point_in_tet_tests: total_tests };
    (sampled_fields, stats)
}

/// Where a resampled field's out-of-solid (`NaN`) grid points landed, bucketed
/// by how many grid axes the point is **extreme** on (task #6154).
///
/// The out-of-solid sentinel is normative (PRD `v0_4/fea-result-model.md` §3),
/// so misses are expected in general — a *count* of them therefore diagnoses
/// nothing. What discriminates the two mechanisms is *where* they land:
///
/// - misses confined to `missed_face`/`missed_edge`/`missed_corner` mean the
///   grid's outermost shell sits marginally outside a surface the mesh does
///   tile — a **tolerance** effect, sized by [`nearest_miss_margin`];
/// - any `missed_interior` means the mesh does **not** tile its own AABB — a
///   **coverage** defect, which no tolerance change can legitimately paper over.
///
/// `missed_interior == 0` is only a *geometric prediction* where the AABB IS the
/// solid, i.e. for a prismatic body. For a non-convex or curved solid,
/// index-interior grid points CAN be legitimately outside the material once the
/// grid is fine enough to sample the concavity, so a non-zero `missed_interior`
/// there is not automatically a defect — do not promote `== 0` to a global
/// invariant.
///
/// The claim is about grid RESOLUTION, not about any particular body: coarseness
/// cuts the other way too. The `4×4×7` cylinder fixture in
/// `reify-eval/tests/solve_elastic_static_body_e2e.rs` samples its own curvature
/// so coarsely that every index-interior node still lands inside the material,
/// and it measures `missed_interior == 0` on a decidedly non-prismatic body —
/// so do not read a curved body as implying a non-zero interior count either.
// No `Eq`: `missed_points` carries `f64` coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridMissReport {
    /// Total grid points examined (`∏ axis_grids[a].len()`).
    pub n_grid: usize,
    /// Grid points whose every stride component is `NaN`.
    pub n_missed: usize,
    /// Misses extreme on NO axis (strictly inside the grid's index box).
    pub missed_interior: usize,
    /// Misses extreme on exactly 1 axis (an AABB face).
    pub missed_face: usize,
    /// Misses extreme on exactly 2 axes (an AABB edge).
    pub missed_edge: usize,
    /// Misses extreme on all 3 axes (an AABB corner).
    pub missed_corner: usize,
    /// Grid points carrying a non-finite component that is NOT the out-of-solid
    /// sentinel — i.e. any point with `!v.is_finite()` somewhere whose `stride`
    /// components are not *all* `NaN`.
    ///
    /// Not a miss — the sentinel is written all-or-nothing, so such a point is a
    /// non-finite *solution* value (a diverged solve), a different defect. It is
    /// counted rather than bucketed, in BOTH build profiles, because that count
    /// is the only thing between a broken field and a report claiming full
    /// coverage.
    ///
    /// The predicate is `!is_finite()`, not `is_nan()`: a diverged solve
    /// overflows to `±INF` at least as readily as it produces `NaN`, and an
    /// all-`INF` point has `n_nan == 0` — under a NaN-only test it would be
    /// neither a miss nor an anomaly, i.e. silently reported as a valid, covered
    /// sample. That is the precise inversion this counter exists to prevent.
    ///
    /// A non-zero value here means the rest of this report is describing a field
    /// that is already broken upstream of the sampler, so read it before drawing
    /// any conclusion from the buckets.
    pub n_partial_nan: usize,
    /// Physical coordinates of each miss, in visit order.
    pub missed_points: Vec<[f64; 3]>,
    /// Per-axis grid indices of each miss, parallel to `missed_points`.
    pub missed_indices: Vec<[usize; 3]>,
}

/// Classify the out-of-solid grid points of a `Regular3D` [`SampledField`].
///
/// Walks the grid in the SAME row-major x-outer/z-inner order the sampler
/// writes — flat index `(ix·ny1 + iy)·nz1 + iz` — derived from `sf.axis_grids`
/// so the classifier cannot drift from the producer's layout.
///
/// A grid point counts as missed iff **all** `stride` components are `NaN`,
/// matching the sentinel's all-or-nothing write. Any OTHER non-finite point — a
/// `NaN` in some but not all components, or an `±INF` anywhere — is a different
/// defect (a non-finite solution value, not an out-of-solid marker), so it is
/// never bucketed: it increments [`GridMissReport::n_partial_nan`] instead.
///
/// That counter, and not an assertion, is what carries the signal. Flagging the
/// condition with a `debug_assert!` was tried and removed: whether the caller's
/// field is finite is a property of the *field*, not an invariant of this
/// function, so a debug-profile abort would deny the caller the very report that
/// exists to describe the situation — and would make the instrument's two build
/// profiles disagree about whether the anomaly is reportable at all. Callers who
/// want it loud assert on the counter (the `reify-eval` e2e reconciler does).
///
/// Bucketing is purely INDEX-based — an axis is *extreme* when
/// `index == 0 || index == axis_grids[a].len() - 1`. Because the §7a grid spans
/// exactly the mesh AABB by construction, index-extremity **is** AABB-boundary,
/// so no coordinate comparison (and no second tolerance to tune) is needed. That
/// matters: a geometric classifier would depend on the same near-boundary float
/// comparison under investigation, and so could not be trusted to judge it.
///
/// # Panics
///
/// Panics if `stride == 0`, if `sf` is not 3-axis, if any axis grid is EMPTY, or
/// if `sf.data.len()` is not `n_grid · stride` — all of which indicate the field
/// was not produced by the `resample_*` entry points this instrument is a
/// sibling of.
pub fn classify_grid_misses(sf: &SampledField, stride: usize) -> GridMissReport {
    assert!(stride > 0, "classify_grid_misses: stride must be > 0");
    assert_eq!(
        sf.axis_grids.len(),
        3,
        "classify_grid_misses: expected a Regular3D field (3 axis grids), got {}",
        sf.axis_grids.len(),
    );
    let nx1 = sf.axis_grids[0].len();
    let ny1 = sf.axis_grids[1].len();
    let nz1 = sf.axis_grids[2].len();
    // Checked BEFORE `last` is computed below: an empty axis underflows
    // `len - 1`, which is a bare "attempt to subtract with overflow" in debug
    // and a wrap to `usize::MAX` in release (harmless only by accident, because
    // `n_grid == 0` makes the loops no-ops). `linspace_inclusive` errors first
    // on the `resample_*` entry points, so this is unreachable through them —
    // but this is a public fn with a deliberately curated `# Panics` list, so
    // the condition is checked, not assumed.
    assert!(
        nx1 > 0 && ny1 > 0 && nz1 > 0,
        "classify_grid_misses: every axis grid must be non-empty, got lengths \
         [{nx1}, {ny1}, {nz1}]",
    );
    let n_grid = nx1 * ny1 * nz1;
    assert_eq!(
        sf.data.len(),
        n_grid * stride,
        "classify_grid_misses: data len {} != n_grid {} × stride {}",
        sf.data.len(),
        n_grid,
        stride,
    );

    let mut report = GridMissReport { n_grid, ..Default::default() };
    let last = [nx1 - 1, ny1 - 1, nz1 - 1];

    for ix in 0..nx1 {
        for iy in 0..ny1 {
            for iz in 0..nz1 {
                let flat = (ix * ny1 + iy) * nz1 + iz;
                let comps = &sf.data[flat * stride..flat * stride + stride];
                let n_nan = comps.iter().filter(|v| v.is_nan()).count();
                if n_nan != stride {
                    // Not the all-or-nothing out-of-solid sentinel. Anything
                    // non-finite HERE is therefore a diverged solution value —
                    // and the test is `!is_finite()`, not `is_nan()`, because an
                    // all-`INF` point has `n_nan == 0` and would otherwise fall
                    // through as a valid, covered sample.
                    if comps.iter().any(|v| !v.is_finite()) {
                        report.n_partial_nan += 1;
                    }
                    continue;
                }

                let idx = [ix, iy, iz];
                let n_extreme = (0..3).filter(|&a| idx[a] == 0 || idx[a] == last[a]).count();
                match n_extreme {
                    0 => report.missed_interior += 1,
                    1 => report.missed_face += 1,
                    2 => report.missed_edge += 1,
                    _ => report.missed_corner += 1,
                }
                report.n_missed += 1;
                report.missed_points.push([
                    sf.axis_grids[0][ix],
                    sf.axis_grids[1][iy],
                    sf.axis_grids[2][iz],
                ]);
                report.missed_indices.push(idx);
            }
        }
    }

    report
}

/// How far outside the mesh a point is, in the units [`point_in_tet_p1`]'s
/// `tol` is expressed in (task #6154).
///
/// Returns `max_e min_i barycentric_p1(elems[e], p)[i]`: the best (least
/// negative) minimum barycentric coordinate over all elements.
///
/// - `>= 0` — `p` is inside some element;
/// - `< 0`  — `p` misses every element, and `|margin|` is the shortfall in
///   *absolute barycentric slack*, i.e. exactly the quantity a caller's `tol`
///   is compared against. A point that misses at `tol` would be accepted by any
///   `tol > |margin|`.
///
/// Because the coordinates are barycentric they are scale-invariant, so the
/// magnitude is directly readable as a fraction of tet extent: `~1e-7` is
/// round-off against a face the mesh tiles, `~0.5` is a hole where an element
/// should have been. That separation is the whole diagnostic value.
///
/// DEGENERATE elements are skipped, not measured against — see the loop comment.
///
/// Returns [`f64::NEG_INFINITY`] for an empty mesh, and for the same reason when
/// every element is degenerate: neither leaves an element to measure against, and
/// a caller applying the coverage-vs-round-off magnitude rule must therefore test
/// [`f64::is_finite`] before reading the sign.
pub fn nearest_miss_margin(nodes: &[[f64; 3]], elems: &[[usize; 4]], p: [f64; 3]) -> f64 {
    let mut best = f64::NEG_INFINITY;
    for conn in elems {
        let phys4: [[f64; 3]; 4] =
            [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
        let bary = barycentric_p1(&phys4, p);
        // A degenerate (zero-volume, collapsed or sliver) tet yields NON-FINITE
        // barycentric coordinates — `barycentric_p1`'s own documented behaviour,
        // and its degeneracy guard is a `debug_assert!`, so in a RELEASE build
        // those NaNs/infinities reach here unannounced.
        //
        // They must be skipped rather than folded, because `f64::min` returns the
        // OTHER operand when one side is NaN: an all-NaN `bary` folded from
        // `f64::INFINITY` collapses to `+INFINITY`, which is the largest possible
        // `best` — so ONE collapsed tet anywhere in `elems` would make EVERY query
        // point report as comfortably inside, i.e. this instrument would assert
        // the exact opposite of the coverage defect it exists to detect.
        //
        // Skipping is also the geometrically correct answer: a zero-volume tet
        // contains nothing, so it can never be the element `p` is nearest inside
        // of, and the margin against the REST of the mesh is still what this
        // function owes its caller.
        if !bary.iter().all(|b| b.is_finite()) {
            continue;
        }
        let min_i = bary.iter().copied().fold(f64::INFINITY, f64::min);
        if min_i > best {
            best = min_i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::{GridSpec, resample_nodal_to_grid};
    use reify_ir::{InterpolationKind, SampledGridKind};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Build a single tet (unit tetrahedron with one corner at origin).
    /// Connectivity: [0,1,2,3]; nodes: (0,0,0),(1,0,0),(0,1,0),(0,0,1).
    fn unit_tet() -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let elems = vec![[0usize, 1, 2, 3]];
        (nodes, elems)
    }

    // ── Test (a): stride-3 affine field recovers exactly at interior grid points ──

    /// Affine displacement field u(x) = A·x + b where
    ///   A = diag(2,3,4), b = (5,6,7)
    /// Nodal values at the unit tet corners.
    #[test]
    fn resample_stride3_affine_exact_interior() {
        let (nodes, elems) = unit_tet();

        // nodal_values for each node: u(node) = A·node + b
        let a = [2.0_f64, 3.0, 4.0];
        let b = [5.0_f64, 6.0, 7.0];
        let mut nodal_values = vec![0.0_f64; nodes.len() * 3];
        for (i, node) in nodes.iter().enumerate() {
            for c in 0..3 {
                nodal_values[i * 3 + c] = a[c] * node[c] + b[c];
            }
        }

        // Grid: 2 elements along each axis → 3 nodes each; spans the tet's
        // bounding box [0,1]×[0,1]×[0,1].  Interior grid points are at
        // (0.5, 0.0, 0.0), etc. — only the centroid (0.25,0.25,0.25) is
        // strictly inside the tet.
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [2, 2, 2],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 3, &grid, "u", 1e-9);

        // (b): verify metadata
        assert_eq!(sf.kind, SampledGridKind::Regular3D);
        // Grid nodes = counts+1 = 3 per axis → 27 total; data = 27*3 = 81
        assert_eq!(sf.data.len(), 3 * 3 * 3 * 3, "data len");
        assert_eq!(sf.interpolation, InterpolationKind::Linear);

        // axis_grids: linspace(0,1, spacing=0.5) → [0.0, 0.5, 1.0]
        for ax in 0..3 {
            assert_eq!(sf.axis_grids[ax].len(), 3, "axis {ax} len");
            assert!((sf.axis_grids[ax][0] - 0.0).abs() < 1e-12);
            assert!((sf.axis_grids[ax][1] - 0.5).abs() < 1e-12);
            assert!((sf.axis_grids[ax][2] - 1.0).abs() < 1e-12);
        }

        // bounds_min/max
        assert_eq!(sf.bounds_min, vec![0.0, 0.0, 0.0]);
        assert_eq!(sf.bounds_max, vec![1.0, 1.0, 1.0]);
        // spacing = (max-min)/counts = 0.5 per axis
        for ax in 0..3 {
            assert!((sf.spacing[ax] - 0.5).abs() < 1e-12, "spacing[{ax}]");
        }

        // (a): check the origin node (ix=0,iy=0,iz=0) — exactly at node 0.
        // Row-major: flat index = (ix*(ny+1) + iy)*(nz+1) + iz
        //   origin → 0, data offset = 0*3 = 0
        let origin_data: Vec<f64> = sf.data[0..3].to_vec();
        for c in 0..3 {
            let expected = b[c]; // u(0,0,0) = b
            assert!(
                (origin_data[c] - expected).abs() < 1e-12,
                "origin component {c}: got {}, expected {}",
                origin_data[c],
                expected
            );
        }

        // centroid of tet: (0.25, 0.25, 0.25) — must be INSIDE the unit tet
        // (barycentric: λ0=0.25, λ1=0.25, λ2=0.25, λ3=0.25 — all positive).
        // But it's not a grid node. Instead, check node at (0.5,0.0,0.0):
        // ix=1, iy=0, iz=0 → flat idx = (1*3+0)*3+0 = 9; data offset = 9*3 = 27
        // u(0.5,0,0) = [a[0]*0.5+b[0], b[1], b[2]] = [6.0, 6.0, 7.0]
        // BUT: (0.5,0.0,0.0) is on the edge of the tet — we allow tol=1e-9.
        let node_100 = &sf.data[27..30];
        let expected_100 = [a[0] * 0.5 + b[0], b[1], b[2]];
        for c in 0..3 {
            assert!(
                (node_100[c] - expected_100[c]).abs() < 1e-12,
                "node(1,0,0) component {c}: got {}, expected {}",
                node_100[c],
                expected_100[c]
            );
        }
    }

    // ── Test (c): grid point outside all elements → NaN ──────────────────────

    #[test]
    fn resample_outside_solid_is_nan() {
        let (nodes, elems) = unit_tet();

        // trivial nodal values (constant)
        let nodal_values = vec![1.0_f64; nodes.len() * 3];

        // Grid spanning [0,2]×[0,2]×[0,2] — points at x=1,y=1,z=1 etc.
        // are far outside the unit tet.
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [2.0, 2.0, 2.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 3, &grid, "u", 1e-9);

        // 8 grid nodes (2×2×2). Find at least one that's NaN (the corner at (2,2,2)).
        // Flat index of (ix=1,iy=1,iz=1) = (1*2+1)*2+1 = 5; data offset = 5*3 = 15
        let outside = &sf.data[15..18];
        for (c, &val) in outside.iter().enumerate() {
            assert!(
                val.is_nan(),
                "outside[{c}] should be NaN, got {}",
                val
            );
        }
    }

    // ── Test (d): stride-9 constant tensor round-trips ────────────────────────

    #[test]
    fn resample_stride9_constant_tensor_roundtrip() {
        let (nodes, elems) = unit_tet();

        // Constant stress tensor at every node: identity-like
        // [1,2,3,4,5,6,7,8,9] (row-major 3×3)
        let tensor: [f64; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let nodal_values: Vec<f64> = nodes
            .iter()
            .flat_map(|_| tensor.iter().copied())
            .collect();

        // Grid: 1 element per axis → 2 nodes per axis; only origin [0,0,0] is in tet
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 9, &grid, "stress", 1e-9);

        // data.len == 2*2*2*9 = 72
        assert_eq!(sf.data.len(), 72, "stride-9 data len");

        // origin node (ix=0,iy=0,iz=0) → exactly at node 0 → barycentric [1,0,0,0]
        // → should recover tensor exactly
        let origin = &sf.data[0..9];
        for (i, &expected) in tensor.iter().enumerate() {
            assert!(
                (origin[i] - expected).abs() < 1e-12,
                "tensor component {i}: got {}, expected {}",
                origin[i],
                expected
            );
        }
    }

    // ── Test (e): data ordering — row-major x-outer / z-inner ────────────────

    #[test]
    fn resample_data_ordering_row_major_x_outer_z_inner() {
        // Build a 2×2×2 hex that exactly tiles the [0,1]³ cube with 6 tets.
        // We just use the unit tet but with a grid of counts=[1,1,1] (2³ nodes).
        // Since the tet only covers part of the cube, many corners will be NaN.
        // We use a box mesh instead.

        // 2×2×2 box mesh: 8 nodes at corners, split into 6 tets (Freudenthal).
        let nodes: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];
        let elems: Vec<[usize; 4]> = vec![
            [0, 1, 2, 6],
            [0, 2, 3, 6],
            [0, 5, 1, 6],
            [0, 3, 7, 6],
            [0, 4, 5, 6],
            [0, 7, 4, 6],
        ];

        // Nodal field: f(node) = 10*x + y  (stride 1 for simplicity)
        let nodal_values: Vec<f64> = nodes.iter().map(|n| 10.0 * n[0] + n[1]).collect();

        // Grid: counts=[1,1,1] → 2×2×2 = 8 grid nodes at axis values [0,1]³
        let grid = GridSpec {
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
            counts: [1, 1, 1],
        };

        let sf = resample_nodal_to_grid(&nodes, &elems, &nodal_values, 1, &grid, "f", 1e-9);

        // Verify all 8 grid nodes are finite (the box covers all corners).
        for (i, &v) in sf.data.iter().enumerate() {
            assert!(!v.is_nan(), "grid point {i} should be finite, got NaN");
        }

        // Row-major x-outer z-inner: flat index = (ix*(ny+1)+iy)*(nz+1)+iz
        // For counts=[1,1,1], nx=1,ny=1,nz=1:
        //   idx(ix,iy,iz) = (ix*2+iy)*2+iz
        //
        // Grid node (1,0,0) → ix=1,iy=0,iz=0 → flat = (1*2+0)*2+0 = 4
        // Coords = (1.0, 0.0, 0.0) → f = 10*1+0 = 10.0
        assert!(
            (sf.data[4] - 10.0).abs() < 1e-12,
            "node(1,0,0) expected 10.0, got {}",
            sf.data[4]
        );

        // Grid node (0,1,0) → ix=0,iy=1,iz=0 → flat = (0*2+1)*2+0 = 2
        // Coords = (0.0, 1.0, 0.0) → f = 10*0+1 = 1.0
        assert!(
            (sf.data[2] - 1.0).abs() < 1e-12,
            "node(0,1,0) expected 1.0, got {}",
            sf.data[2]
        );

        // Grid node (1,1,0) → ix=1,iy=1,iz=0 → flat = (1*2+1)*2+0 = 6
        // Coords = (1.0, 1.0, 0.0) → f = 10*1+1 = 11.0
        assert!(
            (sf.data[6] - 11.0).abs() < 1e-12,
            "node(1,1,0) expected 11.0, got {}",
            sf.data[6]
        );
    }
}

// ── Step-5/6: BVH-backed resample tests ──────────────────────────────────────
//
// Step-5 RED: imports ResampleStats / instrumented fns which don't exist yet
//             → compile error → RED.
// Step-6 GREEN: those items exist → module compiles → tests pass.

#[cfg(test)]
mod bvh_tests {
    // These imports drive the RED compile error in step 5; they resolve in step 6.
    use super::{
        GridSpec, resample_multi_nodal_to_grid_instrumented,
        resample_nodal_to_grid_instrumented,
    };
    use crate::interpolation::barycentric_p1;

    // ── Fixtures ─────────────────────────────────────────────────────────────

    /// Build a Freudenthal box-of-tets: M³ hexes → 6·M³ tets tiling [0,1]³.
    ///
    /// Node index: `ix*(M+1)²+iy*(M+1)+iz`; physical coords: `(ix/M, iy/M, iz/M)`.
    /// Per-hex Freudenthal decomposition uses the (1,1,1)-corner diagonal n6,
    /// matching the existing 6-tet fixture in `tests::resample_data_ordering_…`.
    ///
    /// Hexes are emitted x-outer/z-inner, 6 consecutive tets each, so hex
    /// `(cx,cy,cz)` owns `elems[6*((cx*M + cy)*M + cz) .. +6]` — relied on by
    /// `miss_diag_tests` to punch a coverage hole in one specific hex.
    ///
    /// `pub(super)` so the sibling `miss_diag_tests` module can reuse this one
    /// reference tiling rather than authoring a second tet fixture (#6154).
    pub(super) fn make_box_of_tets(m: usize) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let m1 = m + 1;
        let mut nodes = Vec::with_capacity(m1 * m1 * m1);
        for ix in 0..=m {
            for iy in 0..=m {
                for iz in 0..=m {
                    nodes.push([
                        ix as f64 / m as f64,
                        iy as f64 / m as f64,
                        iz as f64 / m as f64,
                    ]);
                }
            }
        }
        let node = |ix: usize, iy: usize, iz: usize| ix * m1 * m1 + iy * m1 + iz;
        let mut elems = Vec::with_capacity(6 * m * m * m);
        for cx in 0..m {
            for cy in 0..m {
                for cz in 0..m {
                    let n0 = node(cx, cy, cz);
                    let n1 = node(cx + 1, cy, cz);
                    let n2 = node(cx + 1, cy + 1, cz);
                    let n3 = node(cx, cy + 1, cz);
                    let n4 = node(cx, cy, cz + 1);
                    let n5 = node(cx + 1, cy, cz + 1);
                    let n6 = node(cx + 1, cy + 1, cz + 1);
                    let n7 = node(cx, cy + 1, cz + 1);
                    elems.push([n0, n1, n2, n6]);
                    elems.push([n0, n2, n3, n6]);
                    elems.push([n0, n5, n1, n6]);
                    elems.push([n0, n3, n7, n6]);
                    elems.push([n0, n4, n5, n6]);
                    elems.push([n0, n7, n4, n6]);
                }
            }
        }
        (nodes, elems)
    }

    /// Grid spanning slightly beyond [0,1]³ so it includes interior, shared-face
    /// boundary, AND outside (NaN) grid points.
    fn test_grid() -> GridSpec {
        GridSpec {
            bounds_min: [-0.1, -0.1, -0.1],
            bounds_max: [1.1, 1.1, 1.1],
            counts: [5, 5, 5],
        }
    }

    /// Linear-scan oracle mirroring the old `resample_nodal_to_grid` loop
    /// byte-for-byte (same barycentric check + same weight arithmetic +
    /// same break-on-first-hit = lowest-index hit).
    ///
    /// Returns `(data, point_in_tet_test_count)` so callers can assert both
    /// the bit-identical values AND the O(grid·n_elems) baseline count.
    fn linear_resample_single(
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        nodal_values: &[f64],
        stride: usize,
        grid: &GridSpec,
        tol: f64,
    ) -> (Vec<f64>, u64) {
        let [nx, ny, nz] = grid.counts;
        let sx = (grid.bounds_max[0] - grid.bounds_min[0]) / nx.max(1) as f64;
        let sy = (grid.bounds_max[1] - grid.bounds_min[1]) / ny.max(1) as f64;
        let sz = (grid.bounds_max[2] - grid.bounds_min[2]) / nz.max(1) as f64;
        let ax = reify_ir::sampled::linspace_inclusive(grid.bounds_min[0], grid.bounds_max[0], sx)
            .unwrap();
        let ay = reify_ir::sampled::linspace_inclusive(grid.bounds_min[1], grid.bounds_max[1], sy)
            .unwrap();
        let az = reify_ir::sampled::linspace_inclusive(grid.bounds_min[2], grid.bounds_max[2], sz)
            .unwrap();
        let n_grid = ax.len() * ay.len() * az.len();
        let mut data = Vec::with_capacity(n_grid * stride);
        let mut count = 0u64;
        for &x in &ax {
            for &y in &ay {
                for &z in &az {
                    let p = [x, y, z];
                    let mut found = false;
                    'scan: for conn in elems {
                        let phys4: [[f64; 3]; 4] = [
                            nodes[conn[0]],
                            nodes[conn[1]],
                            nodes[conn[2]],
                            nodes[conn[3]],
                        ];
                        let bary = barycentric_p1(&phys4, p);
                        count += 1;
                        if bary.iter().all(|&b| b >= -tol && b <= 1.0 + tol) {
                            for c in 0..stride {
                                let val = bary[0] * nodal_values[conn[0] * stride + c]
                                    + bary[1] * nodal_values[conn[1] * stride + c]
                                    + bary[2] * nodal_values[conn[2] * stride + c]
                                    + bary[3] * nodal_values[conn[3] * stride + c];
                                data.push(val);
                            }
                            found = true;
                            break 'scan;
                        }
                    }
                    if !found {
                        for _ in 0..stride {
                            data.push(f64::NAN);
                        }
                    }
                }
            }
        }
        (data, count)
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Step-5 RED / Step-6 GREEN:
    ///
    /// (1) BIT-IDENTICAL — every output element equals the linear oracle via
    ///     `f64::to_bits()` (NaN-aware) for stride 1, 3, and 9, for both
    ///     `resample_nodal_to_grid_instrumented` and
    ///     `resample_multi_nodal_to_grid_instrumented`, on a grid spanning
    ///     slightly beyond [0,1]³ (interior + boundary + outside NaN points).
    ///
    /// (2) QUERY-COUNT — on M=4 (384 tets) the BVH
    ///     `stats.point_in_tet_tests * 4 < linear_count` (≥4× efficiency).
    ///
    /// (3) SCALING — from M=4 to M=8 (8× more tets, same grid):
    ///     - linear count grows >4×  (confirms Θ(grid·n_elems) scaling)
    ///     - BVH count grows <2×     (confirms sub-linear/log query)
    ///
    /// Fails to compile in step 5 (ResampleStats / instrumented fns absent) → RED.
    #[test]
    fn bvh_resample_bit_identical_and_query_count() {
        let (nodes4, elems4) = make_box_of_tets(4);
        let (nodes8, elems8) = make_box_of_tets(8);
        assert_eq!(elems4.len(), 384, "M=4: 6*4³=384 tets");
        assert_eq!(elems8.len(), 3072, "M=8: 6*8³=3072 tets");

        let grid = test_grid(); // 6×6×6 = 216 grid points in [-0.1,1.1]³
        let tol = 1e-9_f64;

        // Non-trivial nodal fields — varied to catch any bary-weight/index bugs.
        let nv_s3_4: Vec<f64> = nodes4
            .iter()
            .flat_map(|n| {
                [
                    2.0 * n[0] + n[1] + 0.5,
                    n[0] + 3.0 * n[1] + n[2],
                    0.5 * n[1] + n[2] + 1.0,
                ]
            })
            .collect();
        let nv_s3_8: Vec<f64> = nodes8
            .iter()
            .flat_map(|n| {
                [
                    2.0 * n[0] + n[1] + 0.5,
                    n[0] + 3.0 * n[1] + n[2],
                    0.5 * n[1] + n[2] + 1.0,
                ]
            })
            .collect();
        let nv_s9_4: Vec<f64> = nodes4
            .iter()
            .flat_map(|n| {
                let (x, y, z) = (n[0], n[1], n[2]);
                [
                    x + y,
                    y + z,
                    x + z,
                    x * 2.0,
                    y * 2.0,
                    z * 2.0,
                    x + y + z,
                    x - y,
                    y - z,
                ]
            })
            .collect();
        let nv_s1_4: Vec<f64> =
            nodes4.iter().map(|n| n[0] + 2.0 * n[1] + 3.0 * n[2]).collect();

        // ── (1a) BIT-IDENTICAL: single fn, stride 3 ──────────────────────────
        let (sf_s3, stats_s3) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s3_4, 3, &grid, "u_s3", tol,
        );
        let (lin_s3, lin_count_s3) = linear_resample_single(
            &nodes4, &elems4, &nv_s3_4, 3, &grid, tol,
        );
        assert_eq!(sf_s3.data.len(), lin_s3.len(), "stride-3: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s3.data.iter().zip(lin_s3.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-3 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1b) BIT-IDENTICAL: single fn, stride 9 ──────────────────────────
        let (sf_s9, _) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s9_4, 9, &grid, "sigma_s9", tol,
        );
        let (lin_s9, _) = linear_resample_single(&nodes4, &elems4, &nv_s9_4, 9, &grid, tol);
        assert_eq!(sf_s9.data.len(), lin_s9.len(), "stride-9: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s9.data.iter().zip(lin_s9.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-9 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1c) BIT-IDENTICAL: single fn, stride 1 ──────────────────────────
        let (sf_s1, _) = resample_nodal_to_grid_instrumented(
            &nodes4, &elems4, &nv_s1_4, 1, &grid, "f_s1", tol,
        );
        let (lin_s1, _) = linear_resample_single(&nodes4, &elems4, &nv_s1_4, 1, &grid, tol);
        assert_eq!(sf_s1.data.len(), lin_s1.len(), "stride-1: data lengths must match");
        for (i, (&bvh, &lin)) in sf_s1.data.iter().zip(lin_s1.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "stride-1 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (1d) BIT-IDENTICAL: multi fn, stride 3 + stride 9 ───────────────
        let fields_multi: &[(&[f64], usize, &str)] = &[
            (nv_s3_4.as_slice(), 3, "u_multi"),
            (nv_s9_4.as_slice(), 9, "sigma_multi"),
        ];
        let (sf_multi, stats_multi) = resample_multi_nodal_to_grid_instrumented(
            &nodes4, &elems4, fields_multi, &grid, tol,
        );
        assert_eq!(sf_multi.len(), 2, "multi must return 2 fields");
        for (i, (&bvh, &lin)) in sf_multi[0].data.iter().zip(lin_s3.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "multi stride-3 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }
        for (i, (&bvh, &lin)) in sf_multi[1].data.iter().zip(lin_s9.iter()).enumerate() {
            assert_eq!(
                bvh.to_bits(),
                lin.to_bits(),
                "multi stride-9 BIT-IDENTICAL failed at index {i}: bvh={bvh} lin={lin}",
            );
        }

        // ── (2) QUERY-COUNT: BVH ≥4× fewer tests than linear on M=4 ─────────
        assert!(
            stats_s3.point_in_tet_tests * 4 < lin_count_s3,
            "QUERY-COUNT: BVH ({bvh_n}) * 4 = {quad} should be < linear ({lin_n}); \
             BVH must be ≥4× more efficient on M=4 (384 tets)",
            bvh_n = stats_s3.point_in_tet_tests,
            quad = stats_s3.point_in_tet_tests * 4,
            lin_n = lin_count_s3,
        );
        // Also check the multi stats — same locate cost, same assertion.
        assert!(
            stats_multi.point_in_tet_tests * 4 < lin_count_s3,
            "QUERY-COUNT (multi): BVH ({bvh_n}) * 4 = {quad} should be < linear ({lin_n})",
            bvh_n = stats_multi.point_in_tet_tests,
            quad = stats_multi.point_in_tet_tests * 4,
            lin_n = lin_count_s3,
        );

        // ── (3) SCALING: M=4 → M=8 (8× more tets, same grid) ────────────────
        let (_, stats_s3_m8) = resample_nodal_to_grid_instrumented(
            &nodes8, &elems8, &nv_s3_8, 3, &grid, "u_m8", tol,
        );
        let (_, lin_count_s3_m8) =
            linear_resample_single(&nodes8, &elems8, &nv_s3_8, 3, &grid, tol);

        assert!(
            lin_count_s3_m8 > lin_count_s3 * 4,
            "SCALING linear: M=8 count {c8} must be >4× M=4 count {c4} \
             (confirms Θ(grid·n_elems) growth with 8× more tets)",
            c8 = lin_count_s3_m8,
            c4 = lin_count_s3,
        );
        assert!(
            stats_s3_m8.point_in_tet_tests < stats_s3.point_in_tet_tests * 2,
            "SCALING BVH: M=8 count {c8} must be <2× M=4 count {c4} \
             (confirms sub-linear/log growth)",
            c8 = stats_s3_m8.point_in_tet_tests,
            c4 = stats_s3.point_in_tet_tests,
        );
    }
}

/// Task #6154 — the grid-miss diagnostic instrument.
///
/// These three fixtures exist to prove ONE property before the instrument is
/// pointed at the realized box: that it can tell a **coverage hole** apart from
/// **boundary round-off**. Both show up as `f64::NAN` in the sampled data and
/// are indistinguishable by a raw NaN count — which is exactly why the raw
/// count (1055 of 2989 nodes on the realized box) explains nothing on its own.
///
/// - (a) full tiling, grid == mesh AABB  → zero misses at all;
/// - (b) one INTERIOR hex removed        → miss in the `interior` bucket, margin O(0.5);
/// - (c) grid 1e-7 wider than the mesh   → misses only in face/edge/corner, margin O(1e-7).
#[cfg(test)]
mod miss_diag_tests {
    // These imports drive the RED compile error in step-1; they resolve in step-2.
    use super::bvh_tests::make_box_of_tets;
    use super::{GridSpec, classify_grid_misses, nearest_miss_margin, resample_nodal_to_grid};

    /// Stride-3 nodal values. The classifier reads only NaN-ness, so the actual
    /// magnitudes are irrelevant — a ramp keeps every value finite and distinct
    /// so an interpolated hit can never be mistaken for a sentinel.
    fn ramp(n_nodes: usize) -> Vec<f64> {
        (0..n_nodes * 3).map(|i| i as f64).collect()
    }

    /// The §7a production tolerance, mirrored so these fixtures exercise the
    /// same containment slack the realized path uses.
    const TOL: f64 = 1e-9;

    /// (a) FULL TILING — the grid spans exactly the mesh AABB, and every grid
    /// point coincides with a mesh node (`i/4` is exact in binary), so nothing
    /// may be reported as out-of-solid. This is the instrument's zero.
    #[test]
    fn miss_report_full_tiling_reports_zero_misses() {
        let (nodes, elems) = make_box_of_tets(4);
        let vals = ramp(nodes.len());
        let grid = GridSpec {
            bounds_min: [0.0; 3],
            bounds_max: [1.0; 3],
            counts: [4, 4, 4],
        };
        let sf = resample_nodal_to_grid(&nodes, &elems, &vals, 3, &grid, "u", TOL);
        let report = classify_grid_misses(&sf, 3);

        assert_eq!(report.n_grid, 125, "5×5×5 grid nodes over [0,1]³");
        assert_eq!(
            report.n_missed, 0,
            "a mesh that tiles its own AABB must leave NO out-of-solid grid point; \
             got {} misses (interior={}, face={}, edge={}, corner={})",
            report.n_missed, report.missed_interior, report.missed_face, report.missed_edge,
            report.missed_corner,
        );
        assert_eq!(
            report.missed_interior + report.missed_face + report.missed_edge + report.missed_corner,
            report.n_missed,
            "bucket reconciliation identity must hold even at zero",
        );
    }

    /// (b) INTERIOR HOLE — a genuine COVERAGE defect. Removing the 6 tets of the
    /// interior hex `(1,1,1)` opens a cubic void spanning `[0.25,0.5]³`. On an
    /// 8×8×8-element grid (spacing 0.125) exactly one grid point — index (3,3,3),
    /// the void's centre (0.375, 0.375, 0.375) — falls strictly inside it.
    ///
    /// Index (3,3,3) is extreme on NO axis, so it must land in the `interior`
    /// bucket, and its nearest-miss margin must be O(½ tet), not O(round-off).
    #[test]
    fn miss_report_interior_hole_lands_in_interior_bucket_with_large_margin() {
        const M: usize = 4;
        let (nodes, elems_full) = make_box_of_tets(M);

        // Hex (cx,cy,cz) owns 6 consecutive elems at 6*((cx*M + cy)*M + cz).
        let (cx, cy, cz) = (1usize, 1usize, 1usize);
        let lo = 6 * ((cx * M + cy) * M + cz);
        let elems: Vec<[usize; 4]> = elems_full
            .iter()
            .enumerate()
            .filter(|(i, _)| !(lo..lo + 6).contains(i))
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            elems.len(),
            elems_full.len() - 6,
            "exactly one hex (6 tets) must have been removed",
        );

        let vals = ramp(nodes.len());
        let grid = GridSpec {
            bounds_min: [0.0; 3],
            bounds_max: [1.0; 3],
            counts: [8, 8, 8],
        };
        let sf = resample_nodal_to_grid(&nodes, &elems, &vals, 3, &grid, "u", TOL);
        let report = classify_grid_misses(&sf, 3);

        assert_eq!(report.n_grid, 729, "9×9×9 grid nodes");
        assert!(
            report.missed_interior > 0,
            "a coverage hole must surface in the INTERIOR bucket — that is the \
             signature that distinguishes it from boundary round-off; got \
             interior={}, face={}, edge={}, corner={}",
            report.missed_interior, report.missed_face, report.missed_edge, report.missed_corner,
        );
        assert!(
            report.missed_indices.contains(&[3, 3, 3]),
            "the void centre (0.375,0.375,0.375) = index (3,3,3) must be reported \
             missed; got missed_indices = {:?}",
            report.missed_indices,
        );

        // The discriminator: a coverage hole is O(½ tet) deep, not O(ulp).
        let centre = [0.375, 0.375, 0.375];
        let margin = nearest_miss_margin(&nodes, &elems, centre);
        assert!(
            margin < 0.0,
            "the void centre must be outside every remaining tet, got margin {margin}",
        );
        assert!(
            margin.abs() > 1e-3,
            "a COVERAGE hole must produce a large-magnitude negative margin \
             (O(½ tet) in barycentric units), got {margin} — a margin this small \
             would mean boundary round-off, not a missing element",
        );
    }

    /// (c) BOUNDARY ROUND-OFF — no coverage defect at all. The mesh still tiles
    /// `[0,1]³`; only the GRID is 1e-7 wider on each side. With counts [4,4,4]
    /// the axis points are `-1e-7 + i·0.25000005`, so indices 1..=3 stay strictly
    /// inside and only indices 0 and 4 fall marginally outside.
    ///
    /// So every miss is index-extreme on ≥1 axis, and the split is fully
    /// determined by combinatorics on a 5×5×5 grid:
    ///   0 extreme axes → 3³ = 27 interior (all HITS);
    ///   1 → C(3,1)·2·3² = 54 face;  2 → C(3,2)·2²·3 = 36 edge;  3 → 2³ = 8 corner.
    /// 54 + 36 + 8 = 98 misses, and `missed_interior` must be exactly 0.
    #[test]
    fn miss_report_boundary_roundoff_spares_the_interior_with_tiny_margins() {
        const EPS: f64 = 1e-7;
        let (nodes, elems) = make_box_of_tets(4);
        let vals = ramp(nodes.len());
        let grid = GridSpec {
            bounds_min: [-EPS; 3],
            bounds_max: [1.0 + EPS; 3],
            counts: [4, 4, 4],
        };
        let sf = resample_nodal_to_grid(&nodes, &elems, &vals, 3, &grid, "u", TOL);
        let report = classify_grid_misses(&sf, 3);

        assert_eq!(report.n_grid, 125, "5×5×5 grid nodes");
        assert_eq!(
            report.missed_interior, 0,
            "pure boundary round-off must leave EVERY index-interior grid point \
             finite; a non-zero interior count here would mean the mesh failed to \
             tile its own AABB, which is a different defect entirely",
        );
        assert_eq!(report.missed_face, 54, "1 extreme axis: C(3,1)·2·3² = 54");
        assert_eq!(report.missed_edge, 36, "2 extreme axes: C(3,2)·2²·3 = 36");
        assert_eq!(report.missed_corner, 8, "3 extreme axes: 2³ = 8");
        assert_eq!(
            report.n_missed, 98,
            "every index-boundary point of the 5×5×5 grid, and only those",
        );
        assert_eq!(
            report.missed_interior + report.missed_face + report.missed_edge + report.missed_corner,
            report.n_missed,
            "bucket reconciliation identity",
        );

        // The discriminator, mirrored: round-off misses are O(EPS / tet-edge)
        // ≈ 4e-7 in barycentric units — four orders of magnitude below (b).
        assert_eq!(report.missed_points.len(), report.n_missed);
        for (p, idx) in report.missed_points.iter().zip(&report.missed_indices) {
            let margin = nearest_miss_margin(&nodes, &elems, *p);
            assert!(
                margin < 0.0,
                "missed point {p:?} at index {idx:?} must be outside every tet, \
                 got margin {margin}",
            );
            assert!(
                margin.abs() < 1e-4,
                "boundary round-off must produce a TINY negative margin; point \
                 {p:?} at index {idx:?} gave {margin}, which is coverage-hole scale",
            );
        }
    }

    // ── The instrument's own boundary behaviour ──────────────────────────────
    //
    // (a)-(c) above point the instrument at fields the `resample_*` entry points
    // produced. These pin what it does at its documented edges: the three
    // malformed-input panics, the partial-NaN counter, and the empty mesh.

    /// A well-formed stride-3 field over the fully-tiling fixture, for the
    /// malformed-input cases below to corrupt one property of at a time.
    fn well_formed_field() -> reify_ir::SampledField {
        let (nodes, elems) = make_box_of_tets(4);
        let vals = ramp(nodes.len());
        let grid = GridSpec {
            bounds_min: [0.0; 3],
            bounds_max: [1.0; 3],
            counts: [4, 4, 4],
        };
        resample_nodal_to_grid(&nodes, &elems, &vals, 3, &grid, "u", TOL)
    }

    #[test]
    #[should_panic(expected = "stride must be > 0")]
    fn classify_rejects_zero_stride() {
        // Stride 0 would make every point vacuously "all NaN" (an empty
        // component slice), i.e. report 100% out-of-solid on a perfect mesh.
        let _ = classify_grid_misses(&well_formed_field(), 0);
    }

    #[test]
    #[should_panic(expected = "expected a Regular3D field (3 axis grids), got 2")]
    fn classify_rejects_non_3_axis_field() {
        // The index-extremity bucketing is defined on 3 axes; a 1D/2D field
        // would silently mis-shape the flat-index arithmetic.
        let mut sf = well_formed_field();
        sf.axis_grids.truncate(2);
        let _ = classify_grid_misses(&sf, 3);
    }

    #[test]
    #[should_panic(expected = "data len")]
    fn classify_rejects_data_length_mismatch() {
        // A short buffer means the field was not produced by the `resample_*`
        // entry points this instrument is a sibling of — index out of bounds
        // would be the alternative, with a far worse message.
        let mut sf = well_formed_field();
        sf.data.pop();
        let _ = classify_grid_misses(&sf, 3);
    }

    /// A partially-`NaN` grid point is counted, never bucketed.
    ///
    /// Runs in BOTH profiles. It used to be `#[cfg(not(debug_assertions))]`,
    /// because `classify_grid_misses` also tripped a `debug_assert!` on this
    /// input; that assert is gone (a data-dependent abort in a diagnostic
    /// function denied the caller the report describing the very anomaly it
    /// asked about), so the counter is now the single, profile-independent
    /// signal — and this fixture is what pins it.
    #[test]
    fn miss_report_counts_partially_nan_points_without_bucketing_them() {
        let mut sf = well_formed_field();
        // Fixture (a) proves this field has ZERO misses, so any non-zero bucket
        // below is attributable to the clobber alone.
        sf.data[0] = f64::NAN; // 1 of grid point 0's 3 components

        let report = classify_grid_misses(&sf, 3);

        assert_eq!(
            report.n_partial_nan, 1,
            "a 1-of-3 NaN point must be COUNTED as an anomaly — this counter is \
             the only thing standing between a diverged solve and a report \
             claiming full coverage",
        );
        assert_eq!(
            report.n_missed, 0,
            "a partially-NaN point is a non-finite solution value, not the \
             all-or-nothing out-of-solid sentinel, so it must NOT be a miss",
        );
        assert_eq!(
            (report.missed_interior, report.missed_face, report.missed_edge, report.missed_corner),
            (0, 0, 0, 0),
            "and it must not land in any bucket",
        );
    }

    /// `±INF` is an anomaly too — the case a NaN-only test reports as CLEAN.
    ///
    /// A diverged/overflowing solve reaches `±INF` at least as readily as `NaN`,
    /// and an all-`INF` grid point has `n_nan == 0`: under an `is_nan()`-only
    /// predicate it is neither a miss nor an anomaly, so the instrument would
    /// report a broken field as fully covered — the exact inversion
    /// `n_partial_nan` exists to prevent. Both the all-`INF` point (0 of 3 NaN)
    /// and the mixed `INF`-plus-finite point are pinned, since only the former
    /// is invisible to the old predicate.
    #[test]
    fn miss_report_counts_infinite_points_as_anomalies_not_coverage() {
        let mut sf = well_formed_field();
        // Grid point 0: every component +INF — `n_nan == 0`, so a NaN-only
        // predicate sees a perfectly ordinary sample here.
        sf.data[0] = f64::INFINITY;
        sf.data[1] = f64::INFINITY;
        sf.data[2] = f64::INFINITY;
        // Grid point 1: one component -INF, the rest finite.
        sf.data[4] = f64::NEG_INFINITY;

        let report = classify_grid_misses(&sf, 3);

        assert_eq!(
            report.n_partial_nan, 2,
            "both the all-INF point and the mixed INF/finite point are non-finite \
             SOLUTION values, so both must be counted; an all-INF point has \
             n_nan == 0 and is invisible to an is_nan()-only test",
        );
        assert_eq!(
            report.n_missed, 0,
            "INF is not the out-of-solid sentinel (which is all-NaN), so neither \
             point may be reported as a miss",
        );
        assert_eq!(
            (report.missed_interior, report.missed_face, report.missed_edge, report.missed_corner),
            (0, 0, 0, 0),
            "and neither may land in any bucket",
        );
    }

    #[test]
    #[should_panic(expected = "every axis grid must be non-empty")]
    fn classify_rejects_empty_axis_grid() {
        // An empty axis slips past BOTH other malformed-input asserts — there
        // are still 3 axis grids, and `data.len() == 0 == n_grid × stride` — and
        // then underflows `len - 1`: a bare "attempt to subtract with overflow"
        // in debug, a wrap to `usize::MAX` in release. Neither names the actual
        // problem, so the condition gets its own message.
        let mut sf = well_formed_field();
        sf.axis_grids = vec![Vec::new(), Vec::new(), Vec::new()];
        sf.data.clear();
        let _ = classify_grid_misses(&sf, 3);
    }

    /// The INSIDE branch of [`nearest_miss_margin`] — untested until this
    /// amendment, which is how the degenerate-element hole below survived.
    ///
    /// At the centroid of a tet the barycentric coordinates are exactly
    /// `[¼,¼,¼,¼]`, so that element scores `min_i = ¼`. Tets in a conforming
    /// tiling do not overlap, so every OTHER element scores `≤ 0` at an interior
    /// point of this one, and the max over elements is that ¼ — a sharp value,
    /// not merely a sign.
    #[test]
    fn nearest_miss_margin_inside_point_is_the_containing_tets_barycentric_min() {
        let (nodes, elems) = make_box_of_tets(4);
        let conn = elems[0];
        let mut centroid = [0.0_f64; 3];
        for &n in &conn {
            for c in 0..3 {
                centroid[c] += nodes[n][c] / 4.0;
            }
        }

        let margin = nearest_miss_margin(&nodes, &elems, centroid);

        assert!(
            margin.is_finite(),
            "an inside point must yield a FINITE margin; got {margin} at {centroid:?}",
        );
        assert!(
            margin >= 0.0,
            "`>= 0` is this function's documented signal for `p` is inside some \
             element, and {centroid:?} is the centroid of elems[0]; got {margin}",
        );
        assert!(
            (margin - 0.25).abs() < 1e-12,
            "a tet centroid has barycentric coordinates [¼,¼,¼,¼], so the margin \
             must be exactly ¼ — no other tet of a conforming tiling contains an \
             interior point of elems[0]; got {margin}",
        );
    }

    /// A DEGENERATE element must be SKIPPED, never folded into the reduction.
    ///
    /// `barycentric_p1` documents that a degenerate tet returns non-finite
    /// barycentric coordinates, and guards the condition with a `debug_assert!`
    /// only — so in a RELEASE build those values reach `nearest_miss_margin`.
    /// There `f64::min` returns the OTHER operand when one side is NaN, so
    /// folding an all-NaN `bary` from `f64::INFINITY` collapses to `+INFINITY`:
    /// one collapsed tet anywhere in `elems` would make EVERY query point report
    /// as comfortably inside, the exact inverse of the coverage-vs-round-off
    /// verdict this instrument exists to deliver.
    ///
    /// RELEASE-ONLY by construction: in a debug profile `barycentric_p1`'s own
    /// `debug_assert!` fires first, so only the release behaviour is assertable.
    /// The verify pipeline runs both profiles, so it is exercised. Note that
    /// `miss_report_counts_partially_nan_points_without_bucketing_them` above is
    /// NOT gated this way any more — `classify_grid_misses`' data-dependent
    /// `debug_assert!` was removed, whereas this one is `interpolation.rs`'
    /// contract and stands.
    #[test]
    #[cfg(not(debug_assertions))]
    fn nearest_miss_margin_skips_degenerate_elements() {
        let (nodes, elems) = make_box_of_tets(4);
        // Well outside [0,1]³, so the honest answer is a large negative margin.
        let outside = [2.0, 0.5, 0.5];
        let clean = nearest_miss_margin(&nodes, &elems, outside);
        assert!(
            clean.is_finite() && clean < 0.0,
            "fixture check: {outside:?} must miss the intact mesh by a finite \
             negative margin, got {clean}",
        );

        // (a) COLLAPSED — all four corners are the SAME node, so `J` is the zero
        //     matrix, every cofactor is 0, and every barycentric coordinate is
        //     `0/0 = NaN`. This is precisely the `+INFINITY` path.
        // (b) COPLANAR — four `z = 0` nodes of the tiling, so `det J == 0` with
        //     NON-zero cofactors: a mix of ±∞ and NaN rather than all-NaN.
        for (label, degenerate) in
            [("collapsed", [0usize, 0, 0, 0]), ("coplanar", [0usize, 25, 5, 30])]
        {
            let mut polluted = elems.clone();
            polluted.push(degenerate);
            let got = nearest_miss_margin(&nodes, &polluted, outside);
            assert_eq!(
                got.to_bits(),
                clean.to_bits(),
                "a {label} degenerate tet has zero volume and so contains nothing; \
                 appending it must leave the margin BIT-identical to the intact \
                 mesh's {clean}, got {got}",
            );
        }

        // And a mesh of nothing BUT degenerate elements leaves no element to
        // measure against, so it reports the same non-finite sentinel as an empty
        // mesh — the contract a caller tests `is_finite()` for before reading the
        // sign.
        assert_eq!(
            nearest_miss_margin(&nodes, &[[0, 0, 0, 0]], outside),
            f64::NEG_INFINITY,
            "an all-degenerate mesh must report NEG_INFINITY like an empty one, \
             never a large POSITIVE margin claiming the point is inside",
        );
    }

    #[test]
    fn nearest_miss_margin_on_empty_mesh_is_neg_infinity() {
        // Documented return for an empty element list. Pinned because it
        // compares `< 0.0` and so reads to a caller as "a miss" — the ONLY
        // thing distinguishing "outside the mesh" from "there is no mesh" is
        // that this value is not finite, so a caller applying the
        // coverage-vs-round-off magnitude rule must test `is_finite()` first.
        let (nodes, _elems) = make_box_of_tets(4);
        assert_eq!(
            nearest_miss_margin(&nodes, &[], [0.5, 0.5, 0.5]),
            f64::NEG_INFINITY,
            "an empty mesh has no nearest element, so there is no finite margin",
        );
    }
}
