//! Binding-free collection of every use site of a DECLARATION name within one
//! parsed document — `sub _ = Name` construction sites, `constraint Name(…)`
//! instantiations and every type position; `collect_decl_name_spans` is the one
//! place that category list is written down.

use reify_ast::{
    ConstraintInstDecl, Declaration, DefaultDecl, Expr, ExprKind, FieldSource, FnDef,
    ForallConstraintBody, ImportKind, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule,
    TraitBoundRef, TypeExpr, TypeExprKind, TypeParamDecl, VariantPayload,
};
use reify_core::SourceSpan;

use crate::analysis::name_token_span;

use super::walk::{for_each_child_scope, for_each_direct_subexpr, for_each_member_direct_expr};

/// Collect every declaration-name reference to `name` within a SINGLE parsed
/// document: the home declaration's name token (when `name` is declared in this
/// document) plus each USE-SITE token in every top-level declaration, recursing
/// into members and nested scopes. Ascending by `span.start`.
///
/// Use-site categories collected here:
/// - `sub _ = name` construction sites (κ design decision).
/// - `constraint Name(…)` instantiations (#6539).
/// - TYPE POSITIONS — `param p : Name`, `fn f() -> Name`, `type H = Name`,
///   `structure S : Name`, `|p: Name| …` and every other root
///   [`collect_decl_name_uses_in_decl`] fans out over (#6539).
///
/// All of them are structurally invisible to
/// `collect_uses`/`collect_idents_in_expr`: `SubDecl.structure_name` is a plain
/// `String` field, not an `Expr`, and a `TypeExpr` is not an `Expr` at all, so
/// neither ever appears as `ExprKind::Ident`. This dedicated, BINDING-FREE
/// traversal is the only way to surface them — see [`collect_type_name_uses`]
/// for why the value-binding path is the wrong home. The home declaration token
/// is located via goto_def's `decl_name_span_in` so a declaration's
/// rename/reference token is uniform with go-to-definition — fed the `parsed`
/// the caller already holds, rather than `find_declaration_name_span`, which
/// re-parses the same string.
///
/// CAVEAT — the one type reference this traversal cannot locate.
/// `TypeParamDecl.bounds` is a `Vec<String>` carrying no spans of its own (only
/// the enclosing `TypeParamDecl.span`), so the `Numeric` in `structure S<T:
/// Numeric>` is a real reference that no span in the AST identifies, and it is
/// NOT collected. A trait renamed cross-file therefore leaves such a bound
/// stale, exactly as the pre-#6539 type-annotation gap did. Closing it needs a
/// spanned bound in the AST (`Vec<SpannedIdent>`, as `TraitDecl.refinements`
/// already uses), not a change here.
pub(super) fn collect_decl_name_spans(source: &str, parsed: &ParsedModule, name: &str) -> Vec<SourceSpan> {
    let mut spans = Vec::new();
    // Home declaration token (`structure Name` / `occurrence def Name` / …),
    // present only when `name` is declared in THIS document.
    if let Some(decl_token) = crate::goto_def::decl_name_span_in(parsed, source, name) {
        spans.push(decl_token);
    }
    for decl in &parsed.declarations {
        collect_decl_name_uses_in_decl(decl, source, name, &mut spans);
    }
    spans.sort_by_key(|s| s.start);
    spans
}

