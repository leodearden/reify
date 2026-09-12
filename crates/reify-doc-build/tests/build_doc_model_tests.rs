//! Integration tests for `reify_doc_build::build_doc_model`.
//!
//! Each test compiles a small `.ri` source string via
//! `reify_test_support::compile_source_with_stdlib`, then calls
//! `reify_doc_build::build_doc_model(&compiled, source)` and asserts the
//! structure of the returned `DocModel`.

use reify_compiler::CompiledTypeAlias;
use reify_core::{ContentHash, ModulePath, SourceSpan};
use reify_doc::fmt_html::render_html;
use reify_doc::fmt_markdown::{render_markdown, MarkdownOptions, MarkdownOutput};
use reify_doc::model::{DocModel, ItemKind};
use reify_doc_build::build_doc_model;
use reify_test_support::{CompiledModuleBuilder, compile_source_with_stdlib};

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
    param width: Length = 10mm
    param height: Length = 20mm
    param depth: Length
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
    param size: Length = 10mm
    constraint size > 0mm
}

@test structure TestWidget {
    param size: Length = 5mm
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
    param val: Length
    val > 0mm
}

structure Labeled {
    param width: Length = 10mm
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
        ItemKind::Trait { members, .. } => {
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
        ItemKind::TypeAlias { type_repr, .. } => {
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
    param width: Length = 50mm
    param height: Length = 100mm
    constraint width > 0mm
    constraint height > 0mm
}

fn scale(x: Real) -> Real { x }

trait HasLength {
    param length: Length
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

// ---------------------------------------------------------------------------
// task #6342 step-1: parametric type-alias heading (Markdown, end-to-end)
// ---------------------------------------------------------------------------

/// A parametric `pub type Vel<Q: Dimension> = Q / Time` must render its type
/// params in the Markdown H2 heading — today they are silently dropped and the
/// alias documents as an indistinguishable `pub type Vel`.
///
/// The fixture line is copied verbatim from
/// `crates/reify-compiler/tests/fixtures/parametric_alias_def_site_ok.ri:11`,
/// which `parametric_alias_def_site_validation_tests::valid_pub_parametric_alias_accepted`
/// pins to ZERO Error diagnostics, so this test does not depend on any open
/// def-site grammar question.
///
/// Three invariants are pinned here:
///   (a) the heading gains `<Q: Dimension>` INSIDE the backtick code span;
///   (b) identity is unchanged — `header.name`, the `<a id="…">` anchor and the
///       TOC bullet all stay the bare `Vel`;
///   (c) a non-parametric alias emits NO `<>` at all.
#[test]
fn parametric_type_alias_renders_type_params_in_markdown_heading() {
    let source = r#"
pub type Vel<Q: Dimension> = Q / Time
type MyLength = Length
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in parametric alias source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // ── (b) identity is unchanged ─────────────────────────────────────────
    let item = find_item(module, "Vel");
    assert_eq!(
        item.header.name, "Vel",
        "header.name is the join key for anchors / TOC hrefs / split filenames \
         and must stay the bare identifier"
    );

    let md_str = match render_markdown(&model, None, &MarkdownOptions::default()) {
        MarkdownOutput::Single(s) => s,
        MarkdownOutput::Split(_) => panic!("MarkdownOptions::default() must yield single mode"),
    };

    // ── (a) RED: type params render in the heading, inside the code span ──
    assert!(
        md_str.contains("## `pub type Vel<Q: Dimension>` <a id=\"Vel\"></a>"),
        "parametric alias heading must render its type params inside the backtick \
         code span; got markdown:\n{md_str}"
    );

    // ── (b cont.) anchor and TOC bullet stay name-derived ─────────────────
    assert!(
        md_str.contains("<a id=\"Vel\"></a>"),
        "anchor must remain the bare name `Vel` with no angle brackets; got:\n{md_str}"
    );
    assert!(
        !md_str.contains("<a id=\"Vel<"),
        "anchor must NOT absorb the type params; got:\n{md_str}"
    );
    assert!(
        md_str.contains("- [`Vel`](#Vel)"),
        "TOC bullet must link the bare name `Vel`; got:\n{md_str}"
    );

    // ── (c) no-regression: empty type-param list emits no `<>` ────────────
    assert!(
        md_str.contains("## `type MyLength` <a id=\"MyLength\"></a>"),
        "non-parametric alias heading must be unchanged (no empty `<>`); got:\n{md_str}"
    );
}

// ---------------------------------------------------------------------------
// task #6342 step-3(a): parametric type-alias heading (HTML, end-to-end)
// ---------------------------------------------------------------------------

/// The HTML formatter must render the same type params as Markdown, but with
/// `<`/`>` HTML-escaped in the heading TEXT — and must keep them out of the
/// `id`/`href` attributes entirely.
///
/// Escaping is what makes the HTML side a distinct risk from Markdown: the
/// Markdown heading relies on a backtick code span to survive `<Q: …>`, while
/// HTML relies on `escape_into`.  A naive `out.push_str(&generics)` here would
/// emit a literal `<Q: Dimension>` that browsers parse as an unknown tag.
///
/// Uses the same zero-Error fixture as the Markdown test.
#[test]
fn parametric_type_alias_renders_escaped_type_params_in_html_heading() {
    let source = r#"
pub type Vel<Q: Dimension> = Q / Time
type MyLength = Length
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in parametric alias source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let html = render_html(&model, None);

    // ── heading text: params present and HTML-escaped ─────────────────────
    assert!(
        html.contains("<h2>pub type Vel&lt;Q: Dimension&gt;</h2>"),
        "parametric alias <h2> must carry HTML-escaped type params; got html:\n{html}"
    );

    // ── identity: section id and nav href stay the bare name ──────────────
    assert!(
        html.contains("<section id=\"Vel\">"),
        "section id must remain the bare name `Vel`; got html:\n{html}"
    );
    assert!(
        html.contains("<a href=\"#Vel\">Vel</a>"),
        "nav entry must link and label the bare name `Vel`; got html:\n{html}"
    );

    // ── the raw, unescaped form must never appear anywhere ────────────────
    assert!(
        !html.contains("Vel<Q"),
        "type params must never be emitted unescaped (browsers would parse \
         `<Q: Dimension>` as a tag); got html:\n{html}"
    );

    // ── no-regression: non-parametric alias emits no empty `<>` ───────────
    assert!(
        html.contains("<h2>type MyLength</h2>"),
        "non-parametric alias <h2> must be unchanged (no empty `<>`); got html:\n{html}"
    );
}

// ---------------------------------------------------------------------------
// task #6342 step-7: generic function signature
// ---------------------------------------------------------------------------

/// A generic `pub fn pick<T>(...)` must carry its generic segment in the
/// rendered `ItemKind::Function { signature }`.
///
/// Unlike `TypeAlias`, this needs no model change: `signature` is already a
/// single rendered DISPLAY string, so the generics fold into it and
/// `header.name` stays the bare identity `pick`.
///
/// Fixture shaped after the proven stdlib forms `pub fn unwrap_or<T, E>(...)`
/// (`crates/reify-compiler/stdlib/result.ri:52`) and
/// `pub fn clamp_field<D, Q: Dimension>(...)` (`stdlib/fields.ri:162`).
#[test]
fn generic_function_renders_type_params_in_signature() {
    let source = r#"
pub fn pick<T>(a: T, b: T) -> T { a }
fn scale(x: Real) -> Real { x }
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in generic fn source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // ── generic fn: signature carries `<T>` between name and param list ───
    let pick = find_item(module, "pick");
    assert_eq!(
        pick.header.name, "pick",
        "header.name must stay the bare identity `pick`"
    );
    match &pick.kind {
        ItemKind::Function { signature } => {
            assert!(
                signature.starts_with("fn pick<T>("),
                "generic fn signature must splice `<T>` between the name and the \
                 parameter list; got {signature:?}"
            );
        }
        other => panic!("expected ItemKind::Function for 'pick', got {other:?}"),
    }

    // ── no-regression: non-generic fn signature is unchanged, no empty `<>` ─
    let scale = find_item(module, "scale");
    match &scale.kind {
        ItemKind::Function { signature } => {
            assert!(
                signature.starts_with("fn scale("),
                "non-generic fn signature must be unchanged (no empty `<>`); \
                 got {signature:?}"
            );
        }
        other => panic!("expected ItemKind::Function for 'scale', got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// task #6342 step-9: generic trait heading
// ---------------------------------------------------------------------------

/// A generic `pub trait Holder<T: Rigid>` must render its type params in both
/// the Markdown and HTML headings — today `lower_trait` never reads
/// `CompiledTrait.type_params`, so it documents as an indistinguishable
/// `pub trait Holder`.
///
/// Fixture shaped after `trait Container<T: Rigid>`
/// (`crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:13`).
/// The non-generic control is declared in the same source rather than reusing
/// an existing fixture, so the two headings are compared under identical
/// compilation conditions.
#[test]
fn generic_trait_renders_type_params_in_headings() {
    let source = r#"
pub trait Holder<T: Rigid> { param count : Int }
pub trait Plain { param value : Real }
"#;
    let compiled = compile_source_with_stdlib(source);
    let diag_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        diag_errors.is_empty(),
        "compilation errors in generic trait source: {:?}",
        diag_errors
    );

    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // ── (c) identity unchanged ────────────────────────────────────────────
    let holder = find_item(module, "Holder");
    assert_eq!(
        holder.header.name, "Holder",
        "header.name must stay the bare identity `Holder`"
    );

    // ── (a) Markdown heading ──────────────────────────────────────────────
    let md_str = match render_markdown(&model, None, &MarkdownOptions::default()) {
        MarkdownOutput::Single(s) => s,
        MarkdownOutput::Split(_) => panic!("MarkdownOptions::default() must yield single mode"),
    };
    assert!(
        md_str.contains("## `pub trait Holder<T: Rigid>` <a id=\"Holder\"></a>"),
        "generic trait heading must render its type params inside the backtick \
         code span; got markdown:\n{md_str}"
    );

    // ── (b) HTML heading, escaped; section id stays the bare name ─────────
    let html = render_html(&model, None);
    assert!(
        html.contains("<h2>pub trait Holder&lt;T: Rigid&gt;</h2>"),
        "generic trait <h2> must carry HTML-escaped type params; got html:\n{html}"
    );
    assert!(
        html.contains("<section id=\"Holder\">"),
        "section id must remain the bare name `Holder`; got html:\n{html}"
    );

    // ── (d) no-regression: non-generic trait emits no empty `<>` ──────────
    assert!(
        md_str.contains("## `pub trait Plain` <a id=\"Plain\"></a>"),
        "non-generic trait heading must be unchanged (no empty `<>`); got markdown:\n{md_str}"
    );
    assert!(
        html.contains("<h2>pub trait Plain</h2>"),
        "non-generic trait <h2> must be unchanged (no empty `<>`); got html:\n{html}"
    );
}

// ---------------------------------------------------------------------------
// task-6321: type alias renders its type_expr when resolved_type is None
// ---------------------------------------------------------------------------

/// When a `CompiledTypeAlias` exports `resolved_type: None` (a parametric
/// body, an entity-named body, or any other unresolvable RHS), `lower_type_alias`
/// must render the alias's carried `type_expr` (via `impl Display for TypeExpr`)
/// rather than the `"<parameterized>"` sentinel. That sentinel is a lie for an
/// entity-named alias like `type F = Fit` and uninformative even for a
/// genuinely parametric alias like `Container<T> = T`.
///
/// Expected strings are read directly off `impl Display for TypeExpr`
/// (`impl fmt::Display for TypeExpr` in `reify-ast/src/ast.rs`, the
/// `TypeExprKind::Named` arm): a bare `Named { name, type_args: [] }`
/// renders as `name`; a `Named` with args renders `name<arg1, arg2>`.
#[test]
fn alias_with_unresolved_body_renders_its_type_expr() {
    let source = r#"
enum Fit { Close, Medium }
pub type Container<T> = T
pub type F = Fit
pub type Fits = List<Fit>
pub type MyLength = Length
"#;
    let compiled = compile_source_with_stdlib(source);
    let model: DocModel = build_doc_model(&compiled, source);
    let module = &model.modules[0];

    // Parametric alias: body is the bare type param `T`.
    let container_item = find_item(module, "Container");
    match &container_item.kind {
        ItemKind::TypeAlias { type_repr, .. } => {
            assert_eq!(
                type_repr, "T",
                "Container's type_repr must be the type_expr Display 'T', not the \
                 '<parameterized>' sentinel; got {type_repr:?}"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'Container', got {other:?}"),
    }

    // Entity-named alias: body names the local enum `Fit`.
    let f_item = find_item(module, "F");
    match &f_item.kind {
        ItemKind::TypeAlias { type_repr, .. } => {
            assert_eq!(
                type_repr, "Fit",
                "F's type_repr must be the type_expr Display 'Fit', not \
                 '<parameterized>'; got {type_repr:?}"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'F', got {other:?}"),
    }

    // Composite body: List<Fit>.
    let fits_item = find_item(module, "Fits");
    match &fits_item.kind {
        ItemKind::TypeAlias { type_repr, .. } => {
            assert_eq!(
                type_repr, "List<Fit>",
                "Fits's type_repr must be the type_expr Display 'List<Fit>'; got {type_repr:?}"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'Fits', got {other:?}"),
    }

    // Resolvable alias: unchanged path (Some(resolved_type) → type_to_string).
    // Do NOT pin the exact spelling: `reify_core::Type`'s Display is a
    // canonicalising, non-source-syntax rendering, not the source text "Length".
    let my_length_item = find_item(module, "MyLength");
    match &my_length_item.kind {
        ItemKind::TypeAlias { type_repr, .. } => {
            assert!(
                !type_repr.is_empty() && type_repr != "<parameterized>",
                "MyLength type_repr must be non-empty and not the sentinel; got {type_repr:?}"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'MyLength', got {other:?}"),
    }
}

/// Covers the `(None, None)` arm of `lower_type_alias`'s match: a
/// `CompiledTypeAlias` with neither a resolved type nor a carried body.
///
/// This state is unreachable from source — both user-alias construction
/// sites in `type_resolution.rs` set `type_expr: Some(..)` unconditionally —
/// so the fixture is built synthetically via `CompiledModuleBuilder`.
///
/// A `None`/`None` alias says nothing about whether it is parametric, so the
/// old `"<parameterized>"` sentinel is actively misleading here; the doc
/// surface must render `"<unresolved>"` instead.
#[test]
fn alias_with_no_body_renders_unresolved_not_parameterized() {
    let alias = CompiledTypeAlias {
        name: "Bodyless".to_string(),
        resolved_type: None,
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Bodyless"),
    };
    let compiled = CompiledModuleBuilder::new(ModulePath::single("test"))
        .type_alias(alias)
        .build();

    // "" as source deliberately mirrors the `build_doc_model(compiled, "")`
    // call inside `reify_doc_build::build_stdlib_doc_model`: proves the
    // renderer never depends on source text.
    let model: DocModel = build_doc_model(&compiled, "");
    let module = &model.modules[0];

    let item = find_item(module, "Bodyless");
    match &item.kind {
        ItemKind::TypeAlias { type_repr, .. } => {
            assert!(
                !type_repr.is_empty(),
                "Bodyless type_repr must be non-empty"
            );
            assert_eq!(
                type_repr, "<unresolved>",
                "an alias with neither resolved_type nor type_expr must render \
                 '<unresolved>', not the misleading '<parameterized>' sentinel; \
                 got {type_repr:?}"
            );
        }
        other => panic!("expected ItemKind::TypeAlias for 'Bodyless', got {other:?}"),
    }
}
