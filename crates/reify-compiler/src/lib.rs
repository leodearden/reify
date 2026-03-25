pub mod module_dag;

use std::collections::{HashMap, HashSet};

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ConstraintDomain, ConstraintNodeId, ContentHash,
    DimensionVector, Diagnostic, DiagnosticLabel, OptimizationObjective, RealizationNodeId,
    ResolvedFunction, SourceSpan, Type, UnOp, Value, ValueCellId, FIELD_ENTITY_PREFIX,
};

/// A compiled import declaration.
#[derive(Debug, Clone)]
pub struct CompiledImport {
    pub path: String,
    pub kind: reify_syntax::ImportKind,
    pub is_pub: bool,
    pub span: SourceSpan,
}

pub use reify_types::{CompiledFnBody, CompiledFunction};

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
    pub diagnostics: Vec<reify_types::Diagnostic>,
    pub content_hash: ContentHash,
}

/// Whether a TopologyTemplate was compiled from a structure or an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Structure,
    Occurrence,
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
    Auto,
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

// --- Known geometry-producing functions (M1) ---
/// Returns true if the function name refers to a geometry primitive.
fn is_geometry_function(name: &str) -> bool {
    matches!(
        name,
        "box" | "cylinder"
            | "sphere"
            | "linear_pattern"
            | "circular_pattern"
            | "mirror"
            | "loft"
            | "shell"
            | "thicken"
            | "draft"
            | "union"
            | "intersection"
            | "difference"
            | "union_all"
            | "intersection_all"
            | "sweep"
    )
}

// --- Unit conversion ---

