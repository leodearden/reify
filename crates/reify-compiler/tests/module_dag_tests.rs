//! Tests for module DAG: cycle detection, diamond dedup, topological ordering.

mod common;

use std::fs;

use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
use reify_compiler::stdlib_loader;
use reify_core::Severity;

// ── Step 19: Circular import detection ────────────────────────────

#[test]
fn circular_import_detected() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

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
}

// ── Step 21: Diamond import (deduplication) ───────────────────────

#[test]
fn diamond_import_compiles_each_module_once() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // D (leaf) — no imports
    fs::write(dir.join("d.ri"), "structure D { param v: Scalar = 1mm }").unwrap();

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
}

// ── Step 23: Topological ordering ─────────────────────────────────

#[test]
fn topological_order_leaves_first() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // C (leaf)
    fs::write(dir.join("c.ri"), "structure C { param v: Scalar = 1mm }").unwrap();

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
}

#[test]
fn topological_order_diamond() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // D (leaf)
    fs::write(dir.join("d.ri"), "structure D { param v: Scalar = 1mm }").unwrap();

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
}

// ── Step 25: Name resolution with entity import ──────────────────

#[test]
fn entity_import_resolves_structure_name() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

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
    let result = reify_compiler::module_dag::compile_project(&dir.join("b.ri"), &resolver);
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
}

// ── Step 27: Shadowing warning ───────────────────────────────────

#[test]
fn local_definition_shadows_import_with_warning() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

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
    let result = reify_compiler::module_dag::compile_project(&dir.join("b.ri"), &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // The entry module (b) should have its own Bolt definition
    let b_module = modules.last().unwrap();
    let template = &b_module.templates[0];
    assert_eq!(template.name, "Bolt");
}

// ── Step 29: Re-export ───────────────────────────────────────────

#[test]
fn pub_import_re_exports_entity() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

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
    let result = reify_compiler::module_dag::compile_project(&dir.join("c.ri"), &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // Should have 3 modules: a, b, c
    assert_eq!(modules.len(), 3);

    // c should have a sub_component referencing Helper
    let c_module = modules.last().unwrap();
    let template = c_module
        .templates
        .iter()
        .find(|t| t.name == "User")
        .unwrap();
    assert_eq!(template.sub_components.len(), 1);
    assert_eq!(template.sub_components[0].structure_name, "Helper");
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    let compiled_single = reify_compiler::compile(&parsed);
    assert_eq!(compiled_single.templates.len(), 1);
    assert_eq!(compiled_single.templates[0].name, "Bracket");

    // Via compile_project() — write source to temp file and compile
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();
    fs::write(dir.join("bracket.ri"), source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("bracket.ri"), &resolver);
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
}

// ── Step 35: in_progress cleanup on error ────────────────────────

#[test]
fn dag_recovers_after_parse_error_in_dependency() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Module a imports b
    fs::write(
        dir.join("a.ri"),
        "import b\nstructure A { param x: Scalar = 1mm }",
    )
    .unwrap();

    // Module b has invalid syntax (parse error)
    fs::write(dir.join("b.ri"), "this is not valid reify syntax !!!").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();

    // First attempt: should fail because b has parse errors
    let result1 = dag.compile_module("a", &resolver);
    assert!(
        result1.is_err(),
        "first attempt should fail due to parse error in b"
    );

    // Fix module b on disk
    fs::write(dir.join("b.ri"), "structure B { param y: Scalar = 2mm }").unwrap();

    // Second attempt on the SAME dag instance: should succeed
    // This proves in_progress was cleaned up after the first failure
    let result2 = dag.compile_module("a", &resolver);
    assert!(
        result2.is_ok(),
        "second attempt should succeed after fixing b, got: {:?}",
        result2.unwrap_err()
    );

    // Verify both modules are in the DAG
    assert!(dag.modules.contains_key("a"), "should have module 'a'");
    assert!(dag.modules.contains_key("b"), "should have module 'b'");
}

// ── Step 33: CompiledImport preserves kind and is_pub ────────────

#[test]
fn compiled_import_preserves_kind_and_is_pub() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

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
    assert_eq!(
        imp0.kind,
        reify_ast::ImportKind::Entity("Helper".to_string())
    );
    assert!(imp0.is_pub, "first import should be pub");

    // Second import: `import a` → Module, is_pub=false
    let imp1 = &b_module.imports[1];
    assert_eq!(imp1.kind, reify_ast::ImportKind::Module);
    assert!(!imp1.is_pub, "second import should not be pub");
}

// ── step-2 (task-1074): cross-module unit collision names the source module ───

/// When a module redeclares a unit exported by an imported user module,
/// the diagnostic should:
/// (a) mention the source module name ('dep'), NOT 'stdlib',
/// (b) contain 'duplicate' and the unit name 'myunit',
/// (c) have no label with SourceSpan::empty(0).
#[test]
fn compile_project_detects_cross_module_unit_collision() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // dep.ri: exports pub unit 'myunit'
    fs::write(dir.join("dep.ri"), "pub unit myunit : Length = 0.001").unwrap();

    // main.ri: imports dep and redeclares 'myunit'
    fs::write(
        dir.join("main.ri"),
        "import dep\nunit myunit : Length = 0.002",
    )
    .unwrap();

    let errors = common::compile_errors(&dir, "main.ri");

    // There should be a duplicate-unit error
    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("myunit"));
    assert!(
        dup_diag.is_some(),
        "expected a 'duplicate myunit' error, got: {:?}",
        errors
    );
    let dup_diag = dup_diag.unwrap();

    // (a) should mention 'dep' module name
    assert!(
        dup_diag.message.contains("dep"),
        "expected 'dep' in diagnostic message, got: {:?}",
        dup_diag.message
    );

    // (b) should NOT contain 'stdlib'
    assert!(
        !dup_diag.message.contains("stdlib"),
        "diagnostic should NOT mention 'stdlib' for user module collision, got: {:?}",
        dup_diag.message
    );

    // (c) Two labels: labels[0] is the user's in-file dup decl span;
    // labels[1] is the prelude sentinel with provenance in its message.
    common::assert_prelude_collision_labels(dup_diag);
}

// ── step-3 (task-1575): compile_project-level stdlib unit collision mentions stdlib ───

