# Driver-contract matrix — STRAWMAN for ruling

Status: draft 2026-08-26, spec-conformance program. Every cell is a PROPOSAL for Leo's
ruling; nothing here is decided. Derived from the cross-driver divergence survey
(`cross-driver-divergence-survey-draft.md`, same directory) — row cites (D*/DV*/S*)
refer to it and to the survey source reports.

`reify mcp-server` is EXCLUDED as a column: proposed for deletion (zero registered
consumers; broken since 2026-03 in immediately-user-visible ways — see survey D8/D15
and the deletion rationale in session notes). If it is kept instead, it re-enters as
a column that must reach GUI-context parity.

Legend:
- ✓  = owes this stage, and does it today
- ✓* = owes it; already ruled/chartered, fix pending (cite)
- ADD = proposed addition (absent today, no prior ruling)
- —  = excluded by role (proposal: ratify the exclusion explicitly)
- ★  = RULING NEEDED (genuinely open; consolidated in the numbered list below)

| Stage | check | eval/run | test | build | report | explain | doc | GUI (EngineSession) | LSP |
|---|---|---|---|---|---|---|---|---|---|
| 1. Parse (`parse_with_stdlib`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 2. Stdlib prelude at compile | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| 3. Multi-file imports (+cfg DAG) | ✓ | ✓* RU γ/5517 | ✓* RU δ/5518 | ✓* γ/5517 | ✓* δ/5518 | ✓* δ/5518 | ✓* δ/5518 | ✓ file / ✓* buffer (RU) | ✓* ζ/5520 |
| 4. cfg selection surface | ✓ `--cfg` | ✓* RU | ✓* RU | ✓* RU | ✓* RU | ✓* RU | ✓* RU | host-default (ruled v1; selector = UX task) | ★7 (no surface defined) |
| 5. Module-header rule (§7.1/7.2) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ADD ★5 | ADD ★5 |
| 6. Real compile checker (`SimpleConstraintChecker`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ADD (align w/ task 4437 intent; D10) |
| 7. Purpose activation | ✓ `--purpose` | ★4 | ★4 | ★4 | ★4 | ★4 | — | ★4 (no surface at all today) | — |
| 8. Solver (`auto` resolution) | ★1 (propose ✓; no recorded rationale for absence) | ✓ | — (ratify isolation) | ✓ RESTORE (esc-4458-87 inverted by `37bafd9acc`; #6631) | ✓ | ✓ | — | ✓ | — (ratify; attributable Indeterminate) |
| 9. Compute trampolines | — (ratified, main.rs:448-474) | ✓ | partial (no-morph, deliberate) | ✓ | ✓ | ✓ | — | ✓ | — (INV-FEA-1, ratified) |
| 10. Kernel for geometry realization | targeted → ✓* widen (5748) | ✓ | — (ratify) | ✓ | ★2 (kernel-free today; BOM mass cells undef, DV13) | ★2 (same posture) | — | ✓ | — |
| 11. Kernel-measured constraint arms (ReprWithin / GD&T / DFM) | ✓ | ✓ (via build) | — | ✓ | ★2 | ★2 | — | ReprWithin: ruled attributable-Indeterminate (precision PRD :307); GD&T/DFM: ★6 | — (same pattern) |
| 12. OpenVDB (isosurface / thickness-DFM) | ✓ (DFM arm) | ADD (scope accident, DV7) | — | ✓ | ★2 | ★2 | — | ADD ★6 (D8) | — |
| 13. Constraint pass run + verdict gating | ✓ (+ exit truth ✓* 5403/5404) | ★3 (asymmetric today, DV11 — propose uniform run+gate) | ★3 (Indeterminate⇒pass + diagnostics dropped, DV10) | ✓ | ★3 (never runs check; probed exit 0 on violated) | ★3 | — | ✓ panels (no exit gate — role) | ✓ diagnostics (parity = η/5521) |
| 14. FEA persistent cache | — | ✓ | — | ★2 (absent; re-solves, DV8) | ✓ | ✓ | — | ADD (D12: sweep exists, use never landed — comment-vs-code) | — |
| 15. Diagnostic codes in egress (decision 6: codes are the contract) | ADD `--json` | ADD | ADD | ADD | ADD | ADD | ADD | FIX (`code: None` strip, engine.rs:6943) | ✓ |
| 16. Exit/strict semantics | ✓* (5403 family; `--strict`) | ✓ | ★3 | ✓ | ★3 | ✓ | ★ doc exit-2 idiosyncrasy (ratify or normalize) | n/a (interactive) | n/a |

Special rows, outside the table:
- **GUI editor grammar (Lezer)**: governed subset with pinned ledger (D19) — proposal:
  ratify the ledger as the sanctioned-divergence mechanism; not a contract column.
- **Library/test-support tier (D20, S4)**: today compiles a language no product driver
  ships. Proposal: declare it NON-conformant (impl-test convenience), and build a
  check-faithful rung in the conformance crate for the suite's fast in-process tier.
- **GUI warm-edit incremental path**: single-implementor surface; contract = "warm edit
  ≡ full recompile" — a self-oracle (differential) test, no per-cell entries.

## Consolidated ruling questions (the ★s)

1. **Does `check` owe solver results?** Absence has no recorded rationale; `--strict`
   currently fails autos that eval/GUI resolve. Propose YES. (Build's restore is not a
   new ruling — esc-4458-87 already ruled it; #6631 carries the receipts.)
2. **Kernel/cache reach for build/report/explain**: does `report` realize geometry (or
   loudly banner "kernel-free, N cells unresolved")? Does `explain`? Does `build` get
   the FEA cache? Propose: report/explain gain eval's kernel routing; build gets cache.
3. **Constraint-verdict contract per driver**: does eval owe uniform run+gate (today:
   geometry-dependent accident)? What is `test`'s Indeterminate posture, and must it
   print dropped `TestResult.diagnostics`? Does `report` owe a check pass? Propose:
   eval uniform run+gate; test prints diagnostics + Indeterminate stays exit 0 but
   loudly counted; report runs check() and gates.
4. **Purpose**: ratify as check-only for now (spec marks activation check-scoped), or
   charter a GUI purpose surface? (Product question — purposes are invisible to the
   primary use case today.)
5. **Module-header enforcement in GUI/LSP**: the hardening PRD left this open. Propose
   enforce everywhere (it is a language rule, not a CLI convention).
6. **GUI measurement arms**: extend the ReprWithin attributable-Indeterminate ruling by
   analogy to GD&T-Conforms and DFM (and wire OpenVDB), or rule each separately?
   Propose ratify-by-analogy + wire OpenVDB.
7. **LSP cfg surface**: none exists; propose host-default like the GUI, ratified, with
   a workspace-config override deferred.

## Mechanical alignment items implied by already-made decisions (ready to file, no ruling needed)

- GUI `code: None` strip fix (decision 6; engine.rs:6943 — one projection function).
- LSP real-checker alignment (task 4437's own boundary intent; D10).
- GUI FEA-cache use (D12 — the sweep task's comment already promises it).
- eval OpenVDB wiring (DV7 — task δ/5002 scope accident).
- CLI `--json` diagnostics mode (conformance-suite prerequisite; decision 6).

## RULINGS (Leo, 2026-08-26 — matrix CLOSED; strawman above retained for provenance)

1. **check owes solver: YES.** (build's restore was already ruled — esc-4458-87 / #6631.)
2. **All-get-all RATIFIED**: default-full-engine — one shared engine constructor, with
   *named ratified subtractions* only. `test` gets kernels+solver (isolation = per-test
   module isolation, NOT capability starvation). `check` gets FEA trampolines —
   REVERSES the locked `check_fea_violated_constraint_is_not_gated` contract; retire
   that lock as part of implementation. Remaining subtractions: LSP keystroke-latency
   posture; `doc` compile-only. `explain` prerequisite fix: provenance must survive
   `build()` before it can take the kernel.
   **CITE CORRECTED 2026-08-26** (driver-contract-implementation authoring session,
   re-verified at main `9a992fc2f2`): the `engine_eval.rs:3884` anchor named here is
   wrong in two ways. That line is inside field elaboration at HEAD, and the empty
   provenance map it was meant to name lives in the **warm/cached serve path**, not
   `build()` — which constructs no eval result at all. The real gap is a struct-shape
   one: `Engine::check()` computes `objective_provenance` via `eval()` and then discards
   it, because `CheckResult` and `BuildResult` have no such field. That half is owned by
   `docs/prds/v0_6/solver-legibility-telemetry.md` leaf α; the kernel-routing half is
   `docs/prds/v0_6/driver-contract-implementation.md` leaf ζ.
3. **Constraint-verdict contract**: (a) every evaluating driver runs `check()`;
   (b) `eval` and `report` gate exit on violation; **`explain` warns on check failure
   but never gates** (ruled role difference); (c) `@test` Indeterminate ≠ pass — exits
   non-zero unless the test is explicitly annotated indeterminate-tolerant; `test`
   must print `TestResult.diagnostics` (the current drop is a bug).
4. **GUI purpose surface: CHARTERED.** Shape: extract the CLI's hand-assembled
   activation sequence (main.rs:693-824) into a shared `activate_purpose_session()`
   seam used by CLI and EngineSession; v1 = purpose selector + bindings form +
   purpose-scoped verdicts in panels; active purpose sticky across edits/recompiles;
   `set_purpose` debug-MCP tool; purposes activatable in GUI + check, other CLI
   drivers gain `--purpose` in the flag-unification wave. **Additional ruled UX
   requirement**: after activation, GUI elements tied to a purpose whose application
   is STALE (the warm-edit incremental path has fired at least once without the
   purpose being incrementally reapplied) render visibly stale (dimmer/greyer), and a
   prominent "reapply / recheck purpose" affordance appears.
5. **Module-header rule enforced everywhere** (GUI and LSP included).
6. **GUI measurement arms**: (a) OpenVDB wiring → task **#6666**; (b) ReprWithin stays
   per the precision-PRD ruling; (c) GD&T/DFM ratified-by-analogy attributable
   Indeterminate → task **#6667**; on-demand GUI measurement chartered as its own PRD
   (spawned session `prd:reify gui-on-demand-measurement`, brief at
   ~/.claude/spawn-briefs/prd-gui-on-demand-measurement.md).
7. **LSP cfg**: host-default by default; an optional `cfg` map honored in
   initializationOptions — specified now, wired when RU ζ/5520 lands.
   **BASELINE CORRECTED 2026-08-26** (same session): "host-default by default" reads as
   a description of today. It is not — the LSP constructs **no** `CfgSet` anywhere and
   its compile entry point takes no cfg parameter, so the ruling *introduces* the host
   default rather than making an existing one configurable. Note also that RU ζ/5520 is
   the *LSP multi-file diagnostics* leaf, not RU's cfg-surface decision (that is D-4,
   delivered by γ/5517 + δ/5518); the dependency named here is correct — ζ is what routes
   the LSP onto the cfg-bearing entry point — only its label is not.

Special rows, ratified: **Lezer ledger** = the sanctioned-divergence mechanism.
**Warm-edit ≡ full-recompile** differential self-oracle. **Library tier**: fix the
test-support environment itself to be check-faithful and use it for the conformance
crate — ratchet migration (shrinking baseline), loud-named internal rungs preserved
for bootstrap/internals tests, faithfulness pinned by a library-vs-real-`reify check`
parity gate, speed cost measured not assumed.

Also ratified: **delete `reify mcp-server`** → task **#6665**.

Next step: convert this ruled matrix into implementation chartering (driver-contract
PRD(s)). Cells already owned elsewhere: RU 5516-5529 (multi-file/cfg), 5403-family
(exit truth), #6190 (GUI η export), #6646 (viewport poses), #6665, #6666, #6667.
