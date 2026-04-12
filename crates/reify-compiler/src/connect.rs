use super::*;

pub(crate) fn resolve_port_name(expr: &reify_syntax::Expr) -> Option<String> {
    match &expr.kind {
        reify_syntax::ExprKind::Ident(name) => Some(name.clone()),
        reify_syntax::ExprKind::MemberAccess { object, member } => match &object.kind {
            reify_syntax::ExprKind::Ident(obj_name) => Some(format!("{}.{}", obj_name, member)),
            _ => None,
        },
        // Ad-hoc selector: `port @ face("top")` — extract the base port name.
        reify_syntax::ExprKind::AdHocSelector { base, .. } => resolve_port_name(base),
        _ => None,
    }
}

/// Check if an expression is an ad-hoc selector.
fn is_ad_hoc_selector(expr: &reify_syntax::Expr) -> bool {
    matches!(&expr.kind, reify_syntax::ExprKind::AdHocSelector { .. })
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
pub(crate) fn auto_match_port_members(
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
            .filter(|m| matches!(m.kind, ValueCellKind::Param | ValueCellKind::Auto { .. }))
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
    left_names
        .into_iter()
        .map(|name| (name.clone(), name))
        .collect()
}

/// Check if a source port direction is forward-compatible with a destination port direction.
pub(crate) fn is_forward_compatible(
    source: reify_types::PortDirection,
    dest: reify_types::PortDirection,
) -> bool {
    use reify_types::PortDirection::*;
    matches!(
        (source, dest),
        (Out, In) | (Out, Bidi) | (Bidi, In) | (Bidi, Bidi) | (Bidi, Out) | (In, Bidi)
    )
}

/// Accumulated outputs from connection compilation.
pub(crate) struct ConnectAccumulator<'a> {
    pub(crate) constraints: &'a mut Vec<CompiledConstraint>,
    pub(crate) constraint_index: &'a mut u32,
    pub(crate) connections: &'a mut Vec<CompiledConnection>,
    pub(crate) sub_components: &'a mut Vec<SubComponentDecl>,
    pub(crate) connector_index: &'a mut u32,
}

/// Read-only context for compiling connections.
pub(crate) struct ConnectContext<'a> {
    pub(crate) entity_name: &'a str,
    pub(crate) ports: &'a [CompiledPort],
    pub(crate) scope: &'a CompilationScope<'a>,
    pub(crate) enum_defs: &'a [reify_types::EnumDef],
    pub(crate) functions: &'a [CompiledFunction],
}

/// Per-statement inputs for compiling a single connection.
pub(crate) struct ConnectInput<'a> {
    pub(crate) left_expr: &'a reify_syntax::Expr,
    pub(crate) operator: reify_syntax::ConnectOp,
    pub(crate) right_expr: &'a reify_syntax::Expr,
    pub(crate) connector_type: Option<&'a str>,
    pub(crate) params: &'a [(String, reify_syntax::Expr)],
    pub(crate) port_mappings: &'a [(String, String)],
    pub(crate) span: SourceSpan,
}

/// Compile a single connection (from connect statement or chain desugaring).
pub(crate) fn compile_connection(
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
                Diagnostic::error("invalid port reference in connect statement").with_label(
                    DiagnosticLabel::new(left_expr.span, "unsupported expression"),
                ),
            );
            return;
        }
    };
    let right_port = match resolve_port_name(right_expr) {
        Some(name) => name,
        None => {
            diagnostics.push(
                Diagnostic::error("invalid port reference in connect statement").with_label(
                    DiagnosticLabel::new(right_expr.span, "unsupported expression"),
                ),
            );
            return;
        }
    };

    // Look up port directions for compatibility checking
    let dir_of = |name: &str| {
        ctx.ports
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.direction)
    };
    let left_dir = dir_of(&left_port);
    let right_dir = dir_of(&right_port);

    // Bare ident (no dot) that doesn't match any port is undefined
    let is_bare = |name: &str| !name.contains('.');
    if is_bare(&left_port) && left_dir.is_none() {
        diagnostics.push(
            Diagnostic::error(format!(
                "undefined port '{}' in connect statement",
                left_port
            ))
            .with_label(DiagnosticLabel::new(span, "undefined port")),
        );
    }
    if is_bare(&right_port) && right_dir.is_none() {
        diagnostics.push(
            Diagnostic::error(format!(
                "undefined port '{}' in connect statement",
                right_port
            ))
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
        reify_syntax::ConnectOp::Reverse => match (left_dir, right_dir) {
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
        },
        reify_syntax::ConnectOp::Bidirectional => match (left_dir, right_dir) {
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
        },
    };

    // Create compatibility constraint
    let compat_id = ConstraintNodeId::new(ctx.entity_name, *acc.constraint_index);
    let compat_expr = CompiledExpr::literal(Value::Bool(compatible), Type::Bool);
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
                (
                    name.clone(),
                    compile_expr(expr, ctx.scope, ctx.enum_defs, ctx.functions, diagnostics),
                )
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
            guard_expr: None,
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

    // Generate frame constraint when both sides use ad-hoc selectors.
    // Each side's ad-hoc expression compiles to a Frame(3); the constraint
    // records the pair so the evaluator can align them.
    let frame_constraint =
        if is_ad_hoc_selector(input.left_expr) && is_ad_hoc_selector(input.right_expr) {
            let left_frame =
                compile_expr(input.left_expr, ctx.scope, ctx.enum_defs, ctx.functions, diagnostics);
            let right_frame =
                compile_expr(input.right_expr, ctx.scope, ctx.enum_defs, ctx.functions, diagnostics);

            // Frame alignment constraint: the two frames should coincide.
            let frame_eq = CompiledExpr::binop(
                reify_types::BinOp::Eq,
                left_frame,
                right_frame,
                Type::Bool,
            );

            let fc_id = ConstraintNodeId::new(ctx.entity_name, *acc.constraint_index);
            acc.constraints.push(CompiledConstraint {
                id: fc_id.clone(),
                label: Some(format!("frame_align_{}_{}", left_port, right_port)),
                expr: frame_eq,
                domain: None,
                span,
            });
            *acc.constraint_index += 1;
            Some(fc_id)
        } else {
            None
        };

    acc.connections.push(CompiledConnection {
        left_port,
        operator,
        right_port,
        connector_sub,
        compatibility_constraint: compat_id,
        port_mappings: effective_mappings,
        frame_constraint,
        span,
    });
}

