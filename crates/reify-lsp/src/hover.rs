use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Url};

use crate::analysis::{AnalysisContext, format_value};
use crate::convert::{find_word_at_offset, position_to_offset};

/// Compute hover information for the symbol at the given position.
///
/// Returns `None` if there is nothing to show at the given position.
pub fn compute_hover(source: &str, uri: &Url, position: Position) -> Option<Hover> {
    let offset = position_to_offset(source, position);
    let (_word_start, word) = find_word_at_offset(source, offset)?;

    let ctx = AnalysisContext::new(source, uri);

    // Try member lookup first
    if let Some(info) = ctx.find_member_decl(word) {
        let kind_str = match info.kind {
            reify_compiler::ValueCellKind::Param => "param",
            reify_compiler::ValueCellKind::Let => "let",
            reify_compiler::ValueCellKind::Auto => "auto",
        };
        let type_str = info.cell_type.to_string();

        // Try to get the evaluated value
        let value_str = ctx
            .compiled
            .templates
            .first()
            .and_then(|t| ctx.get_value(&t.name, word))
            .map(|v| format!(" = {}", format_value(v)));

        let mut md = format!(
            "```reify\n{kind_str} {word}: {type_str}{}\n```",
            value_str.unwrap_or_default()
        );
        if let Some(doc) = info.doc {
            md.push_str("\n\n");
            md.push_str(doc);
        }

        return Some(make_hover_markdown(md));
    }

    // Try structure name
    for (name, params, lets, constraints) in ctx.structure_names() {
        if name == word {
            let mut md = format!(
                "```reify\nstructure {name}\n```\n\n{params} params, {lets} lets, {constraints} constraints"
            );
            if let Some(doc) = ctx.find_entity_doc(name) {
                md.push_str("\n\n");
                md.push_str(doc);
            }
            return Some(make_hover_markdown(md));
        }
    }

    // Try fn/trait/enum names
    for decl in &ctx.parsed.declarations {
        match decl {
            reify_syntax::Declaration::Function(f) if f.name == word => {
                let params_str: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_expr.name))
                    .collect();
                let ret = f
                    .return_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t.name))
                    .unwrap_or_default();
                let mut md = format!(
                    "```reify\nfn {}({}){}\n```",
                    f.name,
                    params_str.join(", "),
                    ret
                );
                if let Some(doc) = ctx.find_entity_doc(word) {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                return Some(make_hover_markdown(md));
            }
            reify_syntax::Declaration::Trait(t) if t.name == word => {
                let mut md = format!("```reify\ntrait {}\n```", t.name);
                if !t.refinements.is_empty() {
                    md = format!(
                        "```reify\ntrait {} : {}\n```",
                        t.name,
                        t.refinements.join(" + ")
                    );
                }
                if let Some(doc) = ctx.find_entity_doc(word) {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                return Some(make_hover_markdown(md));
            }
            reify_syntax::Declaration::Enum(e) if e.name == word => {
                let mut md = format!("```reify\nenum {}\n```", e.name);
                md.push_str(&format!("\n\n{} variants", e.variants.len()));
                if let Some(doc) = ctx.find_entity_doc(word) {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                return Some(make_hover_markdown(md));
            }
            _ => {}
        }
    }

    // Try keyword
    if let Some(desc) = keyword_description(word) {
        let md = format!("**{word}** — {desc}");
        return Some(make_hover_markdown(md));
    }

    None
}

