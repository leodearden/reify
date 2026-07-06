# Capability manifest — eval-cell-commit-substrate

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/eval-cell-commit-substrate.md`. Every binding below was verified by **executed grep against current main HEAD `4d696e63`** (2026-07-06 re-verification, 26-tool-call anchor sweep) — not a promise.

**Empty-value sentinel:** `Value::Undef` (n/a here — no result-field sampling leaf).
**D3 `.ri` workflow (`scripts/prd-decompose-verify.mjs`): N/A.** Its probe vectors are `tree-sitter parse` / `reify check` / `reify eval` over `.ri` fixtures (grammar / semantic-rejection / eval-IR premises). This PRD introduces **no novel `.ri` syntax** and **asserts no DSL-level rejection** — all substrate is engine-internal Rust, verified by grep. There are zero `.ri`-fixture premises for the workflow to enumerate; the applicable executed check is the grep evidence bound per leaf below (analogous to the overlay's "no novel substrate — grammar gate N/A").

**Numeric floor:** N/A — no leaf asserts a numeric bound / closed-form exactness.
**Rejection-mechanism (branch 4):** N/A as a DSL assertion; the two checker leaves (ι, μ) instead carry **RED-first seeded-violation self-tests** (anti-silent-accept — a checker never observed to fire is itself a silent accept; precedent: landed 4952).

Intermediates (α, β, κ, λ, ξ) are producers only — listed for the DAG-direction column of the leaves that consume them. π is a DECISION bookmark (no signal, no bindings).

---

## Leaf γ — migrate engine_eval.rs transaction/ctx sites

Signal: parity fixture — a warning swallowed on the cached path resurfaces; determinacy identical cold-vs-warm.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `commit_cell_result` primitive exists | Capability→producer | `producer:task-α` (upstream; new `cell_commit.rs`) | PASS |
| `cell_eval_ctx` required-args ctor | Capability→producer | `producer:task-β` (upstream; new `cell_eval_ctx.rs`) | PASS |
| α, β are upstream of γ | DAG-direction | γ `depends_on` α, β | PASS |
| the transaction/ctx copies exist to migrate | grep wired | `engine_eval.rs`: eval/eval_cached bodies; `reeval_cone_cell`:4918; annotation-args post-pass:4357; self-datum post-pass:3451-3473; `cell_eval_ctx`:4852 (4 sites); `eval_ctx_with_meta` ×86 | PASS |
| the three determinacy rules are real & preserved | grep wired | `DeterminacyState` stamping across paths; intentional divergence doc at `reeval_cone_cell` :4883-4897 | PASS |

## Leaf δ — migrate engine_edit.rs edit_param sites

Signal: edit-path re-eval commits identical determinacy; a dropped edit-path warning surfaces.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| α, β primitive + ctor | Capability→producer / DAG-direction | `producer:task-α`, `producer:task-β` upstream (δ deps α,β) | PASS |
| engine_edit.rs transaction copies exist | grep wired | `engine_edit.rs` inline eval→insert→record copies at :953/:1004, :1176/:1357, :2907/:2920, :3089/:3336 | PASS |
| `edit_param` (not `edit_source`) is the target | grep wired | `edit_source`:2369 is explicitly OUT of scope (0 prod callers → π decision) | PASS |

## Leaf ε — migrate unfold.rs guarded-group/unfold site

Signal: unfold path commits via the primitive; parity fixture green.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| α, β primitive + ctor | Capability→producer / DAG-direction | `producer:task-α`, `producer:task-β` upstream (ε deps α,β) | PASS |
| unfold.rs transaction copies exist | grep wired | `unfold.rs` eval→insert→`cache.record_evaluation` at :337-367 and :570-602 | PASS |

## Leaf ι — snapshot↔cache divergence audit (INV-EVAL-4)

Signal: audit green (asserting) over `examples/` in verify; seeded-divergence self-test proves it fires.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| cache entries carry a content-hash comparable to a snapshot value | grep wired | `cache.rs record_evaluation`:663 records value+hash; `compute_cache_key.rs` content-hashing on main | PASS |
| post-pass overwrites are routed through the primitive (so divergences vanish) | DAG-direction | `producer:task-γ` upstream (ι deps γ) | PASS |
| the audit is observed to fire, not silently pass | seeded-violation self-test | RED-first: fabricate a divergent state → assert non-empty report (4952 precedent) | PASS |
| complementary, not duplicate, of 4952 | scope | 4952 = `check_no_stale_undef` (INV-EVAL-5, done); ι = content-hash agreement (INV-EVAL-4) — distinct invariant | PASS |

## Leaf μ — diagnostics-in-cache replay + shared detector registry (INV-EVAL-3)

Signal: field-index-OOB warning resurfaces on a **repeated** `reify check` of unchanged content; each diagnostic emitted exactly once across eval/eval_cached/edit_check.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| a concrete runtime warning is emitted that a cache-hit currently swallows | grep wired | `W_FIELD_OUT_OF_BOUNDS` pushed into `ctx.diagnostics` (engine_edit.rs:2854, :894; concurrent.rs:512 note); `W_FIELD_SAMPLED_INVALID_CONFIG` (engine_eval.rs:2339) | PASS |
| the swallow is structural (NodeCache carries no diagnostics today) | grep wired | `cache.rs record_evaluation`:663 takes **no** diagnostics arg; NodeCache:237 has no diagnostics field (the 2259/2267 fast-path-swallow class) | PASS |
| NodeCache gains a diagnostics vec; a shared registry exists | Capability→producer / DAG-direction | `producer:task-κ` (cache.rs diag field) + `producer:task-λ` (detectors.rs registry) upstream; also deps γ, δ | PASS |
| cold-only asymmetry is real | grep wired | annotation-args post-pass runs on `eval` (:4357) but not `eval_cached` (:4373) | PASS |

## Leaf ν — one ordering core (P5; strengthens INV-EVAL-5)

Signal: existing byte-identical-schedule determinism tests stay green + differential old-vs-new order identical.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| the Kahn core to delegate to exists and is wired in production | grep wired | `run_unified_pass_seeded` engine_fixpoint.rs:273, sole prod caller engine_eval.rs:4177 | PASS |
| dirty.rs has the two sorts to redirect | grep wired | `topological_sort` dirty.rs:153, `compute_levels` :171; `resolve_order` lives separately (resolve_order.rs:302 — untouched) | PASS |
| a determinism corpus exists to pin byte-identical schedules | grep wired | `unified_dag_differential_corpus.rs` + dirty.rs `compute_levels_*` tests | PASS |

## Leaf ο — delete the dead concurrent stack (strengthens INV-EVAL-1/2)

Signal: build+test green post-deletion; test-count delta fully accounted (re-homed vs dropped named).

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| the concurrent stack has zero production callers | grep wired | `ConcurrentScheduler` (reify-runtime/src/concurrent.rs) instantiated only in tests + the un-wired wave-1 adapter; reify-cli imports only `commitment`/`Priority`; `reify-eval/src/concurrent.rs` re-exports used test-only | PASS |
| the wave-1 adapter bug is to be DELETED not fixed | grep wired | 4356-class bare-context bug at `concurrent_eval.rs:399-410` (no `.with_determinacy`/`.with_runtime_diagnostics`) — in dead code | PASS |
| shared-property pins are re-homed first | DAG-direction | `producer:task-ξ` upstream (ο deps ξ); pins named: `run_unified_pass_returns_acyclic_linear_schedule` (src/concurrent.rs:1008), back-prop pins :780/:886 | PASS |

## Leaf ρ — relocate freshness invariant warning (breadcrumb toward INV-EVAL-6)

Signal: `reify-audit --pattern PTODO` accepts the `#5023` citation; breadcrumb present at `run_compute_dispatch`.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `#5023` is a live, non-terminal task (PTODO cite validity) | task-state | task 5023 = `deferred` (async-recalc Phase A bookmark) — live, non-terminal | PASS |
| the load-bearing site exists | grep wired | `run_compute_dispatch` engine_compute.rs:177; confirmed **zero** `propagate_freshness_only` calls there (the gap the breadcrumb names) | PASS |
| `propagate_freshness_only` is KEPT (not deleted) | grep wired | engine_admin.rs:2075 method → freshness_walk.rs:130 free fn (both retained) | PASS |
| the PTODO gate exists to enforce the citation | reference | `docs/prds/reify-audit-ptodo-detector.md`; live tests/infra verify step | PASS |

---

**Aggregate:** all leaf bindings PASS. No `declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `producer-extent-short` / `fixture-ERROR` / `bound≤floor` / `rejection-absent`. Batch is clear to queue.
