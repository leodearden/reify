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

/// Compile and evaluate `source` through the PRODUCTION single-module path, then hand back its
/// diagnostics verbatim.
///
/// Two test-support helpers are deliberately bypassed here, because each would hide the very
/// behaviour under test:
///
/// - `eval_source` routes through `parse_or_panic`, which asserts `parsed.errors.is_empty()`
///   and so panics before any value exists to compare.
/// - `compile_source_with_stdlib_allow_parse_errors` PREPENDS its own
///   `Diagnostic::error` per parse error (`helpers.rs`'s `parse_errors_as_diagnostics`). That
///   is a test-only severity upgrade: production's `forward_parse_errors`
///   (`compile_builder/pre_pass.rs`) emits the same parse errors as `Diagnostic::warning`.
///   Measuring severity through that helper would report a hard refusal that real callers of
///   `reify_compiler::compile_with_stdlib` never see — the exact leak this test exists to
///   close.
///
/// So this calls `parse_with_stdlib` + `compile_with_stdlib` directly, which is what the CLI
/// and GUI do, and evaluates with the same `MockConstraintChecker` engine the other e2e tests
/// use (`result_fallback_e2e.rs`'s manual pipeline).
fn compile_and_eval_as_production_does(
    source: &str,
) -> (Vec<reify_core::Diagnostic>, reify_eval::EvalResult) {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let diagnostics = compiled.diagnostics.clone();
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    (diagnostics, result)
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
        // Any other pairing (including a discriminant mismatch such as Real vs Undef) is a
        // disagreement; the failure message prints both, so a new variant showing up here is
        // self-describing.
        _ => false,
    }
}

/// INV-SF-7, the whole point of task #5392: omitting a separator must either be REFUSED or be
/// value-preserving. It may never be quietly accepted with a different answer.
#[test]
fn adjacent_token_variation_cannot_silently_change_a_value() {
    for v in VARIANTS {
        let (diagnostics, result) = compile_and_eval_as_production_does(v.no_sep);

        // A hard error is a CONFORMING outcome — "a parse error at the ambiguity site" is
        // precisely the acceptance criterion's second arm.
        if !error_diags(&diagnostics).is_empty() {
            continue;
        }

        // No error, so the compile claims this program is well-formed. Then its value is a
        // claim about the source, and it must match what the source plainly says.
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
