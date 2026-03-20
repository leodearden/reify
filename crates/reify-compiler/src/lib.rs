use std::collections::HashMap;

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ConstraintNodeId, ContentHash, DimensionVector,
    Diagnostic, DiagnosticLabel, OptimizationObjective, RealizationNodeId, ResolvedFunction,
    SourceSpan, Type, UnOp, Value, ValueCellId,
};

/// A compiled import declaration.
#[derive(Debug, Clone)]
pub struct CompiledImport {
    pub path: String,
    pub span: SourceSpan,
}

/// A compiled module — the output of the compiler.
#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub path: reify_types::ModulePath,
    pub imports: Vec<CompiledImport>,
    pub enum_defs: Vec<reify_types::EnumDef>,
    pub templates: Vec<TopologyTemplate>,
    pub diagnostics: Vec<reify_types::Diagnostic>,
    pub content_hash: ContentHash,
}

/// A topology template — compiled from a StructureDef.
/// Contains all the value cells, constraints, and realizations.
#[derive(Debug, Clone)]
pub struct TopologyTemplate {
    pub name: String,
    pub value_cells: Vec<ValueCellDecl>,
    pub constraints: Vec<CompiledConstraint>,
    pub realizations: Vec<RealizationDecl>,
    pub sub_components: Vec<SubComponentDecl>,
    pub objective: Option<OptimizationObjective>,
    pub content_hash: ContentHash,
}

/// A sub-component declaration — compiled from a SubDecl.
#[derive(Debug, Clone)]
pub struct SubComponentDecl {
    pub name: String,
    pub structure_name: String,
    pub args: Vec<(String, CompiledExpr)>,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
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
}

/// Transform operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformKind {
    Translate,
    Rotate,
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
    matches!(name, "box" | "cylinder" | "sphere")
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
        _ => None,
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

/// Name scope: maps identifier names to (ValueCellId, Type) within a structure.
struct CompilationScope {
    entity_name: String,
    names: HashMap<String, (ValueCellId, Type)>,
}

impl CompilationScope {
    fn new(entity_name: &str) -> Self {
        CompilationScope {
            entity_name: entity_name.to_string(),
            names: HashMap::new(),
        }
    }

    fn register(&mut self, name: &str, ty: Type) {
        let id = ValueCellId::new(&self.entity_name, name);
        self.names.insert(name.to_string(), (id, ty));
    }

    fn resolve(&self, name: &str) -> Option<&(ValueCellId, Type)> {
        self.names.get(name)
    }
}

