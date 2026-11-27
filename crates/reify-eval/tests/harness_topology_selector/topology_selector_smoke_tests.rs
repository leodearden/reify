//! Worked-example smoke tests for the topology-selector function family
//! (PRD `docs/prds/topology-selectors.md` task 7).
//!
//! Three `.ri` fixtures live under `examples/topology_selectors/`:
//!
//! * `all_topology_selectors_wiring.ri` — compile-time wiring for all 11 names
//! * `block_inertia.ri` — `moment_of_inertia` on a steel box
//! * `fillet_top_edges.ri` — `single`/`flat_map`/`adjacent_faces`/`shared_edges`/`faces_by_normal`/`fillet`
//!
//! Coverage follows the four-level staircase from `field_source_kinds_smoke.rs`
//! (parse → compile_with_stdlib → eval → constraints):
//!
//! | Fixture                              | Parse | Compile | Eval |
//! |--------------------------------------|-------|---------|------|
//! | `all_topology_selectors_wiring.ri`   | ✓ (via examples_smoke walker) | ✓ active | ✗ (eval dispatch pending) |
//! | `block_inertia.ri`                   | ✓     | ✓ active (`block_inertia_compiles_with_stdlib_no_errors`) | ✓ active (`block_inertia_evals_moment_of_inertia_to_tensor`, mock-seeded inertia reply) |
//! | `fillet_top_edges.ri`                | ✓     | ✓ active (`fillet_top_edges_compiles_with_stdlib_no_errors`) | ✓ real OCCT (`fillet_top_edges_example_builds_clean_e2e` in `crates/reify-eval/tests/fillet_curated_edges_e2e.rs`) |
//!
//! The 3-arg `fillet(solid, edges, radius)` binding is live (introduced by
//! #3205/#4360/#4362), and curated 3-arg fillet has been reachable through the
//! production `.ri` pipeline (`reify eval` + GUI) since #5208, which un-ignored
//! the compile tier for `fillet_top_edges.ri`. Its eval tier runs on the real
//! kernel because the bare `MockGeometryKernel` seeds no topology for the
//! selector walk to resolve.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

fn block_inertia_path() -> std::path::PathBuf {
    reify_test_support::examples_corpus::examples_dir().join("topology_selectors/block_inertia.ri")
}

fn fillet_top_edges_path() -> std::path::PathBuf {
    reify_test_support::examples_corpus::examples_dir().join("topology_selectors/fillet_top_edges.ri")
}

fn all_topology_selectors_wiring_path() -> std::path::PathBuf {
    reify_test_support::examples_corpus::examples_dir().join("topology_selectors/all_topology_selectors_wiring.ri")
}

#[test]
fn all_topology_selectors_wiring_compiles_with_stdlib() {
    let source = std::fs::read_to_string(all_topology_selectors_wiring_path())
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
/// The eval tier is `block_inertia_evals_moment_of_inertia_to_tensor`.
#[test]
fn block_inertia_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(block_inertia_path())
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
/// density ρ=7850 kg/m³, the principal moments satisfy
/// I_xx = (1/12)·m·(H²+D²), I_yy = (1/12)·m·(W²+D²), I_zz = (1/12)·m·(W²+H²)
/// where m = ρ·V = 7850·(0.05·0.03·0.01) = 0.11775 kg. Off-diagonals are zero
/// for a principal-axes-aligned box. The test asserts the nested-Tensor shape
/// (rank, 3×3, dimensioned scalars) rather than the numeric values directly so
/// it remains the design-doc-bound contract irrespective of kernel precision.
///
/// Mocks the kernel reply with the analytic principal moments — pre-computed
/// as `f64` constants below — so the test does not depend on an OCCT kernel.
/// The dispatch path under test is
/// `geometry_ops::dispatch_inertia_tensor`, which decodes the kernel's
/// nested `Value::List(List<Real>)` reply into the canonical nested-`Value::Tensor`
/// with each leaf being a `Value::Scalar { dimension: MOMENT_OF_INERTIA, .. }`.
#[test]
fn block_inertia_evals_moment_of_inertia_to_tensor() {
    use reify_ir::GeometryHandleId;

    let source = std::fs::read_to_string(block_inertia_path())
        .expect("examples/topology_selectors/block_inertia.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile errors: {:#?}",
        errors_only(&compiled)
    );

    // Analytic principal moments for a 50mm × 30mm × 10mm box at 7850 kg/m³.
    // m = ρ·V = 7850·(0.05·0.03·0.01) = 0.11775 kg
    // I_xx = (1/12)·m·(H² + D²)  with H=0.03, D=0.01
    // I_yy = (1/12)·m·(W² + D²)  with W=0.05, D=0.01
    // I_zz = (1/12)·m·(W² + H²)  with W=0.05, H=0.03
    let w = 0.05f64;
    let h = 0.03f64;
    let d = 0.01f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);
    // Mock the OCCT kernel reply shape: `Value::List` of 3 rows × 3 cols,
    // each col a dimensionless `Value::Real`. The dispatcher re-wraps each
    // leaf as a MomentOfInertia-dimensioned `Value::Scalar`.
    let inertia_reply = Value::List(vec![
        Value::List(vec![Value::Real(i_xx), Value::Real(0.0), Value::Real(0.0)]),
        Value::List(vec![Value::Real(0.0), Value::Real(i_yy), Value::Real(0.0)]),
        Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(i_zz)]),
    ]);

    let kernel = MockGeometryKernel::new().with_inertia_tensor_result(
        GeometryHandleId(1),
        7850.0,
        inertia_reply,
    );
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), Some(Box::new(kernel)));
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
    let source = std::fs::read_to_string(block_inertia_path())
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
/// ACTIVE (un-ignored by #5208).  The 3-arg `fillet(solid, edges, radius)`
/// stdlib binding this test once waited on is wired in
/// `crates/reify-compiler/src/geometry_modify.rs`
/// (`"fillet" => match compiled_args.len() { 2 | 3 => ... }`, #3205), so the
/// example's `fillet(b, top_edges, 1mm)` compiles with no Error diagnostics.
/// The bulk walker
/// `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs`
/// covers the
/// same file now that its `SKIP_SET` entry is gone; this test is kept as the
/// named, PRD-anchored assertion for the topology-selector family.
///
/// Do NOT modify `fillet_top_edges.ri` to use the 2-arg form: that would
/// fillet ALL edges and defeat the example's pedagogic purpose (demonstrating
/// topology-relational edge selection).
#[test]
fn fillet_top_edges_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(fillet_top_edges_path())
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/fillet_top_edges.ri should compile with no error-severity \
         diagnostics (unblocked by 3-arg fillet stdlib binding), got:\n{:#?}",
        errors_only(&compiled)
    );
}

#[test]
fn fillet_top_edges_ri_parses_cleanly() {
    let source = std::fs::read_to_string(fillet_top_edges_path())
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
