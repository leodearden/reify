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

use reify_constraints::SimpleConstraintChecker;
use reify_core::Type;
use reify_core::diagnostics::{Diagnostic, DiagnosticCode};
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::{BuildResult, Engine, topology_selectors};
use reify_ir::value::{LeafQuery, SelectorNode, SelectorValue};
use reify_ir::{CompiledExprKind, ExportFormat, GeometryHandleId, Value};
use reify_test_support::{
    CountingMockKernel, MockGeometryKernel, compile_source_with_stdlib, errors_only,
    parse_and_compile_with_stdlib,
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

// ── BT4: IndexAccess coercion realizes face geometry ─────────────────────────

/// BT4 (consumer / eager coercion / fixture golden via wired IndexAccess shape).
///
/// Fixture: `bt4_index_access_coercion.ri`
/// ```ri
/// structure def BT4IndexAccess {
///     let b  = box(10mm, 10mm, 10mm)
///     let f0 = faces(b)[0]
/// }
/// ```
///
/// **PRD §5 BT4:** "eager coercion realizes the asserted geometry (fixture
/// golden) — delivered through the wired `faces(b)[0]` IndexAccess shape
/// (task-note esc-4118-55 requirement); fillet appears only as a separate
/// BT4b compile-only transparency fixture."
///
/// ## Assertions
///
/// ### Always-on (compile-level)
///
/// 1. The fixture compiles with **zero** error-severity diagnostics.
/// 2. The `BT4IndexAccess.f0` value cell has `cell_type == Type::Geometry`
///    (the coercion is inserted and `sel[i]` is typed `Geometry`, never a
///    selector — task-note invariant, esc-4118-55).
/// 3. Its `default_expr` is an `IndexAccess { object: ResolveSelector { .. },
///    index: _ }` where the `ResolveSelector`'s `result_type` is
///    `List<Geometry>` (the compiler inserts the bridge before the `[0]`
///    subscript rather than at the outer IndexAccess level).
///
/// ### OCCT-gated (reify_kernel_occt::OCCT_AVAILABLE)
///
/// 4. Building with a real `OcctKernelHandle` realizes `f0` to a
///    `Value::GeometryHandle` with a non-zero `upstream_values_hash` (the
///    asserted face — fixture golden, NOT baseline-diff).
///
/// RED when fixture `bt4_index_access_coercion.ri` is absent.
#[test]
fn bt4_index_access_coercion_realizes_face() {
    let source = std::fs::read_to_string(fixture_path("bt4_index_access_coercion.ri")).expect(
        "fixture bt4_index_access_coercion.ri must exist (create in step-14 to turn GREEN)",
    );

    // ── (1) compile with zero errors (always-on) ─────────────────────────────
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "BT4: bt4_index_access_coercion.ri must compile with zero error diagnostics, \
         got:\n{:#?}",
        errors
    );

    // ── (2) & (3) compile-level IR shape (always-on) ─────────────────────────
    // Find the BT4IndexAccess template.
    let tmpl = compiled
        .templates
        .iter()
        .find(|t| t.name == "BT4IndexAccess")
        .expect("BT4: compiled module must contain template BT4IndexAccess");

    // Find the f0 value cell.
    let f0_cell = tmpl
        .value_cells
        .iter()
        .find(|c| c.id.member == "f0")
        .expect("BT4: BT4IndexAccess must have a value cell 'f0'");

    // (2) result type is Geometry (the coercion converts Selector → Geometry
    //     at the IndexAccess level, so f0 is typed Geometry, never a Selector).
    assert_eq!(
        f0_cell.cell_type,
        Type::Geometry,
        "BT4: f0.cell_type must be Type::Geometry (sel[i] is Geometry, never a selector)"
    );

    // (3) default_expr shape: IndexAccess { object: ResolveSelector, .. }
    //     The compiler inserts a ResolveSelector(Selector(Face)) around the
    //     faces(b) sub-expression before it becomes the IndexAccess object,
    //     so the coerced list is List<Geometry> and the element is Geometry.
    let expr = f0_cell
        .default_expr
        .as_ref()
        .expect("BT4: f0 must have a default_expr (it is a let binding)");

    match &expr.kind {
        CompiledExprKind::IndexAccess { object, .. } => {
            // The object must be the ResolveSelector bridge with result_type
            // List<Geometry>.
            match &object.kind {
                CompiledExprKind::ResolveSelector { .. } => {
                    assert_eq!(
                        object.result_type,
                        Type::List(Box::new(Type::Geometry)),
                        "BT4: ResolveSelector object result_type must be List<Geometry>"
                    );
                }
                other => panic!(
                    "BT4: f0 IndexAccess object must be ResolveSelector{{..}}, got: {other:?}"
                ),
            }
        }
        other => panic!(
            "BT4: f0 default_expr must be IndexAccess{{object: ResolveSelector, ..}}, \
             got: {other:?}"
        ),
    }

    // ── (4) OCCT-gated geometry golden ───────────────────────────────────────
    if !occt_available() {
        // Print to stdout so the skip is visible in `cargo test -- --nocapture` and
        // in CI log parsers that scan stdout. The runtime coercion path
        // (IndexAccess → GeometryHandle) is NOT exercised when OCCT is absent;
        // ensure at least one CI lane has OCCT (reify_kernel_occt::OCCT_AVAILABLE).
        println!(
            "WARN BT4: OCCT geometry-golden SKIPPED — OCCT not available on this runner. \
             IndexAccess coercion path (faces(b)[0] → Value::GeometryHandle) is NOT exercised."
        );
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    let f0_id = ValueCellId::new("BT4IndexAccess", "f0");
    let f0_hash = match result.values.get(&f0_id) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => {
            assert_ne!(
                upstream_values_hash, &[0u8; 32],
                "BT4: f0 GeometryHandle upstream_values_hash must be non-zero \
                 (fixture-golden, not baseline-diff; esc-4118-55 IndexAccess coverage)"
            );
            *upstream_values_hash
        }
        other => panic!(
            "BT4: BT4IndexAccess.f0 = faces(b)[0] must realize to Value::GeometryHandle \
             (Selector → List<Geometry> → [0] coercion), got: {other:?}"
        ),
    };

    // Cross-check: build faces(b)[1] and assert it has a DIFFERENT hash than
    // faces(b)[0]. This proves the coercion selected a specific element (index 0),
    // not an arbitrary face — a wrong-index regression would not slip through the
    // non-zero hash check alone.
    let ref_src = r#"
structure def BT4Ref {
    let b  = box(10mm, 10mm, 10mm)
    let f1 = faces(b)[1]
}
"#;
    let ref_compiled = compile_source_with_stdlib(ref_src);
    let ref_kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut ref_engine = Engine::new(Box::new(SimpleConstraintChecker), Some(ref_kernel));
    let ref_result = ref_engine.build(&ref_compiled, ExportFormat::Step);
    let f1_id = ValueCellId::new("BT4Ref", "f1");
    let f1_hash = match ref_result.values.get(&f1_id) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => *upstream_values_hash,
        other => panic!(
            "BT4 cross-check: BT4Ref.f1 = faces(b)[1] must realize to Value::GeometryHandle, \
             got: {other:?}"
        ),
    };
    assert_ne!(
        f0_hash, f1_hash,
        "BT4: faces(b)[0] must have a DIFFERENT upstream_values_hash than faces(b)[1] \
         (proves IndexAccess coercion selected a distinct element; a wrong-index regression \
         would realise a different face and this cross-check would catch it)"
    );
}

