# Expected-type push-down (bidirectional / contextual typing)

**Status:** active — first increment (let-binding + function-argument positions).
**Milestone:** version-agnostic compiler-typing foundation (root `docs/prds/`).
**Date:** 2026-06-19.
**Supersedes:** cancelled task **3751** ("Shape B: `Type::Unknown` for empty-collection element-type"). 3751's premise was verified-false and premature — its fix locus (`reify-types`/`reify-ir`'s runtime `Value::infer_type`) is **off the compiler path**, its dependency (4431) could not deliver, and its mechanism (`Type::Unknown`) is only meaningful once resolution infrastructure exists. That infrastructure — expected-type push-down — did not exist and was scoped by no task. This PRD builds it.

---

## 1. Goal — what a user observes

A `.ri` author writes an annotated empty collection literal and it Just Works, instead of getting a spurious warning and a wrongly-inferred element type:

```reify
let xs : List<Length>          = []        // resolves to List<Length> — no warning
let s  : Set<Length>           = set {}     // resolves to Set<Length>
let m  : Map<String, Length>   = map {}     // resolves to Map<String, Length>
let xss: List<List<Length>>    = [[]]       // inner [] resolves to List<Length>
firstlen([])  // fn firstlen(xs : List<Length>) — resolves; no "no matching overload" error
```

And a genuinely-unpinnable empty literal gets an honest, distinct diagnostic instead of a silent wrong default:

```reify
fn ident<T>(xs : List<T>) -> Int { xs.count }
... ident([])   // error[E_TYPE_UNDETERMINED]: cannot determine element type of empty list
                //   for type parameter `T` (no element supplies it, no other argument binds it)
```

And a collection literal whose annotation is the wrong *kind* stops being silently accepted:

```reify
let a : Length = []   // error[E_COLLECTION_LITERAL_KIND_MISMATCH]: list literal cannot
                      //   satisfy annotation `Length` (scalar). Today: silently accepted.
```

User-observable surfaces (the G1 consumer): `reify check` diagnostic output (warning suppression on the positive cases; a new `E_TYPE_UNDETERMINED` error on the negative case) and the cell/argument types the compiler assigns (observable through any downstream use that requires the annotated type). Two committed integration suites under `crates/reify-compiler/tests/` pin both directions.

## 2. Background

`/review` (2026-05-14) found 8 empty-literal silent-default sites. Shapes A/C were handled by tasks 3639 / 3749. Shape B — the genuine-ambiguity remainder — is this PRD.

**Verified current behaviour (against `main`, 2026-06-19; reproduced with `reify check`):**