/// Push every span at which `decl` REFERENCES a declaration named `name` — never
/// `decl`'s own name token, which [`collect_decl_name_spans`] supplies once.
///
/// The declaration half of the use-site fan-out, and the single place the root
/// set is written down: every `TypeExpr` root reachable from a top-level
/// declaration is reached from here, directly or through
/// [`collect_decl_name_uses_in_members`]. The match is WILDCARD-FREE over all 14
/// `Declaration` variants, so a new declaration kind is a compile error here
/// rather than a silently uncollected use site — the same forcing-function
/// design as `analysis::decl_name_and_span`.
///
/// Both reference collectors share it: the home document walks every
/// declaration, and an importing document walks the same set once its import
/// exposes `name` under that same local name
/// ([`collect_importer_decl_name_references`]). Sharing one fan-out is what
/// keeps the two from disagreeing about what a use site is.
pub(super) fn collect_decl_name_uses_in_decl(
    decl: &Declaration,
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    match decl {
        Declaration::Structure(s) => {
            collect_entity_body_uses(
                &s.type_params,
                &s.trait_bounds,
                &s.members,
                source,
                name,
                out,
            );
        }
        Declaration::Occurrence(o) => {
            collect_entity_body_uses(
                &o.type_params,
                &o.trait_bounds,
                &o.members,
                source,
                name,
                out,
            );
        }
        // `TraitDecl.refinements` is a `Vec<SpannedIdent>`, the one type
        // reference in the AST that already carries its own exact name-token
        // span — so it is pushed verbatim, with no narrowing and no source scan.
        Declaration::Trait(t) => {
            collect_type_param_uses(&t.type_params, source, name, out);
            for refinement in &t.refinements {
                if refinement.name == name {
                    out.push(refinement.span);
                }
            }
            collect_decl_name_uses_in_members(&t.members, source, name, 0, out);
        }
        // A purpose has THREE child regions, not one. `members` is the obvious
        // one; `structures` (a `structure def` lexically nested in the body,
        // kept out of `members` by task 4639) and `defaults` (kept out by task
        // 4496) are sibling vecs that no member walk can reach, which is why
        // `entity_members` — whose contract is a single `&[MemberDecl]` — is
        // NOT the place to close this and is deliberately left alone.
        //
        // `PurposeParam.entity_kind` is deliberately not a use site: it is an
        // entity-KIND keyword (`Structure`, `Occurrence`), not a reference to a
        // declaration, so collecting it would let a rename rewrite unrelated
        // text.
        Declaration::Purpose(p) => {
            collect_type_param_uses(&p.type_params, source, name, out);
            collect_decl_name_uses_in_members(&p.members, source, name, 0, out);
            for default in &p.defaults {
                collect_default_decl_uses(default, source, name, out);
            }
            // A purpose-nested structure is a first-class entity body — the
            // compiler registers its name in the MODULE-level structure
            // namespace and compiles it into the same template table as a
            // top-level one (pre_pass.rs, entities_phase.rs) — so its body is
            // walked exactly like a top-level structure's, sharing one home
            // rather than a second copy that could drift.
            for nested in &p.structures {
                collect_entity_body_uses(
                    &nested.type_params,
                    &nested.trait_bounds,
                    &nested.members,
                    source,
                    name,
                    out,
                );
            }
        }
        Declaration::Enum(e) => {
            collect_type_param_uses(&e.type_params, source, name, out);
            for variant in &e.variants {
                match &variant.payload {
                    VariantPayload::Named(fields) => {
                        for (_, ty) in fields {
                            collect_type_name_uses(ty, source, name, out);
                        }
                    }
                    VariantPayload::Unit => {}
                }
            }
        }
        Declaration::Function(f) => collect_fn_name_uses(f, source, name, out),
        Declaration::Field(f) => {
            collect_type_name_uses(&f.domain_type, source, name, out);
            collect_type_name_uses(&f.codomain_type, source, name, out);
            match &f.source {
                FieldSource::Analytical { expr } | FieldSource::Composed { expr } => {
                    collect_type_name_uses_in_expr(expr, source, name, out);
                }
                FieldSource::Sampled { config } => {
                    for (_, expr) in config {
                        collect_type_name_uses_in_expr(expr, source, name, out);
                    }
                }
                // Path/format/grid are `Option<String>` literals, not
                // expressions and not declaration references.
                FieldSource::Imported { .. } => {}
            }
        }
        Declaration::Constraint(c) => {
            collect_type_param_uses(&c.type_params, source, name, out);
            for param in &c.params {
                if let Some(ty) = &param.type_expr {
                    collect_type_name_uses(ty, source, name, out);
                }
                if let Some(default) = &param.default {
                    collect_type_name_uses_in_expr(default, source, name, out);
                }
            }
            for predicate in &c.predicates {
                collect_type_name_uses_in_expr(predicate, source, name, out);
            }
        }
        Declaration::Unit(u) => collect_type_name_uses(&u.dimension_type, source, name, out),
        Declaration::TypeAlias(a) => {
            collect_type_param_uses(&a.type_params, source, name, out);
            collect_type_name_uses(&a.type_expr, source, name, out);
        }
        Declaration::Default(d) => collect_default_decl_uses(d, source, name, out),
        Declaration::Joint(j) => {
            collect_type_param_uses(&j.type_params, source, name, out);
            for param in &j.params {
                collect_type_name_uses(&param.type_expr, source, name, out);
                if let Some(default) = &param.default {
                    collect_type_name_uses_in_expr(default, source, name, out);
                }
            }
            for dof in &j.dof {
                collect_type_name_uses(&dof.type_expr, source, name, out);
                if let Some(range) = &dof.range {
                    collect_type_name_uses_in_expr(range, source, name, out);
                }
            }
            for expr in &j.body {
                collect_type_name_uses_in_expr(expr, source, name, out);
            }
        }
        // An import's entity token IS a reference, but admitting it here would
        // key on a bare name match; scope soundness (Invariant 1) requires
        // keying on the RESOLVED target module instead, which is
        // `collect_importer_decl_name_references`' job. A `module` declaration
        // names a module path, never a declaration.
        Declaration::Import(_) | Declaration::Module(_) => {}
    }
}

