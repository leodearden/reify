//! Fn-body separator ambiguity, VALUE layer: an adjacent-token variation may never change a
//! computed value without a hard error.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md) — task #5392 is
//! the enforcement vehicle for the fn-body seam.
//!
//! The syntax half (a fault is diagnosed, and its span points at the ABSORBING `let` line)
//! lives in `crates/reify-syntax/tests/harness_syntax/fn_body_separator_ambiguity_tests.rs`.
//! This module pins the half that actually matters to a user: for every way the token after a
//! separator-less `let` can begin, the outcome must be EITHER a hard error diagnostic OR the
//! same value the separated twin computes. "Clean compile, different number" is the failure
//! this task exists to make impossible.
//!
//! It lives in `reify-eval` rather than `reify-syntax` because it needs
//! `reify-test-support`'s `eval-helpers` feature, which `reify-syntax` cannot enable:
//! `reify-eval` depends on `reify-syntax`, so the reverse dependency would be a cycle.

use reify_core::{ModulePath, ValueCellId};
use reify_ir::Value;
use reify_test_support::{
    cell_value, compile_source_with_stdlib_allow_parse_errors, error_diags, eval_source,
    make_engine,
};

/// One adjacent-token shape, paired with the program the author plainly meant.
struct Variant {
    /// Variant name, printed in every failure message.
    name: &'static str,
    /// The malformed source: a fn-body `let` with no `;` before the next line.
    no_sep: &'static str,
    /// The intended reading. For every variant but `nested_missing` this differs from
    /// `no_sep` by exactly the inserted `;` — see [`twins_differ_only_by_the_separator`],
    /// which mechanically enforces that rather than trusting the eye.
    intended: &'static str,
    /// `false` only for `nested_missing`, whose malformation is a deleted RHS rather than a
    /// deleted separator, so no separator-only twin exists for it.
    separator_only: bool,
    /// The value cell to compare across the twins.
    structure: &'static str,
    member: &'static str,
}

/// The four-outcome matrix from the field report — one entry per way the ABSORBED line can
/// begin — plus the multi-declaration collapse and the nested-MISSING path.
const VARIANTS: &[Variant] = &[
    // Absorbed line starts with an identifier: `2.0 <NL> x0 * 3.0` fuses into `2.0 x0 * 3.0`.
    Variant {
        name: "ident_led",
        no_sep: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let x0 = 2.0\n  x0 * 3.0\n}\n",
        intended: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let x0 = 2.0;\n  x0 * 3.0\n}\n",
        separator_only: true,
        structure: "T",
        member: "v",
    },
    // Absorbed line starts with `-`: the classic absorption, where `let a = 2.0` followed by
    // `-a` reads as the single subtraction `let a = 2.0 - a`.
    Variant {
        name: "unary_minus",
        no_sep: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let a = 2.0\n  -a\n}\n",
        intended: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let a = 2.0;\n  -a\n}\n",
        separator_only: true,
        structure: "T",
        member: "v",
    },
    // Absorbed line starts with a quantity literal — the juxtaposition shape the report named
    // as its hypothesis for the whole defect.
    Variant {
        name: "quantity_led",
        no_sep: "structure T {\n  let v = f()\n}\nfn f() -> Length {\n  let a = 2.0\n  3mm * a\n}\n",
        intended: "structure T {\n  let v = f()\n}\nfn f() -> Length {\n  let a = 2.0;\n  3mm * a\n}\n",
        separator_only: true,
        structure: "T",
        member: "v",
    },
    // Absorbed line starts with `(` — juxtaposition that reads as a call of the let's RHS.
    Variant {
        name: "paren_led",
        no_sep: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let a = 2.0\n  (a + 1.0)\n}\n",
        intended: "structure T {\n  let v = f()\n}\nfn f() -> Real {\n  let a = 2.0;\n  (a + 1.0)\n}\n",
        separator_only: true,
        structure: "T",
        member: "v",
    },
    // The t9+t20 sibling pair: recovery collapses BOTH declarations into one ERROR node, so a
    // first-fault-only diagnosis leaves the second function's break unreported.
    Variant {
        name: "two_fns",
        no_sep: "structure T {\n  let v = g() + h()\n}\nfn g() -> Real {\n  let a = 2.0\n  a * 3.0\n}\nfn h() -> Real {\n  let b = 4.0\n  b + 1.0\n}\n",
        intended: "structure T {\n  let v = g() + h()\n}\nfn g() -> Real {\n  let a = 2.0;\n  a * 3.0\n}\nfn h() -> Real {\n  let b = 4.0;\n  b + 1.0\n}\n",
        separator_only: true,
        structure: "T",
        member: "v",
    },
    // Mechanism M1: a nested `MISSING` inside an otherwise well-formed `function_definition`.
    // Not a separator omission — the RHS itself is absent — but the same silence: the binding
    // is dropped and the function still lowers, so the value changes with no diagnostic.
    Variant {
        name: "nested_missing",
        no_sep: "structure T {\n  let v = f(1)\n}\nfn f(x: Int) -> Int { let y = ; x }\n",
        intended: "structure T {\n  let v = f(1)\n}\nfn f(x: Int) -> Int { let y = 1; x }\n",
        separator_only: false,
        structure: "T",
        member: "v",
    },
];

