//! `Value` → `.ri` source-literal serializer (task #5095, ai-native-editing β;
//! PRD `docs/prds/v0_6/ai-native-editing.md`, INV-GUI-3 substrate).
//!
//! Writes a [`Value`] back out as `.ri` source text, so an agent (or the GUI)
//! can rewrite a `param` default in place. The contract is narrow and total:
//!
//! > For every value [`value_to_ri_literal`] returns `Ok` for, re-parsing
//! > that text yields the **same `Value` variant, the same dimension, and the
//! > identical f64 bits**. Everything else is a structured [`RiLiteralError`]
//! > — never a best-effort string.
//!
//! There is no third outcome. A "close enough" literal would silently corrupt
//! a user's design file, which is strictly worse than refusing to edit it.
//!
//! # The three qualifications on that sentence
//!
//! All three are load-bearing for a caller deciding whether a splice is safe,
//! so they are stated here rather than left in an inline comment.
//!
//! **1. One documented variant change.** A *dimensionless* `Value::Scalar` has
//! no unit to write, so it is emitted as a bare real and re-parses as
//! `Value::Real`. Dimension and bits are preserved; only the discriminant
//! moves — and it moves to the variant that `.ri` source has no way to
//! distinguish, since an un-suffixed number literal is a `Real`. This is the
//! sole exception to the "same variant" clause. It is pinned, not merely
//! excluded, by `ri_literal_roundtrip.rs`'s
//! `a_dimensionless_scalar_downgrades_to_real_with_identical_bits`, because
//! the shared round-trip harness asserts discriminant equality and structurally
//! cannot carry this case.
//!
//! **2. A precondition on the target module's unit registry.** The exactness
//! proof below reproduces the arithmetic of `unit_to_scalar`, the *built-in*
//! table. But the compiler resolves a bare unit as
//! `registry.lookup(..).or_else(|| unit_to_scalar(..))` (reify-compiler's
//! `expr.rs`) — the per-module `UnitRegistry` is consulted FIRST, and the
//! built-in table is only its fallback. So the guarantee reads in full:
//!
//! > …provided the target module's unit registry does not *shadow* the emitted
//! > symbol with a different factor.
//!
//! It holds unconditionally for a module that imports nothing (which is what
//! makes the bare-built-in ladder the right choice), and it holds today for a
//! module that imports `std.units`, whose `cm`/`in`/`g`/`deg` declarations
//! agree with the built-in table bit-for-bit — pinned by
//! `ri_literal_roundtrip.rs`'s
//! `stdlib_unit_declarations_agree_bit_for_bit_with_the_builtin_table`, since
//! that registry is the table that actually wins at resolution time. It does
//! NOT hold for a module that declares its own `unit mm : Length = …` with a
//! different factor; nothing in this crate can see that, and a caller splicing
//! into arbitrary user source should treat a user-shadowed unit symbol as
//! outside the guarantee.
//!
//! **3. Compound emission is OPT-IN, and its precondition is strictly
//! stronger.** [`value_to_ri_literal`] and [`value_to_ri_literal_with_unit`]
//! write bare built-in symbols only, and refuse every dimension that would
//! need a compound `UnitExpr`. [`value_to_ri_literal_in_scope`] with
//! [`UnitScope::SiBaseUnitsSeeded`] lifts that refusal (`2.5m^2`,
//! `101325kg/m/s^2`, `7850kg/m^3`) — but qualification 2 above says a bare
//! symbol resolves as `registry.lookup(..).or_else(|| unit_to_scalar(..))`,
//! and the `or_else` is what makes the bare guarantee TOTAL: it holds even in
//! a module that imports nothing. A compound `UnitExpr` has no such fallback.
//! `resolve_unit_expr` (reify-compiler `units.rs`) does a bare
//! `registry.lookup(name)` and returns `UnknownUnit` on a miss, and the
//! registry is populated purely from prelude/imported `.ri` `unit`
//! declarations. So the compound guarantee is CONTINGENT on the target module,
//! where the bare one is not — measured, not inferred: a `2.5m^2` spliced into
//! a `#no_prelude` module is answered `unknown unit: m`, pinned by
//! `ri_literal_roundtrip.rs`'s
//! `a_compound_literal_does_not_compile_without_a_seeded_registry`.
//!
//! `stdlib/units.ri`'s own `STANDARD_GRAVITY` is the in-repo statement of that
//! asymmetry: its body writes the compositional `1m / (1s * 1s)` rather than
//! the compound literal `9.80665m/s^2`, with the comment that "compound-unit
//! resolution requires the unit registry in scope". Making compound the
//! default would silently downgrade a total contract to a contingent one for
//! every existing caller, so it is a named [`UnitScope`] the caller must
//! consciously assert — a thing this crate structurally cannot verify, since
//! it has no view of the target module's registry.
//!
//! One corollary, easy to mistake for an oversight: **the unit hint is inert
//! for a compound dimension.** A hinted `mm^2` would fold `0.001.powi(2)`,
//! reintroducing exactly the fold-order matching hazard that emitting
//! factor-1.0 atoms structurally avoids (see
//! `reify_core::units::ri_compound_unit_expr`). A hint may change WHICH exact
//! literal is written for a bare dimension; it may never reach a compound one.
//!
//! # Why not `Display for Value`
//!
//! `Display` is a human-facing rendering and breaks the round-trip in three
//! separate ways, each of which this module exists to avoid:
//!
//! 1. Its `Scalar` arm emits `"{si_value} {dimension}"` — the SI base value,
//!    a space, and a dimension string. `0.08 m` is not the `80mm` the user
//!    wrote; and the space alone is fatal, because the `_unit_expr_start`
//!    scanner token is zero-width and refuses to fire across whitespace, so
//!    `80 mm` does not lex as a `quantity_literal` at all.
//! 2. It renders a whole `Real` with `{:.0}`, so `Real(80.0)` becomes `80` —
//!    which re-parses as `Value::Int(80)`. That is a silent *type* change: the
//!    lexer sets `is_real` from `text.contains('.'|'e'|'E')`, and
//!    `classify_number_literal` routes an `is_real == false` token with an
//!    exact-i64 value to `NumberClass::Int`.
//! 3. It emits a `String`'s contents unescaped and unchecked, so a quote, a
//!    backslash or a brace produces text that either fails to lex or (for
//!    `{`/`}`) diverts to `interpolated_string`, where `{expr}` is a hole.
//!
//! # Earned exactness
//!
//! Bit-exactness here is achieved BY CONFIGURATION, not assumed. The obvious
//! `magnitude = si_value / factor` does **not** round-trip: measured over
//! 200k uniform magnitudes per unit, it fails for 4252/200000 `mm` values,
//! 25905/200000 `cm`, 27707/200000 `in` and 18085/200000 `deg` — while
//! factor-1.0 units (`m`, `rad`, `kg`) fail 0/200000.
//!
//! So the serializer walks an ordered ladder of candidate symbols and accepts
//! a rung only when `magnitude * factor == si_value` holds **bit-identically**
//! — the same f64 factor from the same `unit_symbol_to_si` table, applied by
//! the same single multiply the compiler performs for a bare unit literal. An
//! acceptance is therefore a proof, not an estimate. The ladder's last rung
//! has factor exactly 1.0, so `mag * 1.0 == si_value` closes it by IEEE
//! identity and it can never exhaust for a finite input.
//!
//! Neither half is removable. Drop the multiply-back check and ~2% of `mm`
//! and ~9% of `deg` writes corrupt silently; drop the factor-1.0 terminator
//! and the ladder starts falling off its end.
//!
//! # Why the emission ladder is not `display_units::unit_ladders()`
//!
//! `reify_core::display_units` already ships ordered per-dimension unit
//! ladders, and unifying the two tables would look like an obvious cleanup.
//! It is not. That table answers "what may a human pick in the GUI unit
//! picker"; `ri_emittable_units` answers "what may be written into source and
//! read back unchanged". Concretely: the display labels include forms that
//! neither `unit_symbol_to_si` nor the lexer can handle (`mm²`, `cm³`, `L`,
//! `Pa`, `MPa`, `kg/m³`, `N`, `J`, `W`); its Length rungs end at `in`
//! (0.0254), not at a factor-1.0 terminator; and its coverage differs in both
//! directions. Emission order is load-bearing for correctness, display order
//! is picker ergonomics and is free to change — coupling them would let a
//! cosmetic reorder reintroduce lossy write-back. The two are cross-guarded
//! for physical agreement only, in reify-core's `units` tests.

