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
    const BOUNDARY: &str = "\n#[cfg(test)]\nmod tests {";
    let pos = src
        .find(BOUNDARY)
        .unwrap_or_else(|| panic!("could not locate '\\n#[cfg(test)]\\nmod tests {{' boundary in source"));
    &src[..pos]
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
    // Build the column-0 signature prefix we're searching for.
    // A top-level fn appears at column 0; we look for it either at the
    // start of the string or after a newline.
    let sig_open_paren = format!("fn {}(", fn_name);
    let sig_open_angle = format!("fn {}<", fn_name);

    // Find the byte offset of the line containing the function signature.
    let fn_start = {
        let mut found = None;
        // Search for the signature at the start of the string.
        if src.starts_with(&sig_open_paren) || src.starts_with(&sig_open_angle) {
            found = Some(0usize);
        }
        if found.is_none() {
            // Search after each newline.
            let mut pos = 0usize;
            while let Some(nl) = src[pos..].find('\n') {
                let line_start = pos + nl + 1;
                let rest = &src[line_start..];
                if rest.starts_with(&sig_open_paren) || rest.starts_with(&sig_open_angle) {
                    found = Some(line_start);
                    break;
                }
                pos = line_start;
            }
        }
        match found {
            Some(off) => off,
            None => return "",
        }
    };

    // Now scan forward from fn_start to find the next column-0 `}` line.
    let tail = &src[fn_start..];
    let end_offset = {
        let mut off = 0usize;
        // Skip past the signature line itself.
        if let Some(nl) = tail.find('\n') {
            off = nl + 1;
        }
        let mut found_end = tail.len();
        while off < tail.len() {
            let line_tail = &tail[off..];
            let line_end = line_tail.find('\n').map(|n| n + 1).unwrap_or(line_tail.len());
            let line = &line_tail[..line_end];
            // Column-0 closing brace: the line is exactly "}\n" or "}"
            if line == "}\n" || line == "}" {
                found_end = off + line_end;
                break;
            }
            off += line_end;
        }
        found_end
    };

    &tail[..end_offset]
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
    const TOKEN: &str = "GeometryOp::";
    let mut count = 0usize;

    for line in region.lines() {
        // Condition (c): the line must have a `=>` somewhere.
        let arrow_pos = match line.find("=>") {
            Some(p) => p,
            None => continue,
        };

        // Find all occurrences of `GeometryOp::` on this line and check (b)+(c).
        let mut search_start = 0usize;
        while let Some(tok_rel) = line[search_start..].find(TOKEN) {
            let tok_pos = search_start + tok_rel;

            // Condition (b): char immediately before must not be an ASCII identifier char.
            let preceded_by_ident = tok_pos > 0 && {
                let prev = line.as_bytes()[tok_pos - 1];
                prev.is_ascii_alphanumeric() || prev == b'_'
            };

            // Condition (c): token must appear before the first `=>`.
            let before_arrow = tok_pos < arrow_pos;

            if !preceded_by_ident && before_arrow {
                count += 1;
                // Only count the first qualifying occurrence per line.
                break;
            }

            search_start = tok_pos + TOKEN.len();
        }
    }

    count
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

// ── Step-3: Axis-1 guards (engine_build.rs) ─────────────────────────────────

/// `geometry_op_to_operation` must be a pure `descriptor_for` lookup with
/// zero `GeometryOp` dispatch arms (Axis-1 DD-2 guard).
#[test]
fn geometry_op_to_operation_is_pure_table_lookup() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/engine_build.rs"
    ))
    .expect("could not read engine_build.rs");

    let body = slice_fn_body(&src, "geometry_op_to_operation");
    assert!(
        !body.is_empty(),
        "slice_fn_body did not find geometry_op_to_operation in engine_build.rs"
    );
    assert!(
        body.contains("descriptor_for"),
        "geometry_op_to_operation must contain a descriptor_for call (table-driven Axis-1)"
    );
    let arm_count = count_geometryop_dispatch_arms(body);
    assert_eq!(
        arm_count,
        0,
        "geometry_op_to_operation must have 0 GeometryOp dispatch arms, found {arm_count}\n\
         — per-variant logic must not be re-introduced (DD-2 regression)"
    );
}

