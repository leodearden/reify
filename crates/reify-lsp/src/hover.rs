use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Url};

use crate::analysis::{format_value, AnalysisContext};
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

        let md = format!(
            "```reify\n{kind_str} {word}: {type_str}{}\n```",
            value_str.unwrap_or_default()
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: None,
        });
    }

    None
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
        let md = hover_markdown(source, position)
            .expect("hover should return info for thickness ref");
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
}