use crate::Value;
use reify_core::DimensionVector;
use reify_core::units::{ri_compound_unit_expr, ri_emittable_units, unit_symbol_to_si};
use std::fmt;

/// Why a [`Value`] cannot be written as a `.ri` source literal.
///
/// Every rejection is structured rather than a best-effort string, because
/// the alternative — emitting something *close* — silently corrupts a user's
/// design file (PRD §7 B7).
#[derive(Debug, Clone, PartialEq)]
pub enum RiLiteralError {
    /// NaN or ±∞. The grammar has no `inf`/`nan` token, and
    /// `classify_number_literal` guards `value.is_finite()`.
    NonFiniteNumber,
    /// An `i64` outside the exactly-`f64`-representable range (±2^53). Every
    /// decimal `.ri` number literal is lowered through `f64::from_str`, so a
    /// larger integer would come back narrowed.
    IntNotRepresentable {
        /// The integer that could not round-trip.
        value: i64,
    },
    /// A string carrying a character `.ri` string literals cannot hold
    /// verbatim — `"`, `\`, `{`, `}`, or any control character.
    StringNotRepresentable {
        /// The first offending character.
        offending: char,
    },
    /// A dimension with no bare built-in unit symbol, so it would need a
    /// compound `UnitExpr` that only a seeded per-module registry can
    /// resolve.
    UnrepresentableDimension {
        /// The dimension that has no emittable bare symbol.
        dimension: DimensionVector,
    },
    /// The value is a kind that has no `.ri` literal form at all.
    UnsupportedValueKind {
        /// Stable discriminant name (e.g. `"List"`), safe to show a user.
        kind: &'static str,
    },
}

impl fmt::Display for RiLiteralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiLiteralError::NonFiniteNumber => {
                write!(f, "non-finite number has no .ri literal form")
            }
            RiLiteralError::IntNotRepresentable { value } => write!(
                f,
                "integer {value} is outside the exactly-f64-representable range (±2^53)"
            ),
            RiLiteralError::StringNotRepresentable { offending } => write!(
                f,
                "string contains {offending:?}, which a .ri string literal cannot carry verbatim"
            ),
            RiLiteralError::UnrepresentableDimension { dimension } => write!(
                f,
                "dimension {dimension} has no bare built-in unit symbol to emit"
            ),
            RiLiteralError::UnsupportedValueKind { kind } => {
                write!(f, "value kind `{kind}` has no .ri literal form")
            }
        }
    }
}

impl std::error::Error for RiLiteralError {}

/// Format an `f64` as the shortest decimal text that parses back to the same
/// bits, matching the lexer's own `f64::from_str`.
///
/// Rust's `{:?}` for floats is shortest-round-tripping and always contains a
/// `.` or an `e`, which is exactly the `is_real` predicate the lexer applies
/// (`text.contains('.'|'e'|'E')`). When `force_decimal_point` is false a
/// trailing `".0"` is stripped, so a scalar reads `80mm` rather than
/// `80.0mm` — safe because `lower_quantity_literal` discards `is_real`, and
/// it keeps the rewritten source idiomatic. `Value::Real` passes `true`,
/// because there the `is_real` bit is what separates `Real` from `Int`.
///
/// Always DECIMAL: the parser's `0x`/`0b` radix branches force
/// `is_real = false` and are unreachable from this emitter by construction.
fn format_f64_shortest(x: f64, force_decimal_point: bool) -> String {
    let s = format!("{x:?}");
    if force_decimal_point {
        return s;
    }
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// Which unit-resolution regime the TARGET module is asserted to provide
/// (task #6400).
///
/// The two arms are not a preference — they are genuinely different
/// preconditions, and the caller is the only party that can tell them apart.
/// `reify-compiler`'s `expr.rs` forks on the shape of the emitted `UnitExpr`:
/// a BARE unit resolves as `registry.lookup(..).or_else(|| unit_to_scalar(..))`
/// while a COMPOUND one goes to `resolve_unit_expr`, which consults the
/// per-module `UnitRegistry` and NOTHING else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitScope {
    /// Bare built-in symbols only — today's shipped contract, and the default
    /// every existing entry point delegates with.
    ///
    /// Round-trips in ANY module, including one that imports nothing and one
    /// compiled `#no_prelude`, because the built-in table is an unconditional
    /// `or_else` fallback behind the registry. Nothing about this arm is
    /// contingent on what the target file imports.
    BareBuiltinsOnly,
    /// The caller ASSERTS that the target module resolves the SI base symbols
    /// (`m`/`kg`/`s`/`K`/`rad`/`sr`/`USD`) to factor exactly `1.0`. Unlocks
    /// compound emission, and with it every dimension that has no bare symbol
    /// at all — Area, Volume, Force, Pressure, Density, Energy, SolidAngle,
    /// Money.
    ///
    /// True for any module compiled with the stdlib prelude (`compile_with_stdlib`
    /// seeds all stdlib modules unconditionally), and for one that imports
    /// `std.units` and `std.si_units`. NOT true for a `#no_prelude` module, for
    /// a `compile_project` module that imports neither, or inside a stdlib `fn`
    /// body — those load with `unit_registry == None`, where a compound literal
    /// does not compile at all.
    ///
    /// This crate CANNOT verify the assertion: it has no view of the target
    /// module's registry, and inverting the crate DAG to get one is not on the
    /// table. That unverifiability is precisely why this is a named opt-in
    /// rather than the default — the precondition sits in the type at the call
    /// site, where the caller must consciously make the claim.
    SiBaseUnitsSeeded,
}