// ── BT5: single() coercion golden ────────────────────────────────────────────

/// BT5 (consumer / `single()` coercion golden).
///
/// Fixture: `bt5_single_face_by_normal.ri`
/// ```ri
/// structure def BT5SingleFace {
///     let b   = box(10mm, 10mm, 10mm)
///     let dir = vec3(0.0, 0.0, 1.0)
///     let tol = 1deg
///     let top = single(faces_by_normal(b, dir, tol))
/// }
/// ```
///
/// **PRD §5 BT5:** "`single(<selector>)` coercion — `single(faces_by_normal(b,
/// +Z, 1°))` coerces Selector(Face) → List<Geometry> → single face
/// Value::GeometryHandle (OCCT-gated golden)."
///
/// ## Assertions
///
/// ### Always-on (compile-level)
///
/// 1. The fixture compiles with **zero** error-severity diagnostics
///    (`single` accepts the `Selector(Face)` arg via the β coercion rule —
///    the compiled `single(...)` argument is wrapped in `ResolveSelector` with
///    `result_type == List<Geometry>`).
///
/// ### OCCT-gated (reify_kernel_occt::OCCT_AVAILABLE)
///
/// 2. Building with a real `OcctKernelHandle` realizes `top` to a
///    `Value::GeometryHandle` with a non-zero `upstream_values_hash` (the
///    single +Z face — fixture golden, mirroring `selector_coercion_golden.rs`).
///
/// Mirrors the γ golden in `selector_coercion_golden.rs` using the task-local
/// fixture path and the gate's shared helpers.
///
/// RED when fixture `bt5_single_face_by_normal.ri` is absent.
#[test]
fn bt5_single_face_by_normal_coercion() {
    let source = std::fs::read_to_string(fixture_path("bt5_single_face_by_normal.ri")).expect(
        "fixture bt5_single_face_by_normal.ri must exist (create in step-18 to turn GREEN)",
    );

    // ── (1) compile with zero errors (always-on) ─────────────────────────────
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "BT5: bt5_single_face_by_normal.ri must compile with zero error diagnostics \
         (single() accepts Selector(Face) via the β coercion rule), got:\n{:#?}",
        errors
    );

    // ── (2) OCCT-gated geometry golden ───────────────────────────────────────
    if !occt_available() {
        // Print to stdout so the skip is visible in `cargo test -- --nocapture` and
        // in CI log parsers that scan stdout. The single() coercion path
        // (Selector → List<Geometry> → single face) is NOT exercised when OCCT is
        // absent; ensure at least one CI lane has OCCT (reify_kernel_occt::OCCT_AVAILABLE).
        println!(
            "WARN BT5: OCCT geometry-golden SKIPPED — OCCT not available on this runner. \
             single() coercion path (Selector → List<Geometry> → single face) is NOT exercised."
        );
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // `top = single(faces_by_normal(b, +Z, 1°))` must coerce
    // (Selector → List<Geometry> → single) to the single +Z face handle.
    let top_id = ValueCellId::new("BT5SingleFace", "top");
    let _top_hash = match result.values.get(&top_id) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => {
            assert_ne!(
                upstream_values_hash, &[0u8; 32],
                "BT5: top face handle upstream_values_hash must be non-zero \
                 (fixture golden, not baseline-diff; single(+Z face) realized)"
            );
            *upstream_values_hash
        }
        other => panic!(
            "BT5: BT5SingleFace.top = single(faces_by_normal(b, +Z, 1°)) must coerce \
             (Selector → List<Geometry> → single) to Value::GeometryHandle, got: {other:?}"
        ),
    };

    // Cross-check identity: build a combined structure in the SAME OCCT engine
    // that also extracts all 6 box faces via faces(b)[0..5]. Then assert that
    // `top`'s kernel_handle appears exactly once in the 6-face list — i.e.
    // single(faces_by_normal(b,+Z)) selected exactly one of the box's real
    // faces rather than an arbitrary or fabricated handle.
    //
    // NOTE: upstream_values_hash cannot distinguish single(+Z) from single(-Z)
    // because resolve_selector_to_list enumerates results starting at i=0 for
    // both — both single-element lists get compose_sub_handle_hash(parent, Face, 0).
    // kernel_handle IS session-scoped (ephemeral) but within a single engine it
    // uniquely identifies each face, so the within-engine comparison is valid.
    let verify_src = r#"
