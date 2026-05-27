use super::*;
use std::collections::{HashMap, HashSet};

pub(crate) fn resolve_port_name(expr: &reify_ast::Expr) -> Option<String> {
    match &expr.kind {
        reify_ast::ExprKind::Ident(name) => Some(name.clone()),
        reify_ast::ExprKind::MemberAccess { object, member } => match &object.kind {
            reify_ast::ExprKind::Ident(obj_name) => Some(format!("{}.{}", obj_name, member)),
            // Indexed sub-component port refs after `forall` substitution
            // (task 2364): e.g. `vents[0].inlet` parses as
            // `MemberAccess { object: IndexAccess { Ident("vents"),
            //                                       NumberLiteral(0) },
            //                 member: "inlet" }`. Format as the dotted-bracket
            // string `"vents[0].inlet"` so the existing dotted-port-name
            // branches in `compile_connection` (which skip entity-port
            // lookup and direction checks for refs containing '.') flow
            // unchanged.
            reify_ast::ExprKind::IndexAccess {
                object: inner,
                index,
            } => match (&inner.kind, &index.kind) {
                (
                    reify_ast::ExprKind::Ident(obj_name),
                    reify_ast::ExprKind::NumberLiteral { value: n, .. },
                ) => Some(format!("{}[{}].{}", obj_name, *n as i64, member)),
                _ => None,
            },
            _ => None,
        },
        // Bare indexed sub-component (e.g. `vents[0]`) — also produced by
        // `forall` substitution when the body references the bound var
        // without dotting into a port. Returned as `"vents[0]"`. Currently
        // only well-formed `Ident[NumberLiteral]` shapes are accepted; other
        // index expressions (non-literal index, non-Ident object) return
        // None so existing diagnostic behaviour is preserved.
        reify_ast::ExprKind::IndexAccess { object, index } => {
            match (&object.kind, &index.kind) {
                (
                    reify_ast::ExprKind::Ident(obj_name),
                    reify_ast::ExprKind::NumberLiteral { value: n, .. },
                ) => Some(format!("{}[{}]", obj_name, *n as i64)),
                _ => None,
            }
        }
        // Ad-hoc selector: `port @ face("top")` — extract the base port name.
        reify_ast::ExprKind::AdHocSelector { base, .. } => resolve_port_name(base),
        _ => None,
    }
}

/// Check if an expression is an ad-hoc selector.
fn is_ad_hoc_selector(expr: &reify_ast::Expr) -> bool {
    matches!(&expr.kind, reify_ast::ExprKind::AdHocSelector { .. })
}

