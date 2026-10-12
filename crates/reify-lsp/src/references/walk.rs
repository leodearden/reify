//! The AST traversal skeleton shared by every reference walker: a member's
//! direct expressions, the nested member scopes it opens, and an expression's
//! direct sub-expressions. Imports `reify_ast` only.
//!
//! Field-set rationale is centralized in `sub_direct_exprs`'s doc comment —
//! see it for why, rather than restating it at each call site.

use reify_ast::{
    ConnectDecl, Expr, ExprKind, ForallConnectBody, ForallConstraintBody, KeyedSubMemberEntry,
    MemberDecl, StringPart, SubDecl, WhereClause,
};

/// Exhaustively enumerate every direct (non-nested-scope) expression carried
/// by a `SubDecl` — constructor args, specialization param overrides, the
/// `at` placement pose, the indexer clause's domain expression (task #5481/
/// #5579), the inline `at … where { }` relate-block relations (task δ 4384),
/// and the `where` guard condition.
///
/// Both [`for_each_member_direct_expr`] (the shared member-expression
/// enumeration every use walker composes with) and the `MemberDecl::Sub` arm of
/// `cursor_on_member_segment` (`.member`-segment refusal) consume this instead
/// of re-listing `SubDecl`'s fields themselves, so the two scanners
/// cannot drift apart the way they already have twice — `relate_relations`
/// (task δ 4384) and `index_domain` (#5481/#5579) were each added to
/// `collect_uses`' sub walk without a matching guard clause in
/// `cursor_on_member_segment`, silently letting a `.member` segment on that
/// field mis-resolve to an unrelated same-named binding instead of refusing.
///
/// The `let SubDecl { .. } = s` pattern below names every field with no `..`,
/// so adding a new field to `SubDecl` is a compile error here, forcing a
/// deliberate choice: fold it into the returned chain if it is a use-bearing
/// expression, or extend the `_`-bound list with a one-line reason if it
/// isn't — as `index_binder` already is (it mirrors the existing
/// `ExprKind::Quantifier` `variable`/`variable_span` precedent: a
/// binder-introducing field is deliberately not itself a use site here; see
/// the "Known limitation" doc comment on `collect_idents_in_expr`).
///
/// Excludes `body` and `keyed_members.overrides`: both consumers reach them
/// separately, via [`for_each_child_scope`], since they open a
/// nested member scope rather than being a direct expression of this `sub`.
/// Each keyed entry's OWN direct expression (`param_overrides`) is not a
/// `SubDecl`-level field, so it is not part of this iterator either — it is
/// walked by both consumers via [`keyed_entry_param_override_exprs`] at the
/// same keyed-block recursion site instead (see that function's doc comment).
pub(super) fn sub_direct_exprs(s: &SubDecl) -> impl Iterator<Item = &Expr> {
    let SubDecl {
        // Identifiers, flags, and span/cache metadata below — never expressions.
        name: _,
        structure_name: _,
        type_args: _,
        args,
        is_collection: _,
        // Guard condition folded directly into the chain below, unlike every
        // other member kind's `where` (which routes through
        // `visit_where_condition`) — see this function's doc comment.
        where_clause,
        // Nested member scope, not a direct expression — both consumers
        // recurse into it separately at depth + 1 (see doc comment above).
        body: _,
        spec_param_overrides,
        // Nested member scope, not a direct expression — both consumers
        // recurse into it separately at depth + 1, walking each entry's
        // `overrides` member list AND `param_overrides` expressions (see
        // doc comment above and `keyed_entry_param_override_exprs`).
        keyed_members: _,
        is_aux: _,
        is_priv: _,
        pose_expr,
        // Binder site, not a use — mirrors the existing `ExprKind::Quantifier`
        // `variable`/`variable_span` precedent (see doc comment above).
        index_binder: _,
        index_domain,
        relate_relations,
        // The DERIVED arm's clause — `sub b = mirror of a across P { … }`
        // (task #6615). Deliberately `_`-bound, not folded into the chain
        // below: its expression-bearing parts (the `SubDerivationKind`
        // transform operand, each `SubParamOverride` value) and its
        // `members` are reached only once derived subs elaborate, which is
        // A-beta (#6616)'s semantics to define — the same deferral, for the
        // same reason, that `SubDerivation::members`' doc comment records
        // against every member walker in reify-ast. A-beta must wire both
        // together rather than inherit either silently.
        derivation: _,
        span: _,
        content_hash: _,
    } = s;

    args.iter()
        .map(|(_, e)| e)
        .chain(spec_param_overrides.iter().map(|(_, e)| e))
        .chain(pose_expr.iter())
        .chain(index_domain.iter())
        .chain(relate_relations.iter())
        .chain(where_clause.iter().map(|w| &w.condition))
}

