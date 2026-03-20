//! Collection sub-structure tests (task 64).

use reify_types::Severity;

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_coll"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );
    compiled
}

// ─── step-1: parse collection sub form ───

#[test]
fn parse_collection_sub_form() {
    let source = "structure S { sub bolts : List<Bolt> }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "bolts");
    assert_eq!(sub.structure_name, "Bolt");
    assert!(sub.is_collection, "expected is_collection=true for List<Bolt>");
    assert!(sub.args.is_empty(), "collection sub should have no args");
}

#[test]
fn parse_instantiation_sub_form() {
    let source = "structure S { sub rib = Rib() }";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let structure = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let sub = match &structure.members[0] {
        reify_syntax::MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "rib");
    assert_eq!(sub.structure_name, "Rib");
    assert!(!sub.is_collection, "expected is_collection=false for = form");
}

// ─── step-3: compile collection sub ───

#[test]
fn compile_collection_sub() {
    let source = r#"
        structure Bolt { param diameter : Scalar = 10mm }
        structure S { sub bolts : List<Bolt> }
    "#;
    let compiled = compile_no_errors(source);
    // Find the S template
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");
    let sub = &s_template.sub_components[0];
    assert_eq!(sub.name, "bolts");
    assert_eq!(sub.structure_name, "Bolt");
    assert!(sub.is_collection, "compiled SubComponentDecl should have is_collection=true");
}

#[test]
fn compile_instantiation_sub() {
    let source = r#"
        structure Rib { param width : Scalar = 5mm }
        structure S { sub rib = Rib() }
    "#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("should have template S");
    let sub = &s_template.sub_components[0];
    assert_eq!(sub.name, "rib");
    assert_eq!(sub.structure_name, "Rib");
    assert!(!sub.is_collection, "compiled SubComponentDecl should have is_collection=false");
}
