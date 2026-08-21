//! End-to-end acceptance of U+00B7 MIDDLE DOT as a unit-multiply operator.
//!
//! Task #5784 (angle-units leaf κ; PRD
//! `docs/prds/v0_6/angle-units-surface-convergence.md` cluster C, ratified
//! decision 7a).  `Display for DimensionVector` joins base-unit parts with `·`, so
//! `reify eval` prints `7850 kg·m^-3` and `5 m^2·kg·s^-2·rad^-1` — strings Reify
//! could not read back.  κ closes the read direction; normalising the emitted
//! labels is leaf λ and the round-trip property test is leaf μ (#5789).
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
use reify_test_support::compile_source_with_stdlib;

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
fn let_cell_si_value(quantity: &str) -> (f64, DimensionVector) {
    let source = format!("structure def S {{ let x = {quantity} }}");
    let module = compile_source_with_stdlib(&source);
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
/// Returns the compiled module.  Asserts nothing on its own so each numbered
/// assertion below reports its own failure.
fn compile_fixture() -> reify_compiler::CompiledModule {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/unit_middot_mul.ri");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));
    compile_source_with_stdlib(&src)
}

/// (i) The fixture compiles with zero `Severity::Error` diagnostics.
///
/// Before κ this failed loudly with three `Parse error: syntax error: ·m`-class
/// diagnostics.
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

/// (iv) The two shapes `Display for DimensionVector` actually emits are readable.
///
/// These are not in the fixture, but they are the strings that motivated κ: the
/// density and torque renderings a user copies out of `reify eval` output.
#[test]
fn display_shaped_middot_literals_match_their_star_twins() {
    assert_twins(&[
        ("7850kg·m^-3", "7850kg*m^-3"),
        ("9.81m·s^-2", "9.81m*s^-2"),
        ("5m^2·kg·s^-2·rad^-1", "5m^2*kg*s^-2*rad^-1"),
    ]);
}
