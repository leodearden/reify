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
use reify_core::{DiagnosticCode, ModulePath, Severity};
use reify_ir::ExportFormat;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

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
/// has no caps) + 12 edge entries (each `Role::NewEdge`) + 8 corner-vertex
/// entries, plus 1 per-solid representative entry (task #4636's
/// `record_solid_attribute`, keyed on the box's own solid handle — consumed
/// by the OCCT->Manifold cross-kernel ingest forwarder). The per-role /
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
        6 + 12 + 8 + 1,
        "topology_attribute_table must hold 6 face + 12 edge + 8 vertex + 1 solid-representative (task #4636) entries after a single box realization, got {}",
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
/// Concretely: build a single-box structure (table holds 6+12+8+1 = 27
/// entries: 6 faces + 12 edges + 8 corner vertices + 1 per-solid
/// representative entry from task #4636's `record_solid_attribute`), then
/// on the **same engine instance** build a single-sphere structure with no
/// realizations from the box. The table after the second build must
/// reflect only the sphere's entries, not the disjoint union.
///
/// This pins the reset wire-up in `Engine::build` (the
/// `self.topology_attribute_table = TopologyAttributeTable::default()`
/// reset). A missed reset site would surface here as a table bigger than
/// the sphere alone after the second build.
#[test]
fn engine_build_resets_topology_attribute_table_across_builds() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut engine = engine_with_occt();

    // First build: box. The table should hold 6 face + 12 edge + 8 vertex + 1
    // solid-representative (task #4636) entries.
    let box_compiled = compile_no_errors("structure A { let body = box(10mm, 10mm, 10mm) }");
    let box_build = engine.build(&box_compiled, ExportFormat::Step);
    assert_no_geometry_errors(&box_build);
    assert_eq!(
        engine.topology_attribute_table().len(),
        6 + 12 + 8 + 1,
        "first build must seed exactly 6 face + 12 edge + 8 vertex + 1 solid-representative entries for a box realization"
    );

    // Second build: sphere on the SAME engine instance. The table must
    // be cleared and repopulated — the box's 27 entries must not
    // persist alongside the sphere's.
    let sphere_compiled = compile_no_errors("structure B { let body = sphere(5mm) }");
    let sphere_build = engine.build(&sphere_compiled, ExportFormat::Step);
    assert_no_geometry_errors(&sphere_build);

    let after_sphere = engine.topology_attribute_table().len();
    // Sphere's entry count is version-dependent (≥1 face, 0+ edges, no vertices,
    // +1 solid-representative) but emphatically less than the 6+12+8+1 = 27 the
    // box produced. If the table didn't reset, after_sphere would be ≥ 27 + 1 = 28.
    assert!(
        after_sphere < 6 + 12 + 8 + 1,
        "topology_attribute_table must be reset between builds; after the second \
         (sphere) build the table holds {after_sphere} entries, which is ≥ the box's \
         27 — the reset before/after `Engine::build` was missed"
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
/// - the box's 6+12+8+1 = 27 entries (feature_id `A.a#realization[0]`)
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
    // 6 box faces + 12 box edges + 8 box vertices + 1 box solid-representative
    // (task #4636) + ≥1 sphere face = ≥28 entries. Sphere edge counts are
    // version-dependent (0+); the sphere also contributes its own +1
    // solid-representative entry.
    assert!(
        table_len > 6 + 12 + 8 + 1,
        "topology_attribute_table must accumulate entries across both realizations; \
         expected ≥28 (6 box faces + 12 box edges + 8 box vertices + 1 box solid-representative \
         + ≥1 sphere face), got {table_len} — \
         the table was likely reset between realizations within a single build"
    );
}

// ─── task-6550: Engine::build records topology attributes for tube ───────────

/// After `Engine::build()` on a `tube(...)` realization, the engine's
/// `topology_attribute_table()` must contain entries for the tube's 4 faces
/// and its N edges (N ≥ 4: at minimum two cap circles per annulus, plus
/// OCCT's seam edges), plus the one per-solid representative entry from
/// `record_solid_attribute` (task #4636).
///
/// Per-role distribution — 1×Cap(Top), 1×Cap(Bottom), and 2×Side whose
/// `local_index` is ordered by descending radial extent so the outer wall is 0
/// and the bore is 1 — is pinned by the direct-kernel test
/// `seed_primitive_attributes_tube_classifies_annuli_and_orders_walls_by_radius`
/// in `topology_attribute_primitives_direct.rs`. See this file's module
/// rustdoc for why iteration-based assertions live there, not here. This e2e
/// test pins only the count contract: that the seeder is invoked from
/// `Engine::execute_realization_ops` for the `tube(...)` constructor (a missed
/// wire would leave the table empty).
///
/// Measured total on OCCT 7.8 is 11 (4 faces + 6 edges + 1 solid); the `>=`
/// lower bound absorbs per-version seam variance per this file's convention.
#[test]
fn engine_build_records_topology_attributes_for_tube_realization() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors("structure A { let body = tube(10mm, 5mm, 20mm) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    let table = engine.topology_attribute_table();
    // Named so the decomposition stays readable: 4 faces, at least 4 edges,
    // and the 1 per-solid representative entry. (Written as a const rather
    // than inline because clippy::int_plus_one rewrites a literal `>= a + 1`
    // into `> a`, which would erase exactly that decomposition.)
    const MIN_TUBE_ENTRIES: usize = 4 + 4 + 1;
    assert!(
        table.len() >= MIN_TUBE_ENTRIES,
        "topology_attribute_table must hold 4 face + ≥4 edge + 1 solid-representative \
         entries after a tube realization, got {}",
        table.len()
    );
}

