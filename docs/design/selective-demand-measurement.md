# Selective-demand measurement artifact

**Task:** 4532 · **Date:** 2026-06-15
**Cross-ref:** `docs/prds/v0_6/selective-demand.md §G6 premise`

---

## 1. Purpose

This document records the G6 measurement answering two questions:

1. **Is the selective-demand win real?** Does an observed-demand cone built from
   spec §3.2 sources (viewport-visible realizations, property-editor cells,
   constraint-panel constraints) actually exclude a meaningful fraction of the
   production `eval_set` on typical interactive edits?

2. **Is coarse per-realization demand sufficient, or is fine per-cell demand
   needed?** Coarse means registering one `Realization` node per visible body.
   Fine means additionally registering every `Value` cell displayed in the
   property panel.

The measurement uses a **passive side-channel** that is already present in
production builds (task 4532, `DemandPruneMeasurement`).  Production evaluation
is **untouched** — the observed cone never feeds `compute_eval_set`, and the
byte-identity contract is pinned by tests in the harness.

---

## 2. What "observed demand" is

`Engine.observed_demand` is a `DemandRegistry` (the same type as the production
`Engine.demand` field) that the GUI populates via `sync_observed_demand` without
any impact on scheduling.  After each `edit_param` the engine computes:

```
DemandPruneMeasurement {
    eval_set_size:     |production eval_set|,
    observed_retained: |eval_set ∩ observed_cone|,
    would_prune: WouldPruneByKind {
        value, constraint, realization, resolution, compute
    }
}
```

`observed_retained + Σwould_prune == eval_set_size` by construction (the
measurement is computed from the final `last_eval_set`, after both eval waves
and early-cutoff).

The observed cone is a **backward closure** (BFS from registered roots following
dependency edges in reverse), identical in semantics to the production demand
cone.  Registering a `Realization` node pulls in the parameter values the kernel
would read, which in turn pulls in the parameters those values depend on.

---

## 3. Methodology

### Reproducible command

```
cargo test -p reify-eval --test selective_demand_measurement \
    emit_distribution_table -- --nocapture
```

Source: `crates/reify-eval/tests/selective_demand_measurement.rs`

### Spec §3.2 source mapping

| Spec source             | What is registered                         |
|-------------------------|--------------------------------------------|
| Viewport-visible body   | `NodeId::Realization(entity#realization[N])` |
| Property-editor cell    | `NodeId::Value(entity.cell_name)`          |
| Constraint-panel row    | `NodeId::Constraint(entity, index)`        |

### Scenarios

**Scenario A — bracket, body hidden.**
Single-body bracket (`Bracket` structure: 5 params, 3 constraints, 1 box
realization R0).  Observed demand = `{Value(Bracket.thickness)}` only — mimics
a property panel showing `thickness` with the realization viewport-hidden.
Scripted session: `thickness` → 0.003, 0.004, 0.005, 0.006, 0.004 m (5 edits).
The dirty cone of `thickness` is always `{volume, C0, C1, C2, R0}` (5 nodes).

**Scenario B — two-body module, one body visible.**
`TwoBody` structure (param `drive`, two independent box realizations `body_a` /
`body_b`, no constraints).  Observed demand = `{Realization(TwoBody#realization[0])}`
(body_a visible; body_b viewport-hidden).  Scripted session: `drive` →
0.011, 0.012, 0.010, 0.013 m (4 edits).  Both realizations are dirty on every
edit.

---

## 4. Measurement distributions (min / median / max over scripted session)

> **Generated tables — regenerate after any fixture or graph-semantics change.**
> The numbers below are emitted by the `emit_distribution_table` test in
> `crates/reify-eval/tests/selective_demand_measurement.rs`; they are **not**
> auto-pinned to the test output. Reproduce with:
>
> ```
> cargo test -p reify-eval --test selective_demand_measurement \
>     emit_distribution_table -- --nocapture
> ```
>
> If the bracket / two-body fixtures or the dirty-cone semantics change, re-run
> the command and update these tables so a reviewer catches drift here rather
> than trusting stale figures.

### Scenario A: bracket, body hidden — `thickness` property observed only

| Metric                  | min / median / max |
|-------------------------|--------------------|
| eval_set_size           | 5 / 5 / 5          |
| observed_retained       | 0 / 0 / 0          |
| would_prune.value       | 1 / 1 / 1          |
| would_prune.constraint  | 3 / 3 / 3          |
| would_prune.realization | 1 / 1 / 1          |
| **would_prune.total**   | **5 / 5 / 5**      |