/// Exhaustively enumerate the direct expressions carried by a single keyed
/// entry (`"key" => { param_overrides… }`): its `param_overrides` values,
/// mirroring `SubDecl.spec_param_overrides` one level down (see
/// `KeyedSubMemberEntry.param_overrides`'s doc comment, task 3931 γ).
///
/// Field set centralized here for the same anti-drift reason as
/// `sub_direct_exprs` — see its doc comment. The exhaustive `let
/// KeyedSubMemberEntry { .. } = entry` below (no `..`) gives this the same
/// compile-error tripwire: a new expression-bearing field added to
/// `KeyedSubMemberEntry` cannot silently go unwalked by both consumers again
/// (task #5579 amendment — this is what closed that exact gap for
/// `param_overrides` itself).
pub(super) fn keyed_entry_param_override_exprs(entry: &KeyedSubMemberEntry) -> impl Iterator<Item = &Expr> {
    let KeyedSubMemberEntry {
        key: _,
        // Nested member scope, not a direct expression — both consumers
        // recurse into it separately as a nested member scope, exactly as
        // `sub_direct_exprs` excludes `SubDecl::body`.
        overrides: _,
        param_overrides,
        span: _,
    } = entry;
    param_overrides.iter().map(|(_, e)| e)
}

/// Invoke `visit` once for each nested member-list scope that lives directly
/// inside `member` — EXACTLY the member lists [`collect_uses`] descends into: a
/// `GuardedGroup`'s `where` and `else` branches, a `Port` body, a `Sub`'s
/// specialization body and each keyed block's overrides, a
/// `MatchArmDeclGroup`'s per-arm member clusters, and a `Sketch` body.
///
/// [`collect_bindings_in_scope`] uses this to recurse into nested bindings, while
/// `collect_uses` recurses into the same lists (plus their member-level
/// expressions) to collect uses. Keeping the two traversals reading off the same
/// child-scope set is what stops a nested redeclaration from being invisible to
/// binding resolution (the regression pinned by
/// `collect_references_port_body_redeclaration_owns_its_scope`). The callback form
/// avoids allocating a `Vec` of child slices per member; each visited slice is
/// bounded by its own [`members_region`] when recursed.
pub(super) fn for_each_child_scope(member: &MemberDecl, mut visit: impl FnMut(&[MemberDecl])) {
    match member {
        MemberDecl::GuardedGroup(g) => {
            visit(g.members.as_slice());
            visit(g.else_members.as_slice());
        }
        MemberDecl::Port(p) => visit(p.members.as_slice()),
        MemberDecl::Sub(s) => {
            if let Some(body) = &s.body {
                visit(body.as_slice());
            }
            for entry in &s.keyed_members {
                visit(entry.overrides.as_slice());
            }
        }
        MemberDecl::MatchArmDeclGroup(g) => {
            for arm in &g.arms {
                visit(std::slice::from_ref(&*arm.member));
            }
        }
        // A `sketch { … }` body is a nested scope (constrained-2d-sketch α, task
        // 5506). This arm is the single place both `collect_uses` and
        // `collect_bindings_in_scope` learn that; rustc does not flag its
        // absence (the `_ => {}` below swallows a new variant), and without it
        // every use inside a sketch body is invisible to references/rename.
        // Pinned by `collect_references_reach_into_a_sketch_body_which_owns_its_scope`.
        MemberDecl::Sketch(s) => visit(s.members.as_slice()),
        _ => {}
    }
}

