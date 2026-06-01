//! Compiler tests for sub-instance-override `auto` (task 3806, steps 3–4, 9–10).
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
//! Step 4 (GREEN): `entity.rs` iterates `sub.spec_param_overrides`, detects
//! `ExprKind::Auto { free }`, resolves the member type from the child
//! template, and pushes a scoped `ValueCellDecl { kind: Auto { free }, … }`
//! into the parent's `value_cells`.  After that the assertions below pass.
//!
//! Step 9 (RED): Forward-reference regression. Tests A and C expose that when
//! the parent structure is compiled before the child (forward-declared child),
//! the current inline lookup in `entity.rs` emits a spurious "no such param"
//! error and drops the override cell.  Test B is a regression guard: a
//! genuinely-absent member must still produce exactly one error regardless of
//! source order.
//!
//! Step 10 (GREEN): `entity.rs` defers forward-declared-child overrides via a
//! new `pending_sub_override_autos` collection; a post-pass drains it once all
//! templates are compiled, resolves the member type then, and either pushes the
//! scoped `ValueCellDecl` (member found) or emits the genuine "no such param"
//! error (member absent).  Makes Tests A and C GREEN; Test B stays GREEN.

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
///
/// (child declared AFTER parent in source — the nominal order already tested above)
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

// ── Forward-reference tests (step 9 RED → step 10 GREEN) ─────────────────────
//
// When the parent structure is declared BEFORE the child structure (a legal
// forward-reference in the Reify grammar), the compiler processes the parent
// first and the child's `value_cells` are not yet available.  The fix in
// step 10 defers the override registration to a post-pass that runs after all
// templates are compiled.

/// Build source where **parent comes first** (child `Bearing` declared after
/// `A`), so the compiler processes A before Bearing is in `compiled_templates`.
fn source_parent_before_child(override_expr: &str) -> String {
    format!(
        "structure A {{ sub b : Bearing {{ bore = {override_expr} }} }}  \
         structure Bearing {{ param bore : Length = 10mm }}",
    )
}

/// (A) Parent before child, valid override `bore = auto` (strict).
///
/// RED (step 9): current code emits a spurious "no such param" error and drops
///   the cell because `scope.sub_member_types["b"]` is absent when A is
///   compiled (Bearing is not yet in `compiled_templates`).
/// GREEN (step 10): the deferred post-pass resolves the member type from the
///   now-compiled Bearing template and pushes the scoped Auto cell.
#[test]
fn sub_override_auto_strict_forward_declared_child_emits_scoped_cell() {
    let source = source_parent_before_child("auto");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "forward-declared child: expected no errors, got: {:?}",
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
                "forward-declared child: expected scoped cell {:?} in A; got cells: {:?}",
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
        "expected cell_type Length, got {:?}",
        cell.cell_type
    );
}

/// (A-free) Parent before child, valid override `bore = auto(free)`.
///
/// Same forward-reference scenario as above but with the `free` flag.
/// RED until step 10 wires the deferred post-pass.
#[test]
fn sub_override_auto_free_forward_declared_child_emits_scoped_cell() {
    let source = source_parent_before_child("auto(free)");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "forward-declared child (free): expected no errors, got: {:?}",
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
                "forward-declared child (free): expected scoped cell {:?} in A; got cells: {:?}",
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
        "expected cell_type Length, got {:?}",
        cell.cell_type
    );
}

// ── Duplicate-override dedup tests (task 4123 S6, step 7 RED → step 8 GREEN) ──
//
// A specialization body may (erroneously) contain two param_assignment nodes
// for the same member, e.g. `{ bore = auto\n    bore = auto }` — the grammar
// uses `repeat(choice(param_assignment, _member))` with no separator so nothing
// prevents it syntactically.  The parser lowers both into spec_param_overrides,
// producing two entries with the same name.  Without a dedup guard, both push
// sites (entity.rs inline Case 3 AND entities_phase.rs phase_sub_override_autos)
// emit two duplicate scoped ValueCellDecls.  The guard added in step 8 makes
// first-assignment-wins and ensures exactly one cell is emitted.

