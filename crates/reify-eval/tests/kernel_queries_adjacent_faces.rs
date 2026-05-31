//! End-to-end integration test for KGQ-κ relational topology selectors
//! `adjacent_faces` and `shared_edges` (task 3619,
//! PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 Phase 3).
//!
//! Fixture: `examples/kernel_queries/adjacent_faces.ri`
//!
//! ```ri
//! structure def AdjacentFaces {
//!     let b         = box(10mm, 20mm, 30mm)
//!     let zdir      = vec3(0.0, 0.0, 1.0)
//!     let xdir      = vec3(1.0, 0.0, 0.0)
//!     let tol       = 1deg
//!     let top       = single(faces_by_normal(b, zdir, tol))
//!     let side      = single(faces_by_normal(b, xdir, tol))
//!     let neighbors = adjacent_faces(b, top)
//!     let shared    = shared_edges(top, side)
//! }
//! ```
//!
//! **Runtime note — chaining limitation (out of scope for KGQ-κ):**
//! `post_process_topology_selectors` (engine_build.rs:3942-3949) does NOT
//! re-evaluate intervening value cells.  A `single(...)` cell between two
//! selectors is therefore computed (to `Value::Undef`) *before* its selector
//! arg is dispatched, so `adjacent_faces(b, single(faces_by_normal(...)))`
//! and `shared_edges(single(...), single(...))` leave their cells at
//! `Value::Undef` at runtime.  Fixing the selector→list-helper→selector
//! eval-chaining is `engine_build.rs` scope and explicitly out of scope for
//! this task.  Consequently:
//!
//! - **Assertion 1** (always-on) pins *compilation only*: the fixture must
//!   parse + compile with no error diagnostics, confirming that `adjacent_faces`
//!   and `shared_edges` are registered in the grammar and type system on every
//!   CI runner.
//!
//! - **Assertion 2** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) pins the
//!   *runtime semantics* via the kernel + selector-vocabulary layer directly,
//!   bypassing the eval-chaining limitation.  A 10×20×30 mm box is built via
//!   `OcctKernelHandle`; `selector_vocabulary_v2::adjacent_to_face` must
//!   return exactly **4** adjacent faces for a chosen face, and
//!   `GeometryQuery::SharedEdges` must return exactly **1** edge for that
//!   face and one of its neighbours.  This re-anchors, from the
//!   `kernel_queries` consumer namespace, the semantics already covered at
//!   `selector_vocabulary_v2_e2e.rs:226` (`adjacent_to_face_box_each_face_has_four_neighbours`)
//!   and `topology_selectors_integration.rs:201`
//!   (`box_two_adjacent_faces_share_exactly_one_edge`).
//!
//! Modelled on `kernel_queries_directional_selectors.rs`.

use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/adjacent_faces.ri"
);

/// End-to-end pin for KGQ-κ: `adjacent_faces` (4 adjacent faces per box face)
/// and `shared_edges` (1 shared edge per adjacent face pair) on a 10×20×30 mm box.
///
/// Assertion 1 (always-on): the `.ri` fixture compiles cleanly — pins grammar +
/// type-system registration for `adjacent_faces` and `shared_edges` on every CI runner.
///
/// Assertion 2 (OCCT-gated): confirms the box semantics via the kernel/selector layer
/// directly, because the `.ri`'s chained selector cells stay `Value::Undef` at eval
/// (engine_build.rs selector→list-helper→selector chaining limitation, out of scope).
#[test]
fn adjacent_faces_and_shared_edges_compile_and_return_correct_semantics() {
    // ── assertion 1: fixture exists and compiles cleanly (unconditional) ──────

    let source = std::fs::read_to_string(FIXTURE_PATH).expect(
        "examples/kernel_queries/adjacent_faces.ri should exist (task 3619 pre-1)",
    );
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "adjacent_faces.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: OCCT-backed semantics (gated) ────────────────────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping adjacent_faces OCCT assertions: OCCT not available");
        return;
    }

    let mut kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // Build the same 10×20×30 mm box that adjacent_faces.ri models.
    let box_id = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(30.0e-3),
        })
        .expect("10×20×30 mm box should build via OCCT")
        .id;

    // Extract face handles once — indices are stable for the lifetime of the kernel.
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a rectangular box must have exactly 6 faces in TopExp order"
    );

    // ── adjacent_to_face: chosen face must have exactly 4 neighbours ─────────

    let chosen_face = face_handles[0];
    let neighbours =
        reify_eval::adjacent_to_face(&mut kernel, box_id, chosen_face)
            .expect("adjacent_to_face(box, face[0]) should succeed");
    assert_eq!(
        neighbours.len(),
        4,
        "adjacent_to_face(box, face[0]) must return exactly 4 faces \
         (all faces except the chosen face's opposite); got {} — {neighbours:?}",
        neighbours.len()
    );
    // All neighbours must be drawn from the canonical extract_faces set.
    for (i, n) in neighbours.iter().enumerate() {
        assert!(
            face_handles.contains(n),
            "adjacent_to_face result[{i}] ({n:?}) must be in extract_faces output"
        );
        assert!(
            *n != chosen_face,
            "adjacent_to_face must not include the queried face itself ({chosen_face:?})"
        );
    }

    // ── SharedEdges: face[0] and its first neighbour must share exactly 1 edge ─

    let neighbour_face = neighbours[0];

    // Recover the 0-based indices needed by GeometryQuery::SharedEdges.
    let face_a_idx = face_handles
        .iter()
        .position(|h| *h == chosen_face)
        .expect("chosen_face must be in extract_faces list");
    let face_b_idx = face_handles
        .iter()
        .position(|h| *h == neighbour_face)
        .expect("neighbour_face must be in extract_faces list");

    let shared_reply = kernel
        .query(&GeometryQuery::SharedEdges {
            shape: box_id,
            face_a: face_a_idx,
            face_b: face_b_idx,
        })
        .expect("SharedEdges query for two adjacent box faces should succeed");

    let shared_indices = match shared_reply {
        Value::List(items) => items,
        other => panic!(
            "SharedEdges must return Value::List, got: {other:?}"
        ),
    };
    assert_eq!(
        shared_indices.len(),
        1,
        "two adjacent box faces must share exactly 1 edge; \
         SharedEdges(face_a={face_a_idx}, face_b={face_b_idx}) returned {} items: {shared_indices:?}",
        shared_indices.len()
    );
    // The single element must be a valid integer edge index in [0, 12).
    match &shared_indices[0] {
        Value::Int(edge_idx) => {
            assert!(
                *edge_idx >= 0 && *edge_idx < 12,
                "shared edge index {edge_idx} must be in [0, 12) for a 12-edge box"
            );
        }
        other => panic!("shared edge element must be Value::Int, got: {other:?}"),
    }
}
