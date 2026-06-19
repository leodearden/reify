//! Compiler tests for the remaining `auto` binding sites (task 3810, ε):
//! - LET: `let m : Length = auto` (steps 1–2)
//! - CONSTRUCTION named-arg (sub paren-form): `sub bolt : Bolt(length: auto)` (steps 3–4)
//! - CONNECT-PARAM: `connect a -> b : ConnType { gain = auto }` (steps 5–6)
//!
//! ## Structure
//!
//! Each site follows the RED→GREEN arc established in task 3806 (γ):
//! - RED tests assert the compiler emits a scoped/top-level `ValueCellKind::Auto`
//!   cell with the correct `id`, `kind`, and `cell_type`.
//! - They fail until the corresponding `entity.rs` / `connect.rs` producer is wired.

use reify_core::{Type, ValueCellId};
use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only};
use reify_compiler::{find_template, ValueCellKind};

// ── LET site (steps 1–2) ──────────────────────────────────────────────────────

/// `let m : Length = auto` (strict) → top-level `ValueCellKind::Auto { free: false }`.
///
/// RED until step-2 wires the let-auto branch in the entity.rs Let arm.
#[test]
fn let_auto_strict_emits_auto_value_cell() {
    let source = "structure E { let m : Length = auto }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E", "m");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: false },
        "expected Auto {{ free: false }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}

/// `let m : Length = auto(free)` → top-level `ValueCellKind::Auto { free: true }`.
///
/// RED until step-2.
#[test]
fn let_auto_free_emits_auto_free_cell() {
    let source = "structure E { let m : Length = auto(free) }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E", "m");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: true },
        "expected Auto {{ free: true }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}

/// `let m = auto` (no type annotation) → error diagnostic.
///
/// A solver cell needs a declared type; without one we emit a precise diagnostic
/// rather than a silently dimensionless cell.
///
/// RED until step-2 (currently `auto` in a let is compiled as `Value::Undef` with
/// no diagnostic).
#[test]
fn let_auto_without_type_emits_diagnostic() {
    // NOTE: `let m = auto` is syntactically valid (let without type annotation is
    // allowed in the grammar), but semantically invalid when the value is `auto`
    // because the solver needs a declared type to set up the cell.
    let source = "structure E { let m = auto }";
    let module = compile_source_with_stdlib(source);

    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected an error diagnostic for untyped `let m = auto`; got none \
         (diagnostics: {:?})",
        module.diagnostics
    );
}

// ── CONSTRUCTION named-arg / sub paren-form (steps 3–4) ──────────────────────

/// `sub bolt = Bolt(length: auto)` → scoped `ValueCellKind::Auto { free: false }`.
///
/// The scoped id is `ValueCellId::new("E.bolt", "length")`, mirroring the
/// 3806/γ convention for spec_param_overrides.
/// Syntax: paren-form uses `=` before the structure name (grammar rule `named_argument`).
///
/// RED until step-4 wires the sub.args auto loop in entity.rs.
#[test]
fn construction_named_arg_auto_emits_scoped_auto_cell() {
    let source =
        "structure Bolt { param length : Length = 5mm }  \
         structure E { sub bolt = Bolt(length: auto) }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E.bolt", "length");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: false },
        "expected Auto {{ free: false }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}

/// `sub bolt = Bolt(length: auto(free))` → scoped `ValueCellKind::Auto { free: true }`.
///
/// RED until step-4.
#[test]
fn construction_named_arg_auto_free_emits_scoped_auto_free_cell() {
    let source =
        "structure Bolt { param length : Length = 5mm }  \
         structure E { sub bolt = Bolt(length: auto(free)) }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E.bolt", "length");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: true },
        "expected Auto {{ free: true }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}

/// `sub bolt = Bolt(nope: auto)` → "no such param" error (absent member).
///
/// RED until step-4 (currently auto in sub.args compiles silently to Undef,
/// no "no such param" check for paren-form autos).
#[test]
fn construction_named_arg_auto_unknown_member_emits_error() {
    let source =
        "structure Bolt { param length : Length = 5mm }  \
         structure E { sub bolt = Bolt(nope: auto) }";
    let module = compile_source_with_stdlib(source);

    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected an error for absent member `nope` in Bolt; got no errors \
         (diagnostics: {:?})",
        module.diagnostics
    );
    assert!(
        errors.iter().any(|e| e.message.contains("nope") || e.message.contains("Bolt")),
        "error should name the absent member or the structure; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── CONNECT-PARAM site (steps 5–6) ───────────────────────────────────────────

/// `connect a -> b : ConnType { gain = auto }` →
/// parent template has `ValueCellId::new("E.__connector_0", "gain")` with
/// `kind == Auto { free: false }` and `cell_type == Length`.
///
/// RED until step-6 wires the connect-param producer in connect.rs.
#[test]
fn connect_param_auto_emits_scoped_auto_cell() {
    // ConnType declares a `gain` param; E connects two ports via ConnType with `gain = auto`.
    // We use trivial trait-typed ports for the minimal test surface.
    let source = r#"
trait Signal {}
structure ConnType {
    param gain : Length = 5mm
}
structure E {
    port a : Signal = out
    port b : Signal = in
    connect a -> b : ConnType { gain = auto }
}
"#;
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E.__connector_0", "gain");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: false },
        "expected Auto {{ free: false }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}

/// `connect a -> b : ConnType { gain = auto(free) }` →
/// `ValueCellKind::Auto { free: true }`.
///
/// RED until step-6.
#[test]
fn connect_param_auto_free_emits_scoped_auto_free_cell() {
    let source = r#"
trait Signal {}
structure ConnType {
    param gain : Length = 5mm
}
structure E {
    port a : Signal = out
    port b : Signal = in
    connect a -> b : ConnType { gain = auto(free) }
}
"#;
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "E")
        .expect("expected a compiled template for structure E");

    let target_id = ValueCellId::new("E.__connector_0", "gain");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template E; got cells: {:?}",
                target_id,
                template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        cell.kind,
        ValueCellKind::Auto { free: true },
        "expected Auto {{ free: true }}, got {:?}",
        cell.kind
    );

    assert_eq!(
        cell.cell_type,
        Type::length(),
        "expected cell_type == Length, got {:?}",
        cell.cell_type
    );
}
