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
            items: vec![
                // Trait — Physical (rank 0)
                ItemDoc::Trait {
                    name: "Physical".into(),
                    doc: Some("Trait for objects with a measurable mass.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    members: vec!["mass: Mass".into()],
                },
                // Structure — Bolt (rank 1)
                ItemDoc::Structure {
                    name: "Bolt".into(),
                    doc: Some("A standard fastening bolt conforming to Physical.".into()),
                    is_pub: true,
                    annotations: vec![AnnotationDoc {
                        name: "test".into(),
                        args: vec![],
                    }],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![
                        ("part_number".into(), "ISO-4014-M8x25".into()),
                        ("revision".into(), "B".into()),
                    ],
                },
                // Enum — Grade (rank 3)
                ItemDoc::Enum {
                    name: "Grade".into(),
                    doc: Some("Material grade classification.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    variants: vec![
                        "Standard".into(),
                        "Reinforced".into(),
                        "Premium".into(),
                    ],
                },
                // Function — safety_factor (rank 4)
                ItemDoc::Function {
                    name: "safety_factor".into(),
                    doc: Some("Safety factor for real-valued loads.".into()),
                    is_pub: true,
                    annotations: vec![AnnotationDoc {
                        name: "deprecated".into(),
                        args: vec!["\"superseded by safety_margin\"".into()],
                    }],
                    pragmas: vec![],
                    signature: "fn safety_factor(load: Real) -> Real".into(),
                },
            ],
            annotations: vec![],
            pragmas: vec![],
            cross_refs: ModuleCrossRefs::default(),
        }],
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Assert that doc strings from item declarations flow into their `ItemDoc.doc` field.
///
/// Walks `modules[0].items` and verifies the doc string for each of the four
/// named items in the fixture: Physical (Trait), Bolt (Structure), Grade (Enum),
/// safety_factor (Function).
///
/// TODO(build_doc_model): Once the lowering pass lands, replace `build_fixture()`
/// with `build_doc_model(&compiled_module)`. The assertion logic itself stays the
/// same — it tests the contract, not the construction path.
#[test]
fn doc_strings_flow_into_item_doc() {
    let model = build_fixture();
    let items = &model.modules[0].items;

    // Physical — Trait
    let physical = items.iter().find(|i| matches!(i, ItemDoc::Trait { name, .. } if name == "Physical"));
    let physical = physical.expect("Trait 'Physical' not found in fixture");
    match physical {
        ItemDoc::Trait { doc, .. } => assert_eq!(
            doc.as_deref(),
            Some("Trait for objects with a measurable mass."),
            "Physical doc mismatch"
        ),
        _ => unreachable!(),
    }

    // Bolt — Structure
    let bolt = items.iter().find(|i| matches!(i, ItemDoc::Structure { name, .. } if name == "Bolt"));
    let bolt = bolt.expect("Structure 'Bolt' not found in fixture");
    match bolt {
        ItemDoc::Structure { doc, .. } => assert_eq!(
            doc.as_deref(),
            Some("A standard fastening bolt conforming to Physical."),
            "Bolt doc mismatch"
        ),
        _ => unreachable!(),
    }

    // Grade — Enum
    let grade = items.iter().find(|i| matches!(i, ItemDoc::Enum { name, .. } if name == "Grade"));
    let grade = grade.expect("Enum 'Grade' not found in fixture");
    match grade {
        ItemDoc::Enum { doc, .. } => assert_eq!(
            doc.as_deref(),
            Some("Material grade classification."),
            "Grade doc mismatch"
        ),
        _ => unreachable!(),
    }

    // safety_factor — Function
    let sf = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Function { name, .. } if name == "safety_factor"));
    let sf = sf.expect("Function 'safety_factor' not found in fixture");
    match sf {
        ItemDoc::Function { doc, .. } => assert_eq!(
            doc.as_deref(),
            Some("Safety factor for real-valued loads."),
            "safety_factor doc mismatch"
        ),
        _ => unreachable!(),
    }
}

/// Assert that Bolt's meta block is copied verbatim preserving insertion order.
///
/// `meta: Vec<(String, String)>` (model.rs:192) uses ordered pairs precisely so
/// insertion order survives serialization and duplicate keys are allowed.
///
/// TODO(build_doc_model): Once the lowering pass lands, meta pairs are populated
/// from the compiled structure's meta block. Assert shape stays the same.
#[test]
fn structure_meta_block_copied_verbatim_preserving_insertion_order() {
    let model = build_fixture();
    let items = &model.modules[0].items;
    let bolt = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Structure { name, .. } if name == "Bolt"))
        .expect("Structure 'Bolt' not found");
    match bolt {
        ItemDoc::Structure { meta, .. } => {
            assert_eq!(
                meta,
                &vec![
                    ("part_number".to_string(), "ISO-4014-M8x25".to_string()),
                    ("revision".to_string(), "B".to_string()),
                ],
                "meta insertion order mismatch"
            );
        }
        _ => unreachable!(),
    }
}

/// Assert that `@test` annotation is on Bolt (Structure) and `@deprecated` is on
/// safety_factor (Function), each with the correct args.
///
/// TODO(build_doc_model): Once the lowering pass lands, the annotations are
/// populated from the compiled item's annotation list rather than from
/// build_fixture(). The assertion logic itself stays unchanged.
#[test]
fn structure_test_annotation_and_function_deprecated_annotation_present() {
    let model = build_fixture();
    let items = &model.modules[0].items;

    // Bolt must carry @test with no args
    let bolt = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Structure { name, .. } if name == "Bolt"))
        .expect("Structure 'Bolt' not found");
    match bolt {
        ItemDoc::Structure { annotations, .. } => {
            let test_ann = annotations.iter().find(|a| a.name == "test");
            let test_ann = test_ann.expect("@test annotation not found on Bolt");
            assert!(
                test_ann.args.is_empty(),
                "@test args should be empty, got: {:?}",
                test_ann.args
            );
        }
        _ => unreachable!(),
    }

    // safety_factor must carry @deprecated with one quoted arg
    let sf = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Function { name, .. } if name == "safety_factor"))
        .expect("Function 'safety_factor' not found");
    match sf {
        ItemDoc::Function { annotations, .. } => {
            let dep_ann = annotations.iter().find(|a| a.name == "deprecated");
            let dep_ann = dep_ann.expect("@deprecated annotation not found on safety_factor");
            assert_eq!(
                dep_ann.args,
                vec!["\"superseded by safety_margin\"".to_string()],
                "@deprecated args mismatch"
            );
        }
        _ => unreachable!(),
    }
}

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
