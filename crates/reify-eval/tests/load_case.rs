//! SIR-β-mlcfea (task 3549) — `LoadCase` / `MultiCaseResult` structure-def
//! boundary tests.
//!
//! Pins the ctor→`Value::StructureInstance` contract, `self.`-qualified member-
//! access readthrough, and the Map-vs-StructureInstance linguistic-distinguishability
//! invariant (GR-001 §Resolution) for the `LoadCase` / `MultiCaseResult` surface.
//!
//! These tests are the SIR-β-mlcfea closure document. Before this file existed
//! the LoadCase ctor contract was exercised only at the compile-time param-shape
//! level in `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs`; no
//! dedicated eval-layer file pinned the ctor→StructureInstance path.
//!
//! Cross-reference:
//!   - `crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs` — SIR-α tripwire
//!     for `MultiCaseResult(cases: map{})` ctor (broader accessor smoke coverage).
//!   - `crates/reify-eval/tests/structure_instance_e2e.rs` — wave-1 PointLoad /
//!     Steel contract pins.
//!   - `crates/reify-eval/tests/pressure_load.rs` — wave-2 SIR-β-load PressureLoad
//!     boundary tests (primary template for this file).
//!
//! PRD reference: `docs/prds/v0_3/structure-instance-runtime.md` §8 Phase 2.

#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── SIR-β-mlcfea tests ───────────────────────────────────────────────────────

/// task 3549 SIR-β-mlcfea: `LoadCase(...)` ctor lowers to a
/// `Value::StructureInstance` whose `type_name` is `"LoadCase"` and whose
/// fields carry the supplied values for `name`, `loads`, `supports`, and the
/// declared default `Value::Option(None)` for `options`.
///
/// The `options` default (`= none`) compiles to `CompiledExprKind::OptionNone`
/// which evaluates to `Value::Option(None)` — NOT `Value::None` — per
/// `crates/reify-expr/src/lib.rs:611`.
///
/// After task ζ/4444: `loads` and `supports` are typed `List<Load>` /
/// `List<Support>` so each element must be a conforming structure instance
/// (`PointLoad`, `FixedSupport`, etc.).
#[test]
fn load_case_ctor_round_trips_to_structure_instance() {
    const SOURCE: &str = r#"
structure def LoadCaseFixture {
    let case = LoadCase(
        name: "g",
        loads: [PointLoad(point: "a", force: 10.0), PointLoad(point: "b", force: 20.0)],
        supports: [FixedSupport(target: "r")]
    )
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("LoadCaseFixture", "case");
    let case = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("LoadCaseFixture.case cell missing from eval result"));

    match case {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "LoadCase",
                "expected type_name=\"LoadCase\" (SIR-β-mlcfea stdlib structure_def), \
                 got {:?}",
                data.type_name
            );

            // name = "g"
            assert_eq!(
                field(&data.fields, "name"),
                Some(&Value::String("g".to_string())),
                "LoadCase.name must be \"g\"; fields: {:?}",
                data.fields
            );

            // loads = [PointLoad(...), PointLoad(...)]
            match field(&data.fields, "loads") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        2,
                        "LoadCase.loads must have 2 elements; got {:?}",
                        items
                    );
                    match &items[0] {
                        Value::StructureInstance(si) => assert_eq!(
                            si.type_name, "PointLoad",
                            "loads[0] must be StructureInstance{{type_name=\"PointLoad\"}}; \
                             got type_name={:?}",
                            si.type_name
                        ),
                        other => panic!(
                            "loads[0] must be Value::StructureInstance(PointLoad); got {:?}",
                            other
                        ),
                    }
                    match &items[1] {
                        Value::StructureInstance(si) => assert_eq!(
                            si.type_name, "PointLoad",
                            "loads[1] must be StructureInstance{{type_name=\"PointLoad\"}}; \
                             got type_name={:?}",
                            si.type_name
                        ),
                        other => panic!(
                            "loads[1] must be Value::StructureInstance(PointLoad); got {:?}",
                            other
                        ),
                    }
                }
                other => panic!("expected Value::List for LoadCase.loads, got {:?}", other),
            }

            // supports = [FixedSupport(...)]
            match field(&data.fields, "supports") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        1,
                        "LoadCase.supports must have 1 element; got {:?}",
                        items
                    );
                    match &items[0] {
                        Value::StructureInstance(si) => assert_eq!(
                            si.type_name, "FixedSupport",
                            "supports[0] must be StructureInstance{{type_name=\"FixedSupport\"}}; \
                             got type_name={:?}",
                            si.type_name
                        ),
                        other => panic!(
                            "supports[0] must be Value::StructureInstance(FixedSupport); \
                             got {:?}",
                            other
                        ),
                    }
                }
                other => panic!(
                    "expected Value::List for LoadCase.supports, got {:?}",
                    other
                ),
            }

            // options = none → Value::Option(None)
            assert_eq!(
                field(&data.fields, "options"),
                Some(&Value::Option(None)),
                "LoadCase.options default must be Value::Option(None) \
                 (CompiledExprKind::OptionNone → Value::Option(None)); fields: {:?}",
                data.fields
            );
        }
        other => {
            panic!("expected Value::StructureInstance for LoadCaseFixture.case — got {other:?}")
        }
    }
}

