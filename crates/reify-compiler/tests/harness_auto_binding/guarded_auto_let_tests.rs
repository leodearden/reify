//! Compiler tests for `auto` let bindings inside a guarded (`where`) block — task 6888.
//!
//! The UNGUARDED counterpart of binding site 3 (`let m : Length = auto`) is covered by the
//! sibling `auto_binding_sites_remaining_tests.rs`. A guarded auto let lowers to a
//! `ValueCellKind::Auto` cell typed by its declared annotation, and that type is already in
//! scope for a constraint in the same block, which is what the `constraint` line in each
//! source below pins.
//!
//! These are compiler-IR assertions only: a guarded auto cell is NOT resolved by the solver
//! today (the auto-resolution pass traverses neither guarded-group members nor guarded
//! constraints), so nothing here asserts on evaluated values.

use reify_compiler::{
    CompiledGuardedGroup, CompiledModule, TopologyTemplate, ValueCellDecl, ValueCellKind,
    find_template,
};
use reify_core::{Type, ValueCellId};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── local helpers ─────────────────────────────────────────────────────────────

fn assert_no_errors(module: &CompiledModule) {
    let errors = errors_only(module);
    assert!(
        errors.is_empty(),
        "expected no error-severity diagnostics, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

fn template_of<'a>(module: &'a CompiledModule, structure: &str) -> &'a TopologyTemplate {
    find_template(&module.templates, structure)
        .unwrap_or_else(|| panic!("expected a compiled template for structure {structure}"))
}

fn only_group(template: &TopologyTemplate) -> &CompiledGuardedGroup {
    assert_eq!(
        template.guarded_groups.len(),
        1,
        "expected exactly one guarded group in template {}",
        template.name
    );
    &template.guarded_groups[0]
}

