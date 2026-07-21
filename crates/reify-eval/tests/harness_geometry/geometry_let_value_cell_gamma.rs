//! γ (task #4954) eval edit-path boundary test: PRD
//! `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.2 clause 5 (the
//! cell/realization coherence contract) — "a geometry cell whose backing
//! realization vanished must not survive an edit (existing mechanism
//! `engine_edit.rs:2715-2739`, extended to let-backed cells in γ)".
//!
//! Both tests below use `Engine::eval` / `Engine::edit_source` (kernel-free —
//! no `build()`/kernel needed: the R3d in-walk mint stamps a *symbolic*
//! `GeometryHandle`, `kernel_handle: None`, for any `Type::Geometry` cell
//! with a name-matching realization, `engine_build.rs:8436`). Unlike the
//! `build()`-path GHR-δ S12 precedent
//! (`crates/reify-eval/tests/geometry_handle_freshness.rs`
//! `removed_realization_donates_invalidation_to_surviving_geometry_cell`,
//! param-backed) — where an invalidated cache entry stays ABSENT until a
//! later on-demand rebuild — the kernel-free eval path eagerly re-derives
//! every live cell on each `edit_source` call, so a correctly-invalidated
//! cell shows up as a *freshly recomputed* cache entry, not a missing one.
//! The assertions below therefore check the recomputed CONTENT is correct
//! (no residual reference to a vanished realization), not cache-entry
//! absence — empirically confirmed while drafting this test: asserting
//! `cache_store().get(..).is_none()` after `edit_source` fails even for a
//! correctly-recomputed cell, because `edit_source` immediately repopulates
//! the cache with the fresh (non-stale) result.
use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::{RealizationNodeId, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_ir::Value;
use reify_test_support::compile_source_with_stdlib;

fn assert_no_compile_errors(compiled: &reify_compiler::CompiledModule) {
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no compile-time errors; got: {:#?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// OLD: `loc_box` is a top-level geometry let (`box(...)`) — backed by a
/// `RealizationDecl` named `loc_box` and a `Type::Geometry` Let value cell.
const SRC_GEOMETRY_LET: &str = r#"structure def Widget {
    param width : Length = 10mm
    let loc_box = box(width, width, width)
}"#;

/// NEW: `loc_box` is a plain scalar let — no longer a geometry let, so no
/// backing `RealizationDecl` is emitted for it. The cell SURVIVES (same
/// member name, now `Type::Real`).
const SRC_SCALAR_LET: &str = r#"structure def Widget {
    param width : Length = 10mm
    let loc_box = 1.0
}"#;

/// FIRST: the simple clause-5 fixture the plan calls out directly — a
/// geometry let edited into a plain scalar let. `loc_box`'s OWN
/// `default_expr` changes (`box(...)` -> `1.0`), so this scenario is
/// dominated by the pre-existing changed-value-cell recompute path (it does
/// not, by itself, isolate the removed-realization DONATION leg — see the
/// SECOND test below for that), but it directly pins clause 5's literal
/// wording: the surviving cell must never read back as a stale
/// GeometryHandle once its backing realization is gone.
#[test]
fn scalar_conversion_does_not_leave_a_stale_geometry_handle() {
    let compiled_geo = compile_source_with_stdlib(SRC_GEOMETRY_LET);
    assert_no_compile_errors(&compiled_geo);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let baseline = engine.eval(&compiled_geo);

    let loc_box = ValueCellId::new("Widget", "loc_box");
    let loc_box_node = NodeId::Value(loc_box.clone());

    // Precondition: the let-backed geometry cell hydrates to a (symbolic,
    // kernel-free) GeometryHandle and has a live cache entry — the R3d
    // in-walk mint fires for a let-backed cell exactly like a solid param.
    assert!(
        matches!(
            baseline.values.get_or_undef(&loc_box),
            Value::GeometryHandle { .. }
        ),
        "Widget.loc_box must hydrate to a Value::GeometryHandle via the R3d \
         in-walk mint before the edit; got: {:?}",
        baseline.values.get_or_undef(&loc_box)
    );
    assert!(
        engine.cache_store().get(&loc_box_node).is_some(),
        "Widget.loc_box must have a live cache entry after eval()"
    );

    // Edit: `loc_box` becomes a plain scalar let. Its backing RealizationDecl
    // disappears; the value cell survives under the same name (now
    // Type::Real, default_expr = 1.0).
    let compiled_scalar = compile_source_with_stdlib(SRC_SCALAR_LET);
    assert_no_compile_errors(&compiled_scalar);
    let edited = engine
        .edit_source(&compiled_scalar)
        .expect("scalar-let edit_source must not error");

    // Main assertion (PRD §6.2 clause 5): the surviving cell must NOT read
    // back as a stale GeometryHandle.
    let post_edit_value = edited.values.get_or_undef(&loc_box);
    assert!(
        !matches!(post_edit_value, Value::GeometryHandle { .. }),
        "Widget.loc_box (Type::Geometry cell whose backing RealizationDecl \
         vanished under the edit) must NOT survive as a stale GeometryHandle; \
         got: {:?}",
        post_edit_value
    );
    assert!(
        matches!(post_edit_value, Value::Real(r) if (r - 1.0).abs() < 1e-9),
        "Widget.loc_box must re-evaluate to its new scalar default 1.0; got: {:?}",
        post_edit_value
    );

    // The cache entry must track the same non-stale content (the eval path
    // eagerly recomputes changed cells rather than leaving them absent).
    let cache_entry = engine
        .cache_store()
        .get(&loc_box_node)
        .expect("a recomputed cell still gets a fresh cache entry");
    assert!(
        !matches!(
            cache_entry.result,
            CachedResult::Value(Value::GeometryHandle { .. }, _)
        ),
        "Widget.loc_box's cache entry must not retain a stale GeometryHandle \
         result after its backing Realization vanished; got: {:?}",
        cache_entry.result
    );
}

/// SECOND: an isolating mirror of the GHR-δ S12 removed-realization
/// precedent, replayed with two geometry LETS instead of two geometry
/// PARAMS. `b`'s own declaration text is BYTE IDENTICAL across the edit
/// (same member name, same `default_expr`), so `Widget.b`'s value cell does
/// NOT land in the ordinary changed-cell set — the only way its
/// `realization_ref` can be corrected is the removed-realization donation
/// leg (`engine_edit.rs` `for rid in &removed_realizations`) invalidating it
/// so the next read re-derives it against the CURRENT graph (the R3d
/// in-walk mint name-matches the realization fresh rather than trusting a
/// cached index).
///
/// Fixture: dropping the EARLIER geometry let `a` reindexes the LATER
/// geometry let `b` from `Widget#1` down to `Widget#0` — `Widget#1` lands in
/// `removed_realizations` while `Widget.b` (the value cell) survives. `b`
/// does not reference `a`, so dropping `a` still compiles.
#[test]
fn removed_realization_donates_invalidation_to_surviving_let_backed_geometry_cell() {
    const SRC_A: &str = r#"structure def Widget {
    param width : Length = 10mm
    let a = box(width, width, width)
    let b = box(width, width, width)
}"#;
    // Drop the EARLIER geometry let `a`. `b` reindexes Widget#1 -> Widget#0;
    // `b`'s own declaration is untouched (byte-identical default_expr).
    const SRC_B: &str = r#"structure def Widget {
    param width : Length = 10mm
    let b = box(width, width, width)
}"#;

    let compiled_a = compile_source_with_stdlib(SRC_A);
    assert_no_compile_errors(&compiled_a);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let baseline = engine.eval(&compiled_a);

    let b = ValueCellId::new("Widget", "b");
    let b_node = NodeId::Value(b.clone());
    // In SRC_A, `b` is the SECOND geometry-bearing member (after `a`) -> Widget#1.
    let r_b_old = RealizationNodeId::new("Widget", 1);
    let r_b_new = RealizationNodeId::new("Widget", 0);

    // Preconditions: `b` hydrates to a symbolic GeometryHandle backed by
    // Widget#1, and has a live cache entry.
    match baseline.values.get_or_undef(&b) {
        Value::GeometryHandle {
            realization_ref, ..
        } => assert_eq!(
            realization_ref, r_b_old,
            "Widget.b must be backed by Widget#1 before the edit"
        ),
        other => panic!("Widget.b must hydrate to a Value::GeometryHandle; got {other:?}"),
    }
    assert!(
        engine.cache_store().get(&b_node).is_some(),
        "Widget.b must have a live cache entry after eval()"
    );

    // Edit: drop `a`. Widget#1 disappears (b reindexes to Widget#0);
    // Widget.b survives byte-identical.
    let compiled_b = compile_source_with_stdlib(SRC_B);
    assert_no_compile_errors(&compiled_b);
    let edited = engine
        .edit_source(&compiled_b)
        .expect("drop-let edit_source must not error");

    // Precondition: the OLD backing realization Widget#1 was genuinely
    // removed, and Widget.b survived (reindexed to Widget#0) in the new
    // graph — the removed-realization leg's membership-gate-TRUE branch.
    let snap = engine.snapshot().expect("snapshot after edit");
    assert!(
        !snap.graph.realizations.contains_key(&r_b_old),
        "Widget#1 must be removed after dropping the earlier geometry let `a`"
    );
    assert_eq!(
        snap.graph
            .realizations
            .get(&r_b_new)
            .and_then(|rnode| rnode.geometry_cell.clone()),
        Some(b.clone()),
        "Widget#0 must now back Widget.b after the reindex"
    );

    // Main assertion: `Widget.b` must NOT read back with a stale
    // `realization_ref` pointing at the now-vanished Widget#1 — it must be
    // freshly re-minted against Widget#0.
    match edited.values.get_or_undef(&b) {
        Value::GeometryHandle {
            realization_ref, ..
        } => assert_eq!(
            realization_ref, r_b_new,
            "Widget.b's GeometryHandle must reference its NEW backing \
             realization Widget#0, not the removed Widget#1, after the edit"
        ),
        other => panic!(
            "Widget.b must still hydrate to a Value::GeometryHandle after \
             surviving the edit; got {other:?}"
        ),
    }

    // Cache-level guard (mirrors the GHR-δ S12 precedent): the cache entry,
    // if present, must not retain the OLD realization_ref either.
    if let Some(entry) = engine.cache_store().get(&b_node)
        && let CachedResult::Value(
            Value::GeometryHandle {
                realization_ref, ..
            },
            _,
        ) = &entry.result
    {
        assert_eq!(
            *realization_ref, r_b_new,
            "Widget.b's cache entry must not retain a stale reference to the \
             removed Widget#1 realization; got: {:?}",
            entry.result
        );
    }
}
