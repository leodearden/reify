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

use reify_ast::{Declaration, ImportKind, ParsedModule};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{Location, Position, TextEdit, Url, WorkspaceEdit};

use crate::analysis::name_token_span;
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

mod decl_uses;
mod local;
mod rename;
mod walk;

use decl_uses::{collect_decl_name_spans, collect_decl_name_uses_in_decl, import_exposes_entity};
pub use local::{RefSymbolKind, ReferenceSet, RenameTarget, collect_references, compute_references};
use local::collect_references_at;
pub use rename::{compute_document_highlights, compute_rename, prepare_rename};
use rename::{is_renameable, is_valid_rename_identifier};

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
    use crate::analysis::test_fixtures::{NAMED_DECL_SNIPPETS, occurrences, parse_one_clean, span_of};
    use crate::convert::{offset_to_position, span_to_range};
    use reify_ast::MemberDecl;
    use reify_core::ModulePath;

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

