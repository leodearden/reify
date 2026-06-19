//! Integration test for task 3598/θ — Engine-side shell GUI mesh accessor.
//!
//! `Engine::shell_gui_mesh_data()` scans the evaluation graph for
//! shell-extract + elastic-static ComputeNodes, reads their cached results,
//! and returns one `ShellGuiMeshData` per shell body with:
//!
//! - `vertices` (flat f32 XYZ; len % 3 == 0, non-empty)
//! - `indices` (flat u32; len % 3 == 0, non-empty)
//! - `element_kind` (all == 1; len == face_count)
//! - `region_tags` (len == face_count, from SegmentationResult.triangle_labels)
//! - `von_mises_top` / `von_mises_mid` / `von_mises_bottom` (len == vertex_count,
//!   all-finite)
//! - `shell_normals_per_face` (len == 3 * face_count, all-finite)
//!
//! Uses the existing `shell_engine()` + `fea_shell_flexure.ri` fixture from
//! `shell_solve_e2e.rs` (same harness, same fixture file).
//!
//! PRD: docs/prds/v0_4/shell-extract-engine-bridge.md §9 Phase 6 task θ.
//!
//! step-1 RED  — assert structural/length/finiteness contracts on accessor output.
//!               Fails to compile until step-2 adds ShellGuiMeshData + accessor.

use reify_core::Severity;

// ── fixture helpers (mirrors shell_solve_e2e.rs) ──────────────────────────────

fn shell_source() -> &'static str {
    include_str!("../../../examples/fea_shell_flexure.ri")
}

/// Build an engine with both trampolines registered (same as shell_solve_e2e.rs).
fn shell_engine() -> reify_eval::Engine {
    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}

// ── step-1 RED test ───────────────────────────────────────────────────────────

