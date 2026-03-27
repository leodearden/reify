use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Url};

use crate::analysis::AnalysisContext;
use crate::convert::position_to_offset;

/// Cursor context for position-sensitive completions.
#[derive(Debug)]
enum CompletionContext {
    /// Outside any structure definition
    TopLevel,
    /// Inside a structure body, at declaration level
    Body,
    /// Inside an expression (after `=`, inside constraint)
    Expression,
    /// Immediately after a `.` operator
    AfterDot,
    /// In a type annotation position (after `:`)
    TypeAnnotation,
}

/// Top-level keywords (structure definitions, imports).
const TOP_LEVEL_KEYWORDS: &[&str] = &["structure", "import"];

/// Body-level declaration keywords.
const BODY_KEYWORDS: &[&str] = &["param", "let", "constraint", "sub"];

/// Expression-level keywords.
const EXPR_KEYWORDS: &[&str] = &[
    "if", "then", "else", "and", "or", "not", "true", "false", "auto",
];

/// Built-in geometry and math functions.
const BUILTIN_FUNCTIONS: &[&str] = &[
    "box", "cylinder", "sphere", "sin", "cos", "tan", "sqrt", "abs", "min", "max",
    "dot", "cross", "normalize", "magnitude",
];

/// Built-in type names.
const TYPE_NAMES: &[&str] = &["Scalar", "Bool", "Int", "Real", "String"];

/// Determine the completion context from the cursor position.
fn determine_context(source: &str, position: Position) -> CompletionContext {
    let offset = position_to_offset(source, position);
    let before_cursor = &source[..offset.min(source.len())];

    // After-dot: last non-whitespace character is '.'
    let trimmed = before_cursor.trim_end();
    if trimmed.ends_with('.') {
        return CompletionContext::AfterDot;
    }

    // Brace depth determines top-level vs inside structure
    let brace_depth: i32 = before_cursor
        .chars()
        .map(|c| match c {
            '{' => 1,
            '}' => -1,
            _ => 0,
        })
        .sum();

    if brace_depth <= 0 {
        return CompletionContext::TopLevel;
    }

    // Inside structure body — examine current line for sub-context
    let line_start = before_cursor.rfind('\n').map_or(0, |p| p + 1);
    let line_before = before_cursor[line_start..].trim();

    // Type annotation: after ':' in param/sub declaration, no '=' yet
    if (line_before.starts_with("param ") || line_before.starts_with("sub "))
        && let Some(colon_pos) = line_before.find(':')
        && !line_before[colon_pos + 1..].contains('=')
    {
        return CompletionContext::TypeAnnotation;
    }

    // Expression: after '=' assignment
    if let Some(eq_pos) = line_before.find('=') {
        let before_eq = if eq_pos > 0 {
            line_before.as_bytes()[eq_pos - 1]
        } else {
            b' '
        };
        let after_eq = if eq_pos + 1 < line_before.len() {
            line_before.as_bytes()[eq_pos + 1]
        } else {
            b' '
        };
        if before_eq != b'>' && before_eq != b'<' && before_eq != b'!' && after_eq != b'=' {
            return CompletionContext::Expression;
        }
    }

    // Expression: inside constraint expression
    if line_before.starts_with("constraint ") || line_before == "constraint" {
        return CompletionContext::Expression;
    }

    CompletionContext::Body
}

