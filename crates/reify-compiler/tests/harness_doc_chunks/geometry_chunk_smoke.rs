//! Compile-smoke test for the "geometry" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/geometry.md`), served to the in-GUI
//! assistant via `reify_language_reference`.
//!
//! Mirrors sibling task 5347's stdlib.md audit: this test pins every
//! documented geometry-constructor call form to what the compiler actually
//! accepts, so a phantom signature in the doc (or a future compiler
//! signature change) is caught here rather than by a designer typing the
//! documented-but-wrong call into a `.ri` file.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where `geometry.md` itself
//! lives) because `reify-mcp` does not depend on `reify-compiler` and so
//! cannot invoke `compile_source_with_stdlib` — the same cross-crate
//! placement sibling task 5347 used for its stdlib-chunk smoke test.
//!
//! Fixtures are curated `.ri` snippets, not scraped from geometry.md itself:
//! the doc intermixes non-compilable schematic notation (type params, trait
//! lists, `-> Solid` return annotations) with real call forms, so a scraper
//! would need a fragile grammar to separate the two. Each fixture instead
//! mirrors one documented call form exactly, wrapped in the minimal
//! compilable module shape (`examples/bracket.ri`'s
//! `structure def X { let body = box(...) }` pattern).

use reify_test_support::{compile_source_with_stdlib, errors_only};

/// Compile `geometry_expr` wrapped in a minimal `structure def Smoke { let g
/// = <geometry_expr> }` module and assert there are zero Severity::Error
/// diagnostics. On failure, the panic message names `label` and dumps every
/// Error diagnostic so a failing signature is immediately identifiable.
fn assert_compiles(label: &str, geometry_expr: &str) {
    let source = format!("structure def Smoke {{ let g = {} }}", geometry_expr);
    let compiled = compile_source_with_stdlib(&source);
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "{label}: expected `{geometry_expr}` to compile with zero Error diagnostics, got: {:#?}",
        errors
    );
}

// --- Solid primitives (geometry.md "Solid Primitives" block) ---

#[test]
fn box_primitive_compiles() {
    assert_compiles("box", "box(10mm, 20mm, 5mm)");
}

#[test]
fn box_centered_compiles() {
    assert_compiles("box_centered", "box_centered(10mm, 20mm, 5mm)");
}

#[test]
fn cylinder_compiles() {
    assert_compiles("cylinder", "cylinder(5mm, 10mm)");
}

#[test]
fn cylinder_centered_compiles() {
    assert_compiles("cylinder_centered", "cylinder_centered(5mm, 10mm)");
}

#[test]
fn cone_compiles() {
    assert_compiles("cone", "cone(5mm, 3mm, 10mm)");
}

#[test]
fn sphere_compiles() {
    assert_compiles("sphere", "sphere(5mm)");
}

#[test]
fn torus_compiles() {
    assert_compiles("torus", "torus(10mm, 2mm)");
}

#[test]
fn tube_compiles() {
    assert_compiles("tube", "tube(10mm, 5mm, 20mm)");
}

#[test]
fn wedge_compiles() {
    assert_compiles("wedge", "wedge(10mm, 10mm, 10mm, 5mm)");
}

#[test]
fn rounded_box_compiles() {
    // corner_r=3mm > 0 and 2*3mm=6mm < min(20mm,20mm) — satisfies the
    // rounded-corner constraint documented at geometry.md line 79.
    assert_compiles("rounded_box", "rounded_box(20mm, 20mm, 10mm, 3mm)");
}

#[test]
fn half_space_compiles() {
    // geometry.md's "Solid Primitives" block documents the exactly-6-arg
    // `half_space(px, py, pz, nx, ny, nz)` form (geometry.rs:1680,
    // `PrimitiveKind::HalfSpace`) — the first Bounded=false producer.
    //
    // Mixed-dimension convention, same split `revolve_compiles` below pins:
    // args 0-2 are a POINT on the boundary plane, a Length position, so they
    // take `mm` literals; args 3-5 are the OUTWARD NORMAL pointing toward the
    // retained material — a direction whose magnitude is irrelevant and which
    // the compiler does not unit-check — so they take dimensionless literals,
    // to avoid implying a direction vector carries a length unit. Arity alone
    // does not constrain this split, which is why it is pinned here.
    // Grounding site: examples/half_space.ri:19.
    assert_compiles("half_space", "half_space(0mm, 0mm, 0mm, 0, 0, 1)");
}

