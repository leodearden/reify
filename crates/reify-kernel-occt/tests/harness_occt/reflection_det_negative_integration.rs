//! A-ε probe (task #6619, PRD `docs/prds/v0_6/assembly-derivation-toolbox.md`
//! leaf A-ε, boundary test T17): placeholder header. Replaced with the full
//! finding write-up in the doc-only step that lands last in this module's
//! plan.

#![cfg(has_occt)]

use reify_ir::{ExportFormat, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

// ---------------------------------------------------------------------------
// Test 1 (steps 1-2) — det<0 reflection yields a valid, positive-volume solid
// ---------------------------------------------------------------------------

/// For each of three convex, primitive-derived fixtures, reflect across the
/// x=0 plane by BOTH lowerings — [`GeometryOp::Mirror`] (`gp_Trsf::SetMirror`)
/// and [`GeometryOp::AffineApply`] with `linear = diag(-1,1,1)`
/// (`gp_GTrsf` / `BRepBuilderAPI_GTransform`) — and assert that both paths
/// yield a BRepCheck-valid, positive-volume solid whose volume matches the
/// source within a path-specific tolerance.
///
/// RED: `convex_fixtures`, `mirror_across_yz`, `affine_reflect_x`,
/// `volume_of` and `flag_of` do not exist yet, so this fails to COMPILE
/// until step-2 adds them.
///
/// Fixtures are deliberately primitive-derived (never a boolean result: a
/// boolean returns a COMPOUND, and `IsWatertight` hard-returns `false` for
/// any non-SOLID/COMPSOLID/SHELL shape regardless of validity) and are NOT
/// tessellated before reflecting here — pre-tessellation ordering is step-7's
/// subject, and tessellating in this test would contaminate its answer.
///
/// Tolerances: `Mirror` is bit-exact against the source (measured identical
/// to the last printed digit, e.g. cylinder 2.261946710585e-6 m³ both
/// sides), so 1e-12 relative is used. `AffineApply` det<0 rewrites every
/// analytic surface as a B-spline approximation; measured worst-case drift
/// is +8.615e-3 relative (cylinder) and +8.250e-3 (cone), so 2e-2 relative
/// (≈2.3× the measured worst case) is used — tightening this to, say, 1e-9
/// would be a doomed assertion.
#[test]
fn both_reflection_paths_yield_valid_positive_volume_solids() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    for (name, source) in convex_fixtures(&mut kernel) {
        let source_volume = volume_of(&kernel, source);
        assert!(
            source_volume > 0.0,
            "{name}: source fixture should itself have positive volume, got {source_volume:e}"
        );

        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        for (path, target, tol) in [("Mirror", mirrored, 1e-12), ("AffineApply", affine, 2e-2)] {
            // (b) positive volume under reflection.
            let v = volume_of(&kernel, target);
            assert!(
                v > 0.0,
                "{name} via {path}: reflected volume must be positive, got {v:e}"
            );

            // (c)/(d) volume matches source within the path-specific tolerance.
            let rel_err = (v - source_volume).abs() / source_volume;
            assert!(
                rel_err < tol,
                "{name} via {path}: reflected volume {v:e} should match source {source_volume:e} \
                 within {tol:e} relative, got rel_err={rel_err:e}"
            );

            // (e) BRepCheck validity survives reflection.
            for (flag_name, query) in [
                ("IsWatertight", GeometryQuery::IsWatertight(target)),
                ("IsManifold", GeometryQuery::IsManifold(target)),
                ("IsOrientable", GeometryQuery::IsOrientable(target)),
                ("IsClosed", GeometryQuery::IsClosed(target)),
            ] {
                assert!(
                    flag_of(&kernel, query),
                    "{name} via {path}: {flag_name} should be true after det<0 reflection"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (step-2) — kept PRIVATE to this module: they encode this
// probe's fixture contract, not a crate-wide idiom. `tests/common/mod.rs` is
// reserved for helpers duplicated across MANY modules (see its header).
// ---------------------------------------------------------------------------

/// Build the three convex, primitive-derived fixtures this module's tests
/// share: a box, a cylinder and a cone, each translated wholly into x>0.
///
/// Two constraints future editors must not break:
///   - **Primitive-derived only, never a boolean result.** `BRepAlgoAPI_Cut`
///     (and Union/Intersection) return a `COMPOUND`, and `IsWatertight`
///     (`occt_wrapper.cpp`) hard-returns `false` for any shape that is not
///     SOLID/COMPSOLID/SHELL — regardless of actual validity. A boolean-
///     derived fixture would make this module's validity assertions
///     unsatisfiable.
///   - **Positioned wholly at x>0.** step-5's baked-geometry STEP assertion
///     reads a negative leading X coordinate in an exported `CARTESIAN_POINT`
///     entity as the reflection signal; a fixture straddling or left of x=0
///     would make that signal ambiguous.
///
/// All three are additionally CONVEX, which is what licenses the AABB-centre
/// reference direction in the outward-winding tessellation check (steps 3-4)
/// — a concave fixture can legitimately have inward-pointing dot products in
/// its concave regions (measured: 343 outward / 253 inward on a box-minus-
/// cylinder-minus-sphere part), which would make that assertion meaningless.
fn convex_fixtures(kernel: &mut OcctKernel) -> Vec<(&'static str, GeometryHandleId)> {
    let box_src = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.010),
            height: Value::Real(0.020),
            depth: Value::Real(0.030),
        })
        .expect("box_10x20x30 should build");
    let box_id = kernel
        .execute(&GeometryOp::Translate {
            target: box_src.id,
            dx: 0.050,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_10x20x30 translate to x>0 should succeed")
        .id;

    let cyl_src = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.006),
            height: Value::Real(0.020),
        })
        .expect("cylinder_r6_h20 should build");
    let cyl_id = kernel
        .execute(&GeometryOp::Translate {
            target: cyl_src.id,
            dx: 0.030,
            dy: 0.004,
            dz: 0.0,
        })
        .expect("cylinder_r6_h20 translate to x>0 should succeed")
        .id;

    let cone_src = kernel
        .execute(&GeometryOp::Cone {
            bottom_radius: Value::Real(0.008),
            top_radius: Value::Real(0.004),
            height: Value::Real(0.015),
        })
        .expect("cone_r8_r4_h15 should build");
    let cone_id = kernel
        .execute(&GeometryOp::Translate {
            target: cone_src.id,
            dx: 0.030,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("cone_r8_r4_h15 translate to x>0 should succeed")
        .id;

    vec![
        ("box_10x20x30", box_id),
        ("cylinder_r6_h20", cyl_id),
        ("cone_r8_r4_h15", cone_id),
    ]
}

/// Mirror `target` across the x=0 (y-z) plane via [`GeometryOp::Mirror`]
/// (`gp_Trsf::SetMirror`) — the v1 reflective-derivation lowering PRD §3.7
/// names, and the path this module's probe finds bit-exact and immune to
/// both GTransform hazards (step-7).
fn mirror_across_yz(kernel: &mut OcctKernel, target: GeometryHandleId) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Mirror {
            target,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1.0, 0.0, 0.0],
        })
        .expect("Mirror across the x=0 plane should succeed for a det<0 reflection")
        .id
}

