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
use reify_stdlib::eval_builtin;

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

// ── step-5: shape-guard tests for tensegrity_wires ───────────────────────────

// Shared helpers for building Tensegrity-shaped Values directly (bypassing
// the compile pipeline) so these tests are purely unit-level.

fn make_length(meters: f64) -> Value {
    Value::Scalar { si_value: meters, dimension: DimensionVector::LENGTH }
}

fn make_node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![make_length(x), make_length(y), make_length(z)])
}

/// Build a valid 4-node 1-strut 1-cable Tensegrity StructureInstance.
/// Used as the positive-shape control in each shape-guard test.
fn make_valid_tensegrity() -> Value {
    let nodes = Value::List(vec![
        make_node(0.0, 0.0, 0.0),
        make_node(1.0, 0.0, 0.0),
        make_node(0.5, 0.866, 0.0),
        make_node(0.5, 0.289, 0.816),
    ]);
    let struts = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(3)]),
    ]);
    let cables = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(1)]),
    ]);
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

/// Zero args → Undef. Positive control: valid Tensegrity → non-Undef.
/// RED state: the positive control fails because `tensegrity_wires` is not
/// yet recognized by eval_builtin.
#[test]
fn tensegrity_wires_undef_on_zero_args() {
    let result = eval_builtin("tensegrity_wires", &[]);
    assert!(result.is_undef(), "zero args should return Undef, got {:?}", result);

    // Positive control: the function IS recognized and a valid Tensegrity
    // returns a non-Undef list. Fails RED until step-6 registers the builtin.
    let valid = make_valid_tensegrity();
    let positive = eval_builtin("tensegrity_wires", &[valid]);
    assert!(
        !positive.is_undef(),
        "tensegrity_wires(valid Tensegrity) should return non-Undef; \
         got Undef — step-6 not yet implemented"
    );
}

/// Two args → Undef. Positive control: valid Tensegrity → non-Undef.
#[test]
fn tensegrity_wires_undef_on_two_args() {
    let result = eval_builtin("tensegrity_wires", &[Value::Real(1.0), Value::Real(2.0)]);
    assert!(result.is_undef(), "two args should return Undef, got {:?}", result);

    let valid = make_valid_tensegrity();
    let positive = eval_builtin("tensegrity_wires", &[valid]);
    assert!(
        !positive.is_undef(),
        "tensegrity_wires(valid Tensegrity) should return non-Undef; \
         got Undef — step-6 not yet implemented"
    );
}

/// args[0] is Real, not StructureInstance → Undef.
#[test]
fn tensegrity_wires_undef_on_real_arg() {
    let result = eval_builtin("tensegrity_wires", &[Value::Real(1.0)]);
    assert!(result.is_undef(), "Real arg should return Undef, got {:?}", result);

    let valid = make_valid_tensegrity();
    let positive = eval_builtin("tensegrity_wires", &[valid]);
    assert!(
        !positive.is_undef(),
        "tensegrity_wires(valid Tensegrity) should return non-Undef; \
         got Undef — step-6 not yet implemented"
    );
}

/// args[0] is a StructureInstance with wrong type_name → Undef.
#[test]
fn tensegrity_wires_undef_on_wrong_type_name() {
    let fields: PersistentMap<String, Value> = PersistentMap::new();
    let wrong = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields,
    }));
    let result = eval_builtin("tensegrity_wires", &[wrong]);
    assert!(result.is_undef(), "wrong type_name should return Undef, got {:?}", result);

    let valid = make_valid_tensegrity();
    let positive = eval_builtin("tensegrity_wires", &[valid]);
    assert!(
        !positive.is_undef(),
        "tensegrity_wires(valid Tensegrity) should return non-Undef; \
         got Undef — step-6 not yet implemented"
    );
}

// ── step-7: full-shape tensegrity_wires test ──────────────────────────────────

