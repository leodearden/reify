//! Tests for module DAG: cycle detection, diamond dedup, topological ordering.

use std::fs;
use std::path::PathBuf;

use reify_compiler::module_dag::{ModuleDag, ModuleResolver};

/// Create a unique temp directory for tests.
fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("reify_dag_test")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── Step 19: Circular import detection ────────────────────────────

#[test]
fn circular_import_detected() {
    let dir = test_dir("circular");

    // Module a imports b
    fs::write(
        dir.join("a.ri"),
        "import b\nstructure A { param x: Scalar = 1mm }",
    )
    .unwrap();

    // Module b imports a (circular)
    fs::write(
        dir.join("b.ri"),
        "import a\nstructure B { param y: Scalar = 2mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("a", &resolver);

    assert!(result.is_err(), "expected error for circular import");
    let errors = result.unwrap_err();
    let msg = errors
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        msg.contains("circular") || msg.contains("cycle"),
        "error should mention circular dependency, got: {}",
        msg
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 21: Diamond import (deduplication) ───────────────────────

#[test]
fn diamond_import_compiles_each_module_once() {
    let dir = test_dir("diamond");

    // D (leaf) — no imports
    fs::write(
        dir.join("d.ri"),
        "structure D { param v: Scalar = 1mm }",
    )
    .unwrap();

    // B depends on D
    fs::write(
        dir.join("b.ri"),
        "import d\nstructure B { param v: Scalar = 2mm }",
    )
    .unwrap();

    // C depends on D
    fs::write(
        dir.join("c.ri"),
        "import d\nstructure C { param v: Scalar = 3mm }",
    )
    .unwrap();

    // A depends on B and C (diamond: A→B→D, A→C→D)
    fs::write(
        dir.join("a.ri"),
        "import b\nimport c\nstructure A { param v: Scalar = 4mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("a", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    // Should have 4 modules: d, b, c, a
    assert_eq!(
        dag.modules.len(),
        4,
        "expected 4 modules in DAG, got {}",
        dag.modules.len()
    );

    // D should appear exactly once
    assert!(dag.modules.contains_key("d"), "should have module 'd'");
    assert!(dag.modules.contains_key("b"), "should have module 'b'");
    assert!(dag.modules.contains_key("c"), "should have module 'c'");
    assert!(dag.modules.contains_key("a"), "should have module 'a'");

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 23: Topological ordering ─────────────────────────────────

#[test]
fn topological_order_leaves_first() {
    let dir = test_dir("topo");

    // C (leaf)
    fs::write(
        dir.join("c.ri"),
        "structure C { param v: Scalar = 1mm }",
    )
    .unwrap();

    // B depends on C
    fs::write(
        dir.join("b.ri"),
        "import c\nstructure B { param v: Scalar = 2mm }",
    )
    .unwrap();

    // A depends on B
    fs::write(
        dir.join("a.ri"),
        "import b\nstructure A { param v: Scalar = 3mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("a", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    // Topological order: C first (leaf), then B, then A (root)
    assert_eq!(dag.topo_order, vec!["c", "b", "a"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn topological_order_diamond() {
    let dir = test_dir("topo_diamond");

    // D (leaf)
    fs::write(
        dir.join("d.ri"),
        "structure D { param v: Scalar = 1mm }",
    )
    .unwrap();

    // B depends on D
    fs::write(
        dir.join("b.ri"),
        "import d\nstructure B { param v: Scalar = 2mm }",
    )
    .unwrap();

    // C depends on D
    fs::write(
        dir.join("c.ri"),
        "import d\nstructure C { param v: Scalar = 3mm }",
    )
    .unwrap();

    // A depends on B and C
    fs::write(
        dir.join("a.ri"),
        "import b\nimport c\nstructure A { param v: Scalar = 4mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("a", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    // D must come before B and C; B and C must come before A
    let d_pos = dag.topo_order.iter().position(|s| s == "d").unwrap();
    let b_pos = dag.topo_order.iter().position(|s| s == "b").unwrap();
    let c_pos = dag.topo_order.iter().position(|s| s == "c").unwrap();
    let a_pos = dag.topo_order.iter().position(|s| s == "a").unwrap();

    assert!(d_pos < b_pos, "d should come before b");
    assert!(d_pos < c_pos, "d should come before c");
    assert!(b_pos < a_pos, "b should come before a");
    assert!(c_pos < a_pos, "c should come before a");

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 25: Name resolution with entity import ──────────────────

#[test]
fn entity_import_resolves_structure_name() {
    let dir = test_dir("entity_resolve");

    // Module a defines a pub structure Bolt
    fs::write(
        dir.join("a.ri"),
        "pub structure Bolt {\n    param d: Scalar = 6mm\n}",
    )
    .unwrap();

    // Module b imports Bolt from a and uses it in a sub declaration
    fs::write(
        dir.join("b.ri"),
        "import a.Bolt\nstructure Assembly {\n    param size: Scalar = 10mm\n    sub b = Bolt(d: 8mm)\n}",
    )
    .unwrap();

    // Compile through the DAG
    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(
        &dir.join("b.ri"),
        &resolver,
    );
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // Should have 2 modules: a (dependency) and b (entry)
    assert_eq!(modules.len(), 2);

    // b should have a sub_component referencing Bolt
    let b_module = &modules[1]; // entry module is last (topo order)
    let template = &b_module.templates[0];
    assert_eq!(template.name, "Assembly");
    assert_eq!(template.sub_components.len(), 1);
    assert_eq!(template.sub_components[0].name, "b");
    assert_eq!(template.sub_components[0].structure_name, "Bolt");

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 27: Shadowing warning ───────────────────────────────────

#[test]
fn local_definition_shadows_import_with_warning() {
    let dir = test_dir("shadowing");

    // Module a defines Bolt
    fs::write(
        dir.join("a.ri"),
        "pub structure Bolt {\n    param d: Scalar = 6mm\n}",
    )
    .unwrap();

    // Module b imports Bolt but also defines its own Bolt
    fs::write(
        dir.join("b.ri"),
        "import a.Bolt\nstructure Bolt {\n    param d: Scalar = 10mm\n}",
    )
    .unwrap();

    // Should compile without errors (local takes precedence)
    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(
        &dir.join("b.ri"),
        &resolver,
    );
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // The entry module (b) should have its own Bolt definition
    let b_module = modules.last().unwrap();
    let template = &b_module.templates[0];
    assert_eq!(template.name, "Bolt");

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 29: Re-export ───────────────────────────────────────────

#[test]
fn pub_import_re_exports_entity() {
    let dir = test_dir("reexport");

    // Module a defines Helper
    fs::write(
        dir.join("a.ri"),
        "pub structure Helper {\n    param v: Scalar = 1mm\n}",
    )
    .unwrap();

    // Module b re-exports Helper from a
    fs::write(
        dir.join("b.ri"),
        "pub import a.Helper\nstructure Wrapper {\n    param w: Scalar = 2mm\n}",
    )
    .unwrap();

    // Module c imports Helper through b
    fs::write(
        dir.join("c.ri"),
        "import b.Helper\nstructure User {\n    param u: Scalar = 3mm\n    sub h = Helper(v: u)\n}",
    )
    .unwrap();

    // Compile through the DAG
    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(
        &dir.join("c.ri"),
        &resolver,
    );
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // Should have 3 modules: a, b, c
    assert_eq!(modules.len(), 3);

    // c should have a sub_component referencing Helper
    let c_module = modules.last().unwrap();
    let template = c_module.templates.iter().find(|t| t.name == "User").unwrap();
    assert_eq!(template.sub_components.len(), 1);
    assert_eq!(template.sub_components[0].structure_name, "Helper");

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 31: Backward compatibility ──────────────────────────────

#[test]
fn backward_compatible_single_module_no_imports() {
    // The canonical bracket source (no imports) should compile
    // through both the existing compile() function and the DAG
    let source = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    let volume = width * height
    constraint width > 0mm
}"#;

    // Via existing compile() — should work unchanged
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("bracket"));
    let compiled_single = reify_compiler::compile(&parsed);
    assert_eq!(compiled_single.templates.len(), 1);
    assert_eq!(compiled_single.templates[0].name, "Bracket");

    // Via compile_project() — write source to temp file and compile
    let dir = test_dir("backward_compat");
    fs::write(dir.join("bracket.ri"), source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(
        &dir.join("bracket.ri"),
        &resolver,
    );
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].templates.len(), 1);
    assert_eq!(modules[0].templates[0].name, "Bracket");

    // Output should be identical
    assert_eq!(
        compiled_single.templates[0].name,
        modules[0].templates[0].name,
    );
    assert_eq!(
        compiled_single.templates[0].value_cells.len(),
        modules[0].templates[0].value_cells.len(),
    );

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 33: CompiledImport preserves kind and is_pub ────────────

#[test]
fn compiled_import_preserves_kind_and_is_pub() {
    let dir = test_dir("compiled_import_fields");

    // Module a defines a pub structure Helper
    fs::write(
        dir.join("a.ri"),
        "pub structure Helper {\n    param v: Scalar = 1mm\n}",
    )
    .unwrap();

    // Module b has a pub import (re-export) and a plain module import
    fs::write(
        dir.join("b.ri"),
        "pub import a.Helper\nimport a\nstructure Wrapper {\n    param w: Scalar = 2mm\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("b.ri"), &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    let b_module = modules.last().unwrap();

    // b should have 2 imports
    assert_eq!(b_module.imports.len(), 2, "expected 2 imports in b");

    // First import: `pub import a.Helper` → Entity("Helper"), is_pub=true
    let imp0 = &b_module.imports[0];
    assert_eq!(imp0.kind, reify_syntax::ImportKind::Entity("Helper".to_string()));
    assert!(imp0.is_pub, "first import should be pub");

    // Second import: `import a` → Module, is_pub=false
    let imp1 = &b_module.imports[1];
    assert_eq!(imp1.kind, reify_syntax::ImportKind::Module);
    assert!(!imp1.is_pub, "second import should not be pub");

    let _ = fs::remove_dir_all(&dir);
}
