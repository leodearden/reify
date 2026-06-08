//! Tensegrity stdlib builtins.
//!
//! Provides `tensegrity_wires(t)` — the generic wire accessor that converts a
//! `Tensegrity` structure-instance into a `List<TensegrityWire>` for viewport
//! rendering and solver consumption (PRD §3 open typed-element-groups seam).
//!
//! Design decisions (from plan):
//!   DD2: struts/cables are explicit named fields; the accessor is the seam.
//!   DD3: emits Value::StructureInstance(TensegrityWire) with inline endpoint
//!        coordinates (not just indices) so consumers render without a second
//!        nodes-table dereference.
//!   DD4: T0a signal is CLI-only; no kernel line_segment emission here.
//!   Reuse 4: silent-Undef discipline for all shape-guard failures.

use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Evaluate a tensegrity stdlib function by name.
///
/// Returns `Some(value)` if the name is recognised, `None` otherwise.
pub(crate) fn eval_tensegrity(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "tensegrity_wires"    => tensegrity_wires(args),
        "tensegrity_surfaces" => tensegrity_surfaces(args),
        _ => return None,
    })
}

/// Extract the three `(x, y, z)` Length-typed Scalars from a `Value::Point`
/// node. Returns `None` if the value is not a 3-component Point or if any
/// component is not a Length-dimensioned Scalar.
fn extract_node_xyz(node: &Value) -> Option<(Value, Value, Value)> {
    match node {
        Value::Point(comps) if comps.len() == 3 => {
            // Each component must be a Scalar (point3(1m, 2m, 3m) produces
            // Scalar{Length} components; bare Real components from point3(1.0,
            // 2.0, 3.0) are also accepted for unit-less coordinates).
            // We forward the component Value as-is into TensegrityWire.
            Some((comps[0].clone(), comps[1].clone(), comps[2].clone()))
        }
        _ => None,
    }
}

/// Build a `TensegrityWire` `Value::StructureInstance` using `StructureTypeId(0)`
/// as the placeholder type_id (SIR-α convention — `engine_eval.rs:1789`).
fn build_wire(kind: &str, from: i64, to: i64, p_from: &(Value, Value, Value), p_to: &(Value, Value, Value)) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("kind".to_string(),       Value::String(kind.to_string())),
        ("from_index".to_string(), Value::Int(from)),
        ("to_index".to_string(),   Value::Int(to)),
        ("x1".to_string(),         p_from.0.clone()),
        ("y1".to_string(),         p_from.1.clone()),
        ("z1".to_string(),         p_from.2.clone()),
        ("x2".to_string(),         p_to.0.clone()),
        ("y2".to_string(),         p_to.1.clone()),
        ("z2".to_string(),         p_to.2.clone()),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "TensegrityWire".to_string(),
        version: 1,
        fields,
    }))
}

/// Shared accessor prologue: validate arity==1 and arg is a `Tensegrity`
/// `StructureInstance`. Returns a reference to the fields map on success,
/// `None` (→ `Value::Undef`) otherwise. Used by both `tensegrity_wires` and
/// `tensegrity_surfaces` to eliminate the duplicated arity+type-guard block.
fn check_tensegrity_args(args: &[Value]) -> Option<&PersistentMap<String, Value>> {
    if args.len() != 1 {
        return None;
    }
    match &args[0] {
        Value::StructureInstance(data) if data.type_name == "Tensegrity" => Some(&data.fields),
        _ => None,
    }
}

/// Extract and validate node XYZ tuples from a Tensegrity fields map.
/// Returns `Some(node_xyzs)` if `fields["nodes"]` is a valid `List<Point3>`,
/// `None` (→ `Value::Undef`) otherwise.
fn extract_nodes(fields: &PersistentMap<String, Value>) -> Option<Vec<(Value, Value, Value)>> {
    match fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => {
            let mut v = Vec::with_capacity(ns.len());
            for n in ns.iter() {
                v.push(extract_node_xyz(n)?);
            }
            Some(v)
        }
        _ => None,
    }
}