/// Convert a unit string and value to an SI-based `Value::Scalar`.
/// Returns `None` if the unit is unrecognized.
fn unit_to_scalar(value: f64, unit: &str) -> Option<(Value, DimensionVector)> {
    match unit {
        "mm" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "cm" => Some((
            Value::Scalar {
                si_value: value * 0.01,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "m" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "in" => Some((
            Value::Scalar {
                si_value: value * 0.0254,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "deg" => Some((
            Value::Scalar {
                si_value: value * std::f64::consts::PI / 180.0,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "rad" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "kg" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "g" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "s" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::TIME,
            },
            DimensionVector::TIME,
        )),
        _ => None,
    }
}

// --- Type resolution ---

/// Resolve a type name to a `Type`.
fn resolve_type_name(name: &str) -> Option<Type> {
    match name {
        "Scalar" => Some(Type::length()), // Default scalar is length-dimensioned in M1
        "Bool" => Some(Type::Bool),
        "Int" => Some(Type::Int),
        "Real" => Some(Type::Real),
        "String" => Some(Type::String),
        "Length" => Some(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        "Mass" => Some(Type::Scalar {
            dimension: DimensionVector::MASS,
        }),
        _ => None,
    }
}

/// Resolve a type name, also checking type parameter names.
/// Returns `Type::TypeParam(name)` if the name matches a known type parameter.
fn resolve_type_with_params(name: &str, type_param_names: &HashSet<String>) -> Option<Type> {
    if let Some(ty) = resolve_type_name(name) {
        return Some(ty);
    }
    if type_param_names.contains(name) {
        return Some(Type::TypeParam(name.to_string()));
    }
    None
}

/// Convert parsed TypeParamDecl to compiled TypeParam structs.
fn convert_type_params(decls: &[reify_syntax::TypeParamDecl]) -> Vec<reify_types::TypeParam> {
    decls
        .iter()
        .map(|d| {
            let bounds = d
                .bounds
                .iter()
                .map(|b| reify_types::TraitBound {
                    trait_ref: reify_types::TraitRef {
                        name: b.clone(),
                        type_args: vec![],
                    },
                })
                .collect();
            // Resolve defaults: try builtin types first, then preserve
            // structure names as StructureRef (concrete names, not type variables).
            let default = d.default.as_ref().map(|te| {
                resolve_type_name(&te.name)
                    .unwrap_or_else(|| Type::StructureRef(te.name.clone()))
            });
            reify_types::TypeParam {
                name: d.name.clone(),
                bounds,
                default,
            }
        })
        .collect()
}

/// Check whether `from` can be implicitly converted to `to`.
///
/// Encodes all four directional conversion rules for tensor/vector/matrix types:
/// 1. `Vector<N,Q>` ↔ `Tensor<1,N,Q>` — bidirectional
/// 2. `Q` ↔ `Tensor<0,_,Q>` — bidirectional; N is ignored for rank-0
/// 3. `Tensor<2,N,Q>` → `Matrix<N,N,Q>` — one-way (Tensor2 promotes to square matrix)
/// 4. `Matrix` → `Tensor` — NOT implicit (handled by default false return)
///
/// Identity (`from == to`) always returns true.
///
/// **Not applied during overload resolution** (which stays exact-match to avoid
/// ambiguity between `f(Vector<3>)` and `f(Tensor<1,3>)`). Used in trait
/// conformance and field composition type checks.
pub fn implicitly_converts_to(from: &Type, to: &Type) -> bool {
    // Identity: same type always converts to itself.
    if from == to {
        return true;
    }

    match (from, to) {
        // Rule 1a: Vector<N,Q> -> Tensor<1,N,Q>
        (
            Type::Vector { n: vn, quantity: vq },
            Type::Tensor { rank: 1, n: tn, quantity: tq },
        ) => vn == tn && vq == tq,

        // Rule 1b: Tensor<1,N,Q> -> Vector<N,Q>
        (
            Type::Tensor { rank: 1, n: tn, quantity: tq },
            Type::Vector { n: vn, quantity: vq },
        ) => tn == vn && tq == vq,

        // Rule 2a: Q -> Tensor<0,_,Q>  (N is irrelevant for rank-0)
        (
            from_ty,
            Type::Tensor { rank: 0, quantity: tq, .. },
        ) => from_ty == tq.as_ref(),

        // Rule 2b: Tensor<0,_,Q> -> Q  (N is irrelevant for rank-0)
        (
            Type::Tensor { rank: 0, quantity: tq, .. },
            to_ty,
        ) => tq.as_ref() == to_ty,

        // Rule 3: Tensor<2,N,Q> -> Matrix<N,N,Q>  (one-way, square matrices only)
        // Note: Matrix->Tensor is NOT allowed; the default `false` arm handles that.
        (
            Type::Tensor { rank: 2, n: tn, quantity: tq },
            Type::Matrix { m, n: mn, quantity: mq },
        ) => tn == m && tn == mn && tq == mq,

        _ => false,
    }
}

/// Check if an argument type is compatible with a parameter type.
/// Exact match always works. Int→Real widening is allowed.
/// Implicit tensor/vector/matrix conversions are also checked (bidirectional).
///
/// Not used in overload resolution (which uses exact matching), but used
/// in trait conformance and field composition checks.
pub fn type_compatible(param_ty: &Type, arg_ty: &Type) -> bool {
    if param_ty == arg_ty {
        return true;
    }
    // Allow Int→Real widening coercion
    if matches!((param_ty, arg_ty), (Type::Real, Type::Int)) {
        return true;
    }
    // Bidirectional implicit tensor/vector/matrix conversions
    if implicitly_converts_to(param_ty, arg_ty) || implicitly_converts_to(arg_ty, param_ty) {
        return true;
    }
    false
}

/// Result of attempting to resolve a function call against user-defined functions.
enum OverloadResolution<'a> {
    /// Exactly one user-defined function matches by name, arity, and exact param types.
    Resolved(&'a CompiledFunction),
    /// No user-defined function has this name at all — fall through to stdlib.
    NoUserFunctions,
    /// User-defined functions with this name exist, but none match the given arg types.
    /// Carries all same-name candidates for error reporting.
    NoMatch(Vec<&'a CompiledFunction>),
    /// Multiple user-defined functions match — ambiguous call.
    /// Carries all matching candidates for error reporting.
    Ambiguous(Vec<&'a CompiledFunction>),
}

/// Resolve a function call against the list of compiled user functions.
///
/// Uses **exact** type matching (param_ty == arg_ty). Int→Real widening is
/// NOT applied during overload resolution so that `f(Int)` and `f(Real)` are
/// treated as distinct overloads.
fn resolve_function_overload<'a>(
    name: &str,
    arg_types: &[Type],
    functions: &'a [CompiledFunction],
) -> OverloadResolution<'a> {
    // All user functions with the given name (for error reporting).
    let named: Vec<&CompiledFunction> = functions
        .iter()
        .filter(|f| f.name == name)
        .collect();

    if named.is_empty() {
        return OverloadResolution::NoUserFunctions;
    }

    // Among named functions, filter by arity and exact param types.
    let matches: Vec<&CompiledFunction> = named
        .iter()
        .copied()
        .filter(|f| {
            f.params.len() == arg_types.len()
                && f.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|((_, param_ty), arg_ty)| param_ty == arg_ty)
        })
        .collect();

    match matches.len() {
        1 => OverloadResolution::Resolved(matches[0]),
        0 => OverloadResolution::NoMatch(named),
        _ => OverloadResolution::Ambiguous(matches),
    }
}

/// Format a function signature for error messages: `name(T1, T2) -> Ret`.
fn format_fn_signature(f: &CompiledFunction) -> String {
    format!(
        "{}({}) -> {}",
        f.name,
        f.params
            .iter()
            .map(|(_, t)| format!("{}", t))
            .collect::<Vec<_>>()
            .join(", "),
        f.return_type
    )
}

// --- Chained comparison helpers ---

/// Returns true if `op` is a comparison operator that participates in chaining.
fn is_comparison_op(op: &str) -> bool {
    matches!(op, "<" | "<=" | ">" | ">=" | "==" | "!=")
}

/// Flatten a left-nested comparison chain into (operands, operators).
///
/// Given `BinOp(op2, BinOp(op1, a, b), c)` where both op1 and op2 are comparison
/// operators, returns `([a, b, c], [op1, op2])`.
///
/// `outer_op`, `left`, and `right` are the components of the outermost BinOp.
/// Precondition: `outer_op` is a comparison op and `left` is a comparison BinOp.
fn flatten_comparison_chain<'a>(
    outer_op: &'a str,
    left: &'a reify_syntax::Expr,
    right: &'a reify_syntax::Expr,
) -> (Vec<&'a reify_syntax::Expr>, Vec<&'a str>) {
    match &left.kind {
        reify_syntax::ExprKind::BinOp { op: inner_op, left: ll, right: lr }
            if is_comparison_op(inner_op) =>
        {
            // Recurse: flatten the left subtree first, then append current right and op
            let (mut operands, mut ops) = flatten_comparison_chain(inner_op, ll, lr);
            operands.push(right);
            ops.push(outer_op);
            (operands, ops)
        }
        _ => {
            // Base case: left is not a comparison chain; operands = [left, right], ops = [outer_op]
            (vec![left, right], vec![outer_op])
        }
    }
}

// --- BinOp resolution ---

/// Parse a string operator into a `BinOp`.
fn resolve_binop(op: &str) -> Option<BinOp> {
    match op {
        "+" => Some(BinOp::Add),
        "-" => Some(BinOp::Sub),
        "*" => Some(BinOp::Mul),
        "/" => Some(BinOp::Div),
        "%" => Some(BinOp::Mod),
        "**" | "^" => Some(BinOp::Pow),
        "==" => Some(BinOp::Eq),
        "!=" => Some(BinOp::Ne),
        "<" => Some(BinOp::Lt),
        "<=" => Some(BinOp::Le),
        ">" => Some(BinOp::Gt),
        ">=" => Some(BinOp::Ge),
        "&&" | "and" => Some(BinOp::And),
        "||" | "or" => Some(BinOp::Or),
        _ => None,
    }
}

/// Parse a string unary operator into a `UnOp`.
fn resolve_unop(op: &str) -> Option<UnOp> {
    match op {
        "-" => Some(UnOp::Neg),
        "!" | "not" => Some(UnOp::Not),
        _ => None,
    }
}

// --- Type inference for binary operations ---

/// Infer the result type of a binary operation given operand types.
fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        | BinOp::And | BinOp::Or => Type::Bool,
        BinOp::Add | BinOp::Sub => left.clone(), // same dimension required
        BinOp::Mul => match (left, right) {
            (
                Type::Scalar { dimension: ld },
                Type::Scalar { dimension: rd },
            ) => Type::Scalar {
                dimension: ld.mul(rd),
            },
            (Type::Scalar { .. }, _) | (_, Type::Scalar { .. }) => {
                // Scalar * non-scalar preserves the scalar type
                if let Type::Scalar { .. } = left {
                    left.clone()
                } else {
                    right.clone()
                }
            }
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Div => match (left, right) {
            (
                Type::Scalar { dimension: ld },
                Type::Scalar { dimension: rd },
            ) => {
                let result = ld.div(rd);
                if result.is_dimensionless() {
                    Type::Real
                } else {
                    Type::Scalar { dimension: result }
                }
            }
            (Type::Scalar { .. }, _) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified for M1
    }
}

// --- Compilation context ---

/// Name scope: maps identifier names to (ValueCellId, Type, Option<guard_cell_id>)
/// within a structure. The guard cell ID tracks which guard (if any) protects this name.
#[derive(Clone)]
struct CompilationScope {
    entity_name: String,
    names: HashMap<String, (ValueCellId, Type, Option<ValueCellId>)>,
    /// Names of ports declared in this structure, for member access disambiguation.
    port_names: HashSet<String>,
    /// Names of collection sub-components (sub name : List<T>), for count expression handling.
    collection_sub_names: HashSet<String>,
    /// Member types for collection sub-components: collection_name → { member_name → Type }.
    /// Populated from already-compiled child templates to resolve correct types for
    /// indexed member access (e.g., bolts[0].diameter → Type::length()).
    collection_sub_member_types: HashMap<String, HashMap<String, Type>>,
    /// Meta block entries for the current entity: key → value.
    meta_entries: HashMap<String, String>,
}

impl CompilationScope {
    fn new(entity_name: &str) -> Self {
        CompilationScope {
            entity_name: entity_name.to_string(),
            names: HashMap::new(),
            port_names: HashSet::new(),
            collection_sub_names: HashSet::new(),
            collection_sub_member_types: HashMap::new(),
            meta_entries: HashMap::new(),
        }
    }

    fn register(&mut self, name: &str, ty: Type) {
        let id = ValueCellId::new(&self.entity_name, name);
        self.names.insert(name.to_string(), (id, ty, None));
    }

    fn register_guarded(&mut self, name: &str, ty: Type, guard: ValueCellId) {
        let id = ValueCellId::new(&self.entity_name, name);
        self.names.insert(name.to_string(), (id, ty, Some(guard)));
    }

    fn resolve(&self, name: &str) -> Option<(&ValueCellId, &Type)> {
        self.names.get(name).map(|(id, ty, _)| (id, ty))
    }

}

/// Compile an `Expr` from the AST into a `CompiledExpr`.
///
/// Returns `Ok(compiled_expr)` on success or `Err(diagnostic)` on failure.
fn compile_expr(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    let mut lambda_counter = 0u32;
    compile_expr_guarded(expr, scope, enum_defs, functions, diagnostics, None, &mut lambda_counter)
}

/// Compile an `Expr` from the AST into a `CompiledExpr`, with guard context.
///
/// When `current_guard` is Some, references to names guarded by a different
/// guard will produce a diagnostic error about unsafe unguarded references.
#[allow(clippy::only_used_in_recursion)]
fn compile_expr_guarded(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    current_guard: Option<&ValueCellId>,
    lambda_counter: &mut u32,
) -> CompiledExpr {
    match &expr.kind {
        reify_syntax::ExprKind::NumberLiteral(v) => {
            // Whole numbers become Int, fractional become Real
            if *v == (*v as i64) as f64 && v.is_finite() {
                CompiledExpr::literal(Value::Int(*v as i64), Type::Int)
            } else {
                CompiledExpr::literal(Value::Real(*v), Type::Real)
            }
        }
        reify_syntax::ExprKind::QuantityLiteral { value, unit } => {
            match unit_to_scalar(*value, unit) {
                Some((scalar_val, dimension)) => {
                    let ty = Type::Scalar { dimension };
                    CompiledExpr::literal(scalar_val, ty)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown unit: {}", unit))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized unit")),
                    );
                    // Return an undef literal as a fallback
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::BoolLiteral(b) => {
            CompiledExpr::literal(Value::Bool(*b), Type::Bool)
        }
        reify_syntax::ExprKind::StringLiteral(s) => {
            CompiledExpr::literal(Value::String(s.clone()), Type::String)
        }
        reify_syntax::ExprKind::Ident(name) => {
            match scope.resolve(name) {
                Some((id, ty)) => {
                    CompiledExpr::value_ref(id.clone(), ty.clone())
                }
                None => {
                    // Check if this is a collection sub name — resolve to per-member __list_{name}__{member}
                    if scope.collection_sub_names.contains(name.as_str()) {
                        if let Some(members) = scope.collection_sub_member_types.get(name.as_str()) {
                            // Resolve to the first member's per-member list
                            if let Some((first_member, member_ty)) = members.iter().next() {
                                let list_id = ValueCellId::new(&scope.entity_name, format!("__list_{}__{}", name, first_member));
                                let list_type = Type::List(Box::new(member_ty.clone()));
                                return CompiledExpr::value_ref(list_id, list_type);
                            }
                        }
                        // Fallback: no member types available
                        let list_id = ValueCellId::new(&scope.entity_name, format!("__list_{}", name));
                        let list_type = Type::List(Box::new(Type::StructureRef(name.clone())));
                        return CompiledExpr::value_ref(list_id, list_type);
                    }
                    diagnostics.push(
                        Diagnostic::error(format!("unresolved name: {}", name))
                            .with_label(DiagnosticLabel::new(expr.span, "not found in scope")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::BinOp { op, left, right } => {
            // Chained comparison desugaring: `a < b < c` → `And(Lt(a,b), Lt(b,c))`.
            // Detect when the outer op is a comparison and the left operand is also a comparison BinOp.
            if is_comparison_op(op)
                && let reify_syntax::ExprKind::BinOp { op: inner_op, .. } = &left.kind
                && is_comparison_op(inner_op)
            {
                let (operands, ops) = flatten_comparison_chain(op, left, right);
                // Compile each operand exactly once
                let compiled_operands: Vec<CompiledExpr> = operands
                    .iter()
                    .map(|e| compile_expr_guarded(e, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter))
                    .collect();
                // Build pairwise comparison nodes
                let mut pairs: Vec<CompiledExpr> = Vec::new();
                for (i, op_str) in ops.iter().enumerate() {
                    match resolve_binop(op_str) {
                        Some(bin_op) => {
                            let lhs = compiled_operands[i].clone();
                            let rhs = compiled_operands[i + 1].clone();
                            let result_type = infer_binop_type(bin_op, &lhs.result_type, &rhs.result_type);
                            pairs.push(CompiledExpr::binop(bin_op, lhs, rhs, result_type));
                        }
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!("unknown operator: {}", op_str))
                                    .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Real);
                        }
                    }
                }
                // Left-fold pairs into And-chain
                let mut acc = pairs.remove(0);
                for pair in pairs {
                    acc = CompiledExpr::binop(BinOp::And, acc, pair, Type::Bool);
                }
                return acc;
            }

            let compiled_left = compile_expr_guarded(left, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let compiled_right = compile_expr_guarded(right, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            match resolve_binop(op) {
                Some(bin_op) => {
                    let result_type = infer_binop_type(
                        bin_op,
                        &compiled_left.result_type,
                        &compiled_right.result_type,
                    );

                    // Dimension compatibility check for Add/Sub
                    if matches!(bin_op, BinOp::Add | BinOp::Sub) {
                        let op_name = if bin_op == BinOp::Add { "addition" } else { "subtraction" };
                        match (&compiled_left.result_type, &compiled_right.result_type) {
                            // Scalar + Scalar with different dimensions
                            (
                                Type::Scalar { dimension: ld },
                                Type::Scalar { dimension: rd },
                            ) if ld != rd => {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "dimension mismatch in {}: {} vs {}",
                                        op_name,
                                        compiled_left.result_type,
                                        compiled_right.result_type,
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "incompatible dimensions",
                                    )),
                                );
                            }
                            // Scalar + Int/Real or Int/Real + Scalar (dimensioned + dimensionless)
                            (Type::Scalar { .. }, Type::Int | Type::Real)
                            | (Type::Int | Type::Real, Type::Scalar { .. }) => {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "incompatible types in {}: {} vs {}",
                                        op_name,
                                        compiled_left.result_type,
                                        compiled_right.result_type,
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "dimensioned + dimensionless",
                                    )),
                                );
                            }
                            _ => {}
                        }
                    }

                    CompiledExpr::binop(bin_op, compiled_left, compiled_right, result_type)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::UnOp { op, operand } => {
            let compiled_operand = compile_expr_guarded(operand, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            match resolve_unop(op) {
                Some(un_op) => {
                    let result_type = match un_op {
                        UnOp::Not => Type::Bool,
                        UnOp::Neg => compiled_operand.result_type.clone(),
                    };
                    CompiledExpr::unop(un_op, compiled_operand, result_type)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown unary operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::FunctionCall { name, args } => {
            let compiled_args: Vec<CompiledExpr> = args
                .iter()
                .map(|arg| compile_expr_guarded(arg, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter))
                .collect();

            let arg_types: Vec<Type> = compiled_args.iter().map(|a| a.result_type.clone()).collect();

            match resolve_function_overload(name, &arg_types, functions) {
                OverloadResolution::Resolved(matched_fn) => {
                    // Exactly one user fn matches — emit UserFunctionCall
                    let result_type = matched_fn.return_type.clone();
                    let content_hash = {
                        let mut h = ContentHash::of(&[6])
                            .combine(ContentHash::of_str(name));
                        for arg in &compiled_args {
                            h = h.combine(arg.content_hash);
                        }
                        h
                    };
                    CompiledExpr {
                        kind: CompiledExprKind::UserFunctionCall {
                            function_name: name.clone(),
                            args: compiled_args,
                        },
                        result_type,
                        content_hash,
                    }
                }
                OverloadResolution::Ambiguous(candidates) => {
                    // Multiple user fns match — ambiguous call
                    let candidate_sigs: Vec<String> =
                        candidates.iter().map(|f| format_fn_signature(f)).collect();
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "ambiguous function call: {} candidates match {}({}): {}",
                            candidates.len(),
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "ambiguous call")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
                OverloadResolution::NoMatch(named_candidates) => {
                    // User functions with this name exist, but none match — error with candidates
                    let candidate_sigs: Vec<String> =
                        named_candidates.iter().map(|f| format_fn_signature(f)).collect();
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "no matching overload for {}({}), candidates: {}",
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "no matching overload")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
                OverloadResolution::NoUserFunctions => {
                    // No user fn with this name — fall through to stdlib FunctionCall
                    let resolved = ResolvedFunction {
                        name: name.clone(),
                        qualified_name: format!("std::{}", name),
                    };

                    // Infer a result type — for geometry functions, use a placeholder
                    let result_type = if is_geometry_function(name) {
                        Type::dimensionless_scalar()
                    } else {
                        compiled_args
                            .first()
                            .map(|a| a.result_type.clone())
                            .unwrap_or(Type::Real)
                    };

                    let content_hash = {
                        let mut h = ContentHash::of(&[4])
                            .combine(ContentHash::of_str(&resolved.qualified_name));
                        for arg in &compiled_args {
                            h = h.combine(arg.content_hash);
                        }
                        h
                    };

                    CompiledExpr {
                        kind: CompiledExprKind::FunctionCall {
                            function: resolved,
                            args: compiled_args,
                        },
                        result_type,
                        content_hash,
                    }
                }
            }
        }
        reify_syntax::ExprKind::MemberAccess { object, member } => {
            // Check if this is a port member access (port_name.member_name)
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.port_names.contains(name.as_str()) {
                    let composite_key = format!("{}.{}", name, member);
                    if let Some((id, ty)) = scope.resolve(&composite_key) {
                        let id = id.clone();
                        let ty = ty.clone();
                        return CompiledExpr::value_ref(id, ty);
                    } else {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "port '{}' has no member '{}'",
                                name, member
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown port member")),
                        );
                        return CompiledExpr::literal(Value::Undef, Type::Real);
                    }
                }

            // Check if this is an indexed collection member access: collection[i].member
            if let reify_syntax::ExprKind::IndexAccess { object: idx_obj, index } = &object.kind
                && let reify_syntax::ExprKind::Ident(name) = &idx_obj.kind
                && scope.collection_sub_names.contains(name.as_str())
            {
                // Resolve member type from pre-populated collection_sub_member_types
                let member_type = match scope.collection_sub_member_types
                    .get(name.as_str())
                    .and_then(|m| m.get(member.as_str()))
                    .cloned()
                {
                    Some(ty) => ty,
                    None => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "unknown member '{}' on collection sub '{}'",
                                member, name
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                        );
                        Type::Real // fallback to allow continued compilation
                    }
                };

                // For literal integer index, resolve directly to a scoped ValueRef
                if let reify_syntax::ExprKind::NumberLiteral(n) = &index.kind {
                    if n.fract() != 0.0 || *n < 0.0 {
                        diagnostics.push(
                            Diagnostic::error("collection index must be a non-negative integer literal")
                                .with_label(DiagnosticLabel::new(expr.span, "invalid index")),
                        );
                        return CompiledExpr::literal(Value::Undef, member_type);
                    }
                    let i = *n as i64;
                    let scoped_entity = format!("{}.{}[{}]", scope.entity_name, name, i);
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    return CompiledExpr::value_ref(scoped_id, member_type);
                }
                // For non-literal index, compile as IndexAccess into a per-member synthetic list.
                // The eval engine creates __list_{name}__{member} cells that gather each
                // instance's member value into a List, so indexing gives the right value.
                let list_member = format!("__list_{}__{}", name, member);
                let list_id = ValueCellId::new(&scope.entity_name, &list_member);
                let collection_ref = CompiledExpr::value_ref(
                    list_id,
                    Type::List(Box::new(member_type.clone())),
                );
                diagnostics.push(
                    Diagnostic::info(format!(
                        "dynamic collection index: {}[<expr>].{} — result depends on runtime list assembly",
                        name, member
                    ))
                );
                let compiled_idx = compile_expr_guarded(index, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
                return CompiledExpr::index_access(collection_ref, compiled_idx, member_type);
            }

            // Check if this is a collection sub member access: collection.count
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.collection_sub_names.contains(name.as_str())
                && member == "count"
            {
                // Resolve to the synthetic __count_ cell
                let count_member = format!("__count_{}", name);
                let count_id = ValueCellId::new(&scope.entity_name, &count_member);
                return CompiledExpr::value_ref(count_id, Type::Int);
            }

            // Check if this is a meta block access: meta.key
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && name == "meta"
            {
                if scope.meta_entries.is_empty() {
                    diagnostics.push(
                        Diagnostic::error("entity has no meta block".to_string())
                            .with_label(DiagnosticLabel::new(expr.span, "no meta block")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
                if scope.meta_entries.contains_key(member.as_str()) {
                    return CompiledExpr::meta_access(
                        scope.entity_name.clone(),
                        member.clone(),
                    );
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!("meta block has no key: {}", member))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown meta key")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
            }

            // For non-port member access, check if it's a known collection method
            let compiled_obj = compile_expr_guarded(object, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let collection_methods = ["count", "sum", "keys", "values"];
            if collection_methods.contains(&member.as_str()) {
                // Infer result type from method and object type
                let result_type = match member.as_str() {
                    "count" => Type::Int,
                    "sum" => match &compiled_obj.result_type {
                        Type::List(inner) => (**inner).clone(),
                        _ => Type::Real,
                    },
                    "keys" => match &compiled_obj.result_type {
                        Type::Map(k, _) => Type::List(k.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    "values" => match &compiled_obj.result_type {
                        Type::Map(_, v) => Type::List(v.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    _ => Type::Real,
                };
                CompiledExpr::method_call(compiled_obj, member.clone(), vec![], result_type)
            } else {
                diagnostics.push(
                    Diagnostic::error(format!("member access not yet supported: .{}", member))
                        .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
                );
                CompiledExpr::literal(Value::Undef, Type::Real)
            }
        }
        reify_syntax::ExprKind::ListLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| compile_expr_guarded(e, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter))
                .collect();
            // Infer element type from first element, default to Real for empty lists
            let elem_type = compiled_elems.first().map(|e| e.result_type.clone()).unwrap_or(Type::Real);
            let result_type = Type::List(Box::new(elem_type));
            CompiledExpr::list_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::SetLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| compile_expr_guarded(e, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter))
                .collect();
            let elem_type = compiled_elems.first().map(|e| e.result_type.clone()).unwrap_or(Type::Real);
            let result_type = Type::Set(Box::new(elem_type));
            CompiledExpr::set_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::MapLiteral(entries) => {
            let compiled_entries: Vec<(CompiledExpr, CompiledExpr)> = entries
                .iter()
                .map(|(k, v)| {
                    let ck = compile_expr_guarded(k, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
                    let cv = compile_expr_guarded(v, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
                    (ck, cv)
                })
                .collect();
            let key_type = compiled_entries.first().map(|(k, _)| k.result_type.clone()).unwrap_or(Type::String);
            let val_type = compiled_entries.first().map(|(_, v)| v.result_type.clone()).unwrap_or(Type::Real);
            let result_type = Type::Map(Box::new(key_type), Box::new(val_type));
            CompiledExpr::map_literal(compiled_entries, result_type)
        }
        reify_syntax::ExprKind::IndexAccess { object, index } => {
            let compiled_obj = compile_expr_guarded(object, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let compiled_idx = compile_expr_guarded(index, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            // Infer result type from collection's element type
            let result_type = match &compiled_obj.result_type {
                Type::List(inner) => (**inner).clone(),
                Type::Map(_, val) => (**val).clone(),
                _ => Type::Real,
            };
            CompiledExpr::index_access(compiled_obj, compiled_idx, result_type)
        }
        reify_syntax::ExprKind::EnumAccess { type_name, variant } => {
            // Look up the enum type in the registry
            if let Some(enum_def) = enum_defs.iter().find(|e| e.name == *type_name) {
                if enum_def.contains_variant(variant) {
                    CompiledExpr::literal(
                        Value::Enum {
                            type_name: type_name.clone(),
                            variant: variant.clone(),
                        },
                        Type::Enum(type_name.clone()),
                    )
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unknown variant '{}' on enum '{}'",
                            variant, type_name
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown variant")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Enum(type_name.clone()))
                }
            } else {
                diagnostics.push(
                    Diagnostic::error(format!("unknown enum type '{}'", type_name))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown enum")),
                );
                CompiledExpr::literal(Value::Undef, Type::Real)
            }
        }
        reify_syntax::ExprKind::Match { discriminant, arms } => {
            let compiled_discriminant = compile_expr_guarded(discriminant, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let compiled_arms: Vec<reify_types::CompiledMatchArm> = arms
                .iter()
                .map(|arm| {
                    let body = compile_expr_guarded(&arm.body, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
                    reify_types::CompiledMatchArm {
                        patterns: arm.patterns.clone(),
                        body,
                    }
                })
                .collect();

            // Result type from the first arm's body
            let result_type = compiled_arms
                .first()
                .map(|a| a.body.result_type.clone())
                .unwrap_or(Type::Real);

            // Exhaustiveness check: if discriminant is a known enum type,
            // verify all variants are covered by arm patterns or a wildcard.
            if let Type::Enum(ref enum_name) = compiled_discriminant.result_type
                && let Some(enum_def) = enum_defs.iter().find(|e| e.name == *enum_name)
            {
                let has_wildcard = compiled_arms
                    .iter()
                    .any(|arm| arm.patterns.iter().any(|p| p == "_"));

                if !has_wildcard {
                    let covered: std::collections::HashSet<&str> = compiled_arms
                        .iter()
                        .flat_map(|arm| arm.patterns.iter().map(|p| p.as_str()))
                        .collect();

                    let missing: Vec<&str> = enum_def
                        .variants
                        .iter()
                        .filter(|v| !covered.contains(v.as_str()))
                        .map(|v| v.as_str())
                        .collect();

                    if !missing.is_empty() {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "non-exhaustive match on '{}': missing variant(s) {}",
                                enum_name,
                                missing.join(", ")
                            ))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "missing variants",
                            )),
                        );
                    }
                }
            }

            // Content hash: tag [6] + discriminant + all arms
            let mut content_hash = ContentHash::of(&[6]).combine(compiled_discriminant.content_hash);
            for arm in &compiled_arms {
                for pattern in &arm.patterns {
                    content_hash = content_hash.combine(ContentHash::of_str(pattern));
                }
                content_hash = content_hash.combine(arm.body.content_hash);
            }

            CompiledExpr {
                kind: CompiledExprKind::Match {
                    discriminant: Box::new(compiled_discriminant),
                    arms: compiled_arms,
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Auto => {
            // Auto expressions should not appear inside compile_expr — they are
            // handled at the param compilation level. If we reach here, emit an
            // Undef literal as a safe fallback.
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let compiled_cond = compile_expr_guarded(condition, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let compiled_then = compile_expr_guarded(then_branch, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let compiled_else = compile_expr_guarded(else_branch, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);
            let result_type = compiled_then.result_type.clone();

            let content_hash = ContentHash::of(&[5])
                .combine(compiled_cond.content_hash)
                .combine(compiled_then.content_hash)
                .combine(compiled_else.content_hash);

            CompiledExpr {
                kind: CompiledExprKind::Conditional {
                    condition: Box::new(compiled_cond),
                    then_branch: Box::new(compiled_then),
                    else_branch: Box::new(compiled_else),
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Lambda { params, body } => {
            let lambda_entity = format!("$lambda{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            let mut lambda_scope = scope.clone();
            let mut compiled_params: Vec<(String, Option<Type>)> = Vec::new();
            let mut param_types: Vec<Type> = Vec::new();
            let mut param_ids: Vec<ValueCellId> = Vec::new();

            for param in params {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_name(&type_expr.name) {
                        Some(t) => t,
                        None => {
                            diagnostics.push(Diagnostic::error(format!(
                                "unresolved type in lambda param '{}': {}",
                                param.name, type_expr.name
                            )));
                            Type::Real // fallback
                        }
                    }
                } else {
                    Type::Real // default untyped params to Real
                };

                let param_id = ValueCellId::new(&lambda_entity, &param.name);
                lambda_scope
                    .names
                    .insert(param.name.clone(), (param_id.clone(), ty.clone(), None));

                param_ids.push(param_id);
                param_types.push(ty.clone());
                compiled_params.push((param.name.clone(), param.type_expr.as_ref().map(|_| ty)));
            }

            // Compile body in the nested scope
            let compiled_body =
                compile_expr_guarded(body, &lambda_scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);

            // Capture analysis: collect ValueRefs in body, filter out lambda params
            let lambda_param_set: HashSet<ValueCellId> = param_ids.iter().cloned().collect();
            let all_refs = collect_body_refs(&compiled_body);
            let mut seen = HashSet::new();
            let mut captures: Vec<ValueCellId> = Vec::new();
            for id in all_refs {
                if !lambda_param_set.contains(&id) && seen.insert(id.clone()) {
                    captures.push(id);
                }
            }

            let return_type = compiled_body.result_type.clone();
            let result_type = Type::Function {
                params: param_types,
                return_type: Box::new(return_type),
            };

            CompiledExpr::lambda(compiled_params, param_ids, compiled_body, captures, result_type)
        }
        reify_syntax::ExprKind::Quantifier { kind, variable, collection, predicate } => {
            let quant_entity = format!("$quant{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            // Compile collection in the outer scope
            let compiled_collection =
                compile_expr_guarded(collection, scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);

            // Create a nested scope with the bound variable
            let mut quant_scope = scope.clone();
            let variable_id = ValueCellId::new(&quant_entity, variable);
            // Infer element type from the collection's result type
            let elem_type = match &compiled_collection.result_type {
                Type::List(elem) | Type::Set(elem) => *elem.clone(),
                _ => Type::Real, // fallback for unresolved types
            };
            quant_scope
                .names
                .insert(variable.clone(), (variable_id.clone(), elem_type, None));

            // Compile predicate in the nested scope
            let compiled_predicate =
                compile_expr_guarded(predicate, &quant_scope, enum_defs, functions, diagnostics, current_guard, lambda_counter);

            let compiled_kind = match kind {
                reify_syntax::QuantifierKind::ForAll => reify_types::QuantifierKind::ForAll,
                reify_syntax::QuantifierKind::Exists => reify_types::QuantifierKind::Exists,
            };

            CompiledExpr::quantifier(
                compiled_kind,
                variable.clone(),
                variable_id,
                compiled_collection,
                compiled_predicate,
            )
        }
    }
}

/// Compile a single trait declaration into a CompiledTrait.
fn compile_trait(
    trait_decl: &reify_syntax::TraitDecl,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledTrait {
    let mut required_members = Vec::new();
    let mut defaults = Vec::new();

    for member in &trait_decl.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_name(&type_expr.name) {
                        Some(t) => t,
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unresolved type in trait '{}': {}",
                                    trait_decl.name, type_expr.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    type_expr.span,
                                    "unknown type name",
                                )),
                            );
                            Type::Real // fallback
                        }
                    }
                } else {
                    Type::Real
                };

                if param.default.is_some() {
                    // Param with default → trait default
                    defaults.push(TraitDefault {
                        name: Some(param.name.clone()),
                        kind: DefaultKind::Param {
                            cell_type: ty,
                            default_decl: param.clone(),
                        },
                        span: param.span,
                    });
                } else {
                    // Param without default → requirement
                    required_members.push(TraitRequirement {
                        name: param.name.clone(),
                        kind: RequirementKind::Param(ty),
                        span: param.span,
                    });
                }
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // Let bindings always have a value expression → default
                let ty = if let Some(type_expr) = &let_decl.type_expr {
                    match resolve_type_name(&type_expr.name) {
                        Some(t) => t,
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unresolved type in trait '{}': {}",
                                    trait_decl.name, type_expr.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    type_expr.span,
                                    "unknown type name",
                                )),
                            );
                            Type::Real
                        }
                    }
                } else {
                    Type::Real
                };
                let _ = ty; // type used for future type checking
                defaults.push(TraitDefault {
                    name: Some(let_decl.name.clone()),
                    kind: DefaultKind::Let(let_decl.clone()),
                    span: let_decl.span,
                });
            }
            reify_syntax::MemberDecl::Constraint(constraint_decl) => {
                if let Some(label) = &constraint_decl.label {
                    // Labeled constraint with expression in trait → default
                    // (override detection uses label matching at injection site)
                    defaults.push(TraitDefault {
                        name: Some(label.clone()),
                        kind: DefaultKind::Constraint(constraint_decl.clone()),
                        span: constraint_decl.span,
                    });
                } else {
                    // Unlabeled constraint → always injected as default
                    defaults.push(TraitDefault {
                        name: None,
                        kind: DefaultKind::Constraint(constraint_decl.clone()),
                        span: constraint_decl.span,
                    });
                }
            }
            reify_syntax::MemberDecl::Sub(sub_decl) => {
                required_members.push(TraitRequirement {
                    name: sub_decl.name.clone(),
                    kind: RequirementKind::Sub(sub_decl.structure_name.clone()),
                    span: sub_decl.span,
                });
            }
            _ => {
                // Minimize, Maximize, GuardedGroup, AssociatedType — skip for now
            }
        }
    }

    let content_hash = trait_decl.content_hash;

    // Convert parsed type parameters to compiled TypeParam structs
    let type_params = convert_type_params(&trait_decl.type_params);

    CompiledTrait {
        name: trait_decl.name.clone(),
        is_pub: trait_decl.is_pub,
        type_params,
        refinements: trait_decl.refinements.clone(),
        required_members,
        defaults,
        content_hash,
    }
}

/// Compile a parsed purpose declaration into a CompiledPurpose.
fn compile_purpose(
    purpose_def: &reify_syntax::PurposeDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    template_registry: &HashMap<String, &TopologyTemplate>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledPurpose {
    let purpose_name = &purpose_def.name;

    // Create a compilation scope for the purpose body.
    // Purpose params are registered so their members can be referenced.
    let mut scope = CompilationScope::new(purpose_name);

    // Register purpose params as identifiers in scope.
    // Each param binds an entity reference (e.g., `subject : Structure`).
    // Use StructureRef so member access resolves correctly against the entity type.
    for param in &purpose_def.params {
        scope.register(&param.name, Type::StructureRef(param.entity_kind.clone()));
    }

    let mut constraints = Vec::new();
    let mut constraint_index = 0u32;
    let mut objective = None;

    for member in &purpose_def.members {
        match member {
            reify_syntax::MemberDecl::Constraint(constraint) => {
                let compiled_expr =
                    compile_expr(&constraint.expr, &scope, enum_defs, functions, diagnostics);
                let id = ConstraintNodeId::new(purpose_name, constraint_index);
                constraints.push(CompiledConstraint {
                    id,
                    label: constraint.label.clone(),
                    expr: compiled_expr,
                    span: constraint.span,
                    domain: None,
                });
                constraint_index += 1;
            }
            reify_syntax::MemberDecl::Minimize(min_decl) => {
                let compiled_expr =
                    compile_expr(&min_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Minimize(compiled_expr));
            }
            reify_syntax::MemberDecl::Maximize(max_decl) => {
                let compiled_expr =
                    compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Maximize(compiled_expr));
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // Let bindings in purpose bodies are not yet supported:
                // CompiledPurpose has no storage for let expressions, and
                // activate_purpose only injects constraints. Any constraint
                // referencing a let-bound name would produce a ValueCellId
                // with no backing node in the eval graph.
                diagnostics.push(
                    Diagnostic::error(format!(
                        "let bindings in purpose bodies are not yet supported: '{}'",
                        let_decl.name
                    ))
                    .with_label(DiagnosticLabel::new(
                        let_decl.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                diagnostics.push(
                    Diagnostic::error(
                        "guarded blocks in purpose bodies are not yet supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        g.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::Param(p) => {
                diagnostics.push(
                    Diagnostic::error(
                        "param declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        p.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::Sub(s) => {
                diagnostics.push(
                    Diagnostic::error(
                        "sub declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        s.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::Port(p) => {
                diagnostics.push(
                    Diagnostic::error(
                        "port declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        p.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::Connect(c) => {
                diagnostics.push(
                    Diagnostic::error(
                        "connect declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        c.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::Chain(c) => {
                diagnostics.push(
                    Diagnostic::error(
                        "chain declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        c.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::AssociatedType(a) => {
                diagnostics.push(
                    Diagnostic::error(
                        "associated type declarations in purpose bodies are not supported"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        a.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_syntax::MemberDecl::MetaBlock(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "meta blocks in purpose bodies are not supported"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        m.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
        }
    }

    let params: Vec<CompiledPurposeParam> = purpose_def
        .params
        .iter()
        .map(|p| CompiledPurposeParam {
            name: p.name.clone(),
            entity_kind: p.entity_kind.clone(),
        })
        .collect();

    // Resolve reflective schema queries for each purpose param.
    // Look up the bound entity's TopologyTemplate and extract relevant ValueCellIds.
    let mut resolved_queries = Vec::new();
    for param in &params {
        if let Some(template) = template_registry.get(&param.entity_kind) {
            // Resolve "params" query: all Param and Auto value cells
            let param_ids: Vec<ValueCellId> = template
                .value_cells
                .iter()
                .filter(|vc| matches!(vc.kind, ValueCellKind::Param | ValueCellKind::Auto))
                .map(|vc| vc.id.clone())
                .collect();
            if !param_ids.is_empty() {
                resolved_queries.push(ResolvedSchemaQuery {
                    param_name: param.name.clone(),
                    query_kind: "params".to_string(),
                    resolved_ids: param_ids,
                });
            }
        }
    }

    CompiledPurpose {
        name: purpose_def.name.clone(),
        is_pub: purpose_def.is_pub,
        params,
        constraints,
        objective,
        resolved_queries,
        content_hash: purpose_def.content_hash,
    }
}

/// Compile a parsed module into a compiled module.
///
/// Performs name resolution, type checking, and expression compilation.
pub fn compile(
    parsed: &reify_syntax::ParsedModule,
) -> CompiledModule {
    let mut imports = Vec::new();
    let mut functions = Vec::new();
    let mut fields = Vec::new();
    let mut templates = Vec::new();
    let mut diagnostics = Vec::new();

    // Forward parse errors as diagnostics
    for err in &parsed.errors {
        diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }

    // Consolidated pre-pass: iterate declarations once, collecting references
    // for deferred compilation. This replaces 4 separate loops (enum, function,
    // trait, field) with a single match dispatch.
    let mut enum_defs: Vec<reify_types::EnumDef> = Vec::new();
    let mut fn_refs: Vec<&reify_syntax::FnDef> = Vec::new();
    let mut trait_refs: Vec<&reify_syntax::TraitDecl> = Vec::new();
    let mut field_refs: Vec<&reify_syntax::FieldDef> = Vec::new();
    // Unified entity namespace tracker (spec §4.2.1): structures, occurrences,
    // constraints, and fields all share the entity name space.
    // Maps name → (first_span, first_kind_label).
    let mut seen_entity_names: HashMap<String, (SourceSpan, &'static str)> = HashMap::new();

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Enum(e) => {
                enum_defs.push(reify_types::EnumDef {
                    name: e.name.clone(),
                    variants: e.variants.clone(),
                });
            }
            reify_syntax::Declaration::Function(fn_def) => {
                fn_refs.push(fn_def);
            }
            reify_syntax::Declaration::Trait(trait_decl) => {
                trait_refs.push(trait_decl);
            }
            reify_syntax::Declaration::Field(field_def) => {
                if let Some((first_span, first_kind)) = seen_entity_names.get(&field_def.name) {
                    // Duplicate entity name — emit error and skip
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            field_def.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            field_def.span,
                            "field defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    seen_entity_names.insert(field_def.name.clone(), (field_def.span, "field"));
                    field_refs.push(field_def);
                }
            }
            reify_syntax::Declaration::Structure(structure) => {
                if let Some((first_span, first_kind)) = seen_entity_names.get(&structure.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            structure.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            structure.span,
                            "structure defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    seen_entity_names.insert(structure.name.clone(), (structure.span, "structure"));
                }
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                if let Some((first_span, first_kind)) = seen_entity_names.get(&occurrence.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            occurrence.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            occurrence.span,
                            "occurrence defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    seen_entity_names.insert(occurrence.name.clone(), (occurrence.span, "occurrence"));
                }
            }
            reify_syntax::Declaration::Constraint(constraint) => {
                // Constraints reserve names in the entity namespace (spec §4.2.1)
                // even though constraint compilation is not yet implemented.
                if let Some((first_span, first_kind)) = seen_entity_names.get(&constraint.name) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            constraint.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            constraint.span,
                            "constraint defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    seen_entity_names.insert(constraint.name.clone(), (constraint.span, "constraint"));
                }
            }
            // Import, Purpose handled in pass 2 / purpose pass
            _ => {}
        }
    }

    // Compile in dependency order after collecting all references:
    // 1. Functions (need all enum_defs, plus prior compiled functions for self-reference)
    for fn_def in &fn_refs {
        if let Some(compiled_fn) = compile_function(fn_def, &enum_defs, &functions, &mut diagnostics)
        {
            functions.push(compiled_fn);
        }
    }

    // 2. Traits (independent — no deps on enums/functions)
    let mut trait_defs = Vec::new();
    for trait_decl in &trait_refs {
        let compiled_trait = compile_trait(trait_decl, &mut diagnostics);
        trait_defs.push(compiled_trait);
    }

    // Build trait registry for conformance checking.
    let trait_registry: HashMap<String, &CompiledTrait> = trait_defs
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();

    // 3. Fields (need all enum_defs + all compiled functions)
    for field_def in &field_refs {
        let compiled = compile_field(field_def, &enum_defs, &functions, &mut diagnostics);
        fields.push(compiled);
    }

    // Build a field registry so entity scopes can resolve field names.
    let field_registry: HashMap<String, &CompiledField> = fields
        .iter()
        .map(|f| (f.name.clone(), f))
        .collect();

    let mut pending_bound_checks: Vec<PendingBoundCheck> = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                // Only compile the first definition; duplicates have a different
                // span than the one recorded in seen_entity_names.
                let is_first_def = seen_entity_names
                    .get(&structure.name)
                    .is_none_or(|(first_span, _)| *first_span == structure.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(structure);
                    let template = compile_entity(&entity_ref, EntityKind::Structure, &enum_defs, &functions, &trait_registry, &field_registry, &mut pending_bound_checks, &mut diagnostics, &templates);
                    templates.push(template);
                }
            }
            reify_syntax::Declaration::Enum(_) => {
                // Already collected in pre-pass above.
            }
            reify_syntax::Declaration::Import(import) => {
                imports.push(CompiledImport {
                    path: import.path.clone(),
                    kind: import.kind.clone(),
                    is_pub: import.is_pub,
                    span: import.span,
                });
                diagnostics.push(
                    Diagnostic::warning(format!(
                        "import \"{}\" noted; module resolution not yet implemented",
                        import.path
                    ))
                    .with_label(DiagnosticLabel::new(import.span, "import")),
                );
            }
            reify_syntax::Declaration::Function(_) => {
                // Already compiled in pre-pass above.
            }
            reify_syntax::Declaration::Trait(_) => {
                // Already compiled in trait pre-pass above.
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                // Only compile the first definition; duplicates have a different
                // span than the one recorded in seen_entity_names.
                let is_first_def = seen_entity_names
                    .get(&occurrence.name)
                    .is_none_or(|(first_span, _)| *first_span == occurrence.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(occurrence);
                    let template = compile_entity(&entity_ref, EntityKind::Occurrence, &enum_defs, &functions, &trait_registry, &field_registry, &mut pending_bound_checks, &mut diagnostics, &templates);
                    templates.push(template);
                }
            }
            reify_syntax::Declaration::Field(_) => {
                // Already compiled in field pre-pass above.
            }
            reify_syntax::Declaration::Purpose(_) => {
                // Compiled in dedicated purpose pass below.
            }
            reify_syntax::Declaration::Constraint(_) => {
                // Constraint definitions: lowering/compilation not yet implemented.
            }
            reify_syntax::Declaration::Unit(_) => {
                // Unit declarations: compilation not yet implemented (task 208).
            }
        }
    }

    // Post-compilation pass: run deferred bound checks now that all structures
    // are compiled and available in the template registry.
    {
        let template_registry: HashMap<String, &TopologyTemplate> = templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

        for check in pending_bound_checks {
            match check {
                PendingBoundCheck::SubComponent { type_args, target_name, span } => {
                    // Resolve type_params from the template registry now that
                    // all structures are compiled.
                    let type_params = if let Some(target) = template_registry.get(target_name.as_str()) {
                        if target.type_params.is_empty() {
                            continue; // target has no type params, nothing to check
                        }
                        &target.type_params
                    } else {
                        // Target structure not found — skip (may be an external/unknown structure)
                        continue;
                    };

                    check_type_param_bounds(
                        type_params,
                        &type_args,
                        &target_name,
                        &template_registry,
                        &trait_registry,
                        &mut diagnostics,
                        span,
                    );
                }
                PendingBoundCheck::TraitConformance { type_params, type_args, target_name, span } => {
                    check_type_param_bounds(
                        &type_params,
                        &type_args,
                        &target_name,
                        &template_registry,
                        &trait_registry,
                        &mut diagnostics,
                        span,
                    );
                }
            }
        }
    }

    // Check for duplicate function signatures: same name + same param types
    {
        let mut seen: HashMap<(String, Vec<Type>), usize> = HashMap::new();
        for (idx, f) in functions.iter().enumerate() {
            let key = (
                f.name.clone(),
                f.params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            );
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(idx);
            } else {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "duplicate function signature: {}({})",
                        f.name,
                        f.params
                            .iter()
                            .map(|(_, t)| format!("{}", t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
            }
        }
    }

    // Post-compilation pass: check field composition type compatibility.
    // For composed fields, if the body references other fields, verify that
    // the codomain of the inner field matches the domain of the outer field.
    {
        let field_registry: HashMap<&str, &CompiledField> = fields
            .iter()
            .map(|f| (f.name.as_str(), f))
            .collect();

        for field in &fields {
            if let CompiledFieldSource::Composed { expr } = &field.source {
                check_field_composition_types(
                    expr,
                    &field_registry,
                    &mut diagnostics,
                );
            }
        }
    }

    // Purpose compilation pass: compile after templates so reflective schema queries
    // can resolve against TopologyTemplates.
    let compiled_purposes = {
        let purpose_template_registry: HashMap<String, &TopologyTemplate> = templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

        let mut purposes = Vec::new();
        for decl in &parsed.declarations {
            if let reify_syntax::Declaration::Purpose(purpose_def) = decl {
                let compiled = compile_purpose(
                    purpose_def,
                    &enum_defs,
                    &functions,
                    &purpose_template_registry,
                    &mut diagnostics,
                );
                purposes.push(compiled);
            }
        }
        purposes
    };

    // Build a content-sensitive hash by combining the path with all compiled content.
    let content_hash = {
        let path_hash = ContentHash::of_str(&format!("{}", parsed.path));

        // Template content hashes
        let template_hashes = templates.iter().map(|t| t.content_hash);

        // Import path hashes
        let import_hashes = imports.iter().map(|i| ContentHash::of_str(&i.path));

        // Enum def hashes
        let enum_hashes = enum_defs.iter().map(|e| {
            let mut h = ContentHash::of_str(&e.name);
            for v in &e.variants {
                h = h.combine(ContentHash::of_str(v));
            }
            h
        });

        // Function content hashes
        let function_hashes = functions.iter().map(|f: &CompiledFunction| f.content_hash);

        // Trait content hashes
        let trait_hashes = trait_defs.iter().map(|t| t.content_hash);

        // Field content hashes
        let field_hashes = fields.iter().map(|f| f.content_hash);

        // Purpose content hashes
        let purpose_hashes = compiled_purposes.iter().map(|p| p.content_hash);

        let all_hashes = std::iter::once(path_hash)
            .chain(template_hashes)
            .chain(import_hashes)
            .chain(enum_hashes)
            .chain(function_hashes)
            .chain(trait_hashes)
            .chain(field_hashes)
            .chain(purpose_hashes);

        ContentHash::combine_all(all_hashes)
    };

    CompiledModule {
        path: parsed.path.clone(),
        imports,
        enum_defs,
        functions,
        trait_defs,
        fields,
        compiled_purposes,
        templates,
        diagnostics,
        content_hash,
    }
}

/// Shared reference to entity definition fields (used by both StructureDef and OccurrenceDef).
struct EntityDefRef<'a> {
    name: &'a str,
    is_pub: bool,
    type_params: &'a [reify_syntax::TypeParamDecl],
    trait_bounds: &'a [reify_syntax::TraitBoundRef],
    members: &'a [reify_syntax::MemberDecl],
    span: SourceSpan,
    #[allow(dead_code)]
    content_hash: ContentHash,
}

impl<'a> From<&'a reify_syntax::StructureDef> for EntityDefRef<'a> {
    fn from(s: &'a reify_syntax::StructureDef) -> Self {
        EntityDefRef {
            name: &s.name,
            is_pub: s.is_pub,
            type_params: &s.type_params,
            trait_bounds: &s.trait_bounds,
            members: &s.members,
            span: s.span,
            content_hash: s.content_hash,
        }
    }
}

impl<'a> From<&'a reify_syntax::OccurrenceDef> for EntityDefRef<'a> {
    fn from(o: &'a reify_syntax::OccurrenceDef) -> Self {
        EntityDefRef {
            name: &o.name,
            is_pub: o.is_pub,
            type_params: &o.type_params,
            trait_bounds: &o.trait_bounds,
            members: &o.members,
            span: o.span,
            content_hash: o.content_hash,
        }
    }
}

/// Compile a single entity definition (structure or occurrence) into a topology template.
#[allow(clippy::too_many_arguments)]
fn compile_entity(
    structure: &EntityDefRef<'_>,
    entity_kind: EntityKind,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    field_registry: &HashMap<String, &CompiledField>,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    diagnostics: &mut Vec<Diagnostic>,
    compiled_templates: &[TopologyTemplate],
) -> TopologyTemplate {
    let entity_name = structure.name;
    let mut scope = CompilationScope::new(entity_name);
    let mut value_cells = Vec::new();
    let mut constraints = Vec::new();
    let mut sub_components: Vec<SubComponentDecl> = Vec::new();
    let mut ports: Vec<CompiledPort> = Vec::new();
    let mut port_names: HashMap<String, SourceSpan> = HashMap::new();
    let mut duplicate_port_names: HashSet<String> = HashSet::new();
    let mut guarded_groups: Vec<CompiledGuardedGroup> = Vec::new();
    let mut structure_controlling: HashSet<ValueCellId> = HashSet::new();
    let mut connections: Vec<CompiledConnection> = Vec::new();
    let mut objective: Option<OptimizationObjective> = None;
    let mut first_meta_span: Option<SourceSpan> = None;
    let mut constraint_index: u32 = 0;
    let mut guard_index: u32 = 0;
    let mut connector_index: u32 = 0;

    // Collect type parameter names for this structure so we can resolve
    // member types like `param contents : T` to Type::TypeParam("T").
    let type_param_names: HashSet<String> = structure
        .type_params
        .iter()
        .map(|tp| tp.name.clone())
        .collect();

    // Register field names into the scope so expressions can reference fields
    // (e.g., `sample(my_field, point)`). Fields use the FIELD_ENTITY_PREFIX.
    for (field_name, field) in field_registry {
        let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, field_name);
        let field_type = Type::Field {
            domain: Box::new(field.domain_type.clone()),
            codomain: Box::new(field.codomain_type.clone()),
        };
        scope.names.insert(field_name.clone(), (field_id, field_type, None));
    }

    // First pass: register all param and let names into the scope so they can
    // reference each other (forward references within the structure).
    // We need types for the scope, so we resolve types in this pass as well.
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_with_params(&type_expr.name, &type_param_names) {
                        Some(t) => t,
                        None => {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unresolved type: {}",
                                    type_expr.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    type_expr.span,
                                    "unknown type name",
                                )),
                            );
                            Type::Real // fallback
                        }
                    }
                } else {
                    // Infer type from default expression if available
                    Type::Real
                };
                scope.register(&param.name, ty);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // For lets, we need to infer the type from the expression.
                // Geometry lets produce realizations (not value cells) but still
                // need to be registered in scope so subsequent lets can reference them.
                if is_geometry_let(&let_decl.value) {
                    scope.register(&let_decl.name, Type::Geometry);
                } else {
                    // We'll register with a placeholder type; the actual type will
                    // be determined when we compile the expression. For now, use Real.
                    // We'll update this after the expression is compiled.
                    scope.register(&let_decl.name, Type::Real);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                register_guarded_names(&g.members, &mut scope, diagnostics);
                register_guarded_names(&g.else_members, &mut scope, diagnostics);
            }
            reify_syntax::MemberDecl::Port(port_decl) => {
                if let Some(first_span) = port_names.get(&port_decl.name) {
                    // Duplicate port name — emit error and skip registration
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate port name '{}'",
                            port_decl.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            port_decl.span,
                            "duplicate defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            "first defined here",
                        )),
                    );
                    duplicate_port_names.insert(port_decl.name.clone());
                    continue;
                }
                port_names.insert(port_decl.name.clone(), port_decl.span);
                scope.port_names.insert(port_decl.name.clone());
                // Register port body members with composite names: port_name.member_name
                for port_member in &port_decl.members {
                    match port_member {
                        reify_syntax::MemberDecl::Param(param) => {
                            let composite_name = format!("{}.{}", port_decl.name, param.name);
                            let ty = if let Some(type_expr) = &param.type_expr {
                                resolve_type_name(&type_expr.name).unwrap_or(Type::Real)
                            } else {
                                Type::Real
                            };
                            let id = ValueCellId::new(entity_name, &composite_name);
                            scope.names.insert(composite_name, (id, ty, None));
                        }
                        reify_syntax::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            scope.names.insert(composite_name, (id, Type::Real, None));
                        }
                        _ => {}
                    }
                }
            }
            reify_syntax::MemberDecl::Sub(sub) => {
                if sub.is_collection {
                    scope.collection_sub_names.insert(sub.name.clone());
                    // Populate member types from already-compiled child template
                    if let Some(child_tmpl) = compiled_templates.iter().find(|t| t.name == sub.structure_name) {
                        let member_types: HashMap<String, Type> = child_tmpl
                            .value_cells
                            .iter()
                            .map(|vc| (vc.id.member.clone(), vc.cell_type.clone()))
                            .collect();
                        scope.collection_sub_member_types.insert(sub.name.clone(), member_types);
                    }
                }
            }
            reify_syntax::MemberDecl::MetaBlock(meta) => {
                if let Some(first_span) = first_meta_span {
                    diagnostics.push(
                        Diagnostic::error("duplicate meta block".to_string())
                            .with_label(DiagnosticLabel::new(meta.span, "duplicate defined here"))
                            .with_label(DiagnosticLabel::new(first_span, "first defined here")),
                    );
                } else {
                    first_meta_span = Some(meta.span);
                    for (key, value) in &meta.entries {
                        scope.meta_entries.insert(key.clone(), value.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // Trait conformance checking: verify structure satisfies all trait bounds.
    if !structure.trait_bounds.is_empty() {
        check_trait_conformance(
            structure,
            trait_registry,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            diagnostics,
        );

        // Defer type argument checking on parameterized trait bounds (e.g., Container<Bolt>)
        // to the post-compilation pass so forward references are resolved correctly.
        for trait_bound in structure.trait_bounds {
            if !trait_bound.type_args.is_empty()
                && let Some(compiled_trait) = trait_registry.get(&trait_bound.name)
                && !compiled_trait.type_params.is_empty()
            {
                let resolved_args: Vec<Type> = trait_bound
                    .type_args
                    .iter()
                    .map(|ta| {
                        resolve_type_name(&ta.name).unwrap_or_else(|| {
                            if type_param_names.contains(&ta.name) {
                                Type::TypeParam(ta.name.clone())
                            } else {
                                Type::StructureRef(ta.name.clone())
                            }
                        })
                    })
                    .collect();
                // TraitConformance: type_params are known now from the compiled
                // trait, so they're carried directly in the enum variant.
                pending_bound_checks.push(PendingBoundCheck::TraitConformance {
                    type_params: compiled_trait.type_params.clone(),
                    type_args: resolved_args,
                    target_name: trait_bound.name.clone(),
                    span: trait_bound.span,
                });
            }
        }
    }

    // Second pass: compile all members.
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or(Type::Real);

                // Check if the default is ExprKind::Auto
                let is_auto = matches!(
                    param.default.as_ref(),
                    Some(reify_syntax::Expr { kind: reify_syntax::ExprKind::Auto, .. })
                );

                let decl = if is_auto {
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Auto,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr: None,
                        span: param.span,
                    }
                } else {
                    let default_expr = param
                        .default
                        .as_ref()
                        .map(|expr| compile_expr(expr, &scope, enum_defs, functions, diagnostics));

                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr,
                        span: param.span,
                    }
                };

                if let Some(wc) = &param.where_clause {
                    compile_per_decl_guard(
                        entity_name,
                        wc,
                        decl,
                        &mut scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        &mut guarded_groups,
                        &mut structure_controlling,
                        &mut guard_index,
                    );
                } else {
                    value_cells.push(decl);
                }
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // Skip geometry-producing function calls
                if is_geometry_let(&let_decl.value) {
                    continue;
                }

                let compiled_expr = compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                // Update the scope with the inferred type
                scope.register(&let_decl.name, cell_type.clone());

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                let decl = ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    span: let_decl.span,
                };

                if let Some(wc) = &let_decl.where_clause {
                    compile_per_decl_guard(
                        entity_name,
                        wc,
                        decl,
                        &mut scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        &mut guarded_groups,
                        &mut structure_controlling,
                        &mut guard_index,
                    );
                } else {
                    value_cells.push(decl);
                }
            }
            reify_syntax::MemberDecl::Constraint(constraint) => {
                // Detect collection count constraint pattern:
                //   `collection_name.count == expr`  or  `expr == collection_name.count`
                if let Some((coll_name, count_expr)) = extract_count_constraint(&constraint.expr, &scope.collection_sub_names) {
                    let compiled_rhs = compile_expr(count_expr, &scope, enum_defs, functions, diagnostics);
                    let count_member = format!("__count_{}", coll_name);
                    let count_id = ValueCellId::new(entity_name, &count_member);
                    value_cells.push(ValueCellDecl {
                        id: count_id.clone(),
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        cell_type: Type::Int,
                        default_expr: Some(compiled_rhs),
                        span: constraint.span,
                    });
                    structure_controlling.insert(count_id.clone());
                    // Store count_cell on the matching SubComponentDecl
                    if let Some(sub) = sub_components.iter_mut().find(|s| s.name == coll_name) {
                        sub.count_cell = Some(count_id);
                    }
                } else {
                    let compiled_expr = compile_expr(&constraint.expr, &scope, enum_defs, functions, diagnostics);

                    // Check that the constraint expression produces Bool
                    if compiled_expr.result_type != Type::Bool {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "constraint expression has type {}, expected Bool",
                                compiled_expr.result_type,
                            ))
                            .with_label(DiagnosticLabel::new(
                                constraint.expr.span,
                                "expected Bool",
                            )),
                        );
                    }

                    let id = ConstraintNodeId::new(entity_name, constraint_index);
                    let cc = CompiledConstraint {
                        id,
                        label: constraint.label.clone(),
                        expr: compiled_expr,
                        span: constraint.span,
                        domain: None,
                    };
                    constraint_index += 1;

                    if let Some(wc) = &constraint.where_clause {
                        compile_per_decl_constraint_guard(
                            entity_name,
                            wc,
                            cc,
                            &mut scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            &mut guarded_groups,
                            &mut structure_controlling,
                            &mut guard_index,
                        );
                    } else {
                        constraints.push(cc);
                    }
                }
            }
            reify_syntax::MemberDecl::Sub(sub) => {
                let compiled_args: Vec<(String, CompiledExpr)> = sub
                    .args
                    .iter()
                    .map(|(name, expr)| {
                        (name.clone(), compile_expr(expr, &scope, enum_defs, functions, diagnostics))
                    })
                    .collect();

                // Resolve type arguments to Type values.
                let resolved_type_args: Vec<Type> = sub
                    .type_args
                    .iter()
                    .map(|ta| {
                        resolve_type_name(&ta.name).unwrap_or_else(|| {
                            if type_param_names.contains(&ta.name) {
                                Type::TypeParam(ta.name.clone())
                            } else {
                                Type::StructureRef(ta.name.clone())
                            }
                        })
                    })
                    .collect();

                // SubComponent: defer bound checking to the post-compilation
                // pass so forward-referenced structures are available in the
                // registry. type_params are resolved from the target template
                // during the post-pass. Always push — even with empty
                // type_args, the target may have type params requiring defaults.
                {
                    pending_bound_checks.push(PendingBoundCheck::SubComponent {
                        type_args: resolved_type_args.clone(),
                        target_name: sub.structure_name.clone(),
                        span: sub.span,
                    });
                }

                sub_components.push(SubComponentDecl {
                    name: sub.name.clone(),
                    structure_name: sub.structure_name.clone(),
                    visibility: Visibility::Public,
                    args: compiled_args,
                    type_args: resolved_type_args,
                    is_collection: sub.is_collection,
                    count_cell: None,
                    span: sub.span,
                    content_hash: sub.content_hash,
                });
            }
            reify_syntax::MemberDecl::Minimize(min_decl) => {
                let compiled_expr = compile_expr(&min_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Minimize(compiled_expr));
            }
            reify_syntax::MemberDecl::Maximize(max_decl) => {
                let compiled_expr = compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Maximize(compiled_expr));
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                compile_block_guard(
                    entity_name,
                    g,
                    None, // no outer guard
                    &mut scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    &mut guarded_groups,
                    &mut structure_controlling,
                    &mut guard_index,
                    &mut constraint_index,
                );
            }
            reify_syntax::MemberDecl::AssociatedType(_) => {
                // Associated type compilation deferred to a later milestone.
            }
            reify_syntax::MemberDecl::Port(port_decl) => {
                // Skip duplicate port names (already reported in first pass).
                // The first occurrence is compiled; subsequent duplicates are skipped.
                if duplicate_port_names.contains(&port_decl.name)
                    && !port_names.get(&port_decl.name).is_some_and(|&span| span == port_decl.span)
                {
                    continue;
                }
                let direction = port_decl.direction.unwrap_or(reify_types::PortDirection::Bidi);

                // Verify port type_name exists in the trait registry
                if !trait_registry.contains_key(&port_decl.type_name) {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unknown port type '{}' — no trait with this name found in current module",
                            port_decl.type_name
                        ))
                        .with_label(DiagnosticLabel::new(
                            port_decl.span,
                            "unknown port type",
                        )),
                    );
                }

                let mut port_members = Vec::new();
                let mut port_constraints = Vec::new();

                for port_member in &port_decl.members {
                    match port_member {
                        reify_syntax::MemberDecl::Param(param) => {
                            let composite_name = format!("{}.{}", port_decl.name, param.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            let cell_type = scope
                                .resolve(&composite_name)
                                .map(|(_, ty)| ty.clone())
                                .unwrap_or(Type::Real);

                            let is_auto = matches!(
                                param.default.as_ref(),
                                Some(reify_syntax::Expr { kind: reify_syntax::ExprKind::Auto, .. })
                            );

                            let decl = if is_auto {
                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Auto,
                                    visibility: Visibility::Public,
                                    cell_type,
                                    default_expr: None,
                                    span: param.span,
                                }
                            } else {
                                let default_expr = param
                                    .default
                                    .as_ref()
                                    .map(|expr| compile_expr(expr, &scope, enum_defs, functions, diagnostics));

                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Param,
                                    visibility: Visibility::Public,
                                    cell_type,
                                    default_expr,
                                    span: param.span,
                                }
                            };
                            port_members.push(decl);
                        }
                        reify_syntax::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let compiled_expr = compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
                            let cell_type = compiled_expr.result_type.clone();
                            let id = ValueCellId::new(entity_name, &composite_name);

                            scope.names.insert(composite_name, (id.clone(), cell_type.clone(), None));

                            let visibility = if let_decl.is_pub {
                                Visibility::Public
                            } else {
                                Visibility::Private
                            };

                            port_members.push(ValueCellDecl {
                                id,
                                kind: ValueCellKind::Let,
                                visibility,
                                cell_type,
                                default_expr: Some(compiled_expr),
                                span: let_decl.span,
                            });
                        }
                        reify_syntax::MemberDecl::Constraint(constraint) => {
                            let compiled_expr = compile_expr(&constraint.expr, &scope, enum_defs, functions, diagnostics);
                            let id = ConstraintNodeId::new(entity_name, constraint_index);
                            port_constraints.push(CompiledConstraint {
                                id,
                                label: constraint.label.clone(),
                                expr: compiled_expr,
                                span: constraint.span,
                                domain: None,
                            });
                            constraint_index += 1;
                        }
                        _ => {}
                    }
                }

                let frame_expr = port_decl.frame_expr.as_ref().map(|expr| {
                    compile_expr(expr, &scope, enum_defs, functions, diagnostics)
                });

                ports.push(CompiledPort {
                    name: port_decl.name.clone(),
                    direction,
                    type_name: port_decl.type_name.clone(),
                    members: port_members,
                    constraints: port_constraints,
                    frame_expr,
                });
            }
            reify_syntax::MemberDecl::Connect(connect_decl) => {
                let ctx = ConnectContext {
                    entity_name,
                    ports: &ports,
                    scope: &scope,
                    enum_defs,
                    functions,
                };
                let mut acc = ConnectAccumulator {
                    constraints: &mut constraints,
                    constraint_index: &mut constraint_index,
                    connections: &mut connections,
                    sub_components: &mut sub_components,
                    connector_index: &mut connector_index,
                };
                compile_connection(
                    &ctx,
                    &ConnectInput {
                        left_expr: &connect_decl.left.expr,
                        operator: connect_decl.operator,
                        right_expr: &connect_decl.right.expr,
                        connector_type: connect_decl.connector_type.as_deref(),
                        params: &connect_decl.params,
                        port_mappings: &connect_decl.port_mappings,
                        span: connect_decl.span,
                    },
                    diagnostics,
                    &mut acc,
                );
            }
            reify_syntax::MemberDecl::Chain(chain_decl) => {
                if chain_decl.elements.len() < 2 {
                    diagnostics.push(Diagnostic::error(
                        "chain statement requires at least two elements",
                    ).with_label(DiagnosticLabel::new(chain_decl.span, "too few elements")));
                }
                let ctx = ConnectContext {
                    entity_name,
                    ports: &ports,
                    scope: &scope,
                    enum_defs,
                    functions,
                };
                // Desugar chain into pairwise Forward connections
                for pair in chain_decl.elements.windows(2) {
                    let mut acc = ConnectAccumulator {
                        constraints: &mut constraints,
                        constraint_index: &mut constraint_index,
                        connections: &mut connections,
                        sub_components: &mut sub_components,
                        connector_index: &mut connector_index,
                    };
                    compile_connection(
                        &ctx,
                        &ConnectInput {
                            left_expr: &pair[0],
                            operator: reify_syntax::ConnectOp::Forward,
                            right_expr: &pair[1],
                            connector_type: None,
                            params: &[],
                            port_mappings: &[],
                            span: chain_decl.span,
                        },
                        diagnostics,
                        &mut acc,
                    );
                }
            }
            reify_syntax::MemberDecl::MetaBlock(_) => {
                // Meta blocks are collected in the first pass; skip in second pass.
            }
        }
    }

    // Third pass: compile geometry let bindings into realizations.
    let mut realizations = Vec::new();
    let mut realization_index: u32 = 0;

    for member in structure.members {
        if let reify_syntax::MemberDecl::Let(let_decl) = member
            && is_geometry_let(&let_decl.value)
            && let Some(ops) = compile_geometry_call(&let_decl.value, &scope, enum_defs, functions, diagnostics, 0)
        {
            realizations.push(RealizationDecl {
                id: RealizationNodeId::new(entity_name, realization_index),
                operations: ops,
                span: SourceSpan::new(0, 0),
            });
            realization_index += 1;
        }
    }

    // Build a content-sensitive hash by combining the name with all compiled content.
    let content_hash = {
        let name_hash = ContentHash::of_str(entity_name);

        // Value cell default expression hashes (sentinel ContentHash(0) for None)
        let vc_hashes = value_cells.iter().map(|vc| {
            vc.default_expr
                .as_ref()
                .map(|e| e.content_hash)
                .unwrap_or(ContentHash(0))
        });

        // Constraint expression hashes
        let constraint_hashes = constraints.iter().map(|c| c.expr.content_hash);

        // Sub-component content hashes
        let sub_hashes = sub_components.iter().map(|s| s.content_hash);

        // Guarded group hashes: include guard_expr + all member/constraint/else content
        let guard_hashes = guarded_groups.iter().flat_map(|g| {
            std::iter::once(g.guard_expr.content_hash)
                .chain(g.members.iter().map(|m| {
                    m.default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0))
                }))
                .chain(g.constraints.iter().map(|c| c.expr.content_hash))
                .chain(g.else_members.iter().map(|m| {
                    m.default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0))
                }))
                .chain(g.else_constraints.iter().map(|c| c.expr.content_hash))
        });

        // Port member hashes (including identity fields for incremental invalidation)
        let port_hashes = ports.iter().flat_map(|p| {
            // Port identity fields: name, direction, type_name
            std::iter::once(ContentHash::of_str(&p.name))
            .chain(std::iter::once(ContentHash::of(&[p.direction as u8])))
            .chain(std::iter::once(ContentHash::of_str(&p.type_name)))
            // Port member default_expr hashes
            .chain(p.members.iter().map(|m| {
                m.default_expr
                    .as_ref()
                    .map(|e| e.content_hash)
                    .unwrap_or(ContentHash(0))
            }))
            .chain(p.constraints.iter().map(|c| c.expr.content_hash))
            // Frame expression hash
            .chain(std::iter::once(
                p.frame_expr.as_ref().map(|e| e.content_hash).unwrap_or(ContentHash(0))
            ))
        });

        // Connection identity hashes: left_port, operator, right_port, port_mappings, connector_sub
        let connection_hashes = connections.iter().flat_map(|c| {
            std::iter::once(ContentHash::of_str(&c.left_port))
                .chain(std::iter::once(ContentHash::of(&[c.operator.as_u8()])))
                .chain(std::iter::once(ContentHash::of_str(&c.right_port)))
                .chain(
                    c.port_mappings
                        .iter()
                        .flat_map(|(l, r)| [ContentHash::of_str(l), ContentHash::of_str(r)]),
                )
                .chain(std::iter::once(
                    c.connector_sub
                        .as_ref()
                        .map(|s| ContentHash::of_str(s))
                        .unwrap_or(ContentHash(0)),
                ))
        });

        let all_hashes = std::iter::once(name_hash)
            .chain(vc_hashes)
            .chain(constraint_hashes)
            .chain(sub_hashes)
            .chain(guard_hashes)
            .chain(port_hashes)
            .chain(connection_hashes);

        ContentHash::combine_all(all_hashes)
    };

    let visibility = if structure.is_pub {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Reference safety: detect unguarded references to guarded members.
    {
        let mut guarded_cell_map: HashMap<ValueCellId, ValueCellId> = HashMap::new();
        for group in &guarded_groups {
            for m in &group.members {
                guarded_cell_map.insert(m.id.clone(), group.guard_value_cell.clone());
            }
            for m in &group.else_members {
                guarded_cell_map.insert(m.id.clone(), group.guard_value_cell.clone());
            }
        }

        // Build parent_guard chain for nested guard ancestor checking.
        // Maps guard_value_cell -> parent_guard (None for top-level guards).
        let guard_parent_map: HashMap<ValueCellId, Option<ValueCellId>> = guarded_groups
            .iter()
            .map(|g| (g.guard_value_cell.clone(), g.parent_guard.clone()))
            .collect();

        // Check if ref_guard is an ancestor of current_guard in the parent chain.
        // Returns true if ref_guard == current_guard OR if ref_guard appears
        // in the ancestor chain of current_guard (via parent_guard links).
        let is_ancestor_guard = |ref_guard: &ValueCellId, current_guard: &ValueCellId| -> bool {
            if ref_guard == current_guard {
                return true;
            }
            let mut cursor = guard_parent_map.get(current_guard).and_then(|p| p.as_ref());
            while let Some(ancestor) = cursor {
                if ancestor == ref_guard {
                    return true;
                }
                cursor = guard_parent_map.get(ancestor).and_then(|p| p.as_ref());
            }
            false
        };

        for vc in &value_cells {
            if let Some(expr) = &vc.default_expr {
                for ref_id in expr.collect_value_refs() {
                    if guarded_cell_map.contains_key(&ref_id) {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "unguarded reference to guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                vc.span,
                                "references a conditionally-active member",
                            )),
                        );
                    }
                }
            }
        }
        for c in &constraints {
            for ref_id in c.expr.collect_value_refs() {
                if guarded_cell_map.contains_key(&ref_id) {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unguarded reference to guarded cell '{}'",
                            ref_id.member,
                        ))
                        .with_label(DiagnosticLabel::new(
                            c.span,
                            "constraint references a conditionally-active member",
                        )),
                    );
                }
            }
        }
        for group in &guarded_groups {
            for m in &group.members {
                if let Some(expr) = &m.default_expr {
                    for ref_id in expr.collect_value_refs() {
                        if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                            && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                        {
                            diagnostics.push(
                                Diagnostic::warning(format!(
                                    "reference to differently-guarded cell '{}'",
                                    ref_id.member,
                                ))
                                .with_label(DiagnosticLabel::new(
                                    m.span,
                                    "referenced member under a different guard",
                                )),
                            );
                        }
                    }
                }
            }
            for m in &group.else_members {
                if let Some(expr) = &m.default_expr {
                    for ref_id in expr.collect_value_refs() {
                        if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                            && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                        {
                            diagnostics.push(
                                Diagnostic::warning(format!(
                                    "reference to differently-guarded cell '{}'",
                                    ref_id.member,
                                ))
                                .with_label(DiagnosticLabel::new(
                                    m.span,
                                    "referenced member under a different guard",
                                )),
                            );
                        }
                    }
                }
            }
            for c in &group.constraints {
                for ref_id in c.expr.collect_value_refs() {
                    if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                        && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                    {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "reference to differently-guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                c.span,
                                "constraint references member under a different guard",
                            )),
                        );
                    }
                }
            }
            for c in &group.else_constraints {
                for ref_id in c.expr.collect_value_refs() {
                    if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                        && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                    {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "reference to differently-guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                c.span,
                                "constraint references member under a different guard",
                            )),
                        );
                    }
                }
            }
        }
    }

    // Reconciliation sweep: backfill count_cell for collection sub-components
    // whose count constraint was processed before the sub declaration.
    // Match __count_{name} cells in value_cells to sub_components where count_cell is None.
    for vc in &value_cells {
        if let Some(coll_name) = vc.id.member.strip_prefix("__count_")
            && let Some(sub) = sub_components.iter_mut().find(|s| s.name == coll_name && s.count_cell.is_none())
        {
            sub.count_cell = Some(vc.id.clone());
        }
    }

    // Convert parsed type parameters to compiled TypeParam structs
    let type_params = convert_type_params(structure.type_params);

    let trait_bounds: Vec<String> = structure
        .trait_bounds
        .iter()
        .map(|tb| tb.name.clone())
        .collect();

    // Port direction validation for occurrences: warn if missing in/out ports.
    if entity_kind == EntityKind::Occurrence {
        let has_in = ports.iter().any(|p| p.direction == reify_types::PortDirection::In);
        let has_out = ports.iter().any(|p| p.direction == reify_types::PortDirection::Out);
        if !has_in {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "occurrence '{}' has no input port; occurrences typically consume input structures",
                    entity_name
                ))
                .with_label(DiagnosticLabel::new(structure.span, "occurrence defined here")),
            );
        }
        if !has_out {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "occurrence '{}' has no output port; occurrences typically produce output structures",
                    entity_name
                ))
                .with_label(DiagnosticLabel::new(structure.span, "occurrence defined here")),
            );
        }
    }

    TopologyTemplate {
        name: entity_name.to_string(),
        entity_kind,
        visibility,
        type_params,
        trait_bounds,
        value_cells,
        constraints,
        realizations,
        sub_components,
        ports,
        connections,
        guarded_groups,
        structure_controlling,
        objective,
        meta: scope.meta_entries.clone(),
        content_hash,
    }
}

