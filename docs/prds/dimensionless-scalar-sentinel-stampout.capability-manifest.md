# Capability Manifest — `dimensionless-scalar-sentinel-stampout`

Mechanizes G3 (assumed substrate exists) + G6 (premise validity) per leaf. Every binding below is
**PASS** at author time (verified against current `main`, head `61f4e61de8` / live code read
2026-06-17) or carries an explicit deferral. Any FAIL blocks the batch at decompose.

PRD: `docs/prds/dimensionless-scalar-sentinel-stampout.md`.

## Substrate bindings (shared across all leaves) — G3

| Capability the PRD relies on | Evidence form | Verdict | Evidence |
|---|---|---|---|
| `Type::Error` + `is_error()` poison sentinel | wired-on-main | **PASS** | `crates/reify-core/src/ty.rs:492` — `pub fn is_error(&self) -> bool { matches!(self, Type::Error) }` |
| `make_poison_type(diags, diag) -> Type::Error` producer helper | wired-on-main | **PASS** | `crates/reify-compiler/src/expr.rs:105`; live callers at `:1470/:1493/:1529/:1767/:3705/:3894/:3989/:4141` (production paths, not `tests/`) |
| Producer-side `implicitly_converts_to(Error, _) => true` cascade-suppressor | wired-on-main | **PASS** | `crates/reify-compiler/src/type_compat.rs:33-35` (doc), `:52-75` (guard); `Type::Error` excluded from `is_scalar_like_leaf` |
| `check_param_default_type` fires on `is_error()` | wired-on-main | **PASS** | `crates/reify-compiler/src/entity.rs:414` — `if declared.is_error() { return; }`; and `:409` excludes the no-annotation default |
| Canonical correct exemplar to copy | reference | **PASS** | `crates/reify-compiler/src/conformance/checker.rs:110-131,151-166` — "all error paths return `Type::Error`, NOT `dimensionless_scalar()`" |
| `DiagnosticCode::UnresolvedType` emitted at every Tier-1/2 site | wired-on-main | **PASS** | grep: `diagnostics.push(Diagnostic::error(... UnresolvedType ...)` immediately precedes each cited offender line |
| `is_error()` does NOT recurse into `Type::Union` (drives L3 care) | behavioral | **PASS** | `ty.rs:492` is a flat `matches!`; `expr.rs:2718` builds `Type::Union(arm_types)` from `emit_ice_unresolved` results |

## Grammar bindings (behavioral-test fixtures) — G3 grammar gate

Each L5 behavioral-test fixture uses **existing grammar** (no novel syntax). `reify check` proceeds
past parse to semantic diagnostics on all of them — confirmed 2026-06-17 via `target/debug/reify`.

| Fixture shape | Verdict | Evidence |
|---|---|---|
| `structure W { param p : Bogus = 5kg }` | **PASS (parses)** | `reify check` → `error: unresolved type: Bogus` (semantic, not parse) |
| `fn f() -> Bogus { 0 }` | **PASS (parses)** | `reify check` → `error: unresolved return type: Bogus` |
| `field g : <DimensionalOp/IntegerLiteral/Auto/QualifiedAssoc>` | **PASS (parses)** | each already reaches `functions.rs:580-728` which pushes `UnresolvedType` (semantic) |
| trait member missing annotation | **PASS (parses)** | reaches `checker.rs:240-249` "no type annotation" (semantic) |
| `(5kg).sum()`, `[1,2].keys()` (wrong receiver) | **PASS (parses)** | reaches `expr.rs:3678-3691` method-type inference |

> The decompose substrate workflow (`scripts/prd-decompose-verify.mjs`) re-runs `tree-sitter parse
> --quiet` on the committed fixtures + a live `reify check` against a fresh current-main build.

## Premise bindings — G6

This PRD asserts **no numeric bound, exactness claim, or closed-form result** — the G6 numeric/exactness
branches (Reify's frequent-fire branches) **do not apply**. The premise class here is
**rejection-mechanism-backed** (branch 4): every "exactly one error / no secondary cascade" signal
rests on an active mechanism.

| Premise (leaf signal) | Rejection mechanism backing it | Verdict | Evidence |
|---|---|---|---|
| Today `param p : Bogus = 5kg` emits **two** errors (the thing being fixed) | n/a — empirical baseline | **PASS** | task 4640 dry-run, head_sha `139a1522f1` (current main): *"the EMPIRICAL test run confirms the failure"*; + code trace `entity.rs:995 → check_param_default_type` |
| After fix, `param p : Bogus` emits **one** error | `is_error()` guard at `entity.rs:414` fires once `declared = Type::Error` | **PASS** | guard verified present; `Type::Error` ⇒ `is_error()=true` ⇒ early return |
| Secondary conformance / codomain cascade suppressed | `implicitly_converts_to(Error, _) => true` | **PASS** | `type_compat.rs:33-35,52-75` |
| L4: wrong-receiver `.sum()/.keys()/.values()` *should* be rejected (it's silent today) | new `make_poison_type` diagnostic added by L4 | **PASS (mechanism queued in L4)** | the rejection mechanism is the leaf's own deliverable; G6 branch-3 self-production is satisfied because L4 *builds* the diagnostic, it does not assume a sibling produces it |
| L3: ICE'd cell is `is_error()` true without breaking the `Union` consumer | `emit_ice_unresolved` returns `Type::Error`; L3 examines `expr.rs:2718` | **PASS (needs-care, in-leaf)** | the Union interaction is L3's explicit scope; no premise depends on a capability outside L3's own dependency set |

## Anti-orphan / wired-on-main (each fix site is on a production path) — G3/G2

| Leaf | Production entry path (not test-only) | Verdict |
|---|---|---|
| L0 (#4645) entity.rs | `compile_entity` pass-2 param/port/type-arg resolution | **PASS** (live compiler path) |
| L1 functions.rs/traits.rs | `compile_function` / trait-signature / field-type resolution | **PASS** |
| L2 conformance/checker.rs | conformance phase-5 member-vs-requirement check | **PASS** |
| L3 ice.rs | `emit_ice_unresolved` called from `entity.rs:3624/3634`, `guards.rs` | **PASS** |
| L4 expr.rs | `compile_expr` method-call type inference | **PASS** |
| L5 guard | new `reify-audit` detector in dispatch (`crates/reify-audit/src/`) + `crates/reify-compiler/tests/` behavioral suite run in CI | **PASS (deliverable)** |

## Notes for the decompose session

- **Do NOT refile L0.** It is the existing task **#4645** (in-progress, `depends_on 4640`). Update its
  scope to cover entity.rs Tier-2 sites (`1505/1846/3422/3644`) per PRD §5 D3; leave the 4640 dep.
- Wire `{4645, L1, L2, L3, L4} → L5`. L1–L4 carry no intra-batch deps (distinct files, parallel).
- Re-run the substrate workflow before flipping the batch `deferred → pending`; it rebuilds current
  main and re-probes the two-error baseline + the fixture parses (the local binary used at author time
  was stale Jun-16/pre-4318).
- Carry `user_observable_signal` + `consumer_ref` metadata per leaf (PRD §2 table / §9 signals).
