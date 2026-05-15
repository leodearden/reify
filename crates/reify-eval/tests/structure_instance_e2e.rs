//! SIR-α (task 3540) — `Value::StructureInstance` boundary tests.
//!
//! Step-9 seeds this file with a single no-op constructor test. Its purpose is
//! to bring `Value::StructureInstance` into scope as a *constructed* value so
//! the workspace-wide exhaustiveness surface is exercised: any `match` on
//! `&Value` that lacks a wildcard arm (e.g. the deliberate exhaustiveness
//! guard `assert_all_value_variants_listed` in
//! `tests/m8_m11_regression_checkpoint.rs`) fails to compile until step-10's
//! adapter sweep lands the missing arms.
//!
//! Steps 21/23 replace this stub with the full PRD §7.1/§7.2 boundary suite
//! and the `reify eval` golden test.

use reify_types::{PersistentMap, StructureTypeId, Value};

/// No-op constructor: proves `Value::StructureInstance` is reachable from a
/// test binary. Compilation of the whole `reify-eval` test target is the
/// real assertion here (step-9 RED → step-10 GREEN).
#[test]
fn structure_instance_is_constructible() {
    let fields: PersistentMap<String, Value> = [("youngs_modulus".to_string(), Value::Real(205e9))]
        .into_iter()
        .collect();
    let v = Value::StructureInstance {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields,
    };
    match &v {
        Value::StructureInstance {
            type_name, version, ..
        } => {
            assert_eq!(type_name, "Steel_AISI_1045");
            assert_eq!(*version, 1);
        }
        other => panic!("expected StructureInstance, got {other:?}"),
    }
}