/// When the entry module redeclares a unit that was imported from a stdlib
/// module via `compile_project`, the diagnostic should:
/// (a) contain 'duplicate' and the unit name ('myunit'),
/// (b) mention 'stdlib prelude' — NOT a bare module name — because the
///     source module path starts with "std/" which triggers the stdlib branch
///     in `compile_with_prelude`,
/// (c) have no label with SourceSpan::empty(0) (the misleading prelude sentinel).
///
/// This exercises the stdlib collision path through the full `compile_project`
/// pipeline, complementing the unit-registry-level test in `unit_registry_tests.rs`
/// (`prelude_unit_collision_diagnostic_mentions_stdlib`).
#[test]
fn compile_project_stdlib_unit_collision_mentions_stdlib() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Minimal stdlib: a single pub unit.  The resolver maps `std.*` imports
    // to files under `<stdlib_root>/`, so `import std.units` resolves to
    // `<stdlib_root>/units.ri`.
    let stdlib_dir = dir.join("stdlib");
    fs::create_dir_all(&stdlib_dir).unwrap();
    fs::write(
        stdlib_dir.join("units.ri"),
        "pub unit myunit : Length = 1.0",
    )
    .unwrap();

    // main.ri: imports std.units then re-declares 'myunit' — stdlib collision.
    fs::write(
        dir.join("main.ri"),
        "import std.units\nunit myunit : Length = 2.0",
    )
    .unwrap();

    let errors = common::compile_errors_with_stdlib(&dir, "main.ri", &stdlib_dir);

    // (a) There must be a duplicate-unit error mentioning 'myunit'
    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("myunit"));
    assert!(
        dup_diag.is_some(),
        "expected a 'duplicate myunit' error, got: {:?}",
        errors
    );
    let dup_diag = dup_diag.unwrap();

    // (b) The message must say 'stdlib prelude' …
    assert!(
        dup_diag.message.contains("stdlib prelude"),
        "expected 'stdlib prelude' in diagnostic message, got: {:?}",
        dup_diag.message
    );

    // (b) … and must NOT use the user-module phrasing ("already defined in module '…'")
    assert!(
        !dup_diag.message.contains("already defined in module '"),
        "diagnostic should NOT use user-module phrasing for stdlib collision, got: {:?}",
        dup_diag.message
    );

    // (c) Two labels: labels[0] is the user's in-file dup decl (non-empty, not
    //     empty(0)); labels[1] is the prelude sentinel carrying provenance text.
    assert_eq!(
        dup_diag.labels.len(),
        2,
        "stdlib collision should emit two labels, got {:?}",
        dup_diag.labels
    );
    let empty_span = reify_core::SourceSpan::empty(0);
    assert_ne!(
        dup_diag.labels[0].span, empty_span,
        "first label '{}' must not be SourceSpan::empty(0)",
        dup_diag.labels[0].message
    );
    assert!(
        dup_diag.labels[1].span.is_prelude(),
        "second label '{}' must have is_prelude() span, got {:?}",
        dup_diag.labels[1].message,
        dup_diag.labels[1].span
    );
}

// ── step-1 (task-1392): prelude propagation via compile_module ────────────────

/// Pins the compile_module prelude-collection path (the loop that filters
/// import declarations and looks up already-compiled modules).
///
/// Module c defines a pub structure `Part`. Module b imports c.Part and
/// references `Part` in a sub declaration. Compiling via dag.compile_module
/// must produce a compiled b with one sub_component whose structure_name
/// is "Part", proving the prelude was collected and propagated correctly.
#[test]
fn compile_module_prelude_propagates_pub_structure() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Module c: defines pub structure Part
    fs::write(
        dir.join("c.ri"),
        "pub structure Part {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();

    // Module b: imports c.Part and uses it in a sub declaration
    fs::write(
        dir.join("b.ri"),
        "import c.Part\nstructure B {\n    sub p = Part(x: 5mm)\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("b", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    // b should have exactly one template: B
    let b_module = dag.modules.get("b").expect("module 'b' should be in DAG");
    assert_eq!(b_module.templates.len(), 1, "expected 1 template in b");
    let template = &b_module.templates[0];
    assert_eq!(template.name, "B");

    // B should have one sub_component referencing Part
    assert_eq!(
        template.sub_components.len(),
        1,
        "expected 1 sub_component, got {:?}",
        template
            .sub_components
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        template.sub_components[0].structure_name, "Part",
        "sub_component structure_name should be 'Part'"
    );
}

// ── step-2 (task-1392): multi-import prelude via compile_module ───────────────

/// Pins the collect_import_preludes behaviour for multiple imports.
///
/// Modules x and y each define a pub structure (Bolt and Nut). Module z
/// imports both and references each in a sub declaration. Compiling via
/// dag.compile_module('z') must produce a compiled z with 2 sub_components,
/// proving that collect_import_preludes handles multiple imports correctly.
#[test]
fn compile_module_multi_import_prelude() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Module x: pub structure Bolt
    fs::write(
        dir.join("x.ri"),
        "pub structure Bolt {\n    param d: Scalar = 6mm\n}",
    )
    .unwrap();

    // Module y: pub structure Nut
    fs::write(
        dir.join("y.ri"),
        "pub structure Nut {\n    param d: Scalar = 6mm\n}",
    )
    .unwrap();

    // Module z: imports both and uses each in a sub declaration
    fs::write(
        dir.join("z.ri"),
        "import x.Bolt\nimport y.Nut\nstructure Assembly {\n    sub b = Bolt(d: 8mm)\n    sub n = Nut(d: 8mm)\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("z", &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    // z should have exactly one template: Assembly
    let z_module = dag.modules.get("z").expect("module 'z' should be in DAG");
    assert_eq!(z_module.templates.len(), 1, "expected 1 template in z");
    let template = &z_module.templates[0];
    assert_eq!(template.name, "Assembly");

    // Assembly should have 2 sub_components: Bolt and Nut
    assert_eq!(
        template.sub_components.len(),
        2,
        "expected 2 sub_components, got {:?}",
        template
            .sub_components
            .iter()
            .map(|s| &s.structure_name)
            .collect::<Vec<_>>()
    );

    let structure_names: Vec<&str> = template
        .sub_components
        .iter()
        .map(|s| s.structure_name.as_str())
        .collect();
    assert!(
        structure_names.contains(&"Bolt"),
        "expected sub_component with structure_name 'Bolt', got {:?}",
        structure_names
    );
    assert!(
        structure_names.contains(&"Nut"),
        "expected sub_component with structure_name 'Nut', got {:?}",
        structure_names
    );
}

// ── task-370/step-9: circular_import_error_message_deterministic ──────────────

/// Validate that circular-import error messages are deterministic.
///
/// Creates a three-module cycle: cherry -> apple -> banana -> cherry.
/// Module names are chosen so alphabetical order (apple, banana, cherry)
/// DIFFERS from DFS traversal order (cherry, apple, banana).  This ensures
/// IndexSet insertion-order is the source of determinism, not alphabetical sorting.
///
/// Compiles twice (with independent `ModuleDag` instances) and asserts:
/// (a) both runs produce an error mentioning "circular" or "cycle",
/// (b) the error messages are identical between runs, and
/// (c) the module names appear in DFS traversal order within the message
///     ("cherry" before "apple" before "banana").
///
/// IndexSet preserves insertion order (= DFS traversal order), so the message
/// reads "cherry -> apple -> banana -> cherry" — showing the actual import chain.
#[test]
fn circular_import_error_message_deterministic() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // cherry imports apple (DFS entry point)
    fs::write(
        dir.join("cherry.ri"),
        "import apple\nstructure Cherry { param x: Scalar = 1mm }",
    )
    .unwrap();

    // apple imports banana
    fs::write(
        dir.join("apple.ri"),
        "import banana\nstructure Apple { param y: Scalar = 2mm }",
    )
    .unwrap();

    // banana imports cherry (closes the cycle: cherry -> apple -> banana -> cherry)
    fs::write(
        dir.join("banana.ri"),
        "import cherry\nstructure Banana { param z: Scalar = 3mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));

    // First compilation — DFS visits cherry, apple, banana, then detects cherry again.
    // in_progress = [cherry, apple, banana] (insertion order); message: cherry -> apple -> banana -> cherry.
    let mut dag1 = ModuleDag::new();
    let result1 = dag1.compile_module("cherry", &resolver);
    assert!(result1.is_err(), "expected error for 3-cycle (first run)");
    let msg1 = result1
        .unwrap_err()
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        msg1.contains("circular") || msg1.contains("cycle"),
        "first run: error should mention circular dependency, got: {}",
        msg1
    );

    // Second compilation (fresh dag)
    let mut dag2 = ModuleDag::new();
    let result2 = dag2.compile_module("cherry", &resolver);
    assert!(result2.is_err(), "expected error for 3-cycle (second run)");
    let msg2 = result2
        .unwrap_err()
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // (b) Messages must be identical between runs (determinism)
    assert_eq!(
        msg1, msg2,
        "circular-import error messages must be identical across compilations"
    );

    // (c) Module names must appear in DFS traversal order within the message.
    // DFS order: cherry < apple < banana (cherry is the entry, then apple, then banana).
    // The message should read "cherry -> apple -> banana -> cherry".
    // cherry appears twice (start and end of the arrow chain); take the first occurrence.
    let cherry_pos = msg1.find("cherry").expect("'cherry' must appear in error");
    let apple_pos = msg1.find("apple").expect("'apple' must appear in error");
    let banana_pos = msg1.find("banana").expect("'banana' must appear in error");
    assert!(
        cherry_pos < apple_pos,
        "expected 'cherry' before 'apple' (DFS traversal order), got: {}",
        msg1
    );
    assert!(
        apple_pos < banana_pos,
        "expected 'apple' before 'banana' (DFS traversal order), got: {}",
        msg1
    );
}

