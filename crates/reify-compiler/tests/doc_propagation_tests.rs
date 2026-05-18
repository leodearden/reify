//! Integration tests verifying that `///` doc comments are threaded from the
//! AST through the compiler into the corresponding compiled types.
//!
//! Pattern: compile a minimal source containing a doc-commented declaration,
//! then inspect the relevant field in the compiled output.  Each test targets
//! exactly one type/field pair so failures point directly at the broken seam.

use reify_test_support::compile_source;

// ─── step-1: structure → TopologyTemplate ───────────────────────────────────

#[test]
fn structure_def_doc_propagates_to_topology_template() {
    let compiled = compile_source("/// A widget\nstructure Widget { let x = 1.0 }");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should exist");
    assert_eq!(
        template.doc,
        Some("A widget".to_string()),
        "TopologyTemplate.doc should carry the doc comment"
    );
}

// ─── step-3: fn → CompiledFunction ──────────────────────────────────────────

#[test]
fn fn_def_doc_propagates_to_compiled_function() {
    let compiled = compile_source("/// Doubles it\nfn dbl(x: Real) -> Real { x + x }");
    let func = compiled
        .functions
        .iter()
        .find(|f| f.name == "dbl")
        .expect("dbl function should exist");
    assert_eq!(
        func.doc,
        Some("Doubles it".to_string()),
        "CompiledFunction.doc should carry the doc comment"
    );
}

// ─── step-7: enum → EnumDef ─────────────────────────────────────────────────

#[test]
fn enum_decl_doc_propagates_to_enum_def() {
    let compiled = compile_source("/// Primary colors\nenum Color { Red, Green, Blue }");
    let enum_def = compiled
        .enum_defs
        .iter()
        .find(|e| e.name == "Color")
        .expect("Color enum should exist");
    assert_eq!(
        enum_def.doc,
        Some("Primary colors".to_string()),
        "EnumDef.doc should carry the doc comment"
    );
}

// ─── amend: occurrence → TopologyTemplate (OccurrenceDef seam) ─────────────
//
// The From<&StructureDef> and From<&OccurrenceDef> impls in entity.rs are two
// separate code paths that both feed EntityDefRef::doc → TopologyTemplate::doc.
// The step-1 test covers the StructureDef path; this test independently pins
// the OccurrenceDef path so a regression in `doc: o.doc.clone()` cannot
// silently pass the suite.

#[test]
fn occurrence_def_doc_propagates_to_topology_template() {
    let compiled =
        compile_source("/// A joint\noccurrence Weld { let duration = 5.0 }");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Weld")
        .expect("Weld template should exist");
    assert_eq!(
        template.doc,
        Some("A joint".to_string()),
        "TopologyTemplate.doc from an occurrence should carry the doc comment"
    );
}

// ─── step-5: trait → CompiledTrait ──────────────────────────────────────────

#[test]
fn trait_decl_doc_propagates_to_compiled_trait() {
    let compiled =
        compile_source("/// Rigid things\ntrait Rigid { param mass: Real }");
    let trait_def = compiled
        .trait_defs
        .iter()
        .find(|t| t.name == "Rigid")
        .expect("Rigid trait should exist");
    assert_eq!(
        trait_def.doc,
        Some("Rigid things".to_string()),
        "CompiledTrait.doc should carry the doc comment"
    );
}

// ─── negative case: absence of /// → doc must be None, not Some("") ─────────
//
// Catches a class of regression in `extract_doc_comment`/lowering where the
// absence of a doc comment is accidentally lowered to `Some(String::new())`
// instead of `None`.  One combined fixture covers all four compiled types.

#[test]
fn missing_doc_comment_lowers_to_none() {
    let compiled = compile_source(
        "structure Widget { let x = 1.0 }\n\
         occurrence Weld { let duration = 5.0 }\n\
         fn dbl(x: Real) -> Real { x + x }\n\
         trait Rigid { param mass: Real }\n\
         enum Color { Red, Green, Blue }\n",
    );

    let structure = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should exist");
    assert!(
        structure.doc.is_none(),
        "TopologyTemplate.doc must be None when no /// comment is present, got {:?}",
        structure.doc
    );

    let occurrence = compiled
        .templates
        .iter()
        .find(|t| t.name == "Weld")
        .expect("Weld template should exist");
    assert!(
        occurrence.doc.is_none(),
        "TopologyTemplate.doc (occurrence path) must be None when no /// comment is present, got {:?}",
        occurrence.doc
    );

    let func = compiled
        .functions
        .iter()
        .find(|f| f.name == "dbl")
        .expect("dbl function should exist");
    assert!(
        func.doc.is_none(),
        "CompiledFunction.doc must be None when no /// comment is present, got {:?}",
        func.doc
    );

    let trait_def = compiled
        .trait_defs
        .iter()
        .find(|t| t.name == "Rigid")
        .expect("Rigid trait should exist");
    assert!(
        trait_def.doc.is_none(),
        "CompiledTrait.doc must be None when no /// comment is present, got {:?}",
        trait_def.doc
    );

    let enum_def = compiled
        .enum_defs
        .iter()
        .find(|e| e.name == "Color")
        .expect("Color enum should exist");
    assert!(
        enum_def.doc.is_none(),
        "EnumDef.doc must be None when no /// comment is present, got {:?}",
        enum_def.doc
    );
}
