//! E_MODULE_PATH_MISMATCH names the declaration that fixes it, and that
//! declaration is accepted (task #7930).
//!
//! An imported module's expected path is the dotted import path, not its file
//! basename, so these fixtures nest the dependency at `sub/dep.ri`.

use std::fs;

use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
use reify_core::Severity;

/// Compile entry `a` (which imports `sub.dep`) in a tempdir and return the
/// diagnostics of the `sub.dep` module, whose source is `dep_source`.
fn nested_dep_diagnostics(dep_source: &str) -> Vec<reify_core::Diagnostic> {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    fs::write(
        dir.join("a.ri"),
        "module a\nimport sub.dep\nstructure A { param x: Length = 1mm }",
    )
    .unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("dep.ri"), dep_source).unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    dag.compile_module("a", &resolver)
        .expect("compile_module should succeed for diagnostic check");
    dag.modules
        .remove("sub.dep")
        .expect("sub.dep should be compiled")
        .diagnostics
}

#[test]
fn imported_module_mismatch_names_its_dotted_location_path() {
    let diags = nested_dep_diagnostics("module dep\npub structure Dep { param x: Length = 1mm }");
    let mismatches: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("E_MODULE_PATH_MISMATCH"))
        .collect();
    assert_eq!(
        mismatches.len(),
        1,
        "expected exactly one E_MODULE_PATH_MISMATCH, got: {diags:?}"
    );
    assert_eq!(mismatches[0].severity, Severity::Error);
    assert!(
        mismatches[0].message.contains("module sub.dep"),
        "the mismatch should name the corrective declaration `module sub.dep` \
         (the dotted location path, not the basename), got: {}",
        mismatches[0].message
    );
}

#[test]
fn imported_module_with_the_named_declaration_has_no_path_diagnostic() {
    let diags =
        nested_dep_diagnostics("module sub.dep\npub structure Dep { param x: Length = 1mm }");
    let path_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.message.contains("E_MODULE_PATH_MISMATCH")
                || d.message.contains("W_MODULE_DECL_MISSING")
        })
        .collect();
    assert!(
        path_diags.is_empty(),
        "`module sub.dep` at sub/dep.ri should raise no module-path diagnostic, got: {path_diags:?}"
    );
}
