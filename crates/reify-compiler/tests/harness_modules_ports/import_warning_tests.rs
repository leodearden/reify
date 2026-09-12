//! Pin the import-warning behaviour: silent when the import path is in the prelude
//! (resolved by ModuleDag or by `compile_with_stdlib`'s stdlib seed), and a
//! specific actionable warning otherwise.

use std::fs;

use reify_compiler::module_dag::{ModuleResolver, compile_project};
use reify_core::Severity;

/// Assert that compiling `b.ri` (with content `b_source`) alongside a canonical
/// `a.ri` via `compile_project` produces no Warning diagnostic whose message
/// contains `import "<module_name>"`.
///
/// `module_name` must match the stem of the module file written by this helper
/// (currently `a.ri`). Creates a temporary directory, writes the canonical
/// `a.ri` (`pub structure Foo`), writes `b.ri` with the caller-supplied source,
/// runs `compile_project`, and asserts the resulting entry module has no matching
/// import warning.
fn assert_no_import_warning_for(b_source: &str, module_name: &str) {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    fs::write(
        dir.join("a.ri"),
        "pub structure Foo {\n    param x: Length = 1mm\n}",
    )
    .unwrap();

    fs::write(dir.join("b.ri"), b_source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let modules =
        compile_project(&dir.join("b.ri"), &resolver).expect("compile_project should succeed");

    let b_module = modules
        .last()
        .expect("expected at least one module in result");

    let needle = format!("import \"{}\"", module_name);
    let import_warnings: Vec<_> = b_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains(&needle))
        .collect();

    assert!(
        import_warnings.is_empty(),
        "expected no import-warning for path '{}', but got: {:?}",
        module_name,
        import_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// ModuleDag-resolved user import: no warning diagnostic on the entry module.
///
/// Module `a.ri` defines `pub structure Foo`. Module `b.ri` imports `a` and
/// defines its own structure. Compiling via `compile_project` must recursively
/// compile `a`, seed it into `b`'s prelude, and suppress the import warning.
#[test]
fn module_dag_resolved_user_import_emits_no_warning() {
    assert_no_import_warning_for(
        "import a\nstructure Bar {\n    param y: Length = 2mm\n}",
        "a",
    );
}

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
    let source = "import shapes\nstructure S {\n    param w: Length = 80mm\n}";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
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
        || diag
            .labels
            .iter()
            .any(|l| l.message.contains("compile_project") || l.message.contains("ModuleDag"));
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
    // (d) exactly one (actionable) squiggle label
    assert_eq!(
        diag.labels.len(),
        1,
        "diagnostic must have exactly one (actionable) label: {:#?}",
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
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
    assert_no_import_warning_for(
        "import a.{Foo}\nstructure Bar {\n    param y: Length = 2mm\n    sub f = Foo(x: 3mm)\n}",
        "a",
    );
}
