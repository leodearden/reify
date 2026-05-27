# PRD: Result<T,E> / fallback — language-level error handling

Status: deferred (spec-gap batch `spec-gap-2026-05-27`, cluster `result-and-fallback`). Decomposition style **B + H** (design-first contract + boundary tests) per `preferences_implementation_chain_portfolio`. Authored 2026-05-27.

Resolves spec §18.4 roadmap item 4 ("`Result<T>` or `fallback` expressions — language-level error handling"). Today (spec §9.6) Reify has **no `Result` type, no `try`/`catch`, no language-level error propagation**: computation failures are eval-**graph** events (`Freshness::Failed { error: ErrorRef }`, `EventKind::Failed`), surfaced through diagnostics, never reified as a language value an `.ri` author can branch on. This PRD designs the language-level error-handling surface.

**Layering decision (resolved this session, see §5 D0):** error handling ships in **two independently-valuable layers**, not one monolith:
- **Layer A — `fallback` + recovery combinators over the *existing* `Option<T>`.** Generic free functions (`unwrap_or`, `or_else`, `or_default`, `map_or`, `is_some`/`is_none`) plus a user-facing `fallback` surface. **Needs no new grammar** (verified §4.4) and **no dependency on data-carrying-enums**. Independently shippable; covers the dominant "absent / lookup-miss / parse-miss → default" use case.
- **Layer B — `Result<T,E>` as a generic data-carrying enum.** `enum Result<T,E> { Ok { value: T }, Err { error: E } }`. This is **built ON the data-carrying-enums feature** (named-field payload) **plus generic enum type parameters** (which DCE explicitly defers — see §6). Carries an error *payload*, where Layer A's `Option` carries only presence/absence. **Hard cross-PRD dependency**; lands later.

The two layers share the §5 graph-vs-language orthogonality rule (D1) and the §7 recovery-combinator contract. **Fork F1 RESOLVED 2026-05-27 (Leo): Layer B IS built — generic data-carrying enums are now real work** (`docs/prds/v0_6/generic-data-carrying-enums.md`, cluster `generic-data-carrying-enums`, tasks 4029–4034), so Layer B's substrate exists. Layer B is decomposed in this PRD (§8.B), filed as `layer:"B"` tasks 4035–4040, each `depends_on` the relevant generic-enum tasks (hard cross-cluster dep) and Layer-A combinator tasks. Layer A remains fully decomposable on existing `Option` substrate; Layer B chains onto the generic-enum landing.

## §1 — Goal & observable surface

What a user can do when **Layer A** lands (the primary observable surface — needs no new grammar):

```reify
structure def Mount {
    // A fallible stdlib op already yields Option today (e.g. an optional lookup,
    // a may-fail parse). Recover with a language-level default instead of an
    // opaque graph-level Failed node.
    param raw : String = "12mm"

    // `unwrap_or` : recovery to a concrete default when the Option is `none`.
    let bore : Length = unwrap_or(parse_length(raw), 6mm)

    // `or_else` : chain a second fallible attempt before defaulting.
    let pin  : Length = unwrap_or(or_else(parse_length(raw), parse_length("4mm")), 4mm)

    // `is_some` / `if` : branch on presence without unwrapping.
    let has_bore : Bool = is_some(parse_length(raw))
}
```

`reify eval mount.ri` reports `bore = 12 mm`, `pin = 12 mm`, `has_bore = true`. Change `raw = "garbage"` → `parse_length` yields `none` → `bore = 6 mm`, `pin = 4 mm`, `has_bore = false`. The key behavioural difference from today: a recoverable miss becomes a **language value the model branches on**, instead of an uncatchable `Freshness::Failed` node (§5 D1). That is the Layer-A end-to-end signal.

What a user can do when **Layer B** lands (now decomposed here, §8.B — built on generic data-carrying enums, tasks 4029–4034):

```reify
// Result carries an ERROR PAYLOAD that Option cannot.
fn parse_length_r(s : String) -> Result<Length, String> { /* ... */ }

structure def Mount {
    param raw : String = "12mm"
    let bore : Length = match parse_length_r(raw) {
        Ok  { value: v } => v,
        Err { error: msg } => 6mm     // recovers; `msg` is in scope, can be surfaced
    }
}
```

`reify eval` reports `bore = 12 mm`; on `raw = "garbage"` it reports `bore = 6 mm` and the `Err` arm's bound `msg` is observable (e.g. via a diagnostic the model emits). This requires the §6 generic-enum substrate (`generic-data-carrying-enums.md`, tasks 4029–4034) and **is** decomposed here as the Layer-B task set (§8.B, tasks 4035–4040).

## §2 — Consumer (G1)

This is a **core-language capability** (a value/library surface + optional thin syntax), not an in-engine seam — no `engine-integration-norm.md` §3 seam is touched; error recovery is a compile-time + `reify-expr` evaluation concern, never a kernel hook.

Named consumers:

1. **User surface — CLI eval (primary G2 signal-bearer, §8 task δ).** `reify check` / `reify eval` over an `.ri` file that calls a fallible op returning `Option`, recovers with `unwrap_or` / `or_else`, and observably yields the default vs. the value. This is the Layer-A end-to-end leaf.
2. **User surface — stdlib `.ri` example** (`examples/m6_fallback_recovery.ri`) exercising recovery + presence-branching, runs in CI.
3. **Existing fallible-op sites that today dead-end at `Freshness::Failed` or silently yield `undef`.** Concrete consumer sites already in the language:
   - **`map[key]` absent** → spec §9.2.6 says this is an "evaluation failure (not undef)" — a fallible op with *no language recovery today*. A `get_or(map, key, default)` combinator (Layer A) is the recovery surface for it.
   - **String→quantity parse** (`parse_length`-style stdlib helpers) — may-fail, currently must return `undef` or trip a `Failed` node; Layer A lets them return `Option` and recover.
   - **Imported-field / numeric ingestion** (cited in the cluster brief) — a parse-from-string / out-of-range ingestion that wants an explicit "bad input → default or flagged" path rather than a graph-fatal failure.
