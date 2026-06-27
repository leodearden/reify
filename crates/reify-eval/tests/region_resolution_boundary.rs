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
use reify_test_support::{MockGeometryKernel, compile_source_with_stdlib, parse_and_compile_with_stdlib};

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

// ── §6.1 Producer fail-closed rows ────────────────────────────────────────────

// Source consts — step-4 fills these in.
// Empty stubs ⇒ RED for step-3 tests: parse_and_compile_with_stdlib panics on "".

/// Inline fixture for the ByRole-over-Mesh fail-closed test (P3).
///
/// **Mesh production via Manifold union (gate-visibility mechanism):**
///
/// The fail-closed gate (`resolve_selector_to_list`) fires only when the body's
/// `produced_repr` is `Mesh` in the `realized_reprs` snapshot.  There are TWO
/// relevant snapshots:
///
/// 1. **PRE-LOOP snapshot** — taken before the template's step loop; used by
///    `hydrate_value_cell_in_loop` during in-loop `HydrateCell` steps.  This
///    snapshot is EMPTY for the first (and only) template; the gate is
///    fail-open here.
/// 2. **POST-LOOP snapshot** — taken AFTER all `Realize` steps in the loop;
///    used by `run_post_processes` → `post_process_topology_selectors`.  This
///    snapshot includes reprs written by Realize steps within the loop.
///
/// A simple `box(...)` primitive produces `produced_repr = BRep` even with
/// `ExportFormat::Stl` — the Stl export only affects tessellation-at-export,
/// NOT the realization-graph entry.  So a box body would produce `BRep` in
/// the post-loop snapshot, routing ByRole (BRepOnly) to Occt (not Unsupported).
///
/// A `union(box_a, box_b)` with named intermediates and `ExportFormat::Stl`
/// IS demanded as `Mesh` by `compute_demanded_reprs` (terminal realization).
/// The Manifold kernel fulfils the union via the OCCT BRep→Manifold Mesh
/// cross-kernel tessellation path, setting `produced_repr = Mesh` in eval_state.
/// The post-loop snapshot then includes `body`'s Mesh repr, and the gate fires:
///
///   `region_query_capability(ByRole(MidSurfaceFace))` = `Some(BRepOnly)` →
///   `realized_reprs[body.realization_ref]` = `Mesh` →
///   `route_capability(BRepOnly, Mesh)` = `Unsupported` →
///   exactly one QNS Error + `Value::Undef` for `Fail.m`.
///
/// Named intermediate bindings (`box_a`, `box_b_raw`, `box_b`) are REQUIRED so
/// `compute_demanded_reprs` can resolve the union's `GeomRef::Sub` entries via
/// `name_to_idx` (see `manifold_boolean.ri` and `manifold_cross_kernel_real.rs`).
/// Without named intermediates the conservative BRep path is taken and `body`
/// ends up as BRep in the snapshot.
///
/// `mid_surface(body)` returns `Selector(Face)` via `LeafQuery::ByRole(MidSurfaceFace)`.
/// The union body has no MidSurfaceFace entries (those come from shell-extract),
/// so even on BRep the resolution returns empty — but the BRepOnly gate fires
/// BEFORE the kernel query on the Mesh path, producing the structured QNS
/// diagnostic rather than a silent empty list.
const BY_ROLE_OVER_MESH_SRC: &str = r#"structure def Fail {
    let box_a     = box(10mm, 10mm, 10mm)
    let box_b_raw = box(10mm, 10mm, 10mm)
    let box_b     = translate(box_b_raw, 5mm, 0mm, 0mm)
    let body      = union(box_a, box_b)
    let m         = single(mid_surface(body))
}"#;

