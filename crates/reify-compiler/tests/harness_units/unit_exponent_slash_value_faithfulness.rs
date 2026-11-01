//! `^`/`/` adjacency inside a unit expression, VALUE layer: a contiguous
//! `^…/…` spelling is either the rational exponent it reads as, or a LOCATED
//! error. It is never a silent value division.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md),
//! task #9236. Before #9236 the unit exponent was integral, so `50MPa*m^1/2`
//! ended its unit expression at `^1` and the `/2` became a value-level
//! division: 25 MPa·m, with no diagnostic, where 50 MPa·√m was written.
//!
//! The syntax half (tree shape, and each error's span) lives in
//! `reify-syntax`'s `harness_syntax_lowering/unit_expr_lowering_tests.rs`. This
//! module pins what a user sees: the VALUE. Each variant declares its expected
//! class, and a pure [`classify`] sorts the observed outcome into one of
//! [`Outcome`]'s arms; every variant must be [`Outcome::Correct`].
//!
//! The twins use the stdlib-sanctioned `sqrt()` spelling (`50.0 * 1MPa *
//! sqrt(1m)`), which parses without the rational-exponent grammar, so the
//! oracle does not depend on the change it judges. Template:
//! `reify-eval`'s `harness_engine/fn_body_separator_value_faithfulness.rs`.

use std::ops::Range;

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{compile_source_with_stdlib_allow_parse_errors, error_diags, make_engine};

