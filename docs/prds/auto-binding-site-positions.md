# PRD: `auto` at all binding-site positions

**Milestone:** version-agnostic foundation (grammar/value-literal correction)
**Status:** active — authored 2026-05-26
**Type:** extension of shipped M3 `auto` resolution; sibling to `docs/prds/auto-type-param-resolution.md`
**Approach:** B + H (contract + two-way boundary tests) — touches grammar/parser **and** the constraint-solver/determinacy seam.

---

## 0. Context & relationship

`auto` / `auto(free)` (solver-delegation of a value, spec §2.9) shipped in M3 — but only at **one** grammar position: the `param` default (`grammar.js:424`, lowered to `ExprKind::Auto { free }` at `reify-syntax/src/ts_parser.rs:1707-1709`). Spec §2.9 promised `auto` "can appear anywhere a value expression is expected"; the grammar never delivered that. The result is two distinct defects:

- **Silent identifier degradation.** `auto` is not a reserved word, so in any position the grammar doesn't special-case it (`bore = auto`, `Bolt(length: auto)`, `let m = auto`, `clamp(auto)`, `auto + 2mm`), `auto` parses as an ordinary `identifier` — a reference to a nonexistent symbol. This is semantic garbage that **passes the parse-only `/prd` G3 gate** and is only (maybe) caught later at name resolution.
- **Spec/grammar drift.** §2.9 over-promises; §2.10 (line 183) already lists `auto` as a keyword the lexer doesn't actually reserve.

This PRD makes `auto` work correctly at every position where it is *meaningful* — the **binding sites** — and a clean parse error everywhere else. The §2.9 spec text was reconciled in the authoring session (narrowed to the binding-site definition below).

This is **distinct** from `auto`-as-a-type-argument (`Bearing<auto: Seal>`, `docs/prds/auto-type-param-resolution.md`, tasks 3526/3558/3559) — that is a type-parameter feature with its own dark-producer gap and is **out of scope** here.

## 1. Goal & user-observable surface

A Reify author can write `auto` / `auto(free)` to delegate a value to the constraint solver at **any binding site**, and gets a precise parse error if they misuse it as an expression operand.

User-observable signals when this lands:
- `examples/auto_binding_sites.ri` exercises `auto` at a sub-instance override, a structure-construction named argument, a `let` binding, and a connect-parameter assignment, and resolves end-to-end under `reify check` (each delegated cell goes `Auto → Determined`).
- `reify check` on a fixture that writes `auto` as a function-call argument or arithmetic operand emits a precise diagnostic / parse error — never silent acceptance.
- The `/prd` G3 grammar gate stops green-lighting `bore = auto` as if it parsed correctly: misuse now fails the parse, correct use produces an `(auto_keyword)` node.

## 2. Background — the gap, precisely

Empirical parse map (real `tree-sitter` binary, 2026-05-26):

| Position | Today | Target |
|---|---|---|
| `param x : T = auto` / `auto(free)` | ✅ `(auto_keyword)` | unchanged |
| sub-instance override `bore = auto` | ⚠️ silent `(identifier)` | ✅ `(auto_keyword)` |
| construction named-arg `Bolt(length: auto)` | ⚠️ silent `(identifier)` | ✅ `(auto_keyword)` |
| `let m : T = auto` | ⚠️ silent `(identifier)` | ✅ `(auto_keyword)` |
| connect-param `gain = auto` | ⚠️ silent `(identifier)` | ✅ `(auto_keyword)` |
| function-call arg `clamp(auto)` / `clamp(x: auto)` | ⚠️ silent `(identifier)` | ❌ parse error (positional) / semantic reject (named) |
| arithmetic/logical operand `auto + 2mm` | ⚠️ silent `(identifier)` | ❌ parse error |
| `constraint`/`minimize`/`maximize` body | ⚠️ silent `(identifier)` | ❌ parse error |
| field `source = analytical { auto }` | ⚠️ silent `(identifier)` | ❌ parse error |
| collection literal `[auto]` | ⚠️ silent `(identifier)` | ❌ parse error |

The value-position AST node already exists: `ExprKind::Auto { free: bool }` (`reify-types/src/ast.rs:88-90`). No new `Value` machinery is required — this is grammar surface + lowering + a narrow semantic gate + reuse of the existing M3 determinacy/solver path.

## 3. Sketch of approach

1. **Reserve the keyword.** Make `auto` (and `auto(free)`) reserved at the lexer level, matching spec §2.10's existing keyword table. Blast radius is zero: the corpus has **no** bare-`auto` identifiers. Consequence: in any position the grammar does not explicitly admit `auto_keyword`, `auto` becomes a hard **parse error** rather than a silent identifier.

2. **Shared `_binding_value` grammar rule.** Define `_binding_value: $ => choice($.auto_keyword, $._expression)` and use it at all five binding-site value slots — refactoring the existing `param_declaration` default (`grammar.js:424`) to use it, and extending it to `param_assignment` (569), `named_argument` (661), `let_declaration` (445), `connect_param_assignment` (642). `auto_keyword` stays **out of `_expression`**, so operand positions reject it (point 1).

