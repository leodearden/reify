mod ts_parser;

use reify_types::{ContentHash, PortDirection, SourceSpan, SpannedIdent};
use std::fmt;

/// A parsed module — the output of the parser.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: reify_types::ModulePath,
    pub declarations: Vec<Declaration>,
    pub errors: Vec<ParseError>,
    pub content_hash: ContentHash,
    /// Module-level pragmas (e.g., `#optimize` at the top of a file).
    pub pragmas: Vec<Pragma>,
}

/// A top-level declaration in a module.
#[derive(Debug, Clone)]
pub enum Declaration {
    Structure(StructureDef),
    Occurrence(OccurrenceDef),
    Import(ImportDecl),
    Enum(EnumDecl),
    Function(FnDef),
    Trait(TraitDecl),
    Field(FieldDef),
    Purpose(PurposeDef),
    Constraint(ConstraintDef),
    Unit(UnitDecl),
    TypeAlias(TypeAliasDecl),
}

/// A structure definition (the primary entity type in Reify).
#[derive(Debug, Clone)]
pub struct StructureDef {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub trait_bounds: Vec<TraitBoundRef>,
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Block-level pragmas inside this structure.
    pub pragmas: Vec<Pragma>,
    /// Annotations preceding this declaration (e.g., `@test`, `@deprecated("msg")`).
    pub annotations: Vec<Annotation>,
}

/// A trait bound reference with optional type arguments (e.g., `Rigid` or `Container<Bolt>`).
#[derive(Debug, Clone)]
pub struct TraitBoundRef {
    pub name: String,
    pub type_args: Vec<TypeExpr>,
    pub span: SourceSpan,
}