- `compile_expr` (`crates/reify-compiler/src/expr.rs` ~:962) takes **no** `expected_type` parameter. There is no contextual/bidirectional typing anywhere in the compiler.
- The let-binding path (`crates/reify-compiler/src/entity.rs` ~:1659–1715) compiles the RHS **without** the annotation and takes the cell type directly from `compiled_expr.result_type`. The **only** annotation-consulting code is `fixup_option_none_for_let` (`entity.rs` ~:4208), and only for the `Option(None)` special case — a post-hoc patch, not a general mechanism. **The let annotation is otherwise decorative**: `let a : Length = []` (scalar annotation, list RHS) reproduces as a *silent accept* — only the empty-list warning fires, then `All constraints satisfied.` (the annotation is dropped; the cell becomes `List<Real>`). The brief's stated symptom — a `List<Real>` vs `List<Length>` *mismatch cascade* at the let — does **not** occur on `main`; there is no let annotation-vs-RHS check at all. The silent accept is a **bug to fix, not behaviour to preserve** (§5.3); note the asymmetry with **params**, which *do* get a declared-type-vs-init check (`DiagnosticCode::ParamDefaultTypeMismatch`, task 4318) — lets have no equivalent. This PRD closes the slice of that gap that the expected-type channel sits on (collection-literal *kind* mismatch); the general let analogue is a filed follow-up (§11).
- Empty collection literals fall back in `expr.rs`: `ListLiteral` (~:3856) → `Type::dimensionless_scalar()` + warning *"cannot infer element type of empty list literal, defaulting to Real"*; `SetLiteral` (~:3886) likewise; `MapLiteral` key → `Type::String` (~:3926, warning *"cannot infer key type of empty map literal, defaulting to String"*), value → `dimensionless_scalar` (~:3938). These warnings carry **no `DiagnosticCode`** today (bare `Diagnostic::warning(...)`).
- The real *cascade* lives at **argument** position: `firstlen([])` with `fn firstlen(xs : List<Length>)` reproduces as warning → `error: no matching overload for firstlen(List<Real>), candidates: firstlen(List<Scalar[m]>)`. Push-down at the arg fixes this.
- The genuinely-unpinnable case also lives at **argument** position: `ident([])` with `fn ident<T>(xs : List<T>)` reproduces as warning → `All constraints satisfied.` — `T` is silently defaulted. This is the one place a *new* error is honest.
- `Type::Unknown` does **not** exist; `Type::Error` (`crates/reify-core/src/ty.rs` ~:196) is the type-inference poison sentinel — and it is the **wrong** answer for `[]` (genuine ambiguity is not an error). `Type::{List,Set,Map,Option,TypeParam}` and `Type::dimensionless_scalar()` (~:293) all exist.
- `DiagnosticCode` (`crates/reify-core/src/diagnostics.rs` ~:156) is a `#[non_exhaustive]` PascalCase enum; a code is attached with `.with_code(DiagnosticCode::X)`. There is **no** `E_TYPE_UNDETERMINED` code; adding a `TypeUndetermined` variant (wire `"TypeUndetermined"`, PRD-prose mnemonic `E_TYPE_UNDETERMINED`) follows the established `GeometryUnbounded ↔ E_GEOMETRY_UNBOUNDED` pattern.

> Line numbers verified on `main` 2026-06-19 but drift; prefer the stable names (`compile_expr`, `fixup_option_none_for_let`, `DiagnosticCode`, the `ListLiteral`/`SetLiteral`/`MapLiteral` arms) for durable links.

## 3. Activation status

Active now. No upstream substrate prerequisite: `compile_expr`, the empty-literal arms, the let path, the `DiagnosticCode` enum, `Type::{List,Set,Map,TypeParam}`, and the `set {}` / `map {}` / `fn name<T>(…)` / `f([])` grammar forms **all exist on `main`** (parse + behaviour reproduced). This PRD adds a mechanism (the expected-type channel) and one diagnostic code; it introduces **no novel `.ri` syntax** — G3 grammar gate is a no-op (every fixture parses with 0 ERROR nodes today).

## 4. Sketch of approach

Approach **(a)** from the brief: thread a contextual `expected_type: Option<&Type>` into the empty-collection-literal compilation and consult it, generalizing the `fixup_option_none_for_let` special-case into a real, recursive mechanism. **Not** approach (b): no `Type::Unknown`/anonymous-type-param variant is introduced — when context pins the element the literal is typed directly, and when it cannot the answer is either the preserved warning (no context) or the new `E_TYPE_UNDETERMINED` error (generic-arg context), never an `Unknown` placeholder.

**Deliberately narrow.** Push-down resolves the element type only for collection literals whose expected type is the *matching collection kind*; a *non-matching kind* (`let a : Length = []`) is a hard error (`CollectionLiteralKindMismatch`, §5.3) rather than a silent accept. It does **not** introduce general let-annotation-vs-RHS enforcement for *non-collection-literal* RHS (`let a : Length = 5N`) — that would surface mismatches on every annotated let across the stdlib (a large, separate blast radius, filed as a follow-up §11). The line: the channel checks the collection literal's *kind* and resolves its *element type*; it does not deep-check element conformance of non-empty literals (§11).

See §6 for the contract and §7 for the boundary tests.

