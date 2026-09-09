//! Task #5867 — a `self.<sub>.<member>` read whose sub is typed by a STDLIB
//! PRELUDE template must be typed from that prelude template, not fall through
//! to the cross-sub-geometry path and become `Geometry` / `Error`.
//!
//! Stdlib templates live in the prelude and are deliberately NOT merged into
//! `CompiledModule::templates` (io-export δ / esc-4287-15). The sub-target
//! lookup in `entity.rs`'s `Sub` pre-pass used a module-only `find_template`,
//! so for a prelude-typed sub neither `sub_member_types` nor
//! `sub_realization_names` was ever populated while `sub_component_types`
//! always was. That combination is exactly the "forward-declared" shape
//! `scope::sub_member_is_cross_sub_geometry_or_forward_declared` optimistically
//! admits, so the relaying `let` was lowered as a `RealizationDecl` (geometry)
//! instead of a value cell — silently, with zero diagnostics.
//!
//! This is the compile-side mirror of esc-5360-9, which fixed the same
//! module-only assumption on the eval side (`unfold::find_template_in_scope`).

use reify_compiler::{TopologyTemplate, ValueCellKind};
use reify_core::{DimensionVector, Type};

/// Fetch the named template from a compiled module.
///
/// Thin wrapper over the crate's own `reify_compiler::find_template` — kept only
/// for the panic message, which lists every template actually compiled. Several
/// of the fixtures below fail by producing no template at all, and the bare
/// `.unwrap()` a caller would otherwise write says nothing about why.
fn template<'a>(compiled: &'a reify_compiler::CompiledModule, name: &str) -> &'a TopologyTemplate {
    reify_compiler::find_template(&compiled.templates, name).unwrap_or_else(|| {
        let names: Vec<&str> = compiled.templates.iter().map(|t| t.name.as_str()).collect();
        panic!("template {name} absent from compiled module; have: {names:?}")
    })
}

/// Return the `cell_type` of the `Let` value cell `<entity>.<member>`, panicking
/// with the template's whole cell list when it is absent.
///
/// Absence is itself the primary RED signal: when the relaying `let` is
/// mis-lowered as geometry it becomes a `RealizationDecl` and no value cell is
/// emitted at all.
fn let_cell_type(tmpl: &TopologyTemplate, member: &str) -> Type {
    tmpl.value_cells
        .iter()
        .find(|c| c.id.member == member && c.kind == ValueCellKind::Let)
        .unwrap_or_else(|| {
            let cells: Vec<String> = tmpl
                .value_cells
                .iter()
                .map(|c| format!("{}:{:?}={:?}", c.id.member, c.kind, c.cell_type))
                .collect();
            panic!(
                "template {} has no `let` value cell named {member:?}; cells: {cells:?}",
                tmpl.name
            )
        })
        .cell_type
        .clone()
}

/// Assert no `RealizationDecl` on `tmpl` carries the user-facing name `name`.
///
/// A relaying `let` over a sub member is a VALUE read; emitting a realization
/// for it is the exact defect (`GeometryHandle { kernel_handle: None }` at
/// eval). Other realizations on the template are left alone.
fn assert_no_realization_named(tmpl: &TopologyTemplate, name: &str) {
    let offending: Vec<&reify_compiler::RealizationDecl> = tmpl
        .realizations
        .iter()
        .filter(|r| r.name.as_deref() == Some(name))
        .collect();
    assert!(
        offending.is_empty(),
        "template {}: `let {name}` reads a sub MEMBER VALUE and must not lower to a \
         RealizationDecl; got: {offending:?}",
        tmpl.name,
    );
}

/// The Length-dimensioned scalar type, spelled once.
fn scalar_length() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// (a) plain sub whose target is a prelude STRUCTURE
// ─────────────────────────────────────────────────────────────────────────────

const SRC_PRELUDE_STRUCTURE_SUB: &str = r#"
structure def Mid {
    sub tol = DimensionalTolerance(nominal: 10mm, upper_deviation: 2mm, lower_deviation: 1mm)
    let relay = self.tol.tolerance_band
}
"#;

