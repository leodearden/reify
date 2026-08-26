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
//! * **RED on main (#6394 — the enum-variant PAYLOAD axis)** —
//!   [`shadow_payload_field_lowers_to_enum_type`] and
//!   [`shadow_payload_binder_fixture_has_no_errors`]. The `LocalEnumShadowScope`
//!   is installed AFTER `resolve_enum_variant_payloads`, so an enum-variant
//!   payload field typed by the shadowed name lowers to
//!   `Type::StructureRef("Fit")` while every param/let/fn/trait position lowers
//!   to `Type::Enum("Fit")`; passing that payload's `match` binder to a
//!   same-typed `fn` param then fails overload resolution. The first test forces
//!   the fix into the LOWERING, the second pins the leaf `reify check … exits 0`
//!   signal. PRD `docs/prds/v0_6/enum-shadow-coherence.md` §2 R4 / §3 D1.
//! * **Characterization (green on main, must stay green)** —
//!   [`enum_ctor_fixture_constraint_is_satisfied`] and
//!   [`enum_ctor_fixture_binds_the_variant`]. PRD boundary row 9's
//!   user-observable signal: the fixture already evaluates correctly today
//!   (the diagnostic is a Warning, not an Error), and must keep doing so.
//! * **No-overreach guards (green on main AND after)** — the three inline-source
//!   tests in the `No-overreach guards` section. They fail against an over-broad
//!   implementation: a PRELUDE enum must not shadow a LOCAL structure, the stdlib
//!   `Fit` structure must stay reachable from a module with no local `enum Fit`,
//!   and a same-module `structure def N` must still beat a same-module `enum N`.
//! * **Payload-axis no-overreach guards (green on main AND after, #6394)** —
//!   [`prelude_enum_does_not_shadow_local_structure_in_payload`],
//!   [`payload_field_of_prelude_structure_name_unaffected_without_local_enum`] and
//!   [`local_structure_wins_over_same_named_local_enum_in_payload`]: the group
//!   above, mirrored 1:1 on the phase the #6394 hoist newly covers. They fail
//!   against an implementation that hoisted the scope but ALSO widened what enters
//!   the shadow set. Which over-broadening each one actually catches — they do not
//!   partition the two membership rules one-for-one — is recorded on that group's
//!   section comment.
//!
//! A failure in the last three groups is a BEHAVIOUR CHANGE, not an unimplemented
//! feature.
//!
//! **Every helper here goes through the `*_with_stdlib` test-support variants on
//! purpose.** The plain `compile_source` / `eval_source` / `check_source` compile
//! with an EMPTY prelude, where `Fit` is not a structure name at all and the bug
//! does not reproduce — a test written on those is green before AND after the
//! fix, pinning nothing.

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::{Value, VariantPayload};
use reify_test_support::{
    cell_value, check_source_with_stdlib, compile_source_with_stdlib, errors_only, make_engine,
    parse_and_compile_with_stdlib,
};

/// The committed PRD fixture: `enum Fit` + `structure def Hole { param fit: Fit }`
/// + `Hole(fit: Fit.Close, nominal: 4mm)`. Landed by dep #5433.
const FIXTURE: &str = include_str!("../../../../docs/prds/v0_6/fixtures/member_enum_ctor.ri");

/// The committed PRD fixture for the payload axis: `enum Fit` (shadowing the
/// prelude's `structure def Fit`) + `enum Boxed { B { f: Fit } }` +
/// `fn use_fit(f: Fit)`, with a `match` arm handing the payload binder to the
/// fn. Landed with PRD `docs/prds/v0_6/enum-shadow-coherence.md`.
const PAYLOAD_FIXTURE: &str =
    include_str!("../../../../docs/prds/v0_6/fixtures/shadow_payload_binder.ri");

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

