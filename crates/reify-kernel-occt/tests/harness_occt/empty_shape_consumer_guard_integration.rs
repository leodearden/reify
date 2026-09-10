//! The empty-shape PRECONDITION: which consumers of a shape refuse one that
//! carries no topology, and — just as load-bearing — which deliberately do not
//! (task 5318, ruling of 2026-09-08 / esc-5318-7).
//!
//! An empty OCCT boolean result is a LEGAL kernel value. `BRepAlgoAPI_Common`
//! on disjoint operands, and `BRepAlgoAPI_Cut` whose tool fully consumes its
//! target, report `IsDone() == true` and hand back an empty `TopoDS_Compound`;
//! `harness_occt::boolean_result_normalization_integration::
//! empty_boolean_results_stay_untouched_compounds` gates that, and
//! `examples/tolerancing/gdt_oracle_inside.ri` DESIGNS on it (an empty cut is
//! the "inside" verdict, and `volume()` of it is 0.0). So the guard cannot live
//! at the boolean producer.
//!
//! It lives instead at the ops that MINT AN ARTIFACT from a shape and cannot
//! mint one from nothing: the profile-solidifying sweep family, and STEP
//! export. Those are what this file pins, in one place, together with the
//! exclusions:
//!
//!   * NOT guarded — the booleans (the ruling, and the gate above);
//!     `volume`/`area`/mass-property queries (an empty shape's 0.0 IS the GD&T
//!     oracle's answer channel); tessellation (an empty mesh is an honest
//!     rendering, and `zone_profile_realize_smoke` relies on it); the
//!     transforms (empty in, empty out — the emptiness survives to a real
//!     consumer); `fillet`/`chamfer` (already refused by task 7054's
//!     `BRepKind::Solid` gate, since an empty result classifies as `Compound`);
//!     and `fuse_shape_list` (a pure union over a non-empty list, on the hot
//!     pattern-realizer path).
//!
//! The over-fire controls are as important as the rejections: they are what
//! keeps "the operands do not touch" from being confused with "the result is
//! empty". Their expected volumes are exact closed forms for axis-aligned
//! boxes, not tuned thresholds.

#![cfg(has_occt)]

use reify_ir::{
    ExportError, ExportFormat, ExportOptions, GeometryError, GeometryHandleId, GeometryOp,
    GeometryQuery, Value,
};
use reify_kernel_occt::OcctKernel;

/// Build an axis-aligned cube of side `side`, centred on the origin
/// (`make_box` centres its output — cpp/occt_wrapper.cpp).
fn cube(kernel: &mut OcctKernel, side: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(side),
            height: Value::Real(side),
            depth: Value::Real(side),
        })
        .expect("box creation should succeed")
        .id
}

/// Translate `target` along X, returning a fresh handle.
fn translated_x(kernel: &mut OcctKernel, target: GeometryHandleId, dx: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed")
        .id
}

/// Intersect two disjoint 10-unit cubes, yielding a shape with NO topology.
///
/// This rests on a landed gated fact rather than an assumption: main's
/// `empty_boolean_results_stay_untouched_compounds` already pins that this
/// SUCCEEDS and classifies as `BRepKind::Compound` with zero volume.
fn empty_intersection(kernel: &mut OcctKernel) -> GeometryHandleId {
    let left = cube(kernel, 10.0);
    let right_raw = cube(kernel, 10.0);
    let right = translated_x(kernel, right_raw, 100.0);
    kernel
        .execute(&GeometryOp::Intersection { left, right })
        .expect("an intersection of disjoint operands must SUCCEED with an empty result")
        .id
}

/// A real 5-unit-radius disk face, the ordinary profile the sweep family takes.
fn circle_profile(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(radius),
        })
        .expect("CircleProfile creation should succeed")
        .id
}

/// A closed circular WIRE (not a face) — what the loft entry points require,
/// since both downcast every profile with `TopoDS::Wire`.
fn circle_wire(kernel: &mut OcctKernel, radius: f64, z: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, z],
            radius,
            start_angle: 0.0,
            end_angle: 2.0 * std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle) creation should succeed")
        .id
}

