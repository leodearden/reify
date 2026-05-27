//! Integration tests for `reify_doc_build::build_doc_model`.
//!
//! Each test compiles a small `.ri` source string via
//! `reify_test_support::compile_source_with_stdlib`, then calls
//! `reify_doc_build::build_doc_model(&compiled, source)` and asserts the
//! structure of the returned `DocModel`.

use reify_doc::fmt_html::render_html;
use reify_doc::fmt_markdown::{render_markdown, MarkdownOptions, MarkdownOutput};
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
    {
        let default = params[0]
            .default_repr
            .as_deref()
            .expect("width has a default (10mm); default_repr must be Some");
        assert!(
            default.contains("10mm"),
            "width default_repr must contain the actual default value '10mm', \
             not the full declaration; got: {default:?}"
        );
        assert!(
            !default.contains("param"),
            "width default_repr must NOT contain 'param' (should be the RHS value, \
             not the full declaration text); got: {default:?}"
        );
    }
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

// ---------------------------------------------------------------------------
// step-5: annotations and pragmas
// ---------------------------------------------------------------------------
//
// Tests that:
//  - Module-level pragmas are lowered into ModuleDoc.pragmas.
//  - @deprecated / @test annotations on structures are preserved in
//    ItemHeader.annotations (name + rendered args).
//  - Block-level (#solver) pragmas on a structure body land in
//    ItemHeader.pragmas.
//  - Constraint labels (from constraint-def instantiation) are preserved in
//    ConstraintDoc.label.
//
// Note: param-level annotations (e.g., @solver_hint) are consumed/validated
// during compilation and are NOT persisted on ValueCellDecl.  Accordingly,
// ParamDoc.annotations is always empty; this is a known limitation documented
// in build.rs and is NOT asserted here.
//
// Implementation note: the step-2 WIP commit pre-implemented annotation/pragma
// lowering helpers in build.rs, so these tests are GREEN immediately.
// step-6 requires no additional implementation.