/// The RESOLVED type of `<enum_name>.<variant>`'s named payload field `<field>`,
/// or a panic naming what WAS available at whichever level missed.
///
/// Mirrors [`param_cell_type`]'s idiom one level deeper (`EnumDef` →
/// `EnumVariantDef` → `VariantPayload::Named`) and for the same reason: a
/// silently renamed enum, variant or field must surface as a named panic rather
/// than making the caller's `assert_eq!` vacuous. A `VariantPayload::Unit` where
/// `Named` was expected panics too — a variant that lost its payload shape would
/// otherwise be indistinguishable from a missing field.
fn variant_payload_field_type(
    module: &CompiledModule,
    enum_name: &str,
    variant: &str,
    field: &str,
) -> Type {
    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == enum_name)
        .unwrap_or_else(|| {
            panic!(
                "enum {enum_name:?} not found; available: {:?}",
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        });
    let variant_def = enum_def
        .variants
        .iter()
        .find(|v| v.name == variant)
        .unwrap_or_else(|| {
            panic!(
                "variant {variant:?} not found on enum {enum_name:?}; available: {:?}",
                enum_def
                    .variants
                    .iter()
                    .map(|v| &v.name)
                    .collect::<Vec<_>>()
            )
        });
    let fields = match &variant_def.payload {
        VariantPayload::Named(fields) => fields,
        VariantPayload::Unit => panic!(
            "{enum_name}.{variant} carries VariantPayload::Unit, but a NAMED \
             payload field {field:?} was expected"
        ),
    };
    fields
        .iter()
        .find(|(name, _)| name == field)
        .unwrap_or_else(|| {
            panic!(
                "payload field {field:?} not found on {enum_name}.{variant}; available: {:?}",
                fields.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        })
        .1
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

// ── Enum-variant payload coherence (R4, #6394) ───────────────────────
//
// PRD `docs/prds/v0_6/enum-shadow-coherence.md` §2 R4 / §3 D1. Both are RED on
// main: `resolve_enum_variant_payloads` runs BEFORE the `LocalEnumShadowScope`
// install, so the payload position is the one declared-type position in the
// module that does not see the shadow set.

/// R4's root claim, pinned in the IR. `Boxed.B`'s payload field `f` must lower
/// to `Type::Enum("Fit")`.
///
/// Forces the fix into the LOWERING rather than into the overload/conformance
/// machinery, which could silence
/// [`shadow_payload_binder_fixture_has_no_errors`] while leaving the wrong type
/// in the IR for `match`, member typing and trait matching to trip over later.
#[test]
fn shadow_payload_field_lowers_to_enum_type() {
    let module = compile_source_with_stdlib(PAYLOAD_FIXTURE);
    let f = variant_payload_field_type(&module, "Boxed", "B", "f");
    assert_eq!(
        f,
        Type::Enum("Fit".to_string()),
        "`Boxed.B`'s payload field must lower through the SAME shadow set as \
         every param/let/fn/trait position; on main it is \
         `Type::StructureRef(\"Fit\")` because `resolve_enum_variant_payloads` \
         runs before the `LocalEnumShadowScope` install; got {f:?}"
    );
}

/// The LEAF user-observable signal: `reify check
/// docs/prds/v0_6/fixtures/shadow_payload_binder.ri` exits 0.
///
/// Asserted in-process rather than by shelling out to the CLI: INV-SF-2
/// (`error-severity-exits-nonzero`) makes "zero Error-severity diagnostics" and
/// "exit 0" the same proposition, and the existing cross-phase oracles below
/// assert the same way.
#[test]
fn shadow_payload_binder_fixture_has_no_errors() {
    let module = compile_source_with_stdlib(PAYLOAD_FIXTURE);

    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "the payload-binder fixture must compile without errors \
         (`reify check …` exits 0); got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Severity-INDEPENDENT, on purpose: a future change that merely downgraded
    // the overload failure to a Warning would green the check above without
    // fixing anything.
    let overload: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("no matching overload"))
        .map(|d| &d.message)
        .collect();
    assert!(
        overload.is_empty(),
        "no diagnostic of ANY severity may report an overload failure for the \
         payload binder's `use_fit(g)` call; got: {overload:?}"
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

// ── Payload-axis no-overreach guards (green on main AND after) ───────────────
//
// NOT red pins — all three are green BEFORE and AFTER #6394's scope hoist. They
// mirror the `No-overreach guards` group above 1:1 on the newly-covered phase
// (enum-variant payload resolution) and do not drive the fix; they fail against an
// implementation that hoisted the `LocalEnumShadowScope` but ALSO widened what
// enters the shadow set.
//
// What each one actually discriminates — stated precisely, because the three do
// NOT partition `build_local_enum_shadow_set`'s two membership rules one-for-one:
//
// * [`payload_field_of_prelude_structure_name_unaffected_without_local_enum`] —
//   the hoist must not blanket-enum every name: with NO local enum declaring it,
//   a payload field typed by a prelude structure name stays a `StructureRef`.
//   It does NOT discriminate the `ctx.enum_defs`-not-`ctx.resolution_enums` rule,
//   because the prelude declares no `enum Fit` (only `enum FitCategory`,
//   `stdlib/tolerancing.ri:22`) — swapping the membership source leaves "Fit" out
//   of the set either way and this test stays green.
// * [`prelude_enum_does_not_shadow_local_structure_in_payload`] — the payload-axis
//   mirror of [`prelude_enum_does_not_shadow_local_structure`], and the only shape
//   in this group naming something the PRELUDE declares as an `enum`
//   (`enum ThreadSystem`, `stdlib/ports_mechanical.ri:35`). It flips to
//   `Type::Enum` for an implementation that BOTH sources the set from
//   `ctx.resolution_enums` AND drops the local-structure subtraction — a
//   combination neither other guard catches, since either change alone still
//   leaves "ThreadSystem" out of the set.
// * [`local_structure_wins_over_same_named_local_enum_in_payload`] — the
//   local-structure subtraction on its own: "Fit" IS a module-local enum there, so
//   dropping the subtraction alone flips it to `Type::Enum`.
//
// Same `module test.<name>` prologue convention as the group above, so
// `W_MODULE_DECL_MISSING` never pollutes these modules.

/// A LOCAL `structure def ThreadSystem` must win over the PRELUDE
/// `enum ThreadSystem` (`stdlib/ports_mechanical.ri:35`) in a variant-payload
/// position, exactly as it does in a param position.
///
/// The payload-axis mirror of [`prelude_enum_does_not_shadow_local_structure`],
/// and the guard in this group that carries the `ctx.enum_defs`-not-
/// `ctx.resolution_enums` membership rule: `ThreadSystem` is a PRELUDE enum name,
/// so an implementation that both sources the set from `ctx.resolution_enums`
/// (prelude ++ local) and drops the local-structure subtraction pulls it into the
/// shadow set and retypes the user's own structure to `Type::Enum("ThreadSystem")`
/// on the phase #6394's hoist newly covers. The two sibling guards cannot see that
/// combination — see the section comment above.
#[test]
fn prelude_enum_does_not_shadow_local_structure_in_payload() {
    const SOURCE: &str = r#"
module test.payload_prelude_enum_vs_local_structure

structure def ThreadSystem {
    param x: Length = 1mm
}

enum Boxed { B { t: ThreadSystem } }
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let t = variant_payload_field_type(&module, "Boxed", "B", "t");
    assert_eq!(
        t,
        Type::StructureRef("ThreadSystem".to_string()),
        "a local `structure def ThreadSystem` must win over the PRELUDE \
         `enum ThreadSystem` in a variant payload position; got {t:?}"
    );
}

/// With no local `enum Fit` in scope, a payload field `f: Fit` must still reach
/// the stdlib `std.tolerancing.Fit` STRUCTURE (`tolerancing.ri:268`).
///
/// The payload-axis mirror of
/// [`stdlib_fit_structure_param_unaffected_without_local_enum`]: the hoist must
/// not make every `Fit` an enum. Note this shape does NOT discriminate the
/// membership-SOURCE rule (there is no prelude `enum Fit`) — that is
/// [`prelude_enum_does_not_shadow_local_structure_in_payload`]'s job.
#[test]
fn payload_field_of_prelude_structure_name_unaffected_without_local_enum() {
    const SOURCE: &str = r#"
module test.payload_stdlib_fit_structure

enum Boxed { B { f: Fit } }
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let f = variant_payload_field_type(&module, "Boxed", "B", "f");
    assert_eq!(
        f,
        Type::StructureRef("Fit".to_string()),
        "with no local `enum Fit`, the payload field `f: Fit` must keep resolving \
         to the stdlib structure def; got {f:?}"
    );
}

/// The degenerate same-module collision, on the payload axis: a module declaring
/// BOTH `enum Fit` and `structure def Fit` keeps `StructureRef` for a payload
/// field typed `Fit`.
///
/// The payload-axis mirror of [`local_structure_wins_over_same_named_local_enum`]:
/// `build_local_enum_shadow_set` subtracts the local structure/occurrence names,
/// and that subtraction must hold on the newly-covered phase too.
#[test]
fn local_structure_wins_over_same_named_local_enum_in_payload() {
    const SOURCE: &str = r#"
module test.payload_local_structure_vs_local_enum

enum Fit { A }

structure def Fit {
    param x: Length = 1mm
}

enum Boxed { B { f: Fit } }
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let f = variant_payload_field_type(&module, "Boxed", "B", "f");
    assert_eq!(
        f,
        Type::StructureRef("Fit".to_string()),
        "a same-module `structure def Fit` must still beat a same-module \
         `enum Fit` in a variant payload position; got {f:?}"
    );
}

// ── Cross-phase agreement (esc-5429-1) ───────────────────────────────────────
//
// The shadow set must be visible to EVERY phase that lowers a declared type
// name in the module, not just `phase_entities`. `phase_traits` and
// `phase_functions` both run BEFORE `phase_entities` in
// `compile_with_prelude_context_checked_with_config`, so a scope installed only
// inside `phase_entities` left trait requirement types and fn signature params
// at `Type::StructureRef("Fit")` while the conforming structure's own param
// lowered to `Type::Enum("Fit")`. The two then disagreed and a module that
// compiled with a WARNING before #5429 failed with a hard ERROR after it —
// a warning-to-error severity regression on previously-valid user code.
//
// Both tests assert on ERROR-severity diagnostics only: the residual
// `TypeNotConformingToStructureRef` WARNING that the pre-#5429 compiler emitted
// is exactly what #5429 removes, but these tests are about the ERROR.

/// A trait requirement typed by a prelude-shadowing local enum must agree with
/// the conforming structure's param type. Regression oracle for
/// `TypeMismatchForTraitMember: expected Fit, got Enum(Fit)`.
#[test]
fn trait_member_typed_by_shadowing_local_enum_conforms() {
    const SOURCE: &str = r#"
module test.trait_member_shadowing_enum

enum Fit { Close, Medium }

trait HasFit {
    param f: Fit
}

structure def C : HasFit {
    param f: Fit = Fit.Close
}
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "a trait requirement `param f: Fit` must lower through the SAME shadow \
         set as the conforming structure's `param f: Fit`; got errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Pin the DIRECTION of agreement: both sides must be the ENUM, not both
    // left at StructureRef (which would also produce zero errors while silently
    // reverting #5429's root claim).
    let f = param_cell_type(&module, "C", "f");
    assert_eq!(
        f,
        Type::Enum("Fit".to_string()),
        "C.f must still lower to the shadowing local enum; got {f:?}"
    );
}

/// A module-level `fn` whose parameter names the shadowing enum must agree with
/// the calling structure's cell type. Regression oracle for `no matching
/// overload for pick(Enum(Fit)), candidates: pick(Fit) -> Scalar[m]`.
#[test]
fn fn_param_typed_by_shadowing_local_enum_resolves_call() {
    const SOURCE: &str = r#"
module test.fn_param_shadowing_enum

enum Fit { Close, Medium }

fn pick(f: Fit) -> Length {
    match f {
        Close => 1mm,
        Medium => 2mm
    }
}

structure def Consumer {
    param f: Fit = Fit.Close
    let d = pick(f)
}
"#;
    let module = compile_source_with_stdlib(SOURCE);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "`fn pick(f: Fit)` must lower its param through the SAME shadow set as \
         the caller's `param f: Fit`, so overload resolution finds the \
         candidate; got errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let f = param_cell_type(&module, "Consumer", "f");
    assert_eq!(
        f,
        Type::Enum("Fit".to_string()),
        "Consumer.f must still lower to the shadowing local enum; got {f:?}"
    );
}