fn member_named<'a>(members: &'a [ValueCellDecl], name: &str) -> &'a ValueCellDecl {
    members
        .iter()
        .find(|m| m.id.member == name)
        .unwrap_or_else(|| {
            panic!(
                "expected a guarded member named {name:?}; got members: {:?}",
                members
                    .iter()
                    .map(|m| m.id.member.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// Pin the full auto-cell shape: an `Auto { free }` solver cell carrying the DECLARED
/// annotation and no default expression.
fn assert_auto_cell(member: &ValueCellDecl, entity: &str, name: &str, free: bool) {
    assert_eq!(
        member.id,
        ValueCellId::new(entity, name),
        "unexpected value-cell id"
    );
    assert_eq!(
        member.kind,
        ValueCellKind::Auto { free },
        "expected Auto {{ free: {free} }} for {name}, got {:?}",
        member.kind
    );
    assert_eq!(
        member.cell_type,
        Type::length(),
        "expected the declared `: Length` annotation as {name}'s cell_type, got {:?}",
        member.cell_type
    );
    assert!(
        member.default_expr.is_none(),
        "an auto cell has no initializer, but {name} carries default_expr: {:?}",
        member.default_expr
    );
}

// ── (a) strict auto ───────────────────────────────────────────────────────────

#[test]
fn guarded_strict_auto_let_mints_auto_value_cell() {
    let source = "structure GA { param g : Real = 1.0  \
                  where g > 0.0 { let m : Length = auto  constraint self.m == 10mm } }";
    let module = compile_source_with_stdlib(source);

    assert_no_errors(&module);

    let template = template_of(&module, "GA");
    let group = only_group(template);
    assert_auto_cell(member_named(&group.members, "m"), "GA", "m", false);

    // The cell belongs to the guarded group, not the template's top-level cells —
    // guards against a fix that pushes onto the wrong vec.
    assert!(
        !template
            .value_cells
            .iter()
            .any(|c| c.id == ValueCellId::new("GA", "m")),
        "a guarded auto let must stay scoped to its group, but `m` also appeared in \
         template.value_cells"
    );
}

// ── (b) auto(free) ────────────────────────────────────────────────────────────

#[test]
fn guarded_free_auto_let_mints_auto_free_cell() {
    let source = "structure GB { param g : Real = 1.0  \
                  where g > 0.0 { let m : Length = auto(free)  constraint self.m == 10mm } }";
    let module = compile_source_with_stdlib(source);

    assert_no_errors(&module);

    let group = only_group(template_of(&module, "GB"));
    assert_auto_cell(member_named(&group.members, "m"), "GB", "m", true);
}

// ── (c) else branch ───────────────────────────────────────────────────────────

/// Both branches share `compile_guarded_members`, so this is the cheap guard against a fix
/// applied to only one of the two call paths.
#[test]
fn guarded_auto_let_in_else_branch_mints_auto_cell() {
    let source = "structure GC { param g : Real = 1.0  \
                  where g > 0.0 { let a : Length = auto  constraint self.a == 10mm } \
                  else { let b : Length = auto  constraint self.b == 20mm } }";
    let module = compile_source_with_stdlib(source);

    assert_no_errors(&module);

    let group = only_group(template_of(&module, "GC"));
    assert_auto_cell(member_named(&group.members, "a"), "GC", "a", false);
    assert_auto_cell(member_named(&group.else_members, "b"), "GC", "b", false);
}

// ── (d) nested guard ──────────────────────────────────────────────────────────

/// Pins the recursive `MemberDecl::GuardedGroup` path: the inner block's members are compiled
/// by the same arm, reached one level down.
#[test]
fn nested_guarded_auto_let_mints_auto_cell() {
    let source = "structure GE { param g : Real = 1.0  param h : Real = 2.0  \
                  where g > 0.0 { where h > 0.0 { let m : Length = auto  \
                  constraint self.m == 10mm } } }";
    let module = compile_source_with_stdlib(source);

    assert_no_errors(&module);

    let template = template_of(&module, "GE");
    assert_eq!(
        template.guarded_groups.len(),
        2,
        "expected an outer and an inner guarded group"
    );
    let inner = template
        .guarded_groups
        .iter()
        .find(|grp| grp.parent_guard.is_some())
        .expect("expected one guarded group with a parent_guard (the inner `where h > 0.0`)");

    assert_auto_cell(member_named(&inner.members, "m"), "GE", "m", false);
}

/// The guarded auto-let annotation is resolved twice — once by the name-registration prepass,
/// once by the member pass — so every error it can raise must still reach the user exactly
/// once. Returns that single error's message.
fn single_error_message(module: &CompiledModule) -> &str {
    let errors = errors_only(module);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error diagnostic, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    &errors[0].message
}

// ── (e) missing annotation ────────────────────────────────────────────────────

/// An auto let is a solver cell, so an untyped one has no type to solve for: it is rejected
/// and mints no cell.
#[test]
fn untyped_guarded_auto_let_emits_missing_annotation_error() {
    let source = "structure GD { param g : Real = 1.0  where g > 0.0 { let m = auto } }";
    let module = compile_source_with_stdlib(source);

    let message = single_error_message(&module);
    assert!(
        message.contains("auto let binding requires a type annotation"),
        "unexpected error message: {message:?}"
    );

    let group = only_group(template_of(&module, "GD"));
    assert!(
        !group.members.iter().any(|m| m.id.member == "m"),
        "a rejected auto let must mint no cell at all, got members: {:?}",
        group
            .members
            .iter()
            .map(|m| m.id.member.as_str())
            .collect::<Vec<_>>()
    );
}

// ── (f) `Keyed<T>` annotation ─────────────────────────────────────────────────

#[test]
fn guarded_auto_let_with_keyed_annotation_errors_once() {
    let source = "structure def Vent { param area : Length = 1mm } \
                  structure GK { param g : Real = 1.0  \
                  where g > 0.0 { let y : Keyed<Vent> = auto } }";
    let module = compile_source_with_stdlib(source);

    let message = single_error_message(&module);
    assert!(
        message.contains("sub-only collection kind"),
        "expected the Keyed value-position rejection, got: {message:?}"
    );
}