/// Serialize a [`Value`] as `.ri` source text that re-parses to that same value.
///
/// Equivalent to [`value_to_ri_literal_with_unit`] with no unit preference.
///
/// Re-parsing an `Ok` result yields the same variant, dimension and f64 bits,
/// subject to the module-level doc's two qualifications: a *dimensionless*
/// `Value::Scalar` comes back as a `Value::Real` (same dimension, same bits,
/// different variant — an un-suffixed `.ri` number literal has no other
/// reading), and the bit-exactness holds unless the target module's
/// `UnitRegistry` shadows the emitted symbol with a different factor.
pub fn value_to_ri_literal(value: &Value) -> Result<String, RiLiteralError> {
    value_to_ri_literal_with_unit(value, None)
}

/// Serialize a [`Value`] as `.ri` source text, preferring `preferred_unit`
/// for a dimensioned scalar when — and only when — that unit is honourable.
///
/// The hint is the unit symbol the caller read off the literal being
/// replaced, so an edit to `width = 50mm` keeps writing millimetres instead
/// of hopping to whatever the canonical ladder prefers. It is deliberately a
/// plain `&str` resolved through [`unit_symbol_to_si`] rather than a span or
/// a registry handle: that keeps this module free of any reify-compiler
/// coupling, and independent of the span-resolution work in sibling task
/// #5094.
///
/// The hint is **advisory** — it is taken only when it resolves as a bare
/// built-in, its dimension matches the value's, and its magnitude is
/// bit-exact by the same test the ladder applies. Anything else (an unknown
/// symbol, a dimension mismatch, an affine unit like `degC` whose offset the
/// built-in table cannot represent, or a rung that would round) silently
/// falls back to the canonical ladder. A hint can therefore change *which*
/// exact literal is written, never *whether* the write is exact.
///
/// Every arm checks finiteness and representability BEFORE it formats
/// anything, so no partially-formed literal can escape on the error path.
pub fn value_to_ri_literal_with_unit(
    value: &Value,
    preferred_unit: Option<&str>,
) -> Result<String, RiLiteralError> {
    value_to_ri_literal_in_scope(value, preferred_unit, UnitScope::BareBuiltinsOnly)
}

/// Serialize a [`Value`] as `.ri` source text under an explicit
/// [`UnitScope`] (task #6400).
///
/// The widening entry point. [`value_to_ri_literal`] and
/// [`value_to_ri_literal_with_unit`] are exactly this function with
/// [`UnitScope::BareBuiltinsOnly`], so the shipped contract is untouched on
/// both the `Ok` and `Err` paths — pinned by `the_unhinted_entry_point_delegates`.
///
/// Under [`UnitScope::SiBaseUnitsSeeded`] a dimension with NO bare built-in
/// symbol is written as a compound unit expression built from SI base symbols
/// (`2.5m^2`, `101325kg/m/s^2`, `7850kg/m^3`) instead of being refused. See
/// the module doc's qualification 3 for the precondition that carries, and
/// `reify_core::units::ri_compound_unit_expr` for why bit-exactness on that
/// path is structural rather than measured.
///
/// The compound attempt is made in exactly one place — inside the
/// already-existing empty-bare-ladder branch — so a dimension that HAS a bare
/// ladder can never reach it. `80mm` stays `80mm`; `0.08m^1` is unreachable,
/// not merely unpreferred.
pub fn value_to_ri_literal_in_scope(
    value: &Value,
    preferred_unit: Option<&str>,
    scope: UnitScope,
) -> Result<String, RiLiteralError> {
    match value {
        Value::Bool(b) => Ok(if *b {
            "true".to_owned()
        } else {
            "false".to_owned()
        }),
        Value::Int(i) => {
            if !int_is_exactly_f64_representable(*i) {
                return Err(RiLiteralError::IntNotRepresentable { value: *i });
            }
            Ok(i.to_string())
        }
        Value::Real(r) => {
            if !r.is_finite() {
                return Err(RiLiteralError::NonFiniteNumber);
            }
            Ok(format_f64_shortest(*r, true))
        }
        Value::String(s) => {
            if let Some(offending) = first_unrepresentable_char(s) {
                return Err(RiLiteralError::StringNotRepresentable { offending });
            }
            Ok(format!("\"{s}\""))
        }
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if !si_value.is_finite() {
                return Err(RiLiteralError::NonFiniteNumber);
            }
            // A dimensionless scalar has no unit to write, so it goes out as a
            // bare real. That re-parses as `Value::Real`, whose `dimension()`
            // is `DIMENSIONLESS` — dimensionally identical, same bits, but a
            // DIFFERENT variant. This is the module doc's qualification 1, the
            // one documented exception to the "same variant" clause; it is
            // pinned by `ri_literal_roundtrip.rs`'s
            // `a_dimensionless_scalar_downgrades_to_real_with_identical_bits`.
            if dimension.is_dimensionless() {
                return Ok(format_f64_shortest(*si_value, true));
            }
            let ladder = ri_emittable_units(dimension);
            if ladder.is_empty() {
                // THE ONLY behavioural edit the compound widening makes, and it
                // sits inside this branch deliberately: containment is then
                // STRUCTURAL rather than incidental. A dimension with a
                // non-empty bare ladder cannot reach the compound path at all,
                // so the hint logic, the ladder walk and the trailing
                // "unreachable" `Err` below are byte-for-byte what task #5095
                // shipped, and no value that is `Ok` today can change its
                // literal.
                //
                // The magnitude is the SI value VERBATIM — no `exact_magnitude`
                // call and no ladder search on this path. Every atom
                // `ri_compound_unit_expr` writes has registry factor exactly
                // 1.0, so `resolve_unit_expr` folds the whole expression to
                // exactly 1.0 for ANY tree shape (`Unit → 1.0`;
                // `Pow: 1.0.powi(n) == 1.0`; `Mul: 1.0 * 1.0 == 1.0`;
                // `Div: 1.0 / 1.0 == 1.0`), and `expr.rs`'s single
                // `si_value = value * factor` multiply is the IEEE identity.
                // Bit-exactness here is therefore INDEPENDENT of the compiler's
                // fold order rather than a mirror of it that could drift out of
                // step with a future re-association.
                //
                // `preferred_unit` is deliberately not consulted: a hinted
                // `mm^2` would fold `0.001.powi(2)` and reintroduce exactly the
                // fold-order hazard this avoids.
                //
                // The finiteness guard above has already run, so a non-finite
                // compound value returns `NonFiniteNumber` before anything here
                // formats.
                if scope == UnitScope::SiBaseUnitsSeeded
                    && let Some(unit_expr) = ri_compound_unit_expr(dimension)
                {
                    return Ok(format!(
                        "{}{unit_expr}",
                        format_f64_shortest(*si_value, false)
                    ));
                }
                return Err(RiLiteralError::UnrepresentableDimension {
                    dimension: *dimension,
                });
            }
            // Honour the caller's unit first, but only on the ladder's own
            // terms: same dimension, and bit-exact. Note this is checked
            // AFTER the empty-ladder rejection, so a hint can never smuggle a
            // symbol onto a dimension the canonical table refuses to emit.
            if let Some(hint) = preferred_unit
                && let Some((_, hint_dim)) = unit_symbol_to_si(hint)
                && hint_dim == *dimension
                && let Some(magnitude) = exact_magnitude(*si_value, hint)
            {
                return Ok(format!("{}{hint}", format_f64_shortest(magnitude, false)));
            }
            for sym in ladder {
                if let Some(magnitude) = exact_magnitude(*si_value, sym) {
                    return Ok(format!("{}{sym}", format_f64_shortest(magnitude, false)));
                }
            }
            // Unreachable for a finite `si_value` on a non-empty ladder: the
            // last rung's factor is exactly 1.0 (pinned by reify-core's
            // `ri_emittable_ladders_resolve_to_their_dimension_and_end_at_factor_one`),
            // so `mag * 1.0 == si_value` holds by IEEE identity. Kept as a
            // structured error rather than an `unreachable!()` so a future
            // table edit degrades to a refusal, never to a lossy write.
            Err(RiLiteralError::UnrepresentableDimension {
                dimension: *dimension,
            })
        }
        other => Err(RiLiteralError::UnsupportedValueKind {
            kind: value_kind_name(other),
        }),
    }
}

