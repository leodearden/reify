use super::*;
use crate::analysis::test_fixtures::{find_sub, occurrences, span_of};
use crate::convert::offset_to_position;
use reify_core::ModulePath;

/// Whether `span` lies fully within the byte range `[lo, hi)`.
fn within(span: SourceSpan, lo: usize, hi: usize) -> bool {
    (span.start as usize) >= lo && (span.end as usize) <= hi
}

/// Assert that `collect_references` rooted at the declaration token
/// `[decl_off, decl_off + name.len())` reaches exactly one use, at
/// `[use_off, use_off + name.len())`, resolving as `kind` — the shared
/// positive-reachability tail every `sub_direct_exprs`/keyed-entry
/// chain-link test in this file's task #5579 suite pins.
///
/// Positions are caller-supplied byte offsets rather than derived here via
/// `occurrences(source, name)`, so a fixture whose identifier is a
/// substring of other tokens in the source (e.g. the 1-char `n` in `Int`/
/// `in`) can still use this shared tail: the caller locates its own two
/// offsets however is unambiguous for its fixture, then hands them off.
///
/// Centralizes what were five independent ~15-line copies of this same
/// parse→query→assert tail (one per chain-link fixture), leaving each
/// test's unique fixture string and AST-field precondition assertion as
/// the only per-case code.
fn assert_sole_use_reachable(
    source: &str,
    parsed: &ParsedModule,
    name: &str,
    kind: RefSymbolKind,
    decl_off: usize,
    use_off: usize,
) {
    let pos = offset_to_position(source, decl_off as u32);
    let refset = collect_references(source, parsed, pos, false).unwrap_or_else(|| {
        panic!("`{name}` declaration at {decl_off} should resolve to a ReferenceSet")
    });
    assert_eq!(refset.name, name);
    assert_eq!(refset.kind, kind);
    assert_eq!(refset.declaration, span_of(decl_off, name));
    assert_eq!(
        refset.references,
        vec![span_of(use_off, name)],
        "the sole use of `{name}` must be reachable from a find-references \
         query on its declaration"
    );
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

// --- task #5579: indexed-sub domain expression uses (#5481 α consumer gap) ---

#[test]
fn collect_references_reaches_indexed_sub_domain_use() {
    // `n` is used only inside the indexer clause's domain expression
    // (`sub xs[i in 0..n] = …`), never in the sub's args/pose/where. Before
    // this fix, the sub use-walk never walked `SubDecl::index_domain`,
    // so a find-references/rename query rooted at the `param n` decl missed
    // this use entirely (silent miss — no compile error, since #5481 added
    // the AST fields with zero struct-pattern destructures elsewhere).
    let source = "\
structure S {
    param n: Int = 4
    sub xs[i in 0..n] = Hole(bore: 3mm)
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("indexed_sub"));
    // The indexer clause lowers to a fully-populated `index_binder`/
    // `index_domain` pair (both are the AST contract β builds on), though
    // it also travels with an interim #5482 ERROR-severity diagnostic
    // until compiler elaboration lands — see the doc comment on
    // `SubDecl::index_binder` (crates/reify-ast/src/decl.rs). That
    // diagnostic is not the behavior under test, so assert directly on the
    // AST field the walker must reach instead of on the diagnostic's
    // wording (which would break for an unrelated reason once #5482
    // elaboration lands and the diagnostic is retired).
    assert!(
        find_sub(&parsed, "xs").index_domain.is_some(),
        "fixture must actually lower an index_domain for the walker to reach"
    );

    // The 1-char identifier `n` is not safe to locate via `occurrences`
    // (it also matches inside `Int`, `in`, etc.), so find the two sites we
    // care about directly instead.
    let decl_off = source.find("param n:").expect("param n decl") + "param ".len();
    let domain_off = source.find("0..n").expect("0..n domain use") + "0..".len();
    assert_eq!(&source[decl_off..decl_off + 1], "n");
    assert_eq!(&source[domain_off..domain_off + 1], "n");

    assert_sole_use_reachable(
        source,
        &parsed,
        "n",
        RefSymbolKind::Param,
        decl_off,
        domain_off,
    );
}

