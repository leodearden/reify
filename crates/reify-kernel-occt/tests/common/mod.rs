//! Shared helpers for the `harness_occt` integration tests: local-feature
//! (fillet/chamfer) assertions, and bounding-box query parsing.
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
//! across `tests/harness_occt/` in two mutually-inconsistent families — a
//! duplication finding filed during task 5377's reviewer-amendment pass and
//! resolved by task 5893.

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
/// is trimmed, and unrecognised keys are ignored.
///
/// **Deliberately NOT `serde_json`-based**, even though `serde_json` is already
/// a dev-dependency of this crate. The producer
/// (`crates/reify-kernel-occt/src/lib.rs:3673-3677`) builds this string with a
/// single `format!` that passes each component through `f64` **Display**, which
/// is not a JSON number encoder: a non-finite extent is emitted as the bare
/// token `inf` / `-inf` / `NaN`, which strict JSON forbids and
/// `serde_json::from_str` rejects outright, while `<f64 as FromStr>` accepts
/// exactly those tokens. Unbounded extents are live in this harness —
/// `extrude_infinite_integration.rs` asserts `is_finite()` precisely because
/// the op under test is unbounded before clipping — so a JSON-strict parser
/// would degrade that assertion's failure mode from "x/y bbox extents should
/// all be finite" into an opaque "failed to parse as JSON".
///
/// # Panics
///
/// - naming the absent field, if any of the six keys is missing. This is the
///   stricter of the two behaviours found among the seven parsers this
///   consolidates: silently yielding `f64::NAN` would make a downstream
///   `(zmax - zmin).abs() < tol` quietly evaluate false, surfacing a malformed
///   kernel response as a confusing geometry assertion rather than a parse
///   failure.
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
