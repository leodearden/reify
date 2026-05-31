//! Parsed expression and type-expression AST.
//!
//! These types model *source as written* — identifiers unresolved, operators
//! as strings, no types attached. Contrast the name-resolved, type-checked
//! form in `reify-ir::expr::CompiledExpr` ready for evaluation. The parsed
//! AST is produced by the parser in `reify-syntax` and re-exported from it (so
//! `reify_syntax::Expr` etc. continue to resolve unchanged).

use reify_core::SourceSpan;
use std::fmt;

/// An expression in the AST (pre-compilation).
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: SourceSpan,
}

/// Expression kinds in the AST.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Numeric literal: `42`, `3.14`, `1.0`, `1e6`.
    /// `is_real` is `true` when the source token contains `.`, `e`, or `E`;
    /// `false` for bare integer tokens (e.g. `42`, `0`). Used by the compiler
    /// to distinguish `1.0 : Real` from `1 : Int` without re-inspecting source text.
    NumberLiteral { value: f64, is_real: bool },
    /// Quantity literal: `80mm`, `45deg`, `7850kg/m^3`. The `unit` is a
    /// structured [`UnitExpr`] tree (a bare `mm` lowers to `UnitExpr::Unit("mm")`).
    QuantityLiteral { value: f64, unit: UnitExpr },
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
    UnOp { op: String, operand: Box<Expr> },
    /// Function call: `sin(x)`
    FunctionCall { name: String, args: Vec<Expr> },
    /// Member access: `self.width`
    MemberAccess { object: Box<Expr>, member: String },
    /// Enum variant access: `Direction.In`
    EnumAccess { type_name: String, variant: String },
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
    IndexAccess { object: Box<Expr>, index: Box<Expr> },
    /// Match expression: `match d { In => 1, Out => 2 }`
    Match {
        discriminant: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// Auto keyword: solver-determined parameter value.
    /// `free: false` = bare `auto` (strict), `free: true` = `auto(free)`.
    Auto { free: bool },
    /// Lambda expression: `|x, y| x + y`
    Lambda {
        params: Vec<LambdaParam>,
        body: Box<Expr>,
    },
    /// Quantifier expression: `forall x in coll: pred` or `exists x in coll: pred`
    Quantifier {
        kind: QuantifierKind,
        variable: String,
        collection: Box<Expr>,
        predicate: Box<Expr>,
    },
    /// Ad-hoc port selector: `expr @ ident(args)`
    AdHocSelector {
        base: Box<Expr>,
        selector: String,
        args: Vec<Expr>,
    },
    /// Qualified access: `Foo::bar` — access a member through a qualified path
    QualifiedAccess {
        qualifier: Box<Expr>,
        member: String,
    },
    /// Instance qualified access: `obj.(Foo::bar)` — trait-qualified member access on an instance
    InstanceQualifiedAccess {
        object: Box<Expr>,
        qualified: Box<Expr>,
    },
    /// Range expression: `1..10`, `1..<10`, `>2mm`, `>=2mm`, `<10mm`, `<=10mm`
    Range {
        lower: Option<Box<Expr>>,
        upper: Option<Box<Expr>>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// Trait-associated function call on an instance: `obj.(Trait::method)(args)`.
    /// Produced by a `trait_method_call` CST node whose callee is
    /// `instance_qualified_access`.  The `self` receiver is `object`; dispatch
    /// to the concrete implementation is deferred to task δ.
    TraitMethodCall {
        object: Box<Expr>,
        trait_name: String,
        method: String,
        args: Vec<Expr>,
    },
    /// Trait-associated static-function call: `Trait::method(args)`.
    /// Produced by a `trait_method_call` CST node whose callee is
    /// `qualified_access`.  No receiver; dispatch deferred to task δ.
    TraitStaticCall {
        trait_name: String,
        method: String,
        args: Vec<Expr>,
    },
    /// Named-field variant construction: `Circle { radius: 5mm }`.
    /// Produced by a `variant_construction` CST node (grammar task α).
    /// Resolution (is `name` a known enum variant? do fields match the decl?)
    /// and `Value::Enum` construction are deferred to task δ (3942).
    /// Fields are in source-declaration order.
    VariantConstruct {
        name: String,
        fields: Vec<(String, Expr)>,
    },
}

/// A unit expression attached to a [`ExprKind::QuantityLiteral`] — the
/// structured form of a compound unit like `kg/m^3` or `m/s^2`, as well as a
/// bare unit like `mm`.
///
/// Produced by `lower_unit_expr` in `reify-syntax` from the `unit_expr` CST
/// (grammar task α; PRD `docs/prds/unit-expressions.md` §4.1). Parens carry no
/// semantic content — they only steer parse precedence — so they are unwrapped
/// during lowering and there is no `Paren` variant. The signed exponent is an
/// `i32` (PRD §11.3): no realistic SI/derived unit needs a magnitude beyond
/// ~±10, and it matches `f64::powi` natively for the resolver.
///
/// Registry-fold semantics (resolving a `UnitExpr` to an SI factor + dimension
/// vector) are owned by task γ (3803); at this parsed-AST layer the type is
/// purely structural.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitExpr {
    /// A bare unit name: `mm`, `kg`, `MPa`.
    Unit(String),
    /// Multiplication: `kN*m`.
    Mul(Box<UnitExpr>, Box<UnitExpr>),
    /// Division: `kg/m` (left-associative).
    Div(Box<UnitExpr>, Box<UnitExpr>),
    /// Exponentiation by a signed integer: `m^3`, `s^-2`.
    Pow(Box<UnitExpr>, i32),
}