/// Inline fixture for the VolumeMesh fail-closed test (#[cfg(has_gmsh)]).
///
/// `@optimized("test::region-gate-probe")` on `gate_probe` creates a ComputeNode
/// whose target name `engine.register_volume_mesh_demand("test::region-gate-probe")`
/// recognises.  The module-static demand pass overrides `body`'s demanded repr to
/// `VolumeMesh`; `ensure_gmsh_kernel()` provides the Gmsh adapter.
///
/// After the VolumeMesh realization, `single(faces_by_normal(body, dir, tol))`
/// triggers `resolve_selector_to_list`:
///   - `region_query_capability(ByNormal) = Some(BRepAndMesh)`
///   - `realized_reprs[body] = VolumeMesh`
///   - `route_capability(BRepAndMesh, VolumeMesh) = Unsupported`
///     → exactly one QNS Error + `Value::Undef` for `GateFail.top`.
///
/// The cross-reference to `gate_closed_faces_all_over_volume_mesh_yields_undef_and_qns_error`
/// in `geometry_ops.rs` (β's internal coverage) is noted in the module doc P5 entry.
#[cfg(has_gmsh)]
const VOLUME_MESH_GATE_SRC: &str = r#"@optimized("test::region-gate-probe")
fn gate_probe(g: Geometry) -> Int {
    0
}

structure def GateFail {
    let body = box(10mm, 10mm, 10mm)
    let dir  = vec3(0.0, 0.0, 1.0)
    let tol  = 1deg
    let top  = single(faces_by_normal(body, dir, tol))
    let probe = gate_probe(body)
}"#;

// ── P3: ByRole-over-Mesh fail-closed (always-available) ──────────────────────

/// P3 (§6.1 row 3): `mid_surface(body)` (ByRole → BRepOnly capability) consumed
/// via `single()` over a Manifold Mesh-realized body must produce EXACTLY ONE
/// `QueryNotSupportedOnRepr` Error diagnostic and leave the result cell as
/// `Value::Undef`.
///
/// This is the ALWAYS-AVAILABLE fail-closed signal: it requires only the
/// Manifold rlib (always-on, no #[cfg] gate) and OCCT for BRep primitive
/// realization — no native off-BRep geometry solver needed.
///
/// Gate path: `resolve_selector_to_list` → `region_query_capability(ByRole)` =
/// `Some(BRepOnly)` → `route_capability(BRepOnly, Mesh)` = `Unsupported` →
/// pushes QNS Error, returns `Value::Undef`.
///
/// The fixture uses two structures (`ByRoleBody` + `Fail`) so the body's Mesh
/// `produced_repr` is in `eval_state` before `Fail`'s step-loop snapshot (see
/// `BY_ROLE_OVER_MESH_SRC` doc).
///
/// **RED (step-3):** `BY_ROLE_OVER_MESH_SRC` is empty → panics at compile.
/// **GREEN (step-4/6):** real multi-template source + engine wiring added.
#[test]
fn fail_closed_byrole_over_mesh_produces_qns_error_and_undef() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "SKIP fail_closed_byrole_over_mesh: \
             OCCT not available (BRep primitive realization requires OCCT)"
        );
        return;
    }
    let compiled = parse_and_compile_with_stdlib(BY_ROLE_OVER_MESH_SRC);
    let build = build_manifold_stl(&compiled);

    // (a) Exactly ONE QueryNotSupportedOnRepr Error (filtered — the build may
    //     also emit unrelated warnings/info).
    let qns = qns_errors(&build);
    assert_eq!(
        qns.len(),
        1,
        "P3: expected exactly 1 QueryNotSupportedOnRepr Error (ByRole BRepOnly → Mesh → Unsupported); \
         got {} QNS errors: {qns:?}",
        qns.len()
    );

    // (b) Result cell is Value::Undef (no panic, no silent empty list).
    assert_cell_undef(&build, "Fail", "m");
}

// ── P4: VolumeMesh fail-closed (#[cfg(has_gmsh)]) ────────────────────────────

