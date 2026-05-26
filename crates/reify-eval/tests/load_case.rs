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

use reify_test_support::{
    collect_errors, compile_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};
use reify_types::{PersistentMap, Value, ValueCellId};

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
#[test]
fn load_case_ctor_round_trips_to_structure_instance() {
    const SOURCE: &str = r#"
structure def LoadCaseFixture {
    let case = LoadCase(name: "g", loads: [10.0, 20.0], supports: [30.0])
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

            // loads = [10.0, 20.0]
            match field(&data.fields, "loads") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        2,
                        "LoadCase.loads must have 2 elements; got {:?}",
                        items
                    );
                    assert_eq!(
                        items[0],
                        Value::Real(10.0),
                        "loads[0] must be Value::Real(10.0); got {:?}",
                        items[0]
                    );
                    assert_eq!(
                        items[1],
                        Value::Real(20.0),
                        "loads[1] must be Value::Real(20.0); got {:?}",
                        items[1]
                    );
                }
                other => panic!(
                    "expected Value::List for LoadCase.loads, got {:?}",
                    other
                ),
            }

            // supports = [30.0]
            match field(&data.fields, "supports") {
                Some(Value::List(items)) => {
                    assert_eq!(
                        items.len(),
                        1,
                        "LoadCase.supports must have 1 element; got {:?}",
                        items
                    );
                    assert_eq!(
                        items[0],
                        Value::Real(30.0),
                        "supports[0] must be Value::Real(30.0); got {:?}",
                        items[0]
                    );
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
        other => panic!(
            "expected Value::StructureInstance for LoadCaseFixture.case — got {other:?}"
        ),
    }
}

/// task 3549 SIR-β-mlcfea: member-access chain via `self.`-qualified sibling
/// references reads through the `LoadCase` structure instance and resolves each
/// field to the correct scalar value.
///
/// Member access MUST be `self.`-qualified — bare `case.name` does not resolve
/// (confirmed convention across structure_instance_e2e.rs / pressure_load.rs).
#[test]
fn load_case_member_access_reads_name_loads_supports_through() {
    const SOURCE: &str = r#"
structure def LoadCaseAccess {
    let case          = LoadCase(name: "g", loads: [10.0, 20.0], supports: [30.0])
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

    // first_load == 10.0
    let first_load_id = ValueCellId::new("LoadCaseAccess", "first_load");
    let first_load = result
        .values
        .get(&first_load_id)
        .unwrap_or_else(|| panic!("LoadCaseAccess.first_load cell missing"));
    assert_eq!(
        first_load,
        &Value::Real(10.0),
        "self.case.loads[0] must resolve to Value::Real(10.0); got {first_load:?}"
    );

    // first_support == 30.0
    let first_support_id = ValueCellId::new("LoadCaseAccess", "first_support");
    let first_support = result
        .values
        .get(&first_support_id)
        .unwrap_or_else(|| panic!("LoadCaseAccess.first_support cell missing"));
    assert_eq!(
        first_support,
        &Value::Real(30.0),
        "self.case.supports[0] must resolve to Value::Real(30.0); got {first_support:?}"
    );
}

/// task 3549 SIR-β-mlcfea: `MultiCaseResult(cases: map{})` ctor lowers to a
/// `Value::StructureInstance` whose `type_name` is `"MultiCaseResult"` and
/// whose `cases` field is a `Value::Map`.
///
/// Re-pins the SIR-α tripwire in the SIR-β-mlcfea-owned file so this file
/// is the canonical coverage owner for the LoadCase/MultiCaseResult surface.
#[test]
fn multi_case_result_ctor_round_trips_to_structure_instance() {
    const SOURCE: &str = r#"
structure def MultiCaseFixture {
    let mcr = MultiCaseResult(cases: map{})
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("MultiCaseFixture", "mcr");
    let mcr = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("MultiCaseFixture.mcr cell missing from eval result"));

    match mcr {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "MultiCaseResult",
                "expected type_name=\"MultiCaseResult\" (SIR-β-mlcfea stdlib structure_def), \
                 got {:?}",
                data.type_name
            );
            assert!(
                matches!(field(&data.fields, "cases"), Some(Value::Map(_))),
                "MultiCaseResult.cases must be a Value::Map; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for MultiCaseFixture.mcr — got {other:?}"
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
