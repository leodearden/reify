//! Unified-only OCCT acceptance e2e for curated fillet (η; re-homed from 3205 per PRD D6).
//!
//! Under `BuildScheduler::UnifiedDag` with a real OCCT kernel, a curated
//! `fillet(b, edges_at_height(b, 7.5mm, 0.1mm), 2mm)` must:
//!
//! - Dispatch with **4 curated edges** recorded in the kernel-op stream.
//! - Produce a non-Undef Solid with a **finite** `Scalar<Volume>`.
//! - Produce a volume **distinct** from both the original box and an all-edges fillet
//!   (absolute ε = 1e-10 m³ = 0.1 mm³; real differences are tens of mm³).
//!
//! **Geometry:** `box(w, h, d)` is origin-centered. For `box(20mm, 10mm, 15mm)` the
//! top face is at z = +7.5 mm (= depth/2), so `edges_at_height(b, 7.5mm, 0.1mm)`
//! selects exactly the 4 horizontal top edges. The task's literal `15mm` height on a
//! 15mm-deep box would be above the box top (z = 15mm > 7.5mm) and select 0 edges.
//!
//! **Gate:** `#[cfg_attr(not(feature = "unified-dag"), ignore)]` — curated fillet
//! dispatch and volume-distinctness are unified-only; these assertions fail on the legacy
//! default. Run with:
//! `cargo test -p reify-eval --features unified-dag fillet_curated_edges_3205_e2e`

use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{compile_source, errors_only};

#[test]
#[cfg_attr(not(feature = "unified-dag"), ignore)]
fn fillet_curated_edges_3205_e2e() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping fillet_curated_edges_3205_e2e: OCCT not available");
        return;
    }

    // box(20mm, 10mm, 15mm) is origin-centered → top face at z = +7.5mm.
    // edges_at_height(b, 7.5mm, 0.1mm) selects exactly the 4 horizontal top edges.
    let source = r#"pub structure S {
    let b = box(20mm, 10mm, 15mm)
    let e = edges_at_height(b, 7.5mm, 0.1mm)
    let f = fillet(b, e, 2mm)
    let fall = fillet(b, 2mm)
    let v_box = volume(b)
    let v_cur = volume(f)
    let v_all = volume(fall)
}"#;

    let compiled = compile_source(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no errors, got: {:#?}",
        errors_only(&compiled)
    );

    // Wrap the real OCCT kernel in the transparent RecordingKernel proxy.
    // Capture the ops Arc BEFORE moving the kernel into the engine.
    let recording_kernel =
        RecordingKernel::new(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let ops_ref = recording_kernel.ops_ref();

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(recording_kernel)),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) v_cur resolves to a finite Scalar<Volume> — proves non-Undef Solid.
    let v_cur_si = match result.values.get(&ValueCellId::new("S", "v_cur")) {
        Some(Value::Scalar { si_value, dimension }) => {
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "v_cur should have VOLUME dimension, got {:?}",
                dimension
            );
            assert!(
                si_value.is_finite(),
                "v_cur si_value should be finite, got {}",
                si_value
            );
            *si_value
        }
        other => panic!("expected Value::Scalar for v_cur, got {:?}", other),
    };

    // (b) Exactly one GeometryOp::Fillet with edges.len() == 4 (the curated fillet `f`).
    let ops = ops_ref.lock().unwrap();
    let curated_fillets: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::Fillet { edges, .. } if edges.len() == 4))
        .collect();
    assert_eq!(
        curated_fillets.len(),
        1,
        "expected exactly one Fillet op with 4 curated edges; all Fillet ops recorded: {:?}",
        ops.iter()
            .filter(|op| matches!(op, GeometryOp::Fillet { .. }))
            .collect::<Vec<_>>()
    );
    drop(ops);

    // (c) Non-equality: curated fillet volume differs from box and all-edges fillet
    //     by more than 1e-10 m³ (= 0.1 mm³) absolute — far below real differences
    //     (~tens of mm³) and far above float noise.
    let v_box_si = match result.values.get(&ValueCellId::new("S", "v_box")) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected Value::Scalar for v_box, got {:?}", other),
    };
    let v_all_si = match result.values.get(&ValueCellId::new("S", "v_all")) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected Value::Scalar for v_all, got {:?}", other),
    };

    const EPSILON: f64 = 1e-10; // 0.1 mm³ in m³
    assert!(
        (v_cur_si - v_box_si).abs() > EPSILON,
        "curated fillet volume must differ from box volume: \
         v_cur={v_cur_si:.15e}, v_box={v_box_si:.15e}"
    );
    assert!(
        (v_cur_si - v_all_si).abs() > EPSILON,
        "curated fillet volume must differ from all-edges fillet: \
         v_cur={v_cur_si:.15e}, v_all={v_all_si:.15e}"
    );
}
