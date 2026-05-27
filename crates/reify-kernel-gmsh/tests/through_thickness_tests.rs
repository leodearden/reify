//! Tests for the through-thickness element-count diagnostic.
//!
//! Per the v0.3 FEA PRD, a thin region with only one tet through its
//! thickness produces an FEA mesh that's almost guaranteed to under-predict
//! deflection by an order of magnitude. The diagnostic scans the produced
//! volume mesh and emits a warning whenever any geometric thickness has
//! fewer than `min_elements_through_thickness` (default 2) tets through it.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use reify_kernel_gmsh::through_thickness::{
    ThroughThicknessConfig, ThroughThicknessWarning, through_thickness_check,
};
use reify_ir::{ElementOrderTag, Mesh, VolumeMesh};

/// Surface mesh of an axis-aligned 10×10×0.5 slab — six faces, two
/// triangles per face = 12 triangles. The thickness direction is Z.
fn slab_surface_mesh() -> Mesh {
    let v = vec![
        // Bottom face Z=0 (4 verts: 0..3)
        0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 0.0,
        // Top face Z=0.5 (4 verts: 4..7)
        0.0, 0.0, 0.5, 10.0, 0.0, 0.5, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
    ];
    let i = vec![
        // Bottom (CCW from below)
        0, 2, 1, 0, 3, 2, // Top
        4, 5, 6, 4, 6, 7, // Side faces (we don't care about exact winding for this test)
        0, 1, 5, 0, 5, 4, //
        1, 2, 6, 1, 6, 5, //
        2, 3, 7, 2, 7, 6, //
        3, 0, 4, 3, 4, 7, //
    ];
    Mesh {
        vertices: v,
        indices: i,
        normals: None,
    }
}

/// Slab with only ONE tet spanning the thickness direction triggers a
/// warning whose message names "fewer than 2 elements" and includes a
/// numeric mesh_size suggestion.
#[test]
fn single_layer_tet_through_thin_region_emits_warning() {
    let surface = slab_surface_mesh();
    // 8 vertices of the slab volume (matching the surface vertex layout) +
    // a SINGLE tet that "spans" Z=0 → Z=0.5. We don't need a fully meshed
    // slab — the diagnostic counts the layers of unique tet centroids along
    // the thinnest axis, and one tet means one layer.
    let volume = VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            10.0, 0.0, 0.0, // 1
            10.0, 10.0, 0.5, // 2
            0.0, 10.0, 0.5, // 3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        !warnings.is_empty(),
        "single-layer thin region must emit at least one warning"
    );
    let w = &warnings[0];
    assert!(
        w.message.contains("fewer than 2"),
        "warning message must call out 'fewer than 2 elements'; got: {}",
        w.message
    );
    // Heuristic check: the message should mention 'mesh_size' as part of the
    // remediation guidance.
    assert!(
        w.message.to_lowercase().contains("mesh_size"),
        "warning should suggest a smaller mesh_size; got: {}",
        w.message
    );
}

/// A well-resolved slab with several tet layers through the thickness emits
/// no warnings.
#[test]
fn well_resolved_thickness_emits_no_warning() {
    let surface = slab_surface_mesh();
    // Four tets stacked along Z: centroids at Z ≈ 0.0625, 0.1875, 0.3125,
    // 0.4375 (four distinct layers in the thinnest direction).
    let volume = VolumeMesh {
        vertices: vec![
            // Tet 0: Z 0.0..0.125
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.125, 0.0, 10.0, 0.125,
            // Tet 1: Z 0.125..0.25
            0.0, 0.0, 0.125, 10.0, 0.0, 0.125, 10.0, 10.0, 0.25, 0.0, 10.0, 0.25,
            // Tet 2: Z 0.25..0.375
            0.0, 0.0, 0.25, 10.0, 0.0, 0.25, 10.0, 10.0, 0.375, 0.0, 10.0, 0.375,
            // Tet 3: Z 0.375..0.5
            0.0, 0.0, 0.375, 10.0, 0.0, 0.375, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
        ],
        tet_indices: vec![
            0, 1, 2, 3, //
            4, 5, 6, 7, //
            8, 9, 10, 11, //
            12, 13, 14, 15, //
        ],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        warnings.is_empty(),
        "4-tet-thick slab should produce no warnings; got {} warnings: {:?}",
        warnings.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>(),
    );
}