// ── amendment (task-1392): compile_project multi-import prelude path ──────────

/// Covers the block-scoped prelude path in `compile_project` (lines 257-260 in
/// module_dag.rs) with multiple imports.
///
/// Modules p and q each define a pub structure (Pin and Socket). Module r imports
/// both and references each in a sub declaration. `compile_project` on r must
/// return a compiled entry module with 2 sub_components, proving that:
///   1. `collect_import_preludes` aggregates both imports, and
///   2. the block-scoped borrow in `compile_project` is correct under multiple
///      simultaneously-borrowed prelude modules.
#[test]
fn compile_project_multi_import_prelude() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Module p: pub structure Pin
    fs::write(
        dir.join("p.ri"),
        "pub structure Pin {\n    param d: Scalar = 2mm\n}",
    )
    .unwrap();

    // Module q: pub structure Socket
    fs::write(
        dir.join("q.ri"),
        "pub structure Socket {\n    param d: Scalar = 2mm\n}",
    )
    .unwrap();

    // Entry module r: imports both and uses each in a sub declaration
    fs::write(
        dir.join("r.ri"),
        "import p.Pin\nimport q.Socket\nstructure Connector {\n    sub pin = Pin(d: 3mm)\n    sub sock = Socket(d: 3mm)\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project(&dir.join("r.ri"), &resolver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());

    let modules = result.unwrap();
    // Should have 3 modules in topo order: p, q, r
    assert_eq!(
        modules.len(),
        3,
        "expected 3 modules (p, q, r), got {}",
        modules.len()
    );

    // Entry module r is last (topological order: dependencies first)
    let r_module = modules.last().unwrap();
    assert_eq!(r_module.templates.len(), 1, "expected 1 template in r");
    let template = &r_module.templates[0];
    assert_eq!(template.name, "Connector");

    // Connector should have 2 sub_components: Pin and Socket
    assert_eq!(
        template.sub_components.len(),
        2,
        "expected 2 sub_components, got {:?}",
        template
            .sub_components
            .iter()
            .map(|s| &s.structure_name)
            .collect::<Vec<_>>()
    );

    let structure_names: Vec<&str> = template
        .sub_components
        .iter()
        .map(|s| s.structure_name.as_str())
        .collect();
    assert!(
        structure_names.contains(&"Pin"),
        "expected sub_component with structure_name 'Pin', got {:?}",
        structure_names
    );
    assert!(
        structure_names.contains(&"Socket"),
        "expected sub_component with structure_name 'Socket', got {:?}",
        structure_names
    );
}

// ── step-1 (task-1759): #no_prelude suppresses import prelude pub units ───────

