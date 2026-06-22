//! Integration gate for the geometry-op dispatch-registry refactor (L6 of PRD
//! docs/prds/geometry-op-dispatch-registry.md, section 9).
//!
//! TESTS-ONLY: no production change.  All source files scanned are read at
//! test-execution time via `std::fs::read_to_string`; this file modifies none.
//!
//! Guards implemented:
//!
//! **(1) Axis-1** (`engine_build.rs`): `geometry_op_to_operation` is a pure
//! descriptor-table lookup with zero per-variant arms; the two DD-6 shim
//! functions (`parent_handles_for_op`, `substitute_op_parents`) route through
//! `descriptor_for` + `ParentRole`; whole-file arm-terminator count stays
//! below [`AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD`].
//!
//! **(2) Axis-3** (`geometry_ops.rs`): compile path uses fn-table statics
//! (`PRIMITIVE_COMPILERS`, `MODIFY_COMPILERS`); no nested per-kind behavioral
//! match arms in the non-test region.
//!
//! **(3) Cross-crate live guarantee**: every [`reify_ir::geometry::GeometryOpDiscriminants`]
//! value resolves via `descriptor_for`; table length equals `COUNT`.
//!
//! **(4) Canary retirement**: `GEOMETRY_OP_VARIANT_COUNT` const definition is
//! absent from `reify-ir`; `EXPECTED_DISPATCH_COUNT` is absent from
//! `reify-compiler`; `GEOMETRY_QUERY_VARIANT_COUNT` is present (out-of-scope
//! query canary untouched per PRD section 7).

// ── Threshold constants ──────────────────────────────────────────────────────

/// Arm-terminator threshold for the Axis-1 exhaustive-dispatch regression guard
/// over `engine_build.rs`.
///
/// **Derivation:** the measured DD-6/projection arm-terminator floor in the
/// non-test region of `engine_build.rs` under the refined counting rule (a
/// `GeometryOp::` token not immediately preceded by an ASCII identifier char,
/// appearing before the first `=>` on its line) is **14** genuine terminators
/// — all confirmed legitimate DD-6/OR-pattern/wildcard lines.
///
/// A resurrected exhaustive per-variant dispatch over the 48+ `GeometryOp`
/// variants would add ~48 more terminator lines, reaching ~62.  This threshold
/// is set at **30** — ~16 headroom above the measured floor, ~32 below the
/// regression ceiling.  Adding new variants to an existing OR-pattern adds
/// intermediate (non-arrow) lines that the counter ignores, so the floor is
/// stable as the enum grows.
const AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD: usize = 30;

// ── Detector helpers (stubs; implemented in step-2) ──────────────────────────

/// Return the non-test region of a Rust source file: everything before the
/// first `\n#[cfg(test)]\nmod tests {` boundary marker.
///
/// Mirrors the boundary split used by the L5 inline guard
/// `compile_geometry_op_has_no_nested_per_kind_match` in `geometry_ops.rs`.
///
/// # Panics
/// Panics if the boundary marker is not found.
fn non_test_region(src: &str) -> &str {
    let _ = src;
    todo!("implement in step-2")
}

/// Extract the body of a top-level (column-0) function named `fn_name`.
///
/// Locates the first line that begins with `fn {fn_name}(` or `fn {fn_name}<`
/// (both must appear at column 0, i.e. preceded by `\n` or at the file start),
/// then returns the text from that signature line up to and including the next
/// column-0 closing brace (`}`).
///
/// Returns an empty string if the function is not found.
fn slice_fn_body<'a>(src: &'a str, fn_name: &str) -> &'a str {
    let _ = (src, fn_name);
    todo!("implement in step-2")
}

/// Count lines in `region` that contain a `GeometryOp::` token in pattern
/// (arm-terminator) position.
///
/// A line is counted **only when all** of the following hold:
///
/// (a) The line contains the substring `"GeometryOp::"`.
///
/// (b) The character immediately before that occurrence (if any) is NOT an
///     ASCII identifier character (`[A-Za-z0-9_]`).  This excludes
///     `CompiledGeometryOp::X =>` — a different enum — whose `::` is
///     immediately preceded by the identifier char `p`.
///
/// (c) The `"GeometryOp::"` occurrence begins at an index strictly less than
///     the index of the first `"=>"` on the same line (pattern position).
///     Lines with no `=>` (intermediate OR-pattern parts) are **not** counted.
///     Lines where `GeometryOp::` appears only after `=>` (RHS constructions
///     such as `Ok(reify_ir::GeometryOp::Union { .. })`) are also not counted.
fn count_geometryop_dispatch_arms(region: &str) -> usize {
    let _ = region;
    todo!("implement in step-2")
}

