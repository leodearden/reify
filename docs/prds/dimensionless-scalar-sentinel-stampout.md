# PRD: Stamp out `Type::dimensionless_scalar()` as an unresolved/invalid-type sentinel

**Status:** authored 2026-06-17 · version-agnostic compiler-correctness foundation
**Provenance:** broad multi-agent investigation (Workflow `wf_b52a42e1-8a6`, 51 agents) spawned from `/.claude/spawn-briefs/dimensionless-scalar-sentinel-stampout.md`. Scope ratified by Leo 2026-06-17 (broadest tier + both anti-regression guards).
**Origin escalation:** `esc-4640-60` — task 4640's S2(2) test surfaced the symptom.

---

## 1. The invariant (the principle this PRD restores)

> `Type::dimensionless_scalar()` (the dimensionless `Real` scalar) must mean **only** a genuine
> dimensionless scalar. Any compiler producer that **emits an error diagnostic** for an
> **unresolvable or structurally-invalid type expression** — or that fails to resolve a type
> **name** present in source — must return **`Type::Error`** (the poison sentinel), never
> `Type::dimensionless_scalar()`.

This is **not a new contract.** It is already documented and partially enforced in-tree:

- `crates/reify-compiler/src/type_compat.rs:33-35` and `:52-75` — the anti-cascade guard at the top
  of `implicitly_converts_to` short-circuits `implicitly_converts_to(Error, _) => true`, so an
  `Error`-typed producer suppresses the downstream type-mismatch cascade. (`Type::Error` is
  explicitly excluded from `is_scalar_like_leaf` for exactly this reason.)
- `crates/reify-compiler/src/conformance/checker.rs:110-131,151-166` — the **canonical correct
  exemplar**: *"All error paths return `Type::Error` (poison sentinel), NOT
  `Type::dimensionless_scalar()`"*, with the rationale spelled out.
- `crates/reify-core/src/ty.rs:480-494` — `Type::Error` / `is_error()` doc: operations that see an
  `Error` operand must propagate `Error`, not fall back to `dimensionless_scalar()`.
- `crates/reify-compiler/src/expr.rs:105` — **`make_poison_type(diagnostics, diagnostic)` already
  exists** and is the canonical producer-side helper (emits the diagnostic **and** returns
  `Type::Error`); used at ~10 sites already.

The bug is that a family of producer sites — built before this contract was crisp, or that drifted —
write `Type::dimensionless_scalar()` where they should call `make_poison_type(...)` / return
`Type::Error`. This PRD stamps out the remaining offenders and adds a guard so they stay stamped out.

### Two harms (why this matters, not just cosmetics)

1. **Anti-cascade defeat.** `is_error()` is `matches!(self, Type::Error)` (`ty.rs:492`). An unknown
   type that becomes `dimensionless_scalar()` is *not* `is_error()`, so guards like
   `check_param_default_type` (`entity.rs:414`, `if declared.is_error() { return; }`) never fire and
   a **spurious secondary** diagnostic (`ParamDefaultTypeMismatch`, conformance mismatch, codomain
   mismatch, …) piles on top of the real root-cause error. This is the task-4640 S2(2) symptom.
2. **Surface-aliasing.** An unknown/deleted/invalid type name becomes *indistinguishable* from a
   genuine dimensionless scalar, defeating the static probes the
   `prd-gate-executable-substrate-verification` work relies on (an unresolved name silently reads as
   a real `Real`). Cf. that PRD's §3 `resolve_type_name` precedent (4577).

---

## 2. Consumer & user-observable surface (G1, G2)

**Not an in-engine seam** (no kernel/dispatch/ComputeNode wiring) — the engine-integration sub-check
does not apply. The consumers are:

- **Every Reify author** who mistypes or uses an unknown / structurally-invalid type name. The
  user-observable surface is the **`reify check` diagnostic stream**: exactly **one** root-cause
  error (`UnresolvedType` / "unresolved …"), with the secondary cascade suppressed.
- **The anti-cascade machinery** (`is_error()` guards, the `implicitly_converts_to(Error, _)`
  producer-side wildcard) — the in-tree consumer that *requires* the poison to be `Type::Error`.
- **Static substrate-verification probes** (the `prd-gate-executable-substrate-verification` gate),
  which require `Type::Error` to be distinguishable from a real dimensionless scalar.