/// Verifies that `#no_prelude` suppresses the import prelude so that pub units
/// from imported modules are NOT seeded into the unit registry.
///
/// dep defines `pub unit myunit : Length = 0.001`.  consumer declares
/// `#no_prelude`, imports dep, and references `5myunit` in a param.
/// Because `#no_prelude` shadows the prelude with `&[]` (lib.rs:131), the
/// unit lookup fails and the consumer module must carry an "unknown unit"
/// diagnostic for `myunit`.
///
/// This exercises the lib.rs:130-131 empty-slice shadowing path through the
/// full `compile_project` pipeline.
#[test]
fn no_prelude_suppresses_import_prelude() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // dep.ri: exports pub unit myunit
    fs::write(
        dir.join("dep.ri"),
        "pub unit myunit : Length = 0.001\npub structure Part {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();

    // consumer.ri: #no_prelude suppresses dep's pub units
    fs::write(
        dir.join("consumer.ri"),
        "#no_prelude\nimport dep\nstructure S {\n    param y: Length = 5myunit\n}",
    )
    .unwrap();

    let errors = common::compile_errors(&dir, "consumer.ri");
    let unknown_unit_diag = errors
        .iter()
        .find(|d| d.message.contains("unknown unit") && d.message.contains("myunit"));
    assert!(
        unknown_unit_diag.is_some(),
        "expected 'unknown unit: myunit' error (prelude suppressed by #no_prelude), got: {:#?}",
        errors
    );

    // Positive control: WITHOUT #no_prelude the same import/unit resolves fine.
    let _tmp2 = tempfile::tempdir().unwrap();
    let dir2 = _tmp2.path().to_path_buf();
    fs::write(
        dir2.join("dep.ri"),
        "pub unit myunit : Length = 0.001\npub structure Part {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();
    fs::write(
        dir2.join("consumer.ri"),
        "import dep\nstructure S {\n    param y: Length = 5myunit\n}",
    )
    .unwrap();
    let errors2 = common::compile_errors(&dir2, "consumer.ri");
    assert!(
        errors2.is_empty(),
        "positive control (no #no_prelude): expected zero errors but got: {:#?}",
        errors2
    );
}

// ── step-3 (task-1759): private units not exported through reference-based prelude ──

/// Verifies that the `if cu.is_pub` filter (lib.rs:266) correctly blocks
/// private units from being seeded into a consumer module's unit registry,
/// even through the reference-based prelude indirection introduced in task 1392.
///
/// dep defines a private `unit secret : Length = 0.005` (no pub keyword).
/// consumer imports dep and references `3secret` in a param.
/// Because `secret` is not pub, the is_pub filter prevents it from being
/// seeded, and the consumer module must carry an "unknown unit" diagnostic.
#[test]
fn private_unit_not_exported_through_import_prelude() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // dep.ri: private unit (no pub) and a pub structure to make import valid
    fs::write(
        dir.join("dep.ri"),
        "unit secret : Length = 0.005\npub structure Widget {\n    param w: Scalar = 1mm\n}",
    )
    .unwrap();

    // consumer.ri: imports dep and tries to use the private unit
    fs::write(
        dir.join("consumer.ri"),
        "import dep\nstructure S {\n    param z: Length = 3secret\n}",
    )
    .unwrap();

    let errors = common::compile_errors(&dir, "consumer.ri");
    let unknown_unit_diag = errors
        .iter()
        .find(|d| d.message.contains("unknown unit") && d.message.contains("secret"));
    assert!(
        unknown_unit_diag.is_some(),
        "expected 'unknown unit: secret' error (private unit not exported), got: {:#?}",
        errors
    );

    // Positive control: with `pub unit secret` the unit resolves and there are no errors.
    let _tmp2 = tempfile::tempdir().unwrap();
    let dir2 = _tmp2.path().to_path_buf();
    fs::write(
        dir2.join("dep.ri"),
        "pub unit secret : Length = 0.005\npub structure Widget {\n    param w: Scalar = 1mm\n}",
    )
    .unwrap();
    fs::write(
        dir2.join("consumer.ri"),
        "import dep\nstructure S {\n    param z: Length = 3secret\n}",
    )
    .unwrap();
    let errors2 = common::compile_errors(&dir2, "consumer.ri");
    assert!(
        errors2.is_empty(),
        "positive control (pub unit): expected zero errors but got: {:#?}",
        errors2
    );
}

// ── step-2 (task-1833): cycle_error_excludes_non_cycle_ancestors ─────────────

/// Validates that cycle detection excludes non-cycle ancestors from the error
/// message, showing only the modules that are part of the actual cycle.
///
/// Creates a 4-module graph: d -> a -> b -> a
///   - d is an ancestor that triggers the DFS, but is NOT part of the cycle
///   - a -> b -> a is the actual cycle
///
/// The error message should show exactly "a -> b -> a" without mentioning "d".
/// IndexSet::get_index_of finds where 'a' starts in in_progress ([d, a, b]),
/// then slices from that index to exclude the non-cycle ancestor.
#[test]
fn cycle_error_excludes_non_cycle_ancestors() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // d imports a (DFS entry; d is NOT in the cycle)
    fs::write(
        dir.join("d.ri"),
        "import a\nstructure D { param v: Scalar = 4mm }",
    )
    .unwrap();

    // a imports b
    fs::write(
        dir.join("a.ri"),
        "import b\nstructure A { param v: Scalar = 1mm }",
    )
    .unwrap();

    // b imports a (closes the cycle: a -> b -> a)
    fs::write(
        dir.join("b.ri"),
        "import a\nstructure B { param v: Scalar = 2mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("d", &resolver);
    assert!(
        result.is_err(),
        "expected error for cycle a->b->a triggered via d"
    );

    let msg = result
        .unwrap_err()
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // (a) error mentions circular/cycle
    assert!(
        msg.contains("circular") || msg.contains("cycle"),
        "error should mention circular dependency, got: {}",
        msg
    );

    // (b) message is exactly the cycle chain — this positive assertion proves 'd'
    // is absent without fragile per-pattern negative checks.
    assert_eq!(
        msg, "circular dependency detected: a -> b -> a",
        "message should contain exactly the cycle, not non-cycle ancestors"
    );
}

// ── step-1 (task-1852): cycle_error_preserves_dfs_traversal_order ────────────

/// Validates that the cycle error message reflects DFS traversal order (as
/// preserved by IndexSet) and NOT alphabetical order.
///
/// Creates a 3-module cycle: zebra -> middle -> alpha -> zebra
///   - Alphabetical order: alpha, middle, zebra
///   - DFS traversal order: zebra, middle, alpha  (i.e. reversed)
///
/// An alphabetically-sorted (or HashSet-backed) implementation cannot produce
/// the expected message "zebra -> middle -> alpha -> zebra", so this test
/// serves as a direct regression guard for the IndexSet guarantee introduced
/// in task-1833.
#[test]
fn cycle_error_preserves_dfs_traversal_order() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // zebra imports middle (DFS entry point)
    fs::write(
        dir.join("zebra.ri"),
        "import middle\nstructure Zebra { param v: Scalar = 3mm }",
    )
    .unwrap();

    // middle imports alpha
    fs::write(
        dir.join("middle.ri"),
        "import alpha\nstructure Middle { param v: Scalar = 2mm }",
    )
    .unwrap();

    // alpha imports zebra (closes the cycle: zebra -> middle -> alpha -> zebra)
    fs::write(
        dir.join("alpha.ri"),
        "import zebra\nstructure Alpha { param v: Scalar = 1mm }",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("zebra", &resolver);
    assert!(
        result.is_err(),
        "expected error for cycle zebra->middle->alpha->zebra"
    );

    let msg = result
        .unwrap_err()
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // DFS traversal order (zebra, middle, alpha) — NOT alphabetical (alpha, middle, zebra).
    // If IndexSet is replaced with HashSet or BTreeSet this assertion will fail.
    assert_eq!(
        msg, "circular dependency detected: zebra -> middle -> alpha -> zebra",
        "message must reflect DFS insertion order, not alphabetical order"
    );
}

// ── step-7 (task-512): std.* delegation to embedded stdlib ────────────────────

/// When stdlib_root points at a non-existent directory, compile_module for a
/// `std.*` path must delegate to the embedded stdlib loader rather than
/// returning an fs-resolution error.
///
/// Today this fails because ModuleDag::compile_module attempts resolver.resolve_import_path
/// which returns Err when the stdlib_root directory is missing, and the error
/// propagates without any embedded-stdlib fallback.
///
/// After step-8 implements the fallback, this test turns green.
#[test]
fn std_module_import_delegates_to_embedded_stdlib_loader_when_stdlib_root_missing() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // nonexistent_stdlib intentionally does NOT exist on disk.
    let stdlib_root = dir.join("nonexistent_stdlib");
    assert!(
        !stdlib_root.exists(),
        "precondition: stdlib_root must not exist for this test"
    );

    let resolver = ModuleResolver::new(&dir, &stdlib_root);
    let mut dag = ModuleDag::new();

    // Attempting to compile std.units must succeed via the embedded stdlib fallback.
    let result = dag.compile_module("std.units", &resolver);
    assert!(
        result.is_ok(),
        "compile_module(\"std.units\") with missing stdlib_root should succeed via embedded fallback, \
         but got: {:?}",
        result.err()
    );

    // The compiled module must be stored in the DAG under its canonical dotted key.
    assert!(
        dag.modules.contains_key("std.units"),
        "dag.modules must contain \"std.units\" after successful delegation"
    );

    // The module's path must decode to ["std", "units"].
    let m = dag.modules.get("std.units").unwrap();
    assert_eq!(
        m.path.0,
        vec!["std".to_string(), "units".to_string()],
        "embedded std.units module must have path [\"std\", \"units\"]"
    );
}

