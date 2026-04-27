//! Shadowing lint (spec §8.5).
//!
//! Walks the parsed AST once and emits a Warning diagnostic
//! ([`DiagnosticCode::Shadowing`]) whenever a child-scope binder uses the same
//! name as a name visible from an enclosing parent scope.
//!
//! # Scope model
//!
//! For every top-level declaration that introduces a body scope (structure,
//! occurrence, trait, fn, constraint def, field, purpose) we build an initial
//! frame containing the body's locally-declared names (params, lets, ports,
//! subs, guarded-group members) along with the source span of the declaring
//! site. The frame stack is then threaded through the expression walker;
//! entering a `Lambda` or `Quantifier` pushes a new frame (the binder names),
//! recursing into the body sees both frames, and exiting pops the frame.
//!
//! # Exclusion rules (spec §6.4, §8.8, §8.11)
//!
//! - **Imports do not enter any frame.** `Declaration::Import(_)` is matched
//!   explicitly and passes through without contributing to upward visibility.
//! - **Trait-merged members are not visited.** The lint walks ONLY the
//!   structure's own member list — trait member sets are never folded in,
//!   so a structure that declares one `param mass` to satisfy two traits
//!   has exactly one `mass` entry in its frame and produces no shadow.
//! - **GuardedGroup members are siblings, not children.** `where { … } else
//!   { … }` registers all branch members into the SAME parent frame
//!   (mutually-exclusive siblings per match-arm desugaring), not as a child
//!   scope. Same-name entries across the two branches overwrite in the frame
//!   without producing a shadow.

use std::collections::HashMap;

use reify_syntax::{Expr, ExprKind, ParsedModule};
use reify_types::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Stack-safety bound on the structural recursion in [`walk_expr_depth`].
///
/// Mirrors the convention in `dot_chain_lint.rs`: 256 is generous (typical
/// hand-written code never exceeds ~20). Exceeding the bound is treated as a
/// "should never happen" invariant violation; debug builds panic for fuzzer
/// visibility, release builds silently truncate so end-users never see an
/// unactionable diagnostic.
const MAX_EXPR_DEPTH: usize = 256;

/// A single lexical scope frame: names declared in this scope mapped to their
/// declaration spans.
type Frame = HashMap<String, SourceSpan>;

/// Walk every top-level declaration in `parsed` and emit a Warning for each
/// shadowing binder discovered.
///
/// Pushed diagnostics use [`reify_types::DiagnosticCode::Shadowing`] with the
/// canonical message form `"declaration of '<name>' shadows enclosing
/// declaration"` and two labels (child binder site + original parent decl
/// site).
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for decl in &parsed.declarations {
        walk_declaration(decl, diagnostics);
    }
}

/// Build the initial body frame for a top-level declaration and walk every
/// expression position with that frame as the only ancestor.
fn walk_declaration(decl: &reify_syntax::Declaration, diagnostics: &mut Vec<Diagnostic>) {
    use reify_syntax::Declaration;
    match decl {
        Declaration::Structure(s) => {
            // Spec §8.8 trait-merge exclusion: walk ONLY the structure's own
            // `s.members`. Trait member sets are never injected into the frame,
            // so a structure that declares one member to satisfy multiple
            // trait requirements has exactly one entry in its frame — no
            // false-positive shadow. Exclusion is automatic by single-source
            // iteration; no explicit filter is required.
            let frame = collect_body_frame(&s.members);
            let frames: Vec<&Frame> = vec![&frame];
            walk_members(&s.members, &frames, diagnostics);
        }
        Declaration::Occurrence(o) => {
            // Same single-source-iteration rule as Structure (§8.8): we never
            // visit trait member sets, only the occurrence's own members.
            let frame = collect_body_frame(&o.members);
            let frames: Vec<&Frame> = vec![&frame];
            walk_members(&o.members, &frames, diagnostics);
        }
        // Imports do NOT participate in upward visibility per spec §8.11.
        // Match the variant explicitly and pass through: no frame is built,
        // no module-scope frame aggregates imports, and `walk_declaration`
        // does not extract names from `ImportDecl`. A `let` (or any later
        // decl) with the same name as an imported symbol therefore CANNOT
        // be flagged as shadowing the import — the import simply does not
        // exist as far as this lint is concerned.
        Declaration::Import(_) => {}
        // The remaining declaration arms are wired in subsequent steps
        // (functions, constraint defs, traits, fields, purposes). Until then
        // they pass through without forming a frame, matching the lint's
        // "no frame ⇒ no shadow" invariant.
        _ => {}
    }
}