/// Module-level pragmas, item-level annotations, and item-level pragmas are
/// all lowered correctly.
#[test]
fn annotations_and_pragmas_lowering() {
    // #version(0.1) is a known module-level pragma (no warnings produced).
    // #solver(backend="ipopt") is a known block-level pragma on a structure.
    // @deprecated("...") and @test are item-level annotations.
    let source = r#"
#version(0.1)

@deprecated("use NewWidget instead")
structure OldWidget {
    #solver(backend="ipopt")
    param size: Scalar = 10mm
    constraint size > 0mm
}

@test structure TestWidget {
    param size: Scalar = 5mm
    constraint size > 0mm
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in annotation test source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    assert_eq!(model.modules.len(), 1, "expected one module");
    let module = &model.modules[0];

    // ── Module-level pragma ───────────────────────────────────────────────────
    // #version(0.1) should appear in ModuleDoc.pragmas.
    let version_pragma = module.pragmas.iter().find(|p| p.name == "version");
    assert!(
        version_pragma.is_some(),
        "expected 'version' pragma in module.pragmas; got: {:?}",
        module.pragmas
    );
    let version = version_pragma.unwrap();
    assert_eq!(
        version.args.len(),
        1,
        "#version should have 1 arg; got: {:?}",
        version.args
    );

    // ── @deprecated annotation on OldWidget ──────────────────────────────────
    let old_widget = find_item(module, "OldWidget");
    let deprecated_ann = old_widget
        .header
        .annotations
        .iter()
        .find(|a| a.name == "deprecated");
    assert!(
        deprecated_ann.is_some(),
        "OldWidget should carry @deprecated annotation; got annotations: {:?}",
        old_widget.header.annotations
    );
    let dep = deprecated_ann.unwrap();
    assert_eq!(
        dep.args.len(),
        1,
        "@deprecated should have 1 arg string; got: {:?}",
        dep.args
    );
    assert!(
        dep.args[0].contains("NewWidget"),
        "@deprecated arg must mention 'NewWidget'; got: {:?}",
        dep.args[0]
    );

    // ── Block-level (#solver) pragma on OldWidget ─────────────────────────────
    let solver_pragma = old_widget
        .header
        .pragmas
        .iter()
        .find(|p| p.name == "solver");
    assert!(
        solver_pragma.is_some(),
        "OldWidget should carry #solver pragma in header.pragmas; got: {:?}",
        old_widget.header.pragmas
    );
    let solver = solver_pragma.unwrap();
    assert!(
        !solver.args.is_empty(),
        "#solver pragma should have at least one arg; got: {:?}",
        solver.args
    );
    // Rendered as "backend=\"ipopt\"" (KeyValue form).
    assert!(
        solver.args[0].contains("backend"),
        "#solver arg should contain 'backend'; got: {:?}",
        solver.args[0]
    );

    // ── @test annotation on TestWidget ───────────────────────────────────────
    // The @test annotation must be preserved so the formatters' Tests-section
    // partitioning logic (which looks at header.annotations) still works.
    let test_widget = find_item(module, "TestWidget");
    let test_ann = test_widget
        .header
        .annotations
        .iter()
        .find(|a| a.name == "test");
    assert!(
        test_ann.is_some(),
        "TestWidget must carry @test annotation for formatter Tests-section partitioning; got: {:?}",
        test_widget.header.annotations
    );
}

/// Constraint labels from constraint-def instantiation are preserved in
/// ConstraintDoc.label.
#[test]
fn constraint_label_from_instantiation() {
    let source = r#"
constraint def Positive {
    param val: Scalar
    val > 0mm
}

structure Labeled {
    param width: Scalar = 10mm
    constraint Positive(val: width)
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in constraint label test: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // Labeled has one constraint whose label is set by the instantiation.
    let labeled = find_item(module, "Labeled");
    let (_, constraints) = match &labeled.kind {
        ItemKind::Structure {
            params,
            constraints,
            ..
        } => (params, constraints),
        other => panic!("expected Structure for 'Labeled', got {other:?}"),
    };
    assert!(
        !constraints.is_empty(),
        "Labeled must have at least one constraint; got none"
    );
    // The constraint instantiation produces a label like "Positive#0[0]".
    let c = &constraints[0];
    assert!(
        c.label.is_some(),
        "constraint from instantiation must have a label; got label: {:?}",
        c.label
    );
    let label_str = c.label.as_ref().unwrap();
    assert!(
        label_str.contains("Positive"),
        "constraint label should mention the constraint def name 'Positive'; got: {label_str:?}"
    );
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

// ---------------------------------------------------------------------------
// Amendment: purpose with explicit minimize / maximize objective
// ---------------------------------------------------------------------------

/// Verify that `lower_purpose` renders explicit minimize/maximize objectives
/// as clean placeholders rather than Rust Debug AST output.
///
/// `CompiledExpr` has no source span, so we cannot span-slice the objective
/// expression.  The lowering emits "<minimize>" / "<maximize>" instead of
/// `format!("{expr:?}")` which would produce unreadable internal AST text.
#[test]
fn purpose_with_explicit_objective() {
    let source = r#"
purpose with_minimize(subject: Structure) {
    minimize 1.0
}

purpose with_maximize(subject: Structure) {
    maximize 1.0
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in purpose minimize/maximize source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // ── minimize purpose ──────────────────────────────────────────────────
    let min_item = find_item(module, "with_minimize");
    match &min_item.kind {
        ItemKind::Purpose { direction, expr_repr } => {
            assert_eq!(direction, "minimize", "direction must be 'minimize'");
            // Must NOT contain Rust internal Debug output.
            assert!(
                !expr_repr.contains("CompiledExpr"),
                "minimize expr_repr must not contain Rust Debug output 'CompiledExpr'; \
                 got: {expr_repr:?}"
            );
            assert!(
                !expr_repr.contains("BinOp"),
                "minimize expr_repr must not contain Rust Debug output 'BinOp'; \
                 got: {expr_repr:?}"
            );
            // Should be the clean placeholder.
            assert_eq!(
                expr_repr, "<minimize>",
                "minimize expr_repr must be '<minimize>' placeholder; got: {expr_repr:?}"
            );
        }
        other => panic!("expected ItemKind::Purpose for 'with_minimize', got {other:?}"),
    }

    // ── maximize purpose ──────────────────────────────────────────────────
    let max_item = find_item(module, "with_maximize");
    match &max_item.kind {
        ItemKind::Purpose { direction, expr_repr } => {
            assert_eq!(direction, "maximize", "direction must be 'maximize'");
            assert!(
                !expr_repr.contains("CompiledExpr"),
                "maximize expr_repr must not contain Rust Debug output; got: {expr_repr:?}"
            );
            assert_eq!(
                expr_repr, "<maximize>",
                "maximize expr_repr must be '<maximize>' placeholder; got: {expr_repr:?}"
            );
        }
        other => panic!("expected ItemKind::Purpose for 'with_maximize', got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// step-7: serde round-trip and render integration test
// ---------------------------------------------------------------------------

/// Build a DocModel from a multi-item source, assert it serde round-trips,
/// and assert both HTML and Markdown renders are non-empty and contain the
/// declared item names.
#[test]
fn doc_model_serde_roundtrip_and_render() {
    let source = r#"
pub structure Bracket {
    param width: Scalar = 50mm
    param height: Scalar = 100mm
    constraint width > 0mm
    constraint height > 0mm
}

fn scale(x: Real) -> Real { x }

trait HasLength {
    param length: Scalar
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in serde/render test: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    assert_eq!(model.modules.len(), 1, "expected one module");

    // ── (a) Serde round-trip ─────────────────────────────────────────────────
    let json_str = serde_json::to_string(&model).expect("model must serialize to JSON");
    assert!(!json_str.is_empty(), "serialized JSON must be non-empty");

    let model2: DocModel =
        serde_json::from_str(&json_str).expect("model must deserialize from JSON");

    // Round-trip equality: re-serialize and compare JSON strings (avoids
    // needing PartialEq on DocModel while still catching structural diffs).
    let json_str2 = serde_json::to_string(&model2).expect("round-tripped model must re-serialize");
    assert_eq!(
        json_str, json_str2,
        "serde round-trip must be lossless (to_string→from_str→to_string equality)"
    );

    // ── (b) HTML render ──────────────────────────────────────────────────────
    let html = render_html(&model, None);
    assert!(!html.is_empty(), "render_html output must be non-empty");
    assert!(
        html.contains("Bracket"),
        "HTML must contain item name 'Bracket'; snippet: {:?}",
        &html[..html.len().min(500)]
    );
    assert!(
        html.contains("scale"),
        "HTML must contain item name 'scale'; snippet: {:?}",
        &html[..html.len().min(500)]
    );
    assert!(
        html.contains("HasLength"),
        "HTML must contain item name 'HasLength'; snippet: {:?}",
        &html[..html.len().min(500)]
    );

    // ── (c) Markdown render ──────────────────────────────────────────────────
    let md_out = render_markdown(&model, None, &MarkdownOptions::default());
    let md_str = match md_out {
        MarkdownOutput::Single(s) => s,
        MarkdownOutput::Split(parts) => parts
            .into_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    assert!(!md_str.is_empty(), "render_markdown output must be non-empty");
    assert!(
        md_str.contains("Bracket"),
        "Markdown must contain item name 'Bracket'; snippet: {:?}",
        &md_str[..md_str.len().min(500)]
    );
    assert!(
        md_str.contains("scale"),
        "Markdown must contain item name 'scale'; snippet: {:?}",
        &md_str[..md_str.len().min(500)]
    );
    assert!(
        md_str.contains("HasLength"),
        "Markdown must contain item name 'HasLength'; snippet: {:?}",
        &md_str[..md_str.len().min(500)]
    );
}
