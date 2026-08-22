//! `parse(value_to_ri_literal(v)) ≡ v` — the round-trip property for the
//! `.ri` source-literal serializer (task #5095, ai-native-editing β).
//!
//! This is β's own INTERNAL check, not the leaf signal: γ is what proves an
//! end-to-end edit works. What it pins here is the one claim the serializer
//! makes on its own — that anything it agrees to write comes back **bit for
//! bit**, with the same dimension and the same `Value` variant.
//!
//! # Why this test lives in reify-compiler
//!
//! It cannot live next to the code it tests. `crates/reify-ir/tests/
//! dag_invariant.rs`'s `reify_ir_depends_only_on_reify_core_and_reify_ast`
//! text-scans reify-ir's whole `Cargo.toml` — dev-dependencies included — and
//! rejects any `reify-*` line naming anything but `reify-core`/`reify-ast`.
//! So reify-ir's own tests can never reach a parser. reify-compiler's test
//! tier already dev-deps `reify-ir`, `reify-eval` and
//! `reify-test-support { features = ["eval-helpers"] }`.
//!
//! # Why it evaluates rather than inspecting the compiled expression
//!
//! A negative literal (`-80mm`) is a `unary_expression` wrapping the quantity,
//! so its compiled shape depends on constant folding. Going all the way to a
//! `Value` via `eval_source` makes the assertion independent of that.
//!
//! Plain `eval_source` deliberately does NOT seed the stdlib unit registry.
//! That is not an oversight — it is the guarantee under test. Every symbol the
//! serializer emits is a bare built-in resolved by the compiler's
//! unconditional `unit_to_scalar` fallback, so a rewritten literal must parse
//! in a module that imports nothing.

use reify_core::DimensionVector;
use reify_ir::Value;
use reify_ir::ri_literal::{value_to_ri_literal, value_to_ri_literal_with_unit};
use reify_test_support::{cell_value, eval_source};

// ─── harness ─────────────────────────────────────────────────────────────────

/// One round-trip case: a `Value`, the unit hint to serialize it with, and
/// (optionally) the exact literal it must produce.
struct Case {
    value: Value,
    hint: Option<&'static str>,
    expect_literal: Option<&'static str>,
    label: String,
}

impl Case {
    fn new(label: impl Into<String>, value: Value) -> Self {
        Case {
            value,
            hint: None,
            expect_literal: None,
            label: label.into(),
        }
    }

    fn with_hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint);
        self
    }

    fn emitting(mut self, literal: &'static str) -> Self {
        self.expect_literal = Some(literal);
        self
    }
}

/// The `.ri` type name to declare a param of, for a value this test generates.
///
/// Every name here is in `NAMED_DIMENSIONS` (or is a primitive), so it
/// resolves with no stdlib import.
fn ri_type_name(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Real(_) => "Real",
        Value::String(_) => "String",
        Value::Scalar { dimension, .. } => match *dimension {
            DimensionVector::LENGTH => "Length",
            DimensionVector::ANGLE => "Angle",
            DimensionVector::MASS => "Mass",
            DimensionVector::TIME => "Time",
            DimensionVector::TEMPERATURE => "Temperature",
            // A dimensionless Scalar is emitted as a bare real and comes back
            // as a `Value::Real`, so `Real` is the type its param must declare.
            // That variant change is the serializer's one documented
            // exception, pinned by
            // `a_dimensionless_scalar_downgrades_to_real_with_identical_bits`
            // — it cannot ride `assert_round_trips`, whose `assert_identical`
            // asserts discriminant equality.
            DimensionVector::DIMENSIONLESS => "Real",
            other => panic!("no .ri type name wired for dimension {other}"),
        },
        other => panic!("no .ri type name wired for {other:?}"),
    }
}