// --- task #5579 (amendment): index_binder shadow — pin the deliberate
// current behavior (reviewer-surfaced gap: neither `sub_direct_exprs` nor
// its consumers' doc comments previously had a test observing the
// consequence of NOT registering `index_binder` as a binding site) ---

#[test]
fn collect_references_from_outer_param_reaches_shadowed_index_binder_use() {
    // `i` is declared twice here: once as an outer `param i`, and again as
    // the indexed-sub's own `index_binder` (`sub xs[i in 0..4] = …`). Per
    // the deliberate choice documented on `sub_direct_exprs` (mirroring
    // the "Known limitation" on `collect_idents_in_expr`: binder-
    // introducing expressions are not modeled as their own scope),
    // `index_binder` is NOT registered as a local binding, so
    // `Hole(bore: i)` — logically a use of the BINDER's `i` under
    // indexed-sub semantics — resolves TODAY as a use of the unrelated
    // OUTER `param i` instead.
    //
    // This pins that current, deliberately-deferred answer so a future
    // change to binder scope-modeling must update this test rather than
    // silently flip which `i` a rename touches.
    let source = "\
structure S {
    param i: Int = 0
    sub xs[i in 0..4] = Hole(bore: i)
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("index_binder_shadow"));
    // See `collect_references_reaches_indexed_sub_domain_use` above for
    // why this fixture is not asserted parse-clean: the indexed form
    // always travels with an interim #5482 diagnostic, unrelated to the
    // shadow under test here.
    let sub = find_sub(&parsed, "xs");
    assert!(
        sub.index_binder.is_some(),
        "fixture must actually lower an index_binder for the shadow to exist"
    );

    // The 1-char identifier `i` is not safe to locate via `occurrences`
    // (it also matches inside the `in` keyword), so find the two sites we
    // care about directly instead, mirroring
    // `collect_references_reaches_indexed_sub_domain_use`'s approach for
    // the equally-short `n`.
    let decl_off = source.find("param i:").expect("param i decl") + "param ".len();
    let use_off = source.find("bore: i)").expect("bore: i) use") + "bore: ".len();
    assert_eq!(&source[decl_off..decl_off + 1], "i");
    assert_eq!(&source[use_off..use_off + 1], "i");

    assert_sole_use_reachable(
        source,
        &parsed,
        "i",
        RefSymbolKind::Param,
        decl_off,
        use_off,
    );
}

// --- task #5579 (amendment): inline relate-block relation use reachable
// from find-references (task δ 4384 `relate_relations` symmetry) ---

#[test]
fn collect_references_reaches_inline_relate_block_use() {
    // `gap` is used only inside the inline `at … where { }` relate-block's
    // relation expression (task δ 4384), never in the sub's args/pose/
    // where. Mirrors `collect_references_reaches_indexed_sub_domain_use`
    // above, but for `SubDecl::relate_relations` instead of
    // `index_domain` — this is the positive reachability counterpart to
    // `member_access_field_segment_refuses_in_inline_relate_block`, which
    // only exercises the separate `cursor_on_member_segment` refusal
    // guard and would stay green even if the
    // `for rel in &s.relate_relations` walk in the sub use-walk
    // (now folded into `sub_direct_exprs`) were deleted.
    let source = "\
structure S {
    param gap: Length = 1mm
    sub bolt : Bolt at auto where {
        concentric(bolt.face, gap)
    }
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("relateuse"));
    assert!(
        parsed.errors.is_empty(),
        "inline relate-block fixture must parse clean: {:?}",
        parsed.errors
    );
    assert!(
        !find_sub(&parsed, "bolt").relate_relations.is_empty(),
        "fixture must actually lower a relate_relations entry for the walker to reach"
    );

    // gap[0] = `param gap` decl, gap[1] = the sole use inside the relate block.
    let gap = occurrences(source, "gap");
    assert_eq!(gap.len(), 2, "1 param decl + 1 use inside the relate block");

    assert_sole_use_reachable(source, &parsed, "gap", RefSymbolKind::Param, gap[0], gap[1]);
}