/// `parent_handles_for_op` and `substitute_op_parents` must route through
/// `descriptor_for` and match on `ParentRole` (DD-6 shim pattern).
#[test]
fn parent_and_substitute_route_through_parentrole() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/engine_build.rs"
    ))
    .expect("could not read engine_build.rs");

    for fn_name in &["parent_handles_for_op", "substitute_op_parents"] {
        let body = slice_fn_body(&src, fn_name);
        assert!(
            !body.is_empty(),
            "slice_fn_body did not find {fn_name} in engine_build.rs"
        );
        assert!(
            body.contains("descriptor_for"),
            "{fn_name} must contain descriptor_for (table-driven DD-6)"
        );
        assert!(
            body.contains("ParentRole::"),
            "{fn_name} must match on ParentRole (DD-6 shim pattern)"
        );
    }
}

/// The whole `engine_build.rs` non-test region must stay below the regression
/// threshold for `GeometryOp` dispatch arms.
///
/// Measured DD-6 floor is 14; threshold [`AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD`]
/// of 30 has headroom. A resurrected exhaustive 48-variant dispatch would
/// push the count to ~62 and trip this gate.
#[test]
fn engine_build_has_no_resurrected_exhaustive_geometryop_dispatch() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/engine_build.rs"
    ))
    .expect("could not read engine_build.rs");

    let non_test = non_test_region(&src);
    let count = count_geometryop_dispatch_arms(non_test);
    assert!(
        count < AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD,
        "engine_build.rs non-test region has {count} GeometryOp dispatch arm(s), \
         threshold is {AXIS1_EXHAUSTIVE_DISPATCH_THRESHOLD} — a value at/above the \
         threshold indicates an exhaustive per-variant match was re-introduced \
         (DD-2 regression; measured DD-6 floor is 14)"
    );
}

// ── Step-4: Axis-3 guard (geometry_ops.rs) ──────────────────────────────────

/// The `geometry_ops.rs` non-test region must have no nested per-kind
/// behavioral match arms and must contain the fn-table statics.
#[test]
fn geometry_ops_dispatch_is_fn_table_not_nested_kind_match() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/geometry_ops.rs"
    ))
    .expect("could not read geometry_ops.rs");

    let non_test = non_test_region(&src);

    // Mirror the L5 inline guard filter exactly.
    let bad_arms: Vec<&str> = non_test
        .lines()
        .filter(|line| {
            let has_kind_enum = line.contains("PrimitiveKind::")
                || line.contains("ModifyKind::")
                || line.contains("TransformKind::")
                || line.contains("PatternKind::")
                || line.contains("SweepKind::")
                || line.contains("CurveKind::")
                || line.contains("ProfileKind::");
            let has_fat_arrow = line.contains("=>");
            has_kind_enum && has_fat_arrow
        })
        .collect();

    assert!(
        bad_arms.is_empty(),
        "found {} nested per-kind behavioral match arm(s) in geometry_ops.rs \
         non-test region — all dispatch must go through fn-tables:\n{}",
        bad_arms.len(),
        bad_arms.join("\n")
    );

    // The fn-table statics must be present (Axis-3 implementation proof).
    assert!(
        non_test.contains("PRIMITIVE_COMPILERS"),
        "geometry_ops.rs non-test region must contain PRIMITIVE_COMPILERS fn-table static"
    );
    assert!(
        non_test.contains("MODIFY_COMPILERS"),
        "geometry_ops.rs non-test region must contain MODIFY_COMPILERS fn-table static"
    );
}

// ── Step-5: Cross-crate live descriptor-table guarantee ──────────────────────

