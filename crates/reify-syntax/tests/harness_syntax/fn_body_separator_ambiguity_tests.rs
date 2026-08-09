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

/// Render every diagnostic as a `(message, start, end)` triple so a failure is diagnosable
/// from the test output alone.
fn triples(m: &reify_ast::ParsedModule) -> Vec<(&str, u32, u32)> {
    m.errors
        .iter()
        .map(|e| (e.message.as_str(), e.span.start, e.span.end))
        .collect()
}

/// The missing-separator diagnostic must NAME the cause and point at the ABSORBING `let`
/// line — not at the whole declaration, and not at the absorbed line one row later.
///
/// This is the verbatim t9 shape from the probe. Measured: tree-sitter collapses the entire
/// `function_definition` into `(ERROR [3,0]-[6,1])`, inside which recovery fuses the `let`
/// RHS with the following line into one `binary_expression`. Reporting that whole blob is
/// what made the error appear to point at an unrelated later line.
#[test]
fn missing_semicolon_after_fn_let_is_located_at_the_let_line() {
    let source = "structure T {\n  let v = f(1)\n}\nfn f(i: Int) -> Real {\n  let x0 = cos(0deg)\n  x0 * sgn(i, 0)\n}\n";

    // Offsets via `str::find` — never hard-coded, so the test does not go stale when the
    // fixture is edited (convention from `auto_type_arg_tests.rs`).
    let let_off = source.find("let x0").expect("fixture must contain 'let x0'") as u32;
    let absorbed_end = (source.find("x0 * sgn").expect("fixture must contain 'x0 * sgn'")
        + "x0 * sgn(i, 0)".len()) as u32;
    let fn_kw_off = source.find("fn f(").expect("fixture must contain 'fn f('") as u32;

    let m = reify_syntax::parse(source, ModulePath::single("t"));

    // (a) Something must be reported at all.
    assert!(
        !m.errors.is_empty(),
        "INV-SF-7 violated — omitting the `;` after `let x0` produced NO diagnostic.\n\
         source:\n{source}",
    );

    // (b) The message must explain the actual cause, not emit a generic "syntax error".
    let separator_errors: Vec<_> = m
        .errors
        .iter()
        .filter(|e| e.message.contains("';'") && e.message.contains("let"))
        .collect();
    assert!(
        !separator_errors.is_empty(),
        "expected a diagnostic naming the missing `;` after a `let` binding; the reported \
         errors explain nothing actionable.\n\
         got: {:?}",
        triples(&m),
    );

    // (c) That diagnostic must be LOCAL to the absorbing region: it may not start at the
    // `fn` keyword (whole-declaration blob) nor run past the absorbed expression.
    let localised = separator_errors
        .iter()
        .any(|e| e.span.start >= let_off && e.span.end <= absorbed_end);
    assert!(
        localised,
        "expected a separator diagnostic whose span lies inside the absorbing region \
         (bytes {let_off}..{absorbed_end} — from `let x0` through the absorbed \
         `x0 * sgn(i, 0)`); every one either starts before the `let` or runs past the \
         absorbed expression.\n\
         got: {:?}",
        triples(&m),
    );

    // (d) Belt and braces: the whole-declaration blob span must be gone entirely.
    let blob = m.errors.iter().find(|e| e.span.start < fn_kw_off + 1);
    assert!(
        blob.is_none(),
        "a diagnostic still starts at or before the `fn` keyword (byte {fn_kw_off}), i.e. it \
         spans the whole declaration rather than the absorbing line.\n\
         offender: {:?}\n\
         got: {:?}",
        blob.map(|e| (&e.message, e.span.start, e.span.end)),
        triples(&m),
    );
}