/// P4 (§6.1 row 4): a predicate selector (`faces_by_normal`, BRepAndMesh
/// capability) consumed over a Gmsh VolumeMesh-realized body must produce
/// EXACTLY ONE `QueryNotSupportedOnRepr` Error and leave the result cell as
/// `Value::Undef`.
///
/// Gate path: `route_capability(BRepAndMesh, VolumeMesh)` = `Unsupported`
/// (geometry_ops.rs:140: `ReprKind::VolumeMesh => unsupported(diagnostics)`).
///
/// **RED (step-3):** `VOLUME_MESH_GATE_SRC` is empty → panics at compile.
/// **GREEN (step-4):** real source + engine demand wiring added.
#[cfg(has_gmsh)]
#[test]
fn fail_closed_predicate_over_volume_mesh_produces_qns_error_and_undef() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "SKIP fail_closed_predicate_over_volume_mesh: \
             OCCT not available (BRep primitive realization requires OCCT)"
        );
        return;
    }
    let compiled = parse_and_compile_with_stdlib(VOLUME_MESH_GATE_SRC);

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(kernel)),
    );
    engine.register_volume_mesh_demand("test::region-gate-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "P4: ensure_gmsh_kernel() must acquire the gmsh adapter (gmsh rlib anchored by module-level extern crate)"
    );
    let build = engine.build(&compiled, ExportFormat::Step);

    // (a) Exactly ONE QueryNotSupportedOnRepr Error.
    let qns = qns_errors(&build);
    assert_eq!(
        qns.len(),
        1,
        "P4: expected exactly 1 QueryNotSupportedOnRepr Error (BRepAndMesh → VolumeMesh → Unsupported); \
         got {} QNS errors: {qns:?}",
        qns.len()
    );

    // (b) Result cell is Value::Undef.
    assert_cell_undef(&build, "GateFail", "top");
}

// ── §6.1 Producer content-hash stability row ──────────────────────────────────

/// Inline fixture for the content-hash stability tests (P6a/P6b).
///
/// Let-bound `dir` and `tol` avoid the inline-arg dispatcher gap (PRD §5):
/// inline `vec3(0,0,1)` / `1deg` args in `faces_by_normal` would need the
/// eval-path dispatcher to evaluate inline function-call args, which is not
/// part of R2b scope.  The `let`-bound form pre-resolves them into `values`
/// before the selector-mint pass runs.
///
/// `Widget.top` is a `Value::Selector(Face)` after eval/build — used by both
/// the cross-run stability (P6a) and the eval-vs-build equality (P6b) rows.
const CONTENT_HASH_SRC: &str = r#"structure def Widget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let top = faces_by_normal(body, dir, tol)
}"#;

// ── P6a: Cross-run content-hash stability ────────────────────────────────────

/// P6a (§6.1 row 6, part 1): two independent kernel-free `Engine::eval` runs on
/// the same compiled source must yield byte-identical `content_hash` for the
/// selector cell.
///
/// Pins PRD §4 invariant 1 (re-eval-stable naming): the content address of a
/// symbolic selector is determined by the selector's description (target entity,
/// `LeafQuery` variant, arguments) and the content address of its upstream values
/// — NOT by any mutable kernel handle or run-specific state.
///
/// `kernel_handle` is excluded from the hash via `hash_ghr` (α #4811,
/// `reify-ir/src/value.rs` `content_hash` impl).
///
/// **RED (step-5):** `CONTENT_HASH_SRC` is empty → `parse_and_compile_with_stdlib`
///   panics → test fails as intended.
/// **GREEN (step-6):** real source filled in.
#[test]
fn selector_content_hash_is_cross_run_stable() {
    let compiled = compile_source_with_stdlib(CONTENT_HASH_SRC);
    let cell_id = ValueCellId::new("Widget", "top");

    // Run 1 — fresh Engine.
    let hash1 = {
        let result = eval_kernel_free(&compiled);
        let val = result.values.get_or_undef(&cell_id);
        match &val {
            Value::Selector(_) => val.content_hash(),
            other => panic!(
                "P6a run1: expected Value::Selector for Widget.top, got: {other:?}"
            ),
        }
    };

    // Run 2 — separate Engine instance, same compiled module.
    let hash2 = {
        let result = eval_kernel_free(&compiled);
        let val = result.values.get_or_undef(&cell_id);
        match &val {
            Value::Selector(_) => val.content_hash(),
            other => panic!(
                "P6a run2: expected Value::Selector for Widget.top, got: {other:?}"
            ),
        }
    };

    assert_eq!(
        hash1,
        hash2,
        "P6a: content_hash must be byte-identical across independent Engine::eval runs \
         (PRD §4 invariant 1 — re-eval-stable naming; kernel_handle excluded via hash_ghr)"
    );
}

