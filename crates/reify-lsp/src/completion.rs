use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Url};

use crate::analysis::AnalysisContext;
use crate::convert::position_to_offset;

/// The syntactic context at the cursor position, used to filter completions.
#[derive(Debug)]
pub enum CursorContext {
    /// Cursor is outside all structure/occurrence spans.
    TopLevel,
    /// Cursor is inside a structure/occurrence body on a line that doesn't
    /// indicate a more specific context (expression, dot access, type position).
    StructureBody {
        /// Name of the enclosing structure/occurrence.
        structure_name: String,
    },
    /// Cursor is in an expression position (after `=`, inside a constraint, etc).
    Expression {
        /// Name of the enclosing structure, if any.
        structure_name: Option<String>,
    },
    /// Cursor is immediately after a `.` — member access.
    DotAccess,
    /// Cursor is in a type annotation position (after `:` in a declaration).
    TypePosition,
}

/// Determine the syntactic context at the given cursor position.
pub fn determine_context(source: &str, position: Position, ctx: &AnalysisContext) -> CursorContext {
    let offset = position_to_offset(source, position);

    // Check if cursor is inside a structure/occurrence span
    let enclosing = ctx.enclosing_decl_name_at(offset);

    if enclosing.is_none() {
        return CursorContext::TopLevel;
    }

    let structure_name = enclosing.unwrap().to_string();

    // Extract the current line prefix (text from start of line to cursor)
    let line_prefix = extract_line_prefix(source, offset);

    // Check for DotAccess: scan backward through whitespace for a '.'
    {
        let trimmed = line_prefix.trim_end();
        if trimmed.ends_with('.') {
            return CursorContext::DotAccess;
        }
    }

    // Check for TypePosition: look for ':' without intervening '=' on the line prefix
    // Must check before Expression since 'param x: ' has no '=' yet
    {
        let trimmed = line_prefix.trim_start();
        if starts_with_decl_keyword(trimmed)
            && let Some(colon_pos) = line_prefix.rfind(':')
        {
            let after_colon = &line_prefix[colon_pos + 1..];
            if !after_colon.contains('=') {
                return CursorContext::TypePosition;
            }
        }
    }

    // Check for Expression: cursor after '=' on the line, or inside a constraint expression
    {
        if line_prefix.contains('=') {
            // Cursor is after an '=' sign — expression position
            // But only if the cursor is after the last '=' on the line
            if let Some(eq_pos) = line_prefix.rfind('=') {
                let cursor_in_line = line_prefix.len();
                if cursor_in_line > eq_pos {
                    return CursorContext::Expression {
                        structure_name: Some(structure_name),
                    };
                }
            }
        }

        // Constraint lines: everything after "constraint " is an expression
        let trimmed = line_prefix.trim_start();
        if trimmed.starts_with("constraint") && trimmed.len() > "constraint".len() {
            let after_kw = &trimmed["constraint".len()..];
            if after_kw.starts_with(|c: char| c.is_whitespace()) {
                return CursorContext::Expression {
                    structure_name: Some(structure_name),
                };
            }
        }
    }

    // Default: inside a structure body but no more specific context
    CursorContext::StructureBody { structure_name }
}

/// Extract the text from the start of the current line to the given byte offset.
fn extract_line_prefix(source: &str, offset: usize) -> &str {
    let start = source[..offset]
        .rfind('\n')
        .map(|pos| pos + 1)
        .unwrap_or(0);
    &source[start..offset]
}

/// Check if a trimmed line starts with a declaration keyword (param, let, sub).
fn starts_with_decl_keyword(trimmed: &str) -> bool {
    for kw in &["param", "let", "sub"] {
        if trimmed.starts_with(kw)
            && trimmed[kw.len()..]
                .starts_with(|c: char| c.is_whitespace())
        {
            return true;
        }
    }
    false
}

