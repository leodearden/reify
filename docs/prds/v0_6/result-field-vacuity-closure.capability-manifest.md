# Capability manifest — result-field-vacuity-closure

**PRD:** `docs/prds/v0_6/result-field-vacuity-closure.md` · **Decomposed:** 2026-08-31 ·
**As-of:** main `4dc3e1da77` (2026-08-31; the PRD's own landing commit — anchors by symbol).

Machine-readable twin: `result-field-vacuity-closure.capability-manifest.yaml` (stamped with real
task ids by `commit_planning`; the sidecar is normative for `delivered_check` descriptors — this
file is the human-readable mirror).

Every deliverable-shaped `delivered_check` was **executed against the tree at decompose time and
confirmed RED today** (flips green only on delivery); every substrate binding was confirmed PASS by
measurement, not assertion.

**D3 adversarial verification ran and initially BLOCKED the batch** (workflow `wf_0f28544c-9dc`,
16 findings). Resolution before filing, recorded in the PRD's amended sections: the δ↔γ reorder
(ADV-γ-7), D8 allowlist-is-the-baseline pinned (ADV-γ-9), D9 stats-plumbing carve-out (ADV-γ-8,
correcting this manifest's original "same extension shape" claim), the §2.3 mechanism-modal
fake-value family re-bucketed to allowlist-owned-by-#7012 with a #7012 extent rewrite
(ADV-β-1/-4), `.topology` moved to the undeclared-write registry (ADV-β-3), V8's predicate
sharpened (ADV-β-5), V7/V8 pinned not-release-gated (ADV-β-6), and the ζ-owner discharge closing
ADV-β-7. ADV-γ-1/2/3 were disposed as harness-scope (the α probe harness cannot execute
`reify-audit`; see the `premise-probe-harness-disposition` capability). Adversary fixtures authored as
evidence: `tests/prd-gate/fixtures/adv_beta_v7_degraded_arith.ri`, `adv_beta_v7_pm_is_real_zero.ri`,
`adv_beta_undef_arith_control.ri` — their landing exposed a further measured defect: git's
pre-commit hook env (`GIT_INDEX_FILE`/`GIT_DIR`) leaks into the PTODO infra test's hermetic fixture
repos, deterministically redding any main commit that adds tracked source (6/22 subtests fail under
the hook env, 22/22 green outside it). **#7106** fixes the leak and lands these three fixtures (their
contents are embedded verbatim in that task). After these amendments no binding resolves to a FAIL
value — the batch queues clean.

## Leaf table

| Label | Task | Capability bindings |
|---|---|---|
| α | (stamped in yaml) | declaration mechanism (INV-PD-2 C1′) following #7079's syntax convention |
| β | (stamped in yaml) | modal family declared; V7/V8 value-honesty boundary tests (not release-gated); degenerate-builder honest flips |
| δ | (stamped in yaml) | full ~22-file producer-family sweep + declarations — **intermediate, precedes γ** (ADV-γ-7) |
| γ | (stamped in yaml) | `reify-audit --pattern PVAC` + allowlist + D9 stats dispatch + same-diff infra registration |
| ε | (stamped in yaml) | doc-truth cleanup (stale `task 4578` prose; false θ/ι consumer claim) |
| ζ-owner-part-topology | (stamped in yaml) | discharge: live owner for `.part`/`.topology`/re-echo entries |
| ζ-bookmark-ptodo-prose-cites | (stamped in yaml) | discharge: PTODO prose-cite grammar bookmark (D6) |
| η | (stamped in yaml) | PRD-close terminal stamp + census chartered→shipped refresh |

## Bindings (mirror of the yaml; the yaml is normative)

**α — pdrop-declaration-syntax-convention-upstream** (PASS): syntax decided by **#7079** (pending,
verified 2026-08-31); real `depends_on` edge wired. Check: `manual` — no grep can be authored
against an undecided form without manufacturing the esc-6739-1 false-red shape; γ's V1–V6 are the
behavioural cover. **no-new-diagnostic-code-in-v1** (PASS): D2 gate-only — deliberate contrast with
PDROP α; `manual` (bare-token absent-check would violate the scoping rule).