/// Apply the general dense 3×3 linear map `linear` (zero translation) to
/// `target` via [`GeometryOp::AffineApply`] (`gp_GTrsf` /
/// `BRepBuilderAPI_GTransform`). [`affine_reflect_x`] delegates here with
/// `diag(-1,1,1)`; step-7 reuses this general form directly with the
/// IDENTITY `diag(1,1,1)` to prove its two GTransform hazards are
/// determinant-independent rather than reflection artifacts.
fn affine_linear(
    kernel: &mut OcctKernel,
    target: GeometryHandleId,
    linear: [[f64; 3]; 3],
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::AffineApply {
            target,
            linear,
            translation: [0.0, 0.0, 0.0],
        })
        .unwrap_or_else(|e| {
            panic!(
                "AffineApply({linear:?}) on handle {target:?} should succeed: neither the Rust \
                 finiteness guard nor the C++ Hadamard singularity guard should reject a \
                 det<0 (or det=1 identity) linear map, got {e:?}"
            )
        })
        .id
}

/// Reflect `target` across the x=0 (y-z) plane via [`GeometryOp::AffineApply`]
/// with `linear = diag(-1,1,1)` (det = -1) — the general `gp_GTrsf` /
/// `BRepBuilderAPI_GTransform` path, contrasted against [`mirror_across_yz`]'s
/// dedicated `gp_Trsf::SetMirror` path.
fn affine_reflect_x(kernel: &mut OcctKernel, target: GeometryHandleId) -> GeometryHandleId {
    affine_linear(
        kernel,
        target,
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    )
}

