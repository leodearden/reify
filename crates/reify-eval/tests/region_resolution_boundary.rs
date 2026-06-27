//! Two-way region-resolution boundary test (P0γ, task #4813).
//!
//! Pins the PRD §6 region-resolution boundary matrix from
//! `docs/prds/naming-convergence/P0-region-reference-layer-model.md`:
//! the producer side (reify-eval → kernels, §6.1) and the consumer side
//! (call sites → region reference, §6.2).
//!
//! All production behavior is already landed by the prerequisites (α #4811,
//! β #4812) and no new production code is introduced here — the tests verify
//! the landed surface.
//!
//! ## Producer coverage (§6.1)
//!
//! | Row | Fixture                           | Gate                   | Assertion            |
//! |-----|-----------------------------------|------------------------|----------------------|
//! | P1  | SELECTOR_BOX_SRC / OCCT           | BRepAndMesh → Occt     | no QNS + non-Undef   |
//! | P2  | SELECTOR_BOX_SRC / Manifold Mesh  | BRepAndMesh → Manifold | no QNS + non-Undef   |
//! | P3  | BY_ROLE_OVER_MESH_SRC / Manifold  | BRepOnly → Unsupported | 1 QNS + Undef        |
//! | P4  | #[cfg(has_gmsh)] VOLUME_MESH_GATE_SRC | any → Unsupported | 1 QNS + Undef        |
//! | P5  | β's crate-internal ::tests        | (cross-ref, see below) | (see P5 note)        |
//! | P6  | SELECTOR_BOX_SRC / kernel-free    | eval, no kernel        | stable content_hash  |
//!
//! **P5 note (Sdf/Voxel):** Current main has no clean public seam to demand
//! Sdf (Fidget) or Voxel (OpenVDB) realization from an external test binary.
//! PRD §6's intro explicitly sanctions crate-internal `::tests` for reprs
//! without an external realization-demand seam.  The Sdf/Voxel single-repr
//! fail-closed coverage lives in:
//!   - `crates/reify-eval/src/geometry_ops.rs` →
//!     `gate_closed_faces_all_over_sdf_yields_undef_and_qns_error`
//!   - `crates/reify-eval/src/geometry_ops.rs` →
//!     `gate_closed_faces_all_over_voxel_yields_undef_and_qns_error`
//!
//! ## Consumer coverage (§6.2)
//!
//! | Row | Fixture                  | Assertion                              |
//! |-----|--------------------------|----------------------------------------|
//! | C1  | KIND_MISMATCH_EDGE_SRC   | compile-time SelectorKindMismatch      |
//! | C2  | POINT_CONSUMER_SRC       | Value::Frame, kernel-free eval         |
//! | C3  | KIND_MISMATCH_BODY_SRC   | compile-time SelectorKindMismatch      |
//! | C4  | (doc only, P4 seam)      | see FEA-target contract below          |
//!
//! ## FEA-target contract (C4, P4 seam — documented here, implemented in P4)
//!
//! `validate_selector_target` (`reify-stdlib/src/helpers.rs:214`) currently
//! rejects `Value::Selector` and `Value::Frame` — it accepts only
//! `Value::Map` / `Value::String`.  P4 flips it to accept `RegionRef`
//! (= `pub type RegionRef = SelectorValue;`, task #4811).
//!
//! The contract P4 must satisfy:
//! - A 2-manifold (`FaceSelector`) `RegionRef` is accepted as an FEA
//!   `face: target`.
//! - A 3-manifold (`BodySelector`) ref passed where a `FaceSelector` is
//!   expected is a construct-time `SelectorKindMismatch` (C3 above).
//! - A pose (`Value::Frame`) and a region-set (`RegionRef`) are DISTINCT
//!   target categories (PRD §4 invariant 4, D1).
//!
//! No live assertion here — asserting P4's acceptance capability would be a
//! doomed dependency-capability RED (P4 has not landed).  P4 tracks
//! separately.
//!
//! ## Dead-strip discipline
//!
//! A kernel rlib is only linked into the test binary when the binary
//! references one of its symbols (the dead-strip invariant documented in
//! `crates/reify-eval/Cargo.toml`).
//!
//! - **Manifold** — always-on (unconditional `inventory::submit!` in
//!   `register.rs`).  Anchored by calling
//!   `reify_kernel_manifold::register::manifold_capability_descriptor()`
//!   inside `build_manifold_stl()`, the first Manifold-using helper (mirrors
//!   `manifold_cross_kernel_real.rs:66`).
//! - **Gmsh** — `#[cfg(has_gmsh)]` gated.  Anchored by the module-level
//!   `extern crate reify_kernel_gmsh as _` below (mirrors
//!   `volume_mesh_realization_e2e.rs:26`).
//!
//! These anchors are ISOLATED to this binary and MUST NOT appear in other
//! reify-eval integration-test binaries — doing so would contaminate those
//! binaries' kernel registries and break OCCT-only registry-size assertions.

