//! Debug-gate integration suite for the no-stale-Undef invariant checker
//! (task α, PRD docs/prds/v0_6/eval-uniform-dependency-handling.md §6.1).
//!
//! Runs `reify_eval::invariants::check_no_stale_undef` — and the
//! `Engine::check_no_stale_undef` convenience wrapper — over the eval
//! fixture corpus + examples/, proving the invariant holds post-eval.
//!
//! Step-1 (RED): the mandatory anti-silent-accept seeded-violation
//! self-test. Fabricates a minimal post-eval state (NOT a real
//! compile+eval) containing one genuine stale-Undef consumer and asserts
//! the checker actually fires — a checker that always returns `vec![]`
//! would otherwise make every downstream corpus test in this suite
//! vacuously green.

use std::collections::HashMap;

use reify_core::{ContentHash, Type, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::deps::DependencyTrace;
use reify_eval::graph::{EvaluationGraph, ValueCellNode};
use reify_ir::{CompiledExpr, DeterminacyState, PersistentMap, Value};

/// Seeded state: `producer` is resolved (non-Undef); `consumer`'s
/// `default_expr` is a `ValueRef(producer)` — NOT an undef literal — and its
/// stored value is `Undef` even though its one static dependency is fully
/// resolved. This is precisely the causeless staleness §6.1 exists to catch:
/// no exclusion (auto, missing/Undef dep, @optimized, guard-inactive,
/// undef-literal) applies, so the checker MUST report it.
#[test]
fn seeded_stale_undef_violation_is_reported() {
    let producer_id = ValueCellId::new("SeededDemo", "producer");
    let consumer_id = ValueCellId::new("SeededDemo", "consumer");

    let mut graph = EvaluationGraph::default();

    let producer_expr = CompiledExpr::literal(Value::length(1.0), Type::length());
    graph.value_cells.insert(
        producer_id.clone(),
        ValueCellNode {
            id: producer_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: Type::length(),
            default_expr: Some(producer_expr),
            content_hash: ContentHash::of_str("seeded-producer"),
        },
    );

    let consumer_expr = CompiledExpr::value_ref(producer_id.clone(), Type::length());
    graph.value_cells.insert(
        consumer_id.clone(),
        ValueCellNode {
            id: consumer_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: Type::length(),
            default_expr: Some(consumer_expr),
            content_hash: ContentHash::of_str("seeded-consumer"),
        },
    );

    let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    values.insert(
        producer_id.clone(),
        (Value::length(1.0), DeterminacyState::Determined),
    );
    values.insert(
        consumer_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );

    let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();
    trace_map.insert(
        NodeId::Value(consumer_id.clone()),
        DependencyTrace {
            reads: vec![producer_id.clone()],
            realization_reads: Vec::new(),
        },
    );

    let violations =
        reify_eval::invariants::check_no_stale_undef(&graph, &values, &trace_map, &[]);

    assert!(
        !violations.is_empty(),
        "expected the checker to report the seeded stale-Undef consumer, got zero \
         violations — a checker that never fires would make the corpus sweep \
         vacuously green"
    );
    assert!(
        violations.iter().any(|v| v.cell == consumer_id),
        "expected a violation naming consumer cell {:?}, got {:?}",
        consumer_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

/// Seeded state (task #5120 R2c): a `union` composition cell whose two
/// operands are ALREADY resolved `Value::Selector(Face)` cells, but the
/// union cell itself is (mis-scheduled) still `Value::Undef`. Once
/// composition is wired onto the kernel-free symbolic-eval surface
/// (`geometry_ops::is_symbolic_eval_wired_selector_ctor` returns `true` for
/// `union`), clause 8's build-only exemption no longer applies to this call
/// — so this is precisely the causeless staleness §6.1 exists to catch, and
/// the checker MUST report it. A well-formed composition can never actually
/// go stale like this once wired (both operands resolve within the same
/// mint pass), so this fabricated state is the only way to exercise the
/// class-coverage change — mirrors `seeded_stale_undef_violation_is_reported`
/// above.
///
/// **RED before wiring**: `union` is still build-only-exempt (clause 8 rule
/// 2), so the checker does NOT report this cell — this test FAILS.
/// **GREEN after wiring** (task #5120 step-2): `union` leaves the build-only
/// exemption by construction, so the checker reports it.
#[test]
fn seeded_stale_undef_composition_violation_is_reported() {
    use reify_core::ty::SelectorKind;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};

    let op1_id = ValueCellId::new("SeededComposition", "op1");
    let op2_id = ValueCellId::new("SeededComposition", "op2");
    let union_id = ValueCellId::new("SeededComposition", "u");

    let selector_type = Type::Selector(SelectorKind::Face);

    let mut graph = EvaluationGraph::default();

    // op1 / op2: Selector(Face) leaf producers. Their own default_expr is
    // irrelevant to this seeded state — only their STORED value and
    // cell_type matter — so a harmless Undef literal is used.
    graph.value_cells.insert(
        op1_id.clone(),
        ValueCellNode {
            id: op1_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, selector_type.clone())),
            content_hash: ContentHash::of_str("seeded-op1"),
        },
    );
    graph.value_cells.insert(
        op2_id.clone(),
        ValueCellNode {
            id: op2_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, selector_type.clone())),
            content_hash: ContentHash::of_str("seeded-op2"),
        },
    );

    // The union cell: default_expr = FunctionCall{"union", [ValueRef(op1), ValueRef(op2)]},
    // both args typed Type::Selector(Face) — the structural key
    // `consumes_geometry_or_selector` reads.
    let union_expr = CompiledExpr {
        kind: reify_ir::CompiledExprKind::FunctionCall {
            function: reify_ir::ResolvedFunction {
                name: "union".to_string(),
                qualified_name: "std::union".to_string(),
            },
            args: vec![
                CompiledExpr::value_ref(op1_id.clone(), selector_type.clone()),
                CompiledExpr::value_ref(op2_id.clone(), selector_type.clone()),
            ],
        },
        result_type: selector_type.clone(),
        content_hash: ContentHash::of_str("seeded-union"),
    };
    graph.value_cells.insert(
        union_id.clone(),
        ValueCellNode {
            id: union_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type,
            default_expr: Some(union_expr),
            content_hash: ContentHash::of_str("seeded-union-cell"),
        },
    );

    // Resolved Selector(Face) leaf value, reused for both operands.
    let ghr = GeometryHandleRef {
        realization_ref: reify_core::identity::RealizationNodeId::new("SeededComposition", 0),
        upstream_values_hash: [0x7Cu8; 32],
        kernel_handle: None,
    };
    let leaf_sv = SelectorValue::leaf(SelectorKind::Face, ghr, LeafQuery::All)
        .expect("Face/All is a valid kind-closure");

    let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    values.insert(
        op1_id.clone(),
        (Value::Selector(leaf_sv.clone()), DeterminacyState::Determined),
    );
    values.insert(
        op2_id.clone(),
        (Value::Selector(leaf_sv), DeterminacyState::Determined),
    );
    // The union cell is (mis-scheduled) still Undef despite both operands
    // being fully resolved — the causeless staleness this test seeds.
    values.insert(
        union_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );

    let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();
    trace_map.insert(
        NodeId::Value(union_id.clone()),
        DependencyTrace {
            reads: vec![op1_id.clone(), op2_id.clone()],
            realization_reads: Vec::new(),
        },
    );

    let violations =
        reify_eval::invariants::check_no_stale_undef(&graph, &values, &trace_map, &[]);

    assert!(
        violations.iter().any(|v| v.cell == union_id),
        "expected a stale-Undef violation naming the union cell {:?} now that composition \
         is eval-wired (task #5120 R2c) — got {:?}",
        union_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

/// Positive companion to `seeded_stale_undef_composition_violation_is_reported`
/// above (review suggestion, task #5120 R2c): proves the by-NAME clause-8
/// classification change for `union`/`intersect`/`difference` cannot regress
/// the SOLID-CSG-BOOLEAN overload sharing those names (e.g.
/// `manifold_boolean`'s `union(box_a, box_b): Solid`, `m5_geometry_flange`'s
/// `difference(body, holes): Solid`).
///
/// `geometry_ops::is_symbolic_eval_wired_selector_ctor` classifies a `union`
/// `FunctionCall` as eval-wired BY NAME ALONE — it inspects only
/// `function.name`, never arg/result types — so it cannot distinguish the
/// selector-composition overload from the solid-boolean one. For a
/// solid-boolean `union` cell this means clause 8 rule 2
/// (`consumes_geometry_or_selector(expr) &&
/// !is_symbolic_eval_wired_selector_ctor(expr)`) evaluates to `false` — i.e.
/// clause 8 ALONE no longer exempts it, exactly as for the selector overload
/// seeded above.
///
/// This seeds that exact shape — a `union` cell typed `Type::Geometry` with
/// `Type::Geometry`-typed args (contrast the `Type::Selector(Face)` shape
/// above), both operands resolved to `Value::GeometryHandle` — left
/// `Value::Undef`, and asserts the checker does NOT report it. That proves
/// clause 7's `Type::Geometry` cell-type check (this module's doc comment,
/// clause 7 — which runs BEFORE clause 8 and never consults the call's name)
/// exempts it end-to-end regardless of clause 8's by-name classification, so
/// solid-boolean diagnostics cannot regress from the R2c by-name wiring.
/// Were clause 7 ever removed, weakened, or reordered after clause 8, this
/// test would fail.
#[test]
fn seeded_solid_boolean_union_undef_is_exempted_by_geometry_clause() {
    let box_a_id = ValueCellId::new("SeededSolidBoolean", "box_a");
    let box_b_id = ValueCellId::new("SeededSolidBoolean", "box_b");
    let union_id = ValueCellId::new("SeededSolidBoolean", "u");

    let geometry_type = Type::Geometry;

    let mut graph = EvaluationGraph::default();

    // box_a / box_b: Type::Geometry leaf producers (realized solid handles).
    // Their own default_expr is irrelevant to this seeded state — only their
    // STORED value and cell_type matter — so a harmless Undef literal is used
    // (mirrors op1/op2 above).
    graph.value_cells.insert(
        box_a_id.clone(),
        ValueCellNode {
            id: box_a_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: geometry_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, geometry_type.clone())),
            content_hash: ContentHash::of_str("seeded-box-a"),
        },
    );
    graph.value_cells.insert(
        box_b_id.clone(),
        ValueCellNode {
            id: box_b_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: geometry_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, geometry_type.clone())),
            content_hash: ContentHash::of_str("seeded-box-b"),
        },
    );

    // The union cell: default_expr = FunctionCall{"union", [ValueRef(box_a),
    // ValueRef(box_b)]}, both args typed Type::Geometry — the solid-boolean
    // overload shape (contrast the Type::Selector(Face) args seeded above).
    let union_expr = CompiledExpr {
        kind: reify_ir::CompiledExprKind::FunctionCall {
            function: reify_ir::ResolvedFunction {
                name: "union".to_string(),
                qualified_name: "std::union".to_string(),
            },
            args: vec![
                CompiledExpr::value_ref(box_a_id.clone(), geometry_type.clone()),
                CompiledExpr::value_ref(box_b_id.clone(), geometry_type.clone()),
            ],
        },
        result_type: geometry_type.clone(),
        content_hash: ContentHash::of_str("seeded-solid-union"),
    };
    graph.value_cells.insert(
        union_id.clone(),
        ValueCellNode {
            id: union_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: geometry_type,
            default_expr: Some(union_expr),
            content_hash: ContentHash::of_str("seeded-solid-union-cell"),
        },
    );

    // Resolved GeometryHandle value (a realized solid body), reused for both
    // operands.
    let handle = Value::GeometryHandle {
        realization_ref: reify_core::identity::RealizationNodeId::new("SeededSolidBoolean", 0),
        upstream_values_hash: [0x51u8; 32],
        kernel_handle: None,
    };

    let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    values.insert(
        box_a_id.clone(),
        (handle.clone(), DeterminacyState::Determined),
    );
    values.insert(box_b_id.clone(), (handle, DeterminacyState::Determined));
    // The union cell is left Undef despite both operands being fully
    // resolved — clause 7's documented standing surface gap (Type::Geometry
    // cells are hydrated only inside build()'s local ValueMap, never written
    // back into the retained eval_state() this checker inspects).
    values.insert(
        union_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );

    let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();
    trace_map.insert(
        NodeId::Value(union_id.clone()),
        DependencyTrace {
            reads: vec![box_a_id.clone(), box_b_id.clone()],
            realization_reads: Vec::new(),
        },
    );

    let violations =
        reify_eval::invariants::check_no_stale_undef(&graph, &values, &trace_map, &[]);

    assert!(
        !violations.iter().any(|v| v.cell == union_id),
        "expected the solid-boolean union cell {:?} to be EXEMPTED by clause 7 \
         (Type::Geometry) end-to-end, regardless of clause 8's by-name eval-wired \
         classification of `union` — got a reported violation: {:?}",
        union_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

/// Reviewer follow-up (task #5120 R2c amendment): `is_symbolic_eval_wired_selector_ctor`
/// classifies `union`/`intersect`/`difference` BY NAME alone, with no regard
/// for whether a given call's operands are actually resolved. R2c's own
/// design assumes INLINE nested operands (both resolve within the same mint
/// pass), but nothing stops a composition from taking a cross-cell
/// `ValueRef` operand instead — e.g. `let u = union(a, b)` where `a` is
/// itself a still-build-only (kernel-bearing) selector cell. Since
/// `expr_requires_build_only_resolution` only walks `u`'s own static expr
/// tree (a `ValueRef(a)` leaf, never `a`'s definition — see that function's
/// doc comment), clause 8 alone cannot see that `a` is unresolvable on
/// `eval()`. This seeds exactly that shape and proves the α net stays sound
/// anyway, via clause 4 (not clause 8).
///
/// `op1` stands in for that build-only cross-cell producer — as with op1/op2
/// in `seeded_stale_undef_composition_violation_is_reported` above, its own
/// `default_expr` is irrelevant to this seeded state (a harmless Undef
/// literal is used); only its `cell_type` (`Selector(Face)`) and its STORED
/// value (`Undef` — it is never resolved here, exactly as a real build-only
/// cell would remain until `build()`) matter. `op2` is a fully-resolved
/// `Selector(Face)` leaf. `u = union(op1, op2)` is left `Undef`, matching
/// what the real symbolic mint would actually do:
/// `reconstruct_selector_value_symbolic`'s `ValueRef` arm reads `op1`'s
/// stored `Undef`, returns `None`, and that `None` short-circuits
/// `eval_variadic_composition_symbolic`'s `collect::<Option<Vec<_>>>()` for
/// the whole call.
///
/// Asserts the checker does NOT report `u`: clause 4 ("a read that is itself
/// Undef, makes `c` EXEMPT") fires on `u`'s `op1` read before clause 8 is
/// ever consulted, independently of clause 8's by-name composition
/// classification. Clause 4 and clause 8 are complementary, not redundant:
/// clause 4 asks "is this specific dependency resolved right now", clause 8
/// asks "can this call EVER resolve on `eval()` at all".
#[test]
fn seeded_composition_over_unresolved_cross_cell_operand_is_exempted_by_dependency_clause() {
    use reify_core::ty::SelectorKind;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};

    let op1_id = ValueCellId::new("SeededCrossCellComposition", "op1");
    let op2_id = ValueCellId::new("SeededCrossCellComposition", "op2");
    let union_id = ValueCellId::new("SeededCrossCellComposition", "u");

    let selector_type = Type::Selector(SelectorKind::Face);

    let mut graph = EvaluationGraph::default();

    // op1: stands in for a build-only (kernel-bearing) selector cell — its
    // own default_expr is irrelevant to this seeded state (mirrors op1/op2
    // in seeded_stale_undef_composition_violation_is_reported above); only
    // its cell_type and its STORED value (Undef, never resolved) matter.
    graph.value_cells.insert(
        op1_id.clone(),
        ValueCellNode {
            id: op1_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, selector_type.clone())),
            content_hash: ContentHash::of_str("seeded-cross-cell-op1"),
        },
    );
    // op2: a fully-resolved Selector(Face) leaf producer.
    graph.value_cells.insert(
        op2_id.clone(),
        ValueCellNode {
            id: op2_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type.clone(),
            default_expr: Some(CompiledExpr::literal(Value::Undef, selector_type.clone())),
            content_hash: ContentHash::of_str("seeded-cross-cell-op2"),
        },
    );

    // The union cell: default_expr = FunctionCall{"union", [ValueRef(op1),
    // ValueRef(op2)]} — a cross-cell ValueRef shape, NOT the inline nested
    // FunctionCall shape R2c's design targets.
    let union_expr = CompiledExpr {
        kind: reify_ir::CompiledExprKind::FunctionCall {
            function: reify_ir::ResolvedFunction {
                name: "union".to_string(),
                qualified_name: "std::union".to_string(),
            },
            args: vec![
                CompiledExpr::value_ref(op1_id.clone(), selector_type.clone()),
                CompiledExpr::value_ref(op2_id.clone(), selector_type.clone()),
            ],
        },
        result_type: selector_type.clone(),
        content_hash: ContentHash::of_str("seeded-cross-cell-union"),
    };
    graph.value_cells.insert(
        union_id.clone(),
        ValueCellNode {
            id: union_id.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            cell_type: selector_type,
            default_expr: Some(union_expr),
            content_hash: ContentHash::of_str("seeded-cross-cell-union-cell"),
        },
    );

    let ghr = GeometryHandleRef {
        realization_ref: reify_core::identity::RealizationNodeId::new(
            "SeededCrossCellComposition",
            0,
        ),
        upstream_values_hash: [0x9Eu8; 32],
        kernel_handle: None,
    };
    let leaf_sv = SelectorValue::leaf(SelectorKind::Face, ghr, LeafQuery::All)
        .expect("Face/All is a valid kind-closure");

    let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    // op1 is NEVER resolved — the build-only cross-cell producer this test
    // seeds; it stays Undef exactly as a real build-only cell would until
    // build().
    values.insert(
        op1_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );
    values.insert(
        op2_id.clone(),
        (Value::Selector(leaf_sv), DeterminacyState::Determined),
    );
    // u is left Undef, matching what the real symbolic mint would do when
    // one operand cell reads Undef (see doc comment above).
    values.insert(
        union_id.clone(),
        (Value::Undef, DeterminacyState::Undetermined),
    );

    let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();
    trace_map.insert(
        NodeId::Value(union_id.clone()),
        DependencyTrace {
            reads: vec![op1_id.clone(), op2_id.clone()],
            realization_reads: Vec::new(),
        },
    );

    let violations =
        reify_eval::invariants::check_no_stale_undef(&graph, &values, &trace_map, &[]);

    assert!(
        !violations.iter().any(|v| v.cell == union_id),
        "expected the composition cell {:?} to be EXEMPTED by clause 4 (its op1 \
         read is itself unresolved) regardless of clause 8's by-name eval-wired \
         classification of `union` — got a reported violation: {:?}",
        union_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

/// Reviewer follow-up (task #5120 R2c amendment, round 3): on the
/// kind-closure `Err` path, `eval_variadic_composition_symbolic`
/// (`geometry_ops.rs`) mints a plain `Value::Undef` (no `UndefCause` tag) for
/// a composition whose operands resolve to MISMATCHED `SelectorKind`s (e.g.
/// `union(faces(b), edges(b))`). That cell is `Type::Selector`-typed (clause
/// 7's `Type::Geometry` carve-out does not apply) and `union` IS
/// `is_symbolic_eval_wired_selector_ctor` (clause 8's build-only exemption
/// does not apply either) — so IF this cell's Undef ever reached a
/// `check_no_stale_undef` call, the review asked whether anything actually
/// exempts it.
///
/// This drives the REAL compiler + `Engine::eval` over the exact BT1 fixture
/// already pinned by `selector_boundary_gate.rs::bt1_wrong_kind_union_rejected`
/// and `reify-compiler/tests/selector_composition_tests.rs`, to settle the
/// question empirically instead of by assumption — mirroring the discovery
/// method of `symbolic_selector_composition_eval.rs`'s
/// `solid_csg_boolean_union_is_not_stale_undef_on_eval_and_resolves_on_build`
/// (whose own doc comment records a similar "verified fact, not assumption"
/// correction).
///
/// **Verified facts:**
///
/// 1. `union(faces(b), edges(b))` ALWAYS emits exactly one Error-severity
///    `DiagnosticCode::SelectorKindMismatch` at compile time —
///    `selector_composition_result_type` (units.rs) emits it unconditionally
///    for any kind mismatch, with no escape hatch, so a real compiled `.ri`
///    program can never reach `eval()` in this shape unless a caller
///    explicitly disregards that error. This is the leg of the reviewer's
///    disjunction that actually holds.
/// 2. It is NOT independently exempted at the checker level (the other leg
///    does not hold): if a caller disregards diagnostics and evaluates
///    anyway — as this test deliberately does, to close the loop — the
///    resulting `Value::Undef` cell IS reported by `check_no_stale_undef`;
///    no clause catches it. Tagging it `UndefCause::OpContractFailed`
///    (the review's alternative suggestion) would not change that:
///    `check_no_stale_undef`'s clause 6 excludes cells only by the
///    *structural shape* of `default_expr` (a literal `undef`) — the
///    checker's signature (`graph, values, trace_map, functions`) never
///    receives an `UndefCause` map at all, by design (its own doc comment:
///    "a pure op-contract-failure cell... α cannot distinguish that from
///    genuine staleness").
///
/// So the real backstop is caller discipline, not a checker-level
/// exemption: every current caller of `check_no_stale_undef` in this crate
/// (`run_corpus_shard` below, and every real-pipeline test in
/// `symbolic_selector_composition_eval.rs`) checks `compiled.diagnostics`
/// for errors FIRST and skips eval / never calls the checker when errors are
/// present. `check_no_stale_undef` is also not wired into any live GUI/CLI/
/// LSP surface today — only this crate's own test suite calls it — so there
/// is no live user-facing false-positive risk from this defensive arm right
/// now. This test exists so a regression in either guarantee (the compiler
/// dropping the diagnostic, or a future caller skipping the errors-first
/// check) is caught by a named test failure instead of resting on this doc
/// comment alone.
#[test]
fn seeded_kind_mismatch_composition_undef_is_unexempted_without_caller_discipline() {
    use reify_core::diagnostics::DiagnosticCode;

    let source = std::fs::read_to_string(format!(
        "{}/tests/fixtures/selectors/bt1_wrong_kind_union.ri",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture bt1_wrong_kind_union.ri must exist");
    let compiled = reify_test_support::compile_source_with_stdlib(&source);

    // Leg 1: the compiler ALWAYS rejects this before eval.
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert_eq!(
        errors.len(),
        1,
        "bt1_wrong_kind_union.ri must compile with exactly 1 error, got {errors:#?}"
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "expected DiagnosticCode::SelectorKindMismatch, got {:?}",
        errors[0].code
    );

    // Leg 2: deliberately evaluate anyway — bypassing the errors-first
    // discipline every real caller follows — to empirically settle whether
    // the checker itself would exempt the resulting cell.
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        None,
    );
    engine.eval(&compiled);

    let violations = engine.check_no_stale_undef();
    let cell_id = ValueCellId::new("BT1WrongKindUnion", "u");
    assert!(
        violations.iter().any(|v| v.cell == cell_id),
        "expected the kind-mismatch union cell {:?} to be reported (no clause \
         exempts a Selector-typed, deps-resolved, eval-wired composition cell \
         left Undef by the kind-closure Err arm) when eval'd despite the \
         compile error — got {:?}. If this now passes with zero violations, \
         the exemption mechanism changed and this test's doc comment (and the \
         R2c review finding it pins) should be revisited",
        cell_id,
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

// ── The shared engine constructor every sweep in this file uses ─────────────

/// The ONE engine-construction site every sweep in this file routes through —
/// the deliberately-undef fixture test, `run_corpus_shard`'s eval sweep,
/// `build_surface_violations`'s build sweep and `diag_per_file_timing`.
///
/// It existed as three hand-copied blocks until the copies drifted (task 5578):
/// the build sweep registered the shell-extract trampolines and the eval sweep
/// did not, so the eval sweep was still sweeping
/// `examples/fea_shell_too_thick_annotated.ri` on a DEGRADED dispatch —
/// `@optimized target "shell-extract::extract": no registered compute trampoline`
/// — which is the very defect class this file gates against. One constructor is
/// what keeps them from drifting again.
///
/// The registered arm calls [`Engine::register_production_compute_fns`], the
/// canonical bundler production uses (`reify-cli`'s `configured_eval_engine`
/// routes through it), rather than hand-listing individual registrars: a NEW
/// production trampoline set then reaches these sweeps automatically. The
/// mesh-morph producer is `Unavailable` because `reify-eval`'s own tests do not
/// depend on `reify-mesh-morph`; it is a producer-side optimization, not a
/// dispatch target, so no `@optimized` target goes unregistered because of it.
///
/// `register_compute` is an explicit switch, not a convenience knob: `false` is
/// what lets `seeded_build_surface_sweep_reports_a_planted_violation` reproduce
/// the task-5578 defect in miniature. Only that self-test may pass `false`.
///
/// A fresh [`Engine`] per call, so `register_production_compute_fns`'s
/// panic-on-double-registration contract is never at risk.
fn gate_engine(register_compute: bool) -> reify_eval::Engine {
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(reify_test_support::MockGeometryKernel::new())),
    );
    if register_compute {
        engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
            reason: "reify-eval's own test harness does not depend on reify-mesh-morph",
        });
    }
    engine
}

