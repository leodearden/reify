//! Parsed declaration AST and parser-produced helpers (Annotation, Pragma, NumberClass).
//! Phase 2 ε of docs/prds/core-ast-ir-layering.md — relocated from reify-syntax/lib.rs.
//!
//! References only reify-core primitives
//! (SourceSpan/ContentHash/PortDirection/SpannedIdent/ModulePath/TEST_ANNOTATION)
//! and the in-crate Expr/TypeExpr from `reify_ast::ast`.
//!
//! Critically: NO ir-tier type references — `cargo build -p reify-ast` enforces this
//! and the dag_invariant.rs test pins it at the Cargo.toml level.

use std::convert::Infallible;
use std::ops::ControlFlow;

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
    /// Declared module path from a top-of-file `module a.b.c` declaration, if present.
    ///
    /// `None` for files without a module declaration (the entire existing corpus).
    /// `Some(path)` when the parser found a `module_declaration` node at the top.
    /// This is the structured form of `ModuleDecl.path`; the raw dotted string is
    /// stored in `Declaration::Module(ModuleDecl)` in the declarations list.
    ///
    /// Left untouched for task γ (path-vs-location enforcement); `ParsedModule.path`
    /// (the resolver-derived path) is the authoritative module identity (PRD D-6).
    pub declared_module_path: Option<ModulePath>,
}

impl ParsedModule {
    /// Render every parse error against `source` as `line:col: message`, in parse order.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
    /// The three production surfaces that print parse errors — the two loops in
    /// `reify-cli/src/main.rs` and `CliToolContext::load_file` — all want exactly this, and
    /// this is where they get it, so no caller has to rebuild the loop or re-derive positions
    /// its own way.
    ///
    /// `source` must be the string the spans index (the one handed to the parser); a different
    /// string yields wrong positions, never a panic. Batched via
    /// [`ParseError::render_all`], so the newline table is built once for the whole list
    /// rather than once per diagnostic.
    ///
    /// Exists as a METHOD rather than leaving callers to write
    /// `reify_ast::ParseError::render_all(&parsed.errors, source)` because `reify-cli` does not
    /// depend on `reify-ast` and so cannot name that path. The only route that resolves today
    /// is `reify_syntax::ParseError`, via the `pub use reify_ast::*` block that
    /// `reify-syntax/src/lib.rs` marks TRANSIENT and slates for removal by the PRD task η
    /// follow-up — building three new production call sites on a re-export scheduled for
    /// deletion would hand that sweep a breakage. Method resolution needs no crate path.
    pub fn render_errors(&self, source: &str) -> Vec<String> {
        ParseError::render_all(&self.errors, source)
    }
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
    /// `default Material = steel` — ambient-default declaration.
    ///
    /// Grammar producer only (task A). Semantics (scope resolution, injection
    /// into structures) are deferred to task B.
    Default(DefaultDecl),
    /// A `module a.b.c` declaration at the top of a file.
    ///
    /// Positional: placed via the grammar's `source_file: seq(optional($.module_declaration),
    /// repeat($._declaration))` rule, so a `module` decl after any other declaration is a
    /// parse ERROR. No enforcement semantics here — task γ reads `declared_module_path`.
    Module(ModuleDecl),
    /// `joint revolute(a: Axis, b: Axis) with angle: Angle in 0deg..120deg = { … }`
    ///
    /// Grammar producer only (task α 4395). Semantics (DOF self-check, body
    /// type-check against Type::Relation, validate_range) are deferred to task β.
    Joint(JointDef),
}

/// `module company.products.actuators` — a top-of-file module path declaration.
///
/// Mirrors `ImportDecl.path` in using a raw dotted `String` as the wire
/// representation. The structured form `ModulePath` is stored alongside in
/// `ParsedModule.declared_module_path` (parsed via `ModulePath::from_dotted`).
#[derive(Debug, Clone)]
pub struct ModuleDecl {
    /// Dot-separated module path string exactly as written in source (e.g., "a.b.c").
    pub path: String,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
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
    /// An associated function declared inside a trait body:
    /// `fn area(self) -> Scalar { ... }` (with body) or
    /// `fn req(self) -> Real` (bodyless / required, `body = None`).
    Fn(FnDef),
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
    /// `crates/reify-compiler/tests/harness_patterns/match_block_decl_lowering_tests.rs`.
    /// Some legacy hand-built tests remain in `match_arm_decl_group_compile_tests.rs`
    /// for AST-shape granularity.
    MatchArmDeclGroup(MatchArmDeclGroupDecl),
    /// A member-level `relate { … }` block: a flat set of geometric relation
    /// expressions (geometric-relations v0_6, design §4/§5; task δ 4384).
    ///
    /// Each relation must type to `Type::Relation`; the compiler enforces this
    /// with `E_RELATE_EXPECTS_RELATION`. The inline `sub … at … where { }` form
    /// carries the same flat relation set on `SubDecl.relate_relations` instead
    /// of producing a separate `MemberDecl::Relate`.
    Relate(RelateDecl),
}

