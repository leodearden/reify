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
