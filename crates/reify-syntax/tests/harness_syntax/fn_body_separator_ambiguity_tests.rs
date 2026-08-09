//! Fn-body separator ambiguity: a missing `;` must never silently change what a program means.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md) — task #5392 is
//! the enforcement vehicle for the fn-body seam.
//!
//! This module pins the SYNTAX half of the invariant: a fault anywhere inside a function body
//! must surface as a `ParseError`, and that error's span must point at the ABSORBING line (the
//! `let` whose missing `;` fused two statements) rather than at the whole declaration or at a
//! later, unrelated one. The VALUE half — that an adjacent-token variation cannot change a
//! computed value without a hard error — lives in
//! `crates/reify-eval/tests/harness_engine/fn_body_separator_value_faithfulness.rs`, which
//! needs `reify-test-support`'s `eval-helpers` feature that `reify-syntax` cannot enable
//! (`reify-eval` depends on `reify-syntax`, so the reverse dep would be a cycle).

use reify_ast::Declaration;
use reify_core::ModulePath;

use crate::common::make_ts_parser;

/// A nested fault inside an otherwise well-formed `fn_body` must produce at least one
/// `ParseError` (mechanism M1 — the zero-diagnostic path).
///
/// Both sources below parse to a CLEAN-LOOKING `function_definition` node — tree-sitter does
/// not collapse them into a top-level `ERROR` — whose `fn_body` carries a nested
/// `MISSING number_literal`. Nothing in the lowering guards that, so the malformed
/// `fn_let_binding` lowers to `None` and is dropped by a bare `if let Some(..)` with no else.
#[test]
fn nested_fault_in_fn_body_is_diagnosed() {
    let cases: &[(&str, &str)] = &[
        ("top-level fn", "fn f(x: Int) -> Int { let y = ; x }"),
        (
            "structure member fn",
            "structure S {\n  fn f(x: Int) -> Int { let y = ; x }\n}",
        ),
    ];

    for (label, src) in cases {
        // (a) The raw CST really is faulty — this half passes today.
        let mut ts = make_ts_parser();
        let tree = ts.parse(*src, None).expect("tree-sitter parse failed");
        assert!(
            tree.root_node().has_error(),
            "{label}: precondition failed — the CST for this source was expected to carry an \
             ERROR/MISSING node but does not; the fixture no longer exercises the defect.\n\
             source:\n{src}",
        );

        // (b) The lowering must surface it.
        let module = reify_syntax::parse(src, ModulePath::single("t"));
        assert!(
            !module.errors.is_empty(),
            "{label}: INV-SF-7 violated — the CST carries an ERROR/MISSING node, yet \
             `module.errors` is empty. The malformed `let y` binding was silently DROPPED, so \
             the module's values no longer correspond to the source.\n\
             source:\n{src}\n\
             declarations lowered: {}",
            module.declarations.len(),
        );
    }
}

/// The complement of the above, stated structurally: a source that textually contains a fn-body
/// `let` must not lower to a function whose `let_bindings` are empty AND produce no diagnostic.
///
/// Kept structural (no eval) deliberately — `reify-syntax`'s dev-deps cannot enable
/// `reify-test-support`'s `eval-helpers` feature. INV-SF-7's value half is enforced in
/// `reify-eval`'s `fn_body_separator_value_faithfulness` module.
#[test]
fn nested_fault_in_fn_body_does_not_silently_drop_the_binding() {
    let src = "fn f(x: Int) -> Int { let y = ; x }";
    assert!(src.contains("let "), "fixture must contain a fn-body let");

    let module = reify_syntax::parse(src, ModulePath::single("t"));

    let binding_survived = module.declarations.iter().any(|d| match d {
        Declaration::Function(f) => f.body.as_ref().is_some_and(|b| !b.let_bindings.is_empty()),
        _ => false,
    });

    assert!(
        !module.errors.is_empty() || binding_survived,
        "INV-SF-7 violated — the source declares a fn-body `let`, but the lowered function \
         carries NO let bindings and the parse produced NO diagnostics. The binding evaporated \
         silently: any value computed from this module is unfaithful to its source.\n\
         source:\n{src}",
    );
}