/// Push every reference to `name` inside an entity body — its type parameters'
/// defaults, its trait bounds and its members.
///
/// Takes the three slices rather than a declaration, because the three entity
/// bodies that share this shape are three distinct types: `StructureDef`,
/// `OccurrenceDef`, and a `StructureDef` nested in a `PurposeDef`. One home for
/// the walk keeps a purpose-nested structure from being walked less thoroughly
/// than a top-level one, which is the shape of gap #6539 exists to close.
fn collect_entity_body_uses(
    type_params: &[TypeParamDecl],
    trait_bounds: &[TraitBoundRef],
    members: &[MemberDecl],
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    collect_type_param_uses(type_params, source, name, out);
    collect_trait_bound_uses(trait_bounds, source, name, out);
    collect_decl_name_uses_in_members(members, source, name, 0, out);
}

/// Push every reference to `name` in an ambient-default declaration: the type it
/// applies to and its value expression.
///
/// Shared by the top-level `Declaration::Default` arm and `PurposeDef.defaults`,
/// which are the same `DefaultDecl` at the two positions the grammar allows.
fn collect_default_decl_uses(d: &DefaultDecl, source: &str, name: &str, out: &mut Vec<SourceSpan>) {
    collect_type_name_uses(&d.type_expr, source, name, out);
    collect_type_name_uses_in_expr(&d.value, source, name, out);
}

/// Push every reference to `name` inside a function definition: its type
/// parameters' defaults, each parameter's declared type and default expression,
/// the return type, and the body's `let` annotations and expressions.
///
/// Shared by the top-level `Declaration::Function` arm and the `MemberDecl::Fn`
/// arm (a trait's associated function), which are the same `FnDef` in two
/// positions.
fn collect_fn_name_uses(f: &FnDef, source: &str, name: &str, out: &mut Vec<SourceSpan>) {
    collect_type_param_uses(&f.type_params, source, name, out);
    for param in &f.params {
        collect_type_name_uses(&param.type_expr, source, name, out);
        if let Some(default) = &param.default {
            collect_type_name_uses_in_expr(default, source, name, out);
        }
    }
    if let Some(return_type) = &f.return_type {
        collect_type_name_uses(return_type, source, name, out);
    }
    if let Some(body) = &f.body {
        for binding in &body.let_bindings {
            if let Some(ty) = &binding.type_expr {
                collect_type_name_uses(ty, source, name, out);
            }
            collect_type_name_uses_in_expr(&binding.value, source, name, out);
        }
        collect_type_name_uses_in_expr(&body.result_expr, source, name, out);
    }
}

