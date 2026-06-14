//! Tests for cfg-gated import filtering in the module DAG.
//!
//! S5: DAG gating — `compile_project_with_entry_source_cfg` and `ModuleDag::with_cfg`
//! filter imports by satisfying the `#cfg(...)` predicate against the active `CfgSet`.
//!
//! S7: W_CFG_NO_IMPORT — a `#cfg` not immediately preceding an import emits a warning.

use std::fs;
use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{compile_project_with_entry_source_cfg, ModuleDag, ModuleResolver};
use reify_test_support::{compile_source, warnings_only};

// ── S5: DAG gating ───────────────────────────────────────────────────────────

/// Helper: extract module name strings from a Vec<CompiledModule>.
fn module_names(modules: &[reify_compiler::CompiledModule]) -> Vec<String> {
    modules.iter().map(|m| format!("{}", m.path)).collect()
}

/// Helper: make a CfgSet with only a target set.
fn target_cfg(target: &str) -> CfgSet {
    CfgSet { target: Some(target.to_string()), ..Default::default() }
}

/// Write the standard three-sibling layout into `dir`.
///
/// - `main.ri`: gated imports for linux/wasm + ungated import for common
/// - `platform_linux.ri`, `platform_wasm.ri`, `common.ri`: stub structures
fn write_gating_fixtures(dir: &Path) {
    let entry_src = "#cfg(target = \"linux\")\nimport platform_linux\n\
                     #cfg(target = \"wasm\")\nimport platform_wasm\n\
                     import common";
    fs::write(dir.join("main.ri"), entry_src).unwrap();
    fs::write(dir.join("platform_linux.ri"), "structure LinuxOnly { param x: Real }").unwrap();
    fs::write(dir.join("platform_wasm.ri"), "structure WasmOnly { param x: Real }").unwrap();
    fs::write(dir.join("common.ri"), "structure Common { param x: Real }").unwrap();
}

/// With `target = "linux"`, only `platform_linux` and `common` are compiled;
/// `platform_wasm` is skipped (its `#cfg(target = "wasm")` is unsatisfied).
#[test]
fn cfg_gating_linux_includes_linux_and_common() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();
    write_gating_fixtures(&dir);

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("main.ri");
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let cfg = target_cfg("linux");

    let modules = compile_project_with_entry_source_cfg(&entry_path, &entry_src, &resolver, &cfg)
        .expect("compilation should succeed");

    let names = module_names(&modules);
    assert!(
        names.iter().any(|n| n == "platform_linux"),
        "expected 'platform_linux' in module set, got {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "common"),
        "expected 'common' in module set, got {:?}",
        names
    );
    assert!(
        names.iter().all(|n| n != "platform_wasm"),
        "expected 'platform_wasm' to be absent (cfg unsatisfied), got {:?}",
        names
    );
}

/// With `target = "wasm"`, only `platform_wasm` and `common` are compiled;
/// `platform_linux` is skipped (its `#cfg(target = "linux")` is unsatisfied).
#[test]
fn cfg_gating_wasm_includes_wasm_and_common() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();
    write_gating_fixtures(&dir);

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("main.ri");
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let cfg = target_cfg("wasm");

    let modules = compile_project_with_entry_source_cfg(&entry_path, &entry_src, &resolver, &cfg)
        .expect("compilation should succeed");

    let names = module_names(&modules);
    assert!(
        names.iter().any(|n| n == "platform_wasm"),
        "expected 'platform_wasm' in module set, got {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "common"),
        "expected 'common' in module set, got {:?}",
        names
    );
    assert!(
        names.iter().all(|n| n != "platform_linux"),
        "expected 'platform_linux' to be absent (cfg unsatisfied), got {:?}",
        names
    );
}

/// Direct gating via `ModuleDag::with_cfg`: `mid.ri` has two gated imports;
/// `dag.modules` holds only the matching sibling after `compile_module`.
///
/// This tests the `with_cfg` entry point specifically (not `compile_project_with_entry_source_cfg`).
/// For the transitivity property (gate firing at non-entry depth), see
/// `cfg_gating_transitive_two_levels`.
#[test]
fn cfg_gating_via_with_cfg_entrypoint() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // mid.ri: gated linux + gated wasm imports
    let mid_src = "#cfg(target = \"linux\")\nimport sib_linux\n\
                   #cfg(target = \"wasm\")\nimport sib_wasm";
    fs::write(dir.join("mid.ri"), mid_src).unwrap();
    fs::write(dir.join("sib_linux.ri"), "structure SibLinux { param x: Real }").unwrap();
    fs::write(dir.join("sib_wasm.ri"), "structure SibWasm { param x: Real }").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let cfg = target_cfg("linux");

    let mut dag = ModuleDag::with_cfg(cfg);
    dag.compile_module("mid", &resolver).expect("compile_module should succeed");

    assert!(
        dag.modules.contains_key("sib_linux"),
        "expected sib_linux compiled, got keys: {:?}",
        dag.modules.keys().collect::<Vec<_>>()
    );
    assert!(
        !dag.modules.contains_key("sib_wasm"),
        "expected sib_wasm absent (cfg unsatisfied), got keys: {:?}",
        dag.modules.keys().collect::<Vec<_>>()
    );
}