structure def BT5Verify {
    let b   = box(10mm, 10mm, 10mm)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let top = single(faces_by_normal(b, dir, tol))
    let f0  = faces(b)[0]
    let f1  = faces(b)[1]
    let f2  = faces(b)[2]
    let f3  = faces(b)[3]
    let f4  = faces(b)[4]
    let f5  = faces(b)[5]
}
"#;
    let verify_compiled = compile_source_with_stdlib(verify_src);
    let verify_kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut verify_engine = Engine::new(Box::new(SimpleConstraintChecker), Some(verify_kernel));
    let verify_result = verify_engine.build(&verify_compiled, ExportFormat::Step);

    // top's kernel_handle (session-scoped, valid within this engine instance).
    let top_kh = match verify_result
        .values
        .get(&ValueCellId::new("BT5Verify", "top"))
    {
        Some(Value::GeometryHandle { kernel_handle, .. }) => *kernel_handle,
        other => panic!("BT5 verify: BT5Verify.top must realize to GeometryHandle, got: {other:?}"),
    };

    // Collect all 6 face kernel_handles from faces(b)[0..5].
    let face_khs: Vec<Option<GeometryHandleId>> = (0..6_usize)
        .map(|i| {
            let member = format!("f{i}");
            match verify_result
                .values
                .get(&ValueCellId::new("BT5Verify", &member))
            {
                Some(Value::GeometryHandle { kernel_handle, .. }) => *kernel_handle,
                other => panic!(
                    "BT5 verify: BT5Verify.{member} must realize to GeometryHandle, got: {other:?}"
                ),
            }
        })
        .collect();

    // top must appear exactly once among f0..f5 — proves the +Z face selector
    // resolved to a real box face (not a fabricated or wrong-kind handle).
    let match_count = face_khs.iter().filter(|&kh| *kh == top_kh).count();
    assert_eq!(
        match_count, 1,
        "BT5: single(faces_by_normal(b, +Z, 1°)) kernel_handle must appear exactly \
         once in faces(b)[0..5]; got {match_count} matches among {face_khs:?}"
    );
}