// ── Linker anchors ────────────────────────────────────────────────────────────

// Gmsh linker anchor — see module doc.  Manifold is anchored via
// build_manifold_stl() calling manifold_capability_descriptor().
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

// ── Imports ───────────────────────────────────────────────────────────────────

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::diagnostics::Diagnostic;
use reify_core::{DiagnosticCode, Severity};
use reify_core::identity::ValueCellId;
use reify_eval::{BuildResult, Engine, EvalResult};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect all `Severity::Error` diagnostics from a compiled module.
fn compile_errors(compiled: &CompiledModule) -> Vec<&Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Collect all `code == QueryNotSupportedOnRepr && severity == Error`
/// diagnostics from a build result — the "fail-closed" signal (β gate, #4812).
fn qns_errors(build: &BuildResult) -> Vec<&Diagnostic> {
    build
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::QueryNotSupportedOnRepr)
                && d.severity == Severity::Error
        })
        .collect()
}

/// Assert that the cell `(entity, member)` in `build.values` is `Value::Undef`.
///
/// Panics with a descriptive message if the cell is absent or holds a different
/// value — both indicate a regression in the fail-closed gate contract.
fn assert_cell_undef(build: &BuildResult, entity: &str, member: &str) {
    let cell_id = ValueCellId::new(entity, member);
    match build.values.get(&cell_id) {
        Some(Value::Undef) => {}
        Some(other) => panic!(
            "{entity}.{member}: expected Value::Undef (fail-closed gate), got: {other:?}"
        ),
        None => panic!(
            "{entity}.{member}: cell absent from build result; \
             expected Value::Undef (fail-closed gate must produce Undef, not silently omit)"
        ),
    }
}

// ── Engine builders ───────────────────────────────────────────────────────────

/// Build `compiled` against a real OCCT kernel (`ExportFormat::Step`).
///
/// The caller MUST guard with `if !reify_kernel_occt::OCCT_AVAILABLE { return; }`
/// before calling this.
fn build_occt(compiled: &CompiledModule) -> BuildResult {
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(kernel)),
    );
    engine.build(compiled, ExportFormat::Step)
}

/// Build `compiled` against Manifold (`ExportFormat::Stl`) using all
/// inventory-registered kernels (OCCT for BRep primitives + Manifold for Mesh).
///
/// Includes the Manifold linker anchor (dead-strip invariant — see module doc):
/// calling `manifold_capability_descriptor()` from this helper ensures the
/// linker retains the Manifold rlib for every test binary that calls this fn.
///
/// The caller MUST guard with `if !reify_kernel_occt::OCCT_AVAILABLE { return; }`
/// because BRep primitive realization (e.g. `box(...)`) requires OCCT even when
/// the demanded output repr is Mesh.
fn build_manifold_stl(compiled: &CompiledModule) -> BuildResult {
    // Linker anchor: forces the Manifold rlib into this binary so
    // inventory::submit! fires and the "manifold" registry entry is present.
    let _anchor = reify_kernel_manifold::register::manifold_capability_descriptor();
    assert!(
        !_anchor.supports.is_empty(),
        "manifold_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty, Manifold registration is broken)"
    );
    let mut engine = Engine::with_registered_kernels(Box::new(SimpleConstraintChecker));
    engine.build(compiled, ExportFormat::Stl)
}

/// Evaluate `compiled` kernel-free (no geometry kernel, symbolic result).
fn eval_kernel_free(compiled: &CompiledModule) -> EvalResult {
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.eval(compiled)
}

// ── §6.1 Producer rows ────────────────────────────────────────────────────────

