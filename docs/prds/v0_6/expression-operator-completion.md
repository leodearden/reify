# Expression & Operator Completion

Status: **deferred** (decompose-ready). Authored 2026-05-27 in interactive `/prd` session as part of the `spec-gap-2026-05-27` batch. Cluster: `expression-operator-completion`.

Closes four expression/operator gaps between `docs/reify-language-spec.md` and the live grammar/compiler/evaluator, all of which are syntax surfaces the spec promises but the tree-sitter grammar cannot parse today. Decomposition mode: **bare B (vertical slices)** — see §G5 note; each gap is an independent, architecturally-shallow grammar→lowering→eval slice.

---

## §0 — Goal and motivating signal

A Reify author can write, in a `.ri` file, and have it parse, type-check, and evaluate correctly:

```
let r        = total % per_row              // integer modulo
param t : Length = undef                    // explicit undef literal (overrides default)
let blocked  = occupied implies has_exit     // Kleene logical implication
fn double(x: Int) -> Int = x * 2            // expression-body fn sugar
```

Today every one of these **fails to parse** (or, for `undef`, silently mis-parses as a variable reference). The in-GUI assistant's own syntax reference (`crates/reify-mcp/src/tools/chunks/syntax.md` lines 27, 51, 53) already advertises `%`, `implies`, and `undef` as working language features, so the gap is a documentation-vs-reality lie the assistant tells users.

**User-observable surfaces (consumers):**

