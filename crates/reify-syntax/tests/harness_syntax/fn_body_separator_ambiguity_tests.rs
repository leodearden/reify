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

/// The STRUCTURAL complement of the above: a function whose body carries a CST fault must be
/// REFUSED — absent from `declarations` — not lowered to a plausible-looking declaration that
/// quietly lost the malformed binding.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// This is the property `lower_function_checked` introduced: the fn arms were the sole member
/// kind lowered UNGUARDED, so a nested MISSING node evaporated the `let` and left behind a
/// `Declaration::Function` whose body no longer corresponded to its source. Its sibling above
/// pins the DIAGNOSTIC half; stating this half as a disjunction with that one ("a diagnostic
/// was emitted OR the binding survived") would make it unfailable, since the sibling already
/// asserts the first disjunct outright for this very fixture.
///
/// Kept structural (no eval) deliberately — `reify-syntax`'s dev-deps cannot enable
/// `reify-test-support`'s `eval-helpers` feature. INV-SF-7's value half is enforced in
/// `reify-eval`'s `fn_body_separator_value_faithfulness` module.
#[test]
fn a_faulty_fn_body_is_refused_rather_than_lowered_without_its_binding() {
    let src = "fn f(x: Int) -> Int { let y = ; x }";
    assert!(src.contains("let "), "fixture must contain a fn-body let");

    let module = reify_syntax::parse(src, ModulePath::single("t"));

    let lowered: Vec<(&str, usize)> = module
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Function(f) => Some((
                f.name.as_str(),
                f.body.as_ref().map_or(0, |b| b.let_bindings.len()),
            )),
            _ => None,
        })
        .collect();

    assert!(
        lowered.is_empty(),
        "INV-SF-7 violated — the body of `f` carries a CST fault, so its `let y` binding could \
         not be lowered; the declaration must therefore be REFUSED rather than pushed without \
         it. Lowered instead as (name, let_bindings): {lowered:?}. A caller reading this AST \
         sees a function whose body disagrees with its source.\n\
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
/// (constraint / port / connect / guarded-block bodies) and `check_and_lower!` were moved onto
/// one-line, bounded, fault-located diagnostics separately, by task #6156.
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

/// A badly broken file is bounded, and the truncation is announced.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// `diagnose_error_node` emits at most `MAX_DIAGNOSTICS` (8) reports per `ERROR` node so that
/// a file whose every declaration is broken cannot bury its first real fault under recovery
/// noise. Truncation is never silent: one final `(further errors suppressed)` diagnostic says
/// so, and — the point of the whole change — that note is anchored to the first SUPPRESSED
/// fault rather than to the enclosing node, so it does not smuggle the blob span back in.
///
/// No fixture in `MALFORMED_FN_BODY_SOURCES` reaches the cap (each has a single break), so
/// this is the only test that exercises it.
///
/// Fixture size is empirical, not arbitrary. Recovery does not collapse the whole file into
/// one `ERROR`: it re-syncs periodically, and within a single collapsed run only about every
/// OTHER declaration contributes a distinct anchoring `let` (measured: 12 broken functions
/// yield 5 separator diagnostics from the first node). 24 is the smallest round count that
/// pushes one node past the cap of 8.
///
/// The assertions below are deliberately NOT `suppressed.len() == 1` / `separator <= 8`.
/// `MAX_DIAGNOSTICS` is a PER-ERROR-NODE cap and those are file-global counts, so they pinned
/// a recovery-shape accident rather than the property they named. Measured on this fixture the
/// parse already yields FOUR distinct generic diagnostics — i.e. at least four contributing
/// `ERROR` nodes — and the global count stayed under 8 only because the nodes after the capped
/// one happened to contribute no separator diagnostics at all. Let recovery redistribute those
/// 24 breaks across two capped nodes and the implementation would still be behaving exactly as
/// documented (<=8 per node, one note per capped node) while both assertions failed.
///
/// So this pins what is observable from outside a node boundary, under ANY split:
///
/// - the cap is reached and ANNOUNCED (at least one suppression note);
/// - every note is emitted only AFTER its node actually hit the cap — diagnostics are pushed
///   in walk order, so a note preceded by fewer than `MAX_DIAGNOSTICS` reports means the cap
///   fired early;
/// - the report stays BOUNDED — strictly fewer separator diagnostics than broken functions,
///   which is what removing the cap would blow through (it would emit one per function);
/// - and the cap BOUNDS the report rather than REPLACING it (at least two located reports).
#[test]
fn a_file_of_broken_functions_is_bounded_and_says_so() {
    const FNS: usize = 24;
    /// Mirrors `ts_parser::fault_diagnosis::MAX_DIAGNOSTICS`, which an integration test cannot
    /// name: `ts_parser` is a private module of `reify-syntax`, so nothing inside it is
    /// reachable from here at any visibility short of re-exporting it from the crate root.
    /// Kept as a named constant so the assertions below read as the per-node cap they are,
    /// not as bare magic numbers.
    const MAX_DIAGNOSTICS: usize = 8;

    let mut source = String::new();
    for n in 0..FNS {
        // Each function omits the `;` after its own `let`, so each is an independent
        // absorbing site.
        source.push_str(&format!(
            "fn f{n}(i: Int) -> Real {{\n  let a{n} = {n}\n  a{n} + i\n}}\n"
        ));
    }

    let m = reify_syntax::parse(&source, ModulePath::single("t"));
    let all = triples(&m);
    // Carries each note's INDEX in emission order alongside the note: the per-node cap check
    // below needs the position, and recovering it afterwards by identity comparison is both
    // fussier and less obvious than collecting it here.
    let suppressed: Vec<(usize, &reify_ast::ParseError)> = m
        .errors
        .iter()
        .enumerate()
        .filter(|(_, e)| e.message.contains("further errors suppressed"))
        .collect();
    assert!(
        !suppressed.is_empty(),
        "{FNS} independently broken functions must trip the per-ERROR-node diagnostic cap on \
         at least one node, so the truncation is announced rather than silent; got none. If \
         this fires because recovery started splitting the file into smaller `ERROR` nodes \
         (none of which reaches {MAX_DIAGNOSTICS} anchoring `let`s), the fixture — not the \
         cap — is what needs to grow: raise FNS until one node caps again.\n\
         diagnostics: {all:?}",
    );

    // Diagnostics are pushed in walk order and a node's suppression note is the LAST report it
    // emits, so a note appearing before `MAX_DIAGNOSTICS` reports have been pushed means the
    // cap fired early. This is the per-node bound restated in the only terms a caller outside
    // `diagnose_error_node` can actually observe.
    for &(idx, _) in &suppressed {
        assert!(
            idx >= MAX_DIAGNOSTICS,
            "a `(further errors suppressed)` note was emitted at index {idx}, before its node \
             could have reached the cap of {MAX_DIAGNOSTICS} — truncation must announce a cap \
             that was actually hit.\ndiagnostics: {all:?}",
        );
    }

    let separator = m
        .errors
        .iter()
        .filter(|e| e.message.contains("';'"))
        .count();
    assert!(
        separator < FNS,
        "the cap must BOUND the report: {FNS} broken functions produced {separator} separator \
         diagnostics, i.e. roughly one per function, so a badly broken file buries its first \
         fault under recovery noise.\ndiagnostics: {all:?}",
    );
    assert!(
        separator >= 2,
        "the cap must bound the report, not replace it: expected several located separator \
         diagnostics alongside the suppression note, got {separator}.\ndiagnostics: {all:?}",
    );

    // The suppression note must be LOCATED, not a whole-file blob. It is anchored at the first
    // fault it declined to report, so it sits strictly inside the source and is short.
    let note = suppressed[0].1;
    assert!(
        note.span.start > 0 && (note.span.end as usize) < source.len(),
        "the suppression note spans the whole collapsed node ({}..{} of {} bytes) — that is \
         precisely the blob span this class of diagnostic exists to eliminate.",
        note.span.start,
        note.span.end,
        source.len(),
    );
    assert!(
        note.span.end - note.span.start <= 40,
        "the suppression note should point at a single suppressed fault, but spans {} bytes.",
        note.span.end - note.span.start,
    );
    assert!(
        !note.message.contains('\n'),
        "the suppression note echoes source rather than describing the truncation: {:?}",
        note.message,
    );
}

/// A structure MEMBER `let` swept into a collapsed function's `ERROR` node must never be
/// blamed for a missing `;`.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// The sibling unit test `let_anchors_distinguish_fn_bindings_from_member_declarations` pins
/// the CLASSIFIER; this pins the user-visible consequence, which is the thing that was
/// actually wrong: only `fn_let_binding` requires a `;`, so "missing ';' after `let` binding in
/// function body" pointed at a structure member names a construct that is not a function body
/// AND advises an edit the grammar rejects — a user who takes the advice gets a second error.
///
/// Both fixtures are MEASURED to have teeth, not assumed to. In each, the unterminated `(` in
/// `let y = (1` collapses the function AND the member `let a` that follows it into ONE `ERROR`
/// whose children are bare tokens, so neither `let` has a `fn_let_binding` / `let_declaration`
/// parent to be classified by and both fall to the positional fallback; and the member `let`'s
/// own line is followed by a further fault on a LATER row, so the `fault.row > let.row` guard
/// in `diagnose_error_node` does not save it either. Against the latched implementation these
/// produced, verbatim, `missing ';' after `let` binding in function body` spanning 47..59 and
/// 47..64 — anchored at `let a`.
///
/// That last condition is why a shorter fixture will not do. Most malformed member-`let`
/// shapes are rescued incidentally by the row guard, so they pass either way and pin nothing;
/// these two are the ones where the classifier is load-bearing.
///
/// Deliberately asserts only the NEGATIVE. Which generic diagnostics this debris produces, and
/// where, is a recovery detail the test does not control; that a member `let` is not accused of
/// a missing separator is the contract.
#[test]
fn a_member_let_following_a_collapsed_fn_is_never_blamed_for_a_missing_separator() {
    let cases: &[(&str, &str)] = &[
        (
            "bare trailing expression",
            "structure T {\n  fn g() -> Int { let y = (1 }\n  let a = 3\n  b\n}\n",
        ),
        (
            "call RHS then a binary expression",
            "structure T {\n  fn g() -> Int { let y = (1 }\n  let a = cos(0)\n  a * 2\n}\n",
        ),
    ];

    for (label, src) in cases {
        let member_let = src.find("let a").expect("fixture must contain `let a`") as u32;

        let m = reify_syntax::parse(src, ModulePath::single("t"));
        let all = triples(&m);

        // Precondition: the fixture still reaches the lowering with a fault to describe. If
        // recovery ever starts preserving the member `let_declaration` here, this fixture stops
        // exercising the positional fallback and must be re-derived rather than quietly kept.
        assert!(
            !m.errors.is_empty(),
            "{label}: precondition failed — this fixture is meant to be malformed, but parsed \
             with no diagnostics at all.\nsource:\n{src}",
        );

        for (message, start, end) in &all {
            assert!(
                !(message.contains("';'") && *start >= member_let),
                "{label}: INV-SF-7 violated — a missing-separator diagnostic was anchored at or \
                 after the structure member `let a` (byte {member_let}): {message:?} spanning \
                 {start}..{end}. A member `let` is newline-separated and takes no `;`, so this \
                 names a construct that is not a function body and tells the user to make an \
                 edit the grammar rejects — take the advice and you get a second \
                 error.\ndiagnostics: {all:?}\nsource:\n{src}",
            );
        }
    }
}

/// A GENERIC (un-anchorable) fault must still be reported with a TOKEN-PRECISE span.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
///
/// The sibling tests above pin the ANCHORED branch of `diagnose_error_node`, where a fn-body
/// `let` supplies both a cause (`missing ';' after `let` binding …`) and a location. This test
/// pins the other branch. The fixture below — a positional payload pattern `some(v)`, which
/// `tree-sitter-reify`'s `match_pattern` has no production for — collapses inside a structure
/// body with NO fn-body `let` to blame, so it falls through to the generic arm whose message is
/// deliberately content-free (`"syntax error in structure body"`).
///
/// That makes the SPAN the entire information content of the diagnostic, which is why it is
/// pinned here rather than left implicit. Before this task the same input produced
/// `format!("syntax error: {}", node_text(child))` spanning the whole declaration: the message
/// carried the information and the span carried none. The trade is only sound in the direction
/// this task took it if the span is genuinely token-precise — measured on this branch, exactly
/// one diagnostic at bytes 78..79, precisely the payload binder `v` inside `some(v)`.
///
/// Recorded honestly: this test arrives GREEN, because step-4/step-8/step-10 already produce
/// that span. It is kept because `reify-compiler`'s cross-PRD ratchet
/// `enums_chunk_option_smoke.rs::option_payload_binding_pattern_still_fails_to_parse` was
/// re-pointed onto exactly this property (its old message-substring pin was satisfiable only by
/// the source echo removed here), and a property another crate's ratchet depends on must be
/// pinned inside this task's own corpus.
///
/// Broadening the generic MESSAGE is deliberately out of scope. Measured, no snippet choice
/// works: `snippet(fault)` is `"v"` (says nothing), and `snippet(node)` is the declaration
/// header, which `fn_body_parse_error_messages_do_not_echo_source_blocks` forbids.
#[test]
fn an_unanchorable_fault_is_reported_with_a_token_precise_span() {
    let source = "structure def D {\n\
                  param c : Option<Length> = some(3mm)\n\
                  let m = match c { some(v) => v, none => 0mm }\n\
                  }";

    // Offsets via `str::find` — never hard-coded (convention from `auto_type_arg_tests.rs`).
    let match_start = source
        .find("match c")
        .expect("fixture must contain 'match c'") as u32;
    let match_end = (source
        .find("0mm }")
        .expect("fixture must contain the match's final arm '0mm }'")
        + "0mm }".len()) as u32;
    let let_m = source.find("let m").expect("fixture must contain 'let m'") as u32;

    let m = reify_syntax::parse(source, ModulePath::single("t"));

    // (a) The fault must be reported at all — this is the branch with no `let` to blame, and
    // falling silent here is the very zero-diagnostic failure mode this task exists to close.
    assert!(
        !m.errors.is_empty(),
        "INV-SF-7 violated — a positional payload pattern `some(v)` has no grammar production, \
         yet the parse produced NO diagnostic.\nsource:\n{source}",
    );

    // (b) At least one diagnostic must sit INSIDE the `match` expression. With the message
    // deliberately generic, the span is the only thing telling a user where to look.
    let inside: Vec<_> = m
        .errors
        .iter()
        .filter(|e| e.span.start >= match_start && e.span.end <= match_end)
        .collect();
    assert!(
        !inside.is_empty(),
        "expected a diagnostic inside the `match` expression (bytes {match_start}..{match_end}); \
         every one lies outside it, so the only located evidence about this fault points \
         somewhere else.\ngot: {:?}",
        triples(&m),
    );

    // (c) And it must be NARROW: a whole-declaration blob also "contains" the match, so
    // containment alone is not evidence. It must additionally start after `let m` — the blob
    // span this class of diagnostic exists to eliminate began at or before it.
    let precise = inside
        .iter()
        .any(|e| e.span.start > let_m && e.span.end - e.span.start <= 40);
    assert!(
        precise,
        "expected a TOKEN-PRECISE diagnostic — starting after `let m` (byte {let_m}) and \
         spanning at most 40 bytes. Every in-range diagnostic is either anchored at or before \
         the `let` or spans a whole region; a blob span carries no more information than the \
         file name.\ngot: {:?}",
        triples(&m),
    );

    // (d) The message must still be a one-liner, not echoed source (mechanism M3).
    for e in &m.errors {
        assert!(
            !e.message.contains('\n') && e.message.len() <= 200,
            "generic-branch diagnostic echoes source rather than describing the fault: {:?}",
            (&e.message, e.span.start, e.span.end),
        );
    }
}


/// A SECOND independently-broken declaration inside ONE collapsed `ERROR` node must get its
/// own located report, even when neither fault has an fn-body `let` to blame.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// `diagnose_error_node` was rewritten so that one collapsed node is not treated as one fault
/// — but only on the arm that CAN name a cause. The arm that cannot was capped at a single
/// report per node, so every later broken declaration inside the same node was still dropped
/// in silence: the same defect, on the other arm.
///
/// The fixture is MEASURED to have teeth. Both `fn`s here are broken by an unterminated `(`,
/// which recovery collapses — together with the member `let`s between them — into ONE
/// `ERROR` spanning the whole structure body (asserted below, so a grammar change that starts
/// splitting them makes this test say so rather than pass vacuously). Neither fault is
/// anchorable: the nearest preceding `let` in each case is a structure MEMBER binding, which
/// `LetAnchor::in_fn_body` refuses to blame for a missing `;`. Against the once-per-node
/// implementation this source produced exactly ONE diagnostic, at bytes 51..52 — `fn h`'s
/// break, forty bytes later, went entirely unreported.
///
/// Asserts one report per broken declaration, not an exact count: how much debris recovery
/// leaves around each break is a grammar detail this test does not control.
#[test]
fn a_second_broken_declaration_in_one_collapsed_node_is_not_dropped() {
    let source = "structure T {\n  fn g() -> Int { let y = (1 }\n  let a = 3\n  fn h() -> Int { let z = (2 }\n  let b = 4\n}\n";

    // Offsets via `str::find` — never hard-coded (convention from `auto_type_arg_tests.rs`).
    let second_fn = source.find("fn h").expect("fixture must contain 'fn h'") as u32;

    // Precondition: the two breaks really are fused into ONE `ERROR`. Reported per-node
    // deduplication is only observable while that holds.
    let mut ts = make_ts_parser();
    let tree = ts.parse(source, None).expect("tree-sitter parse failed");
    let mut outermost = Vec::new();
    collect_outermost_faults(tree.root_node(), &mut outermost);
    assert!(
        outermost.len() == 1 && outermost[0].0 < second_fn && outermost[0].1 > second_fn,
        "precondition failed — the fixture no longer collapses both broken functions into a \
         single ERROR node, so it cannot exercise per-node deduplication at all. Outermost \
         fault nodes: {outermost:?}; `fn h` starts at byte {second_fn}.\nsource:\n{source}",
    );

    let m = reify_syntax::parse(source, ModulePath::single("t"));
    let before: Vec<_> = m.errors.iter().filter(|e| e.span.end <= second_fn).collect();
    let after: Vec<_> = m.errors.iter().filter(|e| e.span.start >= second_fn).collect();

    assert!(
        !before.is_empty(),
        "the FIRST broken function produced no located diagnostic.\ngot: {:?}",
        triples(&m),
    );
    assert!(
        !after.is_empty(),
        "INV-SF-7 — the second broken function (from byte {second_fn}) produced NO diagnostic: \
         recovery fused it into the first one's ERROR node and the report was deduplicated \
         away, leaving a broken declaration silently undiagnosed.\ngot: {:?}",
        triples(&m),
    );

    for e in &m.errors {
        assert!(
            !e.message.contains('\n'),
            "diagnostic echoes source rather than describing the fault (mechanism M3): {:?}",
            (&e.message, e.span.start, e.span.end),
        );
    }
}

/// Byte ranges of the OUTERMOST `ERROR`/`MISSING` nodes in `node`'s subtree — the nodes
/// `diagnose_error_node` is invoked on, so this is how a test states "recovery collapsed
/// these breaks into one node" in the terms the production code sees.
fn collect_outermost_faults(node: tree_sitter::Node<'_>, out: &mut Vec<(u32, u32)>) {
    if node.is_error() || node.is_missing() {
        out.push((node.start_byte() as u32, node.end_byte() as u32));
        return;
    }
    if !node.has_error() {
        return;
    }
    for i in 0..node.child_count() {
        collect_outermost_faults(node.child(i).expect("child index in range"), out);
    }
}
