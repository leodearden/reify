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
//! site. The frame stack is then threaded through the expression walker as a
//! [`FrameStack`] linked list (one node per scope, each living on the call
//! stack); entering a `Lambda` or `Quantifier` constructs a new node referencing
//! the binder names with `parent` set to the current stack, recursing into the
//! body sees both frames, and the new node drops automatically when the match
//! arm exits.
//!
//! Function and purpose bodies are treated as a CHILD scope of their params:
//! a body let-binding (or sub/port/etc.) that re-uses a param name shadows the
//! param and emits a Warning. (Inside a structure or trait body, params and
//! lets are siblings in the same scope — collisions belong to the duplicate-
//! decl error path, not this lint.)
//!
//! Port-internal members are treated as a CHILD scope of the enclosing entity
//! body: lambda params inside a port's `let f = …` see the port's own param
//! and let names as a parent scope. (Without this, a `port p { param q ; let f
//! = |q| q }` would not detect the lambda's `q` shadow.)
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
//!   scope. Same-name entries across the two branches use first-seen semantics:
//!   the THEN-branch occurrence wins; the ELSE-branch entry is a no-op.

use std::collections::HashMap;

use reify_ast::{Expr, ExprKind, ParsedModule};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

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

/// A linked-list-of-stack-frames lexical scope stack.
///
/// Each [`FrameStack`] node lives on the call stack: pushing a new scope is
/// `let new = FrameStack { frame: &child, parent: outer }` and passing
/// `Some(&new)` into the recursive call. Popping is automatic when the new
/// node drops at the end of the enclosing block.
///
/// This avoids the per-recursion `frames.to_vec()` allocation that the prior
/// `&[&Frame]` design incurred — important for the lambda/quantifier hot path
/// (every nested binder paid an O(depth) heap allocation, even though the
/// stack only grew by one entry).
struct FrameStack<'a> {
    frame: &'a Frame,
    parent: Option<&'a FrameStack<'a>>,
}