// ── P6b: Eval-vs-build content-hash equality ─────────────────────────────────

/// P6b (§6.1 row 6, part 2): `Engine::eval` (symbolic, no kernel) and
/// `Engine::build` (MockGeometryKernel-realized) on the same compiled source
/// must produce `content_hash`-equal `Value::Selector` cells for `Widget.top`.
///
/// Pins PRD §4 invariant 1 + α #4811 DD-6: `SelectorValue.content_hash()`
/// excludes `kernel_handle` (computed via `hash_ghr`), so symbolic and realized
/// selectors are hash-identical when their `LeafQuery` and upstream values match.
///
/// Uses `MockGeometryKernel` (from `reify-test-support`) as the build kernel —
/// it registers a geometry handle for the body primitive but does not alter the
/// selector's content address.
#[test]
fn selector_eval_vs_build_content_hash_equal() {
    let compiled = compile_source_with_stdlib(CONTENT_HASH_SRC);
    let cell_id = ValueCellId::new("Widget", "top");

    // Path A: pure eval (no kernel) — symbolic selector.
    let eval_hash = {
        let result = eval_kernel_free(&compiled);
        let val = result.values.get_or_undef(&cell_id);
        match &val {
            Value::Selector(_) => val.content_hash(),
            other => panic!(
                "P6b eval: expected Value::Selector for Widget.top, got: {other:?}"
            ),
        }
    };

    // Path B: build with MockGeometryKernel — realized selector.
    let build_hash = {
        let kernel = MockGeometryKernel::new();
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(kernel)),
        );
        let result = engine.build(&compiled, ExportFormat::Step);
        let build_errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.clone())
            .collect();
        assert!(
            build_errors.is_empty(),
            "P6b: build must succeed with MockGeometryKernel; got: {build_errors:?}"
        );
        let val = result.values.get_or_undef(&cell_id);
        match &val {
            Value::Selector(_) => val.content_hash(),
            other => panic!(
                "P6b build: expected Value::Selector for Widget.top, got: {other:?}"
            ),
        }
    };

    assert_eq!(
        eval_hash,
        build_hash,
        "P6b: content_hash must be equal between symbolic (eval) and realized (build) selectors \
         (α #4811 DD-6: kernel_handle excluded from SelectorValue.content_hash via hash_ghr)"
    );
}

// ── §6.2 Consumer rows ────────────────────────────────────────────────────────

// Source consts — step-8 fills POINT_CONSUMER_SRC in.
// Empty stub ⇒ RED for step-7 test: eval returns no cells →
// get_or_undef returns Value::Undef → matches!(Undef, Value::Frame) = false.

/// Inline fixture for the C2 `@point → Value::Frame` consumer test.
///
/// `PoseTest.location = body @ point(5mm, 5mm, 5mm)` — the `@point` selector
/// uses `body` as the geometry base (a scope variable, valid per the `@` operator
/// grammar) and three literal length coordinates as its args.
///
/// Layer-1 eval_expr resolves `SelectorKind::Point` directly from the coordinate
/// args (kernel-free); the base (`body`) is not queried.  `location` is a
/// `Value::Frame` with origin at (5mm, 5mm, 5mm) and identity basis.
const POINT_CONSUMER_SRC: &str = r#"structure def PoseTest {
    param body : Solid = box(10mm, 10mm, 10mm)
    let location = body @ point(5mm, 5mm, 5mm)
}"#;

// ── C2: @point evaluates eagerly to Value::Frame (kernel-free) ────────────────

