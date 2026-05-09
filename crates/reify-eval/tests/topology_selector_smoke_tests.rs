//! Worked-example smoke tests for the topology-selector function family
//! (PRD `docs/prds/topology-selectors.md` task 7).
//!
//! Three `.ri` fixtures live under `examples/topology_selectors/`:
//!
//! * `all_topology_selectors_wiring.ri` — compile-time wiring for all 11 names
//! * `block_inertia.ri` — `moment_of_inertia` on a steel box
//! * `fillet_top_edges.ri` — `single`/`flat_map`/`adjacent_faces`/`shared_edges`/`faces_by_normal`/`fillet`
//!
//! ## Scope: parse + compile-with-stdlib + `#[ignore]`-gated eval
//!
//! Coverage follows the four-level staircase from `field_source_kinds_smoke.rs`
//! (parse → compile_with_stdlib → eval → constraints):
//!
//! | Fixture                              | Parse | Compile | Eval (gated) |
//! |--------------------------------------|-------|---------|--------------|
//! | `all_topology_selectors_wiring.ri`   | ✓ (via examples_smoke walker) | ✓ active | ✗ (eval dispatch pending) |
//! | `block_inertia.ri`                   | ✓     | ✓ active (`block_inertia_compiles_with_stdlib_no_errors`) | `#[ignore]` (dispatch pending) |
//! | `fillet_top_edges.ri`                | ✓     | `#[ignore]` (3-arg fillet binding missing) | `#[ignore]` (3-arg fillet + dispatch) |
//!
//! The `#[ignore]`-gated eval tests document future-state contracts and pin the
//! API surface so a follow-up agent knows exactly what to implement to unblock them.
//! They are not run in CI but their presence is intentional — see each test's
//! `#[ignore]` string for the precise blocker and pointer into the code.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib, MockGeometryKernel};
use reify_types::{ExportFormat, GeometryOp, ModulePath, Value, ValueCellId};

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

/// Eval-deepening contract: the `i` cell in `BlockInertia` must resolve to a
/// rank-2 `Value::Tensor` (a `Tensor<2, 3, MomentOfInertia>` per PRD §3.9 —
/// a 3×3 matrix of `MomentOfInertia`-dimensioned scalars).
///
/// For a uniform-density box with side lengths W=50mm, H=30mm, D=10mm and
/// density ρ=7850 kg/m³, the principal moments satisfy I_xx = (1/12)·m·(H²+D²),
/// etc.  The test asserts the nested-Tensor shape rather than numeric values so
/// it remains valid for any kernel-precision implementation.
///
/// **Blocked by**: eval-side dispatch for `moment_of_inertia` in
/// `crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector` — the
/// 11 task-2699 names fall through to `_ => return None` today (see
/// `geometry_ops.rs:1646-1661`), leaving cells at `Value::Undef`.  Tracked by
/// PRD `docs/prds/topology-selectors.md` task 8 as task 2691's eval arms.
/// This smoke test documents the contract and lands the gating test; remove the
/// `#[ignore]` once the `moment_of_inertia` arm is added to
/// `try_eval_topology_selector`.
#[test]
#[ignore = "pending eval-side dispatch for moment_of_inertia in \
            crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector — \
            the 11 task-2699 names fall through to None today (see geometry_ops.rs:1646-1661), \
            leaving cells at Value::Undef. Tracked by PRD docs/prds/topology-selectors.md \
            task 8 as task 2691's eval arms; this smoke task documents the contract and \
            lands the gating test."]
fn block_inertia_evals_moment_of_inertia_to_tensor() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile errors: {:#?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("BlockInertia", "i");
    match result.values.get(&cell) {
        // Enforce rank-2: exactly 3 rows, each a Tensor of exactly 3 cols.
        // A rank-1 Tensor (e.g. a 3-vector) would satisfy `Value::Tensor(_)`
        // but is NOT the expected 3×3 MomentOfInertia matrix (PRD §3.9).
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            // rank-2, 3×3 — correct
        }
        other => panic!(
            "expected BlockInertia.i to be a rank-2 Value::Tensor of 3 rows × 3 cols \
             (Tensor<2,3,MomentOfInertia> per PRD §3.9), got {other:?}"
        ),
    }
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

