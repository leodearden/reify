# Numeric & Range Literal Forms

**Status:** deferred (spec-gap-filling batch `spec-gap-2026-05-27`) · **Milestone:** v0.6 · **Authored:** 2026-05-27
**Approach:** bare B (vertical slices). Self-contained; no contract section. See §G5 note.

## 1. Goal

Close three lexer/grammar gaps between `docs/reify-language-spec.md` §2.3 / §2.8 and the
real `tree-sitter-reify` grammar. After this PRD lands, a user writing a `.ri` file can:

- **Group digits with `_`** for readability: `1_000_000`, `0.000_001`, `1_000mm` all
  evaluate identically to their separator-free forms.
- **Write hex and binary integer literals**: `0xFF` evaluates to `Int(255)`, `0b1010`
  to `Int(10)`.
- **Write single-sided range literals** using comparison-operator prefixes:
  `>2mm`, `>=2mm`, `<100MPa`, `<=100MPa` produce a `Range<T>` with exactly one bound,
  usable with `contains`, `lower`, `upper`.

All three are documented in the spec but unsupported (or silently misparsed) by the
parser today. The user-observable surface is the parser/evaluator: `.ri` files that
the spec says are legal currently fail or — worse — parse to the wrong AST.

## 2. Background: what exists, what's missing

The pipeline is `tree-sitter-reify/grammar.js` (+ `src/scanner.c`) → `reify-syntax`
(`lower_*` → `ExprKind`) → `reify-compiler` (`compile_expr_guarded` → IR
`CompiledExpr`) → `reify-ir` (`Value`) → `reify-expr`/`reify-eval` (runtime).

The decisive finding from the audit pass: **for single-sided ranges the entire
back half of the pipeline already supports optional bounds.** Only the front door
(grammar + AST lowering) is missing. Specifically:

| Layer | Single-sided range support today |
|---|---|
| `reify-ast` `ExprKind::Range` | `lower: Option<Box<Expr>>`, `upper: Option<Box<Expr>>` — already optional (`ast.rs:108-114`) |
| `reify-syntax` `lower_range_expr` | **NO** — `child_by_field_name("lower")?` / `"upper"?` early-return; comment at `ts_parser.rs:2446-2447` assumes both always present |
| grammar `range_expression` | **NO** — only `lower..upper` / `lower..<upper` arms (`grammar.js:908-913`) |
| `reify-compiler` Range lowering | **YES** — `.map` over each bound; dimensional check gated on `if let (Some,Some)`; element-type inferred from whichever bound present (`expr.rs:848-933`) |
| `reify-ir` `Value::Range` / `RangeConstructor` | **YES** — `lower`/`upper` are `Option<Box<Value>>` (`value.rs:552-557`) |
| `reify-expr` `contains` / `lower` / `upper` / `span` | **YES** — `contains` skips absent bounds (correct half-open semantics); `lower`/`upper` return `Option(None)`; `span` returns `Undef` when a bound is absent (`lib.rs:1367-1451`) |

So single-sided ranges are a **grammar + one-function-AST-lowering** change. The
compiler already even documents (`expr.rs:906-908`) that the only reason both bounds
are always present is "the parser always provides both via `?`".

For numeric literals the gap is purely lexical. The `number_literal` token is
`token(/\d+(\.\d+)?([eE][+-]?\d+)?/)` (`grammar.js:1062`) — decimal-only, no `_`,
no radix prefix. The AST carries `NumberLiteral { value: f64, is_real: bool }`
(`ast.rs`), and `classify_number_literal(value, is_real)` (`reify-ast/src/decl.rs:822`)
maps that to `Int(i64)` / `Real(f64)` / `LossyReal(f64)` with a documented
precision-loss path for integer-form tokens that don't round-trip through f64.

### 2.1 The silent-misparse trap (motivates `grammar_confirmed=false`)

`tree-sitter parse --quiet` exits **0** on `1_000_000`, `0xFF`, `0b1010` today — but
the CST is wrong. They degrade to `quantity_literal`:

```
1_000_000  →  number_literal "1" + unit_expr "_000_000"
0xFF       →  number_literal "0" + unit_expr "xFF"
0b1010     →  number_literal "0" + unit_expr "b1010"
```