/// `DimensionalTolerance` (`stdlib/tolerancing.ri`) carries its own derived let
/// `tolerance_band = upper_deviation - lower_deviation : Length`. Relaying it
/// through `self.tol.tolerance_band` must produce a `Scalar[Length]` value cell.
///
/// RED before the fix: `cell Mid.relay : Error` plus a bogus
/// `RealizationDecl { name: Some("relay") }`, with zero diagnostics.
#[test]
fn prelude_structure_sub_member_read_is_typed_not_geometry() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(SRC_PRELUDE_STRUCTURE_SUB);
    let mid = template(&compiled, "Mid");

    assert_eq!(
        let_cell_type(mid, "relay"),
        scalar_length(),
        "Mid.relay must be typed from the PRELUDE template DimensionalTolerance, \
         not fall through to the cross-sub-geometry path",
    );
    assert_no_realization_named(mid, "relay");
}

// ─────────────────────────────────────────────────────────────────────────────
// (b) discriminator control: identical shape, MODULE-LOCAL sub target
// ─────────────────────────────────────────────────────────────────────────────

const SRC_USER_MODULE_SUB: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Ctl {
    sub k = Kid()
    let ctl_relay = self.k.off
}
"#;

/// The prelude-vs-module discriminator. This shape is GREEN today because
/// `Kid` lives in `CompiledModule::templates`, which the module-only
/// `find_template` does see.
///
/// It is pinned so a future regression cannot "pass" (a) by breaking both
/// alike — the defect is specifically an asymmetry between the two.
#[test]
fn user_module_sub_member_read_control_unchanged() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(SRC_USER_MODULE_SUB);
    let ctl = template(&compiled, "Ctl");

    assert_eq!(
        let_cell_type(ctl, "ctl_relay"),
        scalar_length(),
        "the module-local control must stay typed — if this fails the fix broke \
         the case that already worked",
    );
    assert_no_realization_named(ctl, "ctl_relay");
}

// ─────────────────────────────────────────────────────────────────────────────
// (b2) module-local definitions SHADOW the prelude — both declaration orders
// ─────────────────────────────────────────────────────────────────────────────

const SRC_LOCAL_SHADOWS_PRELUDE_BACKWARD: &str = r#"
structure def DisplayStyle {
    param opacity : Length = 3mm
}

structure def Styler {
    sub d = DisplayStyle()
    let op = self.d.opacity
}
"#;

/// Property 1 of `types::find_template_with_prelude` — module-local shadows
/// prelude — pinned with a name that ACTUALLY collides.
///
/// Test (b) above uses `Kid`, which has no prelude counterpart, so it only
/// exercises "module lookup still works". This one redefines the real stdlib
/// `DisplayStyle` (whose `opacity` is `Real`) with a `Length`-typed `opacity`,
/// so the two candidates are distinguishable by type alone: flipping the
/// `.or_else` order to prelude-first would type `op` as the prelude's `Real`
/// and fail here, while leaving every other test in this module green.
#[test]
fn module_local_definition_shadows_same_named_prelude_template() {
    let compiled =
        reify_test_support::compile_source_with_stdlib(SRC_LOCAL_SHADOWS_PRELUDE_BACKWARD);
    let styler = template(&compiled, "Styler");

    assert_eq!(
        let_cell_type(styler, "op"),
        scalar_length(),
        "`self.d.opacity` must resolve against the MODULE-LOCAL DisplayStyle \
         (opacity : Length), not the stdlib one (opacity : Real)",
    );
    assert_no_realization_named(styler, "op");
}

const SRC_LOCAL_SHADOWS_PRELUDE_FORWARD: &str = r#"
structure def User {
    sub c = Color(alpha: 0.5mm)
    let a = self.c.alpha
}

structure def Color {
    param alpha : Length = 1mm
}
"#;