/// Compute completion items for the given position.
///
/// Returns context-sensitive completions based on the cursor position:
/// - TopLevel: top-level keywords, type names, structure names, builtins
/// - StructureBody: body/expr keywords, scoped members, structures, builtins, types
/// - Expression: expr keywords, members, builtins, structures, types
/// - DotAccess: member names only
/// - TypePosition: type names and structure names only
pub fn compute_completions(source: &str, uri: &Url, position: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let ctx = AnalysisContext::new(source, uri);
    let cursor_ctx = determine_context(source, position, &ctx);

    match cursor_ctx {
        CursorContext::TopLevel => {
            push_keywords(&mut items, TOP_LEVEL_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            push_structure_names(&mut items, &ctx);
        }
        CursorContext::StructureBody { ref structure_name } => {
            push_keywords(&mut items, BODY_KEYWORDS);
            push_keywords(&mut items, EXPR_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            push_scoped_members(&mut items, &ctx, structure_name);
            push_structure_names(&mut items, &ctx);
        }
        CursorContext::Expression {
            ref structure_name, ..
        } => {
            push_keywords(&mut items, EXPR_KEYWORDS);
            push_builtins(&mut items);
            push_type_names(&mut items);
            if let Some(name) = structure_name {
                push_scoped_members(&mut items, &ctx, name);
            } else {
                push_all_members(&mut items, &ctx);
            }
            push_structure_names(&mut items, &ctx);
        }
        CursorContext::DotAccess => {
            push_all_members(&mut items, &ctx);
        }
        CursorContext::TypePosition => {
            push_type_names(&mut items);
            push_structure_names(&mut items, &ctx);
        }
    }

    items
}

fn push_keywords(items: &mut Vec<CompletionItem>, keywords: &[&str]) {
    for kw in keywords {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }
}

fn push_builtins(items: &mut Vec<CompletionItem>) {
    for func in BUILTIN_FUNCTIONS {
        items.push(CompletionItem {
            label: func.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        });
    }
}

fn push_type_names(items: &mut Vec<CompletionItem>) {
    for ty in TYPE_NAMES {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            ..Default::default()
        });
    }
}

fn push_all_members(items: &mut Vec<CompletionItem>, ctx: &AnalysisContext) {
    for (name, _kind, cell_type) in ctx.member_names() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(cell_type.to_string()),
            ..Default::default()
        });
    }
}

fn push_scoped_members(items: &mut Vec<CompletionItem>, ctx: &AnalysisContext, structure_name: &str) {
    for (name, _kind, cell_type) in ctx.member_names_for_structure(structure_name) {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(cell_type.to_string()),
            ..Default::default()
        });
    }
}

fn push_structure_names(items: &mut Vec<CompletionItem>, ctx: &AnalysisContext) {
    for (name, _params, _lets, _constraints, _kind) in ctx.structure_names() {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::STRUCT),
            ..Default::default()
        });
    }
}

/// Keywords that are only valid at the top level (outside structure bodies).
const TOP_LEVEL_KEYWORDS: &[&str] = &[
    "structure",
    "occurrence",
    "import",
    "fn",
    "trait",
    "enum",
];

/// Keywords that start declaration lines inside a structure body.
const BODY_KEYWORDS: &[&str] = &[
    "param",
    "let",
    "constraint",
    "sub",
    "auto",
    "purpose",
    "minimize",
    "maximize",
    "port",
    "connect",
    "where",
];

/// Keywords valid inside expressions (conditions, values, operators).
const EXPR_KEYWORDS: &[&str] = &[
    "if", "then", "else", "and", "or", "not", "true", "false",
];