/// A deferred bound check to be executed after all structures are compiled.
/// This ensures forward references are resolved correctly.
///
/// Two distinct paths produce pending bound checks:
/// - **SubComponent**: a `sub x = Foo<Bar>()` instantiation where type_params
///   are not yet known (resolved from the template registry in the post-pass).
/// - **TraitConformance**: a `structure def X : Trait<Arg>` declaration where
///   type_params are already known from the compiled trait definition.
enum PendingBoundCheck {
    /// Deferred check for a sub-component instantiation of a generic structure.
    /// The type_params are resolved from the template registry during the
    /// post-compilation pass, since the target structure may not yet be compiled.
    SubComponent {
        type_args: Vec<Type>,
        target_name: String,
        span: SourceSpan,
    },
    /// Deferred check for trait conformance with type arguments.
    /// The type_params are known at construction time from the compiled trait.
    TraitConformance {
        type_params: Vec<reify_types::TypeParam>,
        type_args: Vec<Type>,
        target_name: String,
        span: SourceSpan,
    },
}

/// Check that type arguments satisfy the bounds on type parameters.
///
/// For each type param with bounds, verify that the corresponding type arg
/// declares conformance to all required traits. Forwarded type params
/// (Type::TypeParam) are skipped — their bounds are enforced at the concrete
/// instantiation site.
/// When type_args are fewer than type_params, fill in defaults from TypeParam.default.
/// If a type_param has no default and no arg is provided, emit an error.
/// If type_args exceed type_params, emit an arity error.
fn check_type_param_bounds(
    type_params: &[reify_types::TypeParam],
    type_args: &[Type],
    target_structure_name: &str,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    diagnostics: &mut Vec<Diagnostic>,
    span: SourceSpan,
) {
    // Check arity: too many type args
    if type_args.len() > type_params.len() {
        diagnostics.push(
            Diagnostic::error(format!(
                "too many type arguments for '{}': expected {}, got {}",
                target_structure_name, type_params.len(), type_args.len()
            ))
            .with_label(DiagnosticLabel::new(
                span,
                format!("'{}' declares {} type parameter(s)", target_structure_name, type_params.len()),
            )),
        );
    }

    for (i, tp) in type_params.iter().enumerate() {
        let effective_arg: &Type = if let Some(arg) = type_args.get(i) {
            arg
        } else if let Some(ref default_type) = tp.default {
            default_type
        } else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "missing type argument for type parameter '{}' of '{}' (no default provided)",
                    tp.name, target_structure_name
                ))
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("'{}' requires a type argument for '{}'", target_structure_name, tp.name),
                )),
            );
            continue;
        };

        // Skip bound checking for forwarded type params — bounds are
        // enforced at the concrete instantiation site.
        if matches!(effective_arg, Type::TypeParam(_)) {
            continue;
        }

        let arg_name = match effective_arg.as_name() {
            Some(name) => name,
            None => continue, // builtin types don't need bound checking
        };

        let arg_template = template_registry.get(arg_name);

        for bound in &tp.bounds {
            let bound_name = &bound.trait_ref.name;
            let satisfies = if let Some(tmpl) = arg_template {
                satisfies_trait_bound(&tmpl.trait_bounds, bound_name, trait_registry)
            } else {
                false
            };

            if !satisfies {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type argument '{}' does not satisfy bound '{}' on type parameter '{}' of '{}'",
                        arg_name, bound_name, tp.name, target_structure_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!("'{}' does not implement '{}'", arg_name, bound_name),
                    )),
                );
            }
        }
    }
}

