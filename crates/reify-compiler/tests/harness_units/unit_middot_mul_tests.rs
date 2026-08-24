//! End-to-end acceptance of U+00B7 MIDDLE DOT as a unit-multiply operator.
//!
//! Task #5784 (angle-units leaf κ; PRD
//! `docs/prds/v0_6/angle-units-surface-convergence.md` cluster C, ratified
//! decision 7a).  `Display for DimensionVector` joins base-unit parts with `·`, so
//! `reify eval` prints lines like `7850 kg·m^-3` and `5 m^2·kg·s^-2·rad^-1`.  κ
//! makes the UNIT SUBSTRING of such a line readable — `7850kg·m^-3` now parses and
//! evaluates identically to `7850kg*m^-3`.
//!
//! κ does NOT make the whole eval line round-trippable, and the tests here do not
//! claim it does: the SPACE between magnitude and unit is a separate, still-open
//! blocker.  Measured on this branch, `let a = 7850 kg·m^-3` and its `*` twin
//! `let a = 7850 kg*m^-3` BOTH exit 1 with `Parse error: invalid let: …`, so that
//! gap is pre-existing rather than a `·` regression.  Normalising the emitted
//! labels is leaf λ; the round-trip property test is leaf μ (#5789), which must
//! not assume the space is already handled.
//!
//! WHAT THIS FILE LOCKS THAT THE GRAMMAR AND LOWERING TESTS CANNOT
//! `tree-sitter-reify/tests/unit_middot_mul_grammar_tests.rs` proves the CST is
//! clean; `crates/reify-syntax/tests/harness_syntax/unit_expr_lowering_tests.rs`
//! proves the lowered `UnitExpr` matches its `*` twin.  Between the two sits the
//! failure mode this file exists for: a CLEAN CST whose members are silently
//! DROPPED during lowering, producing a template with zero errors and no value
//! cells.  Assertion (ii) below — every cell PRESENT with a `default_expr` — is
//! that lock, at the level a user actually observes.  See INV-SF-7
//! `parse-is-value-faithful` in `docs/legibility/design-invariants.md`.
//!
//! The fixture is read from disk (single literal leaf path — never the fixtures
//! DIRECTORY, which `tests/infra/test_verify_scope.sh`'s PG-DRIFT-DIR scenario
//! forbids any tracked `.rs` from naming) so it cannot drift from the committed
//! artifact.  Read-only input: this test must never edit the fixture.  Naming the
//! fixture here is also what obliges `scripts/verify.sh` to list
//! `unit_middot_mul.ri` in `_RUST_COUPLED_RI_FIXTURES` — PG-DRIFT derives that set
//! by grepping tracked `.rs` files, so the two must always move together.

use crate::common::{assert_eq_rel, expect_scalar};
use reify_core::{DimensionVector, Severity};
use reify_test_support::compile_source_with_stdlib_allow_parse_errors;

