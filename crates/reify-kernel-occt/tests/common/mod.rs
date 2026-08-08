//! Shared helpers for the `harness_occt` integration tests: local-feature
//! (fillet/chamfer) assertions, bounding-box query parsing, and JSON-Point3
//! (xyz) query parsing.
//!
//! The local-feature assertions were extracted from
//! `fillet_with_history_integration.rs` and
//! `chamfer_with_history_integration.rs` to eliminate byte-for-byte duplication
//! of the edge-buffer assertion blocks (h)–(l). Future history-record additions
//! (e.g. result_subshape_index sentinels) require only a single edit here rather
//! than dual edits with drift risk.
//!
//! Block (g) mirrors the silent_drop_count invariant from
//! `boolean_op_history_integration.rs` (parity, not extraction).
//!
//! The bounding-box half ([`BBox`], [`parse_bbox`]) consolidates seven
//! hand-rolled `GeometryQuery::BoundingBox` JSON parsers that had accumulated
//! across `tests/harness_occt/` — a duplication finding filed during task
//! 5377's reviewer-amendment pass and resolved by task 5893. The wire-format
//! contract, and why the parser is deliberately not `serde_json`-based, are
//! stated ONCE on [`parse_bbox`]; do not restate either here.
//!
//! The JSON-Point3 half ([`Xyz`], [`parse_xyz`], [`xyz_of`]) does the same for
//! the `{"x":_,"y":_,"z":_}` format shared by the Centroid / EdgeTangent /
//! FaceNormal / FaceNormalAt / ClosestPointOnShape queries — a duplication
//! finding filed during task 5893's review and resolved by task 5937. Its
//! wire-format and strictness contracts are likewise stated ONCE, on
//! [`parse_xyz`].

#![cfg(has_occt)]

use reify_kernel_occt::{
    DeletedRecord, HistoryRecord, LocalFeatureOpHistoryRecords, OcctKernelHandle,
};
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Private trait implemented by both [`HistoryRecord`] and [`DeletedRecord`]
/// so that [`assert_records_in_range`] can operate on slices of either type.
/// Both types carry `parent_index: u8` and `parent_subshape_index: u32`;
/// only `HistoryRecord` additionally carries `result_subshape_index`.
trait ParentBounded {
    fn parent_index(&self) -> u8;
    fn parent_subshape_index(&self) -> u32;
}

impl ParentBounded for HistoryRecord {
    fn parent_index(&self) -> u8 {
        self.parent_index
    }
    fn parent_subshape_index(&self) -> u32 {
        self.parent_subshape_index
    }
}

impl ParentBounded for DeletedRecord {
    fn parent_index(&self) -> u8 {
        self.parent_index
    }
    fn parent_subshape_index(&self) -> u32 {
        self.parent_subshape_index
    }
}

/// Assert that every record in `records` has `parent_index == 0` and
/// `parent_subshape_index < max_psi`.
///
/// `field` is included verbatim in every failure message (e.g. `"edge_modified"`,
/// `"edge_deleted"`), and `op_name` identifies the operation (e.g. `"fillet"`).
/// `range_explanation` is appended to the out-of-range panic message to provide
/// triage context (e.g. `"12-edge box"` or
/// `"8-vertex box; edge_generated is keyed by parent VERTEX"`).
fn assert_records_in_range<R: ParentBounded>(
    records: &[R],
    max_psi: u32,
    op_name: &str,
    field: &str,
    range_explanation: &str,
) {
    for r in records {
        assert_eq!(
            r.parent_index(),
            0,
            "{op_name} {field} records always have parent_index=0, got {}",
            r.parent_index()
        );
        assert!(
            r.parent_subshape_index() < max_psi,
            "{op_name} {field} parent_subshape_index {} out of range \
             (expected < {max_psi}; {range_explanation})",
            r.parent_subshape_index()
        );
    }
}