/// Every `GeometryOpDiscriminants` variant must resolve via `descriptor_for`
/// to `Some`; table length equals `COUNT`; disc fields are unique.
///
/// Executes the live every-op-handled guarantee from the CONSUMER crate,
/// complementing (not duplicating) L1's in-crate completeness test which
/// cannot prove cross-crate public usability.
#[test]
fn descriptor_table_is_the_live_op_handled_guarantee() {
    use reify_ir::geometry::{
        descriptor_for, GeometryOpDiscriminants, GEOMETRY_OP_DESCRIPTORS,
    };
    use strum::{EnumCount, IntoEnumIterator};

    // Every discriminant resolves to Some.
    let mut missing = Vec::new();
    for disc in GeometryOpDiscriminants::iter() {
        if descriptor_for(disc).is_none() {
            missing.push(format!("{:?}", disc));
        }
    }
    assert!(
        missing.is_empty(),
        "descriptor_for returned None for {} discriminant(s): {:?}\n\
         — add a matching row to GEOMETRY_OP_DESCRIPTORS",
        missing.len(),
        missing
    );

    // Table length equals the discriminant count.
    let disc_count = GeometryOpDiscriminants::COUNT;
    let table_len = GEOMETRY_OP_DESCRIPTORS.len();
    assert_eq!(
        table_len,
        disc_count,
        "GEOMETRY_OP_DESCRIPTORS has {table_len} rows but GeometryOpDiscriminants::COUNT is {disc_count}"
    );

    // Disc fields are unique (no duplicate descriptor rows).
    let mut seen = std::collections::HashSet::new();
    for d in GEOMETRY_OP_DESCRIPTORS {
        assert!(
            seen.insert(d.disc),
            "duplicate descriptor row for {:?} in GEOMETRY_OP_DESCRIPTORS",
            d.disc
        );
    }
}

// ── Step-6: Canary-retirement + query-untouched ──────────────────────────────

/// Verify canary retirement: the `const GEOMETRY_OP_VARIANT_COUNT` definition
/// is absent from `reify-ir`; `EXPECTED_DISPATCH_COUNT` is absent from
/// `reify-compiler`; the out-of-scope `const GEOMETRY_QUERY_VARIANT_COUNT`
/// is still present in `reify-ir` (per PRD §7).
///
/// Matching the const-definition form (not the bare identifier) is
/// comment-tolerant: a surviving historical comment at reify-ir geometry.rs
/// near line 7644 names `GEOMETRY_OP_VARIANT_COUNT` without defining it.
#[test]
fn canaries_retired_and_query_canary_untouched() {
    let reify_ir_geometry = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reify-ir/src/geometry.rs"
    ))
    .expect("could not read reify-ir/src/geometry.rs");

    let reify_compiler_geometry = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reify-compiler/src/geometry.rs"
    ))
    .expect("could not read reify-compiler/src/geometry.rs");

    // Match const-definition form, not bare name substring (comment-tolerant).
    let op_variant_count_def = "const GEOMETRY_OP_VARIANT_COUNT";
    assert!(
        !reify_ir_geometry
            .lines()
            .any(|line| line.contains(op_variant_count_def)),
        "reify-ir geometry.rs still defines `{op_variant_count_def}` — \
         the L1 canary was retired in task #4670 and must not be re-introduced"
    );

    let expected_dispatch_def = "EXPECTED_DISPATCH_COUNT";
    assert!(
        !reify_compiler_geometry
            .lines()
            .any(|line| line.contains(expected_dispatch_def)),
        "reify-compiler geometry.rs still contains `{expected_dispatch_def}` — \
         the L3 canary was retired in task #4672 and must not be re-introduced"
    );

    // GEOMETRY_QUERY_VARIANT_COUNT is out-of-scope per §7 and must be untouched.
    let query_count_def = "const GEOMETRY_QUERY_VARIANT_COUNT";
    assert!(
        reify_ir_geometry
            .lines()
            .any(|line| line.contains(query_count_def)),
        "reify-ir geometry.rs is missing `{query_count_def}` — \
         the query canary is out-of-scope and must not have been removed"
    );
}