/// C2 (§6.2 row 2): `body @ point(x, y, z)` evaluates eagerly to `Value::Frame`
/// under a kernel-free `Engine` (no geometry kernel needed — Layer-1 eval_expr
/// handles `SelectorKind::Point` directly from the literal coordinate args).
///
/// Pins the `SelectorKind::Point` arm in `reify-expr/src/lib.rs:1194`: the three
/// length-scalar args are evaluated and assembled into a `Value::Frame` with
/// identity basis, without touching `geometry_ops` or any kernel handle.
///
/// Note: `Value::Frame` (a pose) and `RegionRef` (a selector region-set) are
/// DISTINCT target categories (PRD §4 invariant 4, D1).  This test confirms
/// that the `@point` construct produces the former, NOT the latter.
///
/// **RED (step-7):** `POINT_CONSUMER_SRC` is empty → `compile_source_with_stdlib("")`
///   compiles with no errors but produces no value cells → `get_or_undef` returns
///   `Value::Undef` → `matches!(Undef, Value::Frame { .. })` is false → fails.
/// **GREEN (step-8):** real source constant added.
#[test]
fn point_consumer_evals_to_frame_kernel_free() {
    let compiled = compile_source_with_stdlib(POINT_CONSUMER_SRC);
    let errors = compile_errors(&compiled);
    assert!(
        errors.is_empty(),
        "C2: @point source must compile with no errors; got: {errors:?}"
    );

    let result = eval_kernel_free(&compiled);
    let cell_id = ValueCellId::new("PoseTest", "location");
    let val = result.values.get_or_undef(&cell_id);
    assert!(
        matches!(val, Value::Frame { .. }),
        "C2: `body @ point(x,y,z)` must evaluate to Value::Frame \
         (kernel-free Layer-1 eval_expr; SelectorKind::Point arm); \
         note: Value::Frame (pose) ≠ RegionRef (selector region-set) per PRD §4 invariant 4; \
         got: {val:?}"
    );
}

// ── §6.2 Kind-discipline and dimensionality-rejection rows ───────────────────

// Source consts — step-10 fills these in.
// Empty stubs ⇒ RED for step-9 tests: compile_source_with_stdlib("") produces
// zero error diagnostics → assert_eq!(errors.len(), 1, ...) fails as intended.

/// Inline fixture for C1: passing `EdgeSelector` to a `FaceSelector` param.
///
/// `needs_face(s: FaceSelector)` defines a face-kind parameter; passing
/// `edges(b)` (an `EdgeSelector`) triggers `DiagnosticCode::SelectorKindMismatch`
/// at construct/compile time (mirrors `bt6_kind_typed_param.ri` from
/// `selector_boundary_gate.rs`).
const KIND_MISMATCH_EDGE_SRC: &str = r#"fn needs_face(s: FaceSelector) -> Int { 42 }
structure def C1Reject {
    let b = box(10mm, 10mm, 10mm)
    let n = needs_face(edges(b))
}"#;

/// Inline fixture for C3: passing `BodySelector` (3-manifold) to a
/// `FaceSelector` (2-manifold) param — dimensionality rejection.
///
/// `needs_face(s: FaceSelector)` defines a 2-manifold param; passing
/// `solid_body(b, "main")` (a `BodySelector`, 3-manifold) triggers
/// `DiagnosticCode::SelectorKindMismatch` at construct/compile time.
/// `solid_body(geometry, name) -> Selector(Body)` is the Named-leaf BodySelector
/// constructor (`GEOMETRY_TOPOLOGY_SELECTOR_NAMES` in `reify-compiler/src/units.rs`).
const KIND_MISMATCH_BODY_SRC: &str = r#"fn needs_face(s: FaceSelector) -> Int { 42 }
structure def C3Reject {
    let b = box(10mm, 10mm, 10mm)
    let n = needs_face(solid_body(b, "main"))
}"#;

// ── C1: Edge→Face kind mismatch rejected at compile time ─────────────────────

