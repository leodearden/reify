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
//! Step-19 adds an end-to-end RED test that compiles a Reify source
//! containing a `PointLoad(...)` call and asserts the resulting cell value
//! is a `Value::StructureInstance` named `"PointLoad"`. This pins the
//! wave-1 stdlib swap (PRD §6, Q-SIR-4) end-to-end through the
//! parse → compile-with-stdlib → eval pipeline. Currently RED: there is
//! no `structure def PointLoad` in the stdlib yet, so the source-level
//! `PointLoad(...)` call falls through the function-call lowering as a
//! plain `FunctionCall` and `eval_builtin("PointLoad", ...)` returns
//! `Value::Undef`. Step-20 lands the structure-def + retires the
//! `point_load` builtin arm, flipping this to GREEN.
//!
//! Steps 21/23 replace this stub with the full PRD §7.1/§7.2 boundary suite
//! and the `reify eval` golden test.

#![allow(clippy::mutable_key_type)]

use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{PersistentMap, StructureTypeId, Value, ValueCellId};

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

/// task 3540 step-19 (RED): end-to-end check of the wave-1 stdlib swap.
///
/// Compiles a tiny structure that calls `PointLoad()` (the new structure-def
/// constructor; step-20 lands it in `crates/reify-compiler/stdlib/fea_multi_case.ri`).
/// Asserts the bound cell value is a `Value::StructureInstance` whose
/// `type_name` is `"PointLoad"`.
///
/// Currently RED: there is no `structure def PointLoad` in the stdlib, so
/// `PointLoad()` lowers to a plain `FunctionCall` and `eval_builtin("PointLoad", ...)`
/// returns `Value::Undef`. The assertion will then panic with a "expected
/// StructureInstance, got Undef" message until step-20 ships.
///
/// Note: the ctor invocation is zero-arg — every `PointLoad` param has a
/// declared default in step-20's structure-def, so the SIR-α path can build
/// a fully-defaulted instance without dragging the selector / dimensioned-
/// vector validation surface into this RED test.
#[test]
fn point_load_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def PointLoadFixture {
    let load = PointLoad()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PointLoadFixture", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PointLoadFixture.load cell missing from eval result"));

    match load {
        Value::StructureInstance { type_name, .. } => {
            assert_eq!(
                type_name, "PointLoad",
                "expected type_name=\"PointLoad\" (the wave-1 SIR-α stdlib structure_def), \
                 got {type_name:?}"
            );
        }
        other => panic!(
            "expected Value::StructureInstance for PointLoadFixture.load — \
             RED until step-20 lands `structure def PointLoad : Load {{ ... }}` \
             in stdlib/fea_multi_case.ri; got {other:?}"
        ),
    }
}