/// True transitive gating: `entry.ri` imports `mid` (ungated), and `mid.ri` has
/// gated imports for linux/wasm siblings.  Compiled from `entry`, the gate fires
/// two levels down — the unsatisfied sibling is never compiled.
#[test]
fn cfg_gating_transitive_two_levels() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // entry.ri: ungated import of mid
    fs::write(dir.join("entry.ri"), "import mid").unwrap();
    // mid.ri: gated linux + gated wasm imports (one level below entry)
    let mid_src = "#cfg(target = \"linux\")\nimport sib_linux\n\
                   #cfg(target = \"wasm\")\nimport sib_wasm";
    fs::write(dir.join("mid.ri"), mid_src).unwrap();
    fs::write(dir.join("sib_linux.ri"), "structure SibLinux { param x: Real }").unwrap();
    fs::write(dir.join("sib_wasm.ri"), "structure SibWasm { param x: Real }").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("entry.ri");
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let cfg = target_cfg("linux");

    let modules =
        compile_project_with_entry_source_cfg(&entry_path, &entry_src, &resolver, &cfg)
            .expect("compilation should succeed");

    let names = module_names(&modules);
    assert!(
        names.iter().any(|n| n == "mid"),
        "expected 'mid' (ungated, one level down) in module set, got {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "sib_linux"),
        "expected 'sib_linux' (linux gate satisfied, two levels down) in module set, got {:?}",
        names
    );
    assert!(
        names.iter().all(|n| n != "sib_wasm"),
        "expected 'sib_wasm' absent (wasm gate unsatisfied, two levels down), got {:?}",
        names
    );
}

/// Stacked `#cfg` predicates on a single import use AND semantics: ALL
/// predicates must be satisfied for the import to be followed.  An import
/// with `#cfg(target="linux") + #cfg(target="wasm")` is never simultaneously
/// satisfied, so it is always skipped regardless of the active target.
///
/// This guards against a regression that switches `.all()` to `.any()` in
/// `import_cfg_satisfied`.
#[test]
fn cfg_gating_stacked_predicates_and_semantics() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // stacked.ri is gated by BOTH linux AND wasm simultaneously — never satisfied.
    let entry_src = "#cfg(target = \"linux\")\n\
                     #cfg(target = \"wasm\")\n\
                     import stacked\n\
                     import always";
    fs::write(dir.join("main_stacked.ri"), entry_src).unwrap();
    fs::write(dir.join("stacked.ri"), "structure Stacked { param x: Real }").unwrap();
    fs::write(dir.join("always.ri"), "structure Always { param x: Real }").unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("main_stacked.ri");

    // Under target=linux: first predicate is satisfied but second (#cfg(target="wasm")) is not.
    // AND-of-predicates → stacked is skipped.
    let modules =
        compile_project_with_entry_source_cfg(&entry_path, entry_src, &resolver, &target_cfg("linux"))
            .expect("compilation should succeed under linux");
    let names = module_names(&modules);
    assert!(
        names.iter().all(|n| n != "stacked"),
        "stacked should be absent under linux (second predicate #cfg(target=wasm) unsatisfied): {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "always"),
        "always (ungated) should be present under linux, got {:?}",
        names
    );

    // Under target=wasm: first predicate (#cfg(target="linux")) is not satisfied.
    // AND → stacked is still skipped.
    let modules2 =
        compile_project_with_entry_source_cfg(&entry_path, entry_src, &resolver, &target_cfg("wasm"))
            .expect("compilation should succeed under wasm");
    let names2 = module_names(&modules2);
    assert!(
        names2.iter().all(|n| n != "stacked"),
        "stacked should be absent under wasm (first predicate #cfg(target=linux) unsatisfied): {:?}",
        names2
    );
}

