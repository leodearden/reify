//! Tree-sitter based parser for the Reify language.
//!
//! Parses source text into tree-sitter CST, then lowers to the `ParsedModule` AST.

use crate::*;
use reify_types::{ContentHash, ModulePath, SourceSpan};

/// Parse source text into a `ParsedModule` using tree-sitter.
pub fn parse(source: &str, module_path: ModulePath) -> ParsedModule {
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");

    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    let mut lowering = Lowering::new(source);
    lowering.lower_source_file(root);

    let content_hash = ContentHash::of_str(source);

    ParsedModule {
        path: module_path,
        declarations: lowering.declarations,
        errors: lowering.errors,
        content_hash,
    }
}

/// CST → AST lowering context.
struct Lowering<'a> {
    source: &'a str,
    declarations: Vec<Declaration>,
    errors: Vec<ParseError>,
}

impl<'a> Lowering<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            declarations: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Extract the source text for a node.
    fn node_text(&self, node: tree_sitter::Node) -> &'a str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    /// Create a SourceSpan from a tree-sitter node.
    fn span(&self, node: tree_sitter::Node) -> SourceSpan {
        SourceSpan::new(node.start_byte() as u32, node.end_byte() as u32)
    }

    /// Compute content hash for a node from its source text.
    fn content_hash(&self, node: tree_sitter::Node) -> ContentHash {
        ContentHash::of_str(self.node_text(node))
    }

    // ── Top-level lowering ──────────────────────────────────

    fn lower_source_file(&mut self, node: tree_sitter::Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "structure_definition" => {
                    if let Some(decl) = self.lower_structure(child) {
                        self.declarations.push(Declaration::Structure(decl));
                    }
                }
                "import_declaration" => {
                    if let Some(decl) = self.lower_import(child) {
                        self.declarations.push(Declaration::Import(decl));
                    }
                }
                "ERROR" => {
                    self.errors.push(ParseError {
                        message: format!("syntax error: {}", self.node_text(child)),
                        span: self.span(child),
                    });
                }
                _ => {}
            }
        }
    }

    fn lower_import(&self, node: tree_sitter::Node) -> Option<ImportDecl> {
        let mut cursor = node.walk();
        let mut path = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "string_literal" {
                let text = self.node_text(child);
                // Strip quotes
                path = Some(text[1..text.len() - 1].to_string());
            }
        }

        Some(ImportDecl {
            path: path?,
            span: self.span(node),
        })
    }

    fn lower_structure(&mut self, node: tree_sitter::Node) -> Option<StructureDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let mut members = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "param_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid param: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(p) = self.lower_param(child) {
                        members.push(MemberDecl::Param(p));
                    }
                }
                "let_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid let: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(l) = self.lower_let(child) {
                        members.push(MemberDecl::Let(l));
                    }
                }
                "constraint_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid constraint: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(c) = self.lower_constraint(child) {
                        members.push(MemberDecl::Constraint(c));
                    }
                }
                "sub_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid sub: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(s) = self.lower_sub(child) {
                        members.push(MemberDecl::Sub(s));
                    }
                }
                "minimize_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid minimize: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(m) = self.lower_minimize(child) {
                        members.push(MemberDecl::Minimize(m));
                    }
                }
                "maximize_declaration" => {
                    if child.is_error() || child.has_error() {
                        self.errors.push(ParseError {
                            message: format!("invalid maximize: {}", self.node_text(child)),
                            span: self.span(child),
                        });
                    } else if let Some(m) = self.lower_maximize(child) {
                        members.push(MemberDecl::Maximize(m));
                    }
                }
                "ERROR" => {
                    self.errors.push(ParseError {
                        message: format!("syntax error: {}", self.node_text(child)),
                        span: self.span(child),
                    });
                }
                _ => {}
            }
        }

        let content_hash = self.content_hash(node);

        Some(StructureDef {
            name,
            members,
            span: self.span(node),
            content_hash,
        })
    }

    // ── Where clause lowering ─────────────────────────────────

    fn lower_where_clause(&self, node: tree_sitter::Node) -> Option<WhereClause> {
        // Find the where_clause child node within a member declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "where_clause" {
                let condition_node = child.child_by_field_name("condition")?;
                let condition = self.lower_expr(condition_node)?;
                return Some(WhereClause {
                    condition,
                    span: self.span(child),
                });
            }
        }
        None
    }

    // ── Member lowering ─────────────────────────────────────

    fn lower_param(&self, node: tree_sitter::Node) -> Option<ParamDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_expr = node.child_by_field_name("type").map(|t| {
            // type_expr wraps an identifier
            let ident = if t.kind() == "type_expr" {
                t.child(0).unwrap_or(t)
            } else {
                t
            };
            TypeExpr {
                name: self.node_text(ident).to_string(),
                span: self.span(ident),
            }
        });

        let default = node.child_by_field_name("default")
            .and_then(|d| {
                if d.kind() == "auto_keyword" {
                    Some(Expr { kind: ExprKind::Auto, span: self.span(d) })
                } else {
                    self.lower_expr(d)
                }
            });

        let where_clause = self.lower_where_clause(node);

        Some(ParamDecl {
            name,
            type_expr,
            default,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_let(&self, node: tree_sitter::Node) -> Option<LetDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_expr = node.child_by_field_name("type").map(|t| {
            let ident = if t.kind() == "type_expr" {
                t.child(0).unwrap_or(t)
            } else {
                t
            };
            TypeExpr {
                name: self.node_text(ident).to_string(),
                span: self.span(ident),
            }
        });

        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_expr(value_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(LetDecl {
            name,
            type_expr,
            value,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_constraint(&self, node: tree_sitter::Node) -> Option<ConstraintDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(ConstraintDecl {
            label: None,
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_minimize(&self, node: tree_sitter::Node) -> Option<MinimizeDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(MinimizeDecl {
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_maximize(&self, node: tree_sitter::Node) -> Option<MaximizeDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(MaximizeDecl {
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_sub(&self, node: tree_sitter::Node) -> Option<SubDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let struct_node = node.child_by_field_name("structure_name")?;
        let structure_name = self.node_text(struct_node).to_string();

        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "named_argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if arg_child.kind() == "named_argument"
                        && let Some(pair) = self.lower_named_arg(arg_child)
                    {
                        args.push(pair);
                    }
                }
            }
        }

        let where_clause = self.lower_where_clause(node);

        Some(SubDecl {
            name,
            structure_name,
            args,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_named_arg(&self, node: tree_sitter::Node) -> Option<(String, Expr)> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();
        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_expr(value_node)?;
        Some((name, value))
    }

    // ── Expression lowering ─────────────────────────────────

    fn lower_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        match node.kind() {
            "binary_expression" => self.lower_binary_expr(node),
            "unary_expression" => self.lower_unary_expr(node),
            "conditional_expression" => self.lower_conditional(node),
            "quantity_literal" => self.lower_quantity_literal(node),
            "number_literal" => self.lower_number_literal(node),
            "string_literal" => self.lower_string_literal(node),
            "bool_literal" => self.lower_bool_literal(node),
            "identifier" => self.lower_identifier(node),
            "function_call" => self.lower_function_call(node),
            "member_access" => self.lower_member_access(node),
            "parenthesized_expression" => {
                // Unwrap parenthesized expression — find the inner expression
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "(" && child.kind() != ")" {
                        return self.lower_expr(child);
                    }
                }
                None
            }
            _ => {
                // Fallback: try to lower as an expression if it's a named node
                if node.is_named() {
                    // Unknown named node — skip
                    None
                } else {
                    None
                }
            }
        }
    }

    fn lower_binary_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        let left_node = node.child_by_field_name("left")?;
        let op_node = node.child_by_field_name("op")?;
        let right_node = node.child_by_field_name("right")?;

        let left = self.lower_expr(left_node)?;
        let right = self.lower_expr(right_node)?;
        let op = self.node_text(op_node).to_string();

        Some(Expr {
            kind: ExprKind::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span: self.span(node),
        })
    }

    fn lower_unary_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        let op_node = node.child_by_field_name("op")?;
        let operand_node = node.child_by_field_name("operand")?;

        let op = self.node_text(op_node).to_string();
        let operand = self.lower_expr(operand_node)?;

        Some(Expr {
            kind: ExprKind::UnOp {
                op,
                operand: Box::new(operand),
            },
            span: self.span(node),
        })
    }

    fn lower_conditional(&self, node: tree_sitter::Node) -> Option<Expr> {
        let condition_node = node.child_by_field_name("condition")?;
        let then_node = node.child_by_field_name("then")?;
        let else_node = node.child_by_field_name("else")?;

        let condition = self.lower_expr(condition_node)?;
        let then_branch = self.lower_expr(then_node)?;
        let else_branch = self.lower_expr(else_node)?;

        Some(Expr {
            kind: ExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span: self.span(node),
        })
    }

    fn lower_quantity_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value_node = node.child_by_field_name("value")?;
        let unit_node = node.child_by_field_name("unit")?;

        let value: f64 = self.node_text(value_node).parse().ok()?;
        let unit = self.node_text(unit_node).to_string();

        Some(Expr {
            kind: ExprKind::QuantityLiteral { value, unit },
            span: self.span(node),
        })
    }

    fn lower_number_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value: f64 = self.node_text(node).parse().ok()?;
        Some(Expr {
            kind: ExprKind::NumberLiteral(value),
            span: self.span(node),
        })
    }

    fn lower_string_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let text = self.node_text(node);
        // Strip quotes
        let s = text[1..text.len() - 1].to_string();
        Some(Expr {
            kind: ExprKind::StringLiteral(s),
            span: self.span(node),
        })
    }

    fn lower_bool_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value = match self.node_text(node) {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        Some(Expr {
            kind: ExprKind::BoolLiteral(value),
            span: self.span(node),
        })
    }

    fn lower_identifier(&self, node: tree_sitter::Node) -> Option<Expr> {
        let name = self.node_text(node).to_string();
        Some(Expr {
            kind: ExprKind::Ident(name),
            span: self.span(node),
        })
    }

    fn lower_function_call(&self, node: tree_sitter::Node) -> Option<Expr> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if arg_child.is_named()
                        && let Some(expr) = self.lower_expr(arg_child)
                    {
                        args.push(expr);
                    }
                }
            }
        }

        Some(Expr {
            kind: ExprKind::FunctionCall { name, args },
            span: self.span(node),
        })
    }

    fn lower_member_access(&self, node: tree_sitter::Node) -> Option<Expr> {
        let object_node = node.child_by_field_name("object")?;
        let member_node = node.child_by_field_name("member")?;

        let object = self.lower_expr(object_node)?;
        let member = self.node_text(member_node).to_string();

        Some(Expr {
            kind: ExprKind::MemberAccess {
                object: Box::new(object),
                member,
            },
            span: self.span(node),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: count ERROR nodes in a tree-sitter tree.
    fn count_errors(node: tree_sitter::Node) -> usize {
        let mut count = if node.is_error() || node.is_missing() { 1 } else { 0 };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            count += count_errors(child);
        }
        count
    }

    fn parse_bracket() -> ParsedModule {
        let source = reify_test_support::bracket_source();
        parse(source, reify_types::ModulePath::single("bracket"))
    }

    #[test]
    fn ts_parse_produces_correct_structure() {
        let module = parse_bracket();
        assert!(module.errors.is_empty(), "expected no errors: {:?}", module.errors);
        assert_eq!(module.declarations.len(), 1);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        assert_eq!(structure.name, "Bracket");
        assert_eq!(structure.members.len(), 10);

        let params: Vec<_> = structure.members.iter()
            .filter(|m| matches!(m, MemberDecl::Param(_)))
            .collect();
        let lets: Vec<_> = structure.members.iter()
            .filter(|m| matches!(m, MemberDecl::Let(_)))
            .collect();
        let constraints: Vec<_> = structure.members.iter()
            .filter(|m| matches!(m, MemberDecl::Constraint(_)))
            .collect();

        assert_eq!(params.len(), 5, "expected 5 params");
        assert_eq!(lets.len(), 2, "expected 2 lets");
        assert_eq!(constraints.len(), 3, "expected 3 constraints");

        // Verify member names in order
        let names: Vec<String> = structure.members.iter().map(|m| match m {
            MemberDecl::Param(p) => format!("param:{}", p.name),
            MemberDecl::Let(l) => format!("let:{}", l.name),
            MemberDecl::Constraint(_) => "constraint".into(),
            MemberDecl::Sub(s) => format!("sub:{}", s.name),
            MemberDecl::Minimize(_) => "minimize".into(),
            MemberDecl::Maximize(_) => "maximize".into(),
            MemberDecl::GuardedGroup(_) => "guarded_group".into(),
        }).collect();
        assert_eq!(names, vec![
            "param:width", "param:height", "param:thickness",
            "param:fillet_radius", "param:hole_diameter",
            "let:volume",
            "constraint", "constraint", "constraint",
            "let:body",
        ]);
    }

    /// Helper to get structure members from bracket parse.
    fn bracket_members() -> Vec<MemberDecl> {
        let module = parse_bracket();
        match module.declarations.into_iter().next().unwrap() {
            Declaration::Structure(s) => s.members,
            _ => panic!("expected Structure"),
        }
    }

    #[test]
    fn quantity_literal_80mm() {
        let members = bracket_members();
        let width = match &members[0] {
            MemberDecl::Param(p) => p,
            _ => panic!("expected Param"),
        };
        assert_eq!(width.name, "width");
        match &width.default.as_ref().unwrap().kind {
            ExprKind::QuantityLiteral { value, unit } => {
                assert!((value - 80.0).abs() < f64::EPSILON);
                assert_eq!(unit, "mm");
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        }
    }

    #[test]
    fn number_literal_4() {
        // In `constraint thickness < width / 4`, the `4` is a number literal
        let members = bracket_members();
        // constraints[1] is `constraint thickness < width / 4`
        let constraint = match &members[7] {
            MemberDecl::Constraint(c) => c,
            _ => panic!("expected Constraint"),
        };
        // expr is `thickness < width / 4`
        match &constraint.expr.kind {
            ExprKind::BinOp { right, .. } => {
                // right is `width / 4`
                match &right.kind {
                    ExprKind::BinOp { right: inner_right, .. } => {
                        match &inner_right.kind {
                            ExprKind::NumberLiteral(v) => {
                                assert!((v - 4.0).abs() < f64::EPSILON);
                            }
                            other => panic!("expected NumberLiteral(4), got {:?}", other),
                        }
                    }
                    other => panic!("expected BinOp, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn function_call_box() {
        let members = bracket_members();
        // Last member: `let body = box(width, height, thickness)`
        let body = match &members[9] {
            MemberDecl::Let(l) => l,
            _ => panic!("expected Let"),
        };
        assert_eq!(body.name, "body");
        match &body.value.kind {
            ExprKind::FunctionCall { name, args } => {
                assert_eq!(name, "box");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, ExprKind::Ident(n) if n == "width"));
                assert!(matches!(&args[1].kind, ExprKind::Ident(n) if n == "height"));
                assert!(matches!(&args[2].kind, ExprKind::Ident(n) if n == "thickness"));
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn binary_ops_left_associative() {
        let members = bracket_members();
        // `let volume = width * height * thickness`
        let volume = match &members[5] {
            MemberDecl::Let(l) => l,
            _ => panic!("expected Let"),
        };
        assert_eq!(volume.name, "volume");
        // Should be ((width * height) * thickness)
        match &volume.value.kind {
            ExprKind::BinOp { op, left, right } => {
                assert_eq!(op, "*");
                // right is "thickness"
                assert!(matches!(&right.kind, ExprKind::Ident(n) if n == "thickness"));
                // left is (width * height)
                match &left.kind {
                    ExprKind::BinOp { op: inner_op, left: ll, right: lr } => {
                        assert_eq!(inner_op, "*");
                        assert!(matches!(&ll.kind, ExprKind::Ident(n) if n == "width"));
                        assert!(matches!(&lr.kind, ExprKind::Ident(n) if n == "height"));
                    }
                    other => panic!("expected inner BinOp, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn comparison_with_quantity() {
        let members = bracket_members();
        // `constraint thickness > 2mm`
        let constraint = match &members[6] {
            MemberDecl::Constraint(c) => c,
            _ => panic!("expected Constraint"),
        };
        match &constraint.expr.kind {
            ExprKind::BinOp { op, left, right } => {
                assert_eq!(op, ">");
                assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "thickness"));
                match &right.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 2.0).abs() < f64::EPSILON);
                        assert_eq!(unit, "mm");
                    }
                    other => panic!("expected QuantityLiteral, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn spans_are_valid_and_cover_source_text() {
        let source = reify_test_support::bracket_source();
        let module = parse(source, reify_types::ModulePath::single("bracket"));

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure spans entire source
        assert_eq!(structure.span.start, 0);
        assert_eq!(structure.span.end as usize, source.len());

        // All member spans are non-empty, within source, and contain expected keywords
        for (i, m) in structure.members.iter().enumerate() {
            let span = match m {
                MemberDecl::Param(p) => p.span,
                MemberDecl::Let(l) => l.span,
                MemberDecl::Constraint(c) => c.span,
                MemberDecl::Sub(s) => s.span,
                MemberDecl::Minimize(m) => m.span,
                MemberDecl::Maximize(m) => m.span,
                MemberDecl::GuardedGroup(g) => g.span,
            };
            assert!(span.start < span.end, "member {} span empty", i);
            assert!((span.end as usize) <= source.len(), "member {} span overflows", i);

            let text = &source[span.start as usize..span.end as usize];
            match m {
                MemberDecl::Param(p) => {
                    assert!(text.starts_with("param"), "param member {} text: {:?}", i, text);
                    assert!(text.contains(&p.name), "param {} name in text", i);
                }
                MemberDecl::Let(l) => {
                    assert!(text.starts_with("let"), "let member {} text: {:?}", i, text);
                    assert!(text.contains(&l.name), "let {} name in text", i);
                }
                MemberDecl::Constraint(_) => {
                    assert!(text.starts_with("constraint"), "constraint member {} text: {:?}", i, text);
                }
                MemberDecl::Sub(s) => {
                    assert!(text.starts_with("sub"), "sub member {} text: {:?}", i, text);
                    assert!(text.contains(&s.name), "sub {} name in text", i);
                }
                MemberDecl::Minimize(_) => {
                    assert!(text.starts_with("minimize"), "minimize member {} text: {:?}", i, text);
                }
                MemberDecl::Maximize(_) => {
                    assert!(text.starts_with("maximize"), "maximize member {} text: {:?}", i, text);
                }
                MemberDecl::GuardedGroup(_) => {
                    assert!(text.starts_with("where"), "guarded_group member {} text: {:?}", i, text);
                }
            }
        }

        // Expression spans are valid
        if let MemberDecl::Param(p) = &structure.members[0] {
            let def_span = p.default.as_ref().unwrap().span;
            let def_text = &source[def_span.start as usize..def_span.end as usize];
            assert_eq!(def_text, "80mm", "width default text");

            let ty_span = p.type_expr.as_ref().unwrap().span;
            let ty_text = &source[ty_span.start as usize..ty_span.end as usize];
            assert_eq!(ty_text, "Scalar", "width type text");
        }
    }

    #[test]
    fn content_hashes_computed_from_source_text() {
        let source = reify_test_support::bracket_source();
        let module = parse(source, reify_types::ModulePath::single("bracket"));

        // Module content hash = hash of entire source
        assert_eq!(module.content_hash, ContentHash::of_str(source), "module hash");

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure content hash = hash of structure node's source text (not entire file)
        assert_ne!(structure.content_hash, ContentHash(0), "structure hash should be non-zero");

        // Each member content hash = hash of its source text slice
        for (i, m) in structure.members.iter().enumerate() {
            let (span, hash) = match m {
                MemberDecl::Param(p) => (p.span, p.content_hash),
                MemberDecl::Let(l) => (l.span, l.content_hash),
                MemberDecl::Constraint(c) => (c.span, c.content_hash),
                MemberDecl::Sub(s) => (s.span, s.content_hash),
                MemberDecl::Minimize(m) => (m.span, m.content_hash),
                MemberDecl::Maximize(m) => (m.span, m.content_hash),
                MemberDecl::GuardedGroup(g) => (g.span, g.content_hash),
            };
            let text = &source[span.start as usize..span.end as usize];
            assert_eq!(hash, ContentHash::of_str(text), "member {} hash from source text", i);
        }

        // All param hashes should be unique
        let param_hashes: Vec<ContentHash> = structure.members.iter()
            .filter_map(|m| match m {
                MemberDecl::Param(p) => Some(p.content_hash),
                _ => None,
            })
            .collect();
        for (i, h1) in param_hashes.iter().enumerate() {
            for (j, h2) in param_hashes.iter().enumerate() {
                if i != j {
                    assert_ne!(h1, h2, "params {} and {} have same hash", i, j);
                }
            }
        }
    }

    #[test]
    fn error_recovery_partial_parse() {
        let source = r#"structure Broken {
    param width: Scalar = 80mm
    param !!!invalid!!!
    param height: Scalar = 100mm
}"#;
        let module = parse(source, reify_types::ModulePath::single("broken"));

        // Should have parse errors
        assert!(!module.errors.is_empty(), "expected errors for malformed input");

        // Should also have recovered declarations
        assert!(!module.declarations.is_empty(), "expected partial declarations");

        if let Declaration::Structure(s) = &module.declarations[0] {
            assert_eq!(s.name, "Broken");
            // Should have at least some valid members (width and/or height)
            let valid_params: Vec<_> = s.members.iter()
                .filter_map(|m| match m {
                    MemberDecl::Param(p) => Some(&p.name),
                    _ => None,
                })
                .collect();
            assert!(
                !valid_params.is_empty(),
                "expected at least some valid params, got none"
            );
        } else {
            panic!("expected Structure declaration");
        }
    }

    #[test]
    fn parse_deterministic() {
        // Parsing the same source twice produces identical output.
        let source = reify_test_support::bracket_source();
        let m1 = parse(source, reify_types::ModulePath::single("bracket"));
        let m2 = parse(source, reify_types::ModulePath::single("bracket"));

        assert_eq!(m1.content_hash, m2.content_hash);
        assert_eq!(m1.declarations.len(), m2.declarations.len());
        assert_eq!(m1.errors.len(), m2.errors.len());

        let s1 = match &m1.declarations[0] { Declaration::Structure(s) => s, _ => panic!() };
        let s2 = match &m2.declarations[0] { Declaration::Structure(s) => s, _ => panic!() };

        assert_eq!(s1.name, s2.name);
        assert_eq!(s1.span, s2.span);
        assert_eq!(s1.content_hash, s2.content_hash);
        assert_eq!(s1.members.len(), s2.members.len());

        for (i, (m_a, m_b)) in s1.members.iter().zip(s2.members.iter()).enumerate() {
            let (hash_a, span_a) = match m_a {
                MemberDecl::Param(p) => (p.content_hash, p.span),
                MemberDecl::Let(l) => (l.content_hash, l.span),
                MemberDecl::Constraint(c) => (c.content_hash, c.span),
                MemberDecl::Sub(s) => (s.content_hash, s.span),
                MemberDecl::Minimize(m) => (m.content_hash, m.span),
                MemberDecl::Maximize(m) => (m.content_hash, m.span),
                MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
            };
            let (hash_b, span_b) = match m_b {
                MemberDecl::Param(p) => (p.content_hash, p.span),
                MemberDecl::Let(l) => (l.content_hash, l.span),
                MemberDecl::Constraint(c) => (c.content_hash, c.span),
                MemberDecl::Sub(s) => (s.content_hash, s.span),
                MemberDecl::Minimize(m) => (m.content_hash, m.span),
                MemberDecl::Maximize(m) => (m.content_hash, m.span),
                MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
            };
            assert_eq!(hash_a, hash_b, "member {} hash determinism", i);
            assert_eq!(span_a, span_b, "member {} span determinism", i);
        }
    }

    #[test]
    fn parse_minimize_declaration() {
        let source = r#"structure S {
    param volume: Scalar = 100mm
    minimize volume
}"#;
        let module = parse(source, reify_types::ModulePath::single("test_min"));
        assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        // Should have 2 members: param + minimize
        assert_eq!(structure.members.len(), 2);

        match &structure.members[1] {
            MemberDecl::Minimize(m) => {
                assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "volume"));
            }
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn parse_maximize_declaration() {
        let source = r#"structure S {
    param thickness: Scalar = 5mm
    maximize thickness
}"#;
        let module = parse(source, reify_types::ModulePath::single("test_max"));
        assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        assert_eq!(structure.members.len(), 2);

        match &structure.members[1] {
            MemberDecl::Maximize(m) => {
                assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "thickness"));
            }
            other => panic!("expected Maximize, got {:?}", other),
        }
    }

    #[test]
    fn parse_minimize_complex_expression() {
        let source = r#"structure S {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    minimize width * height
}"#;
        let module = parse(source, reify_types::ModulePath::single("test_min_complex"));
        assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        match &structure.members[2] {
            MemberDecl::Minimize(m) => {
                match &m.expr.kind {
                    ExprKind::BinOp { op, .. } => assert_eq!(op, "*"),
                    other => panic!("expected BinOp(*), got {:?}", other),
                }
            }
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn parse_minimize_with_other_members() {
        let source = r#"structure S {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    let vol = w * h
    constraint w > 0mm
    minimize w
}"#;
        let module = parse(source, reify_types::ModulePath::single("test_min_mixed"));
        assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        // 2 params + 1 let + 1 constraint + 1 minimize = 5 members
        assert_eq!(structure.members.len(), 5);

        // Verify minimize is present alongside other members
        assert!(
            structure.members.iter().any(|m| matches!(m, MemberDecl::Minimize(_))),
            "should contain a Minimize member"
        );
        assert!(
            structure.members.iter().any(|m| matches!(m, MemberDecl::Constraint(_))),
            "should contain a Constraint member"
        );
    }

    #[test]
    fn minimize_span_and_hash() {
        let source = r#"structure S {
    param x: Scalar = 5mm
    minimize x
}"#;
        let module = parse(source, reify_types::ModulePath::single("test_min_span"));
        assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        match &structure.members[1] {
            MemberDecl::Minimize(m) => {
                // Span should cover the full "minimize x" text
                let text = &source[m.span.start as usize..m.span.end as usize];
                assert!(text.starts_with("minimize"), "span text: {:?}", text);
                assert!(text.contains("x"), "span text should contain 'x': {:?}", text);

                // Content hash should match the source text of the node
                assert_eq!(
                    m.content_hash,
                    reify_types::ContentHash::of_str(text),
                    "content_hash should match source text"
                );
            }
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn tree_sitter_parses_bracket_source_without_errors() {
        let source = reify_test_support::bracket_source();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");

        let tree = parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        assert_eq!(root.kind(), "source_file");
        assert_eq!(
            count_errors(root),
            0,
            "Expected zero ERROR nodes, got tree:\n{}",
            root.to_sexp()
        );
    }
}
