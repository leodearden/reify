//! Integration tests pinning the *shape contract* of `reify_doc::build_doc_model` output.
//!
//! ## Purpose
//!
//! These tests assert behavioural contracts that the future `reify_doc::build_doc_model`
//! lowering pass must satisfy (PRD §slice 2, `docs/prds/reify-doc-tool.md` lines 51–67):
//!
//! - Doc strings flow into `ItemDoc.doc`
//! - Trait-bound info is captured
//! - Meta entries are copied verbatim preserving insertion order
//! - Items are sorted: traits → structures → occurrences → enums → functions → constants,
//!   with alphabetical tiebreak within each kind
//! - Constraint `expr_repr` matches a byte-slice of the source
//! - `@solver_hint(...)` surfaces as an `AnnotationDoc` on the param
//!
//! ## Stage 2 reconciliation TODO
//!
//! `build_doc_model()` does not yet exist in `crates/reify-doc/src/` or
//! `crates/reify-doc-build/src/`. Until that lowering pass lands, these tests
//! manually instantiate `DocModel` structs via `build_fixture()` to assert
//! structural shape contracts. Once `build_doc_model()` is implemented:
//!
//! 1. Replace the call to `build_fixture()` in each test with
//!    `build_doc_model(&compiled_module)`.
//! 2. Remove the hand-crafted fixture helper entirely.
//! 3. See individual `TODO(build_doc_model)` comments in each test for
//!    assertion-specific migration notes.
//!
//! Pattern mirrors `fmt_markdown_tests.rs::build_integration_full_v01_fixture`
//! (line 1391+) which uses the same inline-construction style with TODO marker.

use reify_doc::model::{
    AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ModuleDoc, ModuleCrossRefs, ParamDoc,
};

/// Simulated source text for the Bolt structure definition.
///
/// Used to pin the constraint `expr_repr` byte-range contract.
/// FIXTURE_SOURCE[32..50] == "length >= diameter"
///
/// TODO(build_doc_model): When the lowering pass lands, the constraint span
/// will come from actual compiler source metadata rather than this hardcoded
/// constant. Update `constraint_expr_repr_matches_source_byte_range_slice`
/// to re-anchor against the real span recorded on the compiled constraint.
const FIXTURE_SOURCE: &str = "structure def Bolt { constraint length >= diameter }";

/// Build the minimal integration fixture.
///
/// Returns a `DocModel` shaped for testing the six behaviour contracts.
/// Fixture grows incrementally across TDD impl steps (steps 2, 4, 6, 8, 10, 12, 14, 16).
///
/// TODO(build_doc_model): Replace with `reify_doc::build_doc_model(&compiled_module)`
/// once the lowering pass is available.
fn build_fixture() -> DocModel {
    DocModel {
        modules: vec![ModuleDoc {
            path: "test_fixture".into(),
            doc: Some(
                "Minimal fixture pinning the build_doc_model output-shape contract.".into(),
            ),
            items: vec![],
            annotations: vec![],
            pragmas: vec![],
            cross_refs: ModuleCrossRefs::default(),
        }],
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Assert the fixture module path and top-level doc-comment are correct.
///
/// TODO(build_doc_model): This test will require no changes once `build_doc_model`
/// is called — the module path and doc come directly from the compiled module.
#[test]
fn fixture_module_path_and_top_level_doc() {
    let model = build_fixture();
    assert_eq!(model.modules.len(), 1, "expected exactly one module");
    let m = &model.modules[0];
    assert_eq!(m.path, "test_fixture");
    assert_eq!(
        m.doc.as_deref(),
        Some("Minimal fixture pinning the build_doc_model output-shape contract.")
    );
}
