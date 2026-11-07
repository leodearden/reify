//! `Engine::measure_thickness_pair`'s D5 degradation arms.
//!
//! Every path that cannot produce an honest field (no OpenVDB kernel, a field
//! the extractors reject, a field without exactly three axis spacings) must
//! yield `(None, None)`: never a panic, never a fabricated number.
//!
//! `MockGeometryKernel` is the tessellating source kernel, and
//! `DensifyToSpacingKernel` stands in for OpenVDB, handing back a field with
//! the chosen spacing. Every early exit in the realize chain also yields
//! `(None, None)`, so a test that means to exercise the field itself also
//! asserts that the stand-in densified. Only then can its `(None, None)` come
//! from the spacing guard or the extractors.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reify_core::identity::RealizationNodeId;
use reify_ir::GeometryHandleId;
use reify_ir::value::GeometryHandleRef;
use reify_test_support::mocks::{MockConstraintChecker, MockGeometryKernel};

use crate::Engine;

struct DensifyToSpacingKernel {
    spacing: Vec<f64>,
    densified: Arc<AtomicBool>,
}

impl reify_ir::GeometryKernel for DensifyToSpacingKernel {
    fn execute(
        &mut self,
        _op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
    }
    fn query(&self, _q: &reify_ir::GeometryQuery) -> Result<reify_ir::Value, reify_ir::QueryError> {
        unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
    }
    fn export(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _format: reify_ir::ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
    }
    fn tessellate(
        &self,
        _handle: reify_ir::GeometryHandleId,
        _tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
    }
    fn ingest_mesh(
        &mut self,
        _mesh: &reify_ir::Mesh,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        Ok(reify_ir::GeometryHandle {
            id: GeometryHandleId(42),
            repr: None,
        })
    }
    fn densify_grid_to_sampled(
        &mut self,
        _handle: reify_ir::GeometryHandleId,
    ) -> Result<reify_ir::SampledField, reify_ir::QueryError> {
        self.densified.store(true, Ordering::SeqCst);
        Ok(reify_ir::SampledField {
            name: "thickness-pair-stub".into(),
            kind: reify_ir::SampledGridKind::Regular3D,
            bounds_min: vec![0.0; 3],
            bounds_max: vec![0.0; 3],
            spacing: self.spacing.clone(),
            axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
            interpolation: reify_ir::InterpolationKind::Linear,
            data: vec![0.0],
            oob_emitted: AtomicBool::new(false),
        })
    }
}

/// An engine with a tessellating source kernel and a resolvable subject.
/// `Some(spacing)` also registers the OpenVDB stand-in, whose densified
/// field carries that spacing; `None` leaves OpenVDB absent. The returned
/// flag turns true once the stand-in has densified.
fn engine_and_subject(
    openvdb_spacing: Option<Vec<f64>>,
) -> (Engine, GeometryHandleRef, Arc<AtomicBool>) {
    let mut engine = Engine::new(
        crate::SolverPosture::SolverFree,
        Box::new(MockConstraintChecker::new()),
        None,
    );
    engine
        .geometry_kernels
        .insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
    engine.default_kernel_name = Some("occt".to_string());
    let densified = Arc::new(AtomicBool::new(false));
    if let Some(spacing) = openvdb_spacing {
        engine.geometry_kernels.insert(
            crate::kernel_registry::openvdb_kernel_name().to_string(),
            Box::new(DensifyToSpacingKernel {
                spacing,
                densified: Arc::clone(&densified),
            }),
        );
    }

    let realization_ref = RealizationNodeId::new("thickness-pair", 0);
    engine
        .realization_handles
        .insert(realization_ref.clone(), GeometryHandleId(1));
    let subject = GeometryHandleRef {
        realization_ref,
        upstream_values_hash: [0u8; 32],
        kernel_handle: Some(GeometryHandleId(1)),
    };
    (engine, subject, densified)
}

#[test]
fn thickness_pair_without_openvdb_kernel_is_none_none() {
    let (mut engine, subject, _) = engine_and_subject(None);
    assert_eq!(
        engine.measure_thickness_pair(subject),
        (None, None),
        "no OpenVDB kernel"
    );
}

#[test]
fn thickness_pair_zero_spacing_field_is_none_none() {
    let (mut engine, subject, densified) = engine_and_subject(Some(vec![0.0, 0.0, 0.0]));
    assert_eq!(
        engine.measure_thickness_pair(subject),
        (None, None),
        "zero-spacing field: both extractors return Err"
    );
    assert!(
        densified.load(Ordering::SeqCst),
        "zero-spacing field: the realize chain exited before densifying"
    );
}

#[test]
fn thickness_pair_field_without_three_spacings_is_none_none() {
    for spacing in [vec![], vec![1.0, 1.0]] {
        let case = format!("spacing {spacing:?}");
        let (mut engine, subject, densified) = engine_and_subject(Some(spacing));
        assert_eq!(
            engine.measure_thickness_pair(subject),
            (None, None),
            "{case}"
        );
        assert!(
            densified.load(Ordering::SeqCst),
            "{case}: the realize chain exited before densifying"
        );
    }
}