// --- 2D profiles (geometry.md "2D profiles" block) ---

#[test]
fn rectangle_compiles() {
    assert_compiles("rectangle", "rectangle(20mm, 10mm)");
}

#[test]
fn circle_profile_compiles() {
    assert_compiles("circle_profile", "circle(5mm)");
}

#[test]
fn ellipse_compiles() {
    assert_compiles("ellipse", "ellipse(10mm, 5mm)");
}

#[test]
fn rounded_rect_compiles() {
    // corner_r=2mm > 0 and 2*2mm=4mm < min(20mm,10mm) — satisfies the
    // rounded-corner constraint documented at geometry.md line 94.
    assert_compiles("rounded_rect", "rounded_rect(20mm, 10mm, 2mm)");
}

// --- Sweep (geometry.md anchoring table) ---

#[test]
fn extrude_compiles() {
    assert_compiles("extrude", "extrude(circle(5mm), 10mm)");
}

// --- Point/vector constructors (geometry.md "Geometry Constructors (Prelude)" block) ---

#[test]
fn point2_compiles() {
    assert_compiles("point2", "point2(0mm, 0mm)");
}

#[test]
fn point3_compiles() {
    assert_compiles("point3", "point3(0mm, 0mm, 0mm)");
}

#[test]
fn vec2_compiles() {
    assert_compiles("vec2", "vec2(1.0, 0.0)");
}

#[test]
fn vec3_compiles() {
    assert_compiles("vec3", "vec3(0.0, 0.0, 1.0)");
}

// --- Anchoring table: revolve(profile, ox, oy, oz, ax, ay, az, angle) ---

#[test]
fn revolve_compiles() {
    // geometry.md line 121 documents the 8-arg
    // `revolve(profile, ox, oy, oz, ax, ay, az, angle)` form (geometry.rs:1969):
    // profile + origin (0,0,0) + axis direction (0,0,1) + angle. The origin
    // triple (ox,oy,oz) is a Length position, so it takes `mm` literals;
    // the axis triple (ax,ay,az) is a direction only — its magnitude is
    // irrelevant and the compiler does not unit-check it — so it takes
    // dimensionless literals to avoid implying a direction vector has a
    // length unit.
    assert_compiles(
        "revolve",
        "revolve(circle(5mm), 0mm, 0mm, 0mm, 0.0, 0.0, 1.0, 90deg)",
    );
}

// --- Prelude/2D-profile constructors: line_segment, arc, polygon ---
//
// geometry.md's "Geometry Constructors (Prelude)" block (lines 56-61) and
// "2D profiles" block (line 87) documented four phantom call forms with no
// matching compiler arm: `line(start, end)`, `arc(center, radius,
// start_angle, end_angle)` [4-arg], `circle(center, radius)` [2-arg], and
// `polygon(points)`/`polygon(vertices)` [1-arg collection]. All four are now
// corrected to their authoritative flat-coordinate forms below.
//
// (`line` was never a registered builtin at all — absent from
// GEOMETRY_FUNCTION_NAMES and reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS —
// so pre-fix it silently "compiled" via expr.rs's permissive
// unresolved-function fallback (types an >= 1-arg call from its first
// argument's result_type, no diagnostic) rather than erroring; arc/circle/
// polygon are registered builtins and pre-fix failed their own
// arg-count-exact / coordinate-pair checks. See this task's escalation
// resolution for the verification detail. The phantom `circle(center,
// radius)` prelude form is simply removed, not replaced — there is only
// ever the one 1-arg `circle(radius)`, already pinned by
// `circle_profile_compiles` above.)