// ── Step-7: Engine-path corpus test over the deliberately-undef fixtures ────

/// The four fixtures purpose-built for the undef-self-describing PRD family
/// (tasks 4321/4322/4323/4326, α/β/γ/η) — each deliberately packed with
/// non-solver Undef origins (Unbound, propagated, UserUndef, AwaitingSolve,
/// and an op cell reading an Undef input). None of these origins may be
/// reported by `check_no_stale_undef`: every one is excluded by clause 1
/// (auto), clause 2 (no `default_expr`), clause 4 (Undef/missing dep), or
/// clause 6 (undef-literal) — see `docs/prds/v0_6/eval-uniform-dependency-handling.md`
/// §6.1. `undef_cause_solve_failed.ri` is deliberately NOT in this list (it
/// needs a solver-attached engine, `MockConstraintSolver::new_infeasible`,
/// to exercise its SolveFailed classification); it's still covered by the
/// broad corpus sweep (step 9), where its lone cell is an Auto param exempt
/// via clause 1 regardless of solver wiring.
const DELIBERATELY_UNDEF_FIXTURES: &[&str] = &[
    "undef_causes_layer1",
    "undef_trace",
    "undef_boundary_representative",
    "undef_cause_op_contract",
];

/// RED until step-8: `Engine::check_no_stale_undef` does not exist yet.
#[test]
fn deliberately_undef_fixtures_report_zero_violations() {
    for name in DELIBERATELY_UNDEF_FIXTURES {
        let path = format!(
            "{}/tests/fixtures/{name}.ri",
            env!("CARGO_MANIFEST_DIR")
        );
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading fixture {name}.ri at {path}: {e}"));

        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        assert!(
            errors.is_empty(),
            "{name}.ri should compile without errors: {errors:#?}"
        );

        let mut engine = gate_engine(true);
        engine.eval(&compiled);

        let violations = engine.check_no_stale_undef();
        assert!(
            violations.is_empty(),
            "{name}.ri: expected zero stale-Undef violations (every Undef here \
             is a deliberate, excluded origin), got {violations:?}"
        );
    }
}

