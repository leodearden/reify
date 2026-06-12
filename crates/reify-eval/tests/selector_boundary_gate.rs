//! Topology-selector boundary-test integration gate (BT1–BT8).
//!
//! Asserts the full §5 BT table from PRD
//! `docs/prds/topology-selector-value-type.md` across both the producer side
//! (type-checker, resolve, composition — BT1/BT2/BT3/BT7/BT8) and the
//! consumer side (kind-typed params, eager coercion — BT4/BT5/BT6).
//!
//! ## Ratified divergences from a literal PRD §5 reading
//!
//! **BT6 (kind-typed param rejects wrong selector):** PRD says
//! `E_SELECTOR_KIND_MISMATCH`. Empirically verified on main@803c3eea9d:
//! `needs_face(edges(b))` (param `: FaceSelector`) produces ONE
//! `Severity::Error` with `code = None` and message "no matching overload for
//! needs_face(EdgeSelector), candidates: needs_face(FaceSelector)".
//! `DiagnosticCode::SelectorKindMismatch` is emitted ONLY by the composition
//! path (`units.rs::selector_composition_result_type`). This test asserts the
//! ACTUAL behavior (overload-mismatch error, `code = None`) and does NOT pin
//! `SelectorKindMismatch` for the param-binding path. The gap is recorded
//! non-blocking in esc-4120-17.
//!
//! **BT4 (eager coercion realizes geometry):** Fillet edge-selector RUNTIME
//! resolution is out of scope (esc-4118-52). The geometry golden uses the
//! wired `faces(b)[0]` IndexAccess shape (OCCT-gated); fillet appears as a
//! separate compile-only call-site-transparency fixture (BT4b). This honours
//! γ's G6 re-scope (esc-4118-55) which changed BT4 from a baseline-diff to a
//! fixture-golden with explicit IndexAccess coverage.
//!
//! ## Harness strategy
//!
//! - **Compile-only BTs (BT1, BT6):** always-on via `compile_source_with_stdlib`
//!   + `errors_only`.
//! - **Resolve-semantics BTs (BT2, BT3, BT8):** always-on, deterministic —
//!   build the fixture with an unstaged `MockGeometryKernel` (kernel-free), read
//!   the `Value::Selector` cell, then call `topology_selectors::resolve` against
//!   the `staged_box_kernel()` and assert set semantics. No OCCT.
//! - **BT7 (kernel-free construction / K2):** always-on, `CountingMockKernel`
//!   wrapping an unstaged mock; assert `total_query_count() == 0` after build.
//! - **Geometry goldens (BT4, BT5):** always-on compile assertion +
//!   OCCT-gated (`reify_kernel_occt::OCCT_AVAILABLE`) realized-handle assertion,
//!   mirroring `selector_coercion_golden.rs`.

#![allow(dead_code)] // helpers used by subsequent test steps

use reify_constraints::SimpleConstraintChecker;
use reify_core::diagnostics::{Diagnostic, DiagnosticCode};
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::{topology_selectors, BuildResult, Engine};
use reify_ir::value::{LeafQuery, SelectorNode, SelectorValue};
use reify_ir::{ExportFormat, GeometryHandleId, Value};
use reify_test_support::{
    compile_source_with_stdlib, errors_only, parse_and_compile_with_stdlib, CountingMockKernel,
    MockGeometryKernel,
};

// ── Fixture path helper ──────────────────────────────────────────────────────