/// Build a frame from a member list: params, lets, ports (by port name),
/// subs, and guarded-group members (siblings) all merge into the SAME frame.
///
/// Spec §6.4: `where { … } else { … }` branches register members into the
/// same parent scope as siblings under mutually-exclusive guards — they are
/// NOT a child scope and MUST NOT shadow each other. We therefore fold both
/// branches into the same frame (silently overwriting on duplicate names —
/// duplicate detection is the existing duplicate-decl error path's job, not
/// this lint's).
///
/// Spec §8.8 (trait-merge exclusion): we walk ONLY the supplied `members`
/// list; trait member sets are never injected here, so a structure that
/// declares one member satisfying two traits has a single frame entry —
/// no false-positive shadow.
fn collect_body_frame(members: &[reify_syntax::MemberDecl]) -> Frame {
    let mut frame: Frame = HashMap::new();
    collect_body_frame_into(members, &mut frame, 0);
    frame
}

fn collect_body_frame_into(
    members: &[reify_syntax::MemberDecl],
    frame: &mut Frame,
    depth: usize,
) {
    use reify_syntax::MemberDecl;
    if depth > reify_syntax::MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) => {
                frame.insert(p.name.clone(), p.span);
            }
            MemberDecl::Let(l) => {
                frame.insert(l.name.clone(), l.span);
            }
            MemberDecl::Sub(s) => {
                frame.insert(s.name.clone(), s.span);
            }
            MemberDecl::Port(p) => {
                frame.insert(p.name.clone(), p.span);
                // Port-internal members live in the port's own scope, not the
                // enclosing entity's scope, so we do NOT fold them upward.
            }
            MemberDecl::GuardedGroup(g) => {
                // Both branches register into the SAME parent frame as
                // siblings (spec §6.4 — match-arm-style guarded decls
                // are mutually-exclusive siblings, NOT a child scope).
                // Same-name decls across the two branches silently
                // overwrite in the frame; we do NOT flag intra-frame
                // duplicates here — those belong to the existing
                // duplicate-decl error path. Recurse so nested groups
                // also fold into the same parent frame.
                collect_body_frame_into(&g.members, frame, depth + 1);
                collect_body_frame_into(&g.else_members, frame, depth + 1);
            }
            // The remaining variants do not contribute named binders to the
            // body's name-resolution scope.
            MemberDecl::Constraint(_)
            | MemberDecl::ConstraintInst(_)
            | MemberDecl::Minimize(_)
            | MemberDecl::Maximize(_)
            | MemberDecl::AssociatedType(_)
            | MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_) => {}
        }
    }
}

/// Recurse through a member list, walking every expression-bearing position
/// against the supplied frame stack.
fn walk_members(
    members: &[reify_syntax::MemberDecl],
    frames: &[&Frame],
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_members_depth(members, frames, diagnostics, 0);
}

