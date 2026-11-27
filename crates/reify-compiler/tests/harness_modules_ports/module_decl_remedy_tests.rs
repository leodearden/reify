//! E_MODULE_PATH_MISMATCH names the declaration that fixes it, and that
//! declaration is accepted (task #7930).
//!
//! An imported module's expected path is the dotted import path, not its file
//! basename, so the DAG-level fixtures nest the dependency at `sub/dep.ri`.
//!
//! An imported file's header diagnostic reaches the program entry coded and
//! naming that file (task #6953, PRD resolution-unification ξ).

use std::fs;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_ir::Satisfaction;
use reify_test_support::mocks::MockConstraintChecker;

/// Compile entry `a` (which imports `sub.dep`) in a tempdir and return the
/// diagnostics of the `sub.dep` module, whose source is `dep_source`.
fn nested_dep_diagnostics(dep_source: &str) -> Vec<Diagnostic> {
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
        .filter(|d| d.code == Some(DiagnosticCode::ModulePathMismatch))
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
    let path_diags: Vec<_> = diags.iter().filter(|d| is_header_diagnostic(d)).collect();
    assert!(
        path_diags.is_empty(),
        "`module sub.dep` at sub/dep.ri should raise no module-path diagnostic, got: {path_diags:?}"
    );
}

/// True iff `d` is a module-header diagnostic, a wrong or a missing `module` line.
fn is_header_diagnostic(d: &Diagnostic) -> bool {
    matches!(
        d.code,
        Some(DiagnosticCode::ModulePathMismatch | DiagnosticCode::ModuleDeclMissing)
    )
}

const ENTRY_IMPORTING_PARTS: &str =
    "module main\nimport parts\nstructure def Rig { param x : Length = 1mm }\n";
const PARTS_BODY: &str = "pub structure def Pulley { param d : Length = 1mm }\n";

/// Compile `main.ri` (`main_source`) as a program in a tempdir whose `parts.ri`
/// holds `parts_source`, and return the program and parts.ri's displayed path.
fn compile_entry_importing_parts(
    main_source: &str,
    parts_source: &str,
) -> (CompiledProgram, String) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let parts_path = dir.join("parts.ri");
    fs::write(&parts_path, parts_source).unwrap();
    let entry_path = dir.join("main.ri");
    fs::write(&entry_path, main_source).unwrap();
    let program = compile_program(
        &entry_path,
        main_source,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );
    (program, parts_path.display().to_string())
}

/// The one diagnostic re-homed from `parts` onto the entry that carries `code`.
fn the_rehomed_parts_diagnostic(program: &CompiledProgram, code: DiagnosticCode) -> &Diagnostic {
    let coded: Vec<&Diagnostic> = program
        .rehomed_from("parts")
        .filter(|d| d.code == Some(code))
        .collect();
    assert_eq!(
        coded.len(),
        1,
        "expected exactly one {code:?} re-homed from parts, got: {:?}",
        program.entry().diagnostics
    );
    coded[0]
}

#[test]
fn an_imported_files_wrong_header_surfaces_on_the_entry_naming_its_file() {
    let (program, parts_path) = compile_entry_importing_parts(
        ENTRY_IMPORTING_PARTS,
        &format!("module wrongname\n{PARTS_BODY}"),
    );

    let mismatch = the_rehomed_parts_diagnostic(&program, DiagnosticCode::ModulePathMismatch);
    assert_eq!(mismatch.severity, Severity::Error);
    assert!(
        mismatch.message.contains(&parts_path),
        "the mismatch should name the imported file {parts_path}, got: {}",
        mismatch.message
    );
    assert!(
        mismatch.message.contains("module parts"),
        "the mismatch should name the corrective `module parts`, got: {}",
        mismatch.message
    );
}

#[test]
fn an_imported_files_missing_header_surfaces_on_the_entry_naming_its_file() {
    let (program, parts_path) = compile_entry_importing_parts(ENTRY_IMPORTING_PARTS, PARTS_BODY);

    let missing = the_rehomed_parts_diagnostic(&program, DiagnosticCode::ModuleDeclMissing);
    assert_eq!(missing.severity, Severity::Warning);
    assert!(
        missing.message.contains(&parts_path),
        "the missing-header warning should name the imported file {parts_path}, got: {}",
        missing.message
    );
    assert!(
        missing.message.contains("module parts"),
        "the missing-header warning should name the expected `module parts`, got: {}",
        missing.message
    );
}

#[test]
fn the_entrys_own_header_diagnostic_carries_the_same_code() {
    let (program, _parts_path) = compile_entry_importing_parts(
        "module elsewhere\nimport parts\nstructure def Rig { param x : Length = 1mm }\n",
        &format!("module parts\n{PARTS_BODY}"),
    );

    let own_mismatches = program
        .entry_own_diagnostics()
        .filter(|d| d.code == Some(DiagnosticCode::ModulePathMismatch))
        .count();
    assert_eq!(
        own_mismatches,
        1,
        "the entry's wrong header should carry ModulePathMismatch, got: {:?}",
        program.entry().diagnostics
    );
    let from_parts: Vec<&Diagnostic> = program
        .rehomed_from("parts")
        .filter(|d| is_header_diagnostic(d))
        .collect();
    assert!(
        from_parts.is_empty(),
        "a correctly headed parts.ri raises no header diagnostic, got: {from_parts:?}"
    );
}
