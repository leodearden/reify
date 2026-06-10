//! Scope-aware reference collection over the parsed AST (task α, 4201).
//!
//! This module is the foundation producer for find-references (β), rename (γ),
//! and occurrence-highlight (δ). It walks the **parsed** AST
//! ([`reify_ast::ParsedModule`]) rather than the compiled IR: the compiled IR is
//! value-only (it discards both source spans and identifier names), whereas every
//! parsed [`reify_ast::Expr`] carries a [`SourceSpan`] and
//! [`reify_ast::ExprKind::Ident`] gives the exact identifier-token span. Reference
//! collection is therefore a scope-aware walk over the parsed AST.
//!
//! Three public entry points:
//! - [`collect_references`] — every span (declaration ∪ uses) of the binding under
//!   the cursor, scoped to a single entity body.
//! - [`prepare_rename`] — the rename target token + placeholder, or `None` when the
//!   cursor is not on a renameable local value-member binding.
//! - [`compute_rename`] — a [`WorkspaceEdit`] covering declaration ∪ references.
//!
//! Declaration and reference spans are **name-token spans** (just the identifier),
//! not full member-statement spans, so rename edits stay minimal and uniform. This
//! deliberately differs from `goto_def`, which returns full-member spans for
//! navigation.

use std::collections::HashMap;

use reify_ast::{
    ConnectDecl, Declaration, Expr, ExprKind, ForallConnectBody, ForallConstraintBody,
    MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule, StringPart, SubDecl, WhereClause,
};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, TextEdit, Url,
    WorkspaceEdit,
};

use crate::analysis::{enclosing_decl_at, name_token_span};
use crate::completion::{BODY_KEYWORDS, EXPR_KEYWORDS, TOP_LEVEL_KEYWORDS};
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

/// The kind of symbol a [`ReferenceSet`] resolves to.
///
/// The enum is defined **complete** for the reify-lsp ↔ frontend seam (so β/γ/δ
/// have a stable contract), but only the value-member kinds (`Param`, `Let`,
/// `Auto`, `Sub`, `Port`) are reference-collected and rename-eligible in this
/// single-file foundation phase. The declaration-name kinds
/// (`Structure`/`Occurrence`/`Trait`/`Enum`/`Variant`/`Fn`) are classification-only
/// here; full cross-declaration + cross-file rename is deferred to phase κ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefSymbolKind {
    Structure,
    Occurrence,
    Trait,
    Enum,
    Variant,
    Fn,
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
/// mirroring [`compute_document_highlights`] — runs the scope-aware collection,
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
/// Factored out of [`collect_references`] so [`prepare_rename`] can reuse the
/// `position_to_offset` + `find_word_at_offset` lookup it has already performed,
/// instead of recomputing the cursor word a second time per request.
fn collect_references_at(
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
fn entity_members(decl: &Declaration) -> Option<&[MemberDecl]> {
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
        // Leaves with no sub-expressions.
        ExprKind::Ident(_)
        | ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. }
        | ExprKind::Undef => false,
    }
}

/// Return `true` when the cursor byte offset `off` falls on the `.member`
/// segment of a `MemberAccess` expression anywhere within `members` at any
/// nesting depth up to `MAX_MEMBER_NESTING_DEPTH`.
///
/// Mirrors the member-kind dispatch of `collect_uses` (470-566) AND the
/// nested-scope descent of `for_each_child_scope` (335-357): for each member,
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
                s.args.iter().any(|(_, a)| expr_member_segment_hit(a, off))
                    || s.spec_param_overrides
                        .iter()
                        .any(|(_, o)| expr_member_segment_hit(o, off))
                    || s.pose_expr
                        .as_ref()
                        .is_some_and(|p| expr_member_segment_hit(p, off))
                    || s.where_clause
                        .as_ref()
                        .is_some_and(|w| expr_member_segment_hit(&w.condition, off))
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
            // Not walked by `collect_uses` — no tracked binding.
            MemberDecl::Fn(_) | MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => false,
        };
        if direct_hit {
            return true;
        }
        // Recurse into nested member-list scopes exactly as for_each_child_scope
        // (335-357) and collect_uses do: GuardedGroup where/else bodies, Port body,
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

/// Invoke `visit` once for each nested member-list scope that lives directly
/// inside `member` — EXACTLY the member lists [`collect_uses`] descends into: a
/// `GuardedGroup`'s `where` and `else` branches, a `Port` body, a `Sub`'s
/// specialization body and each keyed block's overrides, and a
/// `MatchArmDeclGroup`'s per-arm member clusters.
///
/// [`collect_bindings_in_scope`] uses this to recurse into nested bindings, while
/// `collect_uses` recurses into the same lists (plus their member-level
/// expressions) to collect uses. Keeping the two traversals reading off the same
/// child-scope set is what stops a nested redeclaration from being invisible to
/// binding resolution (the regression pinned by
/// `collect_references_port_body_redeclaration_owns_its_scope`). The callback form
/// avoids allocating a `Vec` of child slices per member; each visited slice is
/// bounded by its own [`members_region`] when recursed.
fn for_each_child_scope(member: &MemberDecl, mut visit: impl FnMut(&[MemberDecl])) {
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
        _ => {}
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
/// Covers every expression-bearing member kind so the reference set is complete
/// (Invariant 2): param/let/constraint/objective expressions and their `where`
/// clauses, constraint-instantiation args, sub constructor args / specialization
/// overrides / pose / body, port frames and bodies, guarded `where`/`else`
/// branches, `forall` connect/constraint bodies, bare connect/chain elements,
/// and match-arm decl clusters. Recursion into nested member lists is bounded by
/// [`MAX_MEMBER_NESTING_DEPTH`], mirroring `reify_ast::find_named_member_span`.
///
/// Associated functions (`MemberDecl::Fn`) are intentionally NOT walked: a fn
/// body opens its own parameter scope (its params can shadow an entity binding),
/// which this single-file foundation does not model, so collecting uses there
/// could produce false positives (Invariant 1). Deferred to a later phase.
fn collect_uses(members: &[MemberDecl], name: &str, depth: usize, out: &mut Vec<SourceSpan>) {
    if depth > MAX_MEMBER_NESTING_DEPTH {
        return;
    }
    for member in members {
        match member {
            MemberDecl::Param(p) => {
                if let Some(default) = &p.default {
                    collect_idents_in_expr(default, name, out);
                }
                collect_uses_in_where(&p.where_clause, name, out);
            }
            MemberDecl::Let(l) => {
                collect_idents_in_expr(&l.value, name, out);
                collect_uses_in_where(&l.where_clause, name, out);
            }
            MemberDecl::Constraint(c) => {
                collect_idents_in_expr(&c.expr, name, out);
                collect_uses_in_where(&c.where_clause, name, out);
            }
            MemberDecl::ConstraintInst(c) => {
                for (_, arg) in &c.args {
                    collect_idents_in_expr(arg, name, out);
                }
                collect_uses_in_where(&c.where_clause, name, out);
            }
            MemberDecl::Sub(s) => collect_uses_in_sub(s, name, depth, out),
            MemberDecl::Minimize(m) => {
                collect_idents_in_expr(&m.expr, name, out);
                collect_uses_in_where(&m.where_clause, name, out);
            }
            MemberDecl::Maximize(m) => {
                collect_idents_in_expr(&m.expr, name, out);
                collect_uses_in_where(&m.where_clause, name, out);
            }
            // Recurse into guarded branches so uses inside `where`/`else` blocks
            // are collected (and later resolved to their innermost binding); the
            // guard condition itself is an outer-scope expression.
            MemberDecl::GuardedGroup(g) => {
                collect_idents_in_expr(&g.condition, name, out);
                collect_uses(&g.members, name, depth + 1, out);
                collect_uses(&g.else_members, name, depth + 1, out);
            }
            // Ports carry an optional placement frame and a nested member body.
            MemberDecl::Port(p) => {
                if let Some(frame) = &p.frame_expr {
                    collect_idents_in_expr(frame, name, out);
                }
                collect_uses(&p.members, name, depth + 1, out);
            }
            MemberDecl::Connect(c) => collect_uses_in_connect(c, name, out),
            MemberDecl::Chain(c) => {
                for el in &c.elements {
                    collect_idents_in_expr(el, name, out);
                }
            }
            MemberDecl::ForallConnect(f) => {
                collect_idents_in_expr(&f.collection, name, out);
                match &f.body {
                    ForallConnectBody::Connect(c) => collect_uses_in_connect(c, name, out),
                    ForallConnectBody::Chain(c) => {
                        for el in &c.elements {
                            collect_idents_in_expr(el, name, out);
                        }
                    }
                }
            }
            MemberDecl::ForallConstraint(f) => {
                collect_idents_in_expr(&f.collection, name, out);
                match &f.body {
                    ForallConstraintBody::Constraint(c) => {
                        collect_idents_in_expr(&c.expr, name, out);
                        collect_uses_in_where(&c.where_clause, name, out);
                    }
                    ForallConstraintBody::Instantiation(c) => {
                        for (_, arg) in &c.args {
                            collect_idents_in_expr(arg, name, out);
                        }
                        collect_uses_in_where(&c.where_clause, name, out);
                    }
                }
            }
            // Match-arm decl clusters (spec §6.4): the discriminant is an
            // outer-scope expression; each arm's member is recursed as a child.
            MemberDecl::MatchArmDeclGroup(g) => {
                collect_idents_in_expr(&g.discriminant, name, out);
                for arm in &g.arms {
                    collect_uses(std::slice::from_ref(&*arm.member), name, depth + 1, out);
                }
            }
            // See the fn-body note above — intentionally not walked.
            MemberDecl::Fn(_) => {}
            // Type-only / expression-free members: nothing to collect.
            MemberDecl::AssociatedType(_) | MemberDecl::MetaBlock(_) => {}
        }
    }
}

/// Collect uses inside an optional `where` clause condition.
fn collect_uses_in_where(where_clause: &Option<WhereClause>, name: &str, out: &mut Vec<SourceSpan>) {
    if let Some(w) = where_clause {
        collect_idents_in_expr(&w.condition, name, out);
    }
}

/// Collect uses inside a `connect`/`chain` declaration: both port-ref endpoints
/// and every named connector parameter value.
fn collect_uses_in_connect(c: &ConnectDecl, name: &str, out: &mut Vec<SourceSpan>) {
    collect_idents_in_expr(&c.left.expr, name, out);
    collect_idents_in_expr(&c.right.expr, name, out);
    for (_, param) in &c.params {
        collect_idents_in_expr(param, name, out);
    }
}

/// Collect uses inside a `sub` declaration: constructor args, specialization
/// param overrides, the `at` placement pose, the `where` guard, and any nested
/// specialization-body / keyed-block members (depth-bounded).
fn collect_uses_in_sub(s: &SubDecl, name: &str, depth: usize, out: &mut Vec<SourceSpan>) {
    for (_, arg) in &s.args {
        collect_idents_in_expr(arg, name, out);
    }
    for (_, ov) in &s.spec_param_overrides {
        collect_idents_in_expr(ov, name, out);
    }
    if let Some(pose) = &s.pose_expr {
        collect_idents_in_expr(pose, name, out);
    }
    collect_uses_in_where(&s.where_clause, name, out);
    if let Some(body) = &s.body {
        collect_uses(body, name, depth + 1, out);
    }
    for entry in &s.keyed_members {
        collect_uses(&entry.overrides, name, depth + 1, out);
    }
}

/// Recursively push the span of every `ExprKind::Ident(name)` in `expr`.
///
/// Covers every `ExprKind` that contains sub-expressions so no in-scope use is
/// missed (Invariant 2). The match is exhaustive (no wildcard) so a new
/// `ExprKind` variant is a compile error here rather than a silently-dropped
/// reference.
///
/// Known limitation: binder-introducing expressions (`Lambda` params,
/// `Quantifier` variables, `Match` pattern binders) open their own value scope,
/// which this foundation phase does not model. Their bodies are still walked for
/// completeness; a binder that shadows `name` is therefore not handled here and
/// is deferred along with the other nested-scope cases.
fn collect_idents_in_expr(expr: &Expr, name: &str, out: &mut Vec<SourceSpan>) {
    match &expr.kind {
        ExprKind::Ident(ident) => {
            if ident == name {
                out.push(expr.span);
            }
        }
        ExprKind::BinOp { left, right, .. } => {
            collect_idents_in_expr(left, name, out);
            collect_idents_in_expr(right, name, out);
        }
        ExprKind::UnOp { operand, .. } => collect_idents_in_expr(operand, name, out),
        ExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_idents_in_expr(arg, name, out);
            }
        }
        // The base of a member access (`h` in `h.diameter`) is an identifier use
        // of the sub/port/binding; the `.member` segment is a field name, not a
        // tracked binding, so it is not recursed into.
        ExprKind::MemberAccess { object, .. } => collect_idents_in_expr(object, name, out),
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_idents_in_expr(condition, name, out);
            collect_idents_in_expr(then_branch, name, out);
            collect_idents_in_expr(else_branch, name, out);
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            for item in items {
                collect_idents_in_expr(item, name, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                collect_idents_in_expr(k, name, out);
                collect_idents_in_expr(v, name, out);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            collect_idents_in_expr(object, name, out);
            collect_idents_in_expr(index, name, out);
        }
        ExprKind::Match { discriminant, arms } => {
            collect_idents_in_expr(discriminant, name, out);
            for arm in arms {
                collect_idents_in_expr(&arm.body, name, out);
            }
        }
        ExprKind::Lambda { body, .. } => collect_idents_in_expr(body, name, out),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            collect_idents_in_expr(collection, name, out);
            collect_idents_in_expr(predicate, name, out);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            collect_idents_in_expr(base, name, out);
            for arg in args {
                collect_idents_in_expr(arg, name, out);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => collect_idents_in_expr(qualifier, name, out),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            collect_idents_in_expr(object, name, out);
            collect_idents_in_expr(qualified, name, out);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                collect_idents_in_expr(l, name, out);
            }
            if let Some(u) = upper {
                collect_idents_in_expr(u, name, out);
            }
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            collect_idents_in_expr(object, name, out);
            for arg in args {
                collect_idents_in_expr(arg, name, out);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for arg in args {
                collect_idents_in_expr(arg, name, out);
            }
        }
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, v) in fields {
                collect_idents_in_expr(v, name, out);
            }
        }
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPart::Hole(e) = part {
                    collect_idents_in_expr(e, name, out);
                }
            }
        }
        // Leaves with no sub-expressions.
        ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Auto { .. }
        | ExprKind::Undef => {}
    }
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