/// The FORWARD-reference half of the same property, and the reason
/// `find_template_with_prelude` takes a `local_entity_names` set rather than
/// relying on `find_template` losing the race.
///
/// `compiled_templates` holds only the templates compiled EARLIER in the
/// module, so for a local definition that appears AFTER its first use the
/// module-first arm misses and — without the local-name guard — the prelude arm
/// would bind `Color` to the stdlib `Color` (`materials_appearance.ri:17`,
/// which has no `alpha`). Generic stdlib names (`Color`, `Layer`, `Datum`,
/// `Fit`, `Coating`, `Frame3`, …) make that collision realistic.
///
/// MEASURED: at base commit 30d90a097a this source compiles with ZERO error
/// diagnostics; with the unguarded prelude fallback it emitted a hard
/// `unknown member 'alpha' on sub 'c'` — a NEW build failure on source the user
/// considers valid. The local-name guard restores the pre-existing
/// forward-declared/optimistic path for exactly this case, leaving the
/// genuinely-prelude-typed cases (a)/(c)/(e) fixed.
#[test]
fn forward_referenced_local_definition_does_not_bind_to_prelude() {
    let compiled =
        reify_test_support::compile_source_with_stdlib(SRC_LOCAL_SHADOWS_PRELUDE_FORWARD);

    let errors = reify_test_support::errors_only(&compiled);
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("alpha") || d.message.contains("unknown member")),
        "a name the module defines itself must never bind to the same-named PRELUDE \
         template, even when the local definition appears after its first use; got: \
         {errors:?}",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (c) plain sub whose target is a prelude OCCURRENCE
// ─────────────────────────────────────────────────────────────────────────────

const SRC_PRELUDE_OCCURRENCE_SUB: &str = r#"
structure def Part {
    let body = box(10mm, 10mm, 10mm)
    sub o = STLOutput(subject: body, resolution: 0.2mm, path: "o.stl")
    let res = self.o.resolution
}
"#;

