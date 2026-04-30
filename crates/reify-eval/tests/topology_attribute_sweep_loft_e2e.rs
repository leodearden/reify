//! End-to-end tests for v0.2 persistent-naming-v2 attribute auto-population
//! during sweep-style ops with custom OCCT history mappers — task 5b (#2619).
//!
//! Mirrors `topology_attribute_extrude_revolve_e2e.rs` (5a) but for the two
//! ops that don't use `BRepBuilderAPI_MakeShape::Modified`/`Generated` directly:
//!   - `GeometryOp::Sweep` → `BRepOffsetAPI_MakePipe` (single-parent — reuses
//!     `SweepOpHistoryRecords` and `populate_sweep_attributes`).
//!   - `GeometryOp::Loft` → `BRepOffsetAPI_ThruSections` (multi-parent — uses
//!     the new `LoftOpHistoryRecords` and `populate_loft_attributes`).
//!
//! Source-language constructors only expose `box`/`cylinder`/`sphere`/`tube`
//! at the source layer; we therefore cannot drive a real OCCT sweep/loft
//! through `Engine::build` end-to-end. Instead, this file pairs a
//! **synthesised `CompiledModule`** (built via the same
//! `CompiledModuleBuilder` / `TopologyTemplateBuilder` pattern) with a
//! **mock `GeometryKernel` that injects synthetic
//! `AttributeHistory::Sweep` / `AttributeHistory::Loft` records**.
//!
//! The kernel-direct integration tests (`sweep_with_history_integration.rs`,
//! `loft_with_history_integration.rs`) cover the FFI behaviour against
//! real OCCT; these e2e tests cover the engine-level wiring —
//! `Engine::execute_realization_ops`'s match-on-AttributeHistory →
//! `populate_sweep_attributes` / `populate_loft_attributes` →
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
use reify_test_support::*;
use reify_types::{
    AttributeHistory, CapKind, CompiledExpr, ExportFormat, FeatureId, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, HistoryRecord,
    LoftOpHistoryRecords, Mesh, ModulePath, QueryError, Role, SweepOpHistoryRecords, TessError,
    TopologyAttribute, Type, Value,
};

// ─── HistoryMockKernel ────────────────────────────────────────────────────────

/// Mock `GeometryKernel` that wraps `MockGeometryKernel` to:
///
/// 1. Override `execute_with_history` to inject synthetic
///    `AttributeHistory::Sweep` / `AttributeHistory::Loft` records
///    for matching `GeometryOp` variants. All other ops (curves, etc.)
///    fall through to `inner.execute(op)` and return
///    `AttributeHistory::None`. Each non-Sweep/Loft handle is recorded
///    in `profile_handle_order` so `extract_*` can return *per-section*
///    slices for the loft multi-parent test (otherwise the test cannot
///    distinguish a section-mixup bug — see reviewer suggestion #2).
///
/// 2. Override `extract_faces` / `extract_edges` to return:
///       - `result_faces` / `result_edges` when the queried handle
///         matches `last_sweep_result` (the most-recent Sweep/Loft
///         result),
///       - `profile_faces_per_section[i]` / `profile_edges_per_section[i]`
///         when the queried handle is the i-th profile handle allocated
///         (matched by position in `profile_handle_order`).  Falls back
///         to section 0 if the queried handle's allocation index is past
///         the end of the per-section slice list (covers the sweep test,
///         which allocates a path handle that is not extracted but still
///         passes through `execute_with_history`).
///
/// Other trait methods (`query`, `export`, `tessellate`) delegate to
/// `inner` unchanged.
struct HistoryMockKernel {
    inner: MockGeometryKernel,
    /// Per-section profile face slice family.  Each entry is the
    /// `extract_faces` result for the corresponding profile handle.
    /// The Nth profile handle (in `execute_with_history` allocation
    /// order) maps to `profile_faces_per_section[N]`; out-of-range
    /// indices fall back to section 0.
    profile_faces_per_section: Vec<Vec<GeometryHandleId>>,
    /// Per-section profile edge slice family — see
    /// [`profile_faces_per_section`] for indexing semantics.
    profile_edges_per_section: Vec<Vec<GeometryHandleId>>,
    result_faces: Vec<GeometryHandleId>,
    result_edges: Vec<GeometryHandleId>,
    sweep_history: Option<SweepOpHistoryRecords>,
    loft_history: Option<LoftOpHistoryRecords>,
    /// Allocation-order list of profile handle ids.  Updated by
    /// `execute_with_history` for every non-Sweep/Loft op.  The Nth
    /// handle's `extract_faces`/`extract_edges` returns the Nth section
    /// slice from `profile_*_per_section`.  `Arc<Mutex<...>>` so the
    /// wrapper is `Send + Sync` even though the trait methods need
    /// interior mutability.
    profile_handle_order: Arc<Mutex<Vec<GeometryHandleId>>>,
    /// Set whenever `execute_with_history` runs a Sweep or Loft op.
    /// Used by `extract_faces` / `extract_edges` to return result-vs-profile
    /// slices without depending on the inner `next_id` allocation order.
    last_sweep_result: Arc<Mutex<Option<GeometryHandleId>>>,
}

