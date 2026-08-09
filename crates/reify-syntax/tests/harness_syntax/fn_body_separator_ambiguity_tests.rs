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

/// The malformed fn-body sources this module governs. Shared by the message-quality sweep so
/// a new fixture added to one assertion is covered by the other.
const MALFORMED_FN_BODY_SOURCES: &[(&str, &str)] = &[
    (
        "nested missing value",
        "fn f(x: Int) -> Int { let y = ; x }",
    ),
    (
        "nested missing value, structure member",
        "structure S {\n  fn f(x: Int) -> Int { let y = ; x }\n}",
    ),
    (
        "t9 shape — ident-led absorbed line",
        "structure T {\n  let v = f(1)\n}\nfn f(i: Int) -> Real {\n  let x0 = cos(0deg)\n  x0 * sgn(i, 0)\n}\n",
    ),
    (
        "quantity-led absorbed line",
        "fn f() -> Length {\n  let a = 2mm\n  3mm * a\n}\n",
    ),
    (
        "unary-minus-led absorbed line",
        "fn f(i: Int) -> Real {\n  let a = 2\n  -a\n}\n",
    ),
    (
        "large body, ~15 lines, separator omitted after the first let",
        "fn f(i: Int) -> Real {\n  let a0 = 1\n  let a1 = 2;\n  let a2 = 3;\n  let a3 = 4;\n  let a4 = 5;\n  let a5 = 6;\n  let a6 = 7;\n  let a7 = 8;\n  let a8 = 9;\n  let a9 = 10;\n  let b0 = 11;\n  let b1 = 12;\n  a0 + a1 + i\n}\n",
    ),
];

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
    let let_off = source
        .find("let x0")
        .expect("fixture must contain 'let x0'") as u32;
    let absorbed_end = (source
        .find("x0 * sgn")
        .expect("fixture must contain 'x0 * sgn'")
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

/// The same fn-body fault in MEMBER position gets the same located, non-echoing diagnostic.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
///
/// A structure member `fn` collapses into an `ERROR` inside the structure body, which is a
/// DIFFERENT dispatch arm from the top-level collapse the sibling tests cover: the members
/// loop matches `"ERROR"` itself and so shadows `lower_member`'s own `"ERROR"` arm. Measured
/// before that arm was converted, this fixture reported
/// `2:3: syntax error: fn f(i: Int) -> Real {\n…` — five lines of echoed source, anchored to
/// the `fn` header instead of the absorbing `let`, and running past the end of the function to
/// swallow the following `let v = 1` member. A user reading it would edit the wrong line.
#[test]
fn missing_semicolon_in_a_member_fn_is_located_and_does_not_swallow_later_members() {
    let source = "structure T {\n  fn f(i: Int) -> Real {\n    let x0 = 1\n    x0 * 2\n  }\n  let v = 1\n}\n";

    // Offsets via `str::find` — never hard-coded (convention from `auto_type_arg_tests.rs`).
    let let_off = source
        .find("let x0")
        .expect("fixture must contain 'let x0'") as u32;
    let absorbed_end = (source
        .find("x0 * 2")
        .expect("fixture must contain 'x0 * 2'")
        + "x0 * 2".len()) as u32;
    let fn_kw_off = source.find("fn f(").expect("fixture must contain 'fn f('") as u32;
    let later_member = source
        .find("let v = 1")
        .expect("fixture must contain 'let v = 1'") as u32;

    let m = reify_syntax::parse(source, ModulePath::single("t"));

    // (a) Something must be reported at all.
    assert!(
        !m.errors.is_empty(),
        "INV-SF-7 violated — omitting the `;` after `let x0` in a MEMBER fn produced NO \
         diagnostic.\nsource:\n{source}",
    );

    // (b) The message must name the actual cause, exactly as in top-level position.
    let separator_errors: Vec<_> = m
        .errors
        .iter()
        .filter(|e| e.message.contains("';'") && e.message.contains("let"))
        .collect();
    assert!(
        !separator_errors.is_empty(),
        "expected a diagnostic naming the missing `;` after a `let` binding in a member fn; \
         a member-position fault must not be reported more vaguely than the top-level one.\n\
         got: {:?}",
        triples(&m),
    );

    // (c) It must be LOCAL to the absorbing region, not anchored to the `fn` header.
    let localised = separator_errors
        .iter()
        .any(|e| e.span.start >= let_off && e.span.end <= absorbed_end);
    assert!(
        localised,
        "expected a separator diagnostic whose span lies inside the absorbing region \
         (bytes {let_off}..{absorbed_end}); every one either starts before the `let` (the `fn` \
         header / whole-member blob) or runs past the absorbed expression.\n\
         got: {:?}",
        triples(&m),
    );

    // (d) The whole-member blob span must be gone: nothing may start at or before `fn`.
    let blob = m.errors.iter().find(|e| e.span.start <= fn_kw_off);
    assert!(
        blob.is_none(),
        "a diagnostic still starts at or before the `fn` keyword (byte {fn_kw_off}), i.e. it \
         spans the whole member rather than the absorbing line.\n\
         offender: {:?}\ngot: {:?}",
        blob.map(|e| (&e.message, e.span.start, e.span.end)),
        triples(&m),
    );

    // (e) No diagnostic may SPAN from the broken member across into the following one: that
    // blob span is what made a later, entirely well-formed `let v = 1` look broken too.
    let swallower = m
        .errors
        .iter()
        .find(|e| e.span.start < later_member && e.span.end > later_member);
    assert!(
        swallower.is_none(),
        "a diagnostic spans from the broken member across into the following, well-formed \
         `let v = 1` (byte {later_member}); the fault must stay inside the member that has it.\n\
         offender: {:?}\ngot: {:?}",
        swallower.map(|e| (&e.message, e.span.start, e.span.end)),
        triples(&m),
    );

    // (e2) And no diagnostic may CLAIM a missing `;` against that following member. Recovery
    // debris does land there (tree-sitter emits a second ERROR at `v`), so a narrow generic
    // "syntax error in structure body" is honest and permitted — but "missing ';' after `let`"
    // is a specific causal claim, and `let v = 1` has no separator problem. Misattributing it
    // sends the user to edit a line that is already correct.
    let misattributed = m
        .errors
        .iter()
        .find(|e| e.message.contains("';'") && e.span.start >= later_member);
    assert!(
        misattributed.is_none(),
        "a missing-`;` diagnostic is anchored at the following, well-formed `let v = 1` \
         (byte {later_member}) — that member has no separator problem; only the earlier \
         `let x0` does.\noffender: {:?}\ngot: {:?}",
        misattributed.map(|e| (&e.message, e.span.start, e.span.end)),
        triples(&m),
    );

    // (f) And it must not echo a block of source (mechanism M3), matching the top-level class.
    for e in &m.errors {
        assert!(
            !e.message.contains('\n') && e.message.len() <= 200 && !e.message.contains("fn f("),
            "member-position diagnostic echoes source instead of describing the fault: {:?}",
            (&e.message, e.span.start, e.span.end),
        );
    }
}

/// Two sibling functions, each missing its own separator, must each get their OWN diagnostic
/// inside their OWN declaration.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
///
/// Measured: tree-sitter collapses BOTH declarations into a single `(ERROR [0,0]-[7,1])`
/// carrying two independent fault descendants (`(ERROR [2,2]-[2,3])` for `g`'s absorbed line
/// and `(ERROR [6,2]-[6,3])` for `h`'s). Reporting only the first — or worse, the enclosing
/// blob — is exactly how the probe saw a parse error attributed to an unrelated later line.
/// One collapsed ERROR node is not one fault.
#[test]
fn t9_t20_sibling_fns_each_get_their_own_error() {
    let source = "fn g(i: Int) -> Real {\n  let a = 2\n  a * sgn(i, 0)\n}\nfn h(i: Int) -> Real {\n  let b = 3\n  b + 1\n}\n";

    // Offsets via `str::find`, never hard-coded.
    let let_a = source.find("let a").expect("fixture must contain 'let a'") as u32;
    let let_b = source.find("let b").expect("fixture must contain 'let b'") as u32;
    let fn_h = source.find("fn h(").expect("fixture must contain 'fn h('") as u32;

    let m = reify_syntax::parse(source, ModulePath::single("t"));

    let separator: Vec<_> = m
        .errors
        .iter()
        .filter(|e| e.message.contains("';'"))
        .collect();

    // (a) One diagnostic per absorbing `let` — not one blob for the whole file.
    assert_eq!(
        separator.len(),
        2,
        "INV-SF-7 violated — two sibling functions each omit the `;` after their own `let`, so \
         two separator diagnostics are required; got {}. A single collapsed ERROR node is not a \
         single fault.\n\
         got: {:?}",
        separator.len(),
        triples(&m),
    );

    // (b) `g`'s fault stays inside `g` and does not leak into `h`'s lines.
    let in_g = separator
        .iter()
        .filter(|e| e.span.start >= let_a && e.span.end <= fn_h)
        .count();
    assert_eq!(
        in_g,
        1,
        "expected exactly one separator diagnostic confined to `g`'s absorbing region \
         (bytes {let_a}..{fn_h}, i.e. from `let a` up to the `fn h` header); got {in_g}.\n\
         got: {:?}",
        triples(&m),
    );

    // (c) `h`'s fault is reported at `h`'s own `let`.
    let in_h = separator.iter().filter(|e| e.span.start >= let_b).count();
    assert_eq!(
        in_h,
        1,
        "expected exactly one separator diagnostic anchored at or after `let b` (byte \
         {let_b}); got {in_h}. `h`'s missing separator has no diagnostic of its own.\n\
         got: {:?}",
        triples(&m),
    );

    // (d) No diagnostic straddles both declarations.
    let straddler = separator
        .iter()
        .find(|e| e.span.start < let_a && e.span.end > let_b);
    assert!(
        straddler.is_none(),
        "a separator diagnostic spans BOTH functions (start < {let_a} and end > {let_b}) — it \
         is the whole-file blob span, so its reported location belongs to neither fault.\n\
         offender: {:?}\n\
         got: {:?}",
        straddler.map(|e| (&e.message, e.span.start, e.span.end)),
        triples(&m),
    );
}

/// Diagnostics for this class must be readable one-liners, never swaths of echoed source
/// (mechanism M3's source-echo half).
///
/// The old `"ERROR"` arms interpolated the whole node's text, so reporting a four-line
/// function produced a four-line message, and a two-function collapse produced one message
/// containing both declarations in full. A diagnostic that reprints the file is not a
/// diagnostic.
///
/// Scoped to the fn-body sources deliberately. The acceptance criteria scope the general
/// parse-error-quality cleanup to "at least for this class"; the remaining ERROR arms
/// (constraint / port / connect / guarded-block bodies) and `check_and_lower!` keep the old
/// shape and are filed as follow-up work.
#[test]
fn fn_body_parse_error_messages_do_not_echo_source_blocks() {
    for (label, src) in MALFORMED_FN_BODY_SOURCES {
        let m = reify_syntax::parse(src, ModulePath::single("t"));
        assert!(
            !m.errors.is_empty(),
            "{label}: fixture produced no diagnostics at all, so it no longer exercises the \
             message-quality property.\nsource:\n{src}",
        );
        for e in &m.errors {
            assert!(
                !e.message.contains('\n'),
                "{label}: diagnostic message spans multiple lines — it is echoing a block of \
                 source rather than describing the fault.\nmessage: {:?}",
                e.message,
            );
            assert!(
                e.message.len() <= 200,
                "{label}: diagnostic message is {} bytes — unbounded source interpolation.\n\
                 message: {:?}",
                e.message.len(),
                e.message,
            );
            assert!(
                !e.message.contains("fn f("),
                "{label}: diagnostic message contains the declaration header it is reporting \
                 about, i.e. it echoes `node_text`.\nmessage: {:?}",
                e.message,
            );
        }
    }
}

/// The mandatory anti-false-positive companion to every negative test above: none of the new
/// guards may start rejecting valid programs.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// Refusing a faulty parse is only half the invariant — a parser that refused everything
/// would satisfy the negative tests trivially while being useless. The `function_signature`
/// and quantity-literal cases are the two specifically at risk: a bodyless trait signature
/// legitimately lowers to `body: None` and must not trip the fn-arm fault guard, and
/// well-formed juxtaposition like `2mm * 3` must not trip the `let`-anchored separator
/// diagnostic.
#[test]
fn well_formed_fn_bodies_produce_no_diagnostics() {
    let cases: &[(&str, &str)] = &[
        (
            "one separated let",
            "fn f(x: Int) -> Int { let y = 2; y + x }",
        ),
        (
            "several separated lets, multi-line",
            "fn f(x: Int) -> Int {\n  let a = 1;\n  let b = 2;\n  let c = 3;\n  a + b + c + x\n}\n",
        ),
        ("no lets at all", "fn f(x: Int) -> Int { x + 1 }"),
        ("expression body", "fn f(x: Int) -> Int = x + 1"),
        ("typed let", "fn f(x: Int) -> Int { let y: Int = 2; y + x }"),
        (
            "fn member inside a structure",
            "structure S {\n  fn f(x: Int) -> Int { let y = 2; y + x }\n}\n",
        ),
        (
            "trait default body plus a bodyless signature",
            "trait T {\n  fn provided(x: Int) -> Int { let y = 2; y + x }\n  fn required(x: Int) -> Int\n}\n",
        ),
        (
            "well-formed quantity-literal juxtaposition",
            "fn f() -> Length {\n  let a = 2mm;\n  a * 3\n}\n",
        ),
        (
            "two well-formed sibling fns",
            "fn g(i: Int) -> Int {\n  let a = 2;\n  a * i\n}\nfn h(i: Int) -> Int {\n  let b = 3;\n  b + i\n}\n",
        ),
    ];

    for (label, src) in cases {
        let m = reify_syntax::parse(src, ModulePath::single("t"));
        assert!(
            m.errors.is_empty(),
            "{label}: a well-formed function body was rejected — the fn-body fault guards are \
             over-eager, which breaks valid programs.\n\
             diagnostics: {:?}\n\
             source:\n{src}",
            triples(&m),
        );
    }
}
