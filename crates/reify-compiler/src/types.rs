use std::collections::{HashMap, HashSet};

use reify_types::{
    CompiledExpr, ContentHash, ConstraintDomain, ConstraintNodeId, DimensionVector,
    OptimizationObjective, RealizationNodeId, SourceSpan, Type, ValueCellId,
};

pub use reify_types::{CompiledFnBody, CompiledFunction};

/// A compiled import declaration.
#[derive(Debug, Clone)]
pub struct CompiledImport {
    pub path: String,
    pub kind: reify_syntax::ImportKind,
    pub is_pub: bool,
    pub span: SourceSpan,
}

/// A compiled trait definition.
#[derive(Debug, Clone)]
pub struct CompiledTrait {
    pub name: String,
    pub is_pub: bool,
    /// Type parameters declared on this trait (e.g., `<T: Rigid>`).
    pub type_params: Vec<reify_types::TypeParam>,
    /// Names of traits this trait refines (parent traits).
    pub refinements: Vec<String>,
    /// Members that conforming structures must provide (no default).
    pub required_members: Vec<TraitRequirement>,
    /// Members with defaults that are injected if the structure doesn't override.
    pub defaults: Vec<TraitDefault>,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#precision(bits=32)`).
    pub pragmas: Vec<reify_syntax::Pragma>,
}

/// A required member in a trait — conforming structures must provide this.
#[derive(Debug, Clone)]
pub struct TraitRequirement {
    pub name: String,
    pub kind: RequirementKind,
    pub span: SourceSpan,
}

/// The kind of requirement a trait imposes.
#[derive(Debug, Clone)]
pub enum RequirementKind {
    /// A param with a specific type: `param x : Length`
    Param(Type),
    /// A let with a specific type: `let x : Length`
    Let(Type),
    /// A sub-component: `sub hole = Hole`
    Sub(String),
}

/// A default member provided by a trait — injected if not overridden.
#[derive(Debug, Clone)]
pub struct TraitDefault {
    pub name: Option<String>,
    pub kind: DefaultKind,
    pub span: SourceSpan,
}

/// The kind of default a trait provides.
#[derive(Debug, Clone)]
pub enum DefaultKind {
    /// A param with a default expression: `param x : Length = 10mm`
    Param {
        cell_type: Type,
        default_decl: reify_syntax::ParamDecl,
    },
    /// A let with a value expression: `let x = expr`
    Let(reify_syntax::LetDecl),
    /// A constraint with an expression: `constraint label : expr`
    Constraint(reify_syntax::ConstraintDecl),
}

/// The compiled source of a field.
#[derive(Debug, Clone)]
pub enum CompiledFieldSource {
    /// Analytical field: defined by a lambda expression.
    Analytical { expr: CompiledExpr },
    /// Sampled field: defined by config key-value pairs.
    Sampled { config: Vec<(String, CompiledExpr)> },
    /// Composed field: defined by a composition lambda.
    Composed { expr: CompiledExpr },
    /// Imported field: placeholder for externally-sourced field data.
    Imported,
}

/// A compiled field declaration.
#[derive(Debug, Clone)]
pub struct CompiledField {
    pub name: String,
    pub is_pub: bool,
    pub domain_type: Type,
    pub codomain_type: Type,
    pub source: CompiledFieldSource,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
}

/// A compiled purpose parameter — binds an entity reference.
#[derive(Debug, Clone)]
pub struct CompiledPurposeParam {
    pub name: String,
    pub entity_kind: String,
}

/// A resolved reflective schema query — e.g., `subject.params` resolved to concrete ValueCellIds.
#[derive(Debug, Clone)]
pub struct ResolvedSchemaQuery {
    /// The purpose parameter name this query was on (e.g., "subject").
    pub param_name: String,
    /// The kind of schema query (e.g., "params", "geometric_params", "ports").
    pub query_kind: String,
    /// The resolved ValueCellIds from the bound entity's TopologyTemplate.
    pub resolved_ids: Vec<ValueCellId>,
}

