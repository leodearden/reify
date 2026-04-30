//! Worked-example smoke tests for the topology-selector function family
//! (PRD `docs/prds/topology-selectors.md` task 7).
//!
//! Two `.ri` fixtures live under `examples/topology_selectors/`:
//!
//! * `block_inertia.ri` — `moment_of_inertia` on a steel box
//! * `fillet_top_edges.ri` — `single`/`flat_map`/`adjacent_faces`/`shared_edges`/`faces_by_normal`/`fillet`
//!
//! ## Scope: parse-only
//!
//! The natural maximum coverage (parse → compile_with_stdlib → eval → constraints,
//! per `field_source_kinds_smoke.rs`) is not achievable today: the .ri-language
//! stdlib bindings for `moment_of_inertia`, `faces_by_normal`, `adjacent_faces`,
//! `shared_edges`, `single`, and `flat_map` are not yet wired. Their OCCT FFIs
//! and Rust eval implementations exist (tasks 2325, 2327), but task 2325's triage
//! note (esc-2325-47) explicitly deferred stdlib language-level wiring until a
//! `Tensor`/`MomentOfInertia` type-system task lands. A `compile_with_stdlib`
//! assertion of "no errors" would therefore fail today on every undefined-name
//! diagnostic.
//!
//! `reify_syntax::parse` is independent of name-resolution and gives a stable,
//! defensible signal: it pins that the fixtures are syntactically well-formed
//! Reify source and detects regressions in tree-sitter grammar / lowering.
//!
//! Richer compile_with_stdlib + eval + analytic-tensor coverage is captured as
//! follow-up task 2691 (hard-deps on tasks 2696, 2698, 2699 — the stdlib
//! type-system, list-helper, and language-level wiring tasks). Once those land,
//! this file should grow to mirror `field_source_kinds_smoke.rs`'s 4-level
//! pattern.

use reify_types::ModulePath;

const BLOCK_INERTIA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/block_inertia.ri"
);

const FILLET_TOP_EDGES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/fillet_top_edges.ri"
);

#[test]
fn block_inertia_ri_parses_cleanly() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

#[test]
fn fillet_top_edges_ri_parses_cleanly() {
    let source = std::fs::read_to_string(FILLET_TOP_EDGES_PATH)
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
