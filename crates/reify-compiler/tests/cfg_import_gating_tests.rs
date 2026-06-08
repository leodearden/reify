//! Tests for cfg-gated import filtering in the module DAG.
//!
//! S5: DAG gating — `compile_project_with_entry_source_cfg` and `ModuleDag::with_cfg`
//! filter imports by satisfying the `#cfg(...)` predicate against the active `CfgSet`.
//!
//! S7: W_CFG_NO_IMPORT — a `#cfg` not immediately preceding an import emits a warning.

use std::fs;
use std::path::PathBuf;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{compile_project_with_entry_source_cfg, ModuleDag, ModuleResolver};

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
fn write_gating_fixtures(dir: &PathBuf) {
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

/// Transitive gating via `ModuleDag::with_cfg`: `mid.ri` has two gated imports;
/// `dag.modules` holds only the matching sibling after `compile_module`.
#[test]
fn cfg_gating_transitive_via_with_cfg() {
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

// ── S7: W_CFG_NO_IMPORT ──────────────────────────────────────────────────────
// (Added in step S7 — this section is a placeholder for future tests)