// --- task #5579 (amendment): mutation-coverage gap closure for the three
// `sub_direct_exprs` chain links no test previously exercised — deleting
// `.chain(pose_expr.iter())`, `.chain(where_clause.iter().map(|w|
// &w.condition))`, or `.chain(spec_param_overrides.iter().map(|(_, e)|
// e))` each independently left the whole reify-lsp suite green. These
// three tests mirror `collect_references_reaches_indexed_sub_domain_use`
// / `_inline_relate_block_use` above, one per previously-uncovered link. ---

#[test]
fn collect_references_reaches_sub_pose_expr_use() {
    // `dx` is used only inside the sub's `at <expr>` placement pose, never
    // in its args/where/index-domain/relate-block.
    let source = "\
structure S {
    param dx: Length = 1mm
    sub b : Bolt at translate(dx)
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("pose"));
    assert!(
        parsed.errors.is_empty(),
        "pose fixture must parse clean: {:?}",
        parsed.errors
    );
    assert!(
        find_sub(&parsed, "b").pose_expr.is_some(),
        "fixture must actually lower a pose_expr for the walker to reach"
    );

    // dx[0] = `param dx` decl, dx[1] = the sole use inside the pose expr.
    let dx = occurrences(source, "dx");
    assert_eq!(dx.len(), 2, "1 param decl + 1 use inside the pose expression");

    assert_sole_use_reachable(source, &parsed, "dx", RefSymbolKind::Param, dx[0], dx[1]);
}

#[test]
fn collect_references_reaches_sub_where_clause_use() {
    // `ready` is used only inside the sub's own trailing `where <cond>`
    // guard (`SubDecl::where_clause` — distinct from both a member-level
    // `where { }` GuardedGroup and the inline `at … where { }`
    // relate-block), never in its args/pose/index-domain.
    let source = "\
structure S {
    param ready: Bool = true
    sub b = Widget() where ready
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("subwhere"));
    assert!(
        parsed.errors.is_empty(),
        "sub-where fixture must parse clean: {:?}",
        parsed.errors
    );
    assert!(
        find_sub(&parsed, "b").where_clause.is_some(),
        "fixture must actually lower a where_clause for the walker to reach"
    );

    // ready[0] = `param ready` decl, ready[1] = the sole use inside the
    // sub's where guard.
    let ready = occurrences(source, "ready");
    assert_eq!(
        ready.len(),
        2,
        "1 param decl + 1 use inside the sub's where guard"
    );

    assert_sole_use_reachable(
        source,
        &parsed,
        "ready",
        RefSymbolKind::Param,
        ready[0],
        ready[1],
    );
}

#[test]
fn collect_references_reaches_sub_spec_param_override_use() {
    // `margin` is used only inside the sub's specialization-body param
    // override value (`SubDecl::spec_param_overrides`), never in its
    // args/pose/where/index-domain.
    let source = "\
structure S {
    param margin: Length = 1mm
    sub b : Bearing { bore = margin }
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("specoverride"));
    assert!(
        parsed.errors.is_empty(),
        "spec-override fixture must parse clean: {:?}",
        parsed.errors
    );
    assert!(
        !find_sub(&parsed, "b").spec_param_overrides.is_empty(),
        "fixture must actually lower a spec_param_overrides entry for the \
         walker to reach"
    );

    // margin[0] = `param margin` decl, margin[1] = the sole use inside the
    // override value.
    let margin = occurrences(source, "margin");
    assert_eq!(
        margin.len(),
        2,
        "1 param decl + 1 use inside the specialization-body override"
    );

    assert_sole_use_reachable(
        source,
        &parsed,
        "margin",
        RefSymbolKind::Param,
        margin[0],
        margin[1],
    );
}

// --- task #5579 (amendment): keyed-entry `param_overrides` use reachable
// from find-references (reviewer-surfaced sibling gap to the five chain
// links above) ---

