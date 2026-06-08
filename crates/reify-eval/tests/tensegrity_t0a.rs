//! T0a tests: Strut/Cable/Tensegrity/TensegrityWire ctor + tensegrity_wires.
//!
//! Covers:
//!   step-3: SIR-α ctor boundary tests (Strut, Cable, Tensegrity evaluate to
//!           Value::StructureInstance via the existing ctor-lowering path)
//!   step-5: Shape-guard tests for tensegrity_wires (Undef for bad inputs)
//!   step-7: Full-shape tensegrity_wires test (6-wire T-prism output)
//!   step-9: CLI golden test (cli_reify_eval_prints_t_prism_wireframe)

#![allow(clippy::mutable_key_type)]

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::eval_builtin;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

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
            assert_eq!(
                data.type_name, "Strut",
                "type_name should be Strut, got {:?}",
                data.type_name
            );

            // section_area: Area-dimensioned scalar, non-Undef
            let sa = field(&data.fields, "section_area").unwrap_or_else(|| {
                panic!(
                    "Strut missing section_area field; fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                )
            });
            match sa {
                Value::Scalar { dimension, .. } => assert_eq!(
                    *dimension,
                    DimensionVector::AREA,
                    "Strut.section_area should have AREA dimension, got {:?}",
                    dimension
                ),
                other => panic!("Strut.section_area should be Scalar, got {:?}", other),
            }

            // material: nested StructureInstance (Steel_AISI_1045)
            let mat = field(&data.fields, "material")
                .unwrap_or_else(|| panic!("Strut missing material field"));
            match mat {
                Value::StructureInstance(mdata) => assert_eq!(
                    mdata.type_name, "Steel_AISI_1045",
                    "Strut.material should be Steel_AISI_1045, got {:?}",
                    mdata.type_name
                ),
                other => panic!(
                    "Strut.material should be StructureInstance, got {:?}",
                    other
                ),
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
            assert_eq!(
                data.type_name, "Cable",
                "type_name should be Cable, got {:?}",
                data.type_name
            );

            // pretension defaults to 0N (Force dimension, si_value ≈ 0.0)
            let pt = field(&data.fields, "pretension").unwrap_or_else(|| {
                panic!(
                    "Cable missing pretension field; fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                )
            });
            match pt {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::FORCE,
                        "Cable.pretension should have FORCE dimension, got {:?}",
                        dimension
                    );
                    assert!(
                        si_value.abs() < 1e-9,
                        "Cable.pretension default should be 0N (si_value ≈ 0), got {}",
                        si_value
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
                    assert_eq!(
                        items.len(),
                        4,
                        "Tensegrity.nodes should have 4 points, got {}",
                        items.len()
                    );
                    for (i, item) in items.iter().enumerate() {
                        assert!(
                            matches!(item, Value::Point(_)),
                            "nodes[{}] should be Value::Point, got {:?}",
                            i,
                            item
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
                    assert_eq!(
                        pairs.len(),
                        1,
                        "struts should have 1 pair, got {}",
                        pairs.len()
                    );
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
                    assert_eq!(
                        pairs.len(),
                        1,
                        "cables should have 1 pair, got {}",
                        pairs.len()
                    );
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
        other => panic!(
            "expected Value::StructureInstance for TNet.t, got {:?}",
            other
        ),
    }
}

// ── step-5: shape-guard tests for tensegrity_wires ───────────────────────────

// Shared helpers for building Tensegrity-shaped Values directly (bypassing
// the compile pipeline) so these tests are purely unit-level.

fn make_length(meters: f64) -> Value {
    Value::Scalar {
        si_value: meters,
        dimension: DimensionVector::LENGTH,
    }
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
    let struts = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(3)])]);
    let cables = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(1)])]);
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
    assert!(
        result.is_undef(),
        "zero args should return Undef, got {:?}",
        result
    );

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
    assert!(
        result.is_undef(),
        "two args should return Undef, got {:?}",
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

/// args[0] is Real, not StructureInstance → Undef.
#[test]
fn tensegrity_wires_undef_on_real_arg() {
    let result = eval_builtin("tensegrity_wires", &[Value::Real(1.0)]);
    assert!(
        result.is_undef(),
        "Real arg should return Undef, got {:?}",
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
    assert!(
        result.is_undef(),
        "wrong type_name should return Undef, got {:?}",
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
        make_node(1.0, 0.0, 0.0),     // node 0
        make_node(-0.5, 0.866, 0.0),  // node 1
        make_node(-0.5, -0.866, 0.0), // node 2
        // top triangle (60° rotated, z=1m)
        make_node(0.0, 1.0, 1.0),     // node 3
        make_node(-0.866, -0.5, 1.0), // node 4
        make_node(0.866, -0.5, 1.0),  // node 5
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
        ("nodes".to_string(), Value::List(nodes.clone())),
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
    assert_eq!(
        wires.len(),
        6,
        "T-prism should have 6 wires (3 struts + 3 cables)"
    );

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
            "wire[{}] kind should be 'strut'",
            i
        );
        assert_eq!(
            wire.fields.get(&"from_index".to_string()),
            Some(&Value::Int(*from as i64)),
            "wire[{}] from_index",
            i
        );
        assert_eq!(
            wire.fields.get(&"to_index".to_string()),
            Some(&Value::Int(*to as i64)),
            "wire[{}] to_index",
            i
        );
        // Verify x1/y1/z1 match nodes[from] components
        let expected_from = match &nodes[*from] {
            Value::Point(comps) => comps.clone(),
            other => panic!("nodes[{}] should be Point, got {:?}", from, other),
        };
        assert_eq!(
            wire.fields.get(&"x1".to_string()),
            Some(&expected_from[0]),
            "wire[{}] x1",
            i
        );
        assert_eq!(
            wire.fields.get(&"y1".to_string()),
            Some(&expected_from[1]),
            "wire[{}] y1",
            i
        );
        assert_eq!(
            wire.fields.get(&"z1".to_string()),
            Some(&expected_from[2]),
            "wire[{}] z1",
            i
        );
        let expected_to = match &nodes[*to] {
            Value::Point(comps) => comps.clone(),
            other => panic!("nodes[{}] should be Point, got {:?}", to, other),
        };
        assert_eq!(
            wire.fields.get(&"x2".to_string()),
            Some(&expected_to[0]),
            "wire[{}] x2",
            i
        );
        assert_eq!(
            wire.fields.get(&"y2".to_string()),
            Some(&expected_to[1]),
            "wire[{}] y2",
            i
        );
        assert_eq!(
            wire.fields.get(&"z2".to_string()),
            Some(&expected_to[2]),
            "wire[{}] z2",
            i
        );
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
            "wire[{}] kind should be 'cable'",
            idx
        );
        assert_eq!(
            wire.fields.get(&"from_index".to_string()),
            Some(&Value::Int(*from as i64)),
            "wire[{}] from_index",
            idx
        );
        assert_eq!(
            wire.fields.get(&"to_index".to_string()),
            Some(&Value::Int(*to as i64)),
            "wire[{}] to_index",
            idx
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
        ("nodes".to_string(), nodes),
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
    assert_eq!(
        w0.fields.get(&"kind".to_string()),
        Some(&Value::String("strut".to_string()))
    );

    // Second wire must be the cable
    let w1 = match &wires[1] {
        Value::StructureInstance(d) => d,
        other => panic!("wires[1] should be StructureInstance, got {:?}", other),
    };
    assert_eq!(
        w1.fields.get(&"kind".to_string()),
        Some(&Value::String("cable".to_string()))
    );
}

// ── step-9: CLI golden test ───────────────────────────────────────────────────

/// `reify eval examples/tensegrity_t_prism.ri` must print the T-prism instance
/// and 6 tagged TensegrityWire values. Output compared against the committed
/// golden at `crates/reify-eval/tests/golden/tensegrity_t_prism.txt`.
/// Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
///
/// `CARGO_BIN_EXE_reify` is only injected for `reify-cli`'s own integration
/// tests, so this cross-crate test execs the pre-built `reify` binary
/// directly. It deliberately does NOT use `cargo run`: even when the binary
/// is already compiled, `cargo run` re-fingerprints the entire workspace and
/// blocks on the global cargo build-lock before exec, and under concurrent
/// multi-worktree verify load that overhead can push the test past its time
/// budget (esc-4340-32, exit 124). The merge gate's debug `--workspace` pass
/// builds all `[[bin]]` targets (including `reify`) at `target/debug/reify`;
/// its release pass is scoped to release-sensitive crates and does NOT rebuild
/// `reify-cli`, so the resolution below prefers the profile-local bin and falls
/// back to the debug-profile one when it is absent. The cargo runner
/// (`.cargo/run-with-occt.sh`) exports `LD_LIBRARY_PATH` into this test
/// process's environment, which the spawned child inherits, so OCCT shared
/// libraries resolve without going through cargo.
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

    // Resolve the prebuilt `reify` binary from this test binary's own location.
    // The integration-test binary lives at `…/target/<profile>/deps/<testbin>`,
    // so its grandparent is `…/target/<profile>` and the `reify` bin sits beside
    // it at `…/target/<profile>/reify`.
    //
    // Cross-task seam (task/4390 HAS LANDED): the merge gate's RELEASE pass
    // (verify.sh, DF_VERIFY_ROLE=merge --profile both) is scoped to
    // release-sensitive crates and deliberately does NOT build `reify-cli`, so
    // `target/release/reify` is absent during the release test pass. The
    // preceding DEBUG pass runs the full `--workspace` (building
    // `target/debug/reify`), and the reify CLI's golden output is
    // profile-independent (the release pass exists to re-check reify-eval's own
    // overflow/debug-assert behaviour, not the spawned CLI). So prefer the
    // profile-local bin but fall back to the debug-profile sibling when it is
    // absent. (Per-task verifies are unaffected: a reify-eval change pulls
    // `reify-cli` into the affected set as a reverse-dep, so the debug bin is
    // built.)
    let test_bin = std::env::current_exe().expect("current_exe");
    let profile_dir = test_bin
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary lives in target/<profile>/deps");
    let profile_local = profile_dir.join("reify");
    let reify_bin = if profile_local.exists() {
        profile_local
    } else {
        // Release pass: target/release/reify is absent (reify-cli not built);
        // fall back to the debug-profile bin the debug pass built.
        profile_dir
            .parent()
            .map(|target_dir| target_dir.join("debug").join("reify"))
            .filter(|p| p.exists())
            .unwrap_or(profile_local)
    };

    let output = std::process::Command::new(&reify_bin)
        .current_dir(&workspace_root)
        .arg("eval")
        .arg(&example)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; is it built? \
                 The gated verify pass builds it when it compiles `reify-cli` \
                 (`cargo test -p reify-cli`, or the merge gate's debug \
                 `--workspace` pass that builds all `[[bin]]` targets). Note: an \
                 ad-hoc `cargo test -p reify-eval` alone does NOT build the \
                 `reify` bin.",
                reify_bin.display()
            )
        });

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