impl<'a> FrameStack<'a> {
    /// Walk the stack from innermost to outermost, returning the first
    /// matching parent decl span. Implements the "nearest visible parent"
    /// rule used by [`push_shadow_diagnostic`] sites.
    fn lookup(&self, name: &str) -> Option<SourceSpan> {
        if let Some(span) = self.frame.get(name) {
            return Some(*span);
        }
        self.parent.and_then(|p| p.lookup(name))
    }
}

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
fn walk_declaration(decl: &reify_ast::Declaration, diagnostics: &mut Vec<Diagnostic>) {
    use reify_ast::Declaration;
    match decl {
        Declaration::Structure(s) => {
            // Spec §8.8 trait-merge exclusion: walk ONLY the structure's own
            // `s.members`. Trait member sets are never injected into the frame,
            // so a structure that declares one member to satisfy multiple
            // trait requirements has exactly one entry in its frame — no
            // false-positive shadow. Exclusion is automatic by single-source
            // iteration; no explicit filter is required.
            let frame = collect_body_frame(&s.members);
            let stack = FrameStack {
                frame: &frame,
                parent: None,
            };
            walk_members(&s.members, Some(&stack), diagnostics);
        }
        Declaration::Occurrence(o) => {
            // Same single-source-iteration rule as Structure (§8.8): we never
            // visit trait member sets, only the occurrence's own members.
            let frame = collect_body_frame(&o.members);
            let stack = FrameStack {
                frame: &frame,
                parent: None,
            };
            walk_members(&o.members, Some(&stack), diagnostics);
        }
        // Imports do NOT participate in upward visibility per spec §8.11.
        // Match the variant explicitly and pass through: no frame is built,
        // no module-scope frame aggregates imports, and `walk_declaration`
        // does not extract names from `ImportDecl`. A `let` (or any later
        // decl) with the same name as an imported symbol therefore CANNOT
        // be flagged as shadowing the import — the import simply does not
        // exist as far as this lint is concerned.
        Declaration::Import(_) => {}
        Declaration::Function(f) => {
            // The fn body is a CHILD scope of the params: a body `let` whose
            // name matches a fn-param is a shadow per spec §8.5. Delegate
            // the params/body-frame scaffolding to `walk_child_scope_body`.
            // Top-level fns always have Some body; bodyless trait fns (None)
            // are deferred to task δ and have nothing to shadow-check.
            if let Some(body) = &f.body {
                walk_child_scope_body(
                    f.params.iter().map(|p| (p.name.clone(), p.span)),
                    body.let_bindings.iter().map(|l| (l.name.clone(), l.span)),
                    |body_stack, diagnostics| {
                        for l in &body.let_bindings {
                            walk_expr(&l.value, Some(body_stack), diagnostics);
                            if let Some(wc) = &l.where_clause {
                                walk_expr(&wc.condition, Some(body_stack), diagnostics);
                            }
                        }
                        walk_expr(&body.result_expr, Some(body_stack), diagnostics);
                    },
                    diagnostics,
                );
            }
        }
        Declaration::Constraint(cd) => {
            // Build a frame from the constraint def's params and walk every
            // predicate expression and every param default against it. The
            // constraint def has no separate body-scope (predicates are bare
            // expressions, not let-bindings), so a single frame is correct.
            let mut frame: Frame = HashMap::new();
            for p in &cd.params {
                frame.insert(p.name.clone(), p.span);
            }
            let stack = FrameStack {
                frame: &frame,
                parent: None,
            };
            for p in &cd.params {
                if let Some(default) = &p.default {
                    walk_expr(default, Some(&stack), diagnostics);
                }
                if let Some(wc) = &p.where_clause {
                    walk_expr(&wc.condition, Some(&stack), diagnostics);
                }
            }
            for predicate in &cd.predicates {
                walk_expr(predicate, Some(&stack), diagnostics);
            }
        }
        Declaration::Trait(t) => {
            // Build the trait's body frame from its `members` (params, lets,
            // sub-decls, ports, guarded-group members; same shape as the
            // entity body via `collect_body_frame`) and walk every embedded
            // expression (param defaults, let values, constraint expressions,
            // etc.) against that frame.
            //
            // The trait's `refinements` (super-traits) are NOT folded in —
            // upstream trait member sets do not contribute to this trait's
            // own lexical scope. This mirrors the structure-side §8.8
            // single-source iteration rule applied to trait merging.
            let frame = collect_body_frame(&t.members);
            let stack = FrameStack {
                frame: &frame,
                parent: None,
            };
            walk_members(&t.members, Some(&stack), diagnostics);
        }
        Declaration::Field(f) => {
            // Fields have no body params at the top level — the lambda
            // inside `analytical { |p| … }` (or `composed { … }`) introduces
            // its own scope, naturally caught by `walk_expr`'s Lambda
            // handling. We therefore walk the source expression with an
            // EMPTY top-level frame and let the lambda push add the domain
            // binders; any inner lambda binding the same name then shadows
            // against that pushed frame.
            let frame: Frame = HashMap::new();
            let stack = FrameStack {
                frame: &frame,
                parent: None,
            };
            match &f.source {
                reify_ast::FieldSource::Analytical { expr }
                | reify_ast::FieldSource::Composed { expr } => {
                    walk_expr(expr, Some(&stack), diagnostics);
                }
                reify_ast::FieldSource::Sampled { config } => {
                    for (_cfg_name, value) in config {
                        // _cfg_name is a sampled-config key (e.g. "resolution"),
                        // not a binder.
                        walk_expr(value, Some(&stack), diagnostics);
                    }
                }
                reify_ast::FieldSource::Imported { .. } => {}
            }
        }
        Declaration::Purpose(p) => {
            // The purpose body is a CHILD scope of the params: a body decl
            // whose name matches a purpose-param is a shadow per spec §8.5.
            // Delegate the params/body-frame scaffolding to
            // `walk_child_scope_body`. The HashMap iterator from
            // `collect_body_frame` is forwarded directly — iteration order is
            // already nondeterministic in the original code (design decision
            // #3, task 2499), so no sort is introduced here.
            walk_child_scope_body(
                p.params.iter().map(|pp| (pp.name.clone(), pp.span)),
                collect_body_frame(&p.members),
                |body_stack, diagnostics| {
                    walk_members(&p.members, Some(body_stack), diagnostics);
                },
                diagnostics,
            );
        }
        // The remaining declaration arms (Enum, Unit, TypeAlias) do not
        // introduce expression-bearing scopes that the lint needs to walk;
        // they pass through without forming a frame.
        _ => {}
    }
}

