//! Trait type-arg rejection — task 5049 α (E_TYPE_ARG_ON_TRAIT).
//!
//! Tests for:
//! (a) A trait name used with non-empty type args (`SpecLike<Foo>`) emits
//!     exactly one `DiagnosticCode::TypeArgOnTrait` naming the trait and its
//!     type argument, and poisons the annotated cell to `Type::Error`
//!     (the trait-with-args intercept arm in
//!     `resolve_type_expr_with_aliases_kinded`).
//! (b) Bare trait names (no type args) keep resolving to `Type::TraitObject`
//!     byte-identically — guard, must stay green pre- and post-fix.
//! (c) Structure names with type args keep resolving via the 4603
//!     `Type::Applied` path byte-identically — guard, must stay green pre-
//!     and post-fix.
//!
//! Scaffold replicates the committed probe fixture
//! `tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri`
//! (`SpecLike` / `Foo` / `Holder`), plus the 4603 `Coupling<P: HasMotion>` /
//! `Prismatic` scaffold for the structure-with-args guard.

use reify_core::{diagnostics::DiagnosticCode, Type};
use reify_test_support::compile_source;

// ─── Shared fixture source ───────────────────────────────────────────────────
//
// `SpecLike` trait (mirrors the probe fixture), a non-conforming plain
// structure `Foo`, `HasMotion` trait with conforming structure `Prismatic`,
// and a generic `Coupling<P: HasMotion>` (4603 structure-with-args scaffold).
fn base_source() -> &'static str {
    r#"
        trait SpecLike {
            param density : Real = 1.0
        }

        structure def Foo {
            param x : Real = 1.0
        }

        trait HasMotion {}
        structure def Prismatic : HasMotion {}
        structure def Coupling<P: HasMotion> { param p : P }
    "#
}

// ═══════════════════════════════════════════════════════════════════════════════
// RED: trait-with-args rejection (step-4 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// `param m : SpecLike<Foo>` (trait name with a non-empty type arg) must emit
/// exactly one `DiagnosticCode::TypeArgOnTrait` diagnostic naming the trait
/// `SpecLike` and the type argument `Foo`.
///
/// RED until step-4: today this silently resolves to `Type::TraitObject("SpecLike")`
/// with `Foo` dropped and zero diagnostics.
#[test]
fn trait_with_args_emits_type_arg_on_trait() {
    let source = format!(
        "{}\nstructure def Holder {{ param m : SpecLike<Foo> }}",
        base_source()
    );
    let module = compile_source(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgOnTrait))
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "SpecLike<Foo> (trait with type args) must emit exactly one TypeArgOnTrait \
         diagnostic; got: {:?}",
        errors
    );

    let message = &errors[0].message;
    assert!(
        message.contains("SpecLike"),
        "TypeArgOnTrait message must name the trait 'SpecLike'; got: {}",
        message
    );
    assert!(
        message.contains("Foo"),
        "TypeArgOnTrait message must name the type argument 'Foo'; got: {}",
        message
    );
}

/// `param m : SpecLike<Foo>` must poison the `m` value cell to `Type::Error`
/// (anti-cascade sentinel), not `Type::TraitObject` and not `Type::Applied`.
///
/// RED until step-4.
#[test]
fn trait_with_args_poisons_cell_to_error() {
    let source = format!(
        "{}\nstructure def Holder {{ param m : SpecLike<Foo> }}",
        base_source()
    );
    let module = compile_source(&source);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Holder")
        .expect("Holder template must exist");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("Holder must have a value cell named 'm'");

    assert_eq!(
        m_cell.cell_type,
        Type::Error,
        "SpecLike<Foo> must poison the 'm' cell to Type::Error; got {:?}",
        m_cell.cell_type
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// GUARD: byte-identical pre-existing behavior (green pre+post step-4)
// ═══════════════════════════════════════════════════════════════════════════════

/// Empty-args invariant: `param m : SpecLike` (no type args) must still
/// resolve to `Type::TraitObject("SpecLike")` and emit no `TypeArgOnTrait`.
///
/// Must stay GREEN through step-4 (empty-args → fallthrough → TraitObject,
/// unchanged).
#[test]
fn bare_trait_object_unchanged_when_no_type_args() {
    let source = format!(
        "{}\nstructure def Holder {{ param m : SpecLike }}",
        base_source()
    );
    let module = compile_source(&source);

    let type_arg_on_trait_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgOnTrait))
        .collect();
    assert!(
        type_arg_on_trait_errors.is_empty(),
        "bare `SpecLike` (no type args) must emit NO TypeArgOnTrait; got: {:?}",
        type_arg_on_trait_errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Holder")
        .expect("Holder template must exist");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("Holder must have a value cell named 'm'");

    assert_eq!(
        m_cell.cell_type,
        Type::TraitObject("SpecLike".to_string()),
        "bare `SpecLike` must resolve to TraitObject(\"SpecLike\"), got {:?}",
        m_cell.cell_type
    );
}

/// Structure-with-args invariant: `param c : Coupling<Prismatic>` must still
/// resolve via the 4603 `Type::Applied` path and emit no `TypeArgOnTrait`.
///
/// Must stay GREEN through step-4 (structure arm runs before the new trait
/// arm, so structures are entirely unaffected).
#[test]
fn structure_with_args_applied_path_unchanged() {
    let source = format!(
        "{}\nstructure def Holder {{ param c : Coupling<Prismatic> }}",
        base_source()
    );
    let module = compile_source(&source);

    let type_arg_on_trait_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgOnTrait))
        .collect();
    assert!(
        type_arg_on_trait_errors.is_empty(),
        "Coupling<Prismatic> (structure with args) must emit NO TypeArgOnTrait; got: {:?}",
        type_arg_on_trait_errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Holder")
        .expect("Holder template must exist");

    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("Holder must have a value cell named 'c'");

    let expected = Type::Applied {
        name: "Coupling".to_string(),
        args: vec![Type::StructureRef("Prismatic".to_string())],
    };
    assert_eq!(
        c_cell.cell_type, expected,
        "Coupling<Prismatic> must resolve to Applied{{\"Coupling\", [StructureRef(\"Prismatic\")]}}, \
         got {:?}",
        c_cell.cell_type
    );
}
