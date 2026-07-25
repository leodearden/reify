# Capability manifest — resolution-unification (v0_6)

PRD: `docs/prds/v0_6/resolution-unification.md` (committed `b1f265676e`, amended
`2c7f942b46` — stdlib-namespace reconciliation folded in-file). Machine-readable
sidecar twin (stamped with real task ids at `commit_planning`):
`resolution-unification.capability-manifest.yaml`.

Decompose session 2026-07-25 (prd-reify-2661800). Static evidence grep-verified
2026-07-25 against main `3d4abb7d22`; behavioral premises probed via the D3
workflow (`wf_9fd40737-148`, 8 leaves) + direct decompose-session probes, all
against a freshly rebuilt debug binary (probe-binary-freshness rule: `reify`
embeds the stdlib via `include_str!`). PRD line anchors (vs `366c63a679`)
re-resolved to current main where drifted; bindings below are pattern-anchored.

Two D3-adversary corrections folded into task texts (wf_9fd40737-148):
1. **Enum fixture syntax**: the grammar is `optional(pub) 'enum' Name` — there
   is NO `enum def` form. #5387's own repro text (`pub enum def FitClass`) is a
   parse error whose exit-1 masked the defect pre-state. λ's fixtures use
   `pub enum FitClass { … }` (verified: parses clean standalone; cross-file
   reference still fails `unresolved name: FitClass` — the #5387 symptom is
   real under correct syntax).
2. **#10 is not silent**: `reify eval` on the cross-file-fn fixture surfaces
   `warning: import "mathlib" not resolved by this entry point` and
   `note: Entry.y is undef (because: op contract failed (OpContractViolation))`
   (stdout `Entry.y = undef`, exit 0). The PRD §0 "silently produces undef"
   phrasing is stale — the cause-note IS surfaced. The deliverable (`42`, not
   `undef`) is unchanged; λ's test pins stdout content, not silence.

Batch shape: **15 tasks α–ο** (P0 α; P1 β γ δ ε ζ η; P2 θ ι κ λ; P3 μ ν ξ ο).
The spawn brief's "14 tasks" was a miscount; the PRD §8 list is authoritative.

Cross-PRD notes:
- `stdlib-namespace.md` decomposed first (batch 5493–5505, filed 2026-07-25).
  Four of its tasks carry textual gates on THIS batch; this session wires the
  real edges as second filer: 5500 (its η) → β; 5502 (its ι) → μ; 5503 (its κ)
  → θ, ι, κ; 5505 (its ν) → β.
- Seam partition per the D-6 amendment: namespace α (5493) owns compile-side
  collision alignment in `entities_phase.rs`/`enums_phase.rs`; **θ here owns
  only the units seed loop** (`compile_builder/units_phase.rs::phase_units`,
  verified `:32`) — disjoint sites, no contested pair.
- Known benign cross-check: 5493's sidecar `delivered_check` expects
  `fn find_template_with_prelude` **present** (pre-state census evidence); κ
  here deletes it in P2. 5493 is in-progress now and lands long before κ
  dispatches, so the checks never overlap live — noted so a future reader does
  not read the pair as a contradiction.