/// Push every reference to `name` in a type-parameter list — each parameter's
/// DEFAULT type (`<T = Name>`), recursively.
///
/// `TypeParamDecl.bounds` is NOT reached: it is a `Vec<String>` with no spans of
/// its own, so a bound reference cannot be located from the AST at all. That
/// residual is stated once, on [`collect_decl_name_spans`].
fn collect_type_param_uses(
    type_params: &[TypeParamDecl],
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    for type_param in type_params {
        if let Some(default) = &type_param.default {
            collect_type_name_uses(default, source, name, out);
        }
    }
}

/// Push the name-token span of every trait bound in `bounds` that references
/// `name` (`structure S : Name`, `occurrence def O : Container<Name>`),
/// recursing into each bound's type arguments.
///
/// `TraitBoundRef.span` covers the WHOLE bound, name and type arguments
/// together, so the token is always narrowed with [`name_token_span`] — unlike
/// `TraitDecl.refinements`, whose `SpannedIdent` already carries the exact name
/// token.
fn collect_trait_bound_uses(
    bounds: &[TraitBoundRef],
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    for bound in bounds {
        if references_decl_name(&bound.name, name) {
            out.push(name_token_span(source, bound.span, name));
        }
        for arg in &bound.type_args {
            collect_type_name_uses(arg, source, name, out);
        }
    }
}

/// Push the name-token span of each use site of `name` among `members`,
/// recursing into nested member-list scopes (guarded branches, port bodies, sub
/// specialization bodies / keyed overrides, match-arm members) exactly as
/// `collect_uses` does — depth-bounded by [`MAX_MEMBER_NESTING_DEPTH`] — so a
/// use nested inside a `where`/port/sub block is not missed.
///
/// Four categories, and the match over them is WILDCARD-FREE so a new
/// `MemberDecl` variant is a compile error rather than a silently uncollected
/// use site:
/// - a `sub binding = Name(...)` CONSTRUCTION site, whose `structure_name` is a
///   plain `String`; the emitted span is the first whole-word `name` token
///   inside the `sub` statement, i.e. the construction token;
/// - a `constraint Name(...)` INSTANTIATION, whose `name` is a plain `String`
///   for the same reason and is narrowed the same way;
/// - a TYPE POSITION on a member that declares one (`param`/`let` annotations, a
///   sub's type arguments, an associated type's default, a `port`'s trait, an
///   associated function's whole signature and body);
/// - a lambda parameter's type annotation anywhere in the member's own
///   expressions, reached via [`for_each_member_direct_expr`].
///
/// `MemberDecl::Fn` is walked here even though `for_each_member_direct_expr`
/// yields nothing for it — see that function's doc comment for why the value
/// walk stops at a fn body and the type walk does not.
fn collect_decl_name_uses_in_members(
    members: &[MemberDecl],
    source: &str,
    name: &str,
    depth: usize,
    out: &mut Vec<SourceSpan>,
) {
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Sub(s) => {
                if s.structure_name == name {
                    out.push(name_token_span(source, s.span, name));
                }
                for arg in &s.type_args {
                    collect_type_name_uses(arg, source, name, out);
                }
            }
            MemberDecl::Param(p) => {
                if let Some(ty) = &p.type_expr {
                    collect_type_name_uses(ty, source, name, out);
                }
            }
            MemberDecl::Let(l) => {
                if let Some(ty) = &l.type_expr {
                    collect_type_name_uses(ty, source, name, out);
                }
            }
            MemberDecl::AssociatedType(a) => {
                if let Some(ty) = &a.default_type {
                    collect_type_name_uses(ty, source, name, out);
                }
            }
            MemberDecl::Fn(f) => collect_fn_name_uses(f, source, name, out),
            // `PortDecl.type_name` names a TRAIT (the compiler looks it up in
            // the trait registry, entity.rs), so it is a real reference to a
            // top-level declaration. It is a plain `String` with no span of its
            // own, so the token is narrowed out of the port's span.
            MemberDecl::Port(p) => {
                if p.type_name == name {
                    out.push(name_token_span(source, p.span, name));
                }
            }
            // `constraint Foo(x: w)` — a CONSTRAINT-DEF INSTANTIATION. Twins
            // the `Sub` arm above: `ConstraintInstDecl.name` is a plain
            // `String`, so the instantiation name never appears as an
            // `ExprKind::Ident` and is invisible to every ident walk.
            MemberDecl::ConstraintInst(c) => {
                collect_constraint_inst_uses(c, source, name, out);
            }
            // `forall v in coll: constraint Foo(…)` carries the SAME
            // `ConstraintInstDecl` one level in, so it is the same use site and
            // shares the same arm rather than being a second, driftable copy.
            MemberDecl::ForallConstraint(f) => {
                if let ForallConstraintBody::Instantiation(c) = &f.body {
                    collect_constraint_inst_uses(c, source, name, out);
                }
            }
            // Expression-only members: any type reference they carry is a
            // lambda parameter annotation, collected below through
            // `for_each_member_direct_expr` rather than arm by arm.
            MemberDecl::Constraint(_)
            | MemberDecl::Minimize(_)
            | MemberDecl::Maximize(_)
            | MemberDecl::GuardedGroup(_)
            | MemberDecl::Connect(_)
            | MemberDecl::Chain(_)
            | MemberDecl::MetaBlock(_)
            | MemberDecl::ForallConnect(_)
            | MemberDecl::MatchArmDeclGroup(_)
            | MemberDecl::Relate(_)
            | MemberDecl::Sketch(_) => {}
        }
        for_each_member_direct_expr(member, &mut |expr| {
            collect_type_name_uses_in_expr(expr, source, name, out);
        });
        for_each_child_scope(member, |child| {
            collect_decl_name_uses_in_members(child, source, name, depth + 1, out);
        });
    }
}