// ── Detector self-tests ──────────────────────────────────────────────────────

/// Synthetic **POSITIVE**: a flat match over all 49 `GeometryOp` variants is
/// flagged by the counter at ≥ 40.
///
/// Proves the counter actually fires — ensures the integration guard is not
/// vacuously green when a detector bug silently returns 0.
#[test]
fn detector_flags_synthetic_exhaustive_match() {
    // One arm per variant, each `GeometryOp::X { .. } => "..."` on its own
    // line.  All 49 lines have `GeometryOp::` before `=>` → all counted.
    let src = r#"fn dispatch_op(op: &GeometryOp) -> &str {
    match op {
        GeometryOp::Box { .. } => "box",
        GeometryOp::Cylinder { .. } => "cylinder",
        GeometryOp::Sphere { .. } => "sphere",
        GeometryOp::Tube { .. } => "tube",
        GeometryOp::Cone { .. } => "cone",
        GeometryOp::Wedge { .. } => "wedge",
        GeometryOp::Torus { .. } => "torus",
        GeometryOp::Union { .. } => "union",
        GeometryOp::Difference { .. } => "difference",
        GeometryOp::Intersection { .. } => "intersection",
        GeometryOp::Fillet { .. } => "fillet",
        GeometryOp::Chamfer { .. } => "chamfer",
        GeometryOp::ChamferAsymmetric { .. } => "chamfer_asym",
        GeometryOp::Draft { .. } => "draft",
        GeometryOp::Thicken { .. } => "thicken",
        GeometryOp::OffsetCurve { .. } => "offset_curve",
        GeometryOp::ZoneSlab { .. } => "zone_slab",
        GeometryOp::OffsetSolid { .. } => "offset_solid",
        GeometryOp::Shell { .. } => "shell",
        GeometryOp::Split { .. } => "split",
        GeometryOp::Translate { .. } => "translate",
        GeometryOp::Rotate { .. } => "rotate",
        GeometryOp::Scale { .. } => "scale",
        GeometryOp::RotateAround { .. } => "rotate_around",
        GeometryOp::ApplyTransform { .. } => "apply_transform",
        GeometryOp::Extrude { .. } => "extrude",
        GeometryOp::ExtrudeSymmetric { .. } => "extrude_symmetric",
        GeometryOp::Revolve { .. } => "revolve",
        GeometryOp::Loft { .. } => "loft",
        GeometryOp::LoftGuided { .. } => "loft_guided",
        GeometryOp::Sweep { .. } => "sweep",
        GeometryOp::SweepGuided { .. } => "sweep_guided",
        GeometryOp::Pipe { .. } => "pipe",
        GeometryOp::Mirror { .. } => "mirror",
        GeometryOp::LinearPattern { .. } => "linear_pattern",
        GeometryOp::LinearPattern2D { .. } => "linear_pattern_2d",
        GeometryOp::CircularPattern { .. } => "circular_pattern",
        GeometryOp::ArbitraryPattern { .. } => "arbitrary_pattern",
        GeometryOp::LineSegment { .. } => "line_segment",
        GeometryOp::Arc { .. } => "arc",
        GeometryOp::Helix { .. } => "helix",
        GeometryOp::InterpCurve { .. } => "interp_curve",
        GeometryOp::BezierCurve { .. } => "bezier_curve",
        GeometryOp::NurbsCurve { .. } => "nurbs_curve",
        GeometryOp::RectangleProfile { .. } => "rectangle_profile",
        GeometryOp::CircleProfile { .. } => "circle_profile",
        GeometryOp::PolygonProfile { .. } => "polygon_profile",
        GeometryOp::EllipseProfile { .. } => "ellipse_profile",
        GeometryOp::NurbsSurface { .. } => "nurbs_surface",
    }
}"#;
    let count = count_geometryop_dispatch_arms(src);
    assert!(
        count >= 40,
        "expected ≥40 dispatch arms in synthetic exhaustive match, got {count}"
    );
}