/// A `relate { concentric(…)  flush(…) }` member block (task δ 4384).
///
/// Holds the relation expressions in source order. Mirrors the bare-expression
/// shape of `ConstraintDef.predicates` — separation between members is handled
/// by the grammar (GLR), so no separator token is stored. An empty `relate { }`
/// lowers to `relations: vec![]`.
#[derive(Debug, Clone)]
pub struct RelateDecl {
    /// The relation expressions, in source order. Each must type to
    /// `Type::Relation` (compiler enforcement, task δ step-14).
    pub relations: Vec<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
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

/// `param width: Length = 80mm`
#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub doc: Option<String>,
    /// Whether this param is marked `priv` (PRD §D-3/D-4: private to the structure).
    /// `priv param` is hidden from importers; default-visible params have `is_priv == false`.
    pub is_priv: bool,
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
    /// Whether this binding is marked `priv` (PRD §4 D-3; task 4755).
    /// `let` members are already default-private (not importable), so `priv let` is
    /// syntactically redundant — `is_priv == true` is consumed by task δ (#3978) to
    /// emit E_PRIV_REDUNDANT. Default-visible (non-`priv`) bindings have `is_priv == false`.
    pub is_priv: bool,
    /// Whether this binding is marked `aux` (PRD §2.1: auxiliary geometry).
    /// `aux let` declares a derived binding that is not surfaced in the public
    /// interface but participates in constraint solving.
    pub is_aux: bool,
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
    /// Whether this constraint is marked `priv` (PRD §4 D-3; task 4755).
    /// `constraint` members are already default-private (not importable), so `priv constraint`
    /// is syntactically redundant — `is_priv == true` is consumed by task δ (#3978) to
    /// emit E_PRIV_REDUNDANT. Default-visible constraints have `is_priv == false`.
    pub is_priv: bool,
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

/// A single entry in a keyed sub-member block (task 3929, PRD §2.2).
///
/// Represents one `"key" => { overrides }` entry inside a
/// `sub name : Keyed<T> { "k1" => { … }  "k2" => { … } }` declaration.
///
/// `key` is the unquoted string key (e.g. `"intake"` in the source becomes
/// `key = "intake"` here, with the surrounding double-quotes stripped).
/// `overrides` reuses the same `Vec<MemberDecl>` shape as a specialization
/// body (PRD §2.2/§9-Q4 — no new override grammar).
///
/// Keyed TYPE kind recognition, NodeId identity, E_DUP_MEMBER_KEY,
/// resolution, eval, connect, and structural-classifier are deferred to
/// downstream tasks (PRD tasks β/γ/δ/ε); only the grammar + AST shape +
/// lowering are in scope here.
#[derive(Debug, Clone)]
pub struct KeyedSubMemberEntry {
    pub key: String,
    pub overrides: Vec<MemberDecl>,
    /// Per-key parameter overrides from the entry's `{ param = value }` block
    /// (task 3931 γ).  E.g. `"intake" => { area = 5mm }` →
    /// `[("area", Expr { kind: QuantityLiteral { value: 5.0, unit: mm }, .. })]`.
    ///
    /// Mirrors `SubDecl.spec_param_overrides`: the entry's `overrides`
    /// specialization-body carries `param_assignment` nodes that
    /// `lower_specialization_body_members` drops (it returns only `_member`
    /// decls), so they are collected here separately via `lower_binding_value`.
    /// Empty when the entry's body has no `param = value` assignments. The
    /// compiler compiles these in the parent scope into
    /// `SubComponentDecl.keyed_member_overrides` and applies them as per-key
    /// elaboration args.
    pub param_overrides: Vec<(String, Expr)>,
    pub span: SourceSpan,
}

/// `sub mount_hole = Hole(diameter: 6mm)` or `sub part = Box<Bolt>()`
///
/// A sub carries its spec §8.7 specialization overrides in one of two mutually
/// exclusive shapes: `body: Some(members)` (`sub motor : T { … }`), or one
/// `keyed_members[].overrides` list per key. `body: None` therefore does NOT
/// mean "no specialization scope" — it is also the keyed form.
///
/// [`walk_specialization_scope_members`] is the single discriminator for "does
/// this sub open a specialization scope?", and the traversal contract for both
/// shapes. Do not re-derive it from a field test: `body.is_some()` is false by
/// construction for the keyed form, which is exactly how a keyed sub stayed
/// invisible to the compiler's §8.7 check until task 6958.
#[derive(Debug, Clone)]
pub struct SubDecl {
    pub name: String,
    pub structure_name: String,
    pub type_args: Vec<TypeExpr>,
    pub args: Vec<(String, Expr)>,
    pub is_collection: bool,
    pub where_clause: Option<WhereClause>,
    /// Members of the NON-KEYED specialization body, when this `sub` opens
    /// one. `None` for the keyed form (see `keyed_members`) as well as for the
    /// bare instantiation, collection, and bare-colon-no-body forms — so
    /// `is_none()` is not a "opens no specialization scope" test.
    ///
    /// Both the grammar (task 3569) and the CST→AST lowering (task 3571) are
    /// wired. `param_assignment` nodes inside the body are currently dropped
    /// during lowering — their full round-trip is tracked by task 3573.
    pub body: Option<Vec<MemberDecl>>,
    /// Parameter overrides from the specialization-body (`specialization_body`
    /// grammar rule).  E.g. `sub b : Bearing { bore = auto }` →
    /// `[("bore", Expr { kind: Auto { free: false }, .. })]`.
    ///
    /// Named `spec_param_overrides` to tie it to the `specialization_body`
    /// grammar term and disambiguate from the unrelated runtime
    /// `Engine::param_overrides` (task 4123 S5).  Mirrors `args` (named
    /// constructor arguments) but for the colon-form specialization body's
    /// `param_assignment` nodes.  All values — both `auto`/`auto(free)` and
    /// non-auto expressions — are collected here so the AST is a complete
    /// representation of the source.  The compiler acts only on
    /// `ExprKind::Auto` entries in this task (γ = task 3806); non-auto
    /// resolution is deferred to task ε.  Empty for bare instantiation, the
    /// paren-arg form, or any sub with no param_assignment overrides.
    pub spec_param_overrides: Vec<(String, Expr)>,
    /// Keyed sub-member entries when this `sub` uses a keyed block
    /// `{ "k" => { overrides } }` (task 3929, PRD §2.2).
    ///
    /// Empty when the sub is NOT a keyed block (instantiation, collection,
    /// bare-colon-no-body, or specialization-body forms). Non-empty only when
    /// the `body` field child in the CST was a `keyed_member_block`.
    ///
    /// `body` is `None` when `keyed_members` is non-empty (the two
    /// discriminators are mutually exclusive by construction in `lower_sub`).
    pub keyed_members: Vec<KeyedSubMemberEntry>,
    /// Whether this sub-component is marked `aux` (PRD §2.1: auxiliary placement).
    /// `aux sub` declares a sub-component used for internal geometry only,
    /// not surfaced in the public component interface.
    ///
    /// Parsed and stored here (task 3899); first consumed by the T2
    /// sub-placement compiler lowering task.
    pub is_aux: bool,
    /// Whether this sub-component is marked `priv` (PRD §D-3/D-4: private to the structure).
    /// `priv sub` is hidden from importers; default-visible subs have `is_priv == false`.
    pub is_priv: bool,
    /// Optional placement pose expression from the `at <expr>` clause (PRD §2.2).
    /// `None` when no `at` clause is present; `Some(expr)` when the sub-component
    /// carries an explicit placement frame or transform.
    ///
    /// Parsed and stored here (task 3899); first consumed by the T2
    /// sub-placement compiler lowering task. Note: `pose_expr.is_some()` on a
    /// collection-form `SubDecl` (`is_collection == true`) is grammatically
    /// accepted but semantically invalid — the compiler (T2) must reject it
    /// with a diagnostic (PRD §10).
    pub pose_expr: Option<Expr>,
    /// Binder of the indexer clause — the `i` in `sub idlers[i in 0..4] = …`
    /// (indexed-sub-instantiation.md §3.1, task α). The CST field is named
    /// `binder`; the `index_` prefix here ties it to `index_domain` and
    /// disambiguates from the unrelated `where_clause` and quantifier binders
    /// elsewhere in the AST.
    ///
    /// `Some(_)` only for the indexed instantiation form; `None` for the bare
    /// instantiation, collection, and specialization arms. Paired with
    /// `index_domain`: both are `Some` or both are `None`. The grammar makes the
    /// clause one indivisible `optional(seq(…))` (PRD §9.1 Q1 is decided at α
    /// as: no binder-omission form), but the type does not encode the pairing,
    /// so it is enforced at the single producer: `ts_parser::lower_sub` lowers
    /// the two halves JOINTLY and, if the domain expression fails to lower,
    /// drops BOTH halves and emits a diagnostic rather than emitting a
    /// half-populated pair. A consumer may therefore rely on the pairing (β's
    /// domain typing does), and any new producer must uphold it.
    ///
    /// A `SpannedIdent` rather than a bare `String` because the span must cover
    /// the binder token ALONE for an unused-binder (`W_UNUSED`-conventions)
    /// diagnostic to underline exactly it.
    ///
    /// Parsed and stored here (task α); binder scoping is first consumed by
    /// task β.
    ///
    /// A populated pair always travels with an interim `#5482` diagnostic —
    /// rationale at the `TODO(#5482)` site in `ts_parser::lower_sub`.
    pub index_binder: Option<SpannedIdent>,
    /// Domain expression of the indexer clause — the `0..4` in
    /// `sub idlers[i in 0..4] = …` (indexed-sub-instantiation.md §3.1, task α).
    /// The CST field is named `domain`.
    ///
    /// `Some(_)` only for the indexed instantiation form; see `index_binder`
    /// for the pairing invariant. α stores this **syntactically only** — it is
    /// any `$._expression`, not yet checked to be a range. Typing it as
    /// `Range<Int>` and deriving the collection count cell from it are task β's
    /// job; α deliberately leaves `is_collection == false` so an indexed sub
    /// cannot reach the existing collection-sub compile path with no count cell
    /// and no element template.
    pub index_domain: Option<Expr>,
    /// Inline relate-block relations from the trailing `at … where { }` form
    /// (geometric-relations v0_6, design §4/§5; task δ 4384). Empty unless the
    /// sub carries an inline `where { … }` relate-block after its `at <pose>`
    /// clause. Carries the SAME flat relation set a member-level `relate { }`
    /// would (`MemberDecl::Relate`); both homes enforce `Type::Relation`
    /// identically (`E_RELATE_EXPECTS_RELATION`).
    pub relate_relations: Vec<Expr>,
    /// The derivation clause of a DERIVED sub — `sub b = mirror of a across
    /// <plane> { … }` / `sub b = image of a under <transform> { … }`
    /// (assembly-derivation-toolbox.md, leaf A-alpha, task #6615).
    ///
    /// `Some(_)` only for the derived arm; `None` for the bare instantiation,
    /// collection and specialization arms.
    ///
    /// # Discriminator invariant
    ///
    /// When `derivation.is_some()`, every specialization-scope field is empty:
    /// `structure_name.is_empty()`, `body.is_none()`,
    /// `spec_param_overrides.is_empty()`, `keyed_members.is_empty()`,
    /// `args.is_empty()`, `type_args.is_empty()`, `is_collection == false`.
    ///
    /// This is enforced at the single producer (`ts_parser::lower_sub`) rather
    /// than encoded in the type, and it is load-bearing rather than tidy:
    /// existing compiler consumers read `structure_name` / `args` /
    /// `is_collection` as SPECIALIZATION-SCOPE signals, so a derived sub that
    /// left any of them populated would be silently mistaken for one of their
    /// own. Any new producer must uphold it; it is pinned by
    /// `derived_sub_leaves_every_specialization_scope_field_empty` in
    /// reify-syntax's `harness_syntax::derived_sub_arm_parser_tests`.
    ///
    /// Note that `pose_expr` is deliberately NOT part of that invariant: an
    /// explicit `at` on a derived sub parses and lowers here, and is rejected
    /// one layer down as `E_DERIVED_SUB_EXPLICIT_AT` (T8) — a compile-scope
    /// diagnostic, per the D3-adversary ownership ruling.
    ///
    /// Parsed and stored here (A-alpha); first consumed by A-beta (#6616),
    /// which owns every compile-scope rejection for the derived arm.
    ///
    /// # Why this is boxed
    ///
    /// `SubDerivation` is 232 bytes, and `SubDecl` is the largest
    /// `MemberDecl` variant, so storing it inline grew `MemberDecl` from 536
    /// to 768 bytes — past `clippy::large_enum_variant`'s 200-byte gap over
    /// the second-largest variant (`ForallConstraint`, 384). Boxing costs one
    /// pointer on the common (`None`) path, which every non-derived sub takes,
    /// instead of 232 bytes on every `MemberDecl` in the AST. Consumers reach
    /// the fields through auto-deref unchanged.
    pub derivation: Option<Box<SubDerivation>>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// The derivation clause of a derived sub: which prototype, under which
/// transform, and what the derived copy overrides or drops.
///
/// `docs/prds/v0_6/assembly-derivation-toolbox.md` leaf A-alpha (task #6615).
#[derive(Debug, Clone)]
pub struct SubDerivation {
    /// Which derivation constructor this is, carrying its transform operand.
    pub kind: SubDerivationKind,
    /// The prototype sub this one is derived FROM — the `a` in `mirror of a`.
    ///
    /// A `SpannedIdent` rather than a bare `String` because A-beta's unknown /
    /// non-sibling / cyclic-prototype diagnostics must underline the prototype
    /// token ALONE; a derivation-wide span would degrade all three messages.
    ///
    /// The PRD names a bare sibling-sub identifier, so the grammar admits no
    /// dotted path here and `mirror of a.child` is a parse error.
    pub prototype: SpannedIdent,
    /// Param overrides from the derived body: `z = 55mm`, `w = auto(free)`,
    /// `z = 55mm where hinged`.
    ///
    /// Shares `SubDecl::spec_param_overrides`' lowering path — both route
    /// through `lower_binding_value`, so an `auto` override in a derived body
    /// resolves identically to one in a specialization body — but NOT its
    /// `(String, Expr)` shape, because the derived surface carries a name span
    /// and a `where` guard that a pair cannot hold. See [`SubParamOverride`].
    ///
    /// Ordinary overrides only — a `<param> = default` RESET goes to
    /// `param_resets` instead.
    pub param_overrides: Vec<SubParamOverride>,
    /// Params RESET to the prototype's declared default by `<param> = default`.
    ///
    /// Kept separate from `param_overrides` rather than encoded as a sentinel
    /// value: a reset carries no expression at all, so there is nothing
    /// meaningful to put in an `Expr`, and a consumer must be able to tell
    /// "inherit the default" from "override with this value" without
    /// pattern-matching for a magic node. `SpannedIdent` so a diagnostic about
    /// resetting an unknown param can underline the param name.
    pub param_resets: Vec<SpannedIdent>,
    /// `keep` / `exclude` dispositions, in SOURCE ORDER.
    ///
    /// Order is load-bearing: A-beta resolves each path against the
    /// prototype's feature tree and reports failures positionally, so a
    /// reordered list would point its diagnostics at the wrong item.
    pub dispositions: Vec<SubDisposition>,
    /// `let` and `constraint` members declared inside the derived body.
    ///
    /// NOT reached by [`walk_specialization_scope_members`], nor by any other
    /// member walker in this module: all of them enter through `SubDecl::body`,
    /// which the discriminator invariant keeps `None` on every derived sub. So
    /// the specialization-scope check, the priv-redundant lint and the span
    /// lookups all see a derived sub as having NO members, rather than failing
    /// loudly. That is deliberate at A-alpha — a derived body is not a
    /// specialization scope, and what its members scope to is a semantic
    /// question A-beta (#6616) owns — but it means A-beta must WIRE the
    /// traversal when derived subs start elaborating, not assume an existing
    /// walker already covers them.
    pub members: Vec<MemberDecl>,
    pub span: SourceSpan,
}

/// Which derivation constructor a [`SubDerivation`] uses.
///
/// An ELEMENT type with several constructors (PRD §6 D1), NOT a flattened
/// plane-or-transform pair: Layer-3 group elements become FURTHER constructors
/// here, and consumers are expected to switch on the constructor. Flattening it
/// to a bare `plane: Option<Expr>` / `transform: Option<Expr>` would make every
/// such addition a breaking change to every consumer.
#[derive(Debug, Clone)]
pub enum SubDerivationKind {
    /// `mirror of <prototype> across <plane>`
    Mirror { plane: Expr },
    /// `image of <prototype> under <transform>`
    Image { transform: Expr },
}

/// One param override in a derived body: `z = 55mm`, `w = auto(free)`,
/// `z = 55mm where hinged`.
///
/// A struct rather than the `(String, Expr)` pair `SubDecl::spec_param_overrides`
/// uses, because the derived surface carries two things that pair cannot hold:
///
/// * the override NAME's own span, so A-beta's "overrides a param the prototype
///   does not declare" diagnostic underlines the param token ALONE — the same
///   reasoning that makes [`SubDerivation::prototype`] and
///   [`SubDerivation::param_resets`] spanned;
/// * the optional `where` guard. The grammar admits one
///   (`derived_param_assignment` ends in `optional(field('guard', …))`) and the
///   spec documents it, so discarding it would silently hand A-beta a
///   CONDITIONAL override indistinguishable from an unconditional one.
#[derive(Debug, Clone)]
pub struct SubParamOverride {
    /// The overridden param's name, spanned: `z` in `z = 55mm`.
    pub name: SpannedIdent,
    /// The override value. `auto` / `auto(free)` reach `ExprKind::Auto` here,
    /// exactly as on the specialization arm.
    pub value: Expr,
    /// The `where` guard, when the source carries one. Parsed and stored here
    /// (A-alpha); evaluated by A-beta (#6616) together with the override.
    pub guard: Option<WhereClause>,
}

/// One `keep` / `exclude` item in a derived body.
#[derive(Debug, Clone)]
pub struct SubDisposition {
    pub kind: SubDispositionKind,
    /// The dotted feature path, split into segments: `web.hub` → `["web",
    /// "hub"]`. Resolution against the prototype's feature tree is A-beta's.
    ///
    /// Segments are spanned, not bare `String`s, so an unresolvable-path
    /// diagnostic underlines the failing HOP — the `hub` of `web.hub` — rather
    /// than the whole disposition. Same reasoning as [`SubDerivation::prototype`].
    pub path: Vec<SpannedIdent>,
    /// The plane from the RESERVED `keep <path> using <plane>` tail (PRD
    /// §3.3/§11).
    ///
    /// Stored only — it has no v1 meaning and no lowering consequence. Parsing
    /// and storing it now keeps the surface stable for v2 without committing to
    /// a semantics; `None` on every `exclude`, and on a `keep` with no tail.
    pub using_plane: Option<Expr>,
    pub span: SourceSpan,
}

/// `keep` or `exclude`.
#[derive(Debug, Clone)]
pub enum SubDispositionKind {
    Keep,
    Exclude,
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
    /// Whether this port is marked `priv` (PRD §D-3/D-4: private to the structure).
    /// `priv port` is hidden from importers; default-visible ports have `is_priv == false`.
    pub is_priv: bool,
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

/// Resolve a param's DEFAULT-EXPRESSION span within a member list.
///
/// Substrate for INV-GUI-3 (PRD `docs/prds/v0_6/ai-native-editing.md` §6.1): a
/// caller that wants to rewrite a param's default literal in source needs the
/// byte range of the literal itself.
///
/// **Invariant (§6.1):** the returned [`SourceSpan`] is the default
/// EXPRESSION range ONLY — never the whole `param … = …` declaration, and
/// never the `=` that precedes it. The tree-sitter grammar binds the `default`
/// field to the expression node alone (`tree-sitter-reify/grammar.js`), so
/// `ParamDecl.default`'s `Expr.span` already carries exactly that range; this
/// helper only locates it.
///
/// **Contract:** returns `Some(span)` if and only if EXACTLY ONE matching
/// `Param` is reachable within [`MAX_MEMBER_NESTING_DEPTH`] AND that param's
/// `default` is `Some`. `None` in every other case — the name is absent, the
/// member found is not a `Param`, the `Param` has no `default`, or the name is
/// declared more than once.
///
/// **Matches [`MemberDecl::Param`] ONLY**, unlike the sibling
/// [`find_named_member_span`], which matches both `Param` and `Let`. A `let`
/// binding has no param default to rewrite, so resolving one here would hand
/// the caller a span it must not splice into. That divergence is deliberate:
/// see the asymmetry note on [`walk_specialization_scope_members`], which
/// already flags these member-walking helpers as individually correct but
/// caller-surprising when one contract is inferred from another.
///
/// **Does not reach inside port bodies.** A port-body param is addressed by the
/// COMPOSITE name `<port>.<param>`, never by its bare name, and is not an
/// editable cell at all — see `collect_param_default_candidates` for why
/// matching a bare name in there could only misfire.
///
/// **Refuses rather than guesses on a multiply-declared name** — the one place
/// this helper deliberately diverges from [`find_named_member_span`]'s
/// first-match-wins. `where c { param x = 1 } else { param x = 2 }` is legal
/// source: `where`/`else` branch members are mutually-exclusive SIBLINGS
/// registered into the same parent frame (see
/// `reify_compiler::compile_builder::shadow_lint`'s module docs), and the AST
/// keeps NO record of which branch the condition selects. Picking the first
/// would let the caller rewrite the possibly INACTIVE branch's literal — the
/// user sets a value and the design does not change, a silent wrong edit.
/// Refusing hands the caller (the `set_parameter` path) a clean rejection to
/// surface as a structured error, which is what PRD §7 B7 requires.
pub fn find_param_default_span(members: &[MemberDecl], name: &str) -> Option<SourceSpan> {
    find_param_default_expr(members, name).map(|e| e.span)
}

/// Resolve a param's DEFAULT EXPRESSION within a member list — the
/// `&Expr`-returning primitive [`find_param_default_span`] is a thin
/// `.map(|e| e.span)` over.
///
/// Same contract, same refusal rules, ONE traversal: a caller that needs the
/// span gets it from here, and a caller that needs to inspect the
/// [`ExprKind`](crate::ast::ExprKind) before acting on the span gets that from
/// here too. Both readings therefore agree by construction, which is the point
/// — the asymmetry note on [`walk_specialization_scope_members`] records that
/// this module's member walkers drift when the same question is answered by two
/// hand-rolled recursion sets, and a second walk that recovered the expression
/// would be exactly that drift.
///
/// The caller that motivated it is `EngineSession::apply_param_to_source`
/// (INV-GUI-3 source write-back, task 5096 γ): it may splice over a
/// `NumberLiteral`/`QuantityLiteral`/`StringLiteral`/`BoolLiteral` default but
/// must REFUSE a `BinOp`, an `Auto`, a call or an identifier, because
/// overwriting one of those destroys a user-authored parametric relationship or
/// a solver-determined value. That literal-ness gate lives with the splice, in
/// the GUI engine — this helper only hands over the expression it needs to make
/// the decision, and deliberately does not narrow its own contract to literals:
/// a walker that answered "where is the default, if it is one of four kinds"
/// would be a different question, and the kinds it admits are the GUI's policy
/// rather than the AST's.
///
/// [`find_param_default_span`] is a `.map(|e| e.span)` wrapper over this and,
/// as of γ, has no in-tree caller beyond the tests and the reify-ast API-surface
/// guard: `apply_param_to_source` needs the expression, so it calls this. The
/// wrapper stays because it is α's published span API (task #5094) for the
/// consumers still to come — δ's MCP tools and η's re-homed slider, which want
/// a span without borrowing the AST — not because anything calls it today.
pub fn find_param_default_expr<'a>(members: &'a [MemberDecl], name: &str) -> Option<&'a Expr> {
    let mut candidates = ParamDefaultCandidates::default();
    collect_param_default_candidates(members, name, 0, &mut candidates);
    if candidates.count == 1 {
        candidates.first_default
    } else {
        None
    }
}

/// The member lists a `sub` declaration's specialization overrides live in.
///
/// A sub carries its overrides in exactly one of two shapes, and this is the
/// single place in the workspace that knows both:
///   * `body: Some(members)` — the non-keyed specialization body
///     `sub p : Foo { … }`.
///   * `keyed_members[].overrides` — one specialization body per key,
///     `sub p : Foo { "a" => { … }  "b" => { … } }`.
///
/// The two are mutually exclusive by construction in `lower_sub` (stated on
/// `SubDecl.keyed_members`), so this yields the body, OR each keyed entry's
/// overrides in declaration order, never both. A bare instantiation,
/// collection, or bare-colon-no-body sub yields nothing at all — it opens no
/// specialization scope.
///
/// Both shapes are lowered by the SAME `lower_specialization_body_members`, and
/// a keyed entry's `overrides` CST field is literally a `specialization_body`
/// node, so spec §8.7 governs them identically. Every recursion set therefore
/// answers both with ONE cell, `MemberRecursionSet::sub_overrides`: a per-shape
/// flag could express a body-yes/keyed-no set, which would mean nothing.
fn sub_override_bodies(sub: &SubDecl) -> impl Iterator<Item = &[MemberDecl]> {
    sub.body
        .as_deref()
        .into_iter()
        .chain(sub.keyed_members.iter().map(|e| e.overrides.as_slice()))
}

/// Visit every member of a specialization-scope body (spec §8.7).
///
/// One scope per override list the sub carries — its non-keyed `body`, or each
/// `keyed_members[]` entry — with `visitor` invoked on every member of each.
/// Why both shapes are one and the same thing under §8.7 is stated ONCE, on
/// `sub_override_bodies` (module-private, so not linkable from here), which
/// owns the union so no caller re-derives it.
///
/// The walker is a no-op only when the sub carries NEITHER shape — a bare
/// instantiation, a collection, or a bare-colon-no-body sub. Those open no
/// specialization scope, which makes this function the single discriminator for
/// "is this a scope root?"; its callers need not re-test the fields.
///
/// Each scope is walked from depth 0, so a keyed sub's per-entry scopes are
/// bounded exactly as a body scope is.
///
/// The traversal itself is `walk_members` driven by
/// `MemberRecursionSet::SPECIALIZATION_SCOPE`, so this walker recurses into:
///   * `MemberDecl::Sub(s)` that itself carries overrides in either shape —
///     nested specialization scopes (spec §8.7 nested-sub criterion).
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
/// `PortDecl.members` but does NOT recurse into a sub's overrides. These two
/// helpers have divergent contracts that are individually correct but can
/// surprise callers who infer one from the other. The shared-helper
/// consolidation that would unify them is now DONE: both are `walk_members`
/// calls that differ only in the `MemberRecursionSet` they pass. The divergent
/// contracts stay divergent on purpose — they are declared side by side as
/// data instead of hand-rolled apart.
///
/// **Anti-drift.** The canonical list of every member-recursion set in this
/// module, and of each caller's exit rule, is the table on
/// `MemberRecursionSet` — this doc deliberately does not restate it, because a
/// second copy is exactly the drift surface the consolidation removed. A newly
/// added [`MemberDecl`] variant is now classified in ONE place,
/// `walk_members`'s wildcard-free match, which fails to compile until that
/// classification is made rather than silently defaulting to "never descended
/// into".
///
/// **Derived subs are invisible to every one of those sets.** Each of them
/// reaches a sub's members through `SubDecl.body`, and `body` is `None` on
/// every derived sub (`sub b = mirror of a across P { … }`), so a derived
/// body's `let` / `constraint` members are never visited. See
/// [`SubDerivation::members`] for why that is deliberate at task #6615 and
/// what task #6616 must do about it.
pub fn walk_specialization_scope_members<'a, F>(sub: &'a SubDecl, visitor: &mut F)
where
    F: FnMut(&'a MemberDecl),
{
    for overrides in sub_override_bodies(sub) {
        walk_all(overrides, MemberRecursionSet::SPECIALIZATION_SCOPE, visitor);
    }
}

/// Visit every member reachable under `members`, using the widest recursion
/// set in the `MemberRecursionSet` table (module-private, so not linkable from
/// here) and bounded by [`MAX_MEMBER_NESTING_DEPTH`].
///
/// `visitor` sees each member parent-before-children, and the walk never exits
/// early. Takes a `&[MemberDecl]` rather than the `&SubDecl` its sibling
/// [`walk_specialization_scope_members`] takes, because its caller applies it
/// to whole declaration bodies (structure / occurrence / trait / purpose, plus
/// each structure nested in a purpose) rather than to one sub's scope.
pub fn walk_all_member_bodies<'a, F>(members: &'a [MemberDecl], visitor: &mut F)
where
    F: FnMut(&'a MemberDecl),
{
    walk_all(members, MemberRecursionSet::ALL_MEMBER_BODIES, visitor);
}

/// Drive [`walk_members`] over `members` under `set`, visiting everything.
///
/// The adapter both visit-everything wrappers above share. `Infallible` as the
/// break type statically pins "never exits early" — the closure always returns
/// `Continue`, so `walk_members` can never produce a `Break`.
fn walk_all<'a, F>(members: &'a [MemberDecl], set: MemberRecursionSet, visitor: &mut F)
where
    F: FnMut(&'a MemberDecl),
{
    let _: ControlFlow<Infallible> = walk_members(members, set, 0, &mut |m| {
        visitor(m);
        ControlFlow::Continue(())
    });
}

/// Which optional bodies a member-recursion walk descends into.
///
/// The single place the member-recursion set is declared as data, replacing
/// the three independent hand-rolled recursion sets this module used to
/// carry (one per walker). `GuardedGroupDecl.{members,else_members}` and
/// `MatchArmDeclArmDecl.member` are recursed UNCONDITIONALLY by every caller
/// today, so only the two cells that actually differ — a sub's specialization
/// overrides (see [`sub_override_bodies`] for the two shapes they take) and
/// `PortDecl.members` — are modeled as flags.
///
/// This table is the module's canonical anti-drift artifact: every
/// member-recursion set in this module is one of the consts below, and
/// [`walk_members`] is the single traversal every caller shares. The exit
/// rule is NOT a property of the set — it is the `ControlFlow` break type the
/// caller's visitor chooses — but it is listed here so one table carries the
/// whole picture.
///
/// | const | used by | sub overrides (`body` or keyed) | `PortDecl.members` | early exit |
/// |---|---|---|---|---|
/// | `SPECIALIZATION_SCOPE` | [`walk_specialization_scope_members`] | yes | no | no — `B = Infallible` pins it |
/// | `NAMED_MEMBER_LOOKUP` | [`find_named_member_span_depth`] | no | yes | yes — first match wins |
/// | `PARAM_DEFAULT_LOOKUP` | [`collect_param_default_candidates`] | no | no | yes — once ambiguous |
/// | `ALL_MEMBER_BODIES` | [`walk_all_member_bodies`] | yes | yes | no — `B = Infallible` pins it |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MemberRecursionSet {
    sub_overrides: bool,
    port_body: bool,
}