/// A rectangular face profile in the XY plane, translated `cy` along Y.
///
/// The companion revolve turns about the X AXIS, which LIES IN that plane, so
/// the sweep encloses a real ring. Revolving the same profile about Z would be
/// degenerate — the profile plane is perpendicular to Z, so the sweep never
/// leaves it and encloses no volume (measured: 0.0).
fn offset_rect_profile(kernel: &mut OcctKernel, cy: f64) -> GeometryHandleId {
    let rect = kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.005),
            height: Value::Real(0.010),
        })
        .expect("RectangleProfile creation should succeed")
        .id;
    kernel
        .execute(&GeometryOp::Translate {
            target: rect,
            dx: 0.0,
            dy: cy,
            dz: 0.0,
        })
        .expect("translate should succeed")
        .id
}

/// A straight line-segment path along +Z, the spine both sweep entry points
/// downcast with `TopoDS::Wire`.
fn straight_path(kernel: &mut OcctKernel, length: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (path) creation should succeed")
        .id
}

/// A guide wire offset from the spine, for the guided sweep/loft variants.
fn offset_guide(kernel: &mut OcctKernel, dx: f64, length: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: dx,
            y1: 0.0,
            z1: 0.0,
            x2: dx,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (guide) creation should succeed")
        .id
}

fn volume_of(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    match kernel.query(&GeometryQuery::Volume(id)) {
        Ok(Value::Real(v)) => v,
        other => panic!("expected Value::Real from a Volume query, got {other:?}"),
    }
}

/// Assert `id`'s volume is within `rel_tol` relative of `expected` (non-zero).
fn assert_volume_near(kernel: &OcctKernel, id: GeometryHandleId, expected: f64, what: &str) {
    let v = volume_of(kernel, id);
    let rel = (v - expected).abs() / expected.abs();
    assert!(
        rel < 1e-6,
        "{what}: volume {v:.9} not within 1e-6 relative of the exact closed form \
         {expected:.9} (rel={rel:.3e})"
    );
}

/// Assert `result` is `Err(GeometryError::OperationFailed(msg))` naming the
/// input as `empty` and naming the argument role the guard reports
/// (`role_word`, e.g. "profile"), so a designer reading it knows WHICH of their
/// arguments collapsed.
///
/// Substring matching is deliberately loose so re-wording the diagnostic does
/// not make these tests brittle. Generic over the `Ok` type so one assertion
/// covers `execute(..)` (a `GeometryHandle`) and the with-history entry points
/// (a `(GeometryHandle, …Records)` pair).
fn assert_empty_input_rejected<T>(result: Result<T, GeometryError>, role_word: &str) {
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("empty"),
                "expected a message naming the input as empty, got: {msg}"
            );
            assert!(
                msg.contains(role_word),
                "expected a message naming '{role_word}', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for an empty '{role_word}', got Ok"),
        Err(other) => {
            panic!("expected OperationFailed for an empty '{role_word}', got {other:?}")
        }
    }
}

/// Assert `result` is an `Err` whose message mentions at least one of `words`.
///
/// Used by the CHARACTERIZATION PINS below, which record behaviour that already
/// exists rather than behaviour this task adds. Matching is deliberately weak:
/// the point is that the op refuses an empty profile at all, not that it
/// refuses it with any particular wording.
fn assert_rejected_mentioning_any<T: std::fmt::Debug>(
    result: Result<T, GeometryError>,
    words: &[&str],
    what: &str,
) {
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                words.iter().any(|w| msg.contains(w)),
                "{what}: expected a rejection mentioning one of {words:?}, got: {msg}"
            );
        }
        Ok(handle) => panic!("{what}: expected a rejection, got Ok({handle:?})"),
    }
}

// --- Extrude: an empty profile cannot be solidified ---

