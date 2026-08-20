//! Task 4370 AXIS-1 step-5 (RED): recompile-determinism / content-hash-stability
//! for the nested-selector hoist.
//!
//! The ORIGINAL step-5 asked for a `CompiledModule` serialize+deserialize
//! round-trip. That mechanism does NOT exist and cannot be added cheaply:
//! `CompiledModule` derives only `Debug`/`Clone` (`reify-compiler/src/types.rs`
//! `#[derive(Debug, Clone)] pub struct CompiledModule`), there is no serde
//! anywhere in the compiler IR, and adding it is blocked by an `AtomicBool` in
//! `SampledField` (explicit in-code note at `types.rs`: "CompiledModule has no
//! serde, so this is in-memory struct-field passing, not serialization").
//!
//! This test pins the ACHIEVABLE cache-correctness property that
//! design-decision-5 actually names — "synthetic cells + rewritten exprs fold
//! into the module content_hash (cache and serialize/deserialize round-trip
//! correctness)": the hoist is DETERMINISTIC (identical `content_hash` across
//! recompiles, so cache keys are stable), its synthetic `__sel*` cells are
//! present, and the nested selector field resolves to a `Value::Selector`.
//!
//! RED on main (steps 1-4 applied, step-6 hoist absent): no `__sel*` cell is
//! minted and the nested `face(b, "x_max")` field evaluates to `Value::Undef`.
//! GREENed by step-6 (`phase_hoist_nested_selector_ctors`).

#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{CompiledExprKind, PersistentMap, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

const SOURCE: &str = r#"
structure def Holder {
    param sel : FaceSelector
}
structure def Harness {
    let b = box(1m, 1m, 1m)
    let h = Holder(sel: face(b, "x_max"))
}
"#;

fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

fn harness_template(
    module: &reify_compiler::CompiledModule,
) -> &reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == "Harness")
        .expect("Harness template must be present")
}

#[test]
fn hoisted_selector_cells_are_deterministic_and_resolve() {
    // Fresh-compile the SAME source twice.
    let m1 = parse_and_compile_with_stdlib(SOURCE);
    let m2 = parse_and_compile_with_stdlib(SOURCE);

    let t1 = harness_template(&m1);
    let t2 = harness_template(&m2);

    // (a) Each compile minted the synthetic `__sel*` Let cell carrying the
    //     hoisted `face(b, "x_max")` ctor verbatim.
    for (label, t) in [("compile #1", t1), ("compile #2", t2)] {
        let sel: Vec<_> = t
            .value_cells
            .iter()
            .filter(|c| c.id.member.starts_with("__sel"))
            .collect();
        assert!(
            !sel.is_empty(),
            "{label}: expected a synthetic __sel* cell; cells: {:?}",
            t.value_cells
                .iter()
                .map(|c| c.id.member.as_str())
                .collect::<Vec<_>>()
        );
        for c in &sel {
            assert_eq!(
                c.kind,
                reify_compiler::ValueCellKind::Let,
                "{label}: __sel {} must be a Let",
                c.id.member
            );
            match c.default_expr.as_ref().map(|e| &e.kind) {
                Some(CompiledExprKind::FunctionCall { function, .. }) => assert_eq!(
                    function.name, "face",
                    "{label}: __sel default_expr must be the hoisted face(b, name) ctor"
                ),
                other => panic!(
                    "{label}: __sel {} default_expr must be a face FunctionCall, got {:?}",
                    c.id.member, other
                ),
            }
        }
    }

    // (b) The enclosing template's content_hash is byte-identical across
    //     recompiles — the cache-key stability that design-decision-5's
    //     "serialize/deserialize round-trip correctness" reduces to. Locks the
    //     hoist's __sel* id minting as deterministic.
    assert_eq!(
        t1.content_hash, t2.content_hash,
        "Harness template content_hash must be deterministic across recompiles \
         (hoisted __sel* ids must be minted deterministically)"
    );

    // (c) The nested selector field resolves to a Value::Selector after eval.
    let mut engine = make_simple_engine();
    let result = engine.eval(&m1);
    let h = result
        .values
        .get(&ValueCellId::new("Harness", "h"))
        .unwrap_or_else(|| panic!("Harness.h missing from eval result"));
    let data = match h {
        Value::StructureInstance(d) => d,
        other => panic!("Harness.h must be a Value::StructureInstance, got {other:?}"),
    };
    let sel = field(&data.fields, "sel").unwrap_or_else(|| {
        panic!(
            "Holder instance must carry the `sel` field; fields: {:?}",
            data.fields
        )
    });
    assert!(
        matches!(sel, Value::Selector(_)),
        "Holder.sel must evaluate to Value::Selector once the nested \
         face(b, \"x_max\") ctor is hoisted to a top-level __sel* cell; got {sel:?}"
    );
}