// ── amend:task-512: embedded stdlib fallback adds transitive deps ─────────────

/// When `std.materials.mechanical` is requested via the embedded stdlib fallback
/// (stdlib_root is missing), all modules that appear before it in the stdlib
/// slice — i.e. its transitive compile-time deps — must also be added to
/// `modules` and `topo_order`.
///
/// Without this fix, consumers that iterate `topo_order` expecting to see every
/// transitively-imported stdlib module would only see `std.materials.mechanical`
/// but miss `std.units` and `std.si_units` (which it was compiled against).
#[test]
fn std_module_fallback_adds_transitive_dependencies_to_dag() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // nonexistent_stdlib intentionally does NOT exist on disk.
    let stdlib_root = dir.join("nonexistent_stdlib");
    assert!(
        !stdlib_root.exists(),
        "precondition: stdlib_root must not exist for this test"
    );

    let resolver = ModuleResolver::new(&dir, &stdlib_root);
    let mut dag = ModuleDag::new();

    // std.materials.mechanical is preceded by std.units and std.si_units in the
    // embedded stdlib slice (topological order); they are its transitive deps.
    let result = dag.compile_module("std.materials.mechanical", &resolver);
    assert!(
        result.is_ok(),
        "compile_module(\"std.materials.mechanical\") with missing stdlib_root should \
         succeed via embedded fallback, but got: {:?}",
        result.err()
    );

    // The directly-imported module must be in the DAG.
    assert!(
        dag.modules.contains_key("std.materials.mechanical"),
        "dag.modules must contain \"std.materials.mechanical\" after delegation"
    );

    // Transitive dependencies that std.materials.mechanical was compiled against
    // must also appear in both modules and topo_order.
    for dep in &["std.units", "std.si_units"] {
        assert!(
            dag.modules.contains_key(*dep),
            "dag.modules must contain transitive dep \"{}\" after delegating \
             embedded stdlib for std.materials.mechanical",
            dep
        );
        assert!(
            dag.topo_order.contains(&dep.to_string()),
            "topo_order must contain transitive dep \"{}\"",
            dep
        );
    }
    // The requested module itself must also be in topo_order.
    assert!(
        dag.topo_order
            .contains(&"std.materials.mechanical".to_string()),
        "topo_order must contain \"std.materials.mechanical\""
    );
}

// ── step-1 (task-2073): partial stdlib overlay is detected and errors ─────────

/// Verifies that mixing filesystem and embedded stdlib sources within a single
/// `ModuleDag` instance produces a clear diagnostic instead of silently mixing
/// incompatible stdlib versions.
///
/// Setup: stdlib dir contains `units.ri` (so `std.units` resolves via filesystem)
/// but does NOT contain `materials_mechanical.ri` (so `std.materials.mechanical`
/// would previously fall back to the embedded stdlib silently).
///
/// After the all-or-nothing fix:
/// - `compile_module("std.units")` must succeed (filesystem mode committed).
/// - `compile_module("std.materials.mechanical")` must fail with a diagnostic
///   that (a) mentions "std.materials.mechanical", (b) contains "overlay" or
///   "partial", and (c) references "filesystem" or "embedded".
///
/// This test FAILS today because the current code silently falls back to the
/// embedded stdlib for std.materials.mechanical (no overlay check).
#[test]
fn partial_stdlib_overlay_errors_when_fs_missing_later_module() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Create a partial stdlib dir: units.ri present, materials_mechanical.ri absent.
    // Use the real stdlib content so the compile is robust against semantic changes.
    let stdlib_dir = dir.join("stdlib");
    fs::create_dir_all(&stdlib_dir).unwrap();
    fs::write(
        stdlib_dir.join("units.ri"),
        include_str!("../stdlib/units.ri"),
    )
    .unwrap();
    // materials_mechanical.ri intentionally NOT written.

    let resolver = ModuleResolver::new(&dir, &stdlib_dir);
    let mut dag = ModuleDag::new();

    // First std.* import resolves via filesystem → commits to FileSystem mode.
    let result_units = dag.compile_module("std.units", &resolver);
    assert!(
        result_units.is_ok(),
        "compile_module(\"std.units\") should succeed via filesystem, got: {:?}",
        result_units.err()
    );

    // Second std.* import is not on the filesystem → partial overlay → must error.
    let result_mech = dag.compile_module("std.materials.mechanical", &resolver);
    assert!(
        result_mech.is_err(),
        "compile_module(\"std.materials.mechanical\") should fail because std.units was \
         already resolved from the filesystem but materials_mechanical.ri is absent; \
         the DAG must not silently mix sources"
    );

    let errors = result_mech.unwrap_err();
    let msg = errors
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // (a) Must name the offending module.
    assert!(
        msg.contains("std.materials.mechanical"),
        "diagnostic must mention the offending module 'std.materials.mechanical', got: {}",
        msg
    );

    // (b) Must mention the overlay / partial mix.
    assert!(
        msg.to_lowercase().contains("overlay") || msg.to_lowercase().contains("partial"),
        "diagnostic must mention 'overlay' or 'partial', got: {}",
        msg
    );

    // (c) Must reference the stdlib source (filesystem or embedded).
    assert!(
        msg.to_lowercase().contains("filesystem") || msg.to_lowercase().contains("embedded"),
        "diagnostic must reference 'filesystem' or 'embedded', got: {}",
        msg
    );
}

// ── amend (task-2073): reverse-direction overlay — embedded-first, then fs ───