i.e. the parser reads them as `1` with unit `_000_000`, `0` with unit `xFF`, etc.
A naive G3 "exit 0 = parses" check passes; the feature is still absent. Every
numeric-literal task in this PRD therefore carries `grammar_confirmed=false` and its
observable signal asserts the **CST shape / evaluated value**, not merely exit 0.
(`0deg..<360deg`, by contrast, genuinely parses correctly today — it is an
ordinary two-sided range over existing quantity literals — so it is NOT in scope.)

Single-sided ranges fail loudly: `>2mm` produces an `(ERROR ...)` node on the prefix
operator (exit 1), which is the honest signal.

## 3. Resolved design decisions

These were load-bearing forks. `AskUserQuestion` was unreachable in the authoring
environment, so each is a reasoned default with rationale, **flagged for Leo's review**
in the hand-back.

### D1 — Underscore placement rules: between-digits only.

A `_` is legal only **between two digits** of the same numeric run. Leading (`_5`),
trailing (`5_`), doubled (`1__0`), adjacent to the radix prefix (`0x_FF`, `0xFF_`),
adjacent to the decimal point (`1_.0`, `1._0`), and adjacent to the exponent marker
(`1_e3`, `1e_3`, `1e3_`) are all **rejected at the lexer** (the token simply doesn't
match; the offending text falls out as an error or a separate token).

- Decimal: `\d(_?\d)*` per integer/fraction/exponent run.
- Rationale: matches Rust, Swift, Java, Python 3.6+. Between-digits-only is the least
  surprising rule and keeps the regex a clean token with no semantic post-validation.
  `_` is purely cosmetic — stripped before numeric conversion, never part of the value.

### D2 — Hex/binary are integer-only; no hex floats, no radix on quantity values' fraction.

`0x`/`0X` and `0b`/`0B` introduce **integer** literals only. No `0x1.8p3` hex-float
form (spec defers complex/exotic literal syntax, §18). They MAY carry a unit
(`0xFFmm` is a quantity literal with integer value 255) because the quantity grammar
is `number_literal unit_expr` and a hex literal is a `number_literal` — but the value
is always integral. Octal (`0o...`) is **out of scope** (not in spec §2.3).

- Allowed digits: hex `[0-9a-fA-F]` (with `_` between), binary `[01]` (with `_`
  between). A bare `0x` / `0b` with no following digit is a lexer error.
- Rationale: spec §2.3 lists exactly `0xFF` and `0b1010`, both integers. Engineering
  models use hex/binary for bit-masks and IDs, which are discrete — `Int`, never `Real`.

### D3 — Hex/binary route through the existing `f64` AST field + `LossyReal` path; NO new exact-integer AST variant.

`ExprKind::NumberLiteral { value: f64, is_real: bool }` stays. The AST-lowering for a
hex token computes the integer via `i64::from_str_radix(digits, 16)` (after stripping
`0x` and `_`), then stores it as `value: (n as f64), is_real: false`. The existing
`classify_number_literal` then yields `Int(n)` for any value that round-trips through
f64 (exact up to 2^53) and `LossyReal` (with the already-required precision-loss
diagnostic) beyond that.

- **G6 premise:** `0xFF` → 255.0 → `Int(255)` ✓ (exact). `0b1010` → 10.0 → `Int(10)` ✓.
  Values ≤ 2^53 are exact through f64 (`u64`/`i64` hex up to `0x1F_FFFF_FFFF_FFFF`).
  Beyond 2^53 the existing `LossyReal` diagnostic fires — identical to the current
  behavior for 20-digit decimal integers (`decl.rs:809-817`).
- **Alternative considered & rejected:** adding `ExprKind::IntLiteral(i64)` to carry
  full 64-bit precision. Rejected for this PRD: it ripples through every match on
  `NumberLiteral` across `reify-compiler`, `reify-eval`, `reify-expr`, indexing
  guards (`expr.rs:1632,1706`), pragma lowering, and tests — a large blast radius for
  a precision improvement that the existing `LossyReal` path already flags. If exact
  >2^53 hex/bin integers are later required, that is a separate AST-layering PRD
  (declared as future-scope in §8). For v0.6, hex/bin inherit the same precision
  envelope as decimal integers, which is internally consistent.

### D4 — `is_real` classification must be radix-aware.