impl MemberRecursionSet {
    /// Used by [`walk_specialization_scope_members`] (spec §8.7): a sub's
    /// specialization overrides open a scope that is a child of the enclosing
    /// one. A port body, by contrast, is forbidden inside a specialization
    /// scope (task 2369) and so is never descended into here.
    const SPECIALIZATION_SCOPE: Self = Self {
        sub_overrides: true,
        port_body: false,
    };
    /// Used by `find_named_member_span_depth` (hover/goto-definition): a
    /// port-body param/let IS addressable by its bare name for these
    /// purposes, but a sub's specialization overrides describe a CHILD
    /// instance, not declarations of the entity being looked up in.
    const NAMED_MEMBER_LOOKUP: Self = Self {
        sub_overrides: false,
        port_body: true,
    };
    /// Used by `collect_param_default_candidates` (cell-id resolution):
    /// neither a port-body param (addressed only by the composite
    /// `<port>.<param>` name) nor a sub's specialization overrides are
    /// editable top-level cells, so neither is descended into.
    const PARAM_DEFAULT_LOOKUP: Self = Self {
        sub_overrides: false,
        port_body: false,
    };
    /// Used by [`walk_all_member_bodies`] — today, `priv_redundant_lint.rs`'s
    /// E_PRIV_REDUNDANT pass, which asks "does any `let`/`constraint` anywhere
    /// under this declaration carry `priv`?" and so may skip no optional body
    /// at all. Both cells `true`: the widest set in the table above.
    const ALL_MEMBER_BODIES: Self = Self {
        sub_overrides: true,
        port_body: true,
    };
}

/// The single member-recursion walker behind
/// [`walk_specialization_scope_members`], [`walk_all_member_bodies`],
/// `find_named_member_span_depth`, and `collect_param_default_candidates`.
///
/// Visits every member of `members`, invoking `visitor` on each one
/// (parent-before-children), and recurses according to `set` — see
/// [`MemberRecursionSet`] for which optional bodies are conditional and which
/// are unconditional. `visitor` returns [`ControlFlow`] so one walker can
/// serve both a visit-everything caller (`Continue(())` always, using an
/// uninhabited `Infallible` break type to make "never exits early" a
/// static property) and an early-exit caller (`Break` on a match); a `Break`
/// unwinds out of every nesting level via `?`, not just the innermost loop.
/// Recursion is bounded by [`MAX_MEMBER_NESTING_DEPTH`] to prevent stack
/// overflow on pathological input.
fn walk_members<'a, B, F>(
    members: &'a [MemberDecl],
    set: MemberRecursionSet,
    depth: usize,
    visitor: &mut F,
) -> ControlFlow<B>
where
    F: FnMut(&'a MemberDecl) -> ControlFlow<B>,
{
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return ControlFlow::Continue(());
    }
    for member in members {
        visitor(member)?;
        match member {
            // Spec §8.7 nested-sub criterion: a nested SubDecl that carries
            // specialization overrides opens its own specialization scope —
            // descended into only when `set.sub_overrides`, over every override
            // list `sub_override_bodies` yields. The `?` keeps a `Break`
            // unwinding out of every list AND every nesting level.
            MemberDecl::Sub(s) => {
                if set.sub_overrides {
                    for overrides in sub_override_bodies(s) {
                        walk_members(overrides, set, depth + 1, visitor)?;
                    }
                }
            }
            // Port bodies — descended into only when `set.port_body`.
            MemberDecl::Port(p) => {
                if set.port_body {
                    walk_members(&p.members, set, depth + 1, visitor)?;
                }
            }
            // Spec §8.7 + shadow_lint.rs:39-43: `where { … } else { … }`
            // members are siblings inside the enclosing scope, recursed into
            // UNCONDITIONALLY — every caller today agrees on this cell.
            MemberDecl::GuardedGroup(g) => {
                walk_members(&g.members, set, depth + 1, visitor)?;
                walk_members(&g.else_members, set, depth + 1, visitor)?;
            }
            // Spec §6.4 (task 2372): match-arm decl clusters desugar each arm
            // to a same-name guarded decl, recursed into UNCONDITIONALLY —
            // every caller today agrees on this cell too.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    walk_members(std::slice::from_ref(&*arm.member), set, depth + 1, visitor)?;
                }
            }
            // LOAD-BEARING: no `_` wildcard here. `MemberDecl` is not
            // `#[non_exhaustive]`, so listing every remaining variant
            // explicitly means a newly added variant fails THIS match at
            // compile time rather than silently defaulting to "never
            // descended into" — the exact silent-omission failure mode this
            // consolidation exists to prevent.
            MemberDecl::Param(_)
            | MemberDecl::Let(_)
            | MemberDecl::Constraint(_)
            | MemberDecl::ConstraintInst(_)
            | MemberDecl::Minimize(_)
            | MemberDecl::Maximize(_)
            | MemberDecl::AssociatedType(_)
            | MemberDecl::Fn(_)
            | MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::ForallConnect(_)
            | MemberDecl::ForallConstraint(_)
            | MemberDecl::Relate(_) => {}
        }
    }
    ControlFlow::Continue(())
}

fn find_named_member_span_depth<'a>(
    members: &'a [MemberDecl],
    name: &str,
    depth: usize,
) -> Option<MemberSpanInfo<'a>> {
    // The `_ => ControlFlow::Continue(())` wildcard below is a name-matching
    // predicate over Param/Let, not a recursion-set decision — only
    // `walk_members`'s own match is exhaustiveness-load-bearing. Hoisting the
    // Param/Let name test ahead of the recursion arms cannot reorder
    // anything, because Param/Let have no recursion arm; first-match-wins is
    // `Break` propagating out through `walk_members`'s `?`.
    match walk_members(
        members,
        MemberRecursionSet::NAMED_MEMBER_LOOKUP,
        depth,
        &mut |member| match member {
            MemberDecl::Param(p) if p.name == name => ControlFlow::Break(MemberSpanInfo {
                span: p.span,
                doc: p.doc.as_deref(),
            }),
            MemberDecl::Let(l) if l.name == name => ControlFlow::Break(MemberSpanInfo {
                span: l.span,
                doc: l.doc.as_deref(),
            }),
            _ => ControlFlow::Continue(()),
        },
    ) {
        ControlFlow::Break(info) => Some(info),
        ControlFlow::Continue(()) => None,
    }
}

/// Accumulator for [`collect_param_default_candidates`].
///
/// `count` is how many matching `Param` members the traversal reached;
/// `first_default` is the default EXPRESSION of the FIRST one (already `None`
/// when that param has no default). [`find_param_default_expr`] yields
/// `first_default` only when `count == 1`, so a multiply-declared name is
/// refused rather than resolved to whichever branch happened to come first.
///
/// Borrows the `&Expr` out of the walked member list rather than copying its
/// `SourceSpan`, so the one traversal serves both the span-only caller
/// ([`find_param_default_span`]) and the caller that must inspect the
/// expression kind before splicing over the span.
#[derive(Default)]
struct ParamDefaultCandidates<'a> {
    count: usize,
    first_default: Option<&'a Expr>,
}

