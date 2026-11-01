//! Pins the conformance verdict of every acceptance-divergence row at a struct-ctor
//! site and at a trait-carrying fn-call site, with one expected diagnostic list per
//! row asserted at both sites. Spec: docs/prds/v0_6/compiler-declaration-passes.md
//! §6 B11.

use reify_compiler::CompiledModule;
use reify_core::diagnostics::DiagnosticCode;
use reify_core::{Diagnostic, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only, is_ctor_conformance_code};

fn ctor_conformance_diags(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

fn non_ctor_conformance_errors(module: &CompiledModule) -> Vec<&Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| !is_ctor_conformance_code(d.code))
        .collect()
}

/// Which `Map` slot of the fn-call site's `probe(m : Map<…>)` param carries the
/// type under test; the other slot carries the trait that makes `m` reach the walker.
#[derive(Clone, Copy)]
enum FnSlot {
    Key,
    Value,
}

/// One divergence fixture, rendered as a struct-ctor site and as a fn-call site
/// whose param and struct field are both named `m`.
#[derive(Clone, Copy)]
struct Probe {
    name: &'static str,
    decls: &'static str,
    root_members: &'static str,
    param: &'static str,
    arg: &'static str,
    fn_slot: FnSlot,
}

impl Probe {
    const fn new(name: &'static str, param: &'static str, arg: &'static str) -> Self {
        Probe {
            name,
            decls: "",
            root_members: "",
            param,
            arg,
            fn_slot: FnSlot::Key,
        }
    }

    fn ctor_src(&self) -> String {
        let Probe {
            name,
            decls,
            root_members,
            param,
            arg,
            ..
        } = self;
        format!(
            "module test.{name}_ctor\n{decls}\nstructure def Holder {{ param m : {param} }}\n\
             structure def Root {{\n{root_members}\n    let x = Holder(m: {arg})\n}}\n"
        )
    }

    fn fn_src(&self) -> String {
        let Probe {
            name,
            decls,
            root_members,
            param,
            arg,
            fn_slot,
        } = self;
        let (map_ty, map_lit) = match fn_slot {
            FnSlot::Key => (
                format!("Map<{param}, Marker>"),
                format!("map{{{arg} => Thing()}}"),
            ),
            FnSlot::Value => (
                format!("Map<Marker, {param}>"),
                format!("map{{Thing() => {arg}}}"),
            ),
        };
        format!(
            "module test.{name}_fn\n{FN_SITE_TRAIT_DECLS}{decls}\n\
             fn probe(m : {map_ty}) -> Real {{ 0.0 }}\n\
             structure def Root {{\n{root_members}\n    let x = probe({map_lit})\n}}\n"
        )
    }

    fn assert_verdict(&self, expected: &[(DiagnosticCode, &str)]) {
        assert_row(&self.ctor_src(), &self.fn_src(), expected);
    }
}

const FN_SITE_TRAIT_DECLS: &str =
    "trait Marker {}\nstructure def Thing : Marker { param x : Real = 0.0 }\n";

/// Asserts that BOTH sources emit exactly `expected`.
fn assert_row(ctor_src: &str, fn_src: &str, expected: &[(DiagnosticCode, &str)]) {
    assert_site("ctor", ctor_src, expected);
    assert_site("fn-call", fn_src, expected);
}

/// Asserts that `src` emits exactly `expected` (code, Error severity, full message,
/// in order) as ctor-conformance diagnostics and no other error.
fn assert_site(site: &str, src: &str, expected: &[(DiagnosticCode, &str)]) {
    let module = compile_source_with_stdlib(src);
    let hygiene = non_ctor_conformance_errors(&module);
    assert!(
        hygiene.is_empty(),
        "{site} site: fixture must be well-formed apart from the row under test.\n\
         source:\n{src}\nnon-conformance errors: {hygiene:#?}"
    );
    let observed: Vec<(Option<DiagnosticCode>, Severity, &str)> = ctor_conformance_diags(&module)
        .into_iter()
        .map(|d| (d.code, d.severity, d.message.as_str()))
        .collect();
    let wanted: Vec<(Option<DiagnosticCode>, Severity, &str)> = expected
        .iter()
        .map(|&(code, message)| (Some(code), Severity::Error, message))
        .collect();
    assert_eq!(
        observed, wanted,
        "{site} site verdict differs.\nsource:\n{src}"
    );
}

const SILENT: &[(DiagnosticCode, &str)] = &[];

#[test]
fn vector_accepts_any_rank1_tensor() {
    Probe {
        root_members: "    param t : Tensor<1, 2, Length> = undef",
        ..Probe::new("vector_tensor", "Vector3<Length>", "t")
    }
    .assert_verdict(SILENT);
}

#[test]
fn vector_rejects_other_arity() {
    Probe::new("vector_arity", "Vector3<Length>", "vec2(1mm, 2mm)").assert_verdict(&[(
        DiagnosticCode::TypeNotConformingToVector,
        "argument 'm' has type 'Vector2<Scalar[m]>' but param 'm' requires vector type \
         'Vector3<Scalar[m]>'",
    )]);
}

#[test]
fn vector_quantity_slot_tolerates_dimensionless_arg_and_rejects_other_dimension() {
    Probe::new("vector_dimensionless", "Vector3<Length>", "vec3(1, 2, 3)").assert_verdict(SILENT);
    Probe::new("vector_mass", "Vector3<Length>", "vec3(1kg, 2kg, 3kg)").assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has quantity 'Scalar[kg]' but param 'm' requires quantity 'Scalar[m]' \
         (the compared shape 'Vector3<Scalar[kg]>' is otherwise accepted at \
         'Vector3<Scalar[m]>'; only the quantity slot disagrees)",
    )]);
}