/// An occurrence definition (a process/transformation entity type in Reify).
/// Structurally identical to StructureDef but semantically represents a process.
#[derive(Debug, Clone)]
pub struct OccurrenceDef {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub trait_bounds: Vec<TraitBoundRef>,
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Block-level pragmas inside this occurrence.
    pub pragmas: Vec<Pragma>,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// A member declaration within a structure or trait.
#[derive(Debug, Clone)]
pub enum MemberDecl {
    Param(ParamDecl),
    Let(LetDecl),
    Constraint(ConstraintDecl),
    ConstraintInst(ConstraintInstDecl),
    Sub(SubDecl),
    Minimize(MinimizeDecl),
    Maximize(MaximizeDecl),
    GuardedGroup(GuardedGroupDecl),
    AssociatedType(AssociatedTypeDecl),
    Port(PortDecl),
    Connect(ConnectDecl),
    Chain(ChainDecl),
    MetaBlock(MetaBlockDecl),
    /// `forall v in coll: connect ...` or `forall v in coll: chain ...`
    ForallConnect(ForallConnectDecl),
    /// `forall v in coll: constraint ...` or `forall v in coll: constraint Inst(...)`
    ForallConstraint(ForallConstraintDecl),
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
    pub doc: Option<String>,
    pub type_expr: Option<TypeExpr>,
    pub default: Option<Expr>,
    pub where_clause: Option<WhereClause>,
    pub annotations: Vec<Annotation>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `let volume = width * height * thickness`
#[derive(Debug, Clone)]
pub struct LetDecl {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_expr: Option<TypeExpr>,
    pub value: Expr,
    pub where_clause: Option<WhereClause>,
    pub annotations: Vec<Annotation>,
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

/// `constraint MinWall(wall: thickness)` inside a structure body.
///
/// Instantiates a named constraint definition, binding named arguments to
/// the constraint def's parameters. During compilation each predicate from
/// the constraint def is substituted with the bound arguments and compiled
/// in the calling entity's scope.
#[derive(Debug, Clone)]
pub struct ConstraintInstDecl {
    pub name: String,
    pub args: Vec<(String, Expr)>,
    pub where_clause: Option<WhereClause>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `sub mount_hole = Hole(diameter: 6mm)` or `sub part = Box<Bolt>()`
///
/// Specialization-scope body (`sub motor : T { ... }`) is represented by
/// `body: Some(...)`; `None` means a bare instantiation or collection form.
/// The `Some(_)` discriminator IS the spec §8.7 specialization-scope flag —
/// see `walk_specialization_scope_members` for the traversal contract.
#[derive(Debug, Clone)]
pub struct SubDecl {
    pub name: String,
    pub structure_name: String,
    pub type_args: Vec<TypeExpr>,
    pub args: Vec<(String, Expr)>,
    pub is_collection: bool,
    pub where_clause: Option<WhereClause>,
    /// Members of a specialization-scope body, when this `sub` opens one.
    /// `None` for bare instantiation or collection forms (the only forms
    /// the current grammar produces; the `{ body }` form is reserved for a
    /// future grammar update).
    pub body: Option<Vec<MemberDecl>>,
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

/// `port mount : in MechanicalPort { direction = out  param d : Length = 5mm }`
#[derive(Debug, Clone)]
pub struct PortDecl {
    pub name: String,
    pub direction: Option<PortDirection>,
    pub type_name: String,
    pub members: Vec<MemberDecl>,
    pub frame_expr: Option<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// Information about a named member's source span and doc comment.
///
/// Returned by [`find_named_member_span`] — a named alternative to a bare tuple.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberSpanInfo<'a> {
    pub span: SourceSpan,
    pub doc: Option<&'a str>,
}

/// Maximum nesting depth for recursive member lookups. Prevents stack
/// overflow on pathological input with deeply nested guarded groups or ports.
/// 32 is generous for any realistic Reify source (typical nesting is 2-3 levels).
pub const MAX_MEMBER_NESTING_DEPTH: usize = 32;

/// Recursively search a member list for a named param or let declaration.
///
/// Returns [`MemberSpanInfo`] for the first match. Recurses into
/// `GuardedGroup.members`, `GuardedGroup.else_members`, and `Port.members`
/// so that declarations inside `where cond { ... } else { ... }` blocks
/// and port bodies are found. Recursion is bounded by
/// [`MAX_MEMBER_NESTING_DEPTH`] to prevent stack overflow on pathological input.
pub fn find_named_member_span<'a>(
    members: &'a [MemberDecl],
    name: &str,
) -> Option<MemberSpanInfo<'a>> {
    find_named_member_span_depth(members, name, 0)
}

/// Visit every member of a specialization-scope body (spec §8.7).
///
/// A `SubDecl` whose `body.is_some()` opens a specialization scope; this
/// walker iterates its members, invoking `visitor` on each one. When the
/// `body` is `None` (bare instantiation or collection form), the walker is
/// a no-op — those forms are not specialization scopes.
///
/// In later steps the walker will recurse into:
///   * `MemberDecl::Sub(s)` whose `s.body.is_some()` — nested specialization
///     scopes (spec §8.7 nested-sub criterion).
///   * `MemberDecl::GuardedGroup(g)` — both `g.members` (the `where { … }`
///     branch) and `g.else_members` (the `else { … }` branch). Both branches
///     are siblings inside the enclosing specialization scope.
///
/// The walker does NOT recurse into `PortDecl.members`; port bodies have
/// their own grammar and are themselves forbidden inside a specialization
/// scope (the rejection rule lives in task 2369). Recursion is bounded by
/// [`MAX_MEMBER_NESTING_DEPTH`] to prevent stack overflow on pathological
/// input — same convention as [`find_named_member_span`].
pub fn walk_specialization_scope_members<'a, F>(sub: &'a SubDecl, visitor: &mut F)
where
    F: FnMut(&'a MemberDecl),
{
    if let Some(body) = sub.body.as_ref() {
        walk_members_depth(body, visitor, 0);
    }
}

fn walk_members_depth<'a, F>(members: &'a [MemberDecl], visitor: &mut F, depth: usize)
where
    F: FnMut(&'a MemberDecl),
{
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        visitor(member);
        match member {
            // Spec §8.7 nested-sub criterion: a nested SubDecl whose own
            // body is `Some(_)` opens its own specialization scope. Visit
            // the outer Sub first (parent-before-children), then descend.
            MemberDecl::Sub(s) => {
                if let Some(nested) = s.body.as_ref() {
                    walk_members_depth(nested, visitor, depth + 1);
                }
            }
            // Spec §8.7 + shadow_lint.rs:39-43: `where { … } else { … }`
            // members are siblings inside the enclosing specialization
            // scope. Recurse into both branches so the visitor sees their
            // members at the same logical level as the parent's other
            // direct children.
            MemberDecl::GuardedGroup(g) => {
                walk_members_depth(&g.members, visitor, depth + 1);
                walk_members_depth(&g.else_members, visitor, depth + 1);
            }
            _ => {}
        }
    }
}

fn find_named_member_span_depth<'a>(
    members: &'a [MemberDecl],
    name: &str,
    depth: usize,
) -> Option<MemberSpanInfo<'a>> {
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return None;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) if p.name == name => {
                return Some(MemberSpanInfo {
                    span: p.span,
                    doc: p.doc.as_deref(),
                });
            }
            MemberDecl::Let(l) if l.name == name => {
                return Some(MemberSpanInfo {
                    span: l.span,
                    doc: l.doc.as_deref(),
                });
            }
            MemberDecl::GuardedGroup(g) => {
                if let Some(result) = find_named_member_span_depth(&g.members, name, depth + 1) {
                    return Some(result);
                }
                if let Some(result) = find_named_member_span_depth(&g.else_members, name, depth + 1)
                {
                    return Some(result);
                }
            }
            MemberDecl::Port(port) => {
                if let Some(result) = find_named_member_span_depth(&port.members, name, depth + 1) {
                    return Some(result);
                }
            }
            _ => {}
        }
    }
    None
}