/// Depth-bounded worker for [`find_param_default_expr`] (and therefore, via its
/// `.map(|e| e.span)` delegation, for [`find_param_default_span`]).
///
/// A thin visitor over [`walk_members`] carrying
/// [`MemberRecursionSet::PARAM_DEFAULT_LOOKUP`], so the recursion set is
/// `GuardedGroup` (BOTH `members` and `else_members`) and each
/// `MatchArmDeclGroup` arm's `member`. Everything else is skipped — in
/// particular the two bodies below, each for its own reason.
///
/// Deliberately does NOT stop on the FIRST match, because the public contract
/// needs the candidate COUNT to decide between resolving and refusing. It DOES
/// stop on the second, and that costs nothing observable: `first_default` is
/// written only at `count == 1`, so it is frozen from the first hit onward, and
/// [`find_param_default_span`] reads only `count == 1` vs not — it never
/// distinguishes 2 from 7. `count` therefore saturates at 2.
///
/// **`PortDecl.members` is deliberately NOT traversed**, and here this helper
/// diverges from [`find_named_member_span`] ON PURPOSE. Parity with that
/// sibling is the WRONG invariant for this one: `find_named_member_span` serves
/// hover/goto-definition, where surfacing a port-internal declaration under its
/// bare name is exactly right. This helper serves cell_id resolution, where it
/// is not — a port-body param is not addressable by its bare name and is not an
/// editable cell at all. The compiler registers port-body members under the
/// COMPOSITE member name `ValueCellId(entity, "<port>.<param>")` (see
/// `reify_compiler::entity`'s port-body registration), those decls land in
/// `CompiledPort.members`, and nothing ever merges them into
/// `TopologyTemplate.value_cells` — which is the only map the `set_parameter`
/// path and the property panel key off. So recursing into a port body could
/// never supply the candidate a caller asked for; it could only
///
///   * FALSELY REFUSE — an entity with a top-level `param d` AND a port-internal
///     `param d` would push `count` to 2 and refuse a genuinely editable cell.
///     That collision is silently legal: `shadow_lint` deliberately does not
///     fold port-internal members into the enclosing frame and emits no
///     warning, so nothing else flags it either; or
///   * RETURN A WRONG SPAN — with only a port-internal `d` present, the bare
///     name `d` would resolve to the PORT member's default, handing a caller a
///     range it must not splice.
///
/// Supporting port members would mean splitting the requested name on `'.'` and
/// resolving `<port>` → `<param>`, mirroring the compiler's composite naming —
/// a deliberate feature, not a side effect of a shared recursion set.
///
/// **A sub's specialization overrides are deliberately NOT traversed either** —
/// in any shape [`sub_override_bodies`] yields. That omission mirrors
/// [`find_named_member_span`] and is the asymmetry already documented on
/// [`walk_specialization_scope_members`]. A sub's overrides SPECIALIZE a child
/// instance; they are not the child's own param declarations, so a default span
/// found there would belong to a different entity than the caller's cell_id
/// names — splicing into it would rewrite the wrong declaration.
fn collect_param_default_candidates<'a>(
    members: &'a [MemberDecl],
    name: &str,
    depth: usize,
    out: &mut ParamDefaultCandidates<'a>,
) {
    // The depth bound lives in `walk_members`, which returns `Continue` at the
    // bound: a subtree cut off there contributes ZERO candidates, so a param
    // that is only reachable past the bound leaves `count == 0` and the caller
    // returns `None` — same observable outcome as the pre-consolidation early
    // `return`.
    //
    // The `if let` below is a name-matching predicate over `Param`, not a
    // recursion-set decision; only `walk_members`'s own match is
    // exhaustiveness-load-bearing. `Break` on the second hit unwinds out of
    // every nesting level via `?` — where the hand-rolled loop instead re-tested
    // `out.count > 1` at the top of every frame's every iteration. Both stop
    // mutating `out` the instant `count` reaches 2, so they agree on the only
    // two cells anyone reads.
    let _: ControlFlow<()> = walk_members(
        members,
        MemberRecursionSet::PARAM_DEFAULT_LOOKUP,
        depth,
        &mut |member| {
            if let MemberDecl::Param(p) = member
                && p.name == name
            {
                out.count += 1;
                if out.count == 1 {
                    out.first_default = p.default.as_ref();
                }
                if out.count > 1 {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        },
    );
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
    Connect(Box<ConnectDecl>),
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
    /// `#cfg(...)` pragmas attached positionally to this import; ANDed for DAG gating,
    /// empty for an ungated import.
    pub cfg_predicates: Vec<Pragma>,
}

/// A single variant inside an `enum` declaration.
///
/// Bare variants (e.g. `Point`) carry `payload: VariantPayload::Unit`.
/// Named-field variants (e.g. `Circle { radius: Length }`) carry
/// `payload: VariantPayload::Named(vec![("radius", <TypeExpr>)])`.
///
/// Helpers:
/// - `EnumVariantDecl::unit(name)` — construct a unit variant by name.
/// - `From<&str>` / `From<String>` — shorthand for `unit(name)`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantDecl {
    pub name: String,
    pub payload: VariantPayload,
    pub span: SourceSpan,
}

impl EnumVariantDecl {
    /// Construct a unit (bare) variant with an empty span.
    pub fn unit(name: impl Into<String>) -> Self {
        EnumVariantDecl {
            name: name.into(),
            payload: VariantPayload::Unit,
            span: SourceSpan::empty(0),
        }
    }
}

impl From<&str> for EnumVariantDecl {
    fn from(name: &str) -> Self {
        EnumVariantDecl::unit(name)
    }
}

impl From<String> for EnumVariantDecl {
    fn from(name: String) -> Self {
        EnumVariantDecl::unit(name)
    }
}

/// The optional payload of an [`EnumVariantDecl`].
#[derive(Debug, Clone, PartialEq)]
pub enum VariantPayload {
    /// Bare variant with no fields: `Point`.
    Unit,
    /// Named-field variant: `Circle { radius: Length }`.
    /// Fields are stored in source-declaration order.
    Named(Vec<(String, TypeExpr)>),
}

/// `enum Direction { In, Out, Bidi }`
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    /// Type parameters declared on the enum head: `enum Maybe<T>` → `[T]`.
    /// Empty for non-generic enums (invariant INV-6). Mirrors `StructureDef.type_params`.
    pub type_params: Vec<TypeParamDecl>,
    pub variants: Vec<EnumVariantDecl>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// `fn area(w: Length, h: Length) -> Scalar { w * h }`
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    pub params: Vec<FnParam>,
    pub return_type: Option<TypeExpr>,
    /// The function body. `Some` for a defined function; `None` for a
    /// bodyless required associated function inside a trait
    /// (`fn req(self) -> Real` with no `{ ... }`).
    pub body: Option<FnBody>,
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
    /// Ambient-default declarations directly in this purpose body.
    ///
    /// Extracted from `purpose_member` nodes into a dedicated vec (parallel to
    /// `pragmas`) so that `members` contains only `MemberDecl` variants and the
    /// `Declaration::Default` blast radius is kept small (task 4496 design
    /// decision — NOT a `MemberDecl::Default` variant).
    pub defaults: Vec<DefaultDecl>,
    /// Structure definitions lexically nested in this purpose body.
    ///
    /// Extracted from `purpose_member` nodes into a dedicated vec (parallel to
    /// `defaults` and `pragmas`) so that `members` stays a pure `MemberDecl`
    /// list with zero blast radius on existing match sites (task 4639 design
    /// decision — NOT a `MemberDecl::Structure` variant).
    pub structures: Vec<StructureDef>,
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

/// An ambient-default declaration: `default Material = steel`
///
/// Valid at two positions: file top-level (`Declaration::Default`) and directly
/// inside a `purpose` body (`PurposeDef.defaults`).
///
/// Grammar producer only (task A); semantics (scope resolution, injection into
/// structures) are deferred to task B. No `pub` prefix and no annotations in v1.
#[derive(Debug, Clone)]
pub struct DefaultDecl {
    /// The type name this default applies to (e.g., `Material`).
    pub type_expr: TypeExpr,
    /// The default value expression (e.g., `steel`).
    pub value: Expr,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A `joint` definition (geometric-joints α, task 4395).
///
/// `joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = { coaxial(a, b)  on(a.point, stop) }`
///
/// Grammar producer only (task α 4395). Semantics — DOF self-check
/// (E_JOINT_DOF_MISMATCH), body type-check against `Type::Relation`,
/// `validate_range` call — are deferred to task β. Mirrors `FnDef`/`RelateDecl`
/// field conventions (span, content_hash, annotations).
#[derive(Debug, Clone)]
pub struct JointDef {
    pub name: String,
    pub doc: Option<String>,
    pub is_pub: bool,
    pub type_params: Vec<TypeParamDecl>,
    /// Datum parameter list: `(a: Axis, b: Axis, stop: Plane)`.
    pub params: Vec<FnParam>,
    /// The DOF fields, in source order. Length 1 for the single form
    /// (`with angle: Angle`); length N for the record form
    /// (`with { angle: Angle, travel: Length }`).
    pub dof: Vec<JointDofField>,
    /// The body expressions, in source order. Block form (`= { … }`) lowers
    /// to one `Expr` per `relation_member`; single-expr form (`= expr`) lowers
    /// to a 1-element Vec. Carried as `Vec<Expr>`, unvalidated (β validates).
    pub body: Vec<Expr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Annotations preceding this declaration.
    pub annotations: Vec<Annotation>,
}

/// A single DOF field inside a `joint … with` clause.
///
/// `name: TypeExpr` with an optional `in <range>` bound expression.
/// `range` is carried as `Option<Expr>` and is NOT validated here (deferred to β).
#[derive(Debug, Clone)]
pub struct JointDofField {
    pub name: String,
    pub type_expr: TypeExpr,
    /// The optional `in <range>` bound (e.g., `0deg..120deg`).
    /// `None` when the `in` clause is absent.
    pub range: Option<Expr>,
    pub span: SourceSpan,
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

/// A function parameter: `w: Length` or `w: Length = default_expr`
#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: String,
    /// `true` when this parameter is the implicit `self` receiver of a
    /// trait-associated function. The `type_expr` in that case is a sentinel
    /// `TypeExprKind::Named { name: "self", .. }` — `is_self` is the source
    /// of truth and the sentinel type is replaced by the concrete receiver
    /// type during dispatch in later task δ/ζ.
    pub is_self: bool,
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
/// The parser-produced parallel of `reify_ir::annotation::has_test_annotation`
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

impl ParseError {
    /// Render as `line:col: message`, both 1-based, using the source text the span indexes.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
    /// a parse error a user cannot locate is only marginally better than silence. `SourceSpan`
    /// carries byte offsets only, and this — together with
    /// [`render_with_offsets`](Self::render_with_offsets), which it delegates to — is the
    /// single place that converts them for human output, so every caller reports positions the
    /// same way.
    ///
    /// Degrades rather than aborting on a span that does not index `source`: the prelude
    /// sentinel ([`SourceSpan::PRELUDE_SENTINEL_OFFSET`]) becomes the canonical
    /// "no user-file location" fallback `1:1`, and any other out-of-range offset (a stale
    /// span, or one belonging to a different file) is clamped to `source.len()`. See
    /// [`render_with_offsets`](Self::render_with_offsets) for why each is handled that way.
    ///
    /// This builds a newline table to serve ONE lookup, which is the same O(len(source)) it
    /// would cost to scan; rendering a whole list should go through
    /// [`render_all`](Self::render_all) instead, which builds one table for all of them.
    pub fn render(&self, source: &str) -> String {
        self.render_with_offsets(source, &reify_core::build_line_offsets(source))
    }

    /// Render every error in `errors` against `source`, building the newline table ONCE.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
    /// [`render`](Self::render) is O(len(source)) per call, and every caller renders in a loop
    /// over `Parsed::errors`, so the naive shape costs O(errors × len(source)). That was
    /// tolerable when a malformed file produced one or two parse errors; it is not now that
    /// `diagnose_error_node` emits up to 8 diagnostics per `ERROR` node and a file can carry
    /// several such nodes (measured on the 24-broken-function corpus: 22 diagnostics from one
    /// parse), each of which would re-scan the whole source from byte 0.
    ///
    /// Same output as mapping [`render`](Self::render), one string per input error, in order —
    /// by construction, since both go through
    /// [`render_with_offsets`](Self::render_with_offsets).
    pub fn render_all(errors: &[ParseError], source: &str) -> Vec<String> {
        // The only short-circuit worth having: with nothing to render, the table would be a
        // scan of the whole source for no lookups at all. A SINGLE error is deliberately NOT
        // special-cased — `render` builds exactly this same table to serve its one lookup, so
        // forking there would buy nothing and leave two code paths to keep in agreement.
        if errors.is_empty() {
            return Vec::new();
        }
        let line_offsets = reify_core::build_line_offsets(source);
        errors
            .iter()
            .map(|e| e.render_with_offsets(source, &line_offsets))
            .collect()
    }

    /// [`render`](Self::render) against a pre-built newline table.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
    /// `line_offsets` must be [`reify_core::build_line_offsets`] of the SAME `source`; passing
    /// another file's table yields a wrong position, never a panic. O(log M) to find the line
    /// plus O(column) to count the codepoints on it, against
    /// [`reify_core::byte_offset_to_line_col`]'s O(len(source)) scan from byte 0.
    ///
    /// Deliberately reproduces that function's semantics exactly, including its two degenerate
    /// inputs, rather than approximating them:
    ///
    /// - The prelude sentinel ([`SourceSpan::PRELUDE_SENTINEL_OFFSET`]) short-circuits to
    ///   `(1, 1)`, the canonical "no user-file location" fallback, before anything indexes
    ///   `source`.
    /// - Any other out-of-range offset (a stale span, or one belonging to a different file) is
    ///   clamped to `source.len()`.
    ///
    /// A `line`/`col` pair is derived the same way `byte_offset_to_line_col` derives it: the
    /// line is one more than the number of `'\n'` strictly before `offset` (a binary search
    /// here, a full character walk there), and the column is one more than the number of
    /// codepoints between the start of that line and `offset`. The column walk starts at the
    /// line start — always a character boundary, since `'\n'` is one byte — and stops at
    /// `offset` without ever slicing there, so an offset that is not a character boundary
    /// degrades to a position rather than panicking, exactly as the scanning version does.
    ///
    /// "Reproduces exactly" is a DUPLICATION, not a settled placement, and
    /// `render_with_offsets_agrees_with_the_scanning_render` — an exhaustive `0..=source.len()`
    /// equivalence test — exists only to hold the two in lockstep. The conversion belongs in
    /// `reify_core` beside [`reify_core::build_line_offsets`] and its table-based INVERSE
    /// `line_col_to_byte_offset_with_offsets`, as the missing forward sibling of that pair,
    /// with [`reify_core::byte_offset_to_line_col`] delegating to it; this method would then be
    /// a formatting wrapper and that test would collapse to a couple of unit cases. Only the
    /// PARSE-ERROR formatting has a reason to live here — `reify-cli` cannot name
    /// `reify_ast::ParseError` — and that reason does not extend to the line/col algorithm,
    /// since `reify-cli` depends on `reify-core` directly. Not done here: `reify-core` is
    /// outside task #5392's lock set.
    pub fn render_with_offsets(&self, source: &str, line_offsets: &[usize]) -> String {
        let offset = self.span.start as usize;
        if offset == SourceSpan::PRELUDE_SENTINEL_OFFSET {
            return format!("1:1: {}", self.message);
        }
        let offset = offset.min(source.len());

        // Number of newlines strictly before `offset`; `line_offsets` is sorted ascending.
        let preceding_newlines = line_offsets.partition_point(|&nl| nl < offset);
        let line = preceding_newlines + 1;
        let line_start = match preceding_newlines.checked_sub(1) {
            Some(i) => line_offsets[i] + 1,
            None => 0,
        };
        let col = source[line_start..]
            .char_indices()
            .take_while(|(i, _)| line_start + i < offset)
            .count()
            + 1;
        format!("{line}:{col}: {}", self.message)
    }
}

#[cfg(test)]
mod parse_error_render_tests {
    use super::ParseError;
    use reify_core::SourceSpan;

    /// A parse error a user cannot locate is only marginally better than silence.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
    /// `SourceSpan` carries byte offsets only, and the CLI printed `err.message` alone — no
    /// file, no line, no column. `render` is the single place that converts an offset into a
    /// position a human can act on.
    #[test]
    fn parse_error_render_includes_line_and_column() {
        let source =
            "structure T {\n  let v = f(1)\n}\nfn f(i: Int) -> Real {\n  let x0 = 1\n  x0\n}\n";
        let off = source
            .find("let x0")
            .expect("fixture must contain 'let x0'") as u32;

        let err = ParseError {
            message: "missing ';' after `let` binding in function body".to_string(),
            span: SourceSpan::new(off, off + 6),
        };

        let rendered = err.render(source);

        // `let x0` is on line 5, indented two spaces, so column 3 — both 1-based.
        assert!(
            rendered.starts_with("5:3:"),
            "expected the rendered error to lead with its 1-based line:column position \
             `5:3:`; got {rendered:?}",
        );
        assert!(
            rendered.contains("missing ';'"),
            "the rendered error must still carry its message; got {rendered:?}",
        );
        assert!(
            !rendered.contains('\n'),
            "a rendered parse error must be a single line — a multi-line render means source \
             is being echoed back into the diagnostic; got {rendered:?}",
        );
    }

    /// Offset 0 is the boundary the loop in `byte_offset_to_line_col` never enters: it must
    /// still report `1:1`, not `0:0`.
    #[test]
    fn parse_error_render_at_offset_zero_is_one_one() {
        let source = "fn f() -> Int { 1 }\n";
        let err = ParseError {
            message: "syntax error in source file".to_string(),
            span: SourceSpan::new(0, 2),
        };

        let rendered = err.render(source);
        assert!(
            rendered.starts_with("1:1:"),
            "byte offset 0 must render as the 1-based position `1:1:`; got {rendered:?}",
        );
    }

    /// A prelude-sentinel span must degrade, never abort.
    ///
    /// `SourceSpan::PRELUDE_SENTINEL_OFFSET` (`u32::MAX`) lies far past the end of any real
    /// source. `byte_offset_to_line_col` short-circuits it to `(1, 1)` ahead of its
    /// `debug_assert!(offset <= source.len())`, but only if `render` passes the sentinel
    /// through unclamped — clamping it to `source.len()` first would silently turn a
    /// "no user-file location" marker into a bogus end-of-file position.
    #[test]
    fn parse_error_render_tolerates_the_prelude_sentinel() {
        let source = "fn f() -> Int { 1 }\n";
        let sentinel = SourceSpan::PRELUDE_SENTINEL_OFFSET as u32;
        let err = ParseError {
            message: "syntax error in source file".to_string(),
            span: SourceSpan::new(sentinel, sentinel),
        };

        let rendered = err.render(source);
        assert!(
            rendered.starts_with("1:1:"),
            "a prelude-sentinel span must render as the `1:1:` no-user-file-location \
             fallback rather than panicking or reporting an end-of-file position; \
             got {rendered:?}",
        );
    }

    /// The batch path must agree with the scanning path at EVERY offset.
    ///
    /// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
    /// `render_with_offsets` exists only to avoid re-scanning the source once per diagnostic;
    /// it earns that by reproducing `reify_core::byte_offset_to_line_col` exactly, so the two
    /// are compared here at every byte offset of a fixture chosen to hit each way they could
    /// diverge: an offset before any newline (line 1, where the `checked_sub` line-start
    /// branch is taken), offsets on and immediately after a newline (the off-by-one the binary
    /// search would get wrong), MULTI-BYTE characters (byte length != codepoint count, so a
    /// column computed from bytes would drift), consecutive newlines (an empty line, whose
    /// line start IS its line end), a trailing newline, and `source.len()` itself.
    ///
    /// Offsets that fall inside a multi-byte character are included deliberately: a span is
    /// normally on a character boundary, but the scanning version tolerates one that is not,
    /// and a slicing implementation would panic there instead.
    #[test]
    fn render_with_offsets_agrees_with_the_scanning_render() {
        let source = "fn f() -> Int {
  let α = 1

  α + 1
}
";
        let line_offsets = reify_core::build_line_offsets(source);

        for offset in 0..=source.len() {
            let err = ParseError {
                message: "m".to_string(),
                span: SourceSpan::new(offset as u32, offset as u32),
            };
            let (line, col) = reify_core::byte_offset_to_line_col(source, offset);
            assert_eq!(
                err.render_with_offsets(source, &line_offsets),
                format!("{line}:{col}: m"),
                "batch render disagrees with the scanning render at byte offset {offset} \
                 of {source:?}",
            );
        }
    }

    /// `render_all` is the loop the CLI and MCP call sites run: empty in, empty out, and the
    /// spans that have no line to report still degrade rather than panicking.
    ///
    /// INV-SF-7, task #5392. `render_all` has ONE code path — only the empty case returns
    /// early, and it builds nothing — so agreement with mapping `render` holds by
    /// construction and is not what needs pinning. What does is the behaviour on the two
    /// inputs that carry no usable position: an offset past the end of `source` (a stale
    /// span, or one belonging to a different file) and the prelude sentinel.
    #[test]
    fn render_all_handles_empty_input_and_locationless_spans() {
        let source = "fn f() -> Int {
  let x = 1
  x
}
";
        let mk = |off: u32, msg: &str| ParseError {
            message: msg.to_string(),
            span: SourceSpan::new(off, off),
        };

        assert!(
            ParseError::render_all(&[], source).is_empty(),
            "rendering no errors must yield no strings",
        );

        let errors = vec![
            mk(0, "first"),
            mk(18, "second"),
            // Out of range: clamped to the end of the source, never a panic.
            mk(source.len() as u32 + 500, "past end"),
            mk(SourceSpan::PRELUDE_SENTINEL_OFFSET as u32, "prelude"),
        ];
        let rendered = ParseError::render_all(&errors, source);

        assert_eq!(
            rendered.len(),
            errors.len(),
            "render_all must yield one string per input error, in order; got {rendered:?}",
        );

        // Derived, not hard-coded: the clamp target is whatever the canonical scanning
        // conversion reports for the end of the source.
        let (end_line, end_col) = reify_core::byte_offset_to_line_col(source, source.len());
        assert_eq!(
            rendered[2],
            format!("{end_line}:{end_col}: past end"),
            "an out-of-range offset must clamp to the end of the source",
        );
        assert_eq!(
            rendered[3], "1:1: prelude",
            "a prelude-sentinel span has no user-file location and must fall back to `1:1`",
        );
    }
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

/// Shared hand-built `MemberDecl`/`SubDecl` fixture constructors — no parser.
///
/// Hoisted out of `find_param_default_span_tests` (pre-1) so more than one
/// `#[cfg(test)]` module in this file can build the same nested-member shapes
/// without duplicating any `ParamDecl`/`LetDecl`/`GuardedGroupDecl`/`PortDecl`/
/// `MatchArmDeclGroupDecl`/`SubDecl` literal.
#[cfg(test)]
mod member_test_fixtures {
    use super::{
        Expr, GuardedGroupDecl, KeyedSubMemberEntry, LetDecl, MatchArmDeclArmDecl,
        MatchArmDeclGroupDecl, MemberDecl, ParamDecl, PortDecl, SubDecl,
    };
    use crate::ast::ExprKind;
    use reify_core::{ContentHash, PortDirection, SourceSpan};

    /// Build a `MemberDecl::Param` by hand — no parser.
    ///
    /// `decl_span` is the whole `param … = …` declaration's span; `default_span`,
    /// when `Some`, is the span of the default EXPRESSION alone. Keeping the two
    /// distinct is what makes the §6.1 invariant assertable.
    pub(super) fn param(
        name: &str,
        decl_span: (u32, u32),
        default_span: Option<(u32, u32)>,
    ) -> MemberDecl {
        MemberDecl::Param(ParamDecl {
            name: name.to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: default_span.map(|(s, e)| Expr {
                kind: ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::new(s, e),
            }),
            where_clause: None,
            annotations: Vec::new(),
            span: SourceSpan::new(decl_span.0, decl_span.1),
            content_hash: ContentHash(0),
        })
    }

    /// Build a `MemberDecl::Let` by hand — the sibling variant that
    /// [`super::find_named_member_span`] matches but this helper deliberately does not.
    pub(super) fn let_member(
        name: &str,
        decl_span: (u32, u32),
        value_span: (u32, u32),
    ) -> MemberDecl {
        MemberDecl::Let(LetDecl {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            is_priv: false,
            is_aux: false,
            type_expr: None,
            value: Expr {
                kind: ExprKind::NumberLiteral {
                    value: 80.0,
                    is_real: false,
                },
                span: SourceSpan::new(value_span.0, value_span.1),
            },
            where_clause: None,
            annotations: Vec::new(),
            span: SourceSpan::new(decl_span.0, decl_span.1),
            content_hash: ContentHash(0),
        })
    }

    /// A `BoolLiteral(true)` stand-in for a guard condition / match discriminant.
    pub(super) fn dummy_expr() -> Expr {
        Expr {
            kind: ExprKind::BoolLiteral(true),
            span: SourceSpan::new(0, 1),
        }
    }

    /// `where <cond> { members } else { else_members }`.
    pub(super) fn guarded(members: Vec<MemberDecl>, else_members: Vec<MemberDecl>) -> MemberDecl {
        MemberDecl::GuardedGroup(GuardedGroupDecl {
            condition: dummy_expr(),
            members,
            else_members,
            span: SourceSpan::new(0, 1),
            content_hash: ContentHash(0),
        })
    }

    /// `port <name> : in <T> { members }`.
    pub(super) fn port(name: &str, members: Vec<MemberDecl>) -> MemberDecl {
        MemberDecl::Port(PortDecl {
            name: name.to_string(),
            direction: Some(PortDirection::In),
            type_name: "FluidPort".to_string(),
            is_priv: false,
            members,
            frame_expr: None,
            span: SourceSpan::new(0, 1),
            content_hash: ContentHash(0),
        })
    }

    /// `match <disc> { P => <member> … }` at decl level.
    pub(super) fn match_arm_group(arms: Vec<(&str, MemberDecl)>) -> MemberDecl {
        MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
            discriminant: dummy_expr(),
            arms: arms
                .into_iter()
                .map(|(pattern, member)| MatchArmDeclArmDecl {
                    patterns: vec![pattern.to_string()],
                    member: Box::new(member),
                    span: SourceSpan::new(0, 1),
                })
                .collect(),
            span: SourceSpan::new(0, 1),
            content_hash: ContentHash(0),
        })
    }

    /// `depth` levels of GuardedGroup nesting wrapping one param that HAS a default.
    ///
    /// Same shape as `build_nested_guarded_members` in
    /// crates/reify-lsp/src/analysis.rs, but with a default attached so the
    /// depth assertions can name the exact span rather than settling for `is_some`.
    pub(super) fn build_nested_guarded_members(
        depth: usize,
        target: &str,
        default_span: (u32, u32),
    ) -> Vec<MemberDecl> {
        let mut current = vec![param(target, (0, 40), Some(default_span))];
        for _ in 0..depth {
            current = vec![guarded(current, vec![])];
        }
        current
    }

    /// Build a `SubDecl` by hand — no parser. Mirrors the field-literal shape
    /// established by `make_sub_with_body` in
    /// `crates/reify-syntax/tests/harness_syntax/sub_decl_specialization_tests.rs`
    /// (`match_decl_block_tests.rs` builds the same shape from inline `SubDecl`
    /// literals rather than a named helper). reify-ast had no `SubDecl` test
    /// builder of its own before this.
    ///
    /// `body: None` yields a bare-instantiation/collection-shaped `SubDecl`
    /// (no specialization scope); `body: Some(members)` opens one.
    pub(super) fn sub_with_body(name: &str, body: Option<Vec<MemberDecl>>) -> SubDecl {
        SubDecl {
            name: name.to_string(),
            structure_name: "Foo".to_string(),
            type_args: Vec::new(),
            args: Vec::new(),
            is_collection: false,
            where_clause: None,
            body,
            spec_param_overrides: Vec::new(),
            keyed_members: Vec::new(),
            is_aux: false,
            is_priv: false,
            pose_expr: None,
            index_binder: None,
            index_domain: None,
            relate_relations: Vec::new(),
            derivation: None,
            span: SourceSpan::new(0, 1),
            content_hash: ContentHash(0),
        }
    }

    /// Build a keyed-block `SubDecl` by hand — `sub <name> : Foo { "k" => { … } }`.
    ///
    /// The keyed counterpart of [`sub_with_body`]: each `(key, overrides)` pair
    /// becomes one `KeyedSubMemberEntry`, and `body` stays `None` because
    /// `lower_sub` makes the two forms mutually exclusive (see the doc on
    /// `SubDecl.keyed_members`). `param_overrides` stays empty on purpose: a
    /// `"k" => { area = 5mm }` param ASSIGNMENT lowers THERE, never into
    /// `overrides`, so it is not a `MemberDecl` any walker could reach.
    pub(super) fn sub_with_keyed_members(
        name: &str,
        entries: Vec<(&str, Vec<MemberDecl>)>,
    ) -> SubDecl {
        SubDecl {
            keyed_members: entries
                .into_iter()
                .map(|(key, overrides)| KeyedSubMemberEntry {
                    key: key.to_string(),
                    overrides,
                    param_overrides: Vec::new(),
                    span: SourceSpan::new(0, 1),
                })
                .collect(),
            ..sub_with_body(name, None)
        }
    }
}