/// Built-in geometry and math functions.
const BUILTIN_FUNCTIONS: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "sin",
    "cos",
    "tan",
    "sqrt",
    "abs",
    "min",
    "max",
    "dot",
    "cross",
    "normalize",
    "magnitude",
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

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
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
        // Note: Position(1,0) is inside the structure body, so 'structure'
        // (a top-level keyword) is not expected here after position-aware narrowing.
    }

    // TODO(task-2): Position(7,17) is Expression context. When position-sensitive
    // filtering lands, verify these assertions still hold in Expression context.
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
        assert!(width_item.detail.is_some(), "width should have type detail");
        assert!(
            width_item.detail.as_ref().unwrap().contains("Scalar"),
            "width detail should mention Scalar"
        );
    }

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
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

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
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

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
    #[test]
    fn completions_include_type_names() {
        let source = reify_test_support::bracket_source();
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let types: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .collect();
        let type_labels: Vec<&str> = types.iter().map(|t| t.label.as_str()).collect();
        assert!(type_labels.contains(&"Scalar"), "should include 'Scalar'");
        assert!(type_labels.contains(&"Bool"), "should include 'Bool'");
        assert!(type_labels.contains(&"Int"), "should include 'Int'");
        assert!(type_labels.contains(&"Real"), "should include 'Real'");
        assert!(type_labels.contains(&"String"), "should include 'String'");
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

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
    #[test]
    fn completions_include_occurrence_names() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let structs: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::STRUCT))
            .collect();
        assert!(
            structs.iter().any(|s| s.label == "Joint"),
            "should include 'Joint' occurrence in completions"
        );
    }

    // --- position-sensitive completion tests (task 481) ---
    // These tests assert that completions are context-sensitive based on cursor position.
    // They are #[ignore] because the current implementation returns everything everywhere;
    // task 2 will implement position-sensitive filtering to make them pass.

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
        assert!(
            keyword_labels.contains(&"structure"),
            "top-level should include 'structure'"
        );
        assert!(
            keyword_labels.contains(&"import"),
            "top-level should include 'import'"
        );

        // Body-only keywords should NOT be present at top level
        // (Future keywords like fn, trait, enum would also be asserted here once added to KEYWORDS)
        assert!(
            !keyword_labels.contains(&"param"),
            "top-level should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "top-level should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "top-level should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"sub"),
            "top-level should NOT include 'sub'"
        );
    }

    #[test]
    fn completion_inside_body_excludes_top_level_keywords() {
        let source = reify_test_support::bracket_source();
        // Line 6 is the empty line between params and lets, inside body (col 0 since line is empty)
        let items = compute_completions(source, &test_uri(), Position::new(6, 0));

        let keyword_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
            .map(|k| k.label.as_str())
            .collect();

        // Inside a structure body, declaration keywords should be present
        assert!(
            keyword_labels.contains(&"param"),
            "body should include 'param'"
        );
        assert!(keyword_labels.contains(&"let"), "body should include 'let'");
        assert!(
            keyword_labels.contains(&"constraint"),
            "body should include 'constraint'"
        );
        assert!(keyword_labels.contains(&"sub"), "body should include 'sub'");

        // Top-level-only keywords should NOT appear inside a body
        assert!(
            !keyword_labels.contains(&"structure"),
            "body should NOT include 'structure'"
        );
        assert!(
            !keyword_labels.contains(&"import"),
            "body should NOT include 'import'"
        );
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
        assert!(
            func_labels.contains(&"sin"),
            "expression should include 'sin'"
        );
        assert!(
            func_labels.contains(&"cos"),
            "expression should include 'cos'"
        );

        // Declaration keywords should NOT appear in expression context
        assert!(
            !keyword_labels.contains(&"param"),
            "expression should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "expression should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "expression should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "expression should NOT include 'structure'"
        );
    }

    #[test]
    fn completion_after_dot_returns_only_members() {
        // Cursor is after a dot — should only return member completions
        // Note: Bar is undefined, but the exclusion assertions are what matter
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n    sub part: Bar\n    let x = part.\n}";
        // Line 4, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(4, 17));

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
        assert!(
            keyword_labels.is_empty(),
            "after dot should have no keywords, got: {:?}",
            keyword_labels
        );
        // After a dot, no builtin functions should appear
        assert!(
            func_labels.is_empty(),
            "after dot should have no builtin functions, got: {:?}",
            func_labels
        );
        // After a dot, no type names should appear
        assert!(
            type_labels.is_empty(),
            "after dot should have no type names, got: {:?}",
            type_labels
        );
        // Ideally this would also assert that Bar's members are returned,
        // but Bar is undefined so we can only check exclusions here.
    }

    #[test]
    fn completion_after_dot_defined_struct_returns_only_members() {
        // Foo is defined with params a and b.
        // Bar references Foo via `sub part: Foo`, then `let x = part.` triggers dot-access.
        // This test has both positive (a, b present) AND negative (no keywords/functions/types)
        // assertions, addressing the vacuous_test finding in the exclusion-only test above.
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    param b: Scalar = 2mm\n}\nstructure Bar {\n    sub part: Foo\n    let x = part.\n}";
        // Line 6, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(6, 17));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
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

        let type_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
            .map(|t| t.label.as_str())
            .collect();

        // Positive: Foo's members should appear
        assert!(
            !var_labels.is_empty(),
            "after dot with defined struct should have member completions"
        );
        assert!(
            var_labels.contains(&"a"),
            "should include Foo's 'a', got: {:?}",
            var_labels
        );
        assert!(
            var_labels.contains(&"b"),
            "should include Foo's 'b', got: {:?}",
            var_labels
        );

        // Negative: no keywords, functions, or types after dot
        assert!(
            keyword_labels.is_empty(),
            "after dot should have no keywords, got: {:?}",
            keyword_labels
        );
        assert!(
            func_labels.is_empty(),
            "after dot should have no builtin functions, got: {:?}",
            func_labels
        );
        assert!(
            type_labels.is_empty(),
            "after dot should have no type names, got: {:?}",
            type_labels
        );
    }

    #[test]
    fn completion_after_dot_includes_known_members() {
        // Two defined structures so push_all_members returns real members.
        let source = "structure Bracket {\n    param width: Scalar = 80mm\n    param height: Scalar = 100mm\n}\nstructure Assembly {\n    sub part: Bracket\n    let x = part.\n}";
        // Line 6, col 17 is after the dot on "    let x = part."
        let items = compute_completions(source, &test_uri(), Position::new(6, 17));

        let var_labels: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .map(|v| v.label.as_str())
            .collect();

        // DotAccess context calls push_all_members, which returns members from
        // all defined structures. Bracket's params should appear.
        assert!(
            !var_labels.is_empty(),
            "after dot with defined structures should have member completions"
        );
        assert!(
            var_labels.contains(&"width"),
            "should include Bracket's 'width', got: {:?}",
            var_labels
        );
        assert!(
            var_labels.contains(&"height"),
            "should include Bracket's 'height', got: {:?}",
            var_labels
        );
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
        assert!(
            type_labels.contains(&"Scalar"),
            "type position should include 'Scalar'"
        );
        assert!(
            type_labels.contains(&"Bool"),
            "type position should include 'Bool'"
        );
        assert!(
            type_labels.contains(&"Int"),
            "type position should include 'Int'"
        );
        assert!(
            type_labels.contains(&"Real"),
            "type position should include 'Real'"
        );
        assert!(
            type_labels.contains(&"String"),
            "type position should include 'String'"
        );

        // Structure names should be available as types
        assert!(
            struct_labels.contains(&"Foo"),
            "type position should include struct 'Foo'"
        );

        // Keywords should NOT appear in type position
        assert!(
            !keyword_labels.contains(&"param"),
            "type position should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "type position should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "type position should NOT include 'structure'"
        );

        // Builtin functions should NOT appear in type position
        assert!(
            !func_labels.contains(&"sin"),
            "type position should NOT include 'sin'"
        );
        assert!(
            !func_labels.contains(&"box"),
            "type position should NOT include 'box'"
        );
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
        assert!(
            var_labels.contains(&"width"),
            "constraint expr should include 'width'"
        );
        assert!(
            var_labels.contains(&"height"),
            "constraint expr should include 'height'"
        );

        // Builtin functions should be available in expressions
        assert!(
            func_labels.contains(&"sin"),
            "constraint expr should include 'sin'"
        );
        assert!(
            func_labels.contains(&"abs"),
            "constraint expr should include 'abs'"
        );

        // Declaration keywords should NOT appear inside a constraint expression
        assert!(
            !keyword_labels.contains(&"param"),
            "constraint expr should NOT include 'param'"
        );
        assert!(
            !keyword_labels.contains(&"let"),
            "constraint expr should NOT include 'let'"
        );
        assert!(
            !keyword_labels.contains(&"constraint"),
            "constraint expr should NOT include 'constraint'"
        );
        assert!(
            !keyword_labels.contains(&"structure"),
            "constraint expr should NOT include 'structure'"
        );
    }

    // --- determine_context unit tests ---

    #[test]
    fn determine_context_top_level_outside_structure() {
        // Cursor on line 3 (after the closing brace) is outside any structure.
        let source = "structure Foo {\n    param x: Scalar = 1mm\n}\n";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 0), &ctx);
        assert!(
            matches!(result, CursorContext::TopLevel),
            "expected TopLevel, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_structure_body_blank_line() {
        // Cursor inside bracket source on the empty line 6 (col 0 since line is empty).
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(6, 0), &ctx);
        assert!(
            matches!(result, CursorContext::StructureBody { .. }),
            "expected StructureBody, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_after_equals() {
        // "let x = " — cursor after '=' on a let line
        let source = "structure Foo {\n    let x = \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 12), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression after '=', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_in_constraint() {
        // "constraint thickness > 2mm" — cursor inside the expression
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(9, 27), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression in constraint, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_expression_param_default() {
        // "param x: Scalar = " — cursor after '=' in a param default
        let source = "structure Foo {\n    param x: Scalar = \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 23), &ctx);
        assert!(
            matches!(result, CursorContext::Expression { .. }),
            "expected Expression after param default '=', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_dot_access_after_dot() {
        // "let x = part." — cursor immediately after the dot
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    sub part: Bar\n    let x = part.\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 18), &ctx);
        assert!(
            matches!(result, CursorContext::DotAccess),
            "expected DotAccess after '.', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_dot_access_with_trailing_space() {
        // "let x = part. " — cursor after dot + space
        let source = "structure Foo {\n    param a: Scalar = 1mm\n    sub part: Bar\n    let x = part. \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(3, 19), &ctx);
        assert!(
            matches!(result, CursorContext::DotAccess),
            "expected DotAccess after '. ', got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_type_position_after_colon_in_param() {
        // "param x: " — cursor after ': ' in a param declaration
        let source = "structure Foo {\n    param x: \n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(1, 13), &ctx);
        assert!(
            matches!(result, CursorContext::TypePosition),
            "expected TypePosition after ':' in param, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_type_position_after_colon_in_let() {
        // "let x: " — cursor after ': ' in a let with type annotation
        let source = "structure Foo {\n    let x: Int = 5\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Position right after "let x: " — col 11 = after "    let x: "
        let result = determine_context(source, Position::new(1, 11), &ctx);
        assert!(
            matches!(result, CursorContext::TypePosition),
            "expected TypePosition after ':' in let, got {:?}",
            result
        );
    }

    #[test]
    fn determine_context_empty_source_is_top_level() {
        let source = "";
        let ctx = AnalysisContext::new(source, &test_uri());
        let result = determine_context(source, Position::new(0, 0), &ctx);
        assert!(
            matches!(result, CursorContext::TopLevel),
            "expected TopLevel for empty source, got {:?}",
            result
        );
    }

    // --- guarded-group completion tests ---

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
    #[test]
    fn completions_include_guarded_group_members() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param guarded_x : Scalar = 5mm
    }
}"#;
        let items = compute_completions(source, &test_uri(), Position::new(1, 0));
        let variables: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::VARIABLE))
            .collect();
        let var_labels: Vec<&str> = variables.iter().map(|v| v.label.as_str()).collect();
        assert!(
            var_labels.contains(&"cond"),
            "should include top-level param 'cond', got: {var_labels:?}"
        );
        assert!(
            var_labels.contains(&"guarded_x"),
            "should include guarded-group param 'guarded_x', got: {var_labels:?}"
        );
    }

    // --- linalg builtin completions (step-11) ---

    // TODO(task-2): Position(1,0) is inside a structure body. When position-sensitive
    // filtering lands, verify these assertions still hold in StructureBody context.
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
        assert!(
            func_labels.contains(&"normalize"),
            "should include 'normalize'"
        );
        assert!(
            func_labels.contains(&"magnitude"),
            "should include 'magnitude'"
        );
    }
}