/// Whether an `i64` survives the `i64 → f64 → i64` trip the parser forces.
///
/// The magnitude clause is load-bearing on its own: `i as f64 as i64 == i`
/// is *true* for `i64::MIN`, because the saturating `as` cast lands back on
/// `i64::MIN` — the equality alone would wave through a value the parser
/// cannot reproduce.
fn int_is_exactly_f64_representable(i: i64) -> bool {
    let as_f64 = i as f64;
    as_f64 as i64 == i && as_f64.abs() <= (2f64).powi(53)
}

/// The first character a `.ri` string literal cannot carry verbatim.
///
/// `lower_string_literal` strips the outer quotes and performs NO escape
/// decoding, so `"` and `\` cannot appear at all, and neither can a control
/// character (a raw newline/tab would not even lex). `{` and `}` are excluded
/// separately: they fail the `string_literal` token and divert the text to
/// `interpolated_string`, which DOES decode escapes and treats `{expr}` as a
/// hole — a brace-bearing string could therefore evaluate source.
fn first_unrepresentable_char(s: &str) -> Option<char> {
    s.chars()
        .find(|c| matches!(c, '"' | '\\' | '{' | '}') || c.is_control())
}

/// Stable discriminant name for a value kind with no `.ri` literal form.
///
/// Deliberately a fixed `&'static str` per variant rather than `{value:?}`:
/// a `SampledField` or `Matrix` payload could be enormous, and this string
/// ends up in a user-facing MCP error.
///
/// **EXHAUSTIVE BY CONSTRUCTION — do not add a `_` arm.** A catch-all here is
/// not a tidiness question: it collapses to a single useless name exactly the
/// values an agent is most likely to try to edit (a `Direction`, a `Frame`, a
/// `Range`), and it lets a newly added [`Value`] variant degrade silently
/// instead of failing to compile. Listing every variant makes the compiler the
/// guard.
///
/// The `Bool`/`Int`/`Real`/`String`/`Scalar` arms are unreachable from the one
/// caller — [`value_to_ri_literal_with_unit`] handles those variants before it
/// reaches its catch-all — but they are required for exhaustiveness, and they
/// keep this a total `Value → kind name` function rather than a
/// caller-specific residue.
///
/// This duplicates `reify_constraints::value_kind_label`'s variant list
/// (`crates/reify-constraints/src/lib.rs`). The non-duplicating form is a
/// `Value::kind_name()` inherent method both delegate to (the shape
/// `Value::format_hover()` already uses), which lives in
/// `crates/reify-ir/src/value.rs` — outside task #5095's locked scope, so it is
/// deferred to **task #6466**, which holds all three files.
///
/// What the deferral actually costs, stated precisely: both matches are
/// exhaustive with no `_` arm, so a NEW `Value` variant breaks both to compile
/// and cannot drift silently. What CAN drift is a *name* — `value_kind_label`
/// enriches two arms (`Scalar<{dimension}>`, `Enum<{type_name}>`) where this one
/// says plain `Scalar`/`Enum`, and a rename on either side is invisible to the
/// compiler. That is tolerable here because the two strings feed different
/// surfaces (constraint diagnostics vs. this serializer's MCP rejection text)
/// and nothing compares them; it is not tolerable indefinitely, hence #6466.
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Real(_) => "Real",
        Value::String(_) => "String",
        Value::Scalar { .. } => "Scalar",
        Value::Enum { .. } => "Enum",
        Value::List(_) => "List",
        Value::Set(_) => "Set",
        Value::Map(_) => "Map",
        Value::Option(_) => "Option",
        Value::Field { .. } => "Field",
        Value::Lambda { .. } => "Lambda",
        Value::Tensor(_) => "Tensor",
        Value::Point(_) => "Point",
        Value::Vector(_) => "Vector",
        Value::Complex { .. } => "Complex",
        Value::Orientation { .. } => "Orientation",
        Value::Frame { .. } => "Frame",
        Value::Transform { .. } => "Transform",
        Value::Plane { .. } => "Plane",
        Value::Axis { .. } => "Axis",
        Value::Direction { .. } => "Direction",
        Value::BoundingBox { .. } => "BoundingBox",
        Value::Range { .. } => "Range",
        Value::Matrix(_) => "Matrix",
        Value::SampledField(_) => "SampledField",
        Value::StructureInstance(_) => "StructureInstance",
        Value::GeometryHandle { .. } => "GeometryHandle",
        Value::AffineMap { .. } => "AffineMap",
        Value::Selector(_) => "Selector",
        Value::Feature(_) => "Feature",
        Value::Undef => "Undef",
    }
}