#[cfg(test)]
mod find_param_default_span_tests {
    use super::member_test_fixtures::*;
    use super::{find_param_default_expr, find_param_default_span};
    use crate::ast::ExprKind;
    use reify_core::SourceSpan;

    #[test]
    fn param_with_default_returns_the_default_expression_span() {
        // `param width: Length = 80mm` — decl spans (0, 30), the default `80mm` spans (22, 27).
        let members = vec![param("width", (0, 30), Some((22, 27)))];
        assert_eq!(
            find_param_default_span(&members, "width"),
            Some(SourceSpan::new(22, 27))
        );
    }

    #[test]
    fn returned_span_is_the_default_only_never_the_whole_decl() {
        // PRD §6.1 invariant, pinned explicitly rather than left implied by the
        // equality above: the result must NOT be the whole `param … = …` decl span.
        let members = vec![param("width", (0, 30), Some((22, 27)))];
        let found = find_param_default_span(&members, "width").expect("param has a default");
        assert_ne!(
            found,
            SourceSpan::new(0, 30),
            "must return the default EXPRESSION range, never the whole param decl"
        );
    }

    #[test]
    fn param_with_default_returns_the_default_expression_itself() {
        // The `&Expr`-returning primitive `find_param_default_span` delegates to
        // (task 5096 γ, INV-GUI-3 write-back): `apply_param_to_source` must read
        // the default's `ExprKind` — to refuse splicing over a `BinOp` or an
        // `Auto` — from the SAME traversal that produced the span it would
        // splice into, or the two readings could disagree about which
        // declaration they describe.
        let members = vec![param("width", (0, 30), Some((22, 27)))];
        let expr = find_param_default_expr(&members, "width").expect("param has a default");
        assert_eq!(
            expr.kind,
            ExprKind::NumberLiteral {
                value: 80.0,
                is_real: false,
            },
            "must hand back the default EXPRESSION, not merely locate its range"
        );
        assert_eq!(
            expr.span,
            SourceSpan::new(22, 27),
            "the borrowed expression's own span is the default range"
        );
    }

