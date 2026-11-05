//! Value-cell declaration construction for `param`/`let` members, shared by the structure-body pass (`entity.rs`) and the guarded-block pass (`guards.rs`).

use std::collections::HashSet;

use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFunction};

use crate::annotations::{
    extract_solver_hints, lower_annotations, validate_annotations, validate_solver_hint_collections,
};
use crate::scope::CompilationScope;
use crate::type_resolution::{TypeAliasRegistry, resolve_type_expr_with_aliases};
use crate::types::{SolverHint, ValueCellDecl, ValueCellKind, Visibility};

/// Maps a param's `is_priv` flag to the compiled cell's `Visibility`:
/// `priv param` → `Visibility::Private` (hidden from external dot-access,
/// task #3978 δ); otherwise `Visibility::Public`. Shared by every
/// `ValueCellDecl` construction site that lowers a param's visibility —
/// top-level structure-param, port-member, guarded-block (`guards.rs`), and
/// the structure-def skeleton pass — so the mapping can't drift between
/// sites.
pub(crate) fn priv_flag_to_visibility(is_priv: bool) -> Visibility {
    if is_priv {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// Reject `Keyed<T>` in a value/param position (β escalation esc-3930-295, task 3931 γ).
///
/// `Keyed<T>` is a *sub-only* collection kind: it has no `Value::Keyed` form and is
/// elaborated as one child entity per author-assigned key via a `SubComponentDecl`.
/// Using it as the type of a `param`/`let` value cell is meaningless. If the resolved
/// `cell_type` is `Type::Keyed(_)`, emit a clear compile-time Error at cell construction
/// and poison the type to `Type::Error` (anti-cascade) so no Keyed value cell ever
/// reaches the eval graph. This upgrades the eval-layer
/// `is_representable_cell_type(Type::Keyed) == false` backstop (engine_eval.rs,
/// `is_representable_cell_type_rejects_keyed`) to an actionable diagnostic.
///
/// The low-level type *resolver* stays position-blind by design — the guard is layered
/// above it at this cell-construction site (see type_resolution.rs anchor test
/// `resolve_parameterized_keyed_is_position_blind_value_guard_deferred`). Subs lower to
/// `SubComponentDecl` (not value cells), so this cannot misfire on `sub x : Keyed<T>`.
///
/// Returns `true` and mutates `cell_type` to `Type::Error` when it fired.
pub(crate) fn reject_keyed_value_position(
    cell_type: &mut Type,
    name: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if matches!(cell_type, Type::Keyed(_)) {
        diagnostics.push(
            Diagnostic::error(format!(
                "`Keyed<T>` is a sub-only collection kind; it cannot be used as the type \
                 of value `{name}` — declare it as `sub {name} : Keyed<T>` instead"
            ))
            .with_label(DiagnosticLabel::new(
                span,
                "Keyed<T> in a value/param position",
            )),
        );
        *cell_type = Type::Error;
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// OptionNone fixup helpers (shared by entity.rs and guards.rs)
// ---------------------------------------------------------------------------

/// Fix up a compiled default expression for a param member.
///
/// When the expression is `none` and the declared param type is `Option<T>`,
/// the parser produces a fallback `Option<Real>` type. This helper overrides
/// that type with the correct `Option<T>` declared by the annotation.
///
/// Used in three places: top-level entity params (entity.rs), port member
/// params (entity.rs), and guarded member params (guards.rs).
pub(crate) fn fixup_option_none_for_param(compiled: &mut CompiledExpr, cell_type: &Type) {
    if matches!(&compiled.kind, CompiledExprKind::OptionNone)
        && matches!(cell_type, Type::Option(_))
    {
        *compiled = CompiledExpr::option_none(cell_type.clone());
    }
}

/// Fix up a compiled value expression for a let member.
///
/// When the expression is `none` and the let has a typed annotation like
/// `Option<T>`, the parser produces a fallback `Option<Real>` type. This
/// helper resolves the annotation and overrides the type with the correct
/// `Option<T>`.
///
/// Used in three places: top-level entity lets (entity.rs), port member
/// lets (entity.rs), and guarded member lets (guards.rs).
pub(crate) fn fixup_option_none_for_let(
    compiled_expr: &mut CompiledExpr,
    type_expr: Option<&reify_ast::TypeExpr>,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(&compiled_expr.kind, CompiledExprKind::OptionNone)
        && let Some(te) = type_expr
        && let Some(resolved) = resolve_type_expr_with_aliases(
            te,
            type_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        )
        && matches!(&resolved, Type::Option(_))
    {
        *compiled_expr = CompiledExpr::option_none(resolved);
    }
}

// ---------------------------------------------------------------------------
// Param value-cell decl construction (shared auto_free/param decl-building
// block, task ζ #5058, PRD compiler-type-hygiene.md §2 dup (e) / §3 decision
// 10). Single home for the `ValueCellDecl` construction previously
// copy-pasted at three sites: top-level structure params (entity.rs),
// port-member params (entity.rs), and guarded-member params (guards.rs).
// ---------------------------------------------------------------------------

/// Build the `ValueCellDecl` for a `param` member, shared by all three
/// param-decl sites (top-level structure params, port-member params, and
/// guarded-member params).
///
/// Fixes the fields identical across all three sites (`is_aux: false`, the
/// `Auto`-vs-`Param` branch shape keyed on `auto_free`) and parameterizes the
/// three axes that differ (`visibility`, `solver_hints`, and default-value
/// compilation via `compile_default`).
///
/// On the auto branch (`auto_free = Some(free)`) `compile_default` is NOT
/// invoked — `default_expr` is unconditionally `None` — matching the
/// original duplicated code at all three sites, none of which ever compiled
/// a default expression for an auto param. On the non-auto branch
/// (`auto_free = None`), `compile_default` is invoked exactly once with a
/// `&Type` view of `cell_type` (passed by reference so the caller's
/// compile/fixup/type-check logic can use it before `cell_type` is moved
/// into the returned decl) and its return value becomes `default_expr`
/// verbatim.
///
/// `compile_default` encapsulates each call site's own compile-expr variant
/// (`compile_expr_with_expected` at the two entity.rs sites vs.
/// `compile_expr_guarded` at the guards.rs site), the `fixup_option_none_for_param`
/// call, and — at the two entity.rs sites only — the `check_param_default_type`
/// cross-check. `guards.rs`'s guarded param never calls `check_param_default_type`
/// today, so keeping that check inside the closure (rather than hoisting it into
/// this helper) preserves each site's exact diagnostic surface.
pub(crate) fn build_param_value_cell_decl(
    id: ValueCellId,
    auto_free: Option<bool>,
    cell_type: Type,
    visibility: Visibility,
    solver_hints: Vec<SolverHint>,
    span: SourceSpan,
    compile_default: impl FnOnce(&Type) -> Option<CompiledExpr>,
) -> ValueCellDecl {
    if let Some(free) = auto_free {
        ValueCellDecl {
            id,
            kind: ValueCellKind::Auto { free },
            visibility,
            is_aux: false,
            cell_type,
            default_expr: None,
            solver_hints,
            span,
        }
    } else {
        let default_expr = compile_default(&cell_type);
        ValueCellDecl {
            id,
            kind: ValueCellKind::Param,
            visibility,
            is_aux: false,
            cell_type,
            default_expr,
            solver_hints,
            span,
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-let value-cell lowering, shared by every `let x = auto` binding site:
// the top-level `MemberDecl::Let` auto branch in entity.rs and the guarded one
// in guards.rs. Type resolution is its own step because guards.rs's
// name-registration prepass needs the declared type without the decl.
// ---------------------------------------------------------------------------

/// Resolve the declared type of an `auto` let binding.
///
/// An auto let is a solver cell, so its type must come from the declared
/// annotation — there is no initializer to infer one from. `None` means no
/// cell should be minted: either there is no annotation (the
/// mandatory-annotation error is emitted here) or the resolver could not
/// resolve it.
///
/// A `Keyed<T>` annotation is poisoned to `Type::Error` rather than suppressed,
/// so the cell is still minted and no Keyed value reaches the eval graph.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_auto_let_cell_type(
    name: &str,
    type_expr: Option<&reify_ast::TypeExpr>,
    span: SourceSpan,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    let Some(type_expr) = type_expr else {
        diagnostics.push(
            Diagnostic::error(
                "auto let binding requires a type annotation: \
                 use `let <name> : <Type> = auto`",
            )
            .with_label(DiagnosticLabel::new(
                span,
                "missing type annotation for auto let",
            )),
        );
        return None;
    };

    let mut cell_type = resolve_type_expr_with_aliases(
        type_expr,
        type_param_names,
        alias_registry,
        diagnostics,
        structure_names,
        trait_names,
    )?;

    // Reject `Keyed<T>` in a let value position (esc-3930-295, task 3931 γ):
    // poisoned to `Type::Error` so no Keyed value cell reaches the eval graph.
    reject_keyed_value_position(&mut cell_type, name, span, diagnostics);

    Some(cell_type)
}

/// Build the `ValueCellDecl` for an `auto` let whose type
/// `resolve_auto_let_cell_type` has already resolved: an `Auto { free }`
/// solver cell with no default expression, carrying the let's visibility,
/// `aux` flag and validated solver hints. Where the decl goes is the caller's
/// decision.
pub(crate) fn build_auto_let_value_cell_decl(
    entity_name: &str,
    let_decl: &reify_ast::LetDecl,
    free: bool,
    cell_type: Type,
    scope: &CompilationScope,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> ValueCellDecl {
    let visibility = if let_decl.is_pub {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let lowered_annotations = lower_annotations(&let_decl.annotations, diagnostics);
    validate_annotations(&lowered_annotations, "let", diagnostics);
    crate::annotations::display::validate_display_dimension(
        &lowered_annotations,
        &cell_type,
        diagnostics,
    );
    let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);
    validate_solver_hint_collections(&solver_hints, scope, functions, diagnostics);

    ValueCellDecl {
        id: ValueCellId::new(entity_name, &let_decl.name),
        kind: ValueCellKind::Auto { free },
        visibility,
        is_aux: let_decl.is_aux,
        cell_type,
        default_expr: None,
        solver_hints,
        span: let_decl.span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::Value;

    use crate::types::SolverHintKind;

    // ── build_param_value_cell_decl tests (task ζ #5058) ──
    // Exercise the helper's contracts directly (see its doc comment above its
    // definition for the full invariants). Full-pipeline diagnostic coverage
    // (that check_param_default_type still fires at Sites 1/2 after routing
    // through the helper) lives in param_default_type_mismatch_tests.rs.

    /// Shared assertions for the fields that pass straight through
    /// `build_param_value_cell_decl` from its inputs to the returned decl on
    /// both branches (`id`, `span`, `visibility`, `is_aux`, `cell_type`,
    /// `solver_hints`) — factored out so each test below only spells out the
    /// contract unique to it (auto-vs-param `kind`/`default_expr` behavior,
    /// `compile_default` invocation, or cell_type identity).
    fn assert_param_decl_passthrough_fields(
        decl: &ValueCellDecl,
        id: &ValueCellId,
        span: SourceSpan,
        visibility: Visibility,
        cell_type: &Type,
        solver_hints: &[SolverHint],
    ) {
        assert_eq!(&decl.id, id);
        assert_eq!(decl.span, span);
        assert_eq!(decl.visibility, visibility);
        assert!(!decl.is_aux);
        assert_eq!(&decl.cell_type, cell_type);
        assert_eq!(decl.solver_hints, solver_hints);
    }

    /// Auto branch (`auto_free = Some(free)`): `compile_default` must not be
    /// invoked, and the decl must carry `Auto { free }`, `default_expr: None`,
    /// and every other field verbatim from the inputs.
    #[test]
    fn build_param_value_cell_decl_auto_branch_skips_compile_default() {
        for free in [true, false] {
            let id = ValueCellId::new("TestEntity", "x");
            let cell_type = Type::dimensionless_scalar();
            let span = SourceSpan::new(3, 7);
            let solver_hints = vec![SolverHint {
                kind: SolverHintKind::PreferStock,
                collection: "stock_lengths".to_string(),
                span,
            }];
            let invoked = std::cell::Cell::new(false);

            let decl = build_param_value_cell_decl(
                id.clone(),
                Some(free),
                cell_type.clone(),
                Visibility::Private,
                solver_hints.clone(),
                span,
                |_ct: &Type| {
                    invoked.set(true);
                    None
                },
            );

            assert!(
                !invoked.get(),
                "compile_default must not be invoked on the auto branch (free={free})"
            );
            assert_eq!(decl.kind, ValueCellKind::Auto { free });
            assert!(decl.default_expr.is_none());
            assert_param_decl_passthrough_fields(
                &decl,
                &id,
                span,
                Visibility::Private,
                &cell_type,
                &solver_hints,
            );
        }
    }

    /// Non-auto branch (`auto_free = None`): `compile_default` is invoked once
    /// with `&cell_type`, and `default_expr` equals its return value;
    /// `cell_type` and `solver_hints` flow through unchanged. `CompiledExpr`
    /// has no `PartialEq`, so equality here is pinned via `content_hash`.
    #[test]
    fn build_param_value_cell_decl_param_branch_returns_compile_default_result() {
        for yields_default in [true, false] {
            let id = ValueCellId::new("TestEntity", "y");
            // Non-dimensionless, so cell_type equality below isn't trivially
            // satisfied by every dimensionless scalar comparing equal.
            let cell_type = Type::length();
            let span = SourceSpan::new(11, 19);
            let solver_hints = vec![SolverHint {
                kind: SolverHintKind::DiscreteSet,
                collection: "y_values".to_string(),
                span,
            }];
            let synthetic = CompiledExpr::literal(Value::Real(42.0), Type::dimensionless_scalar());
            let expected_hash = synthetic.content_hash;

            let decl = build_param_value_cell_decl(
                id.clone(),
                None,
                cell_type.clone(),
                Visibility::Public,
                solver_hints.clone(),
                span,
                |ct: &Type| {
                    assert_eq!(
                        ct, &cell_type,
                        "compile_default must receive cell_type by reference"
                    );
                    if yields_default {
                        Some(synthetic)
                    } else {
                        None
                    }
                },
            );

            assert_eq!(decl.kind, ValueCellKind::Param);
            assert_eq!(
                &decl.cell_type, &cell_type,
                "cell_type must flow through the helper unchanged"
            );
            assert_eq!(
                decl.solver_hints, solver_hints,
                "solver_hints must flow through the helper unchanged on the non-auto branch \
                 (not otherwise pinned end-to-end — every @solver_hint fixture decorates auto)"
            );
            assert_eq!(
                decl.default_expr.map(|e| e.content_hash),
                if yields_default {
                    Some(expected_hash)
                } else {
                    None
                },
                "default_expr must equal whatever compile_default returned (yields_default={yields_default})"
            );
        }
    }
}