impl HistoryMockKernel {
    fn new(
        profile_faces_per_section: Vec<Vec<GeometryHandleId>>,
        profile_edges_per_section: Vec<Vec<GeometryHandleId>>,
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    ) -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            profile_faces_per_section,
            profile_edges_per_section,
            result_faces,
            result_edges,
            sweep_history: None,
            loft_history: None,
            profile_handle_order: Arc::new(Mutex::new(Vec::new())),
            last_sweep_result: Arc::new(Mutex::new(None)),
        }
    }

    fn with_sweep_history(mut self, history: SweepOpHistoryRecords) -> Self {
        self.sweep_history = Some(history);
        self
    }

    fn with_loft_history(mut self, history: LoftOpHistoryRecords) -> Self {
        self.loft_history = Some(history);
        self
    }

    /// Look up the section index for the given handle in
    /// `profile_handle_order`.  Returns `None` if the handle hasn't
    /// been allocated through this wrapper.
    fn section_index_for(&self, handle: GeometryHandleId) -> Option<usize> {
        self.profile_handle_order
            .lock()
            .unwrap()
            .iter()
            .position(|h| *h == handle)
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
            GeometryOp::Sweep { .. } => {
                *self.last_sweep_result.lock().unwrap() = Some(handle.id);
                self.sweep_history
                    .clone()
                    .map_or(AttributeHistory::None, AttributeHistory::Sweep)
            }
            GeometryOp::Loft { .. } => {
                *self.last_sweep_result.lock().unwrap() = Some(handle.id);
                self.loft_history
                    .clone()
                    .map_or(AttributeHistory::None, AttributeHistory::Loft)
            }
            _ => {
                // Track non-sweep/loft handles in allocation order so
                // `extract_*` can return per-section slices for the
                // multi-parent loft test.
                self.profile_handle_order.lock().unwrap().push(handle.id);
                AttributeHistory::None
            }
        };
        Ok((handle, history))
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if Some(handle) == *self.last_sweep_result.lock().unwrap() {
            return Ok(self.result_faces.clone());
        }
        let section_idx = self.section_index_for(handle).unwrap_or(0);
        // Fall back to section 0 if the per-section slice list is
        // shorter than the allocation order (e.g. the sweep test
        // allocates 2 profile handles but configures only 1 section
        // slice — the path handle is not extracted, so the fallback
        // is never observed in practice but preserves a defined
        // return for any unexpected query).
        let slice = self
            .profile_faces_per_section
            .get(section_idx)
            .or_else(|| self.profile_faces_per_section.first())
            .cloned()
            .unwrap_or_default();
        Ok(slice)
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if Some(handle) == *self.last_sweep_result.lock().unwrap() {
            return Ok(self.result_edges.clone());
        }
        let section_idx = self.section_index_for(handle).unwrap_or(0);
        let slice = self
            .profile_edges_per_section
            .get(section_idx)
            .or_else(|| self.profile_edges_per_section.first())
            .cloned()
            .unwrap_or_default();
        Ok(slice)
    }

    fn query(&self, q: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_types::ExportError> {
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

/// Build a curve op whose corresponding handle in the mock kernel acts as
/// a stand-in profile/path. Curves are not seeded by
/// `seed_primitive_attributes_for_handle`, so the only entries the table
/// gets are the injected sweep/loft history entries — clean assertions.
fn line_segment_curve_op() -> CompiledGeometryOp {
    CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(10.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(0.0)),
        ],
    }
}