// ── step-3 (task-4412): Membrane ctor eval test ───────────────────────────────

/// `Membrane(thickness: 2mm, material: Steel_AISI_1045())` evaluates to a
/// `Value::StructureInstance` with type_name "Membrane". The `prestress` field
/// carries the 0*1Pa default (Pressure-dimensioned Scalar with si_value ~0.0).
///
/// RED (step-3): fails until `structure def Membrane` is added to tensegrity.ri
/// in step-4. After step-4 the SIR ctor-lowering path handles it automatically.
#[test]
fn membrane_ctor_evaluates_to_structure_instance_with_prestress_default() {
    const SOURCE: &str = r#"
structure def F {
    let m = Membrane(thickness: 2mm, material: Steel_AISI_1045())
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("F", "m");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("F.m cell missing from eval result"));

    match v {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Membrane",
                "type_name should be Membrane, got {:?}",
                data.type_name
            );

            // thickness: Length-dimensioned scalar
            let th = field(&data.fields, "thickness").unwrap_or_else(|| {
                panic!(
                    "Membrane missing thickness field; fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                )
            });
            match th {
                Value::Scalar { dimension, .. } => assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "Membrane.thickness should have LENGTH dimension, got {:?}",
                    dimension
                ),
                other => panic!("Membrane.thickness should be Scalar, got {:?}", other),
            }

            // material: nested StructureInstance (Steel_AISI_1045)
            let mat = field(&data.fields, "material")
                .unwrap_or_else(|| panic!("Membrane missing material field"));
            match mat {
                Value::StructureInstance(mdata) => assert_eq!(
                    mdata.type_name, "Steel_AISI_1045",
                    "Membrane.material should be Steel_AISI_1045, got {:?}",
                    mdata.type_name
                ),
                other => panic!(
                    "Membrane.material should be StructureInstance, got {:?}",
                    other
                ),
            }

            // prestress: defaults to 0*1Pa → Pressure-dimensioned Scalar, si_value ~0.0
            let ps = field(&data.fields, "prestress").unwrap_or_else(|| {
                panic!(
                    "Membrane missing prestress field (should be filled by 0*1Pa default); \
                     fields: {:?}",
                    data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
                )
            });
            match ps {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::PRESSURE,
                        "Membrane.prestress should have PRESSURE dimension, got {:?}",
                        dimension
                    );
                    assert!(
                        si_value.abs() < 1e-10,
                        "Membrane.prestress default should be ~0 Pa, got si_value={}",
                        si_value
                    );
                }
                other => panic!(
                    "Membrane.prestress should be Scalar, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected Value::StructureInstance for F.m, got {:?}", other),
    }
}