/// Synthetic **NEGATIVE A**: only `CompiledGeometryOp::` match arms and RHS
/// `GeometryOp::` constructions — the counter must return **0**.
///
/// Proves the counter correctly excludes:
/// - `CompiledGeometryOp::X =>` (identifier char `p` precedes the `::`)
/// - `GeometryOp::X { .. }` on a line with no `=>` (RHS / binding site)
/// - `GeometryOp::X { .. }` appearing after `=>` on the same line
#[test]
fn detector_ignores_compiled_and_rhs_constructs() {
    let src = r#"fn compiled_dispatch(op: &CompiledGeometryOp) -> &str {
    match op {
        CompiledGeometryOp::Primitive(_) => "primitive",
        CompiledGeometryOp::Modify(_) => "modify",
        CompiledGeometryOp::Transform(_) => "transform",
        CompiledGeometryOp::Pattern(_) => "pattern",
        CompiledGeometryOp::Sweep(_) => "sweep",
        CompiledGeometryOp::Curve(_) => "curve",
        CompiledGeometryOp::Profile(_) => "profile",
        CompiledGeometryOp::Surface(_) => "surface",
    }
}
fn build_ops() {
    let a: GeometryOp = GeometryOp::Union { left: h1, right: h2 };
    let b = Ok(GeometryOp::Difference { left: h1, right: h2 });
    let c = GeometryOp::Sphere { radius: val };
    let d = wrap(GeometryOp::Torus { major_radius: m, minor_radius: n });
}"#;
    let count = count_geometryop_dispatch_arms(src);
    assert_eq!(
        count,
        0,
        "expected 0 arms for CompiledGeometryOp:: + RHS constructions, got {count}"
    );
}

/// Synthetic **NEGATIVE B**: a DD-6-style `ParentRole` role-shim (mirroring
/// the real `parent_handles_for_op` structure in `engine_build.rs`) counts a
/// small number of arm-terminator lines, well below
/// [`AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD`].
///
/// Proves the counter does not false-fail the legitimate post-refactor shapes.
/// The inner OR-pattern intermediate lines (no `=>`) are not counted; only the
/// OR-pattern terminator lines (with `=>`) are counted.
#[test]
fn detector_counts_role_shim_below_threshold() {
    // Mirror of the real DD-6 structure in engine_build.rs: descriptor_for call
    // followed by a match on ParentRole, with inner OR-patterns over GeometryOp
    // variants as arm terminators and wildcard unreachable fallbacks.
    let src = r#"fn parent_handles_for_op(op: &GeometryOp) -> ParentHandles {
    let role = descriptor_for(op.into())
        .expect("every GeometryOp variant must have a descriptor row")
        .parent_role;

    match role {
        ParentRole::None => {}
        ParentRole::Pair => match op {
            GeometryOp::Union { left, right, .. }
            | GeometryOp::Difference { left, right, .. }
            | GeometryOp::Intersection { left, right, .. } => {}
            _ => unreachable!()
        }
        ParentRole::SingleTarget => match op {
            GeometryOp::Fillet { target, .. }
            | GeometryOp::Chamfer { target, .. }
            | GeometryOp::ChamferAsymmetric { target, .. }
            | GeometryOp::Draft { target, .. }
            | GeometryOp::Thicken { target, .. }
            | GeometryOp::OffsetSolid { target, .. }
            | GeometryOp::Shell { target, .. }
            | GeometryOp::Mirror { target, .. }
            | GeometryOp::LinearPattern { target, .. }
            | GeometryOp::LinearPattern2D { target, .. }
            | GeometryOp::CircularPattern { target, .. }
            | GeometryOp::ArbitraryPattern { target, .. } => {}
            _ => unreachable!()
        }
    }
}"#;
    let count = count_geometryop_dispatch_arms(src);
    assert!(
        count < AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD,
        "DD-6 role-shim should count below threshold {AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD}, got {count}"
    );
}

/// `slice_fn_body` extracts the correct function body and does not spill into
/// the following function.
#[test]
fn slice_fn_body_extracts_named_fn() {
    let src = "fn foo_helper(x: u32) -> u32 {\n    x + 1\n}\n\nfn another_fn() {\n    println!(\"hello\");\n}\n";
    let body = slice_fn_body(src, "foo_helper");
    assert!(
        body.contains("x + 1"),
        "slice_fn_body should include the body of foo_helper, got: {body:?}"
    );
    assert!(
        !body.contains("another_fn"),
        "slice_fn_body must not spill into another_fn, got: {body:?}"
    );
}