// ── Step-9/10: broad debug-gate corpus sweep ─────────────────────────────────

/// Recursively collect every `.ri` file under `dir` (including subdirectories).
/// Unreadable entries/directories are silently skipped — this only ever walks
/// our own repo directories, which are expected to be readable.
fn collect_ri_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// Files with a residual stale-Undef violation that is NOT a checker gap
/// fixable within `invariants.rs`'s `(graph, values, trace_map, functions)`
/// signature — each traced to its root cause during the α broad-sweep
/// investigation (task 4952 step-10). Matched by path SUFFIX against each
/// corpus file's display path. Every skip is PRINTED (never silent) so
/// bounded coverage never reads as full coverage; if a future engine change
/// resolves one of these, its entry should be deleted (not left as dead
/// weight) — the corpus sweep will still pass either way.
const KNOWN_RESIDUAL_SKIPS: &[(&str, &str)] = &[
    (
        "examples/integration_corner_cases.ri",
        "RecTree.child.{span,depth}: a `sub child = RecTree(...) where depth > 0` \
         self-recursive sub. The compiler statically emits one placeholder level of \
         child value cells regardless of the runtime `where` guard's truth value, but \
         that guard's active/inactive state is a compiler-side concept never threaded \
         into the runtime EvaluationGraph (unlike value-cell-level `guard()` branches, \
         which DO get a GuardedGroupInfo entry). Fixing this needs a new \
         EvaluationGraph field populated from the compiler's sub-instantiation guard \
         info — a change to shared graph-construction code, out of this task's scope.",
    ),
    (
        "crates/reify-eval/tests/fixtures/match_block_decls_bolt.ri",
        "Bolt.head.across_flats: a decl-level `match head_type { ... => sub head: ... }` \
         block. The compiler tracks per-arm active/inactive state in \
         `TopologyTemplate::match_arm_groups` (`GuardedDeclGroup`), but \
         `EvaluationGraph::from_templates` does not carry that field into the runtime \
         graph at all (confirmed: no analogous field exists on EvaluationGraph). Same \
         class of gap as the RecTree entry above, for match blocks instead of `where` \
         guards — needs shared graph-construction plumbing, out of this task's scope.",
    ),
    (
        "examples/multi_load_bracket.ri",
        "MultiLoadBracket.critical_case: `worst_case(results, |r| r)` — a lambda-over-Map \
         combinator. Reproducibly hits a pre-existing reify-expr dispatch gap \
         (\"[reify-expr] sample: Field lambda is not a Lambda: Undef\", printed 3x during \
         this sweep — once per load case) unrelated to geometry, kinematics, or \
         dynamics. A worst_case/lambda-dispatch product limitation, not a staleness \
         false-positive this checker should paper over.",
    ),
    (
        "examples/surface_finish_functional.ri",
        "Demo.total: reads through `let bom = AssemblyBOM()` — a whole-structure VALUE \
         constructor call (not a `sub` declaration) for a structure that itself declares \
         nested subs (`sub p1 = Plate()`, `sub p2 = Bracket()`). Their finishing_cost \
         fields do not resolve when the parent is constructed as an inline value \
         expression rather than a `sub`. A pre-existing struct-constructor-with-nested- \
         subs eval limitation, independent of geometry/staleness.",
    ),
];

/// Number of shards the broad corpus sweep is split across — one shard per
/// `broad_corpus_sweep_shard_NN` `#[test]` fn (below).
///
/// The user-observable debug-gate signal (task α, PRD §6.1 row 6 + §9) is
/// that every `.ri` fixture under `crates/reify-eval/tests/fixtures/` and
/// `examples/`, plus the explicit #4946 R3f-bridge premise fixture
/// `tests/prd-gate/fixtures/geometry_let_selector_consumer.ri`, produces
/// ZERO stale-Undef violations — modulo the explicit, printed
/// `KNOWN_RESIDUAL_SKIPS` above. That was originally ONE test compiling +
/// evaluating all ~270 corpus files sequentially, which passed but took
/// ~270s wall-clock (each file costs a fraction of a second, same as any
/// other single compile+eval test in this suite, just summed 270x) — long
/// enough, as the last test left running with nothing to interleave its
/// output with, to trip the verify pipeline's heartbeat-idle backstop
/// despite every file passing (task 4952 debug fix). Sharding into
/// `CORPUS_SHARD_COUNT` independent `#[test]` fns lets cargo-nextest run
/// them as separate, concurrently-scheduled processes — each reporting its
/// own PASS/SLOW line — so the worst-case silent gap is bounded by roughly
/// one shard's share of the corpus (~11 files) instead of the whole corpus,
/// regardless of host CPU contention.
const CORPUS_SHARD_COUNT: usize = 24;

/// Collects the full, deterministically-sorted corpus file list (fixtures +
/// examples + the explicit #4946 selector-consumer premise fixture) and the
/// selector-consumer path itself. Every shard calls this and keeps only the
/// files whose index (in this SAME sorted order) is `≡ shard_index (mod
/// CORPUS_SHARD_COUNT)`, so the partition is stable across shards/runs
/// without needing to share state between the independent shard processes.
/// Cheap (a directory walk, no compilation) — recomputing it once per shard
/// isn't worth caching.
fn corpus_files() -> (Vec<std::path::PathBuf>, std::path::PathBuf) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = std::path::Path::new(manifest_dir).join("tests/fixtures");
    let examples_dir = std::path::Path::new(manifest_dir).join("../../examples");
    let selector_consumer_path = std::path::Path::new(manifest_dir)
        .join("../../tests/prd-gate/fixtures/geometry_let_selector_consumer.ri");

    let mut files = Vec::new();
    collect_ri_files(&fixtures_dir, &mut files);
    collect_ri_files(&examples_dir, &mut files);
    files.push(selector_consumer_path.clone());
    files.sort();
    (files, selector_consumer_path)
}