/// T-prism: 6 nodes, 3 struts, 3 cables → 6 TensegrityWire values.
/// Verifies:
///   - result is Value::List of exactly 6 elements
///   - elements [0..3] have kind="strut", [3..6] have kind="cable"
///   - from_index/to_index match the supplied pairs
///   - x1/y1/z1/x2/y2/z2 match the corresponding node coordinates
///
/// Also pins declaration order: struts precede cables (DD2 open-groups seam).
#[test]
fn tensegrity_wires_emits_six_tagged_wires() {
    // 6-node T-prism: bottom triangle at z=0m, top triangle at z=1m.
    // Canonical twist: top triangle rotated 60° relative to bottom.
    let nodes = vec![
        // bottom triangle
        make_node(1.0,   0.0,   0.0),  // node 0
        make_node(-0.5,  0.866, 0.0),  // node 1
        make_node(-0.5, -0.866, 0.0),  // node 2
        // top triangle (60° rotated, z=1m)
        make_node(0.0,   1.0,   1.0),  // node 3
        make_node(-0.866, -0.5, 1.0),  // node 4
        make_node(0.866, -0.5, 1.0),   // node 5
    ];
    // 3 struts: cross-members connecting bottom to top
    let strut_pairs = [(0usize, 3usize), (1, 4), (2, 5)];
    // 3 cables: top triangle perimeter
    let cable_pairs = [(3usize, 4usize), (4, 5), (5, 3)];

    let struts = Value::List(
        strut_pairs
            .iter()
            .map(|(f, t)| Value::List(vec![Value::Int(*f as i64), Value::Int(*t as i64)]))
            .collect(),
    );
    let cables = Value::List(
        cable_pairs
            .iter()
            .map(|(f, t)| Value::List(vec![Value::Int(*f as i64), Value::Int(*t as i64)]))
            .collect(),
    );
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(),  Value::List(nodes.clone())),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
    ]
    .into_iter()
    .collect();
    let tensegrity = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }));

    let result = eval_builtin("tensegrity_wires", &[tensegrity]);

    let wires = match &result {
        Value::List(w) => w,
        other => panic!("expected Value::List of wires, got {:?}", other),
    };
    assert_eq!(wires.len(), 6, "T-prism should have 6 wires (3 struts + 3 cables)");

    // First 3: struts
    for (i, (from, to)) in strut_pairs.iter().enumerate() {
        let wire = match &wires[i] {
            Value::StructureInstance(data) => data,
            other => panic!("wire[{}] should be StructureInstance, got {:?}", i, other),
        };
        assert_eq!(wire.type_name, "TensegrityWire", "wire[{}] type_name", i);
        assert_eq!(
            wire.fields.get(&"kind".to_string()),
            Some(&Value::String("strut".to_string())),
            "wire[{}] kind should be 'strut'", i
        );
        assert_eq!(
            wire.fields.get(&"from_index".to_string()),
            Some(&Value::Int(*from as i64)),
            "wire[{}] from_index", i
        );
        assert_eq!(
            wire.fields.get(&"to_index".to_string()),
            Some(&Value::Int(*to as i64)),
            "wire[{}] to_index", i
        );
        // Verify x1/y1/z1 match nodes[from] components
        let expected_from = match &nodes[*from] {
            Value::Point(comps) => comps.clone(),
            other => panic!("nodes[{}] should be Point, got {:?}", from, other),
        };
        assert_eq!(wire.fields.get(&"x1".to_string()), Some(&expected_from[0]), "wire[{}] x1", i);
        assert_eq!(wire.fields.get(&"y1".to_string()), Some(&expected_from[1]), "wire[{}] y1", i);
        assert_eq!(wire.fields.get(&"z1".to_string()), Some(&expected_from[2]), "wire[{}] z1", i);
        let expected_to = match &nodes[*to] {
            Value::Point(comps) => comps.clone(),
            other => panic!("nodes[{}] should be Point, got {:?}", to, other),
        };
        assert_eq!(wire.fields.get(&"x2".to_string()), Some(&expected_to[0]), "wire[{}] x2", i);
        assert_eq!(wire.fields.get(&"y2".to_string()), Some(&expected_to[1]), "wire[{}] y2", i);
        assert_eq!(wire.fields.get(&"z2".to_string()), Some(&expected_to[2]), "wire[{}] z2", i);
    }

    // Last 3: cables
    for (i, (from, to)) in cable_pairs.iter().enumerate() {
        let idx = i + 3;
        let wire = match &wires[idx] {
            Value::StructureInstance(data) => data,
            other => panic!("wire[{}] should be StructureInstance, got {:?}", idx, other),
        };
        assert_eq!(wire.type_name, "TensegrityWire");
        assert_eq!(
            wire.fields.get(&"kind".to_string()),
            Some(&Value::String("cable".to_string())),
            "wire[{}] kind should be 'cable'", idx
        );
        assert_eq!(
            wire.fields.get(&"from_index".to_string()),
            Some(&Value::Int(*from as i64)),
            "wire[{}] from_index", idx
        );
        assert_eq!(
            wire.fields.get(&"to_index".to_string()),
            Some(&Value::Int(*to as i64)),
            "wire[{}] to_index", idx
        );
    }
}