/// Compute completion items for the given position.
///
/// Returns position-sensitive completions filtered by cursor context.
pub fn compute_completions(source: &str, uri: &Url, position: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let context = determine_context(source, position);

    // (a) Keywords — filtered by context
    match context {
        CompletionContext::TopLevel => {
            for kw in TOP_LEVEL_KEYWORDS.iter().chain(EXPR_KEYWORDS.iter()) {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
        }
        CompletionContext::Body => {
            for kw in BODY_KEYWORDS.iter().chain(EXPR_KEYWORDS.iter()) {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
        }
        CompletionContext::Expression => {
            for kw in EXPR_KEYWORDS {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
        }
        CompletionContext::AfterDot | CompletionContext::TypeAnnotation => {}
    }

    // (b) Built-in functions — not in AfterDot or TypeAnnotation
    if !matches!(
        context,
        CompletionContext::AfterDot | CompletionContext::TypeAnnotation
    ) {
        for func in BUILTIN_FUNCTIONS {
            items.push(CompletionItem {
                label: func.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                ..Default::default()
            });
        }
    }

    // (c) Type names — not in AfterDot
    if !matches!(context, CompletionContext::AfterDot) {
        for ty in TYPE_NAMES {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                ..Default::default()
            });
        }
    }

    // Context-dependent items from the source
    let ctx = AnalysisContext::new(source, uri);

    // (d) Value cell members — not in TypeAnnotation (AfterDot SHOULD show members)
    if !matches!(context, CompletionContext::TypeAnnotation) {
        for (name, _kind, cell_type) in ctx.member_names() {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(cell_type.to_string()),
                ..Default::default()
            });
        }
    }

    // (e) Structure names — not in AfterDot
    if !matches!(context, CompletionContext::AfterDot) {
        for (name, _params, _lets, _constraints) in ctx.structure_names() {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::STRUCT),
                ..Default::default()
            });
        }
    }

    items
}

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
        // Inside body: param, let, constraint, sub + expression keywords (13 total)
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
            keyword_labels.contains(&"sub"),
            "should include 'sub'"
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

    // --- position-sensitive completion tests (task 481) ---
    // These tests assert that completions are context-sensitive based on cursor position.
    // The current implementation returns everything everywhere, so the exclusion assertions
    // FAIL — this is the expected TDD "red" state. The follow-up task implements
    // position-sensitive filtering to make them pass (turn "green").

    #[test]
    fn completion_top_level_excludes_body_keywords() {
        // Source: one structure, then a blank line. Cursor is outside any structure.
        let source = "structure Foo {\n    param x: Scalar = 1mm\n}\n";
        let items = compute_completions(source, &test_uri(), Position::new(3, 0));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // At top level, structure-defining and import keywords should be present
        assert!(keyword_labels.contains(&"structure"), "top-level should include 'structure'");
        assert!(keyword_labels.contains(&"import"), "top-level should include 'import'");

        // Body-only keywords should NOT be present at top level
        // (Future keywords like fn, trait, enum would also be asserted here once added to KEYWORDS)
        assert!(!keyword_labels.contains(&"param"), "top-level should NOT include 'param'");
        assert!(!keyword_labels.contains(&"let"), "top-level should NOT include 'let'");
        assert!(!keyword_labels.contains(&"constraint"), "top-level should NOT include 'constraint'");
        assert!(!keyword_labels.contains(&"sub"), "top-level should NOT include 'sub'");
    }

    #[test]
    fn completion_inside_body_excludes_top_level_keywords() {
        let source = reify_test_support::bracket_source();
        // Line 6 is the blank line between params and let, inside body
        let items = compute_completions(source, &test_uri(), Position::new(6, 4));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // Inside a structure body, declaration keywords should be present
        assert!(keyword_labels.contains(&"param"), "body should include 'param'");
        assert!(keyword_labels.contains(&"let"), "body should include 'let'");
        assert!(keyword_labels.contains(&"constraint"), "body should include 'constraint'");
        assert!(keyword_labels.contains(&"sub"), "body should include 'sub'");

        // Top-level-only keywords should NOT appear inside a body
        assert!(!keyword_labels.contains(&"structure"), "body should NOT include 'structure'");
        assert!(!keyword_labels.contains(&"import"), "body should NOT include 'import'");
    }

    #[test]
    fn completion_expression_excludes_declaration_keywords() {
        // Cursor is in an expression position (after `= `)
        let source = "structure Foo {\n    let x = \n}";
        // Line 1, col 12 is after "    let x = " — inside the expression
        let items = compute_completions(source, &test_uri(), Position::new(1, 12));

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // In an expression, builtin functions should be available
        assert!(func_labels.contains(&"sin"), "expression should include 'sin'");
        assert!(func_labels.contains(&"cos"), "expression should include 'cos'");

        // Declaration keywords should NOT appear in expression context
        assert!(!keyword_labels.contains(&"param"), "expression should NOT include 'param'");
        assert!(!keyword_labels.contains(&"let"), "expression should NOT include 'let'");
        assert!(!keyword_labels.contains(&"constraint"), "expression should NOT include 'constraint'");
        assert!(!keyword_labels.contains(&"structure"), "expression should NOT include 'structure'");
    }

    #[test]
    fn completion_after_dot_returns_only_members() {
        // Cursor is after a dot — should only return member completions
        // Note: Bar is undefined, but the exclusion assertions are what matter
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n    sub part: Bar\n    let x = part.\n}";
        // Line 4, col 18 is after "    let x = part." — after the dot
        let items = compute_completions(source, &test_uri(), Position::new(4, 18));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        // After a dot, no keywords should appear
        assert!(keyword_labels.is_empty(), "after dot should have no keywords, got: {:?}", keyword_labels);
        // After a dot, no builtin functions should appear
        assert!(func_labels.is_empty(), "after dot should have no builtin functions, got: {:?}", func_labels);
        // After a dot, no type names should appear
        assert!(type_labels.is_empty(), "after dot should have no type names, got: {:?}", type_labels);
        // Ideally this would also assert that Bar's members are returned,
        // but Bar is undefined so we can only check exclusions here.
    }

    #[test]
    fn completion_type_position_returns_types_and_structs() {
        // Cursor is in a type annotation position (after `x: `)
        let source = "structure Foo {\n    param x: \n}";
        // Line 1, col 13 is after "    param x: " — in type position
        let items = compute_completions(source, &test_uri(), Position::new(1, 13));

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        let struct_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .map(|s| s.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        // In type position, type names should be present
        assert!(type_labels.contains(&"Scalar"), "type position should include 'Scalar'");
        assert!(type_labels.contains(&"Bool"), "type position should include 'Bool'");
        assert!(type_labels.contains(&"Int"), "type position should include 'Int'");
        assert!(type_labels.contains(&"Real"), "type position should include 'Real'");
        assert!(type_labels.contains(&"String"), "type position should include 'String'");

        // Structure names should be available as types
        assert!(struct_labels.contains(&"Foo"), "type position should include struct 'Foo'");

        // Keywords should NOT appear in type position
        assert!(!keyword_labels.contains(&"param"), "type position should NOT include 'param'");
        assert!(!keyword_labels.contains(&"let"), "type position should NOT include 'let'");
        assert!(!keyword_labels.contains(&"structure"), "type position should NOT include 'structure'");

        // Builtin functions should NOT appear in type position
        assert!(!func_labels.contains(&"sin"), "type position should NOT include 'sin'");
        assert!(!func_labels.contains(&"box"), "type position should NOT include 'box'");
    }

    #[test]
    fn completion_constraint_expr_excludes_declaration_keywords() {
        let source = reify_test_support::bracket_source();
        // Line 9: "    constraint thickness > 2mm" — col 27 is inside the expression
        let items = compute_completions(source, &test_uri(), Position::new(9, 27));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        let func_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|f| f.label.as_str())
            .collect();

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // In a constraint expression, member variables should be available
        assert!(var_labels.contains(&"width"), "constraint expr should include 'width'");
        assert!(var_labels.contains(&"height"), "constraint expr should include 'height'");

        // Builtin functions should be available in expressions
        assert!(func_labels.contains(&"sin"), "constraint expr should include 'sin'");
        assert!(func_labels.contains(&"abs"), "constraint expr should include 'abs'");

        // Declaration keywords should NOT appear inside a constraint expression
        assert!(!keyword_labels.contains(&"param"), "constraint expr should NOT include 'param'");
        assert!(!keyword_labels.contains(&"let"), "constraint expr should NOT include 'let'");
        assert!(!keyword_labels.contains(&"constraint"), "constraint expr should NOT include 'constraint'");
        assert!(!keyword_labels.contains(&"structure"), "constraint expr should NOT include 'structure'");
    }

    #[test]
    fn completion_after_dot_includes_scope_members() {
        // After a dot, member variables should still be returned (section (d)).
        // This tests the positive case that completion_after_dot_returns_only_members misses:
        // it only checks exclusions (no keywords, no builtins, no types) but doesn't verify
        // that member VARIABLE items are actually present.
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n    let x = a.\n}";
        // Line 3, col 14 is after "    let x = a." — after the dot
        let items = compute_completions(source, &test_uri(), Position::new(3, 14));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        // After a dot, scope members should be available as completions
        assert!(!var_labels.is_empty(), "after dot should include scope members, got empty list");
        assert!(var_labels.contains(&"a"), "after dot should include member 'a'");
        assert!(var_labels.contains(&"b"), "after dot should include member 'b'");
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
}