#[test]
fn collect_references_reaches_sub_keyed_param_override_use() {
    // `margin` is used only inside a KEYED block's `param_overrides`
    // value (`KeyedSubMemberEntry.param_overrides`, task 3931 γ) — distinct
    // from `collect_references_reaches_sub_spec_param_override_use` above,
    // which covers the sub-level `spec_param_overrides` of a plain
    // (non-keyed) specialization body. Before this fix, neither
    // the sub use-walk nor `cursor_on_member_segment`'s
    // `MemberDecl::Sub` arm walked any keyed entry's `param_overrides` —
    // the identical silent find-references/rename miss this task closes
    // for `index_domain`, just one level deeper (inside `keyed_members`).
    let source = "\
structure S {
    param margin: Length = 1mm
    sub b : Bearing {
        \"k\" => { bore = margin }
    }
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("keyedoverride"));
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
        "fixture must actually lower a param_overrides entry for the walker to reach"
    );

    // margin[0] = `param margin` decl, margin[1] = the sole use inside the
    // keyed entry's override value.
    let margin = occurrences(source, "margin");
    assert_eq!(
        margin.len(),
        2,
        "1 param decl + 1 use inside the keyed entry's param override"
    );

    assert_sole_use_reachable(
        source,
        &parsed,
        "margin",
        RefSymbolKind::Param,
        margin[0],
        margin[1],
    );
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

#[test]
fn collect_references_reach_into_a_sketch_body_which_owns_its_scope() {
    // A `sketch { … }` body is a nested member-list scope
    // (constrained-2d-sketch α, task 5506), reached by both `collect_uses`
    // and `collect_bindings_in_scope` only through `for_each_child_scope`.
    // (a) an outer param used inside the body is a reference of that param;
    // (b) a sketch-local `let` owns the uses inside the block and the outer
    // same-named `let` owns the uses outside it.
    let source = "\
structure S {
    param slot_w: Length = 6mm
    let anchor = point(0mm, 0mm)
    let before = anchor
    sketch profile {
        let anchor = point(1mm, 0mm)
        let tip = point(10mm, 0mm)
        fix(anchor)
        distance(anchor, tip, slot_w)
    }
    let after = anchor
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("sketchscope"));
    assert!(
        parsed.errors.is_empty(),
        "sketch-scope fixture must parse clean: {:?}",
        parsed.errors
    );

    // --- (a) outer param `slot_w`: its decl and its sketch-body use. ---
    let slot = occurrences(source, "slot_w");
    assert_eq!(slot.len(), 2, "slot_w: decl + the sketch-body use");
    let slot_refs = collect_references(
        source,
        &parsed,
        offset_to_position(source, slot[0] as u32),
        true,
    )
    .expect("slot_w declaration resolves");
    assert_eq!(slot_refs.kind, RefSymbolKind::Param);
    assert_eq!(
        slot_refs.references,
        vec![span_of(slot[0], "slot_w"), span_of(slot[1], "slot_w")],
        "the use inside the sketch body must be a reference of the outer param"
    );

    // --- (b) `anchor`: a[0]=outer decl, a[1]=outer use (before),
    // a[2]=sketch-local decl, a[3]/a[4]=sketch-body uses, a[5]=outer use (after).
    let a = occurrences(source, "anchor");
    assert_eq!(
        a.len(),
        6,
        "outer decl, 2 outer uses, sketch decl, 2 sketch uses"
    );

    let outer = collect_references(
        source,
        &parsed,
        offset_to_position(source, a[0] as u32),
        true,
    )
    .expect("outer anchor declaration resolves");
    assert_eq!(outer.kind, RefSymbolKind::Let);
    assert_eq!(
        outer.references,
        vec![
            span_of(a[0], "anchor"),
            span_of(a[1], "anchor"),
            span_of(a[5], "anchor"),
        ],
        "the outer anchor owns its decl and the uses outside the sketch only"
    );

    let local = collect_references(
        source,
        &parsed,
        offset_to_position(source, a[2] as u32),
        true,
    )
    .expect("sketch-local anchor declaration resolves");
    assert_eq!(local.kind, RefSymbolKind::Let);
    assert_eq!(
        local.references,
        vec![
            span_of(a[2], "anchor"),
            span_of(a[3], "anchor"),
            span_of(a[4], "anchor"),
        ],
        "the sketch-local anchor owns its decl and the uses inside the sketch only"
    );

    let from_body_use = collect_references(
        source,
        &parsed,
        offset_to_position(source, a[4] as u32),
        false,
    )
    .expect("sketch-body use resolves");
    assert_eq!(
        from_body_use.declaration,
        span_of(a[2], "anchor"),
        "a sketch-body use must bind to the sketch-local declaration"
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
