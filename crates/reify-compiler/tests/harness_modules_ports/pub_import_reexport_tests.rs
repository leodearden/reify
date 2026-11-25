//! D-7 `pub import` re-export (task #6953, PRD resolution-unification μ): a
//! unit sees, through each import, that module's `pub` surface followed by
//! every module it re-exports along followed `pub import` edges. A plain,
//! non-pub or `#cfg`-gated-out edge re-exports nothing.
//!
//! Visibility is asserted at check time (no `UnknownSubStructure`) and through
//! `program_scope::resolve_in_program`, the environment eval binds through.

use std::fs;
use std::path::Path;

use reify_compiler::TopologyTemplate;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleResolver, compile_project};
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::program_scope::resolve_in_program;
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_ir::Satisfaction;
use reify_test_support::mocks::MockConstraintChecker;

const PARTS: &str = "module parts\npub structure def Pulley { param diameter : Length = 40mm }\nstructure def InternalJig { }\n";
const MAIN_USING_PULLEY: &str = "module main\nimport lib\nstructure def Rig { sub p = Pulley() }\n";
const FACADE_LIB: &str = "module lib\npub import parts\n";

/// Write each `(file name, source)` into `dir`.
fn write_files(dir: &Path, files: &[(&str, &str)]) {
    for (name, source) in files {
        fs::write(dir.join(name), source).unwrap();
    }
}

/// Write `files` and `main.ri` (`main_source`) into a tempdir and compile
/// `main.ri` as a program under `cfg`.
fn compile_main_with_cfg(
    main_source: &str,
    files: &[(&str, &str)],
    cfg: &CfgSet,
) -> CompiledProgram {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_files(dir, files);
    let entry_path = dir.join("main.ri");
    fs::write(&entry_path, main_source).unwrap();
    compile_program(
        &entry_path,
        main_source,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        cfg,
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    )
}

fn compile_main(main_source: &str, files: &[(&str, &str)]) -> CompiledProgram {
    compile_main_with_cfg(main_source, files, &CfgSet::default())
}

/// The program's import units' keys, in order.
fn import_keys(program: &CompiledProgram) -> Vec<&str> {
    program
        .imports
        .iter()
        .map(|unit| unit.import_path.as_str())
        .collect()
}

/// The keys of every user module compiled into the program, entry last.
fn source_keys(program: &CompiledProgram) -> Vec<&str> {
    program
        .sources
        .iter()
        .map(|source| source.module_key.as_str())
        .collect()
}

/// The `UnknownSubStructure` Error among `diagnostics` naming structure `name`.
fn unknown_sub<'d>(
    mut diagnostics: impl Iterator<Item = &'d Diagnostic>,
    name: &str,
) -> Option<&'d Diagnostic> {
    let quoted = format!("\"{name}\"");
    diagnostics.find(|d| {
        d.severity == Severity::Error
            && d.code == Some(DiagnosticCode::UnknownSubStructure)
            && d.message.contains(&quoted)
    })
}

/// Panic unless the entry's own diagnostics leave structure `name` known.
#[track_caller]
fn assert_entry_knows(program: &CompiledProgram, name: &str) {
    if let Some(diag) = unknown_sub(program.entry_own_diagnostics(), name) {
        panic!("`{name}` should be visible at the entry, got: {diag:?}");
    }
}

/// Panic unless the entry's own diagnostics report structure `name` unknown.
#[track_caller]
fn assert_entry_does_not_know(program: &CompiledProgram, name: &str) {
    assert!(
        unknown_sub(program.entry_own_diagnostics(), name).is_some(),
        "`{name}` should be unknown at the entry, got: {:?}",
        program.entry().diagnostics
    );
}

/// The template named `name` in the import unit keyed `module_key`.
fn template_in<'p>(
    program: &'p CompiledProgram,
    module_key: &str,
    name: &str,
) -> &'p TopologyTemplate {
    program
        .imports
        .iter()
        .find(|unit| unit.import_path == module_key)
        .and_then(|unit| unit.module.templates.iter().find(|t| t.name == name))
        .unwrap_or_else(|| panic!("no import unit `{module_key}` declaring `{name}`"))
}

#[test]
fn a_pub_import_re_exports_its_targets_pub_structures() {
    let program = compile_main(
        MAIN_USING_PULLEY,
        &[("lib.ri", FACADE_LIB), ("parts.ri", PARTS)],
    );

    assert_entry_knows(&program, "Pulley");
    assert_eq!(import_keys(&program), ["lib", "parts"]);
    let resolved = resolve_in_program::<TopologyTemplate, str>(&program, &[], "Pulley")
        .expect("eval's environment should bind the re-exported Pulley");
    assert!(
        std::ptr::eq(resolved, template_in(&program, "parts", "Pulley")),
        "Pulley should bind to parts' own definition"
    );
}

