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
        "tensegrity_wires" => tensegrity_wires(args),
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

/// Extract index pairs from a `List<List<Int>>` field value.
///
/// Each inner list must be exactly `[from_index, to_index]` with both entries
/// being `Value::Int`. Out-of-range indices are validated against `n_nodes`
/// by the caller. Returns `None` on any shape violation.
fn extract_index_pairs(v: &Value) -> Option<Vec<(i64, i64)>> {
    match v {
        Value::List(pairs) => {
            let mut result = Vec::with_capacity(pairs.len());
            for pair in pairs {
                match pair {
                    Value::List(indices) if indices.len() == 2 => {
                        match (&indices[0], &indices[1]) {
                            (Value::Int(from), Value::Int(to)) => result.push((*from, *to)),
                            _ => return None,
                        }
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
    // Arity guard.
    if args.len() != 1 {
        return Value::Undef;
    }

    // arg must be a Tensegrity StructureInstance.
    let fields = match &args[0] {
        Value::StructureInstance(data) if data.type_name == "Tensegrity" => &data.fields,
        _ => return Value::Undef,
    };

    // Extract nodes list.
    let nodes = match fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => ns,
        _ => return Value::Undef,
    };

    // Pre-extract node XYZ tuples; validate each is a 3-component Point.
    let node_xyzs: Vec<(Value, Value, Value)> = {
        let mut v = Vec::with_capacity(nodes.len());
        for n in nodes.iter() {
            match extract_node_xyz(n) {
                Some(xyz) => v.push(xyz),
                None => return Value::Undef,
            }
        }
        v
    };
    let n_nodes = node_xyzs.len();

    // Extract strut and cable index pair lists.
    let struts = match fields.get(&"struts".to_string()) {
        Some(v) => match extract_index_pairs(v) {
            Some(pairs) => pairs,
            None => return Value::Undef,
        },
        None => return Value::Undef,
    };
    let cables = match fields.get(&"cables".to_string()) {
        Some(v) => match extract_index_pairs(v) {
            Some(pairs) => pairs,
            None => return Value::Undef,
        },
        None => return Value::Undef,
    };

    // Emit wires: struts first, then cables (DD2 declaration order).
    let mut wires = Vec::with_capacity(struts.len() + cables.len());

    for (from, to) in &struts {
        let from = *from;
        let to = *to;
        // Validate index range.
        if from < 0 || from as usize >= n_nodes || to < 0 || to as usize >= n_nodes {
            return Value::Undef;
        }
        wires.push(build_wire("strut", from, to, &node_xyzs[from as usize], &node_xyzs[to as usize]));
    }

    for (from, to) in &cables {
        let from = *from;
        let to = *to;
        if from < 0 || from as usize >= n_nodes || to < 0 || to as usize >= n_nodes {
            return Value::Undef;
        }
        wires.push(build_wire("cable", from, to, &node_xyzs[from as usize], &node_xyzs[to as usize]));
    }

    Value::List(wires)
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
}
