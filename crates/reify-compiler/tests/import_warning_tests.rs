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
//! Amendment test (coverage gap from reviewer):
//! 4. `module_dag_resolved_destructured_import_emits_no_warning` — pins the
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
/// The filter predicate below implicitly verifies that the diagnostic message
/// contains both the word "import" and the path name "shapes". The four
/// explicit assertions check:
/// (a) Misleading phrase absent — message must NOT contain "not yet implemented".
/// (b) Actionable API referenced — the message or a label contains "compile_project"
///     or "ModuleDag".
/// (c) Severity remains Warning, not Error.
/// (d) A squiggle label is present, anchoring the underline to the import span.
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

    // (a) misleading phrase absent
    assert!(
        !diag.message.contains("not yet implemented"),
        "message must NOT contain 'not yet implemented': {:?}",
        diag.message
    );
    // (b) actionable API referenced — may appear in the message or a label
    let has_api_ref = diag.message.contains("compile_project")
        || diag.message.contains("ModuleDag")
        || diag.labels.iter().any(|l| {
            l.message.contains("compile_project") || l.message.contains("ModuleDag")
        });
    assert!(
        has_api_ref,
        "message or labels must reference 'compile_project' or 'ModuleDag': {:#?}",
        diag
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

    // b.ri: destructured import of Foo from module a; Bar uses Foo as a
    // sub-component so a regression in name binding also fails the test.
    fs::write(
        dir.join("b.ri"),
        "import a.{Foo}\nstructure Bar {\n    param y: Scalar = 2mm\n    sub f = Foo(x: 3mm)\n}",
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