/// Assert well-formedness of all edge-related history buffers produced by a
/// local-feature operation (fillet or chamfer) on a 10 mm box.
///
/// Covers assertion blocks (g)–(l) from the integration-test spec:
///
/// - **(g)** `silent_drop_count == 0`: every Modified/Generated child must be
///   resolvable in the result map. Mirrors the same invariant in
///   `boolean_op_history_integration.rs`.
/// - **(g2)** `face_generated` per-edge coverage: the set of distinct
///   `parent_subshape_index` values in `face_generated` must equal 12, one per
///   parent edge of the 10mm cube. Uses `HashSet` deduplication so the check is
///   independent of OCCT's per-edge face count (esc-2655-26 suggestion #1 /
///   task 2821 amendment). Each record additionally satisfies the same
///   per-record bounds checked in blocks (i)/(j)/(l): `parent_index == 0`
///   and `parent_subshape_index < 12`.
/// - **(h)** Extracts `result_edge_count` via `kernel.extract_edges(result_id)`.
/// - **(i)** `edge_modified` per-record well-formedness: `parent_index == 0`,
///   `parent_subshape_index < 12` (box edges), `result_subshape_index < result_edge_count`.
///   No non-empty assertion — OCCT may route parent edges through Generated/Deleted.
/// - **(j)** `edge_generated` per-record well-formedness: `parent_index == 0`,
///   `parent_subshape_index < 8` (box **vertices** — `edge_generated` is keyed by
///   the parent VERTEX map, not the edge map), `result_subshape_index < result_edge_count`.
/// - **(k)** `face_deleted.is_empty()`: the operation does not consume any parent face.
/// - **(l)** `!edge_deleted.is_empty()`: OCCT marks all 12 parent edges as `IsDeleted()`
///   (they are subsumed by the generated fillet/chamfer surfaces). Per-record bounds:
///   `parent_index == 0`, `parent_subshape_index < 12`.
///
/// `op_name` is included in every failure message (e.g. `"fillet"` or `"chamfer"`).
///
/// # Panics
///
/// Panics with a descriptive message if any assertion fails.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn assert_local_feature_history_well_formed(
    kernel: &OcctKernelHandle,
    result_id: GeometryHandleId,
    history: &LocalFeatureOpHistoryRecords,
    op_name: &str,
) {
    // (g) silent_drop_count must be zero for a well-formed clean local-feature op:
    //     every Modified/Generated child must be resolvable in the result map.
    //     Mirrors the same invariant in boolean_op_history_integration.rs.
    assert_eq!(
        history.silent_drop_count, 0,
        "{op_name} should not silently drop any history record on a clean 10mm-box op; \
         got {} drops",
        history.silent_drop_count
    );

    // (g2) face_generated per-edge coverage (esc-2655-26 suggestion #1 / task 2821 amendment).
    //
    // Collect the distinct parent_subshape_index values from face_generated.  For a
    // clean 10mm cube + fillet/chamfer every one of the 12 parent edges must produce
    // at least one generated face, so the HashSet size must equal 12.
    //
    // Using a HashSet rather than `len()` or `saturating_sub` arithmetic makes the
    // check independent of OCCT's per-edge face count: it tests the actual claim
    // ("all 12 edges are covered") without relying on the specific decomposition
    // (6 modified + 8 corner + 12 lateral) that the prior formula encoded.
    let generated_edge_parents: std::collections::HashSet<u32> = history
        .face_generated
        .iter()
        .map(|r| r.parent_subshape_index)
        .collect();
    assert_eq!(
        generated_edge_parents.len(),
        12,
        "{op_name} face_generated must cover all 12 parent edges of the cube; \
         got {} distinct parent_subshape_index values (face_generated.len()={})",
        generated_edge_parents.len(),
        history.face_generated.len()
    );
    assert_records_in_range(
        &history.face_generated,
        12,
        op_name,
        "face_generated",
        "12-edge box",
    );

    // (h) Derive result_edge_count for index-bounds checks.
    let result_edges = kernel
        .extract_edges(result_id)
        .expect("extract_edges on the local-feature result should succeed");
    let result_edge_count = result_edges.len() as u32;

    // (i) edge_modified per-record well-formedness.
    // No non-empty assertion: for a fully-filleted/chamfered box, OCCT may route
    // parent edges through Generated() or IsDeleted() rather than Modified()
    // (see plan design decision).
    assert_records_in_range(
        &history.edge_modified,
        12,
        op_name,
        "edge_modified",
        "12-edge box",
    );
    for r in &history.edge_modified {
        assert!(
            r.result_subshape_index < result_edge_count,
            "{op_name} edge_modified result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (j) edge_generated per-record well-formedness.
    // parent_subshape_index is into the VERTEX map (box has 8 vertices), not
    // the edge map, because edge_generated is populated via
    // emit_sweep_generated_cross_type(shape_vertex_map, result_edge_map, TopAbs_EDGE).
    assert_records_in_range(
        &history.edge_generated,
        8,
        op_name,
        "edge_generated",
        "8-vertex box; edge_generated is keyed by parent VERTEX",
    );
    for r in &history.edge_generated {
        assert!(
            r.result_subshape_index < result_edge_count,
            "{op_name} edge_generated result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (k) face_deleted must be empty for a clean local-feature op on a convex box.
    assert!(
        history.face_deleted.is_empty(),
        "clean {op_name} must not delete any parent face; got {} face_deleted records",
        history.face_deleted.len()
    );

    // (l) edge_deleted must be non-empty: BRepFilletAPI_Make{Fillet,Chamfer} marks
    // all 12 parent edges as IsDeleted() because they are fully subsumed by the
    // generated fillet/chamfer surfaces. A regression that stops emitting
    // edge_deleted records (e.g. a broken IsDeleted() walk or index-map mismatch)
    // would produce an empty vec — caught here.
    assert!(
        !history.edge_deleted.is_empty(),
        "{op_name} edge_deleted must be non-empty: OCCT marks all parent edges IsDeleted(); \
         got 0 records (regression in edge-deleted emit loop?)"
    );
    assert_records_in_range(
        &history.edge_deleted,
        12,
        op_name,
        "edge_deleted",
        "12-edge box",
    );
}

/// Run the full face-records integration test body for a local-feature operation
/// (fillet or chamfer) on a freshly built 10 mm box.
///
/// Covers assertions (a)–(g) and then delegates (g2)/(h)–(l) to
/// `assert_local_feature_history_well_formed`.  Extracted so that
/// `fillet_with_history_reports_face_records` and
/// `chamfer_with_history_reports_face_records` share a single copy of the
/// ~90-line body rather than maintaining a line-for-line clone.
///
/// `param_m` is the fillet radius / chamfer distance in metres.
/// `op` is a closure that calls `kernel.fillet_with_history` or
/// `kernel.chamfer_with_history` with the given box id and parameter.
/// `op_name` is used in all failure messages (e.g. `"fillet"` or `"chamfer"`).
///
/// # Preconditions
///
/// `param_m` must be small relative to `BOX_SIDE_M` (i.e. `param_m <= BOX_SIDE_M * 0.1`,
/// or ≤ 1 mm on a 10 mm cube).  The 90%-of-original volume lower-bound assertion
/// (assertion block (a)) is only valid for small parameter values; a large radius or
/// distance would remove more than 10% of the box's material and cause a spurious
/// failure.
///
/// # Panics
///
/// Panics with a descriptive message if any assertion fails.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn run_local_feature_reports_face_records<F>(
    kernel: &OcctKernelHandle,
    param_m: f64,
    op: F,
    op_name: &str,
) where
    F: FnOnce(
        GeometryHandleId,
        f64,
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError>,
{
    const BOX_SIDE_M: f64 = 10.0e-3;

    assert!(
        param_m > 0.0,
        "precondition violated: param_m must be positive, got {param_m:.4e} m",
    );
    assert!(
        param_m <= BOX_SIDE_M * 0.1,
        "precondition violated: param_m ({param_m:.4e} m) must be ≤ {} m (10% of BOX_SIDE_M); \
         larger values make the 90%-volume assertion meaningless",
        BOX_SIDE_M * 0.1,
    );

    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_SIDE_M),
            height: Value::Real(BOX_SIDE_M),
            depth: Value::Real(BOX_SIDE_M),
        })
        .expect("box should build");

    let (result_id, history) = op(box_handle.id, param_m).unwrap_or_else(|e| {
        panic!("{op_name}_with_history({param_m:.4e} m) should succeed for a 10mm box: {e:?}")
    });

    // (a) Result volume is positive and strictly less than the original box.
    // Original box: 10mm × 10mm × 10mm = 1000 mm³ = 1.0e-6 m³.
    let orig_vol = BOX_SIDE_M * BOX_SIDE_M * BOX_SIDE_M;
    let vol = kernel
        .query(&GeometryQuery::Volume(result_id))
        .unwrap_or_else(|e| panic!("volume query on the {op_name} result should succeed: {e:?}"));
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "{op_name} result must have positive volume, got {vol_si}"
    );
    assert!(
        vol_si < orig_vol,
        "{op_name} volume must be strictly less than the original ({op_name} removes material): \
         got {vol_si}, original {orig_vol}"
    );
    // Allow up to 10% material removal; precondition: param_m <= BOX_SIDE_M * 0.1
    // (≤ 1 mm on a 10 mm cube) — see function-level precondition doc.
    assert!(
        vol_si >= 0.9 * orig_vol,
        "{op_name} volume should be at least 90% of original: got {vol_si}, original {orig_vol}"
    );

    // (b) face_modified non-empty: parent box faces are trimmed by the op.
    assert!(
        !history.face_modified.is_empty(),
        "{op_name} history.face_modified should be non-empty for a 10mm box — \
         got {} records",
        history.face_modified.len()
    );

    // (c) face_generated non-empty: each edge generates a curved/flat face.
    assert!(
        !history.face_generated.is_empty(),
        "{op_name} history.face_generated should be non-empty for a 10mm box — \
         got {} records",
        history.face_generated.len()
    );

    // (d) Every record has parent_index == 0 (single parent: the box).
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert_eq!(
            r.parent_index, 0,
            "{op_name} face records always have parent_index=0, got {}",
            r.parent_index
        );
    }

    // (e) face_modified.parent_subshape_index < 6 (box has exactly 6 faces).
    for r in &history.face_modified {
        assert!(
            r.parent_subshape_index < 6,
            "face_modified parent_subshape_index {} out of range for a 6-face box",
            r.parent_subshape_index
        );
    }

    // (f) face_generated.parent_subshape_index < 12 (lateral faces come from edges;
    //     a 10mm box has exactly 12 edges).
    for r in &history.face_generated {
        assert!(
            r.parent_subshape_index < 12,
            "face_generated parent_subshape_index {} out of range for a 12-edge box \
             (generated faces come from edges)",
            r.parent_subshape_index
        );
    }

    // (g) Every result_subshape_index is in-range for the result shape's face list.
    let result_faces = kernel
        .extract_faces(result_id)
        .unwrap_or_else(|e| panic!("extract_faces on the {op_name} result should succeed: {e:?}"));
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.result_subshape_index < result_face_count,
            "face record result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }

    // (g2)/(h)–(l) delegated to the shared helper.
    assert_local_feature_history_well_formed(kernel, result_id, &history, op_name);
}

