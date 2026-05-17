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