/// Enumerate every DIRECT (non-nested-scope) expression carried by `member`, in
/// source order: param/let/constraint/objective expressions and their `where`
/// clauses, constraint-instantiation args, a sub's constructor args /
/// specialization overrides / pose / index domain / relate relations, a port's
/// placement frame, a guarded group's condition, bare connect/chain elements,
/// `forall` connect/constraint bodies, a match-arm cluster's discriminant, and a
/// member-level `relate` block's relations.
///
/// The member half of this module's two-part traversal skeleton: this function
/// yields a member's own expressions, [`for_each_child_scope`] yields the nested
/// member lists it opens, and each member walker here is one of the two composed
/// with a visitor. One enumeration rather than one per walker is what stops a
/// newly-added expression-bearing field from being walked by one scanner and
/// silently missed by another — the drift [`sub_direct_exprs`]' doc comment
/// records having already happened twice.
///
/// The match is WILDCARD-FREE, so a new `MemberDecl` variant is a compile error
/// here rather than a silently unwalked expression.
///
/// `MemberDecl::Fn` yields NOTHING. An associated function's body opens its own
/// parameter scope (its params can shadow an entity binding), which this
/// single-file value-reference foundation does not model, so collecting value
/// uses there could produce false positives (Invariant 1). The TYPE-position
/// walk does not inherit that restriction — a signature's types are not scoped
/// by the function's params — so [`collect_decl_name_uses_in_members`] reaches a
/// `Fn` through its own arm instead of through this enumeration.
///
/// The visitor is `&mut dyn FnMut` rather than a generic parameter because its
/// callers recurse through it; a generic would monomorphise without end.
pub(super) fn for_each_member_direct_expr(member: &MemberDecl, visit: &mut dyn FnMut(&Expr)) {
    match member {
        MemberDecl::Param(p) => {
            if let Some(default) = &p.default {
                visit(default);
            }
            visit_where_condition(&p.where_clause, visit);
        }
        MemberDecl::Let(l) => {
            visit(&l.value);
            visit_where_condition(&l.where_clause, visit);
        }
        MemberDecl::Constraint(c) => {
            visit(&c.expr);
            visit_where_condition(&c.where_clause, visit);
        }
        MemberDecl::ConstraintInst(c) => {
            for (_, arg) in &c.args {
                visit(arg);
            }
            visit_where_condition(&c.where_clause, visit);
        }
        // `body` and each keyed entry's `overrides` open nested member scopes and
        // so belong to `for_each_child_scope`, not here; `sub_direct_exprs`'
        // doc comment carries the field-set rationale for both halves.
        MemberDecl::Sub(s) => {
            for expr in sub_direct_exprs(s) {
                visit(expr);
            }
            for entry in &s.keyed_members {
                for expr in keyed_entry_param_override_exprs(entry) {
                    visit(expr);
                }
            }
        }
        MemberDecl::Minimize(m) => {
            visit(&m.expr);
            visit_where_condition(&m.where_clause, visit);
        }
        MemberDecl::Maximize(m) => {
            visit(&m.expr);
            visit_where_condition(&m.where_clause, visit);
        }
        // A guard condition is an OUTER-scope expression; the `where`/`else`
        // branches it guards are child scopes.
        MemberDecl::GuardedGroup(g) => visit(&g.condition),
        // A port's placement frame is a direct expression; its member body is a
        // child scope.
        MemberDecl::Port(p) => {
            if let Some(frame) = &p.frame_expr {
                visit(frame);
            }
        }
        MemberDecl::Connect(c) => visit_connect_exprs(c, visit),
        MemberDecl::Chain(c) => {
            for el in &c.elements {
                visit(el);
            }
        }
        MemberDecl::ForallConnect(f) => {
            visit(&f.collection);
            match &f.body {
                ForallConnectBody::Connect(c) => visit_connect_exprs(c, visit),
                ForallConnectBody::Chain(c) => {
                    for el in &c.elements {
                        visit(el);
                    }
                }
            }
        }
        MemberDecl::ForallConstraint(f) => {
            visit(&f.collection);
            match &f.body {
                ForallConstraintBody::Constraint(c) => {
                    visit(&c.expr);
                    visit_where_condition(&c.where_clause, visit);
                }
                ForallConstraintBody::Instantiation(c) => {
                    for (_, arg) in &c.args {
                        visit(arg);
                    }
                    visit_where_condition(&c.where_clause, visit);
                }
            }
        }
        // Match-arm decl clusters (spec §6.4): the discriminant is an
        // outer-scope expression; each arm's member is a child scope.
        MemberDecl::MatchArmDeclGroup(g) => visit(&g.discriminant),
        // A member-level `relate { … }` block (task δ 4384), mirroring Chain's
        // element walk.
        MemberDecl::Relate(r) => {
            for rel in &r.relations {
                visit(rel);
            }
        }
        // See the `MemberDecl::Fn` paragraph in this function's doc comment.
        MemberDecl::Fn(_) => {}
        // A `sketch { … }` block carries no expression of its own; its body is a
        // child scope, reached through `for_each_child_scope`.
        MemberDecl::Sketch(_) => {}
        // Type-only / expression-free members.
        MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => {}
    }
}