/// Runs the broad corpus sweep over the slice of the corpus assigned to
/// `shard_index` (of `CORPUS_SHARD_COUNT` total — see its doc comment for
/// why the sweep is sharded at all). Semantics per file are identical to the
/// pre-sharding single-test sweep: SKIP any file whose compile emits an
/// Error-severity diagnostic (printed), exempt `KNOWN_RESIDUAL_SKIPS`
/// entries (printed), and require zero violations everywhere else. If the
/// #4946 selector-consumer premise fixture falls in this shard, also
/// require it was evaluated (not skipped) with zero violations — mirroring
/// the original single-test assertion exactly, just scoped to whichever one
/// shard deterministically contains that path.
fn run_corpus_shard(shard_index: usize) {
    let (files, selector_consumer_path) = corpus_files();
    let shard_files: Vec<&std::path::PathBuf> = files
        .iter()
        .enumerate()
        .filter(|(i, _)| i % CORPUS_SHARD_COUNT == shard_index)
        .map(|(_, f)| f)
        .collect();
    let selector_consumer_in_shard = shard_files.iter().any(|p| **p == selector_consumer_path);
    let shard_file_count = shard_files.len();

    let mut skipped: Vec<String> = Vec::new();
    let mut known_residual_skips: Vec<(String, &'static str, usize)> = Vec::new();
    let mut offenders: Vec<(String, Vec<reify_eval::StaleUndefViolation>)> = Vec::new();
    let mut selector_consumer_result: Option<usize> = None;

    for path in shard_files {
        let display = path.display().to_string();
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {display}: {e}"));

        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        if !errors.is_empty() {
            skipped.push(display);
            continue;
        }

        let mut engine = gate_engine(true);
        engine.eval(&compiled);
        let violations = engine.check_no_stale_undef();

        if *path == selector_consumer_path {
            selector_consumer_result = Some(violations.len());
        }

        if let Some((_, reason)) = KNOWN_RESIDUAL_SKIPS
            .iter()
            .find(|(suffix, _)| display.ends_with(suffix))
        {
            known_residual_skips.push((display, reason, violations.len()));
            continue;
        }

        if !violations.is_empty() {
            offenders.push((display, violations));
        }
    }

    eprintln!(
        "broad_corpus_sweep shard {shard_index}/{CORPUS_SHARD_COUNT}: {} files evaluated, {} skipped (compile errors), {} skipped (known residual)",
        shard_file_count - skipped.len() - known_residual_skips.len(),
        skipped.len(),
        known_residual_skips.len(),
    );
    for s in &skipped {
        eprintln!("  SKIP (compile error): {s}");
    }
    for (f, reason, violation_count) in &known_residual_skips {
        eprintln!("  SKIP (known residual, {violation_count} violation(s)): {f}\n    reason: {reason}");
    }

    assert!(
        offenders.is_empty(),
        "expected zero stale-Undef violations across the corpus; offending file(s):\n{}",
        offenders
            .iter()
            .map(|(f, vs)| {
                let detail = vs
                    .iter()
                    .map(|v| format!("    {:?}: {}", v.cell, v.detail))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("  {f}:\n{detail}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    );

    if selector_consumer_in_shard {
        assert_eq!(
            selector_consumer_result,
            Some(0),
            "geometry_let_selector_consumer.ri must be present, evaluated (not skipped due \
             to a compile error), and produce zero violations — the #4946 R3f-bridge premise"
        );
    }
}

/// One `#[test]` fn per corpus shard — see `CORPUS_SHARD_COUNT`'s doc
/// comment for why the broad sweep is sharded, and `run_corpus_shard` for
/// the per-shard logic. `$idx` must range exactly over `0..CORPUS_SHARD_COUNT`
/// (checked by `corpus_shard_count_matches_generated_tests` below).
macro_rules! corpus_shard_tests {
    ($($name:ident = $idx:literal),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_corpus_shard($idx);
            }
        )+

        /// Every shard index passed to THIS macro invocation, in source
        /// order — derived from the same repetition that generates the
        /// `#[test]` fns above, so deleting a `broad_corpus_sweep_shard_NN`
        /// line here shrinks this array too. This is what lets
        /// `corpus_shard_count_matches_generated_tests` detect a deleted
        /// shard line: comparing two independently-hardcoded literals
        /// cannot (both stay unchanged when a line is removed).
        const GENERATED_SHARD_INDICES: &[usize] = &[$($idx),+];
    };
}

corpus_shard_tests! {
    broad_corpus_sweep_shard_00 = 0,
    broad_corpus_sweep_shard_01 = 1,
    broad_corpus_sweep_shard_02 = 2,
    broad_corpus_sweep_shard_03 = 3,
    broad_corpus_sweep_shard_04 = 4,
    broad_corpus_sweep_shard_05 = 5,
    broad_corpus_sweep_shard_06 = 6,
    broad_corpus_sweep_shard_07 = 7,
    broad_corpus_sweep_shard_08 = 8,
    broad_corpus_sweep_shard_09 = 9,
    broad_corpus_sweep_shard_10 = 10,
    broad_corpus_sweep_shard_11 = 11,
    broad_corpus_sweep_shard_12 = 12,
    broad_corpus_sweep_shard_13 = 13,
    broad_corpus_sweep_shard_14 = 14,
    broad_corpus_sweep_shard_15 = 15,
    broad_corpus_sweep_shard_16 = 16,
    broad_corpus_sweep_shard_17 = 17,
    broad_corpus_sweep_shard_18 = 18,
    broad_corpus_sweep_shard_19 = 19,
    broad_corpus_sweep_shard_20 = 20,
    broad_corpus_sweep_shard_21 = 21,
    broad_corpus_sweep_shard_22 = 22,
    broad_corpus_sweep_shard_23 = 23,
}

/// Drift guard: `corpus_shard_tests!` above must enumerate EXACTLY
/// `0..CORPUS_SHARD_COUNT` — one `#[test]` fn per shard index, no gaps and
/// no out-of-range entries — or some corpus files would silently never be
/// swept (a gap) or `run_corpus_shard` would be invoked with an index that
/// can never match any file (dead weight). Asserted against
/// `GENERATED_SHARD_INDICES` — the array the macro emits FROM THE SAME
/// repetition that generates the shard `#[test]` fns — rather than a
/// separately hand-maintained literal count: deleting a
/// `broad_corpus_sweep_shard_NN` line shrinks `GENERATED_SHARD_INDICES` too,
/// so this guard actually fails when that drift occurs (a literal-vs-literal
/// comparison would not: neither literal changes when a line is deleted).
#[test]
fn corpus_shard_count_matches_generated_tests() {
    assert_eq!(
        GENERATED_SHARD_INDICES.len(), CORPUS_SHARD_COUNT,
        "corpus_shard_tests! generated {} shard test(s) but CORPUS_SHARD_COUNT \
         is {CORPUS_SHARD_COUNT} — every index in 0..CORPUS_SHARD_COUNT must \
         have exactly one broad_corpus_sweep_shard_NN test, or some corpus \
         files silently never get swept",
        GENERATED_SHARD_INDICES.len()
    );

    // Stronger than a count match: pin the exact index SET too, so a
    // duplicate/out-of-range index masking a missing one (same count, wrong
    // coverage) can't slip through.
    let mut sorted_indices = GENERATED_SHARD_INDICES.to_vec();
    sorted_indices.sort_unstable();
    let expected: Vec<usize> = (0..CORPUS_SHARD_COUNT).collect();
    assert_eq!(
        sorted_indices, expected,
        "corpus_shard_tests! must enumerate EXACTLY 0..CORPUS_SHARD_COUNT — no \
         gaps, duplicates, or out-of-range indices — got {:?}",
        GENERATED_SHARD_INDICES
    );
}

#[test]
#[ignore = "diagnostic timing harness; run explicitly with --ignored"]
fn diag_per_file_timing() {
    let (files, _selector_consumer_path) = corpus_files();
    let mut timings: Vec<(std::time::Duration, String)> = Vec::new();
    for path in &files {
        let display = path.display().to_string();
        let t0 = std::time::Instant::now();
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let compiled = reify_test_support::compile_source_with_stdlib(&source);
        let errors = reify_test_support::collect_errors(&compiled.diagnostics);
        if !errors.is_empty() {
            continue;
        }
        let mut engine = gate_engine(true);
        engine.eval(&compiled);
        let _ = engine.check_no_stale_undef();
        timings.push((t0.elapsed(), display));
    }
    timings.sort();
    timings.reverse();
    for (d, f) in timings.iter().take(40) {
        eprintln!("DIAG {d:?} {f}");
    }
}

// ── Task 5578: the build() SURFACE ───────────────────────────────────────────
//
// Everything above drives `engine.eval()`. That leaves half the coverage gap
// open: `build()` runs `eval()` and then the realization/export stages on top,
// and it is the surface a user actually invokes. Task 5578's defect lived
// exactly there — an `@optimized` compute dispatch that silently degraded on a
// build — and was invisible to the eval sweep because the eval sweep's per-file
// engine wiring DOES call `register_compute_fns` while the differential
// harness's did not.
//
// The registration switch is therefore not a convenience knob: it is what makes
// the seeded self-test below able to reproduce the defect in miniature, and what
// makes the real sweep's "no trampoline-missing diagnostic" assertion mean
// something. Same guard shape as task 4458's
// `crates/reify-cli/tests/harness_cli/cli_build_fea.rs`, one layer down.

/// The substring `engine_eval.rs` emits when an `@optimized` annotation names a
/// target with no registered compute trampoline. It does NOT abort the eval:
/// the dispatch pushes this codeless Error diagnostic and then BODY-INLINES the
/// fn, whose stdlib body is a never-run sentinel (for the solver fns, a bare
/// struct ctor with all-required, no-default params). Every downstream field
/// read of that sentinel is Undef while its declared reads read as resolved —
/// i.e. a stale-Undef violation attributed to innocent scheduling code.
const TRAMPOLINE_MISSING: &str = "no registered compute trampoline";

/// A miniature reproduction of the task-5578 defect, used ONLY by the seeded
/// self-test below: the T-prism topology (verified valid — lifted from
/// `examples/tensegrity_t_prism.ri`) reduced to one `@optimized`
/// `form_find_free` call plus one FieldAccess consumer of its result.
///
/// Built on a DELIBERATELY unregistered engine this must produce both halves of
/// the failure signature: the trampoline-missing diagnostic AND a stale-Undef
/// violation on `solved`. Built on a registered engine it must produce neither
/// — which is what `build_surface_probe_is_clean_when_registered` pins, so the
/// self-test can never pass for the wrong reason (e.g. a probe that is stale
/// under BOTH arms).
const OPTIMIZED_PROBE_SRC: &str = r#"structure def SeededOptimizedProbe {
    let prism = Tensegrity(
        nodes: [
            point3(1m, 0m, 1m),
            point3(-0.5m, 0.866m, 1m),
            point3(-0.5m, -0.866m, 1m),
            point3(0.866m, 0.5m, 0m),
            point3(-0.866m, 0.5m, 0m),
            point3(0m, -1m, 0m)
        ],
        struts: [[0, 4], [1, 5], [2, 3]],
        cables: [
            [0, 1], [1, 2], [2, 0],
            [3, 4], [4, 5], [5, 3],
            [0, 3], [1, 4], [2, 5]
        ]
    )
    let group_ids = [0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]
    let seeds = [-1.0, 1.0, 1.0]
    let reference_group = 1
    let form = form_find_free(prism, group_ids, seeds, reference_group)
    let solved = form.nodes
}"#;

/// The mandatory anti-silent-accept self-test for the build()-surface sweep —
/// the exact counterpart of `seeded_stale_undef_violation_is_reported` at the
/// top of this file, whose doc explains why a helper that always returns
/// `vec![]` "would otherwise make every downstream corpus test in this suite
/// vacuously green".
///
/// Rather than fabricating post-eval state, this one reproduces THIS VERY BUG
/// in miniature: run the probe through a real `build()` on an engine with the
/// compute trampolines deliberately NOT registered, and require BOTH halves of
/// the observed failure signature. That pins the exact failure mode task 5578
/// fixes and can never go vacuously green — a helper returning empty vectors
/// fails here immediately.
#[test]
fn seeded_build_surface_sweep_reports_a_planted_violation() {
    let observed = build_surface_violations(OPTIMIZED_PROBE_SRC, false);
    let (violations, diagnostics) = (&observed.violations, &observed.diagnostics);

    assert!(
        diagnostics.iter().any(|d| d.contains(TRAMPOLINE_MISSING)),
        "building the @optimized probe on an UNregistered engine must emit the \
         {TRAMPOLINE_MISSING:?} diagnostic — without it the probe is not \
         exercising the @optimized dispatch at all and this self-test proves \
         nothing. got diagnostics: {diagnostics:#?}"
    );

    let solved = ValueCellId::new("SeededOptimizedProbe", "solved");
    assert!(
        violations.iter().any(|v| v.cell == solved),
        "building the @optimized probe on an UNregistered engine must leave \
         `SeededOptimizedProbe.solved` stale-Undef (the body-inlined \
         `FormFindResult()` sentinel has all-required params, so every field \
         read of it folds to Undef while its one declared read resolves) — a \
         `build_surface_violations` that never reports would make the sweep \
         below vacuously green. got {:?}",
        violations.iter().map(|v| &v.cell).collect::<Vec<_>>()
    );
}

/// What one build()-surface probe observed. Returned as a named struct rather
/// than a tuple because the third field is the POSITIVE half of the sweep's
/// contract and a bare `Vec<String>` in tuple position 3 would read as noise.
struct BuildSurfaceOutcome {
    /// `check_no_stale_undef` over the post-`build()` engine state.
    violations: Vec<reify_eval::StaleUndefViolation>,
    /// Every `BuildResult` diagnostic message, in emission order.
    diagnostics: Vec<String>,
    /// Every ComputeNode target present in the post-build evaluation graph,
    /// sorted + deduped — i.e. what the module actually DISPATCHED. This is
    /// observed from the graph rather than inferred from the source text, so it
    /// also sees engine-INSERTED nodes that no stdlib call names (notably
    /// `shell-extract::extract`, which `elastic_static` wires upstream of itself
    /// for a shell element and which appears in no `.ri` file at all).
    dispatched_targets: Vec<String>,
}

/// Compile `source` through the stdlib prelude, run it through a real
/// `Engine::build(.., ExportFormat::Step)`, and report what that build did.
///
/// Engine wiring is [`gate_engine`]'s — the same constructor the eval sweep
/// above uses — with the terminal `engine.eval(&compiled)` swapped for
/// `engine.build(..)`. That swap is the whole point: it is precisely the surface
/// half of the coverage gap task 5578 names, since every sweep above this line
/// only ever drives `eval()`.
///
/// `register_compute` is forwarded to [`gate_engine`] verbatim; see its doc for
/// why `false` exists and who may pass it.
///
/// A source that fails to COMPILE panics rather than being skipped. That is
/// deliberate and differs from `run_corpus_shard`'s printed compile-error skip:
/// this helper only ever runs over an explicit, curated file list, so a compile
/// error there means the LIST is stale — a defect to surface loudly, not a file
/// to quietly drop.
fn build_surface_violations(source: &str, register_compute: bool) -> BuildSurfaceOutcome {
    let compiled = reify_test_support::compile_source_with_stdlib(source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "build_surface_violations: the source must compile without errors — a \
         compile error here means the caller's curated file list is stale: {errors:#?}"
    );

    let mut engine = gate_engine(register_compute);
    let result = engine.build(&compiled, reify_ir::ExportFormat::Step);
    let diagnostics: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();

    // `eval_state()` is populated by the cold `eval()` inside `build()`, and the
    // realization loop mutates only node `produced_*` fields — never graph
    // topology — so the post-build graph carries every ComputeNode the build
    // planned. (Same soundness argument the differential harness's `residue_for`
    // makes for re-reading the post-build graph.)
    let mut dispatched_targets: Vec<String> = engine
        .eval_state()
        .map(|state| {
            state
                .snapshot
                .graph
                .compute_nodes
                .iter()
                .map(|(_, node)| node.target.clone())
                .collect()
        })
        .unwrap_or_default();
    dispatched_targets.sort();
    dispatched_targets.dedup();

    BuildSurfaceOutcome {
        violations: engine.check_no_stale_undef(),
        diagnostics,
        dispatched_targets,
    }
}

/// Non-vacuity companion to `seeded_build_surface_sweep_reports_a_planted_violation`:
/// the SAME probe, same helper, with the trampolines REGISTERED, must produce
/// neither half of the failure signature.
///
/// Without this, the seeded self-test could pass for the wrong reason — a probe
/// that is stale-Undef under BOTH arms (say, a typo'd field name) would satisfy
/// it while proving nothing about the registration switch. Pinning the two arms
/// to OPPOSITE outcomes is what makes the switch, and therefore the sweep below,
/// meaningful.
#[test]
fn build_surface_probe_is_clean_when_registered() {
    let observed = build_surface_violations(OPTIMIZED_PROBE_SRC, true);
    let (violations, diagnostics) = (&observed.violations, &observed.diagnostics);

    assert!(
        !diagnostics.iter().any(|d| d.contains(TRAMPOLINE_MISSING)),
        "with the trampolines registered the probe must NOT emit \
         {TRAMPOLINE_MISSING:?}: {diagnostics:#?}"
    );
    assert!(
        violations.is_empty(),
        "with the trampolines registered the probe must have zero stale-Undef \
         violations — if `solved` is still reported, the registration switch is \
         not the variable this pair of tests thinks it is: {violations:?}"
    );
    // The same positive dispatch pin the real sweep applies per file: prove the
    // probe still REACHES the @optimized path. Without it, a probe edited into
    // no longer calling `form_find_free` would satisfy both absence-assertions
    // above while testing nothing.
    assert!(
        observed
            .dispatched_targets
            .iter()
            .any(|t| t == "solver::form_find_free"),
        "the probe must dispatch a `solver::form_find_free` ComputeNode — \
         without it neither arm of this pair exercises the @optimized path. \
         dispatched: {:?}",
        observed.dispatched_targets
    );
}

// ── The bounded build()-surface sweep ────────────────────────────────────────
//
// Selection method, stated so bounded coverage never reads as full coverage:
//
//   1. Find every `examples/**/*.ri` that CALLS an `@optimized`-annotated stdlib
//      fn — RECURSIVELY. `examples/` holds 147 top-level `.ri` files but 260 in
//      all, and 13 of the callers live in subdirectories (dynamics/, flexures/,
//      modal/, shells/, trajectory/), so a top-level-only grep sees barely half
//      the surface. 31 caller files today, between them reaching 15 of the
//      stdlib's 16 `@optimized` targets. The fn names are NOT listed here: they
//      are read from the compiled stdlib at test time (see step 5), so this
//      comment cannot fall behind a newly-annotated fn.
//   2. Reduce to a DISTINCT-TARGET covering set — one file per `@optimized`
//      dispatch target, choosing the cheapest measured file for each. That is
//      the cost trim the sweep's own purpose licenses: it guards the dispatch
//      path per TARGET, so a second file hitting an already-covered target buys
//      no new coverage. Every dropped file is listed with its measured cost and
//      the target it duplicates in `BUILD_SURFACE_DROPPED_DUPLICATES`.
//   3. Where a target's ONLY example is disproportionately expensive, substitute
//      a hand-written minimal probe for it — but only because assertion (3) in
//      `run_build_surface_sweep` proves the probe still dispatches that exact
//      target, so a substitution can never quietly become zero coverage. Two
//      targets needed this (`solver::buckling_multi_case`, `trajectory::simulate`).
//   4. All four lists, and which `#[test]` runs which case, are PRINTED once by
//      `build_surface_selection_is_reported_and_consistent` — deliberately in
//      ONE place rather than by each sweep, so no run ever reports a covering
//      set larger than the cases it actually built.
//   5. Step 1 is EXECUTABLE, not a hand-run grep. `build_surface_survey_is_exhaustive`
//      re-derives the fn -> target map from the COMPILED stdlib and re-walks
//      `examples/**` on every run, then fails if a caller file is in neither the
//      covering set nor `BUILD_SURFACE_DROPPED_DUPLICATES`, or if a stdlib target
//      is neither credited by a covered case nor listed in
//      `BUILD_SURFACE_UNCOVERED_TARGETS`. It exists because these lists WERE
//      derived by a hand-run, non-recursive grep, and the conclusions written on
//      top of it rotted into false prose right here (task 5578 review round 2):
//      13 caller files never surveyed, and 8 targets declared to have no example
//      caller when only one of them actually had none. Prose cannot fail; that
//      test can.
//
// Measured (debug profile, `--test-threads=1`) the full 31-file list costs
// 532.1s — 136.63s for the 18 top-level files plus 395.5s for the 13
// subdirectory callers the original grep never saw. The covering set as selected
// costs 26.4s end-to-end at `--test-threads=1`: 11.54s for the nine cheap files,
// 10.08s for the buckling-multi-case probe (standing in for a 78.35s example) and
// 4.73s for the trajectory-simulate probe (standing in for a 303.26s example).
// Each probe keeps its own `#[test]` so it schedules as an independent process
// with its own PASS/SLOW line — the same reasoning that sharded the eval sweep
// into `CORPUS_SHARD_COUNT`. Per-case costs below are single-build measurements
// and drift with host contention (the trajectory probe measured 4.7-6.2s across
// runs); they are recorded to justify a RANKING — cheapest caller per target —
// not as a budget anything asserts against.
//
// Exactly one target is genuinely uncovered here — `fdm::slice`, the only one of
// the 16 that no `examples/**/*.ri` calls. It is listed with its reason, and with
// where it IS guarded instead, in `BUILD_SURFACE_UNCOVERED_TARGETS`; the survey
// requires every target the covering set does not credit to be accounted for
// there, so "uncovered" is now a written, checked disposition rather than an
// omission.

/// One member of the bounded build()-surface sweep.
struct BuildSurfaceCase {
    /// Base name of the `examples/<name>.ri` file this case builds — also the
    /// display label and the `BUILD_SURFACE_KNOWN_RESIDUALS` key.
    ///
    /// May be subdirectory-relative (`modal/transient_step_response`): that name
    /// flows unchanged through `examples_dir.join(format!("{name}.ri"))`, the
    /// printed label, the DROP printer, the residual key and the survey's
    /// `examples/`-relative comparison, so a subdir caller needs no special case.
    name: &'static str,
    /// The ComputeNode target ids this file is CREDITED with covering.
    ///
    /// Structured rather than prose because the sweep ASSERTS each one is
    /// actually present in the post-build graph. Without that positive check
    /// both per-file assertions are absence-assertions, and a file later edited
    /// to drop its solver call (or an engine change that stops inserting an
    /// upstream node) would keep the sweep green while silently losing that
    /// target's coverage — bounded coverage quietly reading as full coverage,
    /// the exact failure this section's selection method exists to prevent.
    targets: &'static [&'static str],
    /// Why this file is in the covering set: measured `build()` cost plus what
    /// it uniquely contributes.
    why: &'static str,
    /// `None` — build `examples/<name>.ri`, the default and the honest one.
    ///
    /// `Some(src)` — build this hand-written minimal source instead, for a
    /// target whose only real example is disproportionately expensive. Allowed
    /// ONLY because assertion (3) proves the substitute still dispatches the
    /// credited target; a probe that drifted into not dispatching fails rather
    /// than silently covering nothing. `name` then labels the probe, not a file.
    probe: Option<&'static str>,
}

/// The distinct-target covering set, minus the one heavy member below. See the
/// section comment for how it was derived and what was dropped.
const BUILD_SURFACE_OPTIMIZED_EXAMPLES: &[BuildSurfaceCase] = &[
    BuildSurfaceCase {
        name: "fea_shell_too_thick_annotated",
        targets: &["solver::elastic_static", "shell-extract::extract"],
        why: "15ms — the cheapest elastic-static caller, and the ONLY example \
              that reaches shell-extract (which no .ri file names: the engine \
              inserts it upstream of a shell elastic_static solve)",
        probe: None,
    },
    BuildSurfaceCase {
        name: "fdm_bracket",
        targets: &["fdm::as_printed_material_r_fast"],
        why: "204ms — the only caller of as_printed_material",
        probe: None,
    },
    BuildSurfaceCase {
        name: "fea_multi_case_bracket",
        targets: &["solver::multi_case"],
        why: "1.10s — cheapest solve_load_cases caller",
        probe: None,
    },
    BuildSurfaceCase {
        name: "buckling_column_p2",
        targets: &["solver::buckling"],
        why: "6.19s — cheapest solve_buckling caller (P2 at a coarse \
              cross-section; buckling_column_smoke is the same target at 43.96s)",
        probe: None,
    },
    BuildSurfaceCase {
        name: "tensegrity_cable_net",
        targets: &["solver::form_find"],
        why: "9.8ms — cheapest form_find caller",
        probe: None,
    },
    BuildSurfaceCase {
        name: "tensegrity_pavilion",
        targets: &["solver::form_find_free", "solver::membrane_load"],
        why: "48.5ms — the only membrane_load caller, and a form_find_free \
              caller in the same file",
        probe: None,
    },
    BuildSurfaceCase {
        name: "flexures/printer_z_compliant_mount",
        targets: &["dynamics::inverse_dynamics", "modal::mechanism_modal"],
        why: "17.1ms — cheapest inverse_dynamics caller of four AND the only \
              mechanism_modal caller, so two targets for less than the \
              elastic-static case costs",
        probe: None,
    },
    BuildSurfaceCase {
        name: "modal/transient_step_response",
        targets: &[
            "modal::free_vibration",
            "modal::transient_response",
            "modal::displacement_at",
        ],
        why: "3.20s — three targets in one file, and the cheapest free_vibration \
              caller of five: it solves at ElementOrder.P1, where \
              cantilever_beam_modes (37.93s) and simply_supported_beam_modes \
              (36.06s) are the same target at P2",
        probe: None,
    },
    BuildSurfaceCase {
        name: "trajectory/zvd_robustness",
        targets: &["trajectory::input_shape"],
        why: "12.8ms — cheapest input_shape caller of four",
        probe: None,
    },
];

/// A hand-written minimal `solver::buckling_multi_case` driver — the same
/// two-load-case shape as `examples/buckling_multi_case_smoke.ri` on a SHORTER
/// column, which is what the cost is dominated by (mesh element count along the
/// span, then the eigensolve on it).
///
/// `worst_buckling_case` consumes the result, so a degraded dispatch still
/// surfaces as a stale-Undef violation here exactly as it would on the real
/// example. Argument binding is POSITIONAL (`name:` labels are cosmetic), so
/// `BucklingOptions(n_modes: 1)` binds the FIRST declared param and leaves the
/// rest at their defaults — see examples/buckling_column_p2.ri's header.
const BUCKLING_MULTI_CASE_PROBE_SRC: &str = r#"structure BucklingMultiCaseProbe {
    param length : Length = 200mm
    param width  : Length = 20mm
    param height : Length = 20mm

    let material = Steel_AISI_1045()

    let lc1 = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "top", force: 1000.0)],
        supports: [FixedSupport(target: "base")],
    )
    let lc2 = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "top", force: 2000.0)],
        supports: [FixedSupport(target: "base")],
    )

    let mcbr = solve_buckling_load_cases(
        material, length, width, height, [lc1, lc2], BucklingOptions(n_modes: 1)
    )
    let worst = worst_buckling_case(mcbr)
}"#;

