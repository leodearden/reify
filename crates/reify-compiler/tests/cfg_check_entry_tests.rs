//! Tests for `compile_entry_with_stdlib_cfg` — the `reify check` compiler entry
//! point that seeds the full stdlib prelude AND walks a `#cfg(...)`-gated
//! user-import DAG (Task δ / Slice D).
//!
//! This is the bridge that neither existing entry point provides alone:
//! - `compile_with_stdlib` seeds the stdlib prelude but has no user-import DAG.
//! - `compile_project_with_entry_source_cfg` gates imports by cfg but does NOT
//!   seed the stdlib prelude.
//!
//! The user-observable signal: an entity referencing a pub name defined ONLY in
//! `platform_linux` resolves under `target = "linux"` (import followed) and
//! fails to resolve under `target = "wasm"` (import gated out). A stdlib symbol
//! referenced by the same entry resolves under BOTH targets, proving the stdlib
//! prelude is always seeded regardless of the active target.

use std::fs;
use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{compile_entry_with_stdlib_cfg, ModuleResolver};
use reify_core::Severity;

/// A `CfgSet` with only `target` set (no flags / kv).
fn target_cfg(target: &str) -> CfgSet {
    CfgSet {
        target: Some(target.to_string()),
        ..Default::default()
    }
}

/// True iff `module` has an `Error`-severity diagnostic whose message contains `needle`.
fn has_error_containing(module: &reify_compiler::CompiledModule, needle: &str) -> bool {
    module
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains(needle))
}

/// Dump every diagnostic as `severity: message` for assert-failure context.
fn dump_diags(module: &reify_compiler::CompiledModule) -> Vec<String> {
    module
        .diagnostics
        .iter()
        .map(|d| format!("{}: {}", d.severity, d.message))
        .collect()
}

/// Write the entry + two cfg-gated platform siblings into `dir`.
///
/// - `main.ri`: gated linux/wasm imports; the entry references `LinuxOnly` in
///   **type position** (`param marker : LinuxOnly`) — a pub structure defined
///   ONLY in `platform_linux`, so the name resolves at compile time iff that
///   import is followed. It also instantiates `DisplayStyle` (a stdlib-only
///   structure, present iff the stdlib prelude is seeded).
/// - `platform_linux.ri` / `platform_wasm.ri`: each exports one pub structure.
///
/// A type-position reference is used (not `sub w = LinuxOnly(...)`) because an
/// unknown structure name in a `sub` occurrence is resolved leniently at
/// compile time (deferred to eval) and emits no diagnostic, whereas an unknown
/// **type** is a hard `unresolved type` compile error — the signal this test
/// needs.
fn write_entry_fixtures(dir: &Path) {
    let entry_src = "#cfg(target = \"linux\")\nimport platform_linux\n\
                     #cfg(target = \"wasm\")\nimport platform_wasm\n\
                     \n\
                     structure def Entry {\n\
                     \x20   sub style = DisplayStyle(opacity: 0.5, wireframe: true)\n\
                     \x20   param marker : LinuxOnly\n\
                     }\n";
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    fs::write(
        dir.join("platform_linux.ri"),
        "pub structure def LinuxOnly { param x: Real }\n",
    )
    .unwrap();
    fs::write(
        dir.join("platform_wasm.ri"),
        "pub structure def WasmOnly { param x: Real }\n",
    )
    .unwrap();
}

/// Parse `main.ri` from `dir` with the stdlib enum pre-seed, asserting it is
/// free of parse errors, and return the `ParsedModule`.
fn parse_entry(dir: &Path) -> reify_ast::ParsedModule {
    let entry_src = fs::read_to_string(dir.join("main.ri")).unwrap();
    let parsed = reify_compiler::parse_with_stdlib(
        &entry_src,
        reify_core::ModulePath::single("main"),
    );
    assert!(
        parsed.errors.is_empty(),
        "fixture entry should parse cleanly, got parse errors: {:?}",
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    parsed
}

/// Under `target = "linux"`, the gated linux import is followed: `LinuxOnly`
/// resolves (no error), and the stdlib symbol `DisplayStyle` resolves too
/// (stdlib prelude seeded).
#[test]
fn entry_target_linux_resolves_linux_import_and_stdlib() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_entry_fixtures(dir);

    let parsed = parse_entry(dir);
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let compiled = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("linux"));

    assert!(
        !has_error_containing(&compiled, "LinuxOnly"),
        "under target=linux the platform_linux import is followed, so 'LinuxOnly' \
         must resolve with no Error; diagnostics: {:?}",
        dump_diags(&compiled)
    );
    assert!(
        !has_error_containing(&compiled, "DisplayStyle"),
        "the stdlib prelude must be seeded, so 'DisplayStyle' must resolve with no \
         Error; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// Under `target = "wasm"`, the linux import is gated out: `LinuxOnly` no longer
/// resolves (an Error references it). The stdlib symbol `DisplayStyle` still
/// resolves — stdlib seeding is independent of the active target.
#[test]
fn entry_target_wasm_gates_out_linux_import_but_keeps_stdlib() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_entry_fixtures(dir);

    let parsed = parse_entry(dir);
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let compiled = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("wasm"));

    assert!(
        has_error_containing(&compiled, "LinuxOnly"),
        "under target=wasm the platform_linux import is gated out, so 'LinuxOnly' \
         must be unresolved (Error referencing it); diagnostics: {:?}",
        dump_diags(&compiled)
    );
    assert!(
        !has_error_containing(&compiled, "DisplayStyle"),
        "the stdlib prelude is seeded regardless of target, so 'DisplayStyle' must \
         still resolve under target=wasm; diagnostics: {:?}",
        dump_diags(&compiled)
    );
}

/// True iff `module.templates` contains a template named `name`.
fn entry_has_template(module: &reify_compiler::CompiledModule, name: &str) -> bool {
    module.templates.iter().any(|t| t.name == name)
}

/// Pub templates from a cfg-satisfied import are merged into the entry's
/// `compiled.templates` so cross-module entities resolve for downstream eval.
///
/// `platform_linux` exports `pub structure def LinuxOnly`. Under `target="linux"`
/// the import is followed and `LinuxOnly` is merged into the entry's templates;
/// under `target="wasm"` the import is gated out and `LinuxOnly` is absent.
#[test]
fn entry_merges_pub_templates_from_cfg_satisfied_import() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_entry_fixtures(dir);

    let parsed = parse_entry(dir);
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));

    let compiled_linux = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("linux"));
    assert!(
        entry_has_template(&compiled_linux, "LinuxOnly"),
        "under target=linux the pub structure 'LinuxOnly' from the followed \
         platform_linux import must be merged into entry.templates; templates: {:?}",
        compiled_linux.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let compiled_wasm = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("wasm"));
    assert!(
        !entry_has_template(&compiled_wasm, "LinuxOnly"),
        "under target=wasm the platform_linux import is gated out, so 'LinuxOnly' \
         must NOT be in entry.templates; templates: {:?}",
        compiled_wasm.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}