/// The direct `OcctKernel::execute` path, which reaches `make_prism`
/// (src/lib.rs `GeometryOp::Extrude` -> `ffi::ffi::make_prism`). Today this
/// validates only the distance scalar and hands the empty shape straight to
/// `BRepPrimAPI_MakePrism`.
#[test]
fn execute_extrude_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.execute(&GeometryOp::Extrude {
        profile: empty,
        distance: Value::Real(0.010),
    });
    assert_empty_input_rejected(result, "profile");
}

/// The PRODUCTION path: `handle.rs` routes `GeometryOp::Extrude` through
/// `extrude_with_history`, so a guard that covered only `execute` would leave
/// every real build unprotected.
#[test]
fn extrude_with_history_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.extrude_with_history(empty, 0.010);
    assert_empty_input_rejected(result, "profile");
}

/// `make_prism_infinite` is a separate FFI entry point with its own validation,
/// so it needs its own call site and its own pin.
#[test]
fn execute_extrude_infinite_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.execute(&GeometryOp::ExtrudeInfinite {
        profile: empty,
        axis: [0.0, 0.0, 1.0],
        both: false,
    });
    assert_empty_input_rejected(result, "profile");
}

// --- Revolve / sweep / loft: the rest of the profile-solidifying family ---

#[test]
fn execute_revolve_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.execute(&GeometryOp::Revolve {
        profile: empty,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: std::f64::consts::PI,
    });
    assert_empty_input_rejected(result, "profile");
}

#[test]
fn revolve_with_history_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.revolve_with_history(
        empty,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        std::f64::consts::PI,
    );
    assert_empty_input_rejected(result, "profile");
}

/// NOTE on what this does and does not cover: `make_pipe` downcasts its SPINE
/// with a bare `TopoDS::Wire(spine.shape)`, so an empty SPINE already dies as an
/// OCCT `Standard_TypeMismatch` with an unhelpful message. It is the empty
/// PROFILE that is genuinely silent today, and that is what these two pin.
#[test]
fn execute_sweep_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let path = straight_path(&mut kernel, 0.100);

    let result = kernel.execute(&GeometryOp::Sweep {
        profile: empty,
        path,
    });
    assert_empty_input_rejected(result, "profile");
}

#[test]
fn sweep_with_history_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let path = straight_path(&mut kernel, 0.100);

    let result = kernel.sweep_with_history(empty, path);
    assert_empty_input_rejected(result, "profile");
}

/// The OTHER profile is deliberately non-empty so the pre-existing
/// "requires at least 2 profiles" count check is not what fires.
#[test]
fn execute_loft_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let real = circle_wire(&mut kernel, 0.020, 0.050);

    let result = kernel.execute(&GeometryOp::Loft {
        profiles: vec![empty, real],
    });
    assert_empty_input_rejected(result, "profile");
}

#[test]
fn loft_with_history_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let real = circle_wire(&mut kernel, 0.020, 0.050);

    let result = kernel.loft_with_history(&[empty, real]);
    assert_empty_input_rejected(result, "profile");
}

// --- CHARACTERIZATION PINS: two guided variants are ALREADY covered ---
//
// These record EXISTING behaviour, not behaviour this task adds. Both guided
// entry points route their profile through `section_profile_to_wire`, whose
// default arm rejects an empty compound as "unsupported profile shape type
// 'Compound'". Step-6 deliberately adds NO guard at either site — a second
// guard there would be a duplicated invariant — so these pins are what stop a
// later refactor from dropping the coverage silently.

#[test]
fn sweep_guided_of_an_empty_profile_is_already_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let path = straight_path(&mut kernel, 0.100);
    let guide = offset_guide(&mut kernel, 0.020, 0.100);

    let result = kernel.execute(&GeometryOp::SweepGuided {
        profile: empty,
        path,
        guide,
    });
    assert_rejected_mentioning_any(result, &["Compound", "profile"], "guided sweep");
}

#[test]
fn loft_guided_of_an_empty_profile_is_already_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);
    let real = circle_wire(&mut kernel, 0.020, 0.050);
    let guide = straight_path(&mut kernel, 0.100);

    let result = kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![empty, real],
        guides: vec![guide],
    });
    assert_rejected_mentioning_any(result, &["Compound", "profile"], "guided loft");
}