/// Compile-with-stdlib contract for `fillet_top_edges.ri`.
///
/// **Blocked by**: the missing 3-arg `fillet(solid, edges, radius)` stdlib
/// binding — the current compiler only wires 2-arg `fillet(solid, radius)` at
/// `crates/reify-compiler/src/geometry_modify.rs:115`.  The example uses the
/// 3-arg form on line 25 (`fillet(b, top_edges, 1mm)`), so this currently
/// fails with `fillet() expects 2 arguments, got 3`.
///
/// This is **NOT** a task 2698/2699 gap — `single`, `flat_map`,
/// `faces_by_normal`, `adjacent_faces`, and `shared_edges` are all landed on
/// HEAD.  The sole remaining compile blocker is the 3-arg fillet binding.
///
/// Do NOT modify `fillet_top_edges.ri` to use the 2-arg form: that would
/// fillet ALL edges and defeat the example's pedagogic purpose (demonstrating
/// topology-relational edge selection).
///
/// Remove the `#[ignore]` once a 3-arg `fillet(solid, edges, radius)` stdlib
/// binding is wired in `crates/reify-compiler/src/geometry_modify.rs`.
#[test]
#[ignore = "pending 3-arg fillet(solid, edges, radius) stdlib binding — current compiler \
            only wires 2-arg fillet(solid, radius) per \
            crates/reify-compiler/src/geometry_modify.rs:115. \
            The example uses the 3-arg form (line 25 of fillet_top_edges.ri). \
            This is NOT a task 2698/2699 binding gap (those are landed); \
            it is a separate stdlib-binding gap tracked outside this PRD."]
fn fillet_top_edges_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(FILLET_TOP_EDGES_PATH)
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/fillet_top_edges.ri should compile with no error-severity \
         diagnostics (unblocked by 3-arg fillet stdlib binding), got:\n{:#?}",
        errors_only(&compiled)
    );
}

/// Eval-deepening contract: `engine.build()` on `fillet_top_edges.ri` must
/// produce non-empty `geometry_output`, confirming that the topology-walk
/// pipeline (`faces_by_normal` → `single` → `adjacent_faces` → `shared_edges`
/// → `flat_map`) supplies real edge handles to the 3-arg `fillet` call so the
/// kernel can produce a filleted solid.
///
/// Expected dataflow:
/// 1. `top = single(faces_by_normal(b, vec3(0,0,1), 1deg))` — extract the single
///    upward-facing face handle of the box via `faces_by_normal` + `single`.
/// 2. `top_edges = flat_map(adjacent_faces(b, top), |f| shared_edges(top, f))` —
///    walk topology: for each face adjacent to `top`, collect the edges shared
///    between `top` and that face, giving the four top-perimeter edges.
/// 3. `result = fillet(b, top_edges, 1mm)` — apply the 3-arg fillet to produce
///    a new solid with the four top-perimeter edges rounded to r=1mm.
///
/// Architecture note: `fillet` is a geometry-modification operation; its output
/// solid is produced by the kernel and surfaced via `BuildResult::geometry_output`
/// (a serialised B-Rep blob), NOT via `BuildResult::values`.  Value cells in
/// `values` are reserved for scalar/computed results (Point, Scalar, Tensor,
/// etc.) from topology-query helpers such as `closest_point` / `on` /
/// `moment_of_inertia`.  Therefore this test asserts `geometry_output.is_some()`
/// — the kernel delivered a B-Rep — rather than checking a specific `Value`
/// variant in `values` for the `result` cell.
///
/// **Blocked by two prerequisites** (both must land before this fixture runs):
///
/// (a) 3-arg `fillet(solid, edges, radius)` stdlib binding —
///     `crates/reify-compiler/src/geometry_modify.rs:115` only wires 2-arg
///     `fillet(solid, radius)`; the example fails to compile today with
///     `fillet() expects 2 arguments, got 3`.
///
/// (b) Eval-side dispatch for `single`/`flat_map` list-helper eval AND
///     `faces_by_normal`/`adjacent_faces`/`shared_edges` arms in
///     `try_eval_topology_selector` (`crates/reify-eval/src/geometry_ops.rs:1646-1661`).
///     Until (b) lands the intermediate topology cells stay at `Value::Undef`
///     and `top_edges` carries no real edge handles, so the kernel cannot
///     produce a fillet result.
///
/// Remove the `#[ignore]` once both (a) and (b) are implemented.
///
/// **Test scaffolding note**: the test plumbs `MockGeometryKernel::new()` into
/// `Engine::new(Box::new(checker), Some(Box::new(kernel)))` — no `.with_*_result()`
/// configuration is needed for the `geometry_output.is_some()` assertion because the
/// default mock returns `b"MOCK_EXPORT_DATA"` from `export()`.  Plumbing the kernel
/// is a structural prerequisite of the assertion: `engine_build.rs:681` gates
/// `geometry_output` on `self.geometry_kernel.is_some()`, so a `None`-kernel
/// `Engine` would always produce `geometry_output = None` even after both (a) and
/// (b) land.  This is not a third semantic gap — it is an invisible test-scaffolding
/// requirement that would otherwise confuse a future agent removing the `#[ignore]`.
#[test]
#[ignore = "pending (a) 3-arg fillet(solid, edges, radius) stdlib binding \
            (geometry_modify.rs:115) and (b) eval-side dispatch for single/flat_map \
            (list_helpers eval) AND faces_by_normal/adjacent_faces/shared_edges \
            (try_eval_topology_selector arms at geometry_ops.rs:1646-1661) — \
            both prerequisites must land before this fixture can produce a Solid \
            result at runtime"]
