//! Integration test for v2 relational-walk selectors `siblings_of_face` and
//! `ancestor_faces_of_edge` (task #4759).
//!
//! Fixture: `examples/selectors/relational_selectors_v2.ri`
//!
//! Mirrors `crates/reify-eval/tests/kernel_queries_adjacent_faces.rs` in
//! structure (unconditional compile assertion + OCCT-gated kernel-layer count
//! assertion), and mirrors the `examples/kernel_queries/adjacent_faces.ri`
//! arg-shape.
//!
//! ## Runtime note — chaining limitation (out of scope)
//!
//! `post_process_topology_selectors` (engine_build.rs) does NOT re-evaluate
//! intervening value cells. A `single(...)` cell between two selectors is
//! computed (to `Value::Undef`) before its selector arg is dispatched, so
//! `siblings_of_face(b, single(faces_by_normal(...)))` and
//! `ancestor_faces_of_edge(b, single(edges_parallel_to(...)))` leave their
//! cells at `Value::Undef` at runtime. Fixing the selector→list-helper→selector
//! eval-chaining is `engine_build.rs` scope, explicitly out of scope.
//!
//! Consequently:
//!
//! - **Assertion 1** (always-on): the `.ri` fixture compiles with no error
//!   diagnostics — pins grammar + type-system registration for
//!   `siblings_of_face` and `ancestor_faces_of_edge` on every CI runner.
//!
//! - **Assertion 2** (OCCT-gated): confirms runtime semantics via the kernel +
//!   selector-vocabulary layer directly, bypassing the eval-chaining limitation.
//!   A 10×10×10 mm box is built via `OcctKernelHandle`; `reify_eval::siblings_of_face`
//!   must return exactly **5** sibling faces for a chosen face, and
//!   `reify_eval::ancestor_faces_of_edge` must return exactly **2** owner faces
//!   for a chosen edge.

use reify_ir::{GeometryOp, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/selectors/relational_selectors_v2.ri"
);

/// End-to-end pin for `siblings_of_face` (5 sibling faces per box face) and
/// `ancestor_faces_of_edge` (2 ancestor faces per box edge) on a 10×10×10 mm box.
///
/// Assertion 1 (always-on): the `.ri` fixture compiles cleanly — pins grammar +
/// type-system registration for both selectors on every CI runner.
///
/// Assertion 2 (OCCT-gated): confirms semantics via the kernel/selector layer
/// directly, because the `.ri`'s chained selector cells stay `Value::Undef` at
/// eval (engine_build.rs selector→list-helper→selector chaining limitation, out
/// of scope for this task).
#[test]
fn relational_selectors_v2_compile_and_return_correct_semantics() {
    // ── assertion 1: fixture compiles cleanly (unconditional) ─────────────────

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/selectors/relational_selectors_v2.ri should exist (task #4759 pre-1)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "relational_selectors_v2.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: OCCT-backed semantics (gated) ────────────────────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping relational_selectors_v2 OCCT assertions: OCCT not available");
        return;
    }

    let mut kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // Build the same 10×10×10 mm box that relational_selectors_v2.ri models.
    let box_id = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("10×10×10 mm box should build via OCCT")
        .id;

    // Extract face and edge handles once — indices are stable for the kernel lifetime.
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a rectangular box must have exactly 6 faces in TopExp order"
    );

    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(
        edge_handles.len(),
        12,
        "a rectangular box must have exactly 12 edges in TopExp order"
    );

    // ── siblings_of_face: a chosen face must have exactly 5 siblings ─────────

    let chosen_face = face_handles[0];
    let siblings = reify_eval::siblings_of_face(&mut kernel, box_id, chosen_face)
        .expect("siblings_of_face(box, face[0]) should succeed");
    assert_eq!(
        siblings.len(),
        5,
        "siblings_of_face(box, face[0]) must return exactly 5 faces \
         (a box has 6 faces; siblings = all-but-one = 5); got {} — {siblings:?}",
        siblings.len()
    );
    // The returned handles must be drawn from extract_faces and exclude the chosen face.
    for (i, s) in siblings.iter().enumerate() {
        assert!(
            face_handles.contains(s),
            "siblings_of_face result[{i}] ({s:?}) must be in extract_faces output"
        );
        assert!(
            *s != chosen_face,
            "siblings_of_face must not include the queried face itself ({chosen_face:?})"
        );
    }

    // ── ancestor_faces_of_edge: a chosen edge must have exactly 2 owner faces ─

    let chosen_edge = edge_handles[0];
    let owners = reify_eval::ancestor_faces_of_edge(&mut kernel, box_id, chosen_edge)
        .expect("ancestor_faces_of_edge(box, edge[0]) should succeed");
    assert_eq!(
        owners.len(),
        2,
        "ancestor_faces_of_edge(box, edge[0]) must return exactly 2 faces \
         (every edge of a closed manifold solid bounds exactly 2 faces); \
         got {} — {owners:?}",
        owners.len()
    );
    // All returned handles must be face handles of the parent box.
    for (i, o) in owners.iter().enumerate() {
        assert!(
            face_handles.contains(o),
            "ancestor_faces_of_edge result[{i}] ({o:?}) must be in extract_faces output"
        );
    }
}