/// Build a synthesised `CompiledModule` for sweep:
/// (0) profile curve, (1) path curve, (2) Sweep referencing Step(0)+Step(1).
fn sweep_module() -> reify_compiler::CompiledModule {
    let profile_op = line_segment_curve_op();
    let path_op = line_segment_curve_op();
    let sweep_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Sweep,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![],
    };
    let template = TopologyTemplateBuilder::new("TestSweep")
        .realization("TestSweep", 0, vec![profile_op, path_op, sweep_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_sweep_loft_e2e"))
        .template(template)
        .build()
}

/// Build a synthesised `CompiledModule` for loft (2 sections):
/// (0) profile_1 curve, (1) profile_2 curve, (2) Loft referencing both.
fn loft_module() -> reify_compiler::CompiledModule {
    let profile_op_0 = line_segment_curve_op();
    let profile_op_1 = line_segment_curve_op();
    let loft_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Loft,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![],
    };
    let template = TopologyTemplateBuilder::new("TestLoft")
        .realization("TestLoft", 0, vec![profile_op_0, profile_op_1, loft_op])
        .build();
    CompiledModuleBuilder::new(ModulePath::single("test_sweep_loft_e2e"))
        .template(template)
        .build()
}

/// Collect the projection of every entry in `engine.topology_attribute_table()`
/// keyed by `result_face_handles[idx]` for each `idx in indices`. Returns
/// `None` for any index whose handle has no entry — the caller asserts
/// `Some` for the expected ones and `None` for the deliberately-unkeyed ones.
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

// ─── Test 1: sweep with mock history ────────────────────────────────────────