// ── BT4b: fillet call-site transparency (compile-only) ───────────────────────

/// BT4b (consumer / call-site transparency / D4): `fillet(b,
/// edges_at_height(b, 0mm, 0.01mm), 1mm)` must compile with zero error
/// diagnostics — the edge-selector argument is accepted at the existing fillet
/// call site (D4 transparency).
///
/// PRD §5 BT4: "eager coercion realizes the asserted geometry — D4
/// transparency: every existing consumer compiles UNCHANGED."
///
/// **Scope note (esc-4118-52):** fillet's edge-selector RUNTIME resolution is
/// OUT OF SCOPE for this task. This is a COMPILE-ONLY transparency check:
/// we assert the selector argument is accepted at the fillet call site without
/// asserting any build output or geometry handle. The geometry golden for BT4
/// uses the wired `faces(b)[0]` IndexAccess shape (bt4_index_access_coercion.ri
/// / `bt4_index_access_coercion_realizes_face`).
///
/// RED when fixture `bt4_fillet_transparency.ri` is absent.
#[test]
fn bt4_fillet_call_site_compiles_transparently() {
    let source = std::fs::read_to_string(fixture_path("bt4_fillet_transparency.ri"))
        .expect("fixture bt4_fillet_transparency.ri must exist (create in step-16 to turn GREEN)");

    // Compile with compile_source_with_stdlib — we assert NO errors.
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);

    // D4 transparency: the edge-selector arg is accepted at the fillet call site.
    // fillet RUNTIME resolution is out of scope (esc-4118-52): this test proves
    // compile-time acceptance ONLY.
    assert!(
        errors.is_empty(),
        "BT4b: bt4_fillet_transparency.ri must compile with zero error diagnostics \
         (D4 call-site transparency — fillet accepts EdgeSelector arg at compile time), \
         got:\n{:#?}",
        errors
    );
}

// ── BT8: named-leaf interim — resolve returns [] + one TopologyTagStale ──────

