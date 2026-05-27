//! 2D cross-section meshing for the hex/wedge swept-body pipeline.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! This module is the typed orchestrator that turns a 2D profile boundary
//! (outer ring + optional holes) into a triangle or quad surface mesh,
//! routing the actual Gmsh call through
//! [`reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d`]. Pure-Rust quality
//! helpers ([`compute_quad_skew`], [`recombine_quality_ok`],
//! [`auto_mesh_size_from_boundary`]) live here so they remain unit-testable
//! in stub builds without libgmsh present.

use reify_ir::GeometryError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which 3D element a downstream swept-body sweep step targets.
///
/// `HexPreferred` requests Gmsh's blossom-recombination so 2D quads can be
/// extruded into 3D hex elements; if recombine yields a low-quality quad
/// mesh, [`mesh_swept_profile_2d`] falls back to triangles
/// ([`Mesh2d::Triangle`]) and reports `recombine_attempted=true,
/// recombine_quality_ok=false`. `WedgeOnly` skips recombine entirely and
/// always returns triangles (which a subsequent sweep step turns into
/// wedge elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepElementTarget {
    /// Try to produce a quad mesh (\u{2192} hex extrusion). Falls back to
    /// triangles if recombination fails the skew-quality threshold.
    HexPreferred,
    /// Always produce a triangle mesh (\u{2192} wedge extrusion).
    WedgeOnly,
}

/// 2D mesh of a swept-body cross-section — either all triangles or all
/// quads, never both.
///
/// Vertex coordinates are flat `[x0, y0, x1, y1, …]` with stride 2;
/// connectivity is `[i0, i1, i2, …]` with stride 3 for triangles and 4 for
/// quads. The discriminator carries the element shape implicitly so
/// downstream consumers (`task 2988` sweep step) don't need a separate
/// `ElementShape2d` tag field.
#[derive(Debug, Clone)]
pub enum Mesh2d {
    /// All-triangle mesh. `indices.len() % 3 == 0`.
    Triangle {
        /// Flat `[x0,y0,x1,y1,...]` 2D vertex buffer, stride 2.
        vertices: Vec<f32>,
        /// Flat triangle connectivity, stride 3.
        indices: Vec<u32>,
    },
    /// All-quad mesh. `indices.len() % 4 == 0`.
    Quad {
        /// Flat `[x0,y0,x1,y1,...]` 2D vertex buffer, stride 2.
        vertices: Vec<f32>,
        /// Flat quad connectivity, stride 4 (CCW corner order).
        indices: Vec<u32>,
    },
}

/// Output of [`mesh_swept_profile_2d`] — the produced 2D mesh bundled with
/// recombine-quality diagnostics.
///
/// `recombine_attempted` + `recombine_quality_ok` together let task 2989's
/// diagnostic code emit a "hex meshed" vs "wedge fallback" vs "wedge
/// native" distinction without re-running quality checks downstream.
#[derive(Debug, Clone)]
pub struct Mesh2dReport {
    /// Produced 2D mesh — triangles or quads depending on caller target and
    /// recombine-quality outcome.
    pub mesh: Mesh2d,
    /// `true` iff [`mesh_swept_profile_2d`] asked Gmsh to recombine to
    /// quads (i.e. caller target was [`SweepElementTarget::HexPreferred`]).
    pub recombine_attempted: bool,
    /// `true` iff every quad in the recombined output passed the
    /// `recombine_skew_threshold` quality check, OR no recombine was
    /// attempted (vacuous true). `false` only when recombine was attempted
    /// and at least one quad exceeded the threshold (triggering fall-back
    /// to triangles).
    pub recombine_quality_ok: bool,
}

/// Polygonal description of a 2D cross-section: an outer ring and any
/// number of holes.
///
/// Convention: outer ring CCW, hole rings CW (Gmsh accepts both
/// orientations, but downstream code may use the sign of the shoelace area
/// to disambiguate). All coordinates are 2D `[x, y]` in the profile's
/// local plane. Curved boundaries (arcs, splines) are pre-sampled by the
/// caller into polyline segments — this contract is closed against
/// upstream discretisation strategy.
#[derive(Debug, Clone)]
pub struct ProfileBoundary {
    /// Outer-boundary points (CCW for positive area).
    pub outer: Vec<[f64; 2]>,
    /// Zero or more hole rings (CW for positive holes-out-of-solid area).
    pub holes: Vec<Vec<[f64; 2]>>,
}

