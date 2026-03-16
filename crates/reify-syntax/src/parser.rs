//! Hand-written recursive descent parser for the M1 subset of Reify.
//!
//! Handles: module declarations (optional), structure definitions, param/let/constraint/sub
//! members, and basic expressions (arithmetic, comparisons, quantity literals, function calls,
//! member access).

use reify_types::{ContentHash, ModulePath, SourceSpan};

use crate::{
    ConstraintDecl, Declaration, Expr, ExprKind, LetDecl, MemberDecl, ParamDecl, ParseError,
    ParsedModule, StructureDef, SubDecl, TypeExpr,
};

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    // Literals
    Number(f64),
    StringLit(String),
    // Identifiers & keywords
    Ident(String),
    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Colon,
    Comma,
    Dot,
    Eq,
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    Ne,
    And,
    Or,
    Bang,
    // Special
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: SourceSpan,
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn tokenize(mut self) -> Vec<Token> {
        while self.pos < self.bytes.len() {
            self.skip_spaces();
            if self.pos >= self.bytes.len() {
                break;
            }

            let ch = self.bytes[self.pos];

            match ch {
                b'\n' => {
                    let start = self.pos;
                    self.pos += 1;
                    self.tokens.push(Token {
                        kind: TokenKind::Newline,
                        span: SourceSpan::new(start as u32, self.pos as u32),
                    });
                }
                b'#' => {
                    // Line comment
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                b'(' => self.single_char(TokenKind::LParen),
                b')' => self.single_char(TokenKind::RParen),
                b'{' => self.single_char(TokenKind::LBrace),
                b'}' => self.single_char(TokenKind::RBrace),
                b':' => self.single_char(TokenKind::Colon),
                b',' => self.single_char(TokenKind::Comma),
                b'.' => self.single_char(TokenKind::Dot),
                b'+' => self.single_char(TokenKind::Plus),
                b'-' => self.single_char(TokenKind::Minus),
                b'*' => self.single_char(TokenKind::Star),
                b'/' => self.single_char(TokenKind::Slash),
                b'=' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                        self.pos += 1;
                        self.tokens.push(Token {
                            kind: TokenKind::EqEq,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    } else {
                        self.tokens.push(Token {
                            kind: TokenKind::Eq,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    }
                }
                b'<' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                        self.pos += 1;
                        self.tokens.push(Token {
                            kind: TokenKind::Le,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    } else {
                        self.tokens.push(Token {
                            kind: TokenKind::Lt,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    }
                }
                b'>' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                        self.pos += 1;
                        self.tokens.push(Token {
                            kind: TokenKind::Ge,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    } else {
                        self.tokens.push(Token {
                            kind: TokenKind::Gt,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    }
                }
                b'!' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                        self.pos += 1;
                        self.tokens.push(Token {
                            kind: TokenKind::Ne,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    } else {
                        self.tokens.push(Token {
                            kind: TokenKind::Bang,
                            span: SourceSpan::new(start as u32, self.pos as u32),
                        });
                    }
                }
                b'&' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'&' {
                        self.pos += 1;
                    }
                    self.tokens.push(Token {
                        kind: TokenKind::And,
                        span: SourceSpan::new(start as u32, self.pos as u32),
                    });
                }
                b'|' => {
                    let start = self.pos;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'|' {
                        self.pos += 1;
                    }
                    self.tokens.push(Token {
                        kind: TokenKind::Or,
                        span: SourceSpan::new(start as u32, self.pos as u32),
                    });
                }
                b'"' => self.lex_string(),
                b'0'..=b'9' => self.lex_number(),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(),
                _ => {
                    // Skip unknown characters
                    self.pos += 1;
                }
            }
        }

        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: SourceSpan::new(self.pos as u32, self.pos as u32),
        });

        self.tokens
    }

    fn skip_spaces(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn single_char(&mut self, kind: TokenKind) {
        let start = self.pos;
        self.pos += 1;
        self.tokens.push(Token {
            kind,
            span: SourceSpan::new(start as u32, self.pos as u32),
        });
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        self.pos += 1; // skip opening quote
        let mut value = String::new();
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'"' {
            if self.bytes[self.pos] == b'\\' && self.pos + 1 < self.bytes.len() {
                self.pos += 1;
                match self.bytes[self.pos] {
                    b'n' => value.push('\n'),
                    b't' => value.push('\t'),
                    b'\\' => value.push('\\'),
                    b'"' => value.push('"'),
                    other => {
                        value.push('\\');
                        value.push(other as char);
                    }
                }
                self.pos += 1;
            } else {
                value.push(self.bytes[self.pos] as char);
                self.pos += 1;
            }
        }
        if self.pos < self.bytes.len() {
            self.pos += 1; // skip closing quote
        }
        self.tokens.push(Token {
            kind: TokenKind::StringLit(value),
            span: SourceSpan::new(start as u32, self.pos as u32),
        });
    }

    fn lex_number(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for decimal point
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self.pos + 1 < self.bytes.len()
            && self.bytes[self.pos + 1].is_ascii_digit()
        {
            self.pos += 1; // skip dot
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let text = &self.source[start..self.pos];
        let value: f64 = text.parse().unwrap_or(0.0);
        self.tokens.push(Token {
            kind: TokenKind::Number(value),
            span: SourceSpan::new(start as u32, self.pos as u32),
        });
    }

    fn lex_ident(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_alphanumeric() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let text = &self.source[start..self.pos];
        self.tokens.push(Token {
            kind: TokenKind::Ident(text.to_string()),
            span: SourceSpan::new(start as u32, self.pos as u32),
        });
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect_ident(&mut self) -> Option<(String, SourceSpan)> {
        match self.peek_kind() {
            TokenKind::Ident(_) => {
                let tok = self.advance();
                if let TokenKind::Ident(name) = tok.kind {
                    Some((name, tok.span))
                } else {
                    unreachable!()
                }
            }
            _ => {
                let span = self.current().span;
                self.errors.push(ParseError {
                    message: "expected identifier".into(),
                    span,
                });
                None
            }
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Option<Token> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind) {
            Some(self.advance())
        } else {
            let span = self.current().span;
            self.errors.push(ParseError {
                message: format!("expected {:?}, found {:?}", kind, self.peek_kind()),
                span,
            });
            None
        }
    }

    fn check_ident(&self, name: &str) -> bool {
        matches!(self.peek_kind(), TokenKind::Ident(n) if n == name)
    }

    // -----------------------------------------------------------------------
    // Top-level parsing
    // -----------------------------------------------------------------------

    fn parse_module(&mut self, module_path: ModulePath) -> ParsedModule {
        self.skip_newlines();

        // Optional: `module <path>`
        let path = if self.check_ident("module") {
            self.advance();
            if let Some((name, _)) = self.expect_ident() {
                ModulePath::single(name)
            } else {
                module_path.clone()
            }
        } else {
            module_path.clone()
        };

        let mut declarations = Vec::new();

        loop {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }

            match self.parse_declaration() {
                Some(decl) => declarations.push(decl),
                None => {
                    // Error recovery: skip to next newline or EOF
                    if !self.at_eof() {
                        self.advance();
                    }
                }
            }
        }

        let content_hash = ContentHash::of_str(self.source);

        ParsedModule {
            path,
            declarations,
            errors: self.errors.clone(),
            content_hash,
        }
    }

    fn parse_declaration(&mut self) -> Option<Declaration> {
        if self.check_ident("structure") {
            self.parse_structure_def().map(Declaration::Structure)
        } else if self.check_ident("import") {
            self.parse_import().map(Declaration::Import)
        } else {
            let span = self.current().span;
            self.errors.push(ParseError {
                message: format!("unexpected token: {:?}", self.peek_kind()),
                span,
            });
            None
        }
    }

    fn parse_structure_def(&mut self) -> Option<StructureDef> {
        let start_tok = self.advance(); // consume 'structure'
        let start = start_tok.span.start;

        // Optional 'def' keyword
        if self.check_ident("def") {
            self.advance();
        }

        let (name, _) = self.expect_ident()?;

        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut members = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }

            match self.parse_member() {
                Some(member) => members.push(member),
                None => {
                    // Error recovery: skip to next newline
                    self.skip_to_next_statement();
                }
            }
        }

        let end_tok = self.expect(&TokenKind::RBrace);
        let end = end_tok
            .map(|t| t.span.end)
            .unwrap_or(self.current().span.end);

        let span = SourceSpan::new(start, end);

        // Build the text for the structure content hash
        let structure_text = &self.source[start as usize..end as usize];
        let content_hash = ContentHash::of_str(structure_text);

        Some(StructureDef {
            name,
            members,
            span,
            content_hash,
        })
    }

    fn parse_import(&mut self) -> Option<crate::ImportDecl> {
        let start_tok = self.advance(); // consume 'import'
        let start = start_tok.span.start;

        match self.peek_kind() {
            TokenKind::StringLit(_) => {
                let tok = self.advance();
                if let TokenKind::StringLit(path) = tok.kind {
                    let end = tok.span.end;
                    Some(crate::ImportDecl {
                        path,
                        span: SourceSpan::new(start, end),
                    })
                } else {
                    unreachable!()
                }
            }
            _ => {
                let span = self.current().span;
                self.errors.push(ParseError {
                    message: "expected string literal after import".into(),
                    span,
                });
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // Member parsing
    // -----------------------------------------------------------------------

    fn parse_member(&mut self) -> Option<MemberDecl> {
        if self.check_ident("param") {
            self.parse_param().map(MemberDecl::Param)
        } else if self.check_ident("let") {
            self.parse_let().map(MemberDecl::Let)
        } else if self.check_ident("constraint") {
            self.parse_constraint().map(MemberDecl::Constraint)
        } else if self.check_ident("sub") {
            self.parse_sub().map(MemberDecl::Sub)
        } else {
            let span = self.current().span;
            self.errors.push(ParseError {
                message: format!("expected member declaration (param, let, constraint, sub), found {:?}", self.peek_kind()),
                span,
            });
            None
        }
    }

    fn parse_param(&mut self) -> Option<ParamDecl> {
        let start_tok = self.advance(); // consume 'param'
        let start = start_tok.span.start;

        let (name, _) = self.expect_ident()?;

        // Optional type annotation: `: Type`
        let type_expr = if matches!(self.peek_kind(), TokenKind::Colon) {
            self.advance(); // consume ':'
            let (type_name, type_span) = self.expect_ident()?;
            Some(TypeExpr {
                name: type_name,
                span: type_span,
            })
        } else {
            None
        };

        // Optional default value: `= expr`
        let default = if matches!(self.peek_kind(), TokenKind::Eq) {
            self.advance(); // consume '='
            Some(self.parse_expr()?)
        } else {
            None
        };

        let end = self.end_of_previous();
        let span = SourceSpan::new(start, end);

        let member_text = &self.source[start as usize..end as usize];
        let content_hash = ContentHash::of_str(member_text);

        Some(ParamDecl {
            name,
            type_expr,
            default,
            span,
            content_hash,
        })
    }

    fn parse_let(&mut self) -> Option<LetDecl> {
        let start_tok = self.advance(); // consume 'let'
        let start = start_tok.span.start;

        let (name, _) = self.expect_ident()?;

        // Optional type annotation
        let type_expr = if matches!(self.peek_kind(), TokenKind::Colon) {
            self.advance();
            let (type_name, type_span) = self.expect_ident()?;
            Some(TypeExpr {
                name: type_name,
                span: type_span,
            })
        } else {
            None
        };

        self.expect(&TokenKind::Eq)?;

        let value = self.parse_expr()?;

        let end = self.end_of_previous();
        let span = SourceSpan::new(start, end);

        let member_text = &self.source[start as usize..end as usize];
        let content_hash = ContentHash::of_str(member_text);

        Some(LetDecl {
            name,
            type_expr,
            value,
            span,
            content_hash,
        })
    }

    fn parse_constraint(&mut self) -> Option<ConstraintDecl> {
        let start_tok = self.advance(); // consume 'constraint'
        let start = start_tok.span.start;

        // Optional label: `"label"` before the expression
        let label = if matches!(self.peek_kind(), TokenKind::StringLit(_)) {
            let tok = self.advance();
            if let TokenKind::StringLit(s) = tok.kind {
                Some(s)
            } else {
                None
            }
        } else {
            None
        };

        let expr = self.parse_expr()?;

        let end = self.end_of_previous();
        let span = SourceSpan::new(start, end);

        let member_text = &self.source[start as usize..end as usize];
        let content_hash = ContentHash::of_str(member_text);

        Some(ConstraintDecl {
            label,
            expr,
            span,
            content_hash,
        })
    }

    fn parse_sub(&mut self) -> Option<SubDecl> {
        let start_tok = self.advance(); // consume 'sub'
        let start = start_tok.span.start;

        let (name, _) = self.expect_ident()?;

        self.expect(&TokenKind::Eq)?;

        let (structure_name, _) = self.expect_ident()?;

        self.expect(&TokenKind::LParen)?;

        let mut args = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            let (arg_name, _) = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let arg_value = self.parse_expr()?;
            args.push((arg_name, arg_value));

            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
        }

        self.expect(&TokenKind::RParen)?;

        let end = self.end_of_previous();
        let span = SourceSpan::new(start, end);

        let member_text = &self.source[start as usize..end as usize];
        let content_hash = ContentHash::of_str(member_text);

        Some(SubDecl {
            name,
            structure_name,
            args,
            span,
            content_hash,
        })
    }

    // -----------------------------------------------------------------------
    // Expression parsing (Pratt / precedence climbing)
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_and_expr()?;

        while matches!(self.peek_kind(), TokenKind::Or) {
            let op_tok = self.advance();
            let right = self.parse_and_expr()?;
            let span = SourceSpan::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::BinOp {
                    op: self.source[op_tok.span.start as usize..op_tok.span.end as usize]
                        .to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        Some(left)
    }

    fn parse_and_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison_expr()?;

        while matches!(self.peek_kind(), TokenKind::And) {
            let op_tok = self.advance();
            let right = self.parse_comparison_expr()?;
            let span = SourceSpan::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::BinOp {
                    op: self.source[op_tok.span.start as usize..op_tok.span.end as usize]
                        .to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        Some(left)
    }

    fn parse_comparison_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_additive_expr()?;

        while matches!(
            self.peek_kind(),
            TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::Le
                | TokenKind::Ge
                | TokenKind::EqEq
                | TokenKind::Ne
        ) {
            let op_tok = self.advance();
            let op_str = match &op_tok.kind {
                TokenKind::Lt => "<",
                TokenKind::Gt => ">",
                TokenKind::Le => "<=",
                TokenKind::Ge => ">=",
                TokenKind::EqEq => "==",
                TokenKind::Ne => "!=",
                _ => unreachable!(),
            };
            let right = self.parse_additive_expr()?;
            let span = SourceSpan::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::BinOp {
                    op: op_str.to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        Some(left)
    }

    fn parse_additive_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_multiplicative_expr()?;

        while matches!(self.peek_kind(), TokenKind::Plus | TokenKind::Minus) {
            let op_tok = self.advance();
            let op_str = match &op_tok.kind {
                TokenKind::Plus => "+",
                TokenKind::Minus => "-",
                _ => unreachable!(),
            };
            let right = self.parse_multiplicative_expr()?;
            let span = SourceSpan::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::BinOp {
                    op: op_str.to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        Some(left)
    }

    fn parse_multiplicative_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_unary_expr()?;

        while matches!(self.peek_kind(), TokenKind::Star | TokenKind::Slash) {
            let op_tok = self.advance();
            let op_str = match &op_tok.kind {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                _ => unreachable!(),
            };
            let right = self.parse_unary_expr()?;
            let span = SourceSpan::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::BinOp {
                    op: op_str.to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }

        Some(left)
    }

    fn parse_unary_expr(&mut self) -> Option<Expr> {
        match self.peek_kind() {
            TokenKind::Minus => {
                let op_tok = self.advance();
                let operand = self.parse_unary_expr()?;
                let span = SourceSpan::new(op_tok.span.start, operand.span.end);
                Some(Expr {
                    kind: ExprKind::UnOp {
                        op: "-".to_string(),
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::Bang => {
                let op_tok = self.advance();
                let operand = self.parse_unary_expr()?;
                let span = SourceSpan::new(op_tok.span.start, operand.span.end);
                Some(Expr {
                    kind: ExprKind::UnOp {
                        op: "!".to_string(),
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            match self.peek_kind() {
                TokenKind::Dot => {
                    self.advance(); // consume '.'
                    let (member, member_span) = self.expect_ident()?;
                    let span = SourceSpan::new(expr.span.start, member_span.end);
                    expr = Expr {
                        kind: ExprKind::MemberAccess {
                            object: Box::new(expr),
                            member,
                        },
                        span,
                    };
                }
                _ => break,
            }
        }

        Some(expr)
    }

    fn parse_primary_expr(&mut self) -> Option<Expr> {
        match self.peek_kind().clone() {
            TokenKind::Number(value) => {
                let value = value;
                let tok = self.advance();
                let num_span = tok.span;

                // Check for unit suffix immediately following the number (no whitespace)
                // A unit suffix is an identifier token whose start == number token's end
                if let TokenKind::Ident(unit_name) = self.peek_kind() {
                    let next_tok = self.current();
                    if next_tok.span.start == num_span.end {
                        let unit = unit_name.clone();
                        let unit_tok = self.advance();
                        let span = SourceSpan::new(num_span.start, unit_tok.span.end);
                        return Some(Expr {
                            kind: ExprKind::QuantityLiteral { value, unit },
                            span,
                        });
                    }
                }

                Some(Expr {
                    kind: ExprKind::NumberLiteral(value),
                    span: num_span,
                })
            }
            TokenKind::StringLit(ref s) => {
                let s = s.clone();
                let tok = self.advance();
                Some(Expr {
                    kind: ExprKind::StringLiteral(s),
                    span: tok.span,
                })
            }
            TokenKind::Ident(ref name) => {
                let name = name.clone();

                // Check for keywords
                if name == "true" {
                    let tok = self.advance();
                    return Some(Expr {
                        kind: ExprKind::BoolLiteral(true),
                        span: tok.span,
                    });
                }
                if name == "false" {
                    let tok = self.advance();
                    return Some(Expr {
                        kind: ExprKind::BoolLiteral(false),
                        span: tok.span,
                    });
                }
                if name == "if" {
                    return self.parse_conditional();
                }

                let tok = self.advance();
                let ident_span = tok.span;

                // Check for function call: `name(`
                if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.advance(); // consume '('
                    let mut args = Vec::new();

                    while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
                        if let Some(arg) = self.parse_expr() {
                            args.push(arg);
                        } else {
                            break;
                        }

                        if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }

                    let end_tok = self.expect(&TokenKind::RParen)?;
                    let span = SourceSpan::new(ident_span.start, end_tok.span.end);

                    return Some(Expr {
                        kind: ExprKind::FunctionCall { name, args },
                        span,
                    });
                }

                Some(Expr {
                    kind: ExprKind::Ident(name),
                    span: ident_span,
                })
            }
            TokenKind::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Some(expr)
            }
            _ => {
                let span = self.current().span;
                self.errors.push(ParseError {
                    message: format!("unexpected token in expression: {:?}", self.peek_kind()),
                    span,
                });
                None
            }
        }
    }

    fn parse_conditional(&mut self) -> Option<Expr> {
        let start_tok = self.advance(); // consume 'if'
        let start = start_tok.span.start;

        let condition = self.parse_expr()?;

        // expect 'then'
        if !self.check_ident("then") {
            let span = self.current().span;
            self.errors.push(ParseError {
                message: "expected 'then' in conditional".into(),
                span,
            });
            return None;
        }
        self.advance();

        let then_branch = self.parse_expr()?;

        // expect 'else'
        if !self.check_ident("else") {
            let span = self.current().span;
            self.errors.push(ParseError {
                message: "expected 'else' in conditional".into(),
                span,
            });
            return None;
        }
        self.advance();

        let else_branch = self.parse_expr()?;

        let span = SourceSpan::new(start, else_branch.span.end);

        Some(Expr {
            kind: ExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span,
        })
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Get the end position of the previously consumed token.
    fn end_of_previous(&self) -> u32 {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            self.current().span.end
        }
    }

    /// Skip tokens until we reach a newline or EOF (error recovery).
    fn skip_to_next_statement(&mut self) {
        while !matches!(
            self.peek_kind(),
            TokenKind::Newline | TokenKind::Eof | TokenKind::RBrace
        ) {
            self.advance();
        }
        // Consume the newline if present
        if matches!(self.peek_kind(), TokenKind::Newline) {
            self.advance();
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse source text into a `ParsedModule`.
pub fn parse(source: &str, module_path: ModulePath) -> ParsedModule {
    let tokens = Lexer::new(source).tokenize();
    let mut parser = Parser::new(source, tokens);
    parser.parse_module(module_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_basic() {
        let tokens = Lexer::new("structure Bracket {").tokenize();
        assert!(matches!(tokens[0].kind, TokenKind::Ident(ref n) if n == "structure"));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref n) if n == "Bracket"));
        assert!(matches!(tokens[2].kind, TokenKind::LBrace));
    }

    #[test]
    fn lex_quantity() {
        let tokens = Lexer::new("80mm").tokenize();
        assert!(matches!(tokens[0].kind, TokenKind::Number(v) if (v - 80.0).abs() < f64::EPSILON));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref n) if n == "mm"));
        // The number token ends at offset 2, the ident starts at offset 2
        assert_eq!(tokens[0].span.end, tokens[1].span.start);
    }

    #[test]
    fn parse_simple_structure() {
        let source = "structure Foo {\n    param x: Scalar = 10mm\n}";
        let module = parse(source, ModulePath::single("test"));
        assert!(module.errors.is_empty(), "errors: {:?}", module.errors);
        assert_eq!(module.declarations.len(), 1);
        match &module.declarations[0] {
            Declaration::Structure(s) => {
                assert_eq!(s.name, "Foo");
                assert_eq!(s.members.len(), 1);
            }
            _ => panic!("expected structure"),
        }
    }
}
