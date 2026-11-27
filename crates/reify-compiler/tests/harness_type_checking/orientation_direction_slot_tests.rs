//! `orient_axis_angle`, `orient_look_at` and `orient_basis` take `Direction`
//! slots: a vector or string there is a static `ArgTypeMismatch`, and a
//! Direction-typed value checks clean (PRD `docs/prds/v0_6/direction-typing.md`
//! B14, task θ-a #9644).

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::design_files::{DEV_CAPSTAN, PRINTER_RI, read_design_file};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

/// A structure body holding a point `o`, a let-bound Direction `d`, then `lines`.
fn orientation_source(lines: &[&str]) -> String {
    let mut body = String::from("    let o = point3(0mm, 0mm, 0mm)\n    let d = dir(0, 1, 0)\n");
    for line in lines {
        body.push_str("    ");
        body.push_str(line);
        body.push('\n');
    }
    format!("structure def OrientationDirections {{\n{body}}}\n")
}

fn errors_of(line: &str) -> Vec<Diagnostic> {
    let module = compile_source_with_stdlib(&orientation_source(&[line]));
    errors_only(&module).into_iter().cloned().collect()
}

/// Asserts `errors` are exactly one Error-severity `ArgTypeMismatch` per
/// prefix, each message starting with its prefix; returns the messages in
/// prefix order.
fn assert_mismatches(errors: &[Diagnostic], prefixes: &[&str], case: &str) -> Vec<String> {
    assert_eq!(
        errors.len(),
        prefixes.len(),
        "{case}: expected one Error per prefix {prefixes:?}, got {errors:?}"
    );
    prefixes
        .iter()
        .map(|prefix| {
            let diagnostic = errors
                .iter()
                .find(|d| d.message.starts_with(prefix))
                .unwrap_or_else(|| panic!("{case}: no Error starts {prefix:?}: {errors:?}"));
            assert_eq!(
                diagnostic.code,
                Some(DiagnosticCode::ArgTypeMismatch),
                "{case}: {diagnostic:?}"
            );
            assert_eq!(
                diagnostic.severity,
                Severity::Error,
                "{case}: {diagnostic:?}"
            );
            diagnostic.message.clone()
        })
        .collect()
}

fn assert_hinted(messages: &[String], case: &str) {
    for message in messages {
        assert!(
            message.ends_with(DIRECTION_HINT),
            "{case}: a vector direction must carry the dir(...) fix hint: {message:?}"
        );
    }
}

#[test]
fn b14_a_vector_axis_is_one_mismatch_with_the_dir_hint() {
    for operand in ["vec3(1m, 0mm, 0mm)", "vec3(1, 0, 0)"] {
        let line = format!("let q = orient_axis_angle({operand}, 90deg)");
        let messages = assert_mismatches(
            &errors_of(&line),
            &["orient_axis_angle: axis argument expects Direction, got "],
            &line,
        );
        assert_hinted(&messages, &line);
    }
}

#[test]
fn look_at_reports_one_hinted_mismatch_per_vector_operand() {
    let line = "let q = orient_look_at(vec3(1, 0, 0), vec3(0, 0, 1))";
    let messages = assert_mismatches(
        &errors_of(line),
        &[
            "orient_look_at: forward argument expects Direction",
            "orient_look_at: up argument expects Direction",
        ],
        line,
    );
    assert_hinted(&messages, line);

    for (line, prefix) in [
        (
            "let q = orient_look_at(vec3(1, 0, 0), dir(0, 0, 1))",
            "orient_look_at: forward argument expects Direction",
        ),
        (
            "let q = orient_look_at(dir(1, 0, 0), vec3(0, 0, 1))",
            "orient_look_at: up argument expects Direction",
        ),
    ] {
        let messages = assert_mismatches(&errors_of(line), &[prefix], line);
        assert_hinted(&messages, line);
    }
}

#[test]
fn basis_reports_one_hinted_mismatch_per_vector_axis() {
    let line = "let q = orient_basis(vec3(1, 0, 0), vec3(0, 1, 0), vec3(0, 0, 1))";
    let messages = assert_mismatches(
        &errors_of(line),
        &[
            "orient_basis: x_axis argument expects Direction",
            "orient_basis: y_axis argument expects Direction",
            "orient_basis: z_axis argument expects Direction",
        ],
        line,
    );
    assert_hinted(&messages, line);
}

#[test]
fn a_string_axis_is_one_mismatch_without_the_dir_hint() {
    let line = "let q = orient_axis_angle(\"z\", 90deg)";
    let messages = assert_mismatches(
        &errors_of(line),
        &["orient_axis_angle: axis argument expects Direction"],
        line,
    );
    assert!(
        !messages[0].contains("write dir"),
        "{line}: only a vector earns the dir(...) hint: {:?}",
        messages[0]
    );
}

#[test]
fn direction_typed_operands_check_clean() {
    for operand in ["dir(1, 0, 0)", "d", "axis_z(o).dir"] {
        for line in [
            format!("let q = orient_axis_angle({operand}, 90deg)"),
            format!("let q = orient_look_at({operand}, dir(0, 0, 1))"),
            format!("let q = orient_basis({operand}, {operand}, {operand})"),
        ] {
            let errors = errors_of(&line);
            assert!(
                errors.is_empty(),
                "{line:?} must check clean, got {errors:?}"
            );
        }
    }
}

/// Kernel-free compile only: printer.ri crashes the engine under check/eval
/// (#7383). Both design files compile with no Error at all, the stronger form of
/// "no orientation-slot mismatch".
#[test]
fn the_printer_design_files_compile_without_errors() {
    for design_file in [PRINTER_RI, DEV_CAPSTAN] {
        let module = compile_source_with_stdlib(&read_design_file(design_file));
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "{design_file} must compile clean, got {errors:?}"
        );
    }
}