/// Errors from [`mesh_swept_profile_2d`].
#[derive(Debug)]
pub enum Mesh2dError {
    /// Outer ring is empty — caller passed nothing to mesh.
    EmptyBoundary,
    /// One of the rings (outer or a hole) has <3 distinct points, or the
    /// outer ring is collinear (zero signed area). Geometrically a non-
    /// surface; rejected before any Gmsh call.
    DegenerateBoundary,
    /// Underlying Gmsh call failed. Wraps the original `GeometryError` for
    /// diagnostic chains.
    GmshFailed(GeometryError),
    /// Gmsh is not available in this build (stub build — libgmsh not
    /// detected at compile time). Callers can choose to fall back to a
    /// different mesher or surface this as a configuration error.
    GmshUnavailable,
}

/// User-tunable knobs for one [`mesh_swept_profile_2d`] call.
///
/// Mirrors the [`reify_kernel_gmsh::MeshingOptions`] shape with one
/// addition (`recombine_skew_threshold`) and one omission (no `threads` —
/// 2D meshing is single-threaded in Gmsh's default algorithm regardless of
/// `General.NumThreads`).
#[derive(Debug, Clone)]
pub struct Mesh2dOptions {
    /// Target characteristic mesh edge length in profile-plane units.
    /// `None` triggers auto-derivation via
    /// [`auto_mesh_size_from_boundary`] with `multiplier=1.0`.
    pub mesh_size: Option<f64>,
    /// When `true`, force single-threaded 2D meshing for bit-deterministic
    /// output. Mirrors `MeshingOptions.deterministic`.
    pub deterministic: bool,
    /// Maximum per-quad skew angle (radians, `|corner_angle - \u{3c0}/2|`)
    /// tolerated before triangle fall-back. Default: \u{3c0}/4 (45° off
    /// square). Pointy/triangular profiles cleanly exceed this; reasonable
    /// rectangular profiles stay well under.
    pub recombine_skew_threshold: f64,
}