/// The single covering-set member heavy enough to warrant its own `#[test]`.
const BUILD_SURFACE_HEAVY_EXAMPLE: &[BuildSurfaceCase] = &[BuildSurfaceCase {
    name: "buckling_multi_case_probe",
    targets: &["solver::buckling_multi_case"],
    why: "6.98s measured, vs 78.35s for examples/buckling_multi_case_smoke.ri — \
          the only example that reaches this target, and 9x the whole rest of \
          this sweep for that one target. The probe is the same two-load-case \
          shape on a 200mm rather than 800mm column; assertion (3) proves it \
          still dispatches solver::buckling_multi_case, so the substitution \
          cannot silently become zero coverage. The trampoline itself is \
          separately covered by \
          crates/reify-eval-fea-tests/tests/buckling_multi_case.rs; what this \
          case adds is only the build()-surface half — no stale Undef and no \
          trampoline-missing diagnostic — which is exactly what the probe \
          exercises. Still its own #[test] so it schedules concurrently with \
          the sweep above rather than adding ~7s to it, with its BUILD line \
          printed BEFORE the build so the window is never a silent gap.",
    probe: Some(BUCKLING_MULTI_CASE_PROBE_SRC),
}];

/// A hand-written minimal `trajectory::simulate` driver.
///
/// `simulate_trajectory(p: Profile, mech: Real, modal: ModalResult)` cannot be
/// called without a real `ModalResult`, and `ModalResult`'s six params are all
/// required — hand-constructing one would be the all-Undef sentinel shape this
/// whole file exists to catch. So the probe pays for one genuine modal solve,
/// made as cheap as the API allows: the 200x10x2 mm P1 cantilever from
/// `examples/modal/transient_step_response.ri` at `n_modes: 1` with `NoDamping`.
/// The profile is the two-waypoint cubic ramp from
/// `examples/trajectory/zvd_robustness.ri` (12.8ms on its own).
///
/// `let samples = track.t_samples` is the consumer, and is load-bearing: it is
/// the exact FieldAccess-on-a-solver-result shape that made `TPrism.solved` go
/// stale in task 5578, so a degraded dispatch surfaces here as a stale-Undef
/// violation rather than as a quietly-empty track.
const TRAJECTORY_SIMULATE_PROBE_SRC: &str = r#"structure TrajectorySimulateProbe {
    param length : Length = 200mm
    param width  : Length = 10mm
    param height : Length = 2mm

    let material = Steel_AISI_1045()
    let mi = FEAMaterialInput(material: material)
    let root = FixedSupport(target: "x_min")

    let modal_opts = ModalOptions(
        n_modes: 1,
        boundary_conditions: [root],
        damping: NoDamping(),
        sigma: 0.0,
        tol: 0.000000001,
        max_iters: 200,
        reference_direction: vec3(0.0, 0.0, 1.0),
        element_order: ElementOrder.P1
    )
    let modal = modal_analysis(mi.material, length, width, height, modal_opts)

    let wp0 = Waypoint(t: 0.0s, values: [0.0], vels: none, accels: none)
    let wp1 = Waypoint(t: 1.0s, values: [1.0], vels: none, accels: none)
    let profile = PiecewisePolynomialProfile(
        mechanism: 1.0,
        waypoints: [wp0, wp1],
        boundary: NaturalSpline(),
        spline_kind: SplineKind.CubicSpline
    )

    let track = simulate_trajectory(profile, 1.0, modal)
    let samples = track.t_samples
}"#;

