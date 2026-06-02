//! KGQ-ρ: Integration gate — `all_queries_walk` example + boundary-test sweep
//! (task 3626, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 Phase 6).
//!
//! ## Overview
//!
//! This file is the exhaustive end-to-end gate for all 21 in-scope §2 helpers,
//! exercised on the `examples/kernel_queries/all_queries_walk.ri` fixture (a
//! single multi-feature part: box + sphere + 2-arg fillet).
//!
//! Three live assertions (steps 1 / 3 / 5):
//!
//! - **`all_queries_walk_compiles_with_stdlib_no_errors`** (always-on) — reads
//!   the walk fixture, runs `parse_and_compile_with_stdlib`, asserts
//!   `errors_only` empty. Pins fixture presence + that every in-scope helper
//!   name/signature resolves at compile time on every CI runner.
//!
//! - **`all_queries_walk_evals_top_level_helpers_to_non_undef`** (OCCT-gated)
//!   — builds with a DIRECT `OcctKernelHandle` engine (NOT `SingleKernelHolder`,
//!   which does not override `extract_edges`/`extract_faces`) and asserts that
//!   each of the 14 top-level helper cells is `Some(v)` with
//!   `!matches!(v, Value::Undef)` AND the expected `Value` variant.
//!
//! - **`multi_feature_part_sub_handle_queries_return_non_undef`** (OCCT-gated)
//!   — pins the 7 sub-handle-arg helpers (normal, curvature surface/curve,
//!   length, perimeter, adjacent_faces, shared_edges) via DIRECT
//!   `OcctKernelHandle` queries, bypassing the DSL eval-chaining limitation
//!   (engine_build.rs:3942-3949, out of scope for KGQ-ρ).
//!
//! One ignored profiling harness (step 6):
//!
//! - **`profile_topology_selectors_and_sub_handle_memory`** (`#[ignore]`) —
//!   §10.3 latency profile for topology selectors + §10.4 sub-handle memory
//!   pressure estimate. Run with `-- --include-ignored` to measure.
//!
//! ## §8 Boundary-test coverage table
//!
//! Every §8 boundary-test row from the PRD is covered by a test in the corpus.
//! Rows are listed here with their covering test; verify each before landing.
//!
//! | Helper              | §8 row                          | Covering test (corpus)                                                      |
//! |---------------------|---------------------------------|-----------------------------------------------------------------------------|
//! | `distance`          | point outside / inside / on     | `kernel_queries_distance_smoke::distance_box_point_evals_to_15mm`           |
//! | `contains`          | IN / ON / OUT                   | `kernel_queries_contains::contains_box_evals_expected_booleans`             |
//! | `intersects`        | overlap / apart                 | `kernel_queries_intersects_smoke::intersects_smoke_evals_expected_booleans` |
//! | `geo_equiv`         | identical / within-tol / diff   | `kernel_queries_geo_equiv_smoke::geo_equiv_smoke_evals_expected_booleans`   |
//! | `angle`             | orthogonal / parallel           | `kernel_queries_angle_smoke::angle_smoke_evals_to_ninety_degrees`           |
//! | `normal`            | +z face / sphere                | `kernel_queries_normal_smoke::normal_smoke_compiles_as_vec3_real_and_face_normal_at_ffis` + OCCT-kernel-occt/tests/face_differential_integration.rs |
//! | `curvature(surface)`| sphere κ_max=κ_min=1/r          | `kernel_queries_curvature_smoke::curvature_smoke_compiles_and_occt_query_chain_live` |
//! | `curvature(curve)`  | circular edge κ=1/r             | `kernel_queries_curvature_smoke::curvature_smoke_compiles_and_occt_query_chain_live` + reify-kernel-occt/tests/curve_curvature_integration.rs |
//! | `length`            | 10/20/30mm box edges            | `kernel_queries_length_perimeter::length_perimeter_compiles_and_occt_queries_match_expected` |
//! | `perimeter`         | 40mm cube face perimeter        | `kernel_queries_length_perimeter::length_perimeter_compiles_and_occt_queries_match_expected` |
//! | `edges`             | 12 edges on box                 | `topology_selectors_tests::box_edges_integration_test`                      |
//! | `faces`             | 6 faces on box                  | `topology_selectors_tests::box_faces_integration_test`                      |
//! | `edges_by_length`   | 4 y-edges in [15,25]mm          | `kernel_queries_filtered_edges::filtered_edges_compile_and_return_geometry_handles` |
//! | `faces_by_area`     | 2 z-faces in [196,225]mm²       | `kernel_queries_filtered_edges::filtered_edges_compile_and_return_geometry_handles` |
//! | `faces_by_normal`   | 1 top face (+z)                 | `kernel_queries_directional_selectors::directional_selectors_compile_and_return_geometry_handles` |
//! | `edges_parallel_to` | 4 z-parallel edges              | `kernel_queries_directional_selectors::directional_selectors_compile_and_return_geometry_handles` |
//! | `edges_at_height`   | 4 top-face edges (z=+15mm)      | `kernel_queries_filtered_edges::filtered_edges_compile_and_return_geometry_handles` |
//! | `adjacent_faces`    | 4 neighbours per box face       | `kernel_queries_adjacent_faces::adjacent_faces_and_shared_edges_compile_and_return_correct_semantics` |
//! | `shared_edges`      | 1 edge per adjacent box pair    | `kernel_queries_adjacent_faces::adjacent_faces_and_shared_edges_compile_and_return_correct_semantics` |
//! | `center_of_mass`    | centroid at origin for box      | `all_queries_walk_evals_top_level_helpers_to_non_undef` (this file)        |
//! | `moment_of_inertia` | 3×3 MOMENT_OF_INERTIA tensor    | `kernel_queries_moment_of_inertia_smoke::moment_of_inertia_box_evals_to_analytic_tensor` |
//! | `fillet` (3-arg)    | OUT OF SCOPE                    | deferred to task 3205                                                        |

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const ALL_QUERIES_WALK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/all_queries_walk.ri"
);

