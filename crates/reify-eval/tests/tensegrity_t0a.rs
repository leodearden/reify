//! T0a tests: Strut/Cable/Tensegrity/TensegrityWire ctor + tensegrity_wires.
//!
//! Covers:
//!   step-3: SIR-α ctor boundary tests (Strut, Cable, Tensegrity evaluate to
//!           Value::StructureInstance via the existing ctor-lowering path)
//!   step-5: Shape-guard tests for tensegrity_wires (Undef for bad inputs)
//!   step-7: Full-shape tensegrity_wires test (6-wire T-prism output)
//!   step-9: CLI golden test (cli_reify_eval_prints_t_prism_wireframe)

#![allow(clippy::mutable_key_type)]

use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── step-3: SIR-α ctor boundary tests ────────────────────────────────────────

/// `Strut(section_area: ..., material: Steel_AISI_1045())` evaluates to a
/// `Value::StructureInstance` with type_name "Strut", and the `section_area`
/// field is an Area-dimensioned Scalar, `material` is a nested StructureInstance.
#[test]
fn strut_ctor_evaluates_to_structure_instance() {
    const SOURCE: &str = r#"
structure def F {
    let s = Strut(
        section_area: 100mm * 1mm,
        material: Steel_AISI_1045()
    )
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("F", "s");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("F.s cell missing from eval result"));

    match v {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Strut", "type_name should be Strut, got {:?}", data.type_name);

            // section_area: Area-dimensioned scalar, non-Undef
            let sa = field(&data.fields, "section_area")
                .unwrap_or_else(|| panic!("Strut missing section_area field; fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()));
            match sa {
                Value::Scalar { dimension, .. } => assert_eq!(
                    *dimension,
                    DimensionVector::AREA,
                    "Strut.section_area should have AREA dimension, got {:?}", dimension
                ),
                other => panic!("Strut.section_area should be Scalar, got {:?}", other),
            }

            // material: nested StructureInstance (Steel_AISI_1045)
            let mat = field(&data.fields, "material")
                .unwrap_or_else(|| panic!("Strut missing material field"));
            match mat {
                Value::StructureInstance(mdata) => assert_eq!(
                    mdata.type_name, "Steel_AISI_1045",
                    "Strut.material should be Steel_AISI_1045, got {:?}", mdata.type_name
                ),
                other => panic!("Strut.material should be StructureInstance, got {:?}", other),
            }
        }
        other => panic!("expected Value::StructureInstance for F.s, got {:?}", other),
    }
}

/// `Cable(section_area: ..., material: Steel_AISI_1045())` evaluates to a
/// `Value::StructureInstance` with type_name "Cable". The `pretension` field
/// carries the 0N default (Force-dimensioned Scalar with si_value ~0.0).
#[test]
fn cable_ctor_evaluates_to_structure_instance_with_pretension_default() {
    const SOURCE: &str = r#"
structure def F {
    let c = Cable(
        section_area: 50mm * 1mm,
        material: Steel_AISI_1045()
    )
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("F", "c");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("F.c cell missing from eval result"));

    match v {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Cable", "type_name should be Cable, got {:?}", data.type_name);

            // pretension defaults to 0N (Force dimension, si_value ≈ 0.0)
            let pt = field(&data.fields, "pretension")
                .unwrap_or_else(|| panic!("Cable missing pretension field; fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()));
            match pt {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::FORCE,
                        "Cable.pretension should have FORCE dimension, got {:?}", dimension
                    );
                    assert!(
                        si_value.abs() < 1e-9,
                        "Cable.pretension default should be 0N (si_value ≈ 0), got {}", si_value
                    );
                }
                other => panic!("Cable.pretension should be Scalar(Force), got {:?}", other),
            }
        }
        other => panic!("expected Value::StructureInstance for F.c, got {:?}", other),
    }
}

/// A 4-node 1-strut 1-cable `Tensegrity(...)` evaluates to
/// `Value::StructureInstance` with type_name "Tensegrity", `nodes` is
/// `Value::List` of 4 `Value::Point` values, `struts` and `cables` are
/// `Value::List` of `Value::List` of `Value::Int` index pairs.
#[test]
fn tensegrity_ctor_carries_node_and_index_lists() {
    const SOURCE: &str = r#"
structure def TNet {
    let t = Tensegrity(
        nodes: [
            point3(0m, 0m, 0m),
            point3(1m, 0m, 0m),
            point3(0.5m, 1m, 0m),
            point3(0.5m, 0.5m, 1m)
        ],
        struts: [[0, 3]],
        cables: [[0, 1]]
    )
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("TNet", "t");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("TNet.t cell missing from eval result"));

    match v {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Tensegrity");

            // nodes: List of 4 Point values
            let nodes = field(&data.fields, "nodes")
                .unwrap_or_else(|| panic!("Tensegrity missing nodes field"));
            match nodes {
                Value::List(items) => {
                    assert_eq!(items.len(), 4, "Tensegrity.nodes should have 4 points, got {}", items.len());
                    for (i, item) in items.iter().enumerate() {
                        assert!(
                            matches!(item, Value::Point(_)),
                            "nodes[{}] should be Value::Point, got {:?}", i, item
                        );
                    }
                }
                other => panic!("Tensegrity.nodes should be Value::List, got {:?}", other),
            }

            // struts: List of List of Int
            let struts = field(&data.fields, "struts")
                .unwrap_or_else(|| panic!("Tensegrity missing struts field"));
            match struts {
                Value::List(pairs) => {
                    assert_eq!(pairs.len(), 1, "struts should have 1 pair, got {}", pairs.len());
                    match &pairs[0] {
                        Value::List(indices) => {
                            assert_eq!(indices.len(), 2, "strut pair should have 2 indices");
                            assert!(matches!(indices[0], Value::Int(_)));
                            assert!(matches!(indices[1], Value::Int(_)));
                        }
                        other => panic!("struts[0] should be Value::List, got {:?}", other),
                    }
                }
                other => panic!("Tensegrity.struts should be Value::List, got {:?}", other),
            }

            // cables: List of List of Int
            let cables = field(&data.fields, "cables")
                .unwrap_or_else(|| panic!("Tensegrity missing cables field"));
            match cables {
                Value::List(pairs) => {
                    assert_eq!(pairs.len(), 1, "cables should have 1 pair, got {}", pairs.len());
                    match &pairs[0] {
                        Value::List(indices) => {
                            assert_eq!(indices.len(), 2, "cable pair should have 2 indices");
                            assert!(matches!(indices[0], Value::Int(_)));
                            assert!(matches!(indices[1], Value::Int(_)));
                        }
                        other => panic!("cables[0] should be Value::List, got {:?}", other),
                    }
                }
                other => panic!("Tensegrity.cables should be Value::List, got {:?}", other),
            }
        }
        other => panic!("expected Value::StructureInstance for TNet.t, got {:?}", other),
    }
}
