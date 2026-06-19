# Capability manifest — expected-type-pushdown

Per-leaf capability→evidence bindings (mechanizes G3 + G6). Companion to `expected-type-pushdown.md`. Leaves: **β, δ, ε** (α is an intermediate foundation — no user-observable leaf signal; roped to ε per the C-as-integration-gate pattern). Greek labels map to task IDs at decompose. All evidence verified against `main` 2026-06-19 (commands reproduced with `target/debug/reify check`).

A FAIL value in any row blocks the batch. **No row FAILs** — every substrate exists on `main`; the two new diagnostics are *forward rejections* (the introducing task is the producer, bound to a boundary test that asserts firing — not logged as mere motivation, the esc-4575 anti-pattern).

---

## α — expected-type channel foundation *(intermediate, not a leaf)*

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `compile_expr` exists to thread a context through | Capability→producer | `crates/reify-compiler/src/expr.rs` ~:962 — `pub(crate) fn compile_expr(expr, scope, enum_defs, functions, diagnostics)` (no `expected_type` today; α adds the channel) | PASS (wired-on-main) |
| empty-literal arms exist to consult the channel | Capability→producer | `expr.rs` ListLiteral ~:3856, SetLiteral ~:3886, MapLiteral key ~:3926 / value ~:3938 — each `unwrap_or_else(|| { warn; default })` | PASS |
| `Type::{List,Set,Map}` element slots | Capability→producer | `crates/reify-core/src/ty.rs` List ~:85, Set ~:87, Map ~:89 | PASS |
| **Unlocks** β, δ, ε | DAG-direction | α is upstream of all three (foundation) | PASS |

## β — let-binding push-down + kind-mismatch error *(leaf)*

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| expected-type channel | Capability→producer (anti-inversion) | `producer:task-α` — α is in β's **upstream** dependency closure (β depends on α) | PASS |
| let path consumes the annotation | Capability→producer (wired) | `grep entity.rs ~:1659–1715` — let path compiles RHS then `fixup_option_none_for_let`; β wires the annotation as `expected_type` here (production path, not test-only) | PASS |
| `DiagnosticCode::CollectionLiteralKindMismatch` (new) | Rejection-mechanism (anti-silent-accept, **forward**) | *Current (motivation):* `let a : Length = []` reproduces as `warning: cannot infer element type…` + `All constraints satisfied.` (silent accept — `target/debug/reify check`). *Binding:* β introduces the code in `crates/reify-core/src/diagnostics.rs` (`#[non_exhaustive]` enum, `.with_code(...)` mechanism, ~:156) **and** emits it on the non-matching-kind arm; **boundary test #7 / #7b** assert it fires. Rejection is the deliverable + bound to a test (not motivation-only). | PASS |
| positive resolution (`let xs : List<Length> = []`) | End-to-end capability | all capabilities (resolve annotation type, set elem type) within α+β scope; no downstream dependency | PASS |
| non-regression (`let xs = []` still warns) | End-to-end (invariant) | `expected_type = None` path unchanged (contract §6); boundary test #4 | PASS |
| grammar reality (`let xs : List<Length> = []`, `set {}`, `map {}`) | Grammar-fixture (anti-mismatch) | `tests/prd-gate/fixtures/expected_type_pushdown_let.ri` parses `tree-sitter parse --quiet` exit 0, 0 ERROR (verified); no novel syntax → no grammar-producer task | PASS |
| field-population / numeric floor | — | N/A (no result fields, no numeric bounds) | N/A |

## δ — argument-position push-down + `E_TYPE_UNDETERMINED` *(leaf)*

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| expected-type channel | Capability→producer (anti-inversion) | `producer:task-α` — upstream of δ | PASS |
| call-argument compilation reaches `compile_expr` | Capability→producer (wired) | FunctionCall arguments compiled via `compile_expr` (production path); overload resolution observed live (`firstlen([])` → `no matching overload for firstlen(List<Real>)`) — proves the arg type flows into resolution | PASS |
| `Type::TypeParam(P)` to detect unbound-generic element | Capability→producer | `crates/reify-core/src/ty.rs` TypeParam ~:104; function type-params parse + work (`fn ident<T>(xs:List<T>)` → exit 0 today) | PASS |
| `DiagnosticCode::TypeUndetermined` (new) | Rejection-mechanism (anti-silent-accept, **forward**) | *Current (motivation):* `ident([])` over `fn ident<T>(xs:List<T>)` reproduces as `warning…` + `All constraints satisfied.` (silent default of `T`). *Binding:* δ introduces `TypeUndetermined` (diagnostics.rs) **and** emits it on the unbound-generic arg arm; **boundary test #6** asserts it fires. | PASS |
| positive arg (`firstlen([])`, concrete param) | End-to-end capability | param-type push-down (δ scope) + existing overload resolution; grounded — currently the failing `no matching overload` cascade (`reify check`), which push-down removes | PASS |
| grammar reality (`fn ident<T>(xs:List<T>)`, `firstlen([])`) | Grammar-fixture | `tests/prd-gate/fixtures/expected_type_pushdown_arg.ri` parses exit 0, 0 ERROR (verified; `function_definition` type-params at grammar.js ~:108+) | PASS |
| field-population / numeric floor | — | N/A | N/A |

## ε — integration gate (two-way boundary suite) *(leaf)*

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| both consumer sides exist to test | Capability→producer (anti-inversion) | `producer:task-β`, `producer:task-δ` — both **upstream** of ε (ε depends on both) | PASS |
| the §7 scenarios are reproducible | End-to-end | every §7 row's pre-state reproduced on `main` (#1–#8 fixtures parse + `reify check` baseline captured); ε asserts the post-states | PASS |
| boundary-test suite is the observable signal | Capability→producer (wired) | committed test file under `crates/reify-compiler/tests/` runs in CI (the convention: `compile_source()` + diagnostic-by-`DiagnosticCode` assertions, e.g. `annotation_compile_tests.rs`) | PASS |

---

### Forward-rejection note (G6 branch 4)

`CollectionLiteralKindMismatch` (β) and `TypeUndetermined` (δ) do **not** exist on `main` — the introducing task is their producer. The manifest binds each to a boundary test that asserts the new diagnostic **fires** (tests #7/#7b for β, #6 for δ), and records the reproduced *current* silent-accept as motivation only. This is the correct G6-branch-4 shape for a rejection mechanism that is itself the deliverable (contrast esc-4575, where a silent-accept was logged as test motivation while the rejection capability was assumed but absent).
