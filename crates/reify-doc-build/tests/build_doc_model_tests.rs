//! Integration tests for `reify_doc_build::build_doc_model`.
//!
//! Each test compiles a small `.ri` source string via
//! `reify_test_support::compile_source_with_stdlib`, then calls
//! `reify_doc_build::build_doc_model(&compiled, source)` and asserts the
//! structure of the returned `DocModel`.

use reify_doc::model::{DocModel, ItemKind};
use reify_doc_build::build_doc_model;
use reify_test_support::compile_source_with_stdlib;

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

/// Find an item by name in the first module or panic with a diagnostic message.
fn find_item<'m>(module: &'m reify_doc::model::ModuleDoc, name: &str) -> &'m reify_doc::model::ItemDoc {
    module
        .items
        .iter()
        .find(|i| i.header.name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = module.items.iter().map(|i| i.header.name.as_str()).collect();
            panic!("item '{name}' not found in module; present items: {names:?}")
        })
}

// ---------------------------------------------------------------------------
// step-1: structure with params and constraints
// ---------------------------------------------------------------------------

/// Compile a small source with one `pub structure` that has params (with defaults)
/// and named constraints.  Asserts that `build_doc_model` returns a
/// `DocModel` with the correct module path and the expected `ItemKind::Structure`
/// payload.
#[test]
fn structure_with_params_and_constraints() {
    let source = r#"
pub structure Widget {
    param width: Scalar = 10mm
    param height: Scalar = 20mm
    param depth: Scalar
    constraint depth > 0mm
    constraint width >= height
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let model: DocModel = build_doc_model(&compiled, source);

    // There should be exactly one ModuleDoc.
    assert_eq!(model.modules.len(), 1, "expected one module");
    let module = &model.modules[0];

    // Module path should match the compiled module's path (empty for unnamed source).
    assert_eq!(module.path, compiled.path.to_string());

    // There should be exactly one item: Widget.
    assert_eq!(module.items.len(), 1, "expected one item");
    let item = &module.items[0];

    // Header fields.
    assert_eq!(item.header.name, "Widget");
    assert!(item.header.is_pub, "Widget is pub");

    // It should be a Structure.
    let (params, constraints) = match &item.kind {
        ItemKind::Structure {
            params,
            constraints,
            ..
        } => (params, constraints),
        other => panic!("expected Structure, got {other:?}"),
    };

    // Params: width, height, depth (in source order).
    assert_eq!(params.len(), 3, "expected 3 params; got {params:?}");
    assert_eq!(params[0].name, "width");
    assert!(
        params[0].default_repr.is_some(),
        "width has a default (10mm)"
    );
    assert_eq!(params[1].name, "height");
    assert!(
        params[1].default_repr.is_some(),
        "height has a default (20mm)"
    );
    assert_eq!(params[2].name, "depth");
    assert!(
        params[2].default_repr.is_none(),
        "depth has no default"
    );

    // Constraints: two entries.
    assert_eq!(constraints.len(), 2, "expected 2 constraints; got {constraints:?}");

    // constraint 0: expr_repr is the span-sliced text of "depth > 0mm"
    assert!(
        constraints[0].expr_repr.contains("depth"),
        "first constraint expr_repr must mention 'depth', got: {:?}",
        constraints[0].expr_repr
    );
    // constraint 1: expr_repr is "width >= height"
    assert!(
        constraints[1].expr_repr.contains("width"),
        "second constraint expr_repr must mention 'width', got: {:?}",
        constraints[1].expr_repr
    );

    // line numbers: both constraints must have Some(line) and be >= 1.
    for (i, c) in constraints.iter().enumerate() {
        assert!(
            c.line.is_some(),
            "constraint[{i}].line must be Some, got None"
        );
        assert!(
            c.line.unwrap() >= 1,
            "constraint[{i}].line must be >= 1, got {:?}",
            c.line
        );
    }
}

// ---------------------------------------------------------------------------
// step-3: remaining top-level surfaces
// ---------------------------------------------------------------------------
//
// A single multi-declaration source exercises every remaining `ItemKind`
// variant.  Each assertion checks the item header + kind payload.
//
// Implementation note: the step-2 WIP commit pre-implemented all surface
// lowerings in build.rs, so these tests are GREEN at the time they are
// written rather than the expected RED.  The test coverage is still valid;
// step-4 requires no additional implementation.

/// Diagnostic: list items produced from the multi-kind source. Remove after debugging.
#[test]
fn debug_list_items_from_multi_kind_source() {
    let source = r#"
fn scale(x: Real) -> Real { x }

trait HasValue {
    param value: Real
}

field def temp_field : Real -> Real {
    source = analytical { |x| x }
}

purpose no_op(subject: Structure) {
    constraint 1 > 0
}

enum Color { Red, Green, Blue }

unit cubits : Length = 0.4572

type MyLength = Length

constraint def non_negative {
    param val: Real
    val >= 0.0
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_types::Severity::Error))
        .collect();
    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];
    let names: Vec<_> = module
        .items
        .iter()
        .map(|i| {
            let kind_tag = match &i.kind {
                ItemKind::Structure { .. } => "structure",
                ItemKind::Occurrence { .. } => "occurrence",
                ItemKind::Trait { .. } => "trait",
                ItemKind::Function { .. } => "function",
                ItemKind::Field { .. } => "field",
                ItemKind::Purpose { .. } => "purpose",
                ItemKind::Enum { .. } => "enum",
                ItemKind::Unit { .. } => "unit",
                ItemKind::TypeAlias { .. } => "type_alias",
                ItemKind::ConstraintDef { .. } => "constraint_def",
            };
            format!("{} ({})", i.header.name, kind_tag)
        })
        .collect();
    let msg = format!(
        "DIAGNOSTIC: errors={:?}; items ({})={:?}",
        diag_errors.iter().map(|d| &d.message).collect::<Vec<_>>(),
        names.len(),
        names
    );
    std::fs::write("/tmp/reify_doc_build_diag.txt", &msg).ok();
    panic!("{}", msg);
}

