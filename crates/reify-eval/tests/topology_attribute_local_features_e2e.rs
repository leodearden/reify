//! End-to-end tests for v0.2 persistent-naming-v2 attribute auto-population
//! during local-feature ops (fillet / chamfer) — task 7b (#2831).
//!
//! Tests the full pipeline: parse → compile → Engine::build, then asserts on
//! `engine.topology_attribute_table()`. All tests are guarded by
//! `reify_kernel_occt::OCCT_AVAILABLE` and are skipped if OCCT is not present.
//!
//! # What "propagation ran" looks like for fillet/chamfer
//!
//! Unlike boolean ops (where a boolean cut splits one face into multiple
//! children → non-empty `mod_history`), a clean all-edges fillet/chamfer of
//! a 10mm cube returns exactly 1 generated face per edge (12 records, each
//! with a distinct parent edge, no duplicate-parent edge → no splits). OCCT
//! does not report corner-blend faces as Generated-by multiple edges; it
//! attributes them to exactly one edge each.
//!
//! Under Option B (esc-4832-140, task 4947), `mod_history` is no longer
//! empty for the 6 `face_modified` result faces: each unconditionally
//! carries a `ModEntry` attributing the creating local feature, even though
//! the parent→child mapping is 1:1 (the fillet DID reshape the face). The 12
//! `face_generated` result faces are instead ORIGINATED with a fresh
//! attribute (`Role::LocalFeatureFace`, owned by the creating feature)
//! rather than inheriting the parent edge's attribute — see
//! `fillet_provenance_attributes_creating_feature` below for the
//! provenance-discrimination assertions this enables.
//!
//! The signal used by `fillet_feeds_mod_history` / `chamfer_feeds_mod_history`
//! below to confirm "propagation ran at all" is therefore the **growth of the
//! topology-attribute table** beyond the box's 26 primitive entries. Without
//! the `ExecuteWithHistory` Fillet/Chamfer arms in `handle.rs` (RED), the
//! engine returns `AttributeHistory::None` for both ops and `populate_attribute_history`
//! is a no-op → only the box's 26 entries exist. With the arms (GREEN),
//! `propagate_attributes_via_local_feature_history` copies 6 face_modified +
//! 12 face_generated entries onto the fillet/chamfer result shape → table
//! grows to 44 entries (26 box + 18 result). The assertion `table.len() > 26`
//! is the RED/GREEN discriminator.
//!
//! # Derived counts for `fillet(box(10mm,10mm,10mm), 1mm)`:
//!
//! - Box seeding: 6 faces (Role::Side) + 12 edges (Role::NewEdge) + 8 vertices (Role::CornerVertex) = 26
//! - Fillet propagation: 6 face_modified + 12 face_generated = 18 result entries
//! - Total with propagation: 44

use reify_core::ModulePath;
use reify_eval::topology_selectors::role_is_face;
use reify_ir::{ExportFormat, GeometryHandleId, Role};

/// Run a source string through parse → compile → Engine::build and return
/// the engine. Returns `None` if OCCT is not available.
///
/// Mirrors `build_boolean_source` in `topology_attribute_boolean_e2e.rs`:
/// uses `OcctKernelHandle::spawn()` directly (not wrapped in
/// `SingleKernelHolder`) so that `extract_faces` / `extract_edges` /
/// `extract_vertices` are forwarded to OCCT rather than falling through
/// to the default-trait error stubs.
fn build_local_features_source(source: &str) -> Option<reify_eval::Engine> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_local_features_attr"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Build with real OCCT kernel passed directly.
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(reify_kernel_occt::OcctKernelHandle::spawn())),
    );
    let result = engine.build(&compiled, ExportFormat::Step);

    // No Error diagnostics
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(build_errors.is_empty(), "build errors: {:?}", build_errors);

    // Geometry output should be present
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    Some(engine)
}