4. **Downstream PRD — `docs/prds/v0_6/data-carrying-enums.md`.** Layer B *consumes* DCE's named-field payload grammar + binding semantics (it is literally a DCE enum). Direction: this PRD (Layer B) consumes DCE. DCE does **not** depend back on this PRD (no reciprocal ambiguity — see §6).
5. **Spec self-consistency — §9.6 / §18.4.** The spec promises this item; the companion-doc task (§8 task ε) updates §9.6 to state the orthogonality rule (D1) and §18.4 to mark item 4 landed (Layer A) / in-progress (Layer B).

No mechanism in this PRD is a producer without one of the above consumers.

## §3 — Background: current implementation chain

Verified 2026-05-27.

| Layer | File / site | Today | Needs (Layer A) |
|---|---|---|---|
| Type | `reify-core/src/ty.rs` `Type::Option(Box<Type>)` | exists | unchanged — Layer A is over `Option` |
| Type | `reify-core/src/ty.rs` `Type::Error` | **type-inference poison sentinel**, NOT user-facing | unchanged — must not be confused with Layer B's error payload (§5 D3) |
| Value | `reify-ir/src/value.rs` `Value::Option(Option<Box<Value>>)` | three-state `some`/`none`/`undef` (spec §9.2.8) | unchanged — combinators consume it |
| Grammar | `tree-sitter-reify/grammar.js` `function_call`, generic `fn foo<T>(...)` | both parse today (verified §4.4) | **no change** for free-function combinators |
| Eval | `reify-expr/src/lib.rs` user-function dispatch | evaluates generic stdlib fns over `Option` | recovery combinators are ordinary stdlib `fn`s — no eval change unless a `fallback` keyword is chosen (fork F2) |
| Graph failure | `reify-ir/src/value.rs` `Freshness::Failed { error: ErrorRef }`; `reify-eval/.../journal.rs` `EventKind::Failed` | graph-level computation failure (panic, kernel error); `ErrorRef` = `EvalError(String)` + optional `DiagnosticCode` | **orthogonal** to language-level recovery (§5 D1) — no change unless the bridge intrinsic (fork F3) is adopted |

For **Layer B** the additional chain (all DCE-owned substrate + a generics extension):

| Layer | Site | Today | Needs (Layer B) |
|---|---|---|---|
| Grammar | `enum_declaration` with named-field payload | FAILS (DCE task α) | DCE α |
| Grammar | `enum_declaration` with **type parameters** `enum Result<T,E> {...}` | FAILS (verified §4.4: 10 ERROR nodes; generics-on-enum unsupported) | **generic data-carrying enums** — DCE §10 explicitly defers this |
| Grammar | `match_pattern` named-field binding `Ok { value: v }` | FAILS (DCE task β) | DCE β |
| Type | `Type` has no `Result` | — | `Result<T,E>` resolves as the generic enum type; `Result<L,String>` type-annotation **already parses** in type position (verified §4.4: the `type_arg_list` is fine; only the *body* construction failed) |

## §4 — Sketch of approach

### 4.1 Layer A — recovery combinators over `Option<T>` (free functions, no new grammar)

Per GR-040 (no method-call syntax), all combinators are **free functions** `combinator(subject, ...)`, declared as generic stdlib `fn`s in a new `crates/reify-compiler/stdlib/option_recovery.ri` (or `prelude`-tier if intrinsic; tactical §11 Q1). Minimum set:

```reify
// (illustrative signatures; all parse today — verified §4.4 raf-8/raf-11)
fn unwrap_or<T>(o : Option<T>, dflt : T) -> T            // none → dflt, some(x) → x
fn or_else<T>(o : Option<T>, alt : Option<T>) -> Option<T> // none → alt, else o
fn or_default<T>(o : Option<T>, dflt : T) -> T            // alias of unwrap_or (naming §11 Q2)
fn map_or<T, U>(o : Option<T>, dflt : U, f : (T) -> U) -> U // none → dflt, some(x) → f(x)
fn is_some<T>(o : Option<T>) -> Bool                      // presence predicate
fn is_none<T>(o : Option<T>) -> Bool
fn get_or<K, V>(m : Map<K, V>, key : K, dflt : V) -> V    // recovers the §9.2.6 map-miss "evaluation failure"
```

These are pure, eagerly evaluated, `undef`-propagating (§5 D2). `unwrap` (no default — partial) is **out of scope** (§10): an unwrap-on-`none` would itself be a fallible op, re-introducing exactly the failure this PRD recovers from.

### 4.2 Layer A — the `fallback` user-facing surface (fork F2)

The spec names "`fallback` expressions". Two surfaces achieve the same recovery semantics; **F2 picks one**:
- **F2-a (default, recommended) — `fallback` is a free function** `fallback(o, dflt)`, an alias / friendlier spelling of `unwrap_or`. **Zero new grammar.** The keyword count (spec §17, 46 keywords) is unchanged; "fallback" stays a normal identifier-named stdlib fn.
- **F2-b — `fallback` is an infix keyword operator** `o fallback dflt`. Reads naturally but is **net-new grammar** (verified §4.4 raf-2: `x fallback 0mm` → 3 ERROR nodes; no production today) and adds keyword #47. A grammar prerequisite task (G3 path-b) would gate it.

Default F2-a keeps Layer A grammar-free and fully decomposable now. F2-b is a deferred ergonomics upgrade.

### 4.3 Layer B — `Result<T,E>` as a generic data-carrying enum (decomposed here, §8.B)

```reify
enum Result<T, E> { Ok { value: T }, Err { error: E } }
```

A `Result` value is constructed `Ok { value: x }` / `Err { error: e }` (DCE named-field construction) and consumed by `match` with payload binding (DCE pattern grammar). Recovery combinators generalize: `unwrap_or<T,E>(r : Result<T,E>, dflt : T) -> T`, `is_ok`, `map_err`, etc. This is *purely* a DCE enum plus type parameters — **no new value kind**; `Result` is a library/prelude enum, not an intrinsic (fork F4). It is the error-*payload*-carrying sibling of Layer A's `Option`.

### 4.4 Grammar reality check (G3) — fixtures (tree-sitter 0.26.8, 2026-05-27)

Per the silent-misparse trap, the signal is the **CST ERROR-node count**, not the exit code (`tree-sitter parse -q` rc is working-dir-sensitive; run from `tree-sitter-reify/`).