fn walk_members_depth(
    members: &[reify_syntax::MemberDecl],
    frames: &[&Frame],
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    use reify_syntax::MemberDecl;
    if depth > reify_syntax::MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) => {
                if let Some(default) = &p.default {
                    walk_expr(default, frames, diagnostics);
                }
                if let Some(wc) = &p.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Let(l) => {
                walk_expr(&l.value, frames, diagnostics);
                if let Some(wc) = &l.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Constraint(c) => {
                walk_expr(&c.expr, frames, diagnostics);
                if let Some(wc) = &c.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::ConstraintInst(c) => {
                for (_, expr) in &c.args {
                    walk_expr(expr, frames, diagnostics);
                }
                if let Some(wc) = &c.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Sub(s) => {
                for (_, expr) in &s.args {
                    walk_expr(expr, frames, diagnostics);
                }
                if let Some(wc) = &s.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Minimize(m) => {
                walk_expr(&m.expr, frames, diagnostics);
                if let Some(wc) = &m.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Maximize(m) => {
                walk_expr(&m.expr, frames, diagnostics);
                if let Some(wc) = &m.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::GuardedGroup(g) => {
                walk_expr(&g.condition, frames, diagnostics);
                walk_members_depth(&g.members, frames, diagnostics, depth + 1);
                walk_members_depth(&g.else_members, frames, diagnostics, depth + 1);
            }
            MemberDecl::Port(p) => {
                if let Some(frame) = &p.frame_expr {
                    walk_expr(frame, frames, diagnostics);
                }
                walk_members_depth(&p.members, frames, diagnostics, depth + 1);
            }
            MemberDecl::Connect(c) => {
                walk_expr(&c.left.expr, frames, diagnostics);
                walk_expr(&c.right.expr, frames, diagnostics);
                for (_, expr) in &c.params {
                    walk_expr(expr, frames, diagnostics);
                }
            }
            MemberDecl::Chain(c) => {
                for elem in &c.elements {
                    walk_expr(elem, frames, diagnostics);
                }
            }
            MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => {}
        }
    }
}

/// Walk a single expression, detecting shadowing at lambda/quantifier sites.
fn walk_expr(expr: &Expr, frames: &[&Frame], diagnostics: &mut Vec<Diagnostic>) {
    walk_expr_depth(expr, frames, diagnostics, 0);
}

fn walk_expr_depth(
    expr: &Expr,
    frames: &[&Frame],
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    if depth > MAX_EXPR_DEPTH {
        debug_assert!(
            false,
            "shadow_lint walk_expr_depth exceeded MAX_EXPR_DEPTH = {} (depth = {}); \
             shadow lint coverage truncated at this subtree — likely upstream parser \
             bug or fuzzer input producing pathologically deep AST",
            MAX_EXPR_DEPTH,
            depth
        );
        return;
    }
    let next = depth + 1;
    match &expr.kind {
        ExprKind::Lambda { params, body } => {
            // Build the lambda's own frame and emit a warning for each param
            // that shadows a name from an enclosing frame.
            let mut child: Frame = HashMap::new();
            for p in params {
                if let Some(parent_span) = lookup_in_stack(frames, &p.name) {
                    push_shadow_diagnostic(diagnostics, &p.name, p.span, parent_span);
                }
                child.insert(p.name.clone(), p.span);
            }
            let mut next_frames: Vec<&Frame> = frames.to_vec();
            next_frames.push(&child);
            walk_expr_depth(body, &next_frames, diagnostics, next);
        }
        ExprKind::Quantifier {
            variable,
            collection,
            predicate,
            ..
        } => {
            // The collection is evaluated in the OUTER scope (the variable is
            // not yet bound). The predicate sees the variable.
            walk_expr_depth(collection, frames, diagnostics, next);
            if let Some(parent_span) = lookup_in_stack(frames, variable) {
                push_shadow_diagnostic(diagnostics, variable, expr.span, parent_span);
            }
            let mut child: Frame = HashMap::new();
            child.insert(variable.clone(), expr.span);
            let mut next_frames: Vec<&Frame> = frames.to_vec();
            next_frames.push(&child);
            walk_expr_depth(predicate, &next_frames, diagnostics, next);
        }
        ExprKind::BinOp { left, right, .. } => {
            walk_expr_depth(left, frames, diagnostics, next);
            walk_expr_depth(right, frames, diagnostics, next);
        }
        ExprKind::UnOp { operand, .. } => walk_expr_depth(operand, frames, diagnostics, next),
        ExprKind::FunctionCall { args, .. } => {
            for a in args {
                walk_expr_depth(a, frames, diagnostics, next);
            }
        }
        ExprKind::MemberAccess { object, .. } => {
            walk_expr_depth(object, frames, diagnostics, next);
        }
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr_depth(condition, frames, diagnostics, next);
            walk_expr_depth(then_branch, frames, diagnostics, next);
            walk_expr_depth(else_branch, frames, diagnostics, next);
        }
        ExprKind::ListLiteral(elems) | ExprKind::SetLiteral(elems) => {
            for e in elems {
                walk_expr_depth(e, frames, diagnostics, next);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                walk_expr_depth(k, frames, diagnostics, next);
                walk_expr_depth(v, frames, diagnostics, next);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            walk_expr_depth(object, frames, diagnostics, next);
            walk_expr_depth(index, frames, diagnostics, next);
        }
        ExprKind::Match { discriminant, arms } => {
            walk_expr_depth(discriminant, frames, diagnostics, next);
            for arm in arms {
                walk_expr_depth(&arm.body, frames, diagnostics, next);
            }
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            walk_expr_depth(base, frames, diagnostics, next);
            for a in args {
                walk_expr_depth(a, frames, diagnostics, next);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => {
            walk_expr_depth(qualifier, frames, diagnostics, next);
        }
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            walk_expr_depth(object, frames, diagnostics, next);
            walk_expr_depth(qualified, frames, diagnostics, next);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                walk_expr_depth(l, frames, diagnostics, next);
            }
            if let Some(u) = upper {
                walk_expr_depth(u, frames, diagnostics, next);
            }
        }
        // Leaf expressions — no children.
        ExprKind::NumberLiteral(_)
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. } => {}
    }
}

/// Walk the frame stack from innermost to outermost, returning the first
/// matching parent decl span. Implements the "nearest visible parent" rule.
fn lookup_in_stack(frames: &[&Frame], name: &str) -> Option<SourceSpan> {
    for frame in frames.iter().rev() {
        if let Some(span) = frame.get(name) {
            return Some(*span);
        }
    }
    None
}

/// Push a single Shadowing warning with the canonical message, code, and
/// two labels (child site + original decl site).
fn push_shadow_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    child_span: SourceSpan,
    parent_span: SourceSpan,
) {
    diagnostics.push(
        Diagnostic::warning(format!(
            "declaration of '{name}' shadows enclosing declaration"
        ))
        .with_code(DiagnosticCode::Shadowing)
        .with_label(DiagnosticLabel::new(
            child_span,
            "shadows the enclosing declaration",
        ))
        .with_label(DiagnosticLabel::new(parent_span, "originally declared here")),
    );
}