/// `fillet(box(10mm, 10mm, 10mm), 1mm)` — all-edges fillet.
///
/// Verifies that `ExecuteWithHistory` routes the Fillet op through
/// `fillet_with_history` and that `populate_attribute_history` calls
/// `propagate_attributes_via_local_feature_history`, which copies the box's
/// face/edge attributes onto the fillet result shape.
///
/// The RED/GREEN signal: a 10mm cube has 6F + 12E + 8V = 26 primitive
/// attribute entries. Without the handle.rs Fillet arm (RED), the engine
/// returns `AttributeHistory::None` and `populate_attribute_history` is a
/// no-op → table.len() == 26. With the arm (GREEN), propagation adds 6
/// face_modified + 12 face_generated entries → table.len() == 44 > 26.
///
/// A second, stronger assertion checks that the count of entries with
/// `Role::Side` exceeds the box's 6 (`Role::Side` faces). Propagation copies
/// the parent box face's `Role::Side` to each face_modified result entry, so
/// the GREEN count is 12 (6 box + 6 result). This assertion survives if OCCT
/// reports a different number of face_generated entries across versions, while
/// still proving that face attribute inheritance actually ran.
///
/// Note: under Option B (esc-4832-140), the 6 `face_modified` result faces
/// each carry a one-entry `mod_history` (an unconditional `ModEntry`
/// attributing the fillet), while the 12 `face_generated` result faces
/// carry an empty `mod_history` (they are originated, not split) — see the
/// module-level doc and `fillet_provenance_attributes_creating_feature` for
/// the full derivation and provenance assertions.
#[test]
fn fillet_feeds_mod_history() {
    let source = r#"structure S {
    let r = fillet(box(10mm, 10mm, 10mm), 1mm)
}"#;
    let Some(engine) = build_local_features_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();
    // Box primitive seeding: 6F + 12E + 8V = 26 entries.
    // Fillet propagation (GREEN): 6 face_modified + 12 face_generated = 18 result entries.
    // Total with propagation: 44. Without propagation (RED): only 26.
    assert!(
        table.len() > 26,
        "topology attribute table should exceed the box's 26 primitive entries \
         after fillet propagation (face_modified + face_generated adds 18 result entries); \
         got only {} entries — propagation may not have run",
        table.len()
    );

    // Stronger assertion: result faces must inherit Role::Side from their box parents.
    // Without propagation (RED): 6 Role::Side entries (box's 6 faces only).
    // With propagation (GREEN): ≥12 Role::Side entries (6 box + 6 face_modified inheriting Side).
    let side_count = table
        .iter()
        .filter(|(_id, attr)| attr.role == Role::Side)
        .count();
    assert!(
        side_count > 6,
        "expected >6 entries with Role::Side after fillet propagation \
         (box primitive seeding: 6; fillet face_modified adds ≥6 more inheriting Role::Side \
         from parent box faces); got {} — \
         face attribute inheritance may not have run",
        side_count
    );
}

/// `chamfer(box(10mm, 10mm, 10mm), 1mm)` — all-edges chamfer.
///
/// Mirrors `fillet_feeds_mod_history`: verifies that `ExecuteWithHistory`
/// routes the Chamfer op through `chamfer_with_history` and that propagation
/// adds result entries to the topology attribute table.
///
/// Two assertions (same rationale as `fillet_feeds_mod_history`):
/// - `table.len() > 26`: RED/GREEN discriminator for propagation running at all.
/// - `count(Role::Side) > 6`: confirms face attribute inheritance ran (chamfer
///   face_modified entries carry inherited `Role::Side` from the box parents).
#[test]
fn chamfer_feeds_mod_history() {
    let source = r#"structure S {
    let r = chamfer(box(10mm, 10mm, 10mm), 1mm)
}"#;
    let Some(engine) = build_local_features_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();
    // Box primitive seeding: 6F + 12E + 8V = 26 entries.
    // Chamfer propagation (GREEN): 6 face_modified + 12 face_generated = 18 result entries.
    // Total with propagation: 44. Without propagation (RED): only 26.
    assert!(
        table.len() > 26,
        "topology attribute table should exceed the box's 26 primitive entries \
         after chamfer propagation (face_modified + face_generated adds 18 result entries); \
         got only {} entries — propagation may not have run",
        table.len()
    );

    // Stronger assertion: result faces must inherit Role::Side from their box parents.
    // Without propagation (RED): 6 Role::Side entries (box's 6 faces only).
    // With propagation (GREEN): ≥12 Role::Side entries (6 box + 6 face_modified inheriting Side).
    let side_count = table
        .iter()
        .filter(|(_id, attr)| attr.role == Role::Side)
        .count();
    assert!(
        side_count > 6,
        "expected >6 entries with Role::Side after chamfer propagation \
         (box primitive seeding: 6; chamfer face_modified adds ≥6 more inheriting Role::Side \
         from parent box faces); got {} — \
         face attribute inheritance may not have run",
        side_count
    );
}