#[test]
fn point_quantity_slot_rejects_other_dimension() {
    Probe::new("point_mass", "Point3<Length>", "point3(1kg, 2kg, 3kg)").assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has quantity 'Scalar[kg]' but param 'm' requires quantity 'Scalar[m]' \
         (the compared shape 'Point3<Scalar[kg]>' is otherwise accepted at \
         'Point3<Scalar[m]>'; only the quantity slot disagrees)",
    )]);
}

#[test]
fn point_accepts_numeric_placeholder() {
    Probe::new("point_int", "Point3<Length>", "5").assert_verdict(SILENT);
    Probe::new("point_scalar", "Point3<Length>", "5kg").assert_verdict(SILENT);
}

#[test]
fn matrix_quantity_slot_rejects_other_dimension() {
    Probe::new(
        "matrix_quantity",
        "Matrix<3, 3, MomentOfInertia>",
        "matrix([[1mm, 0mm, 0mm], [0mm, 1mm, 0mm], [0mm, 0mm, 1mm]])",
    )
    .assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has quantity 'Scalar[m]' but param 'm' requires quantity \
         'Scalar[m^2·kg]' (the compared shape 'Tensor2x3<Scalar[m]>' is otherwise accepted \
         at 'Matrix3x3<Scalar[m^2·kg]>'; only the quantity slot disagrees)",
    )]);
}

#[test]
fn matrix_does_not_judge_numeric_nested_list_and_rejects_string_list() {
    Probe::new(
        "matrix_nested_list",
        "Matrix<3, 3, Length>",
        "[[1, 0, 0], [0, 1, 0], [0, 0, 1]]",
    )
    .assert_verdict(SILENT);
    Probe::new(
        "matrix_string_list",
        "Matrix<3, 3, Length>",
        "[\"a\", \"b\"]",
    )
    .assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has type 'List<String>' but param 'm' requires type \
         'Matrix3x3<Scalar[m]>'",
    )]);
}

#[test]
fn matrix_accepts_scalar_leg() {
    Probe::new("matrix_scalar", "Matrix<3, 3, Length>", "5kg").assert_verdict(SILENT);
}

#[test]
fn field_does_not_judge_lambda_or_field_def_and_rejects_string() {
    const FIELD: &str = "Field<Point3<Length>, Real>";
    Probe {
        fn_slot: FnSlot::Value,
        ..Probe::new("field_lambda", FIELD, "|p| 1.0")
    }
    .assert_verdict(SILENT);
    Probe {
        fn_slot: FnSlot::Value,
        decls: "field def scalar_field : Real -> Real { source = analytical { |x| 100.0 } }",
        ..Probe::new("field_def", FIELD, "scalar_field")
    }
    .assert_verdict(SILENT);
    Probe {
        fn_slot: FnSlot::Value,
        ..Probe::new("field_string", FIELD, "\"nope\"")
    }
    .assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has type 'String' but param 'm' requires type \
         'Field<Point3<Scalar[m]>, Real>'",
    )]);
}

#[test]
fn enum_erases_generic_arguments_and_rejects_other_enum() {
    // Erasure is ctor-only: no fn param spelling of an applied generic enum resolves
    // (`unresolved type: Map<Choice<Length>, Marker>`), and a bare `Choice` is an arity error.
    let choice_param = |name, arg| Probe {
        decls: "enum Choice<T> { A, B }\nenum Hue { Red, Green }",
        ..Probe::new(name, "Choice<Length>", arg)
    };
    assert_site(
        "ctor",
        &choice_param("enum_erasure", "Choice.A").ctor_src(),
        SILENT,
    );
    assert_site(
        "ctor",
        &choice_param("enum_generic_cross", "Hue.Red").ctor_src(),
        &[(
            DiagnosticCode::ArgTypeMismatch,
            "argument 'm' has type 'Enum(Hue)' but param 'm' requires type 'Choice<Scalar[m]>'",
        )],
    );
    Probe {
        decls: "enum Choice<T> { A, B }\nenum Hue { Red, Green }",
        ..Probe::new("enum_cross", "Hue", "Choice.A")
    }
    .assert_verdict(&[(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'm' has type 'Enum(Choice)' but param 'm' requires type 'Enum(Hue)'",
    )]);
}

/// Both sites with the call inside a dimension-generic fn forwarding `arg` to a
/// `Length` slot.
fn forwarding_sources(name: &str, arg: &str) -> (String, String) {
    let ctor = format!(
        "module test.{name}_ctor\nstructure def Holder {{ param m : Length }}\n\
         fn fwd<Q: Dimension>(x : Scalar<Q>) -> Holder {{ Holder(m: {arg}) }}\n"
    );
    let fn_call = format!(
        "module test.{name}_fn\n{FN_SITE_TRAIT_DECLS}\
         fn probe(m : Map<Length, Marker>) -> Real {{ 0.0 }}\n\
         fn fwd<Q: Dimension>(x : Scalar<Q>) -> Real {{ probe(map{{{arg} => Thing()}}) }}\n"
    );
    (ctor, fn_call)
}

#[test]
fn scalar_slot_defers_scalar_param_and_rejects_string() {
    let (ctor, fn_call) = forwarding_sources("scalar_param_forward", "x");
    assert_row(&ctor, &fn_call, SILENT);
    let (ctor, fn_call) = forwarding_sources("scalar_param_string", "\"s\"");
    assert_row(
        &ctor,
        &fn_call,
        &[(
            DiagnosticCode::ArgTypeMismatch,
            "argument 'm' has type 'String' but param 'm' requires type 'Scalar[m]'; pass a \
             dimensioned Length literal such as `1m`",
        )],
    );
}