## 5. Resolved design decisions

1. **Mechanism: approach (a), distinct expected-type channel.** Thread `expected_type: Option<&Type>` so the empty-literal arms consult the contextual type. Reuse nothing from `auto_type_param_phase` — that is a *post-compile* phase resolving `Foo<auto: T>()` constructor sites (`crates/reify-compiler/src/auto_type_param.rs` + `…/compile_builder/auto_type_param_phase.rs`), a different mechanism. We consume the existing `Type::TypeParam` representation to *detect* the unbound-generic case, but the channel itself is new and lives at compile time.
2. **No `Type::Unknown`.** The design does not force it (decision 1). `Type::Error` stays reserved for genuine errors (the dimensionless-scalar-sentinel concern, §8), never for `[]`.
3. **Engagement rule (present annotation + collection-literal RHS) — three arms.** When `expected_type = Some(T)` and the literal is a collection literal of kind `K`:
   - **`T`'s kind matches `K`** → engage: resolve the element type from `T` (empty) / recurse into children (non-empty).
   - **`T`'s kind does NOT match `K`** (e.g. `let a : Length = []`, `let xs : Set<Length> = [1mm]`) → **error** `DiagnosticCode::CollectionLiteralKindMismatch` (`E_COLLECTION_LITERAL_KIND_MISMATCH`, `Severity::Error`). This is the fix for the silent accept — **not** a fall-through. Applies to empty *and* non-empty collection literals (the kind disagreement is independent of element count).
   - **`expected_type = None`** → today's behaviour, exactly (no engagement).
4. **Recursion + scope of enforcement.** A *kind-matching non-empty* literal compiles each child with the expected element type as the child's `expected_type`, so a nested empty (`[[]]` under `List<List<Length>>`) resolves. Push-down checks the literal **kind** against the annotation but does **not** enforce *element-type conformance* of non-empty literals — `let xs : List<Length> = [1N]` (matching kind, mismatched element) stays unchanged (out of scope, §11). Only the outermost-kind check and nested-empty resolution are in scope.
5. **Non-regression invariant.** `expected_type = None` on an empty literal ⇒ the current warning + `dimensionless_scalar`/`String` default, byte-for-byte. Bare `let xs = []` (and every other no-context empty literal) is **unchanged**. This is a contract invariant, asserted by boundary test #4.
6. **Two new diagnostic codes, each fired where introduced (no orphan codes).**
   - `CollectionLiteralKindMismatch` — decision 3, fires wherever the channel sees a collection literal under a present, non-matching-kind annotation/parameter (let position in β; reused at arg position in δ). `Severity::Error`.
   - `TypeUndetermined` (`E_TYPE_UNDETERMINED`) — argument position only, when an empty collection literal's corresponding parameter element type is a function type-parameter not bound by any other argument (`ident([])` over `fn ident<T>(xs : List<T>)`). `Severity::Error`. The let position **never** emits it (annotated → resolve / kind-mismatch; unannotated → warning).
7. **Scope = let + argument positions.** Return-type and struct-field-init positions are deferred to the stub PRD `expected-type-pushdown-return-field.md`, whose full design is triggered after this PRD's integration gate (ε) lands. The **general** let annotation-vs-initializer check (non-collection-literal RHS, the true let-analogue of `ParamDefaultTypeMismatch` / 4318) is a filed follow-up (§11), not this PRD.

## 6. Contract — the expected-type channel (H component)

The seam is a single contextual parameter consulted at the empty-collection-literal arms. An implementer of the producer side (the channel) and the consumer sides (let path, call-argument path) can work to this contract without further discussion.

**Signature.** The empty-collection-literal arms gain access to `expected_type: Option<&Type>` — the contextual type the literal is being compiled against. (Tactical: whether this is a new `compile_expr` parameter or a small context struct threaded through an internal entry that the public `compile_expr` delegates to with `None` is an Open Question, §10; the *contract* is identical either way.)

