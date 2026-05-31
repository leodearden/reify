//! Tensegrity T1a — `solver::form_find` anchored Force-Density form-finding.
//!
//! PRD: `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1a. This is
//! the first real consumer slice through the landed ComputeNode seam (GR-002):
//! `@optimized("solver::form_find")` lowers to a ComputeNode whose trampoline
//! cracks the Tensegrity / force-densities / anchors `Value`s, calls the pure
//! FD kernel in `reify-solver-elastic`, and rebuilds a `FormFindResult`.
//!
//! Test layers (TDD order):
//!   step-7  — stdlib declaration of `form_find` / `FormFindResult` type-checks
//!   step-9  — trampoline-unit tests (crafted Values, no compile pipeline)
//!   step-11 — end-to-end + cache-hit + CLI smoke over the cable-net example

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

// ── step-7: stdlib declaration type-checks ───────────────────────────────────

/// `form_find(structure, force_densities, anchors) -> FormFindResult` and the
/// `FormFindResult.nodes` projection must be declared in the stdlib. Free node 0
/// is cabled to four anchors; all three call args are let-bound (the ComputeNode
/// shallow-walk capture contract — see step-12), though here we only require the
/// source to compile and the call to resolve to a `FormFindResult`.
///
/// RED→GREEN signal: Reify resolves an *undeclared* call leniently to `Undef`
/// (no Error diagnostic — only a benign empty-list warning for `struts: []`), so
/// "no Error diagnostics" alone can never fail. The real signal is the eval
/// result: with `form_find` declared but no trampoline registered here, the
/// `@optimized` call body-inlines its `FormFindResult()` fallback
/// (`engine_eval.rs` only dispatches a ComputeNode when a trampoline exists), so
/// `form` is a `FormFindResult` instance. While `form_find` is undeclared the
/// call is `Undef` and the match below fails RED.
#[test]
fn form_find_stdlib_declaration_type_checks() {
    const SOURCE: &str = r#"
structure def F {
    let t = Tensegrity(
        nodes: [
            point3(0m, 0m, 0m),
            point3(1m, 0m, 0m),
            point3(-1m, 0m, 0m),
            point3(0m, 1m, 1m),
            point3(0m, -1m, 1m)
        ],
        struts: [],
        cables: [[0, 1], [0, 2], [0, 3], [0, 4]]
    )
    let q = [1.0, 1.0, 1.0, 1.0]
    let a = [1, 2, 3, 4]
    let form = form_find(t, q, a)
    let ns = form.nodes
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);

    // Invariant: declaring `form_find` / `FormFindResult` must not introduce any
    // Error-severity diagnostic. (This cannot go RED on its own — an undeclared
    // call is lenient — but it guards against a malformed step-8 stdlib edit.)
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "form_find / FormFindResult stdlib declaration should compile without \
         Error-severity diagnostics; got {} error(s): {:#?}",
        errors.len(),
        errors,
    );

    // Signal: `form` resolves to a `FormFindResult` instance (inline-body
    // fallback, since no trampoline is registered in this test).
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let form = result
        .values
        .get(&ValueCellId::new("F", "form"))
        .unwrap_or_else(|| panic!("F.form cell missing from eval result"));
    match form {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "FormFindResult",
                "form_find should return a FormFindResult; got StructureInstance {:?}",
                data.type_name,
            );
            // `nodes` must be a declared param of FormFindResult (the `form.nodes`
            // projection above type-checks against it).
            assert!(
                data.fields.get(&"nodes".to_string()).is_some(),
                "FormFindResult should declare a `nodes` field; fields: {:?}",
                data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            );
        }
        other => panic!(
            "form_find(t, q, a) should evaluate to a FormFindResult StructureInstance \
             (declared in stdlib); got {other:?} — step-8 not yet implemented"
        ),
    }
}
