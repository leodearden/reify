//! Type-preserving pattern binders over generic enums — task δ #4032.
//!
//! Tests for:
//! (a) Annotation layer (S1/S2): `Result<Length, String>` as a structure param
//!     resolves to `Type::Applied { name: "Result", args: [Length, String] }` with NO
//!     errors (not the current "enum `Result` does not accept type arguments" rejection).
//! (b) Binder substitution (S3/S4): `match r { Ok { value: v } => v + 1mm, ... }`
//!     over `r : Result<Length, String>` types `v` as `Length` so `v + 1mm` is clean
//!     and `v + 1N` produces a dimension-mismatch error.
//! (c) Exhaustiveness (S5/S6): non-exhaustive matches over a `Type::Applied` discriminant
//!     still emit the missing-variant diagnostic (DCE D4 preserved for generic enums).

use crate::common::compile_with_stdlib_helper;
use reify_core::{Severity, Type};

// ─── Shared fixtures ─────────────────────────────────────────────────────────

/// Source for `enum Result<T, E>` — the generic two-param enum used across multiple tests.
const RESULT_ENUM_SOURCE: &str = "\
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
";

// ═══════════════════════════════════════════════════════════════════════════════
// S1 — RED: annotation layer (S2 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (a) δ signal: `param r : Result<Length, String>` must resolve `r` to
/// `Type::Applied { name: "Result", args: [Type::length(), Type::String] }` with
/// NO Error-severity diagnostics.
///
/// RED until S2: today `resolve_enum_type` drops args and emits
/// "enum `Result` does not accept type arguments".
#[test]
fn annotation_generic_enum_applied_type_cell_type() {
    let source =
        format!("{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length, String> }}");
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for Result<Length, String> param; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "D")
        .expect("D template must exist");

    let r_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "r")
        .expect("D must have a value cell named 'r'");

    let expected = Type::Applied {
        name: "Result".to_string(),
        args: vec![Type::length(), Type::String],
    };
    assert_eq!(
        r_cell.cell_type, expected,
        "Result<Length, String> must resolve to Applied{{\"Result\", [Length, String]}}, got {:?}",
        r_cell.cell_type
    );
}

