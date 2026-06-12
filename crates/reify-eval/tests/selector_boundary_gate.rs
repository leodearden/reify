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

// ── BT3: difference and intersect set semantics ───────────────────────────────

/// BT3 (producer / resolve): difference and intersect set semantics.
///
/// - `d = difference(faces(b), faces_by_normal(b,+Z,1deg))` →
///   all 6 box faces EXCEPT the +Z face (5 handles: [2,3,4,5,7]).
/// - `i = intersect(faces_by_normal(b,+Z,1deg), faces_by_normal(b,-Z,1deg))` →
///   disjoint → `[]` (empty selection).
///
/// PRD §5 BT3: "difference/intersect set semantics".
///
/// Strategy: build kernel-free → get both selector cells → resolve each
/// against `staged_box_kernel()` (always-on, no OCCT).
///
/// Staged kernel face layout (±X/±Y/±Z, ids 2–7):
///   2=+X, 3=−X, 4=+Y, 5=−Y, 6=+Z, 7=−Z
///
/// RED when fixture `bt3_difference_intersect.ri` is absent.
#[test]
fn bt3_difference_and_intersect_set_semantics() {
    let source = std::fs::read_to_string(fixture_path("bt3_difference_intersect.ri")).expect(
        "fixture bt3_difference_intersect.ri must exist (create in step-10 to turn GREEN)",
    );

    let result = build_with_unstaged_kernel(&source);

    // ── difference: all faces EXCEPT +Z ─────────────────────────────────────
    let sv_d = selector_cell(&result, "BT3SetOps", "d");
    assert_eq!(sv_d.kind, SelectorKind::Face, "BT3: d must be Selector(Face)");

    let mut staged_d = staged_box_kernel();
    let mut diags_d: Vec<Diagnostic> = Vec::new();
    let handles_d = topology_selectors::resolve(sv_d, &mut staged_d, &mut diags_d)
        .expect("BT3: resolve() on difference must not return a QueryError");

    // faces(b) extracts [2,3,4,5,6,7]; faces_by_normal(+Z) = [6];
    // difference = [2,3,4,5,7] (face 6 excluded, order preserved from `a`).
    assert_eq!(
        handles_d,
        vec![
            GeometryHandleId(2), // +X
            GeometryHandleId(3), // -X
            GeometryHandleId(4), // +Y
            GeometryHandleId(5), // -Y
            GeometryHandleId(7), // -Z  (face 6 = +Z is excluded)
        ],
        "BT3: difference(faces(b), +Z-face) must be all 5 non-+Z faces in canonical order"
    );
    assert!(
        diags_d.is_empty(),
        "BT3: difference resolve() must emit no diagnostics, got: {diags_d:#?}"
    );

    // ── intersect: +Z ∩ -Z = empty (disjoint) ────────────────────────────────
    let sv_i = selector_cell(&result, "BT3SetOps", "i");
    assert_eq!(sv_i.kind, SelectorKind::Face, "BT3: i must be Selector(Face)");

    let mut staged_i = staged_box_kernel();
    let mut diags_i: Vec<Diagnostic> = Vec::new();
    let handles_i = topology_selectors::resolve(sv_i, &mut staged_i, &mut diags_i)
        .expect("BT3: resolve() on intersect must not return a QueryError");

    assert!(
        handles_i.is_empty(),
        "BT3: intersect(+Z-face, -Z-face) must be empty (disjoint face sets), \
         got: {handles_i:?}"
    );
    assert!(
        diags_i.is_empty(),
        "BT3: intersect resolve() must emit no diagnostics, got: {diags_i:#?}"
    );
}

// ── BT2: same-kind union resolves to set-union with K3 dedup ─────────────────