/// Build a frame from a member list: params, lets, ports (by port name),
/// subs, and guarded-group members (siblings) all merge into the SAME frame.
///
/// Spec §6.4: `where { … } else { … }` branches register members into the
/// same parent scope as siblings under mutually-exclusive guards — they are
/// NOT a child scope and MUST NOT shadow each other. We therefore fold both
/// branches into the same frame using first-seen semantics (`entry().or_insert`):
/// the THEN-branch occurrence wins; the ELSE-branch entry is a no-op for any
/// name already present. Duplicate-decl detection remains the existing
/// duplicate-decl error path's responsibility, not this lint's.
///
/// Spec §8.8 (trait-merge exclusion): we walk ONLY the supplied `members`
/// list; trait member sets are never injected here, so a structure that
/// declares one member satisfying two traits has a single frame entry —
/// no false-positive shadow.
fn collect_body_frame(members: &[reify_ast::MemberDecl]) -> Frame {
    let mut frame: Frame = HashMap::new();
    collect_body_frame_into(members, &mut frame, 0);
    frame
}

fn collect_body_frame_into(members: &[reify_ast::MemberDecl], frame: &mut Frame, depth: usize) {
    use reify_ast::MemberDecl;
    if depth > reify_ast::MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) => {
                frame.entry(p.name.clone()).or_insert(p.span);
            }
            MemberDecl::Let(l) => {
                frame.entry(l.name.clone()).or_insert(l.span);
            }
            MemberDecl::Sub(s) => {
                frame.entry(s.name.clone()).or_insert(s.span);
            }
            MemberDecl::Port(p) => {
                frame.entry(p.name.clone()).or_insert(p.span);
                // Port-internal members live in the port's own scope, not the
                // enclosing entity's scope, so we do NOT fold them upward.
                // The `walk_members_depth` arm for Port pushes a port-internal
                // frame onto the stack before recursing, so lambda params
                // inside a port member still see port-internal binders as a
                // parent scope.
            }
            MemberDecl::GuardedGroup(g) => {
                // Both branches register into the SAME parent frame as
                // siblings (spec §6.4 — match-arm-style guarded decls
                // are mutually-exclusive siblings, NOT a child scope).
                // First-seen semantics (`entry().or_insert`) ensure the
                // THEN-branch occurrence wins: the ELSE-branch recursive
                // call is a no-op for any name already present in the
                // frame. We do NOT flag intra-frame duplicates here —
                // those belong to the existing duplicate-decl error path.
                // Recurse so nested groups also fold into the same parent
                // frame.
                collect_body_frame_into(&g.members, frame, depth + 1);
                collect_body_frame_into(&g.else_members, frame, depth + 1);
            }
            // The remaining variants do not contribute named binders to the
            // enclosing body's name-resolution scope.
            //
            // For `ForallConnect`/`ForallConstraint`, the bound variable IS a
            // binder — but it is scoped to the forall body ONLY (a child frame
            // built at the walk site in `walk_members_depth`), so it must NOT
            // leak into this parent frame. They are folded into this group
            // because the parent-frame contribution is identical (none); the
            // child-frame construction lives in `walk_members_depth`.
            //
            // A future refactor that mistakenly inserts the forall variable
            // into the parent frame here would silently broaden its lexical
            // visibility — any maintainer touching this match must preserve
            // the no-op behavior.
            MemberDecl::Constraint(_)
            | MemberDecl::ConstraintInst(_)
            | MemberDecl::Minimize(_)
            | MemberDecl::Maximize(_)
            | MemberDecl::AssociatedType(_)
            // Trait fn members do not contribute named binders to the enclosing
            // body scope.  Fn compilation is deferred to task δ/ζ.
            | MemberDecl::Fn(_)
            | MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::ForallConnect(_)
            | MemberDecl::ForallConstraint(_)
            | MemberDecl::MatchArmDeclGroup(_) => {}
        }
    }
}

