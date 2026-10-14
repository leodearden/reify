use serde_json::Value;
use std::process::Command;

// Test fixture for gui subcommand integration tests.
// Uses the same pattern as cli_smoke.rs.
fn reify_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify"));
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd
}

#[test]
fn gui_no_file_shows_usage() {
    let output = reify_cmd()
        .arg("gui")
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with no file should exit non-zero"
    );
    assert!(
        stderr.contains("Usage") || stderr.contains("usage"),
        "should show usage message mentioning file path, got: {stderr}"
    );
    assert!(
        stderr.contains("<file>") || stderr.contains("file"),
        "usage message should mention a file argument, got: {stderr}"
    );
}

#[test]
fn gui_nonexistent_file_shows_error() {
    let output = reify_cmd()
        .args(["gui", "nonexistent.ri"])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with nonexistent file should exit non-zero"
    );
    assert!(
        stderr.contains("not found")
            || stderr.contains("does not exist")
            || stderr.contains("No such file"),
        "should report file not found error, got: {stderr}"
    );
}

#[test]
fn gui_non_ri_file_shows_error() {
    // Create a temporary .txt file inside a unique temp directory
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let txt_file = tmp_dir.path().join("test_reify_gui.txt");
    std::fs::write(&txt_file, "not a reify file").expect("failed to create temp file");

    let output = reify_cmd()
        .args(["gui", txt_file.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with non-.ri file should exit non-zero"
    );
    assert!(
        stderr.contains(".ri"),
        "should mention .ri extension requirement, got: {stderr}"
    );
}

#[test]
fn gui_extension_validation_fires_before_existence_check() {
    // Regression test: extension validation must fire before existence check,
    // so a non-existent non-.ri path reports the extension error rather than not-found.
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let nonexistent_path = tmp_dir.path().join("definitely_nonexistent.txt");
    // Do NOT create the file — it must not exist on disk.
    assert!(
        !nonexistent_path.exists(),
        "test file must not exist for this test to be meaningful"
    );

    let output = reify_cmd()
        .args(["gui", nonexistent_path.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with non-.ri non-existent file should exit non-zero"
    );
    assert!(
        stderr.contains(".ri"),
        "should mention .ri extension requirement (not existence), got: {stderr}"
    );
    assert!(
        !stderr.contains("does not exist")
            && !stderr.contains("not found")
            && !stderr.contains("No such file"),
        "should NOT report file-not-found (extension check fires first), got: {stderr}"
    );
}

#[test]
fn gui_with_valid_ri_file_skips_launch_when_env_set() {
    // Use the existing bracket.ri fixture
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        .args(["gui", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // The command should fail because the launch is skipped via env var,
    // but the error should be about the gui binary not being launched --
    // NOT about argument validation (file exists and has .ri extension).
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        stderr.contains("could not launch reify-gui"),
        "error should be about gui binary not launched (not arg validation), got: {stderr}"
    );
    assert!(
        stderr.contains("REIFY_GUI_SKIP_LAUNCH"),
        "error should mention REIFY_GUI_SKIP_LAUNCH env var, got: {stderr}"
    );
}

#[test]
fn gui_default_mode_probe_indicates_debug_false() {
    // `reify gui <fixture>` (no --debug) should emit `debug=false` on the
    // dedicated probe channel (REIFY_GUI_DEBUG_PROBE=1), keeping the
    // user-visible error message clean. The probe channel is a deliberate
    // test seam — opt-in via env var, not part of the default error path.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        .env("REIFY_GUI_DEBUG_PROBE", "1")
        .args(["gui", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        stderr.contains("REIFY_GUI_DEBUG_PROBE: debug=false"),
        "probe channel should report debug=false, got: {stderr}"
    );
}

#[test]
fn gui_debug_flag_probe_indicates_debug_true() {
    // `reify gui --debug <fixture>` should parse the flag and propagate it via
    // the probe channel as debug=true.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        .env("REIFY_GUI_DEBUG_PROBE", "1")
        .args(["gui", "--debug", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        stderr.contains("REIFY_GUI_DEBUG_PROBE: debug=true"),
        "probe channel for --debug should report debug=true, got: {stderr}"
    );
}

#[test]
fn gui_debug_subcommand_routes_to_cmd_gui_with_debug_true() {
    // `reify gui-debug <fixture>` should route through the same code path as
    // `reify gui --debug <fixture>` and produce debug=true on the probe
    // channel — this is the only end-to-end check that the gui-debug
    // subcommand wiring forwards `--debug` to cmd_gui.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        .env("REIFY_GUI_DEBUG_PROBE", "1")
        .args(["gui-debug", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        stderr.contains("REIFY_GUI_DEBUG_PROBE: debug=true"),
        "gui-debug subcommand should produce debug=true on probe, got: {stderr}"
    );
}

#[test]
fn gui_mcp_flag_is_alias_for_debug() {
    // `--mcp` is the alias spelling of `--debug` and should produce the same
    // debug=true marker on the probe channel.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        .env("REIFY_GUI_DEBUG_PROBE", "1")
        .args(["gui", "--mcp", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        stderr.contains("REIFY_GUI_DEBUG_PROBE: debug=true"),
        "probe channel for --mcp should report debug=true (alias for --debug), got: {stderr}"
    );
}

#[test]
fn gui_skip_launch_default_error_message_does_not_leak_debug_state() {
    // Without REIFY_GUI_DEBUG_PROBE, the SKIP_LAUNCH error must not contain
    // the parsed debug flag — that's internal state and should stay off the
    // user-visible error path. (The probe channel is the structured way to
    // observe it from tests.)
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bracket.ri");
    assert!(fixture.exists(), "fixture file should exist");

    let output = reify_cmd()
        .env("REIFY_GUI_SKIP_LAUNCH", "1")
        // Deliberately do NOT set REIFY_GUI_DEBUG_PROBE
        .args(["gui", "--debug", fixture.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "should exit non-zero when gui launch is skipped"
    );
    assert!(
        !stderr.contains("debug=true") && !stderr.contains("debug=false"),
        "default SKIP_LAUNCH error must not leak debug-flag state, got: {stderr}"
    );
    assert!(
        !stderr.contains("REIFY_GUI_DEBUG_PROBE"),
        "probe-channel marker must not appear without the env var set, got: {stderr}"
    );
}

#[test]
fn gui_unknown_flag_is_rejected() {
    // A `--`-prefixed token that isn't `--debug` or `--mcp` should be rejected
    // explicitly with a clear error, not silently passed through as a file
    // path (which would produce a confusing 'must have .ri extension' error
    // pointing at the flag).
    let output = reify_cmd()
        .args(["gui", "--debugg", "some.ri"])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "reify gui with unknown --typo flag should exit non-zero"
    );
    assert!(
        stderr.contains("--debugg"),
        "error should name the offending flag, got: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("unknown"),
        "error should describe the flag as unknown, got: {stderr}"
    );
}

fn read_tauri_config() -> Value {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../gui/src-tauri/tauri.conf.json");
    let content = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        panic!(
            "failed to read tauri.conf.json at {}: {}",
            config_path.display(),
            e
        )
    });
    serde_json::from_str(&content).expect("tauri.conf.json is not valid JSON")
}

#[test]
fn bundler_config_is_valid() {
    let config = read_tauri_config();

    // bundle.active should be true for distribution
    assert_eq!(
        config["bundle"]["active"],
        Value::Bool(true),
        "bundle.active should be true"
    );

    // identifier should be set
    assert_eq!(
        config["identifier"].as_str().unwrap(),
        "dev.reify.app",
        "identifier should be 'dev.reify.app'"
    );

    // bundle.icon should have entries
    let icons = config["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon should be an array");
    assert!(
        !icons.is_empty(),
        "bundle.icon should have at least one entry"
    );

    // productName should be set
    assert_eq!(
        config["productName"].as_str().unwrap(),
        "Reify",
        "productName should be 'Reify'"
    );
}

#[test]
fn bundler_config_has_platform_targets() {
    let config = read_tauri_config();

    // bundle.targets should include linux targets
    let targets = config["bundle"]["targets"]
        .as_array()
        .expect("bundle.targets should be an array");
    let target_strs: Vec<&str> = targets.iter().map(|t| t.as_str().unwrap()).collect();
    assert!(
        target_strs.contains(&"deb"),
        "bundle.targets should include 'deb', got: {:?}",
        target_strs
    );
    assert!(
        target_strs.contains(&"appimage"),
        "bundle.targets should include 'appimage', got: {:?}",
        target_strs
    );

    // fileAssociations should contain .ri extension
    let file_assocs = config["bundle"]["fileAssociations"]
        .as_array()
        .expect("bundle.fileAssociations should be an array");
    let has_ri = file_assocs.iter().any(|assoc| {
        assoc["ext"]
            .as_array()
            .map(|exts| exts.iter().any(|e| e.as_str() == Some("ri")))
            .unwrap_or(false)
    });
    assert!(has_ri, "fileAssociations should include .ri extension");
}
