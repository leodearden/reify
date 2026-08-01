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
use reify_core::{DimensionVector, ModulePath, Severity, Type};

/// Fetch the named template from a compiled module, panicking with the full
/// template list when absent.
fn template<'a>(compiled: &'a reify_compiler::CompiledModule, name: &str) -> &'a TopologyTemplate {
    compiled
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
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
/// Compiled through `compile_with_stdlib` directly rather than
/// `parse_and_compile_with_stdlib`, which panics on any error diagnostic.
///
/// MEASURED RED: zero diagnostics. MEASURED GREEN:
/// `unknown member 'no_such_member' on sub 'tol'`.
#[test]
fn unknown_member_on_prelude_typed_sub_is_diagnosed() {
    let parsed = reify_compiler::parse_with_stdlib(
        SRC_PRELUDE_SUB_UNKNOWN_MEMBER,
        ModulePath::single("test"),
    );
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse cleanly; got: {:?}",
        parsed.errors,
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<&reify_core::Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("no_such_member") && d.message.contains("tol")),
        "reading a non-existent member of a PRELUDE-typed sub must be diagnosed, not \
         silently accepted; got: {errors:?}",
    );
}