**User-observable leaf signal (G2), per scenario** — all via the `reify check` CLI diagnostic
difference (existing E_* code `UnresolvedType`):

| Scenario | Today (buggy) | After fix (observable) |
|---|---|---|
| `param p : Bogus = 5kg` | `UnresolvedType` **+** spurious `ParamDefaultTypeMismatch` | **one** error: `UnresolvedType` |
| `fn f() -> Bogus { 0 }` | `UnresolvedType` + return-type registered as Real → mis-typed overload resolution | one error; the fn's return type is poison, not a silent Real |
| `field g : <bad-type-expr>` | `UnresolvedType` + Real codomain → possible secondary codomain mismatch | one error |
| trait member missing annotation | "no type annotation" + secondary conformance mismatch | one error |
| `(5kg).sum()` / `[1,2].keys()` (wrong receiver) | **silent accept** (no diagnostic; result typed Real) | **a diagnostic now fires** (was previously masked) |

---

## 3. Sketch of approach

For each offending site: replace `Type::dimensionless_scalar()` with `Type::Error`, calling the
existing `make_poison_type(diagnostics, diag)` helper where the diagnostic is constructed inline, or
returning `Type::Error` directly where the diagnostic was already pushed. **No new substrate, no new
grammar, no new diagnostic codes** (except the expr-method class, §9 L4, which converts a *silent*
path into a diagnosing one — a new `Severity::Error` emission via `make_poison_type`).

The change surface is concentrated in `crates/reify-compiler` (+ a doc/test touch in `reify-core`
and a new detector in `reify-audit`). Each leaf is a distinct file, so they parallelize without
lock contention.

### Verified site map (investigation output, adversarially verified)

Classification axis: a site is a **bug** iff a type expression was **present in source** and either
its **name** failed to resolve or it was **structurally invalid in a type position**, the producer
**emitted (or should emit) an error diagnostic**, and the fallback returned `dimensionless_scalar()`.
Sites where the branch means *"no annotation present"* (a deliberate language default) or *"transient
pass-1 placeholder, overwritten in pass-2"* are **KEEP** and out of scope.