/// P2 (10-node) tets must be handled correctly: the centroid math uses
/// only the first 4 corner indices, and the per-tet stride is 10 (not 4).
/// A regression that used stride 4 for both element orders, or summed all
/// 10 indices into the centroid, would warp the layer count for P2 inputs.
///
/// This test builds a single 10-node tet whose 4 corners span the slab's
/// thickness and appends 6 dummy edge-midpoint indices. The centroid must
/// be computed from the 4 corners only — the midpoint coordinates are
/// chosen so that including them would shift the centroid noticeably (and
/// thus could plausibly change the bin-walking result on a single-tet
/// input). With one tet only, the layer count must still be 1, triggering
/// the warning.
#[test]
fn p2_element_order_uses_corners_only_for_centroid() {
    let surface = slab_surface_mesh();
    // 4 corners (tet[0..4]) span Z=0 → Z=0.5; 6 "edge midpoints" with
    // arbitrary coordinates that, if (incorrectly) included in the centroid
    // sum, would skew the result. The test relies on stride=10 being
    // honoured (only one tet of 10 indices is consumed) and on tet[..4]
    // being the slice the centroid loop sums over.
    let volume = VolumeMesh {
        vertices: vec![
            // Corner 0..3 (the only ones the centroid should sum)
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
            // Edge midpoints 4..9 — deliberately offset along Z so a bug
            // that included them in the centroid would visibly shift it.
            5.0, 0.0, 100.0, 10.0, 5.0, -100.0, 5.0, 10.0, 100.0, 0.0, 5.0, -100.0, 5.0, 5.0, 100.0,
            7.5, 7.5, -100.0,
        ],
        // Single P2 tet: 4 corner + 6 midpoint indices.
        tet_indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        element_order: ElementOrderTag::P2,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    // One tet → one layer → must warn.
    assert!(
        !warnings.is_empty(),
        "single P2 tet must emit a warning (only one layer through thickness); \
         a regression in stride or in the corners-only centroid math could \
         spuriously produce >1 layers and silence the warning"
    );
    // Layer count must be exactly 1 — proves the stride respected the 10-node
    // layout (only one tet was consumed) and the centroid loop didn't fold
    // the midpoint Z=±100 values into the position.
    assert_eq!(
        warnings[0].element_count, 1,
        "P2 single-tet layer count must be 1; got {} — indicates either \
         stride 4 was used (consuming 2.5 tets from 10 indices) or the \
         centroid summed all 10 nodes",
        warnings[0].element_count
    );
}

/// Heuristic-noise characterisation: when tet extents along the thinnest
/// axis vary (perfectly normal in production meshes), the algorithm's
/// half-of-AVERAGE-extent threshold for "distinct layer" may produce a
/// false low-count warning if a small extent collapses into its neighbour's
/// bin. This test pins current behaviour for a 4-tet stack with non-uniform
/// extents. v0.4+ per-region clustering may refine this.
#[test]
fn non_uniform_tet_extents_along_thickness_does_not_collapse_distinct_layers() {
    let surface = slab_surface_mesh();
    // Four tets stacked along Z with non-uniform thicknesses:
    //   Tet 0: Z 0.00..0.05  (thin)
    //   Tet 1: Z 0.05..0.20  (medium)
    //   Tet 2: Z 0.20..0.35  (medium)
    //   Tet 3: Z 0.35..0.50  (medium)
    // Centroids: 0.025, 0.125, 0.275, 0.425.
    // Average extent along Z = (0.05 + 0.15 + 0.15 + 0.15) / 4 = 0.125;
    // half-bin = 0.0625. Gaps between consecutive centroids: 0.10, 0.15,
    // 0.15 — all > 0.0625, so all four layers are detected. This is the
    // currently-acceptable noise floor: gaps must exceed half the AVERAGE
    // extent. Documenting via a test rather than a comment so future
    // tuning of the bin-width formula is forced to consider this case.
    let volume = VolumeMesh {
        vertices: vec![
            // Tet 0: Z 0.0..0.05
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.05, 0.0, 10.0, 0.05,
            // Tet 1: Z 0.05..0.20
            0.0, 0.0, 0.05, 10.0, 0.0, 0.05, 10.0, 10.0, 0.20, 0.0, 10.0, 0.20,
            // Tet 2: Z 0.20..0.35
            0.0, 0.0, 0.20, 10.0, 0.0, 0.20, 10.0, 10.0, 0.35, 0.0, 10.0, 0.35,
            // Tet 3: Z 0.35..0.50
            0.0, 0.0, 0.35, 10.0, 0.0, 0.35, 10.0, 10.0, 0.50, 0.0, 10.0, 0.50,
        ],
        tet_indices: vec![
            0, 1, 2, 3, //
            4, 5, 6, 7, //
            8, 9, 10, 11, //
            12, 13, 14, 15, //
        ],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };
    let warnings = through_thickness_check(&volume, &surface, cfg);
    // 4 detected layers ≥ 2-element threshold → no warning. If the heuristic
    // is later tightened to under-count due to extent variance, this test will
    // fail and force the change to be considered explicitly.
    assert!(
        warnings.is_empty(),
        "non-uniform 4-tet stack should still detect 4 distinct layers; got \
         {} warning(s) — indicates the half-of-average bin width collapsed \
         distinct layers into a single bin",
        warnings.len(),
    );
}

/// The warning struct exposes a `region_index` field so future v0.4+
/// per-face/per-region analysis can attach a face/region identifier.
/// For v0.3 the whole body is a single region (`region_index = 0`).
#[test]
fn warning_includes_face_or_region_identifier() {
    let surface = slab_surface_mesh();
    let volume = VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig::default();
    let warnings: Vec<ThroughThicknessWarning> = through_thickness_check(&volume, &surface, cfg);
    assert!(
        !warnings.is_empty(),
        "expected at least one warning to inspect its region_index"
    );
    // Confirm the region_index field is reachable as a struct member; in v0.3
    // the placeholder is 0 (single-region), but the field exists so v0.4+
    // refinement can attach face/region IDs without changing the struct shape.
    let _: usize = warnings[0].region_index;
    // Defence-in-depth: thickness and element_count fields are also reachable
    // — these are part of the public diagnostic surface.
    let _: f64 = warnings[0].thickness;
    let _: u32 = warnings[0].element_count;
}

/// Pins the LHS branch of the early-return OR-guard at the top of
/// `through_thickness_check` (fires when either `surface.vertices` or
/// `volume.tet_indices` is empty):
/// `if surface.vertices.is_empty() || volume.tet_indices.is_empty() { return Vec::new(); }`
///
/// This test exercises the LHS short-circuit specifically: `surface.vertices` is
/// empty, so the `||` returns immediately without evaluating `tet_indices.is_empty()`.
/// The volume mesh has NON-empty `tet_indices` to force this distinction — a test
/// with BOTH inputs empty would only exercise the LHS and leave the RHS branch
/// unpinned (Rust's `||` short-circuits on the first true operand).
///
/// Partner test `empty_tet_indices_returns_empty_vec` covers the RHS branch.
#[test]
fn empty_surface_vertices_returns_empty_vec() {
    let surface = Mesh {
        vertices: vec![], // empty surface — triggers LHS of the OR
        indices: vec![],
        normals: None,
    };
    // NON-empty tet_indices: forces the LHS branch to be the one that fires.
    let volume = VolumeMesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig::default();
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        warnings.is_empty(),
        "empty surface vertices must yield empty Vec (LHS branch of OR guard); \
         got {} warning(s)",
        warnings.len()
    );
}

