//! End-to-end tests for v0.2 persistent-naming-v2 attribute auto-population
//! during sweep ops (extrude / revolve) — task 5a (#2573).
//!
//! Source-language constructors only expose `box`/`cylinder`/`sphere`/`tube`
//! (compiler/src/types.rs `enum PrimitiveKind`); there is no source-level
//! face-profile constructor (`rect_face` / `circle_face` etc. are
//! kernel-FFI-only). We therefore cannot drive a real OCCT extrude or revolve
//! through `Engine::build` end-to-end at the source layer.
//!
//! Instead, this file pairs a **synthesised `CompiledModule`** (built via the
//! same `CompiledModuleBuilder` / `TopologyTemplateBuilder` pattern as
//! `extrude_e2e.rs`) with a **mock `GeometryKernel` that injects synthetic
//! `AttributeHistory::Extrude` / `AttributeHistory::Revolve` records**. The
//! kernel-direct integration tests (`extrude_with_history_integration.rs`,
//! `revolve_with_history_integration.rs`) cover the FFI behaviour against
//! real OCCT; these e2e tests cover the engine-level wiring —
//! `Engine::execute_realization_ops`'s match-on-AttributeHistory →
//! `populate_extrude_attributes` / `populate_revolve_attributes` →
//! `topology_attribute_table` write path.
//!
//! Selector-stability is verified within each test by running `Engine::build`
//! twice on the same engine and asserting that the per-result-handle
//! `(FeatureId, role, local_index)` triples are equal across both builds.
//! The mock kernel returns *configured* result-face / result-edge handle
//! ids from `extract_*` (rather than freshly allocating them), so the same
//! handle ids are written into the table on both runs and `lookup()` is
//! directly comparable.

use std::sync::{Arc, Mutex};

use reify_compiler::{CompiledGeometryOp, CurveKind, GeomRef, SweepKind};
use reify_core::{ModulePath, Type};
use reify_ir::{
    AttributeHistory, CapKind, CompiledExpr, ExportFormat, FeatureId, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, HistoryRecord,
    Mesh, QueryError, Role, SweepOpHistoryRecords, TessError, TopologyAttribute, Value,
};
use reify_test_support::*;

// ─── HistoryMockKernel ────────────────────────────────────────────────────────

/// Mock `GeometryKernel` that wraps `MockGeometryKernel` to:
///
/// 1. Override `execute_with_history` to inject synthetic
///    `AttributeHistory::Extrude` / `AttributeHistory::Revolve` records
///    for matching `GeometryOp` variants. All other ops fall through to
///    `inner.execute(op)` and return `AttributeHistory::None`.
///
/// 2. Override `extract_faces` / `extract_edges` to return
///    *configured* face/edge slices based on whether the queried handle
///    is the just-allocated sweep result (return `result_faces` /
///    `result_edges`) or any other handle (return `profile_faces` /
///    `profile_edges`). The disambiguator is `last_sweep_result`,
///    which `execute_with_history` updates whenever an Extrude/Revolve
///    op runs through this kernel.
///
/// Other trait methods (`query`, `export`, `tessellate`) delegate to
/// `inner` unchanged.
struct HistoryMockKernel {
    inner: MockGeometryKernel,
    profile_faces: Vec<GeometryHandleId>,
    profile_edges: Vec<GeometryHandleId>,
    result_faces: Vec<GeometryHandleId>,
    result_edges: Vec<GeometryHandleId>,
    extrude_history: Option<SweepOpHistoryRecords>,
    revolve_history: Option<SweepOpHistoryRecords>,
    /// Set whenever `execute_with_history` runs an Extrude or Revolve op.
    /// Used by `extract_faces` / `extract_edges` to return result-vs-profile
    /// slices without depending on the inner `next_id` allocation order.
    /// `Arc<Mutex<...>>` so the wrapper is `Send + Sync` even though the
    /// trait methods need interior mutability.
    last_sweep_result: Arc<Mutex<Option<GeometryHandleId>>>,
}

impl HistoryMockKernel {
    fn new(
        profile_faces: Vec<GeometryHandleId>,
        profile_edges: Vec<GeometryHandleId>,
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    ) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            profile_faces,
            profile_edges,
            result_faces,
            result_edges,
            extrude_history: None,
            revolve_history: None,
            last_sweep_result: Arc::new(Mutex::new(None)),
        }
    }

    fn with_extrude_history(mut self, history: SweepOpHistoryRecords) -> Self {
        self.extrude_history = Some(history);
        self
    }

    fn with_revolve_history(mut self, history: SweepOpHistoryRecords) -> Self {
        self.revolve_history = Some(history);
        self
    }
}