| Fixture | Syntax | ERROR nodes | Verdict |
|---|---|---|---|
| `raf-0-baseline-enum.ri` | bare enum + struct | **0** | clean floor |
| `raf-8-unwrap-or-call.ri` | `unwrap_or(parse_length(s), 0mm)`, `is_ok(...)` | **0** | **Layer A combinators parse today** |
| `raf-11-generic-fn.ri` | `fn unwrap_or<T>(o : Option<T>, dflt : T) -> T { dflt }` | **0** | **generic free-fn declaration parses today** |
| `raf-10-option-fallback-call.ri` | `or_default(some(5mm), 0mm)`, `unwrap_or(none, 0mm)` | **0** | Layer A over Option literals parses |
| `raf-13-if-determined.ri` | `if is_some(o) then unwrap(o) else 0mm` | **0** | presence-branch parses |
| `raf-2-fallback-binop.ri` | `x fallback 0mm` (infix kw) | **3** | F2-b needs grammar |
| `raf-3-question-postfix.ri` | `parse(s)?` (postfix `?`) | **3** | `?`-propagation needs grammar (out of scope §10) |
| `raf-7-result-concrete-decl.ri` | `enum LengthResult { Ok {value:Length}, Err {error:String} }` | **9** | needs DCE named-field payload (DCE α) |
| `raf-1-result-generic-decl.ri` | `enum Result<T,E> { Ok {value:T}, Err {error:E} }` | **10** | needs DCE α **+ generic enums** (DCE-deferred) |
| `raf-9-result-typeann.ri` | `-> Result<Length, String>` (type position) + `Err {...}` body | **4** | the `type_arg_list` parses; only the named-field *construction* body fails — i.e. `Result<…>` as a **type** is fine; only Layer B's value side needs DCE |
| `raf-5-match-result.ri` | `match … { Ok {value:v} =>, Err {error:e} => }` | **9** | needs DCE payload-binding pattern (DCE β) |
| `raf-12-option-match.ri` | `match … { some(v) =>, none => }` | **5** | Option's `some(v)` pattern gap (DCE F4) — Layer A uses combinators, not this match form, so unaffected |

**G3 resolution.**
- **Layer A: no novel grammar.** Combinators + `fallback`-as-function (F2-a) parse today → `grammar_confirmed=true` on every Layer-A leaf. (Only F2-b would add a grammar prerequisite — deferred.)
- **Layer B: substrate does not exist** → DCE named-field grammar (α/β) **plus** generic-enum grammar (DCE-deferred §10). Both are hard prerequisites for any Layer-B task. Under the default F1 (Layer B deferred to a follow-up PRD), no Layer-B grammar task is filed in this batch; the follow-up PRD owns it and depends on the DCE-generics extension.

## §5 — Resolved design decisions

