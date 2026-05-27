//! Topology-trigger tests for `auto:` type-parameter substitution wiring.
//!
//! PRD: `docs/prds/auto-type-param-resolution.md` task 5 ("Topology trigger"),
//! acceptance criterion 7.
//!
//! # Design decisions
//!
//! Source-level `Bearing<auto: Seal>` parsing is not yet supported
//! (`tree-sitter-reify/grammar.js` `type_arg_list` only allows `$.type_expr`).
//! Tests instead build `EvaluationGraph` instances from `TopologyTemplateBuilder`
//! fixtures with `Type::TypeParam`-typed cells and assign
//! `graph.auto_type_substitution` directly — following the same convention as
//! `auto_type_param_determinism_tests.rs:9-15`.
//!
//! `MultiParamResolutionOutcome.substitution: Vec<(String, String)>` was
//! "plumbed through but NOT yet consumed" per `auto_type_param.rs:81-87`.
//! This task is the v0.1 consumer at the graph level.

use reify_compiler::AutoTypeSubstitution;
use reify_eval::graph::EvaluationGraph;
use reify_test_support::TopologyTemplateBuilder;
use reify_core::Type;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a single-type-param fixture template: `Bearing` with one
/// `Type::TypeParam("T")` cell `seal_marker`.
fn single_param_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("Bearing")
        .param("Bearing", "seal_marker", Type::TypeParam("T".into()), None)
        .build()
}

/// Build a two-type-param fixture template: `Bearing` with cells
/// `seal_marker: TypeParam("A")` and `race_marker: TypeParam("B")`.
fn two_param_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("Bearing")
        .param("Bearing", "seal_marker", Type::TypeParam("A".into()), None)
        .param("Bearing", "race_marker", Type::TypeParam("B".into()), None)
        .build()
}

// ─── step-1: different substitution flips topology fingerprint ────────────────

/// A different `auto_type_substitution` must produce a different
/// `topology_fingerprint()`.
///
/// Pins PRD criterion 7's first half: "Resolution flips a SchemaNode's
/// topology fingerprint."
///
/// Architecture mapping: `EvaluationGraph::topology_fingerprint()` maps to the
/// "SchemaNode.compute()" concept in arch §6.2-6.4 — the fingerprint is the
/// cache key that signals re-elaboration is needed when the substitution
/// changes.
#[test]
fn different_substitution_flips_topology_fingerprint() {
    let template = single_param_template();
    let mut graph_a = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    let mut graph_b = EvaluationGraph::from_templates(&[template]);

    graph_a.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];
    graph_b.auto_type_substitution = vec![("T".into(), "GasketSeal".into())];

    assert_ne!(
        graph_a.topology_fingerprint(),
        graph_b.topology_fingerprint(),
        "different substitutions (ORingSeal vs GasketSeal) must produce different fingerprints"
    );
}

// ─── step-3: same substitution yields same topology fingerprint ───────────────

/// Identical `auto_type_substitution` Vecs must produce identical
/// `topology_fingerprint()` values.
///
/// Pins the cache-reuse contract: identical substitution Vecs canonicalize
/// to identical fingerprints, which is the precondition for "same candidate
/// re-selected" warm-state cache reuse on revert (criterion 7, second half).
#[test]
fn same_substitution_yields_same_topology_fingerprint() {
    let template = single_param_template();
    let mut graph_a = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    let mut graph_b = EvaluationGraph::from_templates(&[template]);

    graph_a.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];
    graph_b.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];

    assert_eq!(
        graph_a.topology_fingerprint(),
        graph_b.topology_fingerprint(),
        "identical substitutions must produce identical fingerprints (cache-reuse contract)"
    );
}

// ─── step-5: insertion order does not affect fingerprint ─────────────────────

/// Two `auto_type_substitution` Vecs with the same logical (param→template)
/// map but different insertion order must produce the same
/// `topology_fingerprint()`.
///
/// Pins the revert-stability contract from PRD criterion 7's second half:
/// the same logical substitution from any source must produce the same
/// fingerprint regardless of how the Vec was assembled.
#[test]
fn substitution_vec_insertion_order_does_not_affect_fingerprint() {
    let template = two_param_template();
    let mut graph_x = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    let mut graph_y = EvaluationGraph::from_templates(&[template]);

    // Same logical map, different insertion order.
    graph_x.auto_type_substitution = vec![("A".into(), "X1".into()), ("B".into(), "Y1".into())];
    graph_y.auto_type_substitution = vec![("B".into(), "Y1".into()), ("A".into(), "X1".into())];

    assert_eq!(
        graph_x.topology_fingerprint(),
        graph_y.topology_fingerprint(),
        "insertion-order must not affect fingerprint (revert-stable: \
         same logical map → same fingerprint regardless of source ordering)"
    );
}