/// All remaining declaration kinds in one source: fn, trait, field def,
/// purpose, enum, unit, type alias, and named constraint def.
#[test]
fn all_remaining_top_level_surfaces() {
    let source = r#"
fn scale(x: Real) -> Real { x }

trait HasValue {
    param value: Real
}

field def temp_field : Real -> Real {
    source = analytical { |x| x }
}

purpose no_op(subject: Structure) {
    constraint 1 > 0
}

enum Color { Red, Green, Blue }

unit cubits : Length = 0.4572

type MyLength = Length

constraint def non_negative {
    param val: Real
    val >= 0.0
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let model: DocModel = build_doc_model(&compiled, source);

    assert_eq!(model.modules.len(), 1, "expected one module");
    let module = &model.modules[0];

    // ── Function ──────────────────────────────────────────────────────────
    let fn_item = find_item(module, "scale");
    assert!(!fn_item.header.is_pub, "scale is not pub");
    match &fn_item.kind {
        ItemKind::Function { signature } => {
            assert!(
                signature.starts_with("fn "),
                "signature must start with 'fn ': {signature:?}"
            );
            assert!(
                signature.contains("scale"),
                "signature must contain the fn name 'scale': {signature:?}"
            );
            assert!(
                signature.contains("x"),
                "signature must mention parameter 'x': {signature:?}"
            );
        }
        other => panic!("expected ItemKind::Function for 'scale', got {other:?}"),
    }

    // ── Trait ─────────────────────────────────────────────────────────────
    let trait_item = find_item(module, "HasValue");
    assert!(!trait_item.header.is_pub, "HasValue is not pub");
    match &trait_item.kind {
        ItemKind::Trait { members } => {
            assert_eq!(members.len(), 1, "HasValue has 1 member; got {members:?}");
            assert!(
                members[0].contains("value"),
                "trait member must mention 'value': {:?}",
                members[0]
            );
        }
        other => panic!("expected ItemKind::Trait for 'HasValue', got {other:?}"),
    }

    // ── Field def ─────────────────────────────────────────────────────────
    // `field def name : Domain -> Codomain` compiles to ItemKind::Field.
    let field_item = find_item(module, "temp_field");
    match &field_item.kind {
        ItemKind::Field {
            type_repr,
            default_repr,
        } => {
            assert!(
                !type_repr.is_empty(),
                "temp_field type_repr must be non-empty"
            );
            // Field defs compiled from `field def` declarations have no default value.
            assert!(
                default_repr.is_none(),
                "temp_field field def has no default_repr; got {default_repr:?}"
            );
        }
        other => panic!("expected ItemKind::Field for 'temp_field', got {other:?}"),
    }

    // ── Purpose ───────────────────────────────────────────────────────────
    // When objective is None (no explicit `minimize`/`maximize`), the lowering
    // falls back to direction="minimize" and expr_repr from the first constraint.
    let purpose_item = find_item(module, "no_op");
    match &purpose_item.kind {
        ItemKind::Purpose { direction, .. } => {
            assert_eq!(
                direction, "minimize",
                "no_op purpose direction must be 'minimize'; got {direction:?}"
            );
        }
        other => panic!("expected ItemKind::Purpose for 'no_op', got {other:?}"),
    }

    // ── Enum ──────────────────────────────────────────────────────────────
    let enum_item = find_item(module, "Color");
    match &enum_item.kind {
        ItemKind::Enum { variants } => {
            assert_eq!(variants.len(), 3, "Color has 3 variants; got {variants:?}");
            assert_eq!(variants[0], "Red");
            assert_eq!(variants[1], "Green");
            assert_eq!(variants[2], "Blue");
        }
        other => panic!("expected ItemKind::Enum for 'Color', got {other:?}"),
    }

    // ── Unit ──────────────────────────────────────────────────────────────
    // `unit cubits : Length = 0.4572`: dimension=LENGTH → displays as "m";
    // factor 0.0000254 → some decimal string.
    let unit_item = find_item(module, "cubits");
    match &unit_item.kind {
        ItemKind::Unit { base_unit, scale } => {
            assert!(!base_unit.is_empty(), "cubits base_unit must be non-empty");
            assert!(!scale.is_empty(), "cubits scale must be non-empty");
        }
        other => panic!("expected ItemKind::Unit for 'cubits', got {other:?}"),
    }

    // ── TypeAlias ─────────────────────────────────────────────────────────
    let alias_item = find_item(module, "MyLength");
    match &alias_item.kind {
        ItemKind::TypeAlias { type_repr } => {
            assert!(
                !type_repr.is_empty(),
                "MyLength type_repr must be non-empty"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'MyLength', got {other:?}"),
    }

    // ── ConstraintDef ─────────────────────────────────────────────────────
    let cd_item = find_item(module, "non_negative");
    match &cd_item.kind {
        ItemKind::ConstraintDef { expr_repr } => {
            assert!(
                !expr_repr.is_empty(),
                "non_negative expr_repr must be non-empty"
            );
        }
        other => panic!("expected ItemKind::ConstraintDef for 'non_negative', got {other:?}"),
    }
}
