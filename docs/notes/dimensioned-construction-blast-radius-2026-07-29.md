# Dimensioned-construction blast-radius ledger (task 5756, α)

Measurement-only research record. Produces the per-site migration ledger that
`β` (corpus migration), `γ` (predicate promotion), `δ₁` (param/let default
tolerance removal), `δ₂` (constraint-def numeric leniency removal) and the
§6.4 quantity-slot follow-up all cite. **α lands nothing in `crates/`,
`examples/`, `gui/`, or `stdlib`** — every count below is produced by a local,
uncommitted predicate flip that is measured and then fully reverted (proof in
§11(B)). File:line anchors are point-in-time — re-verify against current
`main` before building on one, exactly as `docs/notes/units-gating-gap-research-2026-07-28.md`
warns for its own anchors.

## Provenance

- **Measured at HEAD:** `08c6c42be97d39772f93ee3368c6466bda37fbc6` (short `08c6c42be9`), branch `task/5756`, base `main`.
- **Date:** 2026-07-29.
- **PRD:** `docs/prds/v0_6/dimensioned-construction-strictness.md`, primarily §6.5 ("What α must still measure") and §11 (decomposition plan, α's charter).
- **Task:** 5756 — "dimensioned-construction α: blast-radius measurement + per-site migration ledger (flip the predicate locally, never land it)". Dependency 5753 (ζ₀, the ruling/reversal-history doc) landed as `767c004fd5`.
- **Sibling record:** `docs/notes/units-gating-gap-research-2026-07-28.md` — the same units-strictness program's phase-1/phase-2 research note (HEAD `d2651bce16` / `1195020471`); its "PHASE 2" section independently pre-surveys this same predicate and is cited throughout below where it corroborates or predates a measurement here.

## How to reproduce

**1. The local predicate flip** (never committed — applied, measured, reverted within a single working-tree session; see §11(B) for the revert proof):

```diff
--- a/crates/reify-compiler/src/conformance/mod.rs
+++ b/crates/reify-compiler/src/conformance/mod.rs
@@ -1691,7 +1691,7 @@ fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
     match param_type {
         Type::Bool | Type::Int | Type::String => true,
-        Type::Scalar { dimension } => dimension.is_dimensionless(),
+        Type::Scalar { .. } => true,
         _ => false,
     }
 }
```

**2. Cargo invocations.** This repo's `cargo` is intercepted by a PreToolUse
condensation wrapper ("skim") that rewrites `cargo test`/`cargo build` output
to terse `PASS: N | FAIL: M | SKIP: K` summaries — useless for harvesting a
per-violation panic transcript. Every invocation below defeats it by
redirecting to a file and reading the file back (`bash -c '... 2>&1' >
/tmp/out.txt 2>&1; cat /tmp/out.txt`), per the project's own documented
bypass list (CLAUDE.md → Mem0 procedural memory on the cargo/skim wrapper).
Exact commands are given in §1 below, next to the transcript they produced.

## Re-verified anchor table

Every anchor the PRD and the task's decompose-time addenda name, re-read
directly against HEAD `08c6c42be9` in this session (not trusted from the PRD
text). All CONFIRMED unless noted.

| # | Anchor | Cited location | HEAD status |
|---|---|---|---|
| 1 | `general_leaf_param_family_is_validated` (the predicate) | `crates/reify-compiler/src/conformance/mod.rs:1691-1697` | **CONFIRMED** — exactly the four arms: `Bool\|Int\|String => true`, `Scalar{dimension} => dimension.is_dimensionless()`, `_ => false`. |
| 2 | Gate 2 (conformance param-default entry) | `conformance/mod.rs:517-532` | **CONFIRMED** — `_ =>` arm; anti-cascade `Type::Error` guard at :519-521; `severity: CTOR_FIELD_CONFORMANCE_SEVERITY` at :530; `walk_param_against_arg(&vc.cell_type, default, &mut ctx)` at :532. |
| 3 | `CTOR_FIELD_CONFORMANCE_SEVERITY` | `conformance/mod.rs:32` | **CONFIRMED** — `Severity::Warning`. α does not touch this const. |
| 4 | Walker lockstep recursion | `conformance/mod.rs:669-701` | **CONFIRMED** — `walk_param_against_arg`: `Option`/`List`(+`ReflectiveCellList`)/`Set`/`Map` arms recurse; `TraitObject` is a leaf (handled after :704). |
| 5 | Gate 5 / Rule 4 (constraint-def numeric leniency) | `crates/reify-compiler/src/type_compat.rs:1482-1486` | **CONFIRMED** — `let is_numeric = |t| matches!(t, Type::Int \| Type::Scalar{..} \| Type::ScalarParam(_)); if is_numeric(param_ty) && is_numeric(arg_ty) { return true }`. Rule 2 at :1474 (`type_carries_type_param \|\| type_carries_trait_object`) already short-circuits before Rule 4. |
| 6 | Fn-call entry guard (i) | `compile_builder/entities_phase.rs:1493` | **CONFIRMED** — `if !type_carries_trait_object(param_ty) { continue; }`, immediately preceding the only production `check_fn_arg_conformance` call at `:1503`. |
| 7 | `OverloadResolution::Resolved` gate | `compile_builder/entities_phase.rs:1486-1488` | **CONFIRMED, and this is anchor drift versus the PRD.** The PRD's own text cites `:1508-1511`; the real anchor at HEAD is `:1486-1488` (`let f = match resolve_function_overload(...) { OverloadResolution::Resolved(f) => f, _ => return };`). Addendum C1's correction re-confirmed. |
| 8 | Guard (ii) candidate backstop, `resolve_function_overload`'s filter | `type_compat.rs:1155` (fn), filter body `:1194-1201` | **CONFIRMED, and FALSIFIED as an independent backstop.** The filter's first disjunct is `type_carries_trait_object(param_ty)` (`:1196`) — the **same predicate** as guard (i). The two guards are mutually exclusive, not conjunctive. See §7 (item 4) for the reachability consequence. |
| 9 | Corpus gate test | `crates/reify-compiler/tests/examples_smoke.rs:198` (`no_example_emits_ctor_field_conformance_diagnostics`) | **CONFIRMED** — `exercised >= 40` floor at :215-220; per-violation panic body (`file [code] message`) at :222-241; `skip_set_entries_exist_under_examples_dir` sanity guard at :248; walks `examples/` only via `discover_ri_files`/`EXAMPLES_DIR` (not the wider `corpus_no_bare_scalar.rs` tree). |
| 10 | `NAMED_DIMENSIONS` registry | `crates/reify-core/src/dimension.rs:514` | **CONFIRMED present; cardinality corrected — see §0.** The array itself is unchanged in shape from what the PRD describes; the number of entries needed independent re-verification (three prior figures disagreed: 49/51/34). |
| 11 | `corpus_no_bare_scalar.rs` reuse target | `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` | **CONFIRMED, reused for infrastructure only — see caveat in §0.** `collect_files` at :46; five-tree walk at :143-160; self-exclusion at :170; parse-only exclusion of `reify-syntax/tests` + `reify-ast/tests` at :183-186. Its own `line_has_bare_scalar` predicate (matching the literal keyword `Scalar` bare) is **not** reused — it answers a different, already-closed migration (bare `Scalar` keyword → named dimension). α reuses the walk/exclusion machinery and comment-stripping technique, and writes its own predicate for bare *numeric literals* at dimensioned-type positions. |

---

## §0 Measurement substrate — dimension-name set (pre-2)

*(filled by pre-2)*

## §1 Flipped-predicate run + transcript (step-1)

*(filled by step-1)*

## §2 Item 7a — §6.2 re-confirmation: gates 3+4, δ₁'s blast radius (step-2)

*(filled by step-2)*

## §3 Item 7b — §6.3 re-confirmation: gate 1, β/γ's blast radius (step-3)

*(filled by step-3)*

## §4 Item 1 — Gate 2 hit count (step-4)

*(filled by step-4)*

## §5 Item 2 — Gate 5 (constraint-def Rule 4) sub-counts (step-5)

*(filled by step-5)*

## §6 Item 3 — `Type::ScalarParam` false positives, D4-5 (step-6)

*(filled by step-6)*

## §7 Item 4 — Fn-call-entry reachability, §3.1 (step-7)

*(filled by step-7)*

## §8 Item 5 — Vector3/Point3/Matrix/Tensor/Field quantity-slot residual, §6.4 (step-8)

*(filled by step-8)*

## §9 Item 6 — Load-struct intersection flag for PRD 5, §10.2 (step-9)

*(filled by step-9)*

## §10 Per-site classified ledger + consumer index (step-10)

*(filled by step-10)*

## §11 Methodology closure + no-source-change proof (step-11)

*(filled by step-11)*