/// C1 (§6.2 row 1): passing an `EdgeSelector` where a `FaceSelector` is
/// expected must be rejected at compile time with exactly ONE
/// `DiagnosticCode::SelectorKindMismatch` error.
///
/// Mirrors `selector_boundary_gate.rs::bt6_kind_typed_param_rejects_wrong_kind`
/// (the PRD §5 BT6 gate) — this row pins the same contract in the §6 consumer
/// boundary matrix.
///
/// **RED (step-9):** `KIND_MISMATCH_EDGE_SRC` is empty → zero error diagnostics
///   → `assert_eq!(errors.len(), 1)` fails.
/// **GREEN (step-10):** inline negative-fixture source added.
#[test]
fn edge_to_face_param_produces_selector_kind_mismatch() {
    let compiled = compile_source_with_stdlib(KIND_MISMATCH_EDGE_SRC);
    let errors = compile_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "C1: expected exactly 1 SelectorKindMismatch error (EdgeSelector → FaceSelector param); \
         got {} errors: {errors:?}",
        errors.len()
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "C1: error must carry DiagnosticCode::SelectorKindMismatch (task #4581); \
         got: {:?}",
        errors[0].code
    );
}

// ── C3: Body→Face dimensionality rejection at compile time ───────────────────

/// C3 (§6.2 row 4): passing a `BodySelector` (3-manifold, `SelectorKind::Body`)
/// where a `FaceSelector` (2-manifold, `SelectorKind::Face`) is expected must be
/// rejected at compile time with exactly ONE
/// `DiagnosticCode::SelectorKindMismatch` error.
///
/// This is the §6.2 dimensionality-rejection row: the compiler enforces manifold
/// dimensionality (SelectorKind) at param-binding time, rejecting Body/Face
/// cross-dimensionality the same way it rejects Edge/Face.
///
/// **RED (step-9):** `KIND_MISMATCH_BODY_SRC` is empty → zero error diagnostics
///   → `assert_eq!(errors.len(), 1)` fails.
/// **GREEN (step-10):** inline negative-fixture source added.
#[test]
fn body_to_face_param_produces_selector_kind_mismatch() {
    let compiled = compile_source_with_stdlib(KIND_MISMATCH_BODY_SRC);
    let errors = compile_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "C3: expected exactly 1 SelectorKindMismatch error (BodySelector → FaceSelector param); \
         got {} errors: {errors:?}",
        errors.len()
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "C3: error must carry DiagnosticCode::SelectorKindMismatch; \
         got: {:?}",
        errors[0].code
    );
}

// ── C4: FEA-target contract (P4 seam — documented here, implemented in P4) ───
//
// This section documents the contract that task P4 (a downstream task) must
// satisfy when it wires `validate_selector_target` to accept `RegionRef`.
//
// CURRENT STATE (before P4):
//   `validate_selector_target` at `reify-stdlib/src/helpers.rs:214` accepts
//   only `Value::Map` / `Value::String` and rejects `Value::Selector` and
//   `Value::Frame`.  A live acceptance assertion would fail today (P4 has not
//   landed), so this section contains NO test — only documentation.
//
// CONTRACT P4 MUST SATISFY:
//   1. A 2-manifold (`FaceSelector`, `SelectorKind::Face`) `RegionRef` is
//      accepted as an FEA `face: target`.
//
//   2. A 3-manifold (`BodySelector`, `SelectorKind::Body`) ref passed where a
//      `FaceSelector` is expected is a CONSTRUCT-TIME `SelectorKindMismatch`
//      (C3 above already pins this — P4 must not regress it).
//
//   3. A pose (`Value::Frame`, from `@point`) and a region-set (`RegionRef`,
//      from selector constructors) are DISTINCT target categories — P4 must
//      accept `Value::Selector(Face)` but reject `Value::Frame` for `face:`.
//      (PRD §4 invariant 4, D1: pose ≠ region-set.)
//
// See also: module doc §6.2 C4 row, `docs/prds/naming-convergence/
// P0-region-reference-layer-model.md` §4 invariant 4 and §6.2.