/// Recurse through a member list, walking every expression-bearing position
/// against the supplied frame stack.
fn walk_members(
    members: &[reify_ast::MemberDecl],
    frames: Option<&FrameStack>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    walk_members_depth(members, frames, diagnostics, 0);
}

fn walk_members_depth(
    members: &[reify_ast::MemberDecl],
    frames: Option<&FrameStack>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    use reify_ast::MemberDecl;
    if depth > reify_ast::MAX_MEMBER_NESTING_DEPTH {
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
                for (_arg_name, expr) in &c.args {
                    // _arg_name belongs to the callee's parameter scope (the
                    // referenced constraint def's param list); it is NOT a
                    // binder in this scope. The argument expression IS
                    // evaluated in this scope and must be walked.
                    walk_expr(expr, frames, diagnostics);
                }
                if let Some(wc) = &c.where_clause {
                    walk_expr(&wc.condition, frames, diagnostics);
                }
            }
            MemberDecl::Sub(s) => {
                for (_arg_name, expr) in &s.args {
                    // _arg_name belongs to the callee's parameter scope (the
                    // referenced structure's param list); it is NOT a binder
                    // in this scope.
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
                if let Some(frame_expr) = &p.frame_expr {
                    walk_expr(frame_expr, frames, diagnostics);
                }
                // Port has its own scope — build a port-internal frame from
                // the port's own members and push it onto the stack before
                // recursing, so lambda params inside a port member see the
                // port-internal binders as a parent scope. (Without this,
                // `port p { param q ; let f = |q| q }` would not detect the
                // inner-lambda shadow.)
                let port_frame = collect_body_frame(&p.members);
                let port_stack = FrameStack {
                    frame: &port_frame,
                    parent: frames,
                };
                walk_members_depth(&p.members, Some(&port_stack), diagnostics, depth + 1);
            }
            MemberDecl::Connect(c) => {
                walk_expr(&c.left.expr, frames, diagnostics);
                walk_expr(&c.right.expr, frames, diagnostics);
                for (_arg_name, expr) in &c.params {
                    // _arg_name belongs to the callee's parameter scope; no
                    // binding is introduced here.
                    walk_expr(expr, frames, diagnostics);
                }
            }
            MemberDecl::Chain(c) => {
                for elem in &c.elements {
                    walk_expr(elem, frames, diagnostics);
                }
            }
            MemberDecl::ForallConnect(f) => {
                walk_forall_binder(
                    &f.variable,
                    f.span,
                    &f.collection,
                    frames,
                    diagnostics,
                    |frames, diagnostics| {
                        super::forall_walk::walk_forall_connect_body(&f.body, |expr| {
                            walk_expr(expr, frames, diagnostics);
                        });
                    },
                );
            }
            MemberDecl::ForallConstraint(f) => {
                walk_forall_binder(
                    &f.variable,
                    f.span,
                    &f.collection,
                    frames,
                    diagnostics,
                    |frames, diagnostics| {
                        super::forall_walk::walk_forall_constraint_body(&f.body, |expr| {
                            walk_expr(expr, frames, diagnostics);
                        });
                    },
                );
            }
            MemberDecl::AssociatedType(_)
            // Trait fn members: no expressions to walk for shadow lint at γ.
            // Fn compilation is deferred to task δ/ζ.
            // TODO(task δ/ζ): add shadow-lint walking for trait fn body
            // expressions (let-bindings, where-clauses, result expr) once
            // trait-fn compilation is live.
            | MemberDecl::Fn(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::MatchArmDeclGroup(_) => {}
        }
    }
}

/// Walk a single expression, detecting shadowing at lambda/quantifier sites.
fn walk_expr(expr: &Expr, frames: Option<&FrameStack>, diagnostics: &mut Vec<Diagnostic>) {
    walk_expr_depth(expr, frames, diagnostics, 0);
}