/// Push the name-token span of a `constraint Name(…)` instantiation when it
/// instantiates `name`.
///
/// `ConstraintInstDecl.name` is a plain `String` with no span of its own, so the
/// token is narrowed out of the instantiation's span — the first whole-word
/// `name` inside `constraint Name(args…)`, which is the instantiation token.
/// Exactly the treatment `SubDecl.structure_name` gets, for exactly the same
/// reason.
///
/// Shared by the `MemberDecl::ConstraintInst` arm and the
/// `ForallConstraintBody::Instantiation` arm, which carry the same struct at two
/// nesting levels.
fn collect_constraint_inst_uses(
    c: &ConstraintInstDecl,
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    if c.name == name {
        out.push(name_token_span(source, c.span, name));
    }
}

/// Push every TYPE-position reference to `name` written inside an expression.
///
/// `LambdaParam.type_expr` (`|p: Name| …`) is the only `TypeExpr` root an
/// expression can carry, so this walk inspects each node for a lambda and
/// otherwise just descends. It is a type walk, not a value walk: an
/// `ExprKind::Ident` equal to `name` is NOT collected here, because a bare
/// identifier in value position names a binding, not a declaration.
fn collect_type_name_uses_in_expr(
    expr: &Expr,
    source: &str,
    name: &str,
    out: &mut Vec<SourceSpan>,
) {
    if let ExprKind::Lambda { params, .. } = &expr.kind {
        for param in params {
            if let Some(ty) = &param.type_expr {
                collect_type_name_uses(ty, source, name, out);
            }
        }
    }
    for_each_direct_subexpr(expr, &mut |child| {
        collect_type_name_uses_in_expr(child, source, name, out);
    });
}