    #[test]
    fn param_without_default_returns_none() {
        // `param thickness : Length` — real shape, e.g.
        // examples/auto/bearing_computed_default_unevaluated.ri:57.
        let members = vec![param("thickness", (0, 24), None)];
        assert_eq!(find_param_default_span(&members, "thickness"), None);
    }

    #[test]
    fn absent_name_returns_none() {
        let members = vec![param("width", (0, 30), Some((22, 27)))];
        assert_eq!(find_param_default_span(&members, "height"), None);
    }

    #[test]
    fn empty_member_slice_returns_none() {
        assert_eq!(find_param_default_span(&[], "width"), None);
    }

    #[test]
    fn let_member_with_matching_name_returns_none() {
        // Deliberate narrowing vs. `find_named_member_span`, which matches BOTH
        // Param and Let (decl.rs `find_named_member_span_depth`). A `let` has no
        // rewritable param default, so it must not resolve.
        let members = vec![let_member("width", (0, 20), (12, 17))];
        assert_eq!(find_param_default_span(&members, "width"), None);
    }

    // ── step-3: nested reach parity with `find_named_member_span` ────────────

    #[test]
    fn param_inside_guarded_then_branch_resolves() {
        // examples/m5_guarded_enum.ri:7-9 —
        //   where shape == Shape.Round { param diameter : Length = size }
        let members = vec![guarded(
            vec![param("diameter", (0, 40), Some((10, 14)))],
            vec![],
        )];
        assert_eq!(
            find_param_default_span(&members, "diameter"),
            Some(SourceSpan::new(10, 14))
        );
    }

    #[test]
    fn param_inside_guarded_else_branch_resolves() {
        // examples/m5_guarded_enum.ri:9-11 —
        //   } else { param side_length : Length = size }
        let members = vec![guarded(
            vec![],
            vec![param("side_length", (0, 40), Some((30, 34)))],
        )];
        assert_eq!(
            find_param_default_span(&members, "side_length"),
            Some(SourceSpan::new(30, 34))
        );
    }

    #[test]
    fn param_inside_port_body_is_not_reachable_by_its_bare_name() {
        // examples/m5_connect_chain.ri:6 —
        //   port inlet : in FluidPort { param diameter : Length = 25mm }
        //
        // The sibling `find_named_member_span` DOES recurse here, and for
        // hover/goto that is right. For cell-id resolution it is not: the
        // compiler registers this cell as ValueCellId(entity, "inlet.diameter")
        // — the COMPOSITE name — and it lands in CompiledPort.members, which is
        // never merged into TopologyTemplate.value_cells. Returning its span for
        // the bare name "diameter" would hand a caller a range belonging to a
        // cell that is not editable at all.
        let members = vec![port(
            "inlet",
            vec![param("diameter", (0, 40), Some((44, 48)))],
        )];
        assert_eq!(find_param_default_span(&members, "diameter"), None);
        // Nor by the composite name — supporting that would mean splitting on
        // '.' and resolving <port> → <param> deliberately, which α does not do.
        assert_eq!(find_param_default_span(&members, "inlet.diameter"), None);
    }

    #[test]
    fn port_internal_param_does_not_falsely_refuse_a_top_level_one() {
        // The cross-scope collision, and the reason the port arm had to go: an
        // entity may declare a top-level `param d` AND a port with its own
        // `param d`. That is silently legal — shadow_lint deliberately does NOT
        // fold port-internal members into the enclosing frame ("Port-internal
        // members live in the port's own scope"), so it emits no warning and
        // nothing else flags it either.
        //
        // With the port body in the recursion set this pushed `count` to 2 and
        // refused `S.d` — a genuinely editable cell — for an ambiguity that does
        // not exist. The top-level param is the ONLY candidate, so it resolves.
        let members = vec![
            param("d", (0, 30), Some((22, 26))),
            port("inlet", vec![param("d", (40, 80), Some((70, 74)))]),
        ];
        assert_eq!(
            find_param_default_span(&members, "d"),
            Some(SourceSpan::new(22, 26)),
            "a port-internal same-name param must not make a top-level param ambiguous"
        );
    }

    #[test]
    fn param_inside_match_arm_decl_group_resolves() {
        // Parity with `find_named_member_span_depth`'s MatchArmDeclGroup arm.
        let members = vec![match_arm_group(vec![(
            "Round",
            param("bore", (0, 40), Some((18, 22))),
        )])];
        assert_eq!(
            find_param_default_span(&members, "bore"),
            Some(SourceSpan::new(18, 22))
        );
    }

    #[test]
    fn param_within_depth_limit_resolves() {
        // 5 levels of GuardedGroup nesting — well inside MAX_MEMBER_NESTING_DEPTH (32).
        let members = build_nested_guarded_members(5, "deep_param", (10, 14));
        assert_eq!(
            find_param_default_span(&members, "deep_param"),
            Some(SourceSpan::new(10, 14))
        );
    }

    #[test]
    fn param_beyond_depth_limit_returns_none() {
        // 33 levels — past MAX_MEMBER_NESTING_DEPTH (32), so the subtree is cut off.
        let members = build_nested_guarded_members(33, "unreachable_param", (10, 14));
        assert_eq!(find_param_default_span(&members, "unreachable_param"), None);
    }

    // ── step-5: refuse to guess when the name is declared more than once ─────

    #[test]
    fn name_declared_in_both_guarded_branches_returns_none() {
        // `where c { param size = … } else { param size = … }` is LEGAL, not a
        // duplicate-decl error: shadow_lint.rs states GuardedGroup branch members
        // are mutually-exclusive SIBLINGS registered into the SAME parent frame.
        // Nothing in the AST records which branch the condition selects, so
        // first-match-wins would let a caller splice a literal into the possibly
        // INACTIVE branch — the user sets a value and the design does not change.
        let members = vec![guarded(
            vec![param("size", (0, 40), Some((10, 14)))],
            vec![param("size", (50, 90), Some((30, 34)))],
        )];
        assert_eq!(
            find_param_default_span(&members, "size"),
            None,
            "a multiply-declared name must be refused, not resolved to one branch"
        );
    }

    #[test]
    fn name_declared_twice_with_only_one_default_still_returns_none() {
        // Refusal is driven by the name being multiply-declared, NOT by how many
        // defaults happen to exist. Otherwise adding a default to the other branch
        // would silently flip an accepted edit into a rejected one.
        let members = vec![guarded(
            vec![param("size", (0, 40), Some((10, 14)))],
            vec![param("size", (50, 90), None)],
        )];
        assert_eq!(find_param_default_span(&members, "size"), None);
    }

    #[test]
    fn name_declared_in_two_match_arms_returns_none() {
        // The canonical real-source ambiguity, and the one the GuardedGroup cases
        // above do NOT cover: `match shape { Round => param d = …  Square => param
        // d = … }`. Each arm desugars to a same-name guarded decl (spec §6.4), so
        // this is the same mutually-exclusive-siblings shape as `where`/`else` —
        // and just as unresolvable from the AST alone.
        //
        // Load-bearing as a REGRESSION guard on the accumulate-don't-short-circuit
        // rewrite: if the MatchArmDeclGroup recursion ever reverted to returning on
        // first match, every other test in this module would still pass.
        let members = vec![match_arm_group(vec![
            ("Round", param("d", (0, 40), Some((10, 14)))),
            ("Square", param("d", (50, 90), Some((60, 64)))),
        ])];
        assert_eq!(
            find_param_default_span(&members, "d"),
            None,
            "a name declared in two match arms is ambiguous and must be refused"
        );
    }

    #[test]
    fn unambiguous_nesting_still_resolves() {
        // Regression guard for step-3's reach: exactly examples/m5_guarded_enum.ri:7-11,
        // where the two branches declare DIFFERENTLY-named params. Step-6 must narrow
        // only the genuinely ambiguous case.
        let members = vec![guarded(
            vec![param("diameter", (0, 40), Some((10, 14)))],
            vec![param("side_length", (50, 90), Some((60, 64)))],
        )];
        assert_eq!(
            find_param_default_span(&members, "diameter"),
            Some(SourceSpan::new(10, 14))
        );
    }
}

/// GOLDEN-MASTER (characterization) tests for the four member-recursion
/// walkers, pinned against the CURRENT (pre-consolidation) hand-rolled
/// implementations. This module must be green BEFORE `walk_members`/
/// `MemberRecursionSet` exist — it is the safety net the consolidation is
/// checked against, not a test of the new code.
#[cfg(test)]
mod member_recursion_set_tests {
    use super::member_test_fixtures::*;
    use super::{
        MAX_MEMBER_NESTING_DEPTH, MemberDecl, find_named_member_span, find_param_default_span,
        walk_all_member_bodies, walk_specialization_scope_members,
    };
    use reify_core::SourceSpan;

    /// One uniquely-named `param` (each carrying a default span, so
    /// `find_param_default_span` is assertable too) planted in each of the
    /// six nested member bodies the walkers disagree on, plus one top-level
    /// marker. Shared across all four entry points so a single fixture pins
    /// the full 4-entry-point × 7-marker reachability table.
    ///
    /// `marker_keyed` is the second shape of the SAME cell as `marker_sub`: a
    /// sub carries its specialization overrides either in `body` or in
    /// `keyed_members[].overrides`, never both, so every recursion set must
    /// answer both identically.
    fn build_reachability_fixture() -> Vec<MemberDecl> {
        vec![
            param("marker_top", (0, 40), Some((10, 14))),
            MemberDecl::Sub(sub_with_body(
                "nested_sub",
                Some(vec![param("marker_sub", (100, 140), Some((110, 114)))]),
            )),
            port(
                "nested_port",
                vec![param("marker_port", (200, 240), Some((210, 214)))],
            ),
            guarded(
                vec![param("marker_then", (300, 340), Some((310, 314)))],
                vec![],
            ),
            guarded(
                vec![],
                vec![param("marker_else", (400, 440), Some((410, 414)))],
            ),
            match_arm_group(vec![(
                "A",
                param("marker_arm", (500, 540), Some((510, 514))),
            )]),
            MemberDecl::Sub(sub_with_keyed_members(
                "keyed_sub",
                vec![(
                    "a",
                    vec![param("marker_keyed", (600, 640), Some((610, 614)))],
                )],
            )),
        ]
    }

    /// Debug tag for a visited member: names the marker param, or names the
    /// container (distinguishing the two `GuardedGroup`s by which branch is
    /// non-empty, since neither carries its own name).
    fn tag(member: &MemberDecl) -> String {
        match member {
            MemberDecl::Param(p) => format!("param:{}", p.name),
            MemberDecl::Sub(s) => format!("sub:{}", s.name),
            MemberDecl::Port(p) => format!("port:{}", p.name),
            MemberDecl::GuardedGroup(g) => {
                format!("guarded(then={},else={})", g.members.len(), g.else_members.len())
            }
            MemberDecl::MatchArmDeclGroup(_) => "match_arm_group".to_string(),
            other => panic!("reachability fixture should not contain {other:?}"),
        }
    }