fn walk_expr_depth(
    expr: &Expr,
    frames: Option<&FrameStack>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) {
    if depth > MAX_EXPR_DEPTH {
        debug_assert!(
            false,
            "shadow_lint walk_expr_depth exceeded MAX_EXPR_DEPTH = {} (depth = {}); \
             shadow lint coverage truncated at this subtree — likely upstream parser \
             bug or fuzzer input producing pathologically deep AST",
            MAX_EXPR_DEPTH, depth
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
                if let Some(parent_span) = frames.and_then(|f| f.lookup(&p.name)) {
                    push_shadow_diagnostic(diagnostics, &p.name, p.span, parent_span);
                }
                child.insert(p.name.clone(), p.span);
            }
            let new_stack = FrameStack {
                frame: &child,
                parent: frames,
            };
            walk_expr_depth(body, Some(&new_stack), diagnostics, next);
            // `new_stack` drops here; the parent stack frame is restored
            // automatically without an explicit pop.
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
            if let Some(parent_span) = frames.and_then(|f| f.lookup(variable)) {
                push_shadow_diagnostic(diagnostics, variable, expr.span, parent_span);
            }
            // TODO(suggestion #3): once `reify_syntax::ExprKind::Quantifier`
            // carries a separate `variable_span` field, replace `expr.span`
            // here with that span so editor squigglies highlight only the
            // bound variable rather than the entire `forall x in coll: pred`
            // expression. The AST extension is a one-line addition in
            // `crates/reify-syntax/src/lib.rs`; this lint emits a wider-than-
            // ideal child label until that lands.
            let mut child: Frame = HashMap::new();
            child.insert(variable.clone(), expr.span);
            let new_stack = FrameStack {
                frame: &child,
                parent: frames,
            };
            walk_expr_depth(predicate, Some(&new_stack), diagnostics, next);
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
        ExprKind::TraitMethodCall { object, args, .. } => {
            walk_expr_depth(object, frames, diagnostics, next);
            for a in args {
                walk_expr_depth(a, frames, diagnostics, next);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for a in args {
                walk_expr_depth(a, frames, diagnostics, next);
            }
        }
        // VariantConstruct — recurse into field value exprs.
        // α binds no new names (no binders), so no shadow-frame is opened.
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, v) in fields {
                walk_expr_depth(v, frames, diagnostics, next);
            }
        }
        // InterpolatedString — recurse into each Hole expr; Literal parts are leaves.
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let reify_ast::StringPart::Hole(e) = part {
                    walk_expr_depth(e, frames, diagnostics, next);
                }
            }
        }
        // Leaf expressions — no children.
        ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. } => {}
    }
}

/// Common scope-management for the `Declaration::Function` and
/// `Declaration::Purpose` arms of [`walk_declaration`] (spec §8.5):
///
/// 1. Build a **params frame** from `params` and wrap it in a [`FrameStack`]
///    with no parent.
/// 2. Iterate `body_names`; for each `(name, span)` that collides with a name
///    visible from `params_stack`, emit a [`push_shadow_diagnostic`] warning
///    and insert the name into the **body frame** (so later body names can
///    shadow each other, but not the param they just shadowed — that first
///    shadow is already recorded).
/// 3. Build a `body_stack` with `parent = Some(&params_stack)` and invoke
///    `walk_body` with it so all expressions inside the body are walked under
///    both scopes.
///
/// Both iterators are generic (`impl IntoIterator`) so the Function call site
/// can forward its native `Vec`-backed `.iter().map(…)` and the Purpose call
/// site can forward the `HashMap`'s `.into_iter()` — no intermediate
/// allocations, and each arm's native iteration order is preserved (spec §8.5,
/// design decision #3 in task 2499).
///
/// Extracted to consolidate the duplicated fn/purpose-body scaffolding in a
/// single edit site, mirroring the [`walk_forall_binder`] pattern for
/// `ForallConnect`/`ForallConstraint`.
fn walk_child_scope_body(
    params: impl IntoIterator<Item = (String, SourceSpan)>,
    body_names: impl IntoIterator<Item = (String, SourceSpan)>,
    walk_body: impl FnOnce(&FrameStack, &mut Vec<Diagnostic>),
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Build params scope.
    let mut params_frame: Frame = HashMap::new();
    for (name, span) in params {
        params_frame.insert(name, span);
    }
    let params_stack = FrameStack {
        frame: &params_frame,
        parent: None,
    };

    // Build body scope, emitting a Shadowing warning for each param collision.
    let mut body_frame: Frame = HashMap::new();
    for (name, span) in body_names {
        if let Some(parent_span) = params_stack.lookup(&name) {
            push_shadow_diagnostic(diagnostics, &name, span, parent_span);
        }
        body_frame.insert(name, span);
    }
    let body_stack = FrameStack {
        frame: &body_frame,
        parent: Some(&params_stack),
    };

    walk_body(&body_stack, diagnostics);
}

