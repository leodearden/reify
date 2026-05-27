//! Parsed declaration AST and parser-produced helpers (Annotation, Pragma, NumberClass).
//! Phase 2 ε of docs/prds/core-ast-ir-layering.md — relocated from reify-syntax/lib.rs.
//!
//! References only reify-core primitives
//! (SourceSpan/ContentHash/PortDirection/SpannedIdent/ModulePath/TEST_ANNOTATION)
//! and the in-crate Expr/TypeExpr from `reify_ast::ast`.
//!
//! Critically: NO ir-tier type references — `cargo build -p reify-ast` enforces this
//! and the dag_invariant.rs test pins it at the Cargo.toml level.

use reify_core::{ContentHash, ModulePath, PortDirection, SourceSpan, SpannedIdent};

use crate::ast::{Expr, TypeExpr};

/// A parsed module — the output of the parser.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: ModulePath,
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
    /// `match <discriminant> { Pattern => <member> ... }` at decl level (task 2372).
    ///
    /// Represents a cluster of same-name declarations produced by an exhaustive
    /// `match` block. See PRD `docs/prds/match-block-decls.md` task 1 and spec §6.4.
    /// Tree-sitter grammar (task 3563) and ts_parser lowering (task 3564) are both
    /// wired; integration tests covering the parse → AST → compile pipeline live in
    /// `crates/reify-compiler/tests/match_block_decl_lowering_tests.rs`.
    /// Some legacy hand-built tests remain in `match_arm_decl_group_compile_tests.rs`
    /// for AST-shape granularity.
    MatchArmDeclGroup(MatchArmDeclGroupDecl),
}

