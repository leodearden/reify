//! A [`GeometryOp::AffineApply`] / [`GeometryOp::ScaleNonUniform`] result
//! must not depend on whether its source was tessellated first: in either
//! ordering it is BRepCheck-valid and re-tessellates identically, and the
//! transform leaves the source's own mesh alone. The GUI tessellates a handle
//! to display it, so a violation only shows once a body has been displayed.
//!
//! Mechanism, measured on system OCCT 7.8.1: `BRepBuilderAPI_GTransform` has
//! no copy-mesh switch, and `BRepTools_GTrsfModification` overrides
//! `NewTriangulation`/`NewPolygon`/`NewPolygonOnTriangulation`, so a source's
//! mesh is carried onto the result even though every analytic surface is
//! rewritten as a B-spline. `BRepCheck_Analyzer` then rejects the result, and
//! `BRepMesh_IncrementalMesh` reuses the carried mesh at any coarser
//! deflection. The `gp_Trsf` transforms call `BRepBuilderAPI_Transform` with
//! Copy=true and the default `theCopyMesh=false`, so they never carry a mesh;
//! that is why [`GeometryOp::Mirror`] is the control.
//!
//! Found by #6619's probe; fixed in #6652 by dropping the carried mesh in
//! `ffi::gtransform_shape`. This module is the single owner of the ordering
//! invariant. The orthogonal analytic-exactness loss is pinned by
//! `reflection_det_negative_integration` in the separate
//! `harness_occt_measurement` binary, and tracked by #7735.
//!
//! The fixtures were copied from `reflection_det_negative_integration`'s
//! same-named ones, but nothing here relies on the two staying alike.
//! Every fixture goes through [`GeometryOp::Translate`]. A raw
//! `Cylinder`/`Cone` primitive does not reproduce the BRepCheck rejection
//! (measured), while any `BRepBuilderAPI_Transform`-derived copy does: the
//! translation's position is irrelevant, but the Translate itself is
//! load-bearing. Without it the validity test would pass vacuously, with no
//! assertion failing.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Mesh, Value};
use reify_kernel_occt::OcctKernel;

/// Deflection (m) a source is tessellated at before it is transformed.
///
/// It must be much finer than [`RETESSELLATION_DEFLECTION`]: a fresh
/// `cylinder_r6_h20` meshes to the same 100 triangles at 1e-4 as at 1e-3, so
/// a coarser pre-tessellation would leave
/// [`transforming_a_tessellated_source_leaves_the_sources_own_mesh_intact`]
/// unable to tell a kept source mesh from a dropped one.
const PRE_TESSELLATION_DEFLECTION: f64 = 1e-5;

/// Deflection (m) at which every compared mesh is requested.
const RETESSELLATION_DEFLECTION: f64 = 1e-3;

/// A named solid, built into a caller-supplied kernel.
struct Fixture {
    name: &'static str,
    build: fn(&mut OcctKernel) -> GeometryHandleId,
}

static FIXTURES: [Fixture; 2] = [
    Fixture {
        name: "cylinder_r6_h20",
        build: cylinder_r6_h20,
    },
    Fixture {
        name: "cone_r8_r4_h15",
        build: cone_r8_r4_h15,
    },
];

/// A named transform, as the op it applies to a source handle.
struct Transform {
    name: &'static str,
    op: fn(GeometryHandleId) -> GeometryOp,
}

static TRANSFORMS: [Transform; 4] = [
    Transform {
        name: "affine_identity",
        op: |target| GeometryOp::AffineApply {
            target,
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        },
    },
    Transform {
        name: "affine_reflect_x",
        op: |target| GeometryOp::AffineApply {
            target,
            linear: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        },
    },
    Transform {
        name: "scale_non_uniform_x2",
        op: |target| GeometryOp::ScaleNonUniform {
            target,
            sx: 2.0,
            sy: 1.0,
            sz: 1.0,
        },
    },
    Transform {
        name: "mirror_yz",
        op: |target| GeometryOp::Mirror {
            target,
            plane_origin: [0.0; 3],
            plane_normal: [1.0, 0.0, 0.0],
        },
    },
];