/// A gated-out import pointing at a non-existent module compiles cleanly: the
/// `#cfg` gate fires before any filesystem resolution, so the missing file is
/// never opened.  Conversely, a satisfied import at the same bad path errors.
#[test]
fn cfg_gating_unsatisfied_import_never_resolved() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    // "ghost.ri" is intentionally absent from `dir`.
    let entry_src = "#cfg(target = \"wasm\")\nimport ghost\nimport anchor";
    fs::write(dir.join("main_nr.ri"), entry_src).unwrap();
    fs::write(dir.join("anchor.ri"), "structure Anchor { param x: Real }").unwrap();
    // Note: ghost.ri is deliberately NOT created.

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("main_nr.ri");

    // Under linux: #cfg(target="wasm") is unsatisfied → ghost is skipped entirely,
    // never resolved from disk.
    let result =
        compile_project_with_entry_source_cfg(&entry_path, entry_src, &resolver, &target_cfg("linux"));
    assert!(
        result.is_ok(),
        "gated-out import to non-existent 'ghost' module should not error under linux \
         (gate fires before filesystem resolution): {:?}",
        result.err()
    );

    // Satisfied import at the same missing path MUST error (module must be resolved).
    let entry_src_satisfied = "#cfg(target = \"linux\")\nimport ghost\nimport anchor";
    let result_bad =
        compile_project_with_entry_source_cfg(&entry_path, entry_src_satisfied, &resolver, &target_cfg("linux"));
    assert!(
        result_bad.is_err(),
        "satisfied import pointing at missing 'ghost' module should return Err, got Ok"
    );
}

/// Gated-out module symbols are not in the entry module's prelude.
///
/// The compiler uses the same `import_cfg_satisfied` gate when building the
/// prelude for the entry module, so the entry module's compilation context
/// does not include symbols from gated-out siblings.  This is observable
/// because the compiler emits an "import … not resolved" diagnostic for any
/// source-level import that is absent from the prelude; that diagnostic
/// appears for gated-out imports and not for satisfied ones.
#[test]
fn cfg_gating_out_module_not_in_entry_prelude() {
    let _tmp = tempfile::tempdir().unwrap();
    let dir = _tmp.path().to_path_buf();

    write_gating_fixtures(&dir);

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let entry_path = dir.join("main.ri");
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let cfg = target_cfg("linux");

    let modules =
        compile_project_with_entry_source_cfg(&entry_path, &entry_src, &resolver, &cfg)
            .expect("compilation should succeed");

    // Entry module is last in topological order.
    let entry = modules.last().expect("at least one module returned");
    assert_eq!(
        format!("{}", entry.path),
        "main",
        "last module should be the entry ('main')"
    );

    // platform_wasm is gated out → absent from entry's prelude.
    // The compiler emits "import … not resolved" for any import absent from prelude.
    let unresolved_wasm: Vec<_> = entry
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("platform_wasm") && d.message.contains("not resolved"))
        .collect();
    assert_eq!(
        unresolved_wasm.len(),
        1,
        "expected exactly 1 'not resolved' diagnostic for gated-out platform_wasm \
         (proving its symbols are absent from entry's prelude), got {:?}",
        unresolved_wasm.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Satisfied imports (platform_linux, common) are in the prelude → no "not resolved".
    let resolved_linux: Vec<_> = entry
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("platform_linux") && d.message.contains("not resolved"))
        .collect();
    assert!(
        resolved_linux.is_empty(),
        "platform_linux (satisfied) should NOT have 'not resolved' diagnostic, got {:?}",
        resolved_linux.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── S7: W_CFG_NO_IMPORT ──────────────────────────────────────────────────────

/// Helper: warnings whose message contains the given substring.
fn warnings_containing<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_core::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

/// `#cfg(linux)` before a structure (not an import) emits exactly one
/// W_CFG_NO_IMPORT warning.
#[test]
fn cfg_before_structure_emits_w_cfg_no_import() {
    let src = "#cfg(linux)\nstructure S { param x: Real }";
    let module = compile_source(src);
    let warns = warnings_containing(&module, "W_CFG_NO_IMPORT");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 W_CFG_NO_IMPORT warning, got {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `#cfg(linux)` at EOF (no following decl) emits exactly one W_CFG_NO_IMPORT.
#[test]
fn cfg_at_eof_emits_w_cfg_no_import() {
    let src = "#cfg(linux)";
    let module = compile_source(src);
    let warns = warnings_containing(&module, "W_CFG_NO_IMPORT");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 W_CFG_NO_IMPORT warning at EOF, got {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `#cfg(linux)` immediately before an import does NOT emit W_CFG_NO_IMPORT.
#[test]
fn cfg_before_import_does_not_emit_w_cfg_no_import() {
    let src = "#cfg(linux)\nimport a.b";
    let module = compile_source(src);
    let warns = warnings_containing(&module, "W_CFG_NO_IMPORT");
    assert!(
        warns.is_empty(),
        "expected zero W_CFG_NO_IMPORT warnings for attached #cfg, got {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A plain structure with no `#cfg` produces zero W_CFG_NO_IMPORT warnings.
#[test]
fn no_cfg_no_w_cfg_no_import() {
    let src = "structure S { param x: Real }";
    let module = compile_source(src);
    let warns = warnings_containing(&module, "W_CFG_NO_IMPORT");
    assert!(
        warns.is_empty(),
        "expected zero W_CFG_NO_IMPORT warnings with no #cfg, got {:?}",
        warns.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
