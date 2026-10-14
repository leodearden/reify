# Capability manifest — solver legibility & telemetry (GUI + debug MCP)

Companion to `docs/prds/v0_6/solver-legibility-telemetry.md`. Mechanizes **G3**
(assumed-substrate verified) and **G6** (premise validity) per leaf, so the substrate
check is paid once here rather than once per task at dispatch.

**Evidence verified against main `2128c3692cbb88f59b6e9edfd25ee801513423bb`**
(2026-08-26), by execution or by `git grep`. Main moves fast — cite-by-symbol;
re-locate at implementation time. **Exception:** the three bindings added by #6751
(`completeness-field-owner-is-p3-zeta` on ξ, `p3-multimodality-slot-cites-6706` and
`p3-multimodality-slot-cites-6711` on ω) were verified against baseline
`cdc501a3f1` (2026-08-27), which is the HEAD their zero-hit RED measurement was taken
at.

Machine-readable twin: `solver-legibility-telemetry.capability-manifest.yaml`.
Its `delivered_check` bindings are the **post-delivery** state (pattern-anchored ERE,
never `file:line`); `kind: manual` entries are recorded but excluded from the
dispatch gate. Every `grep` check in the sidecar was executed at authoring time and
confirmed to resolve in the asserted direction. Two of them (β's verdict token and
δ's mount gate) assert the **post-delivery** state and therefore fail *today* by
design — they are delivery assertions, not evidence of the current defect, which the
`binding` column records in prose instead. The three #6751-authored bindings named
above are a **third** kind, and a green result on them must be read accordingly: they
are docs assertions already satisfied by #6751's own diff, not by their leaf's
delivery. What they buy is *anti-rot* — each was RED at `cdc501a3f1` and goes RED
again if the text it anchors on is removed or reworded, which is the whole point of
re-running them at ω's terminal stamp (PRD §12.5).

**Gate verdict: PASS.** 17 leaves, 56 capability bindings, no binding resolving to
`declared-only`, `test-only`, `producer-absent`, `producer-downstream`,
`producer-extent-short`, `fixture-ERROR`, `bound<=floor` or `rejection-absent`.
No numeric bound is asserted anywhere in this PRD, so the G6 floor check does not
fire. Three false premises were caught at authoring time and are recorded in the PRD
§12 as **forbidden assertions** rather than as failed bindings: `ProvenOptimal` is
unreachable in production, no iteration count escapes `reify-constraints`, and
`candidates[1..]` are undeduped.

One capability is legitimately **producer-absent today and delivered by its own
leaf** — λ's `pub(crate)` observational accessor for `last_check()`. That is not a
gate failure: the leaf that needs it is the leaf that adds it, precedented by
`EngineSession::engine()`.


---

## α (#6721) — P4-alpha: carry objective_provenance through CheckResult and BuildResult

| Capability | Verdict | Evidence |
|---|---|---|
| `provenance-populated-on-cold-eval` | **PASS** | capability→producer (anti-orphan) — Engine::eval populates objective_provenance at two production sites (the per-template Solved arm and dispatch_merged_cluster_solve). Not test-only. This is the data alpha stops discarding.<br>`grep:objective_provenance` → `present` in `crates/reify-eval/src/engine_eval.rs` |
| `narrowing-site-is-real` | **PASS** | signal premise (the defect alpha closes) — Engine::check builds CheckResult from five fields and objective_provenance is not among them, so the field is computed by the inner eval() and dropped at the EvalResult->CheckResult boundary. Verified by reading the CheckResult construction 2026-08-26 (git grep for objective_provenance in engine_constraints.rs returns nothing today). The delivered_check is the POST-delivery state: after alpha the symbol must be present at that seam.<br>`grep:objective_provenance` → `present` in `crates/reify-eval/src/engine_constraints.rs` |
| `buildresult-lacks-field` | **PASS** | signal premise — BuildResult declares values/constraint_results/geometry_output/diagnostics/resolved_params only.<br>_manual_ — a struct-field-absence property across a definition alpha itself edits; asserting absence post-alpha would be wrong, and asserting presence pre-alpha would fail. Pinned by alpha's own Rust test instead. |

## ε (#6722) — P4-epsilon: key per-inequality slack by ConstraintNodeId and carry margin on ConstraintCheckEntry

| Capability | Verdict | Evidence |
|---|---|---|
| `slack-decomposition-exists` | **PASS** | capability→producer (anti-orphan) — collect_slack_terms builds signed-slack CompiledExprs for Ge/Gt/Le/Lt with And-recursion, on the production centrality-objective path in reify-constraints.<br>`grep:fn collect_slack_terms` → `present` in `crates/reify-constraints/src/solver.rs` |
| `op-rule-pact-members-exist` | **PASS** | G7 no-lockstep-duplication — the same decomposition lives in collect_floor_terms and derive_from_expr in-crate plus the cross-crate mirror has_inequality_slack. epsilon must edit all members in lockstep, not add a fifth copy.<br>`grep:fn collect_floor_terms|fn has_inequality_slack` → `present` in `crates/reify-constraints/src/solver.rs`, `crates/reify-eval/src/engine_eval.rs` |
| `equality-has-no-signed-slack` | **PASS** | G6 branch 4 (honest absence) — Eq/Ne/Or are explicitly skipped by the decomposition, so margin is None for an equality — NEVER 0, which would read as "no margin left". Verified by reading the collect_slack_terms doc contract. (Signal restated 2026-09-03, PRD Amendment A1: epsilon asserts margin=None on the escaping ConstraintCheckEntry; the `n/a` RENDERING of that None belongs to the downstream renderers zeta/xi/lambda, not to epsilon.)<br>_manual_ — a negative semantic property of a match arm; the assertion is a test postcondition on epsilon's own Rust test rather than a repo pattern. |
| `tolerance-upstream` | **PASS** | capability→producer via dependency closure — toleranced Scalar equality verdicts are delivered by task 6653, wired UPSTREAM of epsilon. Without it a correctly-solved model reports false-VIOLATED and the slack column would decorate a wrong verdict.<br>_manual_ — upstream task delivery, verified by the wired dependency edge to 6653, not by a repo pattern. |

## β (#6723) — P4-beta: verdict wire contract — fix the status casing and pin it with a two-way boundary test

| Capability | Verdict | Evidence |
|---|---|---|
| `backend-emits-lowercase-after-fix` | **PASS** | signal premise + delivery assertion — build_constraints maps Satisfaction to TitleCase tokens at two sites TODAY (verified 2026-08-26; that is the defect), and build_constraints is the production feed for GuiState.constraints. The delivered_check is the POST-delivery state: after beta the lower-case token the six frontend consumers already compare must be what the backend emits.<br>`grep:"satisfied"` → `present` in `gui/src-tauri/src/engine.rs` |
| `frontend-compares-lowercase` | **PASS** | signal premise (the other half) — StatusBar and ConstraintPanel both switch on lower-case tokens.<br>`grep:'satisfied'` → `present` in `gui/src/panels/StatusBar.tsx`, `gui/src/panels/ConstraintPanel.tsx` |
| `no-normaliser-exists` | **PASS** | rejection-mechanism (anti-silent-accept) — there is no case normalisation on the path between them; the only toUpperCase in the frontend formats a file extension. Observed absence is what makes the badge dead rather than merely inconsistent.<br>`grep:toLowerCase` → `absent` in `gui/src/stores/engineStore.ts`, `gui/src/bridge.ts`, `gui/src/panels/ConstraintPanel.tsx`, `gui/src/panels/StatusBar.tsx` |

## γ (#6724) — P4-gamma: eval-time diagnostics reach the GUI under a solve source tag

| Capability | Verdict | Evidence |
|---|---|---|
| `checkresult-carries-diagnostics` | **PASS** | capability→producer — Engine::check seeds its diagnostics from eval_result.diagnostics and returns them on CheckResult, so the data exists at the GUI boundary.<br>`grep:diagnostics` → `present` in `crates/reify-eval/src/engine_constraints.rs` |
| `no-gui-reader-today` | **PASS** | signal premise (the defect) — EngineSession never reads CheckResult.diagnostics; build_gui_state sources compile diagnostics from compiled.diagnostics and tessellation diagnostics from tess_result.diagnostics.<br>_manual_ — an absence-of-reader property over a whole crate; a bare pattern-absent check would be satisfied trivially by unrelated files. Pinned by gamma's own e2e assertion that the warning reaches the panel. |
| `wire-type-already-carries-code` | **PASS** | capability→producer (anti-orphan) — DiagnosticInfo carries an Option<String> code across the IPC boundary and DiagnosticsPanel already filters by source, so gamma adds a source tag, not a wire format.<br>`grep:code` → `present` in `gui/src-tauri/src/types.rs` |
| `optimality-warning-is-coded` | **PASS** | capability→producer — W_SOLVER_OPTIMALITY_UNPROVEN is emitted with DiagnosticCode::SolverOptimalityUnproven, so gamma's renderer can key on the code rather than message text (INV-SF-6).<br>`grep:SolverOptimalityUnproven` → `present` in `crates/reify-eval/src/engine_eval.rs`, `crates/reify-core/src/diagnostics.rs` |

## δ (#6725) — P4-delta: AutoResolvePanel lifecycle — mount on data, render a single sample honestly

| Capability | Verdict | Evidence |
|---|---|---|
| `mount-no-longer-gated-on-active` | **PASS** | signal premise + delivery assertion — the panel is mounted under a Show gated on autoResolve.active TODAY, while the backend fires start/iteration/complete synchronously in one call and endAutoResolveLoop clears iterations, so the gate can never be observed true (that is the defect). The delivered_check is the POST-delivery state: after delta the mount no longer keys on that flag.<br>`grep:when=\{[^}]*autoResolve\.active` → `absent` in `gui/src/App.tsx` (pattern narrowed 2026-08-27, esc-6739-1: the bare token also matched the implementer's own explanatory comment at App.tsx:786 and falsely re-blocked dependents; this shape still catches the historical defect `<Show when={...autoResolve.active}>` but not the comment. Known trade-off: misses a regression reintroduced via indirection, which `gui/src/__tests__/App.test.tsx` covers behaviourally) |
| `emitter-fires-single-synchronous-trio` | **PASS** | signal premise — emit_auto_resolve_if_any builds one AutoResolveIteration with iteration 0 and calls start, iteration, complete back to back.<br>`grep:fn emit_auto_resolve_if_any` → `present` in `gui/src-tauri/src/engine.rs` |

## ζ (#6726) — P4-zeta: ConstraintPanel — true verdicts plus a slack column

| Capability | Verdict | Evidence |
|---|---|---|
| `margin-upstream` | **PASS** | DAG-direction (anti-inversion) — the margin field is delivered by epsilon, wired UPSTREAM of zeta; the verdict fix is delivered by beta, also upstream.<br>_manual_ — upstream task delivery, verified by wired dependency edges to epsilon and beta. |
| `panel-renders-constraint-rows-today` | **PASS** | capability→producer (anti-orphan) — ConstraintPanel already renders a row per constraint with a status badge, so zeta adds a column to a live surface.<br>`grep:ConstraintPanel` → `present` in `gui/src/panels/index.ts` |

## η (#6727) — P4-eta: PropertyEditor per-auto provenance chip

| Capability | Verdict | Evidence |
|---|---|---|
| `chip-surface-exists` | **PASS** | capability→producer (anti-orphan) — PropertyEditor already renders per-cell determinacy, freshness and an undef-reason chip driven by ValueData, so eta extends a live, plumbed surface.<br>`grep:undef-reason` → `present` in `gui/src/panels/PropertyEditor.tsx` |
| `provenance-reaches-the-gui-load-path` | **PASS** | DAG-direction (anti-inversion) — alpha, upstream, carries provenance onto CheckResult, which is what the GUI load path returns.<br>_manual_ — upstream task delivery, verified by the wired edge to alpha. |
| `edit-path-parity-is-upstream-not-here` | **PASS** | G6 branch 3 (capability in the dependency set) — the warm/edit half requires P1 kappa (task 6699), wired UPSTREAM. eta must not assert edit-path provenance without it; before 6699 the warm map is structurally empty.<br>_manual_ — upstream cross-PRD task delivery, verified by the wired edge to 6699. |

## θ (#6728) — P4-theta: StatusBar solve-summary chip

| Capability | Verdict | Evidence |
|---|---|---|
| `statusbar-already-counts-constraints` | **PASS** | capability→producer (anti-orphan) — StatusBar already renders constraint counts and diagnostic badges and already owns a toggle into the Problems panel, so theta adds a chip to a live surface with an existing drill-in idiom.<br>`grep:StatusBar` → `present` in `gui/src/panels/index.ts` |
| `resolution-profile-is-upstream` | **PASS** | G6 branch 3 — the budget and staleness axes the chip renders are delivered by P1 alpha (task 6689), wired UPSTREAM. theta must not assert a staleness marker before it.<br>_manual_ — upstream cross-PRD task delivery, verified by the wired edge to 6689. |
| `never-asserts-proven-optimal` | **PASS** | G6 branch 1/3 (false-premise guard) — SolverRegistry::production leaves the Logical slot None and CP-SAT is unregistered, so every production objective solve returns BestFound. The chip must never render ProvenOptimal.<br>`grep:ProvenOptimal` → `absent` in `gui/src/panels/StatusBar.tsx` |

## ι (#6730) — P4-iota: DOF badge in StatusBar rendering task 4388's ledger

| Capability | Verdict | Evidence |
|---|---|---|
| `ledger-producer-upstream` | **PASS** | DAG-direction (anti-inversion) — the DOF ledger, conflict sets and W_UNDERDETERMINED extension are delivered by task 4388, wired UPSTREAM. 4388's scope was narrowed on 2026-08-26 to drop the badge and name this leaf as its owner, so there is exactly one owner.<br>_manual_ — upstream task delivery, verified by the wired edge to 4388 and by 4388's rewritten description. |
| `residual-dof-data-already-computed` | **PASS** | capability→producer — RelateSolution carries spent/free/driving/redundant, populated on the relate-solve path today; it currently has no reader, which is the orphan 4388 plus this leaf close.<br>`grep:pub free` → `present` in `crates/reify-eval/src/relate_solve.rs` |
| `dof-zero-is-not-a-fact` | **PASS** | G6 branch 1 (numeric honesty) — SystemBuilder::solve's empty-constraint early return of dof 0 is a known lie for sketches; libslvs reports an honest dof 4 for two free 2D points. The badge must not render that zero as measured.<br>_manual_ — a negative rendering obligation; asserted by iota's own test that a constraint-free sketch does not display a measured zero. |

## κ (#6731) — P4-kappa: constraint-solver producer on SolverProgressSink plus overlay visibility

| Capability | Verdict | Evidence |
|---|---|---|
| `progress-pipeline-wired-end-to-end` | **PASS** | capability→producer (anti-orphan) — SolverProgressSink, the thread-local SOLVE_DISPATCH_CONTEXT, the solver-progress IPC channel, the bridge listener, the engineStore ring buffer and SolverProgressOverlay are all live on main.<br>`grep:SolverProgressSink` → `present` in `crates/reify-eval/src/solver_progress.rs` |
| `only-fea-cg-produces-today` | **PASS** | signal premise (the gap) — the only solver_kind in the tree is "cg", emitted by the FEA elastic-static CG loop. The constraint solver emits nothing on this channel.<br>`grep:solver_kind: "cg"` → `present` in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| `no-iteration-count-is-asserted` | **PASS** | G6 branch 1 (false-premise guard) — no iteration count, wall-clock or exit residual escapes reify-constraints; SolveMeta is private and one bit wide. kappa's signal is a residual TRACE from the per-iteration sink, never a count read off a result type.<br>_manual_ — a scoping obligation on the signal wording; verified by kappa's test asserting a trace of samples rather than a terminal count. |

## λ (#6732) — P4-lambda: solve_report debug-MCP tool

| Capability | Verdict | Evidence |
|---|---|---|
| `tool-registration-surface-exists` | **PASS** | capability→producer (anti-orphan) — tool_defs and dispatch_tool are the live registration and routing pair in debug_server.rs.<br>`grep:fn tool_defs` → `present` in `gui/src-tauri/src/debug_server.rs` |
| `both-allowlists-must-be-updated` | **PASS** | anti-mismatch — a new tool must be added to PURE_ENGINE_SIDE and to KNOWN_DEBUG_TOOL_NAMES or the parity and assertions suites go red. This fourth edit is undocumented in the contract's own defining-a-new-tool section.<br>`grep:PURE_ENGINE_SIDE` → `present` in `gui/src/__tests__/debugParity.test.ts` |
| `value-scenario-harness-exists` | **PASS** | capability→producer — VALUE_SCENARIOS is the committed declarative e2e catalogue and its own doc names downstream tool-leaf tasks as its extenders.<br>`grep:VALUE_SCENARIOS` → `present` in `gui/test/visual/assertions.ts` |
| `last-check-accessor-must-be-added` | **PASS** | capability→producer (producer-absent TODAY, delivered BY this leaf) — EngineSession exposes last_check only under cfg(test); reaching CheckResult needs a new pub(crate) observational accessor, precedented by EngineSession::engine.<br>_manual_ — the capability is absent today and is delivered by lambda itself; asserting presence pre-lambda would fail and post-lambda is lambda's own test. |
| `signal-is-not-ci-gated` | **PASS** | G6 branch 3 (honest signal scope) — scripts/verify.sh references no test:e2e, test:visual, test:smoke, REIFY_DEBUG or 3939, so the debug-MCP e2e harness is not CI-gated. lambda's signal says green via npm --prefix gui run test:e2e against a live reify-gui, never green in CI.<br>`grep:test:e2e|test:visual|REIFY_DEBUG` → `absent` in `scripts/verify.sh` |

## ξ (#6733) — P4-xi: reify explain — the dropped fields, slack, and a failure vocabulary

| Capability | Verdict | Evidence |
|---|---|---|
| `explain-surface-shipped` | **PASS** | capability→producer (anti-orphan) — cmd_explain is wired into the CLI dispatch table and pinned by seven tests; xi extends a live surface.<br>`grep:fn cmd_explain` → `present` in `crates/reify-cli/src/main.rs` |
| `term-contributions-already-computed` | **PASS** | capability→producer — TermContribution carries sense, weight, realized_value and contribution, computed per scope and Arc-shared; explain currently drops it.<br>`grep:struct TermContribution` → `present` in `crates/reify-ir/src/constraint.rs` |
| `infeasible-and-no-autos-are-indistinguishable-today` | **PASS** | signal premise (the defect) — explain prints the same no-provenance sentinel for an infeasible model and for a model with no autos; only stderr and the exit code separate them.<br>`grep:No objective provenance recorded` → `present` in `crates/reify-cli/src/main.rs` |
| `stale-anchor-is-repaired-here` | **PASS** | code-anchor hygiene — cmd_explain's own rustdoc cites engine_eval.rs:3884 for the provenance rationale; that line is now an unrelated field-elaboration arm. xi re-cites by symbol.<br>`grep:engine_eval.rs:3884` → `absent` in `crates/reify-cli/src/main.rs` |
| `completeness-field-owner-is-p3-zeta` | **PASS** | G7 co-tenancy ruling — canonical text is **PRD §8.2 items 1–3**; this row is a pointer, not a fifth copy. The rule: ξ renders the P4-owned fields only and neither adds nor re-renders the `completeness` field, which is P3 ζ #6711's; no dependency edge either way. Anchored on §8.2's ruling sentence rather than on the id `#6711`, which alone now occurs in §4.1, §8, §11 and §12.5 and so would stay green with the ruling deleted.<br>`grep:neither add nor re-render the completeness field` → `present` in `docs/prds/v0_6/solver-legibility-telemetry.md` (0 hits at baseline `cdc501a3f1`) |

## ο (#6735) — P4-omicron: docs-truth for the observable legibility surface

| Capability | Verdict | Evidence |
|---|---|---|
| `chunk-directory-exists` | **PASS** | capability→producer (anti-orphan) — the doc-chunk corpus is a live include_str-backed directory served by the reference tool.<br>`grep:language_chunks|chunks` → `present` in `crates/reify-mcp/src/tools/` |
| `documented-signatures-verified-against-registries` | **PASS** | docs-truth gate — every documented signature must compile as written and be checked against the compiler arms and unit registries, not against prose.<br>_manual_ — a per-signature verification obligation discharged by omicron's smoke .ri compiling as written. |
| `discoverability-acceptance` | **PASS** | docs-truth gate item 4 — an author who knows the goal but not the feature name must reach slack and provenance from the chunks or the corpus index.<br>_manual_ — an intent-level findability judgement; not mechanically expressible. |

## φ (#6736) — P4-phi: two-way solve-record parity gate (INTEGRATION GATE)

| Capability | Verdict | Evidence |
|---|---|---|
| `names-the-boundary-sketch` | **PASS** | G5 B+H closure — phi's signal is the PRD section 5 boundary-test table in full, facing both the producer and the consumer side of every seam this PRD touches.<br>_manual_ — the signal is a committed test target enumerating the ten sketch rows; verified by the target existing and passing. |
| `upstream-leaves-deliver-every-row` | **PASS** | DAG-direction (anti-inversion) — every row's capability is delivered by a leaf wired UPSTREAM of phi (beta, gamma, zeta, eta, theta, kappa, lambda, xi), plus P1 6689 and 6699 for the two cross-PRD rows.<br>_manual_ — upstream task delivery, verified by the wired dependency edges. |

## μ (#6737) — P4-mu: author opt-in to the robustness floor for non-Money objectives

| Capability | Verdict | Evidence |
|---|---|---|
| `special-form-precedent-exists` | **PASS** | capability→producer (anti-orphan) — cost_robustness_tradeoff plus the ObjectiveSet.cost_robustness_lambda marker field is the shipped, grammar-free author-side solver opt-in precedent, with compile-time typing and coded diagnostics.<br>`grep:cost_robustness_lambda` → `present` in `crates/reify-ir/src/constraint.rs` |
| `no-novel-grammar-required` | **PASS** | grammar reality (anti-mismatch) — the default route adds no syntax. The probed alternative, a scope-level pragma, also parses today with zero ERROR nodes, so neither route is a grammar fiction.<br>`grep:pragma:` → `present` in `tree-sitter-reify/grammar.js` |
| `floor-flag-exists-but-is-hardwired` | **PASS** | signal premise — apply_robustness_floor already exists as a parameter of the solve core but is hard-wired by its caller, and ResolutionProblem carries no field for it. mu threads it from the DSL.<br>`grep:apply_robustness_floor` → `present` in `crates/reify-constraints/src/solver.rs` |
| `eval-side-twin-must-move-in-lockstep` | **PASS** | G7 no-lockstep-duplication — objective_is_money is duplicated in reify-eval to gate the Info diagnostic, deliberately, to preserve dependency inversion. mu must edit both.<br>`grep:objective_is_money` → `present` in `crates/reify-eval/src/engine_eval.rs`, `crates/reify-constraints/src/solver.rs` |
| `clamp-box-coupling-is-upstream` | **PASS** | G6 branch 3 — un-gating the floor also un-gates the constraint-derived clamp box, which is blocked on the uniqueness-contract ruling owned by task 5711, wired UPSTREAM.<br>_manual_ — upstream task delivery, verified by the wired edge to 5711. |

## π (#6738) — P4-pi: docs-truth for the robustness opt-in

| Capability | Verdict | Evidence |
|---|---|---|
| `exemplar-corpus-is-compile-gated` | **PASS** | capability→producer (anti-orphan) — examples under the best-practices corpus are auto-compile-gated by the examples smoke test, so a worked example cannot silently rot.<br>`grep:examples_smoke` → `present` in `crates/reify-compiler/tests/` |
| `discoverability-acceptance` | **PASS** | docs-truth gate item 4 — an author who knows the goal, stop my optimum parking on the clearance bound, but not the feature name must reach it from the chunks or the corpus index.<br>_manual_ — an intent-level findability judgement; not mechanically expressible. |

## ω (#6739) — P4-omega: PRD close — terminal status stamp and the AS-AUTHORED freeze

| Capability | Verdict | Evidence |
|---|---|---|
| `terminal-vocabulary-is-closed` | **PASS** | docs-truth / PRD-status contract — the terminal token must be exactly one of SHIPPED, SUPERSEDED or WITHDRAWN, matched case-insensitively as the first token after the Status label, with the landed leaf IDs named on the same line.<br>`grep:SHIPPED|SUPERSEDED|WITHDRAWN` → `present` in `docs/prds/v0_6/solver-legibility-telemetry.md` |
| `cancelled-sibling-counts-as-satisfied` | **PASS** | dependency disposition — a cancelled sibling leaf satisfies omega's edge; if the scheduler treats it as unmet the decompose steward removes the edge by hand rather than leaving omega permanently blocked.<br>_manual_ — a scheduler-disposition rule, not a repo pattern. |
| `p3-multimodality-slot-cites-6706` | **PASS** | G7 / INV-SF-5 posture, **half one of two** — §11's multimodality slot must name #6706, the `Completeness` vocabulary carrier, never "a future PRD". As first landed the §12.5 G7 row promised the citation was "stamped at decompose" and no P3 id appeared anywhere in the document; #6751 stamped it in §4.1, §8 (row + §8.2) and §11. ω is dep-wired behind #6751 and applies the terminal status stamp, so this re-verifies the citation immediately before that stamp — a G7 justification that defers to a decompose-time action must be re-checked at decompose-close. **Two checks, not one alternation:** the requirement is conjunctive, and an ERE alternation is satisfied by either id alone. **Anchored, not a bare id-grep:** `#6706` alone now occurs in §4.1, §8 and §12.5 — including §12.5's account of this very correction — so a bare id-grep would stay green with §11 regressed to "a future PRD". **Non-vacuous:** `paths` is the single PRD file and this pattern had zero hits in it at baseline HEAD `cdc501a3f1` — a genuine RED→GREEN check, unlike the vacuous tree-scoped id-grep that forced `gui-on-demand-measurement.capability-manifest.yaml`:80-85 down to `kind: manual`. Unlike β's and δ's, this binding is GREEN on #6751's own diff.<br>`grep:Vocabulary carrier for the slot.*#6706` → `present` in `docs/prds/v0_6/solver-legibility-telemetry.md` |
| `p3-multimodality-slot-cites-6711` | **PASS** | G7 / INV-SF-5 posture, **half two of two** — the same §11 slot must also name #6711, the first leaf that populates it. Rationale, conjunctive-not-alternation reasoning and non-vacuity are as for `p3-multimodality-slot-cites-6706` above; this pattern likewise had zero hits at baseline `cdc501a3f1` and is anchored to §11's own role line, because `#6711` alone now occurs in §4.1, §8, §8.2, §11 and §12.5. **Both halves must pass** for the INV-SF-5 row to be certified.<br>`grep:First leaf that populates the slot.*#6711` → `present` in `docs/prds/v0_6/solver-legibility-telemetry.md` |