/// A single alternative in a match pattern.
///
/// Mirrors the PRD §7.1 IR `CompiledPattern` shape so γ/ε get a 1:1 lowering
/// target.  The outer `Vec<MatchPattern>` in `MatchArm.patterns` still encodes
/// pipe-alternation (each alternative is a `Variant`) and the wildcard (a
/// single `Wildcard` element).
///
/// `VariantBind` binders are dropped at the reify-compiler β boundary
/// (lossy bridge); γ/ε/ζ widen `CompiledMatchArm` to carry them.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    /// `_` wildcard arm.
    Wildcard,
    /// A bare variant name: `In`, `Circle`, etc.
    Variant(String),
    /// A named-field payload-binding pattern: `Circle { radius: r }`.
    /// `binders` is ordered by source declaration order.
    VariantBind {
        name: String,
        binders: Vec<(String, String)>,
    },
}

/// A match arm: `Pattern1 | Pattern2 => body`
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub patterns: Vec<MatchPattern>,
    pub body: Expr,
    pub span: SourceSpan,
}

/// A lambda parameter: `x` or `x: Real`
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParam {
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub span: SourceSpan,
}

/// The kind of quantifier: universal (forall) or existential (exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuantifierKind {
    ForAll,
    Exists,
}

/// A dimensional operator: multiplication or division between type-level dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimOp {
    Mul,
    Div,
}

impl DimOp {
    /// The source-text spelling of this operator.
    pub fn as_str(self) -> &'static str {
        match self {
            DimOp::Mul => "*",
            DimOp::Div => "/",
        }
    }
}

/// What a [`TypeExpr`] actually is — a named type, a binary dimensional operation,
/// or an integer literal (only valid as a type-argument of parametric types like
/// `Tensor<rank, n, q>` and `Matrix<m, n, q>`).
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExprKind {
    /// A named type with optional type arguments (e.g., `Scalar`, `Box<T>`, `Map<K, V>`).
    Named {
        name: String,
        type_args: Vec<TypeExpr>,
    },
    /// A binary dimensional operator applied to two type expressions (e.g., `Force / Area`).
    DimensionalOp {
        op: DimOp,
        left: Box<TypeExpr>,
        right: Box<TypeExpr>,
    },
    /// An unsigned integer literal in type-argument position (e.g., the `2` and `3`
    /// in `Tensor<2, 3, MomentOfInertia>`). Only valid as a child of
    /// `Named.type_args` for the parametric `Tensor`/`Matrix` constructors —
    /// every other consumer of `TypeExpr` must reject this variant with a
    /// diagnostic.
    IntegerLiteral(u32),
    /// An auto type argument in type-arg position: `auto: Bound` (strict) or
    /// `auto(free): Bound` (free). `free: false` = bare `auto`, `free: true` =
    /// `auto(free)`. `bound` is always a bare identifier — composite/parametric
    /// bounds are explicitly deferred per grammar.js:658–662.
    ///
    /// Parallel to `ExprKind::Auto { free: bool }` for the value-position
    /// analogue. The `auto_keyword` grammar rule is shared between param-default
    /// and type-arg positions (grammar.js:433–436,654–657).
    ///
    /// Actual auto-type resolution semantics are deferred to task 3477/3558
    /// (B1 grammar-fiction chain). Task 3665 wires the lowering extension only.
    Auto { free: bool, bound: String },
    /// A qualified associated-type reference in type position: `Beam::Material`,
    /// `T::Material` (type-param base), or the FORK-G trait-disambiguated form
    /// `Beam::(HasMaterial::Material)` (trait_name = Some("HasMaterial")).
    ///
    /// Type-side analogue of `ExprKind::QualifiedAccess { qualifier, member }`.
    /// `base` is lowered from a bare identifier (structure name or type param);
    /// `trait_name` is Some only for the parenthesized disambiguator.
    ///
    /// Resolution to a concrete `Type` (and E_AMBIGUOUS_ASSOC_TYPE) is deferred
    /// to task ιₑ — new compiler arms MUST NOT resolve; they return
    /// None / Type::Error / the existing unresolved-type diagnostic path.
    QualifiedAssoc {
        base: Box<TypeExpr>,
        trait_name: Option<String>,
        member: String,
    },
}

/// A type expression in the AST (e.g., `Scalar`, `Bool`, `Box<T>`, `Force / Area`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: SourceSpan,
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeExprKind::Named { name, type_args } => {
                write!(f, "{}", name)?;
                if !type_args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in type_args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            TypeExprKind::DimensionalOp { op, left, right } => {
                write!(f, "{} {} {}", left, op.as_str(), right)
            }
            TypeExprKind::IntegerLiteral(n) => write!(f, "{}", n),
            TypeExprKind::Auto { free, bound } => {
                write!(f, "auto{}: {}", if *free { "(free)" } else { "" }, bound)
            }
            TypeExprKind::QualifiedAssoc { base, trait_name, member } => {
                match trait_name {
                    None => write!(f, "{}::{}", base, member),
                    Some(t) => write!(f, "{}::({}::{})", base, t, member),
                }
            }
        }
    }
}