/// `connect a -> b : BoltSet { grade = 8.8  shaft -> input_bore }`
#[derive(Debug, Clone)]
pub struct ConnectDecl {
    pub left: PortRef,
    pub operator: ConnectOp,
    pub right: PortRef,
    pub connector_type: Option<String>,
    pub params: Vec<(String, Expr)>,
    pub port_mappings: Vec<(String, String)>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A reference to a port, possibly via member access (e.g., `motor.shaft`).
#[derive(Debug, Clone)]
pub struct PortRef {
    pub expr: Expr,
}

/// Direction of a connect statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectOp {
    /// `->`
    Forward,
    /// `<-`
    Reverse,
    /// `<->`
    Bidirectional,
}

impl ConnectOp {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// `chain a -> b -> c`
#[derive(Debug, Clone)]
pub struct ChainDecl {
    pub elements: Vec<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `meta { description = "A bracket", part_number = "BR-001" }`
#[derive(Debug, Clone)]
pub struct MetaBlockDecl {
    pub entries: Vec<(String, String)>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// `forall v in coll: connect ...` or `forall v in coll: chain ...`
///
/// The body is a connect-class statement applied per element of `collection`.
#[derive(Debug, Clone)]
pub struct ForallConnectDecl {
    /// The bound variable name (e.g. `"v"` in `forall v in coll: ...`).
    pub variable: String,
    /// The collection expression iterated over.
    pub collection: Expr,
    /// The per-element connect or chain body.
    pub body: ForallConnectBody,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// Body alternatives for a `forall ... : <connect-class>` statement.
#[derive(Debug, Clone)]
pub enum ForallConnectBody {
    /// `forall v in coll: connect v.a -> b.c`
    Connect(ConnectDecl),
    /// `forall v in coll: chain v.a -> b -> c`
    Chain(ChainDecl),
}

/// `forall v in coll: constraint ...` or `forall v in coll: constraint Inst(...)`
///
/// The body is a constraint-class declaration applied per element of `collection`.
#[derive(Debug, Clone)]
pub struct ForallConstraintDecl {
    /// The bound variable name (e.g. `"v"` in `forall v in coll: ...`).
    pub variable: String,
    /// The collection expression iterated over.
    pub collection: Expr,
    /// The per-element constraint or constraint instantiation body.
    pub body: ForallConstraintBody,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// Body alternatives for a `forall ... : <constraint-class>` statement.
#[derive(Debug, Clone)]
pub enum ForallConstraintBody {
    /// `forall v in coll: constraint v.mass < 50`
    Constraint(ConstraintDecl),
    /// `forall v in coll: constraint MinDist(point: v.center)`
    Instantiation(ConstraintInstDecl),
}

/// The kind of import (determines how names are brought into scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// `import std.math` — import entire module
    Module,
    /// `import std.math.sqrt` — import a single entity
    Entity(String),
    /// `import std.mech.{Bolt, Nut}` — import multiple entities
    Destructured(Vec<String>),
    /// `import std.mech as m` — import module with alias
    Aliased { alias: String },
    /// `import std.mech.Bolt as StdBolt` — import entity with alias
    EntityAliased { entity: String, alias: String },
}

/// `import std.mechanical.fasteners`
#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// Dot-separated module path (e.g., "std.math")
    pub path: String,
    /// What form of import this is
    pub kind: ImportKind,
    /// Whether this is a re-export (`pub import ...`)
    pub is_pub: bool,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `enum Direction { In, Out, Bidi }`
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub variants: Vec<String>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `fn area(w: Scalar, h: Scalar) -> Scalar { w * h }`
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub params: Vec<FnParam>,
    pub return_type: Option<TypeExpr>,
    pub body: FnBody,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `trait Rigid { param mass : Mass }`
#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub refinements: Vec<SpannedIdent>,
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Block-level pragmas inside this trait.
    pub pragmas: Vec<Pragma>,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `field def temp : Point3 -> Scalar { source = analytical { |p| p } }`
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub is_pub: bool,
    pub domain_type: TypeExpr,
    pub codomain_type: TypeExpr,
    pub source: FieldSource,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `purpose mfg_ready(subject : Structure) { constraint ... }`
#[derive(Debug, Clone)]
pub struct PurposeDef {
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub params: Vec<PurposeParam>,
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Block-level pragmas inside this purpose.
    pub pragmas: Vec<Pragma>,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// A purpose parameter binding an entity reference: `subject : Structure`
#[derive(Debug, Clone)]
pub struct PurposeParam {
    pub name: String,
    pub entity_kind: String,
    pub span: SourceSpan,
}

/// `constraint def MinWallThickness { param wall : Length  wall >= process.min_wall }`
///
/// A named, parameterized constraint definition at the top level.
/// The body consists of `param` declarations (the constraint's free variables)
/// and bare expression predicates (the constraint assertions, forming a conjunction).
#[derive(Debug, Clone)]
pub struct ConstraintDef {
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub params: Vec<ParamDecl>,
    pub predicates: Vec<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Block-level pragmas inside this constraint def.
    pub pragmas: Vec<Pragma>,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

impl ConstraintDef {
    /// Returns `true` if this constraint def is tagged with the `@test` annotation.
    ///
    /// Callers can use this instead of scanning `annotations` manually.
    /// Symmetric with `TopologyTemplate::is_test()`.
    // TODO: Once constraint-def lowering lands, this moves to CompiledConstraintDef::is_test.
    pub fn is_test(&self) -> bool {
        self.annotations
            .iter()
            .any(|a| a.name == reify_types::annotation::TEST_ANNOTATION)
    }
}

/// A unit declaration: `unit meter : Length` or `unit degC : Temperature = 1 offset 273.15`
///
/// Declares a named measurement unit with an optional conversion factor and offset.
/// The `dimension_type` identifies the physical dimension (e.g., `Length`, `Temperature`).
/// The `conversion` expression gives the SI multiplier (e.g., `0.001` for mm→m).
/// The `offset` expression gives an additive offset for affine units (e.g., 273.15 for °C→K).
#[derive(Debug, Clone)]
pub struct UnitDecl {
    pub name: String,
    pub is_pub: bool,
    pub dimension_type: TypeExpr,
    pub conversion: Option<Expr>,
    pub offset: Option<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// A type alias declaration: `type Pressure = Force / Area`
///
/// Declares a named type alias, optionally with type parameters.
/// The `type_expr` is the aliased type, which can be a simple type, parameterized type,
/// or a dimensional type expression using `*` and `/` operators.
#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub type_expr: TypeExpr,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// The source kind for a field declaration.
#[derive(Debug, Clone)]
pub enum FieldSource {
    /// `analytical { |p| expr }` — a lambda defining the field analytically.
    Analytical { expr: Expr },
    /// `sampled { resolution = 100  interpolation = linear }` — sampled data with config.
    Sampled { config: Vec<(String, Expr)> },
    /// `composed { |f, g| |p| f(g(p)) }` — composition of fields.
    Composed { expr: Expr },
    /// `imported { "path/to/data.vtu" }` — imported from external file.
    Imported { path: String },
}

/// A type parameter declaration: `T`, `T: Numeric`, or `T: Numeric = Int`
#[derive(Debug, Clone)]
pub struct TypeParamDecl {
    pub name: String,
    pub bounds: Vec<String>,
    pub default: Option<TypeExpr>,
    pub span: SourceSpan,
}

/// A function parameter: `w: Scalar`
#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: String,
    pub type_expr: TypeExpr,
    pub span: SourceSpan,
}

/// A function body: let bindings followed by a result expression.
#[derive(Debug, Clone)]
pub struct FnBody {
    pub let_bindings: Vec<LetDecl>,
    pub result_expr: Expr,
}

/// An associated type declaration: `type Material = Steel`
#[derive(Debug, Clone)]
pub struct AssociatedTypeDecl {
    pub name: String,
    pub default_type: Option<TypeExpr>,
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
    QuantityLiteral { value: f64, unit: String },
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
}

/// A match arm: `Pattern1 | Pattern2 => body`
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub patterns: Vec<String>,
    pub body: Expr,
    pub span: SourceSpan,
}

/// A lambda parameter: `x` or `x: Real`
#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub name: String,
    pub type_expr: Option<TypeExpr>,
    pub span: SourceSpan,
}

/// The kind of quantifier: universal (forall) or existential (exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// What a [`TypeExpr`] actually is — a named type or a binary dimensional operation.
#[derive(Debug, Clone)]
pub enum TypeExprKind {
    /// A named type with optional type arguments (e.g., `Scalar`, `Box<T>`, `Map<K, V>`).
    Named { name: String, type_args: Vec<TypeExpr> },
    /// A binary dimensional operator applied to two type expressions (e.g., `Force / Area`).
    DimensionalOp {
        op: DimOp,
        left: Box<TypeExpr>,
        right: Box<TypeExpr>,
    },
}

/// A type expression in the AST (e.g., `Scalar`, `Bool`, `Box<T>`, `Force / Area`).
#[derive(Debug, Clone)]
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
        }
    }
}

/// A pragma directive: `#name` or `#name(args)`.
///
/// Pragmas are metadata directives that appear at module level or inside block scopes.
/// They do not affect the semantics of declarations but can influence compiler passes.
#[derive(Debug, Clone)]
pub struct Pragma {
    pub name: String,
    pub args: Vec<PragmaArg>,
    pub span: SourceSpan,
}

/// A single pragma argument: either `key=value` or a bare value.
#[derive(Debug, Clone)]
pub enum PragmaArg {
    /// `key = value`
    KeyValue { key: String, value: PragmaValue },
    /// bare value (no key)
    Bare(PragmaValue),
}

/// A restricted pragma value (compile-time constant only).
#[derive(Debug, Clone, PartialEq)]
pub enum PragmaValue {
    Ident(String),
    Number(f64),
    String(String),
    Bool(bool),
    /// A dimensioned quantity literal, e.g. `0.001m` or `1mm`.
    ///
    /// `value` is the bare number from the source, `unit` is the trailing
    /// identifier (no whitespace between them per the grammar). Conversion
    /// to SI is done by consumers (e.g. `unit_to_scalar`) — `PragmaValue` is
    /// intentionally a dumb wire representation.
    Quantity { value: f64, unit: String },
}

/// An annotation directive: `@name` or `@name(expr, ...)`.
///
/// Annotations appear immediately before a top-level declaration and are
/// attached to it during lowering via a pending-annotations accumulator.
/// Args are full expressions (not restricted to compile-time constants).
#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<Expr>,
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