/// Auto-match port members between two pre-looked-up ports when no explicit port_mappings given.
///
/// Conditions for auto-matching:
/// 1. Both compiled ports must be `Some` (bare ports that exist in the context).
///    The caller is responsible for the bare-port guard and for passing `None` when a port
///    was not found (undefined-port error will have been emitted by the caller).
/// 2. Both ports must share the same `type_name` (same trait).
/// 3. All Param/Auto members on both sides must match by name (all-or-nothing).
///
/// Returns:
/// - Identity mappings `[(name, name), ...]` sorted alphabetically when all members match.
/// - Empty vec when either port is `None`, traits differ, or members are unmatched.
///   In the unmatched case a Warning diagnostic is emitted.
pub(crate) fn auto_match_port_members(
    left_compiled: Option<&CompiledPort>,
    right_compiled: Option<&CompiledPort>,
    diagnostics: &mut Vec<Diagnostic>,
    span: SourceSpan,
) -> Vec<(String, String)> {
    use std::collections::BTreeSet;

    // Skip if either port is not found (undefined port error already emitted by caller)
    let (left_compiled, right_compiled) = match (left_compiled, right_compiled) {
        (Some(l), Some(r)) => (l, r),
        _ => return Vec::new(),
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
            left_compiled.name, right_compiled.name, left_compiled.type_name
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
    source: reify_core::PortDirection,
    dest: reify_core::PortDirection,
) -> bool {
    use reify_core::PortDirection::*;
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
    pub(crate) enum_defs: &'a [reify_ir::EnumDef],
    pub(crate) functions: &'a [CompiledFunction],
    /// Trait registry for transitive LocatedPort checking.
    pub(crate) trait_registry: &'a HashMap<String, &'a CompiledTrait>,
}

/// Per-statement inputs for compiling a single connection.
pub(crate) struct ConnectInput<'a> {
    pub(crate) left_expr: &'a reify_ast::Expr,
    pub(crate) operator: reify_ast::ConnectOp,
    pub(crate) right_expr: &'a reify_ast::Expr,
    pub(crate) connector_type: Option<&'a str>,
    pub(crate) params: &'a [(String, reify_ast::Expr)],
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

    // Hoist port lookups once — used for both direction checking and auto-matching.
    let left_compiled = ctx.ports.iter().find(|p| p.name == left_port);
    let right_compiled = ctx.ports.iter().find(|p| p.name == right_port);
    let left_dir = left_compiled.map(|p| p.direction);
    let right_dir = right_compiled.map(|p| p.direction);

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
        reify_ast::ConnectOp::Forward => {
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
        reify_ast::ConnectOp::Reverse => match (left_dir, right_dir) {
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
        reify_ast::ConnectOp::Bidirectional => match (left_dir, right_dir) {
            (Some(l), Some(r)) => {
                if l == reify_core::PortDirection::Bidi && r == reify_core::PortDirection::Bidi {
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

    // Asymmetric LocatedPort check: warn when exactly one side of a connection
    // satisfies LocatedPort (directly or via refinement chain). Dotted port names
    // (sub-component references) are skipped because they cannot be resolved to a
    // CompiledPort in the current entity scope.
    {
        let is_bare = |name: &str| !name.contains('.');
        if is_bare(&left_port) && is_bare(&right_port) {
            let left_type = ctx
                .ports
                .iter()
                .find(|p| p.name == left_port)
                .map(|p| p.type_name.as_str());
            let right_type = ctx
                .ports
                .iter()
                .find(|p| p.name == right_port)
                .map(|p| p.type_name.as_str());

            // NOTE: this check only works for trait-typed ports. Ports declared with a
            // structure type (or a built-in type) won't be found in the trait_registry,
            // so trait_satisfies returns false for them — no warning is emitted even if
            // the structure conforms to LocatedPort via a separate trait declaration.
            // This is a known limitation acceptable for the current use-cases.
            if let (Some(lt), Some(rt)) = (left_type, right_type) {
                let mut visited_l = HashSet::new();
                let mut visited_r = HashSet::new();
                let left_located = trait_satisfies(
                    lt,
                    reify_core::LOCATED_PORT_TRAIT,
                    ctx.trait_registry,
                    &mut visited_l,
                );
                let right_located = trait_satisfies(
                    rt,
                    reify_core::LOCATED_PORT_TRAIT,
                    ctx.trait_registry,
                    &mut visited_r,
                );

                if left_located != right_located {
                    let (located_port, located_type, unlocated_port, unlocated_type) =
                        if left_located {
                            (&left_port, lt, &right_port, rt)
                        } else {
                            (&right_port, rt, &left_port, lt)
                        };
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "asymmetric LocatedPort: port \"{}\" ({}) satisfies LocatedPort but port \"{}\" ({}) does not \
                             — frame alignment constraint will not be generated",
                            located_port, located_type, unlocated_port, unlocated_type,
                        ))
                        .with_label(DiagnosticLabel::new(span, "asymmetric spatial frame")),
                    );
                }
            }
        }
    }

    // Create compatibility constraint
    let compat_id = ConstraintNodeId::new(ctx.entity_name, *acc.constraint_index);
    let compat_expr = CompiledExpr::literal(Value::Bool(compatible), Type::Bool);
    acc.constraints.push(CompiledConstraint {
        id: compat_id.clone(),
        label: Some(format!("connect_compat_{}_{}", left_port, right_port)),
        expr: compat_expr,
        domain: None,
        optimized_target: None,
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
            guard_state: GuardState::None,
            span,
            content_hash: conn_hash,
        });

        Some(connector_name)
    } else {
        None
    };

    // Determine effective port mappings: explicit takes priority; otherwise auto-match.
    // Auto-matching is only attempted for bare (non-dotted) ports when directions are compatible.
    // Skipping when incompatible avoids a misleading "members do not match" warning when the
    // real problem is direction incompatibility.
    let effective_mappings = if port_mappings.is_empty() {
        // is_bare guards are defense-in-depth; dotted ports also yield None from the lookup above
        if compatible && is_bare(&left_port) && is_bare(&right_port) {
            auto_match_port_members(left_compiled, right_compiled, diagnostics, span)
        } else {
            Vec::new() // direction error already emitted, or dotted ports; skip auto-match
        }
    } else {
        port_mappings.to_vec()
    };

    // Generate frame constraint when both sides use ad-hoc selectors.
    // Each side's ad-hoc expression compiles to a Frame(3); the constraint
    // records the pair so the evaluator can align them.
    let frame_constraint =
        if is_ad_hoc_selector(input.left_expr) && is_ad_hoc_selector(input.right_expr) {
            let left_frame = compile_expr(
                input.left_expr,
                ctx.scope,
                ctx.enum_defs,
                ctx.functions,
                diagnostics,
            );
            let right_frame = compile_expr(
                input.right_expr,
                ctx.scope,
                ctx.enum_defs,
                ctx.functions,
                diagnostics,
            );

            // Frame alignment constraint: the two frames should coincide.
            let frame_eq =
                CompiledExpr::binop(reify_ir::BinOp::Eq, left_frame, right_frame, Type::Bool);

            let fc_id = ConstraintNodeId::new(ctx.entity_name, *acc.constraint_index);
            acc.constraints.push(CompiledConstraint {
                id: fc_id.clone(),
                label: Some(format!("frame_align_{}_{}", left_port, right_port)),
                expr: frame_eq,
                domain: None,
                optimized_target: None,
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