/// Compile `structure def S { let x = <quantity> }` and return the `x` cell's
/// (si_value, dimension).
///
/// A local `let`-flavoured helper rather than `common::stdlib_param_si_value`,
/// which builds a `param x : <type>` and so demands a named type for every probe.
/// Two of the three fixture bindings have no obvious one: `N·m/rad` carries a
/// `rad^-1` component (`rad` is a real dimension — `stdlib/units.ri`
/// `pub unit rad : Angle`), and `m^2·kg·s^-2` is Energy-shaped but the fixture
/// binds it untyped.  Guessing a type would either fail to compile or silently
/// measure a different quantity.  Untyped `let` cells match the fixture's own form.
///
/// Uses the same `_allow_parse_errors` helper as [`compile_fixture`], for the same
/// reason: the `errs.is_empty()` assertion below is meant to be the ONE place a
/// bad probe is reported, and the plain helper would instead panic inside the
/// parse step with a message that names no `quantity`.
fn let_cell_si_value(quantity: &str) -> (f64, DimensionVector) {
    let source = format!("structure def S {{ let x = {quantity} }}");
    let module = compile_source_with_stdlib_allow_parse_errors(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "source `{source}` produced errors: {errs:?}"
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .unwrap_or_else(|| {
            panic!(
                "`{quantity}`: no `x` value cell — the binding was DROPPED during \
                 lowering despite compiling without errors (INV-SF-7)"
            )
        });
    let expr = cell
        .default_expr
        .as_ref()
        .expect("x cell has no default_expr");
    expect_scalar(expr)
}

/// The committed prd-gate fixture, compiled with the stdlib.
///
/// Pins the U+00B7 SPELLING before compiling — the one property every assertion
/// below is blind to.  `·` and `*` lower to the same `UnitExpr::Mul`, so each
/// (si_value, dimension) comparison against a `*`-spelled twin is invariant under
/// replacing `·` with `*` in the fixture.  Measured on this branch:
/// `sed -i 's/·/*/g' tests/prd-gate/fixtures/unit_middot_mul.ri` left all four
/// tests in this file GREEN.  Exercising U+00B7 is the fixture's entire reason to
/// exist, so the spelling must be asserted directly rather than inferred.
///
/// Counted over NON-COMMENT lines only: the fixture's header quotes `·` several
/// times in prose, and a whole-file count would go RED on an unrelated comment
/// edit while still saying nothing about the `let` lines.
///
/// Returns the compiled module.  Asserts nothing about the COMPILE on its own, so
/// each numbered assertion below reports its own failure — which is why this
/// routes through `compile_source_with_stdlib_allow_parse_errors` rather than
/// plain `compile_source_with_stdlib`.  The plain helper parses via
/// `parse_with_stdlib_or_panic` (`crates/reify-test-support/src/helpers.rs`),
/// which `assert!(parsed.errors.is_empty())` — so a `·` PARSE regression would
/// panic INSIDE this function with `parse errors: [...]` and test (i)'s
/// `Severity::Error` filter would never run.  The `_allow_parse_errors` variant
/// folds parse errors into `module.diagnostics` as `Severity::Error` instead, so
/// (i) genuinely covers the `Parse error: syntax error: ·m` shape it claims to.
fn compile_fixture() -> reify_compiler::CompiledModule {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/unit_middot_mul.ri");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));
    let code_only = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        code_only.matches('·').count(),
        4,
        "the fixture's `let` lines must still spell their unit-multiplies with \
         U+00B7 MIDDLE DOT (torque_like x1, with_div x1, composed x2); found {} \
         in:\n{code_only}",
        code_only.matches('·').count()
    );
    compile_source_with_stdlib_allow_parse_errors(&src)
}

/// (i) The fixture compiles with zero `Severity::Error` diagnostics — parse-layer
/// AND compile-layer, since [`compile_fixture`] folds the former in.
///
/// Before κ this failed loudly with three `Parse error: syntax error: ·m`-class
/// diagnostics, and this assertion is the one that observes them: they arrive as
/// `Severity::Error` entries in `module.diagnostics`, not as a panic somewhere
/// upstream.
#[test]
fn prd_gate_fixture_unit_middot_mul_compiles_clean() {
    let module = compile_fixture();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "tests/prd-gate/fixtures/unit_middot_mul.ri should compile with no errors, \
         got: {errors:?}"
    );
}

/// (ii) THE SILENT-DROP LOCK: all three bindings survive lowering AND evaluate
/// to their `*`-spelled twins' values.
///
/// A clean compile is NOT sufficient evidence that the fixture works — if
/// `lower_unit_expr` fails to recognise the `·` operator it returns `None`, which
/// propagates through `lower_quantity_literal` and `lower_let` as a dropped
/// member, and `check_and_lower!` stays silent because it keys off a CST error
/// that no longer exists.  The observable result is a template with zero errors
/// and zero value cells: assertion (i) would still pass.  The presence half is
/// the assertion that distinguishes "correct" from "silently wrong".
///
/// The value half is what makes the fixture's own three `let` lines LOAD-BEARING
/// rather than merely present.  Each `*`-spelled string in the table below is the
/// pinned expectation; the `·` side is read from disk.  Editing a `let` line in
/// `tests/prd-gate/fixtures/unit_middot_mul.ri` therefore goes RED here — which
/// is the enforcement that fixture's header ("Editing the three `let` lines below
/// therefore changes test inputs — don't") previously asserted with nothing
/// behind it.  Tests (iii)/(iv) below compare `·` against `*` for INLINE strings;
/// only this test couples the two to the committed artifact.
#[test]
fn prd_gate_fixture_all_three_bindings_are_present() {
    let module = compile_fixture();
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UnitMiddotMul")
        .expect("UnitMiddotMul template not found in the compiled fixture");
    // The loop below iterates the PINNED table, not the fixture's cells, so a
    // fourth `let` added to the fixture would be silently ignored.  Pin the count.
    assert_eq!(
        template.value_cells.len(),
        3,
        "UnitMiddotMul must have exactly the three value cells this test pins; \
         got {:?} — a binding was added to or removed from the fixture without \
         updating the table below",
        template
            .value_cells
            .iter()
            .map(|c| c.id.member.as_str())
            .collect::<Vec<_>>()
    );
    // (fixture member name, the `*`-spelled twin of the RHS committed on that line)
    for (member, star_twin) in [
        ("torque_like", "5N*m"),
        ("with_div", "5N*m/rad"),
        ("composed", "5m^2*kg*s^-2"),
    ] {
        let cell = template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| {
                panic!(
                    "value cell `{member}` is MISSING from UnitMiddotMul — the `·` \
                     binding was dropped during lowering without a diagnostic \
                     (INV-SF-7 parse-is-value-faithful).  Cells present: {:?}",
                    template
                        .value_cells
                        .iter()
                        .map(|c| c.id.member.as_str())
                        .collect::<Vec<_>>()
                )
            });
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("value cell `{member}` exists but has no default_expr"));
        let (fixture_v, fixture_d) = expect_scalar(expr);
        let (star_v, star_d) = let_cell_si_value(star_twin);
        assert_eq_rel(
            fixture_v,
            star_v,
            1e-12,
            &format!(
                "fixture binding `{member}` must have the same si_value as its \
                 `*`-spelled twin `{star_twin}` — if this moved, either the \
                 fixture's `let` line was edited or `·` stopped meaning `*`"
            ),
        );
        assert_eq!(
            fixture_d, star_d,
            "fixture binding `{member}` must have the same dimension as its \
             `*`-spelled twin `{star_twin}`"
        );
    }
}