/// Companion to `partial_stdlib_overlay_errors_when_fs_missing_later_module`.
/// That test checks the FileSystem-first → Embedded-miss path. This test checks
/// the Embedded-first → FileSystem-hit path: the `Ok(path) if is_std_path` arm
/// when `stdlib_mode == Some(Embedded)`.
///
/// Setup: stdlib dir contains `materials_mechanical.ri` but NOT `units.ri`.
/// - `compile_module("std.units")` → fs lookup fails → found in embedded stdlib →
///   commits Embedded mode → Ok.
/// - `compile_module("std.materials.mechanical")` → fs lookup succeeds →
///   `stdlib_mode == Some(Embedded)` → partial stdlib overlay error.
///
/// The diagnostic must (a) mention "std.materials.mechanical", (b) contain
/// "overlay" or "partial", and (c) reference "filesystem" or "embedded".
#[test]
fn partial_stdlib_overlay_errors_when_embedded_first_then_fs() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Create a partial stdlib dir: tolerancing.ri present, units.ri absent.
    // The resolver maps `std.tolerancing` → `stdlib_dir/tolerancing.ri` (simple path,
    // no subdirectory) and `std.units` → `stdlib_dir/units.ri` (also simple).
    // Only tolerancing.ri is written — units.ri is absent so std.units must fall back
    // to the embedded stdlib.
    let stdlib_dir = dir.join("stdlib");
    fs::create_dir_all(&stdlib_dir).unwrap();
    fs::write(
        stdlib_dir.join("tolerancing.ri"),
        include_str!("../stdlib/tolerancing.ri"),
    )
    .unwrap();
    // units.ri intentionally NOT written — std.units must fall back to embedded.

    let resolver = ModuleResolver::new(&dir, &stdlib_dir);
    let mut dag = ModuleDag::new();

    // First std.* import: fs fails for std.units → falls back to embedded → commits Embedded.
    let result_units = dag.compile_module("std.units", &resolver);
    assert!(
        result_units.is_ok(),
        "compile_module(\"std.units\") should succeed via embedded stdlib, got: {:?}",
        result_units.err()
    );

    // Second std.* import: fs succeeds for std.tolerancing (tolerancing.ri is present) but
    // stdlib_mode is Embedded → partial overlay → must error.
    let result_tol = dag.compile_module("std.tolerancing", &resolver);
    assert!(
        result_tol.is_err(),
        "compile_module(\"std.tolerancing\") should fail because std.units was already \
         resolved from the embedded stdlib but tolerancing.ri exists on the filesystem; \
         the DAG must not silently mix sources"
    );

    let errors = result_tol.unwrap_err();
    let msg = errors
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // (a) Must name the offending module.
    assert!(
        msg.contains("std.tolerancing"),
        "diagnostic must mention the offending module 'std.tolerancing', got: {}",
        msg
    );

    // (b) Must mention the overlay / partial mix.
    assert!(
        msg.to_lowercase().contains("overlay") || msg.to_lowercase().contains("partial"),
        "diagnostic must mention 'overlay' or 'partial', got: {}",
        msg
    );

    // (c) Must reference the stdlib source (filesystem or embedded).
    assert!(
        msg.to_lowercase().contains("filesystem") || msg.to_lowercase().contains("embedded"),
        "diagnostic must reference 'filesystem' or 'embedded', got: {}",
        msg
    );
}

// ── step-1 (task-2076): transitive-embedded overlay via deferred FileSystem commit ──

/// Regression guard for the deferred-commit overlay escape bug in `compile_module`.
///
/// Background: when the outer `compile_module("std.foo", &resolver)` finds `foo.ri`
/// on the filesystem it defers setting `stdlib_mode = Some(FileSystem)` until after
/// successful compilation. During compilation it recurses into `import std.units`;
/// since `units.ri` is absent from `stdlib_dir`, that inner call falls back to the
/// embedded stdlib and commits `stdlib_mode = Some(Embedded)`. When control returns
/// to the outer call, the deferred `if commit_fs_mode { self.stdlib_mode =
/// Some(FileSystem); }` unconditionally overwrites `Some(Embedded)` — exactly the
/// partial-overlay scenario the all-or-nothing invariant is meant to reject.
///
/// Setup: stdlib_dir contains `foo.ri` (imports std.units) but NOT `units.ri`.
/// - `compile_module("std.foo")` → fs succeeds for foo.ri → defers FileSystem commit.
/// - Recursion into `compile_module("std.units")` → fs fails → embedded fallback →
///   commits `stdlib_mode = Embedded`.
/// - On return, deferred commit tries to overwrite Embedded with FileSystem.
/// - Fixed code must detect the mismatch and return an overlay error.
///
/// Before the fix, this test would have failed: the deferred write silently
/// overwrote `Some(Embedded)` and `compile_module` returned `Ok`. The guarded
/// write now correctly returns `Err`.
#[test]
fn partial_stdlib_overlay_errors_when_outer_fs_and_inner_embedded() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Create a partial stdlib dir: foo.ri present (imports std.units), units.ri absent.
    let stdlib_dir = dir.join("stdlib");
    fs::create_dir_all(&stdlib_dir).unwrap();
    fs::write(
        stdlib_dir.join("foo.ri"),
        "import std.units\npub structure Foo { param v: Scalar = 1mm }",
    )
    .unwrap();
    // units.ri intentionally NOT written — std.units must fall back to embedded stdlib.

    let resolver = ModuleResolver::new(&dir, &stdlib_dir);
    let mut dag = ModuleDag::new();

    // std.foo is found on the filesystem (foo.ri present) but its transitive import
    // std.units falls back to embedded. This is the partial-overlay scenario.
    let result = dag.compile_module("std.foo", &resolver);
    assert!(
        result.is_err(),
        "compile_module(\"std.foo\") should fail because foo.ri resolved from the filesystem \
         but its transitive std.units import was served from the embedded stdlib; \
         the DAG must not silently mix sources, got: {:?}",
        result
    );

    let errors = result.unwrap_err();
    let msg = errors
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("; ");

    // (a) Must mention the overlay / partial mix.
    assert!(
        msg.to_lowercase().contains("overlay") || msg.to_lowercase().contains("partial"),
        "diagnostic must mention 'overlay' or 'partial', got: {}",
        msg
    );

    // (b) Must reference the stdlib source (filesystem or embedded).
    assert!(
        msg.to_lowercase().contains("filesystem") || msg.to_lowercase().contains("embedded"),
        "diagnostic must reference 'filesystem' or 'embedded', got: {}",
        msg
    );

    // (c) Must name the offending module (mirrors sibling test assertion shape).
    assert!(
        msg.contains("std.foo"),
        "diagnostic must name the offending module 'std.foo', got: {}",
        msg
    );

    // (d) Must contain the structural kind marker that only the deferred-commit branch
    // emits, tying this regression test to the specific bug path it guards rather than
    // the sibling entry-guard diagnostic (which emits '(fs-over-embedded/entry)').
    assert!(
        msg.contains("(fs-over-embedded/transitive)"),
        "diagnostic must contain the structural kind marker '(fs-over-embedded/transitive)' \
         to distinguish the deferred-commit branch from the entry-guard branch, got: {}",
        msg
    );
}

// ── step-3 (task-2073): sequential embedded fallbacks don't duplicate in topo_order ──

