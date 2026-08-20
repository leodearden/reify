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

        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        reify_eval::compute_targets::register_compute_fns(&mut engine);
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

        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        reify_eval::compute_targets::register_compute_fns(&mut engine);
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
        let mut engine = reify_eval::Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            Some(Box::new(reify_test_support::MockGeometryKernel::new())),
        );
        reify_eval::compute_targets::register_compute_fns(&mut engine);
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
    let (violations, diagnostics) = build_surface_violations(OPTIMIZED_PROBE_SRC, false);

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

/// Compile `source` through the stdlib prelude, run it through a real
/// `Engine::build(.., ExportFormat::Step)`, and return
/// `(post-build stale-Undef violations, the build's diagnostic messages)`.
///
/// Engine wiring is `run_corpus_shard`'s, verbatim — a
/// [`SimpleConstraintChecker`](reify_constraints::SimpleConstraintChecker) plus
/// a default [`MockGeometryKernel`](reify_test_support::MockGeometryKernel) —
/// with the terminal `engine.eval(&compiled)` swapped for `engine.build(..)`.
/// That swap is the whole point: it is precisely the surface half of the
/// coverage gap task 5578 names, since every sweep above this line only ever
/// drives `eval()`.
///
/// `register_compute` is an explicit switch, not a convenience knob. `false` is
/// what lets `seeded_build_surface_sweep_reports_a_planted_violation` reproduce
/// the task-5578 defect in miniature; `true` is what the real sweep drives.
/// Nothing else may pass `false`.
///
/// The registered arm installs BOTH `register_compute_fns` and
/// `register_shell_extract_compute_fns`, matching production (`reify-cli`'s
/// `configured_eval_engine` → `register_production_compute_fns`, which calls
/// both). `run_corpus_shard` above installs only the first — harmless there
/// because it never asserts on diagnostics, but the sweep below DOES, and
/// `examples/fea_shell_too_thick_annotated.ri` measurably emits
/// `@optimized target "shell-extract::extract": no registered compute trampoline`
/// without the second. That is the same defect this task fixes, so it is fixed
/// here rather than skipped.
///
/// A source that fails to COMPILE panics rather than being skipped. That is
/// deliberate and differs from `run_corpus_shard`'s printed compile-error skip:
/// this helper only ever runs over an explicit, curated file list, so a compile
/// error there means the LIST is stale — a defect to surface loudly, not a file
/// to quietly drop.
fn build_surface_violations(
    source: &str,
    register_compute: bool,
) -> (Vec<reify_eval::StaleUndefViolation>, Vec<String>) {
    let compiled = reify_test_support::compile_source_with_stdlib(source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "build_surface_violations: the source must compile without errors — a \
         compile error here means the caller's curated file list is stale: {errors:#?}"
    );

    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(reify_test_support::MockGeometryKernel::new())),
    );
    if register_compute {
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        reify_eval::register_shell_extract_compute_fns(&mut engine);
    }
    let result = engine.build(&compiled, reify_ir::ExportFormat::Step);
    let messages: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();

    (engine.check_no_stale_undef(), messages)
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
    let (violations, diagnostics) = build_surface_violations(OPTIMIZED_PROBE_SRC, true);

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
}

// ── The bounded build()-surface sweep ────────────────────────────────────────
//
// Selection method, stated so bounded coverage never reads as full coverage:
//
//   1. Grep every `@optimized`-annotated stdlib fn name (`as_printed_material`,
//      `displacement_at`, `fdm_slice`, `form_find`, `form_find_free`,
//      `input_shape`, `inverse_dynamics`, `mechanism_modal_analysis`,
//      `membrane_load`, `modal_analysis`, `simulate_trajectory`,
//      `solve_buckling`, `solve_buckling_load_cases`, `solve_elastic_static`,
//      `solve_load_cases`, `transient_response`) against `examples/`. That
//      yields 18 candidate files.
//   2. Reduce to a DISTINCT-TARGET covering set — one file per `@optimized`
//      dispatch target, choosing the cheapest measured file for each. That is
//      the cost trim the sweep's own purpose licenses: it guards the dispatch
//      path per TARGET, so a second file hitting an already-covered target buys
//      no new coverage. Every dropped file is listed with its measured cost and
//      the target it duplicates in `BUILD_SURFACE_DROPPED_DUPLICATES`, and both
//      lists are PRINTED at run time.
//
// Measured (debug profile, `--test-threads=1`) the full 18-file list costs
// 136.63s; the covering set costs ~86s, ~78s of which is the single
// `buckling_multi_case_smoke` file — which is why that one file gets its own
// `#[test]` (see `build_surface_buckling_multi_case_has_no_stale_undef`), the
// same "independent, concurrently-scheduled process with its own PASS/SLOW
// line" reasoning that made the eval sweep shard into `CORPUS_SHARD_COUNT`.
//
// Out of reach here, and deliberately not faked: `dynamics::inverse_dynamics`,
// `fdm::slice`, `modal::*` and `trajectory::*` have NO `examples/*.ri` caller,
// so this sweep cannot cover them. They are guarded by their own crate tests.