/// Compile each `(·-spelled, *-spelled)` pair as a `let` cell and assert the two
/// evaluate identically.
///
/// Comparing against the twin rather than a hard-coded number keeps the assertion
/// honest if the stdlib's unit factors ever move — the two spellings must agree
/// whatever they denote.  Shared by (iii) and (iv), which differ only in their
/// input table; they stay separately named so a failure still reports which class
/// of literal broke.
fn assert_twins(pairs: &[(&str, &str)]) {
    for (dot, star) in pairs {
        let (dot_v, dot_d) = let_cell_si_value(dot);
        let (star_v, star_d) = let_cell_si_value(star);
        assert_eq_rel(
            dot_v,
            star_v,
            1e-12,
            &format!("`{dot}` and `{star}` must have the same si_value"),
        );
        assert_eq!(
            dot_d, star_d,
            "`{dot}` and `{star}` must have the same dimension"
        );
    }
}

/// (iii) Each `·` binding evaluates identically to its `*` twin.
///
/// This is the literal wording of the task's user-observable signal: "`5N·m`
/// evaluates identically to `5N*m`".
#[test]
fn each_middot_binding_matches_its_star_twin() {
    assert_twins(&[
        ("5N·m", "5N*m"),
        ("5N·m/rad", "5N*m/rad"),
        ("5m^2·kg·s^-2", "5m^2*kg*s^-2"),
    ]);
}

/// (iv) The UNIT SUBSTRINGS of the lines `Display for DimensionVector` emits are
/// readable.
///
/// These are not in the fixture, but they are the shapes that motivated κ: the
/// density, acceleration and torque unit renderings a user copies out of `reify
/// eval` output.
///
/// The inputs are whitespace-STRIPPED, and deliberately so — they are NOT verbatim
/// eval output.  `reify eval` prints `7850 kg·m^-3` WITH a space, and that verbatim
/// line still does not parse; its `*` twin `7850 kg*m^-3` fails identically, so the
/// magnitude/unit space is a separate open blocker rather than a `·` regression.
/// These are the right inputs for what κ closed.
#[test]
fn display_shaped_middot_literals_match_their_star_twins() {
    assert_twins(&[
        ("7850kg·m^-3", "7850kg*m^-3"),
        ("9.81m·s^-2", "9.81m*s^-2"),
        ("5m^2·kg·s^-2·rad^-1", "5m^2*kg*s^-2*rad^-1"),
    ]);
}

/// (v) `·` adjacent to a parenthesised operand evaluates like its `*` twin.
///
/// `is_unit_start` accepts `(`, so this is a live accepted path, and it is the one
/// that drives `lower_unit_expr`'s paren-unwrap arm beneath a `·`-spelled Mul.
#[test]
fn middot_beside_a_paren_group_matches_its_star_twin() {
    assert_twins(&[("5W·(m/K)", "5W*(m/K)"), ("5W/(m·K)", "5W/(m*K)")]);
}
