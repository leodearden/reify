# Hotspot-program briefing for escalation triage / unblock sessions

**Audience:** /unblock and escalation-triage sessions handling tasks that may be impacted by the
2026-07-06 hotspot-hardening program. Read this BEFORE resolving an escalation on any task that
touches eval, engine_build, geometry kernels, the compiler type layer, the GUI bridge, FEA/compute,
or the LSP. Statuses below are **as of 2026-07-06** — always re-query fused-memory for current
task state; this doc records decisions and constraints, not live status.

**Canonical sources (in order of authority):**
- `docs/invariants.md` — the INV-* registry + Leo's ratified fail-closed enforcement posture.
- The eight wave PRDs: `docs/prds/v0_6/{eval-cell-commit-substrate, engine-build-hardening,
  compiler-type-hygiene, ai-native-editing}.md`, `docs/prds/{kernel-seam-contracts,
  compute-fea-hardening, godfile-test-eviction, gui-state-sync*}.md` (+ capability manifests).
- `docs/notes/bug-hotspot-survey-2026-07-05.md` — the underlying evidence (7-agent review).
- fused-memory: `search(query="hotspot-program decisions ledger", project_id="reify")` for the
  full decision rationale; `search_tasks` before filing anything new.

## What happened (one paragraph)

A systematic bug-hotspot survey (git history + task tracker + 7-agent code review) identified the
structural causes of the repo's recurring bug classes. Leo ratified a hardening program: ~90 tasks
(ids **5026–5117**) across 7 hardening waves + 1 deferred feature PRD, every task citing a
registry invariant (INV-*) whose enforcement mechanism is part of its done-criteria. Execution is
in flight under the orchestrator.

## Postures that change how you triage

1. **Fail-closed is the ratified end-state everywhere.** New validators/gates land as: spec →
   one-shot warn-mode corpus sweep → fix bulk producers → flip to enforce, each with a break-glass
   env knob (e.g. `REIFY_MESH_CONTRACT=warn`). **A new Fatal Validation Error or newly-failing
   compile diagnostic is often WORKING AS INTENDED** — before treating a failure as a regression,
   check whether the failing check is a new INV enforcement (the error/diagnostic names its
   contract). Fix the violation, not the gate. Break-glass only to unblock an urgent landing, and
   file the violation.