/// A compiled purpose declaration.
#[derive(Debug, Clone)]
pub struct CompiledPurpose {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<CompiledPurposeParam>,
    pub constraints: Vec<CompiledConstraint>,
    pub objective: Option<OptimizationObjective>,
    /// Reflective schema queries resolved at compile time.
    pub resolved_queries: Vec<ResolvedSchemaQuery>,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#solver(method="gradient")`).
    pub pragmas: Vec<reify_syntax::Pragma>,
}

/// A compiled module — the output of the compiler.
#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub path: reify_types::ModulePath,
    pub imports: Vec<CompiledImport>,
    pub enum_defs: Vec<reify_types::EnumDef>,
    pub functions: Vec<CompiledFunction>,
    pub trait_defs: Vec<CompiledTrait>,
    pub fields: Vec<CompiledField>,
    pub compiled_purposes: Vec<CompiledPurpose>,
    pub templates: Vec<TopologyTemplate>,
    /// Compiled unit declarations from this module.
    pub units: Vec<CompiledUnit>,
    /// Compiled type alias declarations from this module.
    pub type_aliases: Vec<CompiledTypeAlias>,
    /// Constraint definitions declared in this module.
    /// Stored so downstream modules can reference them during compilation.
    pub constraint_defs: Vec<reify_syntax::ConstraintDef>,
    /// Module-level pragmas declared in this module (e.g., `#no_prelude`, `#precision`).
    /// All pragmas are stored here, including consumed ones like `#no_prelude`.
    pub pragmas: Vec<reify_syntax::Pragma>,
    pub diagnostics: Vec<reify_types::Diagnostic>,
    pub content_hash: ContentHash,
}

impl CompiledModule {
    /// Returns all templates tagged with `@test`.
    ///
    /// This is the canonical filter for test entities — consumers should prefer
    /// this over scanning `template.annotations` manually. Per Task 267, test
    /// entities are excluded from the normal evaluation graph.
    pub fn test_templates(&self) -> Vec<&TopologyTemplate> {
        self.templates.iter().filter(|t| t.is_test).collect()
    }

    /// Returns all templates NOT tagged with `@test`.
    ///
    /// These are the templates that participate in the normal evaluation graph.
    pub fn non_test_templates(&self) -> Vec<&TopologyTemplate> {
        self.templates.iter().filter(|t| !t.is_test).collect()
    }

    /// Returns all constraint defs tagged with `@test`.
    ///
    /// Uses `ConstraintDef::is_test()` as the canonical predicate.
    pub fn test_constraint_defs(&self) -> Vec<&reify_syntax::ConstraintDef> {
        self.constraint_defs.iter().filter(|d| d.is_test()).collect()
    }

    /// Returns all constraint defs NOT tagged with `@test`.
    pub fn non_test_constraint_defs(&self) -> Vec<&reify_syntax::ConstraintDef> {
        self.constraint_defs.iter().filter(|d| !d.is_test()).collect()
    }
}

/// Whether a TopologyTemplate was compiled from a structure or an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Structure,
    Occurrence,
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityKind::Structure => f.write_str("structure"),
            EntityKind::Occurrence => f.write_str("occurrence"),
        }
    }
}