impl Default for Mesh2dOptions {
    fn default() -> Self {
        Self {
            mesh_size: None,
            deterministic: false,
            recombine_skew_threshold: std::f64::consts::FRAC_PI_4,
        }
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust quality helpers
// ---------------------------------------------------------------------------

/// Compute the maximum per-corner skew of a 4-node quad.
///
/// Returns `max_i |angle_i - \u{3c0}/2|` over the four interior corners. For
/// a perfect square this is 0.0; for a 30°-leaning parallelogram it is
/// \u{3c0}/6; for a near-collapsed corner it approaches \u{3c0}/2.
///
/// Skew is sign-agnostic: the same value is returned regardless of whether
/// the input quad is in CCW or CW order. Each corner is computed via the
/// unsigned interior angle `atan2(|cross|, dot)` on the prev/next edge
/// vectors emanating from that corner.
///
/// Pure function, no allocation, no Gmsh dependency — unit-testable in
/// stub builds.
pub fn compute_quad_skew(quad: &[[f64; 2]; 4]) -> f64 {
    let half_pi = std::f64::consts::FRAC_PI_2;
    let mut max_dev: f64 = 0.0;
    for i in 0..4 {
        let prev = quad[(i + 3) % 4];
        let curr = quad[i];
        let next = quad[(i + 1) % 4];
        // Edge vectors from `curr` to its prev and next neighbours.
        let e_prev = [prev[0] - curr[0], prev[1] - curr[1]];
        let e_next = [next[0] - curr[0], next[1] - curr[1]];
        let dot = e_prev[0] * e_next[0] + e_prev[1] * e_next[1];
        let cross = e_prev[0] * e_next[1] - e_prev[1] * e_next[0];
        // `atan2(|cross|, dot)` yields the unsigned interior angle in
        // [0, \u{3c0}]; degenerate (zero-length edge) returns 0.0 which
        // gives |0 - \u{3c0}/2| = \u{3c0}/2 — the correct "very bad" score.
        let angle = cross.abs().atan2(dot);
        let dev = (angle - half_pi).abs();
        if dev > max_dev {
            max_dev = dev;
        }
    }
    max_dev
}

/// Check that every quad in a stride-4 index buffer has a maximum per-corner
/// skew under `threshold` radians.
///
/// `vertices` is the flat XY buffer (`stride 2`, f32) feeding both
/// [`Mesh2d::Quad`] and its diagnostic check. `quad_indices` is the
/// stride-4 connectivity into `vertices`; each `chunks_exact(4)` window
/// names one quad's four CCW corners.
///
/// Returns `false` defensively on:
/// - `quad_indices.len() % 4 != 0` (caller bug — quad stride violation).
/// - Any index `>= vertices.len() / 2` (out-of-bounds — connectivity bug).
/// - Any quad whose [`compute_quad_skew`] exceeds `threshold`.
///
/// Returns `true` when `quad_indices.is_empty()` (vacuous — no bad quad).
pub fn recombine_quality_ok(vertices: &[f32], quad_indices: &[u32], threshold: f64) -> bool {
    if !quad_indices.len().is_multiple_of(4) {
        return false;
    }
    let n_verts = vertices.len() / 2;
    for chunk in quad_indices.chunks_exact(4) {
        // Bounds-check each index before indexing — out-of-bounds is a
        // defensive failure (caller corrupted the connectivity buffer).
        for &i in chunk {
            if (i as usize) >= n_verts {
                return false;
            }
        }
        let q: [[f64; 2]; 4] = [
            [
                vertices[(chunk[0] as usize) * 2] as f64,
                vertices[(chunk[0] as usize) * 2 + 1] as f64,
            ],
            [
                vertices[(chunk[1] as usize) * 2] as f64,
                vertices[(chunk[1] as usize) * 2 + 1] as f64,
            ],
            [
                vertices[(chunk[2] as usize) * 2] as f64,
                vertices[(chunk[2] as usize) * 2 + 1] as f64,
            ],
            [
                vertices[(chunk[3] as usize) * 2] as f64,
                vertices[(chunk[3] as usize) * 2 + 1] as f64,
            ],
        ];
        if compute_quad_skew(&q) > threshold {
            return false;
        }
    }
    true
}

/// Derive a target mesh-edge length from a [`ProfileBoundary`]: smallest
/// closed-ring segment length × `multiplier`.
///
/// Iterates the outer ring and every hole ring as closed polylines
/// (`windows(2)` + the wrap-around segment from last point back to first),
/// computes each segment's Euclidean length, and returns the running
/// minimum times `multiplier`.
///
/// Returns `0.0` when `boundary.outer.is_empty()` — mirrors the
/// "unavailable" convention used by
/// [`reify_kernel_gmsh::auto_size::auto_mesh_size_from_features`], letting
/// the caller fall back to the kernel default.
///
/// Pure scalar derivation; no `Result` return.
pub fn auto_mesh_size_from_boundary(boundary: &ProfileBoundary, multiplier: f64) -> f64 {
    if boundary.outer.is_empty() {
        return 0.0;
    }
    let mut min_len = f64::INFINITY;
    let update = |min_len: &mut f64, ring: &[[f64; 2]]| {
        if ring.len() < 2 {
            return;
        }
        // Skip zero-length segments (adjacent duplicate points): they
        // would otherwise yield a min_len of 0.0 and conflate the
        // "duplicate-point caller bug" case with the legitimate
        // "unavailable" sentinel returned for an empty outer ring. The
        // resulting mesh size 0.0 also causes gmsh to fall back to its
        // own default (`Mesh.MeshSizeMin/Max` is only set when > 0), so
        // a ring with all-coincident points reaches the kernel default
        // rather than producing a degenerate mesh.
        for w in ring.windows(2) {
            let dx = w[1][0] - w[0][0];
            let dy = w[1][1] - w[0][1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0 && len < *min_len {
                *min_len = len;
            }
        }
        // …plus the wrap-around segment from last point back to first to
        // close the ring (also skipped if zero-length).
        let last = ring[ring.len() - 1];
        let first = ring[0];
        let dx = first[0] - last[0];
        let dy = first[1] - last[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 && len < *min_len {
            *min_len = len;
        }
    };
    update(&mut min_len, &boundary.outer);
    for hole in &boundary.holes {
        update(&mut min_len, hole);
    }
    if min_len.is_infinite() {
        // Outer ring had <2 distinct points OR every segment was
        // zero-length (all-duplicate ring). Both are caller-bug-ish
        // shapes; fall through to the "unavailable" sentinel so the
        // orchestrator collapses to `None` and gmsh's default mesh-size
        // policy applies.
        return 0.0;
    }
    min_len * multiplier
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Shoelace-formula signed area of a closed 2D ring.
///
/// CCW ring -> positive; CW ring -> negative; collinear / zero-area ring -> 0.0
/// (within float tolerance). Private to the module; used by the
/// `mesh_swept_profile_2d` validation pre-pass to flag degenerate outer rings.
fn ring_signed_area_2d(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let n = ring.len();
    let mut acc: f64 = 0.0;
    for i in 0..n {
        let p = ring[i];
        let q = ring[(i + 1) % n];
        acc += p[0] * q[1] - q[0] * p[1];
    }
    acc * 0.5
}

/// Validation pre-pass shared by every `mesh_swept_profile_2d` target arm.
///
/// Runs before lock acquisition / FFI so error diagnostics stay close to
/// the real cause. Returns `Ok(())` only when the boundary is well-formed
/// (outer ring non-empty, every ring has >=3 points, outer ring has
/// non-zero signed area).
fn validate_boundary(boundary: &ProfileBoundary) -> Result<(), Mesh2dError> {
    if boundary.outer.is_empty() {
        return Err(Mesh2dError::EmptyBoundary);
    }
    if boundary.outer.len() < 3 {
        return Err(Mesh2dError::DegenerateBoundary);
    }
    for hole in &boundary.holes {
        if hole.len() < 3 {
            return Err(Mesh2dError::DegenerateBoundary);
        }
    }
    // Collinear outer ring -> signed area ~ 0. The threshold mirrors
    // `auto_size`'s "geometric tolerance" floor; anything below it is
    // effectively a line segment, not a region.
    if ring_signed_area_2d(&boundary.outer).abs() < 1e-14 {
        return Err(Mesh2dError::DegenerateBoundary);
    }
    // Hole rings must also be non-degenerate: a collinear / near-zero-area
    // hole would either slip through gmsh as an opaque `GmshFailed` or get
    // silently meshed as a slit, neither of which is a useful contract.
    // Mirror the outer-ring tolerance.
    for hole in &boundary.holes {
        if ring_signed_area_2d(hole).abs() < 1e-14 {
            return Err(Mesh2dError::DegenerateBoundary);
        }
    }
    Ok(())
}

/// Mesh a 2D profile boundary into a triangle or quad surface mesh,
/// targeting either a wedge-extrusion or hex-extrusion downstream sweep.
///
/// `boundary` — outer ring + holes; CCW outer, CW holes per the
/// [`ProfileBoundary`] contract.
/// `target` — [`SweepElementTarget::HexPreferred`] enables Gmsh's blossom
/// recombination + skew-quality fall-back to triangles; `WedgeOnly` skips
/// recombination entirely.
/// `options` — mesh size, deterministic flag, recombine-quality threshold
/// (see [`Mesh2dOptions::default`]).
///
/// # Errors
/// - [`Mesh2dError::EmptyBoundary`] — outer ring is empty.
/// - [`Mesh2dError::DegenerateBoundary`] — any ring has <3 points, or
///   outer ring is collinear (zero signed area).
/// - [`Mesh2dError::GmshUnavailable`] — this build was compiled without
///   libgmsh (stub build).
/// - [`Mesh2dError::GmshFailed`] — Gmsh returned an error during meshing.
pub fn mesh_swept_profile_2d(
    boundary: &ProfileBoundary,
    target: SweepElementTarget,
    options: &Mesh2dOptions,
) -> Result<Mesh2dReport, Mesh2dError> {
    validate_boundary(boundary)?;

    // Resolve mesh size: caller override wins, else auto-derive. The
    // `auto_mesh_size_from_boundary(_, 1.0)` returns 0.0 when the outer
    // ring is empty (already rejected) or when no usable segments are
    // present — collapse to `None` so the kernel default applies.
    let resolved_size = match options.mesh_size {
        Some(s) => Some(s),
        None => {
            let auto = auto_mesh_size_from_boundary(boundary, 1.0);
            if auto > 0.0 { Some(auto) } else { None }
        }
    };

    match target {
        SweepElementTarget::WedgeOnly => {
            let result = reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d(
                &boundary.outer,
                &boundary.holes,
                resolved_size,
                false,
                options.deterministic,
            )
            .map_err(map_geometry_error)?;

            let vertices: Vec<f32> = result.vertices_xy.iter().map(|&v| v as f32).collect();
            Ok(Mesh2dReport {
                mesh: Mesh2d::Triangle {
                    vertices,
                    indices: result.triangle_indices,
                },
                recombine_attempted: false,
                // Vacuous: no quads were attempted, so no quad failed
                // the skew threshold.
                recombine_quality_ok: true,
            })
        }
        SweepElementTarget::HexPreferred => {
            let result = reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d(
                &boundary.outer,
                &boundary.holes,
                resolved_size,
                true,
                options.deterministic,
            )
            .map_err(map_geometry_error)?;

            let vertices: Vec<f32> = result.vertices_xy.iter().map(|&v| v as f32).collect();

            // Happy path: a "clean" recombine produced quads only (no
            // leftover triangles from a partial recombination) AND every
            // quad passes the per-corner skew predicate. Returning a
            // `Mesh2d::Quad` when triangles are also present would silently
            // drop those leftover elements — so the presence of any
            // triangle leftover counts as a non-clean recombine and routes
            // through the fall-back path.
            let quality_ok = recombine_quality_ok(
                &vertices,
                &result.quad_indices,
                options.recombine_skew_threshold,
            );
            let clean_recombine =
                !result.quad_indices.is_empty() && result.triangle_indices.is_empty() && quality_ok;
            if clean_recombine {
                return Ok(Mesh2dReport {
                    mesh: Mesh2d::Quad {
                        vertices,
                        indices: result.quad_indices,
                    },
                    recombine_attempted: true,
                    recombine_quality_ok: true,
                });
            }

            // Fall-back optimisation: if the first recombine attempt
            // produced NO quads at all (gmsh tried, couldn't pair any
            // triangles, and emitted a pure-triangle mesh), the triangle
            // buffer already covers the full surface — return it directly
            // and skip the second FFI round-trip.
            //
            // The PARTIAL-recombine case (both `quad_indices` AND
            // `triangle_indices` non-empty) cannot reuse the triangle
            // buffer alone: the triangles only cover the area gmsh
            // couldn't pair; the quad-covered area would be lost. That
            // case must re-mesh with recombine=false to obtain a complete
            // pure-triangle tiling. Similarly, the fully-quad-but-low-skew
            // case has zero triangles to reuse and also re-meshes.
            if result.quad_indices.is_empty() && !result.triangle_indices.is_empty() {
                return Ok(Mesh2dReport {
                    mesh: Mesh2d::Triangle {
                        vertices,
                        indices: result.triangle_indices,
                    },
                    recombine_attempted: true,
                    // Records that recombine was attempted but yielded no
                    // quads — the caller's diagnostic code uses this to
                    // emit the "wedge fallback" vs "wedge native"
                    // distinction.
                    recombine_quality_ok: false,
                });
            }

            // Fall-back: second `mesh_plane_2d` round-trip with
            // `recombine=false` produces a pure-triangle mesh. Acceptable
            // cost — this path is the exception, and the 2D meshes
            // involved are small (cross-section sized).
            let fb = reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d(
                &boundary.outer,
                &boundary.holes,
                resolved_size,
                false,
                options.deterministic,
            )
            .map_err(map_geometry_error)?;
            let fb_vertices: Vec<f32> = fb.vertices_xy.iter().map(|&v| v as f32).collect();
            Ok(Mesh2dReport {
                mesh: Mesh2d::Triangle {
                    vertices: fb_vertices,
                    indices: fb.triangle_indices,
                },
                recombine_attempted: true,
                // Records that recombine was attempted but rejected — the
                // caller (task 2989's diagnostic code) uses this to emit
                // the "wedge fallback" vs "wedge native" distinction.
                recombine_quality_ok: false,
            })
        }
    }
}

/// Map a `GeometryError` from `mesh_plane_2d` to the orchestrator's
/// `Mesh2dError`. The stub arm of `mesh_plane_2d` returns a
/// `GeometryError::OperationFailed` containing
/// [`reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER`]; route that to
/// `GmshUnavailable` so callers can distinguish "no libgmsh in this build"
/// from "libgmsh failed at runtime". The substring check is anchored on the
/// shared `pub const` rather than a duplicated literal, so a stub-message
/// reword can only happen by editing the constant — which is itself
/// referenced here, surfacing the contract at compile time.
fn map_geometry_error(err: GeometryError) -> Mesh2dError {
    match &err {
        GeometryError::OperationFailed(msg)
            if msg.contains(reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER) =>
        {
            Mesh2dError::GmshUnavailable
        }
        _ => Mesh2dError::GmshFailed(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- (a) SweepElementTarget public surface ----
    #[test]
    fn sweep_element_target_variants_are_partial_eq_and_copy() {
        let hex: SweepElementTarget = SweepElementTarget::HexPreferred;
        let wedge: SweepElementTarget = SweepElementTarget::WedgeOnly;
        // Copy: re-use both bindings after a shadow copy.
        let _hex_copy: SweepElementTarget = hex;
        let _wedge_copy: SweepElementTarget = wedge;
        assert_ne!(hex, wedge);
        assert_eq!(hex, SweepElementTarget::HexPreferred);
        assert_eq!(wedge, SweepElementTarget::WedgeOnly);
    }

    // ---- (b) Mesh2d variants accept f32 vertices / u32 indices ----
    #[test]
    fn mesh2d_triangle_and_quad_construct_with_expected_types() {
        let _tri = Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.5, 1.0],
            indices: vec![0_u32, 1, 2],
        };
        let _quad = Mesh2d::Quad {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
            indices: vec![0_u32, 1, 2, 3],
        };
    }

    // ---- (c) Mesh2dReport struct shape ----
    #[test]
    fn mesh2d_report_struct_literal_constructs() {
        let report = Mesh2dReport {
            mesh: Mesh2d::Triangle {
                vertices: vec![0.0_f32; 6],
                indices: vec![0_u32, 1, 2],
            },
            recombine_attempted: false,
            recombine_quality_ok: true,
        };
        assert!(!report.recombine_attempted);
        assert!(report.recombine_quality_ok);
    }

    // ---- (d) ProfileBoundary accepts Vec<[f64;2]> ----
    #[test]
    fn profile_boundary_accepts_2d_points() {
        let pb = ProfileBoundary {
            outer: vec![[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            holes: vec![vec![
                [0.25_f64, 0.25],
                [0.75, 0.25],
                [0.75, 0.75],
                [0.25, 0.75],
            ]],
        };
        assert_eq!(pb.outer.len(), 4);
        assert_eq!(pb.holes.len(), 1);
    }

    // ---- (e) Mesh2dError variants ----
    #[test]
    fn mesh2d_error_has_required_variants() {
        // Each line constructs one variant — a missing variant or renamed
        // field would fail to compile.
        let _empty = Mesh2dError::EmptyBoundary;
        let _degen = Mesh2dError::DegenerateBoundary;
        let _unavail = Mesh2dError::GmshUnavailable;
        // GmshFailed wraps a GeometryError — construct the simplest variant.
        let _failed = Mesh2dError::GmshFailed(reify_ir::GeometryError::OperationFailed(
            "test".to_string(),
        ));
    }

    // ---- (f) Mesh2dOptions::default() ----
    #[test]
    fn mesh2d_options_default_matches_spec() {
        let opts = Mesh2dOptions::default();
        assert_eq!(opts.mesh_size, None);
        assert!(!opts.deterministic);
        assert_eq!(opts.recombine_skew_threshold, std::f64::consts::FRAC_PI_4);
    }

    // ---- step-3: compute_quad_skew ----
    //
    // Definition: per-corner unsigned deviation from \u{3c0}/2; the function
    // returns the maximum over the four corners.

    #[test]
    fn compute_quad_skew_unit_square_is_zero() {
        // Each corner is exactly \u{3c0}/2 — skew is 0.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let s = compute_quad_skew(&q);
        assert!(s.abs() < 1e-12, "unit-square skew = {s}, expected 0.0");
    }

    #[test]
    fn compute_quad_skew_collapsed_corner_is_large() {
        // Repeated vertex collapses one edge to zero length; the two
        // adjacent corners are degenerate. The function must return a
        // value at least \u{3c0}/4 to flag this as bad.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let s = compute_quad_skew(&q);
        assert!(
            s >= std::f64::consts::FRAC_PI_4,
            "collapsed-corner skew = {s}, expected >= \u{3c0}/4"
        );
    }

    #[test]
    fn compute_quad_skew_parallelogram_matches_geometry() {
        // Parallelogram with top side shifted by 0.5: two corners are
        // atan(2) above \u{3c0}/2 and two are below. The max deviation is
        // |\u{3c0}/2 - atan(2)| = atan(0.5) \u{2248} 0.4636 rad.
        let q = [[0.0_f64, 0.0], [1.0, 0.0], [1.5, 1.0], [0.5, 1.0]];
        let expected = (0.5_f64).atan();
        let s = compute_quad_skew(&q);
        assert!(
            (s - expected).abs() < 1e-9,
            "parallelogram skew = {s}, expected {expected}"
        );
    }

    #[test]
    fn compute_quad_skew_is_orientation_agnostic() {
        // CCW and CW orderings of the same shape must produce the same
        // skew score: skew is an unsigned geometric property.
        let ccw = [[0.0_f64, 0.0], [1.0, 0.0], [1.5, 1.0], [0.5, 1.0]];
        let cw = [[0.0_f64, 0.0], [0.5, 1.0], [1.5, 1.0], [1.0, 0.0]];
        let s_ccw = compute_quad_skew(&ccw);
        let s_cw = compute_quad_skew(&cw);
        assert!(
            (s_ccw - s_cw).abs() < 1e-12,
            "skew is sign-dependent: CCW={s_ccw} CW={s_cw}"
        );
    }

    // ---- step-5: recombine_quality_ok ----
    //
    // Wraps compute_quad_skew over a buffer of stride-4 quads,
    // short-circuiting on the first quad whose skew exceeds the threshold.

    #[test]
    fn recombine_quality_ok_two_unit_squares_passes() {
        // Two side-by-side unit squares sharing an edge: vertices index
        // 1 and 2 are shared. Both quads are perfect — skew = 0.
        let vertices: Vec<f32> = vec![
            0.0, 0.0, // 0
            1.0, 0.0, // 1
            1.0, 1.0, // 2
            0.0, 1.0, // 3
            2.0, 0.0, // 4
            2.0, 1.0, // 5
        ];
        let quad_indices: Vec<u32> = vec![0, 1, 2, 3, 1, 4, 5, 2];
        assert!(recombine_quality_ok(
            &vertices,
            &quad_indices,
            std::f64::consts::FRAC_PI_4
        ));
    }

    #[test]
    fn recombine_quality_ok_one_skewed_quad_fails() {
        // One quad with a corner at \u{3c0}/2 + \u{3c0}/3 = 5\u{3c0}/6, so
        // skew = \u{3c0}/3 > \u{3c0}/4. Build geometrically: a degenerate
        // kite where one corner subtends 30° (\u{3c0}/6) — exactly the
        // opposite mis-design where the deviation is \u{3c0}/2 - \u{3c0}/6
        // = \u{3c0}/3.
        //
        // Corner at the origin between e_prev=(1,0) and e_next=(cos30°,
        // sin30°) gives angle = 30° = \u{3c0}/6, deviation \u{3c0}/3.
        let cos30 = (30.0_f64).to_radians().cos();
        let sin30 = (30.0_f64).to_radians().sin();
        let vertices: Vec<f32> = vec![
            0.0,
            0.0, // 0 — sharp corner
            1.0,
            0.0, // 1
            (1.0 + cos30) as f32,
            sin30 as f32, // 2
            cos30 as f32,
            sin30 as f32, // 3
        ];
        let quad_indices: Vec<u32> = vec![0, 1, 2, 3];
        assert!(!recombine_quality_ok(
            &vertices,
            &quad_indices,
            std::f64::consts::FRAC_PI_4
        ));
    }

    #[test]
    fn recombine_quality_ok_empty_indices_is_vacuously_true() {
        // No quads to evaluate -> nothing to fail.
        let vertices: Vec<f32> = vec![0.0; 8];
        let quad_indices: Vec<u32> = vec![];
        assert!(recombine_quality_ok(
            &vertices,
            &quad_indices,
            std::f64::consts::FRAC_PI_4
        ));
    }

    #[test]
    fn recombine_quality_ok_misaligned_stride_returns_false() {
        // quad_indices.len() % 4 != 0 — defensive caller-bug detection.
        let vertices: Vec<f32> = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let quad_indices: Vec<u32> = vec![0, 1, 2]; // missing one
        assert!(!recombine_quality_ok(
            &vertices,
            &quad_indices,
            std::f64::consts::FRAC_PI_4
        ));
    }

    // ---- step-7: auto_mesh_size_from_boundary ----

    #[test]
    fn auto_mesh_size_unit_square_returns_one() {
        // Smallest segment = 1.0 (every side), multiplier = 1.0 -> 1.0.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            holes: vec![],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert!((h - 1.0).abs() < 1e-12, "h = {h}, expected 1.0");
    }

    #[test]
    fn auto_mesh_size_thin_rectangle_picks_smallest_side() {
        // 10 x 0.1 rectangle: smallest segment = 0.1, multiplier = 1.0.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 0.1], [0.0, 0.1]],
            holes: vec![],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert!((h - 0.1).abs() < 1e-12, "h = {h}, expected 0.1");
    }

    #[test]
    fn auto_mesh_size_hole_can_drive_size_down() {
        // Outer ring has side 10; hole has side 0.5. Smallest overall = 0.5.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            holes: vec![vec![[4.0, 4.0], [4.5, 4.0], [4.5, 4.5], [4.0, 4.5]]],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert!((h - 0.5).abs() < 1e-12, "h = {h}, expected 0.5 (from hole)");
    }

    #[test]
    fn auto_mesh_size_empty_outer_returns_zero() {
        // Matches auto_mesh_size_from_features' "unavailable" convention.
        let pb = ProfileBoundary {
            outer: vec![],
            holes: vec![],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert_eq!(h, 0.0);
    }

    // ---- step-9: mesh_swept_profile_2d input validation ----
    //
    // Pinned BEFORE any Gmsh call so these run in both stub and real
    // builds. The actual mesh-producing arms are tested under
    // cfg(has_gmsh) in steps 19+.

    #[test]
    fn mesh_swept_profile_2d_rejects_empty_outer() {
        let pb = ProfileBoundary {
            outer: vec![],
            holes: vec![],
        };
        let r = mesh_swept_profile_2d(
            &pb,
            SweepElementTarget::WedgeOnly,
            &Mesh2dOptions::default(),
        );
        assert!(matches!(r, Err(Mesh2dError::EmptyBoundary)));
    }

    #[test]
    fn mesh_swept_profile_2d_rejects_outer_under_three_points() {
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [1.0, 0.0]],
            holes: vec![],
        };
        let r = mesh_swept_profile_2d(
            &pb,
            SweepElementTarget::WedgeOnly,
            &Mesh2dOptions::default(),
        );
        assert!(
            matches!(r, Err(Mesh2dError::DegenerateBoundary)),
            "expected DegenerateBoundary, got {r:?}"
        );
    }

    #[test]
    fn mesh_swept_profile_2d_rejects_hole_under_three_points() {
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            holes: vec![vec![[4.0, 4.0], [5.0, 5.0]]],
        };
        let r = mesh_swept_profile_2d(
            &pb,
            SweepElementTarget::WedgeOnly,
            &Mesh2dOptions::default(),
        );
        assert!(
            matches!(r, Err(Mesh2dError::DegenerateBoundary)),
            "expected DegenerateBoundary, got {r:?}"
        );
    }

    #[test]
    fn mesh_swept_profile_2d_rejects_collinear_outer() {
        // Signed area = 0 — three colinear points on the x-axis.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
            holes: vec![],
        };
        let r = mesh_swept_profile_2d(
            &pb,
            SweepElementTarget::WedgeOnly,
            &Mesh2dOptions::default(),
        );
        assert!(
            matches!(r, Err(Mesh2dError::DegenerateBoundary)),
            "expected DegenerateBoundary, got {r:?}"
        );
    }

    #[test]
    fn mesh_swept_profile_2d_rejects_collinear_hole() {
        // Outer ring is fine; the hole ring is collinear (three points on
        // the same horizontal line). Without the hole-area check this slips
        // through to gmsh as an opaque failure (or a silent slit). The
        // tolerance mirrors the outer-ring check.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            holes: vec![vec![[3.0, 5.0], [4.0, 5.0], [5.0, 5.0]]],
        };
        let r = mesh_swept_profile_2d(
            &pb,
            SweepElementTarget::WedgeOnly,
            &Mesh2dOptions::default(),
        );
        assert!(
            matches!(r, Err(Mesh2dError::DegenerateBoundary)),
            "expected DegenerateBoundary, got {r:?}"
        );
    }

    // ---- amend: auto_mesh_size_from_boundary all-duplicate ring ----
    //
    // Pins the chosen semantics for the all-coincident-points case: the
    // function returns 0.0 (the "unavailable" sentinel), letting the
    // orchestrator collapse to `None` and gmsh's default mesh-size policy
    // apply rather than producing a min_len of 0.0 that would yield a
    // degenerate mesh size.
    #[test]
    fn auto_mesh_size_all_duplicate_ring_returns_zero() {
        let pb = ProfileBoundary {
            outer: vec![[1.0, 1.0], [1.0, 1.0], [1.0, 1.0]],
            holes: vec![],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert_eq!(
            h, 0.0,
            "all-duplicate ring must yield the unavailable sentinel 0.0"
        );
    }

    // Mixed-duplicate ring: a couple of adjacent duplicates but at least
    // one non-zero segment. The function must skip the zero-length entries
    // and report the smallest *real* segment, not collapse to 0.0.
    #[test]
    fn auto_mesh_size_mixed_duplicate_ring_skips_zero_segments() {
        // Triangle with a duplicate at index 1: real segments are
        // (0,0)->(1,0) length 1, (1,0)->(1,0) length 0 (skipped),
        // (1,0)->(0,1) length sqrt(2), wrap (0,1)->(0,0) length 1.
        // Smallest non-zero segment = 1.0.
        let pb = ProfileBoundary {
            outer: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            holes: vec![],
        };
        let h = auto_mesh_size_from_boundary(&pb, 1.0);
        assert!(
            (h - 1.0).abs() < 1e-12,
            "mixed-duplicate ring h = {h}, expected 1.0",
        );
    }

    // ---- amend: map_geometry_error stub-marker contract ----
    //
    // Pins the end-to-end contract between mesh_profile_2d's stub message
    // and the orchestrator's GmshUnavailable routing without needing a
    // real stub build. Any reword of `STUB_UNAVAILABLE_MARKER` that fails
    // to keep `map_geometry_error` in sync will be caught here.
    #[test]
    fn map_geometry_error_routes_stub_marker_to_unavailable() {
        let stub_msg = format!(
            "mesh_plane_2d: {} in this build (libgmsh not detected at build time)",
            reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER,
        );
        let err = GeometryError::OperationFailed(stub_msg);
        let mapped = map_geometry_error(err);
        assert!(
            matches!(mapped, Mesh2dError::GmshUnavailable),
            "stub marker message must map to GmshUnavailable, got {mapped:?}",
        );
    }

    #[test]
    fn map_geometry_error_routes_runtime_failure_to_gmsh_failed() {
        // A non-stub OperationFailed message must NOT match the marker —
        // routes to GmshFailed instead, so callers can distinguish runtime
        // failures from missing-libgmsh configuration errors.
        let err = GeometryError::OperationFailed(
            "mesh_plane_2d: get_nodes_all stride mismatch".to_string(),
        );
        let mapped = map_geometry_error(err);
        assert!(
            matches!(mapped, Mesh2dError::GmshFailed(_)),
            "runtime failure must map to GmshFailed, got {mapped:?}",
        );
    }
}