impl GeometryKernel for HistoryMockKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.inner.execute(op)?;
        let history = match op {
            GeometryOp::Extrude { .. } => {
                *self.last_sweep_result.lock().unwrap() = Some(handle.id);
                self.extrude_history
                    .clone()
                    .map_or(AttributeHistory::None, AttributeHistory::Extrude)
            }
            GeometryOp::Revolve { .. } => {
                *self.last_sweep_result.lock().unwrap() = Some(handle.id);
                self.revolve_history
                    .clone()
                    .map_or(AttributeHistory::None, AttributeHistory::Revolve)
            }
            _ => AttributeHistory::None,
        };
        Ok((handle, history))
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if Some(handle) == *self.last_sweep_result.lock().unwrap() {
            Ok(self.result_faces.clone())
        } else {
            Ok(self.profile_faces.clone())
        }
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if Some(handle) == *self.last_sweep_result.lock().unwrap() {
            Ok(self.result_edges.clone())
        } else {
            Ok(self.profile_edges.clone())
        }
    }

    fn query(&self, q: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }
}

// ─── shared helpers ───────────────────────────────────────────────────────────

fn mm_literal(v: f64) -> CompiledExpr {
    CompiledExpr::literal(mm(v), Type::length())
}

fn real_literal(v: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::Real(v), Type::Real)
}

/// Build a synthesised `CompiledModule` with two ops:
/// (0) a LineSegment curve (a non-primitive whose primitive-seeding step
///     is a no-op, so the table only carries our injected sweep entries);
/// (1) a Sweep referencing `Step(0)` as its profile, with the requested
///     `kind` and `args`.
fn synthesised_sweep_module(
    realization_name: &str,
    sweep_kind: SweepKind,
    sweep_args: Vec<(String, CompiledExpr)>,
) -> reify_compiler::CompiledModule {
    let line_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(10.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(0.0)),
        ],
    };
    let sweep_op = CompiledGeometryOp::Sweep {
        kind: sweep_kind,
        profiles: vec![GeomRef::Step(0)],
        args: sweep_args,
    };
    let template = TopologyTemplateBuilder::new(realization_name)
        .realization(realization_name, 0, vec![line_op, sweep_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_extrude_revolve_e2e"))
        .template(template)
        .build()
}

fn extrude_module() -> reify_compiler::CompiledModule {
    synthesised_sweep_module(
        "TestExtrude",
        SweepKind::Extrude,
        vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(10.0)),
        ],
    )
}

fn revolve_module() -> reify_compiler::CompiledModule {
    synthesised_sweep_module(
        "TestRevolve",
        SweepKind::Revolve,
        vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(std::f64::consts::PI)),
        ],
    )
}

/// Collect the `(role, local_index, feature_id, user_label, mod_history)`
/// projection of every entry in `engine.topology_attribute_table()` keyed by
/// `result_face_handles[idx]` for each `idx in indices`. Returns `None` for
/// any index whose handle has no entry — the caller asserts `Some` for the
/// expected ones and `None` for the deliberately-unkeyed ones.
fn collect_attrs_at(
    engine: &reify_eval::Engine,
    result_face_handles: &[GeometryHandleId],
    indices: &[usize],
) -> Vec<Option<TopologyAttribute>> {
    let table = engine.topology_attribute_table();
    indices
        .iter()
        .map(|&idx| table.lookup(result_face_handles[idx]).cloned())
        .collect()
}

// ─── Test 1: extrude with mock history ────────────────────────────────────────

