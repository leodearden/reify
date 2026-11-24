//! Datum slots (`Direction`, `Axis`, `Plane`, `Frame`) at ctor fields and param
//! defaults accept only their own type (`docs/prds/v0_6/direction-typing.md`
//! C7 / D8, rows B22, B8 and the type half of B6).

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only, is_ctor_conformance_code};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

fn ctor_conformance_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

/// `A` declares `decl`; `Root` constructs it as `A(<field>: <arg>)`.
fn ctor_field_source(decl: &str, field: &str, arg: &str) -> String {
    format!(
        "structure def A {{\n    {decl}\n}}\n\
         structure def Root {{\n    let x = A({field}: {arg})\n}}\n"
    )
}

fn assert_one_arg_type_mismatch_naming<'m>(
    module: &'m reify_compiler::CompiledModule,
    field: &str,
    case: &str,
) -> &'m Diagnostic {
    let errors = ctor_conformance_errors(module);
    assert_eq!(
        errors.len(),
        1,
        "{case}: expected one ctor-conformance Error, got {errors:?}"
    );
    let diagnostic = errors[0];
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
    assert!(
        diagnostic.message.contains(&format!("'{field}'")),
        "{case}: the mismatch must name '{field}': {diagnostic:?}"
    );
    diagnostic
}

fn assert_checks_clean(source: &str, case: &str) {
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "{case} must check clean, got {errors:?}");
}

#[test]
fn each_datum_slot_rejects_a_foreign_argument_at_a_ctor_field() {
    // `Frame` displays as `Frame3`, so its substring is the family name alone.
    for (decl, field, arg, type_name) in [
        ("param f : Frame", "f", "\"hello\"", "Frame"),
        ("param a : Axis", "a", "5", "Axis"),
        ("param p : Plane", "p", "vec3(0, 0, 1)", "Plane"),
        ("param d : Direction", "d", "\"hello\"", "Direction"),
    ] {
        let module = compile_source_with_stdlib(&ctor_field_source(decl, field, arg));
        let case = format!("{decl} <- {arg}");
        let diagnostic = assert_one_arg_type_mismatch_naming(&module, field, &case);
        assert!(
            diagnostic.message.contains(type_name),
            "{case}: the mismatch must name the required type {type_name}: {diagnostic:?}"
        );
    }
}

#[test]
fn the_task_repro_reports_one_mismatch_per_datum_field() {
    let source = "structure def L {\n    \
                  param d : Direction = undef\n    \
                  param p : Plane = undef\n    \
                  param a : Axis = undef\n    \
                  param f : Frame = undef\n}\n\
                  structure def Root {\n    let l = L(d: \"x\", p: 42, a: \"y\", f: 7)\n}\n";
    let module = compile_source_with_stdlib(source);
    let errors = ctor_conformance_errors(&module);

    assert_eq!(errors.len(), 4, "one Error per datum field, got {errors:?}");
    for diagnostic in &errors {
        assert_eq!(
            diagnostic.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "{diagnostic:?}"
        );
    }
    for field in ["d", "p", "a", "f"] {
        let naming = format!("argument '{field}'");
        assert_eq!(
            errors
                .iter()
                .filter(|d| d.message.contains(&naming))
                .count(),
            1,
            "exactly one Error must name '{field}': {errors:?}"
        );
    }
}

#[test]
fn a_direction_param_default_rejects_a_string() {
    let module =
        compile_source_with_stdlib("structure def A {\n    param d : Direction = \"hello\"\n}\n");
    assert_one_arg_type_mismatch_naming(&module, "d", "Direction default <- String");
}