- **CLI** — `reify check <file.ri>` and `reify eval <file.ri>` on the fixtures above succeed (parse + evaluate) instead of emitting parse errors.
- **In-GUI assistant** — `crates/reify-mcp/src/tools/chunks/syntax.md` and `.../functions.md` already document these operators; this PRD makes the docs true (G1 consumer: the assistant's authoritative syntax reference).
- **Stdlib `.ri` examples** — modulo (`%`) unblocks index-wrapping idioms (`bolt_index % bolt_count`) used in `List.generate` patterns; expression-body `fn` shortens the large body of one-line helper fns the stdlib already wants.

---

## §1 — Background

The four gaps were surfaced by a coverage survey of `docs/reify-language-spec.md` (v-agnostic; we are well past v0.2 — version labels in the spec are ignored per the survey brief). Each gap has the same shape: the spec defines the surface, the **downstream** layers (AST op-string passthrough, compiler op resolution, IR `BinOp`, evaluator) are already wired, but the **grammar** (and for `implies`, the IR variant + eval) is missing.

Key code facts established at author time (2026-05-27):

| Gap | Grammar (`tree-sitter-reify/grammar.js`) | AST (`reify-ast`) | Compiler resolve (`type_compat.rs`) | IR `BinOp` (`reify-ir/src/expr.rs`) | Eval (`reify-expr`) |
|---|---|---|---|---|---|
| `%` modulo | **absent** in `binary_expression` | op carried as opaque `String` (no change) | `"%" => BinOp::Mod` ✅ (line 433) | `BinOp::Mod` ✅ | `eval_mod` ✅ (`lib.rs:2578`) |
| `undef` literal | **absent** — parses as bare `identifier` (silent degradation, like the historic `auto` bug) | needs new `ExprKind::Undef` (mirror `ExprKind::Auto`) | n/a | `Value::Undef` ✅ (`value.rs:599`) | trivial |
| expr-body `fn` | **absent** — `fn_body` requires `{ let* expr }` | desugars to `FnBody { let_bindings: [], result_expr }` ✅ | n/a | n/a | n/a |
| `implies` | **absent** | op carried as opaque `String` (no change) | **absent** — no `"implies"` arm | **absent** — no `BinOp::Implies` | **absent** — `kleene_implies` deferred YAGNI (`kleene.rs`) |

The AST stores binary operators as `ExprKind::BinOp { op: String, .. }` (`reify-ast/src/ast.rs:45`) and `ts_parser.rs::lower_binary_expr` (`crates/reify-syntax/src/ts_parser.rs:2411`) copies the grammar `op` field text through verbatim. So **for `%`, the grammar is the only missing layer** — the op-string `"%"` already resolves to `BinOp::Mod` in `type_compat.rs::resolve_binop` and `eval_mod` already evaluates it. `implies` is the only gap needing the full stack.

---

## §2 — Premise validation (G6)

The signals in this PRD assert end-to-end parse+evaluate capability, not numeric thresholds. Each end-to-end claim is traced to its task's dependency set in §6. Three semantic premises were validated against the spec at author time:

1. **`%` on dimensioned quantities is NOT defined.** Spec §5.1 ("Unary negation and modulo"): *"Modulo is `Int % Int -> Int` only."* The arithmetic-operator table promotes `Int → Real` only for the *other* operators; modulo is explicitly singled out as integer-only. So `5mm % 2mm` is a **type error**, not a value. Implication: the grammar adds `%` but the type-checker must reject `Scalar<Q>` and `Real` operands. Note: `eval_mod` (`lib.rs:2578`) *currently* also accepts `Real % Real`; spec says Int-only. Tightening eval to Int-only is a **tactical** follow-up (§Open questions Q1), not a blocker — the grammar slice can land with the type-checker as the enforcement point, matching how `^` (task 3805) enforces "non-integer exponent on dimensioned base → type error" in the checker, not the grammar.

2. **`%` precedence is the multiplicative band, left-associative.** Spec §16: level 8, sharing the band with `*` and `/`, left-associative. So `a % b * c` parses `(a % b) * c` and `a * b % c` parses `(a * b) % c`. The grammar adds `%` at the same `prec.left(6, …)` level as `*`/`/` in `binary_expression`.

3. **`implies` Kleene truth table is consistent with the de-Morgan rewrite already in use.** Spec §9.2.3 gives `a implies b` as: `true⇒true`/`false⇒false` on `(true, b)`; constant `true` on `(false, *)`; on `(undef, *)`: `true⇒true`, `false⇒undef`, `undef⇒undef`. This is **exactly** `kleene_or(kleene_not(a), b)` — the rewrite `kleene_e2e.rs:108-116` already exercises via the evaluator path. Verified row-by-row: e.g. `(undef, false)` → `kleene_not(undef)=undef`, `kleene_or(undef, false)=undef` ✅; `(false, undef)` → `kleene_not(false)=true`, `kleene_or(true, undef)=true` ✅. So `kleene_implies(a,b) := kleene_or(kleene_not(a), b)` is the correct closed form and the truth-table test is achievable from `kleene_and/or/not` already in `kleene.rs`.

4. **Expression-body `fn` is pure sugar (no inference change).** The AST `FnBody` is `{ let_bindings: Vec<LetDecl>, result_expr: Expr }` (`reify-ast/src/decl.rs:767`). `fn f(x:T) -> T = expr` lowers to `FnBody { let_bindings: vec![], result_expr: lower(expr) }` — structurally identical to the block form `fn f(x:T) -> T { expr }` with no let bindings. Signature handling, two-pass fn resolution (per memory: compiler upgraded to two-pass fn-signature resolution), and type inference are unchanged. The grammar adds an alternative `fn_body` arm; nothing downstream of the parser changes.

---

## §3 — Sketch of approach (surface + mechanism)

### 3.1 Modulo `%`

Grammar: add `prec.left(6, seq($._expression, field('op','%'), $._expression))` to `binary_expression` (same precedence band as `*`/`/`). No AST change (op string passthrough). No compiler change (`resolve_binop` already maps `%`). No IR change. No eval change (`eval_mod` exists). Type-checker: confirm `infer_binop_type`'s `BinOp::Mod => left.clone()` and add an operand-type guard rejecting non-`Int` operands with a clear diagnostic.

### 3.2 `undef` literal

Grammar: add an `undef_literal` node (a bare `'undef'` keyword) into `_primary_expression` AND `_binding_value`, mirroring the `auto_keyword` treatment (which is in `_binding_value` only — but `undef` is valid in *any* expression position per spec §5.12, so it also goes in `_primary_expression`). AST: add `ExprKind::Undef` (mirror `ExprKind::Auto { free }`, but no payload). `ts_parser.rs`: lower the new node to `ExprKind::Undef`. Compiler/eval: lower `ExprKind::Undef` to `CompiledExpr::literal(Value::Undef, …)` (the `Value::Undef` variant and its propagation through every operator per spec §9.2 already exist and are tested). **This fixes the silent-degradation bug** where `undef` currently parses as `identifier("undef")` and resolves as an undefined variable reference.

Reservation note: `undef` is already in the spec keyword list (§17, "Total: 46 keywords") but the tree-sitter lexer does NOT reserve it — `identifier` greedily matches it. Adding `'undef'` as a keyword token in expression positions makes the lexer prefer the keyword (tree-sitter rule #2: string token wins over equal-length regex). A corpus regression test must pin that a *param literally named* `undef` is no longer possible (it never was valid per spec — `undef` is reserved) and that `= undef` produces `(undef_literal)`, not `(identifier)`.

### 3.3 Expression-body `fn`

Grammar: change `fn_body` from a single `{ let* result }` form to a `choice` of two forms:
- block: `'{' repeat(fn_let_binding) result '}'` (unchanged)
- expression: `'=' field('result', $._expression)`

`ts_parser.rs::lower_fn_body`: when the expression form is taken, produce `FnBody { let_bindings: vec![], result_expr }`. No other change.

### 3.4 `implies` (full stack)

Grammar: `implies` is the lowest-precedence binary operator (spec §16 level 15, **right-associative**). The current grammar has no keyword logical operators at all (it uses `&&`/`||`/`!`). To attach `implies` coherently it must sit below `or`. **Scope decision (flagged):** this PRD adds the keyword logical-operator band — `and`/`or`/`not`/`implies` — as one grammar production cluster (levels 12-15), because `implies` cannot be cleanly inserted without the band it terminates, and the spec §5.3 mandates *keyword* operators ("Keywords, not symbols"). The `and`/`or`/`not` keyword forms already resolve downstream (`resolve_binop`: `"and" => BinOp::And`, `"or" => BinOp::Or`; `resolve_unop`: `"not" => UnOp::Not`), so only `implies` needs new IR/compiler/eval. See §4 for the scope rationale and §7 seam table.

- Grammar: add `prec.left(13, … 'and' …)`, `prec.left(14, … 'or' …)`, `prec(12, 'not' …)` unary, `prec.right(15, … 'implies' …)` to `binary_expression`/`unary_expression`. (Keeps the existing `&&`/`||`/`!` arms for back-compat with any current fixtures; a follow-up may deprecate them — tactical, Q3.)
- IR: add `BinOp::Implies` to `reify-ir/src/expr.rs`.
- Compiler: add `"implies" => Some(BinOp::Implies)` to `resolve_binop`; add `BinOp::Implies => Type::Bool` to `infer_binop_type` and the exhaustive match arms in `type_compat.rs`.
- Eval: add `BinOp::Implies => eval_implies(left, right, ctx)` to the dispatch in `reify-expr/src/lib.rs` (mirrors `eval_and`/`eval_or` at `lib.rs:1698`); add `pub fn kleene_implies(a, b) := kleene_or(kleene_not(a), b)` to `reify-expr/src/kleene.rs` (re-introducing it exactly as `kleene-logic.md §2` anticipates).

---

## §4 — Scope rationale: why the keyword logical band rides with `implies`

The cluster's four named gaps are `%`, `undef`, expression-body `fn`, and `implies`. The keyword forms `and`/`or`/`not` are a *fifth, adjacent* spec-conformance gap (spec §5.3: logical operators are keywords, not `&&`/`||`/`!`) that was not in the original four. They are pulled into the `implies` grammar task — and only that task — for one coherent reason: `implies` is the lowest member of a 4-level logical precedence band (§16 levels 12-15), and you cannot insert the bottom of a precedence ladder without the rungs above it being present in the same grammar production. Adding `implies` at level 15 while `and`/`or` only exist as `&&`/`||` at the old levels 1-2 would create a precedence inconsistency. So the grammar task defines the full keyword band; the *evaluation* work remains scoped to `implies` alone (the other three already evaluate via the existing op-string mapping). This expansion is **flagged as an assumption** (§Assumptions A1) since it widens one task beyond the literal four-gap brief.

---

## §5 — Pre-conditions for activating

- **Grammar prerequisite (intra-batch):** every lowering/eval task depends on its grammar task (`grammar_confirmed=false` on grammar tasks until the parser is regenerated and the fixture parses).
- **Seam coordination with task 3805 (`^` operator), pending:** task 3805 adds `^` to the *same* `binary_expression` rule at a precedence band *above* multiplicative/unary. `%` (this PRD) adds at the multiplicative band. Both edit `binary_expression` and regenerate the parser. They are not logically dependent but **touch the same grammar file and generated `parser.c`**, so a merge-order coordination edge is declared (§7). No code conflict in lowering/eval (disjoint op strings). See §7.
- No GR-001 / ComputeNode / multi-kernel dependencies — these are pure front-end (grammar→AST→IR→eval) changes.

---

## §6 — Decomposition plan

Decomposition style **bare B (vertical slices)**, four independent slices plus one shared grammar-regen consideration. Greek labels; task IDs assigned at decompose time. Each leaf names a user-observable signal (a `.ri` fixture that parses + evaluates via `reify check`/`reify eval`, or a `cargo test` corpus/eval assertion).

Per-gap each slice is grammar (intermediate, `grammar_confirmed=false`) → lowering+eval+leaf-signal. Because `%`, `undef`, and expr-body `fn` need no new downstream code (only `undef` needs a small AST/lowering addition), several slices collapse grammar+lowering into a single task; `implies` is split grammar / eval because of its IR+eval surface.

### α — Grammar: modulo `%` at multiplicative precedence band
- **What:** add `%` arm to `binary_expression` (`prec.left` same band as `*`/`/`, left-assoc); regenerate `parser.c`; corpus test.
- **Observable signal:** `tree-sitter parse --quiet` on a fixture `let r = a % b` exits 0 with a `(binary_expression … op: "%" …)` node; a corpus test in `tree-sitter-reify/test/corpus/` pins the production and the `(a % b) * c` / `(a * b) % c` associativity+precedence.
- **Type:** intermediate (β consumes it). **grammar_confirmed:** false. **Prereqs:** coordinate with task 3805 (§7). **Crates touched:** tree-sitter-reify.

### β — Modulo lowering + Int-only type guard + eval leaf
- **What:** confirm `ts_parser` passes `"%"` through (no change expected); confirm `resolve_binop`/`eval_mod` path; add a type-checker guard rejecting non-`Int` operands (`5mm % 2mm`, `1.5 % 2` → diagnostic). Stdlib/example `.ri` using `%`.
- **Observable signal:** `reify eval` on a `.ri` fixture computing `7 % 3` returns `Int(1)`; `reify check` on `5mm % 2mm` emits a clear type-error diagnostic (named code, e.g. `E_MODULO_REQUIRES_INT`). Eval test in `reify-expr` pins `Int % Int → Int`, `Int % 0 → Undef`, `undef % 5 → undef` (per spec §9.2.1).
- **Type:** **leaf**. **grammar_confirmed:** true (grammar landed in α). **Prereqs:** α. **Crates touched:** reify-syntax, reify-compiler, reify-expr, examples/stdlib.

### γ — Grammar + AST + lowering: `undef` literal
- **What:** add `undef_literal` node to `_primary_expression` and `_binding_value`; reserve the keyword in the lexer; regen `parser.c`; corpus test pinning `= undef` → `(undef_literal)` (not `(identifier)`). Add `ExprKind::Undef` to `reify-ast`; lower in `ts_parser.rs`; lower `ExprKind::Undef` → `Value::Undef` in the compiler.
- **Observable signal:** `tree-sitter parse` on `param t : Length = undef` and `let a = thickness * undef` yields `(undef_literal)` nodes (corpus test); `reify eval` on a fixture `let a = 5 * undef` returns `undefined` (`Value::Undef`), and `let a = undef` binds `Value::Undef` (not an undefined-variable error). Regression test pins that `undef` no longer parses as `identifier`.
- **Type:** **leaf**. **grammar_confirmed:** false. **Prereqs:** none. **Crates touched:** tree-sitter-reify, reify-ast, reify-syntax, reify-compiler.

### δ — Grammar + lowering: expression-body `fn` sugar
- **What:** make `fn_body` a `choice` of `{ let* result }` and `= expr`; regen `parser.c`; corpus test. In `ts_parser.rs::lower_fn_body`, the `= expr` form produces `FnBody { let_bindings: [], result_expr }`.
- **Observable signal:** `reify eval` on a `.ri` declaring `fn double(x: Int) -> Int = x * 2` and calling `double(21)` returns `Int(42)`; a parser test asserts the expression-body and the equivalent block-body `{ x * 2 }` lower to identical `FnBody` ASTs (modulo span). Corpus test pins both `fn_body` forms.
- **Type:** **leaf**. **grammar_confirmed:** false. **Prereqs:** none. **Crates touched:** tree-sitter-reify, reify-syntax.

### ε — Grammar: keyword logical operator band (`and`/`or`/`not`/`implies`)
- **What:** add keyword `and` (level 13, left), `or` (level 14, left), `not` (level 12, prefix), `implies` (level 15, right-assoc) to `binary_expression`/`unary_expression`; keep `&&`/`||`/`!` arms; regen `parser.c`; corpus tests for precedence (`a or b implies c` → `(a or b) implies c`; right-assoc `a implies b implies c` → `a implies (b implies c)`).
- **Observable signal:** `tree-sitter parse` on `let r = a and b or not c implies d` exits 0 with the spec-correct precedence/associativity tree (corpus test); the `op` fields read `"and"`, `"or"`, `"not"`, `"implies"`.
- **Type:** intermediate (ζ consumes the `implies` production). **grammar_confirmed:** false. **Prereqs:** coordinate with task 3805 (§7, same `binary_expression` file). **Crates touched:** tree-sitter-reify.

### ζ — `implies` IR variant + compiler resolve + Kleene eval leaf
- **What:** add `BinOp::Implies` to `reify-ir`; `"implies" => BinOp::Implies` in `resolve_binop` + `Type::Bool` in `infer_binop_type` and exhaustive arms; `eval_implies` in `reify-expr/src/lib.rs` dispatch; `pub fn kleene_implies(a,b) = kleene_or(kleene_not(a), b)` in `kleene.rs`; truth-table tests.
- **Observable signal:** `reify eval` on a `.ri` fixture: `true implies false` → `false`, `false implies undef` → `true`, `undef implies false` → `undef` (the three non-trivial spec §9.2.3 rows); a `kleene_implies` unit test covers all 9 rows of the §9.2.3 `a implies b` column; `reify check` rejects `5 implies 3` (non-Bool operands) with a clear diagnostic.
- **Type:** **leaf**. **grammar_confirmed:** true (grammar landed in ε). **Prereqs:** ε. **Crates touched:** reify-ir, reify-compiler, reify-expr.

### η — Companion: update kleene-logic.md cross-reference
- **What:** `docs/prds/kleene-logic.md §2` says `kleene_implies` was deferred and "should be reintroduced when `BinOp::Implies` evaluation is wired." Update that note to reference ζ's landing and point at the reintroduced `kleene_implies`. Doc-only.
- **Observable signal:** `docs/prds/kleene-logic.md` updated; doc lint passes; no code change.
- **Type:** **leaf** (doc). **grammar_confirmed:** N/A (true). **Prereqs:** ζ. **Crates touched:** docs only.

### Dependency view

```
α (% grammar) ──→ β (% lower+eval leaf)        [coordinate w/ task 3805 on binary_expression]
γ (undef grammar+AST+lower leaf)               [independent]
δ (expr-body fn grammar+lower leaf)            [independent]
ε (logical-keyword band grammar) ──→ ζ (implies IR+eval leaf) ──→ η (kleene-logic.md doc)
                                                [ε coordinates w/ task 3805 on binary_expression]
```

---

## §7 — Cross-PRD / cross-task relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/kleene-logic.md` (task 2314, done — *test-design reference*, not an owning PRD) | this PRD discharges its open item | `kleene_implies` in `reify-expr/src/kleene.rs`; `BinOp::Implies` eval path | **this PRD** (task ζ) | queued (ζ); doc updated by η |
| task **3805** (`Value-level ^ operator`, pending) | sibling — shared file | `tree-sitter-reify/grammar.js::binary_expression` + generated `parser.c` regen | **shared**; both edit the same rule | coordination edge: α and ε depend-on 3805 so the parser regen serializes (avoids two concurrent `parser.c` rebuilds colliding under the narrow-file-lock orchestrator) |

**G4 resolution for `implies`:** kleene-logic.md is explicitly a *test-design reference* tied to the (done) task 2314 — it implements exhaustive Kleene tests for `and`/`or`/`not` and documents that `implies` was deferred YAGNI (no `BinOp::Implies`, no `kleene_implies`). It does **not** own the operator's implementation and no other PRD designs it end-to-end. Therefore `implies` is in scope **here**, and this PRD's task η updates the kleene-logic.md note so the two docs stay consistent. No reciprocal-ownership ambiguity.

**Coordination with 3805 (not a true logical dependency):** `%` and `^` are independent operators, but both grammar tasks (α, ε) regenerate `parser.c` from the same `binary_expression` rule. The dependency edges α→3805 and ε→3805 exist purely to serialize the grammar regeneration and avoid concurrent `parser.c` rebuild churn / merge conflicts on the generated artifact. If 3805 is cancelled or already merged before this batch activates, those edges become no-ops (the regen still works). **Flagged** — see §Assumptions A2.

---

## §8 — Out of scope

- **`^` exponentiation operator** — owned by task 3805 (pending). Not duplicated here.
- **Tightening `eval_mod` to Int-only** (it currently also accepts `Real % Real`) — tactical follow-up, §Open questions Q1; this PRD enforces Int-only at the **type-checker** (β), which is the user-visible gate, and leaves the eval-layer cleanup as a separate small task.
- **Deprecating `&&`/`||`/`!` symbol forms** in favor of the spec's keyword-only logical operators — tactical, §Open questions Q3. This PRD adds the keyword forms; it does not remove the symbol forms.
- **`undef` in *type* position** (vs value position) — spec §5.12 only covers expression/value positions; type-level undef is not a thing.
- **`auto` writability** — already handled (`ExprKind::Auto`, `auto_keyword`); this PRD's `undef` work mirrors it but does not touch `auto`.

---

## §9 — G5 note (B vs B+H)

Bare **B**. None of the G5 triggers fire: blast radius is the front-end pipeline crates only (tree-sitter-reify, reify-ast, reify-syntax, reify-compiler, reify-expr, reify-ir — but each slice touches a shallow subset and no slice spans a load-bearing engine seam); mechanism count is 4 (5 with the keyword-band expansion); no FEA/ComputeNode/persistent-naming/multi-kernel seam; cross-PRD consumers are documentation surfaces, not downstream PRDs. The work is grammar-rule additions with already-wired downstream paths — the canonical "self-contained feature" case where bare B is correct. No contract section or boundary-test sketch needed.

---

## §10 — Open questions (tactical; deferred, not blocking)

1. **`eval_mod` Real-operand cleanup.** `eval_mod` (`reify-expr/src/lib.rs:2578`) accepts `Real % Real`, but spec §5.1 says Int-only. β enforces Int-only at the type-checker (user-visible). **Suggested resolution:** file a small follow-up to make `eval_mod` return `Value::Undef` (or assert unreachable) on Real operands, since the checker should have rejected them. Decide during β.

2. **`undef` literal node naming / CST shape.** `undef_literal` vs reusing a generic `special_value` node also covering `auto`. **Suggested resolution:** dedicated `undef_literal` mirroring the standalone `auto_keyword` node, for CST clarity and corpus-test stability. Decide during γ.

3. **Symbol-form logical operators (`&&`/`||`/`!`) deprecation.** Spec §5.3 mandates keyword-only; the grammar currently has symbol forms and some fixtures may use them. **Suggested resolution:** keep both forms parseable for now; file a separate deprecation/lint task once the keyword forms are proven and fixtures migrated. Decide post-ε.

4. **Modulo diagnostic code name.** `E_MODULO_REQUIRES_INT` is a placeholder. **Suggested resolution:** align with the existing diagnostic-code naming convention in `reify-compiler` during β.

---

## §11 — Assumptions (decided without Leo; AskUserQuestion unavailable in this session — flagged for review)

- **A1 — keyword logical band rides with `implies` (ε).** The original four-gap brief named `implies` but not the `and`/`or`/`not` keyword forms. ε adds all four keyword logical operators because `implies` is the bottom of a 4-level precedence ladder and cannot be inserted coherently without the band above it (§4). Downstream eval work stays scoped to `implies` only. **If Leo wants `and`/`or`/`not` keyword forms excluded**, ε must instead splice `implies` directly below the existing `||` band — workable but leaves a spec-§5.3 inconsistency (symbol-only `and`/`or`, keyword `implies`).

- **A2 — coordination edges α→3805, ε→3805.** Treated `^`-operator task 3805 as a grammar-file co-editor and added serialization edges so the two `parser.c` regens don't collide. **If Leo prefers** these as a soft note rather than hard deps (e.g. 3805's status is uncertain), drop the edges — the slices are logically independent and the regen is idempotent.

- **A3 — `%`/`implies` precedence and types taken verbatim from spec §5.1/§5.3/§9.2.3/§16.** `%` = Int-only, multiplicative band, left-assoc. `implies` = Bool-only, lowest band, right-assoc, Kleene `¬a ∨ b`. These are spec-dictated, not genuine forks — low risk.

- **A4 — expression-body `fn` is pure desugar** (`FnBody { let_bindings: [], result_expr }`), no inference change (§2.4). Spec §18 #10 calls it "sugar"; the AST shape makes it mechanical.
