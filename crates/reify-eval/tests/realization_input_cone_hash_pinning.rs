//! Integration test pinning the `RealizationNodeData.input_cone_hash` field
//! (task 4728 α).
//!
//! Mirrors the `realization_kernel_provenance.rs` build harness and the
//! `realization_produced_repr_pinning.rs` snapshot-read path:
//! - skips when OCCT is unavailable
//! - parse → reify_compiler::compile → Engine → engine.build(&compiled, Step)
//! - reads engine.snapshot()…graph.realizations, asserts on input_cone_hash
//!
//! Three assertions (all require the production build() path, NOT eval()):
//!
//! (a) is_some() — the field is populated on every realization node after build().
//! (b) MOVES — the same realization's hash differs across two builds whose
//!     feeding params differ (body box width 10mm vs 20mm).
//! (c) STABLE — the same realization's hash is equal across two builds that
//!     differ only in an UNRELATED param outside its op-arg cone (the "other"
//!     box, which is a second independent realization).
//!
//! FIXTURE CONSTRAINT (c): the unrelated param drives a SECOND, INDEPENDENT
//! realization ("other") whose ops do not feed "body"'s op-arg expressions.
//! Changing "other"'s size does not affect "body"'s evaluated arg values.
//!
//! Write sites in engine_build.rs are wired (task 4728 s2 completed) — this
//! test is GREEN. All three assertions below must pass on every build.

use reify_core::{ModulePath, RealizationNodeId, Severity};
use reify_ir::ExportFormat;

/// Parse, compile, build and return the engine.  Panics on any parse/compile
/// error or if the build produced no geometry.
fn build_engine_from_source(source: &str, module_name: &str) -> reify_eval::Engine {
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {module_name}: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {module_name}: {:?}",
        errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let result = engine.build(&compiled, ExportFormat::Step);
    assert!(
        result.geometry_output.is_some(),
        "build produced no geometry in {module_name}; diagnostics: {:?}",
        result.diagnostics
    );
    engine
}

/// Main pinning test — three builds, three assertions.
///
/// Structure "S" has two realizations:
///   - body = box(width, 10mm, 10mm)   ← index 0; the "in-cone" param is width
///   - other = box(side, side, side)   ← index 1; UNRELATED to body's cone
///
/// Build A: width=10mm, side=5mm   (baseline)
/// Build B: width=10mm, side=8mm   (only unrelated "other" changes)
/// Build C: width=20mm, side=5mm   (only in-cone "body" param changes)
#[test]
fn realization_input_cone_hash_is_populated_and_input_sensitive() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping realization_input_cone_hash_is_populated_and_input_sensitive: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Build A — baseline: body 10×10×10 mm, other 5×5×5 mm.
    let source_a = r#"structure S {
    let body = box(10mm, 10mm, 10mm)
    let other = box(5mm, 5mm, 5mm)
}"#;

    // Build B — UNRELATED change: only the "other" box size differs.
    // "body"'s op-arg expressions are identical to build A.
    let source_b = r#"structure S {
    let body = box(10mm, 10mm, 10mm)
    let other = box(8mm, 8mm, 8mm)
}"#;

    // Build C — IN-CONE change: "body" box width is 20mm instead of 10mm.
    let source_c = r#"structure S {
    let body = box(20mm, 10mm, 10mm)
    let other = box(5mm, 5mm, 5mm)
}"#;

    let engine_a = build_engine_from_source(source_a, "hash_pin_a");
    let engine_b = build_engine_from_source(source_b, "hash_pin_b");
    let engine_c = build_engine_from_source(source_c, "hash_pin_c");

    let snap_a = engine_a
        .snapshot()
        .expect("snap_a must be Some after successful build()");
    let snap_b = engine_b
        .snapshot()
        .expect("snap_b must be Some after successful build()");
    let snap_c = engine_c
        .snapshot()
        .expect("snap_c must be Some after successful build()");

    // (a) Every realization node must have input_cone_hash = Some after build().
    //     This pinning guards the PRODUCTION path: build() must populate the field;
    //     eval() alone does not (it never reaches the engine_build.rs write sites).
    assert!(
        !snap_a.graph.realizations.is_empty(),
        "expected at least one realization node in snap_a; \
         check that the source compiles to a non-empty graph"
    );
    for (id, node) in snap_a.graph.realizations.iter() {
        assert!(
            node.input_cone_hash.is_some(),
            "realization {:?}: expected input_cone_hash = Some(_) after build(); \
             got None — unexpected regression, engine_build.rs write sites are wired \
             (task 4728 s2 completed)",
            id
        );
    }

    // "body" is the first realization in structure "S" (compilation index 0).
    let body_id = RealizationNodeId::new("S", 0);

    let body_a = snap_a
        .graph
        .realizations
        .get(&body_id)
        .expect("'body' realization (S/0) must be present in snap_a");
    let body_b = snap_b
        .graph
        .realizations
        .get(&body_id)
        .expect("'body' realization (S/0) must be present in snap_b");
    let body_c = snap_c
        .graph
        .realizations
        .get(&body_id)
        .expect("'body' realization (S/0) must be present in snap_c");

    // (c) STABLE: body hash must be equal across builds A and B.
    //     Only "other" (the second independent realization) changed in B;
    //     "body"'s op-arg cone is unaffected.
    //
    //     Guard: assert body_b.input_cone_hash is Some so the equality can only
    //     pass because the hashes genuinely match, not because both are None.
    assert!(
        body_b.input_cone_hash.is_some(),
        "body realization (snap_b): expected input_cone_hash = Some(_) after build(); \
         got None — unexpected regression, engine_build.rs write sites are wired \
         (task 4728 s2 completed)"
    );
    assert_eq!(
        body_a.input_cone_hash,
        body_b.input_cone_hash,
        "body realization hash must be STABLE when only the unrelated 'other' \
         realization's param changes (build A vs B)"
    );

    // (b) MOVES: body hash must differ between builds A and C.
    //     Build C uses box(20mm, ...) for body vs box(10mm, ...) in A,
    //     so compute_realization_upstream_values_hash must return a different value.
    //
    //     Guard: assert body_c.input_cone_hash is Some so assert_ne!(Some, None)
    //     cannot pass for the wrong reason (masking a write-site regression).
    assert!(
        body_c.input_cone_hash.is_some(),
        "body realization (snap_c): expected input_cone_hash = Some(_) after build(); \
         got None — unexpected regression, engine_build.rs write sites are wired \
         (task 4728 s2 completed)"
    );
    assert_ne!(
        body_a.input_cone_hash,
        body_c.input_cone_hash,
        "body realization hash must MOVE when the in-cone param changes \
         (build A: 10mm width vs build C: 20mm width)"
    );
}