/// BT2 (producer / resolve): `union(faces_by_normal(b,+Z,1deg), faces_by_normal(b,-Z,1deg))`
/// resolves to `[+Z face, -Z face]` in canonical first-seen order with K3
/// dedup (no duplicates). No diagnostics emitted.
///
/// PRD §5 BT2: "same-kind union resolves to set-union — K3 dedup, canonical order".
///
/// Strategy: build the fixture with an unstaged kernel (kernel-free, giving
/// `Value::Selector(Face)` Union cell), then call `topology_selectors::resolve`
/// against `staged_box_kernel()` (always-on, no OCCT). The staged kernel has
/// `extract_faces(GHId(1)) → [2..7]` and per-face normals; `ByNormal{+Z,1°}`
/// selects face 6 and `ByNormal{-Z,1°}` selects face 7.
///
/// RED when fixture `bt2_same_kind_union.ri` is absent.
#[test]
fn bt2_same_kind_union_resolves_to_set_union() {
    let source = std::fs::read_to_string(fixture_path("bt2_same_kind_union.ri")).expect(
        "fixture bt2_same_kind_union.ri must exist (create in step-8 to turn GREEN)",
    );

    // Kernel-free build: obtain the Union Value::Selector(Face) cell.
    let result = build_with_unstaged_kernel(&source);
    let sv = selector_cell(&result, "BT2SameKindUnion", "u");
    assert_eq!(sv.kind, SelectorKind::Face, "BT2: u must be Selector(Face)");

    // The Union must have two children (one per faces_by_normal call).
    match &sv.node {
        SelectorNode::Union(children) => {
            assert_eq!(children.len(), 2, "BT2: union must have 2 children");
        }
        other => panic!("BT2: u must be a Union selector, got: {other:?}"),
    }

    // Call resolve() against the staged box kernel (always-on, no OCCT).
    let mut staged = staged_box_kernel();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let handles = topology_selectors::resolve(sv, &mut staged, &mut diags)
        .expect("BT2: resolve() must not return a QueryError");

    // (a) resolves to [+Z face (id=6), -Z face (id=7)] — canonical first-seen
    //     order, K3 dedup (no overlapping faces here, so the list is 2 items).
    assert_eq!(
        handles,
        vec![GeometryHandleId(6), GeometryHandleId(7)],
        "BT2: union(+Z, -Z) must resolve to [face6(+Z), face7(-Z)] in canonical order"
    );

    // (b) no diagnostics
    assert!(
        diags.is_empty(),
        "BT2: resolve() must emit no diagnostics for a clean same-kind union, got: {diags:#?}"
    );
}

// ── BT7: construction is kernel-free (K2 — zero query() round-trips) ─────────

/// BT7 (producer / K2 invariant): building a bare, unconsumed
/// `let sel = faces_by_normal(b, dir, tol)` must issue ZERO kernel `query()`
/// round-trips. The selector cell must hold a typed `Value::Selector(Face)`
/// with a `ByNormal{+Z, tol>0}` Leaf whose `target.kernel_handle` is the
/// box's realized handle.
///
/// PRD §5 BT7: "construction is kernel-free — K2 invariant".
///
/// Uses `CountingMockKernel` to intercept every `query()` call. `execute()`
/// is NOT counted, so the box realization (which calls `execute()`) does not
/// inflate the counter. Only `query()` (predicate fns like `faces_by_normal`,
/// `extract_faces`, face-normal queries) is counted.
///
/// RED when fixture `bt7_kernel_free_construction.ri` is absent.
#[test]
fn bt7_construction_is_kernel_free() {
    let source = std::fs::read_to_string(fixture_path("bt7_kernel_free_construction.ri")).expect(
        "fixture bt7_kernel_free_construction.ri must exist (create in step-6 to turn GREEN)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);

    // Wrap MockGeometryKernel in CountingMockKernel; capture the Arc<QueryCounts>
    // BEFORE moving the kernel into Engine::new (Arc survives the move).
    let counting_kernel = CountingMockKernel::new(MockGeometryKernel::new());
    let counts = counting_kernel.counts();

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> = Box::new(counting_kernel);
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) `sel` = Value::Selector(Face) with ByNormal{+Z, tol>0} Leaf
    let sv = selector_cell(&result, "BT7KernelFree", "sel");
    assert_eq!(
        sv.kind,
        SelectorKind::Face,
        "BT7: BT7KernelFree.sel must be Value::Selector(Face)"
    );
    match &sv.node {
        SelectorNode::Leaf {
            query: LeafQuery::ByNormal { dir, tol_rad },
            target,
        } => {
            assert_eq!(
                *dir, [0.0, 0.0, 1.0],
                "BT7: ByNormal dir must be +Z (0,0,1)"
            );
            assert!(
                *tol_rad > 0.0,
                "BT7: ByNormal tol_rad must be positive (1deg converted to rad), got {tol_rad}"
            );
            // The box is the first `execute()` call → GeometryHandleId(1).
            assert_eq!(
                target.kernel_handle,
                GeometryHandleId(1),
                "BT7: leaf target.kernel_handle must be the box handle GeometryHandleId(1)"
            );
        }
        other => panic!(
            "BT7: BT7KernelFree.sel must be a SelectorNode::Leaf with ByNormal query, got: {other:?}"
        ),
    }

    // (b) ZERO kernel query() round-trips during construction (K2/BT7).
    //     execute() (for box realization) is NOT counted — only query().
    assert_eq!(
        counts.total(),
        0,
        "BT7: selector construction must issue ZERO kernel query() calls (K2 invariant); \
         if > 0, the selector was resolved eagerly instead of staying lazy"
    );
}

