//! End-to-end tests for v0.2 persistent-naming-v2 attribute auto-population
//! during boolean ops (union / difference / intersection) — task 8 (#2656).
//!
//! Tests the full pipeline: parse → compile → Engine::build, then asserts on
//! `engine.topology_attribute_table()`. All tests are guarded by
//! `reify_kernel_occt::OCCT_AVAILABLE` and are skipped if OCCT is not present.
//!
//! Key signal: a non-empty `mod_history` in any table entry is EXCLUSIVE to
//! Boolean split propagation — primitive seeding always writes
//! `mod_history = Vec::new()`. A test that produces ≥1 entry with non-empty
//! `mod_history` verifies end-to-end that the Boolean wiring actually ran and
//! detected a parent face split.

use reify_core::ModulePath;
use reify_ir::ExportFormat;

/// Run a source string through parse → compile → Engine::build and return
/// the engine. Returns `None` if OCCT is not available.
///
/// Uses `OcctKernelHandle::spawn()` directly (not wrapped in `SingleKernelHolder`)
/// because `SingleKernelHolder` does not forward `extract_faces` / `extract_edges`
/// to the inner kernel — those methods fall back to the default-trait error impl,
/// silently leaving the topology attribute table empty.  Passing the OCCT kernel
/// directly ensures `seed_primitive_attributes_for_handle` can call `extract_faces`
/// and populate the table, matching the `topology_attribute_primitives_e2e.rs` pattern.
fn build_boolean_source(source: &str) -> Option<reify_eval::Engine> {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return None;
    }

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bool_attr"));
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

    // Build with real OCCT kernel passed directly so extract_faces/extract_edges
    // are forwarded to OCCT (not the default-trait error stubs).
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
    assert!(
        build_errors.is_empty(),
        "build errors: {:?}",
        build_errors
    );

    // Geometry output should be present
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    Some(engine)
}

/// `difference(big_box, partial_slot)` where the slot is narrower than the
/// top face, so the cutter crosses two side faces → the top face of the
/// big box is split into multiple children. Asserts ≥1 table entry has
/// non-empty `mod_history` (the exclusive signal of split propagation).
///
/// Geometry: 20mm³ box minus a 5mm×40mm×15mm box translated 5mm in Z.
/// The cutter's 40mm Y-extent and 15mm Z-extent straddle the top face
/// (Z = +10mm boundary of the 20mm box), carving a rectangular notch whose
/// walls come from the cutter — this forces OCCT to report the top face as
/// Modified with multiple children.
#[test]
fn difference_splitting_a_face_feeds_mod_history() {
    let source = r#"structure S {
    let r = difference(box(20mm, 20mm, 20mm), translate(box(5mm, 40mm, 15mm), 0mm, 0mm, 5mm))
}"#;
    let Some(engine) = build_boolean_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();
    assert!(
        !table.is_empty(),
        "topology attribute table must be non-empty after difference build"
    );

    // At least one entry must have a non-empty mod_history — the signature of
    // a face that was split by the Boolean operation.
    let has_split = table.iter().any(|(_id, attr)| !attr.mod_history.is_empty());
    assert!(
        has_split,
        "expected ≥1 topology attribute entry with non-empty mod_history \
         (indicating a parent face was split by the difference); \
         table has {} entries, all with empty mod_history",
        table.len()
    );
}

/// `union(big_box, wall_box)` where the wall's Z-extent straddles the top
/// face of the big box, so the top face appears as Modified with two children
/// in the fuse history. Asserts ≥1 table entry has non-empty `mod_history`.
///
/// Geometry: 20mm³ box fused with a 20mm×5mm×20mm box translated 15mm in Z.
/// The second box starts at Z = 15mm (inside the first box) and ends at
/// Z = 35mm (outside), straddling the first box's top face (Z = +10mm).
#[test]
fn union_splitting_a_face_feeds_mod_history() {
    let source = r#"structure S {
    let r = union(box(20mm, 20mm, 20mm), translate(box(20mm, 5mm, 20mm), 0mm, 0mm, 15mm))
}"#;
    let Some(engine) = build_boolean_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();
    assert!(
        !table.is_empty(),
        "topology attribute table must be non-empty after union build"
    );

    let has_split = table.iter().any(|(_id, attr)| !attr.mod_history.is_empty());
    assert!(
        has_split,
        "expected ≥1 topology attribute entry with non-empty mod_history \
         (indicating a parent face was split by the union); \
         table has {} entries, all with empty mod_history",
        table.len()
    );
}

/// Guard test: `intersection(box, offset_box)` builds cleanly, produces
/// geometry, and leaves the attribute table non-empty. Uses a partial-overlap
/// setup (second box translated +2mm in X) rather than fully-coincident boxes
/// to avoid OCCT coplanar-face fragility.
///
/// The `!table.is_empty()` assertion is intentionally minimal for this guard:
/// the intersection of two convex boxes cannot split a parent face, so
/// `mod_history` — the stronger signal used by the union/difference tests —
/// is not available here. The primary regression guard is the no-Error +
/// non-empty-geometry assertion inside `build_boolean_source`; those WOULD
/// fail if the intersection engine arm regressed to a build error.
#[test]
fn intersection_build_populates_attributes() {
    // Offset second box +2mm in X for a genuine partial overlap
    // (8mm×10mm×10mm result solid) — avoids OCCT coincident-face fragility.
    let source = r#"structure S {
    let r = intersection(box(10mm, 10mm, 10mm), translate(box(10mm, 10mm, 10mm), 2mm, 0mm, 0mm))
}"#;
    let Some(engine) = build_boolean_source(source) else {
        return;
    };

    let table = engine.topology_attribute_table();
    assert!(
        !table.is_empty(),
        "topology attribute table must be non-empty after intersection build; \
         both input box primitives and the intersection result should have been seeded"
    );
}