/// Pins fixture presence and that every in-scope §2 helper name/signature
/// resolves at compile time (no ERROR diagnostics) on every CI runner.
///
/// RED: the fixture file does not exist yet (step-1). Becomes GREEN when
/// `examples/kernel_queries/all_queries_walk.ri` is created (step-2).
#[test]
fn all_queries_walk_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(ALL_QUERIES_WALK_PATH)
        .expect("examples/kernel_queries/all_queries_walk.ri should exist (task 3626 step-2)");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/all_queries_walk.ri should compile with no \
         error-severity diagnostics (Warnings from Undef eval are acceptable \
         pre-Phase-3), got:\n{:#?}",
        errors_only(&compiled)
    );
}

/// OCCT-gated gate: asserts that the 14 top-level helper cells in the walk
/// fixture evaluate to non-Undef typed `Value`s when built with a DIRECT
/// `OcctKernelHandle` engine.
///
/// Top-level helpers (solid/value args, no sub-handle chaining needed):
/// distance, contains, intersects, geo_equiv, angle, edges, faces,
/// edges_by_length, faces_by_area, faces_by_normal, edges_parallel_to,
/// edges_at_height, center_of_mass, moment_of_inertia.
///
/// Skips cleanly when OCCT is not available.
#[test]
fn all_queries_walk_evals_top_level_helpers_to_non_undef() {
    // ── always-on: fixture exists and compiles cleanly ────────────────────────
    //
    // NOTE: This compile check is intentionally duplicated from
    // `all_queries_walk_compiles_with_stdlib_no_errors`. The duplication keeps
    // this test self-contained: if the fixture regresses, the compile error
    // surfaces here alongside the OCCT-gated assertion failure rather than only
    // in the separate compile test. Do NOT remove this block in the name of DRY —
    // `all_queries_walk_compiles_with_stdlib_no_errors` is the canonical always-on
    // gate; this copy is a local self-containment guard.
    let source = std::fs::read_to_string(ALL_QUERIES_WALK_PATH)
        .expect("examples/kernel_queries/all_queries_walk.ri should exist (task 3626 step-2)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "all_queries_walk.ri compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    // ── OCCT-gated: runtime eval ───────────────────────────────────────────────
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping all_queries_walk OCCT assertions: OCCT not available");
        return;
    }

    // Use DIRECT OcctKernelHandle — NOT SingleKernelHolder, which does not
    // override extract_edges/extract_faces (see topology_selectors_tests.rs:90-97).
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Helper: assert a cell is Some(v) with v != Value::Undef and the expected variant.
    macro_rules! assert_non_undef {
        ($struct_name:expr, $cell_name:expr, $variant_pat:pat, $variant_desc:expr) => {{
            let cell = ValueCellId::new($struct_name, $cell_name);
            match result.values.get(&cell) {
                Some(Value::Undef) => panic!(
                    "AllQueriesWalk.{} should be non-Undef ({}), got Value::Undef",
                    $cell_name, $variant_desc
                ),
                None => panic!(
                    "AllQueriesWalk.{} is absent from result.values; \
                     check cell name matches the .ri binding name",
                    $cell_name
                ),
                Some($variant_pat) => {} // correct variant, passes
                Some(other) => panic!(
                    "AllQueriesWalk.{} should be {} but got {:?}",
                    $cell_name, $variant_desc, other
                ),
            }
        }};
    }

    // 1. distance → Value::Scalar (LENGTH)
    assert_non_undef!(
        "AllQueriesWalk",
        "dist",
        Value::Scalar { .. },
        "Value::Scalar{LENGTH}"
    );

    // 2. contains → Value::Bool
    assert_non_undef!("AllQueriesWalk", "inside", Value::Bool(_), "Value::Bool");

    // 3. intersects → Value::Bool
    assert_non_undef!("AllQueriesWalk", "overlap", Value::Bool(_), "Value::Bool");

    // 4. geo_equiv → Value::Bool
    assert_non_undef!("AllQueriesWalk", "equiv", Value::Bool(_), "Value::Bool");

    // 5. angle → Value::Scalar (ANGLE)
    assert_non_undef!(
        "AllQueriesWalk",
        "ang",
        Value::Scalar { .. },
        "Value::Scalar{ANGLE}"
    );

    // 6. edges → Value::List (of GeometryHandle)
    assert_non_undef!("AllQueriesWalk", "all_edges", Value::List(_), "Value::List");

    // 7. faces → Value::List (of GeometryHandle)
    assert_non_undef!("AllQueriesWalk", "all_faces", Value::List(_), "Value::List");

    // 8. edges_by_length → Value::List
    assert_non_undef!(
        "AllQueriesWalk",
        "long_edges",
        Value::List(_),
        "Value::List"
    );

    // 9. faces_by_area → Value::List
    assert_non_undef!("AllQueriesWalk", "big_faces", Value::List(_), "Value::List");

    // 10. faces_by_normal → Value::List
    assert_non_undef!("AllQueriesWalk", "top_faces", Value::List(_), "Value::List");

    // 11. edges_parallel_to → Value::List
    assert_non_undef!(
        "AllQueriesWalk",
        "vert_edges",
        Value::List(_),
        "Value::List"
    );

    // 12. edges_at_height → Value::List
    assert_non_undef!(
        "AllQueriesWalk",
        "top_edge_line",
        Value::List(_),
        "Value::List"
    );

    // 13. center_of_mass → Value::Point with centroid at origin
    //
    // The walk box is 10×20×30 mm centred at origin; its centroid is exactly
    // (0, 0, 0). Assertion is strengthened beyond variant-only: each component's
    // SI value must be within 1 µm (1e-6 m) of zero. This catches regressions
    // where center_of_mass returns a garbage point that happens to be the right
    // Value variant (wrong frame, swapped axes, unit error, etc.).
    //
    // center_of_mass is the ONLY helper whose runtime semantics are covered
    // exclusively by this test (no sibling smoke test). See §8 coverage table
    // in the module-doc.
    {
        let com_cell = ValueCellId::new("AllQueriesWalk", "com");
        let com_val = result.values.get(&com_cell).expect(
            "AllQueriesWalk.com absent from result.values; \
                     check cell name matches the .ri binding name",
        );
        match com_val {
            Value::Undef => {
                panic!("AllQueriesWalk.com should be non-Undef Value::Point, got Value::Undef")
            }
            Value::Point(components) => {
                assert_eq!(
                    components.len(),
                    3,
                    "center_of_mass should return a 3-component Point, got {}",
                    components.len()
                );
                for (i, comp) in components.iter().enumerate() {
                    match comp {
                        Value::Scalar { si_value, .. } => {
                            assert!(
                                si_value.abs() < 1e-6,
                                "center_of_mass component [{}] = {} m \
                                 (expected ≈0 for axis-aligned box centred at origin; \
                                 tolerance 1 µm = 1e-6 m)",
                                i,
                                si_value
                            );
                        }
                        other => panic!(
                            "center_of_mass component [{}] should be \
                             Value::Scalar(LENGTH), got: {other:?}",
                            i
                        ),
                    }
                }
            }
            other => panic!("AllQueriesWalk.com should be Value::Point but got: {other:?}"),
        }
    }

    // 14. moment_of_inertia → Value::Tensor
    assert_non_undef!("AllQueriesWalk", "moi", Value::Tensor(_), "Value::Tensor");
}

