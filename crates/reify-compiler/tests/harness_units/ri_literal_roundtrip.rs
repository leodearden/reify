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
//! serializer emits is a built-in, bare (`80mm`) or as a compound factor
//! (`7850kg/m^3`), and the compiler falls back to the built-in table for both
//! (for a compound atom via `lookup_unit_atom`, task #5503), so a rewritten
//! literal must parse in a module that imports nothing. Every case is spliced
//! as a structure param, which meets a compound literal's
//! position precondition (`reify_ir::ri_literal` qualification 3).
//!
//! That is the *unshadowed* case, and it is the weaker half of what a real
//! caller needs. γ's edit path splices into user modules, which usually DO
//! `import std.units`, and the compiler consults the per-module `UnitRegistry`
//! BEFORE the built-in table. Section (d) below closes that half, at two
//! altitudes:
//!
//!   - `emitted_literals_round_trip_under_the_stdlib_unit_registry` runs the
//!     same batched round-trip through `compile_with_stdlib`, so an emitted
//!     literal is actually resolved registry-first. This is the direct
//!     property.
//!   - `stdlib_unit_declarations_agree_bit_for_bit_with_the_builtin_table`
//!     compares the two tables symbol by symbol. This is the canary: it
//!     localises a drift to a named symbol and factor, which a round-trip
//!     failure would only hint at.
//!
//! Neither subsumes the other — a table comparison cannot see the `import`,
//! the seeding order or the literal lowering; a round-trip cannot say which
//! table entry moved.

use reify_compiler::{CompiledModule, UnitEntry, UnitRegistry, seed_prelude_units, stdlib_loader};
use reify_core::DimensionVector;
use reify_ir::Value;
use reify_ir::ri_literal::{RiLiteralError, value_to_ri_literal, value_to_ri_literal_with_unit};
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
/// A dimensioned scalar declares its dimension's `NAMED_DIMENSIONS` name
/// (`canonical_name`), which resolves with no stdlib import.
fn ri_type_name(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Real(_) => "Real",
        Value::String(_) => "String",
        // A dimensionless Scalar is emitted as a bare real and comes back as a
        // `Value::Real`, so `Real` is the type its param must declare. That
        // variant change is the serializer's one documented exception, pinned
        // by `a_dimensionless_scalar_downgrades_to_real_with_identical_bits` —
        // it cannot ride `assert_round_trips`, whose `assert_identical` asserts
        // discriminant equality.
        Value::Scalar { dimension, .. } if dimension.is_dimensionless() => "Real",
        Value::Scalar { dimension, .. } => dimension
            .canonical_name()
            .unwrap_or_else(|| panic!("dimension {dimension} has no NAMED_DIMENSIONS name")),
        Value::Direction(_) => "Direction",
        other => panic!("no .ri type name wired for {other:?}"),
    }
}

/// Which unit-resolution regime the spliced source is compiled under.
///
/// The compiler resolves a bare unit as `scope.lookup_unit_in_registry(..)
/// .or_else(|| unit_to_scalar(..))`, so these are the two sides of that
/// `or_else` — and a round-trip proof under one is not a proof under the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prelude {
    /// No stdlib. Nothing seeds the `UnitRegistry`, so every emitted symbol
    /// resolves through the compiler's unconditional `unit_to_scalar`
    /// fallback — the table the serializer's exactness proof is actually
    /// against. The UNSHADOWED case.
    None,
    /// Full stdlib prelude, plus an explicit `import std.units`. Every stdlib
    /// `pub unit` is seeded into the `UnitRegistry`, which is consulted FIRST,
    /// so the factor a literal is multiplied by comes from `units.ri` /
    /// `si_units.rs` rather than from `unit_symbol_to_si`. The SHADOWED case —
    /// and the one γ's edit path actually splices into.
    Stdlib,
}

/// Serialize every case, splice them into ONE `structure def`, evaluate it
/// ONCE, and assert every value came back identical.
///
/// Batching matters: a per-case compile would make a 260-case sweep slow
/// enough that nobody runs it.
fn assert_round_trips(cases: &[Case]) {
    assert_round_trips_under(cases, Prelude::None);
}

/// [`assert_round_trips`], under an explicit unit-resolution regime.
fn assert_round_trips_under(cases: &[Case], prelude: Prelude) {
    let mut source = String::new();
    if prelude == Prelude::Stdlib {
        source.push_str("import std.units\n");
    }
    source.push_str("structure def S {\n");
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

    let result = match prelude {
        Prelude::None => eval_source(&source),
        Prelude::Stdlib => eval_source_with_stdlib(&source),
    };

    for (i, case) in cases.iter().enumerate() {
        let got = cell_value(&result, "S", &format!("x{i}"));
        assert_identical(&case.value, &got, &literals[i], i, &case.label);
    }
}