/// `Engine::shell_gui_mesh_data()` returns exactly one `ShellGuiMeshData` for
/// the shell flexure fixture, with all structural/length/finiteness contracts
/// satisfied.  Physical stress magnitudes are NOT asserted (v0.4 OQ-2).
#[test]
fn shell_gui_mesh_data_returns_one_entry_with_valid_fields() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(shell_source());
    let mut engine = shell_engine();
    let eval_result = engine.eval(&compiled);

    // Require a clean solve.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // Call the accessor under test.
    let views: Vec<reify_eval::ShellGuiMeshData> = engine.shell_gui_mesh_data();

    // Debug output to understand what we got.
    eprintln!(
        "DEBUG: shell_gui_mesh_data returned {} entries",
        views.len()
    );
    for (i, v) in views.iter().enumerate() {
        eprintln!(
            "  [{}] entity_path={:?} vertices={} indices={} element_kind={} region_tags={} von_mises_top={} von_mises_mid={} von_mises_bottom={} shell_normals_per_face={}",
            i,
            v.entity_path,
            v.vertices.len(),
            v.indices.len(),
            v.element_kind.len(),
            v.region_tags.len(),
            v.von_mises_top.len(),
            v.von_mises_mid.len(),
            v.von_mises_bottom.len(),
            v.shell_normals_per_face.len()
        );
    }

    // Exactly one shell body.
    assert_eq!(
        views.len(),
        1,
        "expected exactly one ShellGuiMeshData for the shell flexure fixture, got {}",
        views.len()
    );

    let v = &views[0];

    // entity_path is non-empty.
    assert!(!v.entity_path.is_empty(), "entity_path must be non-empty");

    // vertices: non-empty, flat XYZ (len % 3 == 0).
    assert!(!v.vertices.is_empty(), "vertices must be non-empty");
    assert_eq!(
        v.vertices.len() % 3,
        0,
        "vertices len {} must be divisible by 3",
        v.vertices.len()
    );
    let vertex_count = v.vertices.len() / 3;

    // indices: non-empty, flat triangles (len % 3 == 0).
    assert!(!v.indices.is_empty(), "indices must be non-empty");
    assert_eq!(
        v.indices.len() % 3,
        0,
        "indices len {} must be divisible by 3",
        v.indices.len()
    );
    let face_count = v.indices.len() / 3;

    // element_kind: all == 1 (shell triangle), len == face_count.
    assert_eq!(
        v.element_kind.len(),
        face_count,
        "element_kind len {} must == face_count {}",
        v.element_kind.len(),
        face_count
    );
    assert!(
        v.element_kind.iter().all(|&k| k == 1),
        "all element_kind bytes must be 1 (shell triangle)"
    );

    // region_tags: len == face_count.
    assert_eq!(
        v.region_tags.len(),
        face_count,
        "region_tags len {} must == face_count {}",
        v.region_tags.len(),
        face_count
    );

    // von_mises_top: non-empty, all-finite, len == vertex_count.
    assert!(
        !v.von_mises_top.is_empty(),
        "von_mises_top must be non-empty"
    );
    assert_eq!(
        v.von_mises_top.len(),
        vertex_count,
        "von_mises_top len {} must == vertex_count {}",
        v.von_mises_top.len(),
        vertex_count
    );
    assert!(
        v.von_mises_top.iter().all(|x| x.is_finite()),
        "von_mises_top must be all-finite"
    );

    // von_mises_mid: non-empty, all-finite, len == vertex_count.
    assert!(
        !v.von_mises_mid.is_empty(),
        "von_mises_mid must be non-empty"
    );
    assert_eq!(
        v.von_mises_mid.len(),
        vertex_count,
        "von_mises_mid len {} must == vertex_count {}",
        v.von_mises_mid.len(),
        vertex_count
    );
    assert!(
        v.von_mises_mid.iter().all(|x| x.is_finite()),
        "von_mises_mid must be all-finite"
    );

    // von_mises_bottom: non-empty, all-finite, len == vertex_count.
    assert!(
        !v.von_mises_bottom.is_empty(),
        "von_mises_bottom must be non-empty"
    );
    assert_eq!(
        v.von_mises_bottom.len(),
        vertex_count,
        "von_mises_bottom len {} must == vertex_count {}",
        v.von_mises_bottom.len(),
        vertex_count
    );
    assert!(
        v.von_mises_bottom.iter().all(|x| x.is_finite()),
        "von_mises_bottom must be all-finite"
    );

    // shell_normals_per_face: len == 3 * face_count, all-finite.
    assert_eq!(
        v.shell_normals_per_face.len(),
        3 * face_count,
        "shell_normals_per_face len {} must == 3 * face_count {}",
        v.shell_normals_per_face.len(),
        3 * face_count
    );
    assert!(
        v.shell_normals_per_face.iter().all(|x| x.is_finite()),
        "shell_normals_per_face must be all-finite"
    );

    // Self-consistency: von_mises_mid and von_mises_top are non-negative
    // (stress magnitudes from von Mises formula are ≥ 0).
    assert!(
        v.von_mises_mid.iter().all(|&x| x >= 0.0),
        "von_mises_mid must be non-negative"
    );
    assert!(
        v.von_mises_top.iter().all(|&x| x >= 0.0),
        "von_mises_top must be non-negative"
    );
    assert!(
        v.von_mises_bottom.iter().all(|&x| x >= 0.0),
        "von_mises_bottom must be non-negative"
    );
}

/// `Engine::shell_gui_mesh_data()` returns an empty Vec for a non-shell fixture
/// (so the accessor has negligible overhead on non-shell scenes).
#[test]
fn shell_gui_mesh_data_returns_empty_for_non_shell_fixture() {
    // Use any non-shell fixture; the cantilever is a tet-only solve.
    let source = include_str!("../../../examples/fea_cantilever_smoke.ri");
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = shell_engine();
    engine.eval(&compiled);
    let views: Vec<reify_eval::ShellGuiMeshData> = engine.shell_gui_mesh_data();
    assert!(
        views.is_empty(),
        "expected empty Vec for a non-shell (tet) scene, got {} entries",
        views.len()
    );
}