/// Serialize every case, splice them into ONE `structure def`, evaluate it
/// ONCE, and assert every value came back identical.
///
/// Batching matters: a per-case compile would make a 260-case sweep slow
/// enough that nobody runs it.
fn assert_round_trips(cases: &[Case]) {
    let mut source = String::from("structure def S {\n");
    let mut literals: Vec<String> = Vec::with_capacity(cases.len());

    for (i, case) in cases.iter().enumerate() {
        let literal = value_to_ri_literal_with_unit(&case.value, case.hint).unwrap_or_else(|e| {
            panic!(
                "case {i} [{}]: serializer refused {:?}: {e}",
                case.label, case.value
            )
        });
        if let Some(expected) = case.expect_literal {
            assert_eq!(
                literal, expected,
                "case {i} [{}]: wrong literal for {:?}",
                case.label, case.value
            );
        }
        source.push_str(&format!(
            "  param x{i} : {} = {literal}\n",
            ri_type_name(&case.value)
        ));
        literals.push(literal);
    }
    source.push_str("}\n");

    let result = eval_source(&source);

    for (i, case) in cases.iter().enumerate() {
        let got = cell_value(&result, "S", &format!("x{i}"));
        assert_identical(&case.value, &got, &literals[i], i, &case.label);
    }
}

/// `original` and `got` must be the same variant, the same dimension, and —
/// for the float-bearing variants — the same raw bits.
fn assert_identical(original: &Value, got: &Value, literal: &str, i: usize, label: &str) {
    assert_eq!(
        std::mem::discriminant(original),
        std::mem::discriminant(got),
        "case {i} [{label}]: literal {literal:?} re-parsed as a DIFFERENT Value \
         variant: wrote {original:?}, read back {got:?}"
    );
    assert_eq!(
        got.dimension(),
        original.dimension(),
        "case {i} [{label}]: literal {literal:?} changed dimension: \
         wrote {}, read back {}",
        original.dimension(),
        got.dimension()
    );

    match original {
        Value::Real(_) | Value::Scalar { .. } => {
            let a = original.as_f64().expect("float-bearing variant");
            let b = got.as_f64().expect("float-bearing variant");
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "case {i} [{label}]: literal {literal:?} did not round-trip exactly.\n  \
                 wrote   si = {a:?}  bits = {:#018x}\n  \
                 read    si = {b:?}  bits = {:#018x}",
                a.to_bits(),
                b.to_bits()
            );
        }
        _ => assert_eq!(
            original, got,
            "case {i} [{label}]: literal {literal:?} did not round-trip"
        ),
    }
}

/// The trailing unit symbol of an emitted scalar literal, e.g. `"mm"` from
/// `"1e-6mm"`. Empty when the literal carries no unit.
fn trailing_unit(literal: &str) -> &str {
    let split = literal
        .rfind(|c: char| !c.is_ascii_alphabetic())
        .map(|i| i + 1)
        .unwrap_or(0);
    &literal[split..]
}

// ─── (a) fixed regressions ───────────────────────────────────────────────────

#[test]
fn fixed_regression_values_round_trip_exactly() {
    let witness = -0.5566166674539299_f64;
    let cases = vec![
        // The PRD's headline G3 case.
        Case::new("headline 0.08 m", Value::length(0.08)).emitting("80mm"),
        Case::new("π/2 rad", Value::angle(std::f64::consts::FRAC_PI_2)).emitting("90deg"),
        // Canonical vs hinted for the same value — both must be exact.
        Case::new("0.05 m canonical", Value::length(0.05)).emitting("50mm"),
        Case::new("0.05 m hinted cm", Value::length(0.05))
            .with_hint("cm")
            .emitting("5cm"),
        Case::new("0.0254 m canonical", Value::length(0.0254)).emitting("25.4mm"),
        Case::new("0.0254 m hinted in", Value::length(0.0254))
            .with_hint("in")
            .emitting("1in"),
        Case::new("π/2 hinted rad", Value::angle(std::f64::consts::FRAC_PI_2))
            .with_hint("rad")
            .emitting("1.5707963267948966rad"),
        // The mm-inexact witness must come back exact, via the cm rung.
        Case::new("mm-inexact witness", Value::length(witness)).emitting("-55.66166674539299cm"),
        // A hint that would be lossy is refused, and the exact answer stands.
        Case::new("witness with a lossy mm hint", Value::length(witness))
            .with_hint("mm")
            .emitting("-55.66166674539299cm"),
        Case::new("zero length", Value::length(0.0)).emitting("0mm"),
        Case::new("negative length", Value::length(-0.08)).emitting("-80mm"),
        // Exponent-form magnitudes adjacent to a unit — legal per the corpus.
        Case::new("tiny length", Value::length(1e-9)),
        Case::new("huge length", Value::length(1e12)),
        // Other dimensions, all factor-1.0 ladders.
        Case::new(
            "mass",
            Value::Scalar {
                si_value: 2.75,
                dimension: DimensionVector::MASS,
            },
        )
        .emitting("2.75kg"),
        Case::new(
            "time",
            Value::Scalar {
                si_value: -0.125,
                dimension: DimensionVector::TIME,
            },
        )
        .emitting("-0.125s"),
        Case::new(
            "temperature",
            Value::Scalar {
                si_value: 293.15,
                dimension: DimensionVector::TEMPERATURE,
            },
        )
        .emitting("293.15K"),
        // The `is_real` seam: a whole Real must NOT come back as an Int.
        Case::new("whole real stays real", Value::Real(80.0)).emitting("80.0"),
        Case::new("fractional real", Value::Real(-0.25)).emitting("-0.25"),
        Case::new("int", Value::Int(-42)).emitting("-42"),
        Case::new(
            "int at the exact-f64 boundary",
            Value::Int(9007199254740991),
        )
        .emitting("9007199254740991"),
        Case::new("bool true", Value::Bool(true)).emitting("true"),
        Case::new("bool false", Value::Bool(false)).emitting("false"),
        Case::new("string", Value::String("PLA".into())).emitting("\"PLA\""),
        Case::new("string with a dot", Value::String("M3x0.5".into())).emitting("\"M3x0.5\""),
    ];
    assert_round_trips(&cases);
}

