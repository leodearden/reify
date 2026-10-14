//! Value-member (param/let/auto/sub/port) reference resolution within one
//! entity body: binding collection, scope-aware use resolution, and the
//! `.member`-segment refusal.

use reify_ast::{
    Declaration, Expr, ExprKind, ForallConnectBody, ForallConstraintBody, MAX_MEMBER_NESTING_DEPTH,
    MemberDecl, ParsedModule, StringPart,
};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::decl_oracle::{enclosing_decl_at, name_token_span};
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

use super::walk::{
    for_each_child_scope, for_each_direct_subexpr, for_each_member_direct_expr,
    keyed_entry_param_override_exprs, sub_direct_exprs,
};

/// The kind of symbol a [`ReferenceSet`] resolves to.
///
/// Two families, and which one a symbol falls in decides which rename path can
/// reach it:
/// - DECLARATION NAMES — one variant per NAMED top-level `Declaration` kind
///   (all eleven), plus `Variant` for an enum's variants. Never single-file
///   renameable; renameable cross-file for the kinds
///   `is_renameable_cross_file` admits.
/// - VALUE MEMBERS (`Param`/`Let`/`Auto`/`Sub`/`Port`) — bindings inside an
///   entity body, renameable single-file (`is_renameable`) and keeping those
///   exact semantics on the cross-file path.
///
/// One variant per named declaration kind — rather than a single `Decl` — is
/// what lets `classify_decl_name` match WILDCARD-FREE over
/// `Declaration`, so a new declaration kind is a compile error there instead of
/// a silently unclassified declaration that every rename gate then refuses
/// without saying so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefSymbolKind {
    Structure,
    Occurrence,
    Trait,
    Enum,
    Variant,
    Fn,
    Field,
    Purpose,
    Constraint,
    Unit,
    TypeAlias,
    Joint,
    Param,
    Let,
    Auto,
    Sub,
    Port,
}

/// A resolved set of references to a single binding.
///
/// `declaration` and every element of `references` are **name-token** spans (the
/// identifier only). `references` is sorted ascending by `span.start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSet {
    /// The binding's name.
    pub name: String,
    /// The classified symbol kind.
    pub kind: RefSymbolKind,
    /// Name-token span of the declaration identifier.
    pub declaration: SourceSpan,
    /// Name-token spans of the uses (and the declaration token too, when
    /// `include_declaration` is set), ascending by `span.start`.
    pub references: Vec<SourceSpan>,
}

/// The result of a prepareRename request: the token range to highlight and the
/// placeholder text the editor should pre-fill.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameTarget {
    /// LSP range of the identifier token under the cursor.
    pub range: Range,
    /// The current symbol name, used as the rename placeholder.
    pub placeholder: String,
}

/// Collect every reference to the binding under the cursor, scoped to a single
/// entity body.
///
/// Returns `None` when the cursor is not on an identifier that resolves to a
/// local value-member binding. When `include_declaration` is `true` the
/// declaration name-token span is merged into `references`.
pub fn collect_references(
    source: &str,
    parsed: &ParsedModule,
    pos: Position,
    include_declaration: bool,
) -> Option<ReferenceSet> {
    let offset = position_to_offset(source, pos);
    let (_word_start, word) = find_word_at_offset(source, offset)?;
    collect_references_at(source, parsed, offset, word, include_declaration)
}

/// Map the [`ReferenceSet`] under the cursor to LSP [`Location`]s in `uri`.
///
/// The thin LSP-facing wrapper over [`collect_references`] (task β, 4202). It
/// takes an **already-resolved** [`ParsedModule`] — fed by the handler from the
/// per-document parse cache (`DocumentState::parsed_module`, one parse per edit),
/// mirroring `rename::compute_document_highlights` — runs the scope-aware collection,
/// then maps each name-token [`SourceSpan`] to a [`Location`] via
/// [`span_to_range`], paired with the document `uri`. Returns `None` when the
/// cursor is not on an identifier that resolves to a local value-member binding
/// (propagated from [`collect_references`]). The LSP `context.include_declaration`
/// flag is passed straight through. Single-file: no spawn_blocking, no cross-file
/// FS I/O.
pub fn compute_references(
    source: &str,
    parsed: &ParsedModule,
    uri: &Url,
    pos: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let refset = collect_references(source, parsed, pos, include_declaration)?;
    Some(
        refset
            .references
            .iter()
            .map(|&span| Location {
                uri: uri.clone(),
                range: span_to_range(source, span),
            })
            .collect(),
    )
}