/// A `match <discriminant> { Pattern => <member> ... }` declaration block (task 2372).
///
/// Produces a cluster of same-name guarded declarations when compiled. Each
/// arm's guard is desugared to `discriminant == EnumType.Variant` (spec §6.4).
#[derive(Debug, Clone)]
pub struct MatchArmDeclGroupDecl {
    /// The expression whose variant value selects the active arm (e.g. `head_type`).
    pub discriminant: Expr,
    /// The match arms, in source order.
    pub arms: Vec<MatchArmDeclArmDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A single arm inside a `MatchArmDeclGroupDecl` (task 2372).
///
/// `patterns` uses `Vec<String>` to align with the existing `MatchArm.patterns`
/// shape in this module. A `|`-pipe form collapses multiple variant idents into a
/// single arm's `patterns` list.
#[derive(Debug, Clone)]
pub struct MatchArmDeclArmDecl {
    /// One or more variant ident strings (pipe-collapsed into a single arm).
    pub patterns: Vec<String>,
    /// The per-arm declaration (e.g. a `Sub` whose name is shared across all arms).
    pub member: Box<MemberDecl>,
    pub span: SourceSpan,
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
    /// `None` for bare instantiation, collection, or bare-colon-no-body forms.
    ///
    /// Both the grammar (task 3569) and the CST→AST lowering (task 3571) are
    /// wired. `param_assignment` nodes inside the body are currently dropped
    /// during lowering — their full round-trip is tracked by task 3573.
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
/// `GuardedGroup.members`, `GuardedGroup.else_members`, `Port.members`,
/// and each arm's `member` inside `MatchArmDeclGroup` so that declarations
/// inside `where cond { ... } else { ... }` blocks, port bodies, and
/// match-arm clusters are found. Recursion is bounded by
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
///   * `MemberDecl::MatchArmDeclGroup(g)` — each arm's `member` (spec §6.4,
///     task 2372). The group node is visited first, then each arm's member.
///
/// The walker does NOT recurse into `PortDecl.members`; port bodies have
/// their own grammar and are themselves forbidden inside a specialization
/// scope (the rejection rule lives in task 2369). Recursion is bounded by
/// [`MAX_MEMBER_NESTING_DEPTH`] to prevent stack overflow on pathological
/// input — same convention as [`find_named_member_span`].
///
/// **Asymmetry note:** [`find_named_member_span`] DOES recurse into
/// `PortDecl.members` but does NOT recurse into `SubDecl.body`. These two
/// helpers have divergent contracts that are individually correct but can
/// surprise callers who infer one from the other. A future consolidation
/// (shared `walk_members` helper parameterized by `visit_port_body /
/// visit_sub_body` flags) would unify them; deferred to task η or later.
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
    if depth >= MAX_MEMBER_NESTING_DEPTH {
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
            // Spec §6.4 (task 2372): match-arm decl clusters desugar each arm
            // to a same-name guarded decl. Recurse into each arm's member so
            // the visitor sees per-arm declarations as children of the group.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    walk_members_depth(std::slice::from_ref(&*arm.member), visitor, depth + 1);
                }
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
    if depth >= MAX_MEMBER_NESTING_DEPTH {
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
            // Spec §6.4 (task 2372): recurse into each arm's member to find
            // named declarations inside match-arm clusters.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    if let Some(result) = find_named_member_span_depth(
                        std::slice::from_ref(&*arm.member),
                        name,
                        depth + 1,
                    ) {
                        return Some(result);
                    }
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
        has_test_annotation(&self.annotations)
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
    /// `imported { path = "..." format = OpenVDB grid = "..." }` — imported from external file.
    ///
    /// All three fields are optional at the parser level so that partial blocks still produce
    /// a structured AST. The compiler (task 2666) emits "missing path/format/grid" diagnostics.
    ///
    /// ## Design note: typed fields vs `Vec<(String, Expr)>`
    ///
    /// Unlike [`FieldSource::Sampled`], which carries a generic `Vec<(String, Expr)>` and defers
    /// all key validation to the compiler, `Imported` uses typed `Option<String>` fields. This is
    /// a deliberate choice: `Imported` has three known runtime consumers (path → file I/O,
    /// format → kernel selection, grid → grid-name lookup) that benefit from structured access.
    ///
    /// The trade-off is that unknown keys and type-mismatched values (e.g. `path = OpenVDB`) are
    /// silently dropped at parse time with no extras field to recover them. The compiler can
    /// observe `None` for those fields but cannot distinguish "absent key" from "wrong-type key".
    /// Precise wrong-type diagnostics are therefore out of scope for task 2666's compile phase
    /// unless this variant is later migrated to a `Vec`-based shape (which would break all
    /// `FieldSource::Imported { path, .. }` match sites).
    Imported {
        path: Option<String>,
        format: Option<String>,
        grid: Option<String>,
    },
}

/// A type parameter declaration: `T`, `T: Numeric`, or `T: Numeric = Int`
#[derive(Debug, Clone)]
pub struct TypeParamDecl {
    pub name: String,
    pub bounds: Vec<String>,
    pub default: Option<TypeExpr>,
    pub span: SourceSpan,
}

/// A function parameter: `w: Scalar` or `w: Scalar = default_expr`
#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: String,
    pub type_expr: TypeExpr,
    pub default: Option<Expr>,
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

/// Classification of a numeric literal as Int or Real.
///
/// Returned by [`classify_number_literal`] to centralize the Int/Real
/// boundary so that compiler call sites (literal lowering in
/// `reify-compiler/src/expr.rs` and annotation arg lowering in
/// `reify-compiler/src/annotations.rs`) cannot drift from each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumberClass {
    Int(i64),
    Real(f64),
    /// An integer-form token whose f64 value is non-finite or does not round-trip
    /// cleanly through i64 (e.g. `99999999999999999999` → `f64::INFINITY`).
    /// The caller **must** emit a precision-loss diagnostic for this variant.
    LossyReal(f64),
}