/// The magnitude that would be written in front of `unit`, but ONLY when it
/// recovers `si_value` bit-identically.
///
/// This reproduces the compiler's own arithmetic rather than approximating it:
/// the same `f64` factor from the same [`unit_symbol_to_si`] table, applied by
/// the same single multiply the compiler performs for a bare unit literal
/// (`unit_to_scalar` delegates to that table). So a `Some` here is a *proof*
/// that the literal re-parses to the original bits, not an estimate.
///
/// The proof is against the BUILT-IN table, which the compiler consults only
/// as the `or_else` fallback behind the target module's `UnitRegistry` — see
/// the module doc's qualification 2 for the precondition that carries.
///
/// Naive `si_value / factor` is emphatically NOT enough — measured over 200k
/// uniform magnitudes per unit, it fails to round-trip for ~2.1% of `mm`,
/// ~13% of `cm`, ~14% of `in` and ~9% of `deg` values, while factor-1.0 units
/// (`m`, `rad`, `kg`) never fail.
fn exact_magnitude(si_value: f64, unit: &str) -> Option<f64> {
    let (factor, _dim) = unit_symbol_to_si(unit)?;
    let magnitude = si_value / factor;
    (magnitude * factor == si_value).then_some(magnitude)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    // ─── Caller-supplied unit hint (PRD §12 Q2) ──────────────────────────────
    //
    // γ reads the unit off the existing default literal and passes it here, so
    // rewriting `width = 50mm` to 60mm keeps writing `mm` rather than jumping
    // to whatever the canonical ladder prefers. The hint is ADVISORY: it is
    // taken only when it resolves, matches the dimension, AND is bit-exact.
    // It is a plain `&str` resolved through the built-in table — no span, no
    // registry handle — which is what keeps this independent of sibling α.

    fn lit_with(v: &Value, unit: Option<&str>) -> String {
        value_to_ri_literal_with_unit(v, unit)
            .unwrap_or_else(|e| panic!("expected Ok for {v:?} + {unit:?}, got {e:?}"))
    }

    #[test]
    fn an_exact_hint_overrides_the_canonical_ladder() {
        // Canonical would be `50mm` — the hint keeps the edit in centimetres.
        assert_eq!(lit_with(&Value::length(0.05), Some("cm")), "5cm");
        // `in` is not on the canonical ladder at all; canonical is `25.4mm`.
        assert_eq!(lit_with(&Value::length(0.0254), Some("in")), "1in");
        // Canonical would be `90deg`.
        assert_eq!(
            lit_with(&Value::angle(std::f64::consts::FRAC_PI_2), Some("rad")),
            "1.5707963267948966rad"
        );
    }

    /// A hint the built-in table cannot honour is never fatal — it falls back
    /// to the canonical ladder and still returns `Ok`.
    ///
    /// `degC` stands in for the affine units: those live only in the
    /// compiler's registry and carry an offset `unit_symbol_to_si` has no way
    /// to represent, so they can never be taken as a hint here.
    #[test]
    fn an_unusable_hint_is_advisory_and_falls_back() {
        for hint in [
            Some("furlong"), // unresolvable
            Some("deg"),     // resolvable, but wrong dimension for a Length
            Some("degC"),    // affine — registry-only, offset-bearing
            Some(""),        // degenerate
            None,
        ] {
            assert_eq!(
                lit_with(&Value::length(0.08), hint),
                "80mm",
                "hint {hint:?} must fall back to the canonical ladder"
            );
        }
    }

    /// A hint is never allowed to make the write LOSSY. The `mm` rung is
    /// bit-inexact for this magnitude, so the hint is dropped and the ladder
    /// picks `cm` — the same answer the unhinted call gives.
    #[test]
    fn a_bit_inexact_hint_is_refused() {
        let v = Value::length(-0.5566166674539299);
        assert_eq!(lit_with(&v, Some("mm")), "-55.66166674539299cm");
    }

    #[test]
    fn the_hint_is_ignored_for_non_scalar_values() {
        assert_eq!(lit_with(&Value::Int(7), Some("mm")), "7");
        assert_eq!(lit_with(&Value::Real(1.5), Some("mm")), "1.5");
        assert_eq!(lit_with(&Value::Bool(true), Some("mm")), "true");
        assert_eq!(
            lit_with(&Value::String("PLA".into()), Some("mm")),
            "\"PLA\""
        );
        assert_eq!(
            lit_with(
                &Value::Scalar {
                    si_value: 2.5,
                    dimension: DimensionVector::DIMENSIONLESS,
                },
                Some("mm")
            ),
            "2.5"
        );
    }

    /// `value_to_ri_literal(v)` must be exactly `..._with_unit(v, None)`,
    /// including on the error paths.
    ///
    /// Extended by task #6400 with the third layer: BOTH shipped entry points
    /// must be exactly `value_to_ri_literal_in_scope(.., BareBuiltinsOnly)`.
    /// That is the CONTAINMENT assertion for the compound widening — every
    /// value that is `Ok` today keeps its identical literal, and every value
    /// that is `Err` today stays `Err`. Asserted over the one shared case list
    /// rather than a duplicated one, so a case added for either purpose is
    /// covered by both.
    #[test]
    fn the_unhinted_entry_point_delegates() {
        let cases = [
            Value::Bool(false),
            Value::Int(-42),
            Value::Real(80.0),
            Value::String("M3x0.5".into()),
            Value::length(0.08),
            Value::length(-0.5566166674539299),
            Value::angle(std::f64::consts::FRAC_PI_2),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::PRESSURE,
            },
            // Compound dimensions the widening unlocks under the OTHER scope;
            // here they must still delegate to a refusal.
            Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::AREA,
            },
            Value::Scalar {
                si_value: 7850.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
            Value::Real(f64::NAN),
            Value::Undef,
        ];
        for v in cases {
            assert_eq!(
                value_to_ri_literal(&v),
                value_to_ri_literal_with_unit(&v, None),
                "delegation mismatch for {v:?}"
            );
            assert_eq!(
                value_to_ri_literal(&v),
                value_to_ri_literal_in_scope(&v, None, UnitScope::BareBuiltinsOnly),
                "value_to_ri_literal must be exactly ..._in_scope(.., BareBuiltinsOnly) \
                 for {v:?}"
            );
            for hint in [None, Some("cm"), Some("furlong")] {
                assert_eq!(
                    value_to_ri_literal_with_unit(&v, hint),
                    value_to_ri_literal_in_scope(&v, hint, UnitScope::BareBuiltinsOnly),
                    "hinted delegation mismatch for {v:?} + {hint:?}"
                );
            }
        }
    }

    // ─── Structured rejection contract (PRD §7 B7) ───────────────────────────
    //
    // The serializer must never emit a lossy or unparseable literal. Every
    // value it cannot write back EXACTLY is a structured `Err` a caller can
    // render, never a best-effort string.

    fn err(v: &Value) -> RiLiteralError {
        value_to_ri_literal(v).expect_err(&format!("expected Err for {v:?}"))
    }

    /// The grammar has no `inf`/`nan` token, and `classify_number_literal`
    /// explicitly guards `value.is_finite()`.
    #[test]
    fn non_finite_numbers_are_rejected() {
        for v in [
            Value::Real(f64::NAN),
            Value::Real(f64::INFINITY),
            Value::Real(f64::NEG_INFINITY),
            Value::length(f64::NAN),
            Value::length(f64::INFINITY),
        ] {
            assert!(
                matches!(err(&v), RiLiteralError::NonFiniteNumber),
                "{v:?} must be rejected as non-finite"
            );
        }
    }

    /// Every DECIMAL `.ri` number literal is lowered through `f64::from_str`,
    /// and `parse_number_literal_text`'s own doc-comment states that values
    /// beyond 2^53 are stored as a lossy `(n as f64)`. So an `i64` outside the
    /// exactly-f64-representable range cannot round-trip and must be rejected
    /// rather than silently narrowed.
    #[test]
    fn ints_outside_the_exact_f64_range_are_rejected() {
        for i in [i64::MAX, i64::MIN, (1i64 << 53) + 1, -((1i64 << 53) + 1)] {
            assert!(
                matches!(
                    err(&Value::Int(i)),
                    RiLiteralError::IntNotRepresentable { .. }
                ),
                "Int({i}) must be rejected as not exactly f64-representable"
            );
        }
    }

    #[test]
    fn ints_inside_the_exact_f64_range_are_accepted() {
        assert_eq!(lit(&Value::Int(1i64 << 53)), "9007199254740992");
        assert_eq!(lit(&Value::Int(-(1i64 << 53))), "-9007199254740992");
        assert_eq!(lit(&Value::Int(9007199254740991)), "9007199254740991");
    }

    /// `lower_string_literal` strips the outer quotes and performs NO escape
    /// decoding, so an emitted `\"` comes back as a literal backslash-quote.
    /// Separately, a bare `{`/`}` makes the `string_literal` token fail and
    /// diverts the text to `interpolated_string`, which DOES decode escapes
    /// and treats `{expr}` as a hole — so a brace could evaluate source.
    #[test]
    fn strings_outside_the_verbatim_safe_charset_are_rejected() {
        for s in [
            "say \"hi\"",
            "back\\slash",
            "hole {x}",
            "open {",
            "close }",
            "two\nlines",
            "tab\there",
            "bell\u{0007}",
            "nul\u{0000}",
        ] {
            let v = Value::String(s.to_owned());
            assert!(
                matches!(err(&v), RiLiteralError::StringNotRepresentable { .. }),
                "{s:?} must be rejected as not verbatim-safe"
            );
        }
    }

    #[test]
    fn verbatim_safe_strings_are_accepted() {
        assert_eq!(lit(&Value::String("PLA".into())), "\"PLA\"");
        assert_eq!(lit(&Value::String("M3x0.5".into())), "\"M3x0.5\"");
        assert_eq!(lit(&Value::String(String::new())), "\"\"");
        assert_eq!(lit(&Value::String("naïve—Ω".into())), "\"naïve—Ω\"");
    }

    /// Dimensions with no bare built-in symbol are rejected outright. Several
    /// of these DO have `display_units` ladders — the GUI can still *show* a
    /// pressure — which is the clearest demonstration that the display table
    /// and the emission table answer different questions.
    #[test]
    fn dimensions_needing_a_compound_unit_are_rejected() {
        for dim in [
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::PRESSURE,
            DimensionVector::FORCE,
            DimensionVector::SOLID_ANGLE,
            DimensionVector::MONEY,
        ] {
            let v = Value::Scalar {
                si_value: 1.5,
                dimension: dim,
            };
            assert!(
                matches!(err(&v), RiLiteralError::UnrepresentableDimension { .. }),
                "{:?} must be rejected — no bare built-in symbol",
                dim.canonical_name()
            );
        }
    }

    /// The catch-all must name the kind so γ can render a useful MCP error,
    /// and must NOT embed a `{value:?}` dump (a `SampledField` payload could
    /// be enormous).
    ///
    /// `Complex`, `Direction`, `Range` and `Orientation` are here deliberately:
    /// they are the variants an earlier `_ => "Unsupported"` catch-all
    /// collapsed into one uninformative name, and a datum-ish param is exactly
    /// what an agent is most likely to try to edit. Covering only the variants
    /// a catch-all happens to list gives false confidence.
    #[test]
    fn unsupported_value_kinds_are_rejected_with_a_stable_kind_name() {
        let cases = [
            (Value::Undef, "Undef"),
            (Value::List(vec![]), "List"),
            (Value::Point(vec![]), "Point"),
            (Value::Option(None), "Option"),
            (Value::enum_unit("Fit", "Loose"), "Enum"),
            (
                Value::Complex {
                    re: 1.0,
                    im: -2.0,
                    dimension: DimensionVector::DIMENSIONLESS,
                },
                "Complex",
            ),
            (
                Value::Direction {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                "Direction",
            ),
            (
                Value::Range {
                    lower: Some(Box::new(Value::Int(0))),
                    upper: Some(Box::new(Value::Int(10))),
                    lower_inclusive: true,
                    upper_inclusive: false,
                },
                "Range",
            ),
            (
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                "Orientation",
            ),
        ];
        for (v, expected) in cases {
            match err(&v) {
                RiLiteralError::UnsupportedValueKind { kind } => {
                    assert_eq!(kind, expected, "kind name for {v:?}")
                }
                other => panic!("expected UnsupportedValueKind for {v:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn error_is_a_standard_error() {
        fn assert_error<E: std::error::Error>(e: &E) -> String {
            e.to_string()
        }
        let rendered = assert_error(&err(&Value::Real(f64::NAN)));
        assert!(
            !rendered.is_empty(),
            "Display for RiLiteralError must render something"
        );
    }

    /// A `Value` whose `{:?}` dump would be enormous — a 1001-point sampled
    /// field. Used to make the no-payload property below *witnessable*.
    fn enormous_sampled_field() -> Value {
        let n = 1001;
        Value::SampledField(crate::SampledField {
            name: "temperature_grid".to_owned(),
            kind: crate::SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![0.001],
            axis_grids: vec![(0..n).map(|i| f64::from(i) / 1000.0).collect()],
            interpolation: crate::InterpolationKind::Linear,
            data: (0..n).map(|i| f64::from(i) * 1.5).collect(),
            oob_emitted: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Every rejection renders as its EXACT expected sentence.
    ///
    /// This replaces an earlier `text.len() < 200` assertion whose stated
    /// purpose was to catch a `{value:?}` payload dump. That assertion could
    /// not do the job in either direction: `UnsupportedValueKind` carries only
    /// a `&'static str` kind name, so the payload is structurally unreachable
    /// from `Display` and the test would have had to be rewritten before it
    /// could ever fire — while the magic threshold *would* have fired
    /// spuriously on a legitimate rewording.
    ///
    /// Pinning the exact string does the same job positively and cannot drift
    /// on length: `SampledField` and `Matrix` are here with deliberately
    /// enormous payloads (1001 samples, 4096 cells), so an implementation that
    /// interpolated `{value:?}` fails on the *content*, not on a byte count.
    /// They are also the two variants the old loop omitted entirely.
    ///
    /// The dimension case pins the message TEMPLATE and delegates the
    /// dimension's own rendering to `DimensionVector`'s `Display`, which is
    /// `dimension.rs`'s contract to change, not this module's.
    #[test]
    fn error_display_names_the_kind_and_carries_no_payload() {
        let cases = [
            (
                Value::Real(f64::NAN),
                "non-finite number has no .ri literal form".to_owned(),
            ),
            (
                Value::Int(i64::MAX),
                "integer 9223372036854775807 is outside the exactly-f64-representable range (±2^53)"
                    .to_owned(),
            ),
            (
                Value::String("{".into()),
                "string contains '{', which a .ri string literal cannot carry verbatim".to_owned(),
            ),
            (
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::PRESSURE,
                },
                format!(
                    "dimension {} has no bare built-in unit symbol to emit",
                    DimensionVector::PRESSURE
                ),
            ),
            (
                enormous_sampled_field(),
                "value kind `SampledField` has no .ri literal form".to_owned(),
            ),
            (
                Value::Matrix(vec![vec![Value::Real(1.5); 64]; 64]),
                "value kind `Matrix` has no .ri literal form".to_owned(),
            ),
            (
                Value::Undef,
                "value kind `Undef` has no .ri literal form".to_owned(),
            ),
        ];
        for (v, expected) in cases {
            assert_eq!(
                err(&v).to_string(),
                expected,
                "rejection text drifted for a {} value",
                value_kind_name(&v)
            );
        }
    }

    fn lit(v: &Value) -> String {
        value_to_ri_literal(v).unwrap_or_else(|e| panic!("expected Ok for {v:?}, got {e:?}"))
    }

    #[test]
    fn bool_literals_round_trip_as_keywords() {
        assert_eq!(lit(&Value::Bool(true)), "true");
        assert_eq!(lit(&Value::Bool(false)), "false");
    }

    #[test]
    fn int_literals_are_bare_decimal() {
        assert_eq!(lit(&Value::Int(-42)), "-42");
        assert_eq!(lit(&Value::Int(0)), "0");
        assert_eq!(lit(&Value::Int(7)), "7");
    }

    /// A whole `Real` MUST keep its `.0`.
    ///
    /// The lexer classifies a number literal as real by
    /// `is_real = text.contains('.') || text.contains('e') || text.contains('E')`
    /// (`ts_parser.rs` `parse_number_literal_text`), and
    /// `classify_number_literal` (`reify-ast/src/decl.rs`) then routes an
    /// `is_real == false` token whose value is an exact `i64` to
    /// `NumberClass::Int`. So a bare `80` would come back as `Value::Int(80)`
    /// — a silent type change, not a rounding wobble. `Value`'s own `Display`
    /// makes exactly this mistake (`{:.0}` for whole reals).
    #[test]
    fn whole_real_keeps_its_decimal_point() {
        assert_eq!(lit(&Value::Real(80.0)), "80.0");
        assert_eq!(lit(&Value::Real(0.0)), "0.0");
        assert_eq!(lit(&Value::Real(-3.0)), "-3.0");
    }

    #[test]
    fn fractional_real_uses_shortest_round_tripping_form() {
        assert_eq!(lit(&Value::Real(1.5)), "1.5");
        assert_eq!(lit(&Value::Real(-0.25)), "-0.25");
    }

    #[test]
    fn string_is_quoted() {
        assert_eq!(lit(&Value::String("PLA".into())), "\"PLA\"");
        assert_eq!(lit(&Value::String(String::new())), "\"\"");
    }

    /// The PRD's headline G3 case: a length whose SI value is `0.08` must be
    /// written back as the literal a human would have typed.
    #[test]
    fn length_emits_a_contiguous_quantity_literal() {
        assert_eq!(lit(&Value::length(0.08)), "80mm");
        assert_eq!(lit(&Value::length(0.0)), "0mm");
        assert_eq!(lit(&Value::length(-0.08)), "-80mm");
    }

    #[test]
    fn angle_prefers_degrees() {
        assert_eq!(lit(&Value::angle(std::f64::consts::FRAC_PI_2)), "90deg");
    }

    #[test]
    fn dimensionless_scalar_emits_a_bare_number() {
        assert_eq!(
            lit(&Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::DIMENSIONLESS,
            }),
            "2.5"
        );
    }

    /// LADDER-FALLBACK REGRESSION — the earned-exactness witness.
    ///
    /// `-0.5566166674539299 / 0.001` then re-multiplied by `0.001` does NOT
    /// return the original f64, so the `mm` rung must be REJECTED; the `cm`
    /// rung is bit-exact and is what gets emitted. A naive `si_value / factor`
    /// implementation emits a lossy `mm` form and fails here.
    #[test]
    fn ladder_falls_through_a_bit_inexact_rung() {
        let si = -0.5566166674539299_f64;
        // Pin the premise itself, so a future reader can see the fallback is
        // exercised for a real arithmetic reason and not by coincidence.
        assert_ne!((si / 0.001) * 0.001, si, "mm rung must be inexact here");
        assert_eq!((si / 0.01) * 0.01, si, "cm rung must be exact here");

        assert_eq!(lit(&Value::length(si)), "-55.66166674539299cm");
    }

    /// CONTIGUITY — a quantity literal may not contain whitespace.
    ///
    /// `_unit_expr_start` in `tree-sitter-reify/src/scanner.c` is a zero-width
    /// external token that refuses to fire across whitespace, so `80 mm` does
    /// not parse as a `quantity_literal` at all. `impl Display for Value`
    /// emits `"{si_value} {dimension}"` — SI base plus a space — which is
    /// precisely the gap this serializer exists to close.
    #[test]
    fn emitted_scalars_never_contain_a_space() {
        let cases = [
            Value::length(0.08),
            Value::length(-0.5566166674539299),
            Value::angle(std::f64::consts::FRAC_PI_2),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ];
        for v in cases {
            let s = lit(&v);
            assert!(
                !s.contains(' '),
                "emitted literal {s:?} for {v:?} contains a space; \
                 the scanner will not fire _unit_expr_start across whitespace"
            );
        }
    }

    // ─── The opt-in COMPOUND regime (task #6400) ─────────────────────────────
    //
    // `UnitScope::SiBaseUnitsSeeded` is the caller's assertion that the target
    // module resolves the SI base symbols to factor exactly 1.0 — true for any
    // module compiled with the stdlib prelude. It is a named opt-in rather than
    // a silent widening because its precondition is STRICTLY STRONGER than the
    // bare ladder's: a bare unit resolves through
    // `registry.lookup(..).or_else(|| unit_to_scalar(..))`, so the built-in
    // table is an unconditional fallback, whereas a compound `UnitExpr` goes to
    // `resolve_unit_expr`, which is registry-ONLY. Nothing in this crate can
    // observe the target module's registry, which is exactly why the caller
    // must say so.

    fn seeded(v: &Value) -> String {
        value_to_ri_literal_in_scope(v, None, UnitScope::SiBaseUnitsSeeded)
            .unwrap_or_else(|e| panic!("expected Ok for {v:?} under SiBaseUnitsSeeded, got {e:?}"))
    }

    fn compound(si_value: f64, dimension: DimensionVector) -> Value {
        Value::Scalar {
            si_value,
            dimension,
        }
    }

    /// (a) UNLOCK — dimensions that are `Err` today emit
    /// magnitude-then-compound, with the magnitude being the SI value VERBATIM.
    ///
    /// No ladder scaling occurs on this path and none may: every emitted atom
    /// resolves to factor exactly 1.0, so the compiler's fold is exactly 1.0
    /// for ANY tree shape and its single `si_value = value * factor` multiply
    /// is the IEEE identity. `ri_literal_roundtrip.rs` re-parses each of these
    /// through the real parser and asserts identical f64 BITS.
    #[test]
    fn compound_dimensions_emit_the_si_value_verbatim_when_the_scope_allows_it() {
        let cases: &[(Value, &str)] = &[
            (compound(2.5, DimensionVector::AREA), "2.5m^2"),
            (compound(1e-6, DimensionVector::VOLUME), "1e-6m^3"),
            (compound(101325.0, DimensionVector::PRESSURE), "101325kg/m/s^2"),
            (compound(-9.81, DimensionVector::FORCE), "-9.81m*kg/s^2"),
            (compound(7850.0, DimensionVector::MASS_DENSITY), "7850kg/m^3"),
            (compound(1.5, DimensionVector::SOLID_ANGLE), "1.5sr"),
            (compound(19.99, DimensionVector::MONEY), "19.99USD"),
        ];
        for (v, expected) in cases {
            assert_eq!(&seeded(v), expected, "compound emission drifted for {v:?}");
        }
    }

    /// (a2) The awkward magnitudes, where "verbatim" earns its keep.
    ///
    /// `-0.0` must keep its SIGN BIT — the `.0`-strip in
    /// `format_f64_shortest(x, false)` turns `"-0.0"` into `"-0"`, not `"0"` —
    /// and a value with a long decimal form must be written in full, since a
    /// rounded one is precisely the silent corruption this module exists to
    /// prevent.
    #[test]
    fn compound_magnitudes_are_the_shortest_round_tripping_form() {
        assert_eq!(seeded(&compound(-0.0, DimensionVector::AREA)), "-0m^2");

        for si in [0.1 + 0.2, 1e-9, f64::MIN_POSITIVE / 2.0, 1.7976931348623157e308] {
            let emitted = seeded(&compound(si, DimensionVector::AREA));
            let magnitude = emitted
                .strip_suffix("m^2")
                .unwrap_or_else(|| panic!("{emitted:?} must end in the unit expression"));
            let expected = format!("{si:?}");
            let expected = expected.strip_suffix(".0").unwrap_or(&expected);
            assert_eq!(
                magnitude, expected,
                "magnitude for {si:?} must be the SI value verbatim, not a scaled \
                 or rounded form"
            );
            assert_eq!(
                magnitude.parse::<f64>().map(f64::to_bits),
                Ok(si.to_bits()),
                "emitted magnitude {magnitude:?} must re-parse to identical bits"
            );
        }
    }

    /// (b) CONTAINMENT — under `BareBuiltinsOnly` every value the other scope
    /// unlocks is still the structured refusal it is today. The shipped
    /// contract is untouched; `dimensions_needing_a_compound_unit_are_rejected`
    /// stays green and unmodified.
    #[test]
    fn the_bare_scope_still_refuses_every_compound_dimension() {
        for dim in [
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::PRESSURE,
            DimensionVector::FORCE,
            DimensionVector::MASS_DENSITY,
            DimensionVector::SOLID_ANGLE,
            DimensionVector::MONEY,
        ] {
            let v = compound(2.5, dim);
            assert!(
                seeded(&v).len() > 1,
                "{:?} must be emittable under SiBaseUnitsSeeded — otherwise this \
                 test is vacuous",
                dim.canonical_name()
            );
            assert!(
                matches!(
                    value_to_ri_literal_in_scope(&v, None, UnitScope::BareBuiltinsOnly),
                    Err(RiLiteralError::UnrepresentableDimension { .. })
                ),
                "{:?} must stay a structured refusal under BareBuiltinsOnly",
                dim.canonical_name()
            );
        }
    }

    /// (b2) The bare ladder ALWAYS wins, so widening cannot silently re-spell
    /// an existing param.
    ///
    /// This is structural rather than incidental: compound is reached only from
    /// inside the existing `if ladder.is_empty()` branch, so a dimension with a
    /// non-empty bare ladder can never take the compound path. `0.08m^1` is not
    /// merely unpreferred here — it is unreachable.
    #[test]
    fn a_dimension_with_a_bare_ladder_emits_identically_under_both_scopes() {
        let cases: &[(Value, &str)] = &[
            (Value::length(0.08), "80mm"),
            (Value::angle(std::f64::consts::FRAC_PI_2), "90deg"),
            (compound(2.75, DimensionVector::MASS), "2.75kg"),
            (compound(1.5, DimensionVector::TIME), "1.5s"),
            (compound(293.15, DimensionVector::TEMPERATURE), "293.15K"),
        ];
        for (v, expected) in cases {
            assert_eq!(&lit(v), expected, "shipped literal drifted for {v:?}");
            assert_eq!(
                &seeded(v),
                expected,
                "{v:?} must emit the IDENTICAL bare literal under \
                 SiBaseUnitsSeeded — the bare ladder always wins"
            );
        }
    }

    /// (c) REJECTIONS THAT SURVIVE THE UNLOCK. Each is a case where emitting
    /// something would produce a literal that does not re-parse.
    #[test]
    fn the_unlock_does_not_widen_to_what_still_cannot_be_written() {
        // Current-bearing dimensions: bare `A` is never in the registry.
        for dim in [DimensionVector::VOLTAGE, DimensionVector::CHARGE] {
            assert!(
                matches!(
                    value_to_ri_literal_in_scope(
                        &compound(1.5, dim),
                        None,
                        UnitScope::SiBaseUnitsSeeded
                    ),
                    Err(RiLiteralError::UnrepresentableDimension { .. })
                ),
                "{:?} names a base symbol the registry never carries",
                dim.canonical_name()
            );
        }

        // A fractional exponent has no integral `UnitExpr::Pow` spelling.
        assert!(matches!(
            value_to_ri_literal_in_scope(
                &compound(1.5, DimensionVector::LENGTH.root(2)),
                None,
                UnitScope::SiBaseUnitsSeeded
            ),
            Err(RiLiteralError::UnrepresentableDimension { .. })
        ));

        // Finiteness is checked BEFORE any formatting, so a non-finite
        // compound value is `NonFiniteNumber` — never a partially-formed
        // `NaNm^2`.
        for si in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                value_to_ri_literal_in_scope(
                    &compound(si, DimensionVector::AREA),
                    None,
                    UnitScope::SiBaseUnitsSeeded
                ),
                Err(RiLiteralError::NonFiniteNumber),
                "non-finite {si:?} on a compound dimension must be refused before \
                 anything is formatted"
            );
        }

        // Non-Scalar kinds are untouched by the scope entirely.
        for v in [Value::Undef, Value::List(vec![])] {
            assert!(matches!(
                value_to_ri_literal_in_scope(&v, None, UnitScope::SiBaseUnitsSeeded),
                Err(RiLiteralError::UnsupportedValueKind { .. })
            ));
        }
    }

    /// (d) THE HINT IS INERT FOR A COMPOUND DIMENSION.
    ///
    /// A hint may change WHICH exact literal is written for a bare dimension;
    /// it may never reach a compound one. Honouring `mm` on an Area would mean
    /// emitting `mm^2`, whose factor folds as `0.001.powi(2)` — reintroducing
    /// exactly the fold-order matching hazard that emitting factor-1.0 atoms
    /// structurally avoids.
    #[test]
    fn a_hint_never_reaches_a_compound_dimension() {
        for hint in [Some("mm"), Some("cm"), Some("m"), Some("kg"), Some("furlong")] {
            assert_eq!(
                value_to_ri_literal_in_scope(
                    &compound(2.5, DimensionVector::AREA),
                    hint,
                    UnitScope::SiBaseUnitsSeeded
                ),
                Ok("2.5m^2".to_owned()),
                "hint {hint:?} must not reach the compound path"
            );
        }
    }

}
