//! Compiler tests for sub-instance-override `auto` (task 3806, steps 3–4).
//!
//! These tests verify that when a `sub` specialization body contains a
//! `param_assignment` whose value is `auto` or `auto(free)`, the compiler
//! emits a scoped `ValueCellDecl` in the **parent** template's `value_cells`
//! with the correct `id`, `kind`, and `cell_type`.
//!
//! ## The RED→GREEN arc
//!
//! Step 3 (RED): The tests compile source that includes a sub override like
//! `sub b : Bearing { bore = auto }`. Until step 4 wires up the entity.rs
//! producer, no such cell appears in the parent template — so the asserts fail.
//!
//! Step 4 (GREEN): `entity.rs` iterates `sub.param_overrides`, detects
//! `ExprKind::Auto { free }`, resolves the member type from the child
//! template, and pushes a scoped `ValueCellDecl { kind: Auto { free }, … }`
//! into the parent's `value_cells`.  After that the assertions below pass.

use reify_core::{Type, ValueCellId};
use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_compiler::{find_template, ValueCellKind};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Source shared by both tests — Bearing supplies a `bore : Length` param;
/// structure A instantiates it with an override.
const BEARING_PREAMBLE: &str =
    "structure Bearing { param bore : Length = 10mm }";

/// Build the full test source for a given override expression.
fn source_with_override(override_expr: &str) -> String {
    format!(
        "{BEARING_PREAMBLE}  structure A {{ sub b : Bearing {{ bore = {override_expr} }} }}",
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `bore = auto` (strict) → scoped cell `ValueCellKind::Auto { free: false }`.
///
/// The scoped id must be `ValueCellId::new("A.b", "bore")`, matching the
/// convention in `crates/reify-compiler/src/expr.rs:1529-1531` used when a
/// constraint references `self.b.bore`.
#[test]
fn sub_override_auto_strict_emits_scoped_auto_cell() {
    let source = source_with_override("auto");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");

    let target_id = ValueCellId::new("A.b", "bore");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template A; got cells: {:?}",
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

/// `bore = auto(free)` → scoped cell `ValueCellKind::Auto { free: true }`.
///
/// The `free` flag propagates verbatim from the lowered `ExprKind::Auto { free:
/// true }` through the compiler into the `ValueCellKind`.
#[test]
fn sub_override_auto_free_emits_scoped_auto_free_cell() {
    let source = source_with_override("auto(free)");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");

    let target_id = ValueCellId::new("A.b", "bore");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id == target_id)
        .unwrap_or_else(|| {
            panic!(
                "expected a value cell with id {:?} in template A; got cells: {:?}",
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
