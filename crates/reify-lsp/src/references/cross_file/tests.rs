use super::*;
use crate::decl_oracle::test_fixtures::{NAMED_DECL_SNIPPETS, occurrences, parse_one_clean, span_of};
use crate::convert::offset_to_position;
use crate::references::test_support::apply_edits;
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
/// all kinds, via its own scanner (`decl_oracle::decl_name_and_span` +
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
