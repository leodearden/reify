# Capability manifest — placement-relations-belt.md

Authored at decompose time 2026-07-24 (decompose session spawned from the
author session, brief `2026-07-24-decompose-placement-relations-belt.md`).
Binds each leaf's user-observable signal to substrate evidence per the /prd
gates (G3+G6 mechanized). Machine-readable twin:
`placement-relations-belt.capability-manifest.yaml` (task ids stamped at
`commit_planning`).

**Probe provenance.** Baseline probes executed this session with the
2026-07-24 debug binary (`target/debug/reify`, built 14:59) on main
`ef6863e770`; committed probe fixtures at `fixtures/placement_b3_operand_type.ri`,
`fixtures/placement_b5_world_frame.ri`, `fixtures/placement_b8_min_clearance_structs.ri`,
`fixtures/placement_b9_no_undeclared.ri` (captured outputs quoted per leaf below).
Grammar gate: `fixtures/placement_relations_surfaces.ri` exercises every new
surface (`at auto`+relate, `.world_frame`, `in_frame`, `min_clearance` scalar+list
forms, `self_clearance`, `self_interferes`, `no_undeclared_interference`, `wire`,
`BeltPath`) — `tree-sitter parse --quiet` exit 0, zero ERROR nodes.
`grammar_confirmed=true` on every leaf: no novel productions.

## Batch-shape changes made at decompose (G4 re-walk findings)

The PRD was authored in parallel with the 2026-07-24 silent-undef family
sessions; their batches filed first. Three resolutions, recorded here and in
task metadata — the PRD document itself is NOT edited (its §10 remains the
authoring-time plan; this manifest is the decompose-time delta record):

1. **δ (zero-auto relate verification) is DROPPED — superseded by DIC α
   (#5415)**, `declared-intent-consumption-accounting.md` mechanism A (filed
   first, task/5398 branch). DIC α delivers static verification at fixed
   poses (violated ⇒ Error; satisfied ⇒ silent pass counted in the DIC ζ
   #5420 check ledger; unverifiable ⇒ diagnostic naming why). Our δ's
   `W_RELATE_NO_AUTO`-even-when-satisfied arm **contradicts** DIC α's
   recorded decision + its B2 test ("zero relate diagnostics" on the
   satisfied case) and is superseded by ledger observability (INV-SF-3
   conformant). μ's trap-(a) prereq rewires δ→#5415.
2. **ε narrows to relate-family regression coverage under the general
   error-severity exit gate #5403** (eradicate-silent-undef ε, filed first)
   — exactly ε's PRD dispatch note, with the concrete id now known. ε gains
   a hard dep edge ε←#5403. An independently-implemented relate-family exit
   escalation would be a per-code bolt-on, banned by INV-SF-2.
3. **ζ narrows to the `LocatedPort` arm only** (all §7.3 contracts anyway).
   Decision 6's "the ad-hoc-`@` Bool Eq branch is superseded by the same
   lowering" claim is void: DIC ε (#5419) owns that branch's disposition
   (compile-time generation refusal of the permanently-inert constraints +
   example sweep). Seam note in ζ's task text; no dep edge (different arms
   of `connect.rs` lowering; merge queue serializes textual overlap).

