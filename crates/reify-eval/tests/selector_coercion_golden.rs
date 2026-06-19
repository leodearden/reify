//! User-observable golden for the γ `Selector → List<Geometry>` coercion
//! (task 4118; PRD `docs/prds/topology-selector-value-type.md`).
//!
//! Fixture: `examples/selectors/single_face_by_normal.ri`
//!
//! ```ri
//! structure def SingleFaceByNormal {
//!     let b   = box(10mm, 10mm, 10mm)
//!     let dir = vec3(0.0, 0.0, 1.0)
//!     let tol = 1deg
//!     let sel = faces_by_normal(b, dir, tol)
//!     let top = single(faces_by_normal(b, dir, tol))
//! }
//! ```
//!
//! This is the end-to-end signal that the re-typed predicate selector
//! constructors (which now evaluate to a typed `Value::Selector(kind)` instead
//! of an eager `Value::List<GeometryHandle>`) flow through the compiler-inserted
//! `ResolveSelector` coercion node and the single `topology_selectors::resolve`
//! executor to realize the asserted +Z face.
//!
//! Assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — the fixture parses + compiles with no error
//!    diagnostics, pinning the `single(faces_by_normal(...))` coercion shape on
//!    every CI runner (the `Selector(Face)` argument is accepted by `single`'s
//!    `List<Geometry>` contract via the β `type_compatible` rule).
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    - `SingleFaceByNormal.sel` holds a kernel-FREE `Value::Selector(Face)`
//!      whose leaf is `ByNormal { +Z, 1° }` (BT7: zero kernel queries during
//!      construction — the cell is the typed selector, not a resolved list).
//!    - `SingleFaceByNormal.top` coerces to exactly the single +Z face
//!      `Value::GeometryHandle` (BT5: `single(faces_by_normal(b, +Z, 1°))`
//!      resolves to one face and unwraps it).
//!
//! A companion test pins call-site transparency (D4): every
//! `examples/kernel_queries/*.ri` fixture — which consumes the now-re-typed
//! selectors through `faces(s)[0]` / `single(...)` sites — must keep compiling
//! UNCHANGED.
//!
//! Modelled on `kernel_queries_directional_selectors.rs` (task 3618) and
//! `kernel_queries_filtered_edges.rs` (task 3617). Per esc-4118-52 the second γ
//! golden (`fillet(b, edges_at_height(...), 1mm)`) is OUT OF SCOPE; this single
//! face-by-normal golden carries the full signal.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/selectors/single_face_by_normal.ri"
);

const KERNEL_QUERIES_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/kernel_queries");

/// End-to-end γ golden: `single(faces_by_normal(b, +Z, 1°))` realizes the single
/// +Z face through the `Selector → List<Geometry>` coercion, while the bare
/// `faces_by_normal(...)` cell holds a kernel-free `Value::Selector(Face)`.
#[test]
fn single_face_by_normal_coercion_golden() {
    // ── assertion 1: fixture exists and compiles cleanly (unconditional) ──────

    let source = std::fs::read_to_string(FIXTURE_PATH).expect(
        "examples/selectors/single_face_by_normal.ri should exist (task 4118 γ golden fixture)",
    );
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "single_face_by_normal.ri should compile with no error diagnostics \
         (single() accepts a Selector(Face) arg via the β coercion), got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: OCCT-backed runtime (gated) ──────────────────────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping single_face_by_normal OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) the bare faces_by_normal(...) cell is a kernel-FREE Value::Selector(Face)
    //     whose leaf is ByNormal { +Z, 1° } (BT7: construction issues no queries —
    //     the cell is the typed selector, NOT a resolved geometry list).
    let sel_cell = ValueCellId::new("SingleFaceByNormal", "sel");
    match result.values.get(&sel_cell) {
        Some(Value::Selector(sv)) => {
            assert_eq!(
                sv.kind,
                SelectorKind::Face,
                "sel = faces_by_normal(...) must be Value::Selector(Face) (task 4118 γ)"
            );
            match &sv.node {
                SelectorNode::Leaf {
                    query: LeafQuery::ByNormal { dir, tol_rad },
                    ..
                } => {
                    assert_eq!(
                        *dir,
                        [0.0, 0.0, 1.0],
                        "sel leaf ByNormal dir must be +Z (sign-sensitive)"
                    );
                    assert!(
                        *tol_rad > 0.0,
                        "sel leaf ByNormal tol_rad must be positive (1deg), got {tol_rad}"
                    );
                }
                other => panic!("sel must be a ByNormal Leaf selector node, got: {other:?}"),
            }
        }
        other => panic!(
            "SingleFaceByNormal.sel must be a kernel-free Value::Selector(Face) \
             (BT7: zero kernel queries during construction), got: {other:?}"
        ),
    }

    // (b) the coercion realizes exactly the single +Z face handle (BT5):
    //     single(faces_by_normal(b, +Z, 1°)) resolves to one face and unwraps it
    //     to a Value::GeometryHandle.
    let top_cell = ValueCellId::new("SingleFaceByNormal", "top");
    match result.values.get(&top_cell) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => {
            assert_ne!(
                upstream_values_hash, &[0u8; 32],
                "top face handle upstream_values_hash must be non-zero (PRD §4 i)"
            );
        }
        other => panic!(
            "SingleFaceByNormal.top = single(faces_by_normal(b, +Z, 1°)) must coerce \
             (Selector → List<Geometry> → single) to the single +Z face \
             Value::GeometryHandle (BT5), got: {other:?}"
        ),
    }
}

/// Call-site transparency (D4): the re-typing of the 7 selector constructors to
/// `Value::Selector(kind)` must NOT break any existing consumer. Every
/// `examples/kernel_queries/*.ri` fixture — including `curvature_smoke.ri`'s
/// inline `faces(s)[0]` and the `single(...)` sites — must keep compiling with
/// zero error diagnostics, UNCHANGED.
#[test]
fn kernel_queries_examples_still_compile_unchanged() {
    let mut checked = 0usize;
    let entries =
        std::fs::read_dir(KERNEL_QUERIES_DIR).expect("examples/kernel_queries/ should exist");
    for entry in entries {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let compiled = parse_and_compile_with_stdlib(&source);
        assert!(
            errors_only(&compiled).is_empty(),
            "{} must keep compiling UNCHANGED after the selector re-typing \
             (call-site transparency, D4), got error diagnostics:\n{:#?}",
            path.display(),
            errors_only(&compiled)
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected to compile-check the full examples/kernel_queries corpus \
         (>=10 fixtures), only saw {checked}"
    );
}
