# String Interpolation

Status: deferred (spec-gap-2026-05-27 batch, cluster `string-interpolation`). Authored 2026-05-27 in interactive `/prd` author session. Resolves spec §18 deferred item #14 ("String interpolation — Display/templating concern") and supersedes the §2.4 note "No string interpolation in the core language."

Approach: **B (vertical slice)** with a small grammar prerequisite. Not B+H — see §G5 below. Self-contained: 4 crates touched, one grammar production, no cross-PRD seam ownership contested.

## §1 — Goal and user-observable surface

Today string literals are bare constants (spec §2.4); `"thickness is {t}"` evaluates to the nine-then-some literal characters `thickness is {t}`, with the braces and `t` swallowed as ordinary content (confirmed: `tree-sitter parse` of `"sum is {1 + 1}"` yields a single opaque `(string_literal)` node, no embedded expression).

This PRD makes `{ <expr> }` inside a string literal a **hole** that evaluates the embedded Reify expression, renders the resulting `Value` to text, and splices it in. `{{` and `}}` are literal-brace escapes.

**User-observable signal (the G2 anchor for the whole batch):**

```
$ cat /tmp/interp.ri
structure def Demo : Rigid {
  param t : Length = 5mm
  let label = "thickness is {t}, doubled is {2 * t}"
}

$ reify eval /tmp/interp.ri
Demo.label = "thickness is 5 mm, doubled is 10 mm"
```

The arithmetic hole `"x={1+1}"` → `"x=2"` is the per-leaf checkpoint named in the batch's task ζ.

**Consumers (G1).** Users build interpolated strings wherever a `String` value is authored:
- **`meta` blocks / labels / doc text** — the dominant case. A `meta` field like `description = "M{bolt_size} flange, {hole_count} holes"` renders parameter-driven prose. Consumed by the doc tool (`docs/prds/v0_3/reify-doc-tool.md`, `build_doc_model`) and by GUI hover/property panels via `Value::format_display`.
- **`reify eval` output** — direct user-observable surface (`crates/reify-cli/src/main.rs` `cmd_eval`), which prints each top-level `String` cell via `Value`'s `Display`. This is the CI-checkable signal.
- **Diagnostics / output-naming downstream** — any future PRD that derives an export filename or a warning message from parameters consumes the same lowering. Named as a forward consumer, not a gate.

No fictional/future-only consumers: `reify eval` + the doc tool both exist on `main` today.

## §2 — Sketch of approach

Three layers, matching the existing pipeline (grammar → `reify-syntax` lower → `reify-compiler` lower → `reify-expr` eval):

1. **Grammar (`tree-sitter-reify`).** Replace the monolithic `string_literal: token(seq('"', ..., '"'))` with a structured production that distinguishes a plain string (no holes — stays a single token, zero CST churn for existing corpus) from an `interpolated_string` carrying alternating string-chunk leaves and `interpolation` nodes that wrap `$._expression`. Because tree-sitter `token(...)` is atomic and cannot embed a sub-rule, and because whitespace lives in `extras` (it would be eaten inside the quoted region), the literal text runs between quotes/holes must be lexed by the **external scanner** as a dedicated content-run token. See §G3.

2. **AST (`reify-ast` / `reify-syntax`).** New `ExprKind::InterpolatedString(Vec<StringPart>)` where `StringPart` is `Literal(String) | Hole(Box<Expr>)`. The lowerer in `ts_parser.rs` decodes escape sequences (`\n \t \\ \"` and the new `{{` `}}`) into the `Literal` parts and recursively lowers each `interpolation` child's expression into a `Hole`. A literal with no holes still produces `ExprKind::StringLiteral(String)` (unchanged path).

3. **Compiler + eval (`reify-compiler` / `reify-expr`).** `InterpolatedString` lowers to a **render-then-concat fold**, *not* raw `+`. Each `Hole(expr)` lowers to a call to a new internal render builtin (`__interp_render`, see §3); literal parts lower to `Literal(Value::String(..))`; the parts fold left via the existing `Value::String + Value::String` concatenation (already implemented at `reify-expr/src/lib.rs:2025`). The whole expression's static type is `Type::String`.

### Why render-then-concat and not `+`

Raw `+` on a `String` and a non-`String` (e.g. a `Scalar`) falls through `eval_add` to `Value::Undef` (confirmed at `reify-expr/src/lib.rs:2041`). And `String + Undef` is also `Undef`. So a naive `"x=" + t` lowering would (a) reject every non-string hole and (b) make one `undef` hole poison the entire string. The render builtin sidesteps both: it maps **any** `Value` to a `String` first, so the fold only ever concatenates `String + String`.

