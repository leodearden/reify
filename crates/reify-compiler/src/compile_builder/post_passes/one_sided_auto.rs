//! The one-sided strict-auto refusal, STATIC arm (task #6692, P1-ε; PRD
//! `docs/prds/v0_6/solver-driver-parity.md` §4.6 case 2).
//!
//! A strict `auto` the model bounds on one side only has no designed value:
//! the solver's synthetic centrality objective is monotone in it, so a solve
//! can only return the edge of the solver's internal default domain. Where the
//! module proves that statically, it is a compile Error (INV-SF-4) rather than
//! an `Indeterminate` or a silent 10 m.
//!
//! The proof mirrors today's eval semantics: an auto's solve scope is its
//! owning template's own cells and constraints. A template is judged only
//! when that whole population is static and local. Every other source makes
//! the pass ABSTAIN, leaving the case to the runtime arm in
//! `reify-constraints`' `verify_uniqueness`:
//! - an imported template, a template objective, or an objective-bearing
//!   container that may pass its objective down;
//! - forall templates, guarded groups holding constraints, match-arm groups,
//!   relations and connections;
//! - any purpose that is not bound-free (the module-wide check);
//! - any opaque conjunct, and any auto mention outside `auto op e` /
//!   `e op auto`;
//! - `auto(free)`, non-Scalar autos and autos carrying a solver hint, which
//!   are never judged.
//!
//! A missed refusal keeps today's behaviour; a false one rejects legal code,
//! so the rules are skewed toward silence, as `E_OBJECTIVE_INERT`'s are.

use std::collections::{BTreeSet, HashMap};

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, UnOp};

use super::template_graph::{imported_template_names, may_contain, names_same_structure};
use crate::compile_builder::ctx::CompilationCtx;
use crate::types::{CompiledPurpose, TopologyTemplate, ValueCellDecl, ValueCellKind};

/// Which side of a strict auto the module's constraints bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Below,
    Above,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Side::Below => Side::Above,
            Side::Above => Side::Below,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Side::Below => "below",
            Side::Above => "above",
        }
    }
}

/// A proof that strict auto `auto` is bounded on `bounded` only.
#[derive(Debug, Clone)]
struct OneSidedAutoFinding {
    template_name: String,
    auto: ValueCellId,
    bounded: Side,
    decl_span: SourceSpan,
    bounding_spans: Vec<SourceSpan>,
}

/// Can a purpose node add a bound or an objective to a subject? Only the
/// determinacy and reflective vocabulary and the logical connectives over it
/// are known not to; anything else, a comparison included, is assumed to.
fn is_bound_free_node(node: &CompiledExpr) -> bool {
    match &node.kind {
        CompiledExprKind::BinOp { op, .. } => {
            matches!(op, BinOp::And | BinOp::Or | BinOp::Implies)
        }
        CompiledExprKind::UnOp { op, .. } => matches!(op, UnOp::Not),
        kind => matches!(
            kind,
            CompiledExprKind::Literal(_)
                | CompiledExprKind::ValueRef(_)
                | CompiledExprKind::Conditional { .. }
                | CompiledExprKind::Quantifier { .. }
                | CompiledExprKind::DeterminacyPredicate { .. }
                | CompiledExprKind::ReflectiveCellList(_)
                | CompiledExprKind::PurposeReflectiveAggregation { .. }
                | CompiledExprKind::ListLiteral(_)
        ),
    }
}

fn every_node(expr: &CompiledExpr, predicate: impl Fn(&CompiledExpr) -> bool) -> bool {
    let mut all = true;
    expr.walk(&mut |node| all &= predicate(node));
    all
}

/// A purpose that declares no objective and no bounding constraint or let.
/// The stdlib `simulation_ready` / `design_review` purposes are bound-free.
fn purpose_is_bound_free(purpose: &CompiledPurpose) -> bool {
    purpose.objective.is_none()
        && purpose
            .constraints
            .iter()
            .map(|c| &c.expr)
            .chain(purpose.lets.iter().map(|l| &l.expr))
            .all(|expr| every_node(expr, is_bound_free_node))
}