/// The second heavy covering-set member, in its own `#[test]` for the same
/// reason as the first.
const BUILD_SURFACE_HEAVY_TRAJECTORY: &[BuildSurfaceCase] = &[BuildSurfaceCase {
    name: "trajectory_simulate_probe",
    targets: &["trajectory::simulate"],
    why: "~6s measured (4.7-6.2s across runs), vs 303.26s for \
          examples/trajectory/printer_print_envelope.ri — the only example that \
          reaches this target, and on its own 12x the whole \
          rest of this sweep. That example pays for a 7-mode P2 modal solve on a \
          500mm gantry and then simulates three shaped variants of a four-waypoint \
          path; the probe pays for a 1-mode P1 solve on a 200mm beam and one \
          two-waypoint ramp. Assertion (3) proves it still dispatches \
          trajectory::simulate, so the substitution cannot silently become zero \
          coverage. It also dispatches modal::free_vibration as scaffolding (a \
          ModalResult is a required argument) but is NOT credited with it — that \
          target is covered by modal/transient_step_response, and crediting \
          incidental dispatch would overstate what this case is chosen for. The \
          trajectory solver itself is covered by reify-stdlib's own trajectory \
          tests; what this case adds is the build()-surface half — no stale Undef \
          and no trampoline-missing diagnostic.",
    probe: Some(TRAJECTORY_SIMULATE_PROBE_SRC),
}];

/// Candidate files dropped from the sweep because they duplicate a target the
/// covering set already reaches. PRINTED on every run by
/// `build_surface_selection_is_reported_and_consistent` — never a silent
/// truncation. Costs are the measured debug-profile `build()` wall clock.
const BUILD_SURFACE_DROPPED_DUPLICATES: &[(&str, &str)] = &[
    ("anisotropic_bar", "3.12s — solver::elastic_static, covered by fea_shell_too_thick_annotated (15ms)"),
    ("buckling_column_smoke", "43.96s — solver::buckling, covered by buckling_column_p2 (6.19s)"),
    (
        "buckling_multi_case_smoke",
        "78.35s — solver::buckling_multi_case, covered by the hand-written \
         BUCKLING_MULTI_CASE_PROBE_SRC at 6.98s (11x cheaper, same target, \
         positively asserted). The ONLY entry in this list dropped in favour of \
         a probe rather than another example: every other target has a cheap \
         real caller, this one did not.",
    ),
    ("differential_field_ops", "349ms — solver::elastic_static"),
    (
        "dynamics/closed_2prismatic_idyn",
        "490ms — dynamics::inverse_dynamics, covered by \
         flexures/printer_z_compliant_mount (17.1ms)",
    ),
    (
        "dynamics/closed_4bar_idyn",
        "38.1ms — dynamics::inverse_dynamics, covered by \
         flexures/printer_z_compliant_mount (17.1ms)",
    ),
    (
        "dynamics/toolhead_motor_sizing",
        "25.7ms — dynamics::inverse_dynamics, covered by \
         flexures/printer_z_compliant_mount (17.1ms)",
    ),
    (
        "fea_bracket_minimize_mass",
        "5.19ms — solver::elastic_static, covered by fea_shell_too_thick_annotated \
         (15ms). The ONE entry here that is NOT a cost drop, and the one place the \
         \"cheapest measured file wins\" rule in the selection method above is \
         deliberately NOT applied: this file is ~3x CHEAPER than the member it \
         defers to, and it is cheap for exactly the reason that disqualifies it. \
         Its `solve_elastic_static` call is INLINED in a `constraint` expression \
         rather than bound to a `let` (see the example's own comment for why that \
         is forced), and `gate_engine` wires NO constraint solver — so the cost \
         loop that would evaluate that expression never runs on this surface. \
         MEASURED by building it through `build_surface_violations`: \
         `dispatched_targets` comes back EMPTY, against \
         [\"shell-extract::extract\", \"solver::elastic_static\"] for the covering \
         member. Promoting it would fail assertion (3) outright — credited with a \
         target it reaches zero times — so its 4ms buys no coverage at all. It \
         would need no BUILD_SURFACE_KNOWN_RESIDUALS entry either: measured \
         violations come back empty too. The eval surface, where the FEA loop does \
         run and does converge, is where this example is pinned instead: \
         crates/reify-eval/tests/harness_fea_solver_e2e/fea_bracket_minimize_mass_e2e.rs.",
    ),
    ("fea_cantilever_smoke", "410ms — solver::elastic_static"),
    ("fea_multi_case_smoke", "294ms — solver::elastic_static"),
    ("fea_pressure_smoke", "286ms — solver::elastic_static"),
    ("fea_shell_flexure", "138ms — solver::elastic_static"),
    ("fea_shell_too_thick_auto", "41ms — solver::elastic_static"),
    (
        "modal/cantilever_beam_modes",
        "37.93s — modal::free_vibration at ElementOrder.P2, covered by \
         modal/transient_step_response (3.20s, P1). The P2 accuracy this fixture \
         exists for is an eval-surface property its own tests pin; the dispatch \
         path is identical.",
    ),
    (
        "modal/printer_gantry_modes",
        "3.99s — modal::free_vibration, covered by modal/transient_step_response \
         (3.20s)",
    ),
    (
        "modal/simply_supported_beam_modes",
        "36.06s — modal::free_vibration at ElementOrder.P2, covered by \
         modal/transient_step_response (3.20s, P1)",
    ),
    (
        "multi_load_bracket",
        "2.06s — solver::multi_case, covered by fea_multi_case_bracket (1.10s). \
         Also carries a KNOWN_RESIDUAL_SKIPS entry on the eval surface \
         (MultiLoadBracket.critical_case, a worst_case/lambda-dispatch product \
         limitation), so importing it here would add a known-broken case for zero \
         added target coverage.",
    ),
    (
        "shells/thin_walled_bracket",
        "695ms — solver::elastic_static (plus the engine-inserted \
         shell-extract::extract), both covered by fea_shell_too_thick_annotated \
         (15ms). This is the file the task-5578 review found in NEITHER list: the \
         non-recursive grep never saw it, so the DROP list's own \
         \"every dropped file is listed\" promise was false until the survey \
         started walking examples/ recursively.",
    ),
    ("tensegrity_membrane_formfind", "13ms — solver::form_find, covered by tensegrity_cable_net (9.8ms)"),
    (
        "tensegrity_t_prism",
        "45ms — solver::form_find_free, covered by tensegrity_pavilion (48.5ms). \
         This is task 5578's own reproducer, and it is pinned STRICTLY harder \
         elsewhere: harness_engine's \
         `optimized_compute_outputs_are_definite_on_build_surface` builds it under \
         BOTH schedulers and asserts TPrism.{solved,forces} are DEFINITE, which a \
         zero-violations sweep cannot.",
    ),
    (
        "trajectory/ei_robustness",
        "70.4ms — trajectory::input_shape, covered by trajectory/zvd_robustness \
         (12.8ms)",
    ),
    (
        "trajectory/printer_print_envelope",
        "303.26s — modal::free_vibration + trajectory::input_shape + \
         trajectory::simulate, all three covered elsewhere (3.20s / 12.8ms / the \
         ~6s TRAJECTORY_SIMULATE_PROBE_SRC). It is the ONLY example reaching \
         trajectory::simulate, and at 12x the rest of the sweep for that one \
         target it is the second entry in this list dropped in favour of a probe. \
         Its cost is the 7-mode P2 modal solve on a 500mm gantry plus three \
         simulated variants — none of which the dispatch assertions need.",
    ),
    (
        "trajectory/tots_optimal_ptp",
        "9.71s — trajectory::input_shape (the heavy TOTS arm), covered by \
         trajectory/zvd_robustness (12.8ms). The TOTS solve itself is a \
         trajectory-crate concern; this sweep guards the dispatch.",
    ),
];

/// Build-surface counterpart of `KNOWN_RESIDUAL_SKIPS` — same convention
/// (reasoned per entry, matched exactly, PRINTED never silent), but keyed
/// `(example base name, EXACT cell rendered as "Entity.member", reason)` rather
/// than by file. Cell-level rather than file-level is a deliberate tightening:
/// a blanket file skip would also suppress a NEW violation in that file and the
/// trampoline-missing assertion, both of which still apply here.
///
/// Every entry's file MUST still be swept — `build_surface_selection_is_reported_and_consistent`
/// fails on an orphan, so an entry cannot rot into a permanent, never-exercised
/// exemption by having its file quietly dropped from the covering set.
const BUILD_SURFACE_KNOWN_RESIDUALS: &[(&str, &str, &str)] = &[(
    "fdm_bracket",
    "FdmBracket.defl_print",
    "`let defl_print = max(r_print.displacement)` where `r_print` comes from the \
     6-arg Field overload of `solve_elastic_static` (per-element \
     DiscreteCellField dispatch, task #4757). Its sibling `defl_solid` — the \
     7-arg ConstitutiveLaw overload — resolves fine, and the file emits ZERO \
     trampoline-missing diagnostics, so the @optimized dispatch itself ran: this \
     is a Field-overload/`max`-reduction product limitation on the build surface \
     specifically (the eval-surface sweep above is green on this same file). \
     Pre-existing and unrelated to task 5578; filed as a follow-up (fused-memory \
     ticket tkt_0RSNHY2F9Y7C7SEVJ04ZEAECNG, escalation_id agent-followup-5578) \
     rather than fixed here.",
)];

/// Stdlib `@optimized` targets that NO build()-surface case covers.
///
/// The third and last disposition a target may have. `build_surface_survey_is_exhaustive`
/// requires every target in the compiled stdlib to be either credited by a case
/// in the covering set or listed here — there is no fourth, silent option, which
/// is the whole point of the const existing rather than the fact living in prose.
///
/// Each reason must meet the SAME standard a `BUILD_SURFACE_DROPPED_DUPLICATES`
/// entry meets: a MEASURED `build()` cost, or a re-verifiable fact about the
/// tree plus where the target IS guarded instead. Never a bare "unreachable" —
/// that claim is exactly what rotted here once already (task 5578 review round
/// 2: the section comment asserted eight targets had no `examples/*.ri` caller;
/// seven of them did, and the claim had only ever been checked by a
/// NON-recursive grep).
const BUILD_SURFACE_UNCOVERED_TARGETS: &[(&str, &str)] = &[(
    "fdm::slice",
    "no `examples/**/*.ri` calls `fdm_slice(` — the ONLY one of the stdlib's 16 \
     @optimized targets for which that is true, and the only survivor of the \
     eight targets the section comment once claimed had no example caller. Not a \
     cost drop: there is nothing here to admit, and `build_surface_survey_is_exhaustive` \
     re-checks it recursively on every run (printing this target's caller list, so \
     the entry stops excusing anything the moment an example calls it). The target \
     IS guarded on THIS surface by crates/reify-eval/tests/fdm_slice_e2e.rs, which \
     registers the compute fns, runs a real `engine.build(.., ExportFormat::Step)` \
     and asserts a ComputeNode with `target == \"fdm::slice\"` is in the post-build \
     graph — the same positive dispatch check assertion (3) makes here.",
)];

