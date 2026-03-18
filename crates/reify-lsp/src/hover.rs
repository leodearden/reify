use tower_lsp::lsp_types::{Hover, Position, Url};

/// Compute hover information for the symbol at the given position.
///
/// Returns `None` if there is nothing to show at the given position.
pub fn compute_hover(_source: &str, _uri: &Url, _position: Position) -> Option<Hover> {
    None // TODO: implement
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
