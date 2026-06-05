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

    // Determine the enclosing structure so member lookup is scoped correctly
    let enclosing = ctx.enclosing_decl_name_at(offset);

    // Try member lookup first.
    //
    // Unlike goto_def.rs, which falls back to an unscoped lookup
    // (`find_member_decl(word, None)`) when the scoped search misses — as a
    // navigation convenience — hover intentionally stays scoped. Cross-structure
    // member references are not valid in the Reify language, so showing
    // type/value info from a foreign structure's member would be misleading.
    // If scoped lookup returns None, we fall through to structure names,
    // fn/trait/enum names, and keywords — which is the correct behavior.
    //
    // See test: hover_no_fallback_to_other_structure_member
    if let Some(info) = ctx.find_member_decl(word, enclosing) {
        let kind_str = match info.kind {
            reify_compiler::ValueCellKind::Param => "param",
            reify_compiler::ValueCellKind::Let => "let",
            reify_compiler::ValueCellKind::Auto { free: true } => "auto(free)",
            reify_compiler::ValueCellKind::Auto { free: false } => "auto",
        };
        let type_str = info.cell_type.to_string();

        // Try to get the evaluated value using the member's owning declaration
        let value_str = ctx
            .get_value(info.decl_name, word)
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

    // Try structure/occurrence name
    for entity in ctx.entity_names() {
        if entity.name == word {
            let mut md = format!(
                "```reify\n{kind} {name}\n```\n\n{params} params, {lets} lets, {constraints} constraints",
                kind = entity.kind,
                name = entity.name,
                params = entity.params,
                lets = entity.lets,
                constraints = entity.constraints,
            );
            if let Some(doc) = ctx.find_entity_doc(entity.name) {
                md.push_str("\n\n");
                md.push_str(doc);
            }
            return Some(make_hover_markdown(md));
        }
    }

    // Try fn/trait/enum names
    for decl in &ctx.parsed.declarations {
        match decl {
            reify_ast::Declaration::Function(f) if f.name == word => {
                let params_str: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_expr))
                    .collect();
                let ret = f
                    .return_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t))
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
            reify_ast::Declaration::Trait(t) if t.name == word => {
                let mut md = format!("```reify\ntrait {}\n```", t.name);
                if !t.refinements.is_empty() {
                    md = format!(
                        "```reify\ntrait {} : {}\n```",
                        t.name,
                        t.refinements
                            .iter()
                            .map(|r| r.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" + ")
                    );
                }
                if let Some(doc) = ctx.find_entity_doc(word) {
                    md.push_str("\n\n");
                    md.push_str(doc);
                }
                return Some(make_hover_markdown(md));
            }
            reify_ast::Declaration::Enum(e) if e.name == word => {
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

/// Exhaustive table of every Reify keyword that has a hover description.
///
/// Exposed as `pub(crate)` so the test module can enumerate it for the reverse
/// coverage check (every described keyword must also appear in a completion list).
pub(crate) const KEYWORD_DESCRIPTIONS: &[(&str, &str)] = &[
    ("structure", "Declares a parametric structure."),
    (
        "param",
        "Declares an externally settable parameter with a type and default value.",
    ),
    (
        "let",
        "Declares a computed binding derived from other values.",
    ),
    (
        "constraint",
        "Declares a boolean constraint that must be satisfied.",
    ),
    ("sub", "Declares a sub-structure instantiation."),
    ("import", "Imports declarations from another module."),
    ("if", "Conditional expression."),
    ("then", "Then branch of a conditional."),
    ("else", "Else branch of a conditional."),
    ("and", "Logical AND operator."),
    ("or", "Logical OR operator."),
    ("not", "Logical NOT operator."),
    ("true", "Boolean literal true."),
    ("false", "Boolean literal false."),
    (
        "auto",
        "Marks a parameter for automatic resolution by the constraint solver.",
    ),
    (
        "occurrence",
        "Declares a concrete occurrence of a structure.",
    ),
    ("fn", "Declares a function."),
    ("trait", "Declares a trait."),
    ("enum", "Declares an enumeration type."),
    (
        "purpose",
        "Declares the optimization objective of the structure.",
    ),
    ("minimize", "Declares a quantity to minimize."),
    ("maximize", "Declares a quantity to maximize."),
    ("port", "Declares an interface port for connections."),
    ("connect", "Declares a connection between ports."),
    ("where", "Introduces additional type or value constraints."),
];

/// Return a brief description for Reify keywords.
fn keyword_description(word: &str) -> Option<&'static str> {
    KEYWORD_DESCRIPTIONS
        .iter()
        .find(|(kw, _)| *kw == word)
        .map(|(_, desc)| *desc)
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
    fn hover_on_money_param_shows_usd_in_type_string() {
        let source = "structure S {\n    param p: Money = auto\n}";
        // 'p' is on line 1: "    param p: Money = auto"
        // 4 spaces + "param " = 10 chars, so 'p' is at column 10
        let position = Position::new(1, 10); // on 'p'
        let md =
            hover_markdown(source, position).expect("hover should return info for Money param");
        assert!(
            md.contains("Scalar[USD]"),
            "type string should contain 'Scalar[USD]' (source-form via Type::Display), got: {md}"
        );
        assert!(
            !md.contains("Rational"),
            "type string must not contain 'Rational' (raw Debug), got: {md}"
        );
        assert!(
            !md.contains("DimensionVector("),
            "type string must not contain 'DimensionVector(' (raw Debug), got: {md}"
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

    // --- occurrence hover tests ---

    #[test]
    fn hover_on_occurrence_name_shows_occurrence_keyword() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        // 'Joint' starts after 'occurrence def ' = col 15
        let position = Position::new(0, 16);
        let md =
            hover_markdown(source, position).expect("hover should return info for occurrence name");
        assert!(
            md.contains("occurrence Joint"),
            "should show 'occurrence Joint', not 'structure Joint', got: {md}"
        );
    }

    #[test]
    fn hover_on_occurrence_member_shows_param_info() {
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        // 'diameter' on line 1, col 10
        let position = Position::new(1, 10);
        let md = hover_markdown(source, position)
            .expect("hover should return info for occurrence member");
        assert!(md.contains("param"), "should mention 'param', got: {md}");
        assert!(
            md.contains("diameter"),
            "should mention 'diameter', got: {md}"
        );
        assert!(md.contains("Scalar"), "should mention 'Scalar', got: {md}");
    }

    #[test]
    fn hover_on_documented_occurrence_shows_doc() {
        let source =
            "/// A joint process.\noccurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        // 'Joint' starts after 'occurrence def ' = col 15 on line 1
        let position = Position::new(1, 16);
        let md = hover_markdown(source, position)
            .expect("hover should return info for documented occurrence name");
        assert!(
            md.contains("occurrence Joint"),
            "should show 'occurrence Joint', got: {md}"
        );
        assert!(
            md.contains("A joint process."),
            "should contain doc comment, got: {md}"
        );
    }

    // --- guarded-group hover tests ---

    #[test]
    fn hover_on_structure_with_where_block_shows_correct_counts() {
        let source = r#"structure S {
    param a : Bool = true
    param b : Scalar = 1mm
    where a {
        param guarded_x : Scalar = 5mm
        let guarded_y = 2
    }
    constraint b > 0mm
}"#;
        // 'S' is on line 0 at col 10 (after 'structure ')
        let position = Position::new(0, 10);
        let md =
            hover_markdown(source, position).expect("hover should return info for structure S");
        assert!(
            md.contains("structure S"),
            "should mention 'structure S', got: {md}"
        );
        // Should show correct recursive counts: 3 params, 1 let, 1 constraint
        assert!(
            md.contains("3 params"),
            "should show 3 params (a, b, guarded_x), got: {md}"
        );
        assert!(
            md.contains("1 lets"),
            "should show 1 lets (guarded_y), got: {md}"
        );
        assert!(
            md.contains("1 constraints"),
            "should show 1 constraints, got: {md}"
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

    // --- cross-structure scoping tests ---

    /// Characterization test: hover intentionally does NOT fall back to unscoped
    /// member lookup when the scoped lookup returns None. This is the key
    /// difference from goto_def.rs, which does a two-pass search (scoped, then
    /// unscoped) as a navigation convenience. For hover, showing type/value info
    /// from a foreign structure's member would be misleading, because
    /// cross-structure member references are not valid in the Reify language.
    ///
    /// Here, `unique_a` only exists in structure A. Hovering on the text
    /// `unique_a` inside structure B should return None — it should NOT show
    /// A's member info via an unscoped fallback.
    #[test]
    fn hover_no_fallback_to_other_structure_member() {
        let source = "\
structure A {
    param unique_a: Scalar = 5mm
}
structure B {
    param unique_b: Scalar = 10mm
    let ref_a = unique_a
}";
        // 'unique_a' in 'let ref_a = unique_a' is on line 5, col 16
        let position = Position::new(5, 16);
        let result = hover_markdown(source, position);
        assert!(
            result.is_none(),
            "hover on 'unique_a' inside B should return None \
             (no fallback to A's member), got: {:?}",
            result
        );
    }

    #[test]
    fn hover_value_scoped_to_owning_declaration() {
        // Two structures with same-named param 'width' but different defaults.
        // Hover on 'width' inside B should show B's evaluated value (0.02 m),
        // NOT A's value (0.005 m). This tests the bug scenario where
        // compute_hover used templates.first() for value lookup.
        let source = "structure A {\n    param width: Scalar = 5mm\n}\nstructure B {\n    param width: Scalar = 20mm\n}";
        // 'width' inside B is on line 4, col 10
        let position = Position::new(4, 10);
        let md = hover_markdown(source, position).expect("hover should return info for width in B");
        assert!(
            md.contains("0.02 m"),
            "should show B's value (0.02 m), got: {md}"
        );
        assert!(
            !md.contains("0.005 m"),
            "should NOT show A's value (0.005 m), got: {md}"
        );
    }

    #[test]
    fn hover_value_for_member_reference_in_second_structure() {
        // Two structures with same-named param 'x', where B also has a let
        // that references 'x'. Hover on 'x' in the let expression within B
        // should show B's value (0.02 m), not A's value (0.005 m).
        // This tests that value scoping works for member references in expressions.
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Scalar = 20mm\n    let doubled = x * 2\n}";
        // 'x' in 'let doubled = x * 2' is on line 5, col 18
        let position = Position::new(5, 18);
        let md = hover_markdown(source, position)
            .expect("hover should return info for x reference in B");
        assert!(
            md.contains("0.02 m"),
            "should show B's value (0.02 m), got: {md}"
        );
        assert!(
            !md.contains("0.005 m"),
            "should NOT show A's value (0.005 m), got: {md}"
        );
    }

    #[test]
    fn hover_on_shared_member_in_second_structure() {
        // Two structures with identically-named member 'width' but different types.
        // Hover on 'width' inside B should show Bool, not Scalar.
        let source = "structure A {\n    param width: Scalar = 5mm\n}\nstructure B {\n    param width: Bool = true\n}";
        // 'width' inside B is on line 4, col 10
        let position = Position::new(4, 10);
        let md = hover_markdown(source, position).expect("hover should return info for width in B");
        assert!(
            md.contains("Bool"),
            "should show Bool type from structure B, got: {md}"
        );
        assert!(
            !md.contains("Scalar"),
            "should NOT show Scalar type from structure A, got: {md}"
        );
        assert!(
            md.contains("= true"),
            "should show B's Bool value (= true), got: {md}"
        );
        assert!(
            !md.contains("= 5mm"),
            "should NOT show A's Scalar value (= 5mm), got: {md}"
        );
    }

    // --- auto / auto(free) hover tests ---

    #[test]
    fn hover_on_bare_auto_param_shows_auto() {
        let source = "structure S {\n    param x: Scalar = auto\n}";
        let position = Position::new(1, 10); // on 'x'
        let md =
            hover_markdown(source, position).expect("hover should return info for auto param x");
        assert!(
            md.contains("auto x:"),
            "should contain 'auto x:', got: {md}"
        );
        assert!(
            !md.contains("auto(free)"),
            "should NOT contain 'auto(free)' for bare auto param, got: {md}"
        );
    }

    #[test]
    fn hover_on_auto_free_param_shows_auto_free() {
        let source = "structure S {\n    param x: Scalar = auto(free)\n}";
        let position = Position::new(1, 10); // on 'x'
        let md = hover_markdown(source, position)
            .expect("hover should return info for auto(free) param x");
        assert!(
            md.contains("auto(free) x:"),
            "should contain 'auto(free) x:', got: {md}"
        );
    }

    // --- keyword completeness coverage ---

    /// Verify that every keyword exposed by the completion engine also has a hover
    /// description, AND that every described keyword is still in a completion list.
    ///
    /// Forward check (completion → hover): adding a keyword to any completion list
    /// without a hover entry fails here.
    ///
    /// Reverse check (hover → completion): removing a keyword from every completion
    /// list while leaving its description in KEYWORD_DESCRIPTIONS fails here,
    /// preventing stale descriptions from silently accumulating.
    #[test]
    fn all_completion_keywords_have_hover_descriptions() {
        use std::collections::HashSet;

        use crate::completion::{BODY_KEYWORDS, EXPR_KEYWORDS, TOP_LEVEL_KEYWORDS};

        let completion_union: HashSet<&str> = TOP_LEVEL_KEYWORDS
            .iter()
            .chain(BODY_KEYWORDS.iter())
            .chain(EXPR_KEYWORDS.iter())
            .copied()
            .collect();

        // Forward: every completion keyword must have a description.
        let mut missing_desc: Vec<&str> = completion_union
            .iter()
            .copied()
            .filter(|kw| keyword_description(kw).is_none())
            .collect();
        missing_desc.sort_unstable();
        assert!(
            missing_desc.is_empty(),
            "keywords in completion lists but missing a hover description: {:?}",
            missing_desc
        );

        // Reverse: every described keyword must appear in at least one completion list.
        let mut stale: Vec<&str> = KEYWORD_DESCRIPTIONS
            .iter()
            .map(|(kw, _)| *kw)
            .filter(|kw| !completion_union.contains(kw))
            .collect();
        stale.sort_unstable();
        assert!(
            stale.is_empty(),
            "keywords in KEYWORD_DESCRIPTIONS but absent from all completion lists (stale): {:?}",
            stale
        );
    }

    // --- step-05: injectable hover core over a shared AnalysisContext ---

    /// `compute_hover_in_context`, fed a context built from a shared parse, must
    /// return output identical to the `compute_hover` wrapper — proving the
    /// cache-fed core path is output-equivalent to the per-request path.
    #[test]
    fn compute_hover_in_context_matches_wrapper() {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        let position = Position::new(1, 10); // on 'width'

        let parsed = std::sync::Arc::new(reify_compiler::parse_with_stdlib(
            source,
            reify_core::ModulePath::single("test"),
        ));
        let ctx = AnalysisContext::from_parsed(parsed);

        let via_context = compute_hover_in_context(&ctx, source, position);
        let via_wrapper = compute_hover(source, &uri, position);

        assert!(
            via_context.is_some(),
            "in-context hover should return info for 'width'"
        );
        assert_eq!(
            via_context, via_wrapper,
            "in-context hover must match the wrapper output"
        );
    }
}