/// The distinct-target covering set: `(example base name, the @optimized
/// target(s) it uniquely contributes)`. See the section comment above for how
/// it was derived and what was dropped.
const BUILD_SURFACE_OPTIMIZED_EXAMPLES: &[(&str, &str)] = &[
    (
        "fea_shell_too_thick_annotated",
        "solver::elastic_static + shell-extract::extract (15ms — the cheapest \
         elastic-static caller, and the ONLY example that dispatches shell-extract)",
    ),
    ("fdm_bracket", "fdm::as_printed_material_r_fast (204ms — the only caller)"),
    ("fea_multi_case_bracket", "solver::multi_case (1.10s)"),
    ("buckling_column_p2", "solver::buckling (6.19s)"),
    ("tensegrity_cable_net", "solver::form_find (9.8ms)"),
    (
        "tensegrity_pavilion",
        "solver::form_find_free + solver::membrane_load (48.5ms — the only \
         membrane_load caller)",
    ),
];

/// The single covering-set member heavy enough to warrant its own `#[test]`.
const BUILD_SURFACE_HEAVY_EXAMPLE: &[(&str, &str)] = &[(
    "buckling_multi_case_smoke",
    "solver::buckling_multi_case (78.35s — the ONLY example that dispatches this \
     target, so it cannot be dropped without losing the target entirely; isolated \
     into its own #[test] so it runs as its own concurrently-scheduled process \
     rather than serializing ~78s behind the cheap files)",
)];