// --- OVER-FIRE CONTROLS ---

/// The ordinary case the guard must leave alone: a real disk face extrudes to
/// the exact cylinder volume pi*r^2*h = pi*0.005^2*0.010.
#[test]
fn execute_extrude_of_a_real_face_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let profile = circle_profile(&mut kernel, 0.005);

    let handle = kernel
        .execute(&GeometryOp::Extrude {
            profile,
            distance: Value::Real(0.010),
        })
        .expect("extruding a real face must succeed");
    assert_volume_near(
        &kernel,
        handle.id,
        std::f64::consts::PI * 0.005_f64.powi(2) * 0.010,
        "extrude of a real disk face",
    );
}

/// Two axis-aligned 10-cubes offset 5 on X overlap in exactly 5*10*10 = 500.
#[test]
fn execute_intersection_of_overlapping_boxes_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 10.0);
    let right = translated_x(&mut kernel, right_raw, 5.0);

    let handle = kernel
        .execute(&GeometryOp::Intersection { left, right })
        .expect("overlapping intersection must succeed");
    assert_volume_near(&kernel, handle.id, 500.0, "overlapping intersection");
}

/// A tool that misses entirely returns the target unchanged: 10^3 = 1000.
#[test]
fn execute_difference_with_missing_tool_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 2.0);
    let right = translated_x(&mut kernel, right_raw, 100.0);

    let handle = kernel
        .execute(&GeometryOp::Difference { left, right })
        .expect("difference with a missing tool must succeed");
    assert_volume_near(&kernel, handle.id, 1000.0, "difference with missing tool");
}

/// A two-solid compound: BRepGProp sums the members, 2 * 10^3 = 2000.
#[test]
fn execute_union_of_disjoint_boxes_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 10.0);
    let right = translated_x(&mut kernel, right_raw, 100.0);

    let handle = kernel
        .execute(&GeometryOp::Union { left, right })
        .expect("union of disjoint solids must succeed");
    assert_volume_near(&kernel, handle.id, 2000.0, "union of disjoint solids");
}

/// A real rect profile offset from the axis revolves into a positive-volume
/// ring. Fixture shape mirrors `harness_occt::revolve_with_history_integration`.
#[test]
fn execute_revolve_of_a_real_profile_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let profile = offset_rect_profile(&mut kernel, 0.0175);

    let handle = kernel
        .execute(&GeometryOp::Revolve {
            profile,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [1.0, 0.0, 0.0],
            angle_rad: std::f64::consts::PI,
        })
        .expect("revolving a real profile must succeed");
    assert!(
        volume_of(&kernel, handle.id) > 0.0,
        "a real 180-degree revolve must enclose positive volume"
    );
}

/// A real disk face swept along a straight spine is a positive-volume cylinder.
#[test]
fn execute_sweep_of_a_real_profile_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let profile = circle_profile(&mut kernel, 0.005);
    let path = straight_path(&mut kernel, 0.100);

    let handle = kernel
        .execute(&GeometryOp::Sweep { profile, path })
        .expect("sweeping a real face must succeed");
    assert!(
        volume_of(&kernel, handle.id) > 0.0,
        "a real sweep must enclose positive volume"
    );
}

/// Two real circular WIRES loft into a positive-volume solid.
#[test]
fn execute_loft_of_real_profiles_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let bottom = circle_wire(&mut kernel, 0.020, 0.0);
    let top = circle_wire(&mut kernel, 0.010, 0.050);

    let handle = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![bottom, top],
        })
        .expect("lofting two real wires must succeed");
    assert!(
        volume_of(&kernel, handle.id) > 0.0,
        "a real loft must enclose positive volume"
    );
}

