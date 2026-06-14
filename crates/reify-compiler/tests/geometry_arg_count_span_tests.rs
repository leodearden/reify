//! Tests for source-span labels on geometry arg-count diagnostics (task 487).
//!
//! Each arg-count error for the in-scope geometry functions (box, cylinder,
//! sphere, linear_pattern, circular_pattern, mirror, union (+ siblings),
//! shell, thicken, draft) must attach a `DiagnosticLabel` with a non-empty
//! `SourceSpan`. Pattern mirrors the assertions in
//! `diagnostic_coverage_checkpoint.rs`.
//!
//! shell/thicken/draft tests serve as regression guards — labels are already
//! attached via `compile_modify_op`; these tests lock in that behavior.

use reify_test_support::{compile_source, errors_only};

/// Compile `source`, locate the first error whose message contains `needle`,
/// and assert it carries at least one diagnostic label with a non-empty span.
///
/// Every test in this file follows the same three-step pattern (find error →
/// assert label present → assert span non-empty), so centralizing it here
/// keeps the individual tests one-liners and makes adding future regressions
/// trivial.
#[track_caller]
fn assert_arg_count_label(source: &str, needle: &str) {
    let module = compile_source(source);
    let errors = errors_only(&module);

    let first = errors
        .iter()
        .find(|d| d.message.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "expected '{}' error, got: {:?}",
                needle,
                errors.iter().map(|d| &d.message).collect::<Vec<_>>()
            )
        });
    assert!(
        !first.labels.is_empty(),
        "expected at least one label on '{}' diagnostic",
        needle
    );
    assert!(
        !first.labels[0].span.is_empty(),
        "expected non-empty span on '{}' label",
        needle
    );
    assert_eq!(
        first.labels[0].message, "wrong number of arguments",
        "expected 'wrong number of arguments' label text on '{}' diagnostic",
        needle
    );
}

// ── box() ──────────────────────────────────────────────────────────────

#[test]
fn box_arg_count_diagnostic_has_span_label() {
    // box() expects 3 arguments — passing only 2 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let shape = box(10mm, 20mm)
            }
        "#,
        "box() expects 3 arguments",
    );
}

// ── cylinder() ─────────────────────────────────────────────────────────

#[test]
fn cylinder_arg_count_diagnostic_has_span_label() {
    // cylinder() expects 2 arguments — passing only 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = cylinder(10mm)
            }
        "#,
        "cylinder() expects 2 arguments",
    );
}

// ── sphere() ───────────────────────────────────────────────────────────

#[test]
fn sphere_arg_count_diagnostic_has_span_label() {
    // sphere() expects 1 argument — passing 0 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let s = sphere()
            }
        "#,
        "sphere() expects 1 argument",
    );
}

// ── linear_pattern() ───────────────────────────────────────────────────

#[test]
fn linear_pattern_arg_count_diagnostic_has_span_label() {
    // linear_pattern() expects 6 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let p = linear_pattern(box(10mm, 10mm, 10mm))
            }
        "#,
        "linear_pattern() expects 6 arguments",
    );
}

// ── circular_pattern() ─────────────────────────────────────────────────

#[test]
fn circular_pattern_arg_count_diagnostic_has_span_label() {
    // circular_pattern() expects 9 arguments — passing 2 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let p = circular_pattern(box(10mm, 10mm, 10mm), 1.0)
            }
        "#,
        "circular_pattern() expects 9 arguments",
    );
}

// ── mirror() ───────────────────────────────────────────────────────────

#[test]
fn mirror_arg_count_diagnostic_has_span_label() {
    // mirror() expects 7 arguments — passing 3 should produce a labeled diagnostic
    // (2 args is now valid as the plane-value form: mirror(target, plane_value))
    assert_arg_count_label(
        r#"
            structure S {
                let m = mirror(box(10mm, 10mm, 10mm), 1.0, 2.0)
            }
        "#,
        "mirror() expects 7 arguments",
    );
}

// ── linear_pattern_2d() ────────────────────────────────────────────────

#[test]
fn linear_pattern_2d_arg_count_diagnostic_has_span_label() {
    // linear_pattern_2d() expects 11 arguments — passing 2 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let p = linear_pattern_2d(box(10mm, 10mm, 10mm), 1mm)
            }
        "#,
        "linear_pattern_2d() expects 11 arguments",
    );
}

// ── union() / intersection() / difference() ────────────────────────────

#[test]
fn union_arg_count_diagnostic_has_span_label() {
    // union() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let u = union(box(10mm, 10mm, 10mm))
            }
        "#,
        "union() expects 2 arguments",
    );
}

#[test]
fn intersection_arg_count_diagnostic_has_span_label() {
    // intersection() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let i = intersection(box(10mm, 10mm, 10mm))
            }
        "#,
        "intersection() expects 2 arguments",
    );
}

#[test]
fn difference_arg_count_diagnostic_has_span_label() {
    // difference() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let d = difference(box(10mm, 10mm, 10mm))
            }
        "#,
        "difference() expects 2 arguments",
    );
}

// ── union_all() / intersection_all() ───────────────────────────────────

#[test]
fn union_all_arg_count_diagnostic_has_span_label() {
    // union_all() expects at least 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let u = union_all(box(10mm, 10mm, 10mm))
            }
        "#,
        "union_all() expects at least 2 arguments",
    );
}