/// Synthetic sweep history with start_cap=[2], end_cap=[3],
/// face_generated=[(0,0,4),(0,1,5)] over a result of 8 faces. Asserts the
/// resulting `topology_attribute_table` has exactly 4 entries (2 caps + 2
/// SweptFace) keyed by the configured result-face ids; verifies stability
/// by running `Engine::build` twice and asserting equal entries on both
/// builds.
#[test]
fn engine_build_sweep_with_mock_history_populates_table_with_cap_and_swept_face_entries() {
    let module = sweep_module();
    let result_faces: Vec<GeometryHandleId> = (5000..5008).map(GeometryHandleId).collect();
    let result_edges: Vec<GeometryHandleId> = (6000..6016).map(GeometryHandleId).collect();
    // Sweep is single-parent so a single profile-section slice family
    // covers the populate_sweep_attributes contract.  Profile section 0
    // has 1 face and 4 edges (≥ 2 so face_generated[1].parent_subshape_index=1
    // is in range under the populate_sweep_attributes defense-in-depth
    // check).
    let profile_faces_per_section = vec![vec![GeometryHandleId(5050)]];
    let profile_edges_per_section: Vec<Vec<GeometryHandleId>> =
        vec![(5060..5064).map(GeometryHandleId).collect()];
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
        ],
        start_cap_face_indices: vec![2],
        end_cap_face_indices: vec![3],
        ..Default::default()
    };

    let mut snapshots: Vec<Vec<Option<TopologyAttribute>>> = Vec::with_capacity(2);
    let mut feature_ids_per_build: Vec<FeatureId> = Vec::with_capacity(2);
    for build_idx in 0..2 {
        let kernel = HistoryMockKernel::new(
            profile_faces_per_section.clone(),
            profile_edges_per_section.clone(),
            result_faces.clone(),
            result_edges.clone(),
        )
        .with_sweep_history(history.clone());
        let mut engine = reify_eval::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let _result = engine.build(&module, ExportFormat::Step);

        let table = engine.topology_attribute_table();
        assert_eq!(
            table.len(),
            4,
            "build {build_idx}: expected 4 entries (2 caps + 2 SweptFace), got {}",
            table.len()
        );

        // Cap (Start) — start_cap_face_indices[0] = 2 (sweep parametric Start/End,
        // NOT extrude's Top/Bottom).
        let start = table
            .lookup(result_faces[2])
            .expect("Cap(Start) entry at result_faces[2] missing");
        assert_eq!(start.role, Role::Cap(CapKind::Start));
        assert_eq!(start.local_index, 0);
        assert!(start.user_label.is_none());
        assert!(start.mod_history.is_empty());

        // Cap (End) — end_cap_face_indices[0] = 3.
        let end = table
            .lookup(result_faces[3])
            .expect("Cap(End) entry at result_faces[3] missing");
        assert_eq!(end.role, Role::Cap(CapKind::End));
        assert_eq!(end.local_index, 0);

        // SweptFace entries — face_generated entries with sequential local_index.
        let swept_a = table
            .lookup(result_faces[4])
            .expect("SweptFace entry at result_faces[4] missing");
        assert_eq!(
            swept_a.role,
            Role::SweptFace,
            "sweep face_generated must use Role::SweptFace not Role::Side",
        );
        assert_eq!(swept_a.local_index, 0);
        let swept_b = table
            .lookup(result_faces[5])
            .expect("SweptFace entry at result_faces[5] missing");
        assert_eq!(swept_b.role, Role::SweptFace);
        assert_eq!(swept_b.local_index, 1);

        assert_eq!(
            start.feature_id, end.feature_id,
            "Cap(Start) and Cap(End) must share a FeatureId",
        );
        assert_eq!(
            start.feature_id, swept_a.feature_id,
            "All four sweep entries must share a FeatureId",
        );
        feature_ids_per_build.push(start.feature_id.clone());

        snapshots.push(collect_attrs_at(&engine, &result_faces, &[2, 3, 4, 5]));
    }

    // Stability invariant: same (role, local_index, feature_id, user_label,
    // mod_history) tuples on every build.
    assert_eq!(
        snapshots[0], snapshots[1],
        "selector triples must be invariant across rebuilds — sweep\nbuild 0: {:#?}\nbuild 1: {:#?}",
        snapshots[0], snapshots[1],
    );
    assert_eq!(
        feature_ids_per_build[0], feature_ids_per_build[1],
        "FeatureId must be invariant across rebuilds for the same realization",
    );
}

// ─── Test 2: loft with mock history ──────────────────────────────────────────

