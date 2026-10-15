//! Cross-file declaration-name references and rename over the import graph:
//! resolve the cursor to a declaration home, then union the home document's
//! use sites with each importing open document's. Where `local` and `rename`
//! resolve VALUE-member bindings (param/let/auto/sub/port) within one entity
//! body, this layer resolves a DECLARATION name across files.
//!
//! Pure — the open-document set arrives as `workspace_docs` and import
//! resolution as an injected `resolve_import` closure (mirroring goto_def's
//! pattern), so the scope logic is unit-testable with an in-memory workspace
//! and a mock resolver. The server handlers assemble both from the live
//! DocumentStore.

use std::collections::HashMap;

use reify_ast::{Declaration, ImportKind, ParsedModule};
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{Location, Position, TextEdit, Url, WorkspaceEdit};

use crate::decl_oracle::name_token_span;
use crate::convert::{find_word_at_offset, position_to_offset, span_to_range};

use super::decl_uses::{
    collect_decl_name_spans, collect_decl_name_uses_in_decl, import_exposes_entity,
};
use super::local::{RefSymbolKind, RenameTarget, collect_references_at, compute_references};
use super::rename::{is_renameable, is_valid_rename_identifier, prepare_rename};

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
        crate::decl_oracle::purpose_nested_decl_names(parsed)
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
mod tests;
