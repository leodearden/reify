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
