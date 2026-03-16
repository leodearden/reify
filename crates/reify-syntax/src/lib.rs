mod parser;
mod ts_parser;

use reify_types::{ContentHash, SourceSpan};

/// A parsed module — the output of the parser.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: reify_types::ModulePath,
    pub declarations: Vec<Declaration>,
    pub errors: Vec<ParseError>,
    pub content_hash: ContentHash,
}

/// A top-level declaration in a module.
#[derive(Debug, Clone)]
pub enum Declaration {
    Structure(StructureDef),
    Import(ImportDecl),
}

/// A structure definition (the primary entity type in Reify).
#[derive(Debug, Clone)]
pub struct StructureDef {
    pub name: String,
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A member declaration within a structure.
#[derive(Debug, Clone)]
pub enum MemberDecl {
    Param(ParamDecl),
    Let(LetDecl),
    Constraint(ConstraintDecl),
    Sub(SubDecl),
}

/// `param width: Scalar = 80mm`
#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub default: Option<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `let volume = width * height * thickness`
#[derive(Debug, Clone)]
pub struct LetDecl {
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub value: Expr,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `constraint thickness > 2mm`
#[derive(Debug, Clone)]
pub struct ConstraintDecl {
    pub label: Option<String>,
    pub expr: Expr,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `sub mount_hole = Hole(diameter: 6mm)`
#[derive(Debug, Clone)]
pub struct SubDecl {
    pub name: String,
    pub structure_name: String,
    pub args: Vec<(String, Expr)>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `import "fasteners/bolt"`
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: String,
    pub span: SourceSpan,
}

/// An expression in the AST (pre-compilation).
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: SourceSpan,
}

/// Expression kinds in the AST.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Numeric literal: `42`, `3.14`
    NumberLiteral(f64),
    /// Quantity literal: `80mm`, `45deg`
    QuantityLiteral {
        value: f64,
        unit: String,
    },
    /// String literal: `"hello"`
    StringLiteral(String),
    /// Boolean literal: `true`, `false`
    BoolLiteral(bool),
    /// Identifier reference: `width`
    Ident(String),
    /// Binary operation: `a + b`
    BinOp {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary operation: `-a`, `!b`
    UnOp {
        op: String,
        operand: Box<Expr>,
    },
    /// Function call: `sin(x)`
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
    /// Member access: `self.width`
    MemberAccess {
        object: Box<Expr>,
        member: String,
    },
    /// Conditional: `if cond then a else b`
    Conditional {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
}

/// A type expression in the AST (e.g., `Scalar`, `Bool`).
#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub name: String,
    pub span: SourceSpan,
}

/// A parse error.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: SourceSpan,
}

/// Parse a source string into a `ParsedModule`.
///
/// Currently backed by a hand-written recursive descent parser for the M1 subset.
/// Will be replaced by Tree-sitter in a future milestone.
pub fn parse(source: &str, module_path: reify_types::ModulePath) -> ParsedModule {
    parser::parse(source, module_path)
}