3. **Shared lowering.** A `lower_binding_value` helper in `ts_parser.rs` maps an `auto_keyword` CST child → `ExprKind::Auto { free }` (else `lower_expr`), applied at all five sites.

4. **Narrow semantic gate.** One residual ambiguity: `named_argument` is shared between structure construction (binding — valid) and function-call named args (operand — invalid); `argument_list` (`grammar.js:916`) also admits `named_argument`. The construction-vs-function distinction is **semantic** (is the callee a structure or a function?). So a single compile-phase check rejects `ExprKind::Auto` reaching a function-call argument with `E_AUTO_NOT_AT_BINDING_SITE`. This is the *only* place auto can be parsed-legal-but-semantically-wrong; every other operand position is a parse error.

5. **Solver consumption.** Each binding site's delegated cell adopts `DeterminacyState::Auto` (strict) / Auto-with-skip-uniqueness (`auto(free)`) and flows into the **same** M3 constraint-resolution pass that already resolves `param`-default `auto`. The invariant (§4) is that a binding-site `auto` behaves identically to the equivalent param-default `auto`.

## 4. Contract

### 4.1 Grammar contract
- `auto`, and the `auto(free)` form, are reserved keywords; the lexer never produces an `identifier` token for them.
- `_binding_value = choice(auto_keyword, _expression)` is admitted at exactly five slots: `param_declaration.default`, `param_assignment.value`, `named_argument.value`, `let_declaration.value`, `connect_param_assignment.value`.
- `auto_keyword` is **not** a member of `_expression`, `argument_list`'s `_expression` alternative, field-source expressions, constraint/objective bodies, or collection literals. `auto` in any of those is `(ERROR)`.

### 4.2 Lowering contract
- An `auto_keyword` CST child at any binding-site slot lowers to `ExprKind::Auto { free }`, with `free = true` iff the `modifier` field (`auto(free)`) is present (same rule as the existing param-default path).
- Lowering is identical across the five sites (shared helper) — no per-site divergence in the `free` flag or node shape.

### 4.3 Semantic-gate contract
- `ExprKind::Auto` reaching a **function-call** argument (positional is impossible — parse error; named arrives via shared `named_argument`) → diagnostic `E_AUTO_NOT_AT_BINDING_SITE` (error severity), naming the function and suggesting a `param … = auto` binding instead.
- `ExprKind::Auto` reaching a **structure-construction** named argument → accepted; the constructed `Value::StructureInstance` field cell adopts determinacy `Auto`.

### 4.4 Determinacy/solver contract (the invariant)
> For every binding site, `slot = auto` produces a value cell with `DeterminacyState::Auto`, and `slot = auto(free)` a cell that participates in resolution with uniqueness-verification skipped — and the resolved value, the strict-under-determined error, and the `auto(free)` non-unique warning are **identical** to those produced by the equivalent `param x = auto` / `param x = auto(free)` default.

Strict-`auto` under-determination → existing M3 "not uniquely determined" error. `auto(free)` non-unique → existing warning. No new resolution semantics are introduced; only new *producers* of `DeterminacyState::Auto` cells.

## 5. Pre-conditions for activating

- **M3 `auto` resolution** — shipped (param-default path is the reference implementation).
- **GR-001 / structure-instance-runtime** — **DONE 2026-05-26** (`Value::StructureInstance` live). Required only for the construction-named-arg site; the other three sites do not depend on it.
- No grammar prerequisite task — this PRD *is* the grammar work.

## 6. Out of scope

- **Solver-determined fields** (`field def … { source = auto }`). Field defs have no scalar default slot; a solver-resolved field means solving for a *function*, not a value — a separate, larger feature. Explicitly dropped this session.
- **`auto` as a type argument** (`Bearing<auto: Seal>`) — `docs/prds/auto-type-param-resolution.md`, tasks 3526/3558/3559.
- **`auto` as an expression operand** (`auto + 2mm`, `clamp(auto)`) — intentionally a parse error; the idiom is `length = auto, length > 2mm`.
- **`undef` positional rules.** §2.9's `undef` wording is unchanged; whether `undef` is similarly restricted is not addressed here.

## 7. Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/structure-instance-runtime.md` | consumes | `Value::StructureInstance` field cell adopting `DeterminacyState::Auto` for construction named-arg `auto` | this PRD | wired (GR-001 shipped) |
| `docs/prds/auto-type-param-resolution.md` | sibling (no seam) | shares the `auto_keyword` grammar rule + `free` modifier; this PRD must not regress `auto_type_arg` (`grammar.js:700`) | shared rule; this PRD owns value-position, that PRD owns type-position | independent |

No new contested-ownership seam (checked against `phase-3-breadcrumb-map.md` §3 — none of the three known pairs touch value-position `auto`).

## 8. Boundary-test sketch

