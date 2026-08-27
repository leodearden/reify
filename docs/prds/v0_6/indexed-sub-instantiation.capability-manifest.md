# Capability manifest — indexed-sub-instantiation.md

Authored at decompose time 2026-07-25 (autonomous `/prd` decompose session
prd-reify-63332; PRD landed a73edf74ea). Binds each leaf's user-observable
signal to substrate evidence per the /prd gates (G3+G6 mechanized).
Machine-readable twin: `indexed-sub-instantiation.capability-manifest.yaml`
(task ids pre-stamped at decompose; `commit_planning` re-stamps).

**Probe provenance.** Authoring-time probes 2026-07-24 (PRD §2, 13 premises,
fixtures quoted there; target-surface fixture committed at
`fixtures/indexed_sub_instantiation_surface.ri`). Re-verified at decompose
2026-07-25 by the D3 workflow (`scripts/prd-decompose-verify.mjs`, run
`wf_1783a58f-6ff`) against a fresh `target/debug/reify` (rebuilt this
session, main `94e1631111` + docs; code drift since the authoring binary:
tasks 5214/5364/5072/5342/5234/5281 — none touch collection-sub/forall/
indexed-access semantics).

**D3 outcome (adjudicated).** Raw verdict `blocks:true` on α/β/ζ; every
blocking record adjudicated by the decompose session against the journal +
direct re-probes; **no true substrate failure**:
- α: the `reify check` FAIL record actually OBSERVES the rejection baseline
  (exit 1, hand-parser "Parse error: invalid sub" at the indexer) — match
  artifact, substance confirmed. Three prover baseline fixtures were never
  written to disk (execution artifact); re-authored and re-verified by the
  decompose session directly (all exit 0, 0 ERROR nodes; target fixture's
  ERROR confined to [17,14]–[17,25], the indexer clause).
- β: the adversary FAIL targets the premise RECORD's polarity (ir-kind
  binding must state `observation='absent'` explicitly), not the substrate —
  which it independently CONFIRMED with an anti-vacuity witness (constraint
  on `idlers[0].od` flips OK→VIOLATED under mutation).
- ζ: REAL refinement, folded into γ/ζ bindings below — sub-instance `.body`
  cells are undef on the eval-value surface even for hand-placed subs; the
  posed bodies are real only in the build pipeline (STL: 2 disjoint bodies
  at expected AABBs; STEP: 2 MANIFOLD_SOLID_BREP with correct transforms).
- Adversary additions folded in: α regression-floor correction + 2 extra arm
  baselines; ε's two silent out-of-range baselines + second misroute
  variant; γ's value-position/probe-#10 and distance-query baselines.