/// Compute the rename target for the symbol under the cursor.
///
/// Conservative: returns `Some` only when the cursor resolves to a renameable
/// local value-member binding (`Param`/`Let`/`Auto`/`Sub`/`Port`); `None`
/// otherwise (keywords, literals, builtins, type names, structure names, and
/// imported/cross-module symbols).
pub fn prepare_rename(source: &str, parsed: &ParsedModule, pos: Position) -> Option<RenameTarget> {
    // Resolve the cursor word ONCE and reuse it for both the renameability gate
    // and the returned range. Going through `collect_references_at` (rather than
    // the public `collect_references`) avoids mapping the position to an
    // offset/word a second time per request.
    let offset = position_to_offset(source, pos);
    let (word_start, word) = find_word_at_offset(source, offset)?;

    // collect_references is the renameability gate. It returns None for anything
    // that does not resolve to a local value-member binding — keywords, numeric/
    // quantity literals, builtins/function names, type names, structure/
    // declaration names, and imported/cross-module symbols all fail to resolve —
    // which yields every Invariant-4 refusal for free.
    let refset = collect_references_at(source, parsed, offset, word, true)?;
    if !is_renameable(refset.kind) {
        return None;
    }

    // The rename target is the identifier token UNDER THE CURSOR (a use token, or
    // the declaration name-token when the cursor is on the declaration). Both are
    // name-token spans, so span_to_range over the cursor word gives the range the
    // editor should highlight and pre-fill.
    let range = span_to_range(
        source,
        SourceSpan::new(word_start as u32, (word_start + word.len()) as u32),
    );
    Some(RenameTarget {
        range,
        placeholder: word.to_string(),
    })
}

/// Whether a symbol kind is renameable in this single-file foundation phase: the
/// value-member kinds (`Param`/`Let`/`Auto`/`Sub`/`Port`). The declaration-name
/// kinds (`Structure`/`Occurrence`/`Trait`/`Enum`/`Variant`/`Fn`) lack precise
/// reference spans at sub-/type-reference sites in the current AST, so robust
/// cross-declaration + cross-file rename for them is deferred to phase κ.
///
/// Shared by [`prepare_rename`] and [`compute_rename`] so the two agree exactly
/// on what is renameable.
fn is_renameable(kind: RefSymbolKind) -> bool {
    matches!(
        kind,
        RefSymbolKind::Param
            | RefSymbolKind::Let
            | RefSymbolKind::Auto
            | RefSymbolKind::Sub
            | RefSymbolKind::Port
    )
}

/// Whether `new_name` is a legal Reify identifier suitable as a rename target.
///
/// `compute_rename` writes `new_name` verbatim into every declaration ∪ reference
/// name-token span, so a `new_name` that is not a legal identifier — empty,
/// containing whitespace or punctuation (`foo bar`, `foo-bar`), starting with a
/// digit (`2x`), or a reserved keyword (`let`, `param`, …) — would produce source
/// that no longer re-parses cleanly, violating the re-parse-clean half of
/// Invariant 5. LSP clients do not generally validate the proposed name against a
/// language's identifier grammar, so `compute_rename` (the producer the later
/// server wiring consumes) is the contract surface that must reject these before
/// emitting any edit.
///
/// Grammar: a non-empty token whose first byte is ASCII alphabetic or `_` and
/// whose remaining bytes are ASCII alphanumeric or `_` (mirroring
/// `convert::is_ident_byte`, which defines what `find_word_at_offset` treats as an
/// identifier), and which is not a reserved keyword.
fn is_valid_rename_identifier(new_name: &str) -> bool {
    let mut bytes = new_name.bytes();
    match bytes.next() {
        // First byte must be a letter or underscore (rejects empty and `2x`).
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
        _ => return false,
    }
    // Remaining bytes must all be identifier bytes (rejects whitespace and
    // punctuation: `foo bar`, `foo-bar`).
    if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return false;
    }
    // A reserved keyword would re-parse as that construct rather than an
    // identifier, corrupting the buffer.
    !is_reserved_keyword(new_name)
}

/// Whether `name` is a reserved Reify keyword (and therefore not a valid rename
/// target). Reuses the LSP completion crate's keyword tables — the same lists the
/// editor offers as completions — so the refusal set never drifts from them.
fn is_reserved_keyword(name: &str) -> bool {
    TOP_LEVEL_KEYWORDS.contains(&name)
        || BODY_KEYWORDS.contains(&name)
        || EXPR_KEYWORDS.contains(&name)
}