// ─── (b) deterministic sweep ─────────────────────────────────────────────────

/// xorshift64* — a fixed-seed generator written inline.
///
/// The workspace has no proptest/quickcheck/approx (zero hits across every
/// Cargo.toml), so a hand-rolled deterministic generator is the
/// house-compatible form. Deterministic also means a failure is exactly
/// reproducible from the seed, with no shrinking machinery to interpret.
struct XorShift64Star(u64);

impl XorShift64Star {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A "human" magnitude: at most 3 decimal places, |v| < 1000, non-zero.
    /// Scaled by a rung factor these produce SI values that a real `.ri` file
    /// would contain, and that the *first* ladder rung usually accepts.
    fn human_magnitude(&mut self) -> f64 {
        let n = (self.next_u64() % 1_999_999) as i64 - 999_999;
        let n = if n == 0 { 1 } else { n };
        n as f64 / 1000.0
    }

    /// An arbitrary finite normal `f64` with |v| in roughly [1e-9, 2e9),
    /// assembled straight from random bits. These are almost never exactly
    /// recoverable at a scaled rung, so they drive the ladder down to its
    /// factor-1.0 terminator — the fallback path.
    fn arbitrary_f64(&mut self) -> f64 {
        let bits = self.next_u64();
        let sign = bits & (1u64 << 63);
        let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
        let exponent = ((bits >> 52) & 0x7FF) % 61 + (1023 - 30);
        f64::from_bits(sign | (exponent << 52) | mantissa)
    }
}

#[test]
fn deterministic_sweep_round_trips_exactly() {
    // (dimension, rung factors to scale "human" magnitudes by, sample count)
    let plan: [(DimensionVector, &[f64], usize); 5] = [
        (DimensionVector::LENGTH, &[0.001, 0.01, 1.0, 0.0254], 120),
        (
            DimensionVector::ANGLE,
            &[std::f64::consts::PI / 180.0, 1.0],
            80,
        ),
        (DimensionVector::MASS, &[1.0], 20),
        (DimensionVector::TIME, &[1.0], 20),
        (DimensionVector::TEMPERATURE, &[1.0], 20),
    ];

    let mut rng = XorShift64Star(0x5095_A1B2_C3D4_E5F6);
    let mut cases: Vec<Case> = Vec::new();

    for (dimension, factors, count) in plan {
        for i in 0..count {
            // Alternate the two generators so both the accept path (a human
            // magnitude landing on an early rung) and the fallback path (an
            // arbitrary f64 forced down to the factor-1.0 terminator) get
            // real coverage.
            let si_value = if i % 2 == 0 {
                let factor = factors[(rng.next_u64() as usize) % factors.len()];
                rng.human_magnitude() * factor
            } else {
                rng.arbitrary_f64()
            };
            assert!(
                si_value.is_finite(),
                "generator produced a non-finite value"
            );
            cases.push(Case::new(
                format!("sweep {dimension} #{i}"),
                Value::Scalar {
                    si_value,
                    dimension,
                },
            ));
        }
    }
    assert_eq!(cases.len(), 260);

    // Coverage guard: the sweep is only meaningful if it actually exercised
    // BOTH the first rung and a later one. Deterministic seed ⇒ these counts
    // are fixed, so a regression that collapses the ladder shows up here as a
    // coverage failure even before an exactness failure.
    let mut first_rung = 0usize;
    let mut later_rung = 0usize;
    for case in &cases {
        let literal = value_to_ri_literal(&case.value).expect("sweep value is serializable");
        let unit = trailing_unit(&literal);
        let ladder = reify_core::ri_emittable_units(&case.value.dimension());
        if unit == ladder[0] {
            first_rung += 1;
        } else {
            later_rung += 1;
        }
    }
    println!(
        "sweep ladder coverage: {first_rung} first-rung, {later_rung} later-rung \
         out of {} cases",
        cases.len()
    );
    assert!(
        first_rung > 0 && later_rung > 0,
        "sweep did not exercise both ladder paths: {first_rung} first-rung, \
         {later_rung} later-rung out of {}",
        cases.len()
    );

    assert_round_trips(&cases);
}