2. **New compile diagnostics are landing by design**: trait type-args rejection (#5049) and
   Mul/Div operand typing (#5052→5061→5063) will make previously-"compiling" fixtures fail. The
   Mul/Div fix also changes constraint behavior: expressions like `s > 0` downstream of scalar
   arithmetic that were silently Indeterminate may start evaluating. That is the fix, not a break.
3. **Every hotspot task cites its INV-id.** When resolving its escalation, the enforcement
   mechanism is part of done — a task that lands the mechanism but not its test/lint/flip is not
   done. Registry status flips in the same change that lands enforcement.

## Structural changes in flight — check these before trusting a pre-existing task's premise

- **God-file test evictions (#5026 geometry_ops.rs, #5027 engine_build.rs — CRITICAL priority,
  intentionally wide-lock, designed to land first).** Wholesale test-module moves. If a branch
  conflicts with them: take the moved location; the test text is relocated verbatim.
  engine_build.rs has 7 separate named test modules with a live production fn
  (`p2_substitution_diagnostic`, cited #4744) interleaved — flagged in #5027.
- **The concurrent stack is being DELETED** (ξ #5046 re-homes real test pins → ο #5065 deletes
  `reify-runtime/src/concurrent{,_eval}.rs`, `reify-eval/src/concurrent.rs`). Zero production
  callers. Any escalation premised on those files (including the known bare-context bug in the
  wave-1 adapter) → the answer is delete-not-fix; check ο's status first.
- **`edit_source` is FROZEN.** Zero production callers, ~1450-line parity copy. Its wire-vs-delete
  is Leo's decision, owned by π **#5050** (a pending milestone that escalates to Leo once the eval
  P1 chain lands; recommended default = delete if unanswered by 2026-08-06). Do NOT invest fixes
  in edit_source; do NOT delete it preemptively.
- **Eval cell-commit migration (P1 chain: α #5038 → β #5039 → γ/δ/ε #5053/#5056/#5057).** Once
  landed, `commit_cell_result` + `CellEvalCtx` are the ONLY sanctioned way to evaluate-and-commit
  a cell (INV-EVAL-1/2). A task hand-rolling values/snapshot/cache/journal writes or a bare
  `eval_ctx_with_meta` at an engine cell-eval site should be rebased onto the primitive, not
  merged as-is. Expect merge conflicts in engine_eval.rs/engine_edit.rs for in-flight tasks —
  rebase onto the primitive's shape.
- **#4351 premise changed (2026-06-24, re-confirmed at decompose):** FeatureTagTable is being
  DELETED by #4827; only TopologyAttributeTable gets the KernelHandle re-key. #4351 is adopted
  into engine-build-hardening (edges: 4351→#5059; #5064/#5071 depend on it).
- **#4935 (reify-eval-fea crate move) is deliberately gated** behind 12 FEA-hardening tasks that
  anchor to the files it relocates (pure ordering edges, recorded in compute-fea-hardening PRD
  Open Questions). Do not "unblock" it by removing those edges without Leo.
- **#4876 was re-scoped and adopted** as the kernel-seam preflight leaf (ν #5115 → #4876 → ξ
  #5116): its fix is now "MeshContract preflight → Err → graceful degrade to plain producer",
  fail-closed immediately (the alternative is an uncatchable SIGSEGV). Its files metadata was
  corrected to `mesh_boundary.rs` + `repair.rs` (the previously-cited test file doesn't exist).
- **GUI:** the six copy-pasted "emit quintets" collapse into `post_engine_call_telemetry`
  (#5030); new GuiState fields must be classified or the coverage lint (#5029) fails — that lint
  failing on a feature branch means classify the field, not weaken the lint. The dead `reify_`-
  prefix sidecar interception is being deleted (#5037) — do not resurrect it; the real AI-tools
  feature is the deferred ai-native-editing PRD. The sidecar system prompt was already fixed
  (#5036, merged).
- **LSP/FEA:** the trampoline-free LSP posture is being documented + test-locked, with a
  per-constraint "not evaluated in editor" hint (#5077/#5078). "FEA constraint shows no squiggle"
  is intended; do NOT register compute trampolines in the LSP path. Engine construction should go
  through `Engine::new_production` once #5072 lands — an escalation shaped like "FEA result is
  all-Undef / assertions pass meaninglessly" is the registration-omission class; check the
  engine's construction site first.

## Things that look stuck but are NOT stuck

- **#5117** (ai-native release gate): pending, dependency-gated on 31 wave-terminal tasks. Its
  dispatch far in the future is the design. Never mark it done, never implement it; on dispatch
  it verifies + escalates to Leo, who releases.
- **#5050** (edit_source π): same pattern — pending until the P1 chain lands, then escalates.
- **Deferred bookmarks by design:** #5023 (async-recalc Phase A), #5024 (generic-trait type-args
  language design), #5028 (god-file stage-3 splits), #5068 (Wave-3 builtin registry). Leave
  deferred; they are PRD slots, not stalled work.
- **ai-native batch #5094–#5101:** blocked behind #5117 deliberately.

## Priority + hygiene guidance for triage

- **#4954** (geometry lets emit first-class value cells — the proven root cause of the entire
  stale-Undef class, INV-EVAL-5) is blocked at time of writing and is the single highest-value
  unblock in the backlog. Prioritize it when it surfaces.
- **Dup-check before filing:** ~90 hotspot tasks exist; an escalation's "fix X" follow-up very
  likely already has an owner in 5026–5117. `search_tasks` first; wire a real `add_dependency`
  edge to the owner instead of filing a twin.
- Survey "corrections" worth knowing so you don't misread churn: `persistent_cache.rs`'s heavy
  commit history = young disciplined TDD feature, not chronic bugs; `reify-constraints/solver.rs`
  churn = inherent Nelder-Mead tolerance tuning — do not file architectural fixes there.