**β — modal-producer-sites-wired** (PASS): `placeholder_part` + five echo sites,
`build_modal_topology_value`, `degenerate_modal_result` all on the production path in
`modal_ops.rs`, grep-verified. **undef-degraded-convention-exists** (PASS): the honest form is
already the house convention (degenerate `damping`, buckling `pre_stress`) — V7's premise is
achievable on main today. **degraded-cite-7012-live** (PASS): #7012 pending, verified.
**part-topology-allowlist-owner-live** (PASS): owner filed at this decompose (ζ discharge — the
whole-printer-modal session had no filed tasks at decompose time, checked twice). All `manual`:
substrate/task-store facts whose continuous check is PVAC failure mode (c).

**γ — pattern-enum-substrate** (PASS, corrected per ADV-γ-8): `pub enum Pattern` + per-pattern
module family live — the additive shape covers failure modes (a)–(d) only; the (e) vacuity floor
needs the D9 stats plumbing (PTODO's `check_with_stats` never reaches the bin dispatch, measured).
Unknown-pattern CLI exit is 125, distinct from findings-exit 1 — γ's tests must discriminate.
Check: grep `P[Vv][Aa][Cc]` in `crates/reify-audit/src/lib.rs`, expect **present** — case-tolerant
deliberately (variant casing is γ's call); RED today, verified. The yaml adds
`premise-probe-harness-disposition` (γ premises are not α-probe-bindable) and β adds
`mechanism-modal-fake-family-allowlisted-to-7012` + `v7-v8-not-release-gated` — see the yaml for
their full text.
**infra-ratchet-registration-same-diff** (PASS): the esc-4914-162 rule — registration in γ's own
diff. Check: grep `[Pp][Vv][Aa][Cc]` in `tests/infra/run-all-classification.manifest`, expect
**present**; RED today, verified. **liveness-lane-and-pdrop-machinery-upstream** (PASS):
`fused_memory_client.rs` live; **#7085** pending, real `depends_on` edge; extract-and-share, never
copy. **vacuous-scan-floor** (PASS): C4′(e) with V9 as the negative control.

**δ — producer-family-enumerable** (PASS): the `StructureTypeId(u32::MAX)` grep measured 14
`reify-eval` + 8 `reify-stdlib` files (151 sites) at `90e27653bb`. δ is **intermediate and
precedes γ** (ADV-γ-7 — the reverse order falsifies γ's real-tree green). Check: `manual` —
coverage is enforced behaviourally and continuously by γ's V6 exit-0 under the C4′(e) floor, which
is stronger than any static file list (files move; the floor doesn't).

**ε — stale-prose-present-today** (PASS): 5+1 hits of `task 4578`, 1 hit of the θ/ι claim, all
verified present. Checks: grep `task 4578` (both files) expect **absent** — accepted gap named in
the yaml: forbids the malformed prose form only, canonical `#4578` mentions stay legal, so fix and
check cannot race; grep `locate the DOF assembly` expect **absent**. Both RED today (targets
present), verified.

**ζ discharges** (PASS ×2): the tasks ARE the deliverables; `manual`. The owner task carries the
#7074-precedent clause — **no PRD leaf wires an edge onto an allowlist owner** (it would invert the
liveness contract).

**η — terminal-stamp** (PASS): check grep `\*\*Status:\*\*.*SHIPPED` in the PRD, expect
**present** — construct-anchored so the §10 row's prose mention of SHIPPED cannot satisfy it
(executed: no match today; not description-matchable). **census-status-refresh** (PASS): grep
`PVAC.*shipped` in `docs/legibility/design-invariants.md`, expect **present**; RED today (row reads
`chartered`).