/// A topology template — compiled from a StructureDef or OccurrenceDef.
/// Contains all the value cells, constraints, and realizations.
#[derive(Debug, Clone)]
pub struct TopologyTemplate {
    pub name: String,
    pub entity_kind: EntityKind,
    pub visibility: Visibility,
    /// Type parameters declared on this structure (e.g., `<T: Rigid>`).
    pub type_params: Vec<reify_types::TypeParam>,
    /// Names of traits this structure declares conformance to (e.g., `["Rigid"]`).
    pub trait_bounds: Vec<String>,
    pub value_cells: Vec<ValueCellDecl>,
    pub constraints: Vec<CompiledConstraint>,
    pub realizations: Vec<RealizationDecl>,
    pub sub_components: Vec<SubComponentDecl>,
    pub ports: Vec<CompiledPort>,
    pub connections: Vec<CompiledConnection>,
    pub guarded_groups: Vec<CompiledGuardedGroup>,
    /// ValueCellIds whose boolean value controls topology (guard cells).
    pub structure_controlling: HashSet<ValueCellId>,
    pub objective: Option<OptimizationObjective>,
    /// Key-value entries from the entity's `meta { ... }` block (if any).
    pub meta: HashMap<String, String>,
    pub content_hash: ContentHash,
    /// True if this template participates in a recursive sub-component cycle.
    /// Set by the post-compilation recursive structure detection pass.
    pub is_recursive: bool,
    /// True if this template is tagged with the `@test` annotation.
    /// Derived at compile time from `annotations`; consumers should prefer this
    /// flag over scanning `annotations` themselves.
    pub is_test: bool,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#solver(backend="ipopt")`).
    pub pragmas: Vec<reify_syntax::Pragma>,
}

/// A compiled connection between ports — compiled from a ConnectDecl or desugared from a ChainDecl.
#[derive(Debug, Clone)]
pub struct CompiledConnection {
    pub left_port: String,
    pub operator: reify_syntax::ConnectOp,
    pub right_port: String,
    pub connector_sub: Option<String>,
    pub compatibility_constraint: ConstraintNodeId,
    pub port_mappings: Vec<(String, String)>,
    pub frame_constraint: Option<ConstraintNodeId>,
    pub span: SourceSpan,
}

/// A compiled port declaration — compiled from a PortDecl.
#[derive(Debug, Clone)]
pub struct CompiledPort {
    pub name: String,
    pub direction: reify_types::PortDirection,
    pub type_name: String,
    pub members: Vec<ValueCellDecl>,
    pub constraints: Vec<CompiledConstraint>,
    pub frame_expr: Option<CompiledExpr>,
}

/// A sub-component declaration — compiled from a SubDecl.
#[derive(Debug, Clone)]
pub struct SubComponentDecl {
    pub name: String,
    pub structure_name: String,
    pub visibility: Visibility,
    pub args: Vec<(String, CompiledExpr)>,
    /// Resolved type arguments for parameterized structures
    /// (e.g., `Box<Bolt>()` → `[StructureRef("Bolt")]`; `Box<U>()` → `[TypeParam("U")]`).
    pub type_args: Vec<Type>,
    /// True if this sub uses collection form: `sub name : List<T>`
    pub is_collection: bool,
    /// For collection subs, the synthetic count ValueCell (e.g. `__count_bolts`)
    pub count_cell: Option<ValueCellId>,
    /// Optional guard expression for recursive termination (e.g., `where n > 0`).
    pub guard_expr: Option<CompiledExpr>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A compiled guarded group — a set of members/constraints active only when a guard condition is true.
#[derive(Debug, Clone)]
pub struct CompiledGuardedGroup {
    /// The compiled guard condition expression.
    pub guard_expr: CompiledExpr,
    /// Synthetic ValueCellId for the guard (Bool, Let kind).
    pub guard_value_cell: ValueCellId,
    /// Members active when guard is true.
    pub members: Vec<ValueCellDecl>,
    /// Constraints active when guard is true.
    pub constraints: Vec<CompiledConstraint>,
    /// Members active when guard is false (else branch).
    pub else_members: Vec<ValueCellDecl>,
    /// Constraints active when guard is false (else branch).
    pub else_constraints: Vec<CompiledConstraint>,
    /// Parent guard ValueCellId for nested guards (None for top-level guards).
    /// Used to suppress false-positive cross-guard diagnostics when
    /// inner guard members reference outer guard members.
    pub parent_guard: Option<ValueCellId>,
}

/// A value cell declaration (param or let).
#[derive(Debug, Clone)]
pub struct ValueCellDecl {
    pub id: ValueCellId,
    pub kind: ValueCellKind,
    pub visibility: Visibility,
    pub cell_type: Type,
    pub default_expr: Option<CompiledExpr>,
    pub span: SourceSpan,
}

/// Whether a value cell is a parameter (externally settable), a let (computed),
/// or an auto parameter (solver-determined).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueCellKind {
    Param,
    Let,
    /// Solver-determined parameter: starts as Undef, value provided by constraint solver.
    /// `free`: when true this is an `auto(free)` parameter that skips uniqueness verification.
    Auto { free: bool },
}

