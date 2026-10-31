//! Decoders for the `GeometryQuery` reply wire formats OCCT and Manifold both
//! emit — one copy for every test binary that reads them. Included via
//! `#[path]` by this crate's `tests/queries.rs` and by
//! `reify-kernel-conformance/tests/adjacency_index_space_parity.rs`.

use reify_ir::Value;

/// Parse the `{"xmin":_,…,"zmax":_}` `BoundingBox` wire string into
/// `[xmin, ymin, zmin, xmax, ymax, zmax]`, panicking on any other shape.
pub fn parse_bbox(s: &str) -> [f64; 6] {
    const KEYS: [&str; 6] = ["xmin", "ymin", "zmin", "xmax", "ymax", "zmax"];
    let mut out = [f64::NAN; 6];
    for field in s
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
    {
        let (key, val) = field
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed bbox field {field:?} in {s:?}"));
        let idx = KEYS
            .iter()
            .position(|k| *k == key.trim().trim_matches('"'))
            .unwrap_or_else(|| panic!("unexpected bbox key {key:?} in {s:?}"));
        out[idx] = val
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric bbox value {val:?} in {s:?}"));
    }
    assert!(
        out.iter().all(|c| !c.is_nan()),
        "bbox JSON {s:?} is missing a key"
    );
    out
}

/// Decode the `AdjacentFaces` / `SharedEdges` reply — a `Value::List` of
/// non-negative `Value::Int` — into indices, panicking on any other shape.
pub fn index_list(value: &Value) -> Vec<usize> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Int(i) => usize::try_from(*i)
                    .unwrap_or_else(|_| panic!("indices must be non-negative; got {i}")),
                other => panic!("index list must hold Value::Int; got {other:?}"),
            })
            .collect(),
        other => panic!("expected Value::List of indices; got {other:?}"),
    }
}
