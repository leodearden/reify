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
//! ## Runtime note — FACE chain resolves end-to-end; EDGE half deferred (#4873)
//!
//! The selector→`single(...)`→relational-selector CHAIN resolves at eval time.
//! `template.value_cells` is in source order, so the single
//! `post_process_topology_selectors` pass dispatches `top = single(faces_by_normal(...))`
//! — hydrating it to a real sub-handle (task 4118's `single` arm of
//! `try_eval_resolve_selector`) — BEFORE the consuming `sides = siblings_of_face(b, top)`
//! cell. No fixpoint / re-evaluation is needed; the earlier "chaining limitation"
//! framing was refuted on current main (esc-4857-204).
//!
//! Assertions:
//!
//! - **Assertion 1** (always-on): the `.ri` fixture compiles with no error
//!   diagnostics — pins grammar + type-system registration for
//!   `siblings_of_face` and `ancestor_faces_of_edge` on every CI runner.
//!
//! - **Assertion 2** (OCCT-gated): confirms runtime semantics via the kernel +
//!   selector-vocabulary layer directly. A 10×10×10 mm box is built via
//!   `OcctKernelHandle`; `reify_eval::siblings_of_face` must return exactly **5**
//!   sibling faces for a chosen face, and `reify_eval::ancestor_faces_of_edge`
//!   must return exactly **2** owner faces for a chosen edge.
//!
//! - **Assertion 3** (OCCT-gated,
//!   `relational_selectors_v2_face_chain_resolves_end_to_end`): the true
//!   end-to-end signal — evaluating the `.ri` through `Engine::build` resolves the
//!   chained FACE selector `sides` to a `Value::List` of 5 hydrated face handles
//!   (not `Value::Undef`).
//!
//! The OCCT gate on Assertions 2 & 3 is intentional, not a coverage gap: dispatch
//! semantics (including `upstream_values_hash` stability) are covered
//! unconditionally by the mock-kernel unit tests in
//! `crates/reify-eval/src/geometry_ops.rs` —
//! `siblings_of_face_dispatch_returns_geometry_handle_list` and
//! `ancestor_faces_of_edge_dispatch_returns_geometry_handle_list`.
//!
//! EDGE half deferred (#4873): the `.ri` omits `an_edge = single(edges_parallel_to(...))`
//! / `owners = ancestor_faces_of_edge(b, an_edge)`. On a 10mm cube 4 edges are
//! parallel to +Z, so `single()` of a >1-element list is `Value::Undef` by contract
//! — a FIXTURE CARDINALITY issue, NOT a chaining gap. An end-to-end
//! `owners = Value::List(2)` assertion needs a single-valued edge selector; tracked
//! in #4873.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

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
/// directly — complementary to Assertion 3
/// (`relational_selectors_v2_face_chain_resolves_end_to_end`), which exercises the
/// same `siblings_of_face` through the full `.ri` → `Engine::build` eval path.
#[test]
fn relational_selectors_v2_compile_and_return_correct_semantics() {
    // ── assertion 1: fixture compiles cleanly (unconditional) ─────────────────

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/selectors/relational_selectors_v2.ri should exist (task #4759 pre-1)");
    // Use the non-asserting `compile_source_with_stdlib` so the explicit
    // `errors_only` assert below is the active gate (with a descriptive message),
    // not a redundant check behind the helper's internal panic.
    let compiled = compile_source_with_stdlib(&source);
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

/// Assertion 3 — the user-observable end-to-end signal (#4857, Option B).
///
/// Evaluating `relational_selectors_v2.ri` through the full engine stack resolves
/// the chained FACE selector `let top = single(faces_by_normal(b,+Z,1deg)); let sides
/// = siblings_of_face(b, top)` to a `Value::List` of 5 hydrated face handles. This
/// proves the natural `single(selector)`→relational-selector authoring form works at
/// eval time — not just at the kernel layer (Assertion 2).
///
/// OCCT-gated: `top` hydrates to a real sub-handle via a live kernel, so the build
/// needs a real `OcctKernelHandle`.
///
/// The EDGE half (`owners = ancestor_faces_of_edge(b, single(edges_parallel_to(...)))`)
/// is deferred to #4873: a 10mm cube has 4 edges parallel to +Z, so `single()` of that
/// multi-element list is `Value::Undef` by contract (fixture cardinality, not a chaining
/// gap). An `owners = Value::List(2)` e2e assertion needs a single-valued edge selector.
#[test]
fn relational_selectors_v2_face_chain_resolves_end_to_end() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping relational_selectors_v2 end-to-end eval assertion: OCCT not available"
        );
        return;
    }

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/selectors/relational_selectors_v2.ri should exist");
    let compiled = compile_source_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "relational_selectors_v2.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Build through the full engine stack with a real OCCT kernel so the
    // `single(faces_by_normal(...))` cell hydrates `top` to a real sub-handle
    // BEFORE the `sides = siblings_of_face(b, top)` cell dispatches (the value
    // cells are in source order, so the single post_process pass suffices).
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Stl);

    let sides_id = ValueCellId::new("RelationalSelectorsV2", "sides");
    match result.values.get(&sides_id) {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                5,
                "sides = siblings_of_face(b, single(faces_by_normal(b,+Z,1deg))) must \
                 evaluate end-to-end to a Value::List of 5 face handles (box has 6 faces; \
                 siblings = all-but-one); got {} — {items:?}",
                items.len()
            );
            for (i, item) in items.iter().enumerate() {
                assert!(
                    matches!(item, Value::GeometryHandle { .. }),
                    "sides[{i}] must be a hydrated Value::GeometryHandle, got {item:?}"
                );
            }
        }
        other => panic!(
            "sides must evaluate to Value::List(5) end-to-end through Engine::build; \
             got {other:?}. Engine diagnostics: {:#?}",
            result.diagnostics
        ),
    }
}