// ─── (c) the one documented variant change ───────────────────────────────────

/// A dimensionless `Scalar` round-trips as a `Real` — same dimension, same
/// bits, DIFFERENT variant.
///
/// This is the serializer's sole documented exception to the "same `Value`
/// variant" clause (`ri_literal`'s module doc, qualification 1), and it is the
/// one case the shared harness structurally cannot carry: `assert_identical`
/// asserts `mem::discriminant` equality, so routing this through
/// `assert_round_trips` would fail by construction. Pinning it here means the
/// exception is *asserted*, not silently excluded from the very test that is
/// supposed to police the contract.
///
/// The change is not a defect to fix: an un-suffixed `.ri` number literal has
/// no reading other than `Real`, so the alternative would be refusing to write
/// a dimensionless scalar at all.
#[test]
fn a_dimensionless_scalar_downgrades_to_real_with_identical_bits() {
    // The mm-inexact witness, reused so the magnitude is a genuinely awkward
    // f64 rather than something a shortest-form printer rounds off easily.
    let si = -0.5566166674539299_f64;
    let original = Value::Scalar {
        si_value: si,
        dimension: DimensionVector::DIMENSIONLESS,
    };

    let literal = value_to_ri_literal(&original).expect("a dimensionless scalar is emittable");
    assert_eq!(
        literal, "-0.5566166674539299",
        "a dimensionless scalar must be emitted as a bare real, with no unit"
    );

    let source = format!(
        "structure def S {{\n  param x : {} = {literal}\n}}\n",
        ri_type_name(&original)
    );
    let got = cell_value(&eval_source(&source), "S", "x");

    // The documented downgrade: variant moves Scalar → Real...
    assert!(
        matches!(got, Value::Real(_)),
        "literal {literal:?} must re-parse as Value::Real, got {got:?}"
    );
    assert_ne!(
        std::mem::discriminant(&original),
        std::mem::discriminant(&got),
        "this test exists because the variant DOES change; if it no longer \
         does, the module doc's qualification 1 is stale and should be dropped"
    );
    // ...while dimension and raw bits are preserved exactly.
    assert_eq!(
        got.dimension(),
        original.dimension(),
        "the downgrade must not change the dimension"
    );
    assert_eq!(
        got.as_f64().expect("Real is float-bearing").to_bits(),
        si.to_bits(),
        "the downgrade must be bit-exact"
    );
}

// ─── (d) rejections are never spliced ────────────────────────────────────────

/// Rejected values are deliberately NOT parsed — the point is that nothing
/// unparseable can ever reach a splice in the first place.
#[test]
fn unrepresentable_values_are_refused_rather_than_written() {
    let pressure = Value::Scalar {
        si_value: 101_325.0,
        dimension: DimensionVector::PRESSURE,
    };
    assert!(
        value_to_ri_literal(&pressure).is_err(),
        "a Pressure has no bare built-in symbol and must be refused"
    );

    let braced = Value::String("hole {x}".into());
    assert!(
        value_to_ri_literal(&braced).is_err(),
        "a brace-bearing string must be refused — it would divert the token \
         to interpolated_string, which treats {{expr}} as a hole"
    );
}