/// Check whether a structure's declared trait bounds satisfy a required trait,
/// walking refinement chains transitively.
///
/// Returns true if any of the `structure_trait_bounds` equals `required_trait`
/// or refines it (directly or transitively) through the `trait_registry`.
fn satisfies_trait_bound(
    structure_trait_bounds: &[String],
    required_trait: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
) -> bool {
    for bound in structure_trait_bounds {
        let mut visited = HashSet::new();
        if trait_satisfies(bound, required_trait, trait_registry, &mut visited) {
            return true;
        }
    }
    false
}

/// Recursively check if `trait_name` equals or refines `required_trait`.
fn trait_satisfies(
    trait_name: &str,
    required_trait: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    visited: &mut HashSet<String>,
) -> bool {
    if trait_name == required_trait {
        return true;
    }
    if !visited.insert(trait_name.to_string()) {
        return false; // cycle detected
    }
    if let Some(compiled_trait) = trait_registry.get(trait_name) {
        for refinement in &compiled_trait.refinements {
            if trait_satisfies(refinement, required_trait, trait_registry, visited) {
                return true;
            }
        }
    }
    false
}

/// Extract a port name from a port reference expression.
/// Returns `None` for unsupported expression kinds (complex expressions).
fn resolve_port_name(expr: &reify_syntax::Expr) -> Option<String> {
    match &expr.kind {
        reify_syntax::ExprKind::Ident(name) => Some(name.clone()),
        reify_syntax::ExprKind::MemberAccess { object, member } => {
            match &object.kind {
                reify_syntax::ExprKind::Ident(obj_name) => Some(format!("{}.{}", obj_name, member)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Auto-match port members between two bare port names when no explicit port_mappings given.
///
/// Conditions for auto-matching:
/// 1. Both port names must be bare (no dot), and both must exist in `ports`.
/// 2. Both ports must share the same `type_name` (same trait).
/// 3. All Param/Auto members on both sides must match by name (all-or-nothing).
///
/// Returns:
/// - Identity mappings `[(name, name), ...]` sorted alphabetically when all members match.
/// - Empty vec when ports are dotted, unknown, have different traits, or have unmatched members.
///   In the unmatched case a Warning diagnostic is emitted.
fn auto_match_port_members(
    left_port: &str,
    right_port: &str,
    ports: &[CompiledPort],
    diagnostics: &mut Vec<Diagnostic>,
    span: SourceSpan,
) -> Vec<(String, String)> {
    use std::collections::BTreeSet;

    // Only auto-match bare (non-dotted) port names
    if left_port.contains('.') || right_port.contains('.') {
        return Vec::new();
    }

    // Look up both ports; skip if either is not found (undefined port error already emitted)
    let left_compiled = match ports.iter().find(|p| p.name == left_port) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let right_compiled = match ports.iter().find(|p| p.name == right_port) {
        Some(p) => p,
        None => return Vec::new(),
    };

    // Only auto-match when both ports implement the same trait
    if left_compiled.type_name != right_compiled.type_name {
        return Vec::new();
    }

    // Extract raw member names (strip "{port_name}." prefix) for Param/Auto members only
    let extract_members = |port: &CompiledPort| -> BTreeSet<String> {
        let prefix = format!("{}.", port.name);
        port.members
            .iter()
            .filter(|m| matches!(m.kind, ValueCellKind::Param | ValueCellKind::Auto))
            .filter_map(|m| m.id.member.strip_prefix(&prefix).map(|s| s.to_string()))
            .collect()
    };

    let left_names = extract_members(left_compiled);
    let right_names = extract_members(right_compiled);

    if left_names != right_names {
        // Collect unmatched names from each side
        let only_left: Vec<_> = left_names.difference(&right_names).cloned().collect();
        let only_right: Vec<_> = right_names.difference(&left_names).cloned().collect();

        let mut msg = format!(
            "port members do not match between '{}' and '{}' (same trait '{}'); \
             consider using explicit mapping {{ left_member -> right_member }}",
            left_port, right_port, left_compiled.type_name
        );
        if !only_left.is_empty() {
            msg.push_str(&format!("; unmatched on left: {}", only_left.join(", ")));
        }
        if !only_right.is_empty() {
            msg.push_str(&format!("; unmatched on right: {}", only_right.join(", ")));
        }

        diagnostics.push(
            Diagnostic::warning(msg)
                .with_label(DiagnosticLabel::new(span, "unmatched port members")),
        );
        return Vec::new();
    }

    // All members match — produce sorted identity mappings
    left_names.into_iter().map(|name| (name.clone(), name)).collect()
}

/// Check if a source port direction is forward-compatible with a destination port direction.
fn is_forward_compatible(source: reify_types::PortDirection, dest: reify_types::PortDirection) -> bool {
    use reify_types::PortDirection::*;
    matches!(
        (source, dest),
        (Out, In) | (Out, Bidi) | (Bidi, In) | (Bidi, Bidi) | (Bidi, Out) | (In, Bidi)
    )
}

/// Accumulated outputs from connection compilation.
struct ConnectAccumulator<'a> {
    constraints: &'a mut Vec<CompiledConstraint>,
    constraint_index: &'a mut u32,
    connections: &'a mut Vec<CompiledConnection>,
    sub_components: &'a mut Vec<SubComponentDecl>,
    connector_index: &'a mut u32,
}

/// Read-only context for compiling connections.
struct ConnectContext<'a> {
    entity_name: &'a str,
    ports: &'a [CompiledPort],
    scope: &'a CompilationScope,
    enum_defs: &'a [reify_types::EnumDef],
    functions: &'a [CompiledFunction],
}

/// Per-statement inputs for compiling a single connection.
struct ConnectInput<'a> {
    left_expr: &'a reify_syntax::Expr,
    operator: reify_syntax::ConnectOp,
    right_expr: &'a reify_syntax::Expr,
    connector_type: Option<&'a str>,
    params: &'a [(String, reify_syntax::Expr)],
    port_mappings: &'a [(String, String)],
    span: SourceSpan,
}

/// Compile a single connection (from connect statement or chain desugaring).
fn compile_connection(
    ctx: &ConnectContext,
    input: &ConnectInput,
    diagnostics: &mut Vec<Diagnostic>,
    acc: &mut ConnectAccumulator,
) {
    let left_expr = input.left_expr;
    let right_expr = input.right_expr;
    let operator = input.operator;
    let span = input.span;
    let connector_type = input.connector_type;
    let params = input.params;
    let port_mappings = input.port_mappings;
    let left_port = match resolve_port_name(left_expr) {
        Some(name) => name,
        None => {
            diagnostics.push(
                Diagnostic::error("invalid port reference in connect statement")
                    .with_label(DiagnosticLabel::new(left_expr.span, "unsupported expression")),
            );
            return;
        }
    };
    let right_port = match resolve_port_name(right_expr) {
        Some(name) => name,
        None => {
            diagnostics.push(
                Diagnostic::error("invalid port reference in connect statement")
                    .with_label(DiagnosticLabel::new(right_expr.span, "unsupported expression")),
            );
            return;
        }
    };

    // Look up port directions for compatibility checking
    let dir_of = |name: &str| ctx.ports.iter().find(|p| p.name == name).map(|p| p.direction);
    let left_dir = dir_of(&left_port);
    let right_dir = dir_of(&right_port);

    // Bare ident (no dot) that doesn't match any port is undefined
    let is_bare = |name: &str| !name.contains('.');
    if is_bare(&left_port) && left_dir.is_none() {
        diagnostics.push(
            Diagnostic::error(format!("undefined port '{}' in connect statement", left_port))
                .with_label(DiagnosticLabel::new(span, "undefined port")),
        );
    }
    if is_bare(&right_port) && right_dir.is_none() {
        diagnostics.push(
            Diagnostic::error(format!("undefined port '{}' in connect statement", right_port))
                .with_label(DiagnosticLabel::new(span, "undefined port")),
        );
    }

    // Direction compatibility check
    let compatible = match operator {
        reify_syntax::ConnectOp::Forward => {
            match (left_dir, right_dir) {
                (Some(l), Some(r)) => {
                    if is_forward_compatible(l, r) {
                        true
                    } else {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "incompatible port directions for connect: {:?} -> {:?}",
                                l, r
                            ))
                            .with_label(DiagnosticLabel::new(span, "incompatible directions")),
                        );
                        false
                    }
                }
                _ => true, // Can't check unknown/dotted ports
            }
        }
        reify_syntax::ConnectOp::Reverse => {
            match (left_dir, right_dir) {
                (Some(l), Some(r)) => {
                    if is_forward_compatible(r, l) {
                        true
                    } else {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "incompatible port directions for connect: {:?} <- {:?}",
                                l, r
                            ))
                            .with_label(DiagnosticLabel::new(span, "incompatible directions")),
                        );
                        false
                    }
                }
                _ => true,
            }
        }
        reify_syntax::ConnectOp::Bidirectional => {
            match (left_dir, right_dir) {
                (Some(l), Some(r)) => {
                    if l == reify_types::PortDirection::Bidi && r == reify_types::PortDirection::Bidi {
                        true
                    } else {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "bidirectional connect requires both ports to be bidi, got {:?} <-> {:?}",
                                l, r
                            ))
                            .with_label(DiagnosticLabel::new(span, "both ports must be bidi")),
                        );
                        false
                    }
                }
                _ => true,
            }
        }
    };

    // Create compatibility constraint
    let compat_id = ConstraintNodeId::new(ctx.entity_name, *acc.constraint_index);
    let compat_expr = CompiledExpr::literal(
        Value::Bool(compatible),
        Type::Bool,
    );
    acc.constraints.push(CompiledConstraint {
        id: compat_id.clone(),
        label: Some(format!("connect_compat_{}_{}", left_port, right_port)),
        expr: compat_expr,
        domain: None,
        span,
    });
    *acc.constraint_index += 1;

    // Handle connector sub-entity
    let connector_sub = if let Some(conn_type) = connector_type {
        let connector_name = format!("__connector_{}", *acc.connector_index);
        *acc.connector_index += 1;

        let compiled_args: Vec<(String, CompiledExpr)> = params
            .iter()
            .map(|(name, expr)| {
                (name.clone(), compile_expr(expr, ctx.scope, ctx.enum_defs, ctx.functions, diagnostics))
            })
            .collect();

        let mut conn_hash = ContentHash::of_str(conn_type)
            .combine(ContentHash::of(&[operator.as_u8()]))
            .combine(ContentHash::of_str(&left_port))
            .combine(ContentHash::of_str(&right_port));
        for (_, expr) in &compiled_args {
            conn_hash = conn_hash.combine(expr.content_hash);
        }

        acc.sub_components.push(SubComponentDecl {
            name: connector_name.clone(),
            structure_name: conn_type.to_string(),
            visibility: Visibility::Private,
            args: compiled_args,
            type_args: vec![],
            is_collection: false,
            count_cell: None,
            span,
            content_hash: conn_hash,
        });

        Some(connector_name)
    } else {
        None
    };

    // Determine effective port mappings: explicit takes priority; otherwise auto-match.
    let effective_mappings = if port_mappings.is_empty() {
        auto_match_port_members(&left_port, &right_port, ctx.ports, diagnostics, span)
    } else {
        port_mappings.to_vec()
    };

    acc.connections.push(CompiledConnection {
        left_port,
        operator,
        right_port,
        connector_sub,
        compatibility_constraint: compat_id,
        port_mappings: effective_mappings,
        frame_constraint: None,
        span,
    });
}