/// Classify a parsed numeric literal as `Int`, `Real`, or `LossyReal`,
/// matching the AST's `is_real` flag and detecting integer-form tokens whose
/// f64 value cannot cleanly represent the source integer.
///
/// Branch semantics:
///
/// * `is_real == true` → always `Real(value)`. The parser sets `is_real`
///   when the source token contains `.`, `e`, or `E`. A whole-number
///   real literal like `1.0` stays Real (Int→Real widening at annotated-let
///   injection sites covers `let x : Real = 42`).
/// * `is_real == false` and the f64 round-trips cleanly through `i64`
///   (i.e. `value.is_finite() && value == (value as i64) as f64`) →
///   `Int(value as i64)`.
/// * `is_real == false` otherwise → `LossyReal(value)`. This path is
///   reachable in production: an integer-form token too long to fit in f64
///   (e.g. `99999999999999999999`, 20-digit integers) parses to `f64::INFINITY`
///   or a finite f64 that does not round-trip through i64. Callers **must**
///   emit a precision-loss diagnostic when they receive `LossyReal` — the
///   variant's purpose is to make the lossiness visible at the type level so
///   call sites cannot silently ignore it. The f64 payload should be used as
///   the runtime value (preserving current behavior), but the diagnostic is
///   required.
///
/// This is the single source of truth for the Int/Real boundary on
/// `ExprKind::NumberLiteral`; both `compile_expr_guarded` and
/// `lower_annotations` delegate here.
pub fn classify_number_literal(value: f64, is_real: bool) -> NumberClass {
    if is_real {
        NumberClass::Real(value)
    } else if value.is_finite() && value == (value as i64) as f64 {
        NumberClass::Int(value as i64)
    } else {
        NumberClass::LossyReal(value)
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
    Quantity {
        value: f64,
        unit: String,
    },
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

/// Returns true if the slice contains a `@test` annotation.
///
/// The parser-produced parallel of `reify_types::annotation::has_test_annotation`
/// (which operates on the compiled Annotation); this one operates on the
/// parser-produced Annotation (args: Vec<Expr>).
pub fn has_test_annotation(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| a.name == reify_core::TEST_ANNOTATION)
}

/// A parse error.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: SourceSpan,
}

#[cfg(test)]
mod number_class_tests {
    use super::{classify_number_literal, NumberClass};

    #[test]
    fn is_real_true_whole_number_stays_real() {
        // Whole-number token written with `.` (e.g. `1.0`) must stay Real.
        assert_eq!(classify_number_literal(1.0, true), NumberClass::Real(1.0));
    }

    #[test]
    fn is_real_true_clean_i64_value_stays_real() {
        // Even if the value would round-trip cleanly as i64, is_real=true wins.
        assert_eq!(classify_number_literal(42.0, true), NumberClass::Real(42.0));
    }

    #[test]
    fn is_real_false_clean_i64_becomes_int() {
        // Bare integer token `42` → Int(42).
        assert_eq!(classify_number_literal(42.0, false), NumberClass::Int(42));
    }

    #[test]
    fn is_real_false_zero_becomes_int() {
        // Zero edge case.
        assert_eq!(classify_number_literal(0.0, false), NumberClass::Int(0));
    }

    #[test]
    fn is_real_false_negative_clean_i64_becomes_int() {
        // Sign-symmetric: negative clean i64 should also produce Int.
        assert_eq!(classify_number_literal(-5.0, false), NumberClass::Int(-5));
    }

    #[test]
    fn is_real_false_nan_classifies_as_lossy_real() {
        // NaN is not finite → LossyReal fallback.
        let result = classify_number_literal(f64::NAN, false);
        assert!(matches!(result, NumberClass::LossyReal(v) if v.is_nan()));
    }

    #[test]
    fn is_real_false_infinity_classifies_as_lossy_real() {
        // Inf is not finite → LossyReal fallback.
        assert_eq!(
            classify_number_literal(f64::INFINITY, false),
            NumberClass::LossyReal(f64::INFINITY)
        );
    }

    #[test]
    fn is_real_false_overflow_past_i64_max_classifies_as_lossy_real() {
        // 1e20 cannot be represented as i64; the round-trip check fails.
        // The classifier must return LossyReal, not Real, so callers know to warn.
        assert_eq!(
            classify_number_literal(1e20, false),
            NumberClass::LossyReal(1e20)
        );
    }
}

#[cfg(test)]
mod has_test_annotation_tests {
    use super::{Annotation, has_test_annotation};
    use reify_core::SourceSpan;

    #[test]
    fn empty_slice_returns_false() {
        assert!(!has_test_annotation(&[]));
    }

    #[test]
    fn test_annotation_returns_true() {
        let ann = Annotation { name: "test".into(), args: vec![], span: SourceSpan::empty(0) };
        assert!(has_test_annotation(&[ann]));
    }

    #[test]
    fn non_test_annotation_returns_false() {
        let ann = Annotation {
            name: "deprecated".into(),
            args: vec![],
            span: SourceSpan::empty(0),
        };
        assert!(!has_test_annotation(&[ann]));
    }

    #[test]
    fn test_among_multiple_returns_true() {
        let anns = vec![
            Annotation { name: "deprecated".into(), args: vec![], span: SourceSpan::empty(0) },
            Annotation { name: "test".into(), args: vec![], span: SourceSpan::empty(0) },
        ];
        assert!(has_test_annotation(&anns));
    }
}
