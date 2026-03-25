use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Url};

use crate::analysis::AnalysisContext;

/// Compute completion items for the given position.
///
/// Returns a flat list of all available completions (keywords, identifiers,
/// types, built-in functions, structure names). Client-side filtering applies.
pub fn compute_completions(source: &str, uri: &Url, _position: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // (a) Keywords
    for kw in KEYWORDS {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    // (b) Built-in functions
    for func in BUILTIN_FUNCTIONS {
        items.push(CompletionItem {
            label: func.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        });
    }

    // (c) Type names
    for ty in TYPE_NAMES {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            ..Default::default()
        });
    }

    // Context-dependent items from the source
    let ctx = AnalysisContext::new(source, uri);

    // (d) Value cell members as variables with type detail
    for (name, _kind, cell_type) in ctx.member_names() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(cell_type.to_string()),
            ..Default::default()
        });
    }

    // (e) Structure names
    for (name, _params, _lets, _constraints) in ctx.structure_names() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::STRUCT),
            ..Default::default()
        });
    }

    items
}

/// Reify language keywords.
const KEYWORDS: &[&str] = &[
    "structure",
    "param",
    "let",
    "constraint",
    "sub",
    "import",
    "if",
    "then",
    "else",
    "and",
    "or",
    "not",
    "true",
    "false",
    "auto",
];

/// Built-in geometry and math functions.
const BUILTIN_FUNCTIONS: &[&str] = &[
    "box", "cylinder", "sphere", "sin", "cos", "tan", "sqrt", "abs", "min", "max",
    "dot", "cross", "normalize", "magnitude",
    // Complex number functions
    "complex", "real", "imag", "conjugate", "complex_magnitude", "phase",
];

/// Built-in type names.
const TYPE_NAMES: &[&str] = &["Scalar", "Bool", "Int", "Real", "String"];

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{CompletionItemKind, Url};

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    // --- step-9: completion tests ---

    #[test]
    fn completions_include_keywords() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let keywords: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .collect();
        // Should include at least: structure, param, let, constraint, sub, import,
        // if, then, else, and, or, not, true, false, auto
        assert!(
            keywords.len() >= 12,
            "expected at least 12 keywords, got {}",
            keywords.len()
        );
        let keyword_labels: Vec<&str> = keywords.iter().map(|k| k.label.as_str()).collect();
        assert!(keyword_labels.contains(&"param"), "should include 'param'");
        assert!(keyword_labels.contains(&"let"), "should include 'let'");
        assert!(
            keyword_labels.contains(&"constraint"),
            "should include 'constraint'"
        );
        assert!(
            keyword_labels.contains(&"structure"),
            "should include 'structure'"
        );
    }

    #[test]
    fn completions_include_scope_identifiers() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(7, 17));
        let variables: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .collect();
        let var_labels: Vec<&str> = variables.iter().map(|v| v.label.as_str()).collect();
        // Should include all value cells: width, height, thickness,
        // fillet_radius, hole_diameter, volume (and possibly body)
        assert!(
            variables.len() >= 6,
            "expected at least 6 scope variables, got {}",
            variables.len()
        );
        assert!(var_labels.contains(&"width"), "should include 'width'");
        assert!(var_labels.contains(&"height"), "should include 'height'");
        assert!(
            var_labels.contains(&"thickness"),
            "should include 'thickness'"
        );
        assert!(var_labels.contains(&"volume"), "should include 'volume'");
        // Variables should have type detail
        let width_item = variables.iter().find(|v| v.label == "width").unwrap();
        assert!(
            width_item.detail.is_some(),
            "width should have type detail"
        );
        assert!(
            width_item.detail.as_ref().unwrap().contains("Scalar"),
            "width detail should mention Scalar"
        );
    }

    #[test]
    fn completions_include_structure_names() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let structs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .collect();
        assert!(
            structs.iter().any(|s| s.label == "Bracket"),
            "should include 'Bracket' struct"
        );
    }

    #[test]
    fn completions_include_builtin_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        let func_labels: Vec<&str> = functions.iter().map(|f| f.label.as_str()).collect();
        // Should include built-in geometry/math functions
        assert!(func_labels.contains(&"box"), "should include 'box'");
        assert!(func_labels.contains(&"sin"), "should include 'sin'");
        assert!(func_labels.contains(&"cos"), "should include 'cos'");
        assert!(func_labels.contains(&"sqrt"), "should include 'sqrt'");
        assert!(func_labels.contains(&"abs"), "should include 'abs'");
        assert!(func_labels.contains(&"min"), "should include 'min'");
        assert!(func_labels.contains(&"max"), "should include 'max'");
    }

    #[test]
    fn completions_include_type_names() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let types: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .collect();
        let type_labels: Vec<&str> = types.iter().map(|t| t.label.as_str()).collect();
        assert!(
            type_labels.contains(&"Scalar"),
            "should include 'Scalar'"
        );
        assert!(type_labels.contains(&"Bool"), "should include 'Bool'");
        assert!(type_labels.contains(&"Int"), "should include 'Int'");
        assert!(type_labels.contains(&"Real"), "should include 'Real'");
        assert!(
            type_labels.contains(&"String"),
            "should include 'String'"
        );
    }

    #[test]
    fn completions_on_empty_source_still_include_keywords_and_builtins() {
        let items = compute_completions("", &test_uri(), Position::new(0, 0));
        let keywords: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .collect();
        let functions: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .collect();
        assert!(
            !keywords.is_empty(),
            "empty source should still have keywords"
        );
        assert!(
            !functions.is_empty(),
            "empty source should still have built-in functions"
        );
    }

    // --- linalg builtin completions (step-11) ---

    #[test]
    fn completions_include_linalg_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"dot"), "should include 'dot'");
        assert!(func_labels.contains(&"cross"), "should include 'cross'");
        assert!(func_labels.contains(&"normalize"), "should include 'normalize'");
        assert!(func_labels.contains(&"magnitude"), "should include 'magnitude'");
    }

    // --- complex builtin completions (step-10) ---

    #[test]
    fn completions_include_complex_functions() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();
        assert!(func_labels.contains(&"complex"), "should include 'complex'");
        assert!(func_labels.contains(&"real"), "should include 'real'");
        assert!(func_labels.contains(&"imag"), "should include 'imag'");
        assert!(func_labels.contains(&"conjugate"), "should include 'conjugate'");
        assert!(func_labels.contains(&"complex_magnitude"), "should include 'complex_magnitude'");
        assert!(func_labels.contains(&"phase"), "should include 'phase'");
    }
}