/// Collect all ValueCellId references from a compiled expression tree,
/// recursing into lambda bodies. Used during capture analysis before
/// captures are populated.
fn collect_body_refs(expr: &CompiledExpr) -> Vec<ValueCellId> {
    let mut refs = Vec::new();
    collect_body_refs_inner(expr, &mut refs);
    refs
}

fn collect_body_refs_inner(expr: &CompiledExpr, refs: &mut Vec<ValueCellId>) {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => refs.push(id.clone()),
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_body_refs_inner(left, refs);
            collect_body_refs_inner(right, refs);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_body_refs_inner(operand, refs);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::Conditional { condition, then_branch, else_branch } => {
            collect_body_refs_inner(condition, refs);
            collect_body_refs_inner(then_branch, refs);
            collect_body_refs_inner(else_branch, refs);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            collect_body_refs_inner(discriminant, refs);
            for arm in arms {
                collect_body_refs_inner(&arm.body, refs);
            }
        }
        CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            collect_body_refs_inner(body, refs);
        }
        CompiledExprKind::Quantifier { variable_id, collection, predicate, .. } => {
            collect_body_refs_inner(collection, refs);
            // Filter out the quantifier's bound variable from predicate refs,
            // mirroring collect_value_refs_inner in reify-types/src/expr.rs.
            let mut pred_refs = Vec::new();
            collect_body_refs_inner(predicate, &mut pred_refs);
            for r in pred_refs {
                if r != *variable_id {
                    refs.push(r);
                }
            }
        }
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ListLiteral(elements) => {
            for elem in elements {
                collect_body_refs_inner(elem, refs);
            }
        }
        CompiledExprKind::SetLiteral(elements) => {
            for elem in elements {
                collect_body_refs_inner(elem, refs);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                collect_body_refs_inner(key, refs);
                collect_body_refs_inner(val, refs);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            collect_body_refs_inner(object, refs);
            collect_body_refs_inner(index, refs);
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            collect_body_refs_inner(object, refs);
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::OptionSome(inner) => {
            collect_body_refs_inner(inner, refs);
        }
        CompiledExprKind::OptionNone => {}
        CompiledExprKind::MetaAccess { .. } => {}
    }
}