#[test]
fn datum_slots_accept_datum_values() {
    let root_prelude = "let o = point3(0mm, 0mm, 0mm)\n    \
                        let fr = frame_at(o, dir(1, 0, 0), dir(0, 0, 1))\n    \
                        param dd : Direction";
    let ctor_cases = [
        ("param d : Direction", "d", "dir(0, 1, 0)"),
        ("param d : Direction", "d", "axis_y(o).dir"),
        ("param d : Direction", "d", "plane_xy(0mm).normal"),
        ("param d : Direction", "d", "fr.z"),
        ("param d : Direction", "d", "dd"),
        ("param a : Axis", "a", "axis_x(o)"),
        (
            "param a : Axis",
            "a",
            "axis_through(o, point3(0mm, 0mm, 1mm))",
        ),
        ("param p : Plane", "p", "plane_xy(0mm)"),
        ("param p : Plane", "p", "fr.xy_plane"),
        ("param f : Frame", "f", "fr"),
        ("param f : Frame", "f", "frame3(o, orient_identity())"),
    ];
    for (decl, field, arg) in ctor_cases {
        let source = format!(
            "structure def A {{\n    {decl}\n}}\n\
             structure def Root {{\n    {root_prelude}\n    let x = A({field}: {arg})\n}}\n"
        );
        assert_checks_clean(&source, &format!("{decl} <- {arg}"));
    }
    for default in ["axis_y(point3(0mm, 0mm, 0mm)).dir", "undef"] {
        let source = format!("structure def A {{\n    param d : Direction = {default}\n}}\n");
        assert_checks_clean(&source, &format!("Direction default {default}"));
    }
}

#[test]
fn a_vector3_at_a_direction_ctor_field_ends_with_the_dir_hint() {
    for arg in [
        "vec3(0, 1, 0)",
        "vec3(0, 0, 0)",
        "vec3(0.0, 1.0, 0.0)",
        "vec3(1mm, 0mm, 0mm)",
    ] {
        let module =
            compile_source_with_stdlib(&ctor_field_source("param d : Direction", "d", arg));
        let case = format!("Direction <- {arg}");
        let diagnostic = assert_one_arg_type_mismatch_naming(&module, "d", &case);
        assert!(
            diagnostic.message.contains("Vector3")
                && diagnostic.message.contains("requires type 'Direction'"),
            "{case}: {diagnostic:?}"
        );
        assert!(
            diagnostic.message.ends_with(&format!("; {DIRECTION_HINT}")),
            "{case}: a Vector3 at a Direction slot must carry the dir(...) hint: {diagnostic:?}"
        );
    }
}

#[test]
fn a_vector3_direction_param_default_ends_with_the_dir_hint() {
    let module = compile_source_with_stdlib(
        "structure def A {\n    param d : Direction = vec3(0, 0, 1)\n}\n",
    );
    let diagnostic =
        assert_one_arg_type_mismatch_naming(&module, "d", "Direction default <- Vector3");
    assert!(
        diagnostic.message.ends_with(&format!("; {DIRECTION_HINT}")),
        "{diagnostic:?}"
    );
}

#[test]
fn only_a_vector3_at_a_direction_slot_gets_the_dir_hint() {
    for (decl, field, arg) in [
        ("param p : Plane", "p", "vec3(0, 0, 1)"),
        ("param d : Direction", "d", "vec2(1, 0)"),
        ("param d : Direction", "d", "\"hello\""),
    ] {
        let module = compile_source_with_stdlib(&ctor_field_source(decl, field, arg));
        let case = format!("{decl} <- {arg}");
        let diagnostic = assert_one_arg_type_mismatch_naming(&module, field, &case);
        assert!(
            !diagnostic.message.contains("write dir"),
            "{case}: the dir(...) hint is for a Vector3 at a Direction slot only: {diagnostic:?}"
        );
    }
}

