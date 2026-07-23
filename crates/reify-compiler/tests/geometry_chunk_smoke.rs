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
    // profile + origin (0,0,0) + axis direction (0,0,1) + angle.
    assert_compiles(
        "revolve",
        "revolve(circle(5mm), 0mm, 0mm, 0mm, 0mm, 0mm, 1mm, 90deg)",
    );
}

// --- Prelude/2D-profile phantoms: line, arc, circle, polygon ---
//
// geometry.md's "Geometry Constructors (Prelude)" block (lines 56-62) and
// "2D profiles" block (line 87) document four phantom call forms with no
// matching compiler arm. Three of the four (arc/circle/polygon) ARE
// registered geometry builtins (GEOMETRY_FUNCTION_NAMES, units.rs:88)
// called with the wrong arg count, so each one's own arg-count-exact check
// emits a Severity::Error — RED below.
//
// `line` is different: no `line` builtin is registered anywhere (absent
// from GEOMETRY_FUNCTION_NAMES and from
// reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS), so the call falls all the
// way through expr.rs's unresolved-function handling to its final,
// permissive fallback (expr.rs ~3540-3556): for any call with >= 1
// argument, that fallback types the result from the *first argument's*
// result_type and emits NO diagnostic at all (only the *zero*-arg case
// emits a Severity::Warning). So `line(point3(..), point3(..))` "compiles"
// with zero Error diagnostics today even though `line` is a phantom name —
// this fixture is intentionally NOT red pre-fix; see
// `line_prelude_phantom_compiles_via_permissive_fallback` below. The
// broader compiler gap this reveals (no diagnostic for a call to a
// genuinely undefined function name once arg-count >= 1) is out of scope
// for this doc-audit task and is tracked separately as a follow-up, not
// fixed here.

#[test]
fn line_prelude_phantom_compiles_via_permissive_fallback() {
    // geometry.md line 59 documents a 2-arg `line(start, end)`, but no
    // `line` builtin exists in the compiler at all. Unlike its
    // arc/circle/polygon siblings below, this call is not rejected: it
    // resolves through expr.rs's final unresolved-function fallback (see
    // block comment above), which types any >= 1-arg call from its first
    // argument's result_type with no diagnostic. Kept passing (not forced
    // red by some other mechanism) per house ruling on this task's
    // escalation — the RED state for this suite is genuinely 3-of-4, not
    // 4-of-4.
    assert_compiles(
        "line_prelude_phantom",
        "line(point3(0mm, 0mm, 0mm), point3(10mm, 0mm, 0mm))",
    );
}

#[test]
fn arc_prelude_phantom_is_rejected_pending_doc_fix() {
    // geometry.md line 59 documents a 4-arg
    // `arc(center, radius, start_angle, end_angle)`, but the only compiler
    // arm is the 9-arg
    // `arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)`
    // (geometry_curve.rs:47). RED until step-4 corrects geometry.md's
    // prelude block and this fixture to the real 9-arg form.
    assert_compiles(
        "arc_prelude_phantom",
        "arc(point3(0mm, 0mm, 0mm), 5mm, 0deg, 90deg)",
    );
}

#[test]
fn circle_prelude_phantom_is_rejected_pending_doc_fix() {
    // geometry.md lines 59-60 document a prelude 2-arg
    // `circle(center, radius)`, distinct from the 1-arg profile
    // `circle(radius)` already pinned by `circle_profile_compiles` above.
    // No 2-arg arm exists (geometry.rs:1556 is 1-arg only). RED until
    // step-4 removes this phantom prelude form from geometry.md — there is
    // only ever one `circle`.
    assert_compiles(
        "circle_prelude_phantom",
        "circle(point3(0mm, 0mm, 0mm), 5mm)",
    );
}

#[test]
fn polygon_prelude_phantom_is_rejected_pending_doc_fix() {
    // geometry.md line 61 (prelude block) and line 87 (2D-profiles block)
    // document a 1-arg `polygon(points)`/`polygon(vertices)` taking a
    // point collection, but the only compiler arm is variadic flat
    // coordinate pairs (>= 6 args, even count) (geometry.rs:1570). RED
    // until step-4 corrects both geometry.md blocks and this fixture to
    // the real variadic coordinate-pair form.
    assert_compiles(
        "polygon_prelude_phantom",
        "polygon([point2(0mm, 0mm), point2(10mm, 0mm), point2(5mm, 10mm)])",
    );
}