/// Common scope-management for both `MemberDecl::ForallConnect` and
/// `MemberDecl::ForallConstraint`: walk the collection in the parent scope,
/// emit a Shadowing warning if the bound variable shadows an enclosing decl,
/// then construct a one-element child frame and invoke `body_walker` with the
/// new `FrameStack` so the body's expressions are walked under the bound
/// variable.
///
/// Mirrors the `ExprKind::Quantifier` arm in [`walk_expr_depth`] (see lines
/// 593-619): the collection is evaluated in the outer scope (the variable is
/// not yet bound) and the body sees the variable. The `variable_span` is the
/// child-side label of the Shadowing diagnostic — currently `f.span` (the
/// outer ForallXDecl span) for both variants, matching the Quantifier arm's
/// use of `expr.span` and the TODO at lines 605-611 proposing to migrate both
/// forms together once a separate `variable_span` field lands on the AST
/// nodes.
///
/// Extracted to consolidate the shadow-detection / child-frame logic in one
/// place so a future body variant addition (or migration to a narrower
/// `variable_span`) is a single edit site rather than two parallel ones.
fn walk_forall_binder(
    variable: &str,
    variable_span: SourceSpan,
    collection: &Expr,
    frames: Option<&FrameStack>,
    diagnostics: &mut Vec<Diagnostic>,
    body_walker: impl FnOnce(Option<&FrameStack>, &mut Vec<Diagnostic>),
) {
    // Collection is evaluated in the OUTER scope (variable not yet bound).
    walk_expr(collection, frames, diagnostics);
    // Shadow check before pushing the child frame, so the parent_span we
    // resolve is the enclosing decl rather than the variable itself.
    if let Some(parent_span) = frames.and_then(|fr| fr.lookup(variable)) {
        push_shadow_diagnostic(diagnostics, variable, variable_span, parent_span);
    }
    // Push a one-element child frame and walk the body under it. The new
    // stack node lives on this call's stack frame and drops automatically
    // when `body_walker` returns — same lifetime pattern as Lambda/Quantifier.
    let mut child: Frame = HashMap::new();
    child.insert(variable.to_string(), variable_span);
    let forall_stack = FrameStack {
        frame: &child,
        parent: frames,
    };
    body_walker(Some(&forall_stack), diagnostics);
}

/// Push a single Shadowing warning with the canonical message, code, and
/// two labels (child site + original decl site).
///
/// # Wording pin
///
/// The three message strings emitted here are pinned by
/// `shadow_diagnostic_message_format_is_pinned` in
/// `crates/reify-compiler/tests/shadowing_warning_tests.rs`. They are also
/// documented on the `DiagnosticCode::Shadowing` variant in
/// `crates/reify-types/src/diagnostics.rs` and in the PRD
/// (`docs/prds/shadowing-warning.md`). Any drift between the literals below
/// and the test's literal expectations will surface as a test failure — do
/// not change one without updating the other.
///
/// - Diagnostic message: `"declaration of '<name>' shadows enclosing declaration"`
/// - Child-site label: `"shadows the enclosing declaration"`
/// - Original-decl label: `"originally declared here"`
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
        .with_label(DiagnosticLabel::new(
            parent_span,
            "originally declared here",
        )),
    );
}