**Tier 1 — unknown/unresolved type NAME → Real (the directive's core):**
- `entity.rs:981` (`resolve_qualified_assoc_type` → `None`), `:995` (bare unknown name, `UnresolvedType` emitted), `:1263` (port-param unknown name)
- `functions.rs:99` (fn param unresolved name; the `resolved=false` flag already blocks the *default-type* cascade, but the Real leaks into scope/body/overload resolution), `:202` (fn return), `:393` (assoc-fn param), `:433` (assoc-fn return)
- `traits.rs:57` (trait member annotation; resolver already diagnosed), `:96` (trait member type unresolved — `is_error()` currently false → spurious conformance)
- `conformance/checker.rs:248` (trait member **missing** annotation — trait members *require* one, so this is an error path, not a language default; diagnostic emitted then Real returned)

**Tier 2 — structurally-invalid type-expr in a type position, diagnostic already emitted → Real:**
- `entity.rs:1505`, `:1846`, `:3422` (DimensionalOp / IntegerLiteral in a type-argument position)
- `functions.rs:589`, `:600`, `:612`, `:625`, `:666`, `:680`, `:695`, `:711` (DimensionalOp / IntegerLiteral / Auto / QualifiedAssoc disallowed in field domain/codomain position — each already pushes `UnresolvedType`; full symmetric set across BOTH domain and codomain per the esc-4646-3 ratification)
- `functions.rs` `TypeExprKind::Function` domain arm in `compile_field`, `TypeExprKind::Function` codomain arm in `compile_field` — arrow type disallowed in field domain/codomain position; each already pushes the root-cause "function type not allowed" `UnresolvedType` diagnostic. **CONVERTED by #4657** (reverses the esc-4646-3 KEEP for these two arms): these arms are parse-reachable (`function_type` is a valid `lower_type_expr_node` choice in field positions — `ts_parser.rs lower_field`, task 4595), and returning `dimensionless_scalar()` rather than `Type::Error` did NOT short-circuit the analytical-source codomain check (`field_codomain_compatible`, gated on `codomain_type.is_error()`), so an arrow-typed field codomain with a dimensioned lambda body spawned a secondary `FieldCodomainMismatch` on top of the root cause (the residual cascade gap formerly tracked as esc-4646-36, now resolved). Behavioral coverage: `ds_sentinel_l1_poison_tests.rs::field_arrow_codomain_resolves_to_error_no_cascade` (codomain; cascade-count is the RED/GREEN discriminator) and `::field_arrow_domain_resolves_to_error` (domain; `domain_type.is_error()` is the sole discriminator — domain arm does not feed `field_codomain_compatible`).
- `traits.rs:44` (DimensionalOp in trait param position)
- `entity.rs:3644` (`_ =>` unhandled `MemberDecl` variant — defensive arm, emits a diagnostic; **marginal**, included for contract uniformity)

**Tier 3 — ICE path (needs care):**
- `ice.rs:57` `emit_ice_unresolved` returns `dimensionless_scalar()` after a pass-1-invariant ICE.
  **Blast radius:** its result becomes an `arm_type` collected into `Type::Union(arm_types)`
  (`expr.rs:2718`); `is_error()` does **not** recurse into `Union`, so an `Error` arm would be hidden
  from a `Union`-level `is_error()` check. Must be handled with the Union consumer examined, and the
  `emit_ice_unresolved_returns_type_real` unit test flipped.

**Tier 4 — expr-method receiver-type masking (distinct class, silent today):**
- `expr.rs:3682` (`.sum()` on non-`List`), `:3686` (`.keys()` on non-`Map`), `:3690` (`.values()` on
  non-`Map`) — currently return `Real`/`List<Real>` with **no diagnostic**, masking a type error;
  `:3445` (struct-member lookup fallback — intentional for `TraitObject` but masks a missing member
  on `StructureRef`). Fix = `make_poison_type` with a **new** diagnostic. (These already short-circuit
  incoming poison via `if compiled_obj.result_type.is_error() { return propagate_poison(); }` at
  `expr.rs:3674`; only the wrong-receiver-with-good-type case leaks.)

**KEEP (verified legitimate, out of scope):** `entity.rs:1002` (no annotation), `:1048`/`:1274` (pass-1
let placeholders), `:1266` (no annotation), `:4584` (transient skeleton, `throwaway_diags`, re-resolved
authoritatively in `compile_entity`); `type_resolution.rs:604` (`"Real"` literal); `functions.rs:206`/`:436`,
`traits.rs:155`/`:192`, `checker.rs:1000` (unannotated-return language default);
all of `math_signatures.rs`/`analysis_signatures.rs`/`joint_signatures.rs`/`builtin_signatures.rs`/`signatures_common.rs`/`units.rs`/`types.rs`/`geometry.rs`/`datum_projection.rs`
(85 sites — 100% genuine dimensionless op results); all `reify-ir`/`reify-eval`/`reify-expr`/`reify-core`
runtime sites (600+ — value placeholders / empty-collection element defaults / genuine dimensionless
results — **runtime crates do not perform type-name resolution**).

> **Note on the ANGLE ≠ DIMENSIONLESS concern** (`math_signatures.rs` comments): that is a *separate,
> known* correctness bug (a math op yielding the wrong *dimension*), **not** the unknown-name sentinel.
> Explicitly out of scope here.

---

## 4. Pre-conditions / assumed substrate (G3 — all verified to exist on current main)

| Assumed capability | Status | Evidence |
|---|---|---|
| `Type::Error` variant + `is_error()` | EXISTS | `reify-core/src/ty.rs:492` (`matches!(self, Type::Error)`) |
| `make_poison_type(diags, diag) -> Type::Error` | EXISTS | `expr.rs:105`, used at `:1470/:1493/:1529/:1767/:3705/:3894/:3989/:4141` |
| Producer-side `implicitly_converts_to(Error,_) => true` | EXISTS | `type_compat.rs:33-35,52-75` |
| `check_param_default_type` `is_error()` guard | EXISTS | `entity.rs:414` |
| `DiagnosticCode::UnresolvedType` | EXISTS | emitted at every Tier-1/Tier-2 site today |
| Grammar: `param p : Bogus`, `fn f() -> Bogus`, invalid field type-exprs | PARSES | `reify check` probe 2026-06-17 (proceeds to semantic diagnostics, no parse error) |
| `reify check` emits `UnresolvedType` for unknown names | CONFIRMED | probe: `error: unresolved type: Bogus`, `error: unresolved return type: Bogus` |
| `is_error()` does **not** recurse into `Type::Union` | CONFIRMED | `ty.rs:492` — drives the Tier-3 needs-care handling |

No grammar work, no new substrate task is a prerequisite. The only external dependency is the
sequencing on task 4640 (below).

---

## 5. Resolved design decisions

- **D1 — Scope = broadest (Tier 1+2+3+4) + both guards.** Ratified by Leo 2026-06-17. Rationale:
  Tiers 1 and 2 share one root cause, one harm (surface-aliasing + anti-cascade defeat), and one fix
  (`Type::Error`/`make_poison_type`); a partial fix leaves the same defect alive on a different
  trigger. Tier 3 (ICE) and Tier 4 (expr-method) are folded in as their own carefully-scoped leaves.
- **D2 — Reuse the existing contract; introduce nothing new.** Use `make_poison_type` / `Type::Error`.
  The contract is already documented in `type_compat.rs` / `checker.rs` / `ty.rs`; this PRD's job is
  to make the offenders comply and add a regression guard — it is the **H** (boundary-test) component
  retrofitted onto an existing contract, so the PRD is **bare-B + guard**, not a new B+H contract.
- **D3 — The entity.rs anchor (#4645) absorbs ALL entity.rs sites in scope.** #4645 is already filed
  (in-progress, `depends_on 4640`) scoped to the entity.rs unknown-name arms. Rather than file a
  second task that edits the same file (lock contention), the decompose session **updates #4645** to
  cover the entity.rs Tier-2 sites (`1505/1846/3422`) and the `3644` arm too. #4645 also owns flipping
  task 4640's S2(2) test back to one-error.
- **D4 — #4645 must land after 4640.** 4640 (interim two-error S2(2)) is mid-land and touches the
  same `entity.rs` region + owns the test #4645 flips. Dependency `4645 → 4640` is **already wired**.
- **D5 — The guard lands last.** The reify-audit advisory detector + behavioral test suite depend on
  all fix leaves, so the lint reports zero violations and the behavioral tests are green at landing.
- **D6 — Tier 4 adds new diagnostics.** The expr-method sites are silent-accept today; the fix
  *introduces* a diagnostic. That is a deliberate, user-observable behavior improvement (a previously
  masked type error now surfaces), not a regression.

---

## 6. Out of scope

- The ANGLE ≠ DIMENSIONLESS math-op dimension bug (separate known concern).
- Extending `is_error()` to recurse into `Type::Union` / compound types globally (the `ty.rs:488-491`
  planned propagation follow-up). Tier 3 only handles the *local* `emit_ice_unresolved` → `Union`
  interaction, not a general recursive `is_error()`. If Tier 3 finds the Union interaction needs the
  recursive change, it files that as a follow-up rather than expanding here.
- Any KEEP site in §3 (language defaults, pass-1 placeholders, genuine dimensionless results).
- The ~3353 legitimate `dimensionless_scalar()` call sites repo-wide.

---

## 7. Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Note |
|---|---|---|
| entity.rs fix + 4640 S2(2) test flip | **task #4645** (this PRD's L0) | already filed; depends on 4640 |
| 4640 interim two-error S2(2) | task **4640** | mid-land; #4645 supersedes its interim expectation |
| Surface-aliasing / static-probe reliance on `Type::Error` | `prd-gate-executable-substrate-verification.md` | this PRD *removes* the aliasing those probes are blind to; no shared edit |
| `is_error()` Union-recursion follow-up | `ty.rs:480-494` planned work | out of scope; Tier 3 may file a follow-up |

No contested-ownership pair from the audit breadcrumb map is touched. No new in-engine seam introduced.

---

## 8. Anti-regression guard (the two-way boundary test — G5 H-component)

Ratified: **both** of the following (one leaf, §9 L5):

1. **Behavioral test suite** (`crates/reify-compiler/tests/`): a matrix of `reify`-compile scenarios —
   {param, port-param, fn-return, fn-param, assoc-fn, trait-member, field-domain, field-codomain} ×
   {unknown name, structurally-invalid type-expr} — each asserting **exactly one** error (the
   root-cause `UnresolvedType`-class) and **no** secondary `ParamDefaultTypeMismatch` / conformance /
   codomain cascade. This is the forward boundary test (the fix works).
2. **`reify-audit` advisory detector** (new pattern, Medium/advisory like `malformed-cite`): flag a
   `Type::dimensionless_scalar()` returned within the compiler **type-resolution surface** on a
   block/arm that *immediately follows an error-diagnostic push* (`diagnostics.push(Diagnostic::error(`
   … `UnresolvedType` …)). This is the backward boundary test (a *new* offender can't be reintroduced
   silently). Scoped to `crates/reify-compiler/src/{entity,functions,traits,expr,conformance/*}.rs`
   plus an explicit allowlist comment (`// ds-sentinel:allow`) for the verified KEEP sites that legit-
   imately pair a diagnostic with a dimensionless result. Reuses the flat detector layout
   (`crates/reify-audit/src/p*.rs`).

---

## 9. Decomposition plan (one leaf per task; observable signal each)

> The decompose session files L1–L5 fresh (planning_mode). **L0 is NOT refiled** — it is the existing
> #4645, which the decompose session *updates* (D3) and leaves `depends_on 4640`.

- **L0 = #4645 (existing, UPDATE only)** — entity.rs Tier-1 (`981/995/1263`) + Tier-2
  (`1505/1846/3422/3644`) → `Type::Error`/`make_poison_type`; flip 4640 S2(2) test to one-error.
  *Signal:* `reify check` on `param p : Bogus = 5kg` emits exactly one error (`UnresolvedType`).
  *Deps:* 4640.
- **L1 — functions.rs + traits.rs + trait_requirements.rs** — Tier-1 (`functions 99/202/393/433`,
  `traits 57/96`) + Tier-2 (`functions 589/600/612/625/666/680/695/711`, `traits 44`) → `Type::Error`.
  *Signal:* `reify check` on `fn f() -> Bogus { 0 }`, a trait member with an unknown type, and a
  field with an invalid domain type each emit exactly one error. *Deps:* none (distinct file from L0).
- **L2 — conformance/checker.rs:248** — trait-member missing-annotation error path → `Type::Error`
  (match the file's own `:121-131/:151-166` exemplar). *Signal:* a conforming-structure scenario with
  a trait member lacking an annotation emits one error, no secondary "type mismatch for trait member".
  *Deps:* none.
- **L3 — ice.rs:57 `emit_ice_unresolved`** → `Type::Error`, **with** the `Type::Union(arm_types)`
  consumer at `expr.rs:2718` examined and a guarded-match-arm regression test held green; flip the
  `emit_ice_unresolved_returns_type_real` unit test. *Signal:* (internal-correctness leaf — see Open
  Q) the ICE'd cell is now `is_error()` true, and the existing guarded-match golden/integration test
  is unchanged. *Deps:* none. **needs-care.**
- **L4 — expr.rs method-receiver class** (`3682/3686/3690` + `3445`) → `make_poison_type` with a new
  diagnostic. *Signal:* `reify check` on `(5kg).sum()` / `[1,2].keys()` now emits a diagnostic
  (currently silent-accept) — exactly one error. *Deps:* none.
- **L5 — anti-regression guard** (§8: behavioral test suite + reify-audit advisory detector).
  *Signal:* `reify-audit --pattern <NEW>` reports zero violations on main; the behavioral matrix is
  green in CI. *Deps:* **#4645, L1, L2, L3, L4** (so tests are green + lint clean at landing).

DAG: `4640 → 4645`; `{4645, L1, L2, L3, L4} → L5`. L1–L4 fully parallel (distinct files).

---

## 10. Open (tactical) questions

- **L3 G2 signal.** An ICE is by definition *not* user-reachable, so its leaf signal is internal
  (`is_error()` on the ICE'd cell + no guarded-arm regression). The decompose G2 gate should decide:
  accept the internal signal, **or** fold L3 into L0/#4645 (same crate, the ICE helper is called from
  entity.rs), **or** down-prioritize L3. Recommendation: keep L3 separate with the internal signal
  documented, since the Union blast-radius makes it a genuinely distinct, careful change.
- **L4 diagnostic wording / code.** Whether the new expr-method diagnostic reuses an existing code
  (e.g. a "method not applicable to receiver type" code) or mints one — implementation-time.
- **L5 detector heuristic precision.** The "dimensionless after a diagnostic push" heuristic needs an
  allowlist (`// ds-sentinel:allow`) for legitimate diagnostic-then-dimensionless KEEP sites; the exact
  AST/line-window heuristic vs a structural matcher is an implementation choice for L5.
- **Live re-probe of the two-error premise.** The local `reify` binary is stale (Jun 16, pre-4318);
  the two-error premise is confirmed by 4640's dry-run on current main + code trace. The decompose
  substrate workflow rebuilds current main and re-probes — no action needed at author time.
