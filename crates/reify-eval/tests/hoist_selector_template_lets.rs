//! Task 4370 AXIS-1 step-4 (RED): B-3 interaction — a struct-def `let` that
//! echoes a selector-typed param, in a structure instantiated with a NESTED
//! named-leaf `face(b, name)` ctor, must evaluate to `Value::Selector` (not
//! `Value::Undef`) after eval.
//!
//! Shape:
//! ```text
//! structure def FaceEcho {
//!     param face_sel : FaceSelector
//!     let echoed = self.face_sel      // struct-def let echoing the param member
//! }
//! structure def Harness {
//!     let b  = box(1m, 1m, 1m)
//!     let fe = FaceEcho(face_sel: face(b, "x_max"))   // nested named-leaf ctor
//! }
//! ```
//!
//! RED on main (steps 1-3 applied, step-6 hoist absent): the nested
//! `face(b, "x_max")` sits INLINE in the `FaceEcho` ctor field, so
//! `eval_structure_instance_ctor` (reify-expr/src/lib.rs) evaluates it through
//! reify-expr's registry-free `eval_expr` — which has no kernel-bearing
//! named-leaf selector minting (that lives in reify-eval `geometry_ops.rs`) —
//! yielding `Value::Undef`. `materialize_template_lets` then evaluates
//! `echoed = self.face_sel` against a child scope whose `face_sel` field is
//! that `Undef`, so `echoed` is `Undef`.
//!
//! GREENed by step-6 (`phase_hoist_nested_selector_ctors`): the hoist lifts
//! `face(b, "x_max")` into a top-level `Harness.__sel*` `Let` cell and rewrites
//! the ctor field to a `ValueRef(__sel*)`. The R3d in-walk mint resolves that
//! cell via the step-1/2 symbolic named-leaf stand-in
//! (`try_eval_symbolic_topology_selector`) to a `Value::Selector` BEFORE `fe`
//! is evaluated (the `ValueRef` dep edge orders `__sel*` first), so the ctor
//! field — and hence the echoed `let` — carry the selector.
//!
//! Concern B-3 fallback trigger: if step-6 cannot GREEN this test, Strategy A
//! is the documented fallback (design-decision-1 in `.task/plan.json`).

#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

const SOURCE: &str = r#"
structure def FaceEcho {
    param face_sel : FaceSelector
    let echoed = self.face_sel
}
structure def Harness {
    let b  = box(1m, 1m, 1m)
    let fe = FaceEcho(face_sel: face(b, "x_max"))
}
"#;

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenario index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

/// The struct-def `let echoed = self.face_sel` must evaluate to a
/// `Value::Selector` once the nested `face(b, "x_max")` ctor is hoisted.
#[test]
fn nested_named_leaf_selector_echoed_by_template_let_is_selector() {
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let fe = result
        .values
        .get(&ValueCellId::new("Harness", "fe"))
        .unwrap_or_else(|| panic!("Harness.fe cell missing from eval result"));

    let data = match fe {
        Value::StructureInstance(data) => data,
        other => panic!("Harness.fe must be a Value::StructureInstance, got {other:?}"),
    };
    assert_eq!(
        data.type_name, "FaceEcho",
        "Harness.fe must be a FaceEcho instance"
    );

    let echoed = field(&data.fields, "echoed").unwrap_or_else(|| {
        panic!(
            "FaceEcho instance must carry the `echoed` template let; fields: {:?}",
            data.fields
        )
    });

    assert!(
        matches!(echoed, Value::Selector(_)),
        "the struct-def let `echoed = self.face_sel` must evaluate to \
         Value::Selector after the nested face(b, \"x_max\") ctor is hoisted to a \
         top-level __sel* cell (Concern B-3); got {echoed:?}"
    );
}
