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

use std::fs;

use reify_compiler::module_dag::{ModuleResolver, compile_project};

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
        .filter(|d| {
            d.severity == reify_types::Severity::Warning && d.message.contains("import \"a\"")
        })
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
        .filter(|d| {
            d.severity == reify_types::Severity::Warning
                && d.message.contains("import \"std.units\"")
        })
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