/// (S6-child-before-parent) Duplicate `bore = auto` entries — child declared
/// BEFORE parent, so the inline Case 3 path in `entity.rs` runs.
///
/// RED (step 7): `entity.rs` pushes two ValueCellDecls for `A.b.bore` (count == 2).
/// GREEN (step 8): id-uniqueness guard in inline Case 3 skips the second push.
#[test]
fn sub_override_auto_duplicate_inline_path_emits_exactly_one_cell() {
    // Child (Bearing) before parent (A) → compiler sees Bearing first, so
    // when A is compiled the child template is already present → inline Case 3.
    let source =
        "structure Bearing { param bore : Length = 10mm }  \
         structure A { sub b : Bearing {\n    bore = auto\n    bore = auto\n} }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "duplicate override (inline path): unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");

    let target_id = ValueCellId::new("A.b", "bore");
    let count = template
        .value_cells
        .iter()
        .filter(|c| c.id == target_id)
        .count();

    assert_eq!(
        count, 1,
        "duplicate override (inline path): expected exactly 1 cell for {:?}, got {count}; \
         cells: {:?}",
        target_id,
        template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
}

/// (S6-parent-before-child) Duplicate `bore = auto` entries — parent declared
/// BEFORE child, so the deferred post-pass `phase_sub_override_autos` runs.
///
/// RED (step 7): `entities_phase.rs` pushes two ValueCellDecls for `A.b.bore`.
/// GREEN (step 8): id-uniqueness guard in the post-pass skips the second push.
#[test]
fn sub_override_auto_duplicate_deferred_path_emits_exactly_one_cell() {
    // Parent (A) before child (Bearing) → compiler processes A first, child is
    // forward-declared → post-pass `phase_sub_override_autos` path.
    let source =
        "structure A { sub b : Bearing {\n    bore = auto\n    bore = auto\n} }  \
         structure Bearing { param bore : Length = 10mm }";
    let module = compile_source_with_stdlib(source);

    assert!(
        errors_only(&module).is_empty(),
        "duplicate override (deferred path): unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");

    let target_id = ValueCellId::new("A.b", "bore");
    let count = template
        .value_cells
        .iter()
        .filter(|c| c.id == target_id)
        .count();

    assert_eq!(
        count, 1,
        "duplicate override (deferred path): expected exactly 1 cell for {:?}, got {count}; \
         cells: {:?}",
        target_id,
        template.value_cells.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
}

/// (B) Regression guard: parent before child, GENUINELY absent member `nope`.
///
/// When the post-pass resolves the deferred entry and the member is truly
/// absent from the child template, exactly one error must be emitted naming
/// `nope` (or `Bearing`), and no scoped cell must be pushed for `nope`.
///
/// This test is GREEN under both the old and new code (the error is emitted
/// either inline or in the post-pass), guarding that the deferred path does
/// NOT silently drop genuine errors.
#[test]
fn sub_override_auto_forward_declared_child_genuinely_missing_member_errors() {
    // Parent before child; `nope` is not a param of Bearing.
    let source = "structure A { sub b : Bearing { nope = auto } }  \
                  structure Bearing { param bore : Length = 10mm }";
    let module = compile_source_with_stdlib(source);

    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error for absent member `nope`; got: {:?}",
        errors
    );
    assert!(
        errors[0].message.contains("nope") || errors[0].message.contains("Bearing"),
        "error message should name the absent member or the child structure; got: {:?}",
        errors[0].message
    );

    // No scoped cell must be pushed for the absent member.
    if let Some(template) = find_template(&module.templates, "A") {
        let bogus_id = ValueCellId::new("A.b", "nope");
        assert!(
            !template.value_cells.iter().any(|c| c.id == bogus_id),
            "no cell should be pushed for genuinely-absent member `nope`"
        );
    }
}
