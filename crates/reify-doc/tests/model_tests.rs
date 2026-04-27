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
                    params: vec![ParamDoc {
                        name: "length".into(),
                        doc: Some("Bolt length.".into()),
                        type_repr: "Length".into(),
                        default_repr: Some("100 mm".into()),
                        annotations: vec![AnnotationDoc {
                            name: "solver_hint".into(),
                            args: vec![
                                "\"discrete_set\"".into(),
                                "standard_bolt_lengths".into(),
                            ],
                        }],
                    }],
                    ports: vec![],
                    constraints: vec![ConstraintDoc {
                        label: None,
                        // expr_repr is a byte-range slice of FIXTURE_SOURCE[32..50]
                        expr_repr: FIXTURE_SOURCE[32..50].to_string(),
                        annotations: vec![],
                        line: Some(1),
                    }],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![
                        ("part_number".into(), "ISO-4014-M8x25".into()),
                        ("revision".into(), "B".into()),
                    ],
                },
                // Occurrence — MCU (rank 2)
                ItemDoc::Occurrence {
                    name: "MCU".into(),
                    doc: Some("Microcontroller occurrence.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
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
            cross_refs: ModuleCrossRefs {
                referenced_modules: vec![],
                referenced_items: vec![],
                referenced_traits: vec!["Physical".into()],
            },
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

/// Assert that trait conformance is captured via `ModuleCrossRefs.referenced_traits`.
///
/// ## TODO(build_doc_model) — trait_bounds field gap
///
/// The PRD (docs/prds/reify-doc-tool.md lines 51-67) specifies an abstract
/// `ItemDoc { …, trait_bounds: Vec<String>, … }` field, but the *concrete*
/// `ItemDoc::Structure` variant in `crates/reify-doc/src/model.rs` (lines 176-193)
/// does not yet have this field.
///
/// Until a future model-extension task adds `trait_bounds` to `ItemDoc::Structure`,
/// the closest available signal is `ModuleCrossRefs.referenced_traits`.  This test
/// asserts the proxy: `modules[0].cross_refs.referenced_traits == vec!["Physical"]`.
///
/// **Migration note:** Once `trait_bounds` is added to `ItemDoc::Structure`, pivot
/// this assertion to:
/// ```rust
/// match &items[bolt_index] {
///     ItemDoc::Structure { trait_bounds, .. } => {
///         assert_eq!(trait_bounds, &["Physical"]);
///     }
///     _ => unreachable!(),
/// }
/// ```
/// and remove or update the `referenced_traits` proxy assertion.
#[test]
fn structure_trait_bounds_populated_via_module_cross_refs() {
    let model = build_fixture();
    let m = &model.modules[0];
    // Proxy assertion: the module records that "Physical" is referenced as a trait.
    // TODO(build_doc_model): pivot to ItemDoc::Structure.trait_bounds once that field exists.
    assert_eq!(
        m.cross_refs.referenced_traits,
        vec!["Physical".to_string()],
        "module cross_refs.referenced_traits must include 'Physical'"
    );
}

/// Assert that `ConstraintDoc.expr_repr` matches a byte-range slice of the source string.
///
/// `FIXTURE_SOURCE[32..50] == "length >= diameter"` is asserted as a sanity preamble,
/// then the constraint's `expr_repr` must equal that slice.  Explicit byte indices (not
/// a runtime `find()`) document the contract: the lowering pass records precise source
/// spans and slices them to populate `expr_repr`.
///
/// Also verifies `line == Some(1)` (1-indexed source line).
///
/// TODO(build_doc_model): When the lowering pass lands, the constraint span comes from
/// actual compiler source metadata. Update this test to re-anchor against the real span
/// recorded on the compiled constraint rather than hardcoded indices.
#[test]
fn constraint_expr_repr_matches_source_byte_range_slice() {
    // Sanity: confirm the hardcoded indices produce the expected sub-string.
    assert_eq!(
        &FIXTURE_SOURCE[32..50],
        "length >= diameter",
        "FIXTURE_SOURCE byte-range sanity check failed — indices out of sync"
    );

    let model = build_fixture();
    let items = &model.modules[0].items;
    let bolt = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Structure { name, .. } if name == "Bolt"))
        .expect("Structure 'Bolt' not found");
    match bolt {
        ItemDoc::Structure { constraints, .. } => {
            assert!(!constraints.is_empty(), "Bolt must have at least one constraint");
            let c = &constraints[0];
            assert_eq!(
                c.expr_repr, &FIXTURE_SOURCE[32..50],
                "constraint expr_repr must match source byte-range slice"
            );
            assert_eq!(c.line, Some(1), "constraint line must be 1");
        }
        _ => unreachable!(),
    }
}