/// Query the volume of `id` in m³, panicking (naming the received `Value`
/// variant) if the kernel returns anything other than a numeric value.
///
/// Mirrors the strict `Value`-unwrapping convention of `tests/common/mod.rs`
/// (`parse_bbox`/`xyz_of`): a mismatched shape panics loudly rather than
/// silently defaulting, so a malformed kernel response surfaces as a parse
/// failure rather than a confusing downstream geometry assertion.
fn volume_of(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    let value = kernel
        .query(&GeometryQuery::Volume(id))
        .unwrap_or_else(|e| panic!("Volume query on handle {id:?} should succeed: {e:?}"));
    value.as_f64().unwrap_or_else(|| {
        panic!("Volume query on handle {id:?} should be numeric, got {value:?}")
    })
}

/// Evaluate a boolean [`GeometryQuery`] (e.g. `IsWatertight`, `IsManifold`),
/// panicking (naming the received `Value` variant) if the kernel returns
/// anything other than `Value::Bool`. Mirrors [`volume_of`]'s strictness
/// convention.
fn flag_of(kernel: &OcctKernel, query: GeometryQuery) -> bool {
    let value = kernel
        .query(&query)
        .unwrap_or_else(|e| panic!("{query:?} should succeed: {e:?}"));
    match value {
        Value::Bool(b) => b,
        other => panic!("{query:?} should return Value::Bool, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 2 (steps 3-4) — det<0 reflection tessellates to an outward-wound,
// closed, orientable manifold. THE CORE of T17: this is the assertion that
// actually observes det<0 output orientation.
// ---------------------------------------------------------------------------

/// For each convex fixture and each of the two reflection paths, tessellate
/// at 1e-4 m (0.1 mm) deflection and assert the result is a closed,
/// consistently OUTWARD-wound manifold whose supplied normals agree with
/// that winding.
///
/// RED: `assert_outward_wound_closed_manifold` does not exist yet, so this
/// fails to COMPILE until step-4 adds it.
///
/// **Why the outward-winding check (obligation (b) on
/// [`assert_outward_wound_closed_manifold`]) is load-bearing, and
/// `mesh.validate(0.0)` (obligation (a)) is NOT sufficient on its own:**
/// `Mesh::validate`'s Closed + ConsistentWinding obligations are a directed-
/// edge invariant on the position-welded quotient topology, which a
/// CONSISTENTLY INWARD mesh satisfies exactly as well as an outward one. If
/// OCCT ever stopped reversing face orientation flags under a det<0
/// transform (measured today: a 3 FORWARD/5 REVERSED box flips to 5
/// FORWARD/3 REVERSED under reflection), every triangle of the reflected
/// solid would flip to inward winding and `validate()` would still pass —
/// exactly the defect T17 exists to catch would slip through silently. Only
/// the per-triangle dot product against the AABB-centre reference direction
/// actually observes it, which is exactly what would fail loudly if that
/// regression ever happened.
///
/// **Convexity precondition**: all three fixtures are convex (documented on
/// [`convex_fixtures`]), which is what licenses using the AABB centre as the
/// "outward" reference direction. A non-convex fixture can legitimately
/// produce inward-pointing dot products in its concave regions — measured
/// 343 outward / 253 inward triangles on a box-minus-cylinder-minus-sphere
/// part, which is correct behaviour for a concave part, not a defect. Do NOT
/// add a non-convex fixture to this test; it would make the assertion
/// meaningless rather than stricter.
#[test]
fn both_reflection_paths_tessellate_to_outward_wound_closed_manifold() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    for (name, source) in convex_fixtures(&mut kernel) {
        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        for (path, target) in [("Mirror", mirrored), ("AffineApply", affine)] {
            let mesh = kernel.tessellate(target, 1e-4).unwrap_or_else(|e| {
                panic!("{name} via {path}: tessellate(1e-4) should succeed: {e:?}")
            });
            assert_outward_wound_closed_manifold(&mesh, &format!("{name} via {path}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Tessellation-orientation helpers (step-4)
// ---------------------------------------------------------------------------

/// Compute the geometric normal of triangle (pa, pb, pc) from the emitted
/// winding order: AB × AC. All inputs and the result are in f64.
///
/// Verbatim-shaped reuse of the same-named helper in
/// `tessellation_winding_integration.rs`. Intentionally duplicated rather
/// than hoisted to `tests/common/mod.rs` (reserved for helpers duplicated
/// across MANY modules — see its header): this one is shared by exactly two
/// sibling submodules of the same compile unit, and keeping the name
/// identical documents the kinship for a reader rather than hiding it.
fn tri_winding_normal(pa: [f64; 3], pb: [f64; 3], pc: [f64; 3]) -> [f64; 3] {
    let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let ac = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

/// Per-axis (min+max)/2 over `verts` — the AABB-centre reference direction
/// used by [`assert_outward_wound_closed_manifold`]'s outward-winding check.
/// Robust to non-uniform vertex density across faces, unlike a vertex-cloud
/// mean (same rationale as `tessellation_winding_integration.rs`'s
/// `box_centroid`).
fn aabb_centre(verts: &[[f32; 3]]) -> [f64; 3] {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for v in verts {
        for k in 0..3 {
            let coord = v[k] as f64;
            if coord < min[k] {
                min[k] = coord;
            }
            if coord > max[k] {
                max[k] = coord;
            }
        }
    }
    [
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    ]
}

/// Assert that `mesh` is a closed, orientable manifold whose triangles are
/// ALL outward-wound and whose supplied per-vertex normals agree with that
/// winding — the full T17 core obligation ([`both_reflection_paths_tessellate_to_outward_wound_closed_manifold`]'s
/// doc comment explains why obligation (b) below is the load-bearing one and
/// (a) alone is not sufficient).
///
/// `what` is interpolated into every panic message (e.g.
/// `"cylinder_r6_h20 via AffineApply"`) so a failure identifies which
/// fixture × path regressed. `#[track_caller]` so a failure points at the
/// calling test line, matching this crate's `assert_aabb_eq` /
/// `assert_records_in_range` convention.
///
/// Obligations, verbatim-shaped from `tessellation_winding_integration.rs`'s
/// two tests (this module's convex reflected fixtures are the subject; that
/// module's plain OCCT box is the precedent this reuses):
///
///   (a) `mesh.validate(0.0)` is `Ok` — the INV-GEO-1 mesh contract (Closed +
///       ConsistentWinding + NonDegenerate on the position-welded quotient
///       topology). `tol = 0.0`: this asserts real OCCT tessellation output,
///       not a distance-tolerant approximation.
///   (b) EVERY triangle is OUTWARD-wound: over the welded canonical
///       positions, the winding normal `AB × AC` must have a strictly
///       positive dot product with (triangle centroid − AABB centre).
///   (c) EVERY triangle's averaged supplied per-vertex normal (over the RAW
///       unwelded indices/vertices) agrees (dot > 0) with its raw winding
///       normal.
#[track_caller]
fn assert_outward_wound_closed_manifold(mesh: &reify_ir::Mesh, what: &str) {
    // (a) INV-GEO-1 mesh contract.
    mesh.validate(0.0).unwrap_or_else(|e| {
        panic!("{what}: mesh.validate(0.0) should succeed (INV-GEO-1 contract), got {e:?}")
    });

    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "{what}: index count must be a multiple of 3"
    );
    let num_tris = mesh.indices.len() / 3;

    // (b) Outward winding over the welded canonical positions.
    let (canon_verts, welded) = mesh.weld_positions();
    let centre = aabb_centre(&canon_verts);

    for t in 0..num_tris {
        let ia = welded[mesh.indices[t * 3] as usize] as usize;
        let ib = welded[mesh.indices[t * 3 + 1] as usize] as usize;
        let ic = welded[mesh.indices[t * 3 + 2] as usize] as usize;
        let pa = canon_verts[ia];
        let pb = canon_verts[ib];
        let pc = canon_verts[ic];

        let pa_f64 = [pa[0] as f64, pa[1] as f64, pa[2] as f64];
        let pb_f64 = [pb[0] as f64, pb[1] as f64, pb[2] as f64];
        let pc_f64 = [pc[0] as f64, pc[1] as f64, pc[2] as f64];
        let normal = tri_winding_normal(pa_f64, pb_f64, pc_f64);

        let tri_centroid = [
            (pa_f64[0] + pb_f64[0] + pc_f64[0]) / 3.0,
            (pa_f64[1] + pb_f64[1] + pc_f64[1]) / 3.0,
            (pa_f64[2] + pb_f64[2] + pc_f64[2]) / 3.0,
        ];
        let outward = [
            tri_centroid[0] - centre[0],
            tri_centroid[1] - centre[1],
            tri_centroid[2] - centre[2],
        ];
        let dot = normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];

        assert!(
            dot > 0.0,
            "{what}: triangle {t} (welded verts {ia},{ib},{ic}) geometric normal from emitted \
             winding points inward (dot = {dot:.6}); every triangle must be outward-wound"
        );
    }

    // (c) Supplied normals agree with the RAW (unwelded) winding.
    let supplied = mesh
        .normals
        .as_ref()
        .unwrap_or_else(|| panic!("{what}: tessellate should emit per-vertex normals"));
    assert_eq!(
        supplied.len(),
        mesh.vertices.len(),
        "{what}: normals array must have same length as vertices array"
    );

    for t in 0..num_tris {
        let i0 = mesh.indices[t * 3] as usize;
        let i1 = mesh.indices[t * 3 + 1] as usize;
        let i2 = mesh.indices[t * 3 + 2] as usize;

        let pa = [
            mesh.vertices[i0 * 3] as f64,
            mesh.vertices[i0 * 3 + 1] as f64,
            mesh.vertices[i0 * 3 + 2] as f64,
        ];
        let pb = [
            mesh.vertices[i1 * 3] as f64,
            mesh.vertices[i1 * 3 + 1] as f64,
            mesh.vertices[i1 * 3 + 2] as f64,
        ];
        let pc = [
            mesh.vertices[i2 * 3] as f64,
            mesh.vertices[i2 * 3 + 1] as f64,
            mesh.vertices[i2 * 3 + 2] as f64,
        ];
        let winding_normal = tri_winding_normal(pa, pb, pc);

        let avg_supplied = [
            (supplied[i0 * 3] as f64 + supplied[i1 * 3] as f64 + supplied[i2 * 3] as f64) / 3.0,
            (supplied[i0 * 3 + 1] as f64
                + supplied[i1 * 3 + 1] as f64
                + supplied[i2 * 3 + 1] as f64)
                / 3.0,
            (supplied[i0 * 3 + 2] as f64
                + supplied[i1 * 3 + 2] as f64
                + supplied[i2 * 3 + 2] as f64)
                / 3.0,
        ];

        let dot = winding_normal[0] * avg_supplied[0]
            + winding_normal[1] * avg_supplied[1]
            + winding_normal[2] * avg_supplied[2];

        assert!(
            dot > 0.0,
            "{what}: triangle {t} (raw verts {i0},{i1},{i2}) supplied normals (avg \
             [{:.4},{:.4},{:.4}]) disagree with the geometric winding normal (dot = {dot:.6}); \
             supplied normals must agree with the outward-wound triangles",
            avg_supplied[0],
            avg_supplied[1],
            avg_supplied[2],
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3 (steps 5-6) — reflected B-rep STEP export: baked geometry, no
// det<0 placement.
// ---------------------------------------------------------------------------

/// The STEP-writer half of T17: for each convex fixture, export the SOURCE
/// and BOTH reflections and assert the writer emits a valid det=+1 assembly
/// from a reflected B-rep, with the impropriety BAKED into geometry rather
/// than placed (PRD §3.8: "baked reflected B-reps + det=+1 placements are
/// AP242-conformant by construction"). The FreeCAD mirrored-bodies-vanish
/// defect is the cautionary tale this test rules out.
///
/// RED: `step_entity_count` does not exist yet, so this fails to COMPILE
/// until step-6 adds it.
///
/// `src/lib.rs`'s existing `new_ops_export_step` unit test already exports a
/// mirrored box, but asserts only that the file contains `ISO-10303-21` —
/// cited here as the shallow precedent this module deepens, not duplicated.
///
/// The workspace has NO STEP reader anywhere (no `STEPControl_Reader` /
/// `STEPCAFControl_Reader`), so textual assertion over the exported content
/// is the only verification available — and, per the five obligations below,
/// sufficient.
#[test]
fn reflected_brep_step_export_bakes_geometry_and_emits_no_det_negative_placement() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    // Local export-to-text helper (not a module-level fn: `kernel.export` and
    // `String::from_utf8` are real, already-compiling APIs, so wrapping them
    // in a closure here doesn't change what makes this step RED).
    let export_step_text = |kernel: &OcctKernel, id: GeometryHandleId| -> String {
        let mut buf = Vec::<u8>::new();
        kernel
            .export(id, ExportFormat::Step, &mut buf)
            .unwrap_or_else(|e| panic!("STEP export of handle {id:?} should succeed: {e:?}"));
        String::from_utf8(buf)
            .unwrap_or_else(|e| panic!("STEP export of handle {id:?} should be valid UTF-8: {e}"))
    };

    for (name, source) in convex_fixtures(&mut kernel) {
        let mirrored = mirror_across_yz(&mut kernel, source);
        let affine = affine_reflect_x(&mut kernel, source);

        let source_text = export_step_text(&kernel, source);

        // (a) well-formed STEP framing on the source export.
        assert!(
            source_text.contains("ISO-10303-21") && source_text.contains("END-ISO-10303-21"),
            "{name} source: STEP export should contain ISO-10303-21/END-ISO-10303-21 framing"
        );
        // Baseline for (c): the source's own ADVANCED_FACE count.
        let source_faces = step_entity_count(&source_text, "ADVANCED_FACE");
        // (e) baked-geometry negative control: the source lives wholly at x>0,
        // so it must carry no negative-leading-X CARTESIAN_POINT entity.
        assert_eq!(
            step_entity_count(&source_text, "CARTESIAN_POINT('',(-"),
            0,
            "{name} source: wholly-x>0 fixture should export NO negative-leading-X \
             CARTESIAN_POINT entities"
        );
        // (d) det=+1 by construction on the source too.
        assert_eq!(
            step_entity_count(&source_text, "CARTESIAN_TRANSFORMATION_OPERATOR"),
            0,
            "{name} source: writer should emit zero CARTESIAN_TRANSFORMATION_OPERATOR \
             entities — every placement is an AXIS2_PLACEMENT_3D (y derived as z×x, \
             right-handed by construction), so counting zero is equivalent to det=+1"
        );

        for (path, target) in [("Mirror", mirrored), ("AffineApply", affine)] {
            let text = export_step_text(&kernel, target);

            // (a) well-formed STEP framing; a truncated/failed write is not
            // silently accepted.
            assert!(
                text.contains("ISO-10303-21") && text.contains("END-ISO-10303-21"),
                "{name} via {path}: STEP export should contain ISO-10303-21/END-ISO-10303-21 \
                 framing"
            );

            // (b) the mirrored solid did NOT vanish (the FreeCAD cautionary tale).
            assert!(
                step_entity_count(&text, "MANIFOLD_SOLID_BREP") >= 1,
                "{name} via {path}: reflected export should contain >=1 MANIFOLD_SOLID_BREP \
                 — the mirrored solid must not vanish"
            );

            // (c) no face dropped by the reflection.
            assert_eq!(
                step_entity_count(&text, "ADVANCED_FACE"),
                source_faces,
                "{name} via {path}: reflected ADVANCED_FACE count should equal the source's \
                 ({source_faces}) — no face should be dropped by the reflection"
            );

            // (d) det=+1 assertion: the writer emits no transformation operator at
            // all, so every placement is an AXIS2_PLACEMENT_3D, whose y-axis is
            // DERIVED as z×x and therefore cannot encode a left-handed frame —
            // det=+1 holds by construction, not by numeric check. Substring-match
            // (not an exact-token match) so the `_3D`-suffixed spelling is covered.
            assert_eq!(
                step_entity_count(&text, "CARTESIAN_TRANSFORMATION_OPERATOR"),
                0,
                "{name} via {path}: writer should emit zero CARTESIAN_TRANSFORMATION_OPERATOR \
                 entities — every placement is an AXIS2_PLACEMENT_3D (y derived as z×x, \
                 right-handed by construction and therefore unable to encode a left-handed \
                 frame), so counting zero is equivalent to asserting every placement has \
                 det=+1, with no float comparison"
            );

            // (e) geometry is BAKED, not placed: the reflected export carries >=1
            // negative-leading-X CARTESIAN_POINT while the wholly-x>0 source
            // carries exactly 0 — so a negative leading coordinate can only come
            // from baked mirrored geometry.
            assert!(
                step_entity_count(&text, "CARTESIAN_POINT('',(-") >= 1,
                "{name} via {path}: reflected export should contain >=1 negative-leading-X \
                 CARTESIAN_POINT entity (baked mirrored geometry)"
            );
        }
    }
}
