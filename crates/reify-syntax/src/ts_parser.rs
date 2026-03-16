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
                "ERROR" => {
                    self.errors.push(ParseError {
                        message: format!("syntax error: {}", self.node_text(child)),
                        span: self.span(child),
                    });
                }
                _ => {}
            }
        }

        let content_hash = ContentHash::of_str(self.source);

        Some(StructureDef {
            name,
            members,
            span: self.span(node),
            content_hash,
        })
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
            .and_then(|d| self.lower_expr(d));

        Some(ParamDecl {
            name,
            type_expr,
            default,
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

        Some(LetDecl {
            name,
            type_expr,
            value,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_constraint(&self, node: tree_sitter::Node) -> Option<ConstraintDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        Some(ConstraintDecl {
            label: None,
            expr,
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
                    if arg_child.kind() == "named_argument" {
                        if let Some(pair) = self.lower_named_arg(arg_child) {
                            args.push(pair);
                        }
                    }
                }
            }
        }

        Some(SubDecl {
            name,
            structure_name,
            args,
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
                    if arg_child.is_named() {
                        if let Some(expr) = self.lower_expr(arg_child) {
                            args.push(expr);
                        }
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
    fn spans_match_hand_written_parser() {
        // Compare tree-sitter parser output against the hand-written parser.
        // Note: the bracket_parsed_module() fixture has stale span values from
        // a prior version of bracket_source(). The hand-written parser is the
        // ground truth for correct span computation.
        let source = reify_test_support::bracket_source();
        let hw = crate::parser::parse(source, reify_types::ModulePath::single("bracket"));
        let ts = parse(source, reify_types::ModulePath::single("bracket"));

        let hw_struct = match &hw.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };
        let ts_struct = match &ts.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure span
        assert_eq!(ts_struct.span, hw_struct.span, "structure span mismatch");

        // Each member span
        assert_eq!(ts_struct.members.len(), hw_struct.members.len());
        for (i, (ts_m, hw_m)) in ts_struct.members.iter()
            .zip(hw_struct.members.iter())
            .enumerate()
        {
            let ts_span = match ts_m {
                MemberDecl::Param(p) => p.span,
                MemberDecl::Let(l) => l.span,
                MemberDecl::Constraint(c) => c.span,
                MemberDecl::Sub(s) => s.span,
            };
            let hw_span = match hw_m {
                MemberDecl::Param(p) => p.span,
                MemberDecl::Let(l) => l.span,
                MemberDecl::Constraint(c) => c.span,
                MemberDecl::Sub(s) => s.span,
            };
            assert_eq!(ts_span, hw_span, "member {} span mismatch", i);
        }

        // Key expression spans — verify they're non-empty and match
        if let (MemberDecl::Param(ts_p), MemberDecl::Param(hw_p)) =
            (&ts_struct.members[0], &hw_struct.members[0])
        {
            let ts_def = ts_p.default.as_ref().unwrap();
            let hw_def = hw_p.default.as_ref().unwrap();
            assert_eq!(ts_def.span, hw_def.span, "width default expr span");

            let ts_ty = ts_p.type_expr.as_ref().unwrap();
            let hw_ty = hw_p.type_expr.as_ref().unwrap();
            assert_eq!(ts_ty.span, hw_ty.span, "width type expr span");
        }

        // Volume expression tree spans
        if let (MemberDecl::Let(ts_l), MemberDecl::Let(hw_l)) =
            (&ts_struct.members[5], &hw_struct.members[5])
        {
            assert_eq!(ts_l.value.span, hw_l.value.span, "volume expr span");
        }

        // Body function call span
        if let (MemberDecl::Let(ts_l), MemberDecl::Let(hw_l)) =
            (&ts_struct.members[9], &hw_struct.members[9])
        {
            assert_eq!(ts_l.value.span, hw_l.value.span, "body expr span");
        }
    }

    #[test]
    fn content_hashes_match_hand_written_parser() {
        let source = reify_test_support::bracket_source();
        let hw = crate::parser::parse(source, reify_types::ModulePath::single("bracket"));
        let ts = parse(source, reify_types::ModulePath::single("bracket"));

        // Module content hash
        assert_eq!(ts.content_hash, hw.content_hash, "module content hash");
        assert_eq!(ts.content_hash, ContentHash::of_str(source), "module hash = hash of source");

        let hw_struct = match &hw.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };
        let ts_struct = match &ts.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure content hash
        assert_eq!(ts_struct.content_hash, hw_struct.content_hash, "structure content hash");

        // All member content hashes
        for (i, (ts_m, hw_m)) in ts_struct.members.iter()
            .zip(hw_struct.members.iter())
            .enumerate()
        {
            let ts_hash = match ts_m {
                MemberDecl::Param(p) => p.content_hash,
                MemberDecl::Let(l) => l.content_hash,
                MemberDecl::Constraint(c) => c.content_hash,
                MemberDecl::Sub(s) => s.content_hash,
            };
            let hw_hash = match hw_m {
                MemberDecl::Param(p) => p.content_hash,
                MemberDecl::Let(l) => l.content_hash,
                MemberDecl::Constraint(c) => c.content_hash,
                MemberDecl::Sub(s) => s.content_hash,
            };
            assert_eq!(ts_hash, hw_hash, "member {} content hash mismatch", i);
        }

        // All param hashes should be unique
        let param_hashes: Vec<ContentHash> = ts_struct.members.iter()
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
    fn full_integration_matches_hand_written_parser() {
        // Comprehensive structural comparison between tree-sitter and hand-written parser.
        let source = reify_test_support::bracket_source();
        let hw = crate::parser::parse(source, reify_types::ModulePath::single("bracket"));
        let ts = parse(source, reify_types::ModulePath::single("bracket"));

        // Module level
        assert_eq!(ts.content_hash, hw.content_hash);
        assert_eq!(ts.errors.len(), hw.errors.len());
        assert_eq!(ts.declarations.len(), hw.declarations.len());

        let hw_s = match &hw.declarations[0] { Declaration::Structure(s) => s, _ => panic!() };
        let ts_s = match &ts.declarations[0] { Declaration::Structure(s) => s, _ => panic!() };

        assert_eq!(ts_s.name, hw_s.name);
        assert_eq!(ts_s.span, hw_s.span);
        assert_eq!(ts_s.content_hash, hw_s.content_hash);
        assert_eq!(ts_s.members.len(), hw_s.members.len());

        // Compare each member deeply
        for (i, (ts_m, hw_m)) in ts_s.members.iter().zip(hw_s.members.iter()).enumerate() {
            match (ts_m, hw_m) {
                (MemberDecl::Param(ts_p), MemberDecl::Param(hw_p)) => {
                    assert_eq!(ts_p.name, hw_p.name, "param {} name", i);
                    assert_eq!(ts_p.span, hw_p.span, "param {} span", i);
                    assert_eq!(ts_p.content_hash, hw_p.content_hash, "param {} hash", i);
                    // Type expr
                    match (&ts_p.type_expr, &hw_p.type_expr) {
                        (Some(ts_t), Some(hw_t)) => {
                            assert_eq!(ts_t.name, hw_t.name, "param {} type name", i);
                            assert_eq!(ts_t.span, hw_t.span, "param {} type span", i);
                        }
                        (None, None) => {}
                        _ => panic!("param {} type_expr mismatch", i),
                    }
                    // Default expr kind
                    match (&ts_p.default, &hw_p.default) {
                        (Some(ts_d), Some(hw_d)) => {
                            assert_eq!(ts_d.span, hw_d.span, "param {} default span", i);
                            assert_expr_kind_eq(&ts_d.kind, &hw_d.kind, &format!("param {} default", i));
                        }
                        (None, None) => {}
                        _ => panic!("param {} default mismatch", i),
                    }
                }
                (MemberDecl::Let(ts_l), MemberDecl::Let(hw_l)) => {
                    assert_eq!(ts_l.name, hw_l.name, "let {} name", i);
                    assert_eq!(ts_l.span, hw_l.span, "let {} span", i);
                    assert_eq!(ts_l.content_hash, hw_l.content_hash, "let {} hash", i);
                    assert_eq!(ts_l.value.span, hw_l.value.span, "let {} value span", i);
                    assert_expr_kind_eq(&ts_l.value.kind, &hw_l.value.kind, &format!("let {}", i));
                }
                (MemberDecl::Constraint(ts_c), MemberDecl::Constraint(hw_c)) => {
                    assert_eq!(ts_c.label, hw_c.label, "constraint {} label", i);
                    assert_eq!(ts_c.span, hw_c.span, "constraint {} span", i);
                    assert_eq!(ts_c.content_hash, hw_c.content_hash, "constraint {} hash", i);
                    assert_eq!(ts_c.expr.span, hw_c.expr.span, "constraint {} expr span", i);
                    assert_expr_kind_eq(&ts_c.expr.kind, &hw_c.expr.kind, &format!("constraint {}", i));
                }
                _ => panic!("member {} kind mismatch: {:?} vs {:?}", i, ts_m, hw_m),
            }
        }
    }

    /// Recursively compare expression kinds.
    fn assert_expr_kind_eq(actual: &ExprKind, expected: &ExprKind, ctx: &str) {
        match (actual, expected) {
            (ExprKind::NumberLiteral(a), ExprKind::NumberLiteral(b)) => {
                assert!((a - b).abs() < f64::EPSILON, "{}: num {} != {}", ctx, a, b);
            }
            (ExprKind::QuantityLiteral { value: av, unit: au },
             ExprKind::QuantityLiteral { value: bv, unit: bu }) => {
                assert!((av - bv).abs() < f64::EPSILON, "{}: qty value", ctx);
                assert_eq!(au, bu, "{}: qty unit", ctx);
            }
            (ExprKind::Ident(a), ExprKind::Ident(b)) => {
                assert_eq!(a, b, "{}: ident", ctx);
            }
            (ExprKind::BinOp { op: ao, left: al, right: ar },
             ExprKind::BinOp { op: bo, left: bl, right: br }) => {
                assert_eq!(ao, bo, "{}: binop op", ctx);
                assert_expr_kind_eq(&al.kind, &bl.kind, &format!("{}/left", ctx));
                assert_expr_kind_eq(&ar.kind, &br.kind, &format!("{}/right", ctx));
            }
            (ExprKind::FunctionCall { name: an, args: aa },
             ExprKind::FunctionCall { name: bn, args: ba }) => {
                assert_eq!(an, bn, "{}: fn name", ctx);
                assert_eq!(aa.len(), ba.len(), "{}: fn arg count", ctx);
                for (j, (a, b)) in aa.iter().zip(ba.iter()).enumerate() {
                    assert_expr_kind_eq(&a.kind, &b.kind, &format!("{}/arg{}", ctx, j));
                }
            }
            _ => {
                assert_eq!(
                    format!("{:?}", actual), format!("{:?}", expected),
                    "{}: expr kind mismatch", ctx
                );
            }
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