// ─── step-7: empty substitution yields back-compat fingerprint ───────────────

/// An `EvaluationGraph` with the default (empty) `auto_type_substitution`
/// must produce the same fingerprint as one with an explicitly empty Vec.
/// Both must differ from a graph with a non-empty substitution.
///
/// Pins the back-compat invariant: graphs that never had a substitution
/// applied are bit-identical (at fingerprint level) to graphs that opted
/// into the new field with an empty Vec.
#[test]
fn empty_substitution_yields_back_compat_fingerprint() {
    let template = single_param_template();

    // Default: auto_type_substitution is never touched (empty Vec from Default).
    let graph_default = EvaluationGraph::from_templates(std::slice::from_ref(&template));

    // Explicit empty Vec.
    let mut graph_explicit_empty = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    graph_explicit_empty.auto_type_substitution = vec![];

    // Non-empty substitution.
    let mut graph_with_sub = EvaluationGraph::from_templates(&[template]);
    graph_with_sub.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];

    assert_eq!(
        graph_default.topology_fingerprint(),
        graph_explicit_empty.topology_fingerprint(),
        "default (empty) and explicitly-empty substitution must yield identical fingerprints"
    );
    assert_ne!(
        graph_default.topology_fingerprint(),
        graph_with_sub.topology_fingerprint(),
        "non-empty substitution must differ from the empty-substitution fingerprint"
    );
}

// ─── step-9: substitution flip and revert restores topology fingerprint ──────

/// Flip `auto_type_substitution` from substitution_a to substitution_b, then
/// revert to substitution_a; the fingerprint must follow: flip changes it,
/// revert restores it.
///
/// Pins PRD criterion 7's second half: "the same candidate re-selected after
/// a parameter edit + revert → same fingerprint → cache reuse."
///
/// Note: `WarmStatePool` is keyed purely by `NodeId` (no fingerprint argument
/// is passed to donate/checkout), so pool survival is independent of this
/// fingerprint contract and is already covered by
/// `crates/reify-eval/tests/warm_state_donation.rs::
/// pool_state_survives_round_trip_when_cache_cannot_consume`.
#[test]
fn substitution_flip_and_revert_restores_topology_fingerprint() {
    let template = single_param_template();

    // graph_a: substitution_a = ORingSeal.
    let mut graph_a = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    graph_a.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];
    let fp_a = graph_a.topology_fingerprint();

    // graph_b: substitution_b = GasketSeal (flip).
    let mut graph_b = EvaluationGraph::from_templates(std::slice::from_ref(&template));
    graph_b.auto_type_substitution = vec![("T".into(), "GasketSeal".into())];
    let fp_b = graph_b.topology_fingerprint();

    // The flip must change the fingerprint.
    assert_ne!(
        fp_a, fp_b,
        "substitution flip must change topology fingerprint"
    );

    // graph_c: substitution_a again (revert).
    let mut graph_c = EvaluationGraph::from_templates(&[template]);
    graph_c.auto_type_substitution = vec![("T".into(), "ORingSeal".into())];
    let fp_c = graph_c.topology_fingerprint();

    // The revert must restore the original fingerprint.
    assert_eq!(
        fp_c, fp_a,
        "substitution revert must restore original fingerprint"
    );
    assert_ne!(
        fp_c, fp_b,
        "reverted fingerprint must differ from flipped fingerprint"
    );
}

// ─── producer-bug: uniqueness invariant panics in debug ──────────────────────

/// Supplying duplicate `param_name` entries triggers the `debug_assert!` at
/// `graph.rs:646-655` with the message "param names must be unique; duplicates
/// are a producer bug".
///
/// This pins the producer-bug invariant enforced by the existing assert. In
/// release builds the assert is a no-op — behaviour is undefined per the
/// documented "duplicates are a producer bug" contract and callers must not
/// rely on any particular outcome.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "param names must be unique")]
fn duplicate_param_name_panics_in_debug() {
    let template = single_param_template();
    let mut graph = EvaluationGraph::from_templates(&[template]);
    // "T" appears twice — violates the uniqueness invariant.
    graph.auto_type_substitution = vec![("T".into(), "A".into()), ("T".into(), "B".into())];
    graph.topology_fingerprint(); // triggers the debug_assert!
}

// ─── snapshot propagation: CompiledModule.auto_type_substitution → ───────────
//                           EvaluationGraph via Snapshot::from_compiled_module

