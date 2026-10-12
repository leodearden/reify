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
    ConstraintInstDecl, Declaration, DefaultDecl, Expr, ExprKind, FieldSource, FnDef,
    ForallConstraintBody, ImportKind, MAX_MEMBER_NESTING_DEPTH, MemberDecl, ParsedModule,
    TraitBoundRef, TypeExpr, TypeExprKind, TypeParamDecl, VariantPayload,
};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, TextEdit, Url, WorkspaceEdit,
};

use crate::analysis::name_token_span;
use crate::completion::{BODY_KEYWORDS, EXPR_KEYWORDS, TOP_LEVEL_KEYWORDS};
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

mod local;
mod walk;

pub use local::{RefSymbolKind, ReferenceSet, RenameTarget, collect_references, compute_references};
use local::collect_references_at;
use walk::{for_each_child_scope, for_each_direct_subexpr, for_each_member_direct_expr};

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

// ─── κ (task 4210): cross-file declaration-name references + rename ─────────────────
//
// The single-file producers above resolve VALUE-member bindings
// (param/let/auto/sub/port) within one entity body. The machinery below follows
// the import graph to resolve a DECLARATION name across
// files: its home declaration token, every same-file USE SITE, and — in each
// importing document — the import entity token plus that document's use sites.
// `collect_decl_name_spans` enumerates the use-site categories and is the ONE
// place that list is written down: #6539 widened it, and the surfaces that had
// re-listed it verbatim understated it for exactly as long.
//
// The collectors are PURE: the open-document set arrives as `workspace_docs`
// and target resolution as an injectable
// `resolve_import` closure (mirroring goto_def's pattern), so the whole
// cross-file scope logic is unit-testable with an in-memory workspace + a mock
// resolver. The server handlers assemble both from the live DocumentStore.

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
fn collect_decl_name_spans(source: &str, parsed: &ParsedModule, name: &str) -> Vec<SourceSpan> {
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
fn collect_decl_name_uses_in_decl(
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
fn import_exposes_entity(kind: &ImportKind, name: &str) -> bool {
    match kind {
        ImportKind::Entity(n) => n == name,
        ImportKind::Destructured(names) => names.iter().any(|n| n == name),
        ImportKind::EntityAliased { .. } | ImportKind::Module | ImportKind::Aliased { .. } => false,
    }
}

/// What the cursor resolves to for the κ cross-file producers.
enum CrossFileHome {
    /// A local value-member binding (`param`/`let`/`auto`/`sub`/`port`) — the
    /// single-file producers own it unchanged.
    ValueMember,
    /// A DECLARATION named `name`, homed in document `uri` whose full `source`
    /// is carried so the home-file collector can run over it.
    ///
    /// "Declaration" is the whole module-level namespace, not just `structure`:
    /// since #6539 this carries a Structure, Occurrence, Fn, Enum, Trait, Field,
    /// TypeAlias, Constraint, Purpose or Joint, and since #6534 a `structure
    /// def` nested one level inside a `purpose` body too. Which of those may be
    /// RENAMED is a narrower question, and the one place it is answered is
    /// [`is_renameable_cross_file`] — reaching this variant is not itself a
    /// rename verdict.
    Declaration { uri: Url, name: String, source: String },
}

/// Resolve the cursor (`offset` + the identifier `word` under it) to a cross-file
/// home (κ step-4): a local value-member binding, a declaration in THIS
/// document, or a declaration reached through an `import`.
///
/// Resolution order is deliberate: value-member bindings win first (so the
/// single-file semantics of `param`/`let`/`auto`/`sub`/`port` are untouched),
/// then a same-file declaration name, then an imported entity name resolved via
/// the injected `resolve_import` closure. The import arm covers BOTH the cursor
/// sitting on an `import` entity token and on a `sub _ = Name` construction site,
/// since both carry the same `word`. Returns `None` when the word names none of
/// these (keyword, literal, unresolved/cross-module word).
///
/// DIRECTIONAL ALIAS GAP — step 3 gates on [`import_exposes_entity`], which is
/// `false` for ALIASED imports (`import m.Name as Alias`). So resolution cannot
/// START from an aliased import's entity token: invoking references/rename with
/// the cursor exactly on the `Name` in `import parts.Hole as Bore` returns
/// `None`. This is asymmetric with [`collect_importer_decl_name_references`],
/// which DOES emit that aliased entity token into the reference set when
/// resolution starts elsewhere (e.g. the home declaration) — so the token is
/// part of the set yet is not a valid starting cursor. The asymmetry is
/// internally consistent with the κ step-6 boundary (an aliased entity binds a
/// distinct local name; only its entity token, not the alias or alias uses,
/// names the renamed entity). Allowing the start by resolving the ENTITY (not
/// the alias) through `resolve_import` here would restore symmetry; it is left
/// as a follow-up to keep κ's start-position contract test-covered.
fn resolve_cross_file_home(
    primary_source: &str,
    primary_parsed: &ParsedModule,
    primary_uri: &Url,
    offset: usize,
    word: &str,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<CrossFileHome> {
    // 1. A local value-member binding keeps its exact single-file semantics.
    if collect_references_at(primary_source, primary_parsed, offset, word, true).is_some() {
        return Some(CrossFileHome::ValueMember);
    }
    // 2. A declaration of any admitted kind in THIS document → home is the
    //    current file. Scanned through `primary_parsed`, which the server
    //    already parsed for this edit, rather than `find_declaration_name_span`
    //    — that helper is this same scan preceded by its own
    //    `parse_with_stdlib`, i.e. a second full parse of the primary buffer on
    //    an interactive path, and the server's cached parse is that same
    //    prelude-aware flavour.
    if crate::goto_def::decl_name_span_in(primary_parsed, primary_source, word).is_some() {
        return Some(CrossFileHome::Declaration {
            uri: primary_uri.clone(),
            name: word.to_string(),
            source: primary_source.to_string(),
        });
    }
    // 3. An imported (non-aliased) entity → resolve to its home module via the
    //    injected closure.
    for decl in &primary_parsed.declarations {
        if let Declaration::Import(import) = decl
            && import_exposes_entity(&import.kind, word)
            && let Some((home_uri, home_source)) = resolve_import(&import.path)
        {
            return Some(CrossFileHome::Declaration {
                uri: home_uri,
                name: word.to_string(),
                source: home_source,
            });
        }
    }
    None
}

/// Collect the declaration-name reference spans an importing document
/// `doc_source` contributes for a declaration named `name` homed at `home_uri`:
/// the import entity token plus, for non-aliased imports, every USE SITE in the
/// document, ascending by `span.start`.
///
/// The use sites are exactly the three categories
/// [`collect_decl_name_uses_in_decl`] fans out over — `sub _ = name`
/// construction sites, `constraint name(…)` instantiations, and every type
/// position (#6539) — because this calls that same fan-out. Sharing it with the
/// home document is deliberate: the two cannot disagree about what counts as a
/// use, so a rename driven from either end edits the same set.
///
/// SCOPE SOUNDNESS (Invariant 1, κ step-6): an import contributes ONLY when
/// `resolve_import(import.path)` resolves to `home_uri` — never by a bare
/// entity-name match. Two different modules can both export a `Hole`; keying on
/// the resolved target file (plus the entity name) is what keeps a same-named
/// entity from a DIFFERENT module out of the renamed set.
///
/// Aliased vs non-aliased imports differ:
/// - `import home.Name` / `import home.{Name, …}` (non-aliased) bind `Name`
///   locally, so the import entity token AND every same-file `sub _ = Name`
///   construction site are part of the entity's reference set.
/// - `import home.Name as Alias` binds the entity under a DISTINCT local name;
///   only the entity token (`Name`) names the renamed entity — the alias token
///   and its `sub _ = Alias` uses are a separate binding and are excluded.
///
/// PERFORMANCE — a cheap substring pre-filter skips fully parsing docs that
/// cannot contribute: every span this function emits is a name token for `name`
/// itself, so it requires `name` to appear LITERALLY in `doc_source`, and
/// `!doc_source.contains(name)` is a sound early-out (a
/// whole-word match implies a substring match — the pre-filter never drops a
/// real reference). On a workspace with many open docs this avoids re-parsing
/// every buffer just to discover it never imports the home entity.
fn collect_importer_decl_name_references(
    doc_source: &str,
    name: &str,
    home_uri: &Url,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Vec<SourceSpan> {
    // Cheap pre-filter: a doc that does not even mention `name` as a substring
    // can contribute no import-entity token and no `sub _ = name` construction
    // site, so skip the parse entirely. Sound because every emitted span
    // requires a literal occurrence of `name` in the source.
    if !doc_source.contains(name) {
        return Vec::new();
    }
    let parsed = reify_syntax::parse(doc_source, reify_core::ModulePath::single("_importer"));
    let mut spans = Vec::new();
    // Whether `name` is imported under that SAME local name (non-aliased) — only
    // then do this document's `sub _ = name` construction sites belong to the set.
    let mut exposes_under_same_name = false;
    for decl in &parsed.declarations {
        let Declaration::Import(import) = decl else {
            continue;
        };
        // Resolved-URI scope soundness: an import that resolves to a different
        // module (or fails to resolve) never contributes, even when the entity
        // name coincides.
        if resolve_import(&import.path).is_none_or(|(uri, _)| uri != *home_uri) {
            continue;
        }
        match &import.kind {
            ImportKind::Entity(n) if n == name => {
                spans.push(name_token_span(doc_source, import.span, name));
                exposes_under_same_name = true;
            }
            ImportKind::Destructured(names) if names.iter().any(|n| n == name) => {
                spans.push(name_token_span(doc_source, import.span, name));
                exposes_under_same_name = true;
            }
            ImportKind::EntityAliased { entity, .. } if entity == name => {
                // Entity token only — the alias is a distinct local binding, so
                // its construction sites are NOT part of this entity's set.
                spans.push(name_token_span(doc_source, import.span, name));
            }
            _ => {}
        }
    }
    // Use sites only when imported under the same (non-aliased) name — and then
    // the SAME fan-out the home document uses, so the two documents cannot
    // disagree about what counts as a use of the entity.
    if exposes_under_same_name {
        for decl in &parsed.declarations {
            collect_decl_name_uses_in_decl(decl, doc_source, name, &mut spans);
        }
    }
    spans.sort_by_key(|s| s.start);
    spans
}

/// Cross-file find-references over the import graph (κ, task 4210).
///
/// Resolves the cursor to a declaration "home" — a local declaration name, an
/// `import` entity token, or a `sub _ = Name` construction site — then unions:
/// the home document's declaration token + its same-file use sites, and, for
/// every OTHER document in `workspace_docs` that imports the home entity, that
/// document's import entity token + its use sites. Both halves run the one
/// fan-out described on `collect_decl_name_spans`, so "use site" means the
/// same thing in every document. Each span is mapped to an LSP [`Location`] in
/// its document. `include_declaration` controls whether the home declaration
/// token is part of the set, mirroring the single-file [`compute_references`]
/// contract.
///
/// When the cursor is on a local VALUE-member binding the call delegates to the
/// single-file [`compute_references`] (value members keep single-file scope).
/// Returns `None` when the cursor resolves to neither a value member nor a
/// resolvable declaration — and, since #6972, also when it resolves to a home
/// the DECLARATION-TOKEN oracle declines (a `Unit` reached through an import),
/// rather than reporting a set from which the declaration itself is missing.
/// The refusal is stated at the check.
///
/// PURE: the open-document set arrives as `workspace_docs` and target resolution
/// as the injected `resolve_import` closure (mirroring goto_def), so the whole
/// cross-file scope is unit-testable with an in-memory workspace + mock resolver.
pub fn compute_references_cross_file(
    primary_source: &str,
    primary_parsed: &ParsedModule,
    primary_uri: &Url,
    pos: Position,
    include_declaration: bool,
    workspace_docs: &[(Url, String)],
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<Vec<Location>> {
    let offset = position_to_offset(primary_source, pos);
    let (_word_start, word) = find_word_at_offset(primary_source, offset)?;

    match resolve_cross_file_home(
        primary_source,
        primary_parsed,
        primary_uri,
        offset,
        word,
        resolve_import,
    )? {
        // A local value-member binding keeps exact single-file semantics.
        CrossFileHome::ValueMember => compute_references(
            primary_source,
            primary_parsed,
            primary_uri,
            pos,
            include_declaration,
        ),
        CrossFileHome::Declaration {
            uri: home_uri,
            name,
            source: home_source,
        } => {
            // ONE parse of the home document, shared by all three questions
            // asked of it below. `find_declaration_name_span` and
            // `classify_decl_name` each carry their own `parse` and were being
            // called on this same string; both `_in` forms take the parse
            // instead (the flavour argument is on `classify_decl_name_in`).
            let home_parsed =
                reify_syntax::parse(&home_source, reify_core::ModulePath::single("_home"));
            let home_decl = crate::goto_def::decl_name_span_in(&home_parsed, &home_source, &name);

            // REFUSE A HOME THE ORACLE DECLINES. The home document declares
            // `name`, but `decl_name_span_in` will not locate its token —
            // today exactly the `Unit` kind, which it refuses so that rename
            // stays away from a unit whose literal-suffix uses no collector can
            // reach. `resolve_cross_file_home` step 3 never consults the oracle
            // (it keys only on `import_exposes_entity`), so without this the
            // set would be the importers' tokens with the DECLARATION ITSELF
            // absent even under `include_declaration = true` — one "reference"
            // that is not the declaration. Refusing wholesale matches what
            // `prepare_rename_cross_file` already does for the same kind.
            //
            // A home that does not declare `name` at all (a stale or
            // mis-resolved import) is a different case and keeps its existing
            // behaviour: `classify_decl_name_in` answers `None`, and the
            // importing documents' tokens are still reported.
            if home_decl.is_none() && classify_decl_name_in(&home_parsed, &name).is_some() {
                return None;
            }

            let mut locations = Vec::new();

            // Home document: declaration token + every same-file use site
            // (categories enumerated on `collect_decl_name_spans`).
            for span in collect_decl_name_spans(&home_source, &home_parsed, &name) {
                // include_declaration=false drops the home declaration token.
                if !include_declaration && Some(span) == home_decl {
                    continue;
                }
                locations.push(Location {
                    uri: home_uri.clone(),
                    range: span_to_range(&home_source, span),
                });
            }

            // Every OTHER open document that imports the home entity (matched by
            // RESOLVED home URI, never raw import path — Invariant 1).
            for (doc_uri, doc_source) in workspace_docs {
                if *doc_uri == home_uri {
                    continue; // home-file references already collected above
                }
                for span in collect_importer_decl_name_references(
                    doc_source,
                    &name,
                    &home_uri,
                    resolve_import,
                ) {
                    locations.push(Location {
                        uri: doc_uri.clone(),
                        range: span_to_range(doc_source, span),
                    });
                }
            }

            Some(locations)
        }
    }
}

/// Classify the declaration named `name` in `source` to a [`RefSymbolKind`],
/// for cross-file rename gating. Returns `None` when nothing declares `name`.
///
/// "Declaration" here means a MODULE-LEVEL name: every top-level `Declaration`
/// that declares one, plus the `structure def`s nested one level inside a
/// `purpose` body (#6534). The nested ones classify as
/// [`RefSymbolKind::Structure`] — which is what they are to the compiler, which
/// registers them in the module-level structure namespace and compiles them
/// into the same template table — so they need no new gate:
/// [`is_renameable_cross_file`] already admits `Structure`. Top-level names are
/// scanned FIRST, matching the precedence both goto-def scans use, so a name
/// clash resolves identically everywhere.
///
/// WILDCARD-FREE over all 14 `Declaration` variants. This function and
/// `goto_def::decl_name_span_in` are two per-kind allowlists over the
/// same enum — one deciding the rename GATE, the other the reference SET — and
/// a wildcard on either is a channel through which they drift apart silently.
/// (They did: this one lacked `Field`, which the other admitted.) With both
/// exhaustive, a new declaration kind is a compile error at both.
///
/// They still differ in exactly one place, deliberately: `Unit` is classified
/// here but refused there, so a unit's refusal is stated by
/// [`is_renameable_cross_file`] as a named verdict rather than falling out of an
/// unmatched arm. Reaching a unit home at all needs the import path, since the
/// same-document arm of `resolve_cross_file_home` consults the oracle first.
///
/// PARSE DIFFERENCE, and why it cannot decide kind admission: a caller may hand
/// this either parse flavour — a bare [`reify_syntax::parse`] or
/// `reify_compiler::parse_with_stdlib`, which the oracle uses. The latter is
/// `parse_with_prelude_enums` — it hands the parser the prelude's ENUM NAMES so
/// a qualified `E.V` lowers correctly, and injects NO prelude declarations into
/// `parsed.declarations`. Both therefore see exactly the user's own top-level
/// declarations; the difference is in expression/type SHAPE below the
/// declaration level, which no arm of this match (nor of the oracle's) inspects.
/// That is what lets [`compute_references_cross_file`] compare the two verdicts
/// over ONE parse of the home document.
fn classify_decl_name_in(parsed: &ParsedModule, name: &str) -> Option<RefSymbolKind> {
    let top_level = parsed.declarations.iter().find_map(|decl| {
        let (decl_name, kind) = match decl {
            Declaration::Structure(s) => (s.name.as_str(), RefSymbolKind::Structure),
            Declaration::Occurrence(o) => (o.name.as_str(), RefSymbolKind::Occurrence),
            Declaration::Trait(t) => (t.name.as_str(), RefSymbolKind::Trait),
            Declaration::Enum(e) => (e.name.as_str(), RefSymbolKind::Enum),
            Declaration::Function(f) => (f.name.as_str(), RefSymbolKind::Fn),
            Declaration::Field(f) => (f.name.as_str(), RefSymbolKind::Field),
            Declaration::Purpose(p) => (p.name.as_str(), RefSymbolKind::Purpose),
            Declaration::Constraint(c) => (c.name.as_str(), RefSymbolKind::Constraint),
            Declaration::Unit(u) => (u.name.as_str(), RefSymbolKind::Unit),
            Declaration::TypeAlias(a) => (a.name.as_str(), RefSymbolKind::TypeAlias),
            Declaration::Joint(j) => (j.name.as_str(), RefSymbolKind::Joint),
            // These three declare no name of their own: an import names an
            // entity homed elsewhere, a `default` names the type it applies to,
            // and a `module` names a module path.
            Declaration::Import(_) | Declaration::Default(_) | Declaration::Module(_) => {
                return None;
            }
        };
        (decl_name == name).then_some(kind)
    });
    top_level.or_else(|| {
        crate::analysis::purpose_nested_decl_names(parsed)
            .any(|(nested, _)| nested == name)
            .then_some(RefSymbolKind::Structure)
    })
}

/// [`classify_decl_name_in`] for a caller holding only the source text.
///
/// Parses and delegates. A caller that already has the document's
/// [`ParsedModule`] must call the `_in` form directly rather than pay a second
/// parse of the same string on an interactive path.
fn classify_decl_name(source: &str, name: &str) -> Option<RefSymbolKind> {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("_classify"));
    classify_decl_name_in(&parsed, name)
}

/// Whether a cross-file home of `kind` is renameable. Shared by
/// [`prepare_rename_cross_file`] and the cross-file rename producer so the two
/// agree on what is renameable.
///
/// THE ADMISSION RULE, and it is the only one: a kind is renameable cross-file
/// exactly when EVERY use-site form for that kind is collected.
/// [`compute_rename_cross_file`] uses the reference set as its exact EDIT set,
/// so an uncollected use is left stale by the rename — and Invariant 5 does not
/// catch it, because it only checks that the edited buffers re-PARSE clean,
/// which a dangling name does.
///
/// ADMITTED, with what discharges the rule for each:
/// - `Structure`/`Occurrence` — declaration token, `sub _ = Name` construction
///   sites, and every type position ([`collect_decl_name_spans`], #6539).
/// - `TypeAlias` — its uses are type positions only, and every `TypeExpr` root
///   is walked.
/// - `Constraint` — type positions plus `constraint Name(…)` instantiation
///   names, both collected (#6539).
/// - `Purpose`/`Joint` — VACUOUSLY: the grammar names a purpose only in
///   `purpose_declaration` and a joint only in `joint_definition`, so there is
///   no use-site form to collect and the declaration token alone is a COMPLETE
///   reference set. Not a gap; asserted as such by
///   `tests::cross_file_declaration_kind_admission_tracks_use_site_coverage`.
/// - Every single-file value-member kind, via [`is_renameable`] — those keep
///   their exact single-file semantics on this path.
///
/// REFUSED, and for two different reasons — worth keeping apart, because only
/// the first is a gap anyone can close here:
/// - `Trait`/`Enum`/`Fn`/`Variant`/`Field`: the rule is simply not DISCHARGED.
///   Their type positions ARE collected, but each is also used in VALUE
///   position (`E.Variant`, `f(x)`), which a different collector answers for
///   and which this function has no evidence about.
/// - `Unit`: the rule cannot be discharged from here at all. Its one use form
///   is a literal suffix, and `UnitExpr::Unit(String)` carries no span for any
///   collector to push — see `goto_def::decl_name_span_in`, which
///   refuses `Unit` outright for the same measurement. FIND-REFERENCES refuses
///   it too, in [`compute_references_cross_file`]: the two gates now agree on
///   `Unit` rather than leaving a unit home reporting importers' tokens with no
///   declaration in the set.
///
/// The one residual inside an ADMITTED kind is `TypeParamDecl.bounds`, stated
/// in [`collect_decl_name_spans`]' CAVEAT: a `T: Numeric` bound carries no span,
/// so renaming the TRAIT `Numeric` would leave it stale — which is part of why
/// `Trait` is refused above.
fn is_renameable_cross_file(kind: RefSymbolKind) -> bool {
    is_renameable(kind)
        || matches!(
            kind,
            RefSymbolKind::Structure
                | RefSymbolKind::Occurrence
                | RefSymbolKind::TypeAlias
                | RefSymbolKind::Constraint
                | RefSymbolKind::Purpose
                | RefSymbolKind::Joint
        )
}

/// Cross-file prepare-rename over the import graph (κ, task 4210).
///
/// Lifts the single-file cross-module refusal: a declaration name reached through
/// an `import` (its import entity token or a `sub _ = Name` construction site),
/// or the home declaration token itself, becomes a rename target — whereas the
/// single-file [`prepare_rename`] returns `None` for it (declaration/imported
/// kinds are not single-file renameable).
///
/// Resolution order mirrors [`compute_references_cross_file`]: try the
/// single-file [`prepare_rename`] first (value members keep their exact
/// semantics), then resolve a cross-file declaration home and admit it only when
/// its kind is renameable ([`is_renameable_cross_file`], which states the rule
/// and the per-kind verdicts). Refuses keywords, literals, type-annotation
/// positions, and words resolving to neither a local binding nor a resolvable
/// declaration.
///
/// PURE: target resolution arrives as the injected `resolve_import` closure
/// (mirroring goto_def), so the cross-file path is unit-testable with a mock
/// resolver.
pub fn prepare_rename_cross_file(
    primary_source: &str,
    primary_parsed: &ParsedModule,
    primary_uri: &Url,
    pos: Position,
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<RenameTarget> {
    // 1. Single-file value-member rename keeps its exact semantics.
    if let Some(target) = prepare_rename(primary_source, primary_parsed, pos) {
        return Some(target);
    }

    // 2. Cross-file declaration home — lift the single-file cross-module refusal.
    let offset = position_to_offset(primary_source, pos);
    let (word_start, word) = find_word_at_offset(primary_source, offset)?;
    let CrossFileHome::Declaration { name, source, .. } = resolve_cross_file_home(
        primary_source,
        primary_parsed,
        primary_uri,
        offset,
        word,
        resolve_import,
    )?
    else {
        // A value-member home was already handled by `prepare_rename` above
        // (every value-member kind is single-file renameable); reaching here
        // with one would mean it was non-renameable, so refuse uniformly.
        return None;
    };

    // Admit only the renameable declaration kinds; `is_renameable_cross_file`
    // owns the rule and the per-kind verdicts.
    if !is_renameable_cross_file(classify_decl_name(&source, &name)?) {
        return None;
    }

    // The rename target is the identifier token UNDER THE CURSOR — the home decl
    // token, an import entity token, or a `sub _ = Name` construction token.
    let range = span_to_range(
        primary_source,
        SourceSpan::new(word_start as u32, (word_start + word.len()) as u32),
    );
    Some(RenameTarget {
        range,
        placeholder: word.to_string(),
    })
}

/// Cross-file rename over the import graph (κ, task 4210): a [`WorkspaceEdit`]
/// that renames a DECLARATION (or a single-file value member) to `new_name`
/// across every OPEN document that references it. Which declaration kinds are
/// admitted is [`is_renameable_cross_file`]'s answer, not this one's — it is
/// wider than `structure` and narrower than every named kind.
///
/// SCOPE — open documents only. The guarantee is bounded to the `workspace_docs`
/// set, which the server populates exclusively from documents currently OPEN in
/// the editor (the open-doc snapshot). A file that imports and constructs the
/// renamed declaration but is NOT open is never edited, so the rename can leave
/// stale references to the old name in closed importers. This matches κ design
/// decision #5 (whole-tree on-disk enumeration of unopened importers is a
/// deferred follow-up); the substrate here is the open-doc set + resolved
/// targets. A consuming task that surfaces "only open files were updated" or
/// walks the workspace `.ri` tree would close this gap.
///
/// SCOPE — which use forms the edit set covers. The edit set is the reference
/// set, so it covers the declaration token, import entity tokens, `sub _ = Name`
/// construction sites, `constraint Name(…)` instantiations, and every type
/// position — the categories [`collect_decl_name_spans`] enumerates. The
/// type-position gap this block used to record as a live rename hazard was
/// closed by #6539; the one residual inside an admitted kind is
/// `TypeParamDecl.bounds`, which carries no span in the AST and is stated in
/// that function's CAVEAT. Which KINDS may be renamed at all is a separate
/// question, answered by [`is_renameable_cross_file`].
///
/// The edit set is exactly the cross-file reference set
/// ([`compute_references_cross_file`] with `include_declaration = true`) — the
/// home declaration token + every same-file use site, plus each importing
/// document's import entity token + that document's use sites — one
/// [`TextEdit`] per name-token span, grouped by document URI (ascending and
/// non-overlapping within each file). The SCOPE block above names the
/// categories; [`collect_decl_name_spans`] is where they are enumerated.
///
/// Two refusals keep the multi-file result re-parse-clean (Invariant 5):
/// `new_name` must be a legal Reify identifier ([`is_valid_rename_identifier`] —
/// rejecting empty/whitespace/punctuation/digit-leading/reserved-keyword), and
/// the cursor must resolve to a renameable home. Renameability is gated through
/// [`prepare_rename_cross_file`] so prepare and rename never disagree (value
/// members keep single-file semantics; declaration homes admit only the kinds
/// [`is_renameable_cross_file`] lists). Returns `None` on either refusal.
///
/// PURE: the open-document set arrives as `workspace_docs` and target resolution
/// as the injected `resolve_import` closure (mirroring goto_def), so the whole
/// cross-file rename is unit-testable with an in-memory workspace + mock
/// resolver.
pub fn compute_rename_cross_file(
    primary_source: &str,
    primary_parsed: &ParsedModule,
    primary_uri: &Url,
    pos: Position,
    new_name: &str,
    workspace_docs: &[(Url, String)],
    resolve_import: &dyn Fn(&str) -> Option<(Url, String)>,
) -> Option<WorkspaceEdit> {
    // A `new_name` that is not a legal Reify identifier would corrupt every
    // rewritten token so the buffers no longer re-parse cleanly — refuse before
    // producing any edits (the re-parse-clean half of Invariant 5).
    if !is_valid_rename_identifier(new_name) {
        return None;
    }

    // Renameability gate — share prepare's resolution so prepare and rename never
    // disagree on what is renameable (value member, or an admitted declaration
    // home — `is_renameable_cross_file` lists them).
    prepare_rename_cross_file(primary_source, primary_parsed, primary_uri, pos, resolve_import)?;

    // The cross-file reference set (declaration ∪ uses) is exactly the set of
    // name-token spans to rewrite.
    let locations = compute_references_cross_file(
        primary_source,
        primary_parsed,
        primary_uri,
        pos,
        /* include_declaration = */ true,
        workspace_docs,
        resolve_import,
    )?;

    // One TextEdit per location, grouped by document URI.
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for loc in locations {
        changes.entry(loc.uri).or_default().push(TextEdit {
            range: loc.range,
            new_text: new_name.to_string(),
        });
    }
    // Keep each file's edits ascending (and thus non-overlapping — every span is
    // a distinct name token) so the client applies them deterministically.
    for edits in changes.values_mut() {
        edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::test_fixtures::{NAMED_DECL_SNIPPETS, find_sub, occurrences, parse_one_clean, span_of};
    use crate::convert::{offset_to_position, span_to_range};
    use reify_core::ModulePath;
    use super::local::entity_members;
    use tower_lsp::lsp_types::Range;

    /// Assert that all four rename/reference producers (`prepare_rename`,
    /// `compute_rename`, `compute_document_highlights`, `collect_references`)
    /// refuse to resolve the cursor at `pos` — the shared `.member`-segment
    /// refusal contract every fixture in this file's `.member`-segment suite
    /// pins. `ctx` is interpolated into each failure message to name the
    /// specific scenario under test, e.g. `"inside port body"`.
    ///
    /// Builds its own dummy [`Url`] internally: none of the four producers
    /// read the URI before returning `None` on the refusal path under test
    /// here — `compute_rename` only touches `uri` after the shared
    /// `collect_references` gate it has in common with the other three has
    /// already passed — so the URI's exact value is immaterial to this
    /// assertion.
    ///
    /// Centralizes what were four independent ~22-line copies of this same
    /// assertion block (one per `.member`-segment fixture) so a fifth
    /// producer joining the refusal contract is a one-place edit instead of a
    /// find-and-fix across every fixture test.
    fn assert_all_producers_refuse(source: &str, parsed: &ParsedModule, pos: Position, ctx: &str) {
        let uri = Url::parse("file:///member-segment-refusal.ri").unwrap();
        assert!(
            prepare_rename(source, parsed, pos).is_none(),
            "prepare_rename must refuse .field segment {ctx}"
        );
        assert!(
            compute_rename(source, parsed, &uri, pos, "renamed").is_none(),
            "compute_rename must refuse .field segment {ctx}"
        );
        assert!(
            compute_document_highlights(source, parsed, pos).is_none(),
            "compute_document_highlights must refuse .field segment {ctx}"
        );
        assert!(
            collect_references(source, parsed, pos, true).is_none(),
            "collect_references must refuse .field segment {ctx}"
        );
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

    /// main.ri — imports `parts.Hole` and constructs it via `sub hole = Hole()`.
    ///
    /// The constructor uses the PARENTHESIZED form `Hole()` (not the bare
    /// `sub hole = Hole` the PRD row-8 shorthand writes): the bare, paren-less
    /// form is a Reify *syntax error*, so it never lowers to a `SubDecl` and
    /// carries no `structure_name` for the κ collector to surface. `Hole()` is a
    /// valid construction site (Hole's only `param` has a default), lowers to
    /// `SubDecl { structure_name: "Hole", .. }`, and the rename target
    /// `sub hole = Bore()` re-parses clean (Invariant 5).
    #[allow(dead_code)]
    const MAIN_SRC: &str = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n}";

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
    // test helper; the `impl Fn` closure can't live in a type alias, so the
    // tuple-with-resolver return shape stays inline behind an allow.
    #[allow(clippy::type_complexity)]
    fn canonical_workspace() -> (Vec<(Url, String)>, impl Fn(&str) -> Option<(Url, String)>) {
        let docs = workspace_docs(&[(parts_uri(), PARTS_SRC), (main_uri(), MAIN_SRC)]);
        let mut map = HashMap::new();
        map.insert("parts".to_string(), (parts_uri(), PARTS_SRC.to_string()));
        (docs, mock_resolver(map))
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

        // All four producers must refuse the port-body member-access .field segment.
        assert_all_producers_refuse(source, &parsed, member_pos, "inside port body");

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

        // All four producers must refuse the member-access segment.
        assert_all_producers_refuse(
            source,
            &parsed,
            member_pos,
            "when colliding with a local binding",
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

    // --- task #5579 step-3: member-access .field segment refuses inside an
    // indexed-sub domain expression ---

    #[test]
    fn member_access_field_segment_refuses_in_indexed_sub_domain() {
        // Fixture: `param count` collides in name with the `.count` member-access
        // segment inside the indexed-sub's domain expression `0..cfg.count`. The
        // cursor on that `.count` segment must refuse (None) on all four
        // producers — the identical wrong-rename vector as
        // `member_access_field_segment_refuses_when_colliding_with_local`, just
        // reached through `SubDecl::index_domain` instead of a plain `let` value.
        let source = "\
structure S {
    param count: Int = 4
    sub cfg = Cfg()
    sub xs[i in 0..cfg.count] = Hole(bore: 3mm)
    let other: Int = count
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("indexdom"));
        // Indexed subs also carry the interim #5482 diagnostic (see
        // `collect_references_reaches_indexed_sub_domain_use` above) — not a
        // parse failure, just a "not yet elaborated" marker, and not the
        // behavior under test here. Assert directly on the AST field the
        // guard clause must reach instead, so this test doesn't break for an
        // unrelated reason once #5482 elaboration lands.
        assert!(
            find_sub(&parsed, "xs").index_domain.is_some(),
            "fixture must actually lower an index_domain for the guard to refuse against"
        );

        // d[0] = `param count` decl, d[1] = `.count` segment in `cfg.count`,
        // d[2] = the unrelated `let other` use.
        let d = occurrences(source, "count");
        assert_eq!(
            d.len(),
            3,
            "1 param decl + 1 member-access segment + 1 unrelated use"
        );

        let member_pos = offset_to_position(source, d[1] as u32);

        // All four producers must refuse the `.count` segment inside the domain.
        assert_all_producers_refuse(source, &parsed, member_pos, "inside indexed-sub domain");

        // Over-refusal guard: the BASE `cfg` (start of `cfg.count`) must still
        // resolve as a Sub binding — the guard clause must refuse only the
        // `.count` segment, not the whole domain expression.
        let cfg_off = source.find("cfg.count").expect("cfg.count present");
        let cfg_pos = offset_to_position(source, cfg_off as u32);
        let cfg_set = collect_references(source, &parsed, cfg_pos, false)
            .expect("cursor on base `cfg` must resolve to a ReferenceSet");
        assert_eq!(
            cfg_set.kind,
            RefSymbolKind::Sub,
            "base `cfg` resolves as Sub binding"
        );
    }

    // --- task #5579 step-5: member-access .field segment refuses inside an
    // inline relate-block relation expression ---

    #[test]
    fn member_access_field_segment_refuses_in_inline_relate_block() {
        // Fixture: `param axis` collides in name with the `.axis` member-access
        // segment inside the inline `sub … at … where { }` relate block's
        // relation expression `concentric(bolt.axis, 1)` (task δ 4384). Same
        // wrong-rename vector as the indexed-sub domain case above (step-3),
        // reached through `SubDecl::relate_relations` instead of `index_domain`.
        let source = "\
structure S {
    param axis: Int = 4
    sub bolt : Bolt at auto where {
        concentric(bolt.axis, 1)
    }
    let other: Int = axis
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("relateblock"));
        assert!(
            parsed.errors.is_empty(),
            "inline relate-block fixture must parse clean: {:?}",
            parsed.errors
        );

        // d[0] = `param axis` decl, d[1] = `.axis` segment in `bolt.axis`,
        // d[2] = the unrelated `let other` use.
        let d = occurrences(source, "axis");
        assert_eq!(
            d.len(),
            3,
            "1 param decl + 1 member-access segment + 1 unrelated use"
        );

        let member_pos = offset_to_position(source, d[1] as u32);

        // All four producers must refuse the `.axis` segment inside the relate block.
        assert_all_producers_refuse(source, &parsed, member_pos, "inside inline relate block");

        // Over-refusal guard: the BASE `bolt` (start of `bolt.axis`) must still
        // resolve as a Sub binding — the guard clause must refuse only the
        // `.axis` segment, not the whole relation expression.
        let bolt_off = source.find("bolt.axis").expect("bolt.axis present");
        let bolt_pos = offset_to_position(source, bolt_off as u32);
        let bolt_set = collect_references(source, &parsed, bolt_pos, false)
            .expect("cursor on base `bolt` must resolve to a ReferenceSet");
        assert_eq!(
            bolt_set.kind,
            RefSymbolKind::Sub,
            "base `bolt` resolves as Sub binding"
        );
    }

    // --- task #5579 (amendment): member-access .field segment refuses inside
    // a keyed entry's `param_overrides` expression (reviewer-surfaced sibling
    // gap to the two refusal tests above) ---

    #[test]
    fn member_access_field_segment_refuses_in_keyed_param_override() {
        // Fixture: `param area` collides in name with the `.area` member-access
        // segment inside a KEYED entry's `param_overrides` value `cfg.area`.
        // Same wrong-rename vector as the indexed-sub-domain / inline-relate-
        // block cases above, reached through
        // `KeyedSubMemberEntry.param_overrides` instead of
        // `SubDecl::index_domain` / `relate_relations`.
        let source = "\
structure S {
    param area: Length = 4mm
    sub cfg = Cfg()
    sub b : Bearing {
        \"k\" => { bore = cfg.area }
    }
    let other: Length = area
}";
        let parsed = reify_syntax::parse(source, ModulePath::single("keyedmember"));
        assert!(
            parsed.errors.is_empty(),
            "keyed-override fixture must parse clean: {:?}",
            parsed.errors
        );
        let sub = find_sub(&parsed, "b");
        assert_eq!(
            sub.keyed_members.len(),
            1,
            "fixture must lower one keyed entry"
        );
        assert_eq!(
            sub.keyed_members[0].param_overrides.len(),
            1,
            "fixture must actually lower a param_overrides entry for the guard to refuse against"
        );

        // d[0] = `param area` decl, d[1] = `.area` segment in `cfg.area`,
        // d[2] = the unrelated `let other` use.
        let d = occurrences(source, "area");
        assert_eq!(
            d.len(),
            3,
            "1 param decl + 1 member-access segment + 1 unrelated use"
        );

        let member_pos = offset_to_position(source, d[1] as u32);

        // All four producers must refuse the `.area` segment inside the keyed override.
        assert_all_producers_refuse(source, &parsed, member_pos, "inside keyed param override");

        // Over-refusal guard: the BASE `cfg` (start of `cfg.area`) must still
        // resolve as a Sub binding — the guard clause must refuse only the
        // `.area` segment, not the whole override expression.
        let cfg_off = source.find("cfg.area").expect("cfg.area present");
        let cfg_pos = offset_to_position(source, cfg_off as u32);
        let cfg_set = collect_references(source, &parsed, cfg_pos, false)
            .expect("cursor on base `cfg` must resolve to a ReferenceSet");
        assert_eq!(
            cfg_set.kind,
            RefSymbolKind::Sub,
            "base `cfg` resolves as Sub binding"
        );
    }

    // --- κ step-1 (task 4210): single-file structure-name collector ---

    #[test]
    fn collect_decl_name_spans_decl_plus_same_file_sub_uses() {
        // `SubDecl.structure_name` is a plain `String` field, structurally
        // invisible to the Expr-walking `collect_uses`/`collect_idents_in_expr`.
        // The dedicated structure-name collector must surface the home
        // declaration token (`structure Hole`) PLUS each same-file
        // `sub _ = Hole` construction-site token, ascending by start, each
        // covering exactly `Hole`.
        // NB: the bare `sub x = Hole` form (no parens) parses clean only as the
        // final member of a block; two consecutive bare subs are a syntax error,
        // so multi-sub fixtures use the parenthesized constructor form (which the
        // completeness fixtures above also rely on).
        let source = "\
structure Hole {
    param diameter: Length = 10mm
}
structure Assembly {
    sub a = Hole(diameter: 6mm)
    sub b = Hole(diameter: 8mm)
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

        let spans = collect_decl_name_spans(source, &parsed, "Hole");
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

    /// Every collected span must cover EXACTLY the target name, and the collected
    /// set must be every whole-word occurrence of it in the fixture.
    ///
    /// Both halves matter. The set equality is the completeness half: each
    /// fixture below is written so that every occurrence of the target is either
    /// the declaration token or the one use under test, so a missed use is a
    /// missing element. The exact-cover check is the RENAME-SAFETY half:
    /// `compute_rename_cross_file` uses the reference set as its exact edit set,
    /// so a span one byte too wide — `Box<Hole>` instead of `Hole`, or `pp.Hole`
    /// instead of its last segment — is a DESTRUCTIVE edit, not a cosmetic one.
    fn assert_collects_every_occurrence(label: &str, source: &str, name: &str) {
        let parsed = reify_syntax::parse(source, ModulePath::single("t"));
        assert!(
            parsed.errors.is_empty(),
            "[{label}] fixture must parse clean, got {:?} for:\n{source}",
            parsed.errors
        );
        let expected: Vec<SourceSpan> = occurrences(source, name)
            .into_iter()
            .map(|at| span_of(at, name))
            .collect();
        assert!(
            expected.len() >= 2,
            "[{label}] fixture must hold a declaration AND at least one use, or it \
             asserts nothing:\n{source}"
        );
        let got = collect_decl_name_spans(source, &parsed, name);
        assert_eq!(
            got, expected,
            "[{label}] every occurrence of {name:?} must be collected, each span \
             covering exactly the name token:\n{source}"
        );
    }

    /// AXIS 1 — every `TypeExpr` ROOT reachable from a top-level declaration.
    ///
    /// Each row was probe-verified to parse clean, and each names the AST field
    /// the use travels through. A root missing from `collect_decl_name_uses`'s
    /// fan-out is a silently uncollected use site, which for an admitted kind is
    /// a stale rename.
    #[test]
    fn type_position_use_is_collected_from_every_type_expr_root() {
        const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
        // (label + the AST field under test, source tail, target name)
        let rows: &[(&str, String, &str)] = &[
            ("FnDef.return_type", format!("{DECL}fn mk(x: Length) -> Hole {{ x }}"), "Hole"),
            ("FnParam.type_expr", format!("{DECL}fn id(h: Hole) -> Hole {{ h }}"), "Hole"),
            (
                "FieldDef.domain_type",
                format!("{DECL}field def f : Hole -> Real {{ source = analytical {{ |p| p }} }}"),
                "Hole",
            ),
            ("UnitDecl.dimension_type", format!("{DECL}unit hoop : Hole"), "Hole"),
            ("TypeAliasDecl.type_expr", format!("{DECL}type H = Hole"), "Hole"),
            ("DefaultDecl.type_expr", format!("{DECL}default Hole = 1"), "Hole"),
            (
                "JointDofField.type_expr",
                format!("{DECL}joint ball(c: Point, d: Point) with orientation: Hole = coincident(c, d)"),
                "Hole",
            ),
            ("TypeParamDecl.default", format!("{DECL}type Foo<T = Hole> = T"), "Hole"),
            ("AssociatedTypeDecl.default_type", format!("{DECL}trait T1 {{ type A = Hole }}"), "Hole"),
            ("VariantPayload::Named", format!("{DECL}enum E {{ V {{ h: Hole }} }}"), "Hole"),
            (
                "LambdaParam.type_expr",
                format!(
                    "{DECL}field def f : Point3 -> Real {{ source = analytical {{ |p: Hole| 1.0 }} }}"
                ),
                "Hole",
            ),
            // The same root reached the other way — through a MEMBER's own
            // expression rather than a top-level declaration's — since the two
            // are separate legs of the fan-out and either could rot alone.
            (
                "LambdaParam.type_expr (member expression)",
                format!("{DECL}structure A {{\n    let f = |p: Hole| 1.0\n}}"),
                "Hole",
            ),
            // The two non-`TypeExpr` type references. `TraitDecl.refinements` is a
            // `Vec<SpannedIdent>` carrying exact per-name spans; `trait_bounds` is
            // a `Vec<TraitBoundRef>` whose `span` covers the whole bound and must
            // be narrowed.
            (
                "TraitDecl.refinements",
                "trait Physical { param mass : Mass }\ntrait Solid : Physical { param v : Volume }"
                    .to_string(),
                "Physical",
            ),
            (
                "StructureDef.trait_bounds",
                "trait Physical { param mass : Mass }\nstructure S : Physical {\n    param mass : Mass = 1kg\n}"
                    .to_string(),
                "Physical",
            ),
            (
                "OccurrenceDef.trait_bounds",
                "trait Physical { param mass : Mass }\noccurrence def O : Physical {\n    param mass : Mass = 1kg\n}"
                    .to_string(),
                "Physical",
            ),
            // A third non-`TypeExpr` type reference, beyond the two the plan
            // named: a port's type is a TRAIT name, resolved against the trait
            // registry by the compiler (`entity.rs`'s port-type check), so a
            // trait rename that left it behind would break the port.
            (
                "PortDecl.type_name",
                "trait Flange { param d : Length }\nstructure A {\n    port mount : Flange { param d: Length = 5mm }\n}"
                    .to_string(),
                "Flange",
            ),
        ];
        for (label, source, name) in rows {
            assert_collects_every_occurrence(label, source, name);
        }
    }

    /// AXIS 2 — every type-expression FORM, i.e. every shape the recursion and
    /// the name-narrowing rule must handle.
    ///
    /// `DimensionalOp` is exercised through a type alias rather than a `param`
    /// annotation: `param p : Hole / Time` is a probe-verified SYNTAX ERROR (a
    /// dimensional operator is not accepted in param-annotation position), so the
    /// alias root is the only place the form is reachable.
    ///
    /// The NAMESPACED row is the subtle one. `ts_parser`'s `namespaced_name_text`
    /// dot-JOINS `pp.Hole` into the single `String` "pp.Hole", so a bare
    /// declaration name never equals it and the use is silently dropped — while
    /// the user sees a perfectly ordinary reference. The emitted span must be the
    /// LAST SEGMENT alone, since that is the token a rename may rewrite.
    #[test]
    fn type_position_use_is_collected_for_every_type_expression_form() {
        const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
        let rows: &[(&str, String, &str)] = &[
            (
                "Named with type_args (Box<Hole> \u{2014} span covers the brackets, must narrow)",
                format!("{DECL}structure A {{\n    param p : Box<Hole>\n}}"),
                "Hole",
            ),
            (
                "DimensionalOp (Hole / Time)",
                format!("{DECL}type Q = Hole / Time"),
                "Hole",
            ),
            (
                "Function ((Hole) -> Real)",
                format!("{DECL}structure A {{\n    param p : (Hole) -> Real\n}}"),
                "Hole",
            ),
            (
                "QualifiedAssoc (Hole::Material)",
                format!("{DECL}structure A {{\n    param p : Hole::Material\n}}"),
                "Hole",
            ),
            (
                "namespaced (pp.Hole \u{2014} dot-joined into one String)",
                format!("{DECL}structure A {{\n    param p : pp.Hole\n}}"),
                "Hole",
            ),
            (
                "SubDecl.type_args (Wrap<Hole>)",
                format!("{DECL}structure A {{\n    sub s = Wrap<Hole>()\n}}"),
                "Hole",
            ),
            (
                "nested guarded scope",
                format!(
                    "{DECL}structure A {{\n    param f : Bool = true\n    where f {{\n        param p : Hole\n    }}\n}}"
                ),
                "Hole",
            ),
        ];
        for (label, source, name) in rows {
            assert_collects_every_occurrence(label, source, name);
        }
    }

    /// AXIS 3 — the two child regions of a PURPOSE that no collector reaches.
    ///
    /// `entity_members` admits Structure|Occurrence|Trait|Purpose, so a purpose's
    /// `members` are walked — but a `PurposeDef` has THREE child regions, and the
    /// other two are sibling vecs it never sees: `structures: Vec<StructureDef>`
    /// (a structure lexically nested in the purpose body, kept out of `members`
    /// by task 4639) and `defaults: Vec<DefaultDecl>` (kept out by task 4496,
    /// each carrying a real `DefaultDecl.type_expr` type reference).
    ///
    /// Every use below is therefore invisible today: a construction site, a type
    /// annotation and an ambient default, none of them exotic. This is the
    /// surface workstreams D and C share — a cross-file oracle that resolved a
    /// purpose-nested name while these uses stayed uncollected would rename the
    /// declaration and leave every one of them stale, which is precisely the
    /// hazard the collectors-before-oracle rule exists to prevent.
    #[test]
    fn use_inside_a_purpose_body_is_collected() {
        const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
        let rows: &[(&str, String, &str)] = &[
            (
                "PurposeDef.structures — `sub s = Hole()` construction site",
                format!(
                    "{DECL}purpose P() {{\n    structure def Nested {{\n        sub s = Hole()\n    }}\n}}"
                ),
                "Hole",
            ),
            (
                "PurposeDef.structures — `param p : Hole` type annotation",
                format!(
                    "{DECL}purpose P() {{\n    structure def Nested {{\n        param p : Hole\n    }}\n}}"
                ),
                "Hole",
            ),
            (
                "PurposeDef.defaults — `default Hole = …`",
                format!("{DECL}purpose P() {{\n    default Hole = 1\n}}"),
                "Hole",
            ),
            // All three at once, in a purpose that also takes a param, so the
            // three regions are pinned as coexisting rather than only one at a
            // time — a fan-out that handled each alone but dropped one when the
            // others were present would still be caught.
            (
                "all three regions together",
                format!(
                    "{DECL}purpose P(subject : Structure) {{\n    default Hole = 1\n    structure def Nested {{\n        param p : Hole\n        sub s = Hole()\n    }}\n}}"
                ),
                "Hole",
            ),
        ];
        for (label, source, name) in rows {
            assert_collects_every_occurrence(label, source, name);
        }
    }

    /// AXIS 4 — a CONSTRAINT-DEF instantiation, the third declaration-name use
    /// site whose name is a plain `String` rather than an expression.
    ///
    /// `ConstraintInstDecl { name: String, args, where_clause, span, … }`, and
    /// `for_each_member_direct_expr` yields only its `args` and `where_clause`,
    /// so the instantiation NAME is structurally invisible to every ident walk —
    /// the exact same shape as `SubDecl.structure_name`, which is why this
    /// traversal exists at all.
    ///
    /// WHAT THIS ASSERTS AND WHAT IT DELIBERATELY DOES NOT. The subject here is
    /// the COLLECTOR, so the use site is asserted exactly: present, and spanning
    /// the name token alone rather than the whole member statement — the
    /// rename-safety property, since `compute_rename_cross_file` uses the
    /// reference set as its exact edit set. The DECLARATION token is the
    /// ORACLE's to supply, so the declaration half is asserted as an
    /// IMPLICATION — if the oracle offers a home token, the set carries it.
    /// Written that way when `decl_name_span_in` still refused `Constraint`, so
    /// it was VACUOUSLY true; step-18 admitted the kind, and the implication
    /// became a real assertion with no edit — which is the point of phrasing a
    /// coupling in the direction that lets the gap be closed.
    ///
    /// Non-vacuity: the fixture is asserted to parse clean AND to really contain
    /// a `MemberDecl::ConstraintInst` named `Foo`, so grammar drift fails loudly
    /// here instead of turning the whole test into a pass over an empty AST.
    #[test]
    fn constraint_def_instantiation_is_collected_as_a_use_site() {
        const DECL: &str = "constraint def Foo {\n    param x : Length\n    x > 0mm\n}\n";
        let rows: &[(&str, String)] = &[
            (
                "instantiated in a structure member",
                format!(
                    "{DECL}structure A {{\n    param w : Length = 1mm\n    constraint Foo(x: w)\n}}"
                ),
            ),
            // Also through the purpose regions step-14 opened, so the two legs
            // are pinned as composing rather than each working alone.
            //
            // It must be a purpose-NESTED STRUCTURE, not the purpose body
            // itself: measured, `constraint Foo(x: 1mm)` written directly in a
            // purpose body lowers to `MemberDecl::Constraint` wrapping an
            // `ExprKind::FunctionCall`, NOT to a `ConstraintInst` — the
            // instantiation form is a structure-body form. A `FunctionCall`'s
            // callee is a `String` with no span of its own, so that spelling is
            // a separate, pre-existing use-site gap (filed as a follow-up), not
            // this arm's business.
            (
                "instantiated in a purpose-nested structure",
                format!(
                    "{DECL}purpose P(subject : Structure) {{\n    structure def Nested {{\n        constraint Foo(x: 1mm)\n    }}\n}}"
                ),
            ),
        ];
        for (label, source) in rows {
            let parsed = reify_syntax::parse(source, ModulePath::single("t"));
            assert!(
                parsed.errors.is_empty(),
                "[{label}] fixture must parse clean, got {:?} for:\n{source}",
                parsed.errors
            );
            let instantiates_foo = |members: &[MemberDecl]| {
                members
                    .iter()
                    .any(|m| matches!(m, MemberDecl::ConstraintInst(c) if c.name == "Foo"))
            };
            let found_inst = parsed.declarations.iter().any(|decl| {
                entity_members(decl).is_some_and(instantiates_foo)
                    || matches!(decl, Declaration::Purpose(p)
                        if p.structures.iter().any(|n| instantiates_foo(&n.members)))
            });
            assert!(
                found_inst,
                "[{label}] fixture must really contain a `constraint Foo(…)` \
                 instantiation member, or this test asserts nothing:\n{source}"
            );

            let use_at = *occurrences(source, "Foo")
                .last()
                .expect("fixture mentions Foo");
            let got = collect_decl_name_spans(source, &parsed, "Foo");
            assert!(
                got.contains(&span_of(use_at, "Foo")),
                "[{label}] the instantiation name token at {use_at} must be \
                 collected, got {got:?}:\n{source}"
            );
            // Rename safety: the whole member statement is `constraint Foo(x: w)`,
            // so a span wider than the name is a destructive edit set.
            for span in &got {
                assert_eq!(
                    &source[span.start as usize..span.end as usize],
                    "Foo",
                    "[{label}] every collected span must cover exactly the name \
                     token, got {span:?}:\n{source}"
                );
            }
            // The declaration half, as an implication over the oracle.
            if let Some(home) = crate::goto_def::find_declaration_name_span(source, "Foo") {
                assert!(
                    got.contains(&home),
                    "[{label}] once the oracle offers a home token it must be in \
                     the reference set, got {got:?}:\n{source}"
                );
            }
        }
    }

    // --- κ step-3 (task 4210): cross-file structure references from any signal cursor ---

    /// Sort `Location`s by (uri, start line, start char) so cross-file reference
    /// sets can be compared order-independently — the producer's emission order is
    /// an implementation detail; the *set* of Locations is the contract.
    fn sorted_locations(mut locs: Vec<Location>) -> Vec<Location> {
        locs.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then(a.range.start.line.cmp(&b.range.start.line))
                .then(a.range.start.character.cmp(&b.range.start.character))
        });
        locs
    }

    /// Build the `Location` for the whole-word `text` token starting at byte
    /// `start` in `source`, paired with `uri`.
    fn loc_at(uri: Url, source: &str, start: usize, text: &str) -> Location {
        Location {
            uri,
            range: span_to_range(source, span_of(start, text)),
        }
    }

    #[test]
    fn compute_references_cross_file_same_set_from_all_three_signal_cursors() {
        // CANONICAL SIGNAL (PRD boundary row 8): `Hole` declared in parts.ri,
        // imported + constructed (`sub hole = Hole`) in main.ri. Find-references
        // from ANY of the three signal cursor positions — the parts.ri decl token,
        // the main.ri `import parts.Hole` entity token, or the main.ri
        // `sub hole = Hole` construction site — must surface the SAME cross-file
        // reference set: parts.ri decl token + main.ri import entity token +
        // main.ri sub-use token (3 Locations, correct uris/ranges).
        let (docs, resolver) = canonical_workspace();
        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
        let parsed_main = reify_syntax::parse(MAIN_SRC, ModulePath::single("main"));

        let parts_decl = occurrences(PARTS_SRC, "Hole")[0]; // `structure Hole`
        let main_import = occurrences(MAIN_SRC, "Hole")[0]; // `import parts.Hole`
        let main_use = occurrences(MAIN_SRC, "Hole")[1]; // `sub hole = Hole`
        let expected = sorted_locations(vec![
            loc_at(parts_uri(), PARTS_SRC, parts_decl, "Hole"),
            loc_at(main_uri(), MAIN_SRC, main_import, "Hole"),
            loc_at(main_uri(), MAIN_SRC, main_use, "Hole"),
        ]);

        // (a) cursor on the main.ri `sub hole = Hole` construction site.
        let pos_a = offset_to_position(MAIN_SRC, main_use as u32);
        let got_a = compute_references_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            pos_a,
            true,
            &docs,
            &resolver,
        )
        .expect("(a) cursor on cross-file sub use resolves to a cross-file reference set");
        assert_eq!(sorted_locations(got_a), expected, "(a) sub-use cursor");

        // (b) cursor on the parts.ri `structure Hole` declaration token.
        let pos_b = offset_to_position(PARTS_SRC, parts_decl as u32);
        let got_b = compute_references_cross_file(
            PARTS_SRC,
            &parsed_parts,
            &parts_uri(),
            pos_b,
            true,
            &docs,
            &resolver,
        )
        .expect("(b) cursor on home declaration resolves to a cross-file reference set");
        assert_eq!(sorted_locations(got_b), expected, "(b) declaration cursor");

        // (c) cursor on the main.ri `import parts.Hole` entity token.
        let pos_c = offset_to_position(MAIN_SRC, main_import as u32);
        let got_c = compute_references_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            pos_c,
            true,
            &docs,
            &resolver,
        )
        .expect("(c) cursor on import entity token resolves to a cross-file reference set");
        assert_eq!(sorted_locations(got_c), expected, "(c) import entity cursor");
    }

    // --- task #6539: declaration names used in TYPE POSITION ---

    /// The `is_renameable_cross_file` CAVEAT, made executable — and retired.
    ///
    /// That doc block used to name this exact hazard: "If a structure name ALSO
    /// appears in a type-annotation / refinement position (e.g. `param p:
    /// Name`), renaming the structure rewrites the decl + construction sites but
    /// leaves that type-position use stale." The quote is kept here because this
    /// test is what discharged it; the CAVEAT itself now records the collector's
    /// one remaining residual instead. Structure is an ALREADY-ADMITTED kind, so
    /// this test needed no oracle widening — it was RED for the collector reason
    /// alone, which is what made it the right first assertion of workstream D.
    ///
    /// FIXTURE DISCIPLINE: cleanliness is asserted BEFORE anything else, and the
    /// `param p : Hole` annotation is asserted to be present IN THE AST rather
    /// than only in the source text. `test_fixtures::parse_one_clean` does not
    /// fit here — it requires a single declaration and this fixture needs an
    /// import plus a structure — so the same two assertions are made inline. A
    /// snippet broken by grammar drift would otherwise yield an `Assembly` with
    /// no `param p` at all and make the whole test pass vacuously.
    #[test]
    fn compute_references_cross_file_reports_a_structure_name_in_type_position() {
        let main_src =
            "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n    param p : Hole\n}";
        let parsed_main = reify_syntax::parse(main_src, ModulePath::single("main"));
        assert!(
            parsed_main.errors.is_empty(),
            "fixture must parse clean, got {:?}",
            parsed_main.errors
        );
        let assembly = parsed_main
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Structure(s) if s.name == "Assembly" => Some(s),
                _ => None,
            })
            .expect("fixture must declare `structure Assembly`");
        assert!(
            assembly.members.iter().any(|m| matches!(
                m,
                MemberDecl::Param(param)
                    if param.name == "p"
                        && param
                            .type_expr
                            .as_ref()
                            .is_some_and(|t| t.to_string() == "Hole")
            )),
            "fixture must carry `param p : Hole` as a TYPE POSITION in the AST, \
             not merely as source text; got members: {:?}",
            assembly.members.len()
        );

        let docs = workspace_docs(&[(parts_uri(), PARTS_SRC), (main_uri(), main_src)]);
        let mut map = HashMap::new();
        map.insert("parts".to_string(), (parts_uri(), PARTS_SRC.to_string()));
        let resolver = mock_resolver(map);

        let main_hits = occurrences(main_src, "Hole");
        assert_eq!(
            main_hits.len(),
            3,
            "main.ri holds the import token, the construction site and the type \
             annotation"
        );
        let expected = sorted_locations(vec![
            loc_at(
                parts_uri(),
                PARTS_SRC,
                occurrences(PARTS_SRC, "Hole")[0],
                "Hole",
            ),
            loc_at(main_uri(), main_src, main_hits[0], "Hole"),
            loc_at(main_uri(), main_src, main_hits[1], "Hole"),
            loc_at(main_uri(), main_src, main_hits[2], "Hole"),
        ]);

        // Cursor on the parts.ri home declaration token.
        let decl_offset = occurrences(PARTS_SRC, "Hole")[0];
        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
        let got = compute_references_cross_file(
            PARTS_SRC,
            &parsed_parts,
            &parts_uri(),
            offset_to_position(PARTS_SRC, decl_offset as u32),
            true,
            &docs,
            &resolver,
        )
        .expect("cursor on the home declaration token resolves a cross-file set");
        assert_eq!(
            sorted_locations(got),
            expected,
            "the type-annotation use must be reported alongside the declaration \
             and the construction site \u{2014} a rename's edit set is exactly this \
             reference set, so a missing use is a stale-rename bug"
        );
    }

    #[test]
    fn compute_references_cross_file_exclude_declaration_drops_home_decl_token() {
        // include_declaration=false drops ONLY the parts.ri home declaration token,
        // leaving the two main.ri use Locations (import entity token + sub use).
        let (docs, resolver) = canonical_workspace();
        let parsed_main = reify_syntax::parse(MAIN_SRC, ModulePath::single("main"));

        let main_import = occurrences(MAIN_SRC, "Hole")[0];
        let main_use = occurrences(MAIN_SRC, "Hole")[1];
        let expected = sorted_locations(vec![
            loc_at(main_uri(), MAIN_SRC, main_import, "Hole"),
            loc_at(main_uri(), MAIN_SRC, main_use, "Hole"),
        ]);

        let pos = offset_to_position(MAIN_SRC, main_use as u32);
        let got = compute_references_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            pos,
            false,
            &docs,
            &resolver,
        )
        .expect("cross-file references resolve with include_declaration=false");
        assert_eq!(
            sorted_locations(got),
            expected,
            "include_declaration=false drops the parts.ri home decl token"
        );
    }

    // --- κ step-5 (task 4210): cross-file scope soundness ---

    #[test]
    fn compute_references_cross_file_excludes_same_name_different_module() {
        // Invariant 1 (scope soundness): a DIFFERENT module's same-named `Hole`
        // must NOT be swept into the reference set when renaming parts.ri's
        // `Hole`. `other.ri` imports `widgets.Hole` (resolving — via the mock —
        // to a DIFFERENT file than parts.ri) and constructs it. Even though the
        // entity NAME matches, importer matching keys on the RESOLVED home URI,
        // so `other.ri`'s import token and `sub w = Hole()` use are excluded.
        // The legitimate importer `main.ri` (which imports `parts.Hole`) still
        // contributes — proving the resolver distinguishes the two modules.
        let other_src = "import widgets.Hole\nstructure X {\n    sub w = Hole()\n}";
        let widgets_uri = Url::parse("file:///proj/widgets.ri").unwrap();
        let other_uri = Url::parse("file:///proj/other.ri").unwrap();

        let docs = workspace_docs(&[
            (parts_uri(), PARTS_SRC),
            (main_uri(), MAIN_SRC),
            (other_uri.clone(), other_src),
        ]);
        let mut map = HashMap::new();
        map.insert("parts".to_string(), (parts_uri(), PARTS_SRC.to_string()));
        map.insert(
            "widgets".to_string(),
            (widgets_uri, PARTS_SRC.to_string()),
        );
        let resolver = mock_resolver(map);

        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
        let parts_decl = occurrences(PARTS_SRC, "Hole")[0];
        let main_import = occurrences(MAIN_SRC, "Hole")[0];
        let main_use = occurrences(MAIN_SRC, "Hole")[1];
        // EXACT expected set: parts.ri decl + main.ri import token + main.ri use.
        // NOTHING from other.ri (different module).
        let expected = sorted_locations(vec![
            loc_at(parts_uri(), PARTS_SRC, parts_decl, "Hole"),
            loc_at(main_uri(), MAIN_SRC, main_import, "Hole"),
            loc_at(main_uri(), MAIN_SRC, main_use, "Hole"),
        ]);

        // Rename from the parts.ri declaration token.
        let pos = offset_to_position(PARTS_SRC, parts_decl as u32);
        let got = compute_references_cross_file(
            PARTS_SRC,
            &parsed_parts,
            &parts_uri(),
            pos,
            true,
            &docs,
            &resolver,
        )
        .expect("declaration cursor resolves to a cross-file reference set");
        assert_eq!(
            sorted_locations(got),
            expected,
            "other.ri's `import widgets.Hole` (different module) must be excluded"
        );
    }

    #[test]
    fn compute_references_cross_file_aliased_import_contributes_entity_token_only() {
        // Aliased import: `import parts.Hole as Bore` binds the entity `Hole`
        // under the DISTINCT local name `Bore`. Renaming parts.ri's `Hole` must
        // touch the import ENTITY token (`Hole`) — it names the renamed entity —
        // but NOT the alias token (`Bore`) nor the `sub _ = Bore()` construction
        // sites, which reference the alias binding, not the entity itself.
        let aliased_src = "import parts.Hole as Bore\nstructure Y {\n    sub h = Bore()\n}";
        let aliased_uri = Url::parse("file:///proj/aliased.ri").unwrap();

        let docs = workspace_docs(&[(parts_uri(), PARTS_SRC), (aliased_uri.clone(), aliased_src)]);
        let mut map = HashMap::new();
        map.insert("parts".to_string(), (parts_uri(), PARTS_SRC.to_string()));
        let resolver = mock_resolver(map);

        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
        let parts_decl = occurrences(PARTS_SRC, "Hole")[0];
        // `Hole` appears exactly once in aliased.ri — the import entity token in
        // `import parts.Hole as Bore`. The alias `Bore` and its `sub h = Bore()`
        // use are a distinct binding and contribute nothing.
        let aliased_entity = occurrences(aliased_src, "Hole");
        assert_eq!(
            aliased_entity.len(),
            1,
            "aliased.ri: only the import entity token spells `Hole`"
        );
        let expected = sorted_locations(vec![
            loc_at(parts_uri(), PARTS_SRC, parts_decl, "Hole"),
            loc_at(aliased_uri.clone(), aliased_src, aliased_entity[0], "Hole"),
        ]);

        // Rename from the parts.ri declaration token.
        let pos = offset_to_position(PARTS_SRC, parts_decl as u32);
        let got = compute_references_cross_file(
            PARTS_SRC,
            &parsed_parts,
            &parts_uri(),
            pos,
            true,
            &docs,
            &resolver,
        )
        .expect("declaration cursor resolves to a cross-file reference set");
        assert_eq!(
            sorted_locations(got),
            expected,
            "aliased import contributes only the entity token `Hole`, not the alias or its uses"
        );
    }

    // --- κ step-7 (task 4210): cross-file prepare_rename lifts the refusal ---

    /// The `RenameTarget` for the whole-word `text` token starting at byte
    /// `start` in `source`: range over the token, placeholder = `text`.
    fn rename_target_at(source: &str, start: usize, text: &str) -> RenameTarget {
        RenameTarget {
            range: span_to_range(source, span_of(start, text)),
            placeholder: text.to_string(),
        }
    }

    #[test]
    fn prepare_rename_cross_file_lifts_cross_module_refusal() {
        // κ lifts the single-file cross-module refusal (is_renameable returns
        // None for structure/imported names): prepare_rename over the import
        // graph returns a rename target — range over the cursor token,
        // placeholder = current name — for a structure home reached from ANY of
        // the three signal cursors: the parts.ri decl token, the main.ri
        // `sub hole = Hole()` construction site, and the `import parts.Hole`
        // entity token.
        let (_docs, resolver) = canonical_workspace();
        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));
        let parsed_main = reify_syntax::parse(MAIN_SRC, ModulePath::single("main"));

        // (a) parts.ri `structure Hole` declaration token.
        let parts_decl = occurrences(PARTS_SRC, "Hole")[0];
        let got_a = prepare_rename_cross_file(
            PARTS_SRC,
            &parsed_parts,
            &parts_uri(),
            offset_to_position(PARTS_SRC, parts_decl as u32),
            &resolver,
        )
        .expect("(a) cross-module refusal lifted on the home declaration token");
        assert_eq!(got_a, rename_target_at(PARTS_SRC, parts_decl, "Hole"), "(a)");

        // (b) main.ri `sub hole = Hole()` construction-site use.
        let main_use = occurrences(MAIN_SRC, "Hole")[1];
        let got_b = prepare_rename_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            offset_to_position(MAIN_SRC, main_use as u32),
            &resolver,
        )
        .expect("(b) cross-module refusal lifted on the imported sub use");
        assert_eq!(got_b, rename_target_at(MAIN_SRC, main_use, "Hole"), "(b)");

        // (c) main.ri `import parts.Hole` entity token.
        let main_import = occurrences(MAIN_SRC, "Hole")[0];
        let got_c = prepare_rename_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            offset_to_position(MAIN_SRC, main_import as u32),
            &resolver,
        )
        .expect("(c) cross-module refusal lifted on the import entity token");
        assert_eq!(got_c, rename_target_at(MAIN_SRC, main_import, "Hole"), "(c)");
    }

    #[test]
    fn prepare_rename_cross_file_refuses_keyword_type_and_unresolved() {
        // The cross-file producer keeps the single-file refusals: a keyword, a
        // type-annotation position naming a declaration this workspace does not
        // declare, and a word that resolves to neither a local binding nor a
        // resolvable declaration all return None.
        let (_docs, resolver) = canonical_workspace();
        let parsed_parts = reify_syntax::parse(PARTS_SRC, ModulePath::single("parts"));

        // (a) keyword `structure` — never a rename target.
        let kw = occurrences(PARTS_SRC, "structure")[0];
        assert!(
            prepare_rename_cross_file(
                PARTS_SRC,
                &parsed_parts,
                &parts_uri(),
                offset_to_position(PARTS_SRC, kw as u32),
                &resolver,
            )
            .is_none(),
            "(a) keyword `structure` is not renameable"
        );

        // (b) type-annotation position `Length`. Since #6539 a type position is
        //     no longer refused for BEING one — it is a collected use site, and
        //     an alias named there is renameable. `Length` still refuses for a
        //     different and more basic reason: it is a PRELUDE dimension, so no
        //     top-level declaration in this workspace declares it and there is
        //     no home to rename. Kept as a row because "the cursor is on a type"
        //     must not become sufficient on its own.
        let ty = occurrences(PARTS_SRC, "Length")[0];
        assert!(
            prepare_rename_cross_file(
                PARTS_SRC,
                &parsed_parts,
                &parts_uri(),
                offset_to_position(PARTS_SRC, ty as u32),
                &resolver,
            )
            .is_none(),
            "(b) `Length` names no declaration in this workspace, so there is \
             no home to rename"
        );

        // (c) an imported structure use whose import does NOT resolve (the
        //     canonical resolver maps only `parts`, not `ghost`) → neither a
        //     local binding nor a resolvable structure.
        let ghost_src = "import ghost.Widget\nstructure Z {\n    sub w = Widget()\n}";
        let parsed_ghost = reify_syntax::parse(ghost_src, ModulePath::single("ghost_user"));
        let ghost_uri = Url::parse("file:///proj/ghost_user.ri").unwrap();
        let widget_use = occurrences(ghost_src, "Widget")[1]; // `sub w = Widget()`
        assert!(
            prepare_rename_cross_file(
                ghost_src,
                &parsed_ghost,
                &ghost_uri,
                offset_to_position(ghost_src, widget_use as u32),
                &resolver,
            )
            .is_none(),
            "(c) an unresolvable imported structure use is not renameable"
        );
    }

    /// Per-kind CROSS-FILE admission of a top-level declaration name, and the
    /// coverage rule that decides it. Successor to #6388's non-regression
    /// guard; same subject, opposite verdict for four of five kinds.
    ///
    /// HISTORY — what this test used to assert, and why. #6388 made same-file
    /// go-to-definition resolve top-level declaration names UNIFORMLY across
    /// all kinds, via its own scanner (`analysis::decl_name_and_span` +
    /// `goto_def::decl_name_token`). It deliberately did NOT widen
    /// `goto_def::decl_name_span_in`, the home oracle feeding
    /// `collect_decl_name_spans`, `resolve_cross_file_home` step 2 and the
    /// cross-file rename producer. At that time `collect_uses` /
    /// `collect_idents_in_expr` walked `ExprKind::Ident` in EXPRESSIONS only
    /// and `collect_decl_name_spans` added only `sub _ = Name` construction
    /// sites, so admitting a type-position-only kind would have reported the
    /// DECLARATION token with every use site absent — and because
    /// `compute_rename_cross_file` uses the reference set as its exact EDIT
    /// set, a rename would move the declaration while silently leaving each use
    /// stale. Invariant 5 cannot see that: it checks only that edited buffers
    /// re-PARSE clean, which a dangling name does. So this test asserted, per
    /// kind, that the split HELD.
    ///
    /// WHAT CHANGED (#6539, rolled up in #6972). The RULE is unchanged — a kind
    /// is admitted cross-file iff EVERY use-site form for that kind is
    /// collected — but the collectors moved, so the same rule now yields the
    /// opposite verdict for four kinds. `collect_decl_name_spans` collects
    /// every type position at every `TypeExpr` root, every `constraint Name(…)`
    /// instantiation, and both of a purpose's sibling child regions. The split
    /// that #6388 pinned is therefore RETIRED, not violated.
    ///
    /// TWO GATES, STILL ASSERTED SEPARATELY, because they are still separate
    /// code:
    /// - RENAME is gated by `classify_decl_name_in` + `is_renameable_cross_file`,
    ///   which never consult `decl_name_span_in`. The per-kind verdicts
    ///   below pin THAT gate.
    /// - The REFERENCE SET is gated by `decl_name_span_in`. The
    ///   coupling assertion at the tail pins that one. A rename-only assertion
    ///   cannot: a kind admitted to the home oracle alone leaves every
    ///   `prepare_rename*` call returning None, so the verdicts stay green while
    ///   the reference set silently goes incomplete.
    #[test]
    fn cross_file_declaration_kind_admission_tracks_use_site_coverage() {
        let (_docs, resolver) = canonical_workspace();

        // The five kinds whose uses live in TYPE position only (or, for Unit, in
        // a literal suffix) — the kinds #6388 refused wholesale — each paired
        // with its verdict under the coverage rule and the measurement behind
        // it. Names are FILTERED from the shared snippet table rather than
        // re-copied: that table was hoisted to module scope in `analysis`
        // precisely so it would have one home, and a third verbatim copy here
        // would reintroduce the lockstep-edit burden it was meant to remove.
        //
        // `meter` is the one refusal left, and it is refused for a STRONGER
        // reason than "not yet collected" — the use site is unreachable from
        // both ends (#6972, measured):
        //   - A unit's only use site is a suffixed literal (`5meter`), which
        //     lowers to `ExprKind::QuantityLiteral { value, unit: UnitExpr }`.
        //     That is a LEAF to `collect_idents_in_expr`, and its
        //     `UnitExpr::Unit(String)` (ast.rs:196) carries NO span, so there is
        //     nothing in the AST for any collector to push.
        //   - Even a spanned suffix would not help the user: `find_word_at_offset`
        //     FUSES `5meter` into the single word `"5meter"`, pinned by
        //     `goto_def::tests::
        //     goto_def_unit_suffixed_literal_does_not_resolve_to_its_unit_declaration`,
        //     so goto-def and rename cannot be INVOKED from that position at all.
        // Closing it needs a spanned `UnitExpr` plus a suffix-aware word
        // splitter, neither of which is a change here.
        const ADMISSION: [(&str, bool, &str); 5] = [
            (
                "Pressure",
                true,
                "TypeAlias — every use is a type position, and every `TypeExpr` \
                 root is collected (#6539)",
            ),
            (
                "Foo",
                true,
                "Constraint — type positions plus `constraint Foo(…)` \
                 instantiation names, both collected (#6539)",
            ),
            (
                "lightweight",
                true,
                "Purpose — VACUOUSLY complete: `purpose` appears only in its own \
                 declaration production (grammar.js `purpose_declaration`), so \
                 the grammar admits no use site to miss",
            ),
            (
                "ball",
                true,
                "Joint — vacuously complete for the same reason (grammar.js \
                 `joint_definition` is the sole production naming a joint)",
            ),
            (
                "meter",
                false,
                "Unit — the literal-suffix use site is both uncollectable and \
                 unreachable; see this test's table comment",
            ),
        ];
        let rows: Vec<(&(&str, &str), bool, &str)> = ADMISSION
            .iter()
            .map(|(name, admitted, why)| {
                let snippet = NAMED_DECL_SNIPPETS
                    .iter()
                    .find(|(_, n)| n == name)
                    .unwrap_or_else(|| {
                        panic!(
                            "every name in ADMISSION must still have a row in \
                             NAMED_DECL_SNIPPETS; a renamed or dropped row would \
                             silently shrink this guard, but {name:?} has none"
                        )
                    });
                (snippet, *admitted, *why)
            })
            .collect();

        let uri = Url::parse("file:///proj/guard.ri").unwrap();
        for ((source, name), admitted, why) in &rows {
            // `parse_one_clean` asserts cleanliness FIRST: a snippet broken by
            // grammar drift would yield zero declarations, and every assertion
            // below would then pass — or refuse — vacuously.
            let parsed = parse_one_clean(source, "guard");

            let decl = occurrences(source, name)[0];
            let pos = offset_to_position(source, decl as u32);

            // SINGLE-FILE rename is unchanged by all of this and still refuses
            // every declaration kind: `prepare_rename` admits only value-member
            // bindings, so same-file goto-def navigability has never implied
            // same-file renameability and still does not.
            assert!(
                prepare_rename(source, &parsed, pos).is_none(),
                "single-file prepare_rename must refuse the {name:?} declaration \
                 name (a declaration name is not a value-member binding): {source}"
            );

            let granted =
                prepare_rename_cross_file(source, &parsed, &uri, pos, &resolver).is_some();
            assert_eq!(
                granted, *admitted,
                "cross-file rename admission for {name:?} must be \
                 {admitted} — {why}: {source}"
            );
        }

        // COMPLETENESS OF A VACUOUS SET. Purpose and Joint are admitted on the
        // grounds that the grammar gives their names no use-site syntax, so a
        // reference set holding the declaration token ALONE is COMPLETE for
        // them — not incomplete. Asserted explicitly so a future reader does not
        // read the one-element set as the very gap this task closed and "fix" it
        // by collecting something that is not a reference.
        for (source, name) in [
            NAMED_DECL_SNIPPETS
                .iter()
                .find(|(_, n)| *n == "lightweight")
                .expect("purpose row"),
            NAMED_DECL_SNIPPETS
                .iter()
                .find(|(_, n)| *n == "ball")
                .expect("joint row"),
        ] {
            let parsed = parse_one_clean(source, "guard");
            let decl = occurrences(source, name)[0];
            let refs = compute_references_cross_file(
                source,
                &parsed,
                &uri,
                offset_to_position(source, decl as u32),
                true,
                &workspace_docs(&[(uri.clone(), source)]),
                &resolver,
            )
            .unwrap_or_else(|| {
                panic!(
                    "an admitted kind must resolve to a cross-file home, so \
                     {name:?} must report a reference set: {source}"
                )
            });
            assert_eq!(
                refs,
                vec![loc_at(uri.clone(), source, decl, name)],
                "{name:?} has no use-site syntax in the grammar, so its \
                 reference set is exactly the declaration token — complete, not \
                 truncated: {source}"
            );
        }

        // COUPLING between the two halves, stated in the direction that lets the
        // gap be CLOSED: if either oracle treats the declaration as a home — a
        // granted rename, or a reported reference set — then that set must COVER
        // the use site.
        //
        // Deliberately NOT the earlier form of this assertion, which pinned the
        // reference-set INCOMPLETENESS ("the type-position use must not be
        // reported"). That is a deficiency, not a contract: it would have redded
        // the moment someone taught the collectors to walk type expressions —
        // #6972's remit — reading as a rule forbidding the improvement. Written
        // this way it survived that change intact: the `Pressure` and `Foo` rows
        // flipped from vacuously-true to genuinely-true without an edit, and it
        // is still what guards the `meter` row.
        let use_bearing: &[(&str, &str, &str)] = &[
            (
                "type Pressure = Force\nstructure S {\n    param p : Pressure = 1.0\n}",
                "Pressure",
                "the `param p : Pressure` type annotation",
            ),
            (
                "constraint def Foo {\n    param x : Length\n    x > 0mm\n}\nstructure A {\n    param w : Length = 1mm\n    constraint Foo(x: w)\n}",
                "Foo",
                "the `constraint Foo(x: w)` instantiation",
            ),
            (
                "unit meter : Length\nstructure S {\n    param x : Length = 5meter\n}",
                "meter",
                "the `5meter` literal suffix",
            ),
        ];

        for (source, name, use_desc) in use_bearing {
            let parsed = reify_syntax::parse(source, ModulePath::single("guard_use"));
            assert!(
                parsed.errors.is_empty(),
                "fixture must parse clean, got {:?} for: {source}",
                parsed.errors
            );
            let use_uri = Url::parse("file:///proj/guard_use.ri").unwrap();
            let occ = occurrences(source, name);
            assert_eq!(
                occ.len(),
                2,
                "fixture: declaration + exactly one use ({use_desc}): {source}"
            );
            let decl_pos = offset_to_position(source, occ[0] as u32);
            let (use_start, use_end) = (occ[1], occ[1] + name.len());

            let rename_granted =
                prepare_rename_cross_file(source, &parsed, &use_uri, decl_pos, &resolver).is_some();
            let refs = compute_references_cross_file(
                source,
                &parsed,
                &use_uri,
                decl_pos,
                true,
                &workspace_docs(&[(use_uri.clone(), source)]),
                &resolver,
            );
            let refs_reported = refs.is_some();
            // CONTAINS, not starts-at: a collector later taught to report unit
            // uses might span the whole `5meter` literal rather than just the
            // suffix, and that would still be a correct closing of the gap.
            let use_covered = refs.is_some_and(|locs| {
                locs.iter().any(|l| {
                    l.uri == use_uri
                        && position_to_offset(source, l.range.start) <= use_start
                        && position_to_offset(source, l.range.end) >= use_end
                })
            });

            assert!(
                !(rename_granted || refs_reported) || use_covered,
                "the {name:?} declaration is treated as a home \
                 (rename_granted={rename_granted}, refs_reported={refs_reported}) \
                 but the cross-file reference set does not cover {use_desc}, so a \
                 rename would move the declaration token and silently miss it. \
                 Either teach the declaration-name traversal to collect that use \
                 form, or keep both oracles refusing this kind: {source}"
            );
        }
    }

    /// The MULTI-DOCUMENT half of the `Unit` refusal, which the single-document
    /// rows of `cross_file_declaration_kind_admission_tracks_use_site_coverage`
    /// cannot reach.
    ///
    /// In one document a unit home is already unreachable: step 2 of
    /// `resolve_cross_file_home` consults the oracle, which refuses `Unit`, and
    /// step 3 finds no import — so the whole query is `None` and the guard's
    /// `meter` row is vacuously satisfied. Across TWO documents it is reachable,
    /// because step 3 keys only on `import_exposes_entity` and never consults
    /// the oracle: the home resolves, and the reference set used to come back as
    /// the importer's `import` token ALONE — one "reference" that is not the
    /// declaration, with the declaration missing even under
    /// `include_declaration = true`, and no rename available to act on it.
    ///
    /// The contract is now a wholesale refusal, matching what
    /// `prepare_rename_cross_file` already did for the same kind. Both are
    /// asserted, and the two halves of the non-vacuity are asserted too: the
    /// import really does resolve, and the SAME workspace shape yields a full
    /// reference set for an ADMITTED kind — otherwise this test would pass
    /// against a resolver that simply never resolves anything.
    #[test]
    fn cross_file_references_refuse_a_home_whose_kind_this_oracle_declines() {
        const DEFS_SRC: &str = "pub unit hoop : Length\npub type Pressure = Force\n";
        const USER_SRC: &str = "import defs.{hoop, Pressure}\n\
                                structure S {\n    \
                                param p : Pressure = 1.0\n\
                                }";
        let defs_uri = Url::parse("file:///proj/defs.ri").unwrap();
        let user_uri = Url::parse("file:///proj/user.ri").unwrap();
        let mut map = HashMap::new();
        map.insert("defs".to_string(), (defs_uri.clone(), DEFS_SRC.to_string()));
        let resolver = mock_resolver(map);
        let docs = workspace_docs(&[(defs_uri.clone(), DEFS_SRC), (user_uri.clone(), USER_SRC)]);

        let parsed_user = reify_syntax::parse(USER_SRC, ModulePath::single("user"));
        assert!(
            parsed_user.errors.is_empty(),
            "fixture must parse clean, got {:?}",
            parsed_user.errors
        );
        let refs_at = |name: &str, occurrence: usize| {
            compute_references_cross_file(
                USER_SRC,
                &parsed_user,
                &user_uri,
                offset_to_position(USER_SRC, occurrences(USER_SRC, name)[occurrence] as u32),
                true,
                &docs,
                &resolver,
            )
        };

        // NON-VACUITY (a): the import arm really does reach `defs.ri` for a kind
        // the oracle admits, so the refusal below is about the KIND and not
        // about a resolver that resolves nothing.
        let alias_refs = refs_at("Pressure", 0)
            .expect("an admitted kind imported from defs.ri must resolve to a home");
        assert!(
            alias_refs.iter().any(|l| l.uri == defs_uri),
            "non-vacuity: the admitted kind's set must include the home \
             declaration in defs.ri, got: {alias_refs:?}"
        );

        // NON-VACUITY (b): the unit is genuinely exposed by that same import, so
        // `resolve_cross_file_home` step 3 does reach a home for it.
        assert_eq!(
            classify_decl_name(DEFS_SRC, "hoop"),
            Some(RefSymbolKind::Unit),
            "fixture: defs.ri must declare `hoop` as a unit"
        );
        assert!(
            crate::goto_def::find_declaration_name_span(DEFS_SRC, "hoop").is_none(),
            "fixture: the declaration-token oracle must decline that unit"
        );

        // THE CONTRACT. Both gates refuse, in the same direction.
        assert!(
            refs_at("hoop", 0).is_none(),
            "a home the declaration-token oracle declines must refuse the whole \
             query, not report the importer's token with no declaration in the set"
        );
        assert!(
            prepare_rename_cross_file(
                USER_SRC,
                &parsed_user,
                &user_uri,
                offset_to_position(USER_SRC, occurrences(USER_SRC, "hoop")[0] as u32),
                &resolver,
            )
            .is_none(),
            "rename already refused a unit home; references must not disagree"
        );
    }

    /// The CROSS-FILE path must give the same answer as the same-file path for a
    /// purpose-nested structure (#6534).
    ///
    /// #6534's requirement is CONSISTENCY, not merely coverage: the two paths
    /// run different scans (`resolve_decl_name` walks `decl_name_and_span`, the
    /// cross-file side walks `decl_name_span_in`), so one learning to descend
    /// into purpose bodies while the other does not would make the SAME name
    /// navigable or renameable depending only on which file the cursor sits in —
    /// the exact asymmetry this whole sweep exists to remove.
    ///
    /// ORDERING, and why admitting this is safe rather than merely convenient:
    /// the collectors landed FIRST. Step-12 taught the declaration-name
    /// traversal every type position and step-14 taught it to descend
    /// `PurposeDef.structures`, so by the time the oracle admits this name its
    /// use sites are already collected. Admitting it before that would have
    /// handed `compute_rename_cross_file` an edit set holding the declaration
    /// token alone — which is why the reference set is asserted here to carry
    /// ALL THREE sites, and the rename grant is asserted only alongside it.
    #[test]
    fn cross_file_path_agrees_with_same_file_on_a_purpose_nested_structure() {
        const SRC: &str = "purpose Exploration() {\n    \
                           structure def InPurpose {\n        \
                           param x : Length = 5mm\n    \
                           }\n\
                           }\n\
                           structure Host {\n    \
                           param p : InPurpose\n    \
                           sub s = InPurpose()\n\
                           }";
        let parsed = reify_syntax::parse(SRC, ModulePath::single("nested"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse clean, got {:?}",
            parsed.errors
        );
        // Non-vacuity: the name must really be purpose-nested and really have
        // both use forms, or every assertion below could pass for a top-level
        // structure instead.
        assert!(
            parsed.declarations.iter().any(|d| matches!(
                d,
                Declaration::Purpose(p) if p.structures.iter().any(|s| s.name == "InPurpose")
            )),
            "fixture must nest `InPurpose` inside the purpose body"
        );
        let occ = occurrences(SRC, "InPurpose");
        assert_eq!(
            occ.len(),
            3,
            "fixture: declaration + `param p : InPurpose` + `sub s = InPurpose()`"
        );
        let (decl, type_use, sub_use) = (occ[0], occ[1], occ[2]);

        let uri = Url::parse("file:///proj/nested.ri").unwrap();
        // The fixture imports nothing; a resolver that resolves nothing keeps
        // the cross-file machinery on its single-document path.
        let resolver = |_: &str| -> Option<(Url, String)> { None };
        let decl_pos = offset_to_position(SRC, decl as u32);

        // (a) The cross-file oracle locates the name token — and locates the
        //     SAME token the same-file scan jumps to.
        assert_eq!(
            crate::goto_def::find_declaration_name_span(SRC, "InPurpose"),
            Some(span_of(decl, "InPurpose")),
            "the cross-file oracle must locate a purpose-nested structure's \
             name token"
        );
        assert_eq!(
            crate::goto_def::compute_goto_definition(SRC, &uri, decl_pos).map(|l| l.range),
            Some(span_to_range(SRC, span_of(decl, "InPurpose"))),
            "same-file goto-def and the cross-file oracle must agree on the \
             token; a divergence makes navigability depend on which file the \
             cursor is in"
        );

        // (b) The reference set carries the declaration token AND BOTH use
        //     forms. All three, because `compute_rename_cross_file` uses this
        //     set as its exact edit set.
        let refs = compute_references_cross_file(
            SRC,
            &parsed,
            &uri,
            decl_pos,
            true,
            &workspace_docs(&[(uri.clone(), SRC)]),
            &resolver,
        )
        .expect("a purpose-nested structure must resolve to a cross-file home");
        assert_eq!(
            sorted_locations(refs),
            vec![
                loc_at(uri.clone(), SRC, decl, "InPurpose"),
                loc_at(uri.clone(), SRC, type_use, "InPurpose"),
                loc_at(uri.clone(), SRC, sub_use, "InPurpose"),
            ],
            "declaration token, `param p : InPurpose` type position, and \
             `sub s = InPurpose()` construction site — a rename that moved the \
             declaration while missing either use would leave the file \
             referencing a name that no longer exists"
        );

        // (c) And it is renameable, which is sound ONLY because (b) holds.
        assert!(
            prepare_rename_cross_file(SRC, &parsed, &uri, decl_pos, &resolver).is_some(),
            "a purpose-nested structure classifies as a Structure, which \
             `is_renameable_cross_file` already admits"
        );
    }

    // --- κ step-9 (task 4210): cross-file rename WorkspaceEdit (Invariant 5) ---

    /// Apply LSP `TextEdit`s to `source`, splicing in DESCENDING start order so
    /// earlier byte offsets stay valid as later ones are replaced (mirrors
    /// `compute_rename_edit_is_valid_and_reparses_clean`).
    fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
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
        buffer
    }

    #[test]
    fn compute_rename_cross_file_two_file_workspace_edit_reparses_clean() {
        // CANONICAL SIGNAL: rename Hole→Bore from the main.ri construction site.
        // The WorkspaceEdit must carry BOTH files: parts.ri (the structure decl
        // token) and main.ri (the import entity token + the sub construction
        // site), every edit new_text="Bore". Applying each file's edits and
        // re-parsing must yield ZERO errors (Invariant 5): `structure Bore`,
        // `import parts.Bore`, and `sub hole = Bore()` are all valid.
        let (docs, resolver) = canonical_workspace();
        let parsed_main = reify_syntax::parse(MAIN_SRC, ModulePath::single("main"));
        let main_use = occurrences(MAIN_SRC, "Hole")[1]; // `sub hole = Hole()`

        let edit = compute_rename_cross_file(
            MAIN_SRC,
            &parsed_main,
            &main_uri(),
            offset_to_position(MAIN_SRC, main_use as u32),
            "Bore",
            &docs,
            &resolver,
        )
        .expect("cross-file rename yields a WorkspaceEdit");

        let changes = edit.changes.expect("changes present");
        assert_eq!(changes.len(), 2, "edit spans both parts.ri and main.ri");

        let parts_edits = changes.get(&parts_uri()).expect("parts.ri edits present");
        assert_eq!(parts_edits.len(), 1, "parts.ri: 1 edit (structure decl token)");
        assert!(
            parts_edits.iter().all(|e| e.new_text == "Bore"),
            "every parts.ri edit writes Bore"
        );
        let main_edits = changes.get(&main_uri()).expect("main.ri edits present");
        assert_eq!(main_edits.len(), 2, "main.ri: 2 edits (import token + sub use)");
        assert!(
            main_edits.iter().all(|e| e.new_text == "Bore"),
            "every main.ri edit writes Bore"
        );

        // Invariant 5: apply per-file edits, re-parse, assert ZERO errors in BOTH.
        let parts_after = apply_edits(PARTS_SRC, parts_edits);
        let main_after = apply_edits(MAIN_SRC, main_edits);
        let parts_reparsed = reify_syntax::parse(&parts_after, ModulePath::single("parts"));
        assert!(
            parts_reparsed.errors.is_empty(),
            "parts.ri re-parses clean after rename: {:?}\n{parts_after}",
            parts_reparsed.errors
        );
        let main_reparsed = reify_syntax::parse(&main_after, ModulePath::single("main"));
        assert!(
            main_reparsed.errors.is_empty(),
            "main.ri re-parses clean after rename: {:?}\n{main_after}",
            main_reparsed.errors
        );
        // The renamed tokens are concretely present in both buffers.
        assert!(
            parts_after.contains("structure Bore"),
            "parts.ri now declares `structure Bore`: {parts_after}"
        );
        assert!(
            main_after.contains("import parts.Bore"),
            "main.ri import renamed to `parts.Bore`: {main_after}"
        );
        assert!(
            main_after.contains("= Bore()"),
            "main.ri construction site renamed to `Bore()`: {main_after}"
        );
    }

    #[test]
    fn compute_rename_cross_file_one_char_structure_name_does_not_corrupt_keyword() {
        // BLAST-RADIUS GUARD (task 7529). A declaration span starts at its
        // keyword, so a name-token locator that is not whole-word matches the
        // `s` of `structure` before the `s` of `structure s`. That span reaches
        // this write path: the parts.ri edit rewrites byte 0, yielding
        // `Boretructure s { … }`, which does not parse.
        //
        // The DESTRUCTURED import form is load-bearing and must not be
        // "simplified" to `import parts.s`: ts_parser classifies a trailing
        // import-path segment as an entity only when it starts with an
        // uppercase character, so `import parts.s` lowers to
        // `ImportKind::Module`, the cross-file home never resolves, and this
        // test would silently assert nothing. `import parts.{s}` lowers to
        // `ImportKind::Destructured(["s"])`, which `import_exposes_entity`
        // admits.
        const SHORT_PARTS_SRC: &str = "structure s {\n    param diameter: Length = 10mm\n}";
        const SHORT_MAIN_SRC: &str =
            "import parts.{s}\nstructure Assembly {\n    sub hole = s()\n}";

        let docs = workspace_docs(&[(parts_uri(), SHORT_PARTS_SRC), (main_uri(), SHORT_MAIN_SRC)]);
        let mut map = HashMap::new();
        map.insert(
            "parts".to_string(),
            (parts_uri(), SHORT_PARTS_SRC.to_string()),
        );
        let resolver = mock_resolver(map);

        let parsed_main = reify_syntax::parse(SHORT_MAIN_SRC, ModulePath::single("main"));
        // `occurrences` is unusable for a one-character name (every `s` in
        // `structure`/`parts`/`sub` matches), so anchor on the construction site.
        let assign = SHORT_MAIN_SRC.find("= s()").expect("fixture: construction site");
        let main_use = assign + "= ".len(); // the `s` of `= s()`
        assert_eq!(&SHORT_MAIN_SRC[main_use..main_use + 1], "s");

        let edit = compute_rename_cross_file(
            SHORT_MAIN_SRC,
            &parsed_main,
            &main_uri(),
            offset_to_position(SHORT_MAIN_SRC, main_use as u32),
            "Bore",
            &docs,
            &resolver,
        )
        .expect("cross-file rename of a one-character name yields a WorkspaceEdit");

        let changes = edit.changes.expect("changes present");
        assert_eq!(changes.len(), 2, "edit spans both parts.ri and main.ri");

        let parts_edits = changes.get(&parts_uri()).expect("parts.ri edits present");
        assert_eq!(
            parts_edits.len(),
            1,
            "parts.ri: 1 edit (structure decl token)"
        );
        let main_edits = changes.get(&main_uri()).expect("main.ri edits present");
        assert_eq!(
            main_edits.len(),
            2,
            "main.ri: 2 edits (import token + sub use)"
        );
        assert!(
            parts_edits
                .iter()
                .chain(main_edits)
                .all(|e| e.new_text == "Bore"),
            "every edit writes Bore"
        );

        // Invariant 5: apply per-file edits, re-parse, assert ZERO errors in BOTH.
        let parts_after = apply_edits(SHORT_PARTS_SRC, parts_edits);
        let main_after = apply_edits(SHORT_MAIN_SRC, main_edits);
        let parts_reparsed = reify_syntax::parse(&parts_after, ModulePath::single("parts"));
        assert!(
            parts_reparsed.errors.is_empty(),
            "parts.ri re-parses clean after renaming a one-character name: {:?}\n{parts_after}",
            parts_reparsed.errors
        );
        let main_reparsed = reify_syntax::parse(&main_after, ModulePath::single("main"));
        assert!(
            main_reparsed.errors.is_empty(),
            "main.ri re-parses clean after renaming a one-character name: {:?}\n{main_after}",
            main_reparsed.errors
        );
        assert!(
            parts_after.contains("structure Bore"),
            "parts.ri now declares `structure Bore` — the keyword is intact: {parts_after}"
        );
        assert!(
            main_after.contains("import parts.{Bore}"),
            "main.ri import renamed to `parts.{{Bore}}`: {main_after}"
        );
        assert!(
            main_after.contains("= Bore()"),
            "main.ri construction site renamed to `Bore()`: {main_after}"
        );
    }

    #[test]
    fn compute_rename_cross_file_rejects_invalid_new_names() {
        // An illegal new_name must be refused (None) BEFORE any edit — else the
        // multi-file buffer no longer re-parses cleanly (the re-parse-clean half
        // of Invariant 5).
        let (docs, resolver) = canonical_workspace();
        let parsed_main = reify_syntax::parse(MAIN_SRC, ModulePath::single("main"));
        let pos = offset_to_position(MAIN_SRC, occurrences(MAIN_SRC, "Hole")[1] as u32);

        // Sanity: a legal identifier is accepted (so the refusals attribute to
        // the new_name, not the cursor).
        assert!(
            compute_rename_cross_file(
                MAIN_SRC,
                &parsed_main,
                &main_uri(),
                pos,
                "Bore",
                &docs,
                &resolver
            )
            .is_some(),
            "a legal identifier new_name is accepted"
        );
        for bad in ["2x", "let", "foo bar", ""] {
            assert!(
                compute_rename_cross_file(
                    MAIN_SRC,
                    &parsed_main,
                    &main_uri(),
                    pos,
                    bad,
                    &docs,
                    &resolver
                )
                .is_none(),
                "illegal new_name {bad:?} must be refused"
            );
        }
    }
}