/// `eval_source`, but compiled through `compile_with_stdlib` so the stdlib
/// prelude's `pub unit` declarations are seeded into the `UnitRegistry`.
///
/// Inlined here rather than added to `reify-test-support` as an
/// `eval_source_with_stdlib` sibling of the existing `check_source_with_stdlib`:
/// `crates/reify-test-support/src/helpers.rs` is outside task #5095's locked
/// scope. Everything it uses is already public API, so the only cost of
/// keeping it local is this comment.
fn eval_source_with_stdlib(source: &str) -> reify_eval::EvalResult {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);
    let mut engine = reify_test_support::make_engine();
    let result = engine.eval(&compiled);
    let errors: Vec<&reify_core::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "eval-phase errors: {errors:?}");
    result
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
        // Stricter than the derived numeric `==`: every component's raw bits.
        Value::Direction(a) => {
            let Value::Direction(b) = got else {
                unreachable!("discriminants compared equal above")
            };
            assert_eq!(
                a.components().map(f64::to_bits),
                b.components().map(f64::to_bits),
                "case {i} [{label}]: literal {literal:?} did not round-trip exactly: \
                 wrote {:?}, read back {:?}",
                a.components(),
                b.components()
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
        // NEGATIVE ZERO. The contract is stated in terms of raw f64 BITS, and
        // `-0.0` and `0.0` are the one pair that compares equal while differing
        // in bits — so `assert_identical`'s `to_bits()` comparison is the only
        // assertion in this file that can tell them apart. Both forms go
        // through a `unary_expression` wrapping a zero literal, so this also
        // pins that the negation survives constant folding.
        Case::new("negative-zero length", Value::length(-0.0)).emitting("-0mm"),
        Case::new("negative-zero real", Value::Real(-0.0)).emitting("-0.0"),
        // The `g` rung is reachable only through a caller hint — it is
        // deliberately absent from the canonical Mass ladder (`kg` is the SI
        // base and the exact terminator). It is named in section (d)'s
        // cross-guard symbol list, so it must also be round-tripped somewhere.
        Case::new(
            "mass hinted g",
            Value::Scalar {
                si_value: 0.25,
                dimension: DimensionVector::MASS,
            },
        )
        .with_hint("g")
        .emitting("250g"),
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
        // The two string shapes where "lower_string_literal performs NO escape
        // decoding, so these are verbatim-safe" is LEAST self-evident. That
        // premise is about the LEXER, and a unit test in `ri_literal.rs` can
        // only check what the serializer emits — only a parse-back reaches the
        // lexer, so the claim is unpinned unless it is made here.
        Case::new("empty string", Value::String(String::new())).emitting("\"\""),
        Case::new("non-ASCII string", Value::String("naïve—Ω".into())).emitting("\"naïve—Ω\""),
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
    /// assembled straight from random bits.
    ///
    /// MEASURED, not assumed: these land on the FIRST rung the large majority
    /// of the time. `ri_literal`'s module doc measures naive-division failure
    /// at only ~2% for `mm` and ~9% for `deg`, and the acceptance test here is
    /// exactly that division — so an arbitrary Length is mm-exact ~98% of the
    /// time. This generator is therefore broad *value* coverage (awkward
    /// mantissas, wide exponent range), NOT a fallback-path generator. An
    /// earlier version of this comment claimed the opposite and was wrong by
    /// the module's own numbers; reaching the fallback path reliably needs
    /// [`Self::inexact_at`].
    fn arbitrary_f64(&mut self) -> f64 {
        let bits = self.next_u64();
        let sign = bits & (1u64 << 63);
        let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
        let exponent = ((bits >> 52) & 0x7FF) % 61 + (1023 - 30);
        f64::from_bits(sign | (exponent << 52) | mantissa)
    }

    /// An `f64` the rung with SI factor `factor` CANNOT recover exactly, so
    /// the ladder is forced to walk past it.
    ///
    /// Reject-and-retry over [`Self::arbitrary_f64`] against the serializer's
    /// own acceptance predicate. Sampling FOR the fallback path rather than
    /// hoping to stumble into it is what turns the coverage guard below into a
    /// real floor instead of an accident of the seed.
    ///
    /// Only meaningful for a factor that is not `1.0`: `mag * 1.0 == si_value`
    /// holds by IEEE identity, so a factor-1.0 rung has no inexact values to
    /// find and this would spin. Callers gate on a multi-rung ladder for
    /// exactly that reason. The draw bound is a hang-guard, not an expected
    /// path — at a ~2% hit rate a draw succeeds within a few dozen tries.
    fn inexact_at(&mut self, factor: f64) -> f64 {
        for _ in 0..10_000 {
            let si = self.arbitrary_f64();
            if (si / factor) * factor != si {
                return si;
            }
        }
        panic!(
            "no value inexact at factor {factor} found in 10000 draws — either the \
             generator collapsed or {factor} is 1.0 (which has no inexact values)"
        );
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
        let ladder = reify_core::ri_emittable_units(&dimension);
        // Mass/Time/Temperature ladders are a single factor-1.0 rung, which is
        // exact for EVERY finite value: there is no later rung to reach and no
        // inexact value to construct. So the forced-fallback generator is
        // gated on a multi-rung ladder (Length, Angle).
        let multi_rung = ladder.len() > 1;
        let first_factor = reify_core::unit_symbol_to_si(ladder[0])
            .expect("every ladder rung is a bare built-in")
            .0;

        for i in 0..count {
            // Cycle THREE generators so each path gets sampled coverage rather
            // than incidental coverage:
            let si_value = match i % 3 {
                //   0 — a "human" magnitude scaled by a real rung factor: the
                //       accept path, landing on an early rung.
                0 => {
                    let factor = factors[(rng.next_u64() as usize) % factors.len()];
                    rng.human_magnitude() * factor
                }
                //   1 — an arbitrary f64: broad value coverage. These mostly
                //       land on the FIRST rung too (~98% for Length), which is
                //       why they cannot serve as the fallback sample.
                1 => rng.arbitrary_f64(),
                //   2 — constructed to MISS the first rung, so the ladder is
                //       forced to walk. This is the fallback path, and it is
                //       what gives the coverage guard below a floor to assert.
                _ if multi_rung => rng.inexact_at(first_factor),
                //       Single-rung dimension: nothing to fall through to.
                _ => rng.arbitrary_f64(),
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
    //
    // Counted over MULTI-RUNG dimensions ONLY. Mass/Time/Temperature have a
    // single rung, so their 60 cases satisfy `unit == ladder[0]`
    // unconditionally — no matter what the ladder code does. Including them
    // made the `first_rung > 0` half of this assertion vacuous, which is the
    // hole this counts around.
    let mut first_rung = 0usize;
    let mut later_rung = 0usize;
    let mut multi_rung_cases = 0usize;
    for case in &cases {
        let dim = case.value.dimension();
        let ladder = reify_core::ri_emittable_units(&dim);
        if ladder.len() < 2 {
            continue;
        }
        multi_rung_cases += 1;
        let literal = value_to_ri_literal(&case.value).expect("sweep value is serializable");
        if trailing_unit(&literal) == ladder[0] {
            first_rung += 1;
        } else {
            later_rung += 1;
        }
    }
    println!(
        "sweep ladder coverage (multi-rung dimensions only): {first_rung} first-rung, \
         {later_rung} later-rung out of {multi_rung_cases} multi-rung cases \
         ({} total)",
        cases.len()
    );

    // FLOORS, not `> 0`. One case in three on a multi-rung dimension is
    // *constructed* to miss the first rung (`inexact_at`), so a healthy sweep
    // clears LATER_RUNG_FLOOR by a wide margin. MEASURED at this seed:
    // 129 first-rung, 71 later-rung out of 200 multi-rung cases — 67 of those
    // 71 are the forced-inexact draws (40 Length + 27 Angle), the other 4 are
    // incidental. Both floors are 50, so neither is close to tripping on
    // healthy code, and neither is satisfiable by a collapsed ladder.
    //
    // Two distinct ladder regressions are caught here before any exactness
    // assertion fires: a ladder collapsed to its factor-1.0 terminator drives
    // later_rung to 0, and a ladder that lost its terminator makes the
    // forced-inexact cases unserializable, which the `expect` above turns into
    // a panic.
    const LATER_RUNG_FLOOR: usize = 50;
    const FIRST_RUNG_FLOOR: usize = 50;
    assert!(
        later_rung >= LATER_RUNG_FLOOR,
        "sweep barely exercised the ladder-fallback path: {later_rung} later-rung \
         (floor {LATER_RUNG_FLOOR}) out of {multi_rung_cases} multi-rung cases"
    );
    assert!(
        first_rung >= FIRST_RUNG_FLOOR,
        "sweep barely exercised the first-rung accept path: {first_rung} first-rung \
         (floor {FIRST_RUNG_FLOOR}) out of {multi_rung_cases} multi-rung cases"
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

// ─── (d) the registry precondition ───────────────────────────────────────────

/// Build the stdlib-seeded `UnitRegistry` the way the compiler does.
///
/// The seeded registry is never surfaced across the compile boundary, so this
/// calls `seed_prelude_units` — the one implementation `phase_units` seeds
/// with — over the stdlib modules, and asserts that seeding them reports no
/// collision.
///
/// Iterating EVERY stdlib module matters: `mm` is not declared in
/// `stdlib/units.ri` at all — it is generated into `std.si_units` from the
/// milli prefix — so a `std/units`-only scan would miss the single most
/// frequently emitted symbol in the whole ladder.
fn stdlib_seeded_unit_registry() -> UnitRegistry {
    let modules: Vec<&CompiledModule> = stdlib_loader::load_stdlib().iter().collect();
    let mut registry = UnitRegistry::new();
    let diags = seed_prelude_units(&mut registry, &modules);
    assert!(
        diags.is_empty(),
        "seeding the stdlib must report no cross-module unit collision: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    registry
}

/// CROSS-GUARD against the table that actually WINS at resolution time.
///
/// The serializer proves exactness against `reify_core::unit_symbol_to_si`,
/// the built-in table. But the compiler resolves a bare unit as
/// `scope.lookup_unit_in_registry(..).or_else(|| unit_to_scalar(..))`
/// (`reify-compiler/src/expr.rs`) — the per-module `UnitRegistry` is consulted
/// FIRST and the built-in table is only its fallback. So for a module that
/// imports `std.units`, the registry's factor is the one the literal is
/// actually multiplied by, and the serializer's proof is only as good as the
/// two tables agreeing.
///
/// Today they do, bit-for-bit. Nothing else pins that. `units.ri:24` declares
/// `deg = 3.141592653589793 / 180`, which agrees with `std::f64::consts::PI /
/// 180.0` only because those are the same decimal digits divided the same way
/// — rewrite it as `PI() / 180` evaluated differently, or nudge a digit, and
/// every other test in this diff stays green while every `deg` write-back
/// silently becomes lossy. Hence an assertion on `.to_bits()`, not a tolerance.
///
/// Sibling guard: `reify-core`'s
/// `ri_emittable_units_agrees_physically_with_display_ladders` does the same
/// job against the GUI display ladders. This one is the load-bearing half —
/// display ladders never resolve a literal.
#[test]
fn stdlib_unit_declarations_agree_bit_for_bit_with_the_builtin_table() {
    let registry = stdlib_seeded_unit_registry();

    // Driven from the built-in table ITSELF, not reconstructed by looping the
    // emission ladders and appending a hand-listed tail. A caller-supplied hint
    // is honoured on exactly the same terms as a ladder rung, so EVERY built-in
    // is hint-reachable and every built-in must be exposed to registry drift —
    // there is no such thing as a symbol this guard may skip.
    //
    // The old reconstruction happened to name all 13 symbols today, but it had
    // exactly the defect removed from `reify-core::units` in this same task: add
    // a built-in whose dimension ALREADY has a ladder — say
    // `("ft", 0.3048, LENGTH)` — and the ladder loop would not name it and the
    // hardcoded `["in", "g"]` tail would not either, so it would silently never
    // be compared against the registry. Since the compiler resolves a bare unit
    // as `lookup_unit_in_registry(..).or_else(|| unit_to_scalar(..))`, the
    // registry's factor is the one a rewritten literal is actually multiplied
    // by, so `std.units` shadowing that symbol with a different factor would
    // void `value_to_ri_literal`'s bit-exactness proof for it — with every test
    // in this file green.
    let mut compared = 0usize;
    let mut builtin_only: Vec<&'static str> = Vec::new();

    for &(sym, _, _) in reify_core::BUILTIN_UNITS {
        let (builtin_factor, builtin_dim) = reify_core::unit_symbol_to_si(sym)
            .unwrap_or_else(|| panic!("emittable symbol {sym:?} must be a bare built-in"));

        let Some(entry) = registry.lookup(sym) else {
            // Not shadowed at all — the built-in fallback wins unconditionally,
            // which is the *strongest* form of the guarantee, not a gap.
            // Nothing lands here today: every built-in, `A`/`mol`/`cd`
            // included, is declared in `stdlib/units.ri`.
            builtin_only.push(sym);
            continue;
        };

        assert_eq!(
            entry.factor.to_bits(),
            builtin_factor.to_bits(),
            "unit {sym:?} DRIFTED between the two tables that resolve it.\n  \
             stdlib registry ({}): factor = {:?}  bits = {:#018x}\n  \
             unit_symbol_to_si:      factor = {builtin_factor:?}  bits = {:#018x}\n  \
             The registry is consulted FIRST, so it is the factor a rewritten \
             literal is actually multiplied by — value_to_ri_literal's \
             bit-exactness proof is against the built-in table and is now void \
             for this symbol.",
            entry.source_module.as_deref().unwrap_or("?"),
            entry.factor,
            entry.factor.to_bits(),
            builtin_factor.to_bits()
        );
        assert_eq!(
            entry.dimension, builtin_dim,
            "unit {sym:?} has dimension {:?} in the stdlib registry but {:?} in \
             the built-in table",
            entry.dimension.canonical_name(),
            builtin_dim.canonical_name()
        );
        assert!(
            entry.offset.is_none(),
            "unit {sym:?} is AFFINE in the stdlib registry (offset {:?}), but the \
             built-in table has no offset to represent. `si = mag * factor + \
             offset` is not the arithmetic the serializer proves against, so an \
             affine shadow silently offsets every write-back of this symbol.",
            entry.offset
        );
        compared += 1;
    }

    println!(
        "stdlib/built-in unit cross-guard: {compared} symbols compared, \
         built-in-only (unshadowed): {builtin_only:?}"
    );
    // The guard is worthless if it silently compares nothing — e.g. if
    // `load_stdlib()`'s module set changed shape and every lookup missed.
    // `stdlib/units.ri` declares exactly these 13: mm/cm/m/in/deg/rad/kg/g/s/K/
    // A/mol/cd.
    assert_eq!(
        compared, 13,
        "expected exactly the 13 stdlib-declared built-in symbols to be compared \
         (built-in-only: {builtin_only:?}) — did the stdlib module set or the \
         seeding shape change?"
    );
}

/// DIRECT: a serializer-emitted literal, spliced into a module that imports
/// `std.units`, re-parses bit-exactly through the registry-FIRST path.
///
/// The cross-guard above compares two TABLES. That is one step removed from
/// the property that matters: it pins that `units.ri`'s factors equal
/// `unit_symbol_to_si`'s, not that a literal `value_to_ri_literal` actually
/// wrote survives `expr.rs`'s `lookup_unit_in_registry(..).or_else(..)`
/// resolution. Everything between the two — the `import` declaration, prelude
/// seeding order, `UnitEntry`'s factor plumbing, the quantity-literal lowering
/// — is unexercised by a table comparison and is exactly where γ's edit path
/// lives, since user modules usually DO `import std.units`.
///
/// So this runs the same `assert_identical` the unshadowed sweep runs, over
/// the shadowed regime. Keep the table cross-guard as the cheap canary: it
/// localises a drift to a specific symbol and factor, which a round-trip
/// failure would only hint at.
///
/// The case list covers all 13 symbols the cross-guard compares — the eleven
/// canonical ladder rungs plus `in` and `g`, which are reachable only through a
/// caller-supplied hint — so a registry shadow on ANY of them fails here. It is
/// hand-written rather than driven from `BUILTIN_UNITS` because each case needs
/// a magnitude and a declared `.ri` type, not just a symbol; the cross-guard
/// above is the table-driven half, and the two are meant to be read together.
#[test]
fn emitted_literals_round_trip_under_the_stdlib_unit_registry() {
    let cases = vec![
        // Length: all four symbols. `m` and `in` need a hint (canonical would
        // be `1000mm` and `25.4mm`).
        Case::new("stdlib mm", Value::length(0.08)).emitting("80mm"),
        Case::new("stdlib cm", Value::length(0.05))
            .with_hint("cm")
            .emitting("5cm"),
        Case::new("stdlib m", Value::length(1.0))
            .with_hint("m")
            .emitting("1m"),
        Case::new("stdlib in", Value::length(0.0254))
            .with_hint("in")
            .emitting("1in"),
        // The mm-inexact witness, so the ladder-fallback rung is resolved
        // through the registry too, not just the first rung.
        Case::new("stdlib cm via fallback", Value::length(-0.5566166674539299))
            .emitting("-55.66166674539299cm"),
        // Angle: `deg` is THE drift risk — `units.ri` declares it as
        // `3.141592653589793 / 180`, which matches `PI / 180.0` only because
        // those are the same digits divided the same way.
        Case::new("stdlib deg", Value::angle(std::f64::consts::FRAC_PI_2)).emitting("90deg"),
        Case::new("stdlib rad", Value::angle(std::f64::consts::FRAC_PI_2))
            .with_hint("rad")
            .emitting("1.5707963267948966rad"),
        // Mass: the SI base and the hint-only sub-unit.
        Case::new(
            "stdlib kg",
            Value::Scalar {
                si_value: 2.75,
                dimension: DimensionVector::MASS,
            },
        )
        .emitting("2.75kg"),
        Case::new(
            "stdlib g",
            Value::Scalar {
                si_value: 0.25,
                dimension: DimensionVector::MASS,
            },
        )
        .with_hint("g")
        .emitting("250g"),
        Case::new(
            "stdlib s",
            Value::Scalar {
                si_value: -0.125,
                dimension: DimensionVector::TIME,
            },
        )
        .emitting("-0.125s"),
        // `K` is the one to watch for an AFFINE shadow: `degC` lives beside it
        // in units.ri with an offset, and `si = mag * factor + offset` is not
        // the arithmetic the serializer proves against.
        Case::new(
            "stdlib K",
            Value::Scalar {
                si_value: 293.15,
                dimension: DimensionVector::TEMPERATURE,
            },
        )
        .emitting("293.15K"),
        // The three SI bases declared bare in `stdlib/units.ri` alongside the
        // built-in fallback entries for them.
        Case::new(
            "stdlib A",
            Value::Scalar {
                si_value: 1.5,
                dimension: DimensionVector::CURRENT,
            },
        )
        .emitting("1.5A"),
        Case::new(
            "stdlib mol",
            Value::Scalar {
                si_value: 0.25,
                dimension: DimensionVector::AMOUNT_OF_SUBSTANCE,
            },
        )
        .emitting("0.25mol"),
        Case::new(
            "stdlib cd",
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LUMINOUS_INTENSITY,
            },
        )
        .emitting("3cd"),
    ];
    assert_round_trips_under(&cases, Prelude::Stdlib);
}

// ─── (d2) compound literals (tasks #6400, #10778) ────────────────────────────

/// Hand-picked compound values, each pinning its exact literal where the shape
/// is the point. Coverage of every compound dimension the serializer writes is
/// `named_compound_dimensions`' job.
///
/// The awkward-bits cases are the point, not padding. A naive implementation
/// that scaled the magnitude — or reproduced `resolve_unit_expr`'s fold by hand
/// and got the association wrong — passes on `2.5` and fails here.
fn compound_cases() -> Vec<Case> {
    fn compound(label: &'static str, si_value: f64, dimension: DimensionVector) -> Case {
        Case::new(
            label,
            Value::Scalar {
                si_value,
                dimension,
            },
        )
    }

    vec![
        compound("area", 2.5, DimensionVector::AREA).emitting("2.5m^2"),
        compound("volume", 1e-6, DimensionVector::VOLUME).emitting("1e-6m^3"),
        compound("density", 7850.0, DimensionVector::MASS_DENSITY).emitting("7850kg/m^3"),
        // ASSOCIATIVITY WITNESS. `unit_expr` is `prec.left(1)` for `*` and `/`,
        // so this must fold as `Div(Div(kg, m), s)`. Under RIGHT association it
        // would be `Div(kg, Div(m, s))` — dimension `kg·m^-1·s^+1`, with the
        // Time exponent's SIGN flipped — which `assert_identical`'s dimension
        // check catches. So this case confirms the association against the
        // real parse rather than assuming it, and it is the reason a division
        // CHAIN is safe to emit at all.
        compound(
            "dynamic viscosity",
            0.001,
            DimensionVector::DYNAMIC_VISCOSITY,
        )
        .emitting("0.001kg/m/s"),
        // A second division-chain ASSOCIATIVITY WITNESS: right association
        // would read `s^3/A` as one denominator and flip A's sign.
        compound("voltage", 230.0, DimensionVector::VOLTAGE).emitting("230m^2*kg/s^3/A"),
        compound("charge", 2.5, DimensionVector::CHARGE).emitting("2.5s*A"),
        // The empty-numerator form. A leading `/` is not a valid `unit_expr`,
        // so this is the one shape that MUST use signed powers — and only a
        // real parse can confirm `s^-1` lexes and folds as `Pow(s, -1)`.
        compound("frequency", 60.0, DimensionVector::FREQUENCY).emitting("60s^-1"),
        // NEGATIVE ZERO on a compound dimension. `-0.0` and `0.0` are the one
        // pair that compares equal while differing in bits, so `to_bits()` in
        // `assert_identical` is the only assertion here that can tell them
        // apart — and the sign must survive both the `.0`-strip in
        // `format_f64_shortest` and the `unary_expression` constant fold.
        compound("negative-zero area", -0.0, DimensionVector::AREA).emitting("-0m^2"),
        // Magnitudes a scaling or rounding implementation gets wrong.
        compound("long decimal area", 0.1 + 0.2, DimensionVector::AREA)
            .emitting("0.30000000000000004m^2"),
        compound(
            "subnormal volume",
            f64::MIN_POSITIVE / 2.0,
            DimensionVector::VOLUME,
        ),
        compound("tiny area", 1e-9, DimensionVector::AREA),
        compound(
            "huge volume",
            1.7976931348623157e308,
            DimensionVector::VOLUME,
        ),
        compound(
            "awkward density",
            -0.5566166674539299,
            DimensionVector::MASS_DENSITY,
        ),
        // A RATIONAL exponent (task #9236): FractureToughness's Length^(-1/2)
        // in `Rational`'s `p/q` spelling, read back by the unit grammar.
        compound(
            "fracture toughness",
            5e7,
            DimensionVector::FRACTURE_TOUGHNESS,
        )
        .emitting("50000000kg/m^1/2/s^2"),
        compound(
            "awkward fracture toughness",
            -0.5566166674539299,
            DimensionVector::FRACTURE_TOUGHNESS,
        ),
    ]
}

/// The serializer's compound literals compile with NO unit registry seeded and
/// no import, and re-parse BIT-EXACTLY: every atom is a factor-1.0 built-in,
/// and the compiler resolves a compound atom registry-first, then the built-in
/// table (task #5503). "No registry seeded" means none SEEDED; a structure
/// param meets the position precondition (`reify_ir::ri_literal`
/// qualification 3). This is the "round-trips in a module that imports
/// nothing" claim.
///
/// This tier is the only one that can prove it, because it is the only one that
/// runs the real parser and `resolve_unit_expr`. `reify-core` can pin the
/// emitted STRING and `reify-ir` can pin the emitted MAGNITUDE, but neither can
/// see whether `m^2` resolves, what factor it folds to, or whether `kg/m/s`
/// even parses.
#[test]
fn compound_literals_round_trip_with_no_unit_registry() {
    assert_round_trips_under(&compound_cases(), Prelude::None);
}

/// The same compound literals, resolved registry-first against the stdlib
/// `UnitRegistry` — the SHADOWED case a user module importing `std.units`
/// presents.
#[test]
fn compound_literals_round_trip_under_the_stdlib_unit_registry() {
    assert_round_trips_under(&compound_cases(), Prelude::Stdlib);
}

/// Every `NAMED_DIMENSIONS` dimension the serializer writes as a compound:
/// dimensioned, with no bare ladder, and not refused. Alias rows are skipped,
/// since each repeats its canonical row's dimension.
///
/// Derived rather than listed, like the set it covers: the serializer's
/// built-in slot table is filtered out of `BUILTIN_UNITS` at call time, so a
/// dimension it starts writing joins this list, and is re-parsed, the same
/// day. `a_refused_dimensions_spelling_does_not_compile_without_its_import`
/// is the refused-side mirror.
fn named_compound_dimensions() -> Vec<(DimensionVector, &'static str)> {
    reify_core::NAMED_DIMENSIONS
        .iter()
        .copied()
        .filter(|&(dimension, name)| {
            dimension.canonical_name() == Some(name)
                && !dimension.is_dimensionless()
                && reify_core::ri_emittable_units(&dimension).is_empty()
                && value_to_ri_literal(&Value::Scalar {
                    si_value: 1.0,
                    dimension,
                })
                .is_ok()
        })
        .collect()
}

/// SWEPT: every compound dimension the serializer writes re-parses with
/// identical bits and dimension, under both unit-resolution regimes.
///
/// `compound_cases` holds the exact-string and awkward-bits witnesses for a
/// few dimensions; this carries one plain and one awkward magnitude for all
/// of them.
#[test]
fn every_named_compound_dimension_round_trips_under_both_regimes() {
    let rows = named_compound_dimensions();
    // MEASURED: 34 canonical rows today. A floor, not an equality: the set
    // may grow by itself (see `named_compound_dimensions`), but a shrink means
    // NAMED_DIMENSIONS lost rows or the serializer stopped writing some.
    const NAMED_COMPOUND_ROWS_FLOOR: usize = 34;
    let names: Vec<&str> = rows.iter().map(|&(_, name)| name).collect();
    println!(
        "compound NAMED_DIMENSIONS rows swept: {}: {names:?}",
        rows.len()
    );
    assert!(
        rows.len() >= NAMED_COMPOUND_ROWS_FLOOR,
        "expected at least {NAMED_COMPOUND_ROWS_FLOOR} compound NAMED_DIMENSIONS \
         rows, saw {}: {names:?}",
        rows.len()
    );

    let cases: Vec<Case> = rows
        .iter()
        .flat_map(|&(dimension, name)| {
            [2.5, -0.5566166674539299].map(|si_value| {
                Case::new(
                    format!("{name} {si_value:?}"),
                    Value::Scalar {
                        si_value,
                        dimension,
                    },
                )
            })
        })
        .collect();
    assert_round_trips_under(&cases, Prelude::None);
    assert_round_trips_under(&cases, Prelude::Stdlib);
}

/// BIDIRECTIONAL CROSS-GUARD — the registry-safety assertion this task is
/// named for, and the load-bearing premise behind every case above.
///
/// `RI_COMPOUND_BASE_SYMBOLS` encodes an EMPIRICAL fact about the stdlib: which
/// SI base symbols are actually seeded into the registry, and at what factor.
/// Both directions are checked, because both directions are failures:
///
///   - a `Some` slot MUST be present at factor exactly 1.0. `.to_bits()`, not a
///     tolerance: a factor of `0.9999999999999999` would make every compound
///     write of that dimension silently lossy while looking right in a debug
///     print, and it is exactly the identity `mag * factor == si_value` rests
///     on;
///   - a `None` slot MUST be absent. If the stdlib GAINS a bare declaration
///     for such a slot, the emitter is now needlessly refusing a dimension it
///     could write — a capability gap, not a corruption, but still a bug, and
///     one no forward-only test can see.
#[test]
fn si_base_symbols_resolve_to_factor_one_in_the_stdlib_registry() {
    let registry = stdlib_seeded_unit_registry();
    let mut compared = 0usize;

    for (i, sym) in reify_core::dimension::BASE_UNIT_SYMBOLS.iter().enumerate() {
        match reify_core::RI_COMPOUND_BASE_SYMBOLS[i] {
            Some(emittable) => {
                assert_eq!(
                    emittable, *sym,
                    "slot {i}: RI_COMPOUND_BASE_SYMBOLS says {emittable:?} but \
                     BASE_UNIT_SYMBOLS says {sym:?} — the two tables must name the \
                     same symbol for a slot, or the emitted expression labels the \
                     wrong base dimension"
                );
                let entry = registry.lookup(sym).unwrap_or_else(|| {
                    panic!(
                        "slot {i}: RI_COMPOUND_BASE_SYMBOLS marks {sym:?} emittable, \
                         but it is NOT in the stdlib-seeded UnitRegistry, so a \
                         compound literal naming {sym:?} no longer resolves through \
                         the stdlib registry. Either restore the stdlib declaration, \
                         or set slot {i} to `None` and drop that dimension's cases."
                    )
                });
                assert_factor_one_entry(entry, sym, basis_dimension(i), &format!("slot {i}"));
                compared += 1;
            }
            None => {
                assert!(
                    registry.lookup(sym).is_none(),
                    "slot {i}: the stdlib has GAINED a bare {sym:?} declaration \
                     (factor {:?}), but RI_COMPOUND_BASE_SYMBOLS still marks the slot \
                     unemittable — so `ri_compound_unit_expr` needlessly REFUSES \
                     every dimension touching it. Widen RI_COMPOUND_BASE_SYMBOLS to \
                     `Some({sym:?})`; do NOT weaken this assertion.",
                    registry.lookup(sym).map(|e| e.factor)
                );
                compared += 1;
            }
        }
    }

    // Non-vacuity: all ten slots are decided, and all ten are emittable
    // (m/kg/s/A/K/mol/cd/rad/sr/USD).
    assert_eq!(
        compared, 10,
        "every BASE_UNIT_SYMBOLS slot must be decided in one direction or the other"
    );
    let emittable = reify_core::RI_COMPOUND_BASE_SYMBOLS
        .iter()
        .filter(|s| s.is_some())
        .count();
    assert_eq!(
        emittable, 10,
        "expected all 10 registry-resolvable base symbols \
         (m/kg/s/A/K/mol/cd/rad/sr/USD) to be emittable — did the stdlib lose a \
         unit declaration?"
    );
}

/// FORWARD CROSS-GUARD for `RI_NAMED_DERIVED_SYMBOLS`: every named derived
/// symbol the emitter may write is seeded in the stdlib registry at factor
/// exactly 1.0, with no offset and the row's dimension.
///
/// Forward-only by design. The reverse — "every factor-1.0 registry unit is
/// tabled" — is false by construction: `Hz`/`Bq` share Frequency and `Gy`/`Sv`
/// share AbsorbedDose, so a whole-dimension table can hold at most one of each
/// pair, and other factor-1.0 symbols are deliberately untabled. The reverse
/// coupling that matters — the emitter actually reads the table — is a pure
/// reify-core property, pinned by its
/// `ri_named_derived_symbols_are_unambiguous_and_consulted`.
#[test]
fn named_derived_symbols_resolve_to_factor_one_in_the_stdlib_registry() {
    let registry = stdlib_seeded_unit_registry();
    for &(sym, dim) in reify_core::RI_NAMED_DERIVED_SYMBOLS {
        let entry = registry.lookup(sym).unwrap_or_else(|| {
            panic!(
                "RI_NAMED_DERIVED_SYMBOLS names {sym:?}, but it is NOT in the \
                 stdlib-seeded UnitRegistry, so every literal written with it fails \
                 to resolve. Restore its si_units.rs declaration or drop the row."
            )
        });
        assert_factor_one_entry(entry, sym, dim, "RI_NAMED_DERIVED_SYMBOLS");
    }
    assert_eq!(
        reify_core::RI_NAMED_DERIVED_SYMBOLS.len(),
        4,
        "expected exactly the four N/Pa/J/W rows in RI_NAMED_DERIVED_SYMBOLS — \
         did the named-derived table change?"
    );
}

/// The factor-one invariant both registry guards enforce: `entry` (for `sym`)
/// has no offset, factor bits exactly `1.0`, and dimension `expected`.
///
/// `.to_bits()`, not a tolerance: a factor of `0.9999999999999999` would make
/// every write naming `sym` silently lossy while looking right in a debug
/// print, and `mag * factor == si_value` is exactly the identity the emitter
/// rests on. `context` prefixes every message so a failure names its table row.
fn assert_factor_one_entry(entry: &UnitEntry, sym: &str, expected: DimensionVector, context: &str) {
    let module = entry.source_module.as_deref().unwrap_or("?");
    assert!(
        entry.offset.is_none(),
        "{context}: {sym:?} is AFFINE in the registry (offset {:?}, declared in \
         {module}). `si = mag * factor + offset` is not the arithmetic the emitter \
         proves against — its whole exactness argument is that the fold is the \
         IEEE identity.",
        entry.offset
    );
    assert_eq!(
        entry.factor.to_bits(),
        1.0f64.to_bits(),
        "{context}: {sym:?} resolves at factor {:?} (bits {:#018x}), not exactly 1.0 \
         (bits {:#018x}) — declared in {module}. The emitter writes the SI value \
         VERBATIM as the magnitude precisely because the fold is exactly 1.0; at \
         any other factor every write naming {sym:?} is silently lossy.",
        entry.factor,
        entry.factor.to_bits(),
        1.0f64.to_bits()
    );
    assert_eq!(
        entry.dimension,
        expected,
        "{context}: {sym:?} resolves to dimension {:?} (declared in {module}), not {:?}",
        entry.dimension.canonical_name(),
        expected.canonical_name()
    );
}

/// The single-slot base dimension for `index`, built through
/// `DimensionVector`'s public tuple field.
///
/// `DimensionVector::basis` is private to `dimension.rs`, and the named
/// constants do not cover every slot, so this is the only way to name slot `i`
/// generically from outside the crate.
fn basis_dimension(index: usize) -> DimensionVector {
    let mut exps = [reify_core::Rational::ZERO; 10];
    exps[index] = reify_core::Rational::ONE;
    DimensionVector(exps)
}

/// LOCALISING CANARY for the round-trip above: the arithmetic premise itself.
///
/// `resolve_unit_expr` folds a `UnitExpr` with exactly three operations —
/// `Pow: fa.powi(n)` (or `fa.powf(p/q)` for a rational exponent),
/// `Mul: fa * fb`, `Div: fa / fb`. When every leaf is
/// exactly 1.0, all three are the identity, so the folded factor is exactly 1.0
/// REGARDLESS of tree shape or association, and `expr.rs`'s single
/// `si_value = value * factor` multiply leaves the magnitude untouched.
///
/// That is what lets the emitter write the SI value verbatim without
/// reproducing the compiler's fold order — the task's hard constraint,
/// discharged by making the order irrelevant rather than by matching it.
/// Pinning it here means a `powi` regression localises to this test instead of
/// surfacing as a mystery bit-mismatch across a dozen round-trip cases.
#[test]
fn the_ieee_fold_of_factor_one_atoms_is_exactly_one() {
    let one = 1.0f64;
    assert_eq!(
        (one * one).to_bits(),
        one.to_bits(),
        "UnitExpr::Mul over factor-1.0 atoms must be exactly 1.0"
    );
    assert_eq!(
        (one / one).to_bits(),
        one.to_bits(),
        "UnitExpr::Div over factor-1.0 atoms must be exactly 1.0"
    );
    // Every exponent the emitter can possibly write: `ri_compound_unit_expr`
    // and `resolve_unit_expr` both accept exactly
    // `reify_core::unit_exponent_in_range` — numerator and denominator in i8.
    for n in i8::MIN..=i8::MAX {
        assert_eq!(
            one.powi(i32::from(n)).to_bits(),
            one.to_bits(),
            "1.0f64.powi({n}) must be exactly 1.0 — the compound path's magnitude \
             is the SI value verbatim only because this holds for every exponent"
        );
    }
    // A rational exponent folds through `powf`, which is exact at base 1.0 too.
    for p in i8::MIN..=i8::MAX {
        for q in 2..=i8::MAX {
            let y = f64::from(p) / f64::from(q);
            assert_eq!(
                one.powf(y).to_bits(),
                one.to_bits(),
                "1.0f64.powf({p}/{q}) must be exactly 1.0"
            );
        }
    }
}

/// NEGATIVE PIN: every dimension the serializer refuses although
/// `ri_compound_unit_expr` can spell it really does fail to compile without its
/// import, so each refusal still guards a real failure.
///
/// The refused spellings need `N`/`Pa`/`J`/`W`, `sr` or `USD`, none of which is
/// a built-in. Swept over `NAMED_DIMENSIONS`, so no refused dimension can dodge
/// it. If one of those symbols ever becomes import-free, this fires for its
/// dimensions: that is when the serializer can widen to them.
#[test]
fn a_refused_dimensions_spelling_does_not_compile_without_its_import() {
    let mut swept: Vec<&str> = Vec::new();
    for &(dim, name) in reify_core::NAMED_DIMENSIONS {
        if dim.is_dimensionless() {
            continue;
        }
        let refused = matches!(
            value_to_ri_literal(&Value::Scalar {
                si_value: 1.0,
                dimension: dim,
            }),
            Err(RiLiteralError::UnrepresentableDimension { .. })
        );
        let Some(unit) = reify_core::ri_compound_unit_expr(&dim) else {
            continue;
        };
        if !refused {
            continue;
        }
        let source = format!("structure def S {{\n  param x : {name} = 1{unit}\n}}\n");
        let compiled = reify_test_support::compile_source_allow_parse_errors(&source);
        let errors: Vec<&reify_core::Diagnostic> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect();
        assert!(
            errors.iter().any(|d| d.message.contains("unknown unit")),
            "{name}: the serializer refuses it, but `1{unit}` does not fail with \
             `unknown unit` in a module that imports nothing — the refusal no longer \
             guards a real failure, so the serializer can widen to {name}. \
             Errors: {errors:?}\nSource:\n{source}"
        );
        swept.push(name);
    }
    for expected in [
        "Force",
        "Pressure",
        "Energy",
        "Power",
        "SolidAngle",
        "Money",
        "Illuminance",
    ] {
        assert!(
            swept.contains(&expected),
            "{expected} was not swept (swept: {swept:?}) — is it no longer refused?"
        );
    }
}

/// POSITION PIN: a compound literal re-parses only where the compile scope
/// carries a `UnitRegistry`. A fn body and a fn param default compile with
/// none, whatever the module imports, so the serializer's compound output is
/// rejected there; a bare literal has no such precondition.
#[test]
fn compound_literals_do_not_compile_in_a_fn_body_or_fn_param_default() {
    const NEEDS_A_REGISTRY: &str = "compound unit expression requires a unit registry in scope";

    fn compile_errors(source: &str, prelude: Prelude) -> Vec<String> {
        let compiled = match prelude {
            Prelude::None => reify_test_support::compile_source_allow_parse_errors(source),
            Prelude::Stdlib => reify_test_support::compile_source_with_stdlib_allow_parse_errors(
                &format!("import std.units\n{source}"),
            ),
        };
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .map(|d| d.message.clone())
            .collect()
    }

    let compound = value_to_ri_literal(&Value::Scalar {
        si_value: 2.5,
        dimension: DimensionVector::AREA,
    })
    .expect("an Area is a builtin-only compound the serializer writes");
    assert_eq!(compound, "2.5m^2");
    let bare = value_to_ri_literal(&Value::Scalar {
        si_value: 0.08,
        dimension: DimensionVector::LENGTH,
    })
    .expect("a Length has a bare builtin symbol");
    assert_eq!(bare, "80mm");

    type SourceAt = fn(ty: &str, lit: &str) -> String;
    let positions: [(&str, SourceAt); 2] = [
        ("fn param default", |ty, lit| {
            format!("fn f(x: {ty} = {lit}) -> {ty} {{ x }}\n")
        }),
        ("fn body", |ty, lit| format!("fn g() -> {ty} {{ {lit} }}\n")),
    ];
    for prelude in [Prelude::None, Prelude::Stdlib] {
        for (position, source_at) in positions {
            let source = source_at("Area", &compound);
            let errors = compile_errors(&source, prelude);
            assert!(
                errors.iter().any(|m| m.contains(NEEDS_A_REGISTRY)),
                "Prelude::{prelude:?}, {position}: the compound literal {compound:?} no longer \
                 fails for want of a unit registry, so fn scopes now carry one. Drop the \
                 position precondition from reify-ir ri_literal.rs qualification 3, \
                 update every doc that points at it (grep `position precondition`), \
                 then retire this pin. Errors: {errors:?}\nSource:\n{source}"
            );

            let control = source_at("Length", &bare);
            let control_errors = compile_errors(&control, prelude);
            assert!(
                control_errors.is_empty(),
                "Prelude::{prelude:?}, {position}: the bare control {bare:?} must compile \
                 clean, or the compound failure above is not specific to compound literals. \
                 Errors: {control_errors:?}\nSource:\n{control}"
            );
        }
    }
}

// ─── (e) rejections are never spliced ────────────────────────────────────────

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
        "a Pressure must be refused: its spelling `Pa` needs an import"
    );

    let braced = Value::String("hole {x}".into());
    assert!(
        value_to_ri_literal(&braced).is_err(),
        "a brace-bearing string must be refused — it would divert the token \
         to interpolated_string, which treats {{expr}} as a hole"
    );
}

// ─── (f) directions (direction-typing β) ─────────────────────────────────────

/// A `Direction` is written as `dir(x, y, z)` and re-parses with identical
/// component bits. This rests on `UnitVec3::from_vector` being a bit-level
/// fixed point (docs/prds/v0_6/direction-typing.md C2): re-normalising an
/// already-unit vector changes nothing.
#[test]
fn directions_round_trip_with_identical_bits() {
    use reify_ir::UnitVec3;

    let direction = |v: [f64; 3]| Value::Direction(UnitVec3::from_vector(v).unwrap());
    let mut cases = vec![
        Case::new("+x", direction([1.0, 0.0, 0.0])).emitting("dir(1, 0, 0)"),
        Case::new("-x", direction([-1.0, 0.0, 0.0])).emitting("dir(-1, 0, 0)"),
        Case::new("+y", direction([0.0, 1.0, 0.0])).emitting("dir(0, 1, 0)"),
        Case::new("-y", direction([0.0, -1.0, 0.0])).emitting("dir(0, -1, 0)"),
        Case::new("+z", direction([0.0, 0.0, 1.0])).emitting("dir(0, 0, 1)"),
        Case::new("-z", direction([0.0, 0.0, -1.0])).emitting("dir(0, 0, -1)"),
        Case::new("3-4-5", direction([0.6, 0.8, 0.0])),
        Case::new("1-2-3", direction([1.0, 2.0, 3.0])),
        Case::new("wide range", direction([1e-3, -2.5, 7e5])),
    ];

    // A sweep of arbitrary triplets; a draw from_vector refuses is skipped.
    const SWEEP: usize = 256;
    let fixed = cases.len();
    let mut rng = XorShift64Star(0xD1EC_7104_B0E7_A5EE);
    while cases.len() < fixed + SWEEP {
        let v = [
            rng.arbitrary_f64(),
            rng.arbitrary_f64(),
            rng.arbitrary_f64(),
        ];
        if let Ok(u) = UnitVec3::from_vector(v) {
            let label = format!("sweep #{}", cases.len() - fixed);
            cases.push(Case::new(label, Value::Direction(u)));
        }
    }
    assert_round_trips(&cases);
}