#[test]
fn a_plain_import_chain_does_not_leak() {
    let program = compile_main(
        MAIN_USING_PULLEY,
        &[
            ("lib.ri", "module lib\nimport parts\n"),
            ("parts.ri", PARTS),
        ],
    );

    assert_entry_does_not_know(&program, "Pulley");
    assert_eq!(import_keys(&program), ["lib"]);
    assert!(resolve_in_program::<TopologyTemplate, str>(&program, &[], "Pulley").is_none());
}

#[test]
fn re_export_is_transitive_through_pub_edges_only() {
    let lib = "module lib\npub import mid\n";

    let through_pub = compile_main(
        MAIN_USING_PULLEY,
        &[
            ("lib.ri", lib),
            ("mid.ri", "module mid\npub import parts\n"),
            ("parts.ri", PARTS),
        ],
    );
    assert_entry_knows(&through_pub, "Pulley");
    assert_eq!(import_keys(&through_pub), ["lib", "mid", "parts"]);

    let through_plain = compile_main(
        MAIN_USING_PULLEY,
        &[
            ("lib.ri", lib),
            ("mid.ri", "module mid\nimport parts\n"),
            ("parts.ri", PARTS),
        ],
    );
    assert_entry_does_not_know(&through_plain, "Pulley");
    assert_eq!(import_keys(&through_plain), ["lib", "mid"]);
}

#[test]
fn a_re_exported_modules_non_pub_structure_stays_hidden() {
    let program = compile_main(
        "module main\nimport lib\nstructure def Rig { sub j = InternalJig() }\n",
        &[("lib.ri", FACADE_LIB), ("parts.ri", PARTS)],
    );

    assert_entry_does_not_know(&program, "InternalJig");
}

#[test]
fn a_cfg_gated_out_pub_import_re_exports_nothing() {
    let linux = CfgSet {
        target: Some("linux".to_string()),
        ..Default::default()
    };
    let program = compile_main_with_cfg(
        MAIN_USING_PULLEY,
        &[
            (
                "lib.ri",
                "module lib\n#cfg(target = \"wasm\")\npub import parts\n",
            ),
            ("parts.ri", PARTS),
        ],
        &linux,
    );

    assert_entry_does_not_know(&program, "Pulley");
    assert!(!import_keys(&program).contains(&"parts"));
    assert!(!source_keys(&program).contains(&"parts"));
}

#[test]
fn an_import_unit_sees_what_its_facade_re_exports() {
    let program = compile_main(
        "module main\nimport app\nstructure def Rig { sub m = Machine() }\n",
        &[
            (
                "app.ri",
                "module app\nimport lib\npub structure def Machine { sub p = Pulley() }\n",
            ),
            ("lib.ri", FACADE_LIB),
            ("parts.ri", PARTS),
        ],
    );

    if let Some(diag) = unknown_sub(program.rehomed_from("app"), "Pulley") {
        panic!("app should see Pulley through its facade import, got: {diag:?}");
    }
    assert_entry_knows(&program, "Machine");
}

#[test]
fn a_module_both_imported_and_re_exported_appears_once() {
    let program = compile_main(
        "module main\nimport lib\nimport parts\nstructure def Rig { sub p = Pulley() }\n",
        &[("lib.ri", FACADE_LIB), ("parts.ri", PARTS)],
    );

    assert_entry_knows(&program, "Pulley");
    assert_eq!(import_keys(&program), ["lib", "parts"]);
}

#[test]
fn compile_project_entries_see_re_exports_too() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_files(
        dir,
        &[
            ("a.ri", "module a\npub structure def Helper { }\n"),
            ("b.ri", "module b\npub import a\n"),
            (
                "c.ri",
                "module c\nimport b\nstructure def User { sub h = Helper() }\n",
            ),
        ],
    );

    let modules = compile_project(
        &dir.join("c.ri"),
        &ModuleResolver::new(dir, dir.join("stdlib")),
    )
    .expect("the project should compile");
    let entry = modules.last().expect("the entry is the last module");
    assert_eq!(entry.path.to_dotted(), "c");
    if let Some(diag) = unknown_sub(entry.diagnostics.iter(), "Helper") {
        panic!("c should see Helper through b's re-export, got: {diag:?}");
    }
}