## §3 — The render seam (G4 seam declaration)

**This PRD consumes the existing display mechanism; it does not own a new one.** Reify already has a complete value→text story in `reify-ir`:

- `Value::format_display(&self) -> String` (`crates/reify-ir/src/value.rs:1526`) — the human-facing render. `Value::String` renders **bare** (no surrounding quotes); `Value::Scalar` renders the **engineering-unit numeric** via `DimensionVector::to_display_units` (e.g. `5mm` → `"5"`); recurses through `List`/`Set`/`Map`/`Option`/composites.
- `Value::format_display_pair(&self) -> (String, String)` (`:1684`) — same but returns `(number, unit)` separately so a caller can render `"5 mm"` (value + space + unit).

Interpolation's render builtin (`__interp_render`) is a thin wrapper:
- `Value::Scalar` / `Value::Complex` / `Value::Option(Some(Scalar))` → `format_display_pair`, joined as `"{value} {unit}"` (non-empty unit) so `5mm` → `"5 mm"` (spec §G6 premise, §6.1).
- every other variant → `format_display` verbatim.
- `Value::Undef` → the literal text `"undef"` (the determinacy decision, §6.3), localised in this builtin so the rest of the fold stays total.

**Why a new builtin rather than calling `format_display` from the lowerer:** keeping the render in `reify-expr` builtin-dispatch (not the compiler) preserves the lazy/determinacy semantics — a hole's expression is a real `CompiledExpr` evaluated by the engine, so `{undef_param}` produces a runtime `Undef` value that the builtin maps to `"undef"`, rather than the compiler having to const-fold. It also gives one future-proof spot to add a format-spec (`{x:.2}`) without re-touching the lowerer (out of scope, §7).

`Display` (used elsewhere, e.g. cache keys) is deliberately **not** the render path: `Display` quotes strings (`"foo"`) and prints Scalars in **SI base units** (`0.005 m`). Interpolation must use `format_display`, which is the GUI/human path. This distinction is the one subtle correctness point and is pinned by a leaf test (task ζ).

No reciprocal-ownership ambiguity: the render mechanism is wholly inside `reify-ir`, already shipped, with no other PRD claiming it.

## §4 — Resolved design decisions