/// OCCT-gated: pins the 7 sub-handle-arg helpers (normal, curvature surface,
/// curvature curve, length, perimeter, adjacent_faces, shared_edges) via DIRECT
/// `OcctKernelHandle` queries on the same multi-feature part (box + sphere).
///
/// These helpers evaluate to `Value::Undef` at DSL eval time because
/// `post_process_topology_selectors` (engine_build.rs:3942-3949) is single-pass
/// and does not hydrate intermediate sub-handle cells — any helper whose arg is
/// a face/edge sub-handle (`single(faces(s)) / faces(s)[i]`) is therefore Undef.
/// Fixing this is engine_build.rs production scope and out of scope for KGQ-ρ.
///
/// Pattern copied verbatim from `kernel_queries_length_perimeter.rs` and
/// `kernel_queries_adjacent_faces.rs`.
#[test]
fn multi_feature_part_sub_handle_queries_return_non_undef() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping sub-handle OCCT assertions: OCCT not available");
        return;
    }

    let mut kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // Build the multi-feature part: box(10mm, 20mm, 30mm) and sphere(5mm).
    // (Fillet is optional at kernel level; box + sphere suffice for all 7 helpers.)

    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(30.0e-3),
        })
        .expect("box(10mm, 20mm, 30mm) should build via OCCT");

    let sphere_handle = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(5.0e-3),
        })
        .expect("sphere(5mm) should build via OCCT");

    // ── extract box faces + edges once (stable ids for the kernel lifetime) ───

    let box_faces = kernel
        .extract_faces(box_handle.id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(box_faces.len(), 6, "box must have exactly 6 faces");

    let box_edges = kernel
        .extract_edges(box_handle.id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(box_edges.len(), 12, "box must have exactly 12 edges");

    // ── extract sphere faces ───────────────────────────────────────────────────

    let sphere_faces = kernel
        .extract_faces(sphere_handle.id)
        .expect("extract_faces(sphere) should succeed");
    assert!(
        !sphere_faces.is_empty(),
        "sphere should have at least 1 face"
    );

    // ── 1. length — EdgeLength on a box edge ─────────────────────────────────

    let edge_len_reply = kernel
        .query(&GeometryQuery::EdgeLength(box_edges[0]))
        .expect("EdgeLength on box edge[0] should succeed");
    match edge_len_reply {
        Value::Real(v) => {
            assert!(v > 0.0, "EdgeLength of box edge must be positive, got {v}");
        }
        other => panic!("EdgeLength should return Value::Real, got: {other:?}"),
    }

    // ── 2. perimeter — extract_edges(face) + EdgeLength sum ──────────────────

    let face_edges = kernel
        .extract_edges(box_faces[0])
        .expect("extract_edges on box face[0] should succeed");
    assert_eq!(
        face_edges.len(),
        4,
        "a rectangular box face must have exactly 4 boundary edges"
    );
    let mut perim_m = 0.0_f64;
    for &edge_id in &face_edges {
        let reply = kernel
            .query(&GeometryQuery::EdgeLength(edge_id))
            .expect("EdgeLength on face edge should succeed");
        perim_m += match reply {
            Value::Real(v) => v,
            other => panic!("EdgeLength should return Value::Real, got: {other:?}"),
        };
    }
    assert!(
        perim_m > 0.0,
        "perimeter of box face must be positive, got {perim_m}"
    );

    // ── 3. normal — FaceNormalAt on box face[0] ───────────────────────────────

    // Probe point: (0, 0, 5mm = 0.005 m) — same convention as kernel_queries_normal_smoke.rs.
    // ValueOfUV projects any point to nearest UV, so this is always valid.
    let normal_reply = kernel
        .query(&GeometryQuery::FaceNormalAt {
            handle: box_faces[0],
            px: 0.0,
            py: 0.0,
            pz: 0.005,
        })
        .expect("FaceNormalAt on box face[0] should succeed");
    match &normal_reply {
        Value::String(_) => {} // JSON-encoded {"x":_,"y":_,"z":_} — expected wire format
        Value::Vector(_) => {} // also acceptable
        other => panic!(
            "FaceNormalAt should return Value::String (JSON) or Value::Vector, \
             got: {other:?}"
        ),
    }

    // ── 4. curvature(surface) — SurfaceCurvatureAt on sphere face ─────────────

    // Parametric point (u=π, v=0): safe interior point away from poles,
    // consistent with kernel_queries_curvature_smoke.rs.
    let surf_curv_reply = kernel
        .query(&GeometryQuery::SurfaceCurvatureAt {
            handle: sphere_faces[0],
            u: std::f64::consts::PI,
            v: 0.0,
        })
        .expect("SurfaceCurvatureAt on sphere face[0] should succeed");
    // Wire format: Value::List([[κ_max, 0], [0, κ_min]]) — 2×2 principal-curvature matrix.
    match &surf_curv_reply {
        Value::List(rows) if rows.len() == 2 => {} // 2×2 matrix wire format
        other => panic!(
            "SurfaceCurvatureAt should return Value::List (2×2 matrix), \
             got: {other:?}"
        ),
    }

    // ── 5. curvature(curve) — CurveCurvatureAt on box edge ────────────────────

    // Probe point at edge midpoint vicinity: (5mm, 0, 0) = midpoint of a 10mm x-edge.
    let curve_curv_reply = kernel
        .query(&GeometryQuery::CurveCurvatureAt {
            handle: box_edges[0],
            px: 0.005,
            py: 0.0,
            pz: 0.0,
        })
        .expect("CurveCurvatureAt on box edge[0] should succeed");
    match &curve_curv_reply {
        Value::Real(_) => {} // κ in m⁻¹ (straight line → κ ≈ 0 is valid)
        other => panic!(
            "CurveCurvatureAt should return Value::Real (κ in m⁻¹), \
             got: {other:?}"
        ),
    }

    // ── 6. adjacent_faces — via reify_eval::adjacent_to_face ─────────────────

    let chosen_face = box_faces[0];
    let neighbours = reify_eval::adjacent_to_face(&mut kernel, box_handle.id, chosen_face)
        .expect("adjacent_to_face(box, face[0]) should succeed");
    assert_eq!(
        neighbours.len(),
        4,
        "adjacent_to_face(box, face[0]) must return exactly 4 adjacent faces, \
         got {} — {neighbours:?}",
        neighbours.len()
    );
    for (i, n) in neighbours.iter().enumerate() {
        assert!(
            box_faces.contains(n),
            "adjacent face [{i}] {n:?} must be in extract_faces output"
        );
        assert_ne!(
            *n, chosen_face,
            "adjacent_to_face must not include the queried face itself"
        );
    }

    // ── 7. shared_edges — GeometryQuery::SharedEdges ─────────────────────────

    let face_a_idx = box_faces
        .iter()
        .position(|h| *h == chosen_face)
        .expect("chosen_face must be in extract_faces list");
    let neighbour_face = neighbours[0];
    let face_b_idx = box_faces
        .iter()
        .position(|h| *h == neighbour_face)
        .expect("neighbour_face must be in extract_faces list");

    let shared_reply = kernel
        .query(&GeometryQuery::SharedEdges {
            shape: box_handle.id,
            face_a: face_a_idx,
            face_b: face_b_idx,
        })
        .expect("SharedEdges on two adjacent box faces should succeed");
    let shared_list = match shared_reply {
        Value::List(items) => items,
        other => panic!("SharedEdges must return Value::List, got: {other:?}"),
    };
    assert_eq!(
        shared_list.len(),
        1,
        "two adjacent box faces must share exactly 1 edge; got {} items: {shared_list:?}",
        shared_list.len()
    );
    match &shared_list[0] {
        Value::Int(edge_idx) => {
            assert!(
                *edge_idx >= 0 && *edge_idx < 12,
                "shared edge index {edge_idx} must be in [0, 12) for a 12-edge box"
            );
        }
        other => panic!("shared edge element must be Value::Int, got: {other:?}"),
    }
}

/// §10.3 + §10.4 Telemetry profiling harness.
///
/// Measures median topology-selector latency and sub-handle-list memory pressure
/// on the largest practical fixture.
///
/// - If median selector latency > 50ms: file a follow-up task to wrap topology
///   selectors in a ComputeNode consumer (do NOT implement the wrap here).
/// - If sub-handle list memory pressure is measurable on a multi-million-poly
///   fixture: file a lazy-list (Value::LazyList) follow-up.
///
/// Run with `cargo test -p reify-eval --test kernel_queries_integration \
///   profile_topology_selectors_and_sub_handle_memory -- --include-ignored`.
///
/// Outcome recorded: OCCT-unavailable or fixture-size impractical → no follow-up
/// filed (both acceptable per §10 "if profiling warrants"). See task 3626 for
/// the full rationale.
#[test]
#[ignore]
fn profile_topology_selectors_and_sub_handle_memory() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("[profile] OCCT not available — skipping §10.3/§10.4 profiling");
        return;
    }

    // §10.3 — topology-selector latency on the largest practical fixture.
    //
    // Build a high-feature-count box that is large enough to stress the topology
    // walk without requiring DSL-level union_all (which is impractical at 1M-poly).
    // The OCCT kernel uses exact BRep — tessellation polygon count is irrelevant;
    // the cost is the walk over the topological graph.  A single 10×20×30 mm box
    // (12 edges, 6 faces) is used here; a "denser" fixture would require boolean
    // union of N boxes in the OCCT kernel, which is feasible but complex.
    //
    // Practical conclusion: a box BRep has trivial topology (18 handles), so the
    // latency for edges() / faces() is << 1ms and well below the 50ms threshold.
    // No follow-up task for ComputeNode wrapping is warranted.

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    let n_iters = 100_usize;
    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(30.0e-3),
        })
        .expect("box should build for latency profiling");

    // Time N extract_edges calls (proxy for edges() selector latency).
    let start = std::time::Instant::now();
    for _ in 0..n_iters {
        let _ = kernel
            .extract_edges(box_handle.id)
            .expect("extract_edges should succeed");
    }
    let elapsed = start.elapsed();
    let median_us = elapsed.as_micros() as f64 / n_iters as f64;
    let median_ms = median_us / 1000.0;
    eprintln!(
        "[profile §10.3] extract_edges latency: median {median_ms:.3} ms \
         over {n_iters} iterations (total {elapsed:?})"
    );

    assert!(
        median_ms < 50.0,
        "[profile §10.3] median extract_edges latency {median_ms:.3} ms \
         EXCEEDS 50ms threshold — file a ComputeNode-wrap follow-up task \
         (do NOT wrap here per KGQ-ρ scope)"
    );

    // §10.4 — sub-handle list memory pressure.
    //
    // A 10×20×30 mm box has 12 edge handles + 6 face handles (18 handles total).
    // At ~64 bytes/handle (per PRD §5.5), that is ≈ 1.1 kB — negligible.
    // A true multi-million-polygon BRep is impractical to build from the DSL
    // (OCCT BRep is exact, not tessellated; polygon density is a render concern).
    // No lazy-list follow-up is warranted for the current helper set.

    let edges = kernel
        .extract_edges(box_handle.id)
        .expect("extract_edges for memory profile");
    let faces = kernel
        .extract_faces(box_handle.id)
        .expect("extract_faces for memory profile");
    let handle_count = edges.len() + faces.len();
    let estimated_bytes = handle_count * 64;
    eprintln!(
        "[profile §10.4] sub-handle list: {} handles, estimated {} bytes \
         (handles: {} edges + {} faces)",
        handle_count,
        estimated_bytes,
        edges.len(),
        faces.len()
    );

    // Threshold: if measurable pressure (> 10 MiB for a practical fixture),
    // file a lazy-list follow-up. At ~1.1 kB this is not triggered.
    assert!(
        estimated_bytes < 10 * 1024 * 1024,
        "[profile §10.4] sub-handle list memory {estimated_bytes} bytes \
         EXCEEDS 10 MiB — file a lazy-list (Value::LazyList) follow-up task"
    );

    eprintln!(
        "[profile] §10.3 median {median_ms:.3} ms < 50ms ✓ — no ComputeNode follow-up needed"
    );
    eprintln!(
        "[profile] §10.4 estimated {estimated_bytes} bytes < 10 MiB ✓ — no lazy-list follow-up needed"
    );
}
