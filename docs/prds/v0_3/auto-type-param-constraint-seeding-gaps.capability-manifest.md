# Capability manifest — `auto-type-param-constraint-seeding-gaps.md`

Mechanizes G3 + G6 for the **one** leaf this PRD queues (the Gap C honesty
diagnostic, §6). Per the PRD's §9 hard stop and Leo's decompose authorization
(2026-06-15), the Gap C full-fix const-folder (`cf-α`/`cf-β`, §7.1) and the
Gap D nested-member seeding (`nm-α`/`nm-β`, §7.2) stay **deferred** and carry
**no manifest bindings** until un-deferred.

Verified at decompose on `main`. The D3 decompose-verification workflow
(`scripts/prd-decompose-verify.mjs`) was run over this leaf; run id
`wf_65fbaba3-692` (transcript under
`.claude/projects/-home-leo-src-reify/.../workflows/wf_65fbaba3-692`).

## Evidence-form legend

`grammar-fixture:<path>` parses 0-ERROR · `grep:<file>:<line>` wired on the
production path · `producer:task-<N> upstream` in the transitive dependency
closure, covering the specific extent · `deliverable` = produced **by this
leaf** (not assumed substrate, so not probed-green now).

---

## Leaf (single) — Gap C honesty diagnostic

**Task signal (§6.3):** `examples/auto/bearing_computed_default_unevaluated.ri`
(a `Bearing<T: Seal>` whose constraint reads a **computed-default** cell), run
through `reify check`, emits a new `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`
warning naming the cell and constraint; a sibling fixture using a **literal**
threshold emits **no** such warning (negative control). Regression test asserts
both.

**Deps wired:** `depends_on 4596` (member-access-on-TypeParam node — pending),
`depends_on 4599` (3-site literal sibling-param seeding — done).

| # | Capability the signal asserts | Evidence | Verdict |
|---|---|---|---|
| 1 | Fixture syntax parses (computed `let`-cell default `let clearance : Length = bore_radius - 0.5mm`; computed `param` default `param max_stack = bore_radius * 2`; member access `seal.thickness`; constraint clause) — **no novel syntax** | `grammar-fixture:/tmp/prd-gate-fixtures/seeding-gaps-gapC.ri` → `tree-sitter parse --quiet` exit 0, 0 ERROR | **PASS** |
| 2 | A seeding pass that extracts literal defaults and **skips** non-literal ones exists (the skip-set the diagnostic reports is derivable here) | `grep:crates/reify-compiler/src/auto_type_param.rs:948` (`seed_candidate_value_map`) + literal guard `:996` (`CompiledExprKind::Literal`), called at the **three** ConstraintInput sites `:847 / :1566 / :2666` | **PASS** |
| 3 | Constraint→referenced-ref-cell map (to attribute a constraint to the skipped cell it reads) | `grep:crates/reify-compiler/src/auto_type_param.rs:2358` (`build_constraint_blame_map`) | **PASS** |
| 4 | A `DiagnosticCode` family with the severity/format contract that flows to every consumer (LSP hover, MCP `report_diagnostics`, CLI `reify check`) unchanged — the new `W_*` variant slots beside it | `grep:crates/reify-core/src/diagnostics.rs` — `AutoTypeParam{PoolOverflow,NoCandidate,Ambiguous,NonUnique,DepthBoundExceeded,CrossProductSizeExceeded,BoundedInfeasible,CandidateNotConstructible}` present | **PASS** |
| 5 | Member access on a `Type::TypeParam` receiver compiles to a `ValueRef` (so the demonstrable constraint's `seal.thickness` half is a real ref → the demo is a genuine candidate-disambiguating constraint, not artificial — §5.4) | `producer:task-4596 upstream` (wired `depends_on`; covers the expr.rs member-access-on-TypeParam extent exactly). **Pending**, so NOT probed-green now — `producer-upstream` is the correct evidence form | **PASS** |
| 6 | Literal sibling-param defaults are seeded at all three sites (so the only blame→skip intersection is a **genuinely** non-literal default — invariant §6.2.1, no false positives from unseeded literals) | `producer:task-4599 upstream` (wired `depends_on`; done, merged `54b053d444`) | **PASS** |
| 7 | The `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` code + skip-set collection + blame-map cross-reference + emit | `deliverable` — produced by this leaf; verified at impl-time by the leaf's own regression test (§6.3), which is gated behind #5/#6 so it can go green only once 4596+4599 have landed | n/a (deliverable) |

**G6 branch notes.**
- *Branch 1/2 (numeric / closed-form):* the signal asserts no numeric bound or
  exactness — N/A.
- *Branch 3 (end-to-end capability):* every capability the signal requires is
  delivered by this leaf (#7) or by an **upstream** prerequisite (#5 = 4596,
  #6 = 4599) — none by a task that depends on this leaf. No misattribution.
- *Branch 4 (rejection / negative):* the negative control ("literal-threshold
  fixture emits **no** warning") is **not** a substrate-rejection premise — it
  asserts the leaf's **own new** diagnostic does not false-positive on a literal
  default, which is controllable by the implementation (invariant §6.2.1) and
  verified by the same regression test. No absent-rejection-mechanism hazard.
- *Field-population:* N/A — the leaf reads no result field off a `Value`; the
  skip-set is computed from `CompiledExprKind` defaults and the blame map (both
  internal compiler substrate), so there is no `Value::Undef` sentinel twin.

**No FAIL bindings → leaf does not block the batch.**

## Deferred sketches — no bindings (recorded for the un-defer `/prd` re-run)

- **Gap C full fix (`cf-α`/`cf-β`, §7.1):** needs a compile-time const-folder /
  partial-evaluator — a mechanism that does **not** exist on `main` (PRD §4
  pre-conditions). G5 high-stakes (numeric/unit correctness, reference cycles,
  partiality) → re-enter `/prd` author, not a bare leaf. No binding until then.
- **Gap D (`nm-α`/`nm-β`, §7.2):** needs 4596 **plus** an unbuilt
  nested-member-access feature; today even the one-hop form is poison
  (`expr.rs` `make_poison_literal`). Filing a leaf now would freeze a RED test on
  an unbuilt feature (the esc-3436-210 anti-pattern). No binding until both
  substrates exist.
