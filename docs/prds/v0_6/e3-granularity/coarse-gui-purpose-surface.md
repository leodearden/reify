# E3 coarse-arm re-decomposition — gui-purpose-surface

Part of experiment **E3** (`docs/prds/v0_6/e3-decomposition-granularity-ab.md`). This PRD
randomized to the **coarse arm**: its standard decomposition (10 leaves, #6803 + #6831–#6839,
decomposed 2026-08-27) is retired to `deferred` and replaced by the 5 coarse tasks below.
Integration-gate (η) and PRD-close (κ) leaves are reused in place as singletons.

**Mapping:**

| Coarse task | Constituents | Reduction |
|---|---|---|
| GPS-C1 | α #6803 + β #6831 | 2→1 |
| GPS-C2 | γ #6832 + δ #6833 + ε #6834 | 3→1 |
| GPS-C3 | ζ #6835 + θ #6837 + ι #6838 | 3→1 |
| GPS-C4 (reused #6836) | η — integration gate, deps rewired to C1/C2/C3 | 1→1 |
| GPS-C5 (reused #6839) | κ — PRD close, deps rewired to C1–C4 | 1→1 |

10 leaves → 5 tasks (2.0×). Out-of-PRD edges preserved: C1 ← #5748, GOM-C1 (was #6740);
C2 ← #6723, GOM-C1 (was #6742); C3 ← GOM-C2 (was #6743 and #6746).
Known inbound edges (standard-arm driver-contract): #6773 and #6804 depended on #6803 → remapped
to GPS-C1; #6807 depended on #6837 → remapped to GPS-C3.

## GPS-C1 (α+β): activate_purpose_session seam + GUI routing, commands, intent

**Deps:** #5748 (in-flight on cmd_check), GOM-C1 (was #6740 — the measurement seam).
**Priority:** high.
**Files (5):** crates/reify-cli/src/main.rs, crates/reify-eval/src/lib.rs,
gui/src-tauri/src/engine.rs, gui/src-tauri/src/main.rs, gui/src-tauri/src/commands.rs

α (behaviour-preserving extraction of activate_purpose_session() into reify-eval; both
enumerations' obligations preserved; typed PurposeActivationError; cmd_check purpose branch
rewired byte-identically — the G4 reservation of cmd_check/finish_check/exit codes to
check-diagnostic-truthfulness stands untouched) then β (route BOTH GUI paths through the seam;
set_purpose/clear_purpose Tauri commands on the set_active_fea_case shape; purpose INTENT
reconciled at commit_state with a coded vanished-purpose diagnostic; join #6742's
generation-counter set — under E3 that counter is GOM-C1's γ constituent; surface
active/declared purpose lists on GuiState). Internal order α → β.

Combined signal: (α) the full `reify check` corpus including all 59 --purpose assertions is
byte-identical pre/post rewire, and reify-eval exposes the seam; (β) BT-4/BT-7 shape — an
activated purpose's constraints appear in the panel and survive a source edit; deleting the
declaration raises a coded diagnostic naming it.

## GPS-C2 (γ+δ+ε): verdict partition + epochs, set_purpose debug tool, selector + binding picker

**Deps:** GPS-C1, #6723 (casing fix, γ's edge), GOM-C1 (was #6742 — the generation counter).
**Priority:** high (γ's).
**Files (8):** gui/src-tauri/src/types.rs, gui/src-tauri/src/engine.rs, gui/src/types.ts,
gui/src/stores/engineStore.ts, gui/src-tauri/src/debug_server.rs, gui/test/visual/assertions.ts,
gui/src/App.tsx, gui/src/stores/viewStateStore.ts

γ (purpose partition on ConstraintData + purpose_applied_epoch; fix build_constraints'
unwrap_or_default blank rows via CompiledPurpose.constraints; #6722 margin-field coordination
note applies), δ (set_purpose debug-MCP tool on set_fea_case's six-piece shape with all four
registration guards and the mandatory delta-baseline refresh), ε (SolidJS purpose selector +
entity-binding picker; prelude purposes listed and grouped; replace App.tsx's hardcoded `[]`
with the live active-purpose list; D10 no-viewport-change). Internal order γ → (δ ∥ ε).

Combined signal: (γ) BT-9 — a purpose constraint shows real expression text, cross-highlights
parameters, attributed to its purpose; (δ) a scripted debug-MCP session activates via
set_purpose, reads back active_purposes/purpose_stale/generation, and the next normal command
diffs against a correct baseline; (ε) BT-10 — any .ri file offers simulation_ready/design_review;
activating one makes an auto:purpose view appear with the active view/meshes/camera unchanged.

## GPS-C3 (ζ+θ+ι): ruled staleness UX + purposes-chunk docs-truth + companion corrections

**Deps:** GPS-C2, GOM-C2 (was #6743 — shared stale token; and #6746 — the sibling docs leaf θ
extends), GPS-C1 (ι's α edge, transitively satisfied but kept explicit). **Priority:** medium.
**Files (9):** gui/src/panels/ConstraintPanel.tsx, gui/src/panels/ConstraintPanel.module.css,
gui/src/App.tsx, crates/reify-mcp/src/tools/chunks/purposes.md,
.claude/skills/reify-design/SKILL.md, docs/prds/v0_6/purposes-completion.md,
docs/notes/cross-driver-divergence-survey-draft.md, docs/notes/purpose-reflective-aggregation.md,
docs/prds/v0_6/gui-on-demand-measurement.md

ζ (ruling-4 staleness UX: retained + dimmed verdicts, one "Reapply purpose" affordance, converge
on the one visual token, is_stale name-collision avoided; the 2026-08-27 correction stands —
#6723 owns the casing fix), θ (purposes.md chunk: GUI activation in intent terms, remove the
false manufacturing_ready stdlib claim, document the measured-constraint-in-a-purpose trap,
extend #6746's index line), ι (companion corrections (b)–(e) + the revised item (a): re-measure
the ReprWithin residue at dispatch; file against driver-contract wired to #6773 ONLY if it
escaped #5748/#6803-scope/#6773 — under E3, the #6803 half of that test reads "GPS-C1's α
constituent"). Internal order ζ → (θ ∥ ι). Docs-only constituents land on this task's branch
through the merge queue (sanctioned — the docs-direct-on-main instruction applies to standalone
docs changes only).

Combined signal: (ζ) BT-5 shape — dragging Ball.wall dims purpose rows, Reapply restores fresh
verdicts equal to a fresh session's, epochs monotone; (θ) the purposes chunk matches
determinacy_purposes.ri exactly and warns off the trap; (ι) the residue re-measurement finding
recorded (or the escaped-clause task filed), and the four cited documents no longer contradict
the ruled ownership.

## GPS-C4 = #6836 (η) — B+H integration gate BT-1..BT-11, reused unchanged

Deps rewired: was {α,β,γ,δ,ε,ζ} → now {GPS-C1, GPS-C2, GPS-C3}. Text untouched.

## GPS-C5 = #6839 (κ) — PRD close, reused unchanged

Deps rewired: was {all 9 siblings} → now {GPS-C1, GPS-C2, GPS-C3, GPS-C4}. Text untouched.