/// AMENDMENT PIN (reviewer_comprehensive, task #6550): a selector-bearing
/// module containing a tube collects exactly two Info
/// `TopologyAttributeLocalIndexReassigned` diagnostics, and BOTH are false
/// positives that this task deliberately did not fix.
///
/// # Why they fire
///
/// `detect_local_index_reassignment_diagnostics` groups a realization's
/// entries by `(feature_id, role)` and reports the first distinct-`local_index`
/// pair whose CENTROIDS are within `LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M`
/// (1 nm). A tube produces two such groups:
///
/// - `Side` — the outer wall and the bore are both full 360° revolutions about
///   the z axis, so `BRepGProp::SurfaceProperties` puts both area centroids at
///   the same on-axis point. Distance 0.
/// - `NewEdge` — each annulus contributes a pair of CONCENTRIC circles that
///   likewise share a centroid.
///
/// The `Cap(Top)` / `Cap(Bottom)` groups are singletons, so they are skipped.
///
/// # Why they are wrong, and why they are pinned rather than fixed
///
/// The message says "selector resolution may shuffle after edits". For the two
/// walls that is precisely backwards: their `local_index` is derived from bbox
/// RADIAL EXTENT (outer wall 0, bore 1), not from centroid position or TopExp
/// enumeration order, which is the whole reason the ordering survives an OCCT
/// upgrade. Teaching the tie scan to fall back to a non-centroid discriminator
/// is the real fix, and it lives in `topology_attribute_propagation.rs` —
/// outside this task's module scope. Pinning the current output here means the
/// false positive is a documented, observable fact: whoever does implement that
/// fallback will see this test go red and update it deliberately, and any
/// reader who meets the diagnostic in the wild can find it explained.
///
/// The `let fs = faces(body)` binding is load-bearing: it is what opens the
/// task #5196 L2 selector-presence gate that the tie scan sits behind. Without
/// it the scan never runs and this fixture emits nothing.
///
/// Severity is Info and the build still succeeds — attribute-fragility
/// detection is auxiliary metadata and never regresses a realization to Failed.
#[test]
fn engine_build_tube_trips_the_centroid_tie_scan_false_positive() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled =
        compile_no_errors("structure A { let body = tube(10mm, 5mm, 20mm) let fs = faces(body) }");
    let mut engine = engine_with_occt();
    let build_result = engine.build(&compiled, ExportFormat::Step);
    assert_no_geometry_errors(&build_result);

    // Filter by CODE only, never severity — the L3 Warning→Info downgrade
    // (task #5196) would make a severity filter vacuous.
    let ties: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned))
        .collect();

    // The scan emits AT MOST ONE diagnostic per (feature_id, role) group, and
    // this single-realization fixture has exactly two groups that can tie, so
    // the count is structural rather than an OCCT accident.
    let by_role = |role: &str| -> usize {
        ties.iter()
            .filter(|d| d.message.contains(&format!("role '{role}'")))
            .count()
    };
    assert_eq!(
        (by_role("Side"), by_role("NewEdge"), ties.len()),
        (1, 1, 2),
        "expected exactly one tie diagnostic for the two on-axis Side walls and one for the \
         concentric NewEdge cap circles, and nothing else; got:\n{ties:#?}"
    );
    for d in &ties {
        assert_eq!(
            d.severity,
            Severity::Info,
            "tie diagnostics are advisory and must stay Info: {d:?}"
        );
    }

    // The tied local_index values are deliberately NOT pinned: the Side pair is
    // always {0, 1}, but which NewEdge indices collide depends on OCCT's seam
    // ordering, and that variance is not part of any contract.
    assert!(
        ties.iter()
            .all(|d| d.message.contains("A#realization[0]")),
        "every tie diagnostic must name the tube's own realization: {ties:#?}"
    );
}