// Source consts — step-2 fills these in with real let-bound box + dir/tol +
// faces_by_normal fixture (mirroring symbolic_selector_eval.rs WIDGET_SRC).
// Empty stubs cause the step-1 tests to RED: build.values.get(&cell_id) returns
// None → .expect("cell must be present") panics → test fails as intended.

/// Inline fixture: a let-bound box + let-bound dir/tol + faces_by_normal selector.
///
/// Mirrors `symbolic_selector_eval.rs::WIDGET_SRC`.  Let-bound `dir` and `tol`
/// avoid the out-of-scope inline-arg dispatcher gap (PRD §5): inline
/// `vec3(0,0,1)` / `1deg` args in `faces_by_normal` would need the eval-path
/// dispatcher to evaluate inline function-call args, which is not part of R2b
/// scope.  The `let`-bound form pre-resolves them into `values` before the
/// selector-mint pass runs.
///
/// The `Widget.top` cell is a `Value::Selector(Face)` after build — used by
/// the P1/P2 and P6 rows to assert non-Undef and content-hash stability.
const SELECTOR_BOX_SRC: &str = r#"structure def Widget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let top = faces_by_normal(body, dir, tol)
}"#;

// ── P1/P2: Predicate selector resolves over BRep and Mesh ────────────────────

/// P1 (§6.1 row 1): `faces_by_normal` over an OCCT BRep-realized body must
/// resolve without a `QueryNotSupportedOnRepr` error and leave the selector
/// cell non-Undef (`BRepAndMesh` capability → Occt route → supported).
///
/// **RED (step-1):** `SELECTOR_BOX_SRC` is an empty stub; `build.values.get(&cell_id)`
/// returns `None` → `.expect()` panics → fails as intended.
/// **GREEN (step-2):** real source constant added.
#[test]
fn predicate_resolves_brep_faces_by_normal() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "SKIP predicate_resolves_brep_faces_by_normal: \
             OCCT not available (stub-mode build)"
        );
        return;
    }
    let compiled = parse_and_compile_with_stdlib(SELECTOR_BOX_SRC);
    let build = build_occt(&compiled);

    // (a) No QueryNotSupportedOnRepr Error — BRepAndMesh cap is supported over BRep.
    let qns = qns_errors(&build);
    assert!(
        qns.is_empty(),
        "P1: no QNS errors expected for BRepAndMesh predicate over OCCT BRep; got: {qns:?}"
    );

    // (b) Selector cell exists and is non-Undef.
    let cell_id = ValueCellId::new("Widget", "top");
    let val = build
        .values
        .get(&cell_id)
        .expect("P1: Widget.top cell must be present (RED until step-2 adds SELECTOR_BOX_SRC)");
    assert!(
        !matches!(val, Value::Undef),
        "P1: Widget.top must not be Value::Undef for BRepAndMesh predicate over OCCT BRep; got: {val:?}"
    );
}

/// P2 (§6.1 row 2): `faces_by_normal` over a Manifold Mesh-realized body must
/// resolve without a `QueryNotSupportedOnRepr` error and leave the selector
/// cell non-Undef (`BRepAndMesh` capability → Manifold route → supported).
///
/// **RED (step-1):** `SELECTOR_BOX_SRC` is an empty stub → same failure as P1.
/// **GREEN (step-2):** real source constant + Manifold engine added.
#[test]
fn predicate_resolves_mesh_faces_by_normal() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "SKIP predicate_resolves_mesh_faces_by_normal: \
             OCCT not available (BRep primitive realization requires OCCT even for Mesh builds)"
        );
        return;
    }
    let compiled = parse_and_compile_with_stdlib(SELECTOR_BOX_SRC);
    let build = build_manifold_stl(&compiled);

    // (a) No QueryNotSupportedOnRepr Error — BRepAndMesh cap is supported over Mesh.
    let qns = qns_errors(&build);
    assert!(
        qns.is_empty(),
        "P2: no QNS errors expected for BRepAndMesh predicate over Manifold Mesh; got: {qns:?}"
    );

    // (b) Selector cell exists and is non-Undef.
    let cell_id = ValueCellId::new("Widget", "top");
    let val = build
        .values
        .get(&cell_id)
        .expect("P2: Widget.top cell must be present (RED until step-2 adds SELECTOR_BOX_SRC)");
    assert!(
        !matches!(val, Value::Undef),
        "P2: Widget.top must not be Value::Undef for BRepAndMesh predicate over Manifold Mesh; got: {val:?}"
    );
}
