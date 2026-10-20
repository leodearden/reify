use super::*;
use crate::decl_oracle::test_fixtures::{find_sub, occurrences, span_of};
use crate::convert::offset_to_position;
use reify_core::ModulePath;
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