1. **Syntax: `{ expr }` holes, `{{`/`}}` escapes.** Single-brace delimiters (Python/Rust-style), doubled braces escape to a literal brace. Chosen over `${...}` (shell-style) for terseness and because `$` has no current lexical role we want to reserve. `\{` is **not** an escape (only `{{`) — keeps the escape grammar regular and matches Rust's `format!`.
2. **Render path = `format_display` family, not `Display`.** §3. Bare strings, engineering units.
3. **Dimensioned quantities render value-space-unit:** `5mm` → `"5 mm"` (a space between number and unit, unlike the unit-less literal syntax which forbids the space). Rationale: this is *output prose for humans*, not re-parseable Reify source; readability wins. §6.1.
4. **Holes are full expressions, not just identifiers.** `{2 * t}`, `{cos(theta)}`, `{a.b}` all allowed — the `interpolation` node wraps the grammar's existing `$._expression`. No new expression grammar.
5. **`undef` hole → literal `"undef"`, does not poison the string.** §6.3. The string remains determinate; only the hole's text reflects the indeterminacy. Rationale: interpolated strings are diagnostics/labels; a half-rendered label is more useful than a wholly-`undef` one, and matches `format_display`'s existing `Value::Undef => "(undefined)"`-style totality. (We use `"undef"`, not `"(undefined)"`, to match the spec's surface keyword.)
6. **Nesting:** a hole's expression may itself contain a string literal that interpolates (`{ if cond then "{a}" else "b" }`). This falls out for free because the hole wraps `$._expression` and string literals are expressions — no special-casing. §6.2.
7. **Empty hole `{}` is a parse error**, not an empty string. An empty hole is almost always a typo; erroring is the safer default. Pinned by a negative grammar fixture.
8. **Plain strings (no holes) keep the single-token fast path.** Zero CST/`ExprKind` churn for the ~all existing `.ri` corpus; `grammar_confirmed`-style regression risk is bounded to strings that actually contain `{`.

## §5 — Pre-conditions for activating

- **Grammar prerequisite (task α).** The `interpolated_string` production + external-scanner content-run token must land first. Every downstream task `depends_on` α. This is the only hard prereq; it is **filed in this batch**, not assumed.
- No GR-001 / ComputeNode / Field dependency. No other PRD blocks this one.

## §6 — Premise validation (G6)

Interpolation asserts no numeric accuracy bound, so the G6 surface is "rendering determinism + capability availability". Three premises validated:

1. **`5mm` → `"5 mm"` is producible.** `Value::format_display_pair` already returns `("5", "mm")` for a `Scalar` of `5mm` (`to_display_units(0.005)` on LENGTH → `(5.0, "mm")`, confirmed by the existing test `format_display_triple_scalar_whole_number_trims_decimal`). The render builtin joins them with a space. No new numeric code; the premise rests on shipped, tested behaviour.
2. **Nesting / recursion terminates.** A hole wraps `$._expression`; recursion is bounded by source nesting depth, which is finite per source file. No runtime unbounded recursion — same termination guarantee as any nested expression.
3. **`undef` interpolation is determinate.** The render builtin is total: it maps `Value::Undef` to a fixed string. The fold therefore never produces `Value::Undef` from a determinate set of literal parts; only a hole whose *expression fails to compile* (not evaluates to undef) is a compile error, handled by the existing diagnostic path. The §1 signal asserts only string equality, no numeric premise — passes G6 trivially; the determinacy premise is the substantive one and is satisfied by builtin totality.

No false-premise risk of the esc-3453/esc-3770 shape (no accuracy bound, no closed-form exactness claim).

## §G3 — Grammar gate result

`tree-sitter parse` was run on three fixtures (`"thickness is {t}"`, `"literal {{ }} braces"`, `"sum is {1 + 1}"`). **All exit 0 — but all parse as a single opaque `(string_literal)` token.** The braces and inner expressions are swallowed as ordinary string content; there is **no** interpolation structure in the CST. So the gate **fails in substance**: the syntax "parses" only because any byte sequence between quotes is currently legal string content. `grammar_confirmed = false` for every task; grammar work is a filed prerequisite (task α).

**Scanner finding (the flagged risk).** `string_literal` is a monolithic `token(seq('"', repeat(...), '"'))` at `tree-sitter-reify/grammar.js:1064`. tree-sitter's `token()` is atomic — a sub-rule (`$._expression`) cannot be embedded inside it. The `interpolated_string` rule must be assembled from non-token pieces (`'"'`, content-run, `'{'`, `$._expression`, `'}'`, `'"'`). The literal-text runs between quote/brace boundaries cannot be a normal grammar token because whitespace is in `extras` (`grammar.js:32`) and would be silently eaten inside the string. Therefore the content-run must be lexed by the **external scanner** (`tree-sitter-reify/src/scanner.c`), which already exists, is stateless, and has documented enum-order discipline (`externals` array ↔ `TokenType` enum). The scanner gains: a content-run token that consumes bytes until `"`, `{`, or `\` (handling `{{`/`}}`/`\"` etc.), and likely serialize/deserialize state to track "inside string after a `}`" so the lexer re-enters content mode. This is meaningful but precedented work — the unit-literal `_unit_expr_start` gate is the template. Task α owns it.

## §G4 — Cross-PRD relationship

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `crates/reify-ir` `Value::format_display` / `format_display_pair` | consumes | value→text render | reify-ir (already shipped) | wired |
| `docs/prds/v0_3/reify-doc-tool.md` (`build_doc_model`) | produces-for | rendered `meta`/label strings flow into doc HTML | this PRD (produces `Value::String`); doc tool consumes a plain String, unchanged | no integration task needed |
| GUI hover / property panel | produces-for | interpolated `Value::String` displayed | consumer unchanged (already renders `Value::String`) | no integration task needed |

No contested ownership. The doc tool and GUI consume an ordinary `Value::String`; interpolation is upstream of them and changes nothing on their side. The only seam is the `reify-ir` render functions, which already exist and which this PRD consumes (not extends). G4 = self-contained.

## §G5 — Approach choice

Bare **B (vertical slice)**, not B+H. Heuristic check: crates touched = 4 (`tree-sitter-reify`, `reify-syntax`/`reify-ast`, `reify-compiler`, `reify-expr`) — at the threshold but the cuts are clean and sequential (grammar → AST → lower → eval), not a star-topology seam. Mechanism count ≈ 4 (grammar production, AST node, render builtin, lowering fold) — well under ~8. Cross-PRD consumers = 0 load-bearing (doc tool / GUI consume an unchanged `Value::String`). Touches the grammar/parser (a load-bearing seam) — this is the one B+H trigger, and it is addressed by the dedicated grammar task α with its own parse-fixture signal rather than a contract document. No contract section needed; the "contract" is the existing `format_display` signature.

## §8 — Decomposition plan

Linear-with-fan-in DAG. Greek labels; task IDs assigned at decompose time.

- **α — Grammar: `interpolated_string` production + external-scanner content-run token + lowering to `ExprKind::InterpolatedString`.**
  Crates: `tree-sitter-reify`, `reify-syntax`, `reify-ast`.
  Observable signal: fixture `tests/corpus/interpolated_string.txt` — `"a {x} b"` parses to `(interpolated_string (string_chunk) (interpolation (identifier)) (string_chunk))`, `"{{lit}}"` parses to a plain string-chunk, empty hole `"{}"` is `:error`; `tree-sitter parse --quiet` exits 0 on the positives; a `reify-syntax` parser test asserts the `ExprKind::InterpolatedString` lowering with decoded escape parts. Plain strings still parse as a single token (regression fixture).
  `grammar_confirmed = false` (this task *creates* the grammar). Intermediate — unlocks β, γ, ζ.

- **β — Render builtin `__interp_render` in `reify-expr` (Value → display String).**
  Crates: `reify-expr` (consumes `reify-ir` `format_display`/`format_display_pair`).
  Observable signal: `reify-expr` unit tests — `__interp_render(Scalar 5mm)` → `"5 mm"`, `__interp_render(Int 2)` → `"2"`, `__interp_render(Bool true)` → `"true"`, `__interp_render(Undef)` → `"undef"`, `__interp_render(String "x")` → `"x"` (bare, no quotes — the `Display`-vs-`format_display` correctness pin). Intermediate — unlocks γ. Roped to the ζ integration leaf per the G2 escape hatch.
  Depends on: α (for the `Undef`/string-bare semantics agreed in the same batch; no code dep but sequence keeps the contract coherent — wire as edge).

- **γ — Compiler lowering: `InterpolatedString` → render-then-concat fold; static type `Type::String`.**
  Crates: `reify-compiler` (+ `reify-ir` `CompiledExprKind` if a dedicated node is chosen over a `FunctionCall` fold — impl-time choice, §9.1).
  Observable signal: compiler test — `"x={1+1}"` lowers to a fold whose evaluation yields `Value::String("x=2")`; `"a{t}b"` with `t:Length` type-checks to `Type::String` (no type error from mixing). Intermediate — unlocks ζ.
  Depends on: α, β.

- **ζ — End-to-end integration leaf: `reify eval` renders interpolated strings (the batch's user-observable gate).**
  Crates: stdlib `.ri` example + CLI golden test (`crates/reify-cli`).
  Observable signal: a committed `examples/interpolation.ri` (or CLI golden) where `reify eval` prints `Demo.label = "thickness is 5 mm, doubled is 10 mm"` and `"x={1+1}"` → `"x=2"`, `"{{braces}}"` → `"{braces}"`, `"{undef_param}"` → `"...undef..."`. CI-run. **Leaf** — the user observes the feature here.
  Depends on: α, β, γ.

- **η — Spec + docs update: rewrite §2.4 note + §18 item #14, document `{}`/`{{`/`}}` and the render rules.**
  Crates: docs only (`docs/reify-language-spec.md`), plus `crates/reify-mcp/src/tools/chunks/syntax.md` so the in-GUI assistant knows interpolation parses.
  Observable signal: spec §2.4 no longer says "No string interpolation"; `syntax.md` documents the form; a doc-lint / link check passes. **Leaf** (companion-correction task).
  Depends on: ζ (don't document until it works).

## §9 — Open questions (tactical, deferred to impl)

1. **Dedicated `CompiledExprKind::InterpolatedString` vs. a `FunctionCall` fold over `__interp_render` + `+`.** Both evaluate identically. A dedicated node is cleaner for a future format-spec; the fold reuses existing concat/dispatch. **Suggested resolution:** fold (less new surface), revisit if a format-spec PRD lands. Decide during task γ.
2. **Exact rendering of composite holes (`{my_list}`, `{my_point}`).** `format_display` already defines these (`[a, b]`, `point(x, y)`); interpolation inherits them. Confirm they read well in prose during task β; no design change expected.
3. **Whether `\{` should also be accepted as a brace escape alongside `{{`.** §4 says no (only `{{`). If user feedback wants it, additive. Decide if it ever comes up; default no.

## §10 — Out of scope

- **Format specifiers** (`{x:.2}`, alignment, padding). Future PRD; the render builtin is the extension point (§3).
- **`Display`-path changes.** Interpolation uses `format_display`; nothing about cache-key `Display` changes.
- **Tuples.** Not in the language; holes are single expressions, not tuple-destructured.
- **Interpolation in non-string literals** (e.g. inside identifiers or pragmas). String literals only.
- **Localisation / number-locale formatting.** `format_display_number` rules are inherited as-is.
