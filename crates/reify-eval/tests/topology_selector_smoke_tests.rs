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

use reify_test_support::{errors_only, parse_and_compile_with_stdlib};
use reify_types::ModulePath;

const BLOCK_INERTIA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/block_inertia.ri"
);

const FILLET_TOP_EDGES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/fillet_top_edges.ri"
);

const ALL_TOPOLOGY_SELECTORS_WIRING_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/all_topology_selectors_wiring.ri"
);

/// Verify that `all_topology_selectors_wiring.ri` passes a real face handle (not
/// a Solid) as the second argument to `adjacent_faces` and `shared_edges`.
///
/// Two assertions fire:
///
/// 1. **Structural shape (RED until S2)**: The source must NOT contain the
///    old placeholder strings `"adjacent_faces(body, body)"` or
///    `"shared_edges(body, body)"`.  These were sufficient for compile-time
///    wiring (which keys only on the function name, not arg types) but are
///    semantically wrong — both relational selectors expect a face handle as
///    their second argument (§3.9 PRD signature), not a Solid.  The assertion
///    pins runtime arg-shape correctness and will FAIL RED until Step S2
///    introduces `let top_face = single(top_faces)` and threads it through.
///
/// 2. **Compile clean**: After the fix, the new face-handle form must still
///    compile with no Error-severity diagnostics (tasks 2699 and 2698 are
///    both landed on HEAD).
#[test]
fn all_topology_selectors_wiring_passes_face_handles_to_relational_selectors() {
    let source = std::fs::read_to_string(ALL_TOPOLOGY_SELECTORS_WIRING_PATH)
        .expect("examples/topology_selectors/all_topology_selectors_wiring.ri should exist");

    assert!(
        !source.contains("adjacent_faces(body, body)"),
        "all_topology_selectors_wiring.ri should pass a face handle (not a Solid) as the \
         second argument to adjacent_faces; found the old placeholder `adjacent_faces(body, body)` \
         — fix by introducing `let top_face = single(top_faces)` and using it instead"
    );
    assert!(
        !source.contains("shared_edges(body, body)"),
        "all_topology_selectors_wiring.ri should pass face handles to shared_edges; found the old \
         placeholder `shared_edges(body, body)` — fix by threading `top_face` through the call"
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/all_topology_selectors_wiring.ri should compile with \
         no error-severity diagnostics after the face-handle fix, got:\n{:#?}",
        errors_only(&compiled)
    );
}

#[test]
fn all_topology_selectors_wiring_compiles_with_stdlib() {
    let source = std::fs::read_to_string(ALL_TOPOLOGY_SELECTORS_WIRING_PATH)
        .expect("examples/topology_selectors/all_topology_selectors_wiring.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/all_topology_selectors_wiring.ri should compile with \
         no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
}

/// Verify that `block_inertia.ri` compiles with no Error-severity diagnostics.
///
/// This deepens the existing parse-only smoke to compile-with-stdlib, which became
/// achievable once task 2699 wired `moment_of_inertia` (and its return type
/// `Tensor<2,3,MomentOfInertia>`) as a language-level stdlib binding.
///
/// Note: the runtime value of the `i` cell is still `Value::Undef` because
/// eval-side dispatch for `moment_of_inertia` has not yet been added to
/// `try_eval_topology_selector` in `crates/reify-eval/src/geometry_ops.rs`.
/// That gap is documented by the `#[ignore]`-gated test
/// `block_inertia_evals_moment_of_inertia_to_tensor` (step S5).
#[test]
fn block_inertia_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/block_inertia.ri should compile with no error-severity \
         diagnostics (task 2699 wired moment_of_inertia), got:\n{:#?}",
        errors_only(&compiled)
    );
}

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
