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
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())))
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
        6 + 12 + 8,
        "topology_attribute_table must hold 6 face + 12 edge + 8 vertex entries after a single box realization, got {}",
        table.len()
    );
}

// ─── step-13: Engine::build records topology attributes for cylinder/sphere ──

/// After `Engine::build()` on a `cylinder(...)` realization, the engine's
/// `topology_attribute_table()` must contain entries for the cylinder's 3
/// faces + N edges (N ≥ 2: at minimum the top and bottom cap circles, plus
/// a possible seam edge depending on OCCT's parameterisation).
///
/// Per-role distribution (1×Cap(Top), 1×Cap(Bottom), 1×Side faces; N×NewEdge
/// edges) is pinned by the direct-kernel test
/// `seed_primitive_attributes_cylinder_classifies_cap_top_cap_bottom_and_side`
/// in `topology_attribute_primitives_direct.rs` — see this file's module
/// rustdoc for why iteration-based assertions live there, not here. This
/// e2e test pins only the count contract: that the seeder is invoked from
/// `Engine::execute_realization_ops` for the `cylinder(...)` constructor
/// (a missed wire would leave the table empty).
///
/// Will fail if `Engine::execute_realization_ops` doesn't seed for the
/// `Cylinder` arm.
#[test]
fn engine_build_records_topology_attributes_for_cylinder_realization() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors("structure A { let body = cylinder(5mm, 10mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let table = engine.topology_attribute_table();
    // Lower bound: 3 faces + 2 cap-circle edges. OCCT may emit a third
    // (seam) edge — see the cylinder sub-case of
    // `seed_primitive_attributes_records_new_edge_for_every_extracted_edge`
    // in the direct tests for the per-version variance. A `>=` lower bound
    // is enough to prove the seeder is wired (the per-role counts are pinned
    // by the direct tests).
    assert!(
        table.len() >= 3 + 2,
        "topology_attribute_table must hold ≥3 face + ≥2 edge entries after a cylinder realization, got {}",
        table.len()
    );
}

/// After `Engine::build()` on a `sphere(...)` realization, the engine's
/// `topology_attribute_table()` must contain entries for the sphere's
/// faces + edges. OCCT's sphere parameterisation may emit ≥1 face and
/// 0+ edges (varies across OCCT versions and seam handling). The contract
/// pinned here is "non-empty" — proof the seeder ran for the `Sphere` arm.
///
/// Per-face assertions (every face is `Role::Side`, no `Cap` entries) are
/// pinned by the direct-kernel test
/// `seed_primitive_attributes_sphere_records_role_side_for_each_face` in
/// `topology_attribute_primitives_direct.rs`.
///
/// Will fail if `Engine::execute_realization_ops` doesn't seed for the
/// `Sphere` arm.
#[test]
fn engine_build_records_topology_attributes_for_sphere_realization() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors("structure A { let body = sphere(5mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let table = engine.topology_attribute_table();
    // Lower bound: ≥1 face entry. Sphere edges are version-dependent; the
    // table may hold 1 face entry (smooth sphere) or more (with seams).
    // The "non-empty" contract is enough to prove the Sphere arm was
    // invoked from the engine — the per-face role distribution is pinned
    // by the direct-kernel test.
    assert!(
        !table.is_empty(),
        "topology_attribute_table must be non-empty after a sphere realization (≥1 face entry expected); got 0"
    );
}

// ─── step-15: cross-build reset + multi-realization coverage ──────────────────

/// Across two `Engine::build` calls, `topology_attribute_table` must be
/// reset between builds — the second build's entries must not carry
/// stale entries from the first.
///
/// Concretely: build a single-box structure (table holds 6+12+8 = 26
/// entries: 6 faces + 12 edges + 8 corner vertices), then on the **same
/// engine instance** build a single-sphere structure with no realizations
/// from the box. The table after the second build must reflect only the
/// sphere's entries, not the disjoint union.
///
/// This pins the reset wire-up in `Engine::build` (the
/// `self.topology_attribute_table = TopologyAttributeTable::default()`
/// reset that mirrors the `feature_tag_table` reset in the same call).
/// A missed reset site would surface here as a table bigger than the
/// sphere alone after the second build.
#[test]
fn engine_build_resets_topology_attribute_table_across_builds() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut engine = engine_with_occt();

    // First build: box. The table should hold 6 face + 12 edge + 8 vertex entries.
    let box_compiled = compile_no_errors("structure A { let body = box(10mm, 10mm, 10mm) }");
    let box_build = engine.build(&box_compiled, ExportFormat::Step);
    assert_no_geometry_errors(&box_build);
    assert_eq!(
        engine.topology_attribute_table().len(),
        6 + 12 + 8,
        "first build must seed exactly 6 face + 12 edge + 8 vertex entries for a box realization"
    );

    // Second build: sphere on the SAME engine instance. The table must
    // be cleared and repopulated — the box's 18 entries must not
    // persist alongside the sphere's.
    let sphere_compiled = compile_no_errors("structure B { let body = sphere(5mm) }");
    let sphere_build = engine.build(&sphere_compiled, ExportFormat::Step);
    assert_no_geometry_errors(&sphere_build);

    let after_sphere = engine.topology_attribute_table().len();
    // Sphere's entry count is version-dependent (≥1 face, 0+ edges, no vertices)
    // but emphatically less than the 6+12+8 = 26 the box produced. If the
    // table didn't reset, after_sphere would be ≥ 26 + 1 = 27.
    assert!(
        after_sphere < 6 + 12 + 8,
        "topology_attribute_table must be reset between builds; after the second \
         (sphere) build the table holds {after_sphere} entries, which is ≥ the box's \
         26 — the reset before/after `Engine::build` was missed"
    );
    assert!(
        after_sphere >= 1,
        "second build's sphere realization must seed ≥1 face entry; got {after_sphere}"
    );
}

/// A single module with two realizations (`let a = box(...)` followed
/// by `let b = sphere(...)`) must populate the topology-attribute table
/// with **both** realizations' entries — the seeder must not be
/// over-aggressively reset between realizations within a single build.
///
/// The two realizations produce distinct `RealizationNodeId`s
/// (`A(realization_index=0)` and `A(realization_index=1)` since each
/// `let` becomes a separate realization), and thus distinct
/// `FeatureId`s. The table after the build must hold:
/// - the box's 6+12+8 = 26 entries (feature_id `A.a#realization[0]`)
/// - plus the sphere's ≥1 face entries (feature_id `A.b#realization[1]`)
///
/// A regression that resets the table between realizations within
/// `Engine::build` would surface here as an entry count equal to only
/// the second realization's.
#[test]
fn engine_build_records_topology_attributes_for_multi_realization_module() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled =
        compile_no_errors("structure A { let a = box(10mm, 10mm, 10mm) let b = sphere(5mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let table_len = engine.topology_attribute_table().len();
    // 6 box faces + 12 box edges + 8 box vertices + ≥1 sphere face = ≥27 entries.
    // Sphere edge counts are version-dependent (0+).
    assert!(
        table_len > 6 + 12 + 8,
        "topology_attribute_table must accumulate entries across both realizations; \
         expected ≥27 (6 box faces + 12 box edges + 8 box vertices + ≥1 sphere face), got {table_len} — \
         the table was likely reset between realizations within a single build"
    );
}