/// Reference collection from an **already-resolved** cursor (`offset` + the
/// identifier `word` under it).
///
/// Factored out of [`collect_references`] so `rename::prepare_rename` can reuse the
/// `position_to_offset` + `find_word_at_offset` lookup it has already performed,
/// instead of recomputing the cursor word a second time per request.
pub(super) fn collect_references_at(
    source: &str,
    parsed: &ParsedModule,
    offset: usize,
    word: &str,
    include_declaration: bool,
) -> Option<ReferenceSet> {
    // Confine the entire query to a single entity body — the declaration whose
    // span contains the cursor. `members` is the ONLY member list handed to both
    // `collect_bindings` (declaration resolution) and `collect_uses` (the
    // occurrence walk) below, so a same-named binding declared in any other
    // structure/occurrence/trait/purpose is never visited or included. This is
    // Invariant 1 (no cross-scope false positives) at the entity boundary, pinned
    // by `collect_references_cross_structure_isolation`.
    let enclosing = enclosing_decl_at(&parsed.declarations, offset)?;
    let members = entity_members(enclosing)?;

    // AST-aware member-segment guard (task-4346): if the cursor sits on the
    // `.member` segment of a `MemberAccess` expression (e.g. the `.diameter` in
    // `h.diameter`), refuse — do NOT resolve to a same-named local binding.
    // `find_word_at_offset` is byte-based and returns the bare word even on a
    // member segment; this guard layers the AST-aware refusal at the shared
    // chokepoint so all four producers (find-references, prepare_rename,
    // compute_rename, compute_document_highlights) inherit it consistently.
    // See `cursor_on_member_segment` for the detection logic (depth-bounded
    // member walk mirroring `collect_uses` + `for_each_child_scope`).
    if cursor_on_member_segment(members, offset as u32, 0) {
        return None;
    }

    // Collect every binding of `word` reachable in this entity — the flat body and
    // every nested `where`/`else` branch — each tagged with its scope region and
    // depth. Registration mirrors CompilationScope (params before lets before
    // autos; last registration wins, so a later `let` shadows an earlier same-named
    // `param`), and a guarded binding shadows an outer one within its branch
    // region. See crates/reify-compiler/src/scope.rs as the source of truth for
    // precedence.
    let bindings = collect_bindings(members, word, source);
    if bindings.is_empty() {
        // The cursor word does not name a local value-member binding (e.g. a type
        // name, builtin, keyword, or cross-module symbol) — no references here.
        return None;
    }

    // Which binding does the cursor select? Its own declaration token if the cursor
    // sits on a declaration, else the active binding for a use at this offset.
    let selected = select_binding(&bindings, offset);
    let declaration = bindings[selected].decl_token;
    let kind = bindings[selected].kind;

    // Collect every use of `word`, then keep only the uses that resolve to the
    // selected binding. Partitioning by resolved binding makes a shadowed param and
    // the shadowing let yield disjoint reference sets.
    let mut uses = Vec::new();
    collect_uses(members, word, 0, &mut uses);
    let mut references: Vec<SourceSpan> = uses
        .into_iter()
        .filter(|u| resolve_use(u.start as usize, &bindings) == selected)
        .collect();

    // When requested, merge the declaration name-token span into the set; it is
    // first in source so it sorts to the front of the ascending output.
    if include_declaration {
        references.push(declaration);
    }
    references.sort_by_key(|s| s.start);

    Some(ReferenceSet {
        name: word.to_string(),
        kind,
        declaration,
        references,
    })
}

/// Return the value-bearing member list of an entity declaration
/// (structure / occurrence / trait / purpose), or `None` for declaration kinds
/// that carry no member body. Mirrors the member extraction in `goto_def`.
pub(super) fn entity_members(decl: &Declaration) -> Option<&[MemberDecl]> {
    match decl {
        Declaration::Structure(s) => Some(&s.members),
        Declaration::Occurrence(o) => Some(&o.members),
        Declaration::Trait(t) => Some(&t.members),
        Declaration::Purpose(p) => Some(&p.members),
        _ => None,
    }
}

