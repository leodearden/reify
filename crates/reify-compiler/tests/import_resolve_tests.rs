//! Tests for import path → filesystem path resolution.

use std::fs;

use reify_compiler::module_dag::ModuleResolver;
use reify_test_support::TempDir;

/// Create a unique temp directory for tests, removed when the returned guard
/// drops — including while unwinding out of a failed assertion.
///
/// Bind the guard to a NAMED LOCAL that outlives the test body:
///
/// ```ignore
/// let guard = test_dir("resolve_std");
/// let dir = guard.path().to_path_buf();
/// ```
///
/// `test_dir("x").path().to_path_buf()` compiles but drops the guard at the end
/// of that statement, deleting the directory before the test uses it.
fn test_dir(name: &str) -> TempDir {
    reify_test_support::prefixed_tempdir(&format!("reify_test-{name}-"))
}

// ── Step 13: Basic path resolution ────────────────────────────────

#[test]
fn resolve_std_import_to_stdlib_file() {
    let guard = test_dir("resolve_std");
    let dir = guard.path().to_path_buf();
    let stdlib = dir.join("stdlib");
    fs::create_dir_all(&stdlib).unwrap();
    fs::write(stdlib.join("math.ri"), "// std math module").unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = resolver.resolve_import_path("std.math");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), stdlib.join("math.ri"));
}

#[test]
fn resolve_local_import_to_project_file() {
    let guard = test_dir("resolve_local");
    let dir = guard.path().to_path_buf();
    fs::write(dir.join("shapes.ri"), "// shapes module").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = resolver.resolve_import_path("shapes");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), dir.join("shapes.ri"));
}

#[test]
fn resolve_nested_local_import() {
    let guard = test_dir("resolve_nested");
    let dir = guard.path().to_path_buf();
    let mylib = dir.join("mylib");
    fs::create_dir_all(&mylib).unwrap();
    fs::write(mylib.join("shapes.ri"), "// mylib.shapes").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = resolver.resolve_import_path("mylib.shapes");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), mylib.join("shapes.ri"));
}

// ── Step 15: Missing module ───────────────────────────────────────

#[test]
fn resolve_missing_module_returns_error() {
    let guard = test_dir("resolve_missing");
    let dir = guard.path().to_path_buf();

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
    let guard = test_dir("resolve_dir_mod");
    let dir = guard.path().to_path_buf();
    let stdlib = dir.join("stdlib");
    let fasteners = stdlib.join("mechanical").join("fasteners");
    fs::create_dir_all(&fasteners).unwrap();
    fs::write(fasteners.join("mod.ri"), "// fasteners module").unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = resolver.resolve_import_path("std.mechanical.fasteners");
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap(), fasteners.join("mod.ri"));
}

#[test]
fn resolve_prefers_file_over_directory() {
    let guard = test_dir("resolve_prefer_file");
    let dir = guard.path().to_path_buf();
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
}