/// Per step-15 (1): synthetic prism history with start_cap=[5], end_cap=[6],
/// face_generated=[(0,0,7),(0,1,8)] over a result of 10 faces. Asserts the
/// resulting `topology_attribute_table` has exactly 4 entries (2 caps + 2
/// sides) keyed by the configured result-face ids; verifies stability by
/// running `Engine::build` twice and asserting equal entries on both builds.
#[test]
fn engine_build_extrude_with_mock_history_populates_table_with_cap_and_side_entries() {
    let module = extrude_module();
    let result_faces: Vec<GeometryHandleId> = (5000..5010).map(GeometryHandleId).collect();
    let result_edges: Vec<GeometryHandleId> = (6000..6020).map(GeometryHandleId).collect();
    let profile_faces = vec![GeometryHandleId(5050)];
    let profile_edges: Vec<GeometryHandleId> = (5060..5064).map(GeometryHandleId).collect();
    let history = SweepOpHistoryRecords {
        face_generated: vec![
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 7,
            },
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 1,
                result_subshape_index: 8,
            },
        ],
        start_cap_face_indices: vec![5],
        end_cap_face_indices: vec![6],
        ..Default::default()
    };

    let mut snapshots: Vec<Vec<Option<TopologyAttribute>>> = Vec::with_capacity(2);
    let mut feature_ids_per_build: Vec<FeatureId> = Vec::with_capacity(2);
    for build_idx in 0..2 {
        let kernel = HistoryMockKernel::new(
            profile_faces.clone(),
            profile_edges.clone(),
            result_faces.clone(),
            result_edges.clone(),
        )
        .with_extrude_history(history.clone());
        let mut engine = reify_eval::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let _result = engine.build(&module, ExportFormat::Step);

        let table = engine.topology_attribute_table();
        assert_eq!(
            table.len(),
            4,
            "build {build_idx}: expected 4 entries (2 caps + 2 sides), got {}",
            table.len()
        );

        // Cap (Top) — start_cap_face_indices[0] = 5.
        let top = table
            .lookup(result_faces[5])
            .expect("Cap(Top) entry at result_faces[5] missing");
        assert_eq!(top.role, Role::Cap(CapKind::Top));
        assert_eq!(top.local_index, 0);
        assert!(top.user_label.is_none());
        assert!(top.mod_history.is_empty());

        // Cap (Bottom) — end_cap_face_indices[0] = 6.
        let bottom = table
            .lookup(result_faces[6])
            .expect("Cap(Bottom) entry at result_faces[6] missing");
        assert_eq!(bottom.role, Role::Cap(CapKind::Bottom));
        assert_eq!(bottom.local_index, 0);

        // Side faces — face_generated entries with sequential local_index.
        let side_a = table
            .lookup(result_faces[7])
            .expect("Side entry at result_faces[7] missing");
        assert_eq!(side_a.role, Role::Side);
        assert_eq!(side_a.local_index, 0);
        let side_b = table
            .lookup(result_faces[8])
            .expect("Side entry at result_faces[8] missing");
        assert_eq!(side_b.role, Role::Side);
        assert_eq!(side_b.local_index, 1);

        assert_eq!(
            top.feature_id, bottom.feature_id,
            "Cap(Top) and Cap(Bottom) must share a FeatureId",
        );
        assert_eq!(
            top.feature_id, side_a.feature_id,
            "All four extrude entries must share a FeatureId",
        );
        feature_ids_per_build.push(top.feature_id.clone());

        snapshots.push(collect_attrs_at(&engine, &result_faces, &[5, 6, 7, 8]));
    }

    // Stability invariant: same (role, local_index, feature_id, user_label,
    // mod_history) tuples on every build. Mock kernel returns the same
    // result_faces handle ids from extract_*, and the engine resets the
    // table at build start, so a second build re-emits identical entries.
    assert_eq!(
        snapshots[0], snapshots[1],
        "selector triples must be invariant across rebuilds — extrude\nbuild 0: {:#?}\nbuild 1: {:#?}",
        snapshots[0], snapshots[1],
    );
    assert_eq!(
        feature_ids_per_build[0], feature_ids_per_build[1],
        "FeatureId must be invariant across rebuilds for the same realization",
    );
}

// ─── Test 2: partial revolve with mock history ────────────────────────────────