/// Absolute path to a fixture file in `tests/fixtures/selectors/`.
fn fixture_path(name: &str) -> String {
    format!(
        "{}/tests/fixtures/selectors/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

// ── Staged box kernel ────────────────────────────────────────────────────────

/// Return a `MockGeometryKernel` staged for a canonical axis-aligned box.
///
/// The box body occupies `GeometryHandleId(1)` (the first handle allocated by
/// `MockGeometryKernel::execute` for `box(10mm, 10mm, 10mm)`).
///
/// Six face handles (ids 2–7) are registered via `with_extracted_faces`, each
/// paired with a `with_face_normal_result` encoding the six axis-aligned unit
/// normals so that `faces_by_normal` and `faces()` resolve deterministically:
///
/// | id | normal |
/// |----|--------|
/// |  2 | +X     |
/// |  3 | −X     |
/// |  4 | +Y     |
/// |  5 | −Y     |
/// |  6 | +Z     |
/// |  7 | −Z     |
///
/// Used by the always-on resolve-semantics BTs (BT2, BT3, BT8) so they run on
/// every CI runner without requiring OCCT.
fn staged_box_kernel() -> MockGeometryKernel {
    let parent = GeometryHandleId(1);
    let (f_px, f_nx) = (GeometryHandleId(2), GeometryHandleId(3));
    let (f_py, f_ny) = (GeometryHandleId(4), GeometryHandleId(5));
    let (f_pz, f_nz) = (GeometryHandleId(6), GeometryHandleId(7));

    MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_px, f_nx, f_py, f_ny, f_pz, f_nz])
        .with_face_normal_result(
            f_px,
            Value::String(r#"{"x":1.0,"y":0.0,"z":0.0}"#.to_string()),
        )
        .with_face_normal_result(
            f_nx,
            Value::String(r#"{"x":-1.0,"y":0.0,"z":0.0}"#.to_string()),
        )
        .with_face_normal_result(
            f_py,
            Value::String(r#"{"x":0.0,"y":1.0,"z":0.0}"#.to_string()),
        )
        .with_face_normal_result(
            f_ny,
            Value::String(r#"{"x":0.0,"y":-1.0,"z":0.0}"#.to_string()),
        )
        .with_face_normal_result(
            f_pz,
            Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string()),
        )
        .with_face_normal_result(
            f_nz,
            Value::String(r#"{"x":0.0,"y":0.0,"z":-1.0}"#.to_string()),
        )
}

// ── Selector-cell extractor ──────────────────────────────────────────────────

/// Extract a `&SelectorValue` from a `BuildResult` cell.
///
/// Looks up `ValueCellId::new(struct_name, member)` in `result.values` and
/// unwraps the `Value::Selector` variant. Panics with a descriptive message if
/// the cell is absent or holds a different value variant.
fn selector_cell<'a>(
    result: &'a BuildResult,
    struct_name: &str,
    member: &str,
) -> &'a SelectorValue {
    let cell = ValueCellId::new(struct_name, member);
    match result.values.get(&cell) {
        Some(Value::Selector(sv)) => sv,
        other => panic!(
            "{struct_name}.{member} must be Value::Selector(_) \
             (kernel-free construction, task 4118 γ BT7), got: {other:?}"
        ),
    }
}

// ── Unstaged-kernel engine build ──────────────────────────────────────────────

/// Build a fixture source with an unstaged `MockGeometryKernel`.
///
/// The source is compiled with `parse_and_compile_with_stdlib` (panics on
/// compile errors). The engine is constructed with a `SimpleConstraintChecker`
/// and a default (no staged replies) `MockGeometryKernel`. This is the
/// canonical kernel-free build used by BT2/BT3/BT8 to obtain the
/// `Value::Selector` cell before calling `topology_selectors::resolve()`.
fn build_with_unstaged_kernel(source: &str) -> BuildResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> = Box::new(MockGeometryKernel::new());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    engine.build(&compiled, ExportFormat::Step)
}

// ── OCCT gate ────────────────────────────────────────────────────────────────

/// Return `true` if OCCT is available on this CI runner.
///
/// Used to guard geometry-realization goldens (BT4 IndexAccess, BT5 single)
/// so they are skipped on runners without OCCT without failing the suite.
fn occt_available() -> bool {
    reify_kernel_occt::OCCT_AVAILABLE
}

// ── BT1: wrong-kind union rejected at compile time ────────────────────────────

/// BT1 (producer / type-checker): `union(faces(b), edges(b))` must be rejected
/// at compile time with exactly ONE `SelectorKindMismatch` error naming both
/// the encountered and expected kinds.
///
/// PRD §5 BT1: "mixed-kind `union()` → compile error E_SELECTOR_KIND_MISMATCH,
/// message names both kinds".
///
/// RED when fixture `bt1_wrong_kind_union.ri` is absent (`.expect()` panics).
#[test]
fn bt1_wrong_kind_union_rejected() {
    let source = std::fs::read_to_string(fixture_path("bt1_wrong_kind_union.ri")).expect(
        "fixture bt1_wrong_kind_union.ri must exist (create it in step-2 to turn GREEN)",
    );

    // Compile via compile_source_with_stdlib (NOT parse_and_compile_with_stdlib
    // which panics on errors — we WANT to check for errors here).
    let compiled = compile_source_with_stdlib(&source);
    let errors: Vec<&Diagnostic> = errors_only(&compiled);

    // (a) exactly ONE error-severity diagnostic
    assert_eq!(
        errors.len(),
        1,
        "BT1: expected exactly 1 SelectorKindMismatch error, got {} errors:\n{:#?}",
        errors.len(),
        errors
    );

    let err = errors[0];

    // (b) carries DiagnosticCode::SelectorKindMismatch
    assert_eq!(
        err.code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "BT1: error must carry DiagnosticCode::SelectorKindMismatch, got: {:?}",
        err.code
    );

    // (c) message names BOTH kinds (FaceSelector and EdgeSelector)
    assert!(
        err.message.contains("FaceSelector"),
        "BT1: error message must name FaceSelector, got: {:?}",
        err.message
    );
    assert!(
        err.message.contains("EdgeSelector"),
        "BT1: error message must name EdgeSelector, got: {:?}",
        err.message
    );

    // (d) call-site label/span present (at least one DiagnosticLabel)
    assert!(
        !err.labels.is_empty(),
        "BT1: error must carry at least one call-site label/span, got labels: {:?}",
        err.labels
    );
}