/// Regression guard for the backward-walk short-circuit in the embedded stdlib
/// fallback. Calls `compile_module` twice with overlapping prefix requirements,
/// using a nonexistent `stdlib_root` so ALL `std.*` compilation goes through the
/// embedded fallback path:
///
/// 1. First target is `stdlib[2]` — inserts the contiguous head `stdlib[0..=2]`.
/// 2. Second target is `stdlib[len-1]` — inserts the remaining suffix
///    `stdlib[3..=len-1]`.
///
/// Together the two calls cover `stdlib[0..len]` with disjoint but contiguous
/// prefix requests. After both calls `dag.topo_order` must equal the dotted
/// paths of `load_stdlib()` in order, with no duplicates and no gaps.
///
/// This test passes under the OLD forward-walk-with-contains_key guard too;
/// it is a pinning test that will fail if the backward-walk short-circuit
/// in module_dag.rs has an off-by-one bug (e.g., skips the boundary module
/// or inserts already-present modules a second time).
#[test]
fn sequential_embedded_fallback_no_duplicates_in_topo_order() {
    use std::collections::HashSet;

    let stdlib = stdlib_loader::load_stdlib();
    assert!(
        stdlib.len() >= 4,
        "this test needs at least 4 stdlib modules so the second compile_module call \
         actually inserts a non-empty suffix (stdlib[3..=len-1]); got {}",
        stdlib.len()
    );
    let expected_paths: Vec<String> = stdlib.iter().map(|m| m.path.0.join(".")).collect();

    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Missing stdlib_root so ALL std.* falls back to embedded.
    let stdlib_root = dir.join("nonexistent_stdlib");
    assert!(
        !stdlib_root.exists(),
        "precondition: stdlib_root must not exist for this test"
    );

    let resolver = ModuleResolver::new(&dir, &stdlib_root);
    let mut dag = ModuleDag::new();

    // Two disjoint-prefix calls: first covers stdlib[0..=2], second covers the rest.
    let first_target = &expected_paths[2];
    let second_target = &expected_paths[stdlib.len() - 1];

    let result_first = dag.compile_module(first_target, &resolver);
    assert!(
        result_first.is_ok(),
        "first compile_module('{}') should succeed, got: {:?}",
        first_target,
        result_first.err()
    );

    let result_second = dag.compile_module(second_target, &resolver);
    assert!(
        result_second.is_ok(),
        "second compile_module('{}') should succeed, got: {:?}",
        second_target,
        result_second.err()
    );

    // (a) topo_order must match the full stdlib prefix in order (no duplicates, no gaps).
    assert_eq!(
        dag.topo_order, expected_paths,
        "topo_order must match stdlib prefix in order (no duplicates, no gaps); \
         got: {:?}, expected: {:?}",
        dag.topo_order, expected_paths
    );

    // (b) Defense-in-depth: no duplicates in topo_order.
    let unique: HashSet<&String> = dag.topo_order.iter().collect();
    assert_eq!(
        unique.len(),
        dag.topo_order.len(),
        "topo_order must not contain duplicates, got: {:?}",
        dag.topo_order
    );

    // (c) modules map must have the same number of entries as the stdlib prefix.
    assert_eq!(
        dag.modules.len(),
        expected_paths.len(),
        "dag.modules must have {} entries, got {}",
        expected_paths.len(),
        dag.modules.len()
    );
}

// ── task-3268: compile_project_with_entry_source overload ────────────────────

/// Dirty-entry + clean import.
///
/// `dep.ri` is on disk; `entry.ri` is deliberately NOT written to disk.
/// Calling `compile_project_with_entry_source` with an in-memory source string
/// must succeed and resolve the on-disk sibling `Helper` structure.
///
/// - The absence of `entry.ri` on disk proves the entry source came from memory.
/// - The successful resolution of `Helper` proves the on-disk resolver still
///   works for siblings.
#[test]
fn compile_project_with_entry_source_dirty_entry_with_disk_import() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Only the sibling dep.ri is on disk; entry.ri is never created.
    fs::write(
        dir.join("dep.ri"),
        "pub structure Helper { param d: Scalar = 1mm }",
    )
    .unwrap();

    let entry_source = "import dep.Helper\nstructure Top {\n    sub h = Helper(d: 5mm)\n}";
    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project_with_entry_source(
        &dir.join("entry.ri"),
        entry_source,
        &resolver,
    );

    assert!(result.is_ok(), "expected Ok, got {:?}", result.unwrap_err());
    let modules = result.unwrap();
    assert_eq!(
        modules.len(),
        2,
        "expected 2 modules (dep + entry), got {}",
        modules.len()
    );

    // Entry module (last in topo order) must have 1 template named "Top"
    // with a sub_component referencing "Helper".
    let entry_module = modules.last().unwrap();
    assert_eq!(entry_module.templates.len(), 1);
    let template = &entry_module.templates[0];
    assert_eq!(template.name, "Top");
    assert_eq!(template.sub_components.len(), 1);
    assert_eq!(template.sub_components[0].structure_name, "Helper");
}

/// Entry-source diff is observed: in-memory source is used, not the disk file.
///
/// `entry.ri` on disk defines a template named `Top`; the in-memory string
/// defines a different (valid) template named `TopMem`. The result must contain
/// `TopMem`, not `Top` — proving the in-memory source was compiled rather than
/// the disk file.  Using a valid alternative avoids coupling the test to parser
/// error-handling behaviour.
#[test]
fn compile_project_with_entry_source_uses_in_memory_source_not_disk() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // dep.ri: sibling on disk, imported by both the disk and in-memory variants.
    fs::write(
        dir.join("dep.ri"),
        "pub structure Helper { param d: Scalar = 1mm }",
    )
    .unwrap();

    // entry.ri on disk: defines Top.
    fs::write(
        dir.join("entry.ri"),
        "import dep.Helper\nstructure Top { sub h = Helper(d: 5mm) }",
    )
    .unwrap();

    // In-memory variant: valid source that defines TopMem instead of Top.
    let in_memory = "import dep.Helper\nstructure TopMem { sub h = Helper(d: 10mm) }";

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project_with_entry_source(
        &dir.join("entry.ri"),
        in_memory,
        &resolver,
    );

    assert!(
        result.is_ok(),
        "expected Ok, got Err: {:?}",
        result.unwrap_err()
    );
    let modules = result.unwrap();
    let entry_module = modules.last().unwrap();

    // The in-memory source defines TopMem; the disk file defines Top.
    // If the compiler used the disk file we'd see Top here, not TopMem.
    let template_names: Vec<&str> = entry_module
        .templates
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        template_names.contains(&"TopMem"),
        "expected template 'TopMem' from in-memory source, got {:?}",
        template_names
    );
    assert!(
        !template_names.contains(&"Top"),
        "unexpectedly found 'Top' (from disk) instead of in-memory 'TopMem'"
    );
}