/// Whether a source was tessellated before being transformed.
#[derive(Debug, Clone, Copy)]
enum SourceTessellation {
    Never,
    First,
}

/// Every fixture × transform pair.
fn cases() -> impl Iterator<Item = (&'static Fixture, &'static Transform)> {
    FIXTURES
        .iter()
        .flat_map(|fixture| TRANSFORMS.iter().map(move |transform| (fixture, transform)))
}

fn cylinder_r6_h20(kernel: &mut OcctKernel) -> GeometryHandleId {
    let primitive = execute(
        kernel,
        &GeometryOp::Cylinder {
            radius: Value::Real(0.006),
            height: Value::Real(0.020),
        },
    );
    execute(
        kernel,
        &GeometryOp::Translate {
            target: primitive,
            dx: 0.030,
            dy: 0.004,
            dz: 0.0,
        },
    )
}

fn cone_r8_r4_h15(kernel: &mut OcctKernel) -> GeometryHandleId {
    let primitive = execute(
        kernel,
        &GeometryOp::Cone {
            bottom_radius: Value::Real(0.008),
            top_radius: Value::Real(0.004),
            height: Value::Real(0.015),
        },
    );
    execute(
        kernel,
        &GeometryOp::Translate {
            target: primitive,
            dx: 0.030,
            dy: 0.0,
            dz: 0.0,
        },
    )
}

/// Execute `op` and return the new handle, panicking with `op` on failure.
fn execute(kernel: &mut OcctKernel, op: &GeometryOp) -> GeometryHandleId {
    kernel
        .execute(op)
        .unwrap_or_else(|e| panic!("{op:?} should succeed: {e:?}"))
        .id
}

/// Build `fixture` into its own fresh kernel, so that no other case's meshing
/// can reach its source.
fn fresh_source(fixture: &Fixture) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let source = (fixture.build)(&mut kernel);
    (kernel, source)
}

/// Build `fixture` into a fresh kernel, tessellate it first iff `history` is
/// [`SourceTessellation::First`], then apply `transform`. Returns the kernel
/// and the result's handle.
fn transformed(
    fixture: &Fixture,
    transform: &Transform,
    history: SourceTessellation,
) -> (OcctKernel, GeometryHandleId) {
    let (mut kernel, source) = fresh_source(fixture);
    match history {
        SourceTessellation::Never => {}
        SourceTessellation::First => {
            tessellate(&kernel, source, PRE_TESSELLATION_DEFLECTION);
        }
    }
    let result = execute(&mut kernel, &(transform.op)(source));
    (kernel, result)
}

/// Tessellate `id` at `deflection`. OCCT also stores the mesh on the shape
/// itself, where a later coarser request reuses it.
fn tessellate(kernel: &OcctKernel, id: GeometryHandleId, deflection: f64) -> Mesh {
    kernel.tessellate(id, deflection).unwrap_or_else(|e| {
        panic!("tessellating handle {id:?} at deflection {deflection:e} should succeed: {e:?}")
    })
}

fn triangle_count(kernel: &OcctKernel, id: GeometryHandleId, deflection: f64) -> usize {
    tessellate(kernel, id, deflection).indices.len() / 3
}

/// Query whether `id` is watertight, i.e. a BRepCheck-valid closed solid.
fn is_watertight(kernel: &OcctKernel, id: GeometryHandleId) -> bool {
    match kernel.query(&GeometryQuery::IsWatertight(id)) {
        Ok(Value::Bool(b)) => b,
        other => panic!("IsWatertight should return Ok(Bool(_)), got {other:?}"),
    }
}

