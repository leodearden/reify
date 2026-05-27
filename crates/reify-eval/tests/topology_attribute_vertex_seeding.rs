//! PRD §8 user-observable signal test: vertex attribute seeding for Box primitives.
//!
//! This file is the integration test named in PRD
//! docs/prds/v0_3/mesh-morphing-phase-2.md §8 as the user-observable signal
//! for task 3633 (PNv2 vertex widening C).
//!
//! What it covers:
//!
//! 1. Direct kernel: `OcctKernelHandle::extract_vertices` on a 10mm box
//!    returns exactly 8 vertex handles.
//! 2. Engine-build: `engine.topology_attribute_table().iter()` after a
//!    `box(10mm, 10mm, 10mm)` realization yields exactly 8 `CornerVertex`
//!    entries covering all 8 `(AxisSign, AxisSign, AxisSign)` sign combos.
//! 3. Stability invariant: the `(x,y,z) → local_index` map is identical
//!    across two builds with different thicknesses on the same engine
//!    instance, pinning `pack_sign_bits` determinism.
//! 4. Sanity: each entry has the expected `feature_id`, `user_label == None`,
//!    `mod_history.is_empty()`.
//!
//! Gated on `OCCT_AVAILABLE` (same convention as sibling tests).

use std::collections::{HashMap, HashSet};

use reify_compiler::compile_with_stdlib;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_core::{ModulePath, RealizationNodeId, Severity};
use reify_ir::{AxisSign, ExportFormat, FeatureId, GeometryOp, Role, Value};

// ─── helpers (copied from topology_attribute_primitives_e2e.rs convention) ──

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed =
        reify_syntax::parse(source, ModulePath::single("test_topology_attr_vertex_seeding"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

fn engine_with_occt() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())))
}

fn assert_no_geometry_errors(build_result: &reify_eval::BuildResult) {
    let geom_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:#?}",
        geom_errors
    );
    assert!(
        build_result.geometry_output.is_some(),
        "expected geometry output for a primitive realization"
    );
}

// ─── PRD §8 signal: extract_vertices + attribute seeding for Box ──────────────

/// PRD §8 user-observable signal test for task 3633.
///
/// (a) Direct kernel: `extract_vertices(box_id)` returns exactly 8 handles.
/// (b) Engine-build: the table has exactly 8 `CornerVertex` entries after
///     `box(10mm,10mm,10mm)`, covering all 8 `(AxisSign,AxisSign,AxisSign)`
///     sign combos, each with the local_index from `pack_sign_bits`.
/// (c) Stability: re-building `box(12mm,12mm,12mm)` on the same engine
///     instance yields an identical `(x,y,z) → local_index` map.
/// (d) Sanity: feature_id, user_label, mod_history.
#[test]
fn extract_vertices_and_attribute_seeding_box() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // (a) Direct kernel: extract_vertices returns 8 handles for a 10mm box.
    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("box should build")
        .id;
    let vertex_handles = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices should succeed for a box");
    assert_eq!(
        vertex_handles.len(),
        8,
        "a 10mm box must have exactly 8 vertices; got {}",
        vertex_handles.len()
    );

    // (b) Engine-build: exactly 8 CornerVertex entries covering all 8 sign combos.
    let compiled = compile_no_errors("structure A { let body = box(10mm, 10mm, 10mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let expected_feature_id = FeatureId::from(&RealizationNodeId::new("A", 0));

    let map_1: HashMap<(AxisSign, AxisSign, AxisSign), u32> = engine
        .topology_attribute_table()
        .iter()
        .filter_map(|(_, attr)| {
            if let Role::CornerVertex { x, y, z } = attr.role {
                Some(((x, y, z), attr.local_index))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        map_1.len(),
        8,
        "expected exactly 8 CornerVertex entries (one per sign-combo corner); got {}",
        map_1.len()
    );

    // All 8 sign combos must be present as distinct keys.
    let expected_combos: HashSet<(AxisSign, AxisSign, AxisSign)> = [
        (AxisSign::Pos, AxisSign::Pos, AxisSign::Pos),
        (AxisSign::Pos, AxisSign::Pos, AxisSign::Neg),
        (AxisSign::Pos, AxisSign::Neg, AxisSign::Pos),
        (AxisSign::Pos, AxisSign::Neg, AxisSign::Neg),
        (AxisSign::Neg, AxisSign::Pos, AxisSign::Pos),
        (AxisSign::Neg, AxisSign::Pos, AxisSign::Neg),
        (AxisSign::Neg, AxisSign::Neg, AxisSign::Pos),
        (AxisSign::Neg, AxisSign::Neg, AxisSign::Neg),
    ]
    .into_iter()
    .collect();
    let actual_combos: HashSet<(AxisSign, AxisSign, AxisSign)> = map_1.keys().copied().collect();
    assert_eq!(
        actual_combos, expected_combos,
        "CornerVertex entries must cover all 8 sign-combo corners"
    );

    // (d) Sanity: feature_id, user_label, mod_history for all CornerVertex entries.
    for (_, attr) in engine.topology_attribute_table().iter() {
        if !matches!(attr.role, Role::CornerVertex { .. }) {
            continue;
        }
        assert_eq!(
            attr.feature_id, expected_feature_id,
            "CornerVertex entry must carry the realization's feature_id"
        );
        assert!(
            attr.user_label.is_none(),
            "CornerVertex entry must have no user_label"
        );
        assert!(
            attr.mod_history.is_empty(),
            "CornerVertex entry must have empty mod_history"
        );
    }

    // (c) Stability: re-build with box(12mm,12mm,12mm) on the same engine instance.
    let compiled_12 = compile_no_errors("structure A { let body = box(12mm, 12mm, 12mm) }");
    let build_result_12 = engine.build(&compiled_12, ExportFormat::Step);
    assert_no_geometry_errors(&build_result_12);

    let map_2: HashMap<(AxisSign, AxisSign, AxisSign), u32> = engine
        .topology_attribute_table()
        .iter()
        .filter_map(|(_, attr)| {
            if let Role::CornerVertex { x, y, z } = attr.role {
                Some(((x, y, z), attr.local_index))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        map_1, map_2,
        "pack_sign_bits stability invariant: (x,y,z) → local_index map must be \
         identical across builds with different box dimensions"
    );
}