/// Assert canonical item ordering: Trait < Structure < Occurrence < Enum < Function < constant-like.
///
/// Within each kind, items must be sorted alphabetically by name.
/// Fixture must include at least one Occurrence to exercise the S→O transition.
///
/// TODO(build_doc_model): When build_doc_model lands, item ordering is produced by
/// the lowering pass. This test still verifies the shape contract — the ordering
/// rule is part of the PRD spec and must hold regardless of construction path.
#[test]
fn items_sorted_traits_then_structures_then_occurrences_then_enums_then_functions_then_constants_alphabetical() {
    let model = build_fixture();
    let items = &model.modules[0].items;

    /// Returns the canonical sort rank for an item kind (matches PRD order).
    /// Trait=0, Structure=1, Occurrence=2, Enum=3, Function=4, constant-like=5.
    fn kind_rank(item: &ItemDoc) -> u8 {
        match item {
            ItemDoc::Trait { .. } => 0,
            ItemDoc::Structure { .. } => 1,
            ItemDoc::Occurrence { .. } => 2,
            ItemDoc::Enum { .. } => 3,
            ItemDoc::Function { .. } => 4,
            // Field, Purpose, Unit, TypeAlias, ConstraintDef
            _ => 5,
        }
    }

    fn item_name(item: &ItemDoc) -> &str {
        match item {
            ItemDoc::Trait { name, .. }
            | ItemDoc::Structure { name, .. }
            | ItemDoc::Occurrence { name, .. }
            | ItemDoc::Enum { name, .. }
            | ItemDoc::Function { name, .. }
            | ItemDoc::Field { name, .. }
            | ItemDoc::Purpose { name, .. }
            | ItemDoc::Unit { name, .. }
            | ItemDoc::TypeAlias { name, .. }
            | ItemDoc::ConstraintDef { name, .. } => name.as_str(),
        }
    }

    // Verify at least one Occurrence is present (exercises the S→O transition)
    let has_occurrence = items.iter().any(|i| matches!(i, ItemDoc::Occurrence { .. }));
    assert!(has_occurrence, "fixture must contain at least one Occurrence to exercise the S→O sort transition");

    // Ranks must be non-decreasing
    let ranks: Vec<u8> = items.iter().map(kind_rank).collect();
    for window in ranks.windows(2) {
        assert!(
            window[0] <= window[1],
            "items out of kind order: rank {} came before rank {} in {:?}",
            window[0],
            window[1],
            items.iter().map(|i| (kind_rank(i), item_name(i))).collect::<Vec<_>>()
        );
    }

    // Within equal-rank runs, names must be alphabetically ordered
    let mut prev_rank = u8::MAX;
    let mut prev_name = "";
    for item in items {
        let rank = kind_rank(item);
        let name = item_name(item);
        if rank == prev_rank {
            assert!(
                name >= prev_name,
                "items within rank {} not alphabetically ordered: '{prev_name}' > '{name}'",
                rank
            );
        }
        prev_rank = rank;
        prev_name = name;
    }
}

/// Assert that `@solver_hint("discrete_set", standard_bolt_lengths)` on the `length`
/// param surfaces as `AnnotationDoc { name: "solver_hint", args: ["\"discrete_set\"",
/// "standard_bolt_lengths"] }` on the param.
///
/// Two-arg form: first arg is a quoted string literal, second is a bare identifier.
/// This follows the task spec literally rather than the single-concatenated-arg form
/// used in fmt_markdown_tests.rs (which is a different fixture for a different test).
///
/// TODO(build_doc_model): Once the lowering pass lands, ParamDoc.annotations are
/// populated from the compiled param's annotation list. Assertion logic stays unchanged.
#[test]
fn param_solver_hint_surfaces_as_annotation_doc_with_two_args() {
    let model = build_fixture();
    let items = &model.modules[0].items;
    let bolt = items
        .iter()
        .find(|i| matches!(i, ItemDoc::Structure { name, .. } if name == "Bolt"))
        .expect("Structure 'Bolt' not found");
    match bolt {
        ItemDoc::Structure { params, .. } => {
            let length = params.iter().find(|p| p.name == "length");
            let length = length.expect("param 'length' not found on Bolt");
            assert_eq!(length.type_repr, "Length", "length type_repr mismatch");
            assert_eq!(
                length.default_repr.as_deref(),
                Some("100 mm"),
                "length default_repr mismatch"
            );
            let hint = length.annotations.iter().find(|a| a.name == "solver_hint");
            let hint = hint.expect("@solver_hint not found on length param");
            assert_eq!(
                hint.args,
                vec!["\"discrete_set\"".to_string(), "standard_bolt_lengths".to_string()],
                "@solver_hint args mismatch"
            );
        }
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