/// args[0] is Tensegrity-shaped but struts references out-of-range index → Undef.
#[test]
fn tensegrity_wires_undef_on_out_of_range_index() {
    // 2 nodes but struts references node index 5 (out of range).
    let nodes = Value::List(vec![make_node(0.0, 0.0, 0.0), make_node(1.0, 0.0, 0.0)]);
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

// ── step-7 (task-4412): tensegrity_surfaces integration tests ────────────────

/// Build a Tensegrity StructureInstance with a surfaces field.
fn make_tensegrity_with_surfaces() -> Value {
    // 4 nodes forming a simple quad — two triangles share the diagonal
    let nodes = vec![
        make_node(0.0, 0.0, 0.0), // node 0
        make_node(1.0, 0.0, 0.0), // node 1
        make_node(1.0, 1.0, 0.0), // node 2
        make_node(0.0, 1.0, 0.0), // node 3
    ];
    let surfaces = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]), // tri 0
        Value::List(vec![Value::Int(0), Value::Int(2), Value::Int(3)]), // tri 1
    ]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(nodes)),
        ("struts".to_string(), Value::List(vec![])),
        ("cables".to_string(), Value::List(vec![])),
        ("surfaces".to_string(), surfaces),
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

/// `tensegrity_surfaces` on a Tensegrity with surfaces=[[0,1,2],[0,2,3]] (4 nodes)
/// yields 2 TensegritySurface facets, each kind="membrane", indices and inline
/// coords matching the nodes table.
///
/// RED (step-7): fails until `tensegrity_surfaces` is registered in eval_builtin
/// (step-8).
#[test]
fn tensegrity_surfaces_emits_two_tagged_facets() {
    let t = make_tensegrity_with_surfaces();
    let result = eval_builtin("tensegrity_surfaces", &[t]);

    let facets = match &result {
        Value::List(f) => f,
        other => panic!("expected Value::List of facets, got {:?}", other),
    };
    assert_eq!(facets.len(), 2, "expected 2 facets, got {}", facets.len());

    // Facet 0: triangle [0, 1, 2]
    let f0 = match &facets[0] {
        Value::StructureInstance(d) => d,
        other => panic!("facets[0] should be StructureInstance, got {:?}", other),
    };
    assert_eq!(f0.type_name, "TensegritySurface", "facets[0] type_name");
    assert_eq!(
        f0.fields.get(&"kind".to_string()),
        Some(&Value::String("membrane".to_string())),
        "facets[0] kind"
    );
    assert_eq!(f0.fields.get(&"i0".to_string()), Some(&Value::Int(0)));
    assert_eq!(f0.fields.get(&"i1".to_string()), Some(&Value::Int(1)));
    assert_eq!(f0.fields.get(&"i2".to_string()), Some(&Value::Int(2)));
    // x0 = node 0 x = 0.0m
    match f0.fields.get(&"x0".to_string()) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!((si_value - 0.0).abs() < 1e-12, "facet[0].x0 should be 0.0m")
        }
        other => panic!("facet[0].x0 should be Scalar, got {:?}", other),
    }
    // x1 = node 1 x = 1.0m
    match f0.fields.get(&"x1".to_string()) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!((si_value - 1.0).abs() < 1e-12, "facet[0].x1 should be 1.0m")
        }
        other => panic!("facet[0].x1 should be Scalar, got {:?}", other),
    }
    // x2 = node 2 x = 1.0m
    match f0.fields.get(&"x2".to_string()) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!((si_value - 1.0).abs() < 1e-12, "facet[0].x2 should be 1.0m")
        }
        other => panic!("facet[0].x2 should be Scalar, got {:?}", other),
    }

    // Facet 1: triangle [0, 2, 3]
    let f1 = match &facets[1] {
        Value::StructureInstance(d) => d,
        other => panic!("facets[1] should be StructureInstance, got {:?}", other),
    };
    assert_eq!(f1.type_name, "TensegritySurface", "facets[1] type_name");
    assert_eq!(
        f1.fields.get(&"kind".to_string()),
        Some(&Value::String("membrane".to_string())),
        "facets[1] kind"
    );
    assert_eq!(f1.fields.get(&"i0".to_string()), Some(&Value::Int(0)));
    assert_eq!(f1.fields.get(&"i1".to_string()), Some(&Value::Int(2)));
    assert_eq!(f1.fields.get(&"i2".to_string()), Some(&Value::Int(3)));
    // x2 = node 3 x = 0.0m
    match f1.fields.get(&"x2".to_string()) {
        Some(Value::Scalar { si_value, .. }) => {
            assert!((si_value - 0.0).abs() < 1e-12, "facet[1].x2 should be 0.0m")
        }
        other => panic!("facet[1].x2 should be Scalar, got {:?}", other),
    }
}
