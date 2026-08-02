//! End-to-end binding of an enum-typed ctor param on a user-module structure
//! (task #5429, PRD `docs/prds/v0_6/uniform-member-access.md` §4 M5 / D8).
//!
//! The defect: `std.tolerancing` is in the default prelude and declares
//! `structure def Fit` (`stdlib/tolerancing.ri:268`), so its name lands in the
//! resolver's `structure_names` set. A user module that declares its own
//! `enum Fit` and a `param fit : Fit` therefore had that param lowered to
//! `Type::StructureRef("Fit")` — the structure-name arm outranks both enum
//! fallbacks — and the ctor-conformance walker then rejected the `Enum(Fit)`
//! argument against a "structure type Fit" param.
//!
//! Which tests are which:
//!
//! * **RED on main** — [`enum_ctor_param_lowers_to_enum_type`] and
//!   [`enum_ctor_emits_no_structure_ref_mismatch`]. These are D8's root claim.
//!   The first forces the fix into the LOWERING (the param's `cell_type`) rather
//!   than into the conformance walker, which could have silenced the diagnostic
//!   while leaving the wrong type in the IR for `match`, member typing and trait
//!   matching to trip over later.
//! * **Characterization (green on main, must stay green)** —
//!   [`enum_ctor_fixture_constraint_is_satisfied`] and
//!   [`enum_ctor_fixture_binds_the_variant`]. PRD boundary row 9's
//!   user-observable signal: the fixture already evaluates correctly today
//!   (the diagnostic is a Warning, not an Error), and must keep doing so.
//! * **No-overreach guards (green on main AND after)** — the three inline-source
//!   tests at the bottom. They fail against an over-broad implementation: a
//!   PRELUDE enum must not shadow a LOCAL structure, the stdlib `Fit` structure
//!   must stay reachable from a module with no local `enum Fit`, and a
//!   same-module `structure def N` must still beat a same-module `enum N`.
//!
//! A failure in the last two groups is a BEHAVIOUR CHANGE, not an unimplemented
//! feature.
//!
//! **Every helper here goes through the `*_with_stdlib` test-support variants on
//! purpose.** The plain `compile_source` / `eval_source` / `check_source` compile
//! with an EMPTY prelude, where `Fit` is not a structure name at all and the bug
//! does not reproduce — a test written on those is green before AND after the
//! fix, pinning nothing.

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::Value;
use reify_test_support::{
    cell_value, check_source_with_stdlib, compile_source_with_stdlib, make_engine,
    parse_and_compile_with_stdlib,
};

/// The committed PRD fixture: `enum Fit` + `structure def Hole { param fit: Fit }`
/// + `Hole(fit: Fit.Close, nominal: 4mm)`. Landed by dep #5433.
const FIXTURE: &str = include_str!("../../../../docs/prds/v0_6/fixtures/member_enum_ctor.ri");

/// The lowered `cell_type` of `<template>.<member>`, or a panic naming what was
/// actually available (a silently-renamed template/member would otherwise make
/// an assertion vacuous).
fn param_cell_type(module: &CompiledModule, template: &str, member: &str) -> Type {
    let tpl = module
        .templates
        .iter()
        .find(|t| t.name == template)
        .unwrap_or_else(|| {
            panic!(
                "template {template:?} not found; available: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    tpl.value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| {
            panic!(
                "{template}.{member} not found; available: {:?}",
                tpl.value_cells
                    .iter()
                    .map(|vc| &vc.id.member)
                    .collect::<Vec<_>>()
            )
        })
        .cell_type
        .clone()
}

// ── RED on main ──────────────────────────────────────────────────────────────

/// D8's root claim. `Hole.fit` must lower to `Type::Enum("Fit")`.
///
/// On main it is `Type::StructureRef("Fit")` — the prelude's
/// `std.tolerancing.Fit` structure def wins the bare-name race against the
/// module's own `enum Fit`.
#[test]
fn enum_ctor_param_lowers_to_enum_type() {
    let module = compile_source_with_stdlib(FIXTURE);
    let fit = param_cell_type(&module, "Hole", "fit");
    assert_eq!(
        fit,
        Type::Enum("Fit".to_string()),
        "Hole.fit must lower to the module-local `enum Fit`, not the prelude \
         `structure def Fit` from std.tolerancing; got {fit:?}"
    );
}

/// The user-visible symptom: `Hole(fit: Fit.Close, …)` must bind cleanly.
///
/// On main the module carries exactly one `TypeNotConformingToStructureRef`
/// warning — "argument 'fit' has type 'Enum(Fit)' but param 'fit' requires
/// structure type 'Fit'". It is only a Warning because
/// `CTOR_FIELD_CONFORMANCE_SEVERITY` is still `Severity::Warning`; PRD task δ
/// flips that const to Error, at which point this fixture would hard-fail.
///
/// Asserts on the FILTERED code, never a total diagnostic count: the fixture
/// legitimately emits `W_MODULE_DECL_MISSING` (it has no `module` decl).
#[test]
fn enum_ctor_emits_no_structure_ref_mismatch() {
    let module = compile_source_with_stdlib(FIXTURE);

    let mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToStructureRef))
        .collect();
    assert!(
        mismatches.is_empty(),
        "an enum-typed ctor argument must not be reported as a structure-type \
         mismatch; got: {mismatches:?}"
    );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the fixture must compile without errors; got: {errors:?}"
    );
}