/// Capstone acceptance for Option B (esc-4832-140): fillet-generated and
/// fillet-modified faces are attributed to the *creating* local feature, not
/// the base feature, so `created_by`/`split_by`-style provenance selectors
/// can discriminate a fillet's own faces from the base shape's
/// surviving/modified faces. Task 4832 (gamma) exercises this substrate
/// through actual DSL selectors once this lands.
#[test]
fn fillet_provenance_attributes_creating_feature() {
    // NOTE(esc-4947-11): the base box MUST be bound to its own `let` so that
    // it gets a distinct realization FeatureId from the fillet. `FeatureId` is
    // assigned per named-`let` realization (one `FeatureId::from(realization_id)`
    // per declared `let`, engine_build.rs), NOT per nested op. With the
    // idiomatic single-`let` inline form `fillet(box(...), 1mm)`, `box(...)`
    // and `fillet(...)` share realization S#0 and therefore the SAME
    // FeatureId, so `created_by`/`split_by` provenance selectors cannot
    // discriminate the fillet from its base by construction. Two `let`s give
    // `b` = realization S#0 and `r` = realization S#1 — distinct FeatureIds —
    // which is what makes the disjointness acceptance criteria observable.
    // Task 4832 (gamma, DSL-selector coverage) must use the same two-`let`
    // shape to exercise selector discrimination.
    let source = r#"structure S {
    let b = box(10mm, 10mm, 10mm)
    let r = fillet(b, 1mm)
}"#;
    let Some(engine) = build_local_features_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();

    // The fillet's own realization FeatureId: any Role::LocalFeatureFace
    // entry is owned by it (stream-2 origination in
    // propagate_attributes_via_local_feature_history).
    let fillet_feat = table
        .iter()
        .find(|(_id, attr)| attr.role == Role::LocalFeatureFace)
        .map(|(_id, attr)| attr.feature_id.clone())
        .expect(
            "at least one Role::LocalFeatureFace entry must exist after fillet propagation",
        );

    // The box's realization FeatureId: Role::CornerVertex is a box-only
    // role (local-feature propagation never writes result vertices), so any
    // such entry is unambiguously owned by the base feature.
    let base_feat = table
        .iter()
        .find(|(_id, attr)| matches!(attr.role, Role::CornerVertex { .. }))
        .map(|(_id, attr)| attr.feature_id.clone())
        .expect("at least one Role::CornerVertex entry (box corner) must exist");

    assert_ne!(
        fillet_feat, base_feat,
        "the fillet's realization feature must differ from the box's"
    );

    // created_by(fillet_feat): face-kind entries owned by the fillet — the
    // ~12 originated (face_generated) faces.
    let created_by_fillet: Vec<GeometryHandleId> = table
        .iter()
        .filter(|(_id, attr)| role_is_face(attr.role) && attr.feature_id == fillet_feat)
        .map(|(id, _attr)| id)
        .collect();
    assert!(
        !created_by_fillet.is_empty(),
        "created_by(fillet) must be non-empty (expected ~12 generated faces)"
    );
    for id in &created_by_fillet {
        let attr = table.lookup(*id).expect("collected handle must be in table");
        assert_eq!(
            attr.role,
            Role::LocalFeatureFace,
            "every created_by(fillet) face must carry Role::LocalFeatureFace, got {:?} for {:?}",
            attr.role,
            id
        );
    }

    // split_by(fillet_feat): face-kind entries whose mod_history records the
    // fillet as a splitting feature — the ~6 modified faces.
    let split_by_fillet: Vec<GeometryHandleId> = table
        .iter()
        .filter(|(_id, attr)| {
            role_is_face(attr.role)
                && attr
                    .mod_history
                    .iter()
                    .any(|entry| entry.splitting_feature_id == fillet_feat)
        })
        .map(|(id, _attr)| id)
        .collect();
    assert!(
        !split_by_fillet.is_empty(),
        "split_by(fillet) must be non-empty (expected ~6 modified faces)"
    );

    for id in &created_by_fillet {
        assert!(
            !split_by_fillet.contains(id),
            "created_by(fillet) and split_by(fillet) must be disjoint; {:?} is in both",
            id
        );
    }

    // created_by(base_feat): face-kind entries still owned by the box (the
    // surviving/modified Side faces) — non-empty and disjoint from the
    // fillet's own created_by set.
    let created_by_base: Vec<GeometryHandleId> = table
        .iter()
        .filter(|(_id, attr)| role_is_face(attr.role) && attr.feature_id == base_feat)
        .map(|(id, _attr)| id)
        .collect();
    assert!(
        !created_by_base.is_empty(),
        "created_by(base) must be non-empty (surviving/modified box Side faces)"
    );
    for id in &created_by_base {
        assert!(
            !created_by_fillet.contains(id),
            "created_by(base) and created_by(fillet) must be disjoint; {:?} is in both",
            id
        );
    }

    // Regression guard (same thresholds as fillet_feeds_mod_history): the
    // Role::LocalFeatureFace addition must not shrink table growth or the
    // Role::Side inheritance count.
    assert!(
        table.len() > 26,
        "topology attribute table should exceed the box's 26 primitive entries; got {}",
        table.len()
    );
    let side_count = table
        .iter()
        .filter(|(_id, attr)| attr.role == Role::Side)
        .count();
    assert!(
        side_count > 6,
        "expected >6 entries with Role::Side after fillet propagation; got {side_count}"
    );
}