/// task 3549 SIR-β-mlcfea: member-access chain via `self.`-qualified sibling
/// references reads through the `LoadCase` structure instance and resolves each
/// field to the correct value.
///
/// Member access MUST be `self.`-qualified — bare `case.name` does not resolve
/// (confirmed convention across structure_instance_e2e.rs / pressure_load.rs).
///
/// After task ζ/4444: `first_load` / `first_support` resolve to typed
/// `Value::StructureInstance` values (PointLoad / FixedSupport).
#[test]
fn load_case_member_access_reads_name_loads_supports_through() {
    const SOURCE: &str = r#"
structure def LoadCaseAccess {
    let case          = LoadCase(
        name: "g",
        loads: [PointLoad(point: "a", force: 10.0), PointLoad(point: "b", force: 20.0)],
        supports: [FixedSupport(target: "r")]
    )
    let case_name     = self.case.name
    let first_load    = self.case.loads[0]
    let first_support = self.case.supports[0]
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // case_name == "g"
    let case_name_id = ValueCellId::new("LoadCaseAccess", "case_name");
    let case_name = result
        .values
        .get(&case_name_id)
        .unwrap_or_else(|| panic!("LoadCaseAccess.case_name cell missing"));
    assert_eq!(
        case_name,
        &Value::String("g".to_string()),
        "self.case.name must resolve to Value::String(\"g\"); got {case_name:?}"
    );

    // first_load == StructureInstance{type_name="PointLoad"}
    let first_load_id = ValueCellId::new("LoadCaseAccess", "first_load");
    let first_load = result
        .values
        .get(&first_load_id)
        .unwrap_or_else(|| panic!("LoadCaseAccess.first_load cell missing"));
    match first_load {
        Value::StructureInstance(si) => assert_eq!(
            si.type_name, "PointLoad",
            "self.case.loads[0] must resolve to StructureInstance{{type_name=\"PointLoad\"}}; \
             got type_name={:?}",
            si.type_name
        ),
        other => panic!(
            "self.case.loads[0] must be Value::StructureInstance(PointLoad); got {other:?}"
        ),
    }

    // first_support == StructureInstance{type_name="FixedSupport"}
    let first_support_id = ValueCellId::new("LoadCaseAccess", "first_support");
    let first_support = result
        .values
        .get(&first_support_id)
        .unwrap_or_else(|| panic!("LoadCaseAccess.first_support cell missing"));
    match first_support {
        Value::StructureInstance(si) => assert_eq!(
            si.type_name, "FixedSupport",
            "self.case.supports[0] must resolve to StructureInstance{{type_name=\"FixedSupport\"}}; \
             got type_name={:?}",
            si.type_name
        ),
        other => panic!(
            "self.case.supports[0] must be Value::StructureInstance(FixedSupport); got {other:?}"
        ),
    }
}

// NOTE: `MultiCaseResult(cases: map{})` ctor→StructureInstance is already
// pinned by the SIR-α tripwire in `multi_load_case_stdlib_smoke.rs`. Rather
// than keeping a verbatim duplicate here, the Map-vs-Structure distinction
// test below (`map_value_and_multi_case_result_pattern_match_discriminates`)
// re-exercises MultiCaseResult as part of its discrimination assertion. The
// sole canonical `ctor→type_name` pin remains the smoke file.

/// task 3549 SIR-β-mlcfea: `examples/load_case.ri` compiles error-free under the
/// stdlib prelude AND its documented signal cells evaluate to the correct Value
/// shapes.
///
/// Path is CARGO_MANIFEST_DIR-anchored per the task-348 convention:
/// `crates/reify-eval` → `../../examples/load_case.ri` (workspace root).
///
/// Pins compile-cleanness AND that the example actually exercises the
/// user-observable signal cells documented in the file's comments.
#[test]
fn load_case_example_evals_clean_and_exercises_signal_cells() {
    const EXAMPLE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/load_case.ri");

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect("read examples/load_case.ri");
    let compiled = parse_and_compile_with_stdlib(&src);

    // Must compile with zero Error-severity diagnostics.
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/load_case.ri must compile without errors under stdlib prelude; \
         got errors: {errors:?}"
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // LoadCaseDemo.case → Value::StructureInstance{type_name="LoadCase"}
    let case = result
        .values
        .get(&ValueCellId::new("LoadCaseDemo", "case"))
        .unwrap_or_else(|| panic!("LoadCaseDemo.case cell missing"));
    match case {
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "LoadCase",
            "LoadCaseDemo.case must have type_name=\"LoadCase\"; got {:?}",
            data.type_name
        ),
        other => panic!("expected Value::StructureInstance for LoadCaseDemo.case; got {other:?}"),
    }

    // LoadCaseDemo.case_name → Value::String("g")
    let case_name = result
        .values
        .get(&ValueCellId::new("LoadCaseDemo", "case_name"))
        .unwrap_or_else(|| panic!("LoadCaseDemo.case_name cell missing"));
    assert_eq!(
        case_name,
        &Value::String("g".to_string()),
        "LoadCaseDemo.case_name must be Value::String(\"g\"); got {case_name:?}"
    );

    // LoadCaseDemo.first_load → Value::StructureInstance{type_name="PointLoad"}
    // (after task ζ/4444: typed PointLoad conformer instead of bare Real)
    let first_load = result
        .values
        .get(&ValueCellId::new("LoadCaseDemo", "first_load"))
        .unwrap_or_else(|| panic!("LoadCaseDemo.first_load cell missing"));
    match first_load {
        Value::StructureInstance(si) => assert_eq!(
            si.type_name, "PointLoad",
            "LoadCaseDemo.first_load must be StructureInstance{{type_name=\"PointLoad\"}}; \
             got type_name={:?}",
            si.type_name
        ),
        other => panic!(
            "LoadCaseDemo.first_load must be Value::StructureInstance(PointLoad); \
             got {other:?}"
        ),
    }

    // LoadCaseDemo.first_support → Value::StructureInstance{type_name="FixedSupport"}
    // (after task ζ/4444: typed FixedSupport conformer instead of bare Real)
    let first_support = result
        .values
        .get(&ValueCellId::new("LoadCaseDemo", "first_support"))
        .unwrap_or_else(|| panic!("LoadCaseDemo.first_support cell missing"));
    match first_support {
        Value::StructureInstance(si) => assert_eq!(
            si.type_name, "FixedSupport",
            "LoadCaseDemo.first_support must be StructureInstance{{type_name=\"FixedSupport\"}}; \
             got type_name={:?}",
            si.type_name
        ),
        other => panic!(
            "LoadCaseDemo.first_support must be Value::StructureInstance(FixedSupport); \
             got {other:?}"
        ),
    }

    // MapVsStructureDemo.raw_cases → Value::Map(_)
    let raw_cases = result
        .values
        .get(&ValueCellId::new("MapVsStructureDemo", "raw_cases"))
        .unwrap_or_else(|| panic!("MapVsStructureDemo.raw_cases cell missing"));
    assert!(
        matches!(raw_cases, Value::Map(_)),
        "MapVsStructureDemo.raw_cases must be a Value::Map; got {raw_cases:?}"
    );

    // MapVsStructureDemo.wrapped → Value::StructureInstance{type_name="MultiCaseResult"}
    let wrapped = result
        .values
        .get(&ValueCellId::new("MapVsStructureDemo", "wrapped"))
        .unwrap_or_else(|| panic!("MapVsStructureDemo.wrapped cell missing"));
    match wrapped {
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "MultiCaseResult",
            "MapVsStructureDemo.wrapped must have type_name=\"MultiCaseResult\"; got {:?}",
            data.type_name
        ),
        other => panic!(
            "expected Value::StructureInstance for MapVsStructureDemo.wrapped; got {other:?}"
        ),
    }
}