/// Register names from guarded group members in the compilation scope (pass 1).
/// Recursively handles nested guarded groups.
fn register_guarded_names(
    members: &[reify_syntax::MemberDecl],
    scope: &mut CompilationScope,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for member in members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    resolve_type_name(&type_expr.name).unwrap_or_else(|| {
                        diagnostics.push(
                            Diagnostic::error(format!("unresolved type: {}", type_expr.name))
                                .with_label(DiagnosticLabel::new(type_expr.span, "unknown type name")),
                        );
                        Type::Real
                    })
                } else {
                    Type::Real
                };
                scope.register(&param.name, ty);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value) {
                    scope.register(&let_decl.name, Type::Geometry);
                } else {
                    scope.register(&let_decl.name, Type::Real);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                register_guarded_names(&g.members, scope, diagnostics);
                register_guarded_names(&g.else_members, scope, diagnostics);
            }
            _ => {}
        }
    }
}

/// Compile a block-level `where` guard into a CompiledGuardedGroup.
///
/// Creates a synthetic guard ValueCell and compiles all members within the block.
/// If `outer_guard` is Some, the guard expression becomes AND(outer_guard, inner_condition).
#[allow(clippy::too_many_arguments)]
fn compile_block_guard(
    entity_name: &str,
    g: &reify_syntax::GuardedGroupDecl,
    outer_guard: Option<&ValueCellId>,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    constraint_index: &mut u32,
) {
    let inner_condition = compile_expr(&g.condition, scope, enum_defs, functions, diagnostics);

    // If there's an outer guard, conjoin: guard = outer && inner
    let guard_expr = if let Some(outer_id) = outer_guard {
        let outer_ref = CompiledExpr::value_ref(outer_id.clone(), Type::Bool);
        CompiledExpr::binop(BinOp::And, outer_ref, inner_condition, Type::Bool)
    } else {
        inner_condition
    };

    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;
    structure_controlling.insert(guard_cell_id.clone());

    let mut members = Vec::new();
    let mut group_constraints = Vec::new();

    // Compile main members
    compile_guarded_members(
        entity_name,
        &g.members,
        &guard_cell_id,
        scope,
        enum_defs,
        functions,
        diagnostics,
        &mut members,
        &mut group_constraints,
        guarded_groups,
        structure_controlling,
        guard_index,
        constraint_index,
    );

    let mut else_members = Vec::new();
    let mut else_constraints = Vec::new();

    // Compile else members
    if !g.else_members.is_empty() {
        compile_guarded_members(
            entity_name,
            &g.else_members,
            &guard_cell_id,
            scope,
            enum_defs,
            functions,
            diagnostics,
            &mut else_members,
            &mut else_constraints,
            guarded_groups,
            structure_controlling,
            guard_index,
            constraint_index,
        );
    }

    // Update scope to mark all members and else_members as guarded
    for m in &members {
        scope.register_guarded(&m.id.member, m.cell_type.clone(), guard_cell_id.clone());
    }
    for m in &else_members {
        scope.register_guarded(&m.id.member, m.cell_type.clone(), guard_cell_id.clone());
    }

    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id,
        members,
        constraints: group_constraints,
        else_members,
        else_constraints,
        parent_guard: outer_guard.cloned(),
    });
}

/// Compile members within a guarded block into ValueCellDecls and CompiledConstraints.
/// Handles nested GuardedGroupDecls recursively.
#[allow(clippy::too_many_arguments)]
fn compile_guarded_members(
    entity_name: &str,
    ast_members: &[reify_syntax::MemberDecl],
    current_guard: &ValueCellId,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    members: &mut Vec<ValueCellDecl>,
    group_constraints: &mut Vec<CompiledConstraint>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    constraint_index: &mut u32,
) {
    let guard_ctx = Some(current_guard);
    for member in ast_members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or(Type::Real);

                let is_auto = matches!(
                    param.default.as_ref(),
                    Some(reify_syntax::Expr { kind: reify_syntax::ExprKind::Auto, .. })
                );

                let decl = if is_auto {
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Auto,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr: None,
                        span: param.span,
                    }
                } else {
                    let default_expr = param
                        .default
                        .as_ref()
                        .map(|expr| { let mut lc = 0u32; compile_expr_guarded(expr, scope, enum_defs, functions, diagnostics, guard_ctx, &mut lc) });
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr,
                        span: param.span,
                    }
                };
                members.push(decl);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value) {
                    continue;
                }
                let compiled_expr = { let mut lc = 0u32; compile_expr_guarded(&let_decl.value, scope, enum_defs, functions, diagnostics, guard_ctx, &mut lc) };
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                members.push(ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    span: let_decl.span,
                });
            }
            reify_syntax::MemberDecl::Constraint(constraint) => {
                let compiled_expr = { let mut lc = 0u32; compile_expr_guarded(&constraint.expr, scope, enum_defs, functions, diagnostics, guard_ctx, &mut lc) };
                if compiled_expr.result_type != Type::Bool {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "constraint expression has type {}, expected Bool",
                            compiled_expr.result_type,
                        ))
                        .with_label(DiagnosticLabel::new(
                            constraint.expr.span,
                            "expected Bool",
                        )),
                    );
                }
                let id = ConstraintNodeId::new(entity_name, *constraint_index);
                group_constraints.push(CompiledConstraint {
                    id,
                    label: constraint.label.clone(),
                    expr: compiled_expr,
                    span: constraint.span,
                    domain: None,
                });
                *constraint_index += 1;
            }
            reify_syntax::MemberDecl::GuardedGroup(nested) => {
                // Nested guard: compile with current guard as outer
                compile_block_guard(
                    entity_name,
                    nested,
                    Some(current_guard),
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    guarded_groups,
                    structure_controlling,
                    guard_index,
                    constraint_index,
                );
            }
            _ => {
                // Sub, Minimize, Maximize within guarded blocks: not yet handled
            }
        }
    }
}

/// Compile a per-declaration `where` clause into a single-member CompiledGuardedGroup.
///
/// Creates a synthetic guard ValueCell (Bool, Let kind) with the guard condition as
/// its default expression, and wraps the member in a CompiledGuardedGroup.
#[allow(clippy::too_many_arguments)]
fn compile_per_decl_guard(
    entity_name: &str,
    wc: &reify_syntax::WhereClause,
    member_decl: ValueCellDecl,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
) {
    let guard_expr = compile_expr(&wc.condition, scope, enum_defs, functions, diagnostics);
    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;

    // Update scope to mark this member as guarded (for reference safety checking)
    let member_name = member_decl.id.member.clone();
    let member_type = member_decl.cell_type.clone();

    structure_controlling.insert(guard_cell_id.clone());
    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id.clone(),
        members: vec![member_decl],
        constraints: vec![],
        else_members: vec![],
        else_constraints: vec![],
        parent_guard: None,
    });

    scope.register_guarded(&member_name, member_type, guard_cell_id);
}

/// Compile a per-declaration `where` clause for a constraint into a single-constraint
/// CompiledGuardedGroup.
#[allow(clippy::too_many_arguments)]
fn compile_per_decl_constraint_guard(
    entity_name: &str,
    wc: &reify_syntax::WhereClause,
    constraint: CompiledConstraint,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
) {
    let guard_expr = compile_expr(&wc.condition, scope, enum_defs, functions, diagnostics);
    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;

    structure_controlling.insert(guard_cell_id.clone());
    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id,
        members: vec![],
        constraints: vec![constraint],
        else_members: vec![],
        else_constraints: vec![],
        parent_guard: None,
    });
}

