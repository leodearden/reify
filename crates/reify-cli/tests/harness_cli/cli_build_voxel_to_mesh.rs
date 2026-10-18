use crate::common;

use std::process::Command;

use reify_test_support::stl_bytes::binary_stl_aabb;

/// Task 5002 (δ) integration gate, strengthened by task 6308: `reify build
/// examples/multi_kernel/voxel_to_mesh.ri -o shell.stl` (a real BRep→Voxel
/// operand feeding a Voxel→Mesh `isosurface(...)` terminal) must exit 0 and
/// write the WHOLE isosurface: at least 10,000 triangles, a printed
/// `Triangles: N` that agrees with the file, and an extent in [18, 22] mm on
/// every axis.
///
/// Basis: docs/prds/v0_3/voxel-to-mesh-surfacing.md measured 50,700 triangles;
/// with h = 20 mm / 64 the iso-0 faces sit within ~0.6 mm of ±10 mm. A
/// single-face mesh (the pre-6308 defect: the OpenVDB-owned terminal exported
/// through OCCT printed `Triangles: 2`) fails both the floor and the 3-axis
/// bbox.
///
/// Gated `#[cfg(has_openvdb)]` (reify-cli's build.rs emits this cfg only when
/// OpenVDB native libraries are available) — mirrors the engine-level e2e
/// test's gate.
#[cfg(has_openvdb)]
#[test]
fn build_voxel_to_mesh_writes_the_whole_isosurface() {
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
        n >= 10_000,
        "the 20mm box's marching-cubes isosurface must have at least 10,000 triangles \
         (measured 50,700); got {n}\nstdout: {stdout}"
    );

    let stl = std::fs::read(&output_path).expect("failed to read the written STL");
    let aabb = binary_stl_aabb(&stl);
    assert_eq!(
        aabb.triangles as u64, n,
        "the printed triangle count must agree with the written file"
    );
    for (axis, extent) in aabb.extent().into_iter().enumerate() {
        assert!(
            (18.0..=22.0).contains(&extent),
            "axis {axis}: the 20mm box's isosurface must span 18..=22 mm, got {extent} mm \
             ({aabb:?})"
        );
    }
}