/// Parity with `compile_project` when the in-memory source matches the disk file.
///
/// Both calls must return `Ok`, produce the same number of modules, the same
/// per-module path strings (in topo order), and the same template name lists.
/// This locks in the refactor's equivalence guarantee: when
/// `entry_source == read_to_string(entry_path)`, the two entry points are
/// observationally identical.
#[test]
fn compile_project_with_entry_source_parity_with_compile_project_when_source_matches_disk() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    let dep_source = "pub structure Helper { param d: Scalar = 1mm }";
    let entry_source = "import dep.Helper\nstructure Top {\n    sub h = Helper(d: 5mm)\n}";

    fs::write(dir.join("dep.ri"), dep_source).unwrap();
    fs::write(dir.join("entry.ri"), entry_source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));

    let disk_result = reify_compiler::module_dag::compile_project(&dir.join("entry.ri"), &resolver);
    let mem_result = reify_compiler::module_dag::compile_project_with_entry_source(
        &dir.join("entry.ri"),
        entry_source,
        &resolver,
    );

    assert!(
        disk_result.is_ok(),
        "compile_project failed: {:?}",
        disk_result.unwrap_err()
    );
    assert!(
        mem_result.is_ok(),
        "compile_project_with_entry_source failed: {:?}",
        mem_result.unwrap_err()
    );

    let disk_modules = disk_result.unwrap();
    let mem_modules = mem_result.unwrap();

    // Same module count.
    assert_eq!(
        disk_modules.len(),
        mem_modules.len(),
        "module count differs: disk={} mem={}",
        disk_modules.len(),
        mem_modules.len()
    );

    // Same per-module path strings in the same topo order.
    let disk_paths: Vec<String> = disk_modules.iter().map(|m| format!("{}", m.path)).collect();
    let mem_paths: Vec<String> = mem_modules.iter().map(|m| format!("{}", m.path)).collect();
    assert_eq!(
        disk_paths, mem_paths,
        "module paths differ: disk={:?} mem={:?}",
        disk_paths, mem_paths
    );

    // Same per-module template name lists, sub_component counts, and
    // sub_component structure names — catching any deeper divergence in the
    // compiled topology even when module count and paths agree.
    for (i, (dm, mm)) in disk_modules.iter().zip(mem_modules.iter()).enumerate() {
        let disk_tnames: Vec<&str> = dm.templates.iter().map(|t| t.name.as_str()).collect();
        let mem_tnames: Vec<&str> = mm.templates.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            disk_tnames, mem_tnames,
            "template names differ at module index {}: disk={:?} mem={:?}",
            i, disk_tnames, mem_tnames
        );

        for (j, (dt, mt)) in dm.templates.iter().zip(mm.templates.iter()).enumerate() {
            assert_eq!(
                dt.sub_components.len(),
                mt.sub_components.len(),
                "sub_component count differs at module {}, template {} ('{}'): disk={} mem={}",
                i,
                j,
                dt.name,
                dt.sub_components.len(),
                mt.sub_components.len()
            );

            let disk_sub_names: Vec<&str> = dt
                .sub_components
                .iter()
                .map(|s| s.structure_name.as_str())
                .collect();
            let mem_sub_names: Vec<&str> = mt
                .sub_components
                .iter()
                .map(|s| s.structure_name.as_str())
                .collect();
            assert_eq!(
                disk_sub_names, mem_sub_names,
                "sub_component structure names differ at module {}, template {} ('{}'): disk={:?} mem={:?}",
                i, j, dt.name, disk_sub_names, mem_sub_names
            );
        }
    }
}

/// Closes the coverage gap at `module_dag.rs:619-625`.
///
/// When the in-memory `entry_source` string contains a parse error,
/// `compile_project_with_entry_source` must return `Err(Vec<Diagnostic>)` where
/// every diagnostic has `Severity::Error`.  This test pins that contract,
/// catching regressions where the parser errors are silently dropped or where
/// diagnostics are surfaced with the wrong severity.
///
/// `entry.ri` is intentionally NOT written to disk, which proves the error came
/// from the in-memory string rather than from any disk file (the function's
/// contract at `module_dag.rs:600-602` states `entry_path` need not exist on
/// disk).
#[test]
fn compile_project_with_entry_source_returns_parse_errors_for_invalid_entry_source() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // entry.ri is deliberately NOT written to disk — the parse error must come
    // from the in-memory source string, not from any disk file.
    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = reify_compiler::module_dag::compile_project_with_entry_source(
        &dir.join("entry.ri"),
        "structure {",
        &resolver,
    );

    assert!(
        result.is_err(),
        "expected Err for malformed entry source, got Ok({:?})",
        result.ok()
    );

    let diagnostics = result.unwrap_err();
    assert!(
        !diagnostics.is_empty(),
        "expected non-empty diagnostics for malformed entry source"
    );
    assert!(
        diagnostics.iter().all(|d| d.severity == Severity::Error),
        "expected every diagnostic to have Severity::Error, got: {:?}",
        diagnostics
    );
}

// ── Task γ (step-3): module-path declaration enforcement at DAG site ──

/// Helper: build a simple two-file project (a.ri imports dep.ri), compile
/// entry `a`, and return the diagnostics from the `dep` module.
fn dag_dep_diagnostics(dep_source: &str) -> Vec<reify_core::Diagnostic> {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    fs::write(
        dir.join("a.ri"),
        "import dep\nstructure A { param x: Scalar = 1mm }",
    )
    .unwrap();
    fs::write(dir.join("dep.ri"), dep_source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    dag.compile_module("a", &resolver)
        .expect("compile_module should succeed for diagnostic check");
    dag.modules.remove("dep").expect("dep should be compiled").diagnostics
}

#[test]
fn dag_matching_module_decl_no_path_diagnostic() {
    // dep.ri declares `module dep` (matches resolver path "dep") → no E/W diagnostic
    let diags = dag_dep_diagnostics("module dep\nstructure Dep { param x: Scalar = 1mm }");
    let path_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.message.contains("E_MODULE_PATH_MISMATCH")
                || d.message.contains("W_MODULE_DECL_MISSING")
        })
        .collect();
    assert!(
        path_diags.is_empty(),
        "matching declaration should produce no path diagnostic, got: {:?}",
        path_diags
    );
}

#[test]
fn dag_mismatched_module_decl_emits_error() {
    // dep.ri declares `module not.dep` (mismatch) → E_MODULE_PATH_MISMATCH Error
    let diags = dag_dep_diagnostics("module not.dep\nstructure Dep { param x: Scalar = 1mm }");
    let mismatch: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("E_MODULE_PATH_MISMATCH"))
        .collect();
    assert!(
        !mismatch.is_empty(),
        "expected E_MODULE_PATH_MISMATCH diagnostic, got: {:?}",
        diags
    );
    assert_eq!(
        mismatch[0].severity,
        Severity::Error,
        "E_MODULE_PATH_MISMATCH should be Error severity"
    );
}

#[test]
fn dag_absent_module_decl_emits_warning() {
    // dep.ri has no module declaration → W_MODULE_DECL_MISSING Warning
    let diags = dag_dep_diagnostics("structure Dep { param x: Scalar = 1mm }");
    let missing: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("W_MODULE_DECL_MISSING"))
        .collect();
    assert!(
        !missing.is_empty(),
        "expected W_MODULE_DECL_MISSING diagnostic, got: {:?}",
        diags
    );
    assert_eq!(
        missing[0].severity,
        Severity::Warning,
        "W_MODULE_DECL_MISSING should be Warning severity"
    );
}