/// Check trait conformance for a structure.
///
/// Resolves each trait bound, collects all requirements (including from
/// refinement chains), and verifies the structure satisfies them.
/// Injects trait defaults for members not overridden by the structure.
#[allow(clippy::too_many_arguments)]
fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Collect all structure member names for conformance checking.
    let structure_members: HashMap<String, Type> = structure
        .members
        .iter()
        .filter_map(|m| match m {
            reify_syntax::MemberDecl::Param(p) => {
                let ty = p
                    .type_expr
                    .as_ref()
                    .and_then(|te| resolve_type_name(&te.name))
                    .unwrap_or(Type::Real);
                Some((p.name.clone(), ty))
            }
            reify_syntax::MemberDecl::Let(l) => {
                let ty = l
                    .type_expr
                    .as_ref()
                    .and_then(|te| resolve_type_name(&te.name))
                    .unwrap_or(Type::Real);
                Some((l.name.clone(), ty))
            }
            _ => None,
        })
        .collect();

    // Collect structure constraint labels.
    let structure_constraint_labels: HashSet<String> = structure
        .members
        .iter()
        .filter_map(|m| {
            if let reify_syntax::MemberDecl::Constraint(c) = m {
                c.label.clone()
            } else {
                None
            }
        })
        .collect();

    // Collect all requirements and defaults from all trait bounds,
    // handling refinement chains and deduplication.
    let mut all_requirements: Vec<TraitRequirement> = Vec::new();
    let mut all_defaults: Vec<TraitDefault> = Vec::new();
    let mut visited_traits: HashSet<String> = HashSet::new();
    let mut seen_requirement_names: HashMap<String, Type> = HashMap::new();
    let mut seen_default_names: HashMap<String, Type> = HashMap::new();
    // Tracks sub_name -> structure_name for Sub requirement dedup and conflict detection.
    let mut seen_sub_names: HashMap<String, String> = HashMap::new();

    for trait_bound in structure.trait_bounds {
        collect_all_requirements(
            &trait_bound.name,
            trait_registry,
            &mut all_requirements,
            &mut all_defaults,
            &mut visited_traits,
            &mut seen_requirement_names,
            &mut seen_default_names,
            &mut seen_sub_names,
            &structure_members,
            structure.span,
            diagnostics,
            0,
        );
    }

    // Check each requirement against structure members.
    for req in &all_requirements {
        match &req.kind {
            RequirementKind::Param(expected_type) | RequirementKind::Let(expected_type) => {
                match structure_members.get(&req.name) {
                    Some(actual_type) => {
                        if !implicitly_converts_to(actual_type, expected_type) {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "type mismatch for trait member '{}': expected {}, got {}",
                                    req.name, expected_type, actual_type
                                ))
                                .with_label(DiagnosticLabel::new(
                                    structure.span,
                                    "type mismatch",
                                )),
                            );
                        }
                    }
                    None => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "missing required member '{}' (expected type: {})",
                                req.name, expected_type
                            ))
                            .with_label(DiagnosticLabel::new(
                                structure.span,
                                "required by trait",
                            )),
                        );
                    }
                }
            }
            RequirementKind::Sub(structure_name) => {
                let has_sub = structure.members.iter().any(|m| {
                    if let reify_syntax::MemberDecl::Sub(s) = m {
                        s.name == req.name && s.structure_name == *structure_name
                    } else {
                        false
                    }
                });
                if !has_sub {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "missing required sub-component '{}' of type '{}'",
                            req.name, structure_name
                        ))
                        .with_label(DiagnosticLabel::new(
                            structure.span,
                            "required by trait",
                        )),
                    );
                }
            }
        }
    }

    // Pre-register default member names in scope so their expressions can
    // reference each other (e.g., constraint x > 0 references param x from same trait).
    for default in &all_defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
        {
            let ty = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let(_) => Type::Real,
                DefaultKind::Constraint(_) => continue,
            };
            scope.register(name, ty);
        }
    }

    // Inject defaults for members not overridden by the structure.
    for default in &all_defaults {
        match &default.kind {
            DefaultKind::Param { cell_type, default_decl } => {
                let name = default.name.as_deref().expect("DefaultKind::Param always has Some(name)");
                if !structure_members.contains_key(name) {
                    // Inject default param into value_cells
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    let default_expr = default_decl.default.as_ref().map(|expr| {
                        compile_expr(expr, scope, enum_defs, functions, diagnostics)
                    });

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Private,
                        cell_type: cell_type.clone(),
                        default_expr,
                        span: default.span,
                    });
                }
            }
            DefaultKind::Let(let_decl) => {
                let name = default.name.as_deref().expect("DefaultKind::Let always has Some(name)");
                if !structure_members.contains_key(name) {
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    let compiled_expr = compile_expr(
                        &let_decl.value,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                    );

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        cell_type: compiled_expr.result_type.clone(),
                        default_expr: Some(compiled_expr),
                        span: default.span,
                    });
                }
            }
            DefaultKind::Constraint(constraint_decl) => {
                let label = constraint_decl.label.as_deref();
                let already_has = label.is_some_and(|l| structure_constraint_labels.contains(l));
                if !already_has {
                    let compiled_expr = compile_expr(
                        &constraint_decl.expr,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                    );

                    let constraint_id = ConstraintNodeId {
                        entity: structure.name.to_string(),
                        index: *constraint_index,
                    };
                    *constraint_index += 1;

                    constraints.push(CompiledConstraint {
                        id: constraint_id,
                        label: constraint_decl.label.clone(),
                        expr: compiled_expr,
                        span: default.span,
                        domain: None,
                    });
                }
            }
        }
    }
}

/// Maximum allowed depth for trait refinement chains to prevent stack overflow.
/// Realistic hierarchies rarely exceed 10 levels; 128 provides ample headroom.
const MAX_TRAIT_DEPTH: usize = 128;

/// Recursively collect all requirements and defaults from a trait and its refinements.
#[allow(clippy::too_many_arguments)]
fn collect_all_requirements(
    trait_name: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    requirements: &mut Vec<TraitRequirement>,
    defaults: &mut Vec<TraitDefault>,
    visited: &mut HashSet<String>,
    seen_names: &mut HashMap<String, Type>,
    seen_defaults: &mut HashMap<String, Type>,
    seen_sub_names: &mut HashMap<String, String>,
    structure_members: &HashMap<String, Type>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    if depth > MAX_TRAIT_DEPTH {
        diagnostics.push(
            Diagnostic::error(format!(
                "trait refinement chain too deep (exceeded {} levels) at '{}'",
                MAX_TRAIT_DEPTH, trait_name
            ))
            .with_label(DiagnosticLabel::new(span, "trait chain too deep")),
        );
        return;
    }

    if !visited.insert(trait_name.to_string()) {
        return; // Already visited (diamond pattern)
    }

    let Some(compiled_trait) = trait_registry.get(trait_name) else {
        diagnostics.push(
            Diagnostic::error(format!(
                "unresolved trait: '{}'",
                trait_name
            ))
            .with_label(DiagnosticLabel::new(span, "unknown trait")),
        );
        return;
    };

    // Walk refinement chain first (parents before children)
    for refinement in &compiled_trait.refinements {
        collect_all_requirements(
            refinement,
            trait_registry,
            requirements,
            defaults,
            visited,
            seen_names,
            seen_defaults,
            seen_sub_names,
            structure_members,
            span,
            diagnostics,
            depth + 1,
        );
    }

    // Collect requirements from this trait, checking for conflicts.
    for req in &compiled_trait.required_members {
        let expected_type = match &req.kind {
            RequirementKind::Param(ty) | RequirementKind::Let(ty) => Some(ty.clone()),
            _ => None,
        };

        if let Some(expected_type) = &expected_type {
            if let Some(existing_type) = seen_names.get(&req.name) {
                if existing_type != expected_type {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "conflicting trait requirements for '{}': {} vs {}",
                            req.name, existing_type, expected_type
                        ))
                        .with_label(DiagnosticLabel::new(span, "conflicting traits")),
                    );
                }
                continue; // Deduplicated
            }
            seen_names.insert(req.name.clone(), expected_type.clone());
        }

        // Deduplicate Sub requirements by name; detect conflicting Sub requirements
        // (same name, different structure types from different traits).
        if let RequirementKind::Sub(structure_name) = &req.kind {
            if let Some(existing_structure) = seen_sub_names.get(&req.name) {
                if existing_structure == structure_name {
                    continue; // Exact duplicate — already collected
                } else {
                    // Conflict: same sub-component name required as two different types.
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "conflicting sub-component requirements for '{}': {} vs {}",
                            req.name, existing_structure, structure_name
                        ))
                        .with_label(DiagnosticLabel::new(span, "conflicting traits")),
                    );
                    continue; // Can't satisfy both; skip this requirement
                }
            }
            seen_sub_names.insert(req.name.clone(), structure_name.clone());
        }

        requirements.push(req.clone());
    }

    // Collect defaults from this trait, deduplicating by name.
    for default in &compiled_trait.defaults {
        if default.name.is_none() {
            // Unnamed defaults (e.g., unlabeled constraints) — always push.
            defaults.push(default.clone());
        } else if let Some(name) = &default.name {
            // Extract type for dedup comparison.
            let default_type = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let(_) => Type::Real,
                DefaultKind::Constraint(_) => Type::Bool, // sentinel for constraint label dedup
            };

            if let Some(existing_type) = seen_defaults.get(name.as_str()) {
                if existing_type != &default_type
                    && !structure_members.contains_key(name.as_str())
                {
                    // Same name + different type + not overridden → conflict
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "conflicting trait defaults for '{}': {} vs {}",
                            name, existing_type, default_type
                        ))
                        .with_label(DiagnosticLabel::new(span, "conflicting trait defaults")),
                    );
                }
                // Same name already seen → skip (deduplicate).
                continue;
            }
            seen_defaults.insert(name.clone(), default_type);
            defaults.push(default.clone());
        }
    }
}

/// Compile a function definition into a CompiledFunction.
fn compile_function(
    fn_def: &reify_syntax::FnDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    // Resolve parameter types
    let mut params = Vec::new();
    for p in &fn_def.params {
        let ty = match resolve_type_name(&p.type_expr.name) {
            Some(t) => t,
            None => {
                diagnostics.push(
                    Diagnostic::error(format!("unresolved type: {}", p.type_expr.name))
                        .with_label(DiagnosticLabel::new(p.type_expr.span, "unknown type name")),
                );
                Type::Real // fallback
            }
        };
        params.push((p.name.clone(), ty));
    }

    // Resolve return type
    let return_type = match &fn_def.return_type {
        Some(te) => match resolve_type_name(&te.name) {
            Some(t) => t,
            None => {
                diagnostics.push(
                    Diagnostic::error(format!("unresolved return type: {}", te.name))
                        .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                );
                Type::Real
            }
        },
        None => Type::Real, // default return type
    };

    // Create a scope with function params registered
    let mut scope = CompilationScope::new(&fn_def.name);
    for (name, ty) in &params {
        scope.register(name, ty.clone());
    }

    // Compile body let bindings
    let mut compiled_lets = Vec::new();
    for let_decl in &fn_def.body.let_bindings {
        let compiled_expr = compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
        let let_type = compiled_expr.result_type.clone();
        // Register the let binding in scope for subsequent bindings
        scope.register(&let_decl.name, let_type);
        compiled_lets.push((let_decl.name.clone(), compiled_expr));
    }

    // Compile result expression
    let result_expr = compile_expr(&fn_def.body.result_expr, &scope, enum_defs, functions, diagnostics);

    // Compute content hash
    let content_hash = {
        let name_hash = ContentHash::of_str(&fn_def.name);
        let param_hashes = params.iter().map(|(n, t)| {
            ContentHash::of_str(n).combine(ContentHash::of_str(&format!("{}", t)))
        });
        let body_hash = result_expr.content_hash;
        let let_hashes = compiled_lets.iter().map(|(_, e)| e.content_hash);

        let all_hashes = std::iter::once(name_hash)
            .chain(param_hashes)
            .chain(std::iter::once(body_hash))
            .chain(let_hashes);

        ContentHash::combine_all(all_hashes)
    };

    Some(CompiledFunction {
        name: fn_def.name.clone(),
        is_pub: fn_def.is_pub,
        params,
        return_type,
        body: CompiledFnBody {
            let_bindings: compiled_lets,
            result_expr,
        },
        content_hash,
    })
}

/// Resolve a type name in field context. Unlike resolve_type_name, unresolved
/// names become StructureRef (geometric domain types like Point3, Vector3)
/// but a diagnostic warning is emitted so the user knows the type was not
/// resolved from the built-in set.
fn resolve_field_type_name(
    name: &str,
    span: reify_types::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    resolve_type_name(name).unwrap_or_else(|| {
        diagnostics.push(
            Diagnostic::warning(format!("unresolved field type '{}', treating as structure reference", name))
                .with_label(DiagnosticLabel::new(span, "unknown type name")),
        );
        Type::StructureRef(name.to_string())
    })
}

/// Compile a field declaration into a CompiledField.
fn compile_field(
    field_def: &reify_syntax::FieldDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledField {
    let domain_type = resolve_field_type_name(&field_def.domain_type.name, field_def.domain_type.span, diagnostics);
    let codomain_type = resolve_field_type_name(&field_def.codomain_type.name, field_def.codomain_type.span, diagnostics);

    // Create a scope for compiling field source expressions
    let scope = CompilationScope::new(&field_def.name);

    let source = match &field_def.source {
        reify_syntax::FieldSource::Analytical { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            CompiledFieldSource::Analytical { expr: compiled_expr }
        }
        reify_syntax::FieldSource::Sampled { config } => {
            let compiled_config: Vec<(String, CompiledExpr)> = config
                .iter()
                .map(|(key, val_expr)| {
                    // In sampled config, bare identifiers are treated as string
                    // constants (e.g., `interpolation = linear` -> "linear").
                    let compiled = if let reify_syntax::ExprKind::Ident(name) = &val_expr.kind {
                        if scope.resolve(name).is_none() {
                            CompiledExpr::literal(Value::String(name.clone()), Type::String)
                        } else {
                            compile_expr(val_expr, &scope, enum_defs, functions, diagnostics)
                        }
                    } else {
                        compile_expr(val_expr, &scope, enum_defs, functions, diagnostics)
                    };
                    (key.clone(), compiled)
                })
                .collect();
            CompiledFieldSource::Sampled { config: compiled_config }
        }
        reify_syntax::FieldSource::Composed { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            CompiledFieldSource::Composed { expr: compiled_expr }
        }
        reify_syntax::FieldSource::Imported { .. } => {
            CompiledFieldSource::Imported
        }
    };

    // Compute content hash
    let content_hash = {
        let name_hash = ContentHash::of_str(&field_def.name);
        let domain_hash = ContentHash::of_str(&format!("{}", domain_type));
        let codomain_hash = ContentHash::of_str(&format!("{}", codomain_type));
        let source_hash = match &source {
            CompiledFieldSource::Analytical { expr } => expr.content_hash,
            CompiledFieldSource::Sampled { config } => {
                let hashes = config.iter().map(|(k, e)| {
                    ContentHash::of_str(k).combine(e.content_hash)
                });
                ContentHash::combine_all(hashes)
            }
            CompiledFieldSource::Composed { expr } => expr.content_hash,
            CompiledFieldSource::Imported => ContentHash::of(&[0u8]),
        };
        ContentHash::combine_all([name_hash, domain_hash, codomain_hash, source_hash])
    };

    CompiledField {
        name: field_def.name.clone(),
        is_pub: field_def.is_pub,
        domain_type,
        codomain_type,
        source,
        content_hash,
    }
}

/// Check field composition types in a composed field expression.
///
/// Uses `CompiledExpr::walk` to traverse all 12+ expression variants,
/// looking for nested field calls like `f2(f1(p))`. For each such nesting,
/// verifies that the inner field's codomain matches the outer field's domain.
fn check_field_composition_types(
    expr: &CompiledExpr,
    field_registry: &HashMap<&str, &CompiledField>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut errors = Vec::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::FunctionCall { function, args } = &node.kind {
            // If this function call references a known field
            if let Some(outer_field) = field_registry.get(function.name.as_str()) {
                // Check if any argument is also a field call
                for arg in args {
                    if let CompiledExprKind::FunctionCall { function: inner_fn, .. } = &arg.kind
                        && let Some(inner_field) = field_registry.get(inner_fn.name.as_str())
                    {
                        // inner_field's codomain should implicitly convert to outer_field's domain
                        if !implicitly_converts_to(&inner_field.codomain_type, &outer_field.domain_type) {
                            errors.push(
                                Diagnostic::error(format!(
                                    "field composition type mismatch: codomain of '{}' ({}) does not match domain of '{}' ({})",
                                    inner_field.name, inner_field.codomain_type,
                                    outer_field.name, outer_field.domain_type
                                )),
                            );
                        }
                    }
                }
            }
        }
    });
    diagnostics.extend(errors);
}

/// Check if a let declaration's value is a geometry-producing function call.
fn is_geometry_let(expr: &reify_syntax::Expr) -> bool {
    matches!(
        &expr.kind,
        reify_syntax::ExprKind::FunctionCall { name, .. } if is_geometry_function(name)
    )
}