/// Pins the RHS branch of the early-return OR-guard at the top of
/// `through_thickness_check` (fires when either `surface.vertices` or
/// `volume.tet_indices` is empty):
/// `if surface.vertices.is_empty() || volume.tet_indices.is_empty() { return Vec::new(); }`
///
/// This test exercises the RHS short-circuit specifically: `surface.vertices` is
/// NON-empty (using the `slab_surface_mesh()` helper), so the `||` evaluates the
/// RHS and triggers the early-return on `tet_indices.is_empty()`. A single test
/// with both inputs empty would only exercise the LHS (Rust's `||` short-circuits
/// on the first true operand) and would leave this branch unpinned.
///
/// Note: a secondary `n_tets == 0` guard inside `through_thickness_check` after
/// the BBox walk would also catch empty `tet_indices`. Pinning the documented
/// `tet_indices.is_empty()` contract directly catches a regression that drops
/// EITHER guard — not just the secondary one.
///
/// Partner test `empty_surface_vertices_returns_empty_vec` covers the LHS branch.
#[test]
fn empty_tet_indices_returns_empty_vec() {
    // NON-empty surface: forces LHS of the OR to be false, so the RHS is evaluated.
    let surface = slab_surface_mesh();
    let volume = VolumeMesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        tet_indices: vec![], // empty tet_indices — triggers RHS of the OR
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig::default();
    let warnings = through_thickness_check(&volume, &surface, cfg);
    assert!(
        warnings.is_empty(),
        "empty tet_indices must yield empty Vec (RHS branch of OR guard); \
         got {} warning(s)",
        warnings.len()
    );
}