// ─── Member-segment detection helpers ────────────────────────────────────────
//
// These two helpers implement the AST-aware guard in `collect_references_at`
// that refuses to resolve the cursor when it sits on the `.member` segment of a
// `MemberAccess` expression (e.g. the `.diameter` in `h.diameter`).
//
// Design mirrors the existing traversal pair: `expr_member_segment_hit` mirrors
// `collect_idents_in_expr` (exhaustive, no-wildcard ExprKind recursion) and
// `cursor_on_member_segment` mirrors `collect_uses` (member-kind dispatch +
// nested-scope descent, depth-bounded by `MAX_MEMBER_NESTING_DEPTH`). Keeping
// the two scanners symmetric ensures the detection scan never drifts from the
// use-collection scan.
//
// The `MemberDecl::Sub` arm's field set is centralized in `sub_direct_exprs`
// (plus each keyed entry's own fields in `keyed_entry_param_override_exprs`)
// — see their doc comments for why.

/// Return `true` when the cursor byte offset `off` falls on the `.member`
/// segment of a `MemberAccess` node anywhere in `expr`.
///
/// The `.member` segment is exactly `[object.span.end, expr.span.end)`: the
/// `.`, optional whitespace, and the member name. The cursor is on the SEGMENT
/// iff `off >= object.span.end && off < expr.span.end`. When `off` is below
/// `object.span.end` the cursor is on the BASE, which must stay renameable, so
/// we recurse into `object` only.
///
/// The match is exhaustive (no wildcard) so a new `ExprKind` variant is a
/// compile error that forces a deliberate decision here, keeping this walker
/// symmetric with `collect_idents_in_expr`.
fn expr_member_segment_hit(expr: &Expr, off: u32) -> bool {
    match &expr.kind {
        ExprKind::MemberAccess { object, .. } => {
            // Cursor on the member segment (past the object's span end)?
            if off >= object.span.end && off < expr.span.end {
                return true;
            }
            // Cursor may be on the base object or inside it — recurse.
            expr_member_segment_hit(object, off)
        }
        ExprKind::BinOp { left, right, .. } => {
            expr_member_segment_hit(left, off) || expr_member_segment_hit(right, off)
        }
        ExprKind::UnOp { operand, .. } => expr_member_segment_hit(operand, off),
        ExprKind::FunctionCall { args, .. } => {
            args.iter().any(|a| expr_member_segment_hit(a, off))
        }
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_member_segment_hit(condition, off)
                || expr_member_segment_hit(then_branch, off)
                || expr_member_segment_hit(else_branch, off)
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            items.iter().any(|i| expr_member_segment_hit(i, off))
        }
        ExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(k, v)| expr_member_segment_hit(k, off) || expr_member_segment_hit(v, off)),
        ExprKind::IndexAccess { object, index } => {
            expr_member_segment_hit(object, off) || expr_member_segment_hit(index, off)
        }
        ExprKind::Match { discriminant, arms } => {
            expr_member_segment_hit(discriminant, off)
                || arms.iter().any(|a| expr_member_segment_hit(&a.body, off))
        }
        ExprKind::Lambda { body, .. } => expr_member_segment_hit(body, off),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            expr_member_segment_hit(collection, off) || expr_member_segment_hit(predicate, off)
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            expr_member_segment_hit(base, off) || args.iter().any(|a| expr_member_segment_hit(a, off))
        }
        ExprKind::QualifiedAccess { qualifier, .. } => expr_member_segment_hit(qualifier, off),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            expr_member_segment_hit(object, off) || expr_member_segment_hit(qualified, off)
        }
        ExprKind::Range { lower, upper, .. } => {
            lower.as_ref().is_some_and(|l| expr_member_segment_hit(l, off))
                || upper.as_ref().is_some_and(|u| expr_member_segment_hit(u, off))
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            expr_member_segment_hit(object, off)
                || args.iter().any(|a| expr_member_segment_hit(a, off))
        }
        ExprKind::TraitStaticCall { args, .. } => {
            args.iter().any(|a| expr_member_segment_hit(a, off))
        }
        ExprKind::VariantConstruct { fields, .. } => fields
            .iter()
            .any(|(_, v)| expr_member_segment_hit(v, off)),
        ExprKind::InterpolatedString(parts) => parts.iter().any(|p| {
            if let StringPart::Hole(e) = p {
                expr_member_segment_hit(e, off)
            } else {
                false
            }
        }),
        // Recurse into `auto(seed = expr, …)` params so a cursor landing inside a
        // param value resolves to its symbol (mirrors collect_idents_in_expr).
        ExprKind::Auto { params, .. } => {
            params.iter().any(|(_, v)| expr_member_segment_hit(v, off))
        }
        // Leaves with no sub-expressions.
        ExprKind::Ident(_)
        | ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Undef => false,
    }
}

