//! Cross-cutting boundary/regression gate for the structured `FeatureId` /
//! `Value::Feature` contract at the PRODUCTION engine boundary (P1 ε, task
//! #4810).
//!
//! This is the B+H integration gate over already-landed, green foundation
//! work: P1 δ (`crates/reify-eval/src/shell_extract_compute.rs` +
//! `engine_admin.rs`, #4809). It is a committed CHARACTERIZATION suite, not
//! a RED-first driver — a failing row here means a genuine cross-cutting
//! contract regression in δ, never a bug to silently patch upstream from
//! this file.
//!
//! Rows covered here (see `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`):
//! B10 and B11.
//!
//! Ownership notes:
//! - `synthetic_slab_field` and the `field()` helper are copied verbatim
//!   from `crates/reify-eval/tests/mid_surface_fold_e2e.rs` (itself copied
//!   verbatim from `shell_extract_compute_integration.rs:28`), per the
//!   established single-file-fixture convention — this file has no
//!   cross-test imports.
//! - `mid_surface_fold_e2e.rs`'s own
//!   `naming_value_carries_face_records_and_edges_lists` test already
//!   asserts the B10 shape inline (`Value::Feature`, not `Value::String`);
//!   this file is the dedicated cross-cutting home cited by task ε, and its
//!   own copied fixture is intentionally independent of that file per the
//!   single-file-fixture convention.
//! - B11's FULL no-regression signal is the existing e2e suite staying
//!   green: `topology_attribute_e2e`, `topology_attribute_extrude_revolve_e2e`,
//!   `topology_attribute_sweep_loft_e2e`, `topology_attribute_resolver_e2e`,
//!   `mid_surface_fold_e2e`, `shell_extract_compute_integration`, and
//!   `m8_m11_regression_checkpoint` (run separately; see task #4810 step-6).
//! - B9's exhaustiveness half is owned by
//!   `crates/reify-eval/tests/m8_m11_regression_checkpoint.rs`.

use reify_eval::register_shell_extract_compute_fns;
use reify_ir::{FeatureId, InterpolationKind, SampledField, SampledGridKind, StructureInstanceData, Value};
use reify_test_support::make_simple_engine;

// ── Shared fixture (copied verbatim; see file header) ─────────────────────────

/// Construct a synthetic thin-slab `SampledField` (5×5×3 grid) whose SDF
/// encodes a slab centred at z=0 with half-thickness 0.1.
fn synthetic_slab_field() -> SampledField {
    let x_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let y_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

    let mut data = Vec::with_capacity(5 * 5 * 3);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - 0.1);
            }
        }
    }

    SampledField {
        name: "synthetic_slab".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -0.5],
        bounds_max: vec![1.0, 1.0, 0.5],
        spacing: vec![0.25, 0.25, 0.5],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Access a field by `&str` key in a `StructureInstanceData`.
fn field<'a>(si: &'a StructureInstanceData, key: &str) -> Option<&'a Value> {
    si.fields.get(&key.to_string())
}

/// Dispatch `shell-extract::extract` on the synthetic slab and return the
/// result `Value` (a `StructureInstanceData` wrapping the `ShellExtractionResult`
/// projection).
fn dispatch_shell_extract() -> Value {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);

    let options = Value::Undef;
    let sdf_value = Value::SampledField(synthetic_slab_field());

    let (result, _diags) = engine
        .dispatch_compute_node("shell-extract::extract", &[options, sdf_value], &[], &Value::Undef, None)
        .expect("dispatch_compute_node must succeed on synthetic slab");
    result
}

