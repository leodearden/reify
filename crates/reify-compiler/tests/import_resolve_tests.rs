//! Tests for import path → filesystem path resolution.

use std::fs;
use std::path::PathBuf;

use reify_compiler::module_dag::ModuleResolver;

/// Create a unique temp directory for tests.
fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("reify_test")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ── Step 13: Basic path resolution ────────────────────────────────

#[test]
fn resolve_std_import_to_stdlib_file() {
    let dir = test_dir("resolve_std");
    let stdlib = dir.join("stdlib");
    fs::create_dir_all(&stdlib).unwrap();
    fs::write(stdlib.join("math.ri"), "// std math module").unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = resolver.resolve_import_path("std.math");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), stdlib.join("math.ri"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_local_import_to_project_file() {
    let dir = test_dir("resolve_local");
    fs::write(dir.join("shapes.ri"), "// shapes module").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = resolver.resolve_import_path("shapes");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), dir.join("shapes.ri"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_nested_local_import() {
    let dir = test_dir("resolve_nested");
    let mylib = dir.join("mylib");
    fs::create_dir_all(&mylib).unwrap();
    fs::write(mylib.join("shapes.ri"), "// mylib.shapes").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = resolver.resolve_import_path("mylib.shapes");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), mylib.join("shapes.ri"));

    let _ = fs::remove_dir_all(&dir);
}

// ── Step 15: Missing module ───────────────────────────────────────

#[test]
fn resolve_missing_module_returns_error() {
    let dir = test_dir("resolve_missing");

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = resolver.resolve_import_path("nonexistent.module");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message.contains("not found"),
        "error should mention 'not found', got: {}",
        err.message
    );
}

// ── Step 17: Directory module (mod.ri) ────────────────────────────

#[test]
fn resolve_directory_module_via_mod_ri() {
    let dir = test_dir("resolve_dir_mod");
    let stdlib = dir.join("stdlib");
    let fasteners = stdlib.join("mechanical").join("fasteners");
    fs::create_dir_all(&fasteners).unwrap();
    fs::write(fasteners.join("mod.ri"), "// fasteners module").unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = resolver.resolve_import_path("std.mechanical.fasteners");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), fasteners.join("mod.ri"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_prefers_file_over_directory() {
    let dir = test_dir("resolve_prefer_file");
    let stdlib = dir.join("stdlib");
    fs::create_dir_all(&stdlib).unwrap();
    // Create both math.ri and math/mod.ri
    fs::write(stdlib.join("math.ri"), "// file module").unwrap();
    let math_dir = stdlib.join("math");
    fs::create_dir_all(&math_dir).unwrap();
    fs::write(math_dir.join("mod.ri"), "// dir module").unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = resolver.resolve_import_path("std.math");
    assert!(result.is_ok());
    // Should prefer file.ri over dir/mod.ri
    assert_eq!(result.unwrap(), stdlib.join("math.ri"));

    let _ = fs::remove_dir_all(&dir);
}