/// Pins that struts precede cables in the output list (open-groups seam DD2).
#[test]
fn tensegrity_wires_preserves_declaration_order_struts_then_cables() {
    let nodes = Value::List(vec![
        make_node(0.0, 0.0, 0.0),
        make_node(1.0, 0.0, 0.0),
        make_node(0.0, 1.0, 0.0),
    ]);
    let struts = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(1)])]);
    let cables = Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(2)])]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(),  nodes),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
    ]
    .into_iter()
    .collect();
    let t = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }));

    let result = eval_builtin("tensegrity_wires", &[t]);
    let wires = match &result {
        Value::List(w) => w,
        other => panic!("expected List, got {:?}", other),
    };
    assert_eq!(wires.len(), 2);

    // First wire must be the strut
    let w0 = match &wires[0] {
        Value::StructureInstance(d) => d,
        other => panic!("wires[0] should be StructureInstance, got {:?}", other),
    };
    assert_eq!(w0.fields.get(&"kind".to_string()), Some(&Value::String("strut".to_string())));

    // Second wire must be the cable
    let w1 = match &wires[1] {
        Value::StructureInstance(d) => d,
        other => panic!("wires[1] should be StructureInstance, got {:?}", other),
    };
    assert_eq!(w1.fields.get(&"kind".to_string()), Some(&Value::String("cable".to_string())));
}

// ── step-9: CLI golden test ───────────────────────────────────────────────────

/// `reify eval examples/tensegrity_t_prism.ri` must print the T-prism instance
/// and 6 tagged TensegrityWire values. Output compared against the committed
/// golden at `crates/reify-eval/tests/golden/tensegrity_t_prism.txt`.
/// Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
///
/// RED state: `examples/tensegrity_t_prism.ri` and the golden don't exist yet,
/// so `cargo run` either fails to read the example or the golden read panics.
#[test]
fn cli_reify_eval_prints_t_prism_wireframe() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/tensegrity_t_prism.ri");
    let golden = std::path::Path::new(manifest).join("tests/golden/tensegrity_t_prism.txt");

    let output = std::process::Command::new(env!("CARGO"))
        .current_dir(&workspace_root)
        .args(["run", "-q", "-p", "reify-cli", "--bin", "reify", "--", "eval"])
        .arg(&example)
        .output()
        .expect("failed to spawn `cargo run -p reify-cli -- eval`");

    assert!(
        output.status.success(),
        "`reify eval examples/tensegrity_t_prism.ri` exited non-zero.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");

    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden, &stdout).expect("failed to write golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
        panic!(
            "golden crates/reify-eval/tests/golden/tensegrity_t_prism.txt missing; \
             run once with REIFY_REGENERATE_GOLDEN=1"
        )
    });
    assert_eq!(
        stdout, expected,
        "`reify eval examples/tensegrity_t_prism.ri` stdout drifted from the golden; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update"
    );

    // Defense-in-depth: pins the T0a signal independent of golden content.
    // Fields are sorted alphabetically in the output, so `from_index` precedes
    // `kind`. We match on `kind: "strut"` / `kind: "cable"` substrings.
    assert!(
        stdout.contains("kind: \"strut\""),
        "T0a signal: expected at least one TensegrityWire with kind=\"strut\"; got:\n{stdout}"
    );
    assert!(
        stdout.contains("kind: \"cable\""),
        "T0a signal: expected at least one TensegrityWire with kind=\"cable\"; got:\n{stdout}"
    );
}

/// args[0] is Tensegrity-shaped but struts references out-of-range index → Undef.
#[test]
fn tensegrity_wires_undef_on_out_of_range_index() {
    // 2 nodes but struts references node index 5 (out of range).
    let nodes = Value::List(vec![
        make_node(0.0, 0.0, 0.0),
        make_node(1.0, 0.0, 0.0),
    ]);
    let struts = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(5)]), // index 5 >= nodes.len()=2
    ]);
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
    let result = eval_builtin("tensegrity_wires", &[bad]);
    assert!(
        result.is_undef(),
        "out-of-range strut index should return Undef, got {:?}",
        result
    );

    let valid = make_valid_tensegrity();
    let positive = eval_builtin("tensegrity_wires", &[valid]);
    assert!(
        !positive.is_undef(),
        "tensegrity_wires(valid Tensegrity) should return non-Undef; \
         got Undef — step-6 not yet implemented"
    );
}