- #5387 (blocked, live): λ owns the mechanism per §6 row 2 — 5387 gains a dep
  edge on λ (subsumption; λ keeps 5387's acceptance tests as #11).
- #5391 (bookmark): gains dep edges on γ, λ, μ, α (gates 2–4).

## α — strip stale eval-time mirrors from stdlib examples *(P0 leaf)*

- **mirror-exists-and-shadows** → PASS. `examples/stdlib/ports_breadth.ri`
  carries the local `ThreadSpec` mirror (`:44`, lacking `thread_form`) and the
  "EVAL-TIME RE-DECLARATION NOTE" dead-semantics block (`:16-24`); file is
  auto-covered by `crates/reify-compiler/tests/examples_smoke.rs` (in-file
  comment + directory walk).
- **regime-b-live-mirror-strippable** → PASS *(probe, D3)*. The re-declaration
  hazard is dead: a mirror-stripped copy evaluates clean with stdlib
  `ThreadSpec` (incl. `thread_form`) resolving via prelude fallback. Signal #18
  is achievable — the drifted mirror is the only thing shadowing it.

## β — `compile_program` + `CompiledProgram` + DefEnv skeleton *(P1 intermediate → γ δ ε ζ ξ θ; also consumed cross-PRD by 5500, 5505)*

- **bridge-exists-with-checker** → PASS. `compile_entry_with_stdlib_cfg_checked`
  (`module_dag.rs:911`) already takes `resolver/cfg/checker: &dyn
  ConstraintChecker` — the `compile_program` signature is a return-type
  extension, not new plumbing.
- **merge-helper-shared-two-callers** → PASS. `merge_imported_pub_templates`
  (`module_dag.rs:806`) has exactly two callers: the check bridge
  (`module_dag.rs:995`) and the GUI twin (`gui/src-tauri/src/engine.rs:1029`)
  — P1 net-deletes one caller, writes no new merge logic (D-2).
- **import-units-lack-stdlib-prelude** → PASS *(probe, D3; #19 premise)*.
  Import units compile via `compile_with_prelude_refs` over direct-import
  preludes only (`module_dag.rs:474-486`); GUI twin skips std imports
  (`engine.rs:946` region). An imported module consuming stdlib names
  (`param w : Length = 10mm`) silently poisons today. Fix substrate exists:
  cached `load_stdlib_context()` (`stdlib_loader.rs:503`, `&'static`) — reuse,
  don't rebuild per unit. Interim sanctioned; superseded by namespace N3
  (recorded both PRDs, not drift).
- **import-unit-diagnostics-unread** → PASS *(probe, D3; I2 third shape)*.
  Only the `Err` path reaches `import_error_diags`; a satisfied import that
  compiles `Ok` with Error diagnostics is swallowed (imported
  `param q : Frobnicate = zorp()` → "All constraints satisfied", exit 0 —
  the 4575-class silent-accept sentinel, observed as pre-state). β builds the
  aggregation with file attribution; new diagnostics carry `DiagnosticCode`
  (INV-SF-6).

## γ — eval + build go multi-file, `--cfg` *(P1 leaf; #5391 gate 2)*

- **single-file-pre-state-real** → PASS *(probe, D3)*. `reify eval`/`build` use
  `parse_and_compile` (`reify-cli/main.rs:173`, no import DAG): importing entry
  yields `import "parts" not resolved by this entry point` + unknown-structure
  error today.
- **cfg-threading-substrate** → PASS. `check`'s `--cfg` threading
  (`build_cfg_set`, conditional-compilation.md landed) is the mechanism D-4
  reuses; semantics unchanged.
- G7 waiver: `error-severity-exits-nonzero` — D-11 preserves each command's
  current exit policy; the invariant is owned by #5386/#5403 (real edge there,
  none here).

## δ — test/report/explain/doc go multi-file, `--cfg` *(P1 leaf)*

- **single-file-pre-state-real** → PASS *(probe, D3)*. Same `parse_and_compile`
  root as γ; per-command output difference on an importing entry is the signal.
- G7 waiver: `error-severity-exits-nonzero` — same rationale as γ (D-11).

## ε — GUI: delete the twin, wire `compile_program` *(P1 leaf)*

- **twin-exists-and-requests-deletion** → PASS. `compile_entry_with_imports`
  (`gui/src-tauri/src/engine.rs:885`) is the hand-rolled merge twin; its own
  doc comment requests replacement by a compiler API. Dirty-buffer import-graph
  behavior to preserve at `engine.rs:2250-2260` region.
- **load-from-source-import-silence** → PASS (static). Buffer-only compiles
  silently ignore imports today; D-10 adds an explicit Warning (with
  `DiagnosticCode`, INV-SF-6). Rejection-direction signal #5 observed by the
  GUI suite (`scripts/gui-test.sh` — self-provisioning runner, contract-guarded).

## ζ — LSP diagnostics multi-file *(P1 leaf)*

- **lsp-single-file-pre-state** → PASS. LSP diagnostics compile via
  `compile_with_stdlib` (`reify-lsp/src/diagnostics.rs:132` region) — no import
  DAG, hence false `unknown structure` on multi-file projects. goto-def has its
  own resolver wiring (out of scope, §9).

## η — P1 cross-surface parity harness *(P1 leaf; the P1 integration gate)*

- **parity-premises-producible-from-deps** → PASS (G6 branch 3). Signals #2-#4
  exercise check/eval/GUI/LSP surfaces — all delivered by η's dep set
  {γ, δ, ε, ζ} over β; no capability lives downstream of η (the esc-3436-210
  inversion is absent).
- **drift-guard-substrate-exists** → PASS. `tests/infra/run-all-classification.manifest`,
  `tests/infra/test_no_new_wallclock_upper_bounds.sh`,
  `docs/notes/verify-scope-throughput.md` all present; η's task text requires
  same-diff registrations (+ THROUGHPUT-COUNTS bump if a build_plan pole is
  added) per the overlay rule — esc-4914-162 is the cautionary case.
- G7 waiver: `error-severity-exits-nonzero` — η asserts diagnostic-set
  equality, deliberately not exit-code equality (D-11; owned by #5386/#5403).

## θ — DefEnv full resolution + `Engine::load` + collision normalization *(P2 intermediate → ι; consumed cross-PRD by 5503)*

- **prelude-statics-and-policy-census-real** → PASS. `Engine.prelude:
  &'static [CompiledModule]` (`reify-eval/src/lib.rs:453`);
  `find_template_with_prelude` (`engine_eval.rs:59`); the four-policy
  fragmentation table documented at `prelude_context.rs:32-74`.
- **units-flip-pins-invertible** → PASS. `prelude_module_unit_collision_emits_warning`
  (`unit_registry_tests.rs:1781`) + registry pins exist and are inverted
  same-diff (D-6 breaking change is pinned, not silent);
  `eval_is_idempotent_for_prelude_functions` pins exist for the Q4 overload
  composition decision.
- **units-flip-compile-side-same-diff** → PASS (amendment). The compile-side
  seed loop `phase_units` (`compile_builder/units_phase.rs:32`) carries
  last-wins + the overwrite Warning; θ flips it in the same diff so compile and
  eval never disagree on colliding units. Scope fence: `entities_phase.rs` /
  `enums_phase.rs` collision sites are namespace-α's (5493) — not touched here.
- **enum-fixup-premise-real** → PASS *(probe, D3; #5387 repro)*. Imported
  `FitClass.Clearance` fails check with `unresolved name: FitClass` (parses as
  `MemberAccess`; imported enums unknown at parse). D-9 fixup is
  resolution-phase — no grammar work (grammar_confirmed: true).

## ι — flat-slice call-site migration *(P2 intermediate → κ, λ)*

- **flat-slice-sites-real** → PASS. `&[TopologyTemplate]` sites live in
  `unfold.rs` (2), `structural_query.rs` (5), `engine_build.rs`,
  `engine_eval.rs`; `graph.rs` silent skip verified (`:427` "skip unknown
  structures silently"); conformance mirror at
  `crates/reify-compiler/src/conformance/mod.rs` (`sub_component_validation`).
- **silent-skip-retirement** (#15) → PASS (producer: this leaf;
  negative-assertion direction). Pre-state silence is the verified defect; ι
  builds the structured diagnostic (with `DiagnosticCode`, INV-SF-6) its own
  test observes.

## κ — delete the interim; templates purity *(P2 leaf)*

- **deletion-targets-exist** → PASS. `find_template_with_prelude`
  (`engine_eval.rs:59`), `Engine::prelude` statics (`lib.rs:453`), merge
  consumption at the check bridge (`module_dag.rs:995`) — everything κ deletes
  exists on main today (D-2); I5's deletion is this named task, not an
  aspiration (INV-SF-5 compliant).
- **templates-purity-observable** (#13, #14) → PASS *(probe substrate)*.
  Pre-state: merged import templates DO appear in `entry.templates` (Regime A);
  post-state purity is asserted by κ's own suite + `git grep
  find_template_with_prelude` empty.

## λ — cross-kind end-to-end signals *(P2 leaf; #5391 gate 3 first half; subsumes #5387)*

- **fn-undef-pre-state** (#10) → PASS *(probe)*. Cross-file `pub fn` resolves
  at check; at eval `Entry.y = undef` with the OpContractViolation cause-note
  and unresolved-import warning surfaced, exit 0 (D3 correction: NOT silent —
  see header note 2); post-state stdout prints `42`.
- **enum-crossing-pre-state** (#11) → PASS *(probe; syntax-corrected)*.
  `pub enum FitClass { … }` (no `def` — header note 1) checks clean standalone;
  imported `FitClass.Clearance` fails `unresolved name: FitClass` at the
  importer. λ keeps 5387's acceptance tests (correct-syntax fixtures);
  non-pub invisibility is the rejection half, built by θ/ι's
  visibility-filtered DefEnv and observed by λ's tests.
- **unit-alias-trait-crossing** (#12) → PASS *(probe)*. Verified split:
  `pub unit fortnight : Time = 1209600.0` crosses at **check** (satisfied)
  but fails at **eval** (`error: unknown unit: fortnight`) — the per-kind
  check-vs-eval gap λ pins; the env carries all kinds post-θ (producers θ, ι
  upstream — wired).
- Gate-resident tests → drift-guard registrations same-diff (overlay rule; same
  substrate as η).

## μ — `pub import` re-export *(P3 leaf; #5391 gate 3 second half; consumed cross-PRD by 5502)*

- **pub-import-parses-today** → PASS *(grammar fixture, D3)*. `pub import X`
  parses (tree-sitter, 0 ERROR nodes — PRD §2 verification 2026-07-24,
  re-probed at decompose); `is_pub` recorded at `reify-ast/src/decl.rs:725`
  and consumed nowhere in the compiler (`git grep '\.is_pub'` in
  `module_dag.rs` empty) — grammar_confirmed: true, mechanism absent, μ builds
  it.
- **reexport-nonleak-rejection** (#8) → PASS (producer: this leaf;
  negative-assertion direction). Plain-import chains must NOT leak — observed
  by μ's own boundary fixture, not assumed.

## ν — entity-import narrowing *(P3 leaf)*

- **importkind-carries-data** → PASS. `ImportKind::Entity/EntityAliased/
  Destructured` (`reify-ast/src/decl.rs:704`) parse today and are consumed
  only by LSP goto-def (`reify-lsp/src/goto_def.rs:132-173`) — data present,
  compiler consumer absent, ν builds it (D-8: exposure filter only).
- **narrowing-rejection-fires** (#9) → PASS *(probe, D3; negative-assertion)*.
  Pre-state: `import parts.Pulley` does not reject sibling-template use
  (silent-accept sentinel); post-state rejection built by ν, observed by its
  boundary test with Error text (not bare exit-1 — the pre-state also exits
  nonzero on other grounds; match on the diagnostic).

## ξ — imported-file module-header mismatch surfacing *(P3 leaf)*

- **mechanism-landed-extension** → PASS. `attach_module_path_diag` applied per
  non-std module in the DAG walk (`module_dag.rs:527-529` — verified live);
  the Warning drowns because import-unit diagnostics never reach
  `entry.diagnostics` pre-β (I2 third shape). ξ = surface with imported-file
  attribution over β's aggregation (`module-and-visibility-hardening.md`
  extension, §6). Deps: β (real edge). Diagnostic carries `DiagnosticCode`.

## ο — GUI files panel from `sources` *(P3 leaf)*

- **substrate-present-panel-absent** → PASS. `source_map` exists
  (`gui/src-tauri/src/engine.rs:153`); no frontend files panel exists today
  (grep empty — producer is this leaf, not an orphaned consumer). Signal is
  debug-MCP `store_state` observable (established GUI-test vocabulary).
  Deps: ε (real edge — panel reads `CompiledProgram.sources` via ε's wiring).

## G7 walk (design invariants, docs/legibility/design-invariants.md)

- `undef-has-provenance` — λ **fixes** a silent-undef path (#10); no task adds
  a root-undef path. Pass.
- `error-severity-exits-nonzero` — waived on γ, δ, η (D-11: per-command exit
  policy preserved; invariant owned by #5386/#5403). Stamped in
  `metadata.g7_waivers` on those tasks.
- `declared-intent-consumed-or-diagnosed` — D-10 (ε), #15 (ι), I2 third shape
  (β), #16 (ξ) each convert a silent drop into a diagnostic. cfg-unsatisfied
  imports (I3) are conditional-compilation semantics, not silent drops. Pass.
- `indeterminate-attributable-transient` — N/A (no Indeterminate semantics
  touched).
- `placeholders-owned-and-loud` — I5's deletion is named (κ); β's deprecated
  wrapper (if kept per §10 Q1) must cite κ's real task id via PTODO — encoded
  in β's task text. Pass.
- `diagnostics-carry-codes` — every new diagnostic (β aggregation, ε D-10,
  θ D-6 warnings, ι #15, ξ #16) is required to carry a `DiagnosticCode` —
  encoded in the tasks' texts. Pass.