impl ValueCellKind {
    /// Returns `true` for any `Auto` variant (strict or free).
    pub fn is_auto(&self) -> bool {
        matches!(self, ValueCellKind::Auto { .. })
    }

    /// Returns `true` only for `Auto { free: true }`.
    pub fn is_auto_free(&self) -> bool {
        matches!(self, ValueCellKind::Auto { free: true })
    }
}

/// Visibility of a declaration: `Public` if accessible from outside, `Private` if internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
}

/// A compiled constraint.
#[derive(Debug, Clone)]
pub struct CompiledConstraint {
    pub id: ConstraintNodeId,
    pub label: Option<String>,
    pub expr: CompiledExpr,
    pub span: SourceSpan,
    /// Optional pre-classified constraint domain. When `None`, the
    /// classifier determines the domain at solve time.
    pub domain: Option<ConstraintDomain>,
}

/// A realization declaration — specifies geometry to produce.
#[derive(Debug, Clone)]
pub struct RealizationDecl {
    pub id: RealizationNodeId,
    pub operations: Vec<CompiledGeometryOp>,
    pub span: SourceSpan,
}

/// A compiled geometry operation.
#[derive(Debug, Clone)]
pub enum CompiledGeometryOp {
    /// Create a primitive shape.
    Primitive {
        kind: PrimitiveKind,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Boolean operation on two geometry refs.
    Boolean {
        op: BooleanOp,
        left: GeomRef,
        right: GeomRef,
    },
    /// Modify a shape (fillet, chamfer).
    Modify {
        kind: ModifyKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Transform a shape (translate, rotate).
    Transform {
        kind: TransformKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Pattern a shape (linear, circular, mirror).
    Pattern {
        kind: PatternKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Sweep operation (loft).
    Sweep {
        kind: SweepKind,
        profiles: Vec<GeomRef>,
        args: Vec<(String, CompiledExpr)>,
    },
}

/// Primitive geometry kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    Box,
    Cylinder,
    Sphere,
}

/// Boolean geometry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

/// Modification operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifyKind {
    Fillet,
    Chamfer,
    Shell,
    Draft,
    Thicken,
}

/// Transform operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformKind {
    Translate,
    Rotate,
    Scale,
    RotateAround,
}

/// Pattern operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternKind {
    Linear,
    Circular,
    Mirror,
}

/// Sweep operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SweepKind {
    Loft,
    Extrude,
    Revolve,
    Sweep,
}

/// Reference to a geometry result within a realization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GeomRef {
    /// Result of a previous operation (by index in the operations list).
    Step(usize),
    /// A sub-component's geometry output.
    Sub(String),
}

/// A compiled unit — the public output representation in `CompiledModule`.
#[derive(Debug, Clone)]
pub struct CompiledUnit {
    pub name: String,
    pub is_pub: bool,
    pub dimension: DimensionVector,
    pub factor: f64,
    pub offset: Option<f64>,
    pub content_hash: ContentHash,
}

/// A compiled type alias — the public output representation in `CompiledModule`.
///
/// Contains only semantic data (no `TypeExpr` from `reify_syntax`), preserving
/// the module boundary: downstream crates consuming `CompiledModule` do not
/// transitively depend on `reify_syntax`.
#[derive(Debug, Clone)]
pub struct CompiledTypeAlias {
    pub name: String,
    /// The resolved type for non-parameterized aliases; `None` for parameterized aliases.
    pub resolved_type: Option<Type>,
    /// Type parameters for parameterized aliases (empty for simple aliases).
    pub type_params: Vec<reify_types::TypeParam>,
    pub is_pub: bool,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}