`lower_number_literal` currently sets `is_real = text.contains('.') || contains('e') ||
contains('E')` (`ts_parser.rs:2748`). A hex literal like `0xBEEF` or `0xE` contains
`E`/`e` and `0x1D` contains no marker but `0x.` is impossible (D2). The fix: when the
token starts with `0x`/`0X`/`0b`/`0B`, classify as integer unconditionally
(`is_real = false`), bypassing the `.`/`e`/`E` scan. Decimal tokens keep the existing
scan. This is the single hazard the `lower_number_literal` doc-comment at
`ts_parser.rs:2744-2747` already warns about ("if the grammar gains … hex … update
both the grammar and this classification").

### D5 — Single-sided ranges are first-class `Range` with one bound — NOT desugared to constraints.

`>2mm` lowers to `ExprKind::Range { lower: Some(2mm), upper: None, lower_inclusive:
false, upper_inclusive: true }`. `<=100MPa` → `{ lower: None, upper: Some(100MPa),
lower_inclusive: true, upper_inclusive: false }`. It is a `Value::Range` value, usable
anywhere a range is (`r.contains(x)`, `r.lower()`, `let t : Range<Length> = >2mm`).

- **Inclusivity mapping:** the prefix operator names the *present* bound's inclusivity;
  the absent bound is conventionally inclusive (it's ±∞, inclusivity is vacuous but a
  bool field must be set). `>` → lower exclusive; `>=` → lower inclusive; `<` → upper
  exclusive; `<=` → upper inclusive. The absent-side `*_inclusive` is set to `true`
  (vacuous) so `contains` — which skips absent bounds entirely (`lib.rs:1386,1399`) —
  is unaffected.
- **Rationale:** the AST, IR, and eval already model this exactly (§2). Desugaring to
  a constraint would (a) discard the `Range` value the user can pass around and (b)
  duplicate semantics the runtime already implements correctly. First-class is strictly
  less work AND matches the spec's framing ("Single-sided ranges use comparison
  operators as prefixes" — a *range literal*, §2.8).
- **G6 premise (open-bound semantics):** verified against `reify-expr/src/lib.rs`:
  `>2mm` `.contains(3mm)` → checks only lower bound `2mm < 3mm` → `true`; `.contains(1mm)`
  → `false`; `.lower()` → `some(2mm)`; `.upper()` → `none`; `.span()` → `undef`
  (unbounded). All already implemented — the only missing link is producing the AST node.

### D6 — Grammar disambiguation of prefix `>`/`<` vs binary comparison.

`>`, `>=`, `<`, `<=` are existing binary comparison operators (`binary_expression`,
prec 4, `grammar.js:895-898`). A *prefix* use (`>2mm`) is only valid where an
expression is expected with no left operand. The new `range_expression` arms are
`prec.left(0, seq('>', $._expression))` etc. — range precedence 0 is already the
lowest (`grammar.js:906`), below comparison's 4, so `a > b` still parses as a binary
comparison (the parser only takes the prefix arm when there is no preceding operand to
shift). A grammar conflict between "binary `>`" and "prefix `>`" is expected and
resolved by the GLR parser via the precedence ordering; the corpus test in task δ
must pin both `a > b` (binary) and `>2mm` (prefix range) to lock the resolution.

- **Rationale:** the spec's own EBNF (`reify-language-spec.md:2497-2499`) already
  defines `range_lit ::= … | ('<' | '<=' | '>' | '>=') expr`, confirming prefix form
  is intended and the operator set is exactly these four.

## 4. Sketch of approach

Three independent vertical slices, each grammar-first then lowering then a `.ri`/eval
signal. They share no code beyond `number_literal`/`range_expression` in `grammar.js`,
so they can land in any order; the only intra-batch ordering is grammar-before-lowering
within each feature, plus a final integration `.ri` example task that exercises all
three.

```
// Separators (slice 1)
let big   = 1_000_000          // Int(1000000)
let small = 0.000_001          // Real
let len   = 1_000mm            // Length, value 1000

// Hex / binary (slice 2)
let mask  = 0xFF               // Int(255)
let flags = 0b1010             // Int(10)
let addr  = 0xDEAD_BEEF        // Int(3735928559), separators allowed in radix runs

// Single-sided ranges (slice 3)
let lo : Range<Length>   = >2mm
let hi : Range<Pressure> = <=100MPa
constraint clearance.contains(actual)   // where clearance = >0.5mm
```

## 5. Pre-conditions for activating

- None external. All three slices are self-contained within
  `tree-sitter-reify` + `reify-syntax` (+ a one-line classifier guard for D4).
- Grammar prerequisite tasks (α, γ, ε below) are **internal** to this batch.
- No GR-001 / ComputeNode / multi-kernel dependency.

## 6. Cross-PRD relationship

No cross-PRD seams. This cluster is self-contained (per the batch brief). The only
shared file is `tree-sitter-reify/grammar.js`, edited by three tasks in *this* batch —
coordinated intra-batch, not cross-PRD.

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| — | — | — | — | no cross-PRD seams |

Note: `grammar.js` is also touched by sibling clusters in the same
`spec-gap-2026-05-27` batch. Those edits are in disjoint grammar rules
(`number_literal`/`range_expression` here). If two clusters land concurrently they
merge cleanly at the rule level; the coordinator owns batch-level merge ordering.

## 7. Decomposition plan

Greek labels are PRD-internal; task IDs assigned at decompose time. "Crates touched"
drives the G5 blast-radius read (max 2 crates per task → bare B confirmed).

### Slice 1 — digit separators

- **α — Grammar: `_` digit separators in `number_literal`.**
  Crates: `tree-sitter-reify`. Extend the `number_literal` token to
  `\d(_?\d)*(\.\d(_?\d)*)?([eE][+-]?\d(_?\d)*)?` (between-digits-only, per D1).
  `tree-sitter generate`. Corpus test `test/corpus/numeric_separators.txt` pins
  `1_000_000`, `0.000_001`, `1_000e1_0`, and asserts `1_` / `_1` / `1__0` produce
  `ERROR`. Intermediate — unlocks β.
  *Signal (intermediate):* unlocks β; corpus test asserts the CST is a single
  `number_literal` node spanning the whole token. `grammar_confirmed=false`.

- **β — Lowering: strip `_` before numeric conversion.**
  Crates: `reify-syntax`. In `lower_number_literal`, strip `_` from the token text
  before `parse::<f64>()`. Add `reify-syntax` test asserting `1_000_000` →
  `NumberLiteral { value: 1000000.0, is_real: false }` and `1_000mm` →
  `QuantityLiteral` value 1000. Leaf.
  *Signal (leaf):* `.ri` example `examples/numeric_separators.ri` evaluates
  `1_000_000 == 1000000` to `true` via `reify` eval (CLI/eval path); `reify-syntax`
  unit test asserts the lowered value. Depends: α. `grammar_confirmed=false`.

### Slice 2 — hex / binary integer literals

- **γ — Grammar: hex & binary `number_literal` alternatives.**
  Crates: `tree-sitter-reify`. Add `0[xX][0-9a-fA-F](_?[0-9a-fA-F])*` and
  `0[bB][01](_?[01])*` alternatives to `number_literal` (token-level `choice`, so
  longer-match beats `0` + `_unit_expr_start`). `tree-sitter generate`. Corpus test
  `test/corpus/radix_literals.txt` pins `0xFF`, `0b1010`, `0xDEAD_BEEF`, `0xFFmm`
  (quantity), and asserts the WHOLE `0xFF` is one `number_literal` (not `0`+unit `xFF`),
  plus `0x` / `0b` (no digits) → `ERROR`. Intermediate — unlocks δ.
  *Signal (intermediate):* unlocks δ; corpus test asserts single-node CST shape that
  defeats the §2.1 misparse. `grammar_confirmed=false`.

- **δ — Lowering: radix-aware value + `is_real` guard.**
  Crates: `reify-syntax`. In `lower_number_literal`: if token starts `0x`/`0X` →
  `i64::from_str_radix(&stripped, 16)`; `0b`/`0B` → radix 2; store as `value: n as f64,
  is_real: false` (per D3) and **bypass the `.`/`e`/`E` Real-scan for radix tokens**
  (per D4, defeats the `0xBEEF` E-false-positive). Reuse the existing `LossyReal`
  path for >2^53. Leaf.
  *Signal (leaf):* `examples/radix_literals.ri` evaluates `0xFF == 255` and
  `0b1010 == 10` to `true` via `reify` eval; `reify-syntax` test asserts
  `0xBEEF` lowers to `is_real: false` (not Real). Depends: γ. `grammar_confirmed=false`.

### Slice 3 — single-sided range literals

- **ε — Grammar: prefix comparison-operator range arms.**
  Crates: `tree-sitter-reify`. Add four `range_expression` arms:
  `prec.left(0, seq('>', $._expression))`, `'>='`, `'<'`, `'<='` (per D6). Resolve the
  expected binary-vs-prefix `>`/`<` conflict via precedence. `tree-sitter generate`.
  Corpus test `test/corpus/single_sided_range.txt` pins `>2mm`, `>=2mm`, `<100MPa`,
  `<=100MPa` as `range_expression` with one bound, AND pins `a > b` as
  `binary_expression` (regression guard for D6). Intermediate — unlocks ζ.
  *Signal (intermediate):* unlocks ζ; corpus asserts `>2mm` is `range_expression`
  (exit 0, no ERROR — replacing today's ERROR node) and `a > b` stays binary.
  `grammar_confirmed=false`.

- **ζ — Lowering: single-sided `lower_range_expr` arms.**
  Crates: `reify-syntax`. Rewrite `lower_range_expr` to detect the prefix-operator
  form: no `lower`/`upper` fields → read the operator child + single operand, emit
  `ExprKind::Range` with the appropriate `Some`/`None` bound and inclusivity per D5.
  Keep the two-sided path. Remove the stale `?`-both-present assumption comment.
  Leaf.
  *Signal (leaf):* `examples/single_sided_range.ri` — `let r = >2mm` then a
  `constraint r.contains(3mm)` evaluates `true` and `r.contains(1mm)` `false` via
  `reify` eval; `reify-syntax` test asserts `>2mm` → `Range { lower: Some, upper: None,
  lower_inclusive: false }`. Depends: ε. (Compiler/IR/eval already support optional
  bounds — §2 — so no further crates.) `grammar_confirmed=false`.

### Integration

- **η — Combined `.ri` example + spec cross-check.**
  Crates: `examples` (+ wherever the example-corpus CI runner lives). One
  `examples/numeric_and_range_literals.ri` using all three features in a single
  realistic structure (e.g. a part with `param count = 0xFF`, a dimension
  `1_000mm`, a tolerance `param fit : Range<Length> = >0.05mm`), exercised by the
  existing example-eval CI test. Leaf; integration-gate for the batch.
  *Signal (leaf):* `examples/numeric_and_range_literals.ri` parses (exit 0, no ERROR)
  AND evaluates without diagnostics in the example-corpus CI test; this is the single
  end-to-end proof that all three slices compose. Depends: β, δ, ζ.
  `grammar_confirmed=false` (depends on grammar tasks).

### DAG

```
α ─→ β ─┐
γ ─→ δ ─┼─→ η
ε ─→ ζ ─┘
```

Three parallel grammar→lowering chains converging on the η integration example.

## 8. Out of scope for this PRD

- **Hex floats** (`0x1.8p3`), **octal** (`0o17`), **complex literals** (`3.2+4.1j`,
  spec §18 #15) — not in spec §2.3.
- **Exact >2^53 integer preservation** — hex/bin inherit decimal's f64 precision
  envelope + `LossyReal` diagnostic (D3). A future `ExprKind::IntLiteral(i64)`
  AST-layering PRD could lift this; declared here as future-scope, not queued.
- **`..=` inclusive-upper sugar** — Reify uses `..` for inclusive (spec §2.8); no
  Rust-style `..=`. Not a gap.
- **Two-sided exclusive-lower / exclusive-both** (`2mm<..<5mm`) — spec defines only
  `..` and `..<`; no four-way inclusivity surface syntax. Single-sided exclusivity is
  covered by the prefix operator (`>` exclusive, `>=` inclusive).

## 9. Open questions (tactical — decide at impl time)

1. **Separator in exponent/radix runs — grouping width unenforced.** D1 permits any
   between-digits placement (`1_0_0_0`, `0xDE_AD_BE_EF`); no enforced 3- or 4-digit
   grouping. Standard across languages. Decide during α if a lint (not a parse error)
   is wanted — likely no.
2. **Diagnostic wording for `LossyReal` hex** (`0xFFFFFFFFFFFFFFFF`). Reuse the
   existing decimal precision-loss diagnostic text or add a radix-specific variant?
   Tactical; decide during δ.
3. **`reify` eval CLI entry point for the `.ri` signals.** β/δ/ζ/η name "`reify` eval";
   confirm the exact subcommand (`reify check` / `reify eval` / example-corpus test
   harness) during β — whichever the example-eval CI already uses.

## G5 note

Bare B confirmed. Blast radius ≤ 2 crates/task (`tree-sitter-reify` + `reify-syntax`);
6 mechanisms (3 grammar + 3 lowering) but each is a thin slice with the back-half of
the pipeline already built; no load-bearing seam (the parser is touched but in
self-contained literal rules, not a cross-crate contract); 0 cross-PRD consumers.
No contract section or boundary-test sketch needed.