/// Create a Hover with markdown content.
fn make_hover_markdown(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

/// Return a brief description for Reify keywords.
fn keyword_description(word: &str) -> Option<&'static str> {
    match word {
        "structure" => Some("Declares a parametric structure."),
        "param" => Some("Declares an externally settable parameter with a type and default value."),
        "let" => Some("Declares a computed binding derived from other values."),
        "constraint" => Some("Declares a boolean constraint that must be satisfied."),
        "sub" => Some("Declares a sub-structure instantiation."),
        "import" => Some("Imports declarations from another module."),
        "if" => Some("Conditional expression."),
        "then" => Some("Then branch of a conditional."),
        "else" => Some("Else branch of a conditional."),
        "and" => Some("Logical AND operator."),
        "or" => Some("Logical OR operator."),
        "not" => Some("Logical NOT operator."),
        "true" => Some("Boolean literal true."),
        "false" => Some("Boolean literal false."),
        "auto" => Some("Marks a parameter for automatic resolution by the constraint solver."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Helper: compute hover and extract the markdown text from the result.
    fn hover_markdown(source: &str, position: Position) -> Option<String> {
        let hover = compute_hover(source, &test_uri(), position)?;
        match hover.contents {
            tower_lsp::lsp_types::HoverContents::Markup(markup) => Some(markup.value),
            _ => None,
        }
    }

    // --- step-1: hover on param declarations ---

    #[test]
    fn hover_on_width_param_shows_type_info() {
        let source = reify_test_support::bracket_source();
        // 'width' starts at byte 30 in "param width: Scalar = 80mm"
        // Line 1, char ~6 (after 4-space indent + 'param ')
        let position = Position::new(1, 10); // on 'width'
        let md = hover_markdown(source, position).expect("hover should return info for width");
        assert!(md.contains("param"), "should mention 'param', got: {md}");
        assert!(md.contains("width"), "should mention 'width', got: {md}");
        assert!(
            md.contains("Scalar"),
            "should mention type Scalar, got: {md}"
        );
    }

    #[test]
    fn hover_on_thickness_param_shows_type_info() {
        let source = reify_test_support::bracket_source();
        // 'thickness' is on line 3 (0-indexed)
        let position = Position::new(3, 10); // on 'thickness'
        let md = hover_markdown(source, position).expect("hover should return info for thickness");
        assert!(md.contains("param"), "should mention 'param', got: {md}");
        assert!(
            md.contains("thickness"),
            "should mention 'thickness', got: {md}"
        );
        assert!(
            md.contains("Scalar"),
            "should mention type Scalar, got: {md}"
        );
    }

    // --- step-3: hover on let binding and ident references ---

    #[test]
    fn hover_on_volume_let_shows_let_info() {
        let source = reify_test_support::bracket_source();
        // 'volume' is on line 7: "    let volume = width * height * thickness"
        let position = Position::new(7, 8); // on 'volume'
        let md = hover_markdown(source, position).expect("hover should return info for volume");
        assert!(md.contains("let"), "should mention 'let', got: {md}");
        assert!(md.contains("volume"), "should mention 'volume', got: {md}");
    }

    #[test]
    fn hover_on_thickness_in_constraint_shows_param_info() {
        let source = reify_test_support::bracket_source();
        // 'thickness' in 'constraint thickness > 2mm' is on line 9
        let position = Position::new(9, 15); // on 'thickness' in constraint
        let md =
            hover_markdown(source, position).expect("hover should return info for thickness ref");
        assert!(
            md.contains("param"),
            "should show param (declaration type), got: {md}"
        );
        assert!(
            md.contains("thickness"),
            "should show 'thickness', got: {md}"
        );
    }

    #[test]
    fn hover_on_width_in_let_expr_shows_param_info() {
        let source = reify_test_support::bracket_source();
        // 'width' in 'let volume = width * height * thickness' is on line 7
        let position = Position::new(7, 17); // on 'width' in the expression
        let md = hover_markdown(source, position)
            .expect("hover should return info for width ref in let");
        assert!(
            md.contains("param"),
            "should show param (declaration type), got: {md}"
        );
        assert!(md.contains("width"), "should show 'width', got: {md}");
    }

    #[test]
    fn hover_on_whitespace_returns_none() {
        let source = reify_test_support::bracket_source();
        // Position at the start of line 1 (indentation whitespace)
        let position = Position::new(1, 0);
        assert!(
            compute_hover(source, &test_uri(), position).is_none(),
            "hover on whitespace should return None"
        );
    }

    // --- step-5: hover edge cases ---

    #[test]
    fn hover_on_structure_name_shows_summary() {
        let source = reify_test_support::bracket_source();
        // 'Bracket' is on line 0: "structure Bracket {"
        let position = Position::new(0, 12); // on 'Bracket'
        let md =
            hover_markdown(source, position).expect("hover should return info for structure name");
        assert!(
            md.contains("Bracket"),
            "should mention 'Bracket', got: {md}"
        );
        assert!(
            md.contains("5 params"),
            "should mention param count, got: {md}"
        );
    }

    #[test]
    fn hover_on_keyword_param_shows_description() {
        let source = reify_test_support::bracket_source();
        // 'param' keyword on line 1
        let position = Position::new(1, 6); // on 'param'
        let md =
            hover_markdown(source, position).expect("hover should return info for keyword param");
        assert!(
            md.to_lowercase().contains("param"),
            "should describe param keyword, got: {md}"
        );
    }

    #[test]
    fn hover_on_keyword_constraint_shows_description() {
        let source = reify_test_support::bracket_source();
        // 'constraint' keyword on line 9
        let position = Position::new(9, 6); // on 'constraint'
        let md = hover_markdown(source, position)
            .expect("hover should return info for keyword constraint");
        assert!(
            md.to_lowercase().contains("constraint"),
            "should describe constraint keyword, got: {md}"
        );
    }

    #[test]
    fn hover_on_unknown_word_returns_none() {
        // A source where a word is not a member, structure, or keyword
        let source = "structure Foo {\n  param x: Scalar = unknownword\n}";
        // 'unknownword' is on line 1 around char 22
        let position = Position::new(1, 22);
        // unknownword is not a recognized keyword, member, or structure
        // (it may cause a compile error, but hover should handle it)
        let result = compute_hover(source, &test_uri(), position);
        // unknownword isn't a keyword, member, or structure — should be None
        assert!(result.is_none(), "unknown word should return None hover");
    }

    // --- doc comment hover on members ---

    #[test]
    fn hover_on_documented_param_shows_doc() {
        let source =
            "structure Bracket {\n    /// The width dimension.\n    param width: Scalar = 80mm\n}";
        let position = Position::new(2, 10); // on 'width'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("param width: Scalar"),
            "should contain param signature, got: {md}"
        );
        assert!(
            md.contains("The width dimension."),
            "should contain doc comment, got: {md}"
        );
    }

    #[test]
    fn hover_on_documented_let_shows_doc() {
        let source = "structure Bracket {\n    param width: Scalar = 80mm\n    param height: Scalar = 40mm\n    /// Computed volume.\n    let area = width * height\n}";
        let position = Position::new(4, 8); // on 'area'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("let area"),
            "should contain let signature, got: {md}"
        );
        assert!(
            md.contains("Computed volume."),
            "should contain doc comment, got: {md}"
        );
    }

    #[test]
    fn hover_on_undocumented_param_no_doc_section() {
        let source = reify_test_support::bracket_source();
        let position = Position::new(1, 10); // on 'width'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(md.contains("param"), "should contain 'param', got: {md}");
        assert!(md.contains("width"), "should contain 'width', got: {md}");
        // No doc section — no trailing paragraph
        assert!(
            !md.ends_with("\n\n"),
            "should not end with double newline (empty doc section), got: {md}"
        );
    }

    // --- doc comment hover on structures ---

    #[test]
    fn hover_on_documented_structure_shows_doc() {
        let source =
            "/// A mounting bracket.\nstructure Bracket {\n    param width: Scalar = 80mm\n}";
        let position = Position::new(1, 12); // on 'Bracket'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("structure Bracket"),
            "should contain structure name, got: {md}"
        );
        assert!(
            md.contains("A mounting bracket."),
            "should contain doc comment, got: {md}"
        );
    }

    #[test]
    fn hover_on_undocumented_structure_no_doc_section() {
        let source = reify_test_support::bracket_source();
        let position = Position::new(0, 12); // on 'Bracket'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("Bracket"),
            "should contain structure name, got: {md}"
        );
        // Should not have extra blank doc section
        assert!(
            !md.contains("\n\n\n"),
            "should not have triple newline (empty doc section), got: {md}"
        );
    }

    // --- doc comment hover on fn/trait/enum ---

    #[test]
    fn hover_on_fn_name_shows_signature_and_doc() {
        let source = "/// Compute area.\nfn area(w: Scalar, h: Scalar) -> Scalar { w * h }";
        let position = Position::new(1, 4); // on 'area'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("fn area"),
            "should contain fn signature, got: {md}"
        );
        assert!(
            md.contains("Compute area."),
            "should contain doc comment, got: {md}"
        );
    }

    #[test]
    fn hover_on_fn_name_without_doc_shows_signature() {
        let source = "fn area(w: Scalar, h: Scalar) -> Scalar { w * h }";
        let position = Position::new(0, 4); // on 'area'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("fn area"),
            "should contain fn signature, got: {md}"
        );
        // No doc section
        assert!(
            !md.ends_with("\n\n"),
            "should not end with double newline, got: {md}"
        );
    }

    #[test]
    fn hover_on_trait_name_shows_doc() {
        let source = "/// Rigid body trait.\ntrait Rigid {\n    param mass: Scalar\n}";
        let position = Position::new(1, 7); // on 'Rigid'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("trait Rigid"),
            "should contain trait name, got: {md}"
        );
        assert!(
            md.contains("Rigid body trait."),
            "should contain doc comment, got: {md}"
        );
    }

    #[test]
    fn hover_on_enum_name_shows_doc() {
        let source = "/// Flow direction.\nenum Direction { In, Out }";
        let position = Position::new(1, 6); // on 'Direction'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("enum Direction"),
            "should contain enum name, got: {md}"
        );
        assert!(
            md.contains("Flow direction."),
            "should contain doc comment, got: {md}"
        );
    }

    // --- edge cases ---

    #[test]
    fn hover_multiline_doc_renders_all_lines() {
        let source = "/// First line.\n/// Second line.\nstructure Bracket {\n    param width: Scalar = 80mm\n}";
        let position = Position::new(2, 12); // on 'Bracket'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("First line."),
            "should contain first line, got: {md}"
        );
        assert!(
            md.contains("Second line."),
            "should contain second line, got: {md}"
        );
    }

    #[test]
    fn hover_doc_with_blank_line_paragraph() {
        let source = "/// First paragraph.\n///\n/// Second paragraph.\nstructure Bracket {\n    param width: Scalar = 80mm\n}";
        let position = Position::new(3, 12); // on 'Bracket'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("First paragraph."),
            "should contain first para, got: {md}"
        );
        assert!(
            md.contains("Second paragraph."),
            "should contain second para, got: {md}"
        );
    }

    #[test]
    fn hover_on_documented_param_reference_in_expr_shows_doc() {
        // Hover on 'width' used in a let expression, not at the declaration site
        let source = "structure Bracket {\n    /// The width.\n    param width: Scalar = 80mm\n    let doubled = width * 2\n}";
        let position = Position::new(3, 18); // on 'width' in 'width * 2'
        let md = hover_markdown(source, position).expect("hover should return info");
        assert!(
            md.contains("The width."),
            "should show doc for referenced param, got: {md}"
        );
    }

    #[test]
    fn hover_on_empty_source_returns_none() {
        let result = compute_hover("", &test_uri(), Position::new(0, 0));
        assert!(result.is_none(), "empty source should return None hover");
    }
}