/// Guard on the corpus itself: for every separator variant the twin must differ from the
/// malformed source by exactly the inserted `;`.
///
/// Without this, a typo in a fixture could make the twins differ in some OTHER way, and
/// [`adjacent_token_variation_cannot_silently_change_a_value`] would then be comparing two
/// unrelated programs — passing or failing for reasons that have nothing to do with INV-SF-7.
#[test]
fn twins_differ_only_by_the_separator() {
    for v in VARIANTS {
        if !v.separator_only {
            continue;
        }
        assert_eq!(
            v.intended.replace(";\n", "\n"),
            v.no_sep,
            "{}: the twin must differ from the malformed source ONLY by end-of-line `;` \
             separators; stripping them does not recover the malformed source, so the two \
             fixtures are not twins.\n\
             no_sep:\n{}\nintended:\n{}",
            v.name,
            v.no_sep,
            v.intended,
        );
    }
}

/// Compile `source` through the PRODUCTION single-module path — `parse_with_stdlib` +
/// `compile_with_stdlib`, which is what the CLI and GUI do.
///
/// Two test-support helpers are deliberately bypassed here, because each would distort the
/// very measurement under test:
///
/// - `eval_source` routes through `parse_or_panic`, which asserts `parsed.errors.is_empty()`
///   and so panics before any value exists to compare.
/// - `compile_source_with_stdlib_allow_parse_errors` PREPENDS its own `Diagnostic::error` per
///   parse error (`helpers.rs`'s `parse_errors_as_diagnostics`) and THEN calls
///   `compile_with_stdlib`, whose `forward_parse_errors` (`compile_builder/pre_pass.rs`) now
///   pushes an ERROR for each of the same parse errors. Since task #5392 the two agree on
///   severity, so the helper no longer differs in KIND — it differs in COUNT, reporting every
///   parse error twice. A test whose subject is "what diagnostics does a real caller actually
///   see" cannot measure through a helper that doubles them.
///
/// Evaluation is deliberately NOT done here. Every variant in this corpus is refused, so
/// driving the engine over IR lowered from a CST that carries ERROR/MISSING nodes would be
/// wasted work in the common path and a plausible source of unrelated panics that would be
/// misattributed to INV-SF-7. Callers that reach the value-comparison arm call
/// [`eval_as_production_does`] explicitly.
fn compile_as_production_does(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    reify_compiler::compile_with_stdlib(&parsed)
}

/// Evaluate an already-compiled module with the same `MockConstraintChecker` engine the other
/// e2e tests use (`result_fallback_e2e.rs`'s manual pipeline).
fn eval_as_production_does(compiled: &reify_compiler::CompiledModule) -> reify_eval::EvalResult {
    make_engine().eval(compiled)
}

