mod claude_bridge_tests;
mod commands_tests;
mod debug_boundary_tests;
mod diff_tests;
mod engine_lock_tests;
mod engine_tests;
mod event_bus_tests;
mod gui_state_macro_tests;
mod gui_state_parity_tests;
mod kernel_status_tests;
mod large_stack_tests;
mod lsp_bridge_tests;
mod main_helpers_tests;
mod mcp_context_tests;
mod mcp_dispatch_tests;
mod path_key_tests;
pub(crate) mod test_helpers;
mod test_helpers_tests;
mod types_tests;
mod watcher_tests;

use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::engine::EngineSession;

/// Shared engine fixture for tests across this crate's test modules.
///
/// Builds a real [`EngineSession`] backed by a [`MockGeometryKernel`] with
/// a known-good source file pre-loaded, wrapped in an `Arc<Mutex<…>>` ready
/// for use with [`crate::engine_lock::with_engine_lock`] and related helpers.
pub(crate) fn make_test_engine() -> Arc<Mutex<EngineSession>> {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");
    Arc::new(Mutex::new(session))
}

/// Compile-time assertion that a type satisfies the full GUI IPC contract:
/// serializable, deserializable (owned), cloneable, debuggable, and comparable.
fn assert_ipc_contract<
    T: serde::Serialize + serde::de::DeserializeOwned + Clone + std::fmt::Debug + PartialEq,
>() {
}

// Step 11: Module structure verification — importing all public types.
#[test]
fn public_api_types_are_accessible() {
    use crate::commands::AppState;
    use crate::engine::EngineSession;
    use crate::types::{
        ConstraintData, FileData, GuiState, JointBinding, JointDescriptor, MechanismDescriptor,
        MeshData, ValueData,
    };
    use reify_mcp::{DiagnosticInfo, SourceLocationInfo};

    // Verify full IPC contract (Serialize + DeserializeOwned + Clone + Debug + PartialEq)
    assert_ipc_contract::<GuiState>();
    assert_ipc_contract::<MeshData>();
    assert_ipc_contract::<ValueData>();
    assert_ipc_contract::<ConstraintData>();
    assert_ipc_contract::<SourceLocationInfo>();
    assert_ipc_contract::<FileData>();
    // DiagnosticInfo is the MCP canonical replacement for the removed GUI-local type
    assert_ipc_contract::<DiagnosticInfo>();
    // Mechanism descriptor types introduced in task 2536
    assert_ipc_contract::<MechanismDescriptor>();
    assert_ipc_contract::<JointDescriptor>();
    // JointBinding enum introduced in task 3783
    assert_ipc_contract::<JointBinding>();

    // Verify AppState and EngineSession are usable as types
    let _ = std::any::type_name::<AppState>();
    let _ = std::any::type_name::<EngineSession>();
}

/// Resolve this crate's manifest dir, preferring the RUNTIME
/// `CARGO_MANIFEST_DIR` over the compile-time `env!()` bake.
///
/// The bake goes stale when a seeded warm-lane `target/` is reused from a
/// since-deleted worktree — `CARGO_MANIFEST_DIR` is not part of cargo's
/// fingerprint, so a content-identical rebuild is never triggered. Same
/// hazard and same fix as `eval_crate_manifest_dir` in
/// `crates/reify-eval/src/geometry_ops/tests.rs` (esc-4906-57).
fn gui_crate_manifest_dir() -> String {
    std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string())
}

/// Guards against a file under `src/tests/` that this module never
/// declares with `mod <name>;`. An undeclared file is not part of any
/// compilation unit, so any `#[test]` fns inside it silently never run
/// (task 6812 — `mechanism_descriptors_tests.rs` was exactly this trap:
/// it read as a registered test module but had no `mod` line).
///
/// Only the file → declaration direction is checked here. The reverse —
/// a `mod` declaration with no backing file — is already a hard `rustc`
/// error (E0583), so asserting it here would be dead weight.
#[test]
fn every_test_module_file_is_declared() {
    use std::collections::BTreeSet;

    let manifest_dir = gui_crate_manifest_dir();
    let tests_dir = std::path::Path::new(&manifest_dir).join("src/tests");

    let mod_rs_path = tests_dir.join("mod.rs");
    let mod_rs_src = std::fs::read_to_string(&mod_rs_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} for the orphan-test-module guard: {}",
            mod_rs_path.display(),
            e
        )
    });

    // Parse the declared `mod <name>;` lines out of this very file, one
    // Rust source line at a time — good enough for this directory's
    // uniformly flat `mod ident;` / `pub(crate) mod ident;` style.
    let declared: BTreeSet<String> = mod_rs_src
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                return None;
            }
            let rest = trimmed
                .strip_prefix("pub(crate) ")
                .or_else(|| trimmed.strip_prefix("pub(super) "))
                .or_else(|| trimmed.strip_prefix("pub "))
                .unwrap_or(trimmed);
            let name = rest.strip_prefix("mod ")?.strip_suffix(';')?;
            Some(name.trim().to_string())
        })
        .filter(|name| !name.is_empty())
        .collect();

    // Collect on-disk module candidates: every `*.rs` file in this
    // directory except `mod.rs` itself.
    let on_disk: BTreeSet<String> = std::fs::read_dir(&tests_dir)
        .unwrap_or_else(|e| {
            panic!(
                "failed to read_dir {} for the orphan-test-module guard: {}",
                tests_dir.display(),
                e
            )
        })
        .map(|entry| entry.expect("failed to read a dir entry while scanning src/tests").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rs"))
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .filter(|stem| stem != "mod")
        .collect();

    // NON-VACUITY FLOOR — checked before the real assertion so a broken
    // path or a line-parser that silently stops matching can never make
    // this guard pass vacuously. There are 21 on-disk candidates and 20
    // declarations today; 15 leaves headroom for legitimate future
    // removals (mirrors the ratchet-vacuity floor in
    // `tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh`).
    assert!(
        on_disk.len() >= 15,
        "on-disk scan of {} found only {} `*.rs` file(s) (expected >= 15) — \
         the orphan-test-module guard may be reading the wrong directory",
        tests_dir.display(),
        on_disk.len()
    );
    assert!(
        declared.len() >= 15,
        "declared-module scan of {} found only {} `mod` declaration(s) (expected >= 15) — \
         the orphan-test-module guard's line parser may be broken",
        mod_rs_path.display(),
        declared.len()
    );

    let orphans: Vec<&String> = on_disk.difference(&declared).collect();
    assert!(
        orphans.is_empty(),
        "file(s) under gui/src-tauri/src/tests/ have no `mod` declaration in mod.rs: {:?} — \
         either add `mod <name>;` to gui/src-tauri/src/tests/mod.rs in alphabetical position, \
         or delete the file — an undeclared file in this directory is in NO compilation unit, \
         so any #[test] fns in it silently never run",
        orphans
    );
}