/// task 3549 SIR-β-mlcfea: a raw `map{{...}}` value and a `MultiCaseResult`
/// structure instance coexist in the same fixture and are structurally
/// distinct — they do NOT conflate through the content-hash / cache path.
///
/// Pins the GR-001 §Resolution Map-vs-StructureInstance linguistic-distinguishability
/// invariant for the LoadCase/MultiCaseResult surface. Mirrors
/// `linguistic_map_vs_structure_distinction` in structure_instance_e2e.rs.
#[test]
fn map_value_and_multi_case_result_pattern_match_discriminates() {
    const SOURCE: &str = r#"
structure def MapVsStructFixture {
    let raw_cases = map{"a" => 1, "b" => 2}
    let wrapped   = MultiCaseResult(cases: map{})
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let raw_cases = result
        .values
        .get(&ValueCellId::new("MapVsStructFixture", "raw_cases"))
        .unwrap_or_else(|| panic!("MapVsStructFixture.raw_cases cell missing"));
    let wrapped = result
        .values
        .get(&ValueCellId::new("MapVsStructFixture", "wrapped"))
        .unwrap_or_else(|| panic!("MapVsStructFixture.wrapped cell missing"));

    assert!(
        matches!(raw_cases, Value::Map(_)),
        "a `map{{...}}` literal must remain a Value::Map; got {raw_cases:?}"
    );
    assert!(
        matches!(wrapped, Value::StructureInstance(_)),
        "MultiCaseResult ctor must produce a Value::StructureInstance; got {wrapped:?}"
    );
    assert_ne!(
        raw_cases.content_hash().0,
        wrapped.content_hash().0,
        "a Map and a StructureInstance must never share a content hash \
         (GR-001 §Resolution Map-vs-StructureInstance distinguishability)"
    );
}
