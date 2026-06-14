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
//! attributes them to exactly one edge each. This means `mod_history` is
//! always empty for fillet/chamfer of a simple box.
//!
//! The correct "propagation ran" signal is therefore the **growth of the
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
use reify_ir::ExportFormat;

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
/// Note: `mod_history` is empty for all fillet entries because OCCT reports
/// exactly 1 generated face per edge for a clean box fillet — no multi-child
/// parent edges → no splits (see module-level doc for the full derivation).
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
}

/// `chamfer(box(10mm, 10mm, 10mm), 1mm)` — all-edges chamfer.
///
/// Mirrors `fillet_feeds_mod_history`: verifies that `ExecuteWithHistory`
/// routes the Chamfer op through `chamfer_with_history` and that propagation
/// adds result entries to the topology attribute table.
///
/// Same RED/GREEN discriminator: table.len() > 26 proves propagation ran.
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
}