/// Every case the build()-surface sweeps actually build, paired with the
/// `#[test]` that builds it.
///
/// The covering set is split across three `#[test]`s — one sweep over the cheap
/// members plus one per heavy probe, so each heavy member schedules as its own
/// process with its own PASS/SLOW line. This is the ONE place that knows the
/// split: the selection report prints from it and `build_surface_survey_is_exhaustive`
/// checks coverage against it, so adding a fourth slice cannot leave either of
/// them silently reasoning about a subset — which is exactly the rot shape this
/// section is recovering from.
fn build_surface_cases() -> Vec<(&'static BuildSurfaceCase, &'static str)> {
    BUILD_SURFACE_OPTIMIZED_EXAMPLES
        .iter()
        .map(|c| (c, "build_surface_optimized_examples_have_no_stale_undef"))
        .chain(
            BUILD_SURFACE_HEAVY_EXAMPLE
                .iter()
                .map(|c| (c, "build_surface_buckling_multi_case_has_no_stale_undef")),
        )
        .chain(
            BUILD_SURFACE_HEAVY_TRAJECTORY
                .iter()
                .map(|c| (c, "build_surface_trajectory_simulate_has_no_stale_undef")),
        )
        .collect()
}

/// Every `@optimized` stdlib fn paired with the ComputeNode target its
/// annotation names, derived AT TEST TIME from the COMPILED stdlib.
///
/// `prelude_backed_functions` merges the stdlib `.ri` function table exactly as
/// `Engine::with_prelude_and_kernels` does, and `CompiledFunction::optimized_target`
/// is populated by `compile_function` straight from the annotation
/// (`crates/reify-ir/src/expr.rs`) — so this map is authoritative and
/// self-updating: a new `@optimized` stdlib fn appears here the moment it lands,
/// and mechanically fails `build_surface_survey_is_exhaustive` until it is
/// covered, dropped or explicitly listed uncovered.
///
/// Deliberately NOT a literal list and NOT a text-parse of
/// `crates/reify-compiler/stdlib/*.ri`. Either would be a second copy of the
/// grammar that can drift from the compiler — which is precisely the rot that
/// produced this test (the selection lists above came from a hand-run grep whose
/// conclusions then went stale in prose).
///
/// Overloads collapse to one pair each (`displacement_at` x2,
/// `solve_elastic_static` x3, `solve_load_cases` x2 all name a single target),
/// so the result is sorted + deduped. 16 distinct targets today.
fn stdlib_optimized_fns() -> Vec<(String, String)> {
    let module = reify_test_support::compile_source_with_stdlib("structure def SurveyProbe {}");
    let mut pairs: Vec<(String, String)> = reify_test_support::prelude_backed_functions(&module)
        .into_iter()
        .filter_map(|f| f.optimized_target.map(|target| (f.name, target)))
        .collect();
    pairs.sort();
    pairs.dedup();
    pairs
}

/// Does `source` CALL `fn_name`?
///
/// `//` line comments are stripped FIRST, so a fn named in a header comment (the
/// stdlib names its own `@optimized` fns in prose constantly, and examples cite
/// them too) is not a false positive. A surviving occurrence counts only when
/// the preceding character is not alphanumeric-or-underscore AND the next
/// non-whitespace character is `(`.
///
/// The `(`-requirement is load-bearing in both directions: it is what stops
/// `form_find` from matching the CALL `form_find_free(`, and what keeps a bare
/// mention (a struct field, a string) from counting as a call. The
/// preceding-character check is what stops `worst_buckling_case` from being read
/// as a `solve_buckling` call.
///
/// Comments are stripped per line but the scan runs over the JOINED text, so a
/// call whose `(` sits on the next line still matches.
///
/// Hand-rolled rather than regex-based: reify-eval has no `regex` dev-dependency
/// and this one predicate does not justify adding one.
fn source_calls_fn(source: &str, fn_name: &str) -> bool {
    let code = source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let mut from = 0usize;
    while let Some(rel) = code[from..].find(fn_name) {
        let start = from + rel;
        let end = start + fn_name.len();
        from = start + 1;

        if let Some(prev) = code[..start].chars().next_back()
            && (prev.is_alphanumeric() || prev == '_')
        {
            continue;
        }
        if code[end..].trim_start().starts_with('(') {
            return true;
        }
    }
    false
}

/// What the build()-surface selection survey sees when it is re-run against the
/// two EXTERNAL facts the lists above are supposed to match: the `examples/**`
/// tree and the compiled stdlib.
///
/// Computed, never stated. Every count the section comment and the printed
/// banner quote comes from here, so no number in this file can rot into false
/// prose the way "that yields 18 candidate files" did.
struct OptimizedCallerSurvey {
    /// How many `.ri` files were walked under `examples/` — recursively, via the
    /// same `collect_ri_files` the eval sweep uses, so the two surfaces cannot
    /// disagree about which files exist.
    files_scanned: usize,
    /// `(examples/-relative extension-stripped name, targets it calls)` for every
    /// caller file, sorted by name. The name shape matches `BuildSurfaceCase::name`
    /// and the DROP list exactly, subdirectories included.
    callers: Vec<(String, Vec<String>)>,
    /// Distinct `@optimized` targets in the compiled stdlib, sorted.
    stdlib_targets: Vec<String>,
}

/// Runs the survey. Cheap enough to call from more than one `#[test]`: one
/// stdlib compile plus a 260-file read.
fn survey_optimized_callers() -> OptimizedCallerSurvey {
    let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    let stdlib_fns = stdlib_optimized_fns();
    assert!(
        !stdlib_fns.is_empty(),
        "the compiled stdlib reported ZERO @optimized functions — the map this \
         survey is built on came up empty, so every check keyed off it would pass \
         vacuously. Either `prelude_backed_functions` stopped merging the stdlib \
         or `optimized_target` stopped being populated."
    );
    let mut stdlib_targets: Vec<String> = stdlib_fns.iter().map(|(_, t)| t.clone()).collect();
    stdlib_targets.sort();
    stdlib_targets.dedup();

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_ri_files(&examples_dir, &mut files);
    files.sort();

    let mut callers: Vec<(String, Vec<String>)> = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut targets: Vec<String> = stdlib_fns
            .iter()
            .filter(|(name, _)| source_calls_fn(&source, name))
            .map(|(_, target)| target.clone())
            .collect();
        targets.sort();
        targets.dedup();
        if targets.is_empty() {
            continue;
        }
        let rel = path
            .strip_prefix(&examples_dir)
            .unwrap_or(path)
            .with_extension("");
        callers.push((rel.to_string_lossy().into_owned(), targets));
    }

    OptimizedCallerSurvey {
        files_scanned: files.len(),
        callers,
        stdlib_targets,
    }
}