Evidence fixtures committed beside this manifest at
`tests/prd-gate/fixtures/` (24 files, `indexed_sub_*`, `collection_sub_*`,
`forall_*`, `quantifier_expr_*`, `hand_placed_twin_two_subs_eval.ri`,
`single_sub_pose_resolves.ri`, `posed_subs_distance_query_unresolvable.ri`,
`self_collection_count_redirect_rejected.ri`, `collection_expr_index_resolves.ri`).
Two pre-existing defects surfaced by the run: the tree-sitter corpus red on
main (`imaginary_literal.txt`, 218/219) is NEW — filed as independent task
**#5492** (not part of this batch); the `orient_identity` zero-arg
return-type silent-default-Real warning was ALREADY tracked — **#5344**
(pending) with follow-up **#5380** — no duplicate filed.
Grammar gate: the ONE novel production (indexer clause) is α's deliverable;
its fixture FAILS to parse today by design (probe #4) — `grammar_confirmed=false`
on α only, true elsewhere (all other surfaces parse today: probes #5–#7 contexts).

## Batch-shape changes made at decompose (G4 re-walk findings)

The PRD was authored 2026-07-24 in parallel with sibling sessions whose
batches filed first. Decompose-time deltas, recorded here and in task
metadata — the PRD document itself is NOT edited:

1. **ε narrows to the index-hop + consumes the uniform-member-access
   resolver (#5424, hard dep edge).** `uniform-member-access.md` (batch
   5424–5432, filed 2026-07-24) declares the seam split explicitly (its §G4
   row + out-of-scope): *indexed collection addressing (`subs[i]`) = this
   PRD; member resolution after the index hop = their uniform resolver
   ("this resolver is its substrate")*. Their η (#5430) retires the bespoke
   `self.<sub>.<member>` matchers + `CrossSubGeometryRef` discriminator
   territory that probe #8's misroute lives in. ε therefore must NOT
   re-patch the bespoke matchers (would collide with #5430 and violate
   INV-5 `no-lockstep-duplication`); it fixes the `self.`-prefixed INDEX
   hop to yield the same typed element ref as the bare path and routes
   member resolution through the resolver. Real edge ε←#5424 wired at
   decompose (first-filed owns the seam).
2. **θ absorbs the docs-truth exemplar-corpus item.** The overlay's
   docs-truth gate requires a worked example under `examples/best_practices/`
   + `INDEX.md` line; the PRD's θ named chunks/spec/cheatsheet only. θ's
   task text now carries the exemplar + INDEX.md line. `examples/best_practices/`
   does not exist yet — corpus-seed task #5397 is live but `blocked`; θ
   creates its file + INDEX.md line ADDITIVELY (no hard edge onto a blocked
   task; one-file + one-line append cannot contest #5397's six seeded files).
3. **Consumer anchor refreshed.** `prj/printer_v01/printer.ri` is now 679
   lines with 16 `IdlerPulley(` instantiation sites (the PRD §0's
   995-line / 38-tendon / 34-idler counts are stale; dogfood continued —
   same drift the belt manifest recorded for its μ). The hand-placed
   at-transform3 idiom persists (re-verified by grep 2026-07-25); ζ cites
   the idiom, not counts, and distills into `examples/` rather than editing
   printer.ri (belt μ #5445 owns printer.ri's rear-drive rewrite — no file
   contention).

Cross-PRD rows re-checked otherwise unchanged: belt seam declared
identically on both sides (their sidecar carries zero collection/indexed/
forall claims — no contest); keyed sibling untouched; #5385 disjoint
(mutual disclaimers in place); discrete-solve boundary holds
(`discrete-cost-minimisation` sidecar carries no count-cell claims);
overlay §G4 contested pairs (persistent-naming, OpenVDB, topology-selectors)
untouched.

**G7 walk** (`docs/legibility/design-invariants.md`, on main). No waivers.
Per-slug: `undef-has-provenance` — γ replaces the probe-#2 silent-undef
state; γ/σ1 task texts require any realization-failure path to record an
`UndefCause`, never silent undef; σ1 evicts (removes) dropped-tail cells
rather than leaving them undef. `error-severity-exits-nonzero` — all four
new diagnostics are coded and ride the general severity gates (compile-phase
via check; eval-phase via cmd_eval/cmd_build's Severity::Error exit gate,
task 4458 house pattern); no per-code bolt-ons. `declared-intent-consumed-or-diagnosed`
— the v1 relate block on an indexed sub is rejected loudly
(`E_INDEXED_SUB_RELATE_UNSUPPORTED`) instead of parse-and-ignore; range
forall unrolls to consumed per-element decls. `indeterminate-attributable-transient`
— no new Indeterminate paths. `placeholders-owned-and-loud` — no
placeholder-typed surfaces; v1-restriction TODO stubs must cite the real σ1
task id per PTODO grammar. `diagnostics-carry-codes` — all four new
diagnostics carry codes by construction (§3.6).

## α — grammar + parsers: indexer clause (§7 α)

- `target-surface-fails-today` (signal premise, probe #4) → PASS. The
  committed fixture `fixtures/indexed_sub_instantiation_surface.ri` FAILS
  `tree-sitter parse` today (ERROR at `[`); α is the grammar producer
  (`grammar_confirmed=false` — this IS the grammar work; the only G3
  grammar prerequisite in the batch, all downstream tasks gate on α).
- `existing-arms-regression-floor` → PASS, floor CORRECTED by the D3
  adversary. All THREE existing sub arms parse today — instantiation,
  collection, AND specialization (`sub name : StructName { body }`) — plus
  forall-over-range + index_access in constraint position (load-bearing:
  the target fixture reaches 0 ERROR nodes from the indexer delta alone).
  Committed baselines: `tests/prd-gate/fixtures/indexed_sub_{inst,coll,spec}_arm_baseline.ri`,
  `indexed_sub_forall_range_baseline.ri` (all re-verified exit 0 / 0 ERROR,
  2026-07-25). **`tree-sitter test` corpus is NOT green on main** (218/219;
  pre-existing, sub-arm-UNRELATED failure in `test/corpus/imaginary_literal.txt`
  — filed as independent task #5492) and the corpus is not CI-run — α's
  regression signal is therefore the four committed baselines + the
  reify-syntax parser suites (incl.
  `crates/reify-syntax/tests/harness_syntax/sub_decl_specialization_body_parser_tests.rs`,
  the List-disambiguation pins), NOT blanket `tree-sitter test` exit 0.
- `at-clause-grammar-substrate` → PASS. The `at <pose>` clause exists on
  all three sub arms (task 3899 lineage); the indexer rides the existing
  instantiation arm unchanged after `=`. Hand parser rejects the indexer
  surface today too (`reify check` exit 1 "Parse error: invalid sub" — D3
  prover record), so reify-syntax work is confirmed in-scope for α.

## β — compiler lowering: indexed template → collection-sub expansion (§7 β)

- `collection-elaboration-substrate` (probe #1) → PASS. `sub xs : List<T>`
  + count constraint elaborates N members with per-member value cells at
  positional NodeId paths (`EvaluationGraph::from_templates`,
  `crates/reify-eval/src/graph.rs:320`; `CollectionSubInfo`; eval prints
  `Rig.idlers[0].od …`, `Rig.__count_idlers = 2`; D3 re-verified, with the
  adversary's ANTI-VACUITY witness: `constraint d0 == 36mm` over
  `let d0 = idlers[0].od` → `OK Rig#constraint[0]` exit 0, mutated to
  `== 40mm` → VIOLATED exit 1 — the member cell is consumable, not
  print-only (`fixtures/collection_sub_member_cell_consumable.ri`).
  Polarity note per the D3 adversary: any ir-kind probe of this fixture
  must bind `observation='absent'` explicitly (eval exit 0, no EvalError);
  a `present` binding is wrong-polarity and FAILs.
- `count-cell-classifier-substrate` (probe #12) → PASS.
  `structural_classifier.rs` Rule 3 (`collection_subs.count_cell`) already
  classifies count cells Structural; the derived `__count_<name>` cell
  needs no classifier change.
- `dynamic-count-rejection` (G6 branch 4, producer-self) → PASS.
  `E_INDEXED_SUB_DYNAMIC_COUNT` is β's own deliverable; β's negative
  fixture observes it fire (grep confirms the code is absent today —
  nothing pre-exists to collide with).
- `relate-rejection` (G6 branch 4, producer-self) → PASS.
  `E_INDEXED_SUB_RELATE_UNSUPPORTED` producer-self, same shape; the v1
  relate block parses on the instantiation arm (rides α) and is rejected
  in lowering — INV-SF-3 conformant (loud, not parse-and-ignore).

## γ — per-instance realization + posing + surfacing (§7 γ)

- `silent-undef-baseline` (signal premise, probe #2) → PASS. Per-member
  geometry realization is ABSENT today, silently: eval prints
  `idlers[0].body = undef`, aggregate `[undef, undef]`, zero diagnostics
  (D3 re-verified). This is the state γ replaces; the baseline inverts on
  delivery (INV-SF-1: the replacement is loud where it must be).
- `collection-at-rejected-today` (probe #3) → PASS. `at` on the collection
  arm is rejected loudly today ("'at' placement is not supported on
  collection subs; per-element placement is out of scope in v1") — the
  diagnostic that anticipates this PRD; γ+α+β deliver the indexed
  replacement surface.
- `posing-surfacing-substrate` (probe #13) → PASS, REFINED by the D3
  adversary: the posing/surfacing substrate is real on the BUILD pipeline —
  N=2 hand-placed posed subs export as 2 disjoint solids at the expected
  AABBs in BOTH mesh (STL) and STEP (2 MANIFOLD_SOLID_BREP, correct
  ITEM_DEFINED_TRANSFORMATIONs) — but the EVAL-VALUE surface shows
  `Rig.<sub>.body = undef` even for hand-placed subs (and
  `distance(a.body, b.body)` on posed sub bodies is unresolvable at
  check/eval — `fixtures/posed_subs_distance_query_unresolvable.ri`).
  Consequence: γ's "eval shows `ps[i].body` = real handle" (§6.2 row 1) and
  "queries see posed instances" (row 4) are NEW capability γ delivers for
  sub bodies generally, not a mirror of existing single-sub eval behavior;
  the hand-placed precedent γ extends is the build/export walk. Fixtures:
  `hand_placed_twin_two_subs_eval.ri`, `single_sub_pose_resolves.ri`,
  `collection_sub_value_position_undef_baseline.ri` (probe #10: value
  position resolves, evals to `[undef, undef]` silently). γ closes
  sub-placement §10 deferral #1 — the deferral was explicitly parked for
  this successor.
- `shared-expansion-helper` (G7 INV-5) → PASS by construction: cold
  elaboration and warm re-elaboration share ONE expansion helper (template
  × index → cells/pose/realization), called from `from_templates` and the
  `engine_edit.rs` collection-count phase (σ1 extends, never duplicates).
- `no-pose-warning` (G6 branch 4, producer-self) → PASS.
  `W_INDEXED_SUB_NO_POSE` is γ's own deliverable; γ's test observes it.

## δ — range iteration in forall/exists (§7 δ)

- `forall-collection-lowering-substrate` (probe #5) → PASS. `forall v in
  <coll-sub> : constraint …` works today with per-element `OK forall@v[k]`
  lines (task 2364 lowering; D3 re-verified).
- `range-domain-rejected-today` (probe #6) → PASS. `forall i in 0..4` is
  rejected loudly today ("cannot iterate over non-collection type
  'Range<Int>' in forall: expected List<_> or Set<_>"; D3 re-verified) —
  δ lifts exactly this rejection; no silent-accept to displace.
- `dynamic-bound-rejection` (G6 branch 4, shared diagnostic) → PASS.
  Non-literal range bounds emit `E_INDEXED_SUB_DYNAMIC_COUNT` — the SAME
  code as β's site (one diagnostic, one phase-2 lift for both sites, §3.3);
  δ's negative fixture observes it at the forall site.

## ε — addressing hygiene: `self.`-path parity + out-of-range (§7 ε)

- `bare-indexed-access-works` (probe #7) → PASS. `idlers[0].od` in a `let`
  evaluates today (task 2871 indexed-access lookup; D3 re-verified).
- `self-path-misroute-baseline` (signal premise, probe #8) → PASS. The
  DEFECT is today-true: `self.idlers[0].od` → "error: Geometry has no
  projection '.od'" (misrouted into cross-sub-geometry access; D3
  re-verified). ε's signal is this fixture ceasing to mis-diagnose.
- `count-redirect-quality-target` (probe #9) → PASS. `self.idlers.count`
  redirect ("cannot access aggregation 'count' … use 'idlers.count'
  directly") is the diagnostic-quality bar ε aligns #8 with.
- `uniform-resolver-upstream` → PASS, DAG-direction (cross-batch, hard
  edge ε←#5424). See batch-shape change 1: the uniform-member-access
  resolver is ε's substrate for post-index-hop member resolution; ε must
  not re-patch the bespoke matchers #5430 retires.
- `out-of-range-silent-baseline` (D3 adversary addition) → PASS. BOTH
  out-of-range forms are SILENT today: literal `idlers[5].od` on count 2
  evals exit 0 with `d5 = undef` and `check` prints "All constraints
  satisfied."; computed index likewise (only an info note about dynamic
  indices). Fixtures: `indexed_sub_oob_{literal,computed}_silent_undef.ri`
  — the defect baselines ε's diagnostics replace (INV-SF-1 territory).
- `misroute-second-variant` (D3 adversary addition) → PASS. The probe-#8
  misroute has a SECOND variant: with a no-geometry element type,
  `self.idlers[0].od` fails with the generic poison tail "member access
  not yet supported: .od" (expr.rs:6301) instead of the geometry-projection
  error. ε's parity signal covers BOTH variants
  (`fixtures/indexed_sub_self_member_nogeom_unsupported.ri`).
- `out-of-range-rejection` (G6 branch 4, producer-self) →
  `E_INDEXED_SUB_INDEX_OUT_OF_RANGE` is ε's own deliverable (computed
  index at eval; literal-on-static-count at compile per open Q3); ε's
  tests observe both against the silent baselines above.

## ζ — integration gate: printer-pattern example, parity-verified (§7 ζ)

- `hand-placed-twin-substrate` → PASS, binding CORRECTED by the D3
  adversary: the hand-placed twin is real on the BUILD surface only (2
  disjoint solids at expected AABBs in STL + STEP; eval-value `.body`
  cells are undef even hand-placed — see γ). ζ's parity assertions
  therefore bind to mesh dump + STEP (as the leaf signal already states);
  any eval-cell-level or distance-query assertion in the example is a
  γ-NEW-capability check, not hand-placed parity. Example-authoring note:
  `orient_identity()` currently triggers a "cannot infer return type of
  zero-arg function, defaulting to Real" warning on every check/eval/build
  (silent-default-Real wart, already tracked as #5344 with follow-up #5380)
  — the CI-run example should avoid or annotate it.
- `printer-idiom-anchor` (consumer premise) → PASS, anchor REFRESHED (see
  batch-shape change 3): 16 `IdlerPulley(` at-transform3 sites in today's
  679-line printer.ri; ζ distills into `examples/indexed_idler_array.ri`,
  no printer.ri contention with belt μ #5445.
- `examples-ci-substrate` → PASS. `crates/reify-compiler/tests/examples_smoke.rs`
  recursively compile-gates every `examples/**/*.ri` (recursive discovery
  since 2026-04-26) — the example is CI-run with NO new test infrastructure;
  the parity assertions extend the existing reify-eval suites (§7 preamble;
  no new gate-resident test file, drift-guard registration not tripped).
- `full-stack-upstream` (G6 branch 3) → PASS. Every capability ζ's parity
  signal asserts (indexed elaboration, per-instance realization+posing,
  range forall, addressing) is produced by ζ's own dependency closure
  {α,β,γ,δ,ε} — hard edges wired; nothing lives downstream of ζ.
- `mesh-golden-format` — open Q4 (T5 golden-AABB harness vs body-set
  comparator) decided at ζ; both substrates exist on main.

## η — GUI: instance arrays in tree + inspector (§7 η)

- `debug-mcp-substrate` → PASS. `gui/src-tauri/src/debug_server.rs` +
  viewport_state/meshCount/store_state surface exists (grep re-verified
  2026-07-25); η's signal uses the established debug-MCP assertion shape
  (mesh count + selection highlight + store_state).
- `entity-path-substrate` → PASS. Collection subs already surface count +
  member cells (probe #1); η renders indexed children with the EXISTING
  entity-path scheme (probe #1's positional NodeId paths) — no new naming
  scheme invented (open Q5 decides rendering only).
- `realization-upstream` → PASS, DAG-direction: η depends on γ (real
  meshes to count/highlight); feeds σ2, not ζ.

## θ — docs, chunks, skill, exemplar, discoverability (§7 θ)

- `chunk-files-exist` → PASS. `crates/reify-mcp/src/tools/chunks/{collections,structures}.md`
  present (ls re-verified 2026-07-25); signatures registry-verified in θ's
  own diff per the docs-truth gate (the #5347/#5364 phantom-signature
  precedent is the cautionary case).
- `skill-file-exists` → PASS. `.claude/skills/reify-design/SKILL.md`
  present; cheatsheet gains the idiom + the "patterns fuse geometry;
  indexed subs stay addressable" rule + indexed-vs-keyed contrast (§3.2).
- `best-practices-exemplar` → PASS with coordination note (batch-shape
  change 2): `examples_smoke.rs` recursive discovery auto-gates any
  `examples/best_practices/*.ri`; #5397 (corpus seed) is live-blocked; θ
  adds its file + INDEX.md line additively.
- `discoverability-acceptance` (producer-self) → the intent-query
  acceptance ("place N pulleys in a row" surfaces the indexed-sub section)
  is θ's own deliverable, per the overlay's docs-truth item 4.

## σ1 — dynamic domain bound → warm re-elaboration + eviction (§7 σ1)

- `collection-count-phase-substrate` (probe #11) → PASS. The
  `engine_edit.rs` collection-count re-elaboration phase is landed and
  proven for member value cells (task 2629) + Connect decls (task 2690):
  `child_value_cells` re-elaboration + `forall_emitted` ledger at
  ~engine_edit.rs:1984–2206 (grep re-verified 2026-07-25). σ1 extends this
  path with the γ expansion helper; it does NOT wait on the absent
  SchemaNode-style re-elaboration (#4684's deferral — related, not
  blocking).
- `classifier-substrate` (probe #12) → PASS. Count cells already
  Structural (morph-ineligible, re-elaboration route) — no classifier work.
- `eviction-risk-named` → producer-self: dropped-tail realization/mesh
  eviction correctness is σ1's own named risk with its own assertions
  (§3.7 "stated honestly"); dangling refs get
  `E_INDEXED_SUB_INDEX_OUT_OF_RANGE` (ε's code, eval-graph site), never
  silent undef.

## σ2 — identity/GUI continuity under count edits (§7 σ2; phase-2 gate)

- `identity-contract-fixed` → PASS by construction: §3.2 fixes
  grow-appends/shrink-truncates/no-renumbering NOW (phase-1 shapes
  everything); σ2 asserts NodeId/entity-path/cache survival across grow and
  mesh removal on shrink — the contract's observable.
- `debug-mcp-scenario-substrate` → PASS. Same debug-MCP surface as η
  (selection + meshCount + store_state); σ2 composes η's tree rendering
  (hard edge σ2←η) with σ1's re-elaboration (hard edge σ2←σ1).

## σ3 — range-forall dynamic bounds via the template ledger (§7 σ3)

- `forall-ledger-substrate` → PASS. The `forall_templates` +
  `forall_emitted` count-keyed re-emission ledger exists in
  `engine_edit.rs` (grep re-verified; the 2629/2690 twins are the test
  pattern σ3 sits beside). σ3 keys range templates on their bound cells —
  constraint + connect arms; chain stays compile-time-info, matching
  collection forall.
- `range-lowering-upstream` → PASS, DAG-direction: δ's literal-bound
  unroll is the template σ3 makes count-keyed (hard edges σ3←σ1, σ3←δ).