// --- STEP export: the LAST line of defence ---
//
// A design whose whole product geometry collapsed reaches export even when no
// sweep was involved, and `export_step` today DISCARDS `writer.Transfer`'s
// `IFSelect_ReturnStatus` (unlike `writer.Write`, which checks it) — so it
// returns Ok with header-only bytes and the CLI exits 0 on a phantom artifact.
//
// The error TYPE differs from the sweep guards: this path returns
// `ExportError::FormatError`, not `GeometryError::OperationFailed`
// (src/lib.rs:4316 in `export`, :4366 in `export_with_options`), so
// `assert_empty_input_rejected` does not apply and these get their own sibling
// assertion rather than one helper straddling two error enums.

/// Assert an export was refused as `ExportError::FormatError` naming the shape
/// as empty. On `Ok`, the panic REPORTS the phantom artifact that was written
/// instead — byte count and `ADVANCED_FACE` count — so the pre-guard behaviour
/// is measured by the test rather than asserted from memory.
fn assert_export_rejected_as_empty<T>(result: Result<T, ExportError>, buf: &[u8], what: &str) {
    match result {
        Err(ExportError::FormatError(msg)) => {
            assert!(
                msg.contains("empty"),
                "{what}: expected a message naming the shape as empty, got: {msg}"
            );
        }
        Ok(_) => panic!(
            "{what}: expected ExportError::FormatError, got Ok — {} bytes written, \
             {} ADVANCED_FACE",
            buf.len(),
            String::from_utf8_lossy(buf)
                .matches("ADVANCED_FACE")
                .count()
        ),
        Err(other) => panic!("{what}: expected FormatError, got {other:?}"),
    }
}

#[test]
fn export_step_of_an_empty_shape_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let mut buf = Vec::new();
    let result = kernel.export(empty, ExportFormat::Step, &mut buf);
    assert_export_rejected_as_empty(result, &buf, "export of an empty shape");
}

/// `export_with_options` is the entry point BOTH production export paths use
/// (engine_build.rs:4963 single-body, :5010 compound, :5585 declarative), so a
/// guard proven only on `export` would leave every real build unprotected.
#[test]
fn export_step_with_options_of_an_empty_shape_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let mut buf = Vec::new();
    let result = kernel.export_with_options(
        empty,
        ExportFormat::Step,
        &ExportOptions::default(),
        &mut buf,
    );
    assert_export_rejected_as_empty(result, &buf, "export_with_options of an empty shape");
}

// --- OVER-FIRE CONTROLS for the export guard ---

/// The ordinary case: a plain box still writes a real STEP file.
#[test]
fn export_step_of_a_real_box_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.020);

    let mut buf = Vec::new();
    kernel
        .export(solid, ExportFormat::Step, &mut buf)
        .expect("exporting a real solid must succeed");
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("ISO-10303-21"),
        "a real STEP export must carry the ISO-10303-21 header"
    );
    assert!(
        text.matches("ADVANCED_FACE").count() > 0,
        "a real STEP export must carry faces"
    );
}

/// LOAD-BEARING: a compound that CONTAINS an empty member alongside two real
/// solids must still export, with both solids intact.
///
/// This is the common real-design case, and it is what bounds the export
/// guard's blast radius: Phase-B compounds every product body BEFORE exporting
/// (engine_build.rs:4996-5010), and a compound holding a real solid HAS
/// topology, so the guard cannot fire on it. Only "the whole product
/// collapsed" reaches the guard.
#[test]
fn export_step_of_a_compound_holding_an_empty_member_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 0.020);
    let b_raw = cube(&mut kernel, 0.020);
    let b = translated_x(&mut kernel, b_raw, 0.100);
    let empty = empty_intersection(&mut kernel);

    let compound = kernel
        .make_compound(&[a, b, empty])
        .expect("make_compound of two boxes plus an empty member must succeed");

    let mut buf = Vec::new();
    kernel
        .export(compound.id, ExportFormat::Step, &mut buf)
        .expect("exporting a compound that holds real solids must succeed");
    assert_eq!(
        String::from_utf8_lossy(&buf)
            .matches("MANIFOLD_SOLID_BREP")
            .count(),
        2,
        "both real solids must survive the export; the empty member contributes none"
    );
}