/// Compute a [`WorkspaceEdit`] that renames the binding under the cursor.
///
/// The edit covers declaration ∪ references with one `TextEdit` per name-token
/// span, all keyed by `uri`. Returns `None` when the cursor does not resolve to a
/// renameable local value-member binding.
pub fn compute_rename(
    source: &str,
    parsed: &ParsedModule,
    uri: &Url,
    pos: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    // A `new_name` that is not a legal Reify identifier — empty, whitespace,
    // punctuation, digit-leading, or a reserved keyword — would corrupt the
    // identifier tokens so the buffer no longer re-parses cleanly (violating the
    // re-parse-clean half of Invariant 5), so refuse it outright before producing
    // any edits.
    if !is_valid_rename_identifier(new_name) {
        return None;
    }

    // Guard with the same renameability check as prepare_rename so the two never
    // disagree on what is renameable.
    let refset = collect_references(source, parsed, pos, true)?;
    if !is_renameable(refset.kind) {
        return None;
    }

    // One TextEdit per declaration∪reference name-token span. `references`
    // already contains the declaration token (include_declaration=true) and is
    // sorted ascending by span.start, so the edits are ascending and
    // non-overlapping.
    let edits: Vec<TextEdit> = refset
        .references
        .iter()
        .map(|&span| TextEdit {
            range: span_to_range(source, span),
            new_text: new_name.to_string(),
        })
        .collect();

    let changes = HashMap::from([(uri.clone(), edits)]);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Compute the occurrence-highlight set for the symbol under the cursor.
///
/// Delegates to [`collect_references`] with `include_declaration = true` and maps
/// each name-token [`SourceSpan`] through [`span_to_range`] into a
/// [`DocumentHighlight`] tagged [`DocumentHighlightKind::TEXT`] (read/write-agnostic
/// — this foundation phase does not distinguish read from write occurrences).
/// Returns `None` when the cursor does not resolve to a local value-member binding
/// (keywords, literals, builtins, type/structure names, imported symbols),
/// mirroring the [`compute_rename`] producer split so the boundary invariant has a
/// pure unit-test home.
///
/// The PRD's "highlight set == references set restricted to the active document"
/// (boundary row 7) holds for free with no extra filtering: [`collect_references`]
/// walks a single [`ParsedModule`] scoped to one entity body, so every span it
/// returns is inherently in-document.
pub fn compute_document_highlights(
    source: &str,
    parsed: &ParsedModule,
    pos: Position,
) -> Option<Vec<DocumentHighlight>> {
    let refset = collect_references(source, parsed, pos, /* include_declaration = */ true)?;
    Some(
        refset
            .references
            .iter()
            .map(|&span| DocumentHighlight {
                range: span_to_range(source, span),
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{offset_to_position, span_to_range};
    use reify_core::ModulePath;

    /// Build the name-token span `[start, start + text.len())`.
    fn span_of(start: usize, text: &str) -> SourceSpan {
        SourceSpan::new(start as u32, (start + text.len()) as u32)
    }

    /// Byte offsets of every whole-word-ish occurrence of `needle` in `source`,
    /// ascending. `width`/`volume`/etc. never appear as substrings of other
    /// identifiers in the bracket fixture, so plain match_indices is exact here.
    fn occurrences(source: &str, needle: &str) -> Vec<usize> {
        source.match_indices(needle).map(|(i, _)| i).collect()
    }

    /// Whether `span` lies fully within the byte range `[lo, hi)`.
    fn within(span: SourceSpan, lo: usize, hi: usize) -> bool {
        (span.start as usize) >= lo && (span.end as usize) <= hi
    }

    // ─── κ (task 4210): cross-file references + rename scaffolding ────────────
    //
    // The CANONICAL SIGNAL (PRD boundary row 8): a `Hole` structure declared in
    // `parts.ri` is imported and constructed (`sub hole = Hole`) in `main.ri`.
    // Renaming `Hole`→`Bore` once must update the home declaration, the import
    // entity token, AND the `sub` construction site — across both files — and
    // both must re-parse clean. These fixtures + helpers mirror goto_def's
    // `mock_resolver` pattern so the pure cross-file collectors are unit-testable
    // with an in-memory workspace + an injectable import resolver (no filesystem).

    /// parts.ri — the home module declaring `structure Hole`.
    #[allow(dead_code)]
    const PARTS_SRC: &str = "structure Hole {\n    param diameter: Length = 10mm\n}";

    /// main.ri — imports `parts.Hole` and constructs it via `sub hole = Hole`.
    #[allow(dead_code)]
    const MAIN_SRC: &str = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";

    /// URI of the home module (`parts.ri`).
    #[allow(dead_code)]
    fn parts_uri() -> Url {
        Url::parse("file:///proj/parts.ri").unwrap()
    }

    /// URI of the importing module (`main.ri`).
    #[allow(dead_code)]
    fn main_uri() -> Url {
        Url::parse("file:///proj/main.ri").unwrap()
    }

    /// Build a `workspace_docs` open-document snapshot — the multi-document
    /// workspace view the cross-file collectors scan for importers — from named
    /// `(uri, source)` pairs.
    #[allow(dead_code)]
    fn workspace_docs(docs: &[(Url, &str)]) -> Vec<(Url, String)> {
        docs.iter()
            .map(|(uri, src)| (uri.clone(), (*src).to_string()))
            .collect()
    }

    /// Build a mock `resolve_import` closure mapping an import dot-path
    /// (e.g. `"parts"`) to its `(target_uri, target_source)`, mirroring
    /// goto_def tests' `mock_resolver`. Returns `None` for an unknown path.
    #[allow(dead_code)]
    fn mock_resolver(
        map: HashMap<String, (Url, String)>,
    ) -> impl Fn(&str) -> Option<(Url, String)> {
        move |path: &str| map.get(path).cloned()
    }

    /// The canonical two-file workspace: `parts.ri` + `main.ri` as a
    /// `workspace_docs` snapshot, paired with a resolver that maps the `parts`
    /// import path to `parts.ri`. Returned together so each cross-file test can
    /// drive the collectors from any of the three signal cursor positions.
    #[allow(dead_code)]
    fn canonical_workspace() -> (Vec<(Url, String)>, impl Fn(&str) -> Option<(Url, String)>) {
        let docs = workspace_docs(&[(parts_uri(), PARTS_SRC), (main_uri(), MAIN_SRC)]);
        let mut map = HashMap::new();
        map.insert("parts".to_string(), (parts_uri(), PARTS_SRC.to_string()));
        (docs, mock_resolver(map))
    }

    // --- step-3: collect_references basic single binding ---

    #[test]
    fn collect_references_basic_single_binding_width() {
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));

        let width = occurrences(source, "width");
        assert_eq!(width.len(), 4, "bracket fixture: 1 decl + 3 uses of width");

        // Cursor on the `width` USE in `let volume = width * height * thickness`.
        let pos = offset_to_position(source, width[1] as u32);

        let refset = collect_references(source, &parsed, pos, false)
            .expect("cursor on a width use should resolve to a ReferenceSet");
        assert_eq!(refset.name, "width");
        assert_eq!(refset.kind, RefSymbolKind::Param);
        // declaration is the name-token span of `param width` (the 1st occurrence).
        assert_eq!(refset.declaration, span_of(width[0], "width"));
        // references are the three USE spans, ascending; declaration EXCLUDED.
        assert_eq!(
            refset.references,
            vec![
                span_of(width[1], "width"),
                span_of(width[2], "width"),
                span_of(width[3], "width"),
            ]
        );
        assert!(
            !refset.references.contains(&refset.declaration),
            "include_declaration=false must exclude the declaration token"
        );
    }

    // --- step-5: include_declaration toggle (Boundary row 6) ---

    #[test]
    fn collect_references_include_declaration_toggle() {
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let width = occurrences(source, "width");
        let pos = offset_to_position(source, width[1] as u32);

        let without = collect_references(source, &parsed, pos, false)
            .expect("width use resolves (exclude declaration)");
        let with = collect_references(source, &parsed, pos, true)
            .expect("width use resolves (include declaration)");

        // The two sets differ by exactly the declaration token.
        assert_eq!(
            with.references.len(),
            without.references.len() + 1,
            "include_declaration=true adds exactly one span"
        );
        assert!(
            with.references.contains(&with.declaration),
            "include_declaration=true must contain the declaration token"
        );
        assert!(
            !without.references.contains(&without.declaration),
            "include_declaration=false must exclude the declaration token"
        );
        // The declaration is first in source, so it sorts to the front.
        assert_eq!(
            with.references.first(),
            Some(&with.declaration),
            "declaration token sorts to the front of the ascending set"
        );
        // The full set stays ascending by span.start.
        let mut ascending = with.references.clone();
        ascending.sort_by_key(|s| s.start);
        assert_eq!(with.references, ascending, "set must be ascending by start");
        // with == declaration ∪ without (as the sorted union).
        let expected = {
            let mut v = without.references.clone();
            v.push(with.declaration);
            v.sort_by_key(|s| s.start);
            v
        };
        assert_eq!(with.references, expected);
    }

    // --- step-7: cross-structure isolation (Boundary row 1 — scope soundness) ---

    #[test]
    fn collect_references_cross_structure_isolation() {
        // Two structures each declare a same-named `param width` used once in a
        // `let`. A reference query in one structure must never surface a span from
        // the other (Invariant 1: no cross-scope false positives at the entity
        // boundary).
        let source = "\
structure A {
    param width: Length = 1mm
    let a = width
}
structure B {
    param width: Length = 2mm
    let b = width
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("iso"));

        // A occupies [0, b_start); B occupies [b_start, len).
        let b_start = source.find("structure B").expect("B exists");
        let (a_lo, a_hi) = (0, b_start);
        let (b_lo, b_hi) = (b_start, source.len());

        let width = occurrences(source, "width");
        assert_eq!(
            width.len(),
            4,
            "two decls + two uses of width across A and B"
        );
        // [0]=A decl, [1]=A use, [2]=B decl, [3]=B use.

        // --- Cursor on A's `width` use → only A's spans. ---
        let pos_a = offset_to_position(source, width[1] as u32);
        let set_a = collect_references(source, &parsed, pos_a, true)
            .expect("A's width use resolves to a ReferenceSet");
        assert_eq!(set_a.name, "width");
        assert_eq!(set_a.kind, RefSymbolKind::Param);
        assert!(
            within(set_a.declaration, a_lo, a_hi),
            "A's declaration token must lie within A: {:?}",
            set_a.declaration
        );
        for s in &set_a.references {
            assert!(
                within(*s, a_lo, a_hi),
                "A's reference set must stay within A's byte range: {s:?}"
            );
            assert!(
                !within(*s, b_lo, b_hi),
                "A's reference set must not leak into B's byte range: {s:?}"
            );
        }

        // --- Cursor on B's `width` use → only B's spans. ---
        let pos_b = offset_to_position(source, width[3] as u32);
        let set_b = collect_references(source, &parsed, pos_b, true)
            .expect("B's width use resolves to a ReferenceSet");
        assert_eq!(set_b.name, "width");
        assert_eq!(set_b.kind, RefSymbolKind::Param);
        assert!(
            within(set_b.declaration, b_lo, b_hi),
            "B's declaration token must lie within B: {:?}",
            set_b.declaration
        );
        for s in &set_b.references {
            assert!(
                within(*s, b_lo, b_hi),
                "B's reference set must stay within B's byte range: {s:?}"
            );
            assert!(
                !within(*s, a_lo, a_hi),
                "B's reference set must not leak into A's byte range: {s:?}"
            );
        }
    }

    // --- step-9: flat shadowing (Boundary row 2) ---

    #[test]
    fn collect_references_flat_shadowing() {
        // A `let x` shadows an earlier same-named `param x` in one flat body
        // (CompilationScope: params register before lets, last registration wins).
        // Every use of `x` therefore binds to the `let`, not the `param`.
        let source = "\
structure S {
    param x: Length = 1mm
    let x = 2mm
    let uses = x + x
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("shadow"));

        let xs = occurrences(source, "x");
        assert_eq!(xs.len(), 4, "param x, let x, and two uses of x");
        // xs[0]=param decl token, xs[1]=let decl token, xs[2]/xs[3]=uses.

        // --- Cursor on a use of x (first x in `let uses = x + x`). ---
        let pos_use = offset_to_position(source, xs[2] as u32);
        let from_use = collect_references(source, &parsed, pos_use, false)
            .expect("use of x resolves to the shadowing let");
        // The binding is the innermost (shadowing) `let x`.
        assert_eq!(from_use.kind, RefSymbolKind::Let);
        assert_eq!(from_use.declaration, span_of(xs[1], "x"));
        // References are the two use spans; the `param x` decl token is excluded.
        assert_eq!(
            from_use.references,
            vec![span_of(xs[2], "x"), span_of(xs[3], "x")]
        );
        assert!(
            !from_use.references.contains(&span_of(xs[0], "x")),
            "param x decl token must not appear in the let's reference set"
        );

        // --- Cursor on the `param x` declaration token. ---
        let pos_param = offset_to_position(source, xs[0] as u32);
        let from_param = collect_references(source, &parsed, pos_param, false)
            .expect("param x declaration resolves to the param binding");
        assert_eq!(from_param.kind, RefSymbolKind::Param);
        assert_eq!(from_param.declaration, span_of(xs[0], "x"));
        // The param is shadowed: none of the let's use spans bind to it.
        assert!(
            !from_param.references.contains(&span_of(xs[2], "x")),
            "shadowed param must not own the let's use spans"
        );
        assert!(!from_param.references.contains(&span_of(xs[3], "x")));

        // The two binding's reference sets are disjoint.
        for s in &from_param.references {
            assert!(
                !from_use.references.contains(s),
                "param and let reference sets must be disjoint: {s:?}"
            );
        }
    }

    // --- amend (test_coverage): cursor on a FULLY-SHADOWED param declaration ---

    #[test]
    fn collect_references_shadowed_param_declaration_owns_only_itself() {
        // When a `param x` is fully shadowed by a same-named `let x` in one flat
        // body, every use of `x` binds to the let (last registration wins). So a
        // cursor on the SHADOWED param's declaration yields a reference set that is
        // ONLY the declaration token — it owns no uses — and a rename driven from
        // it edits just that declaration, leaving the let and its uses untouched.
        // This is a defensible consequence of the last-registration-wins model
        // (renaming the dead param does not, and must not, capture the live let's
        // uses); this test pins it so the behavior is intentional and locked.
        let source = "\
structure S {
    param x: Length = 1mm
    let x = 2mm
    let uses = x + x
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("shadowdecl"));
        let xs = occurrences(source, "x");
        assert_eq!(xs.len(), 4, "param x, let x, and two uses of x");
        // xs[0]=param decl, xs[1]=let decl, xs[2]/xs[3]=uses (bound to the let).

        let param_pos = offset_to_position(source, xs[0] as u32);

        // include_declaration=true → the reference set is EXACTLY the declaration
        // token: the shadowed param owns no uses.
        let with_decl = collect_references(source, &parsed, param_pos, true)
            .expect("the shadowed param declaration still resolves to its own binding");
        assert_eq!(with_decl.kind, RefSymbolKind::Param);
        assert_eq!(with_decl.declaration, span_of(xs[0], "x"));
        assert_eq!(
            with_decl.references,
            vec![span_of(xs[0], "x")],
            "the shadowed param owns only its declaration token — no uses"
        );

        // include_declaration=false → no references at all (the uses bind to the let).
        let without_decl = collect_references(source, &parsed, param_pos, false)
            .expect("shadowed param declaration resolves");
        assert!(
            without_decl.references.is_empty(),
            "a fully-shadowed param has zero uses: {:?}",
            without_decl.references
        );

        // The rename it drives edits ONLY the declaration token — not the let
        // declaration (xs[1]) nor the let's uses (xs[2], xs[3]).
        let uri = Url::parse("file:///shadowdecl.ri").unwrap();
        let edit = compute_rename(source, &parsed, &uri, param_pos, "renamed")
            .expect("the shadowed param declaration is renameable");
        let edits = edit
            .changes
            .expect("changes present")
            .get(&uri)
            .expect("edits for uri")
            .clone();
        assert_eq!(
            edits.len(),
            1,
            "renaming the shadowed param edits exactly its declaration token"
        );
        assert_eq!(
            edits[0].range,
            span_to_range(source, span_of(xs[0], "x")),
            "the sole edit targets the param declaration token"
        );
        for off in [xs[1], xs[2], xs[3]] {
            let forbidden = span_to_range(source, span_of(off, "x"));
            assert!(
                edits.iter().all(|e| e.range != forbidden),
                "the let declaration/uses must NOT be renamed: {forbidden:?}"
            );
        }

        // Applying the single edit yields source that re-parses clean — the
        // shadowed-declaration rename is non-corrupting (Invariant 5).
        let start = position_to_offset(source, edits[0].range.start);
        let end = position_to_offset(source, edits[0].range.end);
        let mut buffer = source.to_string();
        buffer.replace_range(start..end, "renamed");
        let reparsed = reify_syntax::parse(&buffer, ModulePath::single("shadowdecl"));
        assert!(
            reparsed.errors.is_empty(),
            "renaming only the shadowed param must re-parse clean: {:?}\n{buffer}",
            reparsed.errors
        );
    }

    // --- step-11: guarded-block visibility/shadowing (Boundary row 3) ---

    #[test]
    fn collect_references_guarded_block_shadowing() {
        // A guarded `param x` (inside `where cond { … }`) shadows the top-level
        // `param x`, but ONLY within the branch region. A use inside the branch
        // (`let inner = x`) binds to the guarded param; a use outside the branch
        // (`let outer = x`) binds to the top-level param. The two binding's
        // reference sets must be disjoint and neither may leak across the guard
        // boundary (the where/else clause of Invariant 1).
        let source = "\
structure S {
    param cond: Bool = true
    param x: Length = 1mm
    where cond {
        param x: Length = 2mm
        let inner = x
    }
    let outer = x
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("guard"));

        let xs = occurrences(source, "x");
        assert_eq!(
            xs.len(),
            4,
            "top param x, guarded param x, inner use, outer use"
        );
        // xs[0]=top decl, xs[1]=guarded decl, xs[2]=inner use, xs[3]=outer use.

        // --- Cursor on the GUARDED `param x` declaration token. ---
        let pos_guarded = offset_to_position(source, xs[1] as u32);
        let from_guarded = collect_references(source, &parsed, pos_guarded, false)
            .expect("guarded param x declaration resolves");
        assert_eq!(from_guarded.kind, RefSymbolKind::Param);
        assert_eq!(from_guarded.declaration, span_of(xs[1], "x"));
        // Only the inside-guard use binds to the guarded param.
        assert_eq!(
            from_guarded.references,
            vec![span_of(xs[2], "x")],
            "guarded param owns only the inside-branch use"
        );
        assert!(
            !from_guarded.references.contains(&span_of(xs[3], "x")),
            "the outside-guard use must not leak into the guarded param's set"
        );

        // --- Cursor on the TOP-LEVEL `param x` declaration token. ---
        let pos_top = offset_to_position(source, xs[0] as u32);
        let from_top = collect_references(source, &parsed, pos_top, false)
            .expect("top-level param x declaration resolves");
        assert_eq!(from_top.kind, RefSymbolKind::Param);
        assert_eq!(from_top.declaration, span_of(xs[0], "x"));
        // Only the outside-guard use binds to the top-level param.
        assert_eq!(
            from_top.references,
            vec![span_of(xs[3], "x")],
            "top-level param owns only the outside-branch use"
        );
        assert!(
            !from_top.references.contains(&span_of(xs[2], "x")),
            "the inside-guard use must not leak into the top-level param's set"
        );

        // The two reference sets are disjoint (neither crosses the guard boundary).
        for s in &from_top.references {
            assert!(
                !from_guarded.references.contains(s),
                "top and guarded reference sets must be disjoint: {s:?}"
            );
        }

        // --- Symmetric cursor-on-use checks: each use resolves to its binding. ---
        let pos_inner = offset_to_position(source, xs[2] as u32);
        let from_inner = collect_references(source, &parsed, pos_inner, false)
            .expect("inner use resolves to the guarded param");
        assert_eq!(
            from_inner.declaration,
            span_of(xs[1], "x"),
            "inner use must bind to the guarded param declaration"
        );

        let pos_outer = offset_to_position(source, xs[3] as u32);
        let from_outer = collect_references(source, &parsed, pos_outer, false)
            .expect("outer use resolves to the top-level param");
        assert_eq!(
            from_outer.declaration,
            span_of(xs[0], "x"),
            "outer use must bind to the top-level param declaration"
        );
    }

    // --- step-13: RefSymbolKind classification for value members ---

    #[test]
    fn collect_references_classifies_value_member_kinds() {
        // One structure exercising each value-member kind. Sub and Port
        // additionally have their name uses collected (a bare `Ident` and the base
        // `Ident` of a `MemberAccess`). Identifier names are chosen to avoid being
        // substrings of one another so `occurrences`/`find` stay exact.
        let source = "\
structure S {
    param plain: Length = 1mm
    let derived = 2mm
    param bore: Length = auto
    sub widget = Hole(diameter: 6mm)
    port mount : Flange { param d: Length = 5mm }
    let wm = widget.diameter
    let wb = widget
    let mb = mount
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("kinds"));

        // The classified kind of the binding whose declaration token is at the
        // first occurrence of `anchor`.
        let kind_at = |anchor: &str| {
            let off = source.find(anchor).expect("anchor present") as u32;
            let pos = offset_to_position(source, off);
            collect_references(source, &parsed, pos, false)
                .unwrap_or_else(|| panic!("cursor on {anchor:?} should resolve to a binding"))
                .kind
        };

        assert_eq!(kind_at("plain"), RefSymbolKind::Param, "param → Param");
        assert_eq!(kind_at("derived"), RefSymbolKind::Let, "let → Let");
        // `param bore: Length = auto` — its default is `ExprKind::Auto` → Auto.
        assert_eq!(kind_at("bore"), RefSymbolKind::Auto, "param = auto → Auto");
        assert_eq!(kind_at("widget"), RefSymbolKind::Sub, "sub → Sub");
        assert_eq!(kind_at("mount"), RefSymbolKind::Port, "port → Port");

        // --- Sub uses collected: MemberAccess base `widget` AND bare `widget`. ---
        let widget = occurrences(source, "widget");
        assert_eq!(widget.len(), 3, "sub decl + 2 uses (widget.diameter, widget)");
        let widget_pos = offset_to_position(source, widget[0] as u32);
        let widget_set =
            collect_references(source, &parsed, widget_pos, false).expect("sub widget resolves");
        assert_eq!(widget_set.kind, RefSymbolKind::Sub);
        assert_eq!(
            widget_set.references,
            vec![span_of(widget[1], "widget"), span_of(widget[2], "widget")],
            "both the MemberAccess base and the bare use of the sub are collected"
        );

        // --- Port use collected: bare `Ident` `mount`. ---
        let mount = occurrences(source, "mount");
        assert_eq!(mount.len(), 2, "port decl + 1 use");
        let mount_pos = offset_to_position(source, mount[0] as u32);
        let mount_set =
            collect_references(source, &parsed, mount_pos, false).expect("port mount resolves");
        assert_eq!(mount_set.kind, RefSymbolKind::Port);
        assert_eq!(mount_set.references, vec![span_of(mount[1], "mount")]);
    }

    // --- amend: Port reference reached through a MemberAccess base ---

    #[test]
    fn collect_references_port_member_access_base() {
        // Complements the Sub MemberAccess-base coverage above (`widget.diameter`)
        // with the Port case: a port reference reached through a MemberAccess base
        // (`mount` in `mount.d`) must be collected, alongside a bare port use.
        let source = "\
structure S {
    port mount : Flange { param d: Length = 5mm }
    let viabase = mount.d
    let bare = mount
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("portbase"));

        let mount = occurrences(source, "mount");
        assert_eq!(
            mount.len(),
            3,
            "port decl + MemberAccess base + bare use of mount"
        );

        // Cursor on the port declaration token.
        let pos = offset_to_position(source, mount[0] as u32);
        let set = collect_references(source, &parsed, pos, false)
            .expect("port mount resolves to a ReferenceSet");
        assert_eq!(set.kind, RefSymbolKind::Port);
        assert_eq!(
            set.references,
            vec![span_of(mount[1], "mount"), span_of(mount[2], "mount")],
            "the MemberAccess base (`mount` in `mount.d`) and the bare use are both collected"
        );
    }

    // --- step-15: prepare_rename happy-path + refusals (Boundary row 4, Invariant 4) ---

    #[test]
    fn prepare_rename_happy_path_and_refusals() {
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let width = occurrences(source, "width");

        // --- Happy path: cursor on a `width` USE → Some, range = that token. ---
        let use_off = width[1];
        let use_pos = offset_to_position(source, use_off as u32);
        let target =
            prepare_rename(source, &parsed, use_pos).expect("cursor on a width use is renameable");
        assert_eq!(target.placeholder, "width");
        assert_eq!(
            target.range,
            span_to_range(source, span_of(use_off, "width")),
            "range must be the LSP range of the width use token under the cursor"
        );

        // --- Happy path: cursor on the `param width` DECLARATION token → Some. ---
        let decl_off = width[0];
        let decl_pos = offset_to_position(source, decl_off as u32);
        let decl_target = prepare_rename(source, &parsed, decl_pos)
            .expect("cursor on the width declaration token is renameable");
        assert_eq!(decl_target.placeholder, "width");
        assert_eq!(
            decl_target.range,
            span_to_range(source, span_of(decl_off, "width")),
            "range must be the decl name-token range"
        );

        // --- Refusals (Invariant 4): nothing that fails to resolve to a local
        // value-member binding is renameable. Cursor on the first occurrence of
        // each anchor must return None. ---
        let refuses = |anchor: &str| {
            let off = source.find(anchor).expect("anchor present");
            let pos = offset_to_position(source, off as u32);
            prepare_rename(source, &parsed, pos)
        };
        assert!(
            refuses("param").is_none(),
            "keyword `param` is not renameable"
        );
        assert!(
            refuses("80mm").is_none(),
            "quantity literal `80mm` is not renameable"
        );
        assert!(
            refuses("box(").is_none(),
            "builtin/function `box` is not renameable"
        );
        assert!(
            refuses("Length").is_none(),
            "type name `Length` is not renameable"
        );
        assert!(
            refuses("Bracket").is_none(),
            "structure name `Bracket` is not renameable"
        );

        // --- Imported/cross-module symbol refusal: cursor on a `Hole` USE. ---
        // `Hole` in `sub s = Hole(...)` is the sub's structure name, not a local
        // value binding, so rename is refused (cross-module rename deferred to κ).
        let import_src = "\
import parts.Hole
structure S {
    sub s = Hole(diameter: 6mm)
}";
        let import_parsed = reify_syntax::parse(import_src, ModulePath::single("imp"));
        let hole_use = import_src
            .match_indices("Hole")
            .nth(1)
            .expect("second `Hole` occurrence is the use site")
            .0;
        let hole_pos = offset_to_position(import_src, hole_use as u32);
        assert!(
            prepare_rename(import_src, &import_parsed, hole_pos).is_none(),
            "imported symbol use `Hole` is not renameable in this phase"
        );
    }

    // --- step-17: compute_rename produces a covering WorkspaceEdit ---

    #[test]
    fn compute_rename_covers_declaration_and_references() {
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let uri = Url::parse("file:///bracket.ri").unwrap();
        let width = occurrences(source, "width");
        assert_eq!(width.len(), 4, "bracket fixture: 1 decl + 3 uses of width");

        // Cursor on a `width` use; rename to "span".
        let pos = offset_to_position(source, width[1] as u32);
        let edit = compute_rename(source, &parsed, &uri, pos, "span")
            .expect("renaming a width use yields a WorkspaceEdit");

        let changes = edit
            .changes
            .expect("WorkspaceEdit.changes must be populated");
        let edits = changes.get(&uri).expect("edits keyed by the document uri");

        // Exactly declaration ∪ references = N+1 = 4 TextEdits (decl + 3 uses).
        assert_eq!(
            edits.len(),
            4,
            "one TextEdit per declaration/reference name-token"
        );

        // Every edit replaces with the new name.
        for e in edits {
            assert_eq!(e.new_text, "span", "every TextEdit applies the new name");
        }

        // Ranges are the name-token ranges of declaration∪references, ascending
        // by source order (declaration token first).
        let expected: Vec<Range> = width
            .iter()
            .map(|&off| span_to_range(source, span_of(off, "width")))
            .collect();
        let actual: Vec<Range> = edits.iter().map(|e| e.range).collect();
        assert_eq!(
            actual, expected,
            "ranges must equal declaration∪references, ascending"
        );

        // Independent non-overlap + ascending check on the produced ranges.
        for pair in actual.windows(2) {
            let prev_end = (pair[0].end.line, pair[0].end.character);
            let next_start = (pair[1].start.line, pair[1].start.character);
            assert!(
                prev_end <= next_start,
                "edits must be non-overlapping and ascending: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }

        // Where prepare_rename refuses (cursor on the type name `Length`),
        // compute_rename returns None too.
        let type_off = source.find("Length").expect("Length type present");
        let type_pos = offset_to_position(source, type_off as u32);
        assert!(
            compute_rename(source, &parsed, &uri, type_pos, "span").is_none(),
            "compute_rename must refuse where prepare_rename refuses"
        );
    }

    // --- step-19: rename edit-validity (Boundary row 5, Invariant 5) ---

    #[test]
    fn compute_rename_edit_is_valid_and_reparses_clean() {
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        // Baseline: bracket_source parses with zero errors.
        assert!(
            parsed.errors.is_empty(),
            "baseline bracket_source must parse clean: {:?}",
            parsed.errors
        );
        let uri = Url::parse("file:///bracket.ri").unwrap();
        let width = occurrences(source, "width");

        // Original reference set at a width use (for the same-shape comparison).
        let orig = collect_references(
            source,
            &parsed,
            offset_to_position(source, width[1] as u32),
            false,
        )
        .expect("width use resolves originally");

        // Rename width → renamed_w.
        let new_name = "renamed_w";
        let pos = offset_to_position(source, width[1] as u32);
        let edit = compute_rename(source, &parsed, &uri, pos, new_name)
            .expect("rename yields a WorkspaceEdit");
        let edits = edit
            .changes
            .expect("changes present")
            .get(&uri)
            .expect("edits for uri")
            .clone();

        // Apply the TextEdits to the buffer in DESCENDING start order, so earlier
        // byte offsets stay valid as later ones are spliced.
        let mut to_apply: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|e| {
                (
                    position_to_offset(source, e.range.start),
                    position_to_offset(source, e.range.end),
                    e.new_text.as_str(),
                )
            })
            .collect();
        to_apply.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut buffer = source.to_string();
        for (start, end, text) in &to_apply {
            buffer.replace_range(*start..*end, text);
        }

        // Re-parse the edited buffer: zero ERROR nodes (Invariant 5). A consistent
        // identifier-for-identifier substitution preserves the grammar.
        let reparsed = reify_syntax::parse(&buffer, ModulePath::single("bracket"));
        assert!(
            reparsed.errors.is_empty(),
            "renamed buffer must re-parse with no errors: {:?}\n--- buffer ---\n{}",
            reparsed.errors,
            buffer
        );

        // collect_references at the renamed token yields the same-shape set.
        let new_occ = occurrences(&buffer, new_name);
        assert_eq!(
            new_occ.len(),
            width.len(),
            "renamed token count must match the original width occurrence count"
        );
        let new_set = collect_references(
            &buffer,
            &reparsed,
            offset_to_position(&buffer, new_occ[1] as u32),
            false,
        )
        .expect("renamed use resolves in the new buffer");
        assert_eq!(new_set.name, new_name, "binding name updated by rename");
        assert_eq!(new_set.kind, orig.kind, "kind unchanged by rename");
        assert_eq!(
            new_set.references.len(),
            orig.references.len(),
            "reference set cardinality must be preserved across rename"
        );

        // An empty or whitespace-only new name would delete/blank the identifiers
        // and corrupt the buffer, so compute_rename must refuse it (returns None).
        assert!(
            compute_rename(source, &parsed, &uri, pos, "").is_none(),
            "empty new name must be refused"
        );
        assert!(
            compute_rename(source, &parsed, &uri, pos, "   ").is_none(),
            "whitespace-only new name must be refused"
        );
    }

    // --- amend (robustness): compute_rename refuses illegal identifier new_names ---

    #[test]
    fn compute_rename_rejects_invalid_identifiers() {
        // compute_rename writes new_name verbatim into every name-token span, so a
        // new_name that is not a legal Reify identifier must be refused (None)
        // before any edit is produced — otherwise the renamed buffer no longer
        // re-parses cleanly (the re-parse-clean half of Invariant 5). LSP clients do
        // not generally validate the proposed name, so this guard lives in the
        // producer.
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));
        let uri = Url::parse("file:///bracket.ri").unwrap();
        let width = occurrences(source, "width");
        // Cursor on a renameable `width` use — only the new_name should gate here.
        let pos = offset_to_position(source, width[1] as u32);

        // Sanity: a legal identifier is accepted (so the refusals below are
        // attributable to the new_name, not the cursor).
        assert!(
            compute_rename(source, &parsed, &uri, pos, "span").is_some(),
            "a legal identifier new_name must be accepted"
        );
        assert!(
            compute_rename(source, &parsed, &uri, pos, "_ok2").is_some(),
            "underscore-leading alphanumeric identifier is legal"
        );

        // Illegal new_names: whitespace, punctuation, digit-leading, and reserved
        // keywords all fail the identifier grammar and must return None.
        for bad in [
            "",         // empty
            "   ",      // whitespace only
            "foo bar",  // embedded whitespace
            "foo-bar",  // punctuation
            "2x",       // leading digit
            "let",      // reserved keyword (body)
            "param",    // reserved keyword (body)
            "structure", // reserved keyword (top-level)
            "if",       // reserved keyword (expression)
        ] {
            assert!(
                compute_rename(source, &parsed, &uri, pos, bad).is_none(),
                "illegal/keyword new_name {bad:?} must be refused"
            );
        }
    }

    // --- step-21: completeness + determinism (Invariants 2 & 3) ---

    #[test]
    fn collect_references_completeness_and_determinism() {
        // A single structure whose `tracked` param is used across every
        // expression-bearing member kind the foundation must cover: another
        // param's default, a let value, two constraints, a minimize objective, a
        // maximize objective, a sub constructor arg, a guarded (`where`) branch,
        // and a port body. Every in-scope use must be collected (Invariant 2),
        // the output strictly ascending, and the call idempotent (Invariant 3).
        // `tracked` is not a substring of any other token, so match_indices is
        // exact. There is exactly one `tracked` binding (top-level), so every use
        // — including the guarded-branch and port-body uses — resolves to it.
        let source = "\
structure Probe {
    param tracked: Length = 1mm
    param enabled: Bool = true
    param derived: Length = tracked * 2
    let scaled = tracked + 3mm
    constraint tracked > 0mm
    constraint tracked < 50mm
    minimize tracked
    maximize tracked
    sub widget = Hole(diameter: tracked)
    where enabled {
        let inner = tracked
    }
    port mount : Flange {
        param d: Length = tracked
    }
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("probe"));
        // Precondition: the fixture must parse clean, else the completeness claim
        // would be meaningless.
        assert!(
            parsed.errors.is_empty(),
            "probe fixture must parse clean: {:?}",
            parsed.errors
        );

        let occ = occurrences(source, "tracked");
        assert_eq!(
            occ.len(),
            10,
            "1 declaration + 9 uses of tracked across every member kind"
        );

        // Cursor on the first USE (the `tracked` in `param derived = tracked * 2`).
        let pos = offset_to_position(source, occ[1] as u32);

        // include_declaration=true: the full set must be EXACTLY every textual
        // occurrence of `tracked` as a name-token span, ascending. This pins
        // "every in-scope use collected, none missed" to source.match_indices.
        let full = collect_references(source, &parsed, pos, true)
            .expect("a tracked use must resolve to a ReferenceSet");
        assert_eq!(full.name, "tracked");
        assert_eq!(full.kind, RefSymbolKind::Param);
        let expected_all: Vec<SourceSpan> = occ.iter().map(|&o| span_of(o, "tracked")).collect();
        assert_eq!(
            full.references, expected_all,
            "declaration ∪ every use must be collected, ascending — none missed"
        );

        // include_declaration=false: exactly the 9 use spans (declaration excluded).
        let uses_only = collect_references(source, &parsed, pos, false)
            .expect("tracked use resolves (exclude declaration)");
        let expected_uses: Vec<SourceSpan> =
            occ.iter().skip(1).map(|&o| span_of(o, "tracked")).collect();
        assert_eq!(
            uses_only.references, expected_uses,
            "all 9 uses collected across every member kind, declaration excluded"
        );

        // Strictly ascending by span.start (determinism, Invariant 3).
        for pair in full.references.windows(2) {
            assert!(
                pair[0].start < pair[1].start,
                "references must be strictly ascending: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }

        // Idempotent: two consecutive calls return identical vectors.
        let again = collect_references(source, &parsed, pos, true)
            .expect("second call must resolve identically");
        assert_eq!(
            full.references, again.references,
            "collect_references must be idempotent"
        );
    }

    // --- step-23: asymmetric scope recursion regression (port-body redeclaration) ---

    #[test]
    fn collect_references_port_body_redeclaration_owns_its_scope() {
        // Regression: binding collection must descend into the SAME nested member
        // lists the use walker descends into. `collect_uses` recurses into port
        // bodies, but `collect_bindings_in_scope` used to recurse only into guarded
        // branches — so a port-local redeclaration was never registered, and
        // `resolve_use` mis-attributed the port-body use to the OUTER binding.
        //
        // Here a port body redeclares `param width` (shadowing the entity-level
        // one) and uses it in `param inner`. The top-level `width` and the
        // port-local `width` must own disjoint reference sets, neither leaking
        // across the port boundary (Invariant 1 for nested scopes).
        let source = "\
structure S {
    param width: Length = 1mm
    param outer: Length = width
    port mount : Flange {
        param width: Length = 2mm
        param inner: Length = width
    }
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("portredec"));
        // Premise: a port body holding multiple params parses clean (verified
        // against examples/m8_ports.ri and the step-21 port-body fixture).
        assert!(
            parsed.errors.is_empty(),
            "port-redeclaration fixture must parse clean: {:?}",
            parsed.errors
        );

        // Four `width` offsets; "Scalar"/"Flange"/"outer"/"inner"/"mount" contain
        // no "width", so the count is exactly 4.
        let w = occurrences(source, "width");
        assert_eq!(
            w.len(),
            4,
            "top decl, top use, port-local decl, port-body use"
        );
        // w[0]=top decl, w[1]=top use (param outer default),
        // w[2]=port-local decl, w[3]=port-body use (param inner default).

        // --- (a) Cursor on the TOP-LEVEL decl → owns w[0] (decl) + w[1] (top use);
        // the port-body use w[3] is ABSENT. ---
        let top = collect_references(source, &parsed, offset_to_position(source, w[0] as u32), true)
            .expect("top-level width declaration resolves");
        assert_eq!(top.kind, RefSymbolKind::Param);
        assert_eq!(top.declaration, span_of(w[0], "width"));
        assert_eq!(
            top.references,
            vec![span_of(w[0], "width"), span_of(w[1], "width")],
            "top-level width owns its decl + the top-level use only"
        );
        assert!(
            !top.references.contains(&span_of(w[3], "width")),
            "the port-body use must NOT leak into the top-level width's set"
        );

        // --- (b) Cursor on the PORT-LOCAL decl token → owns w[2] (decl) + w[3]
        // (port-body use); the top-level use w[1] is ABSENT. ---
        let port =
            collect_references(source, &parsed, offset_to_position(source, w[2] as u32), true)
                .expect("port-local width declaration resolves");
        assert_eq!(port.kind, RefSymbolKind::Param);
        assert_eq!(port.declaration, span_of(w[2], "width"));
        assert_eq!(
            port.references,
            vec![span_of(w[2], "width"), span_of(w[3], "width")],
            "port-local width owns its decl + the port-body use only"
        );
        assert!(
            !port.references.contains(&span_of(w[1], "width")),
            "the top-level use must NOT leak into the port-local width's set"
        );

        // --- (c) The two reference sets are disjoint (share no span). ---
        for s in &top.references {
            assert!(
                !port.references.contains(s),
                "top-level and port-local reference sets must be disjoint: {s:?}"
            );
        }

        // --- Cursor-on-use resolves to the correct declaration on each side. ---
        let from_top_use =
            collect_references(source, &parsed, offset_to_position(source, w[1] as u32), false)
                .expect("top-level use resolves");
        assert_eq!(
            from_top_use.declaration,
            span_of(w[0], "width"),
            "the top-level use must bind to the top-level declaration"
        );
        let from_port_use =
            collect_references(source, &parsed, offset_to_position(source, w[3] as u32), false)
                .expect("port-body use resolves");
        assert_eq!(
            from_port_use.declaration,
            span_of(w[2], "width"),
            "the port-body use must bind to the port-local declaration"
        );
    }

    // --- regression (esc-4201-123): trailing connect/forall/chain/match member
    // span must extend entity_region so its uses resolve to the outer binding ---

    #[test]
    fn collect_references_trailing_forall_use_resolves_to_outer_not_shadow() {
        // Regression: `member_span` used to return None for Connect/Chain/
        // ForallConnect/ForallConstraint/MatchArmDeclGroup, even though
        // `collect_uses` walks all of them. `members_region` (which builds the flat
        // `entity_region` bounding box) therefore stopped at the last param/let/
        // sub/port/guarded member. A use living in a `forall`/connect/chain/match
        // member positioned AFTER that — here the `tracked` in the trailing
        // `forall v in tracked: …` — fell past `entity_region.end`, was contained by
        // NO binding region, and hit `resolve_use`'s last-registered fallback. With
        // a guarded redeclaration present (registered last, deepest), that fallback
        // mis-attributed the top-level forall use to the GUARDED binding (Invariant 1
        // violation). The fix spans those five member kinds so the trailing use
        // falls inside the depth-0 entity region and resolves to the outer binding.
        let source = "\
structure S {
    param tracked: Length = 1mm
    param enabled: Bool = true
    where enabled {
        param tracked: Length = 2mm
    }
    forall v in tracked: constraint v > 0mm
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("trailing"));
        assert!(
            parsed.errors.is_empty(),
            "trailing-forall fixture must parse clean: {:?}",
            parsed.errors
        );

        let t = occurrences(source, "tracked");
        assert_eq!(
            t.len(),
            3,
            "top decl, guarded decl, and the trailing forall-collection use"
        );
        // t[0]=top decl, t[1]=guarded decl, t[2]=use in `forall v in tracked`.

        // --- Cursor on the TOP-LEVEL decl → owns t[0] (decl) + t[2] (forall use).
        // Before the fix t[2] was attributed to the guarded binding instead. ---
        let top = collect_references(source, &parsed, offset_to_position(source, t[0] as u32), true)
            .expect("top-level tracked declaration resolves");
        assert_eq!(top.kind, RefSymbolKind::Param);
        assert_eq!(top.declaration, span_of(t[0], "tracked"));
        assert_eq!(
            top.references,
            vec![span_of(t[0], "tracked"), span_of(t[2], "tracked")],
            "top-level tracked owns its decl + the trailing forall use"
        );

        // --- Cursor on the GUARDED decl → owns t[1] only; the trailing forall use
        // t[2] must NOT leak into it (the mis-attribution the fix removes). ---
        let guarded =
            collect_references(source, &parsed, offset_to_position(source, t[1] as u32), false)
                .expect("guarded tracked declaration resolves");
        assert_eq!(guarded.declaration, span_of(t[1], "tracked"));
        assert!(
            guarded.references.is_empty(),
            "guarded tracked has no in-branch use and must NOT capture the forall use: {:?}",
            guarded.references
        );
        assert!(
            !guarded.references.contains(&span_of(t[2], "tracked")),
            "the trailing forall use must not be mis-attributed to the guarded binding"
        );

        // --- Cursor on the forall use itself resolves to the TOP-LEVEL decl. ---
        let from_use =
            collect_references(source, &parsed, offset_to_position(source, t[2] as u32), false)
                .expect("trailing forall use resolves");
        assert_eq!(
            from_use.declaration,
            span_of(t[0], "tracked"),
            "the trailing forall use must bind to the outer (top-level) declaration"
        );
    }

    // --- step-1 (δ): documentHighlight producer == in-doc references incl. decl ---

    #[test]
    fn compute_document_highlights_equals_in_doc_references() {
        // Boundary row 7: the occurrence-highlight set is EXACTLY the references
        // set restricted to the active document, INCLUDING the declaration token.
        // Because `collect_references` walks a single ParsedModule scoped to one
        // entity body, every span it returns is inherently in-document, so the
        // producer is a pure mapping of
        // collect_references(.., include_declaration=true).references through
        // `span_to_range`, each tagged `DocumentHighlightKind::TEXT`.
        let source = reify_test_support::bracket_source();
        let parsed = reify_syntax::parse(source, ModulePath::single("bracket"));

        let width = occurrences(source, "width");
        assert_eq!(width.len(), 4, "bracket fixture: 1 decl + 3 uses of width");

        // Cursor on a `width` USE (the one in `let volume = width * ...`).
        let pos = offset_to_position(source, width[1] as u32);

        let highlights = compute_document_highlights(source, &parsed, pos)
            .expect("cursor on a width use should produce document highlights");

        // The reference set (declaration ∪ uses) is the producer's source of truth.
        let refset = collect_references(source, &parsed, pos, true)
            .expect("width use resolves to a ReferenceSet");
        let expected_ranges: Vec<Range> = refset
            .references
            .iter()
            .map(|&span| span_to_range(source, span))
            .collect();
        let actual_ranges: Vec<Range> = highlights.iter().map(|h| h.range).collect();

        // Same ranges, same ascending order — the highlight set IS the in-doc
        // reference set (incl. declaration).
        assert_eq!(
            actual_ranges, expected_ranges,
            "highlight ranges must equal the in-doc reference set (incl. declaration), ascending"
        );
        // 1 declaration + 3 uses = 4 highlights.
        assert_eq!(highlights.len(), 4, "width has 1 declaration + 3 uses");
        // Every occurrence highlight is read/write-agnostic TEXT.
        for h in &highlights {
            assert_eq!(
                h.kind,
                Some(DocumentHighlightKind::TEXT),
                "every occurrence highlight is kind TEXT"
            );
        }

        // --- A non-resolvable cursor position yields None. ---
        // A type-name token (`Length`) does not resolve to a local value-member
        // binding, so collect_references → None → producer → None.
        let type_off = source.find("Length").expect("Length type present");
        let type_pos = offset_to_position(source, type_off as u32);
        assert!(
            compute_document_highlights(source, &parsed, type_pos).is_none(),
            "a type-name token is not resolvable, so it produces no highlights"
        );
        // A keyword (`structure`) is likewise non-resolvable.
        let kw_off = source.find("structure").expect("structure keyword present");
        let kw_pos = offset_to_position(source, kw_off as u32);
        assert!(
            compute_document_highlights(source, &parsed, kw_pos).is_none(),
            "a keyword is not resolvable, so it produces no highlights"
        );
    }

    // --- step-3 (task-4346): nested-scope refusal + control ---

    #[test]
    fn member_access_field_segment_refusal_descends_into_nested_scopes() {
        // (a) The member access lives inside a guarded `where` block; the colliding
        // binding is top-level. The flat scan (step-2) does NOT descend into the
        // where-block member list, so the guard fires only after the walker gains
        // nested-scope descent (step-4). This part is RED after step-2.
        let source_nested = "\
structure S {
    param diameter: Length = 5mm
    param enabled: Bool = true
    sub h = Hole(bore: 3mm)
    where enabled {
        let x = h.diameter
    }
}";
        let parsed_nested = reify_syntax::parse(source_nested, ModulePath::single("nested"));
        assert!(
            parsed_nested.errors.is_empty(),
            "nested fixture must parse clean: {:?}",
            parsed_nested.errors
        );

        // d[0]=`param diameter` decl, d[1]=`.diameter` inside the where-block let.
        let d = occurrences(source_nested, "diameter");
        assert_eq!(d.len(), 2, "1 param decl + 1 nested member-access segment");

        let member_pos = offset_to_position(source_nested, d[1] as u32);
        let uri = Url::parse("file:///nested.ri").unwrap();

        // All four producers must refuse the nested member-access segment.
        assert!(
            prepare_rename(source_nested, &parsed_nested, member_pos).is_none(),
            "(a) prepare_rename must refuse nested .field segment"
        );
        assert!(
            compute_rename(source_nested, &parsed_nested, &uri, member_pos, "renamed").is_none(),
            "(a) compute_rename must refuse nested .field segment"
        );
        assert!(
            compute_document_highlights(source_nested, &parsed_nested, member_pos).is_none(),
            "(a) compute_document_highlights must refuse nested .field segment"
        );
        assert!(
            collect_references(source_nested, &parsed_nested, member_pos, true).is_none(),
            "(a) collect_references must refuse nested .field segment"
        );

        // Over-refusal guard: the base `h` inside the where-block still resolves.
        let h_off = source_nested.find("h.diameter").expect("h.diameter present");
        let h_pos = offset_to_position(source_nested, h_off as u32);
        assert!(
            collect_references(source_nested, &parsed_nested, h_pos, false).is_some(),
            "(a) base `h` inside where-block must still resolve"
        );

        // (b) Control: no `param diameter` — the guard is still correct (None)
        // because there is no local binding to mis-resolve to.  This was already
        // None before the fix; it locks the contrast that None is the right
        // answer for a member segment regardless of whether a collision exists.
        let source_no_local = "\
structure S {
    sub h = Hole(bore: 3mm)
    let x = h.diameter
}";
        let parsed_no_local = reify_syntax::parse(source_no_local, ModulePath::single("nolocal"));
        assert!(
            parsed_no_local.errors.is_empty(),
            "(b) no-local fixture must parse clean: {:?}",
            parsed_no_local.errors
        );

        let seg_off = source_no_local.find("h.diameter").expect("h.diameter present")
            + "h.".len(); // point at `diameter`
        let seg_pos = offset_to_position(source_no_local, seg_off as u32);

        assert!(
            prepare_rename(source_no_local, &parsed_no_local, seg_pos).is_none(),
            "(b) prepare_rename must return None for .field when no local binding"
        );
        assert!(
            collect_references(source_no_local, &parsed_no_local, seg_pos, true).is_none(),
            "(b) collect_references must return None for .field when no local binding"
        );
    }

    // --- amendment (task-4346): non-GuardedGroup nested-scope coverage ---

    #[test]
    fn member_access_field_segment_refusal_descends_into_port_body() {
        // Complements `member_access_field_segment_refusal_descends_into_nested_scopes`,
        // which only exercises the GuardedGroup (`where`) descent arm.  This fixture
        // places `h.diameter` inside a Port body, exercising the `Port` arm of
        // `for_each_child_scope` in `cursor_on_member_segment`.  A regression that
        // dropped the Port arm while keeping GuardedGroup would pass the existing
        // nested-scope test but fail here.
        //
        // Note: Sub specialization bodies (`body: Some(...)` on `SubDecl`) are not
        // producible by the current parser (grammar update pending), so the Port arm
        // is the closest parser-reachable sibling to cover the child-scope descent.
        let source = "\
structure S {
    param diameter: Length = 5mm
    sub h = Hole(bore: 3mm)
    port out : Flange {
        param width: Length = h.diameter
    }
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("portbody"));
        assert!(
            parsed.errors.is_empty(),
            "port-body fixture must parse clean: {:?}",
            parsed.errors
        );

        // d[0]=`param diameter` decl (top-level), d[1]=`.diameter` in port-body param default.
        let d = occurrences(source, "diameter");
        assert_eq!(d.len(), 2, "1 param decl + 1 port-body member-access segment");

        let member_pos = offset_to_position(source, d[1] as u32);
        let uri = Url::parse("file:///portbody.ri").unwrap();

        // All four producers must refuse the port-body member-access .field segment.
        assert!(
            prepare_rename(source, &parsed, member_pos).is_none(),
            "prepare_rename must refuse .field segment inside port body"
        );
        assert!(
            compute_rename(source, &parsed, &uri, member_pos, "renamed").is_none(),
            "compute_rename must refuse .field segment inside port body"
        );
        assert!(
            compute_document_highlights(source, &parsed, member_pos).is_none(),
            "compute_document_highlights must refuse .field segment inside port body"
        );
        assert!(
            collect_references(source, &parsed, member_pos, true).is_none(),
            "collect_references must refuse .field segment inside port body"
        );

        // Over-refusal guard: the base `h` inside the port body still resolves as a Sub.
        let h_off = source.find("h.diameter").expect("h.diameter present");
        let h_pos = offset_to_position(source, h_off as u32);
        assert!(
            collect_references(source, &parsed, h_pos, false).is_some(),
            "base `h` inside port body must still resolve"
        );
    }

    // --- step-1 (task-4346): member-access .field segment refuses all four producers ---

    #[test]
    fn member_access_field_segment_refuses_when_colliding_with_local() {
        // Fixture: `param diameter` names a local binding; `h.diameter` is a
        // member-access where the `.diameter` segment shares the same text.
        // When the cursor sits on the `.diameter` segment (NOT the base `h`),
        // all four producers must refuse (return None / empty) — they must NOT
        // mis-resolve to the unrelated `param diameter` binding.
        let source = "\
structure S {
    param diameter: Length = 5mm
    sub h = Hole(bore: 3mm)
    let x = h.diameter
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("s"));
        assert!(
            parsed.errors.is_empty(),
            "member-access fixture must parse clean: {:?}",
            parsed.errors
        );

        // d[0] = `param diameter` decl token; d[1] = `.diameter` segment in `h.diameter`.
        let d = occurrences(source, "diameter");
        assert_eq!(d.len(), 2, "1 param decl + 1 member-access segment");

        let member_pos = offset_to_position(source, d[1] as u32);
        let uri = Url::parse("file:///s.ri").unwrap();

        // All four producers must refuse the member-access segment.
        assert!(
            prepare_rename(source, &parsed, member_pos).is_none(),
            "prepare_rename must refuse cursor on .field segment"
        );
        assert!(
            compute_rename(source, &parsed, &uri, member_pos, "renamed").is_none(),
            "compute_rename must refuse cursor on .field segment"
        );
        assert!(
            compute_document_highlights(source, &parsed, member_pos).is_none(),
            "compute_document_highlights must refuse cursor on .field segment"
        );
        assert!(
            collect_references(source, &parsed, member_pos, true).is_none(),
            "collect_references must refuse cursor on .field segment"
        );

        // Over-refusal guard: the BASE `h` (cursor at the start of `h.diameter`) must
        // still resolve as a Sub binding.
        let h_off = source.find("h.diameter").expect("h.diameter present");
        let h_pos = offset_to_position(source, h_off as u32);
        let h_set = collect_references(source, &parsed, h_pos, false)
            .expect("cursor on base `h` must resolve to a ReferenceSet");
        assert_eq!(
            h_set.kind,
            RefSymbolKind::Sub,
            "base `h` resolves as Sub binding"
        );
        let prepare_h = prepare_rename(source, &parsed, h_pos)
            .expect("prepare_rename on base `h` must return Some");
        assert_eq!(
            prepare_h.placeholder, "h",
            "prepare_rename placeholder is the base name `h`"
        );

        // Over-refusal guard: the `param diameter` DECL token (cursor at d[0]) must
        // still be renameable.
        let decl_pos = offset_to_position(source, d[0] as u32);
        let prepare_decl = prepare_rename(source, &parsed, decl_pos)
            .expect("prepare_rename on param diameter decl must return Some");
        assert_eq!(
            prepare_decl.placeholder, "diameter",
            "prepare_rename placeholder is `diameter` when on the decl"
        );
    }

    // --- step-1 (β, task 4202): compute_references maps a ReferenceSet to LSP Locations ---

    #[test]
    fn compute_references_maps_referenceset_to_locations() {
        // A single structure where one value-member binding (`base`) is used
        // multiple times. compute_references is the thin LSP wrapper over
        // collect_references: it must map each name-token SourceSpan in the
        // ReferenceSet to a Location { uri, range } via span_to_range, preserving
        // count and ascending order. collect_references already proves the
        // underlying counts (collect_references_basic_single_binding_width /
        // _include_declaration_toggle), so this pins only the span→Location mapping.
        let source = "\
structure S {
    param base: Length = 1mm
    let a = base
    let b = base
}";
        let uri = Url::parse("file:///refs.ri").unwrap();
        let parsed = reify_syntax::parse(source, ModulePath::single("refs"));
        let base = occurrences(source, "base");
        assert_eq!(base.len(), 3, "1 decl + 2 uses of base");
        // base[0]=param decl token, base[1]/base[2]=uses.

        // Cursor on the first USE of base (in `let a = base`).
        let pos = offset_to_position(source, base[1] as u32);

        // include_declaration=true → declaration ∪ uses = 3 Locations, ascending.
        let with = compute_references(source, &parsed, &uri, pos, true)
            .expect("cursor on a base use must resolve to Locations");
        let expected: Vec<Location> = base
            .iter()
            .map(|&off| Location {
                uri: uri.clone(),
                range: span_to_range(source, span_of(off, "base")),
            })
            .collect();
        assert_eq!(
            with, expected,
            "each Location.range must equal span_to_range of the name-token span, ascending"
        );
        // Every Location carries the document uri.
        assert!(
            with.iter().all(|l| l.uri == uri),
            "every Location.uri must equal the document uri"
        );

        // include_declaration=false → the declaration token drops; count is N-1.
        let without = compute_references(source, &parsed, &uri, pos, false)
            .expect("cursor on a base use resolves (exclude declaration)");
        assert_eq!(
            without.len(),
            with.len() - 1,
            "include_declaration=false drops exactly the declaration token"
        );
        let expected_uses: Vec<Location> = base
            .iter()
            .skip(1)
            .map(|&off| Location {
                uri: uri.clone(),
                range: span_to_range(source, span_of(off, "base")),
            })
            .collect();
        assert_eq!(
            without, expected_uses,
            "uses-only set, declaration token excluded"
        );

        // A cursor on a non-identifier byte (the `=` sign) resolves to nothing.
        let eq_off = source.find('=').expect("source contains an '='");
        let eq_pos = offset_to_position(source, eq_off as u32);
        assert!(
            compute_references(source, &parsed, &uri, eq_pos, true).is_none(),
            "a cursor off any identifier must return None"
        );
    }

    // --- κ step-1 (task 4210): single-file structure-name collector ---

    #[test]
    fn collect_structure_name_spans_decl_plus_same_file_sub_uses() {
        // `SubDecl.structure_name` is a plain `String` field, structurally
        // invisible to the Expr-walking `collect_uses`/`collect_idents_in_expr`.
        // The dedicated structure-name collector must surface the home
        // declaration token (`structure Hole`) PLUS each same-file
        // `sub _ = Hole` construction-site token, ascending by start, each
        // covering exactly `Hole`.
        let source = "\
structure Hole {
    param diameter: Length = 10mm
}
structure Assembly {
    sub a = Hole
    sub b = Hole
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("kappa1"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse clean: {:?}",
            parsed.errors
        );

        // `Hole` appears exactly 3×: the structure decl + 2 sub construction sites
        // (`param diameter` / `Assembly` contain no `Hole` substring).
        let hole = occurrences(source, "Hole");
        assert_eq!(hole.len(), 3, "1 structure decl + 2 sub construction sites");
        // hole[0]=`structure Hole` decl token, hole[1]=`sub a = Hole`,
        // hole[2]=`sub b = Hole`.

        let spans = collect_structure_name_spans(source, &parsed, "Hole");
        assert_eq!(
            spans,
            vec![
                span_of(hole[0], "Hole"),
                span_of(hole[1], "Hole"),
                span_of(hole[2], "Hole"),
            ],
            "decl token + both sub-use tokens, ascending, each covering exactly `Hole`"
        );
    }
}