/// `sub o = STLOutput(...)` is a shipped, common shape (reify-cli's
/// `output_driver_*.ri` fixtures), and `STLOutput` is an `occurrence def`, not a
/// `structure def`.
///
/// This case is why the prelude sub-target lookup must be UNFILTERED by
/// `EntityKind`: the pre-existing `prelude_template_registry` is
/// `EntityKind::Structure`-filtered (deliberately, for ctor lowering), and
/// reusing it here leaves this case broken. MEASURED: with a Structure-filtered
/// fallback `Part.res` is still `Type::Error` with a bogus realization.
///
/// The legitimate `body` realization must survive untouched.
#[test]
fn prelude_occurrence_sub_member_read_is_typed() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(SRC_PRELUDE_OCCURRENCE_SUB);
    let part = template(&compiled, "Part");

    assert_eq!(
        let_cell_type(part, "res"),
        scalar_length(),
        "an OCCURRENCE-typed prelude sub must be typed too — the prelude sub-target \
         registry must not be EntityKind-filtered",
    );
    assert_no_realization_named(part, "res");

    // Guard the widening: the genuine geometry let must still be a realization.
    assert!(
        part.realizations
            .iter()
            .any(|r| r.name.as_deref() == Some("body")),
        "template Part: the genuine geometry `let body = box(...)` must still lower \
         to a RealizationDecl; got: {:?}",
        part.realizations,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (d) the negative half — an unknown member must be DIAGNOSED, not silent
// ─────────────────────────────────────────────────────────────────────────────

const SRC_PRELUDE_SUB_UNKNOWN_MEMBER: &str = r#"
structure def Mid {
    sub tol = DimensionalTolerance(nominal: 10mm, upper_deviation: 2mm, lower_deviation: 1mm)
    let oops = self.tol.no_such_member
}
"#;

/// The silent-wrong-type class this task closes has a negative half: while the
/// sub is mis-classified as forward-declared, a genuinely bogus member name is
/// accepted with ZERO diagnostics. Once the sub is properly typed from the
/// prelude, `expr.rs`'s existing `make_poison_literal` path fires.
///
/// Compiled through `compile_source_with_stdlib`, which panics on PARSE errors
/// only — unlike `parse_and_compile_with_stdlib`, which also panics on any
/// compile-error diagnostic, and this test's whole point is to assert one.
///
/// MEASURED RED: zero diagnostics. MEASURED GREEN:
/// `unknown member 'no_such_member' on sub 'tol'`.
#[test]
fn unknown_member_on_prelude_typed_sub_is_diagnosed() {
    let compiled = reify_test_support::compile_source_with_stdlib(SRC_PRELUDE_SUB_UNKNOWN_MEMBER);

    let errors = reify_test_support::errors_only(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("no_such_member") && d.message.contains("tol")),
        "reading a non-existent member of a PRELUDE-typed sub must be diagnosed, not \
         silently accepted; got: {errors:?}",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (e) the MATCH-ARM mirror of the same module-only lookup
// ─────────────────────────────────────────────────────────────────────────────

const SRC_PRELUDE_MATCH_ARM_SUB: &str = r#"
enum StyleSel { A, B }

structure def Styled {
    param sel : StyleSel = StyleSel.A
    match sel {
        A => sub s : DisplayStyle,
        B => sub s : DisplayStyle
    }
    let op = self.s.opacity
}
"#;

/// Read the declared type of `<member>` on the PRELUDE template `<template>`,
/// so the expectation below is not a guess about how `Real` surfaces.
fn prelude_member_type(template_name: &str, member: &str) -> Type {
    let prelude = reify_compiler::stdlib_loader::load_stdlib();
    let tmpl = prelude
        .iter()
        .find_map(|m| reify_compiler::find_template(&m.templates, template_name))
        .unwrap_or_else(|| panic!("prelude has no template {template_name}"));
    tmpl.value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("prelude template {template_name} has no member {member}"))
        .cell_type
        .clone()
}

/// The match-arm mirror of the plain-`Sub` defect. The same module-only
/// `find_template` sub-target lookup appears three more times in `entity.rs` —
/// the match-arm `Sub` pre-pass, the per-arm cluster member maps, and
/// `compile_match_arm_decl_group::arm_member_types` — so a prelude-typed arm sub
/// stays broken even after the plain-sub fix.
///
/// `DisplayStyle` is the fixture type because every one of its params is
/// defaulted (`color`, `finish`, `opacity : Real = 1.0`, `wireframe`), so the
/// bare `sub s : DisplayStyle` arm form needs no constructor args. The enum is
/// `StyleSel` and not `Mode`: `Mode` collides with a prelude structure and
/// produces an unrelated `StructureRef("Mode")` mis-resolution (measured).
///
/// MEASURED RED with only the plain-sub fix applied: `cell Styled.op : Error`,
/// a bogus `RealizationDecl { name: Some("op") }`, and TWO false-positive errors
/// `field 'opacity' is not present in match-arm types: DisplayStyle,
/// DisplayStyle` — the per-arm member maps come back empty because the arm-type
/// lookup is still module-only. That false-positive-for-a-resolvable-name is the
/// same failure mode as esc-5360-9.
#[test]
fn prelude_typed_match_arm_sub_member_read_is_typed() {
    let compiled = reify_test_support::compile_source_with_stdlib(SRC_PRELUDE_MATCH_ARM_SUB);
    let styled = template(&compiled, "Styled");

    let op_type = let_cell_type(styled, "op");
    assert_ne!(
        op_type,
        Type::Error,
        "Styled.op must be typed from the prelude DisplayStyle arm types",
    );
    assert_ne!(
        op_type,
        Type::Geometry,
        "Styled.op reads a MEMBER VALUE and must not fall through to the \
         cross-sub-geometry path",
    );
    assert_eq!(
        op_type,
        prelude_member_type("DisplayStyle", "opacity"),
        "Styled.op must carry the declared type of DisplayStyle.opacity",
    );

    assert_no_realization_named(styled, "op");

    let errors = reify_test_support::errors_only(&compiled);
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("not present in match-arm types")),
        "`opacity` IS present on every arm type — a prelude-typed arm sub must not \
         be reported as missing it; got: {errors:?}",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (f) match arms with two DIFFERENT prelude types — the per-arm INTERSECTION
// ─────────────────────────────────────────────────────────────────────────────

/// Both arms of (e) are `DisplayStyle`, so the per-arm member maps built at
/// `entity.rs`'s cluster site and in `compile_match_arm_decl_group` are
/// identical and the agreement/intersection logic over them is only exercised
/// in the degenerate single-type case. These fixtures give the two arms
/// DIFFERENT prelude structures.
///
/// `AsPrintedOptions` (`stdlib/fdm_as_printed.ri:37`) and `FDMSliceOptions`
/// (`stdlib/fdm_slice.ri:96`) are the pair because (i) every param of each is
/// defaulted, so the bare `sub s : T` arm form needs no constructor args;
/// (ii) they SHARE `target_fidelity : Int = 0`, giving the positive half; and
/// (iii) `line_width : Length` is on `AsPrintedOptions` ONLY, giving the
/// negative half. Neither `FidSel` nor its variants collide with a prelude
/// name (checked against all 131 stdlib entity defs).
const SRC_TWO_PRELUDE_ARM_TYPES_COMMON: &str = r#"
enum FidSel { AsPrinted, Slice }

structure def Fid {
    param sel : FidSel = FidSel.AsPrinted
    match sel {
        AsPrinted => sub s : AsPrintedOptions,
        Slice => sub s : FDMSliceOptions
    }
    let fid = self.s.target_fidelity
}
"#;

/// Positive half: a member present on BOTH distinct prelude arm types resolves,
/// and carries the type both arms declare.
///
/// A bug where one arm's lookup resolves and the other's does not — the exact
/// asymmetry class this task exists to close — passes (e) but fails here.
#[test]
fn two_distinct_prelude_arm_types_agree_on_common_member() {
    // The fixture's premise, asserted rather than assumed: both prelude arm
    // types really do declare `target_fidelity`, at the same type.
    let as_printed = prelude_member_type("AsPrintedOptions", "target_fidelity");
    let fdm_slice = prelude_member_type("FDMSliceOptions", "target_fidelity");
    assert_eq!(
        as_printed, fdm_slice,
        "fixture premise: the two prelude arm types must declare `target_fidelity` \
         at the SAME type, else the intersection is not expected to agree",
    );

    let compiled = reify_test_support::compile_source_with_stdlib(SRC_TWO_PRELUDE_ARM_TYPES_COMMON);
    let fid = template(&compiled, "Fid");

    let fid_type = let_cell_type(fid, "fid");
    assert_ne!(fid_type, Type::Error, "Fid.fid must be typed, not poisoned");
    assert_ne!(
        fid_type,
        Type::Geometry,
        "Fid.fid reads a MEMBER VALUE and must not fall through to the \
         cross-sub-geometry path",
    );
    assert_eq!(
        fid_type, as_printed,
        "Fid.fid must carry the type both prelude arm types declare for \
         `target_fidelity`",
    );
    assert_no_realization_named(fid, "fid");

    let errors = reify_test_support::errors_only(&compiled);
    assert!(
        !errors
            .iter()
            .any(|d| d.message.contains("not present in match-arm types")),
        "`target_fidelity` IS present on both prelude arm types; got: {errors:?}",
    );
}

const SRC_TWO_PRELUDE_ARM_TYPES_DISJOINT: &str = r#"
enum FidSel { AsPrinted, Slice }

structure def FidBad {
    param sel : FidSel = FidSel.AsPrinted
    match sel {
        AsPrinted => sub s : AsPrintedOptions,
        Slice => sub s : FDMSliceOptions
    }
    let lw = self.s.line_width
}
"#;

/// Negative half: a member present on only ONE of the two prelude arm types
/// must still draw the `not present in match-arm types` diagnostic.
///
/// Widening the arm-type lookup to consult the prelude must not turn the
/// intersection into a union — pinning only the positive half would let a
/// change that accepts any-arm members pass. `line_width` is on
/// `AsPrintedOptions` and not on `FDMSliceOptions`.
#[test]
fn member_on_only_one_prelude_arm_type_is_still_diagnosed() {
    // Fixture premise: `line_width` really is exclusive to the first arm type.
    let _ = prelude_member_type("AsPrintedOptions", "line_width");
    let fdm_slice = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find_map(|m| reify_compiler::find_template(&m.templates, "FDMSliceOptions"))
        .expect("prelude has no template FDMSliceOptions")
        .value_cells
        .iter()
        .any(|c| c.id.member == "line_width");
    assert!(
        !fdm_slice,
        "fixture premise: FDMSliceOptions must NOT declare `line_width`",
    );

    let compiled =
        reify_test_support::compile_source_with_stdlib(SRC_TWO_PRELUDE_ARM_TYPES_DISJOINT);

    let errors = reify_test_support::errors_only(&compiled);
    assert!(
        errors.iter().any(|d| {
            d.message.contains("not present in match-arm types") && d.message.contains("line_width")
        }),
        "a member declared by only ONE prelude arm type must be rejected — the \
         per-arm maps intersect, they do not union; got: {errors:?}",
    );
}