/// Visit an optional `where` clause's condition expression.
fn visit_where_condition(where_clause: &Option<WhereClause>, visit: &mut dyn FnMut(&Expr)) {
    if let Some(w) = where_clause {
        visit(&w.condition);
    }
}

/// Visit a `connect`/`chain` declaration's expressions: both port-ref endpoints
/// and every named connector parameter value.
fn visit_connect_exprs(c: &ConnectDecl, visit: &mut dyn FnMut(&Expr)) {
    visit(&c.left.expr);
    visit(&c.right.expr);
    for (_, param) in &c.params {
        visit(param);
    }
}

/// Enumerate every DIRECT sub-expression of `expr`, in source order.
///
/// The crate's single exhaustive `ExprKind` child enumeration, shared by the two
/// expression walkers that need it: [`collect_idents_in_expr`] (value uses) and
/// [`collect_type_name_uses_in_expr`] (type positions on lambda params). The
/// match is WILDCARD-FREE, so a new `ExprKind` variant is a compile error here
/// rather than a silently-dropped subtree in both walkers at once.
///
/// Only `Expr`-typed children are yielded. The non-expression parts an
/// expression carries — a member-access `.segment`, a qualified access's member,
/// a match arm's patterns, a lambda's parameter list, a quantifier's binder —
/// are not sub-expressions and are the visiting walker's own business (the
/// lambda parameter list is exactly why [`collect_type_name_uses_in_expr`]
/// inspects `expr` itself before delegating here).
///
/// The visitor is `&mut dyn FnMut` rather than a generic parameter because both
/// callers recurse through it; a generic would monomorphise without end.
pub(super) fn for_each_direct_subexpr(expr: &Expr, visit: &mut dyn FnMut(&Expr)) {
    match &expr.kind {
        ExprKind::BinOp { left, right, .. } => {
            visit(left);
            visit(right);
        }
        ExprKind::UnOp { operand, .. } => visit(operand),
        ExprKind::FunctionCall { args, .. } => {
            for arg in args {
                visit(arg);
            }
        }
        // The base of a member access (`h` in `h.diameter`) is an identifier use
        // of the sub/port/binding; the `.member` segment is a field name, not an
        // expression.
        ExprKind::MemberAccess { object, .. } => visit(object),
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            visit(condition);
            visit(then_branch);
            visit(else_branch);
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            for item in items {
                visit(item);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                visit(k);
                visit(v);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            visit(object);
            visit(index);
        }
        ExprKind::Match { discriminant, arms } => {
            visit(discriminant);
            for arm in arms {
                visit(&arm.body);
            }
        }
        ExprKind::Lambda { body, .. } => visit(body),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            visit(collection);
            visit(predicate);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            visit(base);
            for arg in args {
                visit(arg);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => visit(qualifier),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            visit(object);
            visit(qualified);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                visit(l);
            }
            if let Some(u) = upper {
                visit(u);
            }
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            visit(object);
            for arg in args {
                visit(arg);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for arg in args {
                visit(arg);
            }
        }
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, v) in fields {
                visit(v);
            }
        }
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPart::Hole(e) = part {
                    visit(e);
                }
            }
        }
        // `auto(seed = expr, …)` params carry value expressions that reference
        // real bindings, so they are children — otherwise find-references/rename
        // silently miss occurrences inside `auto(seed = x)`, leaving a dangling
        // reference after rename (mirrors substitute_expr; geometric-relations δ,
        // 4384).
        ExprKind::Auto { params, .. } => {
            for (_, v) in params {
                visit(v);
            }
        }
        // Leaves with no sub-expressions.
        ExprKind::Ident(_)
        | ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Undef => {}
    }
}