#[test]
fn every_conformance_entry_judges_a_datum_slot() {
    let sub_construction = "structure def A {\n    param d : Direction\n}\n\
                            structure def Root {\n    sub x = A(d: \"hello\")\n}\n";
    let port_body_default = "trait P {}\n\
                             structure def Root {\n    port mount : P {\n        \
                             param d : Direction = \"hello\"\n    }\n}\n";
    for (case, source, field) in [
        ("sub construction", sub_construction.to_owned(), "d"),
        ("port-body default", port_body_default.to_owned(), "mount.d"),
        (
            "Axis default",
            "structure def A {\n    param a : Axis = 5\n}\n".to_owned(),
            "a",
        ),
        (
            "Plane default",
            "structure def A {\n    param p : Plane = 5\n}\n".to_owned(),
            "p",
        ),
        (
            "Frame default",
            "structure def A {\n    param f : Frame = \"hello\"\n}\n".to_owned(),
            "f",
        ),
    ] {
        let module = compile_source_with_stdlib(&source);
        assert_one_arg_type_mismatch_naming(&module, field, case);
    }
}

#[test]
fn a_wrapped_direction_slot_keeps_the_dir_hint() {
    for (decl, arg) in [
        ("param d : Option<Direction>", "vec3(0, 0, 1)"),
        ("param d : List<Direction>", "[vec3(0, 0, 1)]"),
    ] {
        let module = compile_source_with_stdlib(&ctor_field_source(decl, "d", arg));
        let case = format!("{decl} <- {arg}");
        let diagnostic = assert_one_arg_type_mismatch_naming(&module, "d", &case);
        assert!(
            diagnostic.message.ends_with(&format!("; {DIRECTION_HINT}")),
            "{case}: {diagnostic:?}"
        );
    }
}

/// A user structure named `Frame3` is not the `Frame` datum, although
/// `Type::Frame(3)` displays as `Frame3`, so the message reads `'Frame3'` on
/// both sides.
#[test]
fn a_frame_slot_rejects_a_user_structure_named_frame3() {
    let source = format!(
        "structure def Frame3 {{\n    param origin : Point3<Length>\n}}\n{}",
        ctor_field_source(
            "param f : Frame",
            "f",
            "Frame3(origin: point3(0mm, 0mm, 0mm))"
        )
    );
    let module = compile_source_with_stdlib(&source);
    assert_one_arg_type_mismatch_naming(&module, "f", "Frame <- user Frame3 structure");
}

/// The ctor-slot hint gate and the builtin-slot one
/// (`builtin_signatures::direction_mismatch_hint`) are separate code; this
/// differential holds them to one answer, a hint exactly for a `Vector3`.
#[test]
fn ctor_and_builtin_direction_slots_agree_on_the_dir_hint() {
    for (arg, earns_hint) in [
        ("vec3(0, 1, 0)", true),
        ("vec3(0.0, 1.0, 0.0)", true),
        ("vec3(1mm, 0mm, 0mm)", true),
        ("vec2(1, 0)", false),
        ("5", false),
        ("\"hello\"", false),
    ] {
        let ctor = compile_source_with_stdlib(&ctor_field_source("param d : Direction", "d", arg));
        let ctor_diagnostic =
            assert_one_arg_type_mismatch_naming(&ctor, "d", &format!("ctor field <- {arg}"));

        let builtin = compile_source_with_stdlib(&format!(
            "structure def Root {{\n    let o = point3(0mm, 0mm, 0mm)\n    \
             let f = frame_at(o, dir(1, 0, 0), {arg})\n}}\n"
        ));
        let builtin_errors = errors_only(&builtin);
        assert_eq!(
            builtin_errors.len(),
            1,
            "frame_at z <- {arg}: expected one Error, got {builtin_errors:?}"
        );
        let builtin_diagnostic = builtin_errors[0];
        assert_eq!(
            builtin_diagnostic.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "frame_at z <- {arg}: {builtin_diagnostic:?}"
        );

        let hinted = format!("; {DIRECTION_HINT}");
        for (slot, diagnostic) in [
            ("ctor field", ctor_diagnostic),
            ("frame_at z", builtin_diagnostic),
        ] {
            assert_eq!(
                diagnostic.message.ends_with(&hinted),
                earns_hint,
                "{slot} <- {arg}: the dir(...) hint is for a Vector3 only: {diagnostic:?}"
            );
        }
    }
}