/// Drill into `result.naming.face_records[0].feature_id`, requiring
/// `face_records` to be non-empty (the synthetic slab is a single connected
/// region, so the fixture always yields exactly one face record).
fn first_face_feature_id(result: &Value) -> FeatureId {
    let outer = match result {
        Value::StructureInstance(d) => d,
        other => panic!("expected ShellExtractionResult StructureInstance, got {other:?}"),
    };
    let naming = match field(outer, "naming") {
        Some(Value::StructureInstance(d)) => d,
        other => panic!("expected naming: StructureInstance, got {other:?}"),
    };
    let face_records = match field(naming, "face_records") {
        Some(Value::List(l)) => l,
        other => panic!("expected face_records: List, got {other:?}"),
    };
    assert!(
        !face_records.is_empty(),
        "fixture must yield >=1 region (synthetic_slab_field is a single connected slab); \
         an empty face_records here signals a segmentation/fixture drift, not a feature_id \
         contract regression"
    );
    let first = match face_records.first() {
        Some(Value::StructureInstance(d)) => d,
        other => panic!("expected face_records[0]: StructureInstance, got {other:?}"),
    };
    match field(first, "feature_id") {
        Some(Value::Feature(fid)) => fid.clone(),
        Some(Value::String(s)) => panic!(
            "face_records[0].feature_id is Value::String({s:?}) -- the pre-P1-delta \
             stringly-typed shape; production must carry Value::Feature (B10)"
        ),
        other => panic!("expected feature_id: Value::Feature, got {other:?}"),
    }
}

/// B10: the production shell-extract compute path (`dispatch_compute_node`
/// through the public engine) projects `naming.face_records[i].feature_id`
/// as `Value::Feature(_)` — explicitly NOT `Value::String(_)` — proving P1 δ
/// flipped the producer end-to-end, not merely in an in-crate unit test.
#[test]
fn b10_production_path_carries_value_feature() {
    let result = dispatch_shell_extract();
    let fid = first_face_feature_id(&result);

    // The recovered FeatureId is structurally sound: it has a non-empty
    // entity name and its Display round-trips through FromStr (B3's
    // contract, now observed at the production engine boundary).
    assert!(!fid.entity().is_empty(), "feature_id entity must be non-empty");
    let s = fid.to_string();
    assert_eq!(
        s.parse::<FeatureId>().as_ref(),
        Ok(&fid),
        "production feature_id Display must round-trip via FromStr: {s:?}"
    );
}

/// B11 (derivation-determinism half of "no regression / cache stability" —
/// PRD row B11): re-deriving the SAME shell-extract dispatch on two
/// independently constructed fresh engines yields a structurally identical
/// projected `FeatureId`. `FeatureId` derivation is structural-by-construction
/// (entity name + index only, no external/random state), so a single
/// `fid_a == fid_b` equality is the entire meaningful assertion here:
/// `entity()`/`index()`/`Display`/`content_hash()` equality all follow from
/// structural equality, so asserting them separately would be redundant.
///
/// This is a low-cost regression tripwire against an accidental dependency
/// on engine-instance-local state creeping into the derivation, NOT a
/// persistence/disk-cache round-trip test — the PRD's actual "cache
/// stability" signal for B11 is the full existing
/// `topology_attribute_*_e2e.rs` suite staying green (see file header
/// note above), and the disk-codec round-trip is B6's `ShellExtractionResult`
/// serialize/deserialize test in
/// `reify-shell-extract/tests/feature_id_boundary.rs`. Named
/// `..._derivation_is_deterministic_...` (rather than
/// `..._is_stable_across_fresh_engines`) to avoid over-claiming a
/// cache-stability signal this unit test does not itself exercise.
#[test]
fn b11_projected_feature_id_derivation_is_deterministic_across_fresh_engines() {
    let result_a = dispatch_shell_extract();
    let result_b = dispatch_shell_extract();

    let fid_a = first_face_feature_id(&result_a);
    let fid_b = first_face_feature_id(&result_b);

    assert_eq!(
        fid_a, fid_b,
        "FeatureId derivation must be deterministic across fresh engines \
         (structural equality here implies entity()/index()/Display/content_hash() \
         equality too, so those are not separately asserted)"
    );
}