/// Triangles in `transform`'s result on `fixture`, re-tessellated at
/// [`RETESSELLATION_DEFLECTION`], after the given source history.
fn retessellated_result_triangles(
    fixture: &Fixture,
    transform: &Transform,
    history: SourceTessellation,
) -> usize {
    let (kernel, result) = transformed(fixture, transform, history);
    triangle_count(&kernel, result, RETESSELLATION_DEFLECTION)
}

/// Every result is BRepCheck-valid whatever its source's tessellation history.
#[test]
fn gtransform_result_is_watertight_whether_or_not_its_source_was_tessellated_first() {
    let mut invalid = Vec::new();
    for (fixture, transform) in cases() {
        for history in [SourceTessellation::Never, SourceTessellation::First] {
            let (kernel, result) = transformed(fixture, transform, history);
            if !is_watertight(&kernel, result) {
                invalid.push((fixture.name, transform.name, history));
            }
        }
    }
    assert!(
        invalid.is_empty(),
        "these (fixture, transform, source tessellation) results are not IsWatertight, i.e. \
         BRepCheck rejects them: {invalid:?}"
    );
}

/// Every result re-tessellates to the same triangle count whatever its
/// source's tessellation history. The comparison is exact, not a tuned
/// threshold: once a result carries no mesh, both orderings mesh identical
/// geometry with the same serial mesher.
#[test]
fn gtransform_result_retessellates_identically_whether_or_not_its_source_was_tessellated_first() {
    let mut diverging = Vec::new();
    for (fixture, transform) in cases() {
        let never = retessellated_result_triangles(fixture, transform, SourceTessellation::Never);
        let first = retessellated_result_triangles(fixture, transform, SourceTessellation::First);
        if first != never {
            diverging.push((fixture.name, transform.name, never, first));
        }
    }
    assert!(
        diverging.is_empty(),
        "these (fixture, transform, triangles if never tessellated, triangles if tessellated \
         first) results re-tessellate at {RETESSELLATION_DEFLECTION:e} differently depending on \
         their source's history: {diverging:?}"
    );
}

/// Transforming a tessellated source leaves the source's own mesh on it, so a
/// coarser re-tessellation of the source still returns that finer mesh. This
/// relies on `BRepMesh_IncrementalMesh` keeping an existing finer mesh, the
/// same OCCT policy that makes a result re-tessellate to a carried mesh.
///
/// A scope guard for #6652's fix rather than a regression test for its
/// defect: dropping the mesh from the SOURCE instead of the result also
/// compiles, since a `const TopoDS_Shape&` does not protect the `TShape`
/// behind it, and would strip a displayed body's mesh.
#[test]
fn transforming_a_tessellated_source_leaves_the_sources_own_mesh_intact() {
    let mut disturbed = Vec::new();
    for (fixture, transform) in cases() {
        let (fresh_kernel, fresh) = fresh_source(fixture);
        let fresh_triangles = triangle_count(&fresh_kernel, fresh, RETESSELLATION_DEFLECTION);

        let (mut kernel, source) = fresh_source(fixture);
        let source_triangles = triangle_count(&kernel, source, PRE_TESSELLATION_DEFLECTION);
        assert_ne!(
            source_triangles, fresh_triangles,
            "{}: the source's mesh at {PRE_TESSELLATION_DEFLECTION:e} must differ from a fresh \
             mesh at {RETESSELLATION_DEFLECTION:e}, otherwise this test cannot observe a dropped \
             source mesh",
            fixture.name
        );

        execute(&mut kernel, &(transform.op)(source));
        let retessellated = triangle_count(&kernel, source, RETESSELLATION_DEFLECTION);
        if retessellated != source_triangles {
            disturbed.push((
                fixture.name,
                transform.name,
                source_triangles,
                retessellated,
            ));
        }
    }
    assert!(
        disturbed.is_empty(),
        "these (fixture, transform, source triangles before, source triangles after) transforms \
         dropped their source's own mesh: {disturbed:?}"
    );
}
