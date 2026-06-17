use super::*;

pub(crate) fn check_recursive_termination(
    templates: &[TopologyTemplate],
    cyclic_sccs: &[HashSet<String>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if cyclic_sccs.is_empty() {
        return;
    }

    // Build map: template name → SCC index (only for cyclic SCCs)
    let name_to_scc: HashMap<&str, usize> = cyclic_sccs
        .iter()
        .enumerate()
        .flat_map(|(i, scc)| scc.iter().map(move |name| (name.as_str(), i)))
        .collect();

    for template in templates {
        if !template.is_recursive {
            continue;
        }

        let Some(&scc_idx) = name_to_scc.get(template.name.as_str()) else {
            continue;
        };
        let scc = &cyclic_sccs[scc_idx];

        for sub in &template.sub_components {
            // Only check subs that target another template in the same SCC (recursive subs).
            // NOTE: this tests SCC *membership* — whether the target is in the same recursive
            // cycle — NOT whether it exists at all. Sub-target existence (module ∪ prelude) is
            // validated separately in `conformance::sub_component_validation::check_sub_structure_existence`
            // (task 4528), which runs before this pass in `compile_with_prelude_context_checked`.
            if !scc.contains(&sub.structure_name) {
                continue;
            }

            // Dispatch on the sub's guard state.
            // - Broken:   the user wrote a guard but it failed to compile. The compile error is
            //             already in diagnostics. Skip termination checks to avoid misleading
            //             follow-on errors (e.g. "add a where clause" when one was already written).
            // - None:     the user wrote no guard at all. Emit the "add a where clause" error.
            // - Compiled: proceed with the guard expression for further analysis.
            let guard = match &sub.guard_state {
                GuardState::Broken => continue,
                GuardState::None => {
                    diagnostics.push(
                        Diagnostic::error(
                            "recursive sub has no termination condition: add a where clause (e.g., `where n > 0`)",
                        )
                        .with_label(DiagnosticLabel::new(sub.span, "recursive sub without guard")),
                    );
                    continue;
                }
                GuardState::Compiled(g) => g,
            };

            // Step 8: guard must reference at least one Int or Bool param
            let guard_refs = termination_collect_refs(guard);
            let referenced_params: Vec<&ValueCellDecl> = template
                .value_cells
                .iter()
                .filter(|vc| {
                    vc.kind == ValueCellKind::Param
                        && matches!(vc.cell_type, Type::Int | Type::Bool)
                        && guard_refs.contains(&vc.id)
                })
                .collect();

            if referenced_params.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        "recursive sub guard does not reference any Int or Bool parameter: the guard must mention a parameter that is decremented toward a base case",
                    )
                    .with_label(DiagnosticLabel::new(sub.span, "guard references no Int/Bool param")),
                );
                continue;
            }

            // Step 14: undef in guard-referenced args is forbidden.
            // Only check args whose param is referenced by the guard — other args are
            // termination-irrelevant and may legally contain undef.
            let guard_param_names: HashSet<String> = referenced_params
                .iter()
                .map(|vc| vc.id.member.clone())
                .collect();
            if termination_args_contain_undef(sub, &guard_param_names) {
                diagnostics.push(
                    Diagnostic::error(
                        "undef is not allowed as a non-termination mechanism in recursive sub arguments",
                    )
                    .with_label(DiagnosticLabel::new(sub.span, "recursive sub uses undef")),
                );
                continue; // Don't pile on more errors for this sub
            }

            // Step 10/12: each guard-referenced param must be modified in the sub's args
            for param in &referenced_params {
                let param_name = &param.id.member;
                let is_modified = sub
                    .args
                    .iter()
                    .find(|(name, _)| name == param_name)
                    .map(|(_, expr)| termination_is_modifying(expr, &param.id))
                    .unwrap_or(false);

                if !is_modified {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "recursive sub does not decrement parameter '{}' toward base case: the argument for '{}' must contain a modifying operation (e.g., `n - 1` for Int, `!flag` for Bool)",
                            param_name, param_name
                        ))
                        .with_label(DiagnosticLabel::new(sub.span, "parameter passed unchanged")),
                    );
                }
            }
        }
    }
}

/// Returns true if any guard-referenced arg of the recursive sub contains `undef`.
///
/// Only args whose parameter name is in `guard_param_names` are checked.
/// Args for params not referenced by the guard are termination-irrelevant and
/// may legally contain undef (e.g., `label: undef` when the guard only mentions `n`).
pub(crate) fn termination_args_contain_undef(
    sub: &SubComponentDecl,
    guard_param_names: &HashSet<String>,
) -> bool {
    sub.args
        .iter()
        .filter(|(name, _)| guard_param_names.contains(name))
        .any(|(_, expr)| {
            let mut found = false;
            expr.walk(&mut |e| {
                if matches!(&e.kind, CompiledExprKind::Literal(Value::Undef)) {
                    found = true;
                }
            });
            found
        })
}

/// Collect all ValueCellIds referenced in an expression (for guard analysis).
pub(crate) fn termination_collect_refs(expr: &CompiledExpr) -> HashSet<ValueCellId> {
    let mut refs = HashSet::new();
    expr.walk(&mut |e| {
        if let CompiledExprKind::ValueRef(id) = &e.kind {
            refs.insert(id.clone());
        }
    });
    refs
}

/// Returns true if `expr` represents a modifying operation on a parameter (not just passing it unchanged).
///
/// For Int params: must contain BinOp::Sub (subtraction moves toward a base case).
/// For Bool params: must contain UnOp::Not.
/// Any expression that is NOT simply `ValueRef(param_id)` AND contains a Sub/Not counts.
///
/// Note: BinOp::Add is intentionally excluded. `n + 1` with guard `n > 0` diverges —
/// the value increases and never reaches the base case. Users should write `n - 1`
/// (BinOp::Sub) for the canonical decrementing pattern.
pub(crate) fn termination_is_modifying(expr: &CompiledExpr, param_id: &ValueCellId) -> bool {
    // If the expression is just the param unchanged, not modifying.
    if matches!(&expr.kind, CompiledExprKind::ValueRef(id) if id == param_id) {
        return false;
    }

    // Walk for Sub (Int modification toward base case) or Not (Bool modification)
    let mut found_mod = false;
    expr.walk(&mut |e| match &e.kind {
        CompiledExprKind::BinOp { op: BinOp::Sub, .. } => found_mod = true,
        CompiledExprKind::UnOp { op: UnOp::Not, .. } => found_mod = true,
        _ => {}
    });
    found_mod
}