/// INV-6: a NON-generic enum `Dir` used as `param d : Dir` still resolves to
/// `Type::Enum("Dir")` with no errors — the plain `resolve_enum_type` path is
/// unchanged by δ.
#[test]
fn annotation_non_generic_enum_cell_type_is_enum() {
    let source = r#"
enum Dir { In, Out }
structure def UseDir {
    param d : Dir
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for non-generic Dir param; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseDir")
        .expect("UseDir template must exist");

    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("UseDir must have a value cell named 'd'");

    assert_eq!(
        d_cell.cell_type,
        Type::Enum("Dir".to_string()),
        "non-generic Dir must resolve to Type::Enum(\"Dir\"), got {:?}",
        d_cell.cell_type
    );
}

// ─── Arity enforcement ───────────────────────────────────────────────────────

/// A generic enum given too FEW type args (`Result<Length>`) must produce an
/// arity-mismatch diagnostic instead of silently building a wrong-arity Applied.
#[test]
fn annotation_generic_enum_too_few_args_errors() {
    let source = format!("{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length> }}");
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an arity-mismatch error for Result<Length> (expects 2 args); got none"
    );
    let has_arity_msg = errors
        .iter()
        .any(|e| e.message.contains("expects 2 type arguments") || e.message.contains("found 1"));
    assert!(
        has_arity_msg,
        "error must describe the arity mismatch; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// A generic enum given too MANY type args (`Result<Length, String, Force>`) must
/// produce an arity-mismatch diagnostic.
#[test]
fn annotation_generic_enum_too_many_args_errors() {
    let source = format!(
        "{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length, String, Force> }}"
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an arity-mismatch error for Result<Length,String,Force> (expects 2 args); got none"
    );
    let has_arity_msg = errors
        .iter()
        .any(|e| e.message.contains("expects 2 type arguments") || e.message.contains("found 3"));
    assert!(
        has_arity_msg,
        "error must describe the arity mismatch; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Error-severity diagnostic messages from compiling `source` against the stdlib.
fn error_messages(source: &str) -> Vec<String> {
    compile_with_stdlib_helper(source)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

fn assert_no_errors(source: &str, context: &str) {
    let errors = error_messages(source);
    assert!(
        errors.is_empty(),
        "{context}: expected no errors; got: {errors:?}"
    );
}

/// `enum Pair` with a default that names an earlier type parameter: bare `Pair`
/// means `Pair<Length, Length>`.
const DEPENDENT_DEFAULTS_ENUM_SOURCE: &str = "enum Pair<A = Length, B = A> { P { a: A, b: B } }";

/// `ty` written in every annotation position that resolves a bare enum name,
/// as `(position, source)` pairs. `value` is an expression of type `ty`, used
/// where a position needs a value.
fn annotation_positions(ty: &str, value: &str) -> Vec<(&'static str, String)> {
    vec![
        ("trait member", format!("trait HasX {{ param x : {ty} }}")),
        (
            "structure param",
            format!("structure def D {{ param x : {ty} }}"),
        ),
        (
            "structure param with default",
            format!("structure def D {{ param x : {ty} = {value} }}"),
        ),
        ("fn param", format!("fn f(x: {ty}) -> Bool {{ true }}")),
        ("fn return", format!("fn g() -> {ty} {{ {value} }}")),
        (
            "constraint-def param",
            format!("constraint def K {{\n    param x : {ty}\n    true\n}}"),
        ),
        (
            "inner builtin arg",
            format!("structure def D {{ param o : Option<{ty}> }}"),
        ),
        (
            "non-parametric alias body",
            format!("type AX = {ty}\nstructure def D {{ param x : AX }}"),
        ),
    ]
}

/// R2(b)/R3: a bare generic enum that is not fully defaulted is exactly one
/// arity error in every annotation position, and through the local-enum shadow
/// arm. Exactly one, so a site where both the ambient bare-name step and the
/// `resolve_enum_type_with_args` fallback fire is caught.
/// (The structure-param row mirrors
/// tests/prd-gate/fixtures/getar_bare_result_annotation_match_panics.ri — pg-drift:allow — prose only, the source is inlined here.)
#[test]
fn bare_generic_enum_is_one_arity_error_in_every_annotation_position() {
    let enums = [
        (
            RESULT_ENUM_SOURCE,
            "Result",
            "Ok { value: 1mm }",
            "enum `Result` expects 2 type arguments, found 0",
        ),
        (
            "enum Duo<A, B = String> { Both { a: A, b: B } }",
            "Duo",
            "Both { a: 1mm, b: \"s\" }",
            "enum `Duo` expects 2 type arguments, found 0",
        ),
    ];
    let mut rows: Vec<(String, String, &str)> = enums
        .iter()
        .flat_map(|&(decl, ty, value, needle)| {
            annotation_positions(ty, value)
                .into_iter()
                .map(move |(position, body)| {
                    (
                        format!("`{ty}` as {position}"),
                        format!("{decl}\n{body}"),
                        needle,
                    )
                })
        })
        .collect();
    rows.push((
        "local generic enum shadowing prelude `structure def Fit`".to_string(),
        "enum Fit<T> { A { v: T } }\nstructure def D { param f : Fit }".to_string(),
        "enum `Fit` expects 1 type argument, found 0",
    ));
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|(label, source, needle)| {
            let errors = error_messages(source);
            let hits = errors.iter().filter(|m| m.contains(needle)).count();
            (hits != 1).then(|| {
                format!("{label}: expected exactly one error containing {needle:?}, found {hits}; got: {errors:?}")
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// R3: a bare enum whose every type parameter is defaulted, including a default
/// that names an earlier parameter, is accepted in every annotation position.
#[test]
fn bare_fully_defaulted_enum_is_accepted_in_every_annotation_position() {
    let enums = [
        (
            "enum Wrapper<T = Length> { Item { value: T }, Empty }",
            "Wrapper",
            "Item { value: 1mm }",
        ),
        (
            DEPENDENT_DEFAULTS_ENUM_SOURCE,
            "Pair",
            "P { a: 1mm, b: 2mm }",
        ),
    ];
    let failures: Vec<String> = enums
        .iter()
        .flat_map(|&(decl, ty, value)| {
            annotation_positions(ty, value)
                .into_iter()
                .filter_map(move |(position, body)| {
                    let errors = error_messages(&format!("{decl}\n{body}"));
                    (!errors.is_empty()).then(|| {
                        format!("`{ty}` as {position}: expected no errors; got: {errors:?}")
                    })
                })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// R3: a bare enum whose every type parameter is defaulted means the defaults,
/// with a default that names an earlier parameter taking that parameter's
/// filled argument.
#[test]
fn annotation_bare_enum_with_all_defaults_fills_from_defaults() {
    let cases = [
        (
            "enum Wrapper<T = Length> { Item { value: T }, Empty }",
            "Wrapper",
            "Wrapper<Length>",
            "Item { value: 1mm }",
            vec![Type::length()],
        ),
        (
            DEPENDENT_DEFAULTS_ENUM_SOURCE,
            "Pair",
            "Pair<Length, Length>",
            "P { a: 1mm, b: 2mm }",
            vec![Type::length(), Type::length()],
        ),
    ];
    for (decl, bare, explicit, value, args) in cases {
        let source = format!(
            "{decl}\nstructure def D {{\n    param a : {bare} = {value}\n    param b : {explicit}\n}}"
        );
        let module = compile_with_stdlib_helper(&source);
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| &d.message)
            .collect();
        assert!(
            errors.is_empty(),
            "`{bare}`: expected no errors; got: {errors:?}"
        );

        let template = module
            .templates
            .iter()
            .find(|t| t.name == "D")
            .expect("D template must exist");
        let cell_type = |member: &str| {
            template
                .value_cells
                .iter()
                .find(|vc| vc.id.member == member)
                .unwrap_or_else(|| panic!("D must have a value cell named '{member}'"))
                .cell_type
                .clone()
        };

        let expected = Type::Applied {
            name: bare.to_string(),
            args,
        };
        assert_eq!(
            cell_type("a"),
            expected,
            "bare `{bare}` must fill from its defaults"
        );
        assert_eq!(
            cell_type("a"),
            cell_type("b"),
            "bare `{bare}` must equal `{explicit}`"
        );
    }
}

/// R2(b) through a parametric alias: a body naming the bare generic enum is
/// resolved at each use of the alias, so the arity error is reported once, at
/// that use, whether or not the alias is `pub`.
#[test]
fn bare_generic_enum_in_parametric_alias_body_is_one_arity_error_at_the_use_site() {
    let use_site = "R<Length>";
    let failures: Vec<String> = ["pub type", "type"]
        .into_iter()
        .filter_map(|alias_kw| {
            let source = format!(
                "{RESULT_ENUM_SOURCE}\n{alias_kw} R<T> = Option<Result>\nstructure def D {{ param x : {use_site} }}"
            );
            let arity_errors: Vec<_> = compile_with_stdlib_helper(&source)
                .diagnostics
                .into_iter()
                .filter(|d| {
                    d.severity == Severity::Error
                        && d.message == "enum `Result` expects 2 type arguments, found 0"
                })
                .collect();
            let labelled: Vec<&str> = arity_errors
                .iter()
                .flat_map(|d| &d.labels)
                .map(|l| {
                    source
                        .get(l.span.start as usize..l.span.end as usize)
                        .unwrap_or("<outside the source>")
                })
                .collect();
            (arity_errors.len() != 1 || labelled != [use_site]).then(|| {
                format!(
                    "`{alias_kw} R<T> = Option<Result>`: expected one arity error labelled \
                     {use_site:?}; got {} labelled {labelled:?}",
                    arity_errors.len()
                )
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `pub` parametric-alias def-site guard probes the bare name `Result`; the
/// arity diagnostic for that bare probe must not leak into (or panic) the compile.
#[test]
fn pub_parametric_alias_naming_generic_enum_compiles_clean() {
    let source = format!("{RESULT_ENUM_SOURCE}\npub type Res<T> = Result<T, String>");
    assert_no_errors(&source, "`pub type Res<T> = Result<T, String>`");
}

// ═══════════════════════════════════════════════════════════════════════════════
// S3 — RED: binder substitution (S4 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (clean) `match r { Ok { value: v } => v + 1mm, Err { error: m } => 6mm }`
/// over `r : Result<Length, String>` → ZERO Error diagnostics after S4.
/// (mismatch) `... Ok { value: v } => v + 1N ...` → AT LEAST ONE Error (dimension mismatch).
///
/// RED after S2 / before S4: the binder `v` is typed `Type::TypeParam("T")` or
/// `Type::Error`, so `v + 1mm` may be suppressed (anti-cascade), causing the
/// mismatch assertion to fail.
#[test]
fn binder_substituted_clean_arm_no_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestClean {{
    param r : Result<Length, String>
    let bore = match r {{
        Ok {{ value: v }} => v + 1mm,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "clean arm (v + 1mm over Length binder) must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Dimension mismatch: `v + 1N` where `v : Length` → at least one error.
///
/// RED after S2 / before S4: unsubstituted binder type means `v + 1N` may not
/// produce the expected dimension error (anti-cascade suppresses).
#[test]
fn binder_substituted_mismatch_arm_has_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestMismatch {{
    param r : Result<Length, String>
    let bad = match r {{
        Ok {{ value: v }} => v + 1N,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "mismatch arm (v + 1N where v : Length) must produce at least one error; got none"
    );
}

/// INV-6: a NON-generic data-carrying enum `Boxed` with `param b : Boxed`
/// correctly types `v` as `Length` in `match b { Cell { value: v } => v + 1mm }`.
///
/// GREEN from the start (DCE ε already handles non-generic binders); regression pin.
#[test]
fn non_generic_enum_binder_clean() {
    let source = r#"
enum Boxed { Cell { value: Length } }
structure def UseBoxed {
    param b : Boxed
    let out = match b {
        Cell { value: v } => v + 1mm,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "non-generic Boxed binder (v + 1mm) must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// INV-6: `v + 1N` over a non-generic `Boxed { Cell { value: Length } }` binder
/// still produces a dimension mismatch error (DCE ε unchanged).
#[test]
fn non_generic_enum_binder_mismatch() {
    let source = r#"
enum Boxed { Cell { value: Length } }
structure def UseBoxedBad {
    param b : Boxed
    let bad = match b {
        Cell { value: v } => v + 1N,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "mismatch arm (v + 1N where v : Length from non-generic Boxed) must error; got none"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// S5 — RED: exhaustiveness over Applied discriminant (S6 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (non-exhaustive) `match r { Ok { value: v } => v + 1mm }` over
/// `r : Result<Length, String>` → assert a non-exhaustiveness Error diagnostic.
///
/// RED after S4 / before S6: the exhaustiveness check keys on `Type::Enum` only,
/// so a `Type::Applied` discriminant skips the check silently.
#[test]
fn exhaustiveness_generic_enum_non_exhaustive_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestNonExh {{
    param r : Result<Length, String>
    let x = match r {{
        Ok {{ value: v }} => v + 1mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-exhaustive match on generic Result<L,S> must produce an error; got none"
    );
    // Robust token check: the error message must mention exhaustiveness.
    // We rely solely on the "exhaustive" token rather than also checking the
    // raw variant name ("Err"), which would accidentally match the word "Error".
    let has_exhaustive_msg = errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("exhaustive"));
    assert!(
        has_exhaustive_msg,
        "error message must mention 'exhaustive' (e.g. 'non-exhaustive match'); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// (exhaustive) `match r {{ Ok {{ value: v }} => v + 1mm, Err {{ error: m }} => 6mm }}`
/// → NO non-exhaustiveness error (only the match binder errors that may come from S3).
///
/// We can't easily separate "non-exhaustive" from other errors before S4, so this
/// assertion is that the number of errors does NOT include a missing-`Err` message.
/// After S6 this should produce zero errors total.
#[test]
fn exhaustiveness_generic_enum_exhaustive_no_missing_variant_error() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestExh {{
    param r : Result<Length, String>
    let x = match r {{
        Ok {{ value: v }} => v + 1mm,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    // There must be no "non-exhaustive" or "missing variant" errors.
    let exhaustive_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            d.message.to_lowercase().contains("exhaustive")
                || d.message.to_lowercase().contains("missing variant")
        })
        .collect();
    assert!(
        exhaustive_errors.is_empty(),
        "exhaustive match on generic Result<L,S> must produce no non-exhaustiveness error; got: {:?}",
        exhaustive_errors
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );
}

/// INV-6: a non-generic `enum Dir { In, Out }` with only `In =>` arm still emits
/// the non-exhaustive error (DCE ε/D4 unchanged for non-generic enums).
#[test]
fn exhaustiveness_non_generic_enum_still_checked() {
    let source = r#"
enum Dir { In, Out }
structure def UseDir {
    param d : Dir
    let x = match d {
        In => 1mm,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-exhaustive match on non-generic Dir must error; got none"
    );
    let has_exhaustive_msg = errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("exhaustive"));
    assert!(
        has_exhaustive_msg,
        "error must mention 'exhaustive' (e.g. 'non-exhaustive match'); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
