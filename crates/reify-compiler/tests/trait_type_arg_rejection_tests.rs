//! Trait type-arg rejection — task 5049 α (E_TYPE_ARG_ON_TRAIT).
//!
//! Tests for:
//! (a) A trait name used with non-empty type args (`SpecLike<Foo>`) emits
//!     exactly one `DiagnosticCode::TypeArgOnTrait` naming the trait and its
//!     type argument, and poisons the annotated cell to `Type::Error`
//!     (the trait-with-args intercept arm in
//!     `resolve_type_expr_with_aliases_kinded`), with no other diagnostic of
//!     any severity leaking through (anti-cascade).
//! (b) Bare trait names (no type args) keep resolving to `Type::TraitObject`
//!     byte-identically — guard, must stay green pre- and post-fix.
//! (c) Structure names with type args keep resolving via the 4603
//!     `Type::Applied` path byte-identically — guard, must stay green pre-
//!     and post-fix.
//! (d) Multiple type args (`SpecLike<Foo, Bar>`) all render in the message,
//!     and an unresolvable type arg (`SpecLike<DoesNotExist>`) does NOT
//!     additionally trigger its own `UnresolvedType` — the arm renders args
//!     as written rather than recursively resolving them, a deliberate
//!     divergence from the structure-with-args arm's recursive resolution.
//!
//! Scaffold replicates the committed probe fixture
//! `tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri`
//! (`SpecLike` / `Foo` / `Holder`), plus the 4603 `Coupling<P: HasMotion>` /
//! `Prismatic` scaffold for the structure-with-args guard.

use reify_core::{diagnostics::DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

// ─── Shared fixture source ───────────────────────────────────────────────────
//
// `SpecLike` trait (mirrors the probe fixture), two non-conforming plain
// structures `Foo` / `Bar` (the latter for the multi-type-arg case),
// `HasMotion` trait with conforming structure `Prismatic`, and a generic
// `Coupling<P: HasMotion>` (4603 structure-with-args scaffold).
fn base_source() -> &'static str {
    r#"
        trait SpecLike {
            param density : Real = 1.0
        }

        structure def Foo {
            param x : Real = 1.0
        }

        structure def Bar {
            param y : Real = 2.0
        }

        trait HasMotion {}
        structure def Prismatic : HasMotion {}
        structure def Coupling<P: HasMotion> { param p : P }
    "#
}