/// Synthetic loft history with start_cap=[2], end_cap=[3],
/// face_generated=[(0,0,4),(0,1,5),(1,0,6),(1,2,7)] over a result of 8
/// faces. Asserts the resulting `topology_attribute_table` has exactly 6
/// entries (2 caps + 4 LoftedFace) keyed by the configured result-face
/// ids; verifies stability by running `Engine::build` twice. Loft is
/// multi-parent: parent_index 0 and 1 reference different sections, but
/// `local_index` increments sequentially across all sections (0,1,2,3
/// across both sections — NOT 0,1,0,1 per section).
///
/// Section sizes are deliberately asymmetric (section 0 has 2 edges,
/// section 1 has 3 edges) and the last face_generated record uses
/// `parent_subshape_index = 2`, which is in range only for section 1's
/// 3-edge slice.  If `populate_loft_op` regressed to extracting only
/// section 0 (or always extracting from a single profile handle), the
/// (1, 2, 7) record would fail the populate_loft_attributes
/// defense-in-depth check and surface as a Diagnostic::warning →
/// fewer-than-6 entries → test fails.  This catches the
/// section-mixup bug class that a uniform-size fixture cannot
/// distinguish (reviewer suggestion #2).
#[test]
fn engine_build_loft_with_mock_history_populates_table_with_cap_and_lofted_face_entries() {
    let module = loft_module();
    let result_faces: Vec<GeometryHandleId> = (7000..7008).map(GeometryHandleId).collect();
    let result_edges: Vec<GeometryHandleId> = (8000..8016).map(GeometryHandleId).collect();
    // Distinct per-section profile slices.  Asymmetric sizes ensure
    // that a "always section 0" or "single-handle bug" in
    // `populate_loft_op` would leave section 1's 3-edge slice replaced
    // by section 0's 2-edge slice, making the (1, 2, ...) record below
    // out of range → catches the bug.
    let profile_faces_per_section = vec![
        vec![GeometryHandleId(7050)],
        vec![GeometryHandleId(7150)],
    ];
    let profile_edges_per_section: Vec<Vec<GeometryHandleId>> = vec![
        // section 0: 2 edges
        vec![GeometryHandleId(7060), GeometryHandleId(7061)],
        // section 1: 3 edges (one more than section 0)
        vec![
            GeometryHandleId(7160),
            GeometryHandleId(7161),
            GeometryHandleId(7162),
        ],
    ];
    let history = LoftOpHistoryRecords {
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
                parent_index: 1,
                parent_subshape_index: 0,
                result_subshape_index: 6,
            },
            // parent_subshape_index=2 is valid only for section 1's
            // 3-edge slice; if the helper got section 0's 2-edge slice
            // for parent_index=1, this record would error.
            HistoryRecord {
                parent_index: 1,
                parent_subshape_index: 2,
                result_subshape_index: 7,
            },
        ],
        start_cap_face_indices: vec![2],
        end_cap_face_indices: vec![3],
        ..Default::default()
    };

    let mut snapshots: Vec<Vec<Option<TopologyAttribute>>> = Vec::with_capacity(2);
    let mut feature_ids_per_build: Vec<FeatureId> = Vec::with_capacity(2);
    for build_idx in 0..2 {
        let kernel = HistoryMockKernel::new(
            profile_faces_per_section.clone(),
            profile_edges_per_section.clone(),
            result_faces.clone(),
            result_edges.clone(),
        )
        .with_loft_history(history.clone());
        let mut engine = reify_eval::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let _result = engine.build(&module, ExportFormat::Step);

        let table = engine.topology_attribute_table();
        assert_eq!(
            table.len(),
            6,
            "build {build_idx}: expected 6 entries (2 caps + 4 LoftedFace), got {}",
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

        // LoftedFace entries — face_generated entries with sequential
        // local_index (0,1,2,3 across all sections, NOT 0,1,0,1 per section).
        for (sequential_idx, result_face_idx) in [4_usize, 5, 6, 7].iter().enumerate() {
            let attr = table
                .lookup(result_faces[*result_face_idx])
                .unwrap_or_else(|| {
                    panic!(
                        "LoftedFace entry at result_faces[{result_face_idx}] missing\
                         (sequential idx {sequential_idx})"
                    )
                });
            assert_eq!(
                attr.role,
                Role::LoftedFace,
                "loft face_generated must use Role::LoftedFace, NOT Role::Side / SweptFace / RevolvedFace",
            );
            assert_eq!(attr.local_index, sequential_idx as u32);
        }

        assert_eq!(
            start.feature_id, end.feature_id,
            "Cap(Start) and Cap(End) must share a FeatureId",
        );
        feature_ids_per_build.push(start.feature_id.clone());

        snapshots.push(collect_attrs_at(
            &engine,
            &result_faces,
            &[2, 3, 4, 5, 6, 7],
        ));
    }

    assert_eq!(
        snapshots[0], snapshots[1],
        "selector triples must be invariant across rebuilds — loft\nbuild 0: {:#?}\nbuild 1: {:#?}",
        snapshots[0], snapshots[1],
    );
    assert_eq!(
        feature_ids_per_build[0], feature_ids_per_build[1],
        "FeatureId must be invariant across rebuilds for the same realization",
    );
}