/// Reports the SELECTION once, and asserts it is internally consistent.
///
/// Separated from the sweeps deliberately (task 5578 review): a per-sweep banner
/// printed "N of the 18 @optimized-bearing files, reduced to a covering set"
/// followed by every DROP line, which was wrong for the heavy sweep — it built
/// one file and reported the whole selection. The selection is one fact about
/// the suite, so it is printed by one test; each sweep prints only the files it
/// actually builds.
///
/// The assertions keep the four lists from contradicting each other:
///
/// - every `BUILD_SURFACE_KNOWN_RESIDUALS` entry names a file that is actually
///   swept, so a residual cannot rot into a never-exercised exemption by having
///   its file dropped from the covering set;
/// - no file sits in two buckets at once (covered AND dropped);
/// - no target sits in two buckets at once (credited AND listed uncovered);
/// - every case credits at least one target, since the targets are what the
///   per-file positive assertion keys off.
///
/// Contrast `build_surface_survey_is_exhaustive`, which checks the same lists
/// against the TREE and the STDLIB. Failures here are fixed by editing a list;
/// failures there are fixed by measuring a candidate and then admitting or
/// dropping it.
#[test]
fn build_surface_selection_is_reported_and_consistent() {
    let cases = build_surface_cases();
    let covered: Vec<&BuildSurfaceCase> = cases.iter().map(|(c, _)| *c).collect();
    let survey = survey_optimized_callers();

    let probes = covered.iter().filter(|c| c.probe.is_some()).count();
    eprintln!(
        "build()-surface coverage is BOUNDED: the {} @optimized-bearing examples/ \
         file(s) — counted RECURSIVELY at run time by the same survey \
         `build_surface_survey_is_exhaustive` enforces, never a hardcoded number — \
         reduced to a distinct-target covering set of {} case(s): {} example file(s) \
         + {probes} hand-written probe(s).",
        survey.callers.len(),
        covered.len(),
        covered.len() - probes
    );
    for (case, test) in &cases {
        let what = match case.probe {
            Some(_) => format!("probe:{}", case.name),
            None => format!("examples/{}.ri", case.name),
        };
        eprintln!("  COVER {what} {:?} [{test}] — {}", case.targets, case.why);
    }
    for (name, why) in BUILD_SURFACE_DROPPED_DUPLICATES {
        eprintln!("  DROP  examples/{name}.ri — {why}");
    }
    for (target, why) in BUILD_SURFACE_UNCOVERED_TARGETS {
        eprintln!("  UNCOVERED TARGET {target} — {why}");
    }
    for (name, cell, reason) in BUILD_SURFACE_KNOWN_RESIDUALS {
        eprintln!("  KNOWN RESIDUAL examples/{name}.ri `{cell}` — {reason}");
    }

    let mut problems: Vec<String> = Vec::new();

    for (name, cell, _) in BUILD_SURFACE_KNOWN_RESIDUALS {
        if !covered
            .iter()
            .any(|c| c.name == *name && c.probe.is_none())
        {
            problems.push(format!(
                "BUILD_SURFACE_KNOWN_RESIDUALS exempts `{cell}` in examples/{name}.ri, but that \
                 file is in neither BUILD_SURFACE_OPTIMIZED_EXAMPLES nor \
                 BUILD_SURFACE_HEAVY_EXAMPLE/BUILD_SURFACE_HEAVY_TRAJECTORY — the exemption is \
                 never exercised and will rot. Either sweep the file again or delete the \
                 residual entry."
            ));
        }
    }
    for (name, _) in BUILD_SURFACE_DROPPED_DUPLICATES {
        if covered.iter().any(|c| c.name == *name) {
            problems.push(format!(
                "examples/{name}.ri is listed BOTH as covered and as a dropped duplicate — the \
                 printed selection would contradict itself."
            ));
        }
    }
    for (target, _) in BUILD_SURFACE_UNCOVERED_TARGETS {
        let creditors: Vec<&str> = covered
            .iter()
            .filter(|c| c.targets.contains(target))
            .map(|c| c.name)
            .collect();
        if !creditors.is_empty() {
            problems.push(format!(
                "`{target}` is listed in BUILD_SURFACE_UNCOVERED_TARGETS but IS credited by \
                 {creditors:?} — the printed selection claims both that nothing covers it and \
                 that something does. Delete the uncovered entry (coverage arrived) or the \
                 credit (it did not)."
            ));
        }
    }
    for case in &covered {
        if case.targets.is_empty() {
            problems.push(format!(
                "examples/{}.ri credits no ComputeNode target — the per-file positive dispatch \
                 assertion keys off `targets`, so an empty list silently reduces the case to two \
                 absence-assertions.",
                case.name
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the build()-surface selection lists contradict each other:\n  {}",
        problems.join("\n  ")
    );
}

/// Makes the build()-surface SELECTION SURVEY executable instead of prose.
///
/// Companion to — deliberately not merged into —
/// `build_surface_selection_is_reported_and_consistent` (task 5578 review round
/// 2). That test asserts the selection lists are INTERNALLY consistent; every
/// failure there is fixed by editing a list. This one asserts the lists match
/// the TREE and the STDLIB — two EXTERNAL facts that change without anyone
/// touching this file, and whose fix is to MEASURE a new candidate and then
/// admit, drop or list it. Fusing them would make one assertion message serve
/// unrelated causes.
///
/// Why it exists at all: the covering set was originally derived from a hand-run
/// grep of `examples/` that was NON-recursive. `examples/` has 147 top-level
/// `.ri` files but 260 recursively, so 13 caller files in subdirectories were
/// never surveyed, and the section comment then asserted — falsely, for 7 of 8
/// targets — that those targets had no example caller at all. Prose cannot fail;
/// this test can.
///
/// TWO assertions:
///
/// (b) FILE-LEVEL — every `examples/**/*.ri` that CALLS an `@optimized` stdlib
///     fn must be in the covering set or in `BUILD_SURFACE_DROPPED_DUPLICATES`.
/// (c) TARGET-LEVEL — every `@optimized` target in the compiled stdlib must be
///     credited by a covered case or listed in `BUILD_SURFACE_UNCOVERED_TARGETS`.
///     ONE-WAY: a credited target absent from the stdlib map does NOT fail —
///     `shell-extract::extract` is credited by `fea_shell_too_thick_annotated`
///     but named by no `.ri` fn at all (the engine inserts it upstream of a
///     shell elastic_static solve), so the credited set is legitimately a
///     superset. An entry in `BUILD_SURFACE_UNCOVERED_TARGETS` that names no
///     stdlib target DOES fail, since nothing could ever cover it.
#[test]
fn build_surface_survey_is_exhaustive() {
    let survey = survey_optimized_callers();
    let cases = build_surface_cases();
    let covered: Vec<&BuildSurfaceCase> = cases.iter().map(|(c, _)| *c).collect();

    let mut problems: Vec<String> = Vec::new();

    // (d) PRINT the survey — this test carries the honesty purpose too, so a
    // green run still reports what was scanned and how each target is disposed.
    eprintln!(
        "build()-surface survey: {} .ri file(s) under examples/ (RECURSIVE), {} of \
         them call an @optimized stdlib fn; {} distinct @optimized target(s) in the \
         compiled stdlib.",
        survey.files_scanned,
        survey.callers.len(),
        survey.stdlib_targets.len()
    );

    for (name, targets) in &survey.callers {
        let is_covered = covered.iter().any(|c| c.probe.is_none() && c.name == name);
        let is_dropped = BUILD_SURFACE_DROPPED_DUPLICATES
            .iter()
            .any(|(dropped, _)| dropped == name);
        let disposition = match (is_covered, is_dropped) {
            (true, _) => "COVERED",
            (_, true) => "DROPPED",
            _ => "UNSURVEYED",
        };
        eprintln!("  {disposition:<10} examples/{name}.ri {targets:?}");
        if !is_covered && !is_dropped {
            problems.push(format!(
                "examples/{name}.ri calls {targets:?} but is in NEITHER the covering set \
                 nor BUILD_SURFACE_DROPPED_DUPLICATES — an @optimized caller nobody \
                 surveyed. MEASURE its build() cost, then admit it (if it reaches a \
                 target the covering set does not) or drop it with that measured cost and \
                 the target it duplicates."
            ));
        }
    }

    for target in &survey.stdlib_targets {
        let credited: Vec<&str> = covered
            .iter()
            .filter(|c| c.targets.contains(&target.as_str()))
            .map(|c| c.name)
            .collect();
        let uncovered_reason = BUILD_SURFACE_UNCOVERED_TARGETS
            .iter()
            .find(|(listed, _)| listed == target);
        match (credited.is_empty(), uncovered_reason) {
            (false, _) => eprintln!("  TARGET COVERED   {target} — by {credited:?}"),
            (true, Some((_, reason))) => eprintln!("  TARGET UNCOVERED {target} — {reason}"),
            (true, None) => {
                let callers_of: Vec<&str> = survey
                    .callers
                    .iter()
                    .filter(|(_, targets)| targets.contains(target))
                    .map(|(name, _)| name.as_str())
                    .collect();
                eprintln!("  TARGET MISSING   {target} — callers: {callers_of:?}");
                problems.push(format!(
                    "@optimized target `{target}` is credited by no covering-set case and is \
                     not listed in BUILD_SURFACE_UNCOVERED_TARGETS. Example callers: \
                     {callers_of:?}. Cover it (measure, then admit the cheapest caller) or \
                     list it uncovered WITH a measured-cost or re-verifiable reason — never \
                     leave it silently absent."
                ));
            }
        }
    }

    for (listed, _) in BUILD_SURFACE_UNCOVERED_TARGETS {
        if !survey.stdlib_targets.iter().any(|t| t == listed) {
            problems.push(format!(
                "BUILD_SURFACE_UNCOVERED_TARGETS lists `{listed}`, which is not an \
                 @optimized target in the compiled stdlib — it was renamed or removed, so \
                 the entry excuses nothing. Delete it."
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the build()-surface selection lists no longer match the tree and the stdlib \
         (this is the rot the survey exists to catch — do NOT fix it by editing the \
         section comment):\n  {}",
        problems.join("\n  ")
    );
}

/// Shared driver for the build()-surface sweep. `cases` is the slice this
/// invocation actually builds — the printed output names exactly those files and
/// nothing else; the full selection (covering set + drops) is reported once by
/// `build_surface_selection_is_reported_and_consistent`.
///
/// THREE assertions per file, of which the third is the positive one:
///
/// 1. ZERO stale-Undef violations after a real `build()`, modulo the exact,
///    printed `BUILD_SURFACE_KNOWN_RESIDUALS` cells.
/// 2. NO diagnostic containing "no registered compute trampoline" — the task
///    4458 guard shape (`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`
///    asserts the same absence against CLI stderr), applied here against
///    `BuildResult.diagnostics`. Never exempted by a residual entry.
/// 3. Every ComputeNode target this case is CREDITED with in `targets` is
///    actually present in the post-build graph.
///
/// (2) is not redundant with (1). The trampoline-missing fallback degrades
/// SILENTLY: it body-inlines an all-required-params sentinel, and whether that
/// surfaces as a stale-Undef violation depends on whether anything downstream
/// happens to read the sentinel's fields. A file whose solver result is computed
/// but never consumed would satisfy (1) while its solver never ran.
///
/// (3) is not redundant with either, and it is the only assertion here that is
/// not an ABSENCE. (1) and (2) are both satisfied vacuously by a file that
/// dispatches nothing at all, so without (3) an example edited to drop its
/// solver call would stay green while the covering set kept crediting it with a
/// target it no longer reaches.
fn run_build_surface_sweep(label: &str, cases: &[BuildSurfaceCase]) {
    let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    eprintln!(
        "{label}: building {} case(s) — see \
         build_surface_selection_is_reported_and_consistent for the full covering \
         set and what was dropped.",
        cases.len()
    );

    let mut offenders: Vec<String> = Vec::new();
    let total = std::time::Instant::now();

    for case in cases {
        let name = case.name;
        let (label, source) = match case.probe {
            Some(src) => (format!("probe:{name}"), src.to_string()),
            None => {
                let path = examples_dir.join(format!("{name}.ri"));
                let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!("reading examples/{name}.ri at {}: {e}", path.display())
                });
                (format!("examples/{name}.ri"), src)
            }
        };

        // PRINTED BEFORE the build, not after: a multi-second build whose line
        // is emitted only on completion is a silent gap — the shape that tripped
        // the verify pipeline's heartbeat-idle backstop in task 4952 and made the
        // eval sweep shard in the first place.
        eprintln!("  BUILD {label} {:?} — {}", case.targets, case.why);

        let t0 = std::time::Instant::now();
        let observed = build_surface_violations(&source, true);
        let elapsed = t0.elapsed();

        let trampoline_missing: Vec<&String> = observed
            .diagnostics
            .iter()
            .filter(|d| d.contains(TRAMPOLINE_MISSING))
            .collect();

        let missing_targets: Vec<&str> = case
            .targets
            .iter()
            .copied()
            .filter(|t| !observed.dispatched_targets.iter().any(|d| d == t))
            .collect();

        // Residuals are matched EXACTLY, by rendered cell — never by prefix and
        // never by file — so a NEW violation in a file that carries a residual
        // still fails, and a residual that got fixed fails too (delete it).
        let mut residual_hits: Vec<&str> = Vec::new();
        let mut unexpected: Vec<&reify_eval::StaleUndefViolation> = Vec::new();
        for v in &observed.violations {
            let rendered = v.cell.to_string();
            match BUILD_SURFACE_KNOWN_RESIDUALS
                .iter()
                .find(|(n, cell, _)| *n == name && *cell == rendered)
            {
                Some((_, _, reason)) => residual_hits.push(reason),
                None => unexpected.push(v),
            }
        }

        eprintln!(
            "  {elapsed:>10.2?}  {label}  (dispatched {:?}; {} violation(s): {} known-residual, \
             {} unexpected; {} trampoline-missing diagnostic(s))",
            observed.dispatched_targets,
            observed.violations.len(),
            residual_hits.len(),
            unexpected.len(),
            trampoline_missing.len()
        );
        for reason in &residual_hits {
            eprintln!("    KNOWN RESIDUAL: {reason}");
        }

        if !missing_targets.is_empty() {
            offenders.push(format!(
                "  {label}: credited with {missing_targets:?} but the post-build graph \
                 dispatched none of them (it dispatched {:?}) — either the example stopped \
                 calling that @optimized fn (its coverage is silently gone: pick another file \
                 for that target) or the engine stopped inserting the node (a real dispatch \
                 regression). Do NOT just edit `targets` to match.",
                observed.dispatched_targets
            ));
        }
        if !trampoline_missing.is_empty() {
            offenders.push(format!(
                "  {label}: {} {TRAMPOLINE_MISSING:?} diagnostic(s) — the \
                 @optimized dispatch fell back to body-inlining a never-run sentinel, \
                 so the solver did NOT run:\n{}",
                trampoline_missing.len(),
                trampoline_missing
                    .iter()
                    .map(|d| format!("    {d}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !unexpected.is_empty() {
            offenders.push(format!(
                "  {label}: {} unexpected stale-Undef violation(s):\n{}",
                unexpected.len(),
                unexpected
                    .iter()
                    .map(|v| format!("    {:?}: {}", v.cell, v.detail))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // A residual that stopped reproducing is dead weight — same rule the
        // eval-surface conventions use: delete the entry, don't leave it.
        for (n, cell, _) in BUILD_SURFACE_KNOWN_RESIDUALS
            .iter()
            .filter(|(n, _, _)| *n == name)
        {
            let still_present = observed
                .violations
                .iter()
                .any(|v| v.cell.to_string() == *cell);
            if !still_present {
                offenders.push(format!(
                    "  examples/{n}.ri: BUILD_SURFACE_KNOWN_RESIDUALS lists `{cell}` but it \
                     no longer reproduces — the underlying defect was fixed; delete the entry \
                     rather than leaving a stale exemption behind."
                ));
            }
        }
    }

    eprintln!("  {label} total: {:.2?}", total.elapsed());

    assert!(
        offenders.is_empty(),
        "the build() surface must leave zero stale-Undef violations (modulo the \
         exact BUILD_SURFACE_KNOWN_RESIDUALS cells), emit no \
         {TRAMPOLINE_MISSING:?} diagnostic, and actually dispatch every target \
         its covering-set entry credits it with.\n\
         A trampoline-missing diagnostic means the engine was constructed without \
         the compute-trampoline registrations (task 5578 / 4458); a stale-Undef \
         violation means a demanded cell was left unevaluated; a missing target \
         means the covering set is crediting coverage that is no longer there. \
         Do NOT add either to BUILD_SURFACE_KNOWN_RESIDUALS to make this pass \
         unless the root cause is genuinely elsewhere and a follow-up task names \
         it.\nOffenders:\n{}",
        offenders.join("\n")
    );
}

/// The build()-surface sweep over the cheap members of the distinct-target
/// covering set (11.54s measured over nine files at `--test-threads=1`). See `run_build_surface_sweep`
/// for the three per-file assertions and
/// `build_surface_selection_is_reported_and_consistent` for the selection this
/// slice comes from.
#[test]
fn build_surface_optimized_examples_have_no_stale_undef() {
    run_build_surface_sweep(
        "build_surface_optimized_examples_have_no_stale_undef",
        BUILD_SURFACE_OPTIMIZED_EXAMPLES,
    );
}

/// The one covering-set member heavy enough to isolate — see
/// `BUILD_SURFACE_HEAVY_EXAMPLE` for why its target cannot simply be dropped and
/// why a hand-written probe (6.98s) stands in for the 78.35s example that is its
/// only other dispatcher. Identical assertions; separate `#[test]` so
/// cargo/nextest schedules it as its own process with its own PASS/SLOW line
/// instead of adding ~7s to the sweep above (the same reasoning that sharded the
/// eval sweep into `CORPUS_SHARD_COUNT` independent tests).
#[test]
fn build_surface_buckling_multi_case_has_no_stale_undef() {
    run_build_surface_sweep(
        "build_surface_buckling_multi_case_has_no_stale_undef",
        BUILD_SURFACE_HEAVY_EXAMPLE,
    );
}

/// The second heavy covering-set member, isolated for the same reason as the
/// first — see `BUILD_SURFACE_HEAVY_TRAJECTORY` for why a ~6s hand-written probe
/// stands in for the 303.26s example that is `trajectory::simulate`'s only
/// other dispatcher, and why the modal solve it pays for along the way is
/// scaffolding rather than credited coverage.
#[test]
fn build_surface_trajectory_simulate_has_no_stale_undef() {
    run_build_surface_sweep(
        "build_surface_trajectory_simulate_has_no_stale_undef",
        BUILD_SURFACE_HEAVY_TRAJECTORY,
    );
}
