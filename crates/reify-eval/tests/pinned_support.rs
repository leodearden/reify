//! SIR-β-sup (task 3546) — `PinnedSupport` structure-def boundary tests.
//!
//! Mirrors the wave-2 pattern in `pressure_load.rs` (task 3544) for the
//! `PinnedSupport` migration from the name-dispatched `pinned_support`
//! builtin to a stdlib `structure def PinnedSupport : Support { ... }`.
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
use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── SIR-β-sup step-1 (RED) → step-2 (GREEN) tests ───────────────────────────

/// task 3546 step-1: bare `PinnedSupport()` constructor lowers to a
/// `Value::StructureInstance` whose `type_name` is `"PinnedSupport"` and whose
/// fields carry the single declared default: `target = ""`.
///
/// RED before step-2 declares `structure def PinnedSupport : Support { ... }`
/// in `crates/reify-compiler/stdlib/fea_multi_case.ri`; source-level
/// `PinnedSupport(...)` currently falls through to the name-dispatched builtin
/// which returns a `Value::Map`, not a `Value::StructureInstance`.
#[test]
fn pinned_support_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def PinnedSupportFixture {
    let support = PinnedSupport()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PinnedSupportFixture", "support");
    let support = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PinnedSupportFixture.support cell missing from eval result"));

    match support {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PinnedSupport",
                "expected type_name=\"PinnedSupport\" (the wave-2 SIR-β-sup stdlib \
                 structure_def), got {:?}",
                data.type_name
            );
            // target default = ""
            assert_eq!(
                field(&data.fields, "target"),
                Some(&Value::String(String::new())),
                "PinnedSupport.target default must be \"\"; fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for PinnedSupportFixture.support — \
             got {other:?}"
        ),
    }
}

/// task 3546 step-1: member-access chain `self.support.target` reads
/// through the `PinnedSupport` structure instance and resolves to
/// `Value::String("")` (the default declared in the structure def).
#[test]
fn pinned_support_member_access_target() {
    const SOURCE: &str = r#"
structure def PinnedSupportAccess {
    let support = PinnedSupport()
    let target  = self.support.target
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PinnedSupportAccess", "target");
    let target = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PinnedSupportAccess.target cell missing from eval result"));

    assert_eq!(
        target,
        &Value::String(String::new()),
        "self.support.target must resolve to Value::String(\"\"); got {target:?}"
    );
}

/// task 3546 amendment — explicit `target` field round-trips a non-default value.
///
/// The retired builtin test `pinned_support_returns_map_with_correct_fields` covered
/// round-tripping an explicit selector string through the `target` key on the old
/// `Value::Map`; this test pins the same contract on the structure-def evaluation
/// path (field-round-trip guard for `PinnedSupport(target: "face_1")`).
///
/// Mirrors `FixedSupport` boundary suite behaviour for explicit args.
#[test]
fn pinned_support_explicit_target_field_round_trips() {
    const SOURCE: &str = r#"
structure def PinnedSupportExplicit {
    let support = PinnedSupport(target: "face_1")
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PinnedSupportExplicit", "support");
    let support = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PinnedSupportExplicit.support cell missing from eval result"));

    match support {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PinnedSupport",
                "expected type_name=\"PinnedSupport\"; got {:?}",
                data.type_name
            );
            assert_eq!(
                field(&data.fields, "target"),
                Some(&Value::String("face_1".to_string())),
                "PinnedSupport(target: \"face_1\").target must round-trip as \
                 Value::String(\"face_1\"); fields: {:?}",
                data.fields
            );
        }
        other => panic!(
            "expected Value::StructureInstance for PinnedSupportExplicit.support — \
             got {other:?}"
        ),
    }
}

/// task 3546 step-1: trait-typed param admission — `param sup : Support = PinnedSupport()`
/// compiles without any Error-severity diagnostics, confirming that `PinnedSupport`
/// nominally conforms to `trait Support` (the empty-marker form from SIR-α wave-1).
///
/// This is the regression guard for the Support trait: FixedSupport must still
/// conform, and PinnedSupport must also conform.
#[test]
fn trait_typed_param_admits_pinned_support() {
    const SOURCE: &str = r#"
structure def SupportHolder {
    param sup : Support = PinnedSupport()
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "PinnedSupport must be admitted for a Support-typed param without Error \
         diagnostics (nominal conformance via empty-marker Support trait); \
         got errors: {errors:?}"
    );
}

/// task 3546 amendment — non-conforming structure rejected for a Support-typed param.
///
/// Negative companion to `trait_typed_param_admits_pinned_support`: confirms that
/// the empty-marker `trait Support { }` does NOT disable trait identity enforcement.
/// Only structures that declare `: Support` (e.g. PinnedSupport, FixedSupport)
/// can fill a `: Support`-typed slot; a plain structure that omits the conformance
/// declaration must produce a "does not conform to trait" diagnostic.
///
/// Without this guard the positive test above cannot distinguish "nominal
/// conformance works" from "the trait constraint is silently ignored entirely".
#[test]
fn trait_typed_param_rejects_non_support_structure() {
    const SOURCE: &str = r#"
structure def NotASupport {
    param value : Real = 0.0
}
structure def SupportConsumer {
    param sup : Support
}
structure def BadUsage {
    sub consumer = SupportConsumer(sup: NotASupport())
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    // Match "trait 'Support'" specifically so a stray mention of the consumer
    // structure name "SupportConsumer" cannot accidentally satisfy the check.
    // The conformance module emits messages of the form:
    //   "type 'NotASupport' does not conform to trait 'Support' required by param 'sup'"
    // (see crates/reify-compiler/src/conformance/mod.rs:374-376).
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("trait 'Support'")),
        "NotASupport must be rejected for a Support-typed param with a 'does not conform \
         to trait Support' error (empty-marker trait still enforces nominal identity); \
         got errors: {errors:?}"
    );
}