/// Push the name-token span of every reference to `name` appearing in TYPE
/// POSITION within `ty`, recursing through the whole type expression.
///
/// WHY THIS LIVES IN THE DECLARATION-NAME TRAVERSAL AND NOT IN
/// `collect_uses`/`collect_idents_in_expr`. [`collect_references_at`] is a
/// VALUE-BINDING machine: it collects param/let/sub/port BINDINGS, collects
/// uses, then keeps only the uses whose `resolve_use(...)` is the selected
/// binding. A top-level declaration name is not a binding, so a type-position
/// span pushed there would be filtered straight back out — inert, and
/// misleading to the next reader. This traversal is the existing binding-free
/// home for exactly this shape of problem; it was created because
/// `SubDecl.structure_name` is a plain `String` invisible to
/// `collect_idents_in_expr`, and a type expression is invisible to it for the
/// same structural reason.
///
/// The match is WILDCARD-FREE over all six `TypeExprKind` variants, so a new
/// variant is a compile error here rather than a silently uncollected use site
/// — the same forcing-function design as `analysis::decl_name_and_span`.
///
/// SPAN PRECISION is the rename-safety property, because
/// [`compute_rename_cross_file`] uses the reference set as its exact edit set.
/// A `Named` type's span is the NAME TOKEN exactly when it is a bare identifier
/// (`lower_type_expr_node`'s bare-identifier arm spans just that node), but
/// covers the whole `Box<T>` construct for the parameterized form
/// (`lower_parameterized_type` spans the entire node). The bare case therefore
/// pushes `ty.span` directly — authoritative by construction, with no source
/// scan and no exposure to [`name_token_span`]'s empty-span miss fallback —
/// while every other shape must narrow.
fn collect_type_name_uses(ty: &TypeExpr, source: &str, name: &str, out: &mut Vec<SourceSpan>) {
    match &ty.kind {
        TypeExprKind::Named {
            name: written,
            type_args,
        } => {
            if written == name && type_args.is_empty() {
                out.push(ty.span);
            } else if references_decl_name(written, name) {
                out.push(name_token_span(source, ty.span, name));
            }
            for arg in type_args {
                collect_type_name_uses(arg, source, name, out);
            }
        }
        TypeExprKind::DimensionalOp { op: _, left, right } => {
            collect_type_name_uses(left, source, name, out);
            collect_type_name_uses(right, source, name, out);
        }
        TypeExprKind::QualifiedAssoc {
            base,
            trait_name: _,
            member: _,
        } => {
            // `base` is a type expression; `member` and `trait_name` name an
            // associated type and a trait, neither of which is a reference to
            // the top-level declaration `name` denotes.
            collect_type_name_uses(base, source, name, out);
        }
        TypeExprKind::Function {
            params,
            return_type,
        } => {
            for param in params {
                collect_type_name_uses(param, source, name, out);
            }
            collect_type_name_uses(return_type, source, name, out);
        }
        // Leaves: an integer type-argument and an `auto` type-argument name no
        // declaration. `Auto.bound` is a bare identifier but carries no span of
        // its own (only the enclosing `TypeExpr.span`), so a bound reference
        // cannot be located from the AST and is not collected.
        TypeExprKind::IntegerLiteral(_) => {}
        TypeExprKind::Auto { .. } => {}
    }
}

/// Whether a type name spelled `written` in source refers to the declaration
/// named `name`.
///
/// Two spellings do. The bare name is the obvious one. The NAMESPACED form
/// (`pp.Hole`) is the trap: `ts_parser::namespaced_name_text` dot-JOINS the
/// qualifier and the name into a single `String`, so a bare declaration name
/// never equals it and, without this comparison, an ordinary-looking use is
/// silently dropped — invisible to find-references and left stale by a rename.
/// Only the LAST segment is ever emitted as a span, because renaming the
/// declaration rewrites `Hole` and never the `pp.` qualifier.
fn references_decl_name(written: &str, name: &str) -> bool {
    written == name || written.rsplit('.').next().unwrap_or(written) == name
}

/// Whether import `kind` brings an entity named `name` into local scope under
/// that SAME name — the non-aliased forms (`import m.Name`, `import m.{Name,…}`).
/// Aliased imports (`import m.Name as Other`) bind a DISTINCT local name and are
/// handled separately (κ step-6), so they return `false` here.
pub(super) fn import_exposes_entity(kind: &ImportKind, name: &str) -> bool {
    match kind {
        ImportKind::Entity(n) => n == name,
        ImportKind::Destructured(names) => names.iter().any(|n| n == name),
        ImportKind::EntityAliased { .. } | ImportKind::Module | ImportKind::Aliased { .. } => false,
    }
}

#[cfg(test)]
mod tests;