`observed_retained = 0`: `thickness` is a leaf param; its backward closure
is just `{thickness}` itself, which is not a member of the eval set (the eval
set contains only downstream dependents of the edited param).  With only the
driving param registered, 100 % of the dependent compute would be pruned by
selective demand.

### Scenario B (observed): two-body, body_a visible

| Metric                  | min / median / max |
|-------------------------|--------------------|
| eval_set_size           | 2 / 2 / 2          |
| observed_retained       | 1 / 1 / 1          |
| would_prune.value       | 0 / 0 / 0          |
| would_prune.constraint  | 0 / 0 / 0          |
| would_prune.realization | 1 / 1 / 1          |
| **would_prune.total**   | **1 / 1 / 1**      |

body_a is retained; body_b's realization node is pruned on every edit.
`TwoBody` has no value or constraint nodes in the dirty cone of `drive`, so the
only prunable kind is `realization`.

### Scenario B (control): two-body, no observed registration

| Metric                  | min / median / max |
|-------------------------|--------------------|
| eval_set_size           | 2 / 2 / 2          |
| observed_retained       | 0 / 0 / 0          |
| would_prune.realization | 2 / 2 / 2          |
| **would_prune.total**   | **2 / 2 / 2**      |

With no observed demand, the entire eval set would be pruned (degenerate case,
shown for completeness — confirms the measurement correctly reports everything
as pruneable when no roots are registered).

---

## 5. G6 finding

### The win is real

| Session  | Nodes would-prune / total | Pruning rate |
|----------|---------------------------|--------------|
| Scenario A (5 edits × 5 nodes) | 25 / 25 | **100 %** |
| Scenario B (4 edits × 2 nodes) | 4 / 8   | **50 %**  |

For Scenario A the 100 % rate is a property of the graph structure: the
property panel shows a leaf driver param whose downstream dependents (including
the hidden body realization) are entirely outside the observed cone.  In a
real session where multiple cells are visible in the panel, the retained
fraction would grow — but the **realization node** (the most expensive
kernel-time item) is always pruneable when the body is viewport-hidden.

For Scenario B, registering a single realization node per visible body gives
exactly 50 % pruning on a 2-body module — one body's work is skipped
deterministically.  On an N-body model with k bodies visible, the expected
pruning rate is `(N-k)/N` for the realization nodes (the dominant cost).

### Coarse per-realization vs fine per-cell

| Granularity | Scenario B pruned realizations | Pruned values |
|-------------|-------------------------------|---------------|
| Coarse (realization per body) | 4 (body_b × 4 edits) | 0 |
| Fine (+ each property cell)   | 4 (same)              | 0 (TwoBody has no value cells in dirty cone) |

In the two-body scenario, coarse per-realization registration captures all
available pruning because the `drive` param's dirty cone contains only
realization nodes — there are no derived value or constraint nodes to
discriminate.

For the bracket (Scenario A), coarse-only registration of a viewport-hidden
body would give the same realization pruning (`would_prune.realization = 1`)
while adding fine per-cell registration of the property panel cells (e.g.
`volume`, `thickness`) would additionally retain those cells and reduce
`would_prune.value/constraint`.

**Recommendation for the `[MILESTONE]` task 4533 PRD expansion:**

> Use **coarse per-realization** as the primary demand source for
> selective scheduling: one `Realization` node per viewport-visible body.
> Its backward closure already covers the parameter cells that drive that
> body's geometry.  Fine per-cell demand for property-panel cells is a
> secondary refinement that reduces false-positives in `would_prune.value`
> (avoiding stale-value UI on displayed cells) but does not change the
> dominant realization-kernel saving.  The two granularities compose
> naturally: register realizations first, then add displayed cell ids on top.

---

## 6. Out of scope

This document records the **passive measurement precondition** only.
The following remain out of scope until their prerequisite tasks land:

- **Enforcing** selective demand in the scheduler (`compute_eval_set` still
  uses only the production `self.demand` cone).
- Demand-scoped driver seeding (θ **4361** / θ2 **4531**).
- `Pending{last_substantive}` surfacing for pruned-but-displayed cells.
- Kernel-time attribution per realization.
- Cold `build()` gating.
- The task 4530 staleness fix (hard prerequisite before any enforcement).
