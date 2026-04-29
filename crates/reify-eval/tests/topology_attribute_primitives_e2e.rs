//! End-to-end tests for the v0.2 topology-attribute table on the build
//! pipeline (PRD docs/prds/v0_2/persistent-naming-v2.md decomposition-plan
//! task 6).
//!
//! Mirror of `feature_tag_e2e.rs` but for the `topology_attribute_table`:
//! parse → compile_with_stdlib → Engine::build (with real OCCT kernel) →
//! `engine.topology_attribute_table()`. The direct-kernel tests in
//! `topology_attribute_primitives_direct.rs` cover the seeder's per-arm
//! semantics (per-face role, per-edge role, FeatureId stamping); this file
//! covers the full pipeline integration — that the seeder is actually
//! invoked from `Engine::execute_realization_ops` for every primitive
//! constructor surfaced in the source.
//!
//! Gated on `OCCT_AVAILABLE` (same convention as `feature_tag_e2e.rs` and
//! `topology_attribute_e2e.rs`). Tests are skipped on builds without OCCT.
//!
//! ## Why these e2e tests pin only `len()` counts
//!
//! `TopologyAttributeTable` (in `reify-types`) currently exposes only
//! `record / lookup / len / is_empty` — there is no `iter()` accessor and
//! the engine does not expose its kernel back to the test. That means a
//! test cannot enumerate the entries to per-role-count them: a fresh
//! kernel spawned in the test allocates fresh handle ids that won't
//! match the engine's internal handle ids, so `lookup()` against
//! independently-built handles always returns `None`. The per-role and
//! per-feature_id contracts are therefore covered by the direct-kernel
//! tests in `topology_attribute_primitives_direct.rs`. These e2e tests
//! pin the count contract — that's enough to prove the seeder is
//! actually wired into `Engine::execute_realization_ops` (a missed wire
//! would leave the table empty).

use reify_compiler::compile_with_stdlib;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{ExportFormat, ModulePath, Severity};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test_topology_attr_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

fn engine_with_occt() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(OcctKernelHandle::spawn())),
    )
}

fn assert_no_geometry_errors(build_result: &reify_eval::BuildResult) {
    let geom_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:#?}",
        geom_errors
    );
    assert!(
        build_result.geometry_output.is_some(),
        "expected geometry output (STEP bytes) for a primitive realization"
    );
}

// ─── step-11: Engine::build records topology attributes for box realization ──

/// After `Engine::build()` on a `box(...)` realization, the engine's
/// `topology_attribute_table()` must contain one entry per face/edge of
/// the resulting box: 6 face entries (each `Role::Side`, since a box
/// has no caps) + 12 edge entries (each `Role::NewEdge`). The per-role /
/// per-feature_id assertions are covered by
/// `topology_attribute_primitives_direct.rs` — see this file's module
/// rustdoc for why iteration-based assertions live there, not here.
///
/// Will fail until step-12 wires `seed_primitive_attributes` into
/// `Engine::execute_realization_ops`.
#[test]
fn engine_build_records_topology_attributes_for_box_realization() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors("structure A { let body = box(10mm, 10mm, 10mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let table = engine.topology_attribute_table();
    assert_eq!(
        table.len(),
        6 + 12,
        "topology_attribute_table must hold 6 face + 12 edge entries after a single box realization, got {}",
        table.len()
    );
}