/// Compile a geometry function call expression into CompiledGeometryOps.
///
/// Maps positional arguments to the named parameters expected by each primitive:
/// - `box(width, height, depth)`
/// - `cylinder(radius, height)`
/// - `sphere(radius)`
///
/// Boolean operations (union, intersection, difference) take nested geometry
/// call expressions as arguments. Each arg is recursively compiled into ops,
/// and GeomRef::Step indices are assigned globally using `step_offset` (the
/// index of the first op this call will emit in the flat step_handles array).
fn compile_geometry_call(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    step_offset: usize,
) -> Option<Vec<CompiledGeometryOp>> {
    let (name, args) = match &expr.kind {
        reify_syntax::ExprKind::FunctionCall { name, args } => (name.as_str(), args),
        _ => return None,
    };

    // Boolean ops: args are nested geometry calls, NOT scalars.
    // Handle before scalar arg compilation below.
    match name {
        "union" | "intersection" | "difference" => {
            if args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "{}() expects 2 arguments, got {}",
                    name,
                    args.len()
                )));
                return None;
            }
            let bool_op = match name {
                "union" => BooleanOp::Union,
                "intersection" => BooleanOp::Intersection,
                "difference" => BooleanOp::Difference,
                _ => unreachable!(),
            };
            // Compile left arg recursively.
            let left_ops = match compile_geometry_call(
                &args[0], scope, enum_defs, functions, diagnostics, step_offset,
            ) {
                Some(ops) => ops,
                None => {
                    // Only emit extra diagnostic if no FunctionCall was detected
                    // (i.e., arg is a literal or ident — not a geometry expression).
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 1 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let left_result_step = step_offset + left_ops.len() - 1;
            let right_offset = step_offset + left_ops.len();
            // Compile right arg recursively.
            let right_ops = match compile_geometry_call(
                &args[1], scope, enum_defs, functions, diagnostics, right_offset,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[1].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 2 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let right_result_step = right_offset + right_ops.len() - 1;
            let mut all_ops = left_ops;
            all_ops.extend(right_ops);
            all_ops.push(CompiledGeometryOp::Boolean {
                op: bool_op,
                left: GeomRef::Step(left_result_step),
                right: GeomRef::Step(right_result_step),
            });
            return Some(all_ops);
        }
        "union_all" | "intersection_all" => {
            if args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "{}() expects at least 2 arguments, got {}",
                    name,
                    args.len()
                )));
                return None;
            }
            let bool_op = match name {
                "union_all" => BooleanOp::Union,
                "intersection_all" => BooleanOp::Intersection,
                _ => unreachable!(),
            };
            // Left-fold: compile all args, interleaving binary Boolean ops.
            // After each pair (accumulator, next_arg), emit a Boolean op whose
            // result becomes the next accumulator.
            let mut all_ops: Vec<CompiledGeometryOp> = Vec::new();
            let mut current_offset = step_offset;

            // Compile first arg.
            let first_ops = match compile_geometry_call(
                &args[0], scope, enum_defs, functions, diagnostics, current_offset,
            ) {
                Some(ops) => ops,
                None => {
                    if !matches!(args[0].kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                        diagnostics.push(Diagnostic::error(format!(
                            "{}() argument 1 must be a geometry expression",
                            name
                        )));
                    }
                    return None;
                }
            };
            let mut accumulator_step = current_offset + first_ops.len() - 1;
            current_offset += first_ops.len();
            all_ops.extend(first_ops);

            // Fold remaining args left-to-right.
            for (i, arg) in args.iter().enumerate().skip(1) {
                let arg_ops = match compile_geometry_call(
                    arg, scope, enum_defs, functions, diagnostics, current_offset,
                ) {
                    Some(ops) => ops,
                    None => {
                        if !matches!(arg.kind, reify_syntax::ExprKind::FunctionCall { .. }) {
                            diagnostics.push(Diagnostic::error(format!(
                                "{}() argument {} must be a geometry expression",
                                name,
                                i + 1
                            )));
                        }
                        return None;
                    }
                };
                let arg_result_step = current_offset + arg_ops.len() - 1;
                current_offset += arg_ops.len();
                all_ops.extend(arg_ops);
                // Emit binary op: (accumulator, arg) → new accumulator at current_offset.
                all_ops.push(CompiledGeometryOp::Boolean {
                    op: bool_op,
                    left: GeomRef::Step(accumulator_step),
                    right: GeomRef::Step(arg_result_step),
                });
                accumulator_step = current_offset;
                current_offset += 1;
            }
            return Some(all_ops);
        }
        _ => {}
    }

    let compiled_args: Vec<CompiledExpr> = args
        .iter()
        .map(|arg| compile_expr(arg, scope, enum_defs, functions, diagnostics))
        .collect();

    match name {
        // --- Primitives ---
        "box" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "box() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                    ("depth".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "cylinder" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "cylinder() expects 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                args: vec![
                    ("radius".to_string(), it.next().unwrap()),
                    ("height".to_string(), it.next().unwrap()),
                ],
            }])
        }
        "sphere" => {
            if compiled_args.len() != 1 {
                diagnostics.push(Diagnostic::error(format!(
                    "sphere() expects 1 argument, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            Some(vec![CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                args: vec![("radius".to_string(), compiled_args.into_iter().next().unwrap())],
            }])
        }
        // --- Patterns ---
        // linear_pattern(target, dx, dy, dz, count, spacing)
        "linear_pattern" => {
            if compiled_args.len() != 6 {
                diagnostics.push(Diagnostic::error(format!(
                    "linear_pattern() expects 6 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                target: GeomRef::Step(0), // target is first arg (evaluated at runtime)
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("dx".to_string(), it.next().unwrap()),
                    ("dy".to_string(), it.next().unwrap()),
                    ("dz".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("spacing".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)
        "circular_pattern" => {
            if compiled_args.len() != 9 {
                diagnostics.push(Diagnostic::error(format!(
                    "circular_pattern() expects 9 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Circular,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ox".to_string(), it.next().unwrap()),
                    ("oy".to_string(), it.next().unwrap()),
                    ("oz".to_string(), it.next().unwrap()),
                    ("ax".to_string(), it.next().unwrap()),
                    ("ay".to_string(), it.next().unwrap()),
                    ("az".to_string(), it.next().unwrap()),
                    ("count".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // mirror(target, ox, oy, oz, nx, ny, nz)
        "mirror" => {
            if compiled_args.len() != 7 {
                diagnostics.push(Diagnostic::error(format!(
                    "mirror() expects 7 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Mirror,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("ox".to_string(), it.next().unwrap()),
                    ("oy".to_string(), it.next().unwrap()),
                    ("oz".to_string(), it.next().unwrap()),
                    ("nx".to_string(), it.next().unwrap()),
                    ("ny".to_string(), it.next().unwrap()),
                    ("nz".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // --- Sweeps ---
        // loft(profile1, profile2, ...)
        "loft" => {
            if compiled_args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "loft() expects at least 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let profiles: Vec<GeomRef> = (0..compiled_args.len())
                .map(GeomRef::Step)
                .collect();
            let args: Vec<(String, CompiledExpr)> = compiled_args
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (format!("profile_{}", i), expr))
                .collect();
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                args,
            }])
        }
        // sweep(profile, path)
        "sweep" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "sweep() expects exactly 2 arguments (profile, path), got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let profiles: Vec<GeomRef> = vec![GeomRef::Step(0), GeomRef::Step(1)];
            let mut it = compiled_args.into_iter();
            let args: Vec<(String, CompiledExpr)> = vec![
                ("profile".to_string(), it.next().unwrap()),
                ("path".to_string(), it.next().unwrap()),
            ];
            Some(vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles,
                args,
            }])
        }
        // --- Modify extensions ---
        // shell(target, thickness, ...)
        "shell" => {
            if compiled_args.len() < 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "shell() expects at least 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            let mut args = vec![
                ("target".to_string(), it.next().unwrap()),
                ("thickness".to_string(), it.next().unwrap()),
            ];
            // Remaining args are face indices to remove
            for (i, expr) in it.enumerate() {
                args.push((format!("face_{}", i), expr));
            }
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                target: GeomRef::Step(0),
                args,
            }])
        }
        // thicken(target, offset)
        "thicken" => {
            if compiled_args.len() != 2 {
                diagnostics.push(Diagnostic::error(format!(
                    "thicken() expects 2 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("offset".to_string(), it.next().unwrap()),
                ],
            }])
        }
        // draft(target, angle, plane)
        "draft" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "draft() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            Some(vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Draft,
                target: GeomRef::Step(0),
                args: vec![
                    ("target".to_string(), it.next().unwrap()),
                    ("angle".to_string(), it.next().unwrap()),
                    ("plane".to_string(), it.next().unwrap()),
                ],
            }])
        }
        _ => {
            diagnostics.push(Diagnostic::error(format!(
                "unsupported geometry function: {}",
                name
            )));
            None
        }
    }
}

/// Detect if a constraint expression matches the count constraint pattern:
///   `collection_name.count == expr`  or  `expr == collection_name.count`
/// Returns `(collection_name, count_expr)` where count_expr is the non-.count side.
fn extract_count_constraint<'a>(
    expr: &'a reify_syntax::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<(String, &'a reify_syntax::Expr)> {
    if let reify_syntax::ExprKind::BinOp { op, left, right } = &expr.kind {
        if op != "==" {
            return None;
        }
        // Check LHS: collection_name.count == expr
        if let Some(name) = extract_collection_count(left, collection_sub_names) {
            return Some((name, right));
        }
        // Check RHS: expr == collection_name.count
        if let Some(name) = extract_collection_count(right, collection_sub_names) {
            return Some((name, left));
        }
    }
    None
}

/// Check if an expression is `collection_name.count` for a known collection sub.
fn extract_collection_count(
    expr: &reify_syntax::Expr,
    collection_sub_names: &HashSet<String>,
) -> Option<String> {
    if let reify_syntax::ExprKind::MemberAccess { object, member } = &expr.kind
        && member == "count"
        && let reify_syntax::ExprKind::Ident(name) = &object.kind
        && collection_sub_names.contains(name.as_str())
    {
        return Some(name.clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Step 21: Verify new geometry function names are recognized ---

    #[test]
    fn compile_geometry_linear_pattern_recognized() {
        assert!(is_geometry_function("linear_pattern"));
    }

    #[test]
    fn compile_geometry_circular_pattern_recognized() {
        assert!(is_geometry_function("circular_pattern"));
    }

    #[test]
    fn compile_geometry_mirror_recognized() {
        assert!(is_geometry_function("mirror"));
    }

    #[test]
    fn compile_geometry_loft_recognized() {
        assert!(is_geometry_function("loft"));
    }

    #[test]
    fn compile_geometry_shell_recognized() {
        assert!(is_geometry_function("shell"));
    }

    #[test]
    fn compile_geometry_thicken_recognized() {
        assert!(is_geometry_function("thicken"));
    }

    #[test]
    fn compile_geometry_draft_recognized() {
        assert!(is_geometry_function("draft"));
    }

    // --- Verify new geometry function calls compile into realizations ---

    #[test]
    fn compile_linear_pattern_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern(w, 1, 0, 0, 4, 20)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // linear_pattern is a geometry function, so should produce a realization
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for linear_pattern call, got {}",
            template.realizations.len()
        );
        // Verify it's a Pattern op with Linear kind
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Pattern { kind: PatternKind::Linear, .. }),
            "expected Pattern(Linear), got {:?}",
            op
        );
    }

    #[test]
    fn compile_mirror_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let mirrored = mirror(w, 0, 0, 0, 1, 0, 0)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_mirror"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for mirror call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Pattern { kind: PatternKind::Mirror, .. }),
            "expected Pattern(Mirror), got {:?}",
            op
        );
    }

    #[test]
    fn compile_loft_produces_realization() {
        let source = r#"structure S {
    param r: Scalar = 10mm
    let swept = loft(r, r)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_loft"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for loft call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Sweep { kind: SweepKind::Loft, .. }),
            "expected Sweep(Loft), got {:?}",
            op
        );
    }

    #[test]
    fn compile_shell_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let hollowed = shell(w, 1)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_shell"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for shell call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Modify { kind: ModifyKind::Shell, .. }),
            "expected Modify(Shell), got {:?}",
            op
        );
    }

    #[test]
    fn compile_thicken_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let thickened = thicken(w, 2)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_thicken"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for thicken call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Modify { kind: ModifyKind::Thicken, .. }),
            "expected Modify(Thicken), got {:?}",
            op
        );
    }

    #[test]
    fn compile_draft_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let drafted = draft(w, 0.1, w)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_draft"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for draft call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Modify { kind: ModifyKind::Draft, .. }),
            "expected Modify(Draft), got {:?}",
            op
        );
    }

    #[test]
    fn compile_circular_pattern_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = circular_pattern(w, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_circpat"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for circular_pattern call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(op, CompiledGeometryOp::Pattern { kind: PatternKind::Circular, .. }),
            "expected Pattern(Circular), got {:?}",
            op
        );
    }

    // --- Boolean function recognition tests (step-1) ---

    #[test]
    fn compile_geometry_union_recognized() {
        assert!(is_geometry_function("union"));
    }

    #[test]
    fn compile_geometry_intersection_recognized() {
        assert!(is_geometry_function("intersection"));
    }

    #[test]
    fn compile_geometry_difference_recognized() {
        assert!(is_geometry_function("difference"));
    }

    #[test]
    fn compile_geometry_union_all_recognized() {
        assert!(is_geometry_function("union_all"));
    }

    #[test]
    fn compile_geometry_intersection_all_recognized() {
        assert!(is_geometry_function("intersection_all"));
    }

    // --- Binary boolean op compilation tests (step-3) ---

    #[test]
    fn compile_union_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm), box(20mm, 20mm, 20mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // union(box, box) should produce 1 realization with 3 ops
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization, got {}",
            template.realizations.len()
        );
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box, box, union), got {}", ops.len());
        assert!(
            matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Primitive::Box at ops[0], got {:?}",
            ops[0]
        );
        assert!(
            matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Primitive::Box at ops[1], got {:?}",
            ops[1]
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Union, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    // --- Nested boolean compilation test (step-11) ---

    #[test]
    fn compile_nested_boolean_produces_five_ops() {
        // union(difference(box, cylinder), sphere)
        // Expected flat ops:
        //   0: Box
        //   1: Cylinder
        //   2: Boolean{Difference, Step(0), Step(1)}
        //   3: Sphere
        //   4: Boolean{Union, Step(2), Step(3)}
        let source = r#"structure S {
    let r = union(difference(box(20mm, 20mm, 20mm), cylinder(5mm, 20mm)), sphere(10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_nested_bool"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(
            ops.len(), 5,
            "expected 5 ops for nested boolean, got {}: {:?}",
            ops.len(), ops
        );
        assert!(
            matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "ops[0] expected Box, got {:?}", ops[0]
        );
        assert!(
            matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
            "ops[1] expected Cylinder, got {:?}", ops[1]
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean { op: BooleanOp::Difference, left: GeomRef::Step(0), right: GeomRef::Step(1) }
            ),
            "ops[2] expected Boolean{{Difference,0,1}}, got {:?}", ops[2]
        );
        assert!(
            matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }),
            "ops[3] expected Sphere, got {:?}", ops[3]
        );
        assert!(
            matches!(
                ops[4],
                CompiledGeometryOp::Boolean { op: BooleanOp::Union, left: GeomRef::Step(2), right: GeomRef::Step(3) }
            ),
            "ops[4] expected Boolean{{Union,2,3}}, got {:?}", ops[4]
        );
    }

    // --- Error case tests for boolean arg validation (step-9, step-10) ---

    #[test]
    fn compile_union_wrong_arity_emits_diagnostic() {
        // union(box(...)) with 1 arg should fail with arity diagnostic
        let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union_arity"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // Should produce no realization (compilation failed)
        assert_eq!(
            template.realizations.len(), 0,
            "expected 0 realizations for wrong-arity union, got {}",
            template.realizations.len()
        );
        // Should have a diagnostic mentioning "expects 2 arguments"
        assert!(
            compiled.diagnostics.iter().any(|d| d.message.contains("expects 2 arguments")),
            "expected 'expects 2 arguments' diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_union_non_geometry_arg_emits_diagnostic() {
        // union(42, box(...)) — first arg is a scalar literal, not geometry
        // The parser may reject bare number literals in function position,
        // so we use a param reference (Scalar param) which is a valid expr but not geometry.
        let source = r#"structure S {
    param w: Scalar = 10mm
    let r = union(w, box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union_nongeom"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // Should produce no realization (compilation failed)
        assert_eq!(
            template.realizations.len(), 0,
            "expected 0 realizations for non-geometry arg union, got {}",
            template.realizations.len()
        );
        // Should have at least one diagnostic
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected diagnostics for non-geometry arg, got none"
        );
    }

    // --- union_all / intersection_all fold compilation tests (step-7) ---

    #[test]
    fn compile_union_all_three_args_produces_five_ops() {
        // union_all(a, b, c) → left-fold: Union(Union(a,b), c)
        // ops: Box_a, Box_b, Boolean{Union,Step(0),Step(1)}, Box_c, Boolean{Union,Step(2),Step(3)}
        let source = r#"structure S {
    let r = union_all(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union_all"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(
            ops.len(), 5,
            "expected 5 ops for union_all(3 args), got {}: {:?}",
            ops.len(), ops
        );
        // ops[0]: Box
        assert!(
            matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Box at ops[0]"
        );
        // ops[1]: Box
        assert!(
            matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Box at ops[1]"
        );
        // ops[2]: Union(Step(0), Step(1))
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean { op: BooleanOp::Union, left: GeomRef::Step(0), right: GeomRef::Step(1) }
            ),
            "expected Boolean{{Union,Step(0),Step(1)}} at ops[2], got {:?}", ops[2]
        );
        // ops[3]: Box
        assert!(
            matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Box at ops[3]"
        );
        // ops[4]: Union(Step(2), Step(3))
        assert!(
            matches!(
                ops[4],
                CompiledGeometryOp::Boolean { op: BooleanOp::Union, left: GeomRef::Step(2), right: GeomRef::Step(3) }
            ),
            "expected Boolean{{Union,Step(2),Step(3)}} at ops[4], got {:?}", ops[4]
        );
    }

    // --- difference and intersection compilation tests (step-5, step-6) ---

    #[test]
    fn compile_difference_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = difference(box(20mm, 20mm, 20mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_diff"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box, box, difference)");
        assert!(
            matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Box at ops[0]"
        );
        assert!(
            matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected Box at ops[1]"
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Difference,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    #[test]
    fn compile_intersection_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = intersection(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_isect"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box, box, intersection)");
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Intersection,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Intersection, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    // --- Step 11: Directly test the catch-all branch in compile_geometry_call ---

    #[test]
    fn unsupported_geometry_fn_emits_diagnostic() {
        // Fabricate a FunctionCall expr with a name that is NOT in the
        // compile_geometry_call match arms (e.g., "make_cube").  This directly
        // exercises the `_ =>` catch-all branch added in step-4.
        let expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::FunctionCall {
                name: "make_cube".to_string(),
                args: vec![reify_syntax::Expr {
                    kind: reify_syntax::ExprKind::NumberLiteral(1.0),
                    span: reify_types::SourceSpan::new(0, 1),
                }],
            },
            span: reify_types::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_types::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let result = compile_geometry_call(&expr, &scope, &enum_defs, &functions, &mut diagnostics, 0);

        assert!(result.is_none(), "unrecognized geometry fn should return None");
        assert!(
            diagnostics.iter().any(|d| d.message.contains("unsupported geometry function")),
            "expected 'unsupported geometry function' diagnostic, got: {:?}",
            diagnostics
        );
    }

    // --- Sweep (pipe) compiler tests (task-310 step-13) ---

    #[test]
    fn is_geometry_function_sweep() {
        assert!(is_geometry_function("sweep"));
    }

    #[test]
    fn compile_sweep_produces_sweep_kind() {
        // sweep(profile, path) = 2 args, both geometry refs
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p, p)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for sweep call"
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Sweep {
                    kind: SweepKind::Sweep,
                    ..
                }
            ),
            "expected Sweep(Sweep), got {:?}",
            op
        );
        // Both profile and path should be in profiles as GeomRefs
        if let CompiledGeometryOp::Sweep { profiles, .. } = op {
            assert_eq!(
                profiles.len(),
                2,
                "sweep should have 2 profiles (profile + path), got {}",
                profiles.len()
            );
            assert_eq!(profiles[0], GeomRef::Step(0));
            assert_eq!(profiles[1], GeomRef::Step(1));
        }
    }

    #[test]
    fn compile_sweep_wrong_arg_count() {
        // sweep with 1 arg (should need 2)
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep_bad"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected diagnostics for wrong arg count"
        );
    }
}
