# E3 coarse-arm re-decomposition — gui-on-demand-measurement

Part of experiment **E3** (`docs/prds/v0_6/e3-decomposition-granularity-ab.md`). This PRD
randomized to the **coarse arm**: its standard decomposition (9 leaves, #6740–#6748, decomposed
2026-08-26) is retired to `deferred` and replaced by the 4 coarse tasks below. The coarse tasks
carry the full text of their constituent standard leaves — nothing is dropped; the constituent
Greek labels and original task ids are kept for traceability. Integration-gate and PRD-close
leaves are preserved as singletons (reused in place, not re-filed), per both the E3 protocol and
the reify /prd overlay's decompose-close obligation.

**Mapping** (standard → coarse; ids filled at filing):

| Coarse task | Constituents | Reduction |
|---|---|---|
| GOM-C1 | α #6740 + β #6741 + γ #6742 | 3→1 |
| GOM-C2 | δ #6743 + ε #6744 + θ #6747 + η #6746 | 4→1 |
| GOM-C3 (reused #6745) | ζ #6745 — integration gate, unchanged | 1→1 |
| GOM-C4 (reused #6748) | ι #6748 — PRD close, deps rewired to C1/C2/C3 | 1→1 |

9 leaves → 4 tasks (2.25×). Out-of-PRD dependency edges preserved: C1 ← #6667, #6723;
C2 ← #6666. Declared-file unions: C1 = 12, C2 = 11.

---

## GOM-C1 (α+β+γ): measurement seam, GUI session measurement pass, staleness epochs

**Deps:** #6667, #6723 (out-of-batch; β's edges). **Priority:** high.
**Files (12):** crates/reify-cli/src/main.rs, crates/reify-eval/src/lib.rs,
crates/reify-eval/src/engine_admin.rs, crates/reify-eval/src/engine_build.rs,
gui/src-tauri/src/engine.rs, gui/src-tauri/src/debug_server.rs, gui/src-tauri/src/main.rs,
gui/src-tauri/src/commands.rs, gui/src-tauri/src/types.rs, gui/src/types.ts,
gui/src/__tests__/debugParity.test.ts, docs/debug-mcp-contract.md

**Task text (description) as filed:** this coarse task is the whole engine/session substrate of
`docs/prds/v0_6/gui-on-demand-measurement.md`: the α seam extraction + capture/refine flag
split, the β GUI session measurement pass + Tauri command + debug-MCP tools + measured-verdict
payload, and the γ staleness epochs. Plan it as one branch with three internal phases in
constituent order α → β → γ (β needs α's seam; γ needs β's session store). Every binding
amendment (A1–A12) and G7 addition of the constituents applies unchanged. The full constituent
text is embedded in the filed task record (see task store; text identical to the three retired
standard leaves #6740/#6741/#6742).

Combined user-observable signal: all three constituent signals hold —
(α) the existing `reify check` CLI harness suites pass UNMODIFIED plus a seam-level test driving
the GUI-shape invocation (refine=off) to check-equal verdicts; (β) via debug-MCP against a
debug-build GUI, `measure_constraints(wait=true)` then `measurement_status` and the
constraint-panel state show MEASURED verdicts matching `reify check` on the same file; (γ) BT-5 —
measure, warm `edit_param`, payload retains the measured verdict with stale=true, re-measure
clears it, epochs monotone.

**Amendments recorded on the filed task after this spec was written** (the task record is
authoritative; listed here so the spec does not read as complete):
- **A5 RULED 2026-08-28 (Leo)** — the β constituent's C-STATUS ambiguity resolves to the
  STRUCTURED CHANNEL for all three kernel-measured kinds; the v1 verdict-only scope-cut is
  explicitly rejected.
- **A5-R 2026-09-01** — the carrier must be dimension-bearing (deviation/min-wall are Lengths,
  overhang/draft are Angles; a bare `Option<f64>` is the INV-AD-4 erasure), and the field is a
  sibling on `ConstraintCheckEntry`, never a payload on `Satisfaction`, per
  solver-legibility-telemetry §8.1 item 2. Two scope cuts named. Mirrored to retired leaf #6741
  and logged in the experiment's §10.

## GOM-C2 (δ+ε+θ+η): frontend affordances, thickness leg, companion corrections, docs-truth

**Deps:** GOM-C1; #6666 (out-of-batch, ε's edge). **Priority:** high (max of constituents).
**Files (11):** gui/src/panels/ConstraintPanel.tsx, gui/src/panels/ConstraintPanel.module.css,
gui/src/panels/StatusBar.tsx, gui/src/panels/ChatPanel.tsx, gui/src/stores/engineStore.ts,
gui/src/__tests__/ConstraintPanel.test.tsx, gui/src/__tests__/StatusBar.test.tsx,
docs/prds/v0_6/precision-nominal-representation-guarantee.md,
crates/reify-mcp/src/tools/chunks/constraints.md, crates/reify-mcp/src/tools/chunks/stdlib.md,
.claude/skills/reify-design/SKILL.md

**Task text as filed:** the designer-facing surface + closure work of the PRD: δ (debounced idle
auto-measure, Measure-now, stale rendering, progress state; A11 as revised — the casing fix is
#6723's, verify only), ε (thickness-DFM / OpenVDB leg under the GUI configuration + BT-1's
thickness parity row per A14), θ (precision PRD §4.6 correction note, amend #6171 to gate on the
refine flag, verify the #6667→#6169 edge), and η (docs-truth: doc-chunk update, reify-design
index line, discoverability acceptance). Internal order: δ ∥ ε first, then θ (needs α+β landed —
satisfied by the C1 dep), then η last (docs describe the landed behaviour). ε's metadata.files
deliberately excluded from the union ([] in the standard leaf — footprint depends on β's landed
shape). η and θ contain docs-only landing instructions written for standalone dispatch; within
this coarse task they land on the SAME task branch as the code (merge queue), which is sanctioned
— the docs-direct-on-main instruction applies only when a change ships alone.

Combined user-observable signal: all four constituent signals hold — (δ) edit → idle delay →
measured verdict appears with no user action; further edit dims it and raises the re-measure
affordance (vitest via scripts/gui-test.sh + a debug-MCP runtime test); (ε) an isosurface design
with a min_feature_size DFM rule shows the same measured thickness verdict in the GUI as
`reify check`, and a no-OpenVDB build yields attributable unmeasured(cause), never a false
verdict; (θ) the committed precision-PRD §4.6 dated correction + updated #6171 record + #6667
edge confirmed; (η) discoverability acceptance on the doc chunks.

## GOM-C3 = #6745 (ζ) — integration gate, reused unchanged

Kept as its own task per the E3 rule "keep any integration-gate task the standard decomposition
has". Deps rewired: was {β #6741, γ #6742} → now {GOM-C1}. Text untouched.

## GOM-C4 = #6748 (ι) — PRD close, reused unchanged

Deps rewired: was {all 8 siblings} → now {GOM-C1, GOM-C2, GOM-C3}. Text untouched. Its
"leaf-ID backfill is not this leaf's job" note still holds; the E3 mapping table above is the
authoritative standard→coarse record.
