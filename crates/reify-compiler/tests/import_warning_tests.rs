//! TDD tests pinning the import-warning behaviour introduced in task 2226.
//!
//! Tests are added in TDD order across steps 1 and 3:
//!
//! Step-1 tests (silent-on-resolved):
//! 1. `module_dag_resolved_user_import_emits_no_warning` — when ModuleDag
//!    recursively compiles a user import and seeds the result into the entry
//!    module's prelude, the import declaration in the entry module should NOT
//!    produce a warning diagnostic (the import was resolved).
//!
//! 2. `compile_with_stdlib_resolved_std_import_emits_no_warning` — when
//!    `compile_with_stdlib` is used and the source imports a stdlib module
//!    (e.g. `std.units`), no warning should fire because the stdlib prelude
//!    already contains `std.units`.
//!
//! Step-3 test (specific wording):
//! 3. `compile_with_stdlib_unresolved_user_import_emits_specific_warning` —
//!    when `compile_with_stdlib` is used and the source imports a user module
//!    that is not in the prelude, a Warning diagnostic is emitted with accurate
//!    wording (references compile_project / ModuleDag; does NOT say "not yet
//!    implemented").
//!
//! Amendment tests (coverage gaps from reviewer):
//! 4. `compile_project_missing_dep_returns_err` — documents that when a dep
//!    file is absent, compile_project returns Err (file-read error), not a
//!    phase_entities warning. The warning path is only exercised when compile()
//!    or compile_with_stdlib() is called directly without resolving the dep.
//! 5. `module_dag_resolved_destructured_import_emits_no_warning` — pins the
//!    path-form invariant: `import a.{Foo}` has path="a" in the AST, and
//!    ModuleDag's prelude key for `a.ri` is also "a", so the gate matches and
//!    no warning fires.

use std::fs;

use reify_compiler::module_dag::{ModuleResolver, compile_project};
use reify_types::Severity;

// ── Step-1: silent-on-resolved tests ─────────────────────────────────────────