/// Candidate files dropped from the sweep because they duplicate a target the
/// covering set already reaches. PRINTED on every run — never a silent
/// truncation. Costs are the measured debug-profile `build()` wall clock.
const BUILD_SURFACE_DROPPED_DUPLICATES: &[(&str, &str)] = &[
    ("anisotropic_bar", "3.12s — solver::elastic_static, covered by fea_shell_too_thick_annotated (15ms)"),
    ("buckling_column_smoke", "43.96s — solver::buckling, covered by buckling_column_p2 (6.19s)"),
    ("differential_field_ops", "349ms — solver::elastic_static"),
    ("fea_cantilever_smoke", "410ms — solver::elastic_static"),
    ("fea_multi_case_smoke", "294ms — solver::elastic_static"),
    ("fea_pressure_smoke", "286ms — solver::elastic_static"),
    ("fea_shell_flexure", "138ms — solver::elastic_static"),
    ("fea_shell_too_thick_auto", "41ms — solver::elastic_static"),
    (
        "multi_load_bracket",
        "2.06s — solver::multi_case, covered by fea_multi_case_bracket (1.10s). \
         Also carries a KNOWN_RESIDUAL_SKIPS entry on the eval surface \
         (MultiLoadBracket.critical_case, a worst_case/lambda-dispatch product \
         limitation), so importing it here would add a known-broken case for zero \
         added target coverage.",
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
];

/// Build-surface counterpart of `KNOWN_RESIDUAL_SKIPS` — same convention
/// (reasoned per entry, matched exactly, PRINTED never silent), but keyed
/// `(example base name, EXACT cell rendered as "Entity.member", reason)` rather
/// than by file. Cell-level rather than file-level is a deliberate tightening:
/// a blanket file skip would also suppress a NEW violation in that file and the
/// trampoline-missing assertion, both of which still apply here.
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

/// Shared driver for the build()-surface sweep. `cases` is a slice of
/// `(example base name, why it is in the covering set)`.
///
/// Two assertions per file, and the second is what this task adds to the class:
///
/// 1. ZERO stale-Undef violations after a real `build()`, modulo the exact,
///    printed `BUILD_SURFACE_KNOWN_RESIDUALS` cells.
/// 2. NO diagnostic containing "no registered compute trampoline" — the task
///    4458 guard shape (`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`
///    asserts the same absence against CLI stderr), applied here against
///    `BuildResult.diagnostics`. Never exempted by a residual entry.
///
/// (2) is not redundant with (1). The trampoline-missing fallback degrades
/// SILENTLY: it body-inlines an all-required-params sentinel, and whether that
/// surfaces as a stale-Undef violation depends on whether anything downstream
/// happens to read the sentinel's fields. A file whose solver result is computed
/// but never consumed would satisfy (1) while its solver never ran.
fn run_build_surface_sweep(label: &str, cases: &[(&str, &str)]) {
    let examples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    eprintln!(
        "{label}: BOUNDED coverage — {} of the 18 @optimized-bearing examples/ \
         files, reduced to a distinct-target covering set:",
        cases.len()
    );
    for (name, why) in cases {
        eprintln!("  COVER examples/{name}.ri — {why}");
    }
    for (name, why) in BUILD_SURFACE_DROPPED_DUPLICATES {
        eprintln!("  DROP  examples/{name}.ri — {why}");
    }

    let mut offenders: Vec<String> = Vec::new();
    let total = std::time::Instant::now();

    for (name, _why) in cases {
        let path = examples_dir.join(format!("{name}.ri"));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading examples/{name}.ri at {}: {e}", path.display()));

        let t0 = std::time::Instant::now();
        let (violations, diagnostics) = build_surface_violations(&source, true);
        let elapsed = t0.elapsed();

        let trampoline_missing: Vec<&String> = diagnostics
            .iter()
            .filter(|d| d.contains(TRAMPOLINE_MISSING))
            .collect();

        // Residuals are matched EXACTLY, by rendered cell — never by prefix and
        // never by file — so a NEW violation in a file that carries a residual
        // still fails, and a residual that got fixed fails too (delete it).
        let mut residual_hits: Vec<&str> = Vec::new();
        let mut unexpected: Vec<&reify_eval::StaleUndefViolation> = Vec::new();
        for v in &violations {
            let rendered = v.cell.to_string();
            match BUILD_SURFACE_KNOWN_RESIDUALS
                .iter()
                .find(|(n, cell, _)| n == name && *cell == rendered)
            {
                Some((_, _, reason)) => residual_hits.push(reason),
                None => unexpected.push(v),
            }
        }

        eprintln!(
            "  {elapsed:>10.2?}  {name}  ({} violation(s): {} known-residual, {} unexpected; \
             {} trampoline-missing diagnostic(s))",
            violations.len(),
            residual_hits.len(),
            unexpected.len(),
            trampoline_missing.len()
        );
        for reason in &residual_hits {
            eprintln!("    KNOWN RESIDUAL: {reason}");
        }

        if !trampoline_missing.is_empty() {
            offenders.push(format!(
                "  examples/{name}.ri: {} {TRAMPOLINE_MISSING:?} diagnostic(s) — the \
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
                "  examples/{name}.ri: {} unexpected stale-Undef violation(s):\n{}",
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
        for (n, cell, _) in BUILD_SURFACE_KNOWN_RESIDUALS.iter().filter(|(n, _, _)| n == name) {
            let still_present = violations
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
         exact BUILD_SURFACE_KNOWN_RESIDUALS cells) and emit no \
         {TRAMPOLINE_MISSING:?} diagnostic for every covered example.\n\
         A trampoline-missing diagnostic means the engine was constructed without \
         the compute-trampoline registrations (task 5578 / 4458); a stale-Undef \
         violation means a demanded cell was left unevaluated. Do NOT add either \
         to BUILD_SURFACE_KNOWN_RESIDUALS to make this pass unless the root cause \
         is genuinely elsewhere and a follow-up task names it.\nOffenders:\n{}",
        offenders.join("\n")
    );
}

/// The build()-surface sweep over the cheap members of the distinct-target
/// covering set (~7.6s measured). See `run_build_surface_sweep` for the two
/// per-file assertions and the section comment above for how the set was chosen.
#[test]
fn build_surface_optimized_examples_have_no_stale_undef() {
    run_build_surface_sweep(
        "build_surface_optimized_examples_have_no_stale_undef",
        BUILD_SURFACE_OPTIMIZED_EXAMPLES,
    );
}

/// The one covering-set member heavy enough to isolate — see
/// `BUILD_SURFACE_HEAVY_EXAMPLE`. Identical assertions; separate `#[test]` so
/// cargo/nextest schedules it as its own process with its own PASS/SLOW line
/// instead of adding ~78s to the sweep above (the same reasoning that sharded
/// the eval sweep into `CORPUS_SHARD_COUNT` independent tests).
#[test]
fn build_surface_buckling_multi_case_has_no_stale_undef() {
    run_build_surface_sweep(
        "build_surface_buckling_multi_case_has_no_stale_undef",
        BUILD_SURFACE_HEAVY_EXAMPLE,
    );
}