/// What a variant must produce.
#[derive(Clone, Copy)]
enum Expect {
    /// A clean compile whose value agrees with this twin expression.
    Twin(&'static str),
    /// An error diagnostic starting inside the variant's contiguous run.
    LocatedError,
}

struct Variant {
    expr: &'static str,
    expect: Expect,
}

const fn twin(expr: &'static str, twin: &'static str) -> Variant {
    Variant {
        expr,
        expect: Expect::Twin(twin),
    }
}

const fn refused(expr: &'static str) -> Variant {
    Variant {
        expr,
        expect: Expect::LocatedError,
    }
}

const VARIANTS: &[Variant] = &[
    // Rational exponents, each against its `sqrt` twin.
    twin("50MPa*m^1/2", "50.0 * 1MPa * sqrt(1m)"),
    twin("1m^1/2", "sqrt(1m)"),
    twin("1m^-1/2", "1.0 / sqrt(1m)"),
    twin("1m^3/2", "1m * sqrt(1m)"),
    // FractureToughness: `Display`'s spelling and the `.ri` emitter's.
    twin("1kg·m^-1/2·s^-2", "1kg/s^2 / sqrt(1m)"),
    twin("1kg/m^1/2/s^2", "1kg/s^2 / sqrt(1m)"),
    // Division controls: their meaning is unchanged.
    twin("1m^2 / 2", "(1m^2) / 2"),
    twin("50MPa/2", "(50MPa) / 2"),
    twin("25USD/1kg", "(25USD) / (1kg)"),
    twin("(1m^1)/2", "(1m) / 2"),
    twin("1m^2/s", "1m^2 / 1s"),
    // Non-canonical exponents: `1m^2/2` was 0.5 m², so rereading it as `m^1`
    // would be a silent wrong value.
    refused("1m^2/4"),
    refused("1m^2/2"),
    refused("1m^2/1"),
    refused("1m^0/2"),
    refused("1m^1/0"),
    // A `/` touching the exponent that the token cannot absorb.
    refused("1m^1/-2"),
    refused("1m^1/ 2"),
    refused("1m^1/2/3"),
    refused("1m^1/2.5"),
    refused("1m^(1/2)"),
];

const PREFIX: &str = "structure def T {\n  let v = ";

fn source_of(expr: &str) -> String {
    format!("{PREFIX}{expr}\n}}\n")
}

/// The byte range, in [`source_of`]'s output, of `expr`'s contiguous run: from
/// its first byte to the first whitespace after the `^`, or to its end.
fn region_of(expr: &str) -> Range<u32> {
    let end = expr
        .find('^')
        .and_then(|caret| expr[caret..].find(char::is_whitespace).map(|ws| caret + ws))
        .unwrap_or(expr.len());
    (PREFIX.len() as u32)..((PREFIX.len() + end) as u32)
}

/// What compiling and, when clean, evaluating one program produced.
#[derive(Debug, Clone)]
enum Observed {
    /// At least one error diagnostic: the start byte of every label on every
    /// error.
    Refused { error_starts: Vec<u32> },
    /// A clean compile: the `T.v` cell's value, `None` when the cell is absent.
    Accepted { value: Option<Value> },
}

/// What a variant must agree with: a twin's value, or a refusal.
#[derive(Debug, Clone, Copy)]
enum Expected<'a> {
    Value(&'a Value),
    LocatedError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The twin's value, or an error located inside the region.
    Correct,
    /// A clean compile with a different value, or one where a refusal was due.
    Wrong,
    /// A clean compile whose cell is absent or `Undef`.
    Undef,
    /// Errors exist, but none starts inside the region.
    MislocatedError,
    /// An error where the twin's value was due: a valid program was rejected.
    UnexpectedRefusal,
}

/// Sort one observation into an [`Outcome`]. Pure, so its every arm is
/// exercised on synthetic inputs by [`classify_reaches_every_outcome`].
fn classify(expected: Expected<'_>, region: &Range<u32>, observed: &Observed) -> Outcome {
    match observed {
        Observed::Refused { error_starts } => match expected {
            Expected::Value(_) => Outcome::UnexpectedRefusal,
            Expected::LocatedError if error_starts.iter().any(|s| region.contains(s)) => {
                Outcome::Correct
            }
            Expected::LocatedError => Outcome::MislocatedError,
        },
        Observed::Accepted { value: None }
        | Observed::Accepted {
            value: Some(Value::Undef),
        } => Outcome::Undef,
        Observed::Accepted { value: Some(got) } => match expected {
            Expected::Value(want) if values_agree(got, want) => Outcome::Correct,
            Expected::Value(_) | Expected::LocatedError => Outcome::Wrong,
        },
    }
}

/// Discriminant + dimension equality, with a 1e-12 relative epsilon on the
/// magnitude: pure defence, since the twins fold to the same IEEE values.
fn values_agree(a: &Value, b: &Value) -> bool {
    fn close(x: f64, y: f64) -> bool {
        x == y || (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0)
    }
    match (a, b) {
        (Value::Real(x), Value::Real(y)) => close(*x, *y),
        (
            Value::Scalar {
                si_value: x,
                dimension: dx,
            },
            Value::Scalar {
                si_value: y,
                dimension: dy,
            },
        ) => dx == dy && close(*x, *y),
        (Value::Int(x), Value::Int(y)) => x == y,
        _ => false,
    }
}

/// Compile `expr` as `T.v` with the stdlib, and evaluate it when clean.
fn observe(expr: &str) -> Observed {
    let compiled = compile_source_with_stdlib_allow_parse_errors(&source_of(expr));
    let errors = error_diags(&compiled.diagnostics);
    if !errors.is_empty() {
        return Observed::Refused {
            error_starts: errors
                .iter()
                .flat_map(|d| d.labels.iter().map(|l| l.span.start))
                .collect(),
        };
    }
    let result = make_engine().eval(&compiled);
    Observed::Accepted {
        value: result.values.get(&ValueCellId::new("T", "v")).cloned(),
    }
}

/// A twin's value. Panics unless it compiles clean and is defined:
/// [`twins_compile_cleanly_and_are_defined`] states that as its own gate.
fn twin_value(twin: &str) -> Value {
    match observe(twin) {
        Observed::Accepted { value: Some(value) } if !matches!(value, Value::Undef) => value,
        other => panic!("twin `{twin}` must compile cleanly to a defined value; got {other:?}"),
    }
}

#[test]
fn twins_compile_cleanly_and_are_defined() {
    for v in VARIANTS {
        if let Expect::Twin(twin) = v.expect {
            let _ = twin_value(twin);
        }
    }
}

/// INV-SF-7 for the unit-exponent `/`: every variant is Correct.
#[test]
fn unit_exponent_slash_variants_are_value_faithful() {
    let mut by_value = 0usize;
    let mut by_error = 0usize;
    let mut failures = Vec::new();
    for v in VARIANTS {
        let region = region_of(v.expr);
        let observed = observe(v.expr);
        let twin = match v.expect {
            Expect::Twin(twin) => Some((twin, twin_value(twin))),
            Expect::LocatedError => None,
        };
        let expected = match &twin {
            Some((_, value)) => Expected::Value(value),
            None => Expected::LocatedError,
        };
        match classify(expected, &region, &observed) {
            Outcome::Correct if twin.is_some() => by_value += 1,
            Outcome::Correct => by_error += 1,
            outcome => failures.push(format!(
                "`{}`: {outcome:?}; observed {observed:?}; expected {}; region {region:?}",
                v.expr,
                match &twin {
                    Some((src, value)) => format!("`{src}` = {value:?}"),
                    None => "a located error".to_string(),
                },
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "INV-SF-7: a unit-exponent `/` changed a value or refused wrongly:\n{}",
        failures.join("\n")
    );
    assert!(
        by_value > 0 && by_error > 0,
        "non-vacuity: both arms must run (value arm {by_value}, error arm {by_error})"
    );
}

/// Each non-passing arm of [`classify`] is reachable, so the gate above cannot
/// pass vacuously.
#[test]
fn classify_reaches_every_outcome() {
    let region = 10..15;
    let metre = Value::Scalar {
        si_value: 1.0,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let half_metre = Value::Scalar {
        si_value: 0.5,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let accepted = |value: Option<Value>| Observed::Accepted { value };
    let refused = |starts: &[u32]| Observed::Refused {
        error_starts: starts.to_vec(),
    };
    let cases = [
        (
            Expected::Value(&metre),
            accepted(Some(metre.clone())),
            Outcome::Correct,
        ),
        (Expected::LocatedError, refused(&[3, 12]), Outcome::Correct),
        (
            Expected::Value(&metre),
            accepted(Some(half_metre.clone())),
            Outcome::Wrong,
        ),
        (
            Expected::LocatedError,
            accepted(Some(half_metre)),
            Outcome::Wrong,
        ),
        (Expected::Value(&metre), accepted(None), Outcome::Undef),
        (
            Expected::Value(&metre),
            accepted(Some(Value::Undef)),
            Outcome::Undef,
        ),
        (
            Expected::LocatedError,
            refused(&[3, 15]),
            Outcome::MislocatedError,
        ),
        (
            Expected::LocatedError,
            refused(&[]),
            Outcome::MislocatedError,
        ),
        (
            Expected::Value(&metre),
            refused(&[12]),
            Outcome::UnexpectedRefusal,
        ),
    ];
    for (expected, observed, want) in cases {
        assert_eq!(
            classify(expected, &region, &observed),
            want,
            "classify({expected:?}, {region:?}, {observed:?})"
        );
    }
}

#[test]
fn region_spans_the_contiguous_run() {
    let start = PREFIX.len() as u32;
    assert_eq!(region_of("1m^1/ 2"), start..start + 5);
    assert_eq!(region_of("1m^1/2/3"), start..start + 8);
    assert_eq!(region_of("25USD/1kg"), start..start + 9);
}