/// Run the full non-solid-input rejection test for a local-feature operation.
///
/// Builds a 10 mm box, then calls `op` with a Face handle and an Edge handle
/// in turn, asserting that both are rejected via `assert_local_feature_rejects_non_solid_input`.
/// Extracted so that `fillet_with_history_rejects_non_solid_input` and
/// `chamfer_with_history_rejects_non_solid_input` share a single copy.
///
/// `param_m` is the fillet radius / chamfer distance in metres (passed to `op`).
/// `op_name` labels the operation in failure messages (e.g. `"fillet_with_history"`).
///
/// # Panics
///
/// Panics with a descriptive message if any assertion fails.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn run_local_feature_rejects_non_solid_input<F, T>(
    kernel: &OcctKernelHandle,
    param_m: f64,
    op: F,
    op_name: &str,
) where
    F: Fn(GeometryHandleId, f64) -> Result<T, GeometryError>,
{
    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("box should build");

    // (a) Reject BRepKind::Face input.
    let faces = kernel
        .extract_faces(box_handle.id)
        .expect("extract_faces should succeed on a solid box");
    assert!(
        !faces.is_empty(),
        "extract_faces should return at least one face for a 10mm box"
    );
    assert_local_feature_rejects_non_solid_input(op(faces[0], param_m), "BRepKind::Face", op_name);

    // (b) Reject BRepKind::Edge input.
    let edges = kernel
        .extract_edges(box_handle.id)
        .expect("extract_edges should succeed on a solid box");
    assert!(
        !edges.is_empty(),
        "extract_edges should return at least one edge for a 10mm box"
    );
    assert_local_feature_rejects_non_solid_input(op(edges[0], param_m), "BRepKind::Edge", op_name);
}