/// `Snapshot::from_compiled_module` must propagate
/// `CompiledModule.auto_type_substitution` verbatim into
/// `EvaluationGraph.auto_type_substitution` AND the substitution must flip
/// `topology_fingerprint` end-to-end through the production pipeline.
///
/// PRD task 5, acceptance criterion 7 (production path closure).
///
/// Cross-reference: test #11
/// (`multi_param_resolution_outcome_substitution_drives_topology_fingerprint`)
/// pins the by-hand graph-level assignment path; this test pins the
/// *production-pipeline* propagation from `CompiledModule` through
/// `Snapshot::from_compiled_module`.
#[test]
fn snapshot_from_compiled_module_propagates_auto_type_substitution_to_graph() {
    use reify_eval::snapshot::Snapshot;
    use reify_test_support::parse_and_compile_with_stdlib;

    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bearing_auto_seal.ri"
    );

    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/bearing_auto_seal.ri should exist");

    // Compile once; stdlib + example is expensive in an integration test.
    // The baseline keeps the default empty auto_type_substitution.
    // The resolved variant is a cheap clone with the substitution injected
    // (bypasses the still-absent `auto:` parser — field is populated here
    // the same way a future parser-lowering task would populate it).
    let module_baseline = parse_and_compile_with_stdlib(&source);
    let mut module_resolved = module_baseline.clone();
    module_resolved.auto_type_substitution =
        AutoTypeSubstitution::new(vec![("T".into(), "ORingSeal".into())]);

    let snap_resolved = Snapshot::from_compiled_module(&module_resolved);
    let snap_baseline = Snapshot::from_compiled_module(&module_baseline);

    // The field must propagate through the production wiring.
    assert_eq!(
        snap_resolved.graph.auto_type_substitution,
        vec![("T".to_string(), "ORingSeal".to_string())],
        "auto_type_substitution must propagate from CompiledModule to \
         EvaluationGraph via Snapshot::from_compiled_module"
    );

    // The substitution must flip the fingerprint end-to-end through the
    // production path (PRD task 5 acceptance criterion 7).
    assert_ne!(
        snap_resolved.topology_fingerprint, snap_baseline.topology_fingerprint,
        "auto_type_substitution must flip topology_fingerprint end-to-end \
         through Snapshot::from_compiled_module"
    );
}

// ─── step-11: MultiParamResolutionOutcome.substitution drives fingerprint ─────

/// Feed `MultiParamResolutionOutcome.substitution` from a real
/// `resolve_auto_type_params` call directly into
/// `EvaluationGraph::auto_type_substitution` and assert the fingerprint
/// changes vs. a baseline graph with no substitution.
///
/// Pins the cross-crate API contract documented at `auto_type_param.rs:81-87`
/// ("the wiring is in place so a future task can read the map without a
/// signature change") — this task IS that follow-up.
#[test]
fn multi_param_resolution_outcome_substitution_drives_topology_fingerprint() {
    use std::collections::HashMap;

    use reify_compiler::auto_type_param::{AutoTypeParam, resolve_auto_type_params};
    use reify_compiler::{CompiledTrait, TopologyTemplate};
    use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
    use reify_core::SourceSpan;

    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bearing_auto_seal.ri"
    );

    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/bearing_auto_seal.ri should exist");
    let module = parse_and_compile_with_stdlib(&source);

    // Build registries (mirrors auto_type_param_determinism_tests.rs::build_registries).
    let template_registry: HashMap<String, &TopologyTemplate> = module
        .templates
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    let trait_registry: HashMap<String, &CompiledTrait> = module
        .trait_defs
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect();

    let bearing = template_registry
        .get("Bearing")
        .expect("Bearing template must exist in bearing_auto_seal.ri");

    let params = vec![AutoTypeParam {
        name: "T".into(),
        bounds: vec!["Seal".into()],
        free: true,
        use_site_span: SourceSpan::empty(0),
    }];
    let checker = MockConstraintChecker::new();
    let functions: &[reify_ir::CompiledFunction] = &[];
    let mut diagnostics = Vec::new();

    let outcome = resolve_auto_type_params(
        &params,
        &template_registry,
        &trait_registry,
        bearing,
        &checker,
        functions,
        &mut diagnostics,
    );

    // The substitution Vec must be non-empty (lex-first candidate was selected).
    assert!(
        !outcome.substitution.is_empty(),
        "resolve_auto_type_params must produce at least one substitution pair for free=true"
    );

    // Build resolved and baseline graphs from the module's templates.
    let mut graph_resolved = EvaluationGraph::from_templates(&module.templates);
    graph_resolved.auto_type_substitution = outcome.substitution.clone();

    let graph_baseline = EvaluationGraph::from_templates(&module.templates);

    assert_ne!(
        graph_resolved.topology_fingerprint(),
        graph_baseline.topology_fingerprint(),
        "a resolved substitution must change the topology fingerprint vs. an unsubstituted baseline"
    );
}