    #[test]
    fn walk_specialization_scope_members_reachability_table() {
        let fixture = build_reachability_fixture();
        let sub = sub_with_body("scope", Some(fixture));
        let mut tags = Vec::new();
        walk_specialization_scope_members(&sub, &mut |m| tags.push(tag(m)));

        assert!(
            tags.contains(&"param:marker_top".to_string()),
            "top-level marker must be reached; tags={tags:?}"
        );
        assert!(
            tags.contains(&"param:marker_sub".to_string()),
            "SubDecl.body IS recursed into by walk_specialization_scope_members; tags={tags:?}"
        );
        assert!(
            tags.contains(&"param:marker_then".to_string()),
            "GuardedGroup.members is always recursed; tags={tags:?}"
        );
        assert!(
            tags.contains(&"param:marker_else".to_string()),
            "GuardedGroup.else_members is always recursed; tags={tags:?}"
        );
        assert!(
            tags.contains(&"param:marker_arm".to_string()),
            "MatchArmDeclGroup arms are always recursed; tags={tags:?}"
        );
        assert!(
            tags.contains(&"param:marker_keyed".to_string()),
            "SubDecl.keyed_members[].overrides IS recursed into by \
             walk_specialization_scope_members — a keyed entry's overrides is a \
             specialization body (spec §8.7); tags={tags:?}"
        );
        assert!(
            !tags.contains(&"param:marker_port".to_string()),
            "PortDecl.members must NOT be recursed by walk_specialization_scope_members; tags={tags:?}"
        );

        // Container nodes are visited themselves (not just skipped-through),
        // and parent-before-children ordering holds for every container that
        // does recurse.
        let sub_idx = tags
            .iter()
            .position(|t| t == "sub:nested_sub")
            .expect("Sub container itself must be visited");
        assert!(
            tags.iter().any(|t| t == "port:nested_port"),
            "Port container itself must be visited, even though its body is not descended; tags={tags:?}"
        );
        let then_guard_idx = tags
            .iter()
            .position(|t| t == "guarded(then=1,else=0)")
            .expect("then-branch GuardedGroup container must be visited");
        let else_guard_idx = tags
            .iter()
            .position(|t| t == "guarded(then=0,else=1)")
            .expect("else-branch GuardedGroup container must be visited");
        let arm_group_idx = tags
            .iter()
            .position(|t| t == "match_arm_group")
            .expect("MatchArmDeclGroup container itself must be visited");

        let keyed_sub_idx = tags
            .iter()
            .position(|t| t == "sub:keyed_sub")
            .expect("keyed Sub container itself must be visited");
        let marker_keyed_idx = tags.iter().position(|t| t == "param:marker_keyed").unwrap();
        assert!(
            keyed_sub_idx < marker_keyed_idx,
            "parent-before-children: keyed Sub container must precede its entry's override param"
        );

        let marker_sub_idx = tags.iter().position(|t| t == "param:marker_sub").unwrap();
        let marker_then_idx = tags.iter().position(|t| t == "param:marker_then").unwrap();
        let marker_else_idx = tags.iter().position(|t| t == "param:marker_else").unwrap();
        let marker_arm_idx = tags.iter().position(|t| t == "param:marker_arm").unwrap();

        assert!(
            sub_idx < marker_sub_idx,
            "parent-before-children: Sub container must precede its nested param"
        );
        assert!(
            then_guard_idx < marker_then_idx,
            "parent-before-children: then-branch GuardedGroup must precede its member"
        );
        assert!(
            else_guard_idx < marker_else_idx,
            "parent-before-children: else-branch GuardedGroup must precede its member"
        );
        assert!(
            arm_group_idx < marker_arm_idx,
            "parent-before-children: MatchArmDeclGroup must precede its arm's member"
        );
    }

    /// A KEYED sub handed to `walk_specialization_scope_members` as the scope
    /// ROOT has its entries' overrides walked.
    ///
    /// A code path distinct from the table above: that fixture reaches its keyed
    /// sub through `walk_members`' recursive `Sub` arm, whereas this one hands a
    /// keyed sub straight to the root entry point, which reads the sub's
    /// override lists itself. A keyed ROOT was a silent no-op until both paths
    /// shared `sub_override_bodies`. As with the body form, the root sub itself
    /// is not visited — the walk is over its scope's members.
    #[test]
    fn walk_specialization_scope_members_visits_keyed_entry_overrides() {
        let sub = sub_with_keyed_members(
            "scope",
            vec![("a", vec![param("marker_keyed_root", (0, 40), None)])],
        );
        let mut tags = Vec::new();
        walk_specialization_scope_members(&sub, &mut |m| tags.push(tag(m)));
        assert_eq!(
            tags,
            vec!["param:marker_keyed_root".to_string()],
            "a keyed sub IS a specialization-scope root — one scope per entry (spec §8.7)"
        );
    }

    /// Every keyed entry is its own scope root, in declaration order.
    ///
    /// Guards against a `.first()`-shaped partial fix at the root entry point;
    /// the single-entry test above cannot tell the two apart.
    #[test]
    fn walk_specialization_scope_members_visits_every_keyed_entry_in_source_order() {
        let sub = sub_with_keyed_members(
            "scope",
            vec![
                ("a", vec![param("marker_entry0", (0, 40), None)]),
                ("b", vec![param("marker_entry1", (100, 140), None)]),
            ],
        );
        let mut tags = Vec::new();
        walk_specialization_scope_members(&sub, &mut |m| tags.push(tag(m)));
        assert_eq!(
            tags,
            vec![
                "param:marker_entry0".to_string(),
                "param:marker_entry1".to_string(),
            ],
            "every keyed entry opens its own scope, walked in declaration order"
        );
    }

    /// Widening the sub cell to the keyed shape must NOT widen the port cell.
    ///
    /// `SPECIALIZATION_SCOPE` still carries `port_body: false`, so a port
    /// DECLARED inside a keyed entry's overrides is visited — that is the member
    /// spec §8.7's forbidden-decl rule reports — while its BODY stays unwalked.
    #[test]
    fn walk_specialization_scope_members_keyed_entry_does_not_widen_the_port_cell() {
        let sub = sub_with_keyed_members(
            "scope",
            vec![(
                "a",
                vec![port(
                    "nested_port",
                    vec![param("marker_in_port", (0, 40), None)],
                )],
            )],
        );
        let mut tags = Vec::new();
        walk_specialization_scope_members(&sub, &mut |m| tags.push(tag(m)));
        assert_eq!(
            tags,
            vec!["port:nested_port".to_string()],
            "the port container is visited, its body is NOT — the keyed widening touches \
             only the sub_overrides cell"
        );
    }

    /// A sub carrying NEITHER override shape visits nothing.
    ///
    /// The walker is itself the "is this a specialization-scope root?"
    /// discriminator, which is what lets the compiler's
    /// `find_specialization_scopes` hand it every `MemberDecl::Sub` unguarded
    /// rather than re-deriving the answer from `body.is_some()` in a second
    /// crate.
    #[test]
    fn walk_specialization_scope_members_is_a_no_op_for_a_sub_with_no_overrides() {
        let mut tags = Vec::new();
        walk_specialization_scope_members(&sub_with_body("bare", None), &mut |m| tags.push(tag(m)));
        assert!(
            tags.is_empty(),
            "a bare instantiation / collection / bare-colon-no-body sub opens no \
             specialization scope; tags={tags:?}"
        );

        let mut keyed_tags = Vec::new();
        walk_specialization_scope_members(&sub_with_keyed_members("empty", vec![]), &mut |m| {
            keyed_tags.push(tag(m));
        });
        assert!(
            keyed_tags.is_empty(),
            "a sub with an empty keyed list opens no scope either; tags={keyed_tags:?}"
        );
    }

    #[test]
    fn find_named_member_span_reachability_table() {
        let fixture = build_reachability_fixture();
        for name in [
            "marker_top",
            "marker_port",
            "marker_then",
            "marker_else",
            "marker_arm",
        ] {
            assert!(
                find_named_member_span(&fixture, name).is_some(),
                "find_named_member_span must reach {name}"
            );
        }
        assert!(
            find_named_member_span(&fixture, "marker_sub").is_none(),
            "find_named_member_span must NOT reach into SubDecl.body"
        );
        assert!(
            find_named_member_span(&fixture, "marker_keyed").is_none(),
            "find_named_member_span must NOT reach into SubDecl.keyed_members[].overrides \
             either — a keyed entry's overrides specialize a CHILD instance, exactly as \
             SubDecl.body does, so the same blindness is the correct contract"
        );
    }

    #[test]
    fn find_param_default_span_reachability_table() {
        let fixture = build_reachability_fixture();
        for (name, expected_span) in [
            ("marker_top", (10, 14)),
            ("marker_then", (310, 314)),
            ("marker_else", (410, 414)),
            ("marker_arm", (510, 514)),
        ] {
            assert_eq!(
                find_param_default_span(&fixture, name),
                Some(SourceSpan::new(expected_span.0, expected_span.1)),
                "find_param_default_span must reach {name}"
            );
        }
        assert_eq!(
            find_param_default_span(&fixture, "marker_sub"),
            None,
            "find_param_default_span must NOT reach into SubDecl.body"
        );
        assert_eq!(
            find_param_default_span(&fixture, "marker_keyed"),
            None,
            "find_param_default_span must NOT reach into SubDecl.keyed_members[].overrides — \
             a default found there belongs to a different entity than the caller's cell_id names"
        );
        assert_eq!(
            find_param_default_span(&fixture, "marker_port"),
            None,
            "find_param_default_span must NOT reach into PortDecl.members"
        );
    }

    /// The union set is the ONLY one of the four that reaches all six markers.
    ///
    /// `marker_sub` AND `marker_port` together is the discriminator: no other
    /// const reaches both (SPECIALIZATION_SCOPE misses port, NAMED_MEMBER_LOOKUP
    /// misses sub, PARAM_DEFAULT_LOOKUP misses both). A `walk_all_member_bodies`
    /// accidentally wired to any of them fails HERE.
    #[test]
    fn walk_all_member_bodies_reachability_table() {
        let fixture = build_reachability_fixture();
        let mut tags = Vec::new();
        walk_all_member_bodies(&fixture, &mut |m| tags.push(tag(m)));

        for marker in [
            "marker_top",
            "marker_sub",
            "marker_keyed",
            "marker_port",
            "marker_then",
            "marker_else",
            "marker_arm",
        ] {
            assert!(
                tags.contains(&format!("param:{marker}")),
                "walk_all_member_bodies is the MAXIMAL recursion set and must reach \
                 {marker}; tags={tags:?}"
            );
        }

        // Containers are visited themselves, parent-before-children.
        let sub_idx = tags
            .iter()
            .position(|t| t == "sub:nested_sub")
            .expect("Sub container itself must be visited");
        let port_idx = tags
            .iter()
            .position(|t| t == "port:nested_port")
            .expect("Port container itself must be visited");
        let keyed_sub_idx = tags
            .iter()
            .position(|t| t == "sub:keyed_sub")
            .expect("keyed Sub container itself must be visited");
        let marker_sub_idx = tags.iter().position(|t| t == "param:marker_sub").unwrap();
        let marker_keyed_idx = tags.iter().position(|t| t == "param:marker_keyed").unwrap();
        let marker_port_idx = tags.iter().position(|t| t == "param:marker_port").unwrap();
        assert!(
            sub_idx < marker_sub_idx,
            "parent-before-children: Sub container must precede its nested param; tags={tags:?}"
        );
        assert!(
            keyed_sub_idx < marker_keyed_idx,
            "parent-before-children: keyed Sub container must precede its entry's override \
             param; tags={tags:?}"
        );
        assert!(
            port_idx < marker_port_idx,
            "parent-before-children: Port container must precede its body param; tags={tags:?}"
        );
    }

    /// A keyed sub with TWO entries reaches BOTH entries' overrides, in
    /// declaration order.
    ///
    /// Every table above carries a SINGLE-entry keyed sub, so a `.first()`-shaped
    /// partial descent would satisfy all of them. This is the assertion that
    /// refuses it.
    #[test]
    fn walk_all_member_bodies_visits_every_keyed_entry_in_declaration_order() {
        let members = vec![MemberDecl::Sub(sub_with_keyed_members(
            "keyed",
            vec![
                ("a", vec![param("marker_entry0", (0, 40), None)]),
                ("b", vec![param("marker_entry1", (100, 140), None)]),
            ],
        ))];
        let mut tags = Vec::new();
        walk_all_member_bodies(&members, &mut |m| tags.push(tag(m)));
        assert_eq!(
            tags,
            vec![
                "sub:keyed".to_string(),
                "param:marker_entry0".to_string(),
                "param:marker_entry1".to_string(),
            ],
            "every keyed entry's overrides must be walked, in declaration order"
        );
    }

    // ── shared cross-cutting contract: depth bound ────────────────────────