**G7 walk** (against `docs/legibility/design-invariants.md`, read from
task/5395 — note: NOT yet on main at decompose time, task 5395 still
`deferred`; the brief's "landed" note was premature but the content is
final on that branch). No waivers. Per-slug: `undef-has-provenance` — α's
placeholder-with-path entries, λ's never-Undef degenerate errors, decision 7;
`error-severity-exits-nonzero` — ε's narrowing (resolution 2) removes the
one bolt-on risk; `declared-intent-consumed-or-diagnosed` — δ's drop
(resolution 1) defers to DIC's ledger; ζ makes `connect` intent consumed;
`indeterminate-attributable-transient` — clearance-family Kleene verdicts
carry the unrealized body's provenance path and clear when geometry
realizes (η/ι task texts instruct populating DIC δ #5418's typed
`IndeterminateReason` carrier if landed — coordinate, don't duplicate);
`placeholders-owned-and-loud` — no stand-in-typed public signatures
introduced; `diagnostics-carry-codes` — every new diagnostic
(`E_POSE_CYCLE`, `E_PORT_FRAME_ILL_FORMED`, clearance indeterminates,
belt degenerate-input errors) is instructed to carry a `DiagnosticCode`.

## α — `RealizedBodySet` walk primitive (§7.1.1)

- `relate-pose-substrate` → PASS. Relate-solved auto poses + explicit `at`
  poses exist as `Value::Frame` (geometric-relations ζ #4386 done;
  `crates/reify-eval/src/relate_solve.rs` `solve_scopes` :826, verified this
  session). `ApplyTransform` placement landed (#3901).
- `aux-markers-exist` → PASS. `is_aux` present across compiler decl surfaces
  (`crates/reify-compiler/src/{auto_type_param.rs,compile_builder/entities_phase.rs,conformance/checker.rs,connect.rs}`,
  grep this session).
- `single-walk-invariant` (G7 `no-lockstep-duplication`) → PASS by
  construction: γ (snapshot enrichment) and η/θ/ι (clearance) name α's walk
  as their only body-enumeration source; μ consumes transitively.
- `undef-placeholder-provenance` (INV-SF-1) → PASS by construction: Undef
  leaf yields placeholder-with-path, never dropped (§7.1.1; consumers go
  loudly indeterminate — see η/B8 binding).

## β — `.world_frame` + `in_frame` + staging/cycle diagnostic (§7.1.2–7.1.3)

- `member-absent-baseline` (signal premise) → PASS. Probe
  `placement_b5_world_frame.ri`: `reify check` → `error: structure 'Part'
  has no member 'world_frame'`, exit 1 — capability absent today, absence is
  loud (no silent wrong answer to guard against); β delivers the member.
- `grammar-surface` → PASS. `.world_frame` member access + `in_frame` call
  parse (grammar fixture, exit 0; `sub.member` in relate operands proven by
  probe p2a / `examples/geometric_relations/bolt_plate.ri` on main).
- `frame-conversion-substrate` → PASS. `frame_to_frame`
  (`crates/reify-stdlib/src/geometry.rs:3183`); relate_solve writes
  `Value::Frame` poses (§4 table, re-verified).
- `cell-dep-graph-substrate` → PASS. `build_dependent_cells`
  (`crates/reify-eval/src/engine_eval.rs:1553`, #5188 done — same evidence
  DIC's manifest binds); §7.1.3's cycle check extends this graph with
  `pose(s) → .world_frame(s)` edges.
- `pose-cycle-rejection` (G6 branch 4, B5) → PASS, producer-self:
  `E_POSE_CYCLE` is β's own deliverable; β's boundary test observes the
  diagnostic fire and nonzero exit. Compile-phase Error-severity already
  gates `check` exit (cmd_check, verified); build-time detection gates via
  cmd_build's Severity::Error exit gate (task 4458 landed — INV-SF-2 house
  pattern).

## γ — sub poses into snapshots (§7.1.4)

- `snapshot-substrate` → PASS. Kinematic trio + snapshot body enumeration:
  `try_eval_kinematic_query` dispatch (`crates/reify-eval/src/geometry_ops.rs:4250–4292`,
  re-verified; `named_steps` top-level-let resolution §2 trap).
- `sub-bodies-unreachable-baseline` (signal premise, B13) → PASS. Sub
  instances unreachable / sub `at` poses not carried (PRD §2 probe-verified
  at authoring; the seam is the `named_steps`-only body resolution —
  unchanged since, no commits to geometry_ops.rs snapshot region since
  366c63a679).
- `walk-upstream` → PASS, DAG-direction: α is a hard prereq; γ consumes
  `realized_bodies` (7.1.4) — never a second walk.
- `byte-identical-backcompat` (C2) → producer-self: γ's own test asserts
  existing top-level-let snapshot behavior unchanged.

## ε — relate-family error-exit regression coverage (narrowed; was trap c)

- `general-gate-upstream` → PASS, producer:#5403 upstream (hard dep edge).
  The general "any Error-severity diagnostic ⇒ nonzero exit" gate + burn-down
  allowlist + ratchet is eradicate-silent-undef ε (#5403, deferred at
  decompose, flipping with its own batch).
- `compile-phase-already-gated` (baseline REFINED vs PRD §2 trap c) → PASS.
  Probe `placement_b3_operand_type.ri`: statically-typed relate operand
  errors ALREADY exit 1 with typed codes (`error: concentric: operand Real
  has no Axis projection`, `DiagnosticCode::DatumProjectionUnavailable`,
  `crates/reify-compiler/src/relation_signatures.rs:546`). The PRD's blanket
  "relate operand type errors exit 0" holds only for the ENGINE-phase
  channel: gradualism skips `Type::Error | Type::TypeParam` operands at
  compile (`relation_signatures.rs:489–492`) and engine-phase diagnostics
  flow through `report_eval_output`/`finish_check` without flipping check's
  exit (PRD §2 seam; two independent 2026-07-24 probe sessions, e.g. #5386's
  confirmed error-text-exit-0). #5403 closes that channel generally; ε pins
  the relate family on both phases with committed regression fixtures and
  guards that relate-family codes never enter the burn-down allowlist.
- `no-bolt-on` (INV-SF-2) → PASS by construction (resolution 2 above): ε
  builds no relate-only escalation; if #5403 is cancelled, ε escalates
  rather than implementing a per-family gate.

## ζ — connect `LocatedPort` frame alignment (§7.3, narrowed per resolution 3)

- `locatedport-substrate` → PASS. `trait LocatedPort : Port { param frame :
  Frame3 }` + refinement machinery
  (`crates/reify-compiler/stdlib/ports.ri`; connect.rs:217, 353–401 — §4
  table, re-verified stdlib header this session).
- `fasten-over-frames` → PASS. `fasten` relation over Frames
  (relation_signatures.rs:186–189, §4 table); the mate lowers to
  `RelationInstance(fasten, …)` in the enclosing scope's relation set —
  catalogued §3.5 ConstraintSolver seam via the relate-solve, no new seam.
- `adhoc-branch-not-ours` → PASS (resolution 3): the ad-hoc-`@` `Bool Eq`
  branch (`connect.rs:584–622`, re-verified :584–590 this session) is DIC ε
  #5419's territory (generation refusal). ζ adds the LocatedPort arm beside
  it and does not touch the ad-hoc disposition.
- `degenerate-assertion-path` → PASS with coordination note: when the target
  sub is not `at auto`, the mate degenerates to a verified assertion — ζ's
  task text instructs routing through DIC α #5415's zero-auto static-verify
  arm (G7 `no-lockstep-duplication`), not a second verify path.
- `ill-formed-frame-rejection` (G6 branch 4) → producer-self:
  `E_PORT_FRAME_ILL_FORMED` is ζ's deliverable; its test observes the typed
  error fire on a non-orthonormal `Frame3`.

## η — `min_clearance(a, b)` Structure/Geometry overload (§7.2, decision 7)

- `kinematic-trio-substrate` → PASS. Arity-3 snapshot trio + pose cache:
  `crates/reify-compiler/src/units.rs` signatures + `try_eval_kinematic_query`
  (`geometry_ops.rs:4250–4292`); η's eval plugs in as a sibling dispatch
  (`try_eval_conformance_query` pattern) — catalogued seam, no orphan.
- `structure-operand-absent-baseline` (signal premise, B7/B8) → PASS. Probe
  `placement_b8_min_clearance_structs.ri`: `min_clearance(a, b)` over sub
  instances today → constraint `INDETERMINATE … operator undefined for these
  operand kinds: StructureInstance`, `No constraints violated (1
  indeterminate).`, exit 0; the `let` form yields silent Undef (no note).
  Capability absent; current behavior is the quiet-indeterminate the PRD
  replaces with loud-indeterminate.
- `loud-indeterminate-rejection` (G6 branch 4, B8) → producer-self: η's own
  Undef-body fixture observes the diagnostic naming the body's provenance
  path (α's placeholder entries make this constructible) and the verdict
  INDETERMINATE — never TRUE. Task text: populate DIC δ #5418's typed
  `IndeterminateReason` if landed.
- `geometry-queries-substrate` → PASS. `intersects`/`distance` +
  `BRepExtrema` distance probes exist on geometry lets (units.rs
  GEOMETRY_QUERY_NAMES, §4 table).

## θ — list forms + AABB prefilter

- `list-type-surface` → PASS. `Type::List` exists (reify-core/src/ty.rs:132,134);
  list literals parse (grammar fixture `min_clearance([bush, boss], [bush,
  boss])`).
- `pairwise-over-walk` → PASS, DAG-direction: η upstream (hard edge); θ is
  the N²/M×N generalization over the same α body sets.
- `aabb-prefilter` → producer-self: prefilter unit test is θ's own signal;
  the tactical margin default is PRD §12 Q2 (decided during θ/ι).

## ι — declared contacts + `self_interferes`/`self_clearance` + `no_undeclared_interference` (decisions 8–9)

- `relate-connect-edges-readable` → PASS. Whitelist derives from relate
  `coincident`/`flush`/`fasten` edges (`RELATION_FN_NAMES`,
  relation_signatures.rs:54–68, re-verified) + connect edges
  (connect.rs machinery) — model facts, not annotations (G7
  `contracts-machine-checked`).
- `constraint-surface-baseline` (signal premise, B9) → PASS. Probe
  `placement_b9_no_undeclared.ri`: `constraint
  no_undeclared_interference(self)` today → `warning: constraint expression
  has type Asm, expected Bool` + INDETERMINATE + exit 0 — the name is
  unbound (no silent success to displace); ι registers the builtin +
  semantics.
- `undeclared-pair-naming-rejection` (G6 branch 4, B9) → producer-self: ι's
  whitelist fixture observes FALSE naming only the un-declared overlapping
  pair, flipping TRUE after declaration. Needs ζ for the connect-edge
  whitelist arm (hard edge ι←ζ).
- `curve-domain-form` → PASS. Path sampling substrate: 3D curve ops
  (`crates/reify-ir/src/geometry.rs:883–933`, re-verified) provide the
  arc-length parameterizable segments; the sampling window is ι's own
  deliverable (B9 curve fixture).

## κ — `wire()` kernel op (decision 11)

- `wire-op-absent` → PASS. No MakeWire/JoinWire/concat op anywhere in
  reify-ir/reify-eval/reify-kernel-occt (grep re-verified this session,
  empty) — κ is the pre-registered Rust escape, §3.1 op-execute
  (`engine-integration-norm.md`) GeometryOp like its siblings.
- `occt-makewire-available` → PASS. OCCT prebuilt kernel linked
  (`crates/reify-kernel-occt/src/lib.rs`; `BRepBuilderAPI_MakeWire` is core
  OCCT topology, same module family as the linked BRepAlgoAPI/BRepExtrema
  uses); OCCT tolerance is the contiguity oracle — non-contiguity ⇒ typed
  error (G6 branch 4, producer-self: κ's test observes it).
- `sweep-consumer` → PASS. `Sweep`/`SweepGuided` exist
  (reify-ir/src/geometry.rs:883–933) — B11's sweep-along-wire consumer is
  landed substrate.

## λ — `belt_path` stdlib (§7.4, §8)

- `ri-expressibility-substrate` → PASS. Trig/vector math for tangent
  construction (`atan2`/`acos`/`sqrt`, math_signatures.rs:49, 97–99, §4
  table); `Arc{center,radius,start_angle,end_angle,axis}` + `LineSegment`
  exist (geometry.rs:883–933); closed-form length = Σ tangents + Σ arcs
  (exact by construction — tangent points and arc endpoints are the same
  computed expressions; no numeric-floor hazard: no iterative solve, no
  mesh, no eigensolver — G6 branch 1/2 N/A).
- `generate-of-geometry-broken` (F0) → PASS, producer:#5385 upstream (HARD
  dep edge, wired): `generate(n, |i| geom)` yields silent Undef today
  (list_helpers.rs:62 compile-side exists, re-verified; eval-side defect is
  #5385 pending-high). λ text: if `flat_map`'s geometry path shares the
  defect, report extending #5385 — never work around.
- `wire-join-upstream` → PASS, producer:κ upstream (hard edge) — F1
  pre-registered escape.
- `pipe-fallback` (F2) → PASS. #5343 (pipe non-+Z) is SOFT: `sweep` with a
  posed profile is the documented fallback (μ text carries it); no edge.
- `degenerate-inputs-rejection` (G6 branch 4) → producer-self: overlapping
  pitch circles / tangent nonexistence ⇒ error naming pulley indices, never
  Undef geometry (λ's own boundary test; INV-SF-1).
- `module-placement` → tactical: conform to stdlib-module-arch outcome at
  dispatch (PRD §12 Q3; content ours, layout theirs — G4 row).
- `findings-doc-deliverable` → producer-self:
  `docs/notes/belt-path-language-findings.md` with pre-registered F0–F2
  rows is part of λ's signal (§8 protocol).

## μ — printer_v01 rear-drive routing v2 (THE LEAF)

- `printer-on-main` → PASS with refreshed anchor: `prj/printer_v01/printer.ri`
  tracked on main, now 679 lines (PRD's "995 lines / :341–383" anchors are
  stale — the dogfood continued: belt loops closed with idlers, tendons
  re-seated, constructor-override workaround retired). The PREMISE stands:
  placement is still hand-solved literal `at transform3(…)` arithmetic
  (e.g. `let idler_in_l = neg_rail_x + r_pitch` + `sub idlers_r = … at
  transform3(orient_identity(), vec3(idler_in_r, 0mm, 0mm))`, re-verified
  this session) with comment-borne tangency reasoning and an eyeball
  interference oracle. μ's task text cites the idiom, not line numbers.
- `trap-a-protection-upstream` → PASS, producer:#5415 upstream (DIC α, hard
  edge — resolution 1): zero-auto relate scopes verify instead of vanish
  while μ authors relate blocks.
- `full-stack-upstream` → PASS, DAG-direction: α, β, η, ι, λ all hard
  prereqs (wired); perturbed-pulley build-FAIL regression is μ's own
  boundary assertion over ι's constraint — every asserted capability is in
  μ's dependency closure (G6 branch 3 clean).
- `tendon-solid-route` → tactical fallback documented: `sweep` posed-profile
  vs #5343 `pipe` — decide at dispatch by checking #5343 (PRD §12 Q5).

## ν — doc-chunk updates (project gate leaf 1)

- `chunk-files-exist` → PASS. `crates/reify-mcp/src/tools/chunks/{constraints,connect,geometry,stdlib}.md`
  all present (ls re-verified).
- `no-dup-5389` → PASS. #5389 (pending) owns the EXISTING kinematic-trio
  docs; ν documents only the NEW surfaces; ζ's diff reconciles connect.md's
  frame-alignment claim (G4 row; coordination note in ν/ζ task text).
- `signature-spot-verify` → producer-self: each documented signature grepped
  against the compiler registries in ν's own diff (signal).

## ξ — reify-design skill cheatsheet update (project gate leaf 2)

- `skill-file-exists` → PASS. `.claude/skills/reify-design/SKILL.md` present.
- `idiom-content-upstream` → PASS, DAG-direction: β, ζ, ι, λ hard prereqs —
  the cheatsheet documents landed idioms only.

## ο — discoverability acceptance (project gate leaf 3)

- `chunk-topic-surface` → PASS. The reify-mcp chunk-topic query surface
  exists (the chunks/ corpus above is its content); ο's scripted acceptance
  (intent queries → new idioms surfaced) runs against ν/ξ's landed content
  (hard edges ν→ο, ξ→ο). Committed transcript is the deliverable —
  producer-self.