- **D0 — Two layers, Option-based recovery first.** Error handling is delivered as Layer A (recovery over the existing `Option<T>`, no new substrate) then Layer B (`Result<T,E>` carrying an error payload, built on generic DCE). The split is load-bearing: Layer A ships value *now* against existing substrate; Layer B waits on substrate that is itself deferred. (See fork F1 for whether Layer B is decomposed here at all.)
- **D1 — Graph-level `Freshness::Failed` and language-level recovery are ORTHOGONAL layers.** This is the §9.6 interaction clarification the cluster brief flags as load-bearing. A graph-`Failed` node (panic, kernel hard-error, cancellation) is **not** automatically a language `none`/`Err`, and a language `none`/`Err` does **not** mark the node `Failed`. They occupy different layers: `Freshness::Failed` is an *evaluation-graph lifecycle* state (uncatchable from `.ri`, surfaced via diagnostics, §9.6 today); `Option`/`Result` are *language values* an author constructs and branches on. A fallible *language* op (parse-miss, lookup-miss) returns `none`/`Err` — a determined value — and never trips `Failed`. A genuine *computation* failure (kernel panic) stays `Failed` and is not reifiable into a `Result` by default. **Crossing the layers is opt-in only (fork F3's bridge intrinsic), never implicit.** Rationale: keeping them orthogonal preserves §9.6's "computation failures are graph events" invariant (existing solver/cache/freshness machinery untouched) while adding the language surface; an implicit `Failed→Err` bridge would make every kernel op's failure catchable, a far larger and riskier semantic change.
- **D2 — Recovery combinators are pure, eager, and `undef`-propagating (§9.2.7).** `unwrap_or(undef, dflt)` is `undef` (the whole `Option` value is undef — existence not yet decided, §9.2.8), NOT `dflt`. Only a **determined `none`** recovers to the default. This mirrors `if undef then … else …` = `undef` (§9.2.4) and the §9.2.8 four-state `Option` table: `undef`-of-`Option<T>` ("existence not decided") is distinct from determined `none` ("absent"). Recovery acts on the *absent* state, not the *undecided* state.
- **D3 — `Type::Error` (poison sentinel) is NOT the Layer-B error payload.** `ty.rs`'s `Type::Error` is the type-inference poison value (cascading-diagnostic suppressant). Layer B's `Err { error: E }` payload type `E` is a normal user type (commonly `String`). The two never unify; no code conflates them. (Guard against the naming collision at impl — §11 Q3.)
- **D4 — Determinacy of `Err` / the recovered value (G6).** `unwrap_or(some(x), d)` = `x`; `unwrap_or(none, d)` = `d`; `unwrap_or(undef, d)` = `undef` (D2). The selector is the `Option` *tag* (`some`/`none`/undef-of-Option), exactly the §9.2.5/§9.2.8 discriminant rule — combinators add no new determinacy semantics, they reuse the existing match/Option determinacy. For Layer B, `match` on `Result` selects by the `Ok`/`Err` tag per DCE's INV-3 (tag-only selection); a determined-tag-with-`undef`-payload selects the arm and binds `undef` (DCE D2/INV-4).
- **D5 — `Result` is a library/prelude enum, not a `Value` intrinsic (fork F4 default).** Layer B adds **no new `Value` variant**; `Result` is an ordinary (generic, data-carrying) `enum`. This keeps the value model singular and means Layer B is "just" DCE-generics + a prelude enum + combinators. (Alt F4-b makes `Result` compiler-intrinsic like `Option`; trade-offs in forks.)
- **D6 — `fallback` semantics = recover-to-default on the absent/error tag.** Whether spelled `fallback(o, d)` (F2-a) or `o fallback d` (F2-b), the meaning is `unwrap_or`. No exception-style unwinding, no `?`-propagation (those are §10 out-of-scope).
- **D7 — Backward compatibility.** Existing `Option` usage (`some`/`none`/`undef`, the §9.2.8 table) is untouched; combinators are additive stdlib fns. No existing fixture changes behaviour. `Freshness::Failed` / `EventKind::Failed` machinery is byte-for-byte unchanged under D1 (unless fork F3 adds the opt-in bridge).

## §6 — Cross-PRD / cross-cluster relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/data-carrying-enums.md` | this (**Layer B**) **consumes**, that **produces** | named-field variant payload grammar/IR (DCE α/γ), payload-binding `match` pattern (DCE β/ε), payload-binding eval (DCE ζ) | **DCE owns** the payload mechanism; Layer B owns only the `Result` *prelude enum + combinators* on top | **deferred** (Layer B not in this batch under default F1) — when Layer B is decomposed, its tasks `depends_on` DCE α, β, γ, ε, ζ |
| `docs/prds/v0_6/generic-data-carrying-enums.md` — **generic enums** | this (**Layer B**) **consumes**, that **produces** | type parameters on `enum` declarations (`enum Result<T,E> {…}`), generic-variant construction inference, type-preserving generic pattern match | **generic-data-carrying-enums owns it** (authored 2026-05-27; tasks 4029–4034) | **RESOLVED / wired** — Layer B's substrate now exists. Layer-B α (4035 `Result` prelude enum + `Ok`/`Err` construction) `depends_on` generic α 4029 (grammar) + generic γ 4031 (construction inference); Layer-B β (4036 match-on-`Result`) `depends_on` generic δ 4032 (typed binders) + generic ε 4033 (eval); Layer-B ε (4039 end-to-end) `depends_on` generic ε 4033. F1 flipped to "Layer B built". |
| `docs/prds/v0_3/structure-instance-runtime.md` (GR-001) | independent | — | n/a | Layer A is over `Option` (no struct ctor); Layer B's `Result` is a DCE enum with inline payload (DCE F1 default = inline name→Value map, no GR-001 edge). **No edge.** |
| spec §9.6 / §18.4 | this **corrects** | the "no language-level error handling" prose + roadmap item 4 | this-prd | companion task §8 ε |

**Seam ownership statement (G4).** Layer A owns its combinators outright (no cross-PRD seam — pure stdlib over existing `Option`). Layer B's *payload + pattern* machinery is **owned by DCE**; Layer B owns only the `Result` enum declaration + `Result`-specialized combinators sitting on DCE's mechanism. **The generic-enum substrate is owned by NEITHER existing PRD** — it is a gap (a generic-data-carrying-enums PRD must be authored before Layer B can land). No contested-ownership pair from `phase-3-breadcrumb-map.md` §3 is touched.

**Tuple constraint (Leo, this session):** tuples are NOT being added. Neither layer introduces a tuple value/type. Layer B's `Result<T,E>` payloads are DCE **named-field** (`Ok { value: T }`, `Err { error: E }`), never positional, never a `(T,E)` tuple. Confirmed consistent with DCE's named-field-only decision.

**Cross-PRD edges (WIRED 2026-05-27 — F1 resolved to "Layer B built"):** the Layer-B tasks (`layer:"B"`, 4035–4040) carry these hard cross-cluster edges, all wired via `add_dependency`:
- **4035** (Layer-B α — `Result` prelude enum + `Ok`/`Err` construction) → **4029** (generic α grammar), **4031** (generic γ construction inference).
- **4036** (Layer-B β — match-on-`Result`) → **4032** (generic δ typed binders), **4033** (generic ε eval).
- **4037** (Layer-B γ — `Result` combinators) → **3979** (Layer-A α combinator resolver), **3981** (Layer-A β combinator eval).
- **4039** (Layer-B ε — end-to-end) → **4033** (generic ε eval) + all Layer-B intermediates.

The generic-enum tasks (4029–4034) transitively depend on the named-field DCE tasks (3936/3940/3942/3944/3946), so Layer B's full prerequisite chain is DCE → generic-enums → Layer B. The Layer-A tasks (3979–3989) are unchanged.

## §7 — Contract section (B+H)

The seam is between `reify-compiler` (resolves combinator calls; type-checks generic instantiation `unwrap_or<Length>`) and `reify-expr` (evaluates the combinators over `Value::Option`). Both sides face the same recovery semantics.

### 7.1 Recovery-combinator contract (the seam surface)

For every recovery combinator `C(subject, …)`:

- **C-1 (tag-driven recovery).** Result selected purely by the subject's tag: `Option` → {`some`, `none`, undef-of-Option}; `Result` (Layer B) → {`Ok`, `Err`, undef}. `some(x)`/`Ok{value:x}` yields the carried value; `none`/`Err` yields the recovery; undef-of-the-type yields `undef` (D2/D4).
- **C-2 (purity & eagerness).** Combinators are pure stdlib `fn`s; arguments are eagerly evaluated (consistent with the eval model — no thunks). `unwrap_or(o, expensive())` evaluates `expensive()` whether or not `o` is `some`. (A lazy `or_else_with(o, () -> T)` is §10 out-of-scope.)
- **C-3 (type discipline).** `unwrap_or<T>(Option<T>, T) -> T`: the default's type must unify with the option's element type; mismatch → compile diagnostic `E_FALLBACK_TYPE` (illustrative §7.3). Generic instantiation reuses the existing generic-fn resolver (verified parses, §4.4 raf-11).
- **C-4 (no graph-failure capture).** Combinators consume `Value::Option` (a determined language value); they never inspect or catch `Freshness::Failed` (D1). A combinator applied to the output of a graph-`Failed` node sees that node's last value per normal freshness rules — it does **not** convert `Failed` into `none`.

### 7.2 Invariants

- **INV-1 (orthogonality).** No combinator reads `Freshness`; no combinator emits `EventKind::Failed`. Verified by a boundary test: a fixture whose only failure is a graph-`Failed` node is **not** recovered by `unwrap_or` (the `Failed` propagates as today, §9.6). (D1.)
- **INV-2 (undef passthrough).** `C(undef, …) == undef` for every combinator (D2); only a determined `none`/`Err` recovers. Pinned by unit + eval test.
- **INV-3 (back-compat).** Existing `Option`/`some`/`none`/`undef` behaviour (§9.2.8) and all `Freshness::Failed` machinery are unchanged. Existing fixtures stay green.
- **INV-4 (no new Value variant, Layer A).** Layer A adds zero `Value`/`Type` variants; it is stdlib `fn`s over `Value::Option`. (Layer B adds the `Result` prelude enum but still no new `Value` variant under D5/F4 default.)

### 7.3 Error semantics (user-visible diagnostics — G2 leaf signals)

| Code (illustrative) | Trigger | Where |
|---|---|---|
| `E_FALLBACK_TYPE` | `unwrap_or(parse_length(s), "x")` — default type ≠ option element type | compiler, combinator call |
| `E_FALLBACK_ARITY` | wrong arg count to a combinator | compiler (existing fn-arity path) |
| (existing) graph `Failed` | a kernel/eval computation failure | eval graph (unchanged, D1) — NOT a combinator concern |

## §8 — Decomposition plan (DAG; not yet filed) — Layer A only (default fork F1)

**B + H.** Layer A introduces no novel grammar (G3 clear), so there is no grammar-prerequisite phase. The shape is: combinator library → compiler resolution/type-check → eval → end-to-end consumer leaf (integration gate) → companion docs. Greek labels; real IDs at decompose. **All Layer-A leaves `grammar_confirmed=true`.**

### Phase 1 — Combinator library + resolution (intermediate)

- **Task α — Recovery-combinator stdlib declarations + compiler resolution + type-check.**
  - Add `crates/.../stdlib/option_recovery.ri` declaring the §4.1 generic combinators (`unwrap_or`, `or_else`, `or_default`, `map_or`, `is_some`, `is_none`, `get_or`); wire them through the generic-fn resolver; emit `E_FALLBACK_TYPE` on default/element type mismatch (C-3). `fallback` = alias of `unwrap_or` per F2-a default.
  - **Observable signal (intermediate → unlocks β):** unit tests in `reify-compiler` pin: `unwrap_or<Length>` resolves and type-checks against `Option<Length>` + `Length` default; `unwrap_or(opt, "x")` against `Option<Length>` emits `E_FALLBACK_TYPE`; `is_some<T>` resolves to `Bool`. **Consumer:** §8 tasks β, δ.
  - **Crates:** reify-compiler (stdlib, fn resolution, type_resolution), stdlib `.ri`. **Prereqs:** none. `grammar_confirmed=true`.

### Phase 2 — Eval (intermediate → integration gate)

- **Task β — Recovery-combinator evaluation over `Value::Option`.**
  - Implement combinator eval in `reify-expr` (or as stdlib `.ri` bodies if expressible — tactical §11 Q1): tag-driven recovery (C-1), undef passthrough (C-2/INV-2), purity/eagerness (C-2). Ensure no `Freshness` read (INV-1).
  - **Observable signal (intermediate):** `reify-expr` eval unit tests pin: `unwrap_or(some(5mm), 0mm) == 5mm`; `unwrap_or(none, 0mm) == 0mm`; `unwrap_or(undef, 0mm) == undef` (INV-2); `or_else(none, some(3mm)) == some(3mm)`; `get_or(map{...}, absent_key, d) == d` (recovers the §9.2.6 map-miss). **Unlocks:** δ. **Consumer:** §8 task δ.
  - **Crates:** reify-expr. **Prereqs:** α.

### Phase 3 — Orthogonality boundary test (LEAF — the D1 pin)

- **Task γ — Graph-failure vs. language-recovery orthogonality boundary test (B+H boundary gate).**
  - The two-way boundary test for D1/INV-1: (a) a determined-`none` language value is recovered by `unwrap_or` to the default; (b) a genuine graph-`Failed` node (synthetic forced-fail, per `engine_admin.rs` panic-eval path) is **NOT** recovered by `unwrap_or` — it stays `Freshness::Failed` and surfaces via the existing diagnostic path (§9.6). This pins the orthogonality invariant from both sides.
  - **Observable signal (LEAF):** an engine/integration test in `reify-eval/tests/` (or `reify-cli`) asserts: language-`none` + `unwrap_or` → default value, freshness `Final`; graph-`Failed` output consumed by `unwrap_or` → the consumer cell is still `Failed` (not `none`, not the default), and the `Failed` diagnostic is emitted. This is the D1 contract, observable through the engine's own freshness/diagnostic read path (not by peeking at internals). **Prereqs:** β.
  - **Crates:** reify-eval (tests), reify-expr. `grammar_confirmed=true`.

### Phase 4 — End-to-end consumer (LEAF — primary integration gate)

- **Task δ — CLI eval recovery example (THE integration gate / primary user-observable signal).**
  - `examples/m6_fallback_recovery.ri`: a `structure def` calling a fallible stdlib op that yields `Option`, recovering with `unwrap_or` / `or_else`, and a presence branch with `is_some`. `reify eval` reports the recovered values.
  - **Observable signal (LEAF — primary, §1):** `reify eval examples/m6_fallback_recovery.ri` reports `bore = 12 mm`, `pin = 12 mm`, `has_bore = true` for valid input; switching the source string to unparseable input reports `bore = 6 mm`, `pin = 4 mm`, `has_bore = false`. Example runs in CI. (CLI output difference — user-observable leaf; this is the §1 signal and the B+H integration gate — α, β are its intermediates; γ is the paired orthogonality boundary leaf.)
  - **Crates:** reify-cli (eval path, no change expected), examples/, reify-expr. **Prereqs:** α, β.

### Phase 5 — Companion corrections (doc; independent)

- **Task ε — Spec §9.6 / §18.4 update for Layer A + the D1 orthogonality rule.**
  - Update §9.6: add the orthogonality rule (graph-`Failed` and language `Option`-recovery are distinct layers, D1); state that fallible *language* ops return `Option` and recover via combinators while *computation* failures stay graph-`Failed`. Update §18.4 item 4: Layer A (Option-recovery / `fallback`) **landed**; `Result<T,E>` (Layer B) **deferred to a follow-up PRD gated on generic data-carrying enums**. Cross-reference DCE for the planned Layer B.
  - **Observable signal:** `docs/reify-language-spec.md` updated; the `raf-*` recovery fixtures referenced; no code change; doc lint passes.
  - **Crates:** none (docs). **Prereqs:** δ.

### Dependency view (Layer A)

```
α ─┬─→ β ─┬─→ γ
   │      └─→ δ ─→ ε
   └────────────→ δ
```

(α unlocks both β and δ's type side; β unlocks γ and δ's eval side; δ is the integration gate; γ is the orthogonality boundary leaf; ε documents what landed.) No out-of-batch prereqs for Layer A (no DCE edge — Layer A is over `Option`).

## §8.B — Decomposition plan — Layer B (`Result<T,E>`) (FILED 2026-05-27; tasks 4035–4040)

**B + H.** Layer B is built on the generic-data-carrying-enums substrate (tasks 4029–4034) + the Layer-A combinator machinery (3979/3981). `Result<T,E>` is a generic data-carrying **prelude enum** (fork F4-a resolved: prelude enum, not a `Value` intrinsic — with generic enums now real, `Result` is "just" a generic DCE enum + combinators; singular value model, no new `Value` variant). Greek labels prefixed `B-`; real IDs filed.

- **Task B-α (4035) — `Result<T,E>` prelude enum + `Ok`/`Err` construction.** Declare `enum Result<T,E> { Ok {value:T}, Err {error:E} }` as a prelude enum; exercise `Ok`/`Err` named-field construction with type-arg inference (reuses generic γ 4031). **Signal:** `reify check` — prelude `Result` in scope; `let r = Ok { value: 5mm }` checks clean (`r : Result<Length, ?>`); `param r : Result<Force, String> = Ok { value: 5mm }` emits the type-param-aware payload-type diagnostic. **Prereqs:** generic α 4029, generic γ 4031. `grammar_confirmed=false`.
- **Task B-β (4036) — match-on-`Result` with typed `Ok`/`Err` binders.** `match r { Ok { value: v } => …, Err { error: msg } => … }` with `v : T`-substituted, `msg : E`-substituted (reuses generic δ 4032, DCE ζ 3946 eval). **Signal:** `reify eval` of a `Result<Length, String>` match reports the `Ok` value / `Err` recovery; `Err` binds `msg : String` (a body doing `msg + 1mm` is a type error); `Ok`/`Err` exhaustiveness enforced. **Prereqs:** 4035, generic δ 4032, generic ε 4033. `grammar_confirmed=false`.
- **Task B-γ (4037) — `Result`-specialized recovery combinators** (`unwrap_or`/`is_ok`/`is_err`/`map_err`/`or_else` over `Result`). Generalizes the Layer-A combinators to `Result<T,E>`; tag-driven recovery (C-1: `Err`→default, `undef`-of-`Result`→`undef`, D2/D4); reuses the Layer-A resolver/eval paths. **Signal:** unit/eval tests — `unwrap_or(Ok{value:5mm},0mm)==5mm`; `unwrap_or(Err{error:"x"},0mm)==0mm`; `unwrap_or(undef,0mm)==undef`; `is_ok`/`is_err` predicates; `map_err` applies a lambda to the error payload. **Prereqs:** 4035, Layer-A α 3979, Layer-A β 3981. `grammar_confirmed=true` (combinator calls parse today).
- **Task B-δ (4038) — `fallback` propagation over `Result`** (`fallback(r, dflt)` = `unwrap_or` on the `Err` tag, D6; chained `or_else` over `Result`). **`?`-postfix is a separate fork (F-Question, default deferred)** — `parse(s)?` needs net-new grammar (raf-3: 3 ERROR) + early-return control flow the pure-functional model lacks; this task ships combinator/`fallback` propagation only. **Signal:** `reify eval` — `fallback(parse_length_r(raw), 6mm)` recovers to 6mm on `Err`, parsed value on `Ok`; `or_else` tries a second `Result` on the first's `Err`; runs in CI. **Prereqs:** 4035, 4036, 4037. `grammar_confirmed=true`.
- **Task B-ε (4039) — CLI eval `Result` example (integration gate, primary Layer-B signal).** `examples/m6_result_recovery.ri`: `fn parse_length_r(s : String) -> Result<Length, String>`, consumed by `match` with payload binding and by `fallback`. **Signal:** `reify eval` reports `bore = 12 mm` for `raw="12mm"`; on `raw="garbage"` reports `bore = 6 mm` with the `Err` payload `msg` observable (the capability `Option` lacks — an error *message*). Runs in CI. (B-α/β/γ/δ are its intermediates.) **Prereqs:** 4035, 4036, 4037, 4038, generic ε 4033. `grammar_confirmed=false`.
- **Task B-ζ (4040) — spec §9.6 / §18.4 update: `Result<T,E>` landed.** Flip §18.4 item 4 to Result-landed (generic prelude enum + `Ok`/`Err` + match + combinators + `fallback`); §9.6 notes Layer A+B both present, orthogonal to graph-`Failed` (D1). **Signal:** spec updated, `m6_result_recovery.ri` referenced, doc lint passes, no code change. **Prereqs:** 4039. `grammar_confirmed=true` (docs).

### Dependency view (Layer B)

```
                generic-enums 4029,4031 ─→ B-α 4035 ─┬─→ B-β 4036 ─┐
                generic-enums 4032,4033 ─→ B-β 4036   │            │
   Layer-A 3979,3981 ─────────────────→ B-γ 4037 ←────┤            ├─→ B-ε 4039 ─→ B-ζ 4040
                                          B-δ 4038 ←───┴────────────┘   ↑
                                          (B-δ ← B-α,B-β,B-γ)  generic-enums 4033 ─┘
```

(DCE → generic-enums → Layer B is the full prerequisite chain. Every Layer-B leaf has a *filed* prerequisite — no assumed substrate; G3 anti-starvation satisfied.)

## §9 — Premise validation (G6)

Every §8 leaf signal classified:

- **δ primary signal — end-to-end capability** ("`bore = 12 mm` / on bad input `bore = 6 mm`"). Trace: requires (a) generic combinator resolution + type-check [α], (b) combinator eval over `Value::Option` [β], (c) a fallible stdlib op yielding `Option` (existing or trivially-added parse helper — verified `Option` + generic fns exist today). Every capability is in δ's dependency set (α, β → δ); none is owned by a task that depends on δ. **Passes** the dependency-set trace. The arithmetic premise (`unwrap_or(some(12mm), 6mm) == 12mm`) is trivial value selection — no numeric accuracy claim. **Achievable.**
- **γ orthogonality signal — end-to-end capability** (graph-`Failed` not recovered; language-`none` recovered). Trace: requires (a) combinator eval [β], (b) the existing forced-`Failed` eval path (verified at `engine_admin.rs:1641` / `freshness_walk.rs:262`). Both in γ's dependency set. The premise (a graph-`Failed` node stays `Failed` through `unwrap_or`) is a **direct consequence of D1/INV-1**, not a numeric claim — combinators provably don't read `Freshness` (the contract). **Consistent and achievable.**
- **β signals — value-equality unit tests** (`unwrap_or(some(5mm),0mm)==5mm`, `unwrap_or(undef,0mm)==undef`). No accuracy/exactness premise; pure tag-selection semantics already in the `Option`/match model (§9.2.8). The `undef` passthrough (D2) is validated against the §9.2.4/§9.2.8 existing rules — internally consistent, no new spec claim. **Pass.** (These are *intermediate* signals, roped to the δ/γ leaves — not synthetic-input leaf closures.)
- **α signals — resolution/type-check + `E_FALLBACK_TYPE` diagnostic emission.** No quantitative premise; pass trivially. Codes illustrative (§7.3, tactical §11 Q3). Intermediate, unlocking β/δ.

No leaf asserts an accuracy bound, closed-form reproduction, or a capability owned downstream. The one genuinely new semantic claim — **D1 orthogonality** — is *resolved here* and pinned by the γ boundary test rather than baked into a numeric RED fixture. **G6 clear.**

## §10 — Out of scope for this PRD

- **`Result<T,E>` decomposition (Layer B), under the default fork F1.** Layer B is fully *specified* (§4.3, §5 D5, §6, §7.1 generalization) but **not decomposed/filed here** — it depends on generic data-carrying enums, which do not exist and are DCE-deferred. A follow-up PRD owns Layer B once the generic-DCE substrate is authored. (Flip F1 to include it here only if Leo wants the DCE + generic-enum prerequisites wired now.)
- **Generic / type-parameterized enums (`enum Result<T,E> {…}`).** The substrate for Layer B; owned by neither this PRD nor DCE today. Must be authored as a generic-data-carrying-enums PRD before Layer B lands.
- **`?`-style error propagation / early return.** `parse(s)?` (verified no grammar, §4.4 raf-3) — exception-style unwinding is a much larger control-flow change; deferred. Recovery is explicit-combinator only.
- **`try`/`catch` / exception unwinding.** Not added; inconsistent with the pure-functional eval model (§9.6's premise).
- **Catching graph-level `Freshness::Failed` into a language `Result`/`Option` (the bridge intrinsic).** Default: NOT added (D1 orthogonality). A `catch_failed(expr) -> Option<T>` / `try_eval` bridge that reifies a graph-`Failed` into a language value is **DESIGN FORK F3** — deferred unless Leo opts in. It is the larger, riskier semantic change (makes every kernel failure catchable).
- **Partial `unwrap` (no default).** An unwrap-on-`none` is itself fallible — re-introduces the failure this PRD removes. Recovery always supplies a default or an alternative.
- **Lazy combinators** (`or_else_with(o, () -> T)` with a thunk default). Eager only in v1 (C-2); lazy variants deferred.
- **`fallback` as an infix keyword (F2-b).** Default ships `fallback` as a free function (F2-a, no grammar); the infix-keyword surface is a deferred ergonomics upgrade gated on a grammar task.

## §11 — Open questions (tactical; decide at impl)

1. **Combinator implementation site** — pure stdlib `.ri` `fn` bodies vs. `reify-expr` intrinsics. If the combinators are expressible as ordinary `.ri` over `match`/`if`, prefer stdlib (less compiler surface). `unwrap_or` likely needs the `some(v)`/`none` match form — which is the DCE-F4 `some(IDENT)` pattern gap (verified fails today, §4.4 raf-12) — so the **intrinsic / `reify-expr` route is the likely default** for v1 (avoids depending on the Option-pattern grammar gap). Decide at β.
2. **Combinator naming** — `unwrap_or` vs `or_default` vs `value_or`; `fallback` as the canonical alias. Decide at α against stdlib naming conventions.
3. **Diagnostic codes/strings** (`E_FALLBACK_TYPE`, `E_FALLBACK_ARITY` illustrative). Decide at α against the diagnostic-code registry. Guard the `Type::Error` naming collision (D3).
4. **`get_or` for `Map`** — whether the map-miss recovery is a distinct `get_or` combinator or a general `Option`-returning `try_get(map,key) -> Option<V>` + `unwrap_or`. The latter is more composable; decide at α/β.

## DESIGN FORKS FOR LEO

> AskUserQuestion does not route to this session; defaults below are reasoned and the PRD is internally consistent under them. Each fork notes the recommended default and the lean.

### F1 — Is `Result<T,E>` (Layer B) decomposed in THIS PRD? *(RESOLVED 2026-05-27: YES — Layer B built)*

The load-bearing fork, **now resolved**. `Result<T,E>` is naturally a generic data-carrying enum (`enum Result<T,E> { Ok {value:T}, Err {error:E} }`). It needs two substrate pieces: (1) DCE named-field payload (DCE α/β/γ/ε/ζ — tasks 3936/3938/3940/3944/3946), and (2) **generic enum type parameters** — **which now exist as filed, owned work**: `docs/prds/v0_6/generic-data-carrying-enums.md` (tasks 4029–4034), authored this session because Leo decided to add generic enums + `Result<T,E>`.

- **RESOLVED — Layer B is decomposed here** (§8.B, tasks 4035–4040), each `depends_on` the generic-enum tasks (hard cross-cluster dep) + Layer-A combinator tasks. The earlier "defer until generics are filed, owned work" condition is **met** — generic enums are now a filed PRD with a wired DAG, so this is not a dependency on a fiction (G3 satisfied: the substrate is queued as explicit prerequisite tasks, not assumed). Layer A still ships independently on `Option`; Layer B chains onto the generic-enum landing.
- **Earlier default (deferred) is OBSOLETE.** Recorded for provenance: the fork previously leaned "defer Layer B" specifically because the generic-enum substrate was unfiled/unowned. That precondition no longer holds.
- **Impact:** Layer A is self-contained + grammar-free; Layer B is gated on the DCE → generic-enums → Layer B chain. The G3 anti-starvation discipline is honoured because every Layer-B leaf has a *filed* prerequisite task, not an assumed capability.

### F2 — `fallback` surface: free function or infix keyword? *(default: free function F2-a — recommended)*

- **Default F2-a — `fallback(o, dflt)` free function** (alias of `unwrap_or`). **Zero new grammar** (verified parses), no new keyword, consistent with GR-040 (no method syntax, free-function idiom). Ships in Layer A's batch as-is.
- **Alt F2-b — `o fallback dflt` infix keyword.** Reads more naturally; **net-new grammar** (verified §4.4 raf-2: 3 ERROR nodes) + keyword #47. Adds a G3 grammar-prerequisite task to the DAG.
- **Impact:** F2-a keeps Layer A grammar-free and fully decomposable now; F2-b is a deferred ergonomics upgrade. *Lean: F2-a; revisit the infix form once the recovery library has usage.*

### F3 — Bridge graph-`Freshness::Failed` into a catchable language value? *(default: NO — orthogonal layers, recommended)*

The §9.6 interaction fork flagged in the cluster brief. Today a graph-`Failed` (kernel panic, hard solver error) is uncatchable from `.ri`.

- **Default F3-a — NO bridge; keep the layers orthogonal (D1).** A language fallible op returns `none`/`Err`; a genuine computation failure stays graph-`Failed`. Crossing is not possible. **Pro:** preserves §9.6's invariant untouched; smallest blast radius; no kernel-failure-handling redesign. **Con:** a kernel op that hard-fails can't be recovered in-language (must be surfaced as a diagnostic and fixed upstream).
- **Alt F3-b — add an opt-in bridge intrinsic** `catch_failed(expr) -> Option<T>` (or `try_eval`) that reifies a graph-`Failed` into a determined `none`/`Err`. **Pro:** lets the model recover from a failing kernel op (e.g. a boolean-fuse that fails on degenerate geometry → fall back to a simpler shape). **Con:** makes *every* kernel op's failure catchable — a large semantic surface (which failures are catchable? cancellation? OOM?), interacts with the freshness/cache/diagnostic machinery, and blurs the clean §9.6 graph-event model. A separate, larger PRD if wanted.
- **Impact:** D1 is written assuming F3-a. Choosing F3-b would add a bridge-intrinsic task + a semantics section on which `Failed` causes are catchable. *Lean: F3-a — keep orthogonal; the catch-failure use case is real but deserves its own PRD with a careful failure-cause taxonomy.*

### F-Question — postfix `?`-propagation over `Result`? *(default: DEFERRED — recommended; live now that Layer B is built)*

The brief asks for "`?`/fallback propagation over `Result`". **`fallback`/combinator propagation IS shipped** in Layer-B δ (4038). The postfix `?` operator (`parse(s)?` → early-return the `Err`) is the open part.

- **Default F-Q-a — DEFER `?`-postfix.** `parse(s)?` needs net-new grammar (verified §4.4 raf-3: 3 ERROR nodes) **and** an early-return / propagation control-flow mechanism that the pure-functional eval model does not have (there is no statement sequence to "return early" from). Recovery in v1 is explicit-combinator only (`fallback`/`unwrap_or`/`or_else` over `Result`, task 4038). **Pro:** keeps the eval model pure; no new control flow; Layer B ships the full `Result` value + combinator surface now. **Con:** the ergonomic `?` sugar waits.
- **Alt F-Q-b — add `?`-postfix.** A grammar prerequisite task for the postfix `?` operator + a propagation desugaring (likely `expr? ≡ match expr { Ok {value:v} => v, Err {error:e} => <propagate e> }`, but "propagate" needs an enclosing `Result`-returning context + early-exit semantics). **Con:** a much larger control-flow change to a pure-functional language; same class as `try`/`catch` (PRD §10 out of scope). A separate PRD if wanted.
- **Impact:** default keeps Layer B combinator-only (consistent with the pure-functional model + §10's `?`-out-of-scope stance for Layer A). *Lean: F-Q-a — defer `?`; ship `fallback`/`or_else` over `Result` (task 4038); `?` is a separate control-flow PRD.*

### F4 — Is `Result` compiler-intrinsic (like `Option`) or a plain prelude enum? *(RESOLVED: prelude enum F4-a — Layer B now in scope)*

Live now that Layer B is built (F1 resolved). **RESOLVED to F4-a (prelude enum, D5).**

- **RESOLVED F4-a — plain (generic, data-carrying) prelude `enum Result<T,E>`** (D5; Layer-B α task 4035). No new `Value` variant; `Result` is library-tier. Singular value model; Layer B = generic-DCE enum + a prelude enum + combinators. **Chosen** because generic enums are now filed/owned work — the "gated on generic DCE landing" cost is acceptable (the chain DCE → generic-enums → Layer B is wired).
- **Alt F4-b — `Result` compiler-intrinsic** like `Option` (a `Value` variant + bespoke `ok(x)`/`err(e)` ctors + intrinsic pattern grammar) — **rejected**. It was the escape hatch to get `Result` *before* generic enums; with generic enums now built, the bespoke-intrinsic cost (a second intrinsic ADT, bespoke pattern grammar, divergence from "Result is just a generic enum") buys nothing. F4-a is strictly cleaner.

## Assumptions

- `Value::Option` (three-state `some`/`none`/`undef`, §9.2.8) and generic free-fn declaration (`fn f<T>(…)`) both exist and parse today. **Verified 2026-05-27** (`ty.rs` `Type::Option`, `value.rs` `Value::Option`, fixture raf-11 parses with 0 ERROR nodes).
- `Freshness::Failed { error: ErrorRef }` / `EventKind::Failed` are the graph-failure machinery and are reserved for evaluation-level failures (not constraint diagnostics). **Verified** (`reify-ir/src/value.rs:2744`, `engine_constraints.rs:274-275` comment, `journal.rs:43`).
- The existing forced-fail eval path (`engine_admin.rs:1641`, `freshness_walk.rs:262`) can synthesize a graph-`Failed` node for the γ boundary test. **Verified** the path exists.
- The §9.2.6 `map[key]`-absent "evaluation failure (not undef)" is a real fallible site that `get_or`/`try_get` recovers. **Verified** in spec §9.2.6.