/// Compile an `Expr` from the AST into a `CompiledExpr`.
///
/// Returns `Ok(compiled_expr)` on success or `Err(diagnostic)` on failure.
fn compile_expr(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    diagnostics: &mut Vec<Diagnostic>,
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
                    diagnostics.push(
                        Diagnostic::error(format!("unresolved name: {}", name))
                            .with_label(DiagnosticLabel::new(expr.span, "not found in scope")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::BinOp { op, left, right } => {
            let compiled_left = compile_expr(left, scope, enum_defs, diagnostics);
            let compiled_right = compile_expr(right, scope, enum_defs, diagnostics);
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
            let compiled_operand = compile_expr(operand, scope, enum_defs, diagnostics);
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
                .map(|arg| compile_expr(arg, scope, enum_defs, diagnostics))
                .collect();

            let resolved = ResolvedFunction {
                name: name.clone(),
                qualified_name: format!("std::{}", name),
            };

            // Infer a result type — for geometry functions, use a placeholder
            let result_type = if is_geometry_function(name) {
                // Geometry functions produce geometry, not a scalar value.
                // We use a dimensionless scalar as placeholder.
                Type::dimensionless_scalar()
            } else {
                // For math functions, use the type of the first argument as a heuristic
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
        reify_syntax::ExprKind::MemberAccess { object, member } => {
            // For M1, compile the object expression but emit a diagnostic
            // since we don't yet support member access fully.
            let _compiled_obj = compile_expr(object, scope, enum_defs, diagnostics);
            diagnostics.push(
                Diagnostic::error(format!("member access not yet supported: .{}", member))
                    .with_label(DiagnosticLabel::new(expr.span, "unsupported in M1")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::ListLiteral(_) => {
            diagnostics.push(
                Diagnostic::error("list literals not yet supported in compiler")
                    .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::SetLiteral(_) => {
            diagnostics.push(
                Diagnostic::error("set literals not yet supported in compiler")
                    .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::MapLiteral(_) => {
            diagnostics.push(
                Diagnostic::error("map literals not yet supported in compiler")
                    .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::IndexAccess { .. } => {
            diagnostics.push(
                Diagnostic::error("index access not yet supported in compiler")
                    .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
            );
            CompiledExpr::literal(Value::Undef, Type::Real)
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
            let compiled_discriminant = compile_expr(discriminant, scope, enum_defs, diagnostics);
            let compiled_arms: Vec<reify_types::CompiledMatchArm> = arms
                .iter()
                .map(|arm| {
                    let body = compile_expr(&arm.body, scope, enum_defs, diagnostics);
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
            let compiled_cond = compile_expr(condition, scope, enum_defs, diagnostics);
            let compiled_then = compile_expr(then_branch, scope, enum_defs, diagnostics);
            let compiled_else = compile_expr(else_branch, scope, enum_defs, diagnostics);
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
    }
}

/// Compile a parsed module into a compiled module.
///
/// Performs name resolution, type checking, and expression compilation.
pub fn compile(
    parsed: &reify_syntax::ParsedModule,
) -> CompiledModule {
    let mut imports = Vec::new();
    let mut templates = Vec::new();
    let mut diagnostics = Vec::new();

    // Pre-pass: collect ALL enum defs before processing structures.
    // This ensures order-independent declarations — a structure declared
    // before an enum can still reference that enum's variants.
    let enum_defs: Vec<reify_types::EnumDef> = parsed
        .declarations
        .iter()
        .filter_map(|d| {
            if let reify_syntax::Declaration::Enum(e) = d {
                Some(reify_types::EnumDef {
                    name: e.name.clone(),
                    variants: e.variants.clone(),
                })
            } else {
                None
            }
        })
        .collect();

    // Forward parse errors as diagnostics
    for err in &parsed.errors {
        diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                let template = compile_structure(structure, &enum_defs, &mut diagnostics);
                templates.push(template);
            }
            reify_syntax::Declaration::Enum(_) => {
                // Already collected in pre-pass above.
            }
            reify_syntax::Declaration::Import(import) => {
                imports.push(CompiledImport {
                    path: import.path.clone(),
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
        }
    }

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

        let all_hashes = std::iter::once(path_hash)
            .chain(template_hashes)
            .chain(import_hashes)
            .chain(enum_hashes);

        ContentHash::combine_all(all_hashes)
    };

    CompiledModule {
        path: parsed.path.clone(),
        imports,
        enum_defs,
        templates,
        diagnostics,
        content_hash,
    }
}

/// Compile a single structure definition into a topology template.
fn compile_structure(
    structure: &reify_syntax::StructureDef,
    enum_defs: &[reify_types::EnumDef],
    diagnostics: &mut Vec<Diagnostic>,
) -> TopologyTemplate {
    let entity_name = &structure.name;
    let mut scope = CompilationScope::new(entity_name);
    let mut value_cells = Vec::new();
    let mut constraints = Vec::new();
    let mut sub_components = Vec::new();
    let mut objective: Option<OptimizationObjective> = None;
    let mut constraint_index: u32 = 0;

    // First pass: register all param and let names into the scope so they can
    // reference each other (forward references within the structure).
    // We need types for the scope, so we resolve types in this pass as well.
    for member in &structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_name(&type_expr.name) {
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
                // Skip geometry function calls — they won't be value cells.
                if is_geometry_let(&let_decl.value) {
                    continue;
                }
                // We'll register with a placeholder type; the actual type will
                // be determined when we compile the expression. For now, use Real.
                // We'll update this after the expression is compiled.
                scope.register(&let_decl.name, Type::Real);
            }
            _ => {}
        }
    }

    // Second pass: compile all members.
    for member in &structure.members {
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

                if is_auto {
                    value_cells.push(ValueCellDecl {
                        id,
                        kind: ValueCellKind::Auto,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr: None,
                        span: param.span,
                    });
                } else {
                    let default_expr = param
                        .default
                        .as_ref()
                        .map(|expr| compile_expr(expr, &scope, enum_defs, diagnostics));

                    value_cells.push(ValueCellDecl {
                        id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr,
                        span: param.span,
                    });
                }
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // Skip geometry-producing function calls
                if is_geometry_let(&let_decl.value) {
                    continue;
                }

                let compiled_expr = compile_expr(&let_decl.value, &scope, enum_defs, diagnostics);
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                // Update the scope with the inferred type
                scope.register(&let_decl.name, cell_type.clone());

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                value_cells.push(ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    span: let_decl.span,
                });
            }
            reify_syntax::MemberDecl::Constraint(constraint) => {
                let compiled_expr = compile_expr(&constraint.expr, &scope, enum_defs, diagnostics);

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
                constraints.push(CompiledConstraint {
                    id,
                    label: constraint.label.clone(),
                    expr: compiled_expr,
                    span: constraint.span,
                });
                constraint_index += 1;
            }
            reify_syntax::MemberDecl::Sub(sub) => {
                let compiled_args: Vec<(String, CompiledExpr)> = sub
                    .args
                    .iter()
                    .map(|(name, expr)| {
                        (name.clone(), compile_expr(expr, &scope, enum_defs, diagnostics))
                    })
                    .collect();

                sub_components.push(SubComponentDecl {
                    name: sub.name.clone(),
                    structure_name: sub.structure_name.clone(),
                    args: compiled_args,
                    span: sub.span,
                    content_hash: sub.content_hash,
                });
            }
            reify_syntax::MemberDecl::Minimize(min_decl) => {
                let compiled_expr = compile_expr(&min_decl.expr, &scope, enum_defs, diagnostics);
                objective = Some(OptimizationObjective::Minimize(compiled_expr));
            }
            reify_syntax::MemberDecl::Maximize(max_decl) => {
                let compiled_expr = compile_expr(&max_decl.expr, &scope, enum_defs, diagnostics);
                objective = Some(OptimizationObjective::Maximize(compiled_expr));
            }
            reify_syntax::MemberDecl::GuardedGroup(_) => {
                // Guard evaluation semantics are not yet implemented in the compiler.
                // Guarded groups will be handled in a future task.
            }
        }
    }

    // Third pass: compile geometry let bindings into realizations.
    let mut realizations = Vec::new();
    let mut realization_index: u32 = 0;

    for member in &structure.members {
        if let reify_syntax::MemberDecl::Let(let_decl) = member
            && is_geometry_let(&let_decl.value)
            && let Some(ops) = compile_geometry_call(&let_decl.value, &scope, enum_defs, diagnostics)
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

        let all_hashes = std::iter::once(name_hash)
            .chain(vc_hashes)
            .chain(constraint_hashes)
            .chain(sub_hashes);

        ContentHash::combine_all(all_hashes)
    };

    TopologyTemplate {
        name: entity_name.clone(),
        value_cells,
        constraints,
        realizations,
        sub_components,
        objective,
        content_hash,
    }
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
fn compile_geometry_call(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<CompiledGeometryOp>> {
    let (name, args) = match &expr.kind {
        reify_syntax::ExprKind::FunctionCall { name, args } => (name.as_str(), args),
        _ => return None,
    };

    let compiled_args: Vec<CompiledExpr> = args
        .iter()
        .map(|arg| compile_expr(arg, scope, enum_defs, diagnostics))
        .collect();

    let named_args = match name {
        "box" => {
            if compiled_args.len() != 3 {
                diagnostics.push(Diagnostic::error(format!(
                    "box() expects 3 arguments, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            let mut it = compiled_args.into_iter();
            vec![
                ("width".to_string(), it.next().unwrap()),
                ("height".to_string(), it.next().unwrap()),
                ("depth".to_string(), it.next().unwrap()),
            ]
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
            vec![
                ("radius".to_string(), it.next().unwrap()),
                ("height".to_string(), it.next().unwrap()),
            ]
        }
        "sphere" => {
            if compiled_args.len() != 1 {
                diagnostics.push(Diagnostic::error(format!(
                    "sphere() expects 1 argument, got {}",
                    compiled_args.len()
                )));
                return None;
            }
            vec![("radius".to_string(), compiled_args.into_iter().next().unwrap())]
        }
        _ => return None,
    };

    let kind = match name {
        "box" => PrimitiveKind::Box,
        "cylinder" => PrimitiveKind::Cylinder,
        "sphere" => PrimitiveKind::Sphere,
        _ => return None,
    };

    Some(vec![CompiledGeometryOp::Primitive {
        kind,
        args: named_args,
    }])
}
