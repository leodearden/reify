# Capability manifest — catalog membership (`constraint x in C`)

> **PRD:** `docs/prds/v0_6/catalog-membership-constraint.md`. **Status:** active — decomposed 2026-10-08,
> task ids stamped (α #8355 … ω #8359). The machine-readable twin is
> `catalog-membership-constraint.capability-manifest.yaml` (same bindings; its mechanical
> `delivered_check`s are copied into the producer tasks at `commit_planning`). Evidence was gathered on
> main `90b846861f`–`f93f3b9db9` (2026-10-07/08) with a fresh debug `reify` and a clean tree-sitter parser.

Probe fixtures: the committed `tests/prd-gate/fixtures/cmc_*.ri` (PRD evidence; every one is a parse
error today, which α flips) and the ephemeral `/tmp/prd-gate-fixtures/cmc/p_*.ri` baselines
(`p_catalog_count`, `p_hint_window_baseline`, `p_eq_exact`, `p_let_catalog_hint_compiles`; not committed,
per the overlay's fixture tiers). δ and ω are docs leaves with no reify/tree-sitter premise to probe; their
bindings are producer-upstream edges and grep/path checks. The D3 decompose-verification run covered α, β, γ
(result recorded in §Verification).

## α — #8355

catalog-membership α: the `in` operator — grammar, typing, evaluation, check-time membership, typed refusal in the continuous solver

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `grammar-producer-in-binary-arm` | producer-self (D1) — binary_expression gains prec.left(-1, … field('op', 'in') …). Scratch-grammar measurement 2026-10-07: no new conflicts, 261/261 corpus, identical trees for every cleanly-parsing tracked .ri. α is the grammar producer for every cmc fixture (grammar_confirmed=false). | **PASS** | grep `field\('op', 'in'\)` present in `tree-sitter-reify/grammar.js` |
| `baseline-in-is-a-parse-error` | existing-substrate baseline — tree-sitter parse of every tests/prd-gate/fixtures/cmc_*.ri exits 1 today (measured 2026-10-08 with a clean parser; a stale prototype .so had been purged from ~/.cache/tree-sitter/lib first); reify check of cmc_membership_check.ri exits 1 (parse error). | **PASS** | manual: baseline evidence for the polarity flip α delivers; asserted by α's B1/B2 tests. |
| `forall-in-header-unchanged` | existing-substrate — `forall v in vents:` (examples/keyed_forall.ri) parses today; the scratch grammar keeps it, the indexed-sub `[i in …]`, `joint … in` and `port … : in` positions unchanged (their `in` follows an identifier/type/keyword slot). | **PASS** | manual: B1 corpus regression tests in α. |
| `binop-in-eval-arm` | producer-self (§5.1) — BinOp::In appended after Implies (content hash uses `op as u8`); reify_expr::eval_binop arm per D4 with early return. | **PASS** | grep `BinOp::In\b` present in `crates/reify-expr/src` |
| `stdlib-catalog-evaluates` | existing-substrate — standard_bolt_lengths() is a zero-arg `pub fn … -> List<Length>` in crates/reify-compiler/stdlib/standard_stock.ri; reify eval of /tmp/prd-gate-fixtures/cmc/p_catalog_count.ri prints S.k = 20, S.first = 0.008 m (2026-10-08). | **PASS** | — |
| `equality-is-the-eq-verdict` | existing-substrate — `==` is exact f64 today (reify_expr::eval_eq; SimpleConstraintChecker adds nothing): reify check of /tmp/prd-gate-fixtures/cmc/p_eq_exact.ri (0.1m + 0.2m == 0.3m) is VIOLATED, exit 1. D3 reuses that verdict; #6653 will tolerance it in one place. | **PASS** | manual: D3 is behavioural (one equality implementation shared by == and in). |
| `dimensional-solver-refusal-both-entry-paths` | producer-self (D7) — one guard (shared with #5470 item 5) at the head of DimensionalSolver::solve_with_meta AND solve_ranked_impl before the multistart loop (which calls solve_core directly, verified on main 90b846861f); refuses only an In whose operand is not solve-invariant. | **PASS** | manual: behavioural — B4, B4b (multistart), B4c (fixed-operand in solves). |
| `solve-invariance-predicate-shared` | producer-self — solver.rs::DerivationCtx::varies_with_solve moves to a shared reify-constraints module (name tactical, PRD §10). | **PASS** | manual: module name is tactical; consumers are β's extractor and the D7 guard. |
| `typed-undef-cause` | producer-self (D7, INV-SF-1) — engine_eval.rs::record_failed_autos records the refusing diagnostic's code instead of SolveFailed{"infeasible"} (cold and merged paths). | **PASS** | manual: B4 asserts the cause on cmc_unextractable.ri. |
| `eval-and-check-exit-1-on-error` | existing-substrate — reify eval and reify check exit 1 on an Error-severity diagnostic (measured on fca4a9ad5f; reify-cli unchanged to f93f3b9db9 for this path). | **PASS** | — |
| `shares-grammar-files-with-8300` | producer-upstream — #8300 (in progress) edits grammar.js/scanner.c; hard edge #8300 → α wired 2026-10-08. | **PASS** | — |

## β — #8356

catalog-membership β: membership domains in the solver layer — extraction, catalog validity, no-member-feasible diagnostic, warm path

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `domain-channel-upstream` | producer-upstream — #5470 (re-scoped 2026-10-07) delivers AutoParam.domain: Option<DiscreteDomain> (hard, producer-agnostic), AutoParam::is_discrete(), enumeration_domain.rs, Int bound-mining, typed AutoNoFiniteDomain, discrete re-widening; edge #5470 → β wired. #5470 depends on #6967. | **PASS** | — |
| `strict-auto-nonuniqueness-upstream` | producer-upstream — #6554 (CP-SAT strict-auto non-uniqueness Error, P3 §3.4, Leo 2026-09-01); edge #6554 → β wired. B5b's strict half needs it. | **PASS** | — |
| `in-operator-upstream` | producer-upstream — α #8355 (BinOp::In, classifier arm, shared predicate, D7 guard); edge α → β wired. | **PASS** | — |
| `membership-extractor` | producer-self (D6) — one extractor in enumeration_domain.rs over top-level `ValueRef(auto) in C` conjuncts with C solve-invariant, run at the start of registry.rs::decompose_prelude. | **PASS** | grep `BinOp::In\b` present in `crates/reify-constraints/src/enumeration_domain.rs` |
| `hint-baseline-returns-non-member` | existing-substrate baseline (polarity flip) — /tmp/prd-gate-fixtures/cmc/p_hint_window_baseline.ri (@solver_hint discrete_set, window [22.5, 25.5] mm) prints S.x = 0.024 m, exit 0, no diagnostic (2026-10-08): today the catalog is ignored. | **PASS** | — |
| `numeric-premises` | arithmetic over standard_bolt_lengths (…16, 20, 25, 30…): [22.5, 25.5] ∩ catalog = {25} (B5); [22.5, 24.5] ∩ = ∅ with boundary pair 20mm (violates >=) / 25mm (violates <=) (B6); [22.5, 30.5] ∩ = {25, 30} (B5b); [22, 24, 26] ∩ [22.5, 25.5] = {24} (B7); n ∈ 1..8, 3n ≥ 10, n² ≤ 17 → {4} (B8). Plates 6 + 8 + 6.5 = 20.5 mm. | **PASS** | — |
| `warm-path-reaches-the-same-registry` | existing-substrate — engine_edit.rs edit_param / edit_source build a ResolutionProblem from constraints filtered by syntactic reads and call self.solver (SolverRegistry::production() in CLI and GUI); deps.rs registers a constraint under every ValueRef it reads, so editing `sizes` dirties `x in sizes`. | **PASS** | manual: B10 warm-path test. |
| `no-member-feasible-diagnostic` | producer-self (D8) — cpsat.rs::verdict_from_enumeration's complete-no-solution arm reports NoCatalogMemberFeasible with the boundary-pair rule; the extractor reports empty intersections itself. | **PASS** | manual: B6 on tests/prd-gate/fixtures/cmc_bolt_gap.ri. |
| `catalog-validity-every-in` | producer-self (ruling C) — every solve-invariant In catalog is validated before search (Undef / empty / Undef member → CatalogUnresolvable), closing CP-SAT's don't-prune-on-Undef gap. | **PASS** | manual: B9. |
| `warm-let-catalog-gap-owned` | out of β's signal — on the warm path dependent_cells is empty, so a let catalog reading an auto looks solve-invariant; owned by #6690 (deletes the warm builder). B9 asserts the cold behaviour only. | **PASS** | — |

## γ — #8357

catalog-membership γ: `@solver_hint("discrete_set", C)` compiles to `constraint x in C`; prefer_stock / preferred_strategy made loud

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `beta-upstream` | producer-upstream — β #8356 (domains, so the sugar's constraint solves); edge β → γ wired. | **PASS** | — |
| `hint-pipeline-accepts-local-catalogs` | existing-substrate — validate_solver_hint_collections accepts any in-scope name; reify check of /tmp/prd-gate-fixtures/cmc/p_let_catalog_hint_compiles.ri (let catalog, annotation first) exits 0 (2026-10-08). The old "compile-rejected" premise was the `@` port-selector parse join (#8300). | **PASS** | — |
| `sugar-lowering` | producer-self (D9) — the DiscreteSet hint lowers to an appended, labelled `x in C` constraint at every cell decl site (entity.rs, guards.rs, entities_phase.rs, connect.rs, port-member site) or a coded rejection. | **PASS** | manual: B11 on tests/prd-gate/fixtures/cmc_sugar_equivalence.ri, B12. |
| `other-hints-loud` | producer-self (D10) — prefer_stock / preferred_strategy emit SolverHintNotConsumed. | **PASS** | manual: compile-diagnostic test; code name tactical. |
| `m11-example-and-test` | producer-self — examples/m11_annotations.ri BoltedPanel gains constraints so its strict bolt_length resolves; m11_annotations_solver_hint_tests.rs updated (it asserts no warnings and pins SolverHint.collection's string shape). | **PASS** | manual: the existing gate test, updated in the same diff. |
| `fixture-places-annotation-first` | existing-substrate — cmc_sugar_equivalence.ri puts the annotated member first, so #8300 is not needed for γ's signal. | **PASS** | — |

## δ — #8358

catalog-membership δ: docs truth — chunks, best-practices exemplar, reify-design index, spec §5/§9.2/§10.7/§12.1/§16 for `x in C`

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `gamma-upstream` | producer-upstream — γ #8357 (documents landed behaviour of α, β, γ); edge γ → δ wired. | **PASS** | — |
| `chunk-fence-gate-exists` | existing-substrate — crates/reify-compiler/tests/harness_doc_chunks/fence_gate.rs compiles every reify-fragment fence in every chunk. | **PASS** | — |
| `membership-chunk-section` | producer-self — a Membership section in constraints.md, named in intent words. | **PASS** | grep `^#+ .*[Mm]embership` present in `crates/reify-mcp/src/tools/chunks/constraints.md` |
| `exemplar-and-index-row` | producer-self — examples/best_practices/catalog_membership.ri and its INDEX.md row in one commit (bidirectional index test). | **PASS** | grep `catalog_membership\.ri` present in `examples/best_practices/INDEX.md` |
| `reify-design-index-line` | producer-self — one line in .claude/skills/reify-design/SKILL.md "Probe-verified idioms — index". | **PASS** | grep `catalog_membership\.ri` present in `.claude/skills/reify-design/SKILL.md` |
| `spec-membership-section` | producer-self (ruling N) — spec §5 Membership subsection, §9.2 undef rule, §16 `in` row, §12.1 last paragraph, §10.7. | **PASS** | grep `^#+ .*[Mm]embership` present in `docs/reify-language-spec.md` |

## ω — #8359

catalog-membership ω: close the PRD — terminal Status stamp, AS-AUTHORED freeze header, LIVE/AS-AUTHORED map (PRD + manifest)

| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `all-leaves-upstream` | producer-upstream — α #8355, β #8356, γ #8357, δ #8358; edges wired. | **PASS** | — |
| `terminal-status-stamp` | producer-self — the PRD's Status line carries SHIPPED with the landed leaf ids. | **PASS** | grep `^> \*\*Status:\*\* \*{0,2}SHIPPED` present in `docs/prds/v0_6/catalog-membership-constraint.md` |

## Verification

D3 (`scripts/prd-decompose-verify.mjs`, Adversary downgraded to sonnet/high and probes pinned to the fresh
debug binary in a scratchpad copy), run `wf_5124ad40-542`, 2026-10-08: **PASS** — α 7/7, β 7/7, γ 3/3 premises
executed with captured output, 0 blocking, 0 malformed, 0 fixture-absent. The first pass reported γ INCOMPLETE
(its Enumerator rewrote absolute probe paths to repo-relative ones that do not exist yet); the re-run bound
the paths verbatim and added a `Parse error` stderr signature to the rejection premise so a file-not-found
exit cannot satisfy it. δ and ω were not submitted (no reify/tree-sitter premise to probe).