/// Shared source: a `Holder` structure annotating `m : SpecLike<Foo>` — the
/// core trait-with-args rejection case, reused by both the diagnostic-shape
/// and cell-poison assertions below. Kept as separate `#[test]`s rather than
/// merged into one, matching this crate's test-isolation convention (see
/// e.g. `type_arg_applied_resolution_tests.rs`, where closely related
/// diagnostic-shape/cell-type checks on the same fixture are likewise split
/// across independent tests).
fn holder_with_spec_like_foo_source() -> String {
    format!(
        "{}\nstructure def Holder {{ param m : SpecLike<Foo> }}",
        base_source()
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// RED: trait-with-args rejection (step-4 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// `param m : SpecLike<Foo>` (trait name with a non-empty type arg) must emit
/// exactly one `DiagnosticCode::TypeArgOnTrait` diagnostic naming the trait
/// `SpecLike` and the type argument `Foo`, and must not leak any *other*
/// Error-severity diagnostic (e.g. a spurious `UnresolvedType`) — the whole
/// point of poisoning to `Type::Error` is to suppress that cascade.
///
/// RED until step-4: today this silently resolves to `Type::TraitObject("SpecLike")`
/// with `Foo` dropped and zero diagnostics.
#[test]
fn trait_with_args_emits_type_arg_on_trait() {
    let source = holder_with_spec_like_foo_source();
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

    // Anti-cascade: assert the *total* Error-severity diagnostic count, not
    // just the TypeArgOnTrait-filtered count above, so a regression that
    // reintroduced a second error under a different code (e.g. a leaked
    // UnresolvedType for `Foo`) would fail this test instead of slipping
    // through unnoticed.
    let all_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        all_errors.len(),
        1,
        "SpecLike<Foo> must anti-cascade to exactly one Error-severity diagnostic \
         total (poisoning to Type::Error must suppress any secondary error); got: {:?}",
        all_errors
    );
}

/// `param m : SpecLike<Foo>` must poison the `m` value cell to `Type::Error`
/// (anti-cascade sentinel), not `Type::TraitObject` and not `Type::Applied`.
///
/// RED until step-4.
#[test]
fn trait_with_args_poisons_cell_to_error() {
    let source = holder_with_spec_like_foo_source();
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
// Rejection-arm details: multi-arg rendering + unresolvable-arg divergence
// ═══════════════════════════════════════════════════════════════════════════════

/// Multiple type args (`SpecLike<Foo, Bar>`) must all render in the
/// `TypeArgOnTrait` message, joined `", "` in source order — the single-arg
/// case above (`SpecLike<Foo>`) can't distinguish "renders the one arg" from
/// "renders all args", so this locks in the arm's `args_as_written` join
/// logic specifically.
#[test]
fn trait_with_multiple_type_args_renders_all_in_message() {
    let source = format!(
        "{}\nstructure def Holder {{ param m : SpecLike<Foo, Bar> }}",
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
        "SpecLike<Foo, Bar> must emit exactly one TypeArgOnTrait diagnostic; got: {:?}",
        errors
    );

    let message = &errors[0].message;
    assert!(
        message.contains("SpecLike"),
        "TypeArgOnTrait message must name the trait 'SpecLike'; got: {}",
        message
    );
    assert!(
        message.contains("Foo, Bar"),
        "TypeArgOnTrait message must render both type arguments, joined \"Foo, Bar\" \
         as written; got: {}",
        message
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
        Type::Error,
        "SpecLike<Foo, Bar> must poison the 'm' cell to Type::Error; got {:?}",
        m_cell.cell_type
    );
}

/// An unresolvable type arg (`SpecLike<DoesNotExist>`) must emit ONLY the
/// `TypeArgOnTrait` rejection — no additional `UnresolvedType` for
/// `DoesNotExist`. The rejection arm renders type args as written (via
/// `TypeExpr`'s `Display`) rather than recursively resolving them the way
/// the structure-with-args arm does; this is a deliberate divergence
/// (unlike a real name, an unresolvable arg gets no independent diagnostic
/// of its own), and this test locks that in rather than leaving it an
/// unverified side effect.
#[test]
fn trait_with_unresolvable_type_arg_emits_only_type_arg_on_trait() {
    let source = format!(
        "{}\nstructure def Holder {{ param m : SpecLike<DoesNotExist> }}",
        base_source()
    );
    let module = compile_source(&source);

    let type_arg_on_trait: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgOnTrait))
        .collect();
    assert_eq!(
        type_arg_on_trait.len(),
        1,
        "SpecLike<DoesNotExist> must emit exactly one TypeArgOnTrait diagnostic; got: {:?}",
        type_arg_on_trait
    );
    let message = &type_arg_on_trait[0].message;
    assert!(
        message.contains("SpecLike") && message.contains("DoesNotExist"),
        "TypeArgOnTrait message must name the trait 'SpecLike' and the (unresolved) \
         type argument 'DoesNotExist' as written; got: {}",
        message
    );

    let unresolved: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
        .collect();
    assert!(
        unresolved.is_empty(),
        "SpecLike<DoesNotExist> must NOT additionally emit UnresolvedType for the \
         unresolvable inner arg — the trait rejection arm renders args as written \
         without recursively resolving them; got: {:?}",
        unresolved
    );

    let all_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        all_errors.len(),
        1,
        "SpecLike<DoesNotExist> must anti-cascade to exactly one Error-severity \
         diagnostic total; got: {:?}",
        all_errors
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
        Type::Error,
        "SpecLike<DoesNotExist> must poison the 'm' cell to Type::Error; got {:?}",
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