    #[test]
    fn depth_bound_applies_identically_to_all_four_entry_points() {
        let at_limit =
            build_nested_guarded_members(MAX_MEMBER_NESTING_DEPTH, "deep_param", (10, 14));
        let beyond_limit =
            build_nested_guarded_members(MAX_MEMBER_NESTING_DEPTH + 1, "deep_param", (10, 14));

        let mut names_at_limit = Vec::new();
        walk_specialization_scope_members(
            &sub_with_body("s", Some(at_limit.clone())),
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    names_at_limit.push(p.name.clone());
                }
            },
        );
        assert!(
            names_at_limit.contains(&"deep_param".to_string()),
            "walk_specialization_scope_members: a param at exactly MAX_MEMBER_NESTING_DEPTH must be reached"
        );

        let mut names_beyond_limit = Vec::new();
        walk_specialization_scope_members(
            &sub_with_body("s", Some(beyond_limit.clone())),
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    names_beyond_limit.push(p.name.clone());
                }
            },
        );
        assert!(
            !names_beyond_limit.contains(&"deep_param".to_string()),
            "walk_specialization_scope_members: a param beyond MAX_MEMBER_NESTING_DEPTH must be cut off"
        );

        // A KEYED root is bounded identically. Each keyed entry is its own
        // scope root, so the depth counter resets per entry exactly as it does
        // for a body root — existing semantics, and the keyed shape must not
        // change it.
        let mut keyed_at_limit = Vec::new();
        walk_specialization_scope_members(
            &sub_with_keyed_members("s", vec![("a", at_limit.clone())]),
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    keyed_at_limit.push(p.name.clone());
                }
            },
        );
        assert!(
            keyed_at_limit.contains(&"deep_param".to_string()),
            "walk_specialization_scope_members: a param at exactly MAX_MEMBER_NESTING_DEPTH \
             inside a keyed entry must be reached"
        );

        let mut keyed_beyond_limit = Vec::new();
        walk_specialization_scope_members(
            &sub_with_keyed_members("s", vec![("a", beyond_limit.clone())]),
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    keyed_beyond_limit.push(p.name.clone());
                }
            },
        );
        assert!(
            !keyed_beyond_limit.contains(&"deep_param".to_string()),
            "walk_specialization_scope_members: a param beyond MAX_MEMBER_NESTING_DEPTH \
             inside a keyed entry must be cut off"
        );

        assert!(
            find_named_member_span(&at_limit, "deep_param").is_some(),
            "find_named_member_span: a param at exactly MAX_MEMBER_NESTING_DEPTH must be reached"
        );
        assert!(
            find_named_member_span(&beyond_limit, "deep_param").is_none(),
            "find_named_member_span: a param beyond MAX_MEMBER_NESTING_DEPTH must be cut off"
        );

        assert_eq!(
            find_param_default_span(&at_limit, "deep_param"),
            Some(SourceSpan::new(10, 14)),
            "find_param_default_span: a param at exactly MAX_MEMBER_NESTING_DEPTH must be reached"
        );
        assert_eq!(
            find_param_default_span(&beyond_limit, "deep_param"),
            None,
            "find_param_default_span: a param beyond MAX_MEMBER_NESTING_DEPTH must be cut off"
        );

        let mut all_bodies_at_limit = Vec::new();
        walk_all_member_bodies(&at_limit, &mut |m| {
            if let MemberDecl::Param(p) = m {
                all_bodies_at_limit.push(p.name.clone());
            }
        });
        assert!(
            all_bodies_at_limit.contains(&"deep_param".to_string()),
            "walk_all_member_bodies: a param at exactly MAX_MEMBER_NESTING_DEPTH must be reached"
        );

        let mut all_bodies_beyond_limit = Vec::new();
        walk_all_member_bodies(&beyond_limit, &mut |m| {
            if let MemberDecl::Param(p) = m {
                all_bodies_beyond_limit.push(p.name.clone());
            }
        });
        assert!(
            !all_bodies_beyond_limit.contains(&"deep_param".to_string()),
            "walk_all_member_bodies: a param beyond MAX_MEMBER_NESTING_DEPTH must be cut off"
        );
    }

    // ── shared cross-cutting contract: exit rules ─────────────────────────

    #[test]
    fn find_named_member_span_first_match_wins_over_ambiguous_guarded_branches() {
        let members = vec![guarded(
            vec![param("dup", (0, 40), Some((10, 14)))],
            vec![param("dup", (50, 90), Some((60, 64)))],
        )];
        let found =
            find_named_member_span(&members, "dup").expect("dup is declared in the then-branch");
        assert_eq!(
            found.span,
            SourceSpan::new(0, 40),
            "the then-branch declaration must win (first-match-wins)"
        );
        assert_eq!(
            find_param_default_span(&members, "dup"),
            None,
            "find_param_default_span must refuse a doubly-declared name rather than resolve it"
        );
    }

    #[test]
    fn find_named_member_span_first_match_wins_over_ambiguous_match_arms() {
        let members = vec![match_arm_group(vec![
            ("A", param("dup", (0, 40), Some((10, 14)))),
            ("B", param("dup", (50, 90), Some((60, 64)))),
        ])];
        let found = find_named_member_span(&members, "dup").expect("dup is declared in arm 0");
        assert_eq!(
            found.span,
            SourceSpan::new(0, 40),
            "arm 0's declaration must win (first-match-wins)"
        );
        assert_eq!(
            find_param_default_span(&members, "dup"),
            None,
            "find_param_default_span must refuse a doubly-declared name rather than resolve it"
        );
    }
}

/// RED until step-3 lands `MemberRecursionSet`/`walk_members` — this module
/// must fail to COMPILE (`cannot find type`/`cannot find function`), not fail
/// an assertion, until then.
#[cfg(test)]
mod member_walker_contract_tests {
    use super::member_test_fixtures::*;
    use super::{MemberDecl, MemberRecursionSet, walk_members};
    use std::ops::ControlFlow;

    // ── (a) declarative recursion-set pin ─────────────────────────────────

    #[test]
    fn recursion_set_consts_match_the_drift_table() {
        assert_eq!(
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            MemberRecursionSet {
                sub_overrides: true,
                port_body: false
            },
            "walk_specialization_scope_members recurses a sub's specialization overrides \
             (body or keyed entries), not PortDecl.members"
        );
        assert_eq!(
            MemberRecursionSet::NAMED_MEMBER_LOOKUP,
            MemberRecursionSet {
                sub_overrides: false,
                port_body: true
            },
            "find_named_member_span recurses PortDecl.members, not a sub's specialization \
             overrides"
        );
        assert_eq!(
            MemberRecursionSet::PARAM_DEFAULT_LOOKUP,
            MemberRecursionSet {
                sub_overrides: false,
                port_body: false
            },
            "find_param_default_span recurses neither a sub's specialization overrides nor \
             PortDecl.members"
        );
        assert_eq!(
            MemberRecursionSet::ALL_MEMBER_BODIES,
            MemberRecursionSet {
                sub_overrides: true,
                port_body: true
            },
            "walk_all_member_bodies (priv_redundant_lint.rs's E_PRIV_REDUNDANT walk) is the \
             MAXIMAL set: it recurses BOTH a sub's specialization overrides and \
             PortDecl.members, because a `let`/`constraint` can carry `priv` in either body"
        );
        assert_ne!(
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            MemberRecursionSet::NAMED_MEMBER_LOOKUP
        );
        assert_ne!(
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            MemberRecursionSet::PARAM_DEFAULT_LOOKUP
        );
        assert_ne!(
            MemberRecursionSet::NAMED_MEMBER_LOOKUP,
            MemberRecursionSet::PARAM_DEFAULT_LOOKUP
        );
        // The union set is distinct from all three pre-existing consts — a
        // fold wired to any of them would silently skip a recursion site.
        assert_ne!(
            MemberRecursionSet::ALL_MEMBER_BODIES,
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            "ALL_MEMBER_BODIES also descends into PortDecl.members"
        );
        assert_ne!(
            MemberRecursionSet::ALL_MEMBER_BODIES,
            MemberRecursionSet::NAMED_MEMBER_LOOKUP,
            "ALL_MEMBER_BODIES also descends into a sub's specialization overrides"
        );
        assert_ne!(
            MemberRecursionSet::ALL_MEMBER_BODIES,
            MemberRecursionSet::PARAM_DEFAULT_LOOKUP,
            "ALL_MEMBER_BODIES descends into both optional bodies, PARAM_DEFAULT_LOOKUP neither"
        );
    }

    // ── (b) variant-coverage pin ───────────────────────────────────────────

    /// Mirrors what `walk_members`'s match arms decide for each `MemberDecl`
    /// variant. EXHAUSTIVE with NO `_` arm: adding a new `MemberDecl` variant
    /// must break this test's compile, forcing a deliberate classification
    /// decision here (and, separately, in `walk_members`'s own match).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DescendKind {
        Always,
        IfSubOverrides,
        IfPortBody,
        Never,
    }

    fn descends_into(member: &MemberDecl) -> DescendKind {
        match member {
            MemberDecl::Param(_) => DescendKind::Never,
            MemberDecl::Let(_) => DescendKind::Never,
            MemberDecl::Constraint(_) => DescendKind::Never,
            MemberDecl::ConstraintInst(_) => DescendKind::Never,
            MemberDecl::Sub(_) => DescendKind::IfSubOverrides,
            MemberDecl::Minimize(_) => DescendKind::Never,
            MemberDecl::Maximize(_) => DescendKind::Never,
            MemberDecl::GuardedGroup(_) => DescendKind::Always,
            MemberDecl::AssociatedType(_) => DescendKind::Never,
            MemberDecl::Fn(_) => DescendKind::Never,
            MemberDecl::Port(_) => DescendKind::IfPortBody,
            MemberDecl::Connect(_) => DescendKind::Never,
            MemberDecl::Chain(_) => DescendKind::Never,
            MemberDecl::MetaBlock(_) => DescendKind::Never,
            MemberDecl::ForallConnect(_) => DescendKind::Never,
            MemberDecl::ForallConstraint(_) => DescendKind::Never,
            MemberDecl::MatchArmDeclGroup(_) => DescendKind::Always,
            MemberDecl::Relate(_) => DescendKind::Never,
        }
    }

    /// Drives `walk_members` to completion (never breaks) and reports whether
    /// a `Param` named `"marker"` was reached anywhere in the tree.
    fn reaches_marker(member: MemberDecl, set: MemberRecursionSet) -> bool {
        let members = vec![member];
        let mut reached = false;
        let _: ControlFlow<()> = walk_members(&members, set, 0, &mut |m| {
            if let MemberDecl::Param(p) = m
                && p.name == "marker"
            {
                reached = true;
            }
            ControlFlow::Continue(())
        });
        reached
    }

    /// One row of the nesting-variant table below: the variant's display name,
    /// a builder for a `MemberDecl` of that variant whose optional body holds a
    /// `param "marker"`, and the classification `descends_into` must report.
    /// Named so the tuple stays under `clippy::type_complexity`.
    type NestingVariant = (&'static str, fn() -> MemberDecl, DescendKind);

    #[test]
    fn walk_members_recursion_matches_declared_classification() {
        let nesting_variants: [NestingVariant; 5] = [
            (
                "Sub",
                || MemberDecl::Sub(sub_with_body("s", Some(vec![param("marker", (0, 40), None)]))),
                DescendKind::IfSubOverrides,
            ),
            // The keyed form of the SAME cell: `sub s : Foo { "a" => { … } }`.
            // Classified identically on purpose — one flag answers both shapes,
            // so no set can be body-yes/keyed-no.
            (
                "KeyedSub",
                || {
                    MemberDecl::Sub(sub_with_keyed_members(
                        "s",
                        vec![("a", vec![param("marker", (0, 40), None)])],
                    ))
                },
                DescendKind::IfSubOverrides,
            ),
            (
                "Port",
                || port("p", vec![param("marker", (0, 40), None)]),
                DescendKind::IfPortBody,
            ),
            (
                "GuardedGroup",
                || guarded(vec![param("marker", (0, 40), None)], vec![]),
                DescendKind::Always,
            ),
            (
                "MatchArmDeclGroup",
                || match_arm_group(vec![("A", param("marker", (0, 40), None))]),
                DescendKind::Always,
            ),
        ];
        let recursion_sets: [(&str, MemberRecursionSet); 4] = [
            (
                "SPECIALIZATION_SCOPE",
                MemberRecursionSet::SPECIALIZATION_SCOPE,
            ),
            (
                "NAMED_MEMBER_LOOKUP",
                MemberRecursionSet::NAMED_MEMBER_LOOKUP,
            ),
            (
                "PARAM_DEFAULT_LOOKUP",
                MemberRecursionSet::PARAM_DEFAULT_LOOKUP,
            ),
            ("ALL_MEMBER_BODIES", MemberRecursionSet::ALL_MEMBER_BODIES),
        ];

        for (variant_name, build, expected_kind) in nesting_variants {
            let actual_kind = descends_into(&build());
            assert_eq!(
                actual_kind, expected_kind,
                "{variant_name}'s declared classification is wrong"
            );

            for (set_name, set) in recursion_sets {
                let expected_reach = match expected_kind {
                    DescendKind::Always => true,
                    DescendKind::IfSubOverrides => set.sub_overrides,
                    DescendKind::IfPortBody => set.port_body,
                    DescendKind::Never => false,
                };
                let actual_reach = reaches_marker(build(), set);
                assert_eq!(
                    actual_reach, expected_reach,
                    "{variant_name} under {set_name}: expected reach={expected_reach}, got {actual_reach}"
                );
            }
        }
    }

    // ── (c) early-exit short-circuit ───────────────────────────────────────

    #[test]
    fn walk_members_break_stops_before_else_branch() {
        let members = vec![guarded(
            vec![param("then_marker", (0, 40), None)],
            vec![param("else_marker", (50, 90), None)],
        )];
        let mut visited = Vec::new();
        let result = walk_members(
            &members,
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            0,
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    visited.push(p.name.clone());
                    if p.name == "then_marker" {
                        return ControlFlow::Break("stopped");
                    }
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(result, ControlFlow::Break("stopped"));
        assert_eq!(
            visited,
            vec!["then_marker".to_string()],
            "must stop before the else-branch member is visited"
        );
    }

    #[test]
    fn walk_members_break_stops_before_second_match_arm() {
        let members = vec![match_arm_group(vec![
            ("A", param("arm0_marker", (0, 40), None)),
            ("B", param("arm1_marker", (50, 90), None)),
        ])];
        let mut visited = Vec::new();
        let result = walk_members(
            &members,
            MemberRecursionSet::NAMED_MEMBER_LOOKUP,
            0,
            &mut |m| {
                if let MemberDecl::Param(p) = m {
                    visited.push(p.name.clone());
                    if p.name == "arm0_marker" {
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(result, ControlFlow::Break(()));
        assert_eq!(
            visited,
            vec!["arm0_marker".to_string()],
            "must stop before arm 1's member is visited"
        );
    }

    #[test]
    fn walk_members_break_inside_nested_sub_body_stops_before_next_sibling() {
        let members = vec![
            MemberDecl::Sub(sub_with_body(
                "s",
                Some(vec![param("nested_marker", (0, 40), None)]),
            )),
            param("sibling_marker", (100, 140), None),
        ];
        let mut visited = Vec::new();
        let result = walk_members(
            &members,
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            0,
            &mut |m| {
                match m {
                    MemberDecl::Sub(s) => visited.push(format!("sub:{}", s.name)),
                    MemberDecl::Param(p) => {
                        visited.push(format!("param:{}", p.name));
                        if p.name == "nested_marker" {
                            return ControlFlow::Break(());
                        }
                    }
                    _ => {}
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(result, ControlFlow::Break(()));
        assert_eq!(
            visited,
            vec!["sub:s".to_string(), "param:nested_marker".to_string()],
            "must unwind out of the nested Sub body without visiting the next top-level sibling"
        );
    }

    #[test]
    fn walk_members_continue_visits_every_member_exactly_once() {
        let members = vec![
            param("a", (0, 40), None),
            guarded(
                vec![param("b", (0, 40), None)],
                vec![param("c", (0, 40), None)],
            ),
            match_arm_group(vec![("A", param("d", (0, 40), None))]),
        ];
        let mut visit_count = 0usize;
        let mut param_order = Vec::new();
        let result: ControlFlow<()> = walk_members(
            &members,
            MemberRecursionSet::SPECIALIZATION_SCOPE,
            0,
            &mut |m| {
                visit_count += 1;
                if let MemberDecl::Param(p) = m {
                    param_order.push(p.name.clone());
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(result, ControlFlow::Continue(()));
        // 6 distinct nodes: a, guarded(container), b, c, match_arm_group(container), d.
        assert_eq!(
            visit_count, 6,
            "every member node, including containers, must be visited exactly once"
        );
        assert_eq!(
            param_order,
            vec!["a", "b", "c", "d"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            "left-to-right, parent-before-children order"
        );
    }
}