#[test]
fn line_segment_compiles() {
    // geometry.md line 59 documents the 6-arg
    // `line_segment(x1, y1, z1, x2, y2, z2)` form (geometry_curve.rs:22).
    assert_compiles(
        "line_segment",
        "line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm)",
    );
}

#[test]
fn arc_compiles() {
    // geometry.md line 60 documents the 9-arg
    // `arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)` form
    // (geometry_curve.rs:47): center + radius + angle range + axis direction.
    assert_compiles(
        "arc",
        "arc(0mm, 0mm, 0mm, 5mm, 0deg, 90deg, 0mm, 0mm, 1mm)",
    );
}

#[test]
fn polygon_compiles() {
    // geometry.md line 61 (prelude block) and line 87 (2D-profiles block)
    // document the variadic flat coordinate-pairs form
    // `polygon(x1, y1, x2, y2, ...)` (>= 6 args, even count; geometry.rs:1570).
    assert_compiles("polygon", "polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)");
}

// --- GD&T tolerance zones (geometry.md "GD&T Tolerance Zones" block) ---
//
// Four zone constructors that produce a tolerance-zone Solid rather than a
// primitive. Their arities are checked by the compiler, but their argument
// DIMENSIONS and ORDER are not — so each form below is a transcription of an
// already-compiling call site (`examples/tolerancing/gdt_zones.ri`,
// `crates/reify-eval/tests/zone_constructors_e2e.rs`,
// `crates/reify-compiler/tests/zone_slab_compile_tests.rs`), concretized to
// literals, rather than a signature read off the arm alone.

#[test]
fn zone_slab_compiles() {
    // geometry.md's "GD&T Tolerance Zones" block documents the 2-arg
    // `zone_slab(face, width)` form. Routed as a Modify extension
    // (geometry.rs:2622 → geometry_modify.rs:86,
    // `compile_modify_2arg(ModifyKind::ZoneSlab, "width")`), so arg 0 is a
    // geometry TARGET — a face/profile, not a solid — offset ±width/2 and
    // capped into a slab. Grounding site: zone_slab_compile_tests.rs:27.
    assert_compiles("zone_slab", "zone_slab(rectangle(40mm, 20mm), 2mm)");
}

#[test]
fn zone_cylinder_compiles() {
    // geometry.md documents the exactly-2-arg `zone_cylinder(axis, width)`
    // form (geometry.rs:2241). Arg 0 is an axis WIRE (its own length sets the
    // cylinder extent — there is deliberately no length argument); `width` is
    // the Ø-zone DIAMETER, lowered to Sweep{Pipe} with radius = width * 0.5.
    // Grounding site: examples/tolerancing/gdt_zones.ri:23.
    assert_compiles(
        "zone_cylinder",
        "zone_cylinder(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 8mm)",
    );
}

#[test]
fn zone_annulus_compiles() {
    // geometry.md documents the exactly-4-arg
    // `zone_annulus(axis, nominal_radius, width, length)` form
    // (geometry.rs:2288) — Difference(Pipe(axis, R + w/2), Pipe(axis, R − w/2)).
    // Arg 3 `length` is accepted and validated, but the swept extent still
    // comes from the axis wire (ratified L2 esc-4476-88 Option A), so the
    // 4-arg spelling must be pinned even though the argument is unused.
    // Grounding site: examples/tolerancing/gdt_zones.ri:28.
    assert_compiles(
        "zone_annulus",
        "zone_annulus(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 20mm, 4mm, 20mm)",
    );
}

#[test]
fn zone_profile_compiles() {
    // geometry.md documents the exactly-2-arg `zone_profile(solid, width)`
    // form (geometry.rs:2355) — Difference(Thicken(solid, +w/2),
    // Thicken(solid, −w/2)) via OCCT Thicken. Arg 0 is a SOLID here (unlike
    // zone_slab's face). Grounding site: examples/tolerancing/gdt_zones.ri:32.
    assert_compiles("zone_profile", "zone_profile(box(10mm, 10mm, 10mm), 1mm)");
}
