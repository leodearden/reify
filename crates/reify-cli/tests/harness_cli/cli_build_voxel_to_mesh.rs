use crate::common;

use std::process::Command;

/// Task 5002 (δ) integration gate: `reify build examples/multi_kernel/voxel_to_mesh.ri`
/// (a real BRep→Voxel operand feeding a Voxel→Mesh `isosurface(...)` terminal)
/// must exit 0 and print a `Triangles: N` line with `N > 0`.
///
/// This is the CLI-observable half of the honest signal proven at the engine
/// level by `crates/reify-eval/tests/voxel_to_mesh_e2e.rs`: the same
/// `tessellate_realizations` mesh egress the GUI viewport uses (PRD B5).
///
/// Gated `#[cfg(has_openvdb)]` (reify-cli's build.rs emits this cfg only when
/// OpenVDB native libraries are available) — mirrors the engine-level e2e
/// test's gate.
///
/// RED (before step-4 wires `cmd_build`): `cmd_build` constructs its engine
/// via `Engine::with_registered_kernel` alone (OCCT only, no OpenVDB), so the
/// isosurface build's Mesh→Voxel + Voxel→Mesh stages have no kernel to
/// dispatch to — the build degrades to an Error diagnostic and exits
/// non-zero. `cmd_build` also does not yet print any `Triangles:` line.
#[cfg(has_openvdb)]
#[test]
fn build_voxel_to_mesh_prints_positive_triangle_count() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("shell.stl");

    let example = common::example_path("multi_kernel/voxel_to_mesh.ri");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &example,
            "-o",
            output_path.to_str().expect("temp path is not valid UTF-8"),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "reify build should exit 0 for multi_kernel/voxel_to_mesh.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should contain 'Wrote' on a successful build\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        output_path.exists(),
        "geometry file should be written on success"
    );

    // Parse the `Triangles: N` line and assert N > 0 (binary — no numeric
    // bound, per PRD's avoidance of a guessed exact count).
    let triangles_line = stdout
        .lines()
        .find(|line| line.starts_with("Triangles:"))
        .unwrap_or_else(|| {
            panic!("stdout should contain a 'Triangles: N' line\nstdout: {stdout}\nstderr: {stderr}")
        });
    let n: u64 = triangles_line
        .trim_start_matches("Triangles:")
        .trim()
        .parse()
        .unwrap_or_else(|e| {
            panic!(
                "failed to parse triangle count from {triangles_line:?}: {e}\nstdout: {stdout}"
            )
        });
    assert!(
        n > 0,
        "triangle count must be > 0 (marching cubes on a closed 20mm box's \
         narrow-band SDF must yield a non-empty mesh); got {n}\nstdout: {stdout}"
    );
}
