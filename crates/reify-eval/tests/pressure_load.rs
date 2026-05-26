//! SIR-β-load (task 3544) — `PressureLoad` structure-def boundary tests.
//!
//! Mirrors the wave-1 pattern in `structure_instance_e2e.rs` for the
//! wave-2 `PressureLoad` migration from the name-dispatched `pressure_load`
//! builtin to a stdlib `structure def PressureLoad : Load { ... }`.
//!
//! PRD reference: `docs/prds/v0_3/structure-instance-runtime.md` §8 Phase 2.
//!
//! Tests are ordered RED (step-1, all failing before the structure def
//! is declared in step-2) → GREEN (after step-2 lands the def in
//! `crates/reify-compiler/stdlib/fea_multi_case.ri`).

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

// ── SIR-β-load step-1 (RED) → step-2 (GREEN) tests ──────────────────────────

/// task 3544 step-1: bare `PressureLoad()` constructor lowers to a
/// `Value::StructureInstance` whose `type_name` is `"PressureLoad"` and whose
/// fields carry the three declared defaults: `direction = "normal"`,
/// `face = ""`, `magnitude = 0.0`.
///
/// RED before step-2 declares `structure def PressureLoad : Load { ... }` in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri`; source-level `PressureLoad(...)`
/// currently falls through to `Value::Undef` (unknown-name path).
#[test]
fn pressure_load_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def PressureLoadFixture {
    let pressure = PressureLoad()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PressureLoadFixture", "pressure");
    let pressure = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PressureLoadFixture.pressure cell missing from eval result"));

    match pressure {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PressureLoad",
                "expected type_name=\"PressureLoad\" (the wave-2 SIR-β-load stdlib \
                 structure_def), got {:?}",
                data.type_name
            );
            // direction default = "normal"
            assert_eq!(
                field(&data.fields, "direction"),
                Some(&Value::String("normal".to_string())),
                "PressureLoad.direction default must be \"normal\"; fields: {:?}",
                data.fields
            );
            // face default = ""
            assert_eq!(
                field(&data.fields, "face"),
                Some(&Value::String(String::new())),
                "PressureLoad.face default must be \"\"; fields: {:?}",
                data.fields
            );
            // magnitude default = 0.0
            assert_eq!(
                field(&data.fields, "magnitude"),
                Some(&Value::Real(0.0)),
                "PressureLoad.magnitude default must be 0.0; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for PressureLoadFixture.pressure — \
             got {other:?}"
        ),
    }
}

/// task 3544 step-1: member-access chain `self.pressure.direction` reads
/// through the `PressureLoad` structure instance and resolves to
/// `Value::String("normal")` (the default declared in the structure def).
#[test]
fn pressure_load_member_access_direction() {
    const SOURCE: &str = r#"
structure def PressureLoadAccess {
    let pressure  = PressureLoad()
    let direction = self.pressure.direction
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PressureLoadAccess", "direction");
    let dir = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PressureLoadAccess.direction cell missing from eval result"));

    assert_eq!(
        dir,
        &Value::String("normal".to_string()),
        "self.pressure.direction must resolve to Value::String(\"normal\"); got {dir:?}"
    );
}

/// task 3544 step-1: member-access chain `self.pressure.magnitude` reads
/// through the `PressureLoad` structure instance and resolves to
/// `Value::Real(0.0)` (the default declared in the structure def).
#[test]
fn pressure_load_member_access_magnitude() {
    const SOURCE: &str = r#"
structure def PressureLoadMag {
    let pressure  = PressureLoad()
    let magnitude = self.pressure.magnitude
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PressureLoadMag", "magnitude");
    let mag = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PressureLoadMag.magnitude cell missing from eval result"));

    assert_eq!(
        mag,
        &Value::Real(0.0),
        "self.pressure.magnitude must resolve to Value::Real(0.0); got {mag:?}"
    );
}

/// task 3544 step-1: trait-typed param admission — `param load : Load = PressureLoad()`
/// compiles without any Error-severity diagnostics, confirming that `PressureLoad`
/// nominally conforms to `trait Load` after the Load trait body is relaxed to an
/// empty marker in step-2.
///
/// This is the regression guard for the Load trait relaxation (design-decision-1
/// in `.task/plan.json`): PointLoad must still conform (it declares `force` +
/// `point` explicitly), and PressureLoad must now also conform.
#[test]
fn trait_typed_param_admits_pressure_load() {
    const SOURCE: &str = r#"
structure def LoadHolder {
    param load : Load = PressureLoad()
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "PressureLoad must be admitted for a Load-typed param without Error \
         diagnostics (nominal conformance after empty-marker Load trait relaxation); \
         got errors: {errors:?}"
    );
}

/// task 3544 amendment — non-conforming structure rejected for a Load-typed param.
///
/// Negative companion to `trait_typed_param_admits_pressure_load`: confirms that
/// the empty-marker `trait Load { }` relaxation does NOT disable trait identity
/// enforcement.  Only structures that declare `: Load` (e.g. PressureLoad,
/// PointLoad) can fill a `: Load`-typed slot; a plain structure that omits the
/// conformance declaration must produce a "does not conform to trait" diagnostic.
///
/// Without this guard the positive test above cannot distinguish "nominal
/// conformance works" from "the trait constraint is silently ignored entirely".
#[test]
fn trait_typed_param_rejects_non_load_structure() {
    const SOURCE: &str = r#"
structure def NotALoad {
    param value : Real = 0.0
}
structure def LoadConsumer {
    param load : Load
}
structure def BadUsage {
    sub consumer = LoadConsumer(load: NotALoad())
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("Load")),
        "NotALoad must be rejected for a Load-typed param with a 'does not conform \
         to trait Load' error (empty-marker trait still enforces nominal identity); \
         got errors: {errors:?}"
    );
}