/// Do two values agree?
///
/// Discriminant equality, plus a 1e-12 RELATIVE epsilon on the float-bearing variants. The
/// epsilon is pure defence, not a tuned tolerance: the twins execute identical arithmetic on
/// identical literals, so bit-equality is what is actually expected.
fn values_agree(a: &Value, b: &Value) -> bool {
    fn close(x: f64, y: f64) -> bool {
        if x == y {
            return true;
        }
        let scale = x.abs().max(y.abs()).max(1.0);
        (x - y).abs() <= 1e-12 * scale
    }
    match (a, b) {
        (Value::Real(x), Value::Real(y)) => close(*x, *y),
        (
            Value::Scalar {
                si_value: x,
                dimension: dx,
            },
            Value::Scalar {
                si_value: y,
                dimension: dy,
            },
        ) => dx == dy && close(*x, *y),
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        // `Undef` is a VALUE here, not an error channel, so two of them agree. Without this
        // arm the `_ => false` catch-all below swallows the pairing and the caller reports an
        // INV-SF-7 violation — "omitting ';' changed the value" — for two values that are
        // identical. That would be a false positive on the first run of a path that, as of
        // #5392, no variant reaches yet (see the `refused` tally in
        // `adjacent_token_variation_cannot_silently_change_a_value`), which is precisely when
        // an unsound fallback is hardest to recognise as a test bug rather than a real
        // regression. This does NOT weaken the corpus: `separated_twins_are_all_clean`
        // independently requires every intended twin to evaluate to a non-`Undef` value, so a
        // twin that silently degraded to undef is still caught — by the test whose job that
        // is.
        (Value::Undef, Value::Undef) => true,
        // Any other pairing (including a discriminant mismatch such as Real vs Undef) is a
        // disagreement; the failure message prints both, so a new variant showing up here is
        // self-describing.
        _ => false,
    }
}

/// INV-SF-7, the whole point of task #5392: omitting a separator must either be REFUSED or be
/// value-preserving. It may never be quietly accepted with a different answer.
///
/// Both arms are conforming, so the assertions below are per-variant. The `refused` tally at
/// the end is a NON-VACUITY guard, not a third requirement: as of #5392 every variant takes
/// the refusal arm, which means the value-comparison arm never executes and the test would
/// silently stop testing anything if that stayed unnoticed. Pinning the split makes any
/// change visible. If it fires because a variant became clean-compiling, the correct response
/// is to confirm the value comparison above now covers that variant and update the count —
/// never to delete the guard.
#[test]
fn adjacent_token_variation_cannot_silently_change_a_value() {
    let mut refused = 0usize;

    for v in VARIANTS {
        let compiled = compile_as_production_does(v.no_sep);
        let diagnostics = compiled.diagnostics.clone();

        // A hard error is a CONFORMING outcome — "a parse error at the ambiguity site" is
        // precisely the acceptance criterion's second arm.
        if !error_diags(&diagnostics).is_empty() {
            refused += 1;
            continue;
        }

        // No error, so the compile claims this program is well-formed. Then its value is a
        // claim about the source, and it must match what the source plainly says.
        let result = eval_as_production_does(&compiled);
        let id = ValueCellId::new(v.structure, v.member);
        let got = result.values.get(&id).cloned().unwrap_or_else(|| {
            panic!(
                "{}: INV-SF-7 violated — omitting the separator produced NO error diagnostic, \
                 yet the value cell {}.{} does not exist at all. A program that is accepted \
                 must produce the values it declares.\n\
                 diagnostics: {:?}\n\
                 source:\n{}",
                v.name, v.structure, v.member, diagnostics, v.no_sep,
            )
        });

        let expected = cell_value(&eval_source(v.intended), v.structure, v.member);

        assert!(
            values_agree(&got, &expected),
            "{}: INV-SF-7 violated — omitting ';' changed the value with no error diagnostic.\n\
             got (no separator): {:?}\n\
             expected (separated twin): {:?}\n\
             diagnostics on the malformed source: {:?}\n\
             malformed source:\n{}\n\
             intended source:\n{}",
            v.name,
            got,
            expected,
            diagnostics,
            v.no_sep,
            v.intended,
        );
    }

    assert_eq!(
        refused,
        VARIANTS.len(),
        "corpus split changed: {refused} of {} variants were refused. As of #5392 every \
         variant is refused, so the value-comparison arm above never runs; if a variant now \
         compiles cleanly, check that the comparison covered it and update this count \
         deliberately. Do not delete this guard — without it the test can silently become \
         vacuous.",
        VARIANTS.len(),
    );
}