/// Is this node's reach described by the refs `collect_value_refs` returns,
/// confined to the template? Deliberately an allowlist, so a variant added
/// later narrows the rule instead of widening it.
fn is_reach_transparent(node: &CompiledExpr) -> bool {
    !node.result_type.is_error()
        && matches!(
            node.kind,
            CompiledExprKind::Literal(_)
                | CompiledExprKind::ValueRef(_)
                | CompiledExprKind::BinOp { .. }
                | CompiledExprKind::UnOp { .. }
                | CompiledExprKind::Conditional { .. }
                | CompiledExprKind::OptionSome(_)
                | CompiledExprKind::OptionNone
                | CompiledExprKind::FunctionCall { .. }
                | CompiledExprKind::ListLiteral(_)
                | CompiledExprKind::IndexAccess { .. }
        )
}

/// Split `expr` on `BinOp::And` into its leaf conjuncts.
fn leaf_conjuncts<'e>(expr: &'e CompiledExpr, out: &mut Vec<&'e CompiledExpr>) {
    match &expr.kind {
        CompiledExprKind::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            leaf_conjuncts(left, out);
            leaf_conjuncts(right, out);
        }
        _ => out.push(expr),
    }
}

/// One template's cells, for closing an expression's reads over its lets.
struct TemplateCells<'t> {
    cells: HashMap<&'t ValueCellId, &'t ValueCellDecl>,
}

impl<'t> TemplateCells<'t> {
    fn of(template: &'t TopologyTemplate) -> Self {
        Self {
            cells: template.value_cells.iter().map(|d| (&d.id, d)).collect(),
        }
    }

    /// The autos `expr` reaches, closing over non-auto cells' default
    /// expressions. `None` when anything on the way is opaque or names a cell
    /// this template does not declare — which includes another template's
    /// auto, the read-DAG edge that could merge solve scopes.
    fn reached_autos(&self, expr: &CompiledExpr) -> Option<BTreeSet<ValueCellId>> {
        let mut autos = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut pending: Vec<&CompiledExpr> = vec![expr];
        while let Some(current) = pending.pop() {
            if !every_node(current, is_reach_transparent) {
                return None;
            }
            for id in current.collect_value_refs() {
                if !visited.insert(id.clone()) {
                    continue;
                }
                let decl = self.cells.get(&id)?;
                if decl.kind.is_auto() {
                    autos.insert(id);
                } else if let Some(default_expr) = &decl.default_expr {
                    pending.push(default_expr);
                }
            }
        }
        Some(autos)
    }

    /// The side `conjunct` bounds `auto` on, when it has the recognised shape
    /// `auto op e` / `e op auto` with `op` an inequality and `e` reaching no
    /// auto.
    fn bounded_side(&self, conjunct: &CompiledExpr, auto: &ValueCellId) -> Option<Side> {
        let CompiledExprKind::BinOp { op, left, right } = &conjunct.kind else {
            return None;
        };
        let auto_on_left = match op {
            BinOp::Ge | BinOp::Gt => Side::Below,
            BinOp::Le | BinOp::Lt => Side::Above,
            _ => return None,
        };
        let is_auto =
            |e: &CompiledExpr| matches!(&e.kind, CompiledExprKind::ValueRef(id) if id == auto);
        let auto_free = |e: &CompiledExpr| self.reached_autos(e).is_some_and(|a| a.is_empty());
        if is_auto(left) && auto_free(right) {
            Some(auto_on_left)
        } else if is_auto(right) && auto_free(left) {
            Some(auto_on_left.opposite())
        } else {
            None
        }
    }
}

/// Template-level abstentions: every way an auto of `template` could be
/// bounded, or governed by an objective, from outside its own constraints.
fn template_is_judgeable(
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
    imported_names: &[&str],
) -> bool {
    let imported = imported_names
        .iter()
        .any(|name| names_same_structure(&template.name, name));
    let inherits_an_objective = all_templates.iter().any(|other| {
        other.name != template.name
            && other.objective.is_some()
            && may_contain(other, &template.name, all_templates, imported_names)
    });
    let has_dynamic_population = !template.forall_templates.is_empty()
        || template
            .guarded_groups
            .iter()
            .any(|g| !g.constraints.is_empty() || !g.else_constraints.is_empty())
        || !template.match_arm_groups.is_empty()
        || !template.relations.is_empty()
        || !template.connections.is_empty();
    !imported && template.objective.is_none() && !inherits_an_objective && !has_dynamic_population
}

