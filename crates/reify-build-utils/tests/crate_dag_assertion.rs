/// Workspace-wide DAG invariant gate.
///
/// Invokes `scripts/assert-crate-dag.sh` and asserts:
///   1. The script exits 0 (all invariants pass).
///   2. Every B-check line (`B1 OK` … `B6 OK`) appears in stdout, confirming
///      each individual invariant was exercised and passed.
///
/// This is the integration gate for task η of
/// docs/prds/core-ast-ir-layering.md. It is intentionally RED until the
/// cutover in step-4 lands (step-2 wires the script; the script exits nonzero
/// while reify-types is still present; step-4 removes reify-types → GREEN).
#[test]
fn workspace_dag_invariant_via_assert_script() {
    use std::path::Path;
    use std::process::Command;

    // CARGO_MANIFEST_DIR resolves to `crates/reify-build-utils/` at test time.
    // Two levels up reaches the workspace root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent() // crates/
        .expect("parent of CARGO_MANIFEST_DIR")
        .parent() // workspace root
        .expect("workspace root");

    let script = workspace_root.join("scripts/assert-crate-dag.sh");

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(workspace_root)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to invoke assert-crate-dag.sh at {}: {e}\n\
                 (script must exist at scripts/assert-crate-dag.sh relative to workspace root)",
                script.display()
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check each invariant is present and marked OK in stdout.
    let required_markers = ["B1 OK", "B2 OK", "B3 OK", "B4 OK", "B5 OK", "B6 OK"];
    let missing: Vec<&str> = required_markers
        .iter()
        .copied()
        .filter(|marker| !stdout.contains(marker))
        .collect();

    if !output.status.success() || !missing.is_empty() {
        panic!(
            "assert-crate-dag.sh failed.\n\
             Exit status: {}\n\
             Missing markers: {:?}\n\
             --- stdout ---\n{}\n--- stderr ---\n{}",
            output.status, missing, stdout, stderr
        );
    }
}