// ── BT6: kind-typed param rejects wrong selector at compile time ──────────────

/// BT6 (consumer / kind-typed param): `needs_face(edges(b))` — passing an
/// `EdgeSelector` where a `FaceSelector` is expected — must be rejected at
/// compile time with exactly ONE Error-severity diagnostic whose message names
/// both "FaceSelector" and "EdgeSelector".
///
/// PRD §5 BT6: "kind-typed param rejects wrong selector at compile time".
///
/// **Ratified divergence (esc-4120-17):** PRD says `E_SELECTOR_KIND_MISMATCH`.
/// Empirically: the error carries `code = None` ("no matching overload for
/// needs_face(EdgeSelector), candidates: needs_face(FaceSelector)").
/// `SelectorKindMismatch` is composition-ONLY. This test asserts the ACTUAL
/// behavior (overload-mismatch, `code = None`, message names both kinds).
///
/// The fixture also contains a positive-control structure (`BT6Accept` with
/// `needs_face(faces(b))`) which must compile with no additional errors —
/// verified by asserting exactly ONE error total (from `BT6Reject`).
///
/// RED when fixture `bt6_kind_typed_param.ri` is absent (`.expect()` panics).
#[test]
fn bt6_kind_typed_param_rejects_wrong_kind() {
    let source = std::fs::read_to_string(fixture_path("bt6_kind_typed_param.ri")).expect(
        "fixture bt6_kind_typed_param.ri must exist (create it in step-4 to turn GREEN)",
    );

    // Compile via compile_source_with_stdlib — we expect errors.
    let compiled = compile_source_with_stdlib(&source);
    let errors: Vec<&Diagnostic> = errors_only(&compiled);

    // (a) exactly ONE error-severity diagnostic
    //     (BT6Accept positive-control contributes zero errors implicitly)
    assert_eq!(
        errors.len(),
        1,
        "BT6: expected exactly 1 overload-mismatch error (from BT6Reject), \
         got {} errors:\n{:#?}",
        errors.len(),
        errors
    );

    let err = errors[0];

    // (b) code = None (NOT SelectorKindMismatch — that is composition-only;
    //     param-binding goes through the overload-resolution NoMatch arm).
    assert_eq!(
        err.code, None,
        "BT6: param-binding error must have code = None (esc-4120-17), got: {:?}",
        err.code
    );

    // (c) message names BOTH the expected kind (FaceSelector) and the found
    //     kind (EdgeSelector) so the user can diagnose the mismatch.
    assert!(
        err.message.contains("FaceSelector"),
        "BT6: error message must name FaceSelector (expected kind), got: {:?}",
        err.message
    );
    assert!(
        err.message.contains("EdgeSelector"),
        "BT6: error message must name EdgeSelector (found kind), got: {:?}",
        err.message
    );
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