fn fillet_top_edges_evals_to_solid_via_topology_walk() {
    let source = std::fs::read_to_string(FILLET_TOP_EDGES_PATH)
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile errors (3-arg fillet binding must be present): {:#?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    // Capture a shared handle to the op log *before* the kernel moves into
    // Engine::new.  This is the established pattern used by
    // curve_constructors_e2e.rs:86, sweep_e2e.rs:55, helpers.rs:575, etc.
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // The original `result.geometry_output.is_some()` assertion was vacuous:
    // MockGeometryKernel::execute (mocks.rs:1219) allocates a fresh
    // GeometryHandleId for *every* op — including the very first
    // `box(50mm, 30mm, 10mm)` call — so `geometry_output` is always `Some`
    // regardless of whether the topology-walk → fillet pipeline ever fired.
    // (Flagged in esc-2691-81, reviewer_comprehensive suggestion 1 of 3.)
    //
    // The strengthened assertion suite below detects the exact regression the
    // task names: if the topology walk silently short-circuits and only the
    // box op is dispatched, either (a) the op count will be 1 (< 2) or (b)
    // the last recorded op will not be a Fillet — both fire loudly.
    let ops = ops_ref.lock().unwrap();

    // (a) at least 2 ops recorded — box realization + at least one fillet op.
    //     Catches the regression scenario where the topology walk silently
    //     short-circuits and only the box op is dispatched.
    assert!(
        ops.len() >= 2,
        "expected at least 2 recorded ops (box + fillet), got {}: {:?}",
        ops.len(),
        ops.iter().map(|r| r.op.kind_name()).collect::<Vec<_>>()
    );

    // (b) the LAST recorded op must be a Fillet — confirms the topology
    //     walk → fillet pipeline ran end-to-end. MockGeometryKernel::execute
    //     (mocks.rs:1219) returns a fresh handle for *every* op, so without
    //     this check the geometry_output assertion below would pass on the
    //     box handle alone (the original false-positive flagged in esc-2691-81).
    assert!(
        matches!(
            ops.last().map(|r| &r.op),
            Some(GeometryOp::Fillet { .. })
        ),
        "expected last recorded op to be GeometryOp::Fillet, got: {:?}",
        ops.last().map(|r| r.op.kind_name())
    );

    // (c) sanity-pin geometry_output (kept from the original assertion)
    //     so a future engine_build.rs:681 regression that drops the export
    //     stage is also caught.
    assert!(
        result.geometry_output.is_some(),
        "expected non-empty geometry_output after a successful Fillet op"
    );

    // TODO(prereq-a, no current task ID): once 3-arg fillet binding lands and
    // extends `GeometryOp::Fillet` with an `edges: Vec<GeometryHandleId>` field,
    // deepen (b) to also assert `!edges.is_empty()` so that a regression where
    // the topology walk delivers an empty edge list (the kernel is invoked but
    // fillets *zero* edges) is caught. The variant currently has only
    // `target: GeometryHandleId` and `radius: Value` (geometry.rs:440-443).
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