#[test]
fn intersection_all_arg_count_diagnostic_has_span_label() {
    // intersection_all() expects at least 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let i = intersection_all(box(10mm, 10mm, 10mm))
            }
        "#,
        "intersection_all() expects at least 2 arguments",
    );
}

// ── shell() / thicken() / draft() — regression guards ──────────────────
//
// These ops already attach labels via `compile_modify_op`; the tests below
// lock in that behavior so a future refactor cannot silently strip the
// labels without tripping a test.

#[test]
fn shell_arg_count_diagnostic_has_span_label() {
    // shell() expects at least 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let s = shell(box(10mm, 10mm, 10mm))
            }
        "#,
        "shell() expects at least 2 arguments",
    );
}

#[test]
fn thicken_arg_count_diagnostic_has_span_label() {
    // thicken() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let t = thicken(box(10mm, 10mm, 10mm))
            }
        "#,
        "thicken() expects 2 arguments",
    );
}

#[test]
fn offset_solid_arg_count_diagnostic_has_span_label() {
    // offset_solid() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let s = offset_solid(box(10mm, 10mm, 10mm))
            }
        "#,
        "offset_solid() expects 2 arguments",
    );
}

#[test]
fn draft_arg_count_diagnostic_has_span_label() {
    // draft() expects 3 or 4 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let d = draft(box(10mm, 10mm, 10mm))
            }
        "#,
        "draft() expects 3 or 4 arguments",
    );
}

#[test]
fn chamfer_arg_count_diagnostic_has_span_label() {
    // chamfer() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = chamfer(box(10mm, 10mm, 10mm))
            }
        "#,
        "chamfer() expects 2 arguments",
    );
}

#[test]
fn fillet_arg_count_diagnostic_has_span_label() {
    // fillet() accepts 2 args (all-edges) or 3 args (curated edges); passing 1
    // should produce a labeled diagnostic naming both valid arities (mirrors the
    // multi-arity message convention used by mirror() in geometry.rs).
    assert_arg_count_label(
        r#"
            structure S {
                let f = fillet(box(10mm, 10mm, 10mm))
            }
        "#,
        "fillet() expects 2 or 3 arguments",
    );
}

#[test]
fn fillet_all_arg_count_diagnostic_has_span_label() {
    // fillet_all() expects exactly 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let f = fillet_all(box(10mm, 10mm, 10mm))
            }
        "#,
        "fillet_all() expects 2 arguments",
    );
}

// ── translate() / rotate() / scale() / rotate_around() ─────────────────

#[test]
fn translate_arg_count_diagnostic_has_span_label() {
    // translate() expects 4 arguments — passing 2 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let t = translate(1mm, 2mm)
            }
        "#,
        "translate() expects 4 arguments",
    );
}

#[test]
fn rotate_arg_count_diagnostic_has_span_label() {
    // rotate() expects 5 arguments — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let r = rotate(box(10mm, 10mm, 10mm), 0.0, 0.0)
            }
        "#,
        "rotate() expects 5 arguments",
    );
}

#[test]
fn scale_arg_count_diagnostic_has_span_label() {
    // scale() expects 2 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let s = scale(box(10mm, 10mm, 10mm))
            }
        "#,
        "scale() expects 2 arguments",
    );
}

#[test]
fn rotate_around_arg_count_diagnostic_has_span_label() {
    // rotate_around() expects 8 arguments — passing 2 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let r = rotate_around(box(10mm, 10mm, 10mm), 1mm)
            }
        "#,
        "rotate_around() expects 8 arguments",
    );
}

// ── line_segment() / arc() / helix() ───────────────────────────────────

#[test]
fn line_segment_arg_count_diagnostic_has_span_label() {
    // line_segment() expects 6 arguments — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = line_segment(0mm, 0mm, 0mm)
            }
        "#,
        "line_segment() expects 6 arguments",
    );
}

#[test]
fn arc_arg_count_diagnostic_has_span_label() {
    // arc() expects 9 arguments — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = arc(0mm, 0mm, 0mm)
            }
        "#,
        "arc() expects 9 arguments",
    );
}

#[test]
fn helix_arg_count_diagnostic_has_span_label() {
    // helix() expects 3 arguments — passing 1 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = helix(10mm)
            }
        "#,
        "helix() expects 3 arguments",
    );
}

// ── interp() / bezier() / nurbs() ──────────────────────────────────────

#[test]
fn interp_arg_count_diagnostic_has_span_label() {
    // interp() expects coordinate triples (at least 6 args) — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = interp(0mm, 0mm, 0mm)
            }
        "#,
        "interp() expects coordinate triples",
    );
}

#[test]
fn bezier_arg_count_diagnostic_has_span_label() {
    // bezier() expects coordinate triples (at least 6 args) — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = bezier(0mm, 0mm, 0mm)
            }
        "#,
        "bezier() expects coordinate triples",
    );
}

#[test]
fn nurbs_arg_count_diagnostic_has_span_label() {
    // nurbs() expects at least 10 arguments — passing 3 should produce a labeled diagnostic
    assert_arg_count_label(
        r#"
            structure S {
                let c = nurbs(0mm, 0mm, 0mm)
            }
        "#,
        "nurbs() expects at least 10 arguments",
    );
}