/// The mandatory clean-input guard: the fix must not degenerate into "reject everything".
///
/// Every intended twin must compile with zero error diagnostics AND evaluate to a defined,
/// non-`Undef` value. Without this, making `diagnose_error_node` fire on anything at all would
/// "pass" the invariant test above by refusing valid programs.
#[test]
fn separated_twins_are_all_clean() {
    for v in VARIANTS {
        let compiled = compile_source_with_stdlib_allow_parse_errors(v.intended);
        let errors = error_diags(&compiled.diagnostics);
        assert!(
            errors.is_empty(),
            "{}: the well-separated twin must compile cleanly — the fix may not start \
             rejecting valid programs.\n\
             errors: {:?}\n\
             source:\n{}",
            v.name,
            errors,
            v.intended,
        );

        let value = cell_value(&eval_source(v.intended), v.structure, v.member);
        assert!(
            !matches!(value, Value::Undef),
            "{}: the well-separated twin compiled cleanly but {}.{} evaluated to undef, so it \
             pins no value for the malformed source to be compared against.\n\
             source:\n{}",
            v.name,
            v.structure,
            v.member,
            v.intended,
        );
    }
}

/// A real design that calls a user-defined function must still produce its VALUE.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
/// the guard against the severity flip in `forward_parse_errors` turning a formerly tolerated
/// warning into a broken build.
///
/// Scope is deliberately one fixture, and deliberately at the EVAL layer.
/// `crates/reify-compiler/tests/examples_smoke.rs` already walks all of `examples/`
/// recursively — asserting `parsed.errors.is_empty()` and zero `Severity::Error` diagnostics
/// from `compile_with_stdlib`, behind an auditable `SKIP_SET` — so it is the corpus-wide guard
/// against the severity flip and a second hand-picked list here would be strictly weaker.
/// What it does NOT do is run the engine. `m5_user_function.ri` declares `fn area` and calls
/// it from `Panel.surface_area`, so evaluating that cell is the shortest path from "the parser
/// still accepts a user fn" to "the value it computes is still there".
#[test]
fn a_design_calling_a_user_fn_still_evaluates_its_cell() {
    const PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/m5_user_function.ri"
    );

    let source =
        std::fs::read_to_string(PATH).unwrap_or_else(|e| panic!("{PATH} should exist: {e}"));

    // `compile_source_with_stdlib` panics on any parse error, which is itself half the guard:
    // this example must continue to parse cleanly.
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let errors = error_diags(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "{PATH}: a checked-in example stopped compiling. Promoting parse errors to ERROR \
         severity may not break real designs.\n\
         errors: {errors:?}",
    );

    // `Panel.surface_area = area(width, height)` with `width = 200`, `height = 100`. Both
    // params are declared `Real` but defaulted from integer literals, so the product carries
    // the `Int` discriminant — measured, not assumed.
    let value = cell_value(&eval_source(&source), "Panel", "surface_area");
    assert!(
        values_agree(&value, &Value::Int(20_000)),
        "{PATH}: `Panel.surface_area` is computed by the user fn `area`, so a fn-body \
         regression shows up here as a changed or undefined value rather than as a \
         diagnostic. got: {value:?}",
    );
}