/// Per step-15 (2): synthetic revolve history with start_cap=[2], end_cap=[3],
/// face_generated=[(0,0,4),(0,1,5),(0,2,6),(0,3,7)] over a result of 8 faces.
/// Partial revolutions retain both cap faces.
#[test]
fn engine_build_partial_revolve_populates_cap_start_end_and_revolved_face() {
    let module = revolve_module();
    let result_faces: Vec<GeometryHandleId> = (7000..7008).map(GeometryHandleId).collect();
    let result_edges: Vec<GeometryHandleId> = (8000..8016).map(GeometryHandleId).collect();
    let profile_faces = vec![GeometryHandleId(7050)];
    let profile_edges: Vec<GeometryHandleId> = (7060..7064).map(GeometryHandleId).collect();
    let history = SweepOpHistoryRecords {
        face_generated: vec![
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 4,
            },
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 1,
                result_subshape_index: 5,
            },
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 2,
                result_subshape_index: 6,
            },
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 3,
                result_subshape_index: 7,
            },
        ],
        start_cap_face_indices: vec![2],
        end_cap_face_indices: vec![3],
        ..Default::default()
    };

    let mut snapshots: Vec<Vec<Option<TopologyAttribute>>> = Vec::with_capacity(2);
    for build_idx in 0..2 {
        let kernel = HistoryMockKernel::new(
            profile_faces.clone(),
            profile_edges.clone(),
            result_faces.clone(),
            result_edges.clone(),
        )
        .with_revolve_history(history.clone());
        let mut engine = reify_eval::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let _result = engine.build(&module, ExportFormat::Step);

        let table = engine.topology_attribute_table();
        assert_eq!(
            table.len(),
            6,
            "build {build_idx}: expected 6 entries (2 caps + 4 revolved faces), got {}",
            table.len()
        );

        // Cap (Start) — start_cap_face_indices[0] = 2.
        let start = table
            .lookup(result_faces[2])
            .expect("Cap(Start) entry at result_faces[2] missing");
        assert_eq!(start.role, Role::Cap(CapKind::Start));
        assert_eq!(start.local_index, 0);
        // Cap (End) — end_cap_face_indices[0] = 3.
        let end = table
            .lookup(result_faces[3])
            .expect("Cap(End) entry at result_faces[3] missing");
        assert_eq!(end.role, Role::Cap(CapKind::End));
        assert_eq!(end.local_index, 0);

        for (sequential_idx, result_face_idx) in [4_usize, 5, 6, 7].iter().enumerate() {
            let attr = table
                .lookup(result_faces[*result_face_idx])
                .unwrap_or_else(|| {
                    panic!(
                        "RevolvedFace entry at result_faces[{result_face_idx}] missing\
                         (sequential idx {sequential_idx})"
                    )
                });
            assert_eq!(
                attr.role,
                Role::RevolvedFace,
                "revolve face_generated must use Role::RevolvedFace"
            );
            assert_eq!(attr.local_index, sequential_idx as u32);
        }

        snapshots.push(collect_attrs_at(
            &engine,
            &result_faces,
            &[2, 3, 4, 5, 6, 7],
        ));
    }

    assert_eq!(
        snapshots[0], snapshots[1],
        "selector triples must be invariant across rebuilds — partial revolve",
    );
}

// ─── Test 3: full revolve with mock history ──────────────────────────────────

/// Per step-15 (3): synthetic full-2π revolve history with empty cap lists,
/// face_generated=[(0,0,0),(0,1,1)]. Full-2π revolutions emit no cap entries
/// because `FirstShape()` and `LastShape()` reference the same closed
/// surface; only the revolved lateral faces survive.
#[test]
fn engine_build_full_revolve_populates_only_revolved_face_no_caps() {
    let module = revolve_module();
    let result_faces: Vec<GeometryHandleId> = (9000..9008).map(GeometryHandleId).collect();
    let result_edges: Vec<GeometryHandleId> = (10000..10016).map(GeometryHandleId).collect();
    let profile_faces = vec![GeometryHandleId(9050)];
    let profile_edges: Vec<GeometryHandleId> = (9060..9064).map(GeometryHandleId).collect();
    let history = SweepOpHistoryRecords {
        face_generated: vec![
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 0,
            },
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 1,
                result_subshape_index: 1,
            },
        ],
        start_cap_face_indices: vec![],
        end_cap_face_indices: vec![],
        ..Default::default()
    };

    let mut snapshots: Vec<Vec<Option<TopologyAttribute>>> = Vec::with_capacity(2);
    for build_idx in 0..2 {
        let kernel = HistoryMockKernel::new(
            profile_faces.clone(),
            profile_edges.clone(),
            result_faces.clone(),
            result_edges.clone(),
        )
        .with_revolve_history(history.clone());
        let mut engine = reify_eval::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let _result = engine.build(&module, ExportFormat::Step);

        let table = engine.topology_attribute_table();
        assert_eq!(
            table.len(),
            2,
            "build {build_idx}: full-2π revolve has no caps — only 2 RevolvedFace entries; got {}",
            table.len(),
        );
        // No Cap entries at any index.
        for idx in [2_usize, 3] {
            assert!(
                table.lookup(result_faces[idx]).is_none(),
                "full-2π revolve must not emit Cap entries; result_faces[{idx}] should be unkeyed",
            );
        }
        // Two RevolvedFace entries with sequential local_index.
        for (sequential_idx, result_face_idx) in [0_usize, 1].iter().enumerate() {
            let attr = table
                .lookup(result_faces[*result_face_idx])
                .expect("RevolvedFace entry missing");
            assert_eq!(attr.role, Role::RevolvedFace);
            assert_eq!(attr.local_index, sequential_idx as u32);
        }

        snapshots.push(collect_attrs_at(&engine, &result_faces, &[0, 1, 2, 3]));
    }

    assert_eq!(
        snapshots[0], snapshots[1],
        "selector triples must be invariant across rebuilds — full revolve",
    );
}