/// Is `decl` a strict, continuous auto with no solver hint — the only shape
/// whose resolution the centrality objective corner-picks?
fn is_judged_auto(decl: &ValueCellDecl) -> bool {
    matches!(decl.kind, ValueCellKind::Auto { free: false })
        && matches!(decl.cell_type, Type::Scalar { .. })
        && decl.solver_hints.is_empty()
}

/// The one-sided strict autos of `template`, ordered by auto id.
fn template_findings(template: &TopologyTemplate) -> Vec<OneSidedAutoFinding> {
    let cells = TemplateCells::of(template);
    let mut conjuncts: Vec<(&CompiledExpr, SourceSpan, BTreeSet<ValueCellId>)> = Vec::new();
    for constraint in &template.constraints {
        let mut leaves = Vec::new();
        leaf_conjuncts(&constraint.expr, &mut leaves);
        for leaf in leaves {
            let Some(autos) = cells.reached_autos(leaf) else {
                return Vec::new();
            };
            conjuncts.push((leaf, constraint.span, autos));
        }
    }

    let mut judged: Vec<&ValueCellDecl> = template
        .value_cells
        .iter()
        .filter(|d| is_judged_auto(d))
        .collect();
    judged.sort_by(|a, b| a.id.cmp(&b.id));

    judged
        .into_iter()
        .filter_map(|decl| {
            let mut sides: Vec<Side> = Vec::new();
            let mut bounding_spans = Vec::new();
            for (leaf, span, autos) in &conjuncts {
                if !autos.contains(&decl.id) {
                    continue;
                }
                // Any mention outside the recognised shape: no proof.
                let side = cells.bounded_side(leaf, &decl.id)?;
                if !sides.contains(&side) {
                    sides.push(side);
                }
                if !bounding_spans.contains(span) {
                    bounding_spans.push(*span);
                }
            }
            let &[bounded] = sides.as_slice() else {
                return None;
            };
            Some(OneSidedAutoFinding {
                template_name: template.name.clone(),
                auto: decl.id.clone(),
                bounded,
                decl_span: decl.span,
                bounding_spans,
            })
        })
        .collect()
}

/// Every one-sided strict auto the module proves, in template order then auto
/// id. Empty when any purpose could add a bound or objective.
fn one_sided_auto_findings(
    all_templates: &[TopologyTemplate],
    purposes: &[CompiledPurpose],
    imported_names: &[&str],
) -> Vec<OneSidedAutoFinding> {
    if !purposes.iter().all(purpose_is_bound_free) {
        return Vec::new();
    }
    all_templates
        .iter()
        .filter(|t| template_is_judgeable(t, all_templates, imported_names))
        .flat_map(template_findings)
        .collect()
}

fn render(finding: &OneSidedAutoFinding) -> Diagnostic {
    let missing = finding.bounded.opposite().word();
    let bounded = finding.bounded.word();
    let diagnostic = Diagnostic::error(format!(
        "strict auto parameter resolution is not uniquely determined: no constraint \
         bounds `{auto}` {missing}. It is bounded only {bounded}, so a solve could only \
         return the edge of the solver's internal default domain — add the missing \
         bound, give `{template}` an objective, or declare it `auto(free)`",
        auto = finding.auto,
        template = finding.template_name,
    ))
    .with_code(DiagnosticCode::ConstraintNonUnique)
    .with_label(DiagnosticLabel::new(
        finding.decl_span,
        format!("no constraint bounds this auto {missing}"),
    ));
    finding.bounding_spans.iter().fold(diagnostic, |d, span| {
        d.with_label(DiagnosticLabel::new(
            *span,
            format!("bounds it {bounded} only"),
        ))
    })
}

/// Post-compilation pass: refuse every strict auto the module provably bounds
/// on one side only (#6692).
///
/// **Ordering.** Must run after `phase_sub_override_autos` /
/// `phase_connect_auto_params`, which mint the sub-override autos it judges,
/// and after `phase_purposes`, whose output `purposes` is.
pub(crate) fn phase_one_sided_auto_check(
    ctx: &mut CompilationCtx,
    purposes: &[CompiledPurpose],
    prelude: &[&crate::CompiledModule],
) {
    let imported_names = imported_template_names(prelude);
    let findings = one_sided_auto_findings(&ctx.templates, purposes, &imported_names);
    ctx.diagnostics.extend(findings.iter().map(render));
}
