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
/// A type-position reference is used (not `sub style = LinuxOnly(...)`) because
/// task 4528 added compile-time validation of sub `structure_name`s against the
/// (module ∪ prelude) template set — a sub targeting `LinuxOnly` would now emit
/// an `unknown structure` Error when the import is inactive (the compile-time
/// error would collide with the cfg-import-gating signal).  The type-position
/// `param marker : LinuxOnly` still gives the `unresolved type` compile error
/// this test keys on.  The existing `sub style = DisplayStyle(...)` is accepted
/// because `DisplayStyle` resolves via the always-seeded stdlib prelude.
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

/// Write a project where TWO cfg-satisfied imports each export the same pub
/// structure `Widget`, triggering the cross-import first-wins collision path.
///
/// Both imports are gated on `#cfg(target = "linux")`, so compiling under
/// `target = "linux"` follows both and the merge sees a name collision.
fn write_collision_fixtures(dir: &Path) {
    let entry_src = "#cfg(target = \"linux\")\nimport helper_a\n\
                     #cfg(target = \"linux\")\nimport helper_b\n\
                     \n\
                     structure def Entry {\n\
                     \x20   param x: Real\n\
                     }\n";
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    fs::write(
        dir.join("helper_a.ri"),
        "pub structure def Widget { param x: Real }\n",
    )
    .unwrap();
    fs::write(
        dir.join("helper_b.ri"),
        "pub structure def Widget { param y: Real }\n",
    )
    .unwrap();
}

/// Two cfg-satisfied imports that both declare `pub structure Widget` collide:
/// the merge keeps the first declarer (first-wins, `Widget` merged exactly once)
/// and pushes a `Warning` naming both module origins. Exercises the
/// cross-import collision branch of `merge_imported_pub_templates`.
#[test]
fn entry_collision_between_two_cfg_satisfied_imports_emits_first_wins_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_collision_fixtures(dir);

    let parsed = parse_entry(dir);
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let compiled = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("linux"));

    // First-wins: 'Widget' is merged exactly once despite two declarers.
    let widget_count = compiled.templates.iter().filter(|t| t.name == "Widget").count();
    assert_eq!(
        widget_count, 1,
        "first-wins: 'Widget' must be merged exactly once; templates: {:?}",
        compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // A kind-neutral first-wins Warning names both colliding module origins.
    let warning = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.severity == Severity::Warning
                && d.message.contains("imported pub template")
                && d.message.contains("Widget")
                && d.message.contains("first-wins")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a first-wins collision Warning for 'Widget'; diagnostics: {:?}",
                dump_diags(&compiled)
            )
        });
    for origin in ["helper_a", "helper_b"] {
        assert!(
            warning.message.contains(&format!("'{}'", origin)),
            "collision warning should name module '{}' in quoted form; got: {}",
            origin,
            warning.message
        );
    }
}

/// Write a project whose entry follows an import of a module that does NOT exist
/// on disk. The import is cfg-satisfied (`#cfg(target = "linux")`), so under
/// `target = "linux"` it is followed and its resolution failure must surface.
fn write_missing_import_fixtures(dir: &Path) {
    let entry_src = "#cfg(target = \"linux\")\nimport missing_module\n\
                     \n\
                     structure def Entry {\n\
                     \x20   param x: Real\n\
                     }\n";
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    // Intentionally do NOT create missing_module.ri.
}

/// A *satisfied* import pointing at a missing module surfaces its failure as an
/// `Error`-severity diagnostic on the returned entry `CompiledModule` rather than
/// short-circuiting (no early `Err`): the entry still compiles (its own `Entry`
/// template is present) and the import failure rides along as a diagnostic. This
/// is the diagnostics-embedded contract `cmd_check` relies on to map a broken
/// gated-DAG import to a non-zero exit.
#[test]
fn entry_followed_import_missing_module_surfaces_error_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_missing_import_fixtures(dir);

    let parsed = parse_entry(dir);
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let compiled = compile_entry_with_stdlib_cfg(&parsed, &resolver, &target_cfg("linux"));

    // The followed-but-missing import surfaces an Error diagnostic naming it.
    assert!(
        has_error_containing(&compiled, "missing_module")
            && has_error_containing(&compiled, "not found"),
        "a followed import of a missing module must surface an Error diagnostic \
         naming it; diagnostics: {:?}",
        dump_diags(&compiled)
    );

    // Not an early return: the entry itself still compiled, so its own template
    // is present alongside the import-failure diagnostic.
    assert!(
        entry_has_template(&compiled, "Entry"),
        "the entry must still compile despite the broken import (diagnostics \
         embedded, not an early Err); templates: {:?}",
        compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}