// ── Characterization: boundary row 9's user-observable signal ────────────────

/// `constraint hd > 4mm` with `hd = 4mm + 0.2mm` is satisfied — exactly one
/// constraint entry, and it is `Satisfied`.
#[test]
fn enum_ctor_fixture_constraint_is_satisfied() {
    let result = check_source_with_stdlib(FIXTURE);
    assert_eq!(
        result.constraint_results.len(),
        1,
        "the fixture declares exactly one constraint; got: {:?}",
        result.constraint_results
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        reify_ir::Satisfaction::Satisfied,
        "hd (4mm + 0.2mm) > 4mm must be Satisfied; got {:?}",
        result.constraint_results[0]
    );
}

/// The `match fit { Close => … }` arm actually selected: `Test.hd` is 4.2mm and
/// `Test.h`'s `fit` field holds the `Fit::Close` variant.
///
/// Pins BOTH halves on purpose. The scalar alone would still pass if `fit` were
/// bound as some other representation that happened to select the first arm.
#[test]
fn enum_ctor_fixture_binds_the_variant() {
    let compiled = parse_and_compile_with_stdlib(FIXTURE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);

    match cell_value(&result, "Test", "hd") {
        Value::Scalar { si_value, .. } => assert!(
            (si_value - 0.0042).abs() < 1e-12,
            "Test.hd must stay 0.0042 m (4mm + the Close arm's 0.2mm); got {si_value}"
        ),
        other => panic!("Test.hd: expected Value::Scalar, got {other:?}"),
    }

    let h = cell_value(&result, "Test", "h");
    let fit = match &h {
        Value::StructureInstance(data) => data.fields.get("fit").cloned().unwrap_or_else(|| {
            panic!("Test.h has no `fit` field; got {h:?}");
        }),
        other => panic!("Test.h: expected Value::StructureInstance, got {other:?}"),
    };
    assert_eq!(
        fit,
        Value::enum_unit("Fit", "Close"),
        "Test.h.fit must be the Fit::Close enum variant; got {fit:?}"
    );
}

// ── No-overreach guards (green on main AND after the fix) ────────────────────
//
// Each source starts with a `module test.<name>` decl so `W_MODULE_DECL_MISSING`
// never pollutes these modules (mirrors struct_ctor_field_conformance_tests.rs).

/// A PRELUDE enum must NOT shadow a LOCAL structure. `std.ports_mechanical`
/// declares `enum ThreadSystem` (`ports_mechanical.ri:35`); a user who declares
/// their own `structure def ThreadSystem` must keep `param t : ThreadSystem`
/// lowering to `Type::StructureRef`.
///
/// This is why #5429 needs its own thread-local rather than hoisting the
/// existing `RESOLUTION_ENUM_NAMES` fallback above the structure arm: that set
/// is prelude ++ local, and hoisting it would silently retype this param to the
/// stdlib enum.
#[test]
fn prelude_enum_does_not_shadow_local_structure() {
    const SOURCE: &str = r#"
module test.prelude_enum_vs_local_structure

structure def ThreadSystem {
    param x: Length = 1mm
}

structure def Consumer {
    param t: ThreadSystem
}
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let t = param_cell_type(&module, "Consumer", "t");
    assert_eq!(
        t,
        Type::StructureRef("ThreadSystem".to_string()),
        "a local `structure def ThreadSystem` must win over the PRELUDE \
         `enum ThreadSystem`; got {t:?}"
    );
}

/// With no local `enum Fit` in scope, `param f : Fit` must still reach the
/// stdlib `std.tolerancing.Fit` STRUCTURE (`tolerancing.ri:268`). The fix must
/// not make every `Fit` an enum.
#[test]
fn stdlib_fit_structure_param_unaffected_without_local_enum() {
    const SOURCE: &str = r#"
module test.stdlib_fit_structure

structure def Consumer {
    param f: Fit
}
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let f = param_cell_type(&module, "Consumer", "f");
    assert_eq!(
        f,
        Type::StructureRef("Fit".to_string()),
        "with no local `enum Fit`, `param f : Fit` must keep resolving to the \
         stdlib structure def; got {f:?}"
    );
}

/// The degenerate same-module collision: a module declaring BOTH `enum Fit` and
/// `structure def Fit` keeps today's `StructureRef` answer. The shadow set
/// subtracts the local structure/occurrence names, so most-local-declaration-wins
/// is applied conservatively — no behaviour change where the current answer is
/// at least defensible.
#[test]
fn local_structure_wins_over_same_named_local_enum() {
    const SOURCE: &str = r#"
module test.local_structure_vs_local_enum

enum Fit { A }

structure def Fit {
    param x: Length = 1mm
}

structure def Consumer {
    param f: Fit
}
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let f = param_cell_type(&module, "Consumer", "f");
    assert_eq!(
        f,
        Type::StructureRef("Fit".to_string()),
        "a same-module `structure def Fit` must still beat a same-module \
         `enum Fit`; got {f:?}"
    );
}