/// BT8 (producer / Named-leaf interim / D8): `face(b, "nope")` must build a
/// `Value::Selector(Face)` whose node is a `Leaf` with `LeafQuery::Named("nope")`
/// (kernel-free construction). Calling `topology_selectors::resolve()` on it
/// must return `Ok(vec![])` (empty selection) and emit EXACTLY ONE diagnostic
/// with `code = Some(DiagnosticCode::TopologyTagStale)` (W_TOPOLOGY_TAG_STALE).
/// The call must NOT panic.
///
/// PRD §5 BT8: "named-leaf (`face(b,tag)`) resolves to [] + 1 TopologyTagStale
/// warning (D8 interim) — no panic".
///
/// Strategy: build kernel-free (selector stays `Value::Selector`, no eager
/// resolution) → assert leaf shape → resolve against `staged_box_kernel()`
/// (always-on, no OCCT).
///
/// RED when fixture `bt8_named_leaf_interim.ri` is absent.
#[test]
fn bt8_named_leaf_interim_empty_with_one_warning() {
    let source = std::fs::read_to_string(fixture_path("bt8_named_leaf_interim.ri"))
        .expect("fixture bt8_named_leaf_interim.ri must exist (create in step-12 to turn GREEN)");

    // (a) Build kernel-free; the Named selector must stay Value::Selector(Face).
    let result = build_with_unstaged_kernel(&source);
    let sv = selector_cell(&result, "BT8NamedLeaf", "s");

    assert_eq!(
        sv.kind,
        SelectorKind::Face,
        "BT8: BT8NamedLeaf.s must be Value::Selector(Face)"
    );

    // (b) The node must be a Leaf with LeafQuery::Named("nope").
    //     Also assert the leaf target references GeometryHandleId(1) (the box), so a
    //     future handle-allocation change fails loudly instead of silently resolving
    //     against the wrong parent and returning [] for the wrong reason.
    match &sv.node {
        SelectorNode::Leaf {
            query: LeafQuery::Named(tag),
            target,
        } => {
            assert_eq!(tag, "nope", "BT8: Named tag must be \"nope\", got: {tag:?}");
            assert_eq!(
                target.kernel_handle,
                Some(GeometryHandleId(1)),
                "BT8: Named leaf target.kernel_handle must be GeometryHandleId(1) \
                 (the box handle); if this drifts, update staged_box_kernel() parent id to match"
            );
        }
        other => panic!(
            "BT8: BT8NamedLeaf.s must be SelectorNode::Leaf{{Named(\"nope\")}}, got: {other:?}"
        ),
    }

    // (c) Resolve against the staged box kernel.
    let mut staged = staged_box_kernel();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let handles = topology_selectors::resolve(sv, &mut staged, &mut diags)
        .expect("BT8: resolve() must not return a QueryError for a Named leaf");

    // (d) Returns Ok([]) — "nope" matches no tag → empty selection.
    assert!(
        handles.is_empty(),
        "BT8: Named(\"nope\") must resolve to [] (no match), got: {handles:?}"
    );

    // (e) Exactly ONE diagnostic with code = TopologyTagStale.
    assert_eq!(
        diags.len(),
        1,
        "BT8: resolve() must emit exactly 1 diagnostic (W_TOPOLOGY_TAG_STALE), \
         got {} diagnostics:\n{:#?}",
        diags.len(),
        diags
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::TopologyTagStale),
        "BT8: the diagnostic must carry DiagnosticCode::TopologyTagStale, \
         got: {:?}",
        diags[0].code
    );
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
    let source = std::fs::read_to_string(fixture_path("bt3_difference_intersect.ri"))
        .expect("fixture bt3_difference_intersect.ri must exist (create in step-10 to turn GREEN)");

    let result = build_with_unstaged_kernel(&source);

    // ── difference: all faces EXCEPT +Z ─────────────────────────────────────
    let sv_d = selector_cell(&result, "BT3SetOps", "d");
    assert_eq!(
        sv_d.kind,
        SelectorKind::Face,
        "BT3: d must be Selector(Face)"
    );

    // Assert both difference children reference the box (GeometryHandleId(1)).
    // Explicit check prevents a handle-id drift from causing staged_box_kernel()
    // to silently return [] instead of the expected face set.
    match &sv_d.node {
        SelectorNode::Difference(a, b) => {
            match &a.node {
                SelectorNode::Leaf { target, .. } => {
                    assert_eq!(
                        target.kernel_handle,
                        Some(GeometryHandleId(1)),
                        "BT3: difference minuend (faces(b)) leaf target.kernel_handle must \
                         be GeometryHandleId(1); update staged_box_kernel() if this drifts"
                    );
                }
                other => panic!("BT3: difference minuend must be a Leaf, got: {other:?}"),
            }
            match &b.node {
                SelectorNode::Leaf { target, .. } => {
                    assert_eq!(
                        target.kernel_handle,
                        Some(GeometryHandleId(1)),
                        "BT3: difference subtrahend (faces_by_normal(+Z)) leaf \
                         target.kernel_handle must be GeometryHandleId(1); \
                         update staged_box_kernel() if this drifts"
                    );
                }
                other => panic!("BT3: difference subtrahend must be a Leaf, got: {other:?}"),
            }
        }
        other => panic!("BT3: d must be SelectorNode::Difference(_,_), got: {other:?}"),
    }

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
    assert_eq!(
        sv_i.kind,
        SelectorKind::Face,
        "BT3: i must be Selector(Face)"
    );

    // Assert each intersect child references the box (GeometryHandleId(1)).
    match &sv_i.node {
        SelectorNode::Intersect(children) => {
            for (i, child) in children.iter().enumerate() {
                match &child.node {
                    SelectorNode::Leaf { target, .. } => {
                        assert_eq!(
                            target.kernel_handle,
                            Some(GeometryHandleId(1)),
                            "BT3: intersect child[{i}] leaf target.kernel_handle must be \
                             GeometryHandleId(1); update staged_box_kernel() if this drifts"
                        );
                    }
                    other => {
                        panic!("BT3: intersect child[{i}] must be a Leaf, got: {other:?}")
                    }
                }
            }
        }
        other => panic!("BT3: i must be SelectorNode::Intersect(_), got: {other:?}"),
    }

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
    let source = std::fs::read_to_string(fixture_path("bt2_same_kind_union.ri"))
        .expect("fixture bt2_same_kind_union.ri must exist (create in step-8 to turn GREEN)");

    // Kernel-free build: obtain the Union Value::Selector(Face) cell.
    let result = build_with_unstaged_kernel(&source);
    let sv = selector_cell(&result, "BT2SameKindUnion", "u");
    assert_eq!(sv.kind, SelectorKind::Face, "BT2: u must be Selector(Face)");

    // The Union must have two children (one per faces_by_normal call).
    // Also assert each child leaf targets GeometryHandleId(1) (the box) so a
    // future handle-allocation change fails loudly instead of silently returning []
    // because staged_box_kernel() staged a different parent id.
    match &sv.node {
        SelectorNode::Union(children) => {
            assert_eq!(children.len(), 2, "BT2: union must have 2 children");
            for (i, child) in children.iter().enumerate() {
                match &child.node {
                    SelectorNode::Leaf { target, .. } => {
                        assert_eq!(
                            target.kernel_handle,
                            Some(GeometryHandleId(1)),
                            "BT2: union child[{i}] leaf target.kernel_handle must be \
                             GeometryHandleId(1) (box handle); if this drifts, update \
                             staged_box_kernel() parent id to match"
                        );
                    }
                    other => panic!("BT2: union child[{i}] must be a Leaf, got: {other:?}"),
                }
            }
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
                *dir,
                [0.0, 0.0, 1.0],
                "BT7: ByNormal dir must be +Z (0,0,1)"
            );
            assert!(
                *tol_rad > 0.0,
                "BT7: ByNormal tol_rad must be positive (1deg converted to rad), got {tol_rad}"
            );
            // The box is the first `execute()` call → GeometryHandleId(1).
            assert_eq!(
                target.kernel_handle,
                Some(GeometryHandleId(1)),
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
    let source = std::fs::read_to_string(fixture_path("bt6_kind_typed_param.ri"))
        .expect("fixture bt6_kind_typed_param.ri must exist (create it in step-4 to turn GREEN)");

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
    let source = std::fs::read_to_string(fixture_path("bt1_wrong_kind_union.ri"))
        .expect("fixture bt1_wrong_kind_union.ri must exist (create it in step-2 to turn GREEN)");

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