/// ModuleDag-resolved user import: no warning diagnostic on the entry module.
///
/// Module `a.ri` defines `pub structure Foo`. Module `b.ri` imports `a` and
/// defines its own structure. Compiling via `compile_project` must recursively
/// compile `a`, seed it into `b`'s prelude, and suppress the import warning.
#[test]
fn module_dag_resolved_user_import_emits_no_warning() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // Module a: leaf, no imports
    fs::write(
        dir.join("a.ri"),
        "pub structure Foo {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();

    // Module b: imports a
    fs::write(
        dir.join("b.ri"),
        "import a\nstructure Bar {\n    param y: Scalar = 2mm\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let modules = compile_project(&dir.join("b.ri"), &resolver)
        .expect("compile_project should succeed for valid two-module project");

    // The entry module (b) is last in topological order.
    let b_module = modules.last().expect("expected at least one module in result");

    // No Warning diagnostic should mention import "a".
    let import_warnings: Vec<_> = b_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("import \"a\""))
        .collect();

    assert!(
        import_warnings.is_empty(),
        "expected no import-warning for resolved module 'a', but got: {:?}",
        import_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Step-3: specific wording for unresolved user imports ─────────────────────

/// Unresolved user import through `compile_with_stdlib`: the warning must use
/// accurate, actionable wording.
///
/// Contracts (must ALL hold):
/// (a) `diag.message.contains("shapes")` — path appears in the message.
/// (b) `diag.message.contains("import")` — word "import" appears (backward
///     compat with boundary2_producer.rs assertion shape).
/// (c) `!diag.message.contains("not yet implemented")` — misleading phrase
///     removed (mirrors geometry_sub_ref_e2e.rs:100 assertion style).
/// (d) Message references the actual API to switch to: contains
///     "compile_project" OR "ModuleDag".
/// (e) `diag.severity == Warning` — still a warning, not an error.
/// (f) `!diag.labels.is_empty()` — squiggle label is present.
///
/// Before step-4 this test fails on contracts (c) and (d) because the old
/// "noted; module resolution not yet implemented" wording is still in place.
#[test]
fn compile_with_stdlib_unresolved_user_import_emits_specific_warning() {
    let source = "import shapes\nstructure S {\n    param w: Scalar = 80mm\n}";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    // Find Warning diagnostics that mention the unresolved import path.
    let import_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("import \"shapes\""))
        .collect();

    assert_eq!(
        import_warnings.len(),
        1,
        "expected exactly 1 import-warning for unresolved module 'shapes', got: {:?}",
        import_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let diag = import_warnings[0];

    // (a) old misleading phrase removed (mirrors geometry_sub_ref_e2e.rs:100)
    assert!(
        !diag.message.contains("not yet implemented"),
        "message must NOT contain 'not yet implemented': {:?}",
        diag.message
    );
    // (b) references actionable API
    assert!(
        diag.message.contains("compile_project") || diag.message.contains("ModuleDag"),
        "message must reference 'compile_project' or 'ModuleDag': {:?}",
        diag.message
    );
    // (c) severity is Warning
    assert_eq!(
        diag.severity,
        Severity::Warning,
        "severity must be Warning: {:?}",
        diag
    );
    // (d) squiggle label present
    assert!(
        !diag.labels.is_empty(),
        "diagnostic must have at least one label: {:?}",
        diag
    );
}

/// stdlib-resolved import: no warning diagnostic for `import std.units`.
///
/// `compile_with_stdlib` seeds the full stdlib prelude (which includes
/// `std.units`) before compiling. The import declaration should NOT produce
/// a warning because the module is already in the prelude.
#[test]
fn compile_with_stdlib_resolved_std_import_emits_no_warning() {
    let source = "import std.units\nstructure S {\n    param x: Length = 5mm\n}";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    // No Warning diagnostic should mention import "std.units".
    let import_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("import \"std.units\""))
        .collect();

    assert!(
        import_warnings.is_empty(),
        "expected no import-warning for resolved stdlib module 'std.units', but got: {:?}",
        import_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Amendment: coverage for compile_project error path ───────────────────────

/// `compile_project` with a missing dependency file returns `Err`.
///
/// When `b.ri` imports `z` but `z.ri` does not exist on disk, `compile_project`
/// short-circuits with a file-read `Err` before ever reaching the entry module's
/// `phase_entities`. This means the "not resolved" Warning from `phase_entities`
/// is NOT emitted in this path — the caller receives file-level error diagnostics
/// instead.
///
/// This test documents that behaviour: the phase_entities warning path is only
/// exercisable via `compile()` / `compile_with_stdlib()` (when the caller has a
/// parsed module but no dep in the prelude). The `compile_with_stdlib` path is
/// covered by `compile_with_stdlib_unresolved_user_import_emits_specific_warning`.
#[test]
fn compile_project_missing_dep_returns_err() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // b.ri imports z, but z.ri does NOT exist.
    fs::write(
        dir.join("b.ri"),
        "import z\nstructure B {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let result = compile_project(&dir.join("b.ri"), &resolver);

    // compile_project must fail because z.ri cannot be read.
    assert!(
        result.is_err(),
        "expected Err when dep 'z' is missing, but got Ok with {} module(s)",
        result.unwrap().len()
    );

    // The Err contains file-read error diagnostics — NOT import warnings.
    // Verify the old misleading phrase doesn't appear in this path either.
    let errors = result.unwrap_err();
    for e in &errors {
        assert!(
            !e.message.contains("not yet implemented"),
            "file-read errors must not say 'not yet implemented': {:?}",
            e.message
        );
    }
}

// ── Amendment: path-form invariant for destructured imports ──────────────────

/// Destructured import `import a.{Foo}` is resolved silently by ModuleDag.
///
/// Pins the path-form invariant: the parser stores path = "a" (just the
/// module segment) and kind = Destructured(["Foo"]). ModuleDag compiles
/// `a.ri` and stores it under the key "a". The prelude lookup key
/// `m.path.0.join(".")` also resolves to "a", so the gate in `phase_entities`
/// matches and no warning fires.
///
/// This ensures the gate works for all import forms — not just Module-kind
/// (`import a`) but also Destructured-kind (`import a.{Foo}`).
#[test]
fn module_dag_resolved_destructured_import_emits_no_warning() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // a.ri: defines pub structure Foo
    fs::write(
        dir.join("a.ri"),
        "pub structure Foo {\n    param x: Scalar = 1mm\n}",
    )
    .unwrap();

    // b.ri: destructured import of Foo from module a
    fs::write(
        dir.join("b.ri"),
        "import a.{Foo}\nstructure Bar {\n    param y: Scalar = 2mm\n}",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let modules = compile_project(&dir.join("b.ri"), &resolver)
        .expect("compile_project should succeed with destructured import");

    // The entry module (b) is last in topological order.
    let b_module = modules.last().expect("expected at least one module in result");

    // No Warning diagnostic should mention import "a" (destructured path).
    let import_warnings: Vec<_> = b_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("import \"a\""))
        .collect();

    assert!(
        import_warnings.is_empty(),
        "expected no import-warning for resolved destructured import 'a.{{Foo}}', but got: {:?}",
        import_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