// ---------------------------------------------------------------------------
// Non-finite centroid tests: NaN, +Inf, −Inf.
//
// All three share the same assertion contract — non-finite centroid → empty
// Vec + exactly one WARN at the target — so the scaffolding is factored into
// a single private helper parameterised by the vertex coordinate value.
// ---------------------------------------------------------------------------

/// Shared scaffold for the three non-finite-centroid guard tests.
///
/// Primes the tracing callsite cache, builds a single-P1-tet `VolumeMesh`
/// whose first vertex has `coord` for all three coordinates (the remaining
/// three vertices are the finite slab corners), runs `through_thickness_check`
/// under a WARN-counting subscriber, and asserts:
///
/// - (a) the returned `Vec` is empty — a non-finite centroid signals upstream
///   pathology, not an under-resolved region,
/// - (b) exactly one WARN event is emitted at the
///   `reify_kernel_gmsh::through_thickness` target.
fn assert_non_finite_first_vertex_returns_empty(coord: f32) {
    // Prime the callsite cache so per-test with_default subscribers see events
    // even if a prior test thread hit the callsite with no subscriber active.
    reify_test_support::prime_tracing_callsite_cache();

    let surface = slab_surface_mesh();

    // Single P1 tet whose first vertex uses `coord` for all three axes.
    // Vertex 0 is the pathological one; vertices 1..3 are finite slab corners.
    let volume = VolumeMesh {
        vertices: vec![
            coord, coord, coord, 10.0, 0.0, 0.0, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    let cfg = ThroughThicknessConfig {
        min_elements_through_thickness: 2,
    };

    let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_kernel_gmsh::through_thickness")
        .build();
    let warn_arc = Arc::clone(&counters[&tracing::Level::WARN]);

    let warnings = tracing::subscriber::with_default(subscriber, || {
        through_thickness_check(&volume, &surface, cfg)
    });

    // (a) Must return empty Vec — non-finite centroid signals upstream pathology.
    assert!(
        warnings.is_empty(),
        "non-finite centroid (coord={coord}) must produce empty Vec, not a \
         spurious layer-count warning; got {} warning(s): {:?}",
        warnings.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    );

    // (b) No panic (implicit — reaching this point means the function returned).

    // (c) Exactly one WARN event must be emitted at the named target.
    let warn_count = warn_arc.load(Ordering::Acquire);
    assert_eq!(
        warn_count, 1,
        "expected exactly 1 WARN event at reify_kernel_gmsh::through_thickness \
         (coord={coord}); got {warn_count}"
    );
}

/// A volume mesh whose first vertex has NaN coordinates must not produce a
/// spurious layer count — NaN poisons `partial_cmp` (treated as Equal against
/// every value), silently scrambling the sort. Instead the function must
/// early-return `Vec::new()` and emit exactly one WARN at the
/// `reify_kernel_gmsh::through_thickness` target.
#[test]
fn nan_centroid_returns_empty_and_emits_warn() {
    assert_non_finite_first_vertex_returns_empty(f32::NAN);
}

/// A volume mesh whose first vertex has +Inf coordinates must not produce a
/// spurious layer count — Inf poisons `bin_width` (avg_tet_extent → Inf →
/// half_bin → Inf → `(w[1] - w[0]).abs() > Inf` is always false →
/// layer_count collapses to 1 regardless of geometry). The function must
/// early-return `Vec::new()` and emit exactly one WARN at the
/// `reify_kernel_gmsh::through_thickness` target.
#[test]
fn inf_centroid_returns_empty_and_emits_warn() {
    assert_non_finite_first_vertex_returns_empty(f32::INFINITY);
}

/// A volume mesh whose first vertex has −Inf coordinates must not produce a
/// spurious layer count. −Inf takes a slightly different arithmetic path than
/// +Inf (avoids any +Inf + −Inf = NaN interaction that could mask the failure
/// mode), so it is tested separately to close the `!is_finite()` predicate's
/// coverage. The expected contract is identical: early-return `Vec::new()` and
/// emit exactly one WARN at the `reify_kernel_gmsh::through_thickness` target.
#[test]
fn neg_inf_centroid_returns_empty_and_emits_warn() {
    assert_non_finite_first_vertex_returns_empty(f32::NEG_INFINITY);
}