/// Return `true` when the cursor byte offset `off` falls on the `.member`
/// segment of a `MemberAccess` expression anywhere within `members` at any
/// nesting depth up to `MAX_MEMBER_NESTING_DEPTH`.
///
/// Mirrors the member-kind dispatch of `collect_uses` AND the
/// nested-scope descent of `for_each_child_scope`: for each member,
/// runs `expr_member_segment_hit` over its direct expressions, then recurses
/// into its child member lists (GuardedGroup where/else bodies, Port body, Sub
/// specialization body / keyed-block overrides, MatchArmDeclGroup per-arm
/// members) at `depth + 1`. Returns `true` on the first hit.
///
/// The public guard in `collect_references_at` calls this with `depth = 0`.
fn cursor_on_member_segment(members: &[MemberDecl], off: u32, depth: usize) -> bool {
    // Mirror collect_uses' recursion bound so the detection scan and the use-
    // collection scan share the same depth limit (same stack-overflow backstop).
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return false;
    }
    for member in members {
        let direct_hit = match member {
            MemberDecl::Param(p) => {
                p.default.as_ref().is_some_and(|d| expr_member_segment_hit(d, off))
                    || p.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::Let(l) => {
                expr_member_segment_hit(&l.value, off)
                    || l.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::Constraint(c) => {
                expr_member_segment_hit(&c.expr, off)
                    || c.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::ConstraintInst(c) => {
                c.args.iter().any(|(_, a)| expr_member_segment_hit(a, off))
                    || c.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::Sub(s) => {
                sub_direct_exprs(s).any(|e| expr_member_segment_hit(e, off))
                    // Each keyed entry's own `param_overrides` values (task
                    // #5579 amendment) — see `keyed_entry_param_override_exprs`.
                    || s.keyed_members.iter().any(|entry| {
                        keyed_entry_param_override_exprs(entry)
                            .any(|e| expr_member_segment_hit(e, off))
                    })
            }
            MemberDecl::Minimize(m) => {
                expr_member_segment_hit(&m.expr, off)
                    || m.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::Maximize(m) => {
                expr_member_segment_hit(&m.expr, off)
                    || m.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
            }
            MemberDecl::GuardedGroup(g) => {
                // Guard condition is an outer-scope expression; child bodies are
                // recursed below via for_each_child_scope.
                expr_member_segment_hit(&g.condition, off)
            }
            MemberDecl::Port(p) => {
                // Frame expr is an outer-scope expression; port body is recursed below.
                p.frame_expr
                    .as_ref()
                    .is_some_and(|f| expr_member_segment_hit(f, off))
            }
            MemberDecl::Connect(c) => {
                expr_member_segment_hit(&c.left.expr, off)
                    || expr_member_segment_hit(&c.right.expr, off)
                    || c.params.iter().any(|(_, p)| expr_member_segment_hit(p, off))
            }
            MemberDecl::Chain(c) => {
                c.elements.iter().any(|e| expr_member_segment_hit(e, off))
            }
            MemberDecl::ForallConnect(f) => {
                expr_member_segment_hit(&f.collection, off)
                    || match &f.body {
                        ForallConnectBody::Connect(c) => {
                            expr_member_segment_hit(&c.left.expr, off)
                                || expr_member_segment_hit(&c.right.expr, off)
                                || c.params.iter().any(|(_, p)| expr_member_segment_hit(p, off))
                        }
                        ForallConnectBody::Chain(c) => {
                            c.elements.iter().any(|e| expr_member_segment_hit(e, off))
                        }
                    }
            }
            MemberDecl::ForallConstraint(f) => {
                expr_member_segment_hit(&f.collection, off)
                    || match &f.body {
                        ForallConstraintBody::Constraint(c) => {
                            expr_member_segment_hit(&c.expr, off)
                                || c.where_clause
                                    .as_ref()
                                    .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
                        }
                        ForallConstraintBody::Instantiation(c) => {
                            c.args.iter().any(|(_, a)| expr_member_segment_hit(a, off))
                                || c.where_clause
                                    .as_ref()
                                    .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
                        }
                    }
            }
            MemberDecl::MatchArmDeclGroup(g) => {
                // Discriminant is outer-scope; arm member bodies are recursed below.
                expr_member_segment_hit(&g.discriminant, off)
            }
            // A member-level `relate { … }` block's relations are direct-scope
            // expressions (task δ 4384) — scan each like Chain's elements.
            MemberDecl::Relate(r) => {
                r.relations.iter().any(|rel| expr_member_segment_hit(rel, off))
            }
            // A `sketch { … }` block carries NO direct-scope expressions of its
            // own — its `let` values and bare relations live in its nested
            // member list, which the `for_each_child_scope` recursion below
            // visits at `depth + 1` (constrained-2d-sketch α, task 5506). This
            // is the `GuardedGroup`/`Port` shape, not the `Relate` shape: a
            // relate block holds `Vec<Expr>` directly, a sketch holds
            // `Vec<MemberDecl>`.
            MemberDecl::Sketch(_) => false,
            // Not walked by `collect_uses` — no tracked binding.
            MemberDecl::Fn(_) | MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => false,
        };
        if direct_hit {
            return true;
        }
        // Recurse into nested member-list scopes exactly as for_each_child_scope
        // and collect_uses do: GuardedGroup where/else bodies, Port body,
        // Sub specialization body / keyed-block overrides, MatchArmDeclGroup
        // per-arm members. This mirrors `collect_bindings_in_scope`'s recursion.
        let mut child_hit = false;
        for_each_child_scope(member, |child| {
            if !child_hit {
                child_hit = cursor_on_member_segment(child, off, depth + 1);
            }
        });
        if child_hit {
            return true;
        }
    }
    false
}

// ─── End member-segment detection helpers ────────────────────────────────────

/// One value-member binding of a target name, tagged with the scope region in
/// which it is visible.
struct Binding {
    /// The classified symbol kind (`Param`/`Let`/`Auto`/`Sub`/`Port`).
    kind: RefSymbolKind,
    /// Name-token span of this binding's declaration identifier.
    decl_token: SourceSpan,
    /// Byte region of the scope in which this binding is visible: the flat entity
    /// body for top-level bindings, or a `where`/`else` branch's byte span for a
    /// guarded binding. A use resolves to the innermost (deepest) binding whose
    /// region contains it, so a guarded binding shadows an outer same-named one
    /// only within its branch.
    region: SourceSpan,
    /// Scope nesting depth: 0 for the flat entity body, +1 per guarded branch.
    depth: u32,
}

/// Collect every binding named `name` reachable from `members` — the flat entity
/// body plus every nested scope (`where`/`else` branches, port bodies, sub
/// specialization bodies / keyed overrides, match-arm member clusters) — tagged
/// with each binding's scope region and nesting depth.
///
/// Within a single scope the order mirrors `CompilationScope` registration:
/// params first (source order), then lets (source order), so a later `let`
/// shadows an earlier same-named `param`. Nested scopes are then layered on as
/// deeper scopes. See `crates/reify-compiler/src/scope.rs` as the source of truth
/// for precedence (mirrored, not reused, per the plan).
///
/// Invariant: binding collection MUST descend into exactly the member lists
/// [`collect_uses`] descends into (visited by [`for_each_child_scope`]) so the
/// two traversals never drift — otherwise a nested redeclaration is invisible to
/// resolution and a nested use is mis-attributed to an outer binding (Invariant 1
/// for nested scopes).
fn collect_bindings(members: &[MemberDecl], name: &str, source: &str) -> Vec<Binding> {
    // The flat entity body is the outermost scope; its region must contain every
    // top-level use. The bounding box of all top-level members covers them all
    // (including uses nested inside guarded groups, since the guarded group's span
    // is itself a top-level member span). Fall back to an unbounded region for an
    // empty body — defensive; a body with no spannable members has no bindings.
    let entity_region = members_region(members).unwrap_or(SourceSpan::new(0, u32::MAX));
    let mut out = Vec::new();
    collect_bindings_in_scope(members, name, source, entity_region, 0, &mut out);
    out
}

/// Register every binding of `name` in `members`, scope by scope, into `out`.
///
/// Params are registered before lets within this scope (params→lets→autos, last
/// registration winning), then every nested member-list scope this scope opens —
/// the child scopes of each member (see [`for_each_child_scope`]): guarded
/// `where`/`else` branches, port bodies, sub bodies / keyed overrides, and
/// match-arm members — is recursed
/// into at `depth + 1`, each bounded by that child's byte region. A nested binding
/// therefore shadows an outer same-named binding only within its region (the
/// where/else/port/sub/match clause of Invariant 1). These are exactly the lists
/// [`collect_uses`] descends into, so binding registration and use collection
/// stay symmetric and never drift.
fn collect_bindings_in_scope(
    members: &[MemberDecl],
    name: &str,
    source: &str,
    region: SourceSpan,
    depth: u32,
    out: &mut Vec<Binding>,
) {
    // Mirror collect_uses' recursion bound so binding registration and use
    // collection share the same depth limit: a binding must never be registered
    // deeper than the use walker will descend, and this is the same
    // stack-overflow backstop collect_uses has. (`depth` is u32 here;
    // MAX_MEMBER_NESTING_DEPTH is a usize, hence the cast.)
    if depth as usize > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    // Single registration pass with two buckets. Params register before lets
    // within a scope (params→lets→autos, last registration winning) so a later
    // `let x` shadows an earlier `param x`: params go straight to `out`, while
    // lets, subs and ports defer into `rest` and are appended after — preserving
    // the only registration ordering shadowing resolution depends on. Sub and port
    // names share the value namespace but do not collide with a param/let name in
    // practice, so their source-order position within `rest` is immaterial. `rest`
    // does not allocate until the first deferred binding, so the common
    // single-binding lookup stays allocation-free. (Replaces four sequential passes
    // over `members`.)
    let mut rest: Vec<Binding> = Vec::new();
    for member in members {
        match member {
            // A `param x = auto` / `auto(free)` binding classifies as Auto.
            MemberDecl::Param(p) if p.name == name => out.push(Binding {
                kind: classify_auto(p.default.as_ref(), RefSymbolKind::Param),
                decl_token: name_token_span(source, p.span, name),
                region,
                depth,
            }),
            // A `let x = auto` binding likewise classifies as Auto.
            MemberDecl::Let(l) if l.name == name => rest.push(Binding {
                kind: classify_auto(Some(&l.value), RefSymbolKind::Let),
                decl_token: name_token_span(source, l.span, name),
                region,
                depth,
            }),
            MemberDecl::Sub(s) if s.name == name => rest.push(Binding {
                kind: RefSymbolKind::Sub,
                decl_token: name_token_span(source, s.span, name),
                region,
                depth,
            }),
            MemberDecl::Port(p) if p.name == name => rest.push(Binding {
                kind: RefSymbolKind::Port,
                decl_token: name_token_span(source, p.span, name),
                region,
                depth,
            }),
            _ => {}
        }
    }
    out.append(&mut rest);
    // Recurse into every nested member-list scope this member opens — guarded
    // `where`/`else` branches, port bodies, sub specialization bodies / keyed
    // overrides, and match-arm member clusters — i.e. EXACTLY the member lists
    // `collect_uses` descends into (visited by `for_each_child_scope`). Binding
    // registration MUST mirror use collection here or the two drift: a nested
    // redeclaration is registered one level deeper, bounded by its own members'
    // byte region, so it shadows an outer same-named binding only within that
    // region (Invariant 1 for nested scopes). A nested use of a NON-redeclared
    // outer name registers no nested binding and still resolves outward by region
    // containment.
    for member in members {
        for_each_child_scope(member, |child| {
            if let Some(region) = members_region(child) {
                collect_bindings_in_scope(child, name, source, region, depth + 1, out);
            }
        });
    }
}

/// Byte region spanning every member in `members` (min start .. max end), or
/// `None` when no member carries a span. Bounds a `where`/`else` branch scope and
/// the flat entity body.
fn members_region(members: &[MemberDecl]) -> Option<SourceSpan> {
    let mut lo: Option<u32> = None;
    let mut hi = 0u32;
    for member in members {
        if let Some(span) = member_span(member) {
            lo = Some(lo.map_or(span.start, |l| l.min(span.start)));
            hi = hi.max(span.end);
        }
    }
    lo.map(|l| SourceSpan::new(l, hi))
}

/// The source span of a member statement, for the member kinds that carry one and
/// can appear in an entity body.
///
/// CRITICAL invariant: this must return `Some` for **every** member kind that
/// [`collect_uses`] descends into, because [`members_region`] derives the flat
/// `entity_region` bounding box from these spans, and [`resolve_use`] only resolves
/// a use to the correct outer binding when that use falls inside a binding region.
/// A use collected from a member whose span were dropped here (and positioned after
/// the last spannable member) would land past `entity_region.end`, be contained by
/// no binding region, and fall through `resolve_use`'s last-registered fallback —
/// mis-attributing a top-level `connect`/`chain`/`forall`/match-arm use to the
/// innermost (e.g. guarded or port-local) binding under shadowing (Invariant 1).
/// So `Connect`/`Chain`/`ForallConnect`/`ForallConstraint`/`MatchArmDeclGroup` —
/// all walked by `collect_uses` — are spanned here too.
///
/// The match is exhaustive (no wildcard) so adding a new `MemberDecl` variant is a
/// compile error that forces a deliberate decision here, keeping `member_span` and
/// `collect_uses` from drifting apart. The three kinds returning `None`
/// (`Fn`/`AssociatedType`/`MetaBlock`) are exactly the ones `collect_uses` does not
/// walk: a fn body opens its own param scope (deferred), and associated-type /
/// meta blocks carry no tracked binding or collected use.
fn member_span(member: &MemberDecl) -> Option<SourceSpan> {
    match member {
        MemberDecl::Param(p) => Some(p.span),
        MemberDecl::Let(l) => Some(l.span),
        MemberDecl::Constraint(c) => Some(c.span),
        MemberDecl::ConstraintInst(c) => Some(c.span),
        MemberDecl::Sub(s) => Some(s.span),
        MemberDecl::Minimize(m) => Some(m.span),
        MemberDecl::Maximize(m) => Some(m.span),
        MemberDecl::GuardedGroup(g) => Some(g.span),
        MemberDecl::Port(p) => Some(p.span),
        MemberDecl::Connect(c) => Some(c.span),
        MemberDecl::Chain(c) => Some(c.span),
        MemberDecl::ForallConnect(f) => Some(f.span),
        MemberDecl::ForallConstraint(f) => Some(f.span),
        MemberDecl::MatchArmDeclGroup(g) => Some(g.span),
        // A `relate { … }` block is walked by `collect_uses` (its relations carry
        // tracked uses), so it must be spanned here too (task δ 4384).
        MemberDecl::Relate(r) => Some(r.span),
        // A `sketch { … }` block is walked by `collect_uses` (its body members
        // carry tracked uses — e.g. `distance(a, b, slot_w)` uses the enclosing
        // `slot_w` param), so the CRITICAL invariant above requires a span here
        // (constrained-2d-sketch α, task 5506). Returning `None` while walking
        // it would put every use collected from the body outside
        // `entity_region` and mis-attribute it under shadowing.
        MemberDecl::Sketch(s) => Some(s.span),
        // Not walked by `collect_uses` — no tracked binding or collected use.
        MemberDecl::Fn(_) | MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => None,
    }
}

/// Select which binding the cursor refers to.
///
/// If `offset` falls on a binding's declaration name-token, that binding is
/// selected directly; otherwise the cursor is on a use and resolves to the active
/// binding for a use at `offset`. `bindings` must be non-empty.
fn select_binding(bindings: &[Binding], offset: usize) -> usize {
    for (i, b) in bindings.iter().enumerate() {
        if offset >= b.decl_token.start as usize && offset < b.decl_token.end as usize {
            return i;
        }
    }
    resolve_use(offset, bindings)
}

/// Resolve a use at `offset` to the index of its active binding.
///
/// Among the bindings whose scope region contains `offset`, the innermost
/// (deepest) one wins; ties at the same depth go to the last-registered binding
/// (params→lets→autos). So a `let` shadows an earlier same-named `param` in the
/// flat body, and a guarded binding shadows an outer one only within its branch
/// region. Falls back to the last binding when `offset` is covered by none
/// (defensive — the flat entity region normally covers every in-entity use).
/// `bindings` must be non-empty.
fn resolve_use(offset: usize, bindings: &[Binding]) -> usize {
    let off = offset as u32;
    let mut best: Option<usize> = None;
    let mut best_depth = 0u32;
    for (i, b) in bindings.iter().enumerate() {
        let contains = off >= b.region.start && off < b.region.end;
        // Iterating ascending, `b.depth >= best_depth` keeps the deepest region
        // and, among equal depths, the last-registered binding.
        if contains && (best.is_none() || b.depth >= best_depth) {
            best = Some(i);
            best_depth = b.depth;
        }
    }
    best.unwrap_or(bindings.len() - 1)
}

/// Walk every value-bearing member of `members`, pushing the span of each
/// `ExprKind::Ident` whose name equals `name`.
///
/// The member fan-out itself is not written here: [`for_each_member_direct_expr`]
/// yields a member's own expressions and [`for_each_child_scope`] yields the
/// nested member lists it opens, so this function is only "an ident use is any
/// `Ident` in any of those expressions". Recursion into nested member lists is
/// bounded by [`MAX_MEMBER_NESTING_DEPTH`], mirroring
/// `reify_ast::find_named_member_span`.
///
/// Associated functions (`MemberDecl::Fn`) are intentionally NOT walked — see
/// [`for_each_member_direct_expr`]'s doc comment for why, and for the reason the
/// type-position walk does not inherit that restriction.
fn collect_uses(members: &[MemberDecl], name: &str, depth: usize, out: &mut Vec<SourceSpan>) {
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        for_each_member_direct_expr(member, &mut |expr| {
            collect_idents_in_expr(expr, name, out);
        });
        for_each_child_scope(member, |child| collect_uses(child, name, depth + 1, out));
    }
}

/// Recursively push the span of every `ExprKind::Ident(name)` in `expr`.
///
/// Covers every `ExprKind` that contains sub-expressions so no in-scope use is
/// missed (Invariant 2); the child enumeration itself lives in
/// [`for_each_direct_subexpr`], whose wildcard-free match is what makes a new
/// `ExprKind` variant a compile error rather than a silently-dropped reference.
///
/// Known limitation: binder-introducing expressions (`Lambda` params,
/// `Quantifier` variables, `Match` pattern binders) open their own value scope,
/// which this foundation phase does not model. Their bodies are still walked for
/// completeness; a binder that shadows `name` is therefore not handled here and
/// is deferred along with the other nested-scope cases.
fn collect_idents_in_expr(expr: &Expr, name: &str, out: &mut Vec<SourceSpan>) {
    if let ExprKind::Ident(ident) = &expr.kind
        && ident == name
    {
        out.push(expr.span);
    }
    for_each_direct_subexpr(expr, &mut |child| collect_idents_in_expr(child, name, out));
}

/// Classify a value binding by its initializer: a binding initialized to the
/// `auto` keyword (`param bore: Length = auto`, `auto(free)`) classifies as
/// [`RefSymbolKind::Auto`]; otherwise it keeps its `base` kind (`Param`/`Let`).
///
/// `init` is the param default or the let value. Reference collection for an
/// `Auto` binding is identical to any other named value binding — only the
/// reported kind differs.
fn classify_auto(init: Option<&Expr>, base: RefSymbolKind) -> RefSymbolKind {
    match init {
        Some(Expr {
            kind: ExprKind::Auto { .. },
            ..
        }) => RefSymbolKind::Auto,
        _ => base,
    }
}

#[cfg(test)]
mod tests;