/// Assert that a local-feature op (`fillet_with_history` or `chamfer_with_history`)
/// rejects a non-`BRepKind::Solid` input handle with a descriptive
/// `GeometryError::OperationFailed` message.
///
/// Pass the raw `Result` returned by the op, the human-readable kind label
/// (e.g. `"BRepKind::Face"`, `"BRepKind::Edge"`), and the op name for failure
/// messages.
///
/// # Panics
///
/// Panics unless `result` is `Err(GeometryError::OperationFailed(msg))` where
/// `msg` contains `"Solid"` or `"BRepKind"`.
///
/// Used by `fillet_with_history_rejects_non_solid_input` and
/// `chamfer_with_history_rejects_non_solid_input` (esc-2655-26 suggestions #2/#5 /
/// task 2821 amendment) to eliminate byte-for-byte duplication and to exercise
/// multiple non-Solid kinds (Face and Edge) from a single test body.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn assert_local_feature_rejects_non_solid_input<T>(
    result: Result<T, GeometryError>,
    kind_label: &str,
    op_name: &str,
) {
    let err = match result {
        Err(e) => e,
        Ok(_) => {
            panic!("{op_name} should have rejected a {kind_label} input handle but returned Ok")
        }
    };
    match &err {
        GeometryError::OperationFailed(msg) => {
            assert!(
                msg.contains("Solid") || msg.contains("BRepKind"),
                "{op_name} OperationFailed message should mention 'Solid' or 'BRepKind' \
                 when rejecting a {kind_label} input: {msg}"
            );
        }
        other => panic!(
            "{op_name} expected GeometryError::OperationFailed for {kind_label} input, \
             got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Bounding-box JSON parsing (task 5893)
// ---------------------------------------------------------------------------

/// The six extents returned by `GeometryQuery::BoundingBox`, parsed from the
/// kernel's wire string by [`parse_bbox`].
///
/// Field names match the JSON keys, so a call site can destructure exactly the
/// axes it cares about: `let BBox { zmin, zmax, .. } = ...`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // fields only read from has_occt integration-test binaries
pub struct BBox {
    pub xmin: f64,
    pub ymin: f64,
    pub zmin: f64,
    pub xmax: f64,
    pub ymax: f64,
    pub zmax: f64,
}

/// Parse the bounding-box string returned by `GeometryQuery::BoundingBox`.
///
/// Wire format:
/// `{"xmin":<f>,"ymin":<f>,"zmin":<f>,"xmax":<f>,"ymax":<f>,"zmax":<f>}`
///
/// Keys parse with or without surrounding quotes, whitespace around `:` and `,`
/// is trimmed, and unrecognised keys are ignored. A repeated key takes
/// last-wins; empty or whitespace-only input recognises no key at all and so
/// takes the missing-field panic below.
///
/// **Deliberately NOT `serde_json`-based**, even though `serde_json` is already
/// a dev-dependency of this crate. This is the canonical statement of that
/// decision — every other mention in this module points here. The producer (the
/// `GeometryQuery::BoundingBox` arm of `OcctKernel::query` in `src/lib.rs`)
/// builds this string with a single `format!` that passes each component
/// through `f64` **Display**, which is not a JSON number encoder: a non-finite
/// extent is emitted as the bare token `inf` / `-inf` / `NaN`, which strict JSON
/// forbids and `serde_json::from_str` rejects outright, while
/// `<f64 as FromStr>` accepts exactly those tokens. Unbounded extents are live
/// in this harness — `extrude_infinite_integration.rs` asserts `is_finite()`
/// precisely because the op under test is unbounded before clipping — so a
/// JSON-strict parser would degrade that assertion's failure mode from "x/y
/// bbox extents should all be finite" into an opaque "failed to parse as JSON".
///
/// # Panics
///
/// - naming the absent field, if any of the six keys is missing. This is the
///   canonical statement of the strictness decision — every other mention in
///   this module points here. It is the stricter of the two behaviours found
///   among the seven parsers this consolidates: silently yielding `f64::NAN`
///   would make a downstream `(zmax - zmin).abs() < tol` quietly evaluate
///   false, surfacing a malformed kernel response as a confusing geometry
///   assertion rather than a parse failure.
/// - quoting the offending pair and the full input, if a value does not parse
///   as `f64` or a pair carries no `:` separator.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn parse_bbox(s: &str) -> BBox {
    const NAMES: [&str; 6] = ["xmin", "ymin", "zmin", "xmax", "ymax", "zmax"];
    let mut fields: [Option<f64>; 6] = [None; 6];

    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        // `splitn` always yields at least one item, so the key is always present.
        let key = parts.next().unwrap_or_default().trim().trim_matches('"');
        let Some(slot) = NAMES.iter().position(|n| *n == key) else {
            continue; // unrecognised key (e.g. a future extension): ignore
        };
        let raw = parts
            .next()
            .unwrap_or_else(|| panic!("bbox pair {pair:?} has no ':' separator, in {s:?}"));
        let val: f64 = raw.trim().parse().unwrap_or_else(|e| {
            panic!("bbox pair {pair:?} has a non-numeric value ({e}), in {s:?}")
        });
        fields[slot] = Some(val);
    }

    let get = |i: usize| -> f64 {
        fields[i].unwrap_or_else(|| panic!("bbox is missing the {} field, in {s:?}", NAMES[i]))
    };
    BBox {
        xmin: get(0),
        ymin: get(1),
        zmin: get(2),
        xmax: get(3),
        ymax: get(4),
        zmax: get(5),
    }
}

impl BBox {
    /// Per-axis extents `(xmax - xmin, ymax - ymin, zmax - zmin)`.
    #[allow(dead_code)] // only called from has_occt integration-test binaries
    pub fn spans(&self) -> (f64, f64, f64) {
        (
            self.xmax - self.xmin,
            self.ymax - self.ymin,
            self.zmax - self.zmin,
        )
    }

    /// True when all six components are finite (no `inf`, no `NaN`).
    ///
    /// Replaces the per-component `is_finite()` loops that several call sites
    /// hand-rolled; `BBox`'s `Debug` derive makes the resulting failure message
    /// strictly more informative than a single offending component.
    #[allow(dead_code)] // only called from has_occt integration-test binaries
    pub fn all_finite(&self) -> bool {
        self.xmin.is_finite()
            && self.ymin.is_finite()
            && self.zmin.is_finite()
            && self.xmax.is_finite()
            && self.ymax.is_finite()
            && self.zmax.is_finite()
    }
}

/// Unwrap the result of a `GeometryQuery::BoundingBox` query into a [`BBox`].
///
/// Takes the already-produced `Result` rather than a kernel plus a handle id
/// because the call sites use two unrelated kernel types — `OcctKernel`
/// (`src/lib.rs:3628`) and `OcctKernelHandle` (`src/handle.rs:370`) — with no
/// `Deref` between them. Both expose
/// `pub fn query(&self, &GeometryQuery) -> Result<Value, QueryError>`, so
/// accepting the `Result` lets ONE helper serve both without introducing a
/// trait or duplicating an accessor per kernel type.
///
/// Generic over `E: Debug` rather than hard-coding `QueryError` so this module
/// need not import reify-ir's error enum, and so a future change to that enum
/// does not ripple into the shared test module.
///
/// # Panics
///
/// Panics if the query failed (surfacing the underlying error's `Debug`), or
/// if it returned any `Value` variant other than `String` (naming the variant
/// received).
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn bbox_of<E: std::fmt::Debug>(query_result: Result<Value, E>) -> BBox {
    let value = query_result.expect("BoundingBox query should succeed");
    match value {
        Value::String(s) => parse_bbox(&s),
        other => panic!("expected bbox String, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Contract tests for the shared `BBox` / `parse_bbox` helpers above, which
// consolidate the seven hand-rolled bbox parsers previously scattered across
// `tests/harness_occt/`. These `#[test]` fns run as `common::<name>` within the
// single `harness_occt` test binary (task 5277 folded the 51 former binaries
// into one compile unit, so `mod common;` is a normal module of it).
// ---------------------------------------------------------------------------

/// Build a bbox string EXACTLY the way the kernel does
/// (`crates/reify-kernel-occt/src/lib.rs:3673-3677`): each component goes
/// through `f64` **Display**, which is not a JSON number encoder.
fn producer_format(xmin: f64, ymin: f64, zmin: f64, xmax: f64, ymax: f64, zmax: f64) -> String {
    format!(
        "{{\"xmin\":{},\"ymin\":{},\"zmin\":{},\"xmax\":{},\"ymax\":{},\"zmax\":{}}}",
        xmin, ymin, zmin, xmax, ymax, zmax
    )
}

/// (a) All six fields are recovered from the producer's exact wire format.
///
/// Exact `assert_eq!` is the correct assertion here, not a tolerance: every
/// literal below is a dyadic rational exactly representable in binary64, `f64`
/// Display emits the shortest round-tripping form, and Rust's `f64: FromStr` is
/// correctly rounded — so the round-trip is an exact-representation identity.
#[test]
fn parse_bbox_reads_all_six_fields() {
    let s = producer_format(-1.0, -2.5, -0.5, 4.0, 2.5, 0.5);
    let bb: BBox = parse_bbox(&s);
    assert_eq!(bb.xmin, -1.0, "xmin from {s}");
    assert_eq!(bb.ymin, -2.5, "ymin from {s}");
    assert_eq!(bb.zmin, -0.5, "zmin from {s}");
    assert_eq!(bb.xmax, 4.0, "xmax from {s}");
    assert_eq!(bb.ymax, 2.5, "ymax from {s}");
    assert_eq!(bb.zmax, 0.5, "zmax from {s}");
}

/// (b) Non-finite extents parse — the contract that distinguishes this helper
/// from a `serde_json`-based one.
///
/// `<f64 as Display>` emits the bare tokens `inf` / `-inf` / `NaN`, which strict
/// JSON forbids and `serde_json::from_str` rejects outright, while
/// `<f64 as FromStr>` accepts exactly those tokens. Unbounded extents are live
/// in this harness: `extrude_infinite_integration.rs` asserts `is_finite()`
/// precisely because the op under test is unbounded before clipping, and that
/// assertion is only meaningful if a non-finite bbox reaches it as a number
/// rather than dying in the parser.
#[test]
fn parse_bbox_accepts_non_finite_extents() {
    let s = producer_format(
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        0.0,
        f64::INFINITY,
        f64::INFINITY,
        0.5,
    );
    assert!(
        s.contains("-inf") && s.contains("\"xmax\":inf"),
        "f64 Display must emit the bare inf/-inf tokens that strict JSON forbids, got {s}"
    );
    let bb = parse_bbox(&s);
    assert!(
        bb.xmin.is_infinite() && bb.xmin.is_sign_negative(),
        "xmin should round-trip as -inf, got {}",
        bb.xmin
    );
    assert!(
        bb.xmax.is_infinite() && bb.xmax.is_sign_positive(),
        "xmax should round-trip as +inf, got {}",
        bb.xmax
    );
    assert_eq!(bb.zmin, 0.0, "finite fields still parse alongside inf ones");
    assert_eq!(bb.zmax, 0.5, "finite fields still parse alongside inf ones");

    // NaN is the other Display token strict JSON forbids.
    let nan = parse_bbox(&producer_format(f64::NAN, 0.0, 0.0, 1.0, 1.0, 1.0));
    assert!(nan.xmin.is_nan(), "NaN should round-trip, got {}", nan.xmin);
}

/// (c) Whitespace around `:` / `,` is trimmed, keys parse with or without
/// surrounding quotes, and an unrecognised extra key is ignored rather than
/// fatal.
#[test]
fn parse_bbox_tolerates_whitespace_and_quoted_keys() {
    let s = "{ \"xmin\" : -1.0 , ymin: -2.0 , \"zmin\" :-3.0 , \
             \"xmax\": 1.0 , ymax : 2.0 , \"zmax\" : 3.0 , \"future_key\": 99.0 }";
    let bb = parse_bbox(s);
    assert_eq!(bb.xmin, -1.0);
    assert_eq!(bb.ymin, -2.0);
    assert_eq!(bb.zmin, -3.0);
    assert_eq!(bb.xmax, 1.0);
    assert_eq!(bb.ymax, 2.0);
    assert_eq!(bb.zmax, 3.0);
}

/// (d) An absent key PANICS naming the field — it must NOT silently return
/// `f64::NAN`, which would make a downstream `(zmax - zmin).abs() < tol`
/// quietly evaluate false and surface a parse failure as a confusing geometry
/// assertion.
#[test]
#[should_panic(expected = "zmax")]
fn parse_bbox_panics_on_missing_field() {
    let _ = parse_bbox("{\"xmin\":-1,\"ymin\":-2,\"zmin\":-3,\"xmax\":1,\"ymax\":2}");
}

/// (e) A non-numeric value panics, naming the offending pair AND quoting the
/// full input so the malformed kernel response is visible in the failure.
#[test]
#[should_panic(expected = "not-a-number")]
fn parse_bbox_panics_on_unparseable_value() {
    let _ =
        parse_bbox("{\"xmin\":-1,\"ymin\":-2,\"zmin\":not-a-number,\"xmax\":1,\"ymax\":2,\"zmax\":3}");
}

/// A second, distinct `Debug` error type, so [`bbox_of`]'s `E: Debug`
/// genericity is exercised by two real instantiations rather than merely
/// asserted in a doc comment. Stands in for `QueryError`, which this module
/// deliberately does not import.
#[derive(Debug)]
#[allow(dead_code)] // field 0 is read only through the derived Debug impl
struct FakeQueryError(&'static str);

/// (a) `bbox_of` unwraps a successful `Value::String` query into the same
/// `BBox` that `parse_bbox` yields for that string, for two different error
/// types.
#[test]
fn bbox_of_unwraps_ok_string() {
    let s = producer_format(-1.0, -2.0, -3.0, 1.0, 2.0, 3.0);
    let expected = BBox {
        xmin: -1.0,
        ymin: -2.0,
        zmin: -3.0,
        xmax: 1.0,
        ymax: 2.0,
        zmax: 3.0,
    };

    // E = String
    let via_string = bbox_of(Ok::<_, String>(Value::String(s.clone())));
    assert_eq!(via_string, expected);
    assert_eq!(via_string, parse_bbox(&s), "bbox_of must agree with parse_bbox");

    // E = FakeQueryError — a second instantiation of the `E: Debug` parameter,
    // which is what lets one helper serve both `OcctKernel::query` and
    // `OcctKernelHandle::query`.
    let via_other = bbox_of(Ok::<_, FakeQueryError>(Value::String(s)));
    assert_eq!(via_other, expected);
}

/// (b) A failed query panics with the fixed prefix the seven call sites used.
#[test]
#[should_panic(expected = "BoundingBox query should succeed")]
fn bbox_of_panics_on_query_error() {
    let _ = bbox_of(Err::<Value, _>(FakeQueryError("kernel exploded")));
}

/// (b, cont.) …and that panic surfaces the underlying error's `Debug`, so the
/// kernel's own diagnostic is not swallowed by the shared helper.
#[test]
#[should_panic(expected = "kernel exploded")]
fn bbox_of_error_panic_surfaces_error_debug() {
    let _ = bbox_of(Err::<Value, _>(FakeQueryError("kernel exploded")));
}

/// (c) A non-`String` `Value` panics naming the received variant, preserving
/// the `panic!("expected bbox String, got {:?}", other)` behaviour of the five
/// call sites that match on `Value::String` today.
#[test]
#[should_panic(expected = "Real")]
fn bbox_of_panics_on_non_string_value() {
    let _ = bbox_of(Ok::<_, String>(Value::Real(1.0)));
}

/// (d) `spans()` returns per-axis extents.
///
/// Endpoints are chosen so every difference is exact in binary64, so
/// `assert_eq!` is the correct assertion — no tolerance is needed or
/// appropriate for a pure subtraction of representable values.
#[test]
fn bbox_spans_returns_per_axis_extents() {
    let bb = BBox {
        xmin: -1.0,
        ymin: -2.5,
        zmin: 0.5,
        xmax: 3.0,
        ymax: 2.5,
        zmax: 4.5,
    };
    let (dx, dy, dz) = bb.spans();
    assert_eq!(dx, 4.0, "x span");
    assert_eq!(dy, 5.0, "y span");
    assert_eq!(dz, 4.0, "z span");
}

/// (e) `all_finite()` is true only when every one of the six components is
/// finite. This is the predicate that replaces `sweep_guided_integration.rs`'s
/// per-component `is_finite()` loop, so it must reject an `inf` or a `NaN` in
/// ANY position, not just the first.
#[test]
fn bbox_all_finite_discriminates() {
    let finite = BBox {
        xmin: -1.0,
        ymin: -2.0,
        zmin: -3.0,
        xmax: 1.0,
        ymax: 2.0,
        zmax: 3.0,
    };
    assert!(finite.all_finite(), "an all-finite box is all_finite");

    let cases = [
        ("xmin=-inf", BBox { xmin: f64::NEG_INFINITY, ..finite }),
        ("ymin=+inf", BBox { ymin: f64::INFINITY, ..finite }),
        ("zmin=NaN", BBox { zmin: f64::NAN, ..finite }),
        ("xmax=+inf", BBox { xmax: f64::INFINITY, ..finite }),
        ("ymax=NaN", BBox { ymax: f64::NAN, ..finite }),
        ("zmax=-inf", BBox { zmax: f64::NEG_INFINITY, ..finite }),
    ];
    for (label, bad) in cases {
        assert!(
            !bad.all_finite(),
            "a box with {label} must not be all_finite, got {bad:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// JSON-Point3 (xyz) parsing (task 5937)
// ---------------------------------------------------------------------------

/// A point/direction triple returned by the kernel's JSON-Point3 queries,
/// parsed from the wire string by [`parse_xyz`].
///
/// Field names match the JSON keys, so a call site can destructure exactly the
/// axes it cares about: `let Xyz { z, .. } = ...`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // fields only read from has_occt integration-test binaries
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Parse the JSON-Point3 string returned by the kernel's point/direction
/// queries.
///
/// Wire format: `{"x":<f>,"y":<f>,"z":<f>}`, shared by five query variants —
/// `Centroid`, `EdgeTangent`, `FaceNormal`, `FaceNormalAt` and
/// `ClosestPointOnShape`, all of which route through `centroid_json`
/// (`src/lib.rs:274`) or an identical `format!`.
///
/// Keys parse with or without surrounding quotes, whitespace around `:` and `,`
/// is trimmed, and unrecognised keys are ignored. A repeated key takes
/// last-wins; empty or whitespace-only input recognises no key at all and so
/// takes the missing-field panic below.
///
/// **Deliberately NOT `serde_json`-based**, even though `serde_json` is already
/// a dev-dependency of this crate. The producer `centroid_json` is a single
/// `format!` that passes each component through `f64` **Display**, which is not
/// a JSON number encoder: a non-finite component is emitted as the bare token
/// `inf` / `-inf` / `NaN`, which strict JSON forbids and `serde_json::from_str`
/// rejects outright, while `<f64 as FromStr>` accepts exactly those tokens. The
/// constraint follows from the producer's formatting alone — unlike the bbox
/// format, no harness site currently produces a non-finite centroid, tangent or
/// normal, so see [`parse_bbox`] for the case where unbounded values are live
/// in this harness and the distinction is load-bearing today.
///
/// # Panics
///
/// - naming the absent field, if any of `x` / `y` / `z` is missing. This adopts
///   the strictest of the behaviours found among the parsers it consolidates:
///   `sweep_guided_integration.rs`'s silently defaulted a missing key to
///   `f64::NAN`, which would make a downstream `(z - target).abs() < tol`
///   quietly evaluate false, surfacing a malformed kernel response as a
///   confusing geometry assertion rather than a parse failure.
/// - quoting the offending pair and the full input, if a value does not parse
///   as `f64` or a pair carries no `:` separator.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn parse_xyz(s: &str) -> Xyz {
    const NAMES: [&str; 3] = ["x", "y", "z"];
    let mut fields: [Option<f64>; 3] = [None; 3];

    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        // `splitn` always yields at least one item, so the key is always present.
        let key = parts.next().unwrap_or_default().trim().trim_matches('"');
        let Some(slot) = NAMES.iter().position(|n| *n == key) else {
            continue; // unrecognised key (e.g. a future extension): ignore
        };
        let raw = parts
            .next()
            .unwrap_or_else(|| panic!("xyz pair {pair:?} has no ':' separator, in {s:?}"));
        let val: f64 = raw.trim().parse().unwrap_or_else(|e| {
            panic!("xyz pair {pair:?} has a non-numeric value ({e}), in {s:?}")
        });
        fields[slot] = Some(val);
    }

    let get = |i: usize| -> f64 {
        fields[i].unwrap_or_else(|| panic!("xyz is missing the {} field, in {s:?}", NAMES[i]))
    };
    Xyz {
        x: get(0),
        y: get(1),
        z: get(2),
    }
}

// ---------------------------------------------------------------------------
// Contract tests for the shared `Xyz` / `parse_xyz` helpers above. Like the
// bbox ones, these `#[test]` fns run as `common::<name>` within the single
// `harness_occt` test binary.
// ---------------------------------------------------------------------------

/// Build a JSON-Point3 string EXACTLY the way the kernel does
/// (`crates/reify-kernel-occt/src/lib.rs:274`, `fn centroid_json`): each
/// component goes through `f64` **Display**, which is not a JSON number encoder.
fn producer_format_xyz(x: f64, y: f64, z: f64) -> String {
    format!("{{\"x\":{},\"y\":{},\"z\":{}}}", x, y, z)
}

/// (a) All three fields are recovered from the producer's exact wire format.
///
/// Exact `assert_eq!` is the correct assertion here, not a tolerance: every
/// literal below is a dyadic rational exactly representable in binary64, `f64`
/// Display emits the shortest round-tripping form, and Rust's `f64: FromStr` is
/// correctly rounded — so the round-trip is an exact-representation identity.
#[test]
fn parse_xyz_reads_all_three_fields() {
    let s = producer_format_xyz(-1.0, 2.5, -0.5);
    let p: Xyz = parse_xyz(&s);
    assert_eq!(p.x, -1.0, "x from {s}");
    assert_eq!(p.y, 2.5, "y from {s}");
    assert_eq!(p.z, -0.5, "z from {s}");
}

/// (b) Non-finite components parse — the contract that distinguishes this
/// helper from a `serde_json`-based one.
///
/// The assertion on the raw string comes first deliberately: it pins the
/// PRODUCER's `f64` Display formatting, not merely this parser's tolerance, so
/// the test fails if someone later swaps `centroid_json` to a real JSON
/// encoder. `<f64 as Display>` emits the bare tokens `inf` / `-inf` / `NaN`,
/// which strict JSON forbids and `serde_json::from_str` rejects outright, while
/// `<f64 as FromStr>` accepts exactly those tokens.
#[test]
fn parse_xyz_accepts_non_finite_components() {
    let s = producer_format_xyz(f64::NEG_INFINITY, f64::INFINITY, f64::NAN);
    assert!(
        s.contains("\"x\":-inf") && s.contains("\"y\":inf"),
        "f64 Display must emit the bare inf/-inf tokens that strict JSON forbids, got {s}"
    );
    let p = parse_xyz(&s);
    assert!(
        p.x.is_infinite() && p.x.is_sign_negative(),
        "x should round-trip as -inf, got {}",
        p.x
    );
    assert!(
        p.y.is_infinite() && p.y.is_sign_positive(),
        "y should round-trip as +inf, got {}",
        p.y
    );
    assert!(p.z.is_nan(), "NaN should round-trip, got {}", p.z);
}

/// (c) Whitespace around `:` / `,` is trimmed, keys parse with or without
/// surrounding quotes, and an unrecognised extra key is ignored rather than
/// fatal.
#[test]
fn parse_xyz_tolerates_whitespace_and_quoted_keys() {
    let s = "{ \"x\" : -1.0 , y: 2.0 , \"z\" :-3.0 , \"future_key\": 99.0 }";
    let p = parse_xyz(s);
    assert_eq!(p.x, -1.0);
    assert_eq!(p.y, 2.0);
    assert_eq!(p.z, -3.0);
}

/// (d) An absent key PANICS naming the field — it must NOT silently return
/// `f64::NAN`, which is what `sweep_guided_integration.rs`'s parser did and
/// which would make a downstream `(z - target).abs() < tol` quietly evaluate
/// false, surfacing a malformed kernel response as a confusing geometry
/// assertion rather than a parse failure.
#[test]
#[should_panic(expected = "z")]
fn parse_xyz_panics_on_missing_field() {
    let _ = parse_xyz("{\"x\":1,\"y\":2}");
}

/// (e) A non-numeric value panics, naming the offending pair AND quoting the
/// full input so the malformed kernel response is visible in the failure.
#[test]
#[should_panic(expected = "not-a-number")]
fn parse_xyz_panics_on_unparseable_value() {
    let _ = parse_xyz("{\"x\":1,\"y\":not-a-number,\"z\":3}");
}

/// (a) `xyz_of` unwraps a successful `Value::String` query into the same `Xyz`
/// that `parse_xyz` yields for that string, for two different error types.
///
/// Reuses the [`FakeQueryError`] declared above for the bbox tests rather than
/// introducing a second stand-in.
#[test]
fn xyz_of_unwraps_ok_string() {
    let s = producer_format_xyz(-1.0, 2.0, -3.0);
    let expected = Xyz {
        x: -1.0,
        y: 2.0,
        z: -3.0,
    };

    // E = String
    let via_string = xyz_of(Ok::<_, String>(Value::String(s.clone())), "Centroid");
    assert_eq!(via_string, expected);
    assert_eq!(via_string, parse_xyz(&s), "xyz_of must agree with parse_xyz");

    // E = FakeQueryError — a second instantiation of the `E: Debug` parameter,
    // which is what lets one helper serve both `OcctKernel::query` and
    // `OcctKernelHandle::query` (unrelated types with no `Deref` between them).
    let via_other = xyz_of(Ok::<_, FakeQueryError>(Value::String(s)), "Centroid");
    assert_eq!(via_other, expected);
}

/// (b) A failed query panics NAMING THE QUERY. This is `query_label`'s whole
/// reason for existing: unlike `bbox_of`, whose seven call sites all issued
/// `GeometryQuery::BoundingBox`, this helper serves five different query
/// variants, so a fixed panic string would erase which one failed.
#[test]
#[should_panic(expected = "FaceNormal")]
fn xyz_of_error_panic_names_the_query() {
    let _ = xyz_of(
        Err::<Value, _>(FakeQueryError("kernel exploded")),
        "FaceNormal",
    );
}

/// (c) …and that same panic surfaces the underlying error's `Debug`, so the
/// kernel's own diagnostic is not swallowed by the shared helper.
#[test]
#[should_panic(expected = "kernel exploded")]
fn xyz_of_error_panic_surfaces_error_debug() {
    let _ = xyz_of(
        Err::<Value, _>(FakeQueryError("kernel exploded")),
        "FaceNormal",
    );
}

/// (d) A non-`String` `Value` panics naming the received variant, preserving
/// the `panic!("expected ... String, got {other:?}")` behaviour of the call
/// sites this consolidates.
#[test]
#[should_panic(expected = "Real")]
fn xyz_of_panics_on_non_string_value() {
    let _ = xyz_of(Ok::<_, String>(Value::Real(1.0)), "Centroid");
}