### 8.1 Producer side (reify-syntax: grammar + lowering)
| Scenario | Precondition | Postcondition |
|---|---|---|
| Each binding site accepts `auto` | fixture with `auto` at param-default, sub-override, construction named-arg, let, connect-param | parse exit 0; each yields `(auto_keyword)` with no `modifier`; lowers to `ExprKind::Auto { free: false }` |
| Each binding site accepts `auto(free)` | same five with `auto(free)` | `(auto_keyword (modifier))`; `ExprKind::Auto { free: true }` |
| Operand positions reject | `auto + 2mm`, `clamp(auto)`, `constraint auto`, `minimize auto`, `[auto]`, `source = analytical { auto }` | parse exit 1, `(ERROR)` node at the `auto` token |
| Keyword reservation | `let auto = 3mm` (auto as a name) | parse error (auto is reserved) |
| `auto_type_arg` not regressed | `Bearing<auto: Seal>` | still parses to `auto_type_arg` (no change) |

### 8.2 Consumer side (reify-compiler/reify-eval: determinacy + solver)
| Scenario | Precondition | Postcondition |
|---|---|---|
| Sub-override resolves | `sub b : Bearing { bore = auto }` with constraints determining `bore` | `bore` cell `Auto → Determined`; value equals the equivalent param-default-`auto` result |
| Construction named-arg resolves | `Bolt(length: auto)` in a constrained context | constructed instance's `length` field resolves identically to a param default |
| `let` resolves | `let m : Length = auto` referenced by a determining constraint | `m` cell `Auto → Determined` |
| connect-param resolves | `connect a -> b { gain = auto }` constrained | connector `gain` cell resolves |
| strict under-determined | any site, `auto`, no unique solution | existing M3 strict error (identical message to param-default case) |
| `auto(free)` non-unique | any site, `auto(free)`, multiple feasible | existing M3 non-unique warning + a feasible value |
| function-arg rejected | `clamp(x: auto)` (clamp is a function) | `E_AUTO_NOT_AT_BINDING_SITE` diagnostic, names `clamp` |

## 9. Decomposition plan

Vertical-slice DAG; Greek labels are placeholders (IDs assigned at decompose).

- **α — Reserve `auto` keyword + shared `_binding_value` rule + wire 5 sites + regen + corpus tests.**
  Crates: `tree-sitter-reify`, `reify-syntax`. Intermediate (unlocks β, δ).
  Signal it unlocks: a regenerated parser where each binding site yields `(auto_keyword)` and every operand fixture yields `(ERROR)` (the §8.1 corpus). Wide-lock foundation task per `feedback_orchestrator_narrow_locks_favor_upfront_design`.
- **β — Shared `lower_binding_value` → `ExprKind::Auto` at all 5 sites.**
  Crates: `reify-syntax`. Intermediate (unlocks γ, δ, ε). Prereq: α.
  Signal it unlocks: AST snapshot tests show each site lowering to `ExprKind::Auto` with the correct `free` flag.
- **γ — Vertical slice + integration gate: sub-instance-override `auto` end-to-end.**
  Crates: `reify-compiler`, `reify-eval`. **Leaf.** Prereq: β.
  Observable signal: `examples/auto_binding_sites.ri` (sub-override slice) resolves under `reify check`; a determinacy probe shows `bore` went `Auto → Determined`. Carries the §8.2 boundary-test signal.
- **δ — Semantic gate: reject `auto` in function-call args with `E_AUTO_NOT_AT_BINDING_SITE`.**
  Crates: `reify-compiler`, `reify-types` (diagnostic registration). **Leaf.** Prereq: β (parallel to γ).
  Observable signal: `reify check` on a `clamp(x: auto)` fixture emits `E_AUTO_NOT_AT_BINDING_SITE` naming `clamp`.
- **ε — Remaining sites end-to-end: construction named-arg, `let`, connect-param.**
  Crates: `reify-compiler`, `reify-eval`. **Leaf.** Prereq: γ (resolution pattern established); construction named-arg additionally relies on GR-001 (done).
  Observable signal: `examples/auto_binding_sites.ri` extended so all four sites resolve in one file under `reify check`.
- **ζ — Companion doc corrections.**
  Crates: docs only. **Leaf.** Prereq: α.
  Observable signal: `docs/architecture-audit/` no longer flags binding-site `auto` as unimplemented; `reference/` grammar notes updated; spec §2.9 (already edited this session) cross-referenced. (The §2.9 spec edit itself is done — ζ covers the audit/gap-register drift only.)

G2: α, β are intermediates with named downstream unlocks; γ, δ, ε, ζ are leaves with user-observable signals. The integration step (γ) is a first-class leaf, not a starved medium-priority follow-up.

## 10. Open questions (tactical)

1. **Diagnostic code name/number.** `E_AUTO_NOT_AT_BINDING_SITE` is a working name; reconcile with the existing `E_AUTO_*` family in `reify-types/src/diagnostics.rs` at task δ. Tactical — any coherent name works.
2. **`auto(free)` lexer disambiguation under reservation.** The existing `auto(free)` precedence handling (`grammar.js:431-435`) resolves the `auto` + `(` shift-reduce conflict; confirm reserving `auto` doesn't reopen it. Decide during α; fallback is the existing `prec(1, …)`.
3. **`let m = auto` with no determining constraint.** Is an unconstrained solver-delegated `let` a strict-`auto` error, or does it inherit the param-default behavior verbatim? Default: verbatim param-default behavior (the §4.4 invariant). Confirm during ε.