**Engagement.** For a collection literal `L` of kind `K ∈ {List, Set, Map}` (empty or non-empty):
- `expected_type = Some(T)` and `T` is the matching `Type::K(…)` ⇒ **engaged (resolve)**.
- `expected_type = Some(T)` and `T` is **not** the matching kind ⇒ **kind mismatch** → emit `DiagnosticCode::CollectionLiteralKindMismatch` (`Severity::Error`), message form *"`<kind>` literal cannot satisfy annotation `<T>`"*, label at `L`'s span. (Fixes `let a : Length = []`.)
- `expected_type = None` ⇒ **not engaged** (today's behaviour).

**Resolution (engaged, empty literal).**
- List/Set: element type := `T`'s element slot. Result `Type::List(elem)` / `Type::Set(elem)`. **No warning.**
- Map: key type := `T`'s key slot, value type := `T`'s value slot. Result `Type::Map(key, value)`. **No warning.**

**Recursion (engaged, non-empty literal).** Each child element is compiled with the expected element type as its own `expected_type`. (Outer pins inner empties; siblings are otherwise typed as today.) The **kind** of each child is checked against the expected element type by the same arm; the *element types* of non-empty literals are not otherwise enforced (out of scope).

**Non-engagement (`expected_type = None`).** Identical to current behaviour: empty ⇒ warning + default (`dimensionless_scalar`, Map key `String`); non-empty ⇒ bottom-up inference from the first element. **Invariant: zero diff for `expected_type = None`.**

**Argument binding (consumer: call-argument compilation).** When a call argument is a collection literal, the expected type is the (unique / unambiguously-selected) candidate's corresponding parameter type. The same three engagement arms apply (matching kind → resolve/recurse; non-matching kind → `CollectionLiteralKindMismatch`). For an **empty** literal whose parameter type is the matching `Type::K(…)`:
- parameter element type is **concrete** (or already-bound) ⇒ resolve (fixes the `firstlen([])` overload cascade).
- parameter element type is a function **type-parameter** `Type::TypeParam(P)` not bound by any other argument of the call ⇒ emit `DiagnosticCode::TypeUndetermined` (`E_TYPE_UNDETERMINED`, `Severity::Error`); do **not** silently default the element to `Real`.

**Error semantics.** `E_TYPE_UNDETERMINED` is the only new diagnostic. It is an error, message form *"cannot determine element type of empty `<kind>` literal for type parameter `<P>`"*, carrying a label at the literal's span. No new warning code is required (the existing warnings are unchanged); optionally the existing empty-literal warning may gain a stable `DiagnosticCode` for testability (tactical, §10).

## 7. Boundary-test sketch (ε's observable signal; faces both sides)

| # | Scenario | Pre | Post (assert via `reify check` / cell type) | Side |
|---|---|---|---|---|
| 1 | annotated empty list | `let xs : List<Length> = []` | **no** "cannot infer element type" warning; `xs : List<Length>` (a downstream `List<Length>` use type-checks) | consumer (let) |
| 2 | annotated empty set / map | `let s : Set<Length> = set {}`; `let m : Map<String,Length> = map {}` | no warning; `s : Set<Length>`, `m : Map<String,Length>` | consumer (let) |
| 3 | nested annotated empty | `let xss : List<List<Length>> = [[]]` | no warning; inner `[]` typed `List<Length>` | producer (recursion) |
| 4 | **non-regression** bare let | `let xs = []` (no annotation) | **still** warns "cannot infer element type…"; `xs : List<Real>` | invariant |
| 5 | positive arg (concrete param) | `firstlen([])`, `fn firstlen(xs : List<Length>)` | **no** "no matching overload" error; resolves; arg typed `List<Length>` | consumer (arg) |
| 6 | **negative** arg (generic param) | `ident([])`, `fn ident<T>(xs : List<T>)` | `error` with `DiagnosticCode::TypeUndetermined`; **no** silent accept | rejection |
| 7 | **kind mismatch, empty** (the `let a : Length = []` fix) | `let a : Length = []` | `error` with `DiagnosticCode::CollectionLiteralKindMismatch`; **no** silent accept (was: warning + `All constraints satisfied`) | rejection |
| 7b | **kind mismatch, non-empty** | `let xs : Set<Length> = [1mm]` | `error` with `DiagnosticCode::CollectionLiteralKindMismatch` | rejection |
| 8 | element-conformance scope guard | `let xs : List<Length> = [1N]` (matching kind, mismatched element) | behaviour **unchanged** — no element-conformance enforcement on non-empty literals (kind matches; out of scope §11) | producer (scope guard) |

## 8. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/expected-type-pushdown-return-field.md` (stub) | produces-for | the expected-type channel (§6) extended to return + struct-field-init positions | **stub PRD** | queued — full design triggered after ε lands |
| `docs/prds/dimensionless-scalar-sentinel-stampout.md` | delineation (no shared mechanism) | empty-literal defaults vs unresolved-type-*name* → `Type::Error` | n/a | that PRD explicitly **keeps** empty-collection element-type defaults out of scope; this PRD does **not** route `[]` through `Type::Error` | 
| `auto-type-param-resolution` (task 4431, DONE) | consumes (representation only) | `Type::TypeParam(P)` — used to *detect* the unbound-generic arg case (§6) | this PRD | wired — we read the existing representation; we do **not** extend the `auto_type_param` phase |
| `docs/prds/v0_6/type-hygiene.md` (task α, DONE) | adjacent (no seam) | both touch compile-time type checking (`type_compat.rs` / `expr.rs`) but different concerns | n/a | informational |
| follow-up task **#4705** (general let annotation-vs-initializer check; gated on ε) | sibling concern | the let-analogue of `ParamDefaultTypeMismatch` (#4318) for non-collection-literal RHS — covers the mismatch cases this PRD's kind-check does not (§11) | that task | queued — depends on ε (#4704) so let-path edits serialize |

No contested-ownership pair from the overlay's known set is touched. The one forward seam (return/field) is owned by the stub PRD by construction; this PRD owns the channel it will extend. The general let-enforcement follow-up is owned by the filed task, not this PRD.

## 9. Decomposition plan

Approach **B + H** (load-bearing seam = the compiler type-inference path; the §6 contract + §7 boundary sketch are first-class so the cross-cutting arg-position integration does not starve under the narrow-lock orchestrator). Greek labels are PRD-local; task IDs assigned at decompose.

- **α — expected-type channel foundation** *(intermediate)*. Introduce `expected_type: Option<&Type>` into the empty-collection-literal compilation (§6 engagement + resolution + recursion); public/no-context behaviour byte-for-byte unchanged (invariant, §5.5). *Modules:* `crates/reify-compiler` (`expr.rs`). *Unlocks:* β, δ, ε. (No user-observable signal in isolation — roped to ε per the C-as-integration-gate pattern.)
- **β — let-binding push-down + kind-mismatch error** *(leaf)*. Wire the let path (`entity.rs` ~:1659–1715) to resolve the declared annotation and pass it as `expected_type` to the RHS compile; reconcile with `fixup_option_none_for_let` (the `Option(None)` patch becomes a special case of the general channel, or is left intact and complementary). **Introduce `DiagnosticCode::CollectionLiteralKindMismatch`** and emit it on the non-matching-kind arm (the `let a : Length = []` fix), in this task. Run the **full `--scope all` verify + the stdlib `.ri` corpus** and triage any newly-surfaced kind mismatches — they are latent bugs the silent accept was hiding; fix or annotate as found (the "be deliberate about what flips" discipline). *Modules:* `crates/reify-compiler` (`entity.rs`) + `crates/reify-core` (`diagnostics.rs`). *Signal (CLI):* `reify check` on #1–#3 emits no empty-literal warning and the cells carry the annotated types (positive); on #7 / #7b emits `DiagnosticCode::CollectionLiteralKindMismatch` (negative); **non-regression** #4 still warns; #8 unchanged. *Deps:* α.
- **δ — argument-position push-down + `E_TYPE_UNDETERMINED`** *(leaf)*. Wire call-argument compilation to push the selected candidate's parameter element type into an empty-collection-literal argument (§6 argument binding); **introduce** `DiagnosticCode::TypeUndetermined` and **emit** it on the unbound-generic case in the same task. *Modules:* `crates/reify-compiler` (call/overload compile) + `crates/reify-core` (`diagnostics.rs`). *Signal (CLI):* `reify check` on #5 resolves with no overload error (positive); on #6 emits `DiagnosticCode::TypeUndetermined` (negative). *Deps:* α.
- **ε — integration gate (two-way boundary suite)** *(leaf)*. Commit the §7 boundary-test suite under `crates/reify-compiler/tests/`, exercising both producer (channel/recursion/engagement) and consumer (let, arg) sides, including the non-regression and scope-guard rows. *Signal:* the suite passes in CI and `reify check` outputs match §7. *Deps:* β, δ. This is the H integration-gate leaf; landing it triggers the stub PRD's full design.

DAG: `α → {β, δ}`; `{β, δ} → ε`.

## 10. Open questions (tactical — defer to impl)

1. **Channel carrier shape.** New `expected_type` parameter on `compile_expr` (threads `None` to every existing call site — wider lock footprint) vs. a small context struct on an internal entry that public `compile_expr` delegates to with `None` (keeps the change inside `expr.rs`). Either satisfies §6. *Suggested:* the internal-context form (smaller blast radius); decide in α.
2. **Overload ambiguity + empty literal.** §6 argument binding specifies the *unique / unambiguously-selected* candidate. When multiple candidates differ only in an empty-literal argument's element type, push-down cannot pick one. *Suggested:* fall back to today's behaviour (warning + `Real` then overload resolution as now) rather than erroring; revisit if a real case appears. Decide in δ.
3. **Stable code on the existing warning.** Optionally attach a `DiagnosticCode` (e.g. `EmptyLiteralElementDefault`) to the preserved empty-literal warning so the non-regression test (#4) can match on a code rather than message text. *Suggested:* add it in α if cheap; otherwise match on message. No user-facing change.
4. **`fixup_option_none_for_let` convergence.** Whether to retire the `Option(None)` special case into the general channel (push the annotation, let the `OptionNone` literal consult it) or leave it as a complementary patch. *Suggested:* leave intact in β unless the general path subsumes it cleanly; no behaviour change either way.

## 11. Out of scope

- **General let annotation-vs-initializer enforcement for *non-collection-literal* RHS** — e.g. `let a : Length = 5N`, `let a : Length = some_geometry()`. This is the true let-analogue of `ParamDefaultTypeMismatch` (params, task 4318) and has a **stdlib-wide blast radius** (every annotated let with a decorative annotation). It is a clean, separate concern filed as a **follow-up trigger task** (see §8) — gated on this PRD's ε so the let-path edits serialize. This PRD fixes only the collection-literal *kind*-mismatch slice (`let a : Length = []`), because the expected-type channel already sits on that exact decision point.
- **Element-type conformance of *non-empty* collection literals** under a matching-kind annotation — `let xs : List<Length> = [1N]` (kind matches, element `Force` ≠ `Length`). Folds into the general enforcement above; kept out to bound β's blast radius to the *kind* check.
- **Return-type and struct-field-init positions.** Deferred to `expected-type-pushdown-return-field.md` (stub).
- **`Type::Unknown` / anonymous type-param inference, and bottom-up first-use unification** (the original 3751 mechanism). Not built — decision 2.
- **The runtime `Value::infer_type` empty-collection defaults** (`crates/reify-ir/src/value.rs`, incl. the `Value::Range` empty arm). Off the compiler path; unchanged. (3751's mis-targeted locus.)