/// Extract index tuples from a `List<List<Int>>` field value.
///
/// Each inner list must be exactly `arity` elements, all `Value::Int`.
/// Generalises the old `extract_index_pairs` (arity=2) and
/// `extract_index_triples` (arity=3) into a single helper so future
/// typed-element groups (DD2 seam) can reuse the same validation without
/// another near-verbatim copy. Returns `None` on any shape violation
/// (wrong outer type, wrong inner length, non-Int inner element).
fn extract_index_tuples(v: &Value, arity: usize) -> Option<Vec<Vec<i64>>> {
    match v {
        Value::List(items) => {
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::List(indices) if indices.len() == arity => {
                        let mut tuple = Vec::with_capacity(arity);
                        for idx in indices {
                            match idx {
                                Value::Int(i) => tuple.push(*i),
                                _ => return None,
                            }
                        }
                        result.push(tuple);
                    }
                    _ => return None,
                }
            }
            Some(result)
        }
        _ => None,
    }
}

/// `tensegrity_wires(t : Tensegrity) -> List<TensegrityWire>`
///
/// Converts a `Tensegrity` structure-instance into a flat list of
/// `TensegrityWire` instances — 3 struts then 3 cables for a T-prism,
/// generalising to any combination of strut/cable pairs.
///
/// # Shape contract
///
/// - Exactly 1 argument.
/// - args[0] must be `Value::StructureInstance` with `type_name == "Tensegrity"`.
/// - `fields["nodes"]` must be `Value::List` of `Value::Point([x, y, z])`.
/// - `fields["struts"]` and `fields["cables"]` must be `Value::List` of
///   `Value::List` of exactly 2 `Value::Int` indices.
/// - Every from/to index must be in `0 .. nodes.len()`.
///
/// Any violation returns `Value::Undef` (silent-Undef per PRD task #10 / DD4).
///
/// # Output order
///
/// Struts precede cables (declaration order per DD2 / open-groups seam).
/// Within each group, pairs are emitted in declaration order.
fn tensegrity_wires(args: &[Value]) -> Value {
    // Shared arity + type guard.
    let fields = match check_tensegrity_args(args) {
        Some(f) => f,
        None => return Value::Undef,
    };

    // Extract and validate nodes.
    let node_xyzs = match extract_nodes(fields) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let n_nodes = node_xyzs.len();

    // Extract strut and cable index pair lists (arity=2).
    let struts = match fields.get(&"struts".to_string()) {
        Some(v) => match extract_index_tuples(v, 2) {
            Some(pairs) => pairs,
            None => return Value::Undef,
        },
        None => return Value::Undef,
    };
    let cables = match fields.get(&"cables".to_string()) {
        Some(v) => match extract_index_tuples(v, 2) {
            Some(pairs) => pairs,
            None => return Value::Undef,
        },
        None => return Value::Undef,
    };

    // Emit wires: struts first, then cables (DD2 declaration order).
    let mut wires = Vec::with_capacity(struts.len() + cables.len());

    for pair in &struts {
        let from = pair[0];
        let to = pair[1];
        // Validate index range.
        if from < 0 || from as usize >= n_nodes || to < 0 || to as usize >= n_nodes {
            return Value::Undef;
        }
        wires.push(build_wire("strut", from, to, &node_xyzs[from as usize], &node_xyzs[to as usize]));
    }

    for pair in &cables {
        let from = pair[0];
        let to = pair[1];
        if from < 0 || from as usize >= n_nodes || to < 0 || to as usize >= n_nodes {
            return Value::Undef;
        }
        wires.push(build_wire("cable", from, to, &node_xyzs[from as usize], &node_xyzs[to as usize]));
    }

    Value::List(wires)
}

