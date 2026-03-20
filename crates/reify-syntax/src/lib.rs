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
    Enum(EnumDecl),
}

/// A structure definition (the primary entity type in Reify).
#[derive(Debug, Clone)]
pub struct StructureDef {
    pub name: String,
    pub is_pub: bool,
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
    Minimize(MinimizeDecl),
    Maximize(MaximizeDecl),
    GuardedGroup(GuardedGroupDecl),
}

/// `where condition { ...members... } else { ...members... }`
#[derive(Debug, Clone)]
pub struct GuardedGroupDecl {
    pub condition: Expr,
    pub members: Vec<MemberDecl>,
    pub else_members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A `where` guard condition applied to a declaration or block.
#[derive(Debug, Clone)]
pub struct WhereClause {
    pub condition: Expr,
    pub span: SourceSpan,
}

/// `param width: Scalar = 80mm`
#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub default: Option<Expr>,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `let volume = width * height * thickness`
#[derive(Debug, Clone)]
pub struct LetDecl {
    pub name: String,
    pub is_pub: bool,
    pub type_expr: Option<TypeExpr>,
    pub value: Expr,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `constraint thickness > 2mm`
#[derive(Debug, Clone)]
pub struct ConstraintDecl {
    pub label: Option<String>,
    pub expr: Expr,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `sub mount_hole = Hole(diameter: 6mm)`
#[derive(Debug, Clone)]
pub struct SubDecl {
    pub name: String,
    pub structure_name: String,
    pub args: Vec<(String, Expr)>,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `minimize volume`
#[derive(Debug, Clone)]
pub struct MinimizeDecl {
    pub expr: Expr,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `maximize thickness`
#[derive(Debug, Clone)]
pub struct MaximizeDecl {
    pub expr: Expr,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `import "fasteners/bolt"`
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: String,
    pub span: SourceSpan,
}

/// `enum Direction { In, Out, Bidi }`
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub is_pub: bool,
    pub variants: Vec<String>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
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
    /// Enum variant access: `Direction.In`
    EnumAccess {
        type_name: String,
        variant: String,
    },
    /// Conditional: `if cond then a else b`
    Conditional {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    /// List literal: `[1, 2, 3]`
    ListLiteral(Vec<Expr>),
    /// Set literal: `set{1, 2, 3}`
    SetLiteral(Vec<Expr>),
    /// Map literal: `map{"a" => 1, "b" => 2}`
    MapLiteral(Vec<(Expr, Expr)>),
    /// Index access: `expr[index]`
    IndexAccess {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    /// Match expression: `match d { In => 1, Out => 2 }`
    Match {
        discriminant: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// Auto keyword: solver-determined parameter value
    Auto,
}

/// A match arm: `Pattern1 | Pattern2 => body`
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub patterns: Vec<String>,
    pub body: Expr,
    pub span: SourceSpan,
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
/// Backed by a Tree-sitter grammar parser with CST→AST lowering.
pub fn parse(source: &str, module_path: reify_types::ModulePath) -> ParsedModule {
    ts_parser::parse(source, module_path)
}