/// Build a `TensegritySurface` `Value::StructureInstance` using `StructureTypeId(0)`
/// as the placeholder type_id (SIR-α convention — mirrors `build_wire`).
fn build_surface(
    i0: i64, i1: i64, i2: i64,
    p0: &(Value, Value, Value),
    p1: &(Value, Value, Value),
    p2: &(Value, Value, Value),
) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("kind".to_string(), Value::String("membrane".to_string())),
        ("i0".to_string(),   Value::Int(i0)),
        ("i1".to_string(),   Value::Int(i1)),
        ("i2".to_string(),   Value::Int(i2)),
        ("x0".to_string(),   p0.0.clone()),
        ("y0".to_string(),   p0.1.clone()),
        ("z0".to_string(),   p0.2.clone()),
        ("x1".to_string(),   p1.0.clone()),
        ("y1".to_string(),   p1.1.clone()),
        ("z1".to_string(),   p1.2.clone()),
        ("x2".to_string(),   p2.0.clone()),
        ("y2".to_string(),   p2.1.clone()),
        ("z2".to_string(),   p2.2.clone()),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "TensegritySurface".to_string(),
        version: 1,
        fields,
    }))
}

/// `tensegrity_surfaces(t : Tensegrity) -> List<TensegritySurface>`
///
/// Converts the `surfaces` triangle list of a `Tensegrity` structure-instance
/// into a flat list of `TensegritySurface` instances (one per triangle).
///
/// # Shape contract
///
/// - Exactly 1 argument.
/// - args[0] must be `Value::StructureInstance` with `type_name == "Tensegrity"`.
/// - `fields["nodes"]` must be `Value::List` of `Value::Point([x, y, z])`.
/// - `fields["surfaces"]` — if ABSENT or EMPTY, returns `Value::List([])` (not Undef).
///   A missing surfaces field is legitimate for non-membrane nets.
/// - If present, `fields["surfaces"]` must be `Value::List` of
///   `Value::List` of exactly 3 `Value::Int` indices.
/// - Every i0/i1/i2 index must be in `0 .. nodes.len()`.
///
/// Any actual violation (wrong arity, wrong type, inner-list length != 3,
/// negative index, out-of-range index, malformed nodes) returns `Value::Undef`
/// (silent-Undef per DD4 / Reuse-4).
///
/// # Output order
///
/// Facets are emitted in declaration order (matching `surfaces` list order).
fn tensegrity_surfaces(args: &[Value]) -> Value {
    // Shared arity + type guard.
    let fields = match check_tensegrity_args(args) {
        Some(f) => f,
        None => return Value::Undef,
    };

    // SHORT-CIRCUIT: check surfaces before extracting nodes. For non-membrane
    // nets the surfaces field is legitimately absent — extracting and validating
    // the entire nodes table would be wasted work in that common case.
    // Design Decision: absent or empty surfaces → [] (not Undef).
    let surfaces_val = match fields.get(&"surfaces".to_string()) {
        None => return Value::List(vec![]),
        Some(v) => v,
    };

    let triples = match extract_index_tuples(surfaces_val, 3) {
        Some(ts) => ts,
        None => return Value::Undef,
    };

    if triples.is_empty() {
        return Value::List(vec![]);
    }

    // Only extract and validate nodes when there are surface triples to emit.
    let node_xyzs = match extract_nodes(fields) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let n_nodes = node_xyzs.len();

    // Emit facets: validate and build each TensegritySurface.
    let mut facets = Vec::with_capacity(triples.len());

    for triple in &triples {
        let i0 = triple[0];
        let i1 = triple[1];
        let i2 = triple[2];
        // Validate all three indices (negative or out-of-range → Undef).
        if i0 < 0 || i0 as usize >= n_nodes
            || i1 < 0 || i1 as usize >= n_nodes
            || i2 < 0 || i2 as usize >= n_nodes
        {
            return Value::Undef;
        }
        facets.push(build_surface(
            i0, i1, i2,
            &node_xyzs[i0 as usize],
            &node_xyzs[i1 as usize],
            &node_xyzs[i2 as usize],
        ));
    }

    Value::List(facets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    fn length(m: f64) -> Value {
        Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
    }

    fn node(x: f64, y: f64, z: f64) -> Value {
        Value::Point(vec![length(x), length(y), length(z)])
    }

    fn simple_tensegrity() -> Value {
        let nodes = Value::List(vec![node(0.0, 0.0, 0.0), node(1.0, 0.0, 0.0)]);
        let struts = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(1)])]);
        let cables = Value::List(vec![]);
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), nodes),
            ("struts".to_string(), struts),
            ("cables".to_string(), cables),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Tensegrity".to_string(),
            version: 1,
            fields,
        }))
    }

    #[test]
    fn happy_path_one_strut_no_cables() {
        let t = simple_tensegrity();
        let result = tensegrity_wires(&[t]);
        match &result {
            Value::List(wires) => {
                assert_eq!(wires.len(), 1, "expected 1 wire");
                match &wires[0] {
                    Value::StructureInstance(data) => {
                        assert_eq!(data.type_name, "TensegrityWire");
                        assert_eq!(data.fields.get(&"kind".to_string()), Some(&Value::String("strut".to_string())));
                        assert_eq!(data.fields.get(&"from_index".to_string()), Some(&Value::Int(0)));
                        assert_eq!(data.fields.get(&"to_index".to_string()), Some(&Value::Int(1)));
                    }
                    other => panic!("expected StructureInstance, got {:?}", other),
                }
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn undef_on_wrong_arity() {
        assert!(tensegrity_wires(&[]).is_undef());
        let t = simple_tensegrity();
        assert!(tensegrity_wires(&[t.clone(), t]).is_undef());
    }

    #[test]
    fn undef_on_wrong_type() {
        assert!(tensegrity_wires(&[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn undef_on_out_of_range_index() {
        let nodes = Value::List(vec![node(0.0, 0.0, 0.0)]);
        let struts = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(5)])]);
        let cables = Value::List(vec![]);
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), nodes),
            ("struts".to_string(), struts),
            ("cables".to_string(), cables),
        ]
        .into_iter()
        .collect();
        let bad = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Tensegrity".to_string(),
            version: 1,
            fields,
        }));
        assert!(tensegrity_wires(&[bad]).is_undef());
    }

    // ── tensegrity_surfaces unit tests (step-7) ───────────────────────────────

    /// Build a Tensegrity with the given surfaces field value.
    fn tensegrity_with_surfaces(surfaces_val: Value) -> Value {
        let nodes = Value::List(vec![
            node(0.0, 0.0, 0.0),
            node(1.0, 0.0, 0.0),
            node(0.5, 1.0, 0.0),
        ]);
        let struts = Value::List(vec![]);
        let cables = Value::List(vec![]);
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), nodes),
            ("struts".to_string(), struts),
            ("cables".to_string(), cables),
            ("surfaces".to_string(), surfaces_val),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Tensegrity".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Build a Tensegrity WITHOUT a surfaces field (simulates existing call sites).
    fn tensegrity_no_surfaces() -> Value {
        let nodes = Value::List(vec![
            node(0.0, 0.0, 0.0),
            node(1.0, 0.0, 0.0),
            node(0.5, 1.0, 0.0),
        ]);
        let struts = Value::List(vec![]);
        let cables = Value::List(vec![]);
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), nodes),
            ("struts".to_string(), struts),
            ("cables".to_string(), cables),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Tensegrity".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Helper: a single triangle index-triple [[0, 1, 2]].
    fn one_triangle() -> Value {
        Value::List(vec![
            Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]),
        ])
    }

    /// tensegrity_surfaces(0 args) and tensegrity_surfaces(2 args) → Undef.
    #[test]
    fn surfaces_undef_on_wrong_arity() {
        assert!(tensegrity_surfaces(&[]).is_undef(), "0-arg should be Undef");
        let t = tensegrity_with_surfaces(one_triangle());
        assert!(
            tensegrity_surfaces(&[t.clone(), t]).is_undef(),
            "2-arg should be Undef"
        );
    }

    /// tensegrity_surfaces(Real(1.0)) and tensegrity_surfaces(wrong type_name) → Undef.
    #[test]
    fn surfaces_undef_on_wrong_type() {
        assert!(
            tensegrity_surfaces(&[Value::Real(1.0)]).is_undef(),
            "Real arg should be Undef"
        );
        // StructureInstance with wrong type_name
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), Value::List(vec![])),
            ("surfaces".to_string(), Value::List(vec![])),
        ]
        .into_iter()
        .collect();
        let wrong_type = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "NotATensegrity".to_string(),
            version: 1,
            fields,
        }));
        assert!(
            tensegrity_surfaces(&[wrong_type]).is_undef(),
            "wrong type_name should be Undef"
        );
    }

    /// surfaces=[[0,1,2]] with 3 nodes → 1 TensegritySurface with correct fields.
    #[test]
    fn surfaces_happy_path_single_triangle() {
        let t = tensegrity_with_surfaces(one_triangle());
        let result = tensegrity_surfaces(&[t]);
        match &result {
            Value::List(facets) => {
                assert_eq!(facets.len(), 1, "expected 1 facet, got {:?}", facets.len());
                match &facets[0] {
                    Value::StructureInstance(data) => {
                        assert_eq!(data.type_name, "TensegritySurface");
                        assert_eq!(
                            data.fields.get(&"kind".to_string()),
                            Some(&Value::String("membrane".to_string()))
                        );
                        assert_eq!(data.fields.get(&"i0".to_string()), Some(&Value::Int(0)));
                        assert_eq!(data.fields.get(&"i1".to_string()), Some(&Value::Int(1)));
                        assert_eq!(data.fields.get(&"i2".to_string()), Some(&Value::Int(2)));
                        // x0 == node(0.0, 0.0, 0.0).x == 0.0
                        match data.fields.get(&"x0".to_string()) {
                            Some(Value::Scalar { si_value, dimension }) => {
                                assert!((si_value - 0.0).abs() < 1e-12, "x0 should be 0.0m");
                                assert_eq!(*dimension, DimensionVector::LENGTH);
                            }
                            other => panic!("x0 should be Scalar, got {:?}", other),
                        }
                        // x1 == node(1.0, 0.0, 0.0).x == 1.0
                        match data.fields.get(&"x1".to_string()) {
                            Some(Value::Scalar { si_value, .. }) => {
                                assert!((si_value - 1.0).abs() < 1e-12, "x1 should be 1.0m");
                            }
                            other => panic!("x1 should be Scalar, got {:?}", other),
                        }
                        // x2 == node(0.5, 1.0, 0.0).x == 0.5
                        match data.fields.get(&"x2".to_string()) {
                            Some(Value::Scalar { si_value, .. }) => {
                                assert!((si_value - 0.5).abs() < 1e-12, "x2 should be 0.5m");
                            }
                            other => panic!("x2 should be Scalar, got {:?}", other),
                        }
                    }
                    other => panic!("expected StructureInstance, got {:?}", other),
                }
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    /// surfaces=[] (explicitly empty) → Value::List([]).
    #[test]
    fn surfaces_empty_surfaces_field_returns_empty_list() {
        let t = tensegrity_with_surfaces(Value::List(vec![]));
        let result = tensegrity_surfaces(&[t]);
        match &result {
            Value::List(facets) => assert_eq!(facets.len(), 0, "expected empty list"),
            other => panic!("expected List([]), got {:?}", other),
        }
    }

    /// surfaces field ABSENT (no surfaces key in fields) → Value::List([]).
    /// Design decision: missing surfaces is not a violation (legitimate for non-membrane nets).
    #[test]
    fn surfaces_missing_surfaces_field_returns_empty_list() {
        let t = tensegrity_no_surfaces();
        let result = tensegrity_surfaces(&[t]);
        match &result {
            Value::List(facets) => assert_eq!(
                facets.len(),
                0,
                "missing surfaces field should return empty list (not Undef)"
            ),
            other => panic!("expected List([]), got {:?}", other),
        }
    }

    /// surfaces=[[0, 1]] (inner list of length 2, not 3) → Undef.
    #[test]
    fn surfaces_undef_on_non_triple() {
        let pair = Value::List(vec![
            Value::List(vec![Value::Int(0), Value::Int(1)]), // only 2 indices
        ]);
        let t = tensegrity_with_surfaces(pair);
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "inner list of length 2 should be Undef (must be a triple)"
        );
    }

    /// surfaces=[[0, 1, 9]] with only 3 nodes (index 9 out of range) → Undef.
    #[test]
    fn surfaces_undef_on_out_of_range_index() {
        let oob = Value::List(vec![
            Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(9)]),
        ]);
        let t = tensegrity_with_surfaces(oob);
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "out-of-range index should be Undef"
        );
    }

    /// surfaces=[[-1, 0, 1]] (negative index) → Undef.
    #[test]
    fn surfaces_undef_on_negative_index() {
        let neg = Value::List(vec![
            Value::List(vec![Value::Int(-1), Value::Int(0), Value::Int(1)]),
        ]);
        let t = tensegrity_with_surfaces(neg);
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "negative index should be Undef"
        );
    }

    /// surfaces=[[Real(0.0), 1, 2]] (non-Int element in inner triple) → Undef.
    /// Exercises the `_ => return None` arm of `extract_index_tuples` for
    /// non-Int inner elements (previously untested branch).
    #[test]
    fn surfaces_undef_on_non_int_triple_element() {
        let bad = Value::List(vec![
            Value::List(vec![Value::Real(0.0), Value::Int(1), Value::Int(2)]),
        ]);
        let t = tensegrity_with_surfaces(bad);
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "non-Int triple element (Real) should be Undef"
        );
    }

    /// surfaces=Value::Int(5) (surfaces field is not a List) → Undef.
    /// Exercises the outer `_ => None` arm of `extract_index_tuples` for
    /// non-List surfaces values (previously untested branch).
    #[test]
    fn surfaces_undef_on_surfaces_not_a_list() {
        let t = tensegrity_with_surfaces(Value::Int(5));
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "surfaces=Int(5) (not a List) should be Undef"
        );
    }

    /// Tensegrity with a malformed node (2-component Point, not 3) and a
    /// non-empty surfaces field → Undef. Exercises the `extract_nodes` path
    /// for non-3-component Points via `tensegrity_surfaces` (previously
    /// only covered for `tensegrity_wires`).
    #[test]
    fn surfaces_undef_on_malformed_node() {
        // 2-component Point — extract_node_xyz returns None → extract_nodes → None → Undef.
        let bad_node = Value::Point(vec![length(0.0), length(0.0)]); // only 2 components
        let nodes = Value::List(vec![
            bad_node,
            node(1.0, 0.0, 0.0),
            node(0.5, 1.0, 0.0),
        ]);
        let fields: PersistentMap<String, Value> = [
            ("nodes".to_string(), nodes),
            ("struts".to_string(), Value::List(vec![])),
            ("cables".to_string(), Value::List(vec![])),
            ("surfaces".to_string(), one_triangle()), // non-empty so nodes are extracted
        ]
        .into_iter()
        .collect();
        let t = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "Tensegrity".to_string(),
            version: 1,
            fields,
        }));
        assert!(
            tensegrity_surfaces(&[t]).is_undef(),
            "malformed node (2-component Point) should be Undef"
        );
    }
}
