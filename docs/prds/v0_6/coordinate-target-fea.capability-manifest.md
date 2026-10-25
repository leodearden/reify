# Capability manifest — coordinate-target-fea

PRD: `docs/prds/v0_6/coordinate-target-fea.md`. Machine-readable twin:
`coordinate-target-fea.capability-manifest.yaml` (stamped by `commit_planning` on 2026-10-06; task ids
in each block heading below). Filed batch: α #8251, ζ #8252, β #8253, γ #8255, ε #8256, θ #8257,
ι #8258, κ #8259, λ #8260, η #8265, M1 #8261, M2 #8262, M3 #8263, ω #8264; standalone findings #8248
(adaptive bc_override defect) and #8249 (per-instance dispatch gap); docs carrier #8250.

**Evidence base.** Bound 2026-10-06 against main `c0c3155242` and the unmerged branch `task/7189`
`676fde4c10`. Code evidence is cited by symbol, with the decompose-time facts F1–F9 from the
decompose drafts. CLI probes ran on `/home/leo/src/warm-lanes/base/target/debug/reify`, built
2026-10-05 from `e5f638d029` ("Merge task/7448 into main"). That warm-base generation (`target.gen.561`)
has since been superseded by one at `cc0b2c8793`. The two committed grammar fixtures,
`tests/prd-gate/fixtures/coordinate_target_fea_kinds.ri` and
`tests/prd-gate/fixtures/coordinate_target_fea_pose_rejected.ri`, were re-run on the `e5f638d029`
binary on 2026-10-06:

- `kinds`: check exit 0, eval exit 0, one INDETERMINATE for `ProbePointMass#constraint[1]`.
- `pose_rejected`: check exit 1, eval exit 1, printing
  `error: argument 'at' has type 'Frame3' but param 'at' requires type 'Point3<Scalar[m]>'`.

Tree-sitter ran on the pre-generated grammar of 2026-09-30, with a session-isolated cache, and found
0 ERROR nodes in both files. The PRD's §3 probes ran on a binary built 2026-09-30; the decompose
re-probe (seat A) confirmed every row on the `e5f638d029` binary.

**PDIAG on task/7189 (F1).** Measured with the real `reify-audit --pattern PDIAG`, run on a
`git archive` export of the branch:

| file | count on main | count on 7189 | baseline row |
|---|---|---|---|
| `elastic_static.rs` | 12 | 15 | 12 |
| `modal_ops.rs` | 25 | 26 | 25 |
| `point_support.rs` (new file) | — | 1 | none |

`reify-audit` exits 3 on 7189 and 0 on main, so the ratchet reds 7189 as it stands. 7189 also has a
content conflict with main in `elastic_static.rs` and must rebase.

**D3 workflow.** Run `wf_61a00e7b-afe`: 11 leaves, 38 agents, 2.54M subagent tokens, `REIFY_BIN`
pinned to the `e5f638d029` binary. Disposition recorded honestly: **BLOCKS** on harness reach and
pre-state, not on a standing falsification.

- Leaves probed: 9 of 11. η and ω were UNENUMERATED, which is expected because they have no probe
  vector.
- Malformed records: 0. Fixture-absent records: 18, because every planned fixture is a leaf
  deliverable.
- Pre-state:
  - `PointSupport` exists only on 7189.
  - `PointForce`/`PointMass` are unresolved-function warnings on main.
  - The body overload returns a hollow or empty result (#6660).
  - `printer.ri` segfaults (#7383).
- Prover mis-bindings:
  - `stderr_contains` on DiagnosticCode names, which the CLI never prints.
  - `ir`/absent exit-0 bindings, which cannot see whether a Warning is present.
  - exit-1 bindings that pass for the wrong reason before α.

Four refinements were adopted into the leaf texts and the PRD correction list:

- BT1: a `Real` radius is a compile-time `ArgTypeMismatch`. A dimensionless force and a negative
  radius are rejected at the solve boundary.
- BT13 is a Rust harness test.
- ι asserts finite, non-zero reduced cells, not just exit 0.
- Every CLI signal asserts message text; codes are asserted by identity in the e2e tests.

**Rulings (Leo, 2026-10-06), every recommendation adopted.** Bindings marked `Q-<letter>` rest on them:

- Q-A (amends D13): #7189 mints and applies its five C9 codes during the rebase it needs; α extracts.
- Q-B: the F9 defect and the F8 dispatch gap are filed as standalone tasks #8248 and #8249.
- Q-C: γ → #7079 and `W_PARAM_NOT_APPLICABLE`; #7084, #7081, #7142, #7166 and #7088 amended (full-text).
- Q-D: the inert `.ri` `radius >= 0mm` constraint is dropped from all three kinds (and 7189's
  PointSupport); the Rust boundary enforces.
- Q-E: NO edges on #7383 or #5312 for λ (the printer.ri file lock serialises λ and #5312; the crash is
  unrelated); λ's durable signal is two committed extracted fixtures with an e2e. No edge on #8102.
- Q-F: ε mints `ModalRigidBodyMode` / `ModalNoModesComputed` for the two pre-existing uncoded modal
  diagnostics BT5/BT6 assert on.

A fable critic pass (seat G) found four blockers, all folded: BT4's single point per end was ill-posed
in modal (torsion rigid mode); the FEA chunk is contested with #7088; the structured Info payload needs
a new `FeaDiagnosticDetail` variant (ζ mints `PointTargetResolved`); ι's test must be a reify-cli
subprocess test because gmsh is a dev-dep of reify-eval only.

**Reported out of the PRD, not bound here (Q-B — filed as #8248 and #8249).**

- F9, a real defect by code path: adaptive refinement on a realized mesh with `bc_override` falls
  back to the uniform lane, and `CantileverAdaptiveProblem::solve_and_estimate` applies
  realized-mesh node indices to the synthetic box. The fallback's own Warning claims the opposite.
  `adaptive_branch_falls_back_to_uniform_refinement_when_the_gmsh_lane_is_unavailable` case (a)
  exercises it and asserts only `Completed`.
- F8, a filing gap: per-instance `@optimized` dispatch under constructor overrides is unowned. The
  cell is body-inlined to its sentinel, undef downstream, with exit 0, and the decline message cites
  #6592, which does not own dispatch.

Binding vocabulary:

- **producer-self**: the leaf itself delivers the capability and its own signal observes it.
- **producer-upstream**: a hard `add_dependency` edge to a named task delivers it. Every such edge
  is wired at filing. The outside edges are #7189 (α), #7075 (η), #6660 and #7417 (ι), #7079 (γ, Q-C)
  and #7453–#7458 (M3). λ has none (Q-E).
- **substrate**: present on main today (or on 7189, where named), cited by symbol.

Verdicts:

- **OPEN** means the binding is a decision the leaf itself makes (ζ's DOF ceiling, η's tolerance).
  It is not a green G3 binding and does not block queueing.

Delivered checks:

- Every grep was polarity-checked against `main` at authoring: it fails today and goes green when
  the leaf lands.
- Every code check anchors on the enum-variant line (`^    <Variant>\b`) in
  `crates/reify-core/src/diagnostics.rs`. Docs and tests mention a code only as
  `DiagnosticCode::<Variant>` or in prose, so they cannot match.
- No check uses `expect: absent`. Where the brief suggested one, the
  manifest binds the positive construct (overlay rule 3) or `manual`.

## α (#8251) — extract the shared node-patch resolver and one strict Point3 reader; 7189's diagnostics coded

- `point-support-kind-upstream` — producer-upstream — hard add_dependency edge to #7189.
  PointSupport(at, radius), compute_targets/point_support.rs (extract_point_supports,
  point_support_nodes, check_on_body, validate, labelled_error, read_radius) and
  elastic_static.rs::CantileverBcs exist only on task/7189 676fde4c10 (seat C 1a-1g). D13 (Leo):
  7189 merges as accepted. F1: 7189 conflicts with main in elastic_static.rs
  (synthetic_cantilever_mesh vs #7448's cantilever_tip_load / realized_cantilever_bc_node_sets; the
  Dirichlet block) and must rebase on its own side. **PASS**.
- `node-patch-resolver` — producer-self (C2) — new compute_targets/node_patch.rs: PointTarget,
  OffBody, patch_nodes, check_on_body, max_tet_edge_length and nearest_node (7189 moved the last two
  from modal_ops into point_support.rs; modal_ops::end_face_neutral_axis_node imports nearest_node —
  seat C surprise 4). Rule unchanged from 7189's point_support_nodes: radius patch, boundary
  inclusive, else the single nearest node (seat C 1b). A test module cannot spell a pub(crate) fn
  definition, so rule 4 is safe. **PASS**. Check: grep `pub\(crate\) fn patch_nodes\b` present in
  `crates/reify-eval/src/compute_targets/node_patch.rs`.
- `off-body-check-kind-agnostic` — producer-self (C2) — check_on_body returns OffBody{at,
  nearest_distance, h_max}; the caller formats it into a diagnostic naming its own kind (7189's
  check_on_body returns a formatted String, seat C 1b). **PASS**. Check: grep `fn check_on_body\b`
  present in `crates/reify-eval/src/compute_targets/node_patch.rs`.
- `shared-strict-point3-reader` — producer-self (C2, F5; critic item 10) — ONE Point3<Length>
  reader: elastic_static.rs::extract_point3_si becomes the strict crate-visible triple parser (exact
  3 finite LENGTH components; the trailing-components test retired; its AABB callers keep working),
  and the kind / list-index / field / span LABELLING wrapper lives in node_patch.rs, which
  point_support.rs and modal_ops::point_support_bcs call; point_support.rs::read_length_point3 is
  deleted. Bound to the POSITIVE call site (overlay rule 3): the wrapper calls extract_point3_si.
  Accepted gap: a rename false-FAILs; the field-error e2e rows are the cover. **PASS**. Check: grep
  `extract_point3_si\(` present in `crates/reify-eval/src/compute_targets/node_patch.rs`.
- `reader-keeps-7189-strictness` — producer-self (F5, seat C surprise 3) — extract_point3_si today
  accepts >=3 components (pinned by extract_point3_si_ignores_trailing_components_past_three),
  checks no finiteness and errors with FeaValueShapeError naming no field, index or span;
  read_length_point3 is exact-3, finite, and names 'PointSupport #i: at'. α adopts 7189's rules and
  retires the trailing-components test. **PASS**. Check: manual — behavioural: α's unit tests plus
  point_support_e2e rows for a 2-component at, a non-finite coordinate and a negative radius, each
  naming 'PointSupport #i: <field>'.
- `radius-nonnegative-at-rust-boundary` — G6 branch 4, producer-self (Q-D) — the .ri 'constraint
  radius >= 0mm' is inert (F2: a template constraint is evaluated against the template default only;
  radius -1mm/-10mm -> OK, exit 0; seat A rows 4/5c/5d; #5765 pending, #6609 deferred). 7189's
  point_support.rs::read_radius already rejects radius < 0; α moves it into the shared reader and
  drops the .ri constraint from PointSupport. **PASS**. Check: manual — observed by α's
  negative-radius e2e row (coded FeaPointTargetFieldInvalid); removing the inert .ri line is not
  grep-checkable without matching documentation prose.
- `code-FeaPointOffBody` — producer-self (C9; Q-A: under recommendation (1) #7189 mints it during
  its rebase — present either way) — Error; message names the kind, at, the distance and h_max.
  Replaces the code-less point_support.rs::labelled_error site through which off-body Errors flow
  (F1). **PASS**. Check: grep `^    FeaPointOffBody\b` present in
  `crates/reify-core/src/diagnostics.rs`.
- `code-FeaPointSupportsUnderRestrained` — producer-self (C9; Q-A) — Error naming the free-mode
  count; 7189's collinear rule via unrestrained_rigid_body_modes (ε generalises it to the rank rule
  over directions). **PASS**. Check: grep `^    FeaPointSupportsUnderRestrained\b` present in
  `crates/reify-core/src/diagnostics.rs`.
- `code-FeaPointTargetRequiresTet` — producer-self (C9; Q-A) — Error; route_with_point_supports with
  ShellForce::On (7189's code-less Diagnostic::error site, F1; seat C surprise 6). Message cites M2
  as the shell-path owner. **PASS**. Check: grep `^    FeaPointTargetRequiresTet\b` present in
  `crates/reify-core/src/diagnostics.rs`.
- `code-FeaPointTargetShellFallback` — producer-self (C9; Q-A) — Warning; shell-classified body with
  ShellForce::Auto falls back to tets (7189's code-less Diagnostic::warning site in
  route_with_point_supports, F1). **PASS**. Check: grep `^    FeaPointTargetShellFallback\b` present
  in `crates/reify-core/src/diagnostics.rs`.
- `code-FeaAdaptiveDeclinedPointTarget` — producer-self (C9; Q-A) — Warning; 7189's any-radius
  adaptive decline (seat C 1d, code-less, F1). θ narrows it to radius = 0 (C7). **PASS**. Check:
  grep `^    FeaAdaptiveDeclinedPointTarget\b` present in `crates/reify-core/src/diagnostics.rs`.
- `code-FeaPointTargetFieldInvalid` — producer-self (seat D HIT row 4, PRD correction list; NOT in
  PRD C9 as authored) — Error for the reader's missing / non-finite / negative / wrong-arity field,
  naming kind, list index and field. A dimension mismatch keeps the existing DimensionedArgRejected
  (substrate, present on main). Under Q-A (1) this is the one code α mints itself. **PASS**. Check:
  grep `^    FeaPointTargetFieldInvalid\b` present in `crates/reify-core/src/diagnostics.rs`.
- `pdiag-no-codeless-sites` — G7 diagnostics-carry-codes (F1) — measured with the real reify-audit
  --pattern PDIAG on a git archive of task/7189: elastic_static.rs 12->15, modal_ops.rs 25->26,
  point_support.rs 1 (new file, no row); reify-audit rc=3 on 7189 vs 0 on main. After α:
  crates/reify-audit/pdiag-baseline.txt rows 12 and 25 not exceeded; point_support.rs and
  node_patch.rs have no row and no code-less site. **PASS**. Check: manual — the mechanical gate
  already exists: tests/infra/test_reify_audit_pdiag.sh (scenario a) runs in every merge gate; a
  grep cannot count coded vs code-less sites.
- `off-body-and-collinear-rejection` — G6 branch 4 — 7189's point_support_off_body.ri /
  point_support_collinear.ri exit 1 through reify eval (F3: reify check never dispatches FEA
  trampolines). D3: on main the off-body fixture already exits 1 for the WRONG reason (PointSupport
  unresolved: does not conform to trait 'Support'), so the signal asserts MESSAGE text (kind, at,
  distance, h_max / the free-mode count) and the e2e asserts the code by identity — never exit 1
  alone (F3: the CLI prints messages, not code names). **PASS**. Check: manual — behavioural:
  point_support_e2e.rs code-identity assertions; an exit code alone cannot express its own failure
  here (D3 finding).
- `every-c9-error-fails-the-solve` — G7 result-fields-populated-or-owned ([CODES]) — every C9 Error
  returns ComputeOutcome::Failed (cell Undef, UndefCause::SolveFailed), never an Error inside
  Completed beside a plausible number. **PASS**. Check: manual — outcome-shape property, asserted by
  the e2e reading the cell as Undef.

## ζ (#8252) — plate-capable synthetic grid; grid counts in ModalCacheKey

- `resolver-and-codes-upstream` — producer-upstream — intra-batch hard edge to α (node_patch
  resolver, strict reader, C9 codes), transitively #7189. **PASS**.
- `one-synthetic-grid-function` — producer-self (C8, D10) — synthetic_grid(dims,
  has_coordinate_kind, element_order) -> (nx, ny, nz) replaces
  elastic_static.rs::synthetic_grid_counts (ny = 1, NX_MAX clamp) and modal_ops::build_beam_mesh's
  private rule (ny = 1, no clamp) — seat C 9, F6. Absent on main. **PASS**. Check: grep
  `fn synthetic_grid\(` present in `crates/reify-eval/src`.
- `modal-builder-uses-the-one-function` — producer-self, wired-on-main — modal_ops::build_beam_mesh
  calls synthetic_grid (no second copy). Accepted gap: a definition placed in modal_ops.rs also
  satisfies the pattern; BT10/BT12 are the behavioural cover. **PASS**. Check: grep
  `synthetic_grid\(` present in `crates/reify-eval/src/modal_ops.rs`.
- `grid-counts-in-modal-cache-key` — producer-self (BT12) over substrate —
  reify-stdlib/src/modal/trampoline.rs::ModalCacheKey = {length, width, height, youngs_modulus,
  poisson_ratio, density, element_order}, compared by f64::to_bits in matches; no grid and no BC
  field (seat C 10, F6). ζ adds the grid counts and keeps the key BC-independent. **PASS**. Check:
  manual — the field name is ζ's choice; the warm-engine BT12 test (toggle a PointSupport at the
  same dims -> cache miss, frequencies equal the cold solve) is the observation.
- `code-FeaSyntheticGridCapped` — producer-self (C8, C9) — Warning when the DOF ceiling binds and
  the in-plane size is coarsened equally in x and y. **PASS**. Check: grep
  `^    FeaSyntheticGridCapped\b` present in `crates/reify-core/src/diagnostics.rs`.
- `dof-ceiling-decided-here` — a binding the leaf must MAKE (PRD §10: 'Decide in ζ from a measured
  solve time') — the constant and its measurement are recorded in the task result and a code comment
  citing C8. **OPEN**. Check: manual — a ruling recorded by ζ; no measurement exists before it runs.
- `coded-info-with-structured-payload` — substrate + producer-self (seat D HIT row 7, heuristic 12;
  critic blocker 3) — the per-support resolved coordinate and patch node count travel as a CODED
  Info with a structured payload, never parsed from text.
  reify-solver-elastic/src/diagnostics.rs::FeaDiagnosticDetail exists on main with exactly
  Unconstrained / ProblemElements / UnresolvedSelector, so ζ MINTS the variant
  `PointTargetResolved { kind, index, node_count, centroid }` there and surfaces it through
  ComputeOutcome structured_detail; the e2e reads it on the FIRST (cold) eval (precedent
  `crates/reify-eval/tests/harness_fea_solver_e2e/fea_structured_detail_e2e.rs` — the warm
  in-process cache re-emits no structured_detail). θ reuses the variant. PDIAG does not anchor on
  Diagnostic::info. **PASS**. Check: grep `PointTargetResolved\b` present in
  `crates/reify-solver-elastic/src/diagnostics.rs`.
- `legacy-scenes-byte-identical` — BT10, D10 — every scene without a coordinate kind keeps exactly
  today's counts per caller, including synthetic_grid_counts' NX_MAX clamp and the uniform lane's
  (2nx, 2ny, 2nz) doubling (F6). **PASS**. Check: manual — byte-identity over the existing fixture
  set is a behavioural comparison, not a grep (brief: never grep BT10).
- `p2-modal-substrate` — substrate — modal_ops::assemble_modal_km has a ModalMesh::P2 arm
  (consistent_element_mass_tet_p2); modal honours element order, static does not (seat C 8).
  **PASS**.
- `plate-modal-first-frequency-finite` — G6 exact by construction — three non-collinear, fully
  restrained (3-direction) interior patches restrain all six rigid-body modes, so the first
  frequency is finite; each resolved node lies within one element of the requested y (patch_nodes:
  radius ball else nearest node, on-body bounded by h_max) once ny > 1. Supports at mid-thickness
  (PRD §10). **PASS**. Check: manual — observed by the e2e's finite-frequency read and the
  per-support structured payload.
- `deterministic-above-parallel-threshold` — substrate —
  reify-solver-elastic/src/solver.rs::PARALLEL_DOF_THRESHOLD (10_000): above it results are
  bit-stable only for a fixed thread count; gated fixtures set deterministic: true (C8). **PASS**.
- `rebaseline-7189-fixtures` — producer-self — 7189's fixtures
  (crates/reify-eval-fea-tests/tests/fixtures/point_support_beam_static.ri,
  point_support_beam_modes.ri, point_support_off_body.ri, point_support_collinear.ri) change grid
  because they carry a coordinate kind; old -> new values recorded in the test with the reason.
  **PASS**. Check: manual — value re-baseline inside the test diff.

## β (#8253) — SolveBoundary, nodal-volume patch weights, PointForce (vertical slice)

- `grid-resolver-codes-upstream` — producer-upstream — intra-batch hard edge to ζ (-> α -> #7189):
  coordinate-aware grid, node_patch resolver, strict reader, FeaPointOffBody /
  FeaPointTargetFieldInvalid / shell-policy codes. **PASS**.
- `pointforce-shape-grammar` — grammar-fixture —
  tests/prd-gate/fixtures/coordinate_target_fea_kinds.ri (mirror ProbePointForce{at :
  Point3<Length>; force : Vector3<Force>; radius : Length = 0mm}): tree-sitter parse --quiet 0 ERROR
  nodes (2026-09-30 generated grammar); reify check exit 0, reify eval exit 0 with every field
  populated (e5f638d029; seat A 1a/1c/1d; re-run on the committed copy 2026-10-06, same result). No
  novel syntax. **PASS**.
- `pointforce-stdlib-kind` — producer-self (C1, Q-D) — structure def PointForce : Load in
  fea_multi_case.ri, without the inert 'constraint radius >= 0mm'. Today PointForce(...) is
  'warning: unresolved function', exit 0, value undef (seat A 3a/3b), so 'resolves' is asserted by
  the absence of that warning, not by exit 0 (D3). **PASS**. Check: grep
  `structure def PointForce\b` present in `crates/reify-compiler/stdlib/fea_multi_case.ri`.
- `solve-boundary` — producer-self (C4, D8, F5) — 7189's CantileverBcs{node_override,
  apply_face_clamp, point_supports} renamed and extended with point_forces, point_masses (filled by
  γ) and gravity; no second struct. #5313 builds its selector variant on it (D8; #5313 gains an edge
  on β). Pattern admits an enum in case #5313's variant reshapes it. **PASS**. Check: grep
  `(struct|enum) SolveBoundary\b` present in `crates/reify-eval/src`.
- `patch-weights-nodal-volume-share` — producer-self (C3, D9) — weights sum to 1; a node's volume is
  the sum of element volume / node count (4 or 10). Does NOT call
  boundary/patch_load.rs::apply_patch_resultant (#7448's tip-traction rule, substrate on main since
  e5f638d029 — seat C 17). **PASS**. Check: grep `fn patch_weights\b` present in
  `crates/reify-eval/src`, `crates/reify-solver-elastic/src`.
- `legacy-pointload-path-and-noloads` — substrate + producer-self — I5: extract_loads ->
  cantilever_tip_load -> apply_patch_resultant stays for PointLoad (seat C 2, 17). I6: FeaNoLoads is
  emitted only when no_tip_force && pressures.is_empty() && no_body_force (seat C 18), which a
  PointForce-only list would trip spuriously (seat C surprise 8); β feeds point forces into the
  predicate. #5802's TractionLoad/BodyForce targets stay intact (F7). **PASS**. Check: manual — I5
  is a preserved path — a grep on a legitimately refactorable fn would false-FAIL γ/ε; I6 is
  observed by a PointForce-only fixture emitting no FeaNoLoads.
- `buckling-refuses-pointforce` — G6 branch 4, producer-self (BT11, C6, F7, Q-C) — today
  buckling.rs::extract_total_load silently turns a Vector3<Force> list into its 1.0 N sentinel (seat
  C 4) and FeaLoadKindUnsupported has no production emitter (seat C 5: definition +
  compute_persist.rs test stub only). β adds a type_name guard in #5802's shape returning
  ComputeOutcome::Failed; the message cites the honour owner #7081 (Q-C (3)). #5802 may land the arm
  first — either producer delivers the refusal in this file. Pattern covers both
  DiagnosticCode::FeaLoadKindUnsupported and an FeaFailure::LoadKindUnsupported mapping. **PASS**.
  Check: grep `LoadKindUnsupported` present in `crates/reify-eval/src/compute_targets/buckling.rs`.
- `bt11-observed-through-eval` — F3 / seat D HIT row 11 — reify check never dispatches FEA
  trampolines, so BT11's non-zero exit is asserted through reify eval, together with the buckling
  cell Undef; the CLI prints "unsupported FEA load kind 'PointForce'", never the code name (D3).
  **PASS**. Check: manual — behavioural e2e: code by identity, cell Undef, eval exit non-zero.
- `bt1-solve-boundary-rejections` — G6 branch 4, producer-self (BT1, F4) — today a dimensionless
  force (vec(0, 0, -1) stored where Vector3<Force> is required), a negative radius (-0.001 m stored)
  and an omitted required field are ACCEPTED by compile, check and eval, exit 0 (seat A 5a-5d; #7874
  deferred). β's solve boundary rejects each naming the list index and field
  (FeaPointTargetFieldInvalid from α / the existing DimensionedArgRejected), observable only through
  a fixture that calls solve_elastic_static. **PASS**. Check: manual — behavioural e2e rows; the
  codes are minted upstream (α) or already exist.
- `bt1-real-radius-compile-rejection` — rejection observed (BT1, D3 refinement (c)) — a Real radius
  at a Length param is a compile-time ArgTypeMismatch, exit 1, before any solve (F4). Substrate; β
  pins it with a fixture. **PASS**.
- `bt14-pose-at-at-rejected` — rejection observed (BT14; P4 D3 unchanged) —
  tests/prd-gate/fixtures/coordinate_target_fea_pose_rejected.ri (ProbePointForce(at: frame3(...))):
  reify check and reify eval both exit 1 with "error: argument 'at' has type 'Frame3' but param 'at'
  requires type 'Point3<Scalar[m]>'" (e5f638d029; seat A 2a-2c; re-run on the committed copy
  2026-10-06). tree-sitter 0 ERROR nodes: the rejection is type-level, not a parse failure.
  **PASS**.
- `bt3-position-ratio-floor` — floor: 10% > ~5.5% error budget (PRD §7 G6 note) — shear deformation
  < 0.5% at L/h = 20; the resultant's offset from at is <= radius + h ~ 13 mm on a = 500 mm, moving
  a^2(3L-a) by < 5% (~4.7% at a = L/2, ~2% at a = L); P1-tet stiffening (the overlay's bending-lock
  hazard) is a near-common factor because the element shape is uniform along the beam. Expected
  ratio f(L/2)/f(L) = 0.3125. Holds only for radius > 0 (the ~13 mm figure assumes radius ~10 mm; a
  radius = 0 force at a2 = L adds a mesh-dependent point-singular indentation at the tip that this
  budget omits) and for the dims-overload box (a tube section needs the body overload, ι). **PASS**.
- `bt13-weights-converge-harness` — exact by construction, producer-self (BT13, D3 refinement (c)) —
  sum w_i = 1 and resultant = force * sum w_i to rounding; the weighted centroid lies within radius
  + h of at (patch nodes within radius, else the nearest node within h_max). A Rust harness test in
  crates/reify-eval-fea-tests: ElasticResult exposes no centroid or resultant field. **PASS**.
  Check: manual — Rust harness assertion over two successive refinements.
- `multi-case-pass-through` — substrate — multi_case.rs::solve_multi_case_trampoline calls the
  static trampoline per case and propagates diagnostics (seat D advisory); β adds one assertion that
  a PointForce in a LoadCase equals the direct solve_elastic_static result. **PASS**. Check: manual
  — equality assertion in β's e2e.
- `shell-route-policy` — producer-upstream (α over 7189) — route_with_point_supports: ShellForce::On
  -> FeaPointTargetRequiresTet, Auto -> tets with FeaPointTargetShellFallback (seat C surprise 6); β
  routes PointForce the same way. **PASS**.
- `radius-zero-singularity-info` — producer-self (C9 note) — a single-shot solve with a radius = 0
  kind emits a coded Info that max_von_mises includes a mesh-dependent point singularity. **PASS**.
  Check: manual — Info code name is β's choice; asserted by identity in the e2e.
- `pdrop-declaration-handoff` — G7 declared-param-reaches-kernel (seat D HIT row 2) — PDROP is
  chartered, not shipped (#7079 mechanism, #7085 gate, pending): β writes the PointForce{at, force,
  radius} rows in-diff if #7079 is on main (honored on elastic_static and multi_case; refused on
  buckling/buckling_multi_case), else appends them to #7085 (buckling rows to #7081). ω verifies the
  hand-off. **PASS**. Check: manual — task-record / declaration hand-off, verified by ω.
- `user-facing-example` — producer-self — examples/fea/gantry_head_force.ri, compile-swept by
  crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs (recursive walk); no
  assertion. examples/fea/ does not exist on main. **PASS**. Check: path present
  `examples/fea/gantry_head_force.ri`.

## γ (#8255) — PointMass — static weight under Gravity and modal nodal inertia

- `solve-boundary-and-weights-upstream` — producer-upstream — intra-batch hard edge to β
  (SolveBoundary with point_masses and gravity declared, patch_weights, the buckling refusal arm).
  **PASS**.
- `pdrop-mechanism-upstream` — producer-upstream — hard add_dependency edge to #7079 (Q-C (1)):
  PDROP's declaration mechanism and W_PARAM_NOT_APPLICABLE. Neither it nor ParamNotApplicable is on
  main (seat D HIT row 1: only comments in compute_persist.rs). Replaces the PRD's
  FeaPointMassNotApplicable, withdrawn as a lock-step duplicate (PRD correction list). **PASS**.
- `pointmass-and-options-grammar` — grammar-fixture —
  tests/prd-gate/fixtures/coordinate_target_fea_kinds.ri: ProbePointMass{at, mass : Mass, radius}
  and ProbeOptions{point_masses : List<ProbePointMass> = []} — tree-sitter 0 ERROR nodes; check/eval
  exit 0; eval prints o_default point_masses: [] and o with one populated mass (seat A 1d).
  'constraint mass >= 0kg' on the required param is INDETERMINATE on every check (seat A 4), so mass
  >= 0 moves to the Rust boundary (C1). **PASS**.
- `pointmass-stdlib-kind` — producer-self (C1) — structure def PointMass : Load (fea_multi_case.ri
  per the draft; the pattern spans the stdlib dir in case module visibility puts it beside
  ModalOptions). Today 'warning: unresolved function: PointMass', exit 0, value undef (seat A
  3a/3b); D3: 'resolves' must be asserted by the absence of that warning. **PASS**. Check: grep
  `structure def PointMass\b` present in `crates/reify-compiler/stdlib`.
- `modal-options-point-masses-param` — producer-self (C1) — ModalOptions gains point_masses :
  List<PointMass> = []; ModalOptions has 8 params today and is shared with mechanism_modal_analysis
  (seat C 6). **PASS**. Check: grep `param point_masses\b` present in
  `crates/reify-compiler/stdlib/modal_analysis.ri`.
- `point-masses-reach-the-modal-kernel` — producer-self, wired-on-main (anti-orphan) — the modal
  trampoline / eigensolve reads ModalOptions.point_masses; no point_mass/nodal_mass hook exists in
  modal_ops.rs today (seat C 6). Accepted gap: a test-only mention in these files false-PASSes; BT4
  is the behavioural cover. **PASS**. Check: grep `point_masses` present in
  `crates/reify-eval/src/modal_ops.rs`, `crates/reify-stdlib/src/modal`.
- `cached-mass-matrix-not-mutated` — substrate hazard (F6, seat C 13) — eigensolve_modal projects a
  SHARED &assembly.m_full from the cached ModalAssembly; γ clones, augments, then project_free.
  mass_matrix_norm, participation_mass and mass-normalisation read the augmented matrix;
  solve_modal_core_participation_mass_satisfies_completeness is extended to sum m_eff = body mass +
  sum point masses (INV-PD-2). **PASS**. Check: manual — observed by the extended completeness test
  and BT12-style warm/cold equality.
- `code-FeaPointMassNoGravity` — producer-self (C6, C9) — Warning: a PointMass in a static
  List<Load> with no Gravity contributes nothing. **PASS**. Check: grep
  `^    FeaPointMassNoGravity\b` present in `crates/reify-core/src/diagnostics.rs`.
- `static-weight-needs-no-density` — producer-self (C6, D3) — weight m * g * w_i with g =
  SolveBoundary.gravity; no material density. The heterogeneous-material gravity Warning in
  elastic_static.rs is corrected and coded while touched (uncoded today; seat D advisory). I6: a
  point mass under gravity counts as a load. **PASS**. Check: manual — behavioural: BT2 and a
  heterogeneous-material fixture.
- `bt2-force-mass-agree-floor` — floor: CG tolerance >> ~1e-15 relative RHS rounding — same patch,
  same weights, so the two load vectors differ only by rounding and the displacement fields agree to
  the CG tolerance (the e2e compares at a relative tolerance no tighter than the solver's CG rtol).
  Condition: the PointForce literal uses Gravity()'s own g (9.81 vs 9.80665 is 3.5e-4, far above any
  CG tolerance). **PASS**.
- `bt4-ordering-exact-in-direction` — G6 exact in direction (PRD §7) — positive added mass cannot
  raise any eigenvalue (Rayleigh quotient); a mass nearer the first-mode antinode lowers f1 more;
  mass on restrained support DOFs gives <=. Conditions the fixture must meet: (a) TWO fully
  restrained supports PER END across the width (7189's point_support_beam_modes.ri shape; critic
  blocker 1) with patch radius >= one element edge — a single point per end on the axis leaves the
  rotation about that axis free, making f1 a rigid mode (C5: modal adds no guard), so the fixture
  asserts no rigid-body Warning; (b) the head mass is large enough that each strict gap exceeds the
  eigensolver tolerance. **PASS**.
- `mechanism-modal-not-applicable` — producer-self via #7079 (Q-C (1)/(2)) —
  ModalOptions.point_masses declared not_applicable on modal::mechanism_modal (the lumped model
  carries mass per body via point_mass / mass_properties) and W_PARAM_NOT_APPLICABLE emitted; #7084
  amended from 8 to 9 params. **PASS**. Check: manual — declaration + upstream mechanism; observed
  by γ's mechanism_modal e2e row.
- `buckling-refuses-pointmass` — producer-upstream β + producer-self — γ extends β's buckling
  type_name arm to PointMass (FeaLoadKindUnsupported, owner #7081 per Q-C (3)); solve_load_cases
  passes it through. **PASS**. Check: manual — extends an arm whose presence is β's check; observed
  by γ's e2e.
- `bt14-pose-at-pointmass-rejected` — rejection observed —
  tests/prd-gate/fixtures/coordinate_target_fea_pose_rejected.ri: the typed at : Point3<Length>
  rejects frame3(...) with exit 1 (the mechanism is the param type, independent of the kind). D3: on
  main a pose at PointMass.at is accepted with only an unresolved-function warning because PointMass
  is undefined — γ's kind closes that. **PASS**.
- `bt1-negative-or-missing-mass` — G6 branch 4, producer-self — the .ri mass constraint is
  INDETERMINATE (seat A 4) and an omitted ctor param compiles silently (F4, #7874); the Rust
  boundary rejects a negative, non-finite or missing mass / at naming list index and field.
  **PASS**. Check: manual — behavioural e2e rows (codes from α).
- `pdrop-handoff-and-modal-overloads` — G7 declared-param-reaches-kernel (seat D HIT rows 2, 13; Q-C
  (2), (4)) — PointMass rows and ModalOptions.point_masses rows written in-diff or appended to #7085
  / #7084; #7142 (body-arg modal) and #7166 (graph overload) must honour or declare point_masses
  (amended at decompose). ω verifies. **PASS**. Check: manual — task-record / declaration hand-off,
  verified by ω.
- `distinct-from-point-mass-builtin` — substrate —
  reify-compiler/src/units.rs::DYNAMICS_CONSTRUCTOR_NAMES = [mass_properties, point_mass] (seat C
  surprise 10); different case-spelling, no hard clash; no rename (PRD §10); κ's chunk says so.
  **PASS**.

## ε (#8256) — directional restraint — per-node basis change, static and modal

- `solve-boundary-upstream` — producer-upstream — intra-batch hard edge to β (SolveBoundary, patch
  resolution), transitively α's FeaPointSupportsUnderRestrained. ε does NOT depend on γ: whichever
  of the two lands first introduces the modal matrix clone (see modal-path-rotates). **PASS**.
- `restrain-shape-grammar` — grammar-fixture —
  tests/prd-gate/fixtures/coordinate_target_fea_kinds.ri: restrain : List<Vector3<Dimensionless>>
  with the three-axis list default and an oblique populated list [vec3(0.0, 0.0, 1.0), vec3(0.6,
  0.8, 0.0)] — 0 ERROR nodes; eval prints both populated (seat A 1a/1d). **PASS**.
- `restrain-stdlib-param` — producer-self (C1, D4) — PointSupport gains param restrain
  (Dimensionless: only the direction is consumed, matching PointLoad.direction / Gravity.direction /
  ModalOptions.reference_direction). **PASS**. Check: grep `param restrain\b` present in
  `crates/reify-compiler/stdlib/fea_multi_case.ri`.
- `no-directional-constraint-today` — substrate absence (seat C 12a-12d) —
  boundary/dirichlet.rs::apply_dirichlet_row_elimination takes global DOFs only;
  mpc.rs::apply_mpc_row_elimination has no production caller and leaves K unsymmetric (and panics
  unless every redistribution target is stored); buckling_kernel.rs::build_expansion_map is private
  and forbids chained pivots (the PRD's 'sharing a node' wording is imprecise, conclusion holds);
  RollerSupport is a Value::Map nothing reads. ε is the producer. **PASS**.
- `full-nodal-blocks-keep-sparsity` — substrate — assembly/global.rs::emit_element_triplets emits
  every 3x3 nodal entry; modal_ops::assemble_global_matrix uses the same path (seat C 11), so a
  per-node basis change preserves the sparsity pattern (D5). **PASS**.
- `nodal-basis-module` — producer-self (C5, D5) — pub struct NodalBasis{node, q} (orthonormal,
  restrained directions first) with rotate_system / rotate_matrix / rotate_vector / back_rotate in a
  new reify-solver-elastic module. **PASS**. Check: grep `pub struct NodalBasis\b` present in
  `crates/reify-solver-elastic/src/boundary/nodal_basis.rs`.
- `displacements-leave-global` — producer-self (C5) — back_rotate before stress recovery, nodal
  recovery, the returned displacement and the stored warm state. **PASS**. Check: grep
  `pub fn back_rotate\b` present in `crates/reify-solver-elastic/src/boundary/nodal_basis.rs`.
- `modal-path-rotates` — producer-self, wired-on-main — rotate_matrix on K and on M (γ's augmented
  copy when γ has landed) before project_free (modal_ops.rs), mode shapes back-rotated,
  participation vector Q^T d. Cache hazard (F6, seat C 13): ModalAssembly.k_full and .m_full are
  SHARED cached references that eigensolve_modal projects with project_free(&assembly.k_full /
  &assembly.m_full); rotate_matrix takes &mut, so ε rotates clones, never the cached assembly.
  Absent today. **PASS**. Check: grep `rotate_matrix\(` present in
  `crates/reify-eval/src/modal_ops.rs`.
- `static-path-rotates` — producer-self, wired-on-main — rotate_system after assembly, first k local
  DOFs fixed per patch node, warm start forward-rotated. Where the call lands (solve_cantilever_fea
  or the solver crate) is ε's choice. **PASS**. Check: manual — call site not fixed by the contract;
  BT5's static half (zero along restrained directions) cannot pass unwired.
- `code-FeaRestraintDirectionsInvalid` — producer-self (C5, C9) — Error for a zero vector, a count
  outside 1-3, or rank below the count, naming the support's list index. **PASS**. Check: grep
  `^    FeaRestraintDirectionsInvalid\b` present in `crates/reify-core/src/diagnostics.rs`.
- `code-ModalRigidBodyMode` — producer-self (seat D HIT row 8; PRD correction list) — codes the
  pre-existing message-prefix Warning at modal_ops.rs::rigid_body_mode_diagnostic (code: None
  today); modal_ops baseline falls. **PASS**. Check: grep `^    ModalRigidBodyMode\b` present in
  `crates/reify-core/src/diagnostics.rs`.
- `code-ModalNoModesComputed` — producer-self (seat D HIT row 8) — codes the pre-existing
  SingularKOverCeiling Error in modal_ops.rs::eigensolve_modal (K_free singular above
  DENSE_FALLBACK_MAX_DIM = 1024). **PASS**. Check: grep `^    ModalNoModesComputed\b` present in
  `crates/reify-core/src/diagnostics.rs`.
- `rank-guard-over-directions` — producer-self over upstream (F5) — 7189's validate(...,
  require_rigid_restraint) / unrestrained_rigid_body_modes assume global-axis DOFs; ε generalises to
  6 - rank(constraint rows in rigid-mode space), skipped under a face clamp. BT6 is exact: three
  z-only points leave x, y translation and z rotation free -> 3. **PASS**. Check: manual — integer
  rank, asserted by the BT6 e2e (FeaPointSupportsUnderRestrained naming 3).
- `bt5-exact-by-construction` — G6 exact by construction (PRD §7) — the restrained local DOF is
  eliminated, so n . u = 0 to rounding (the e2e compares against a machine-ε multiple of max|u|, not
  a literal 0, since back_rotate adds rounding) and the free direction is non-zero. A rank-6 cone /
  vee / flat mount needs the vee's in-plane direction non-parallel to the cone-to-vee line; a wrong
  fixture is self-diagnosing (the static guard fires). **PASS**.
- `bt6-modal-outcome-pinned` — substrate — modal_ops.rs::DENSE_FALLBACK_MAX_DIM = 1024; C5 adds no
  modal guard. The flat-only fixture states its free-DOF count against 1024 so ONE outcome
  (ModalRigidBodyMode Warning below, ModalNoModesComputed Error above) is asserted, never a
  disjunction (seat D HIT row 8). **PASS**. Check: manual — fixture-level pin asserted by code
  identity.
- `pdrop-handoff-and-buckling-owner` — G7 declared-param-reaches-kernel (Q-C (3), (4)) —
  PointSupport.restrain honored on elastic_static, multi_case, free_vibration; supports dropped on
  buckling with owner #7081 (amended to cover PointSupport with restrain in its modal
  build_dirichlet_bcs port); #7142 / #7166 amended. ω verifies. **PASS**. Check: manual —
  task-record / declaration hand-off, verified by ω.

## θ (#8257) — adaptive refinement with coordinate kinds (both lanes carry SolveBoundary)

- `directional-restraint-upstream` — producer-upstream — intra-batch hard edge to ε (-> β -> ζ ->
  α): SolveBoundary, the rank guard, patch weights, FeaAdaptiveDeclinedPointTarget (minted by α).
  **PASS**.
- `adaptive-problems-carry-solve-boundary` — producer-self (C4 I2, D6) over substrate —
  elastic_static.rs::CantileverAdaptiveProblem and RealizedAdaptiveProblem rebuild the mesh and
  re-derive BCs by a hard-coded root-face rule, which is why 7189 declines adaptivity at any radius
  (seat C 1d). Both hold the SolveBoundary and re-resolve on each solve_and_estimate. **PASS**.
  Check: manual — field placement is θ's; BT7 (loop runs, no decline) and BT9 (localized lane does
  not fall back) are the observation.
- `off-body-against-seed-h-max` — producer-self (I4) — the off-body check uses the SEED mesh's h_max
  for the whole run; the rank guard runs on each solved mesh. **PASS**. Check: manual — behavioural:
  a point that snaps on the seed stays on-body through refinement (θ's test).
- `decline-narrowed-to-radius-zero` — producer-self over α's code (C7, D7) —
  FeaAdaptiveDeclinedPointTarget fires only when some coordinate kind has radius = 0 and adaptive:
  true; single-shot result with the non-adaptive defaults; the message cites M3's task id as the
  lifting owner. **PASS**. Check: manual — BT8 asserts the code by identity; BT7 asserts its absence
  for radius > 0 by identity — no absent grep on the message (a test literal would wedge it, overlay
  rule 4).
- `bt7-node-counts-non-decreasing` — G6 exact by construction — on the dims overload the uniform
  lane doubles (nx, ny, nz) each iteration (F6), so every grid nests the previous one's nodes and a
  radius ball's node set can only grow. Holds for nested refinement only; the localized remesh lane
  does not nest, so BT7 is asserted on the dims overload. **PASS**.
- `coded-per-iteration-info` — producer-self (C7, seat D HIT row 7) — a coded Info per iteration
  with the per-patch node count as a structured payload, reusing ζ's
  `FeaDiagnosticDetail::PointTargetResolved`; the e2e reads it on the first (cold) eval; spelling
  decided here (PRD §10). **PASS**. Check: manual — code / payload names are θ's; the e2e
  asserts code + payload, never the text.
- `bt9-gmsh-harness-reachable` — substrate — engine_admin.rs::Engine::ensure_gmsh_kernel exists with
  test-only callers (seat C 16), enough for a Rust harness test on the body overload in
  crates/reify-eval/tests (gmsh is a DEV-dep of reify-eval only — critic blocker 4; precedent
  `solve_elastic_static_body_e2e.rs`); the CLI path is ι's. **PASS**.
- `weights-recomputed-per-mesh` — producer-upstream β (patch_weights) — recomputed on each mesh;
  BT13's property holds per iteration; BT9 asserts sum w_i = 1 on an unstructured mesh. **PASS**.
  Check: manual — behavioural: BT9 harness.

## ι (#8258) — coordinate kinds on a realized body from the CLI

- `adaptive-boundary-upstream` — producer-upstream — intra-batch hard edge to θ (localized lane
  carries SolveBoundary; BT9 harness). **PASS**.
- `body-overload-from-cli-upstream` — producer-upstream — hard add_dependency edges to #6660 and
  #7417 (D12, not re-filed): Engine::ensure_gmsh_kernel has no non-test caller; reify-cli and
  gui/src-tauri do not reference reify-kernel-gmsh (seat C 16). **PASS**.
- `populated-result-not-hollow` — field-population (sentinel Value::Undef; D3 refinement (c);
  INV-PD-2) — the fixture reduces max(result.displacement) and max(von_mises(result.stress)) and the
  signal asserts FINITE, NON-ZERO values; exit 0 alone is satisfied by a hollow Converged
  ElasticResult (#6660) or by the current ComputeOutcome::Failed { diagnostics: vec![] } body-path
  guard (seat C 16 nuance). **PASS**. Check: manual — value read through reify eval; a grep cannot
  see population.
- `off-body-on-realized-mesh` — G6 branch 4 — FeaPointOffBody (α) fires against the realized mesh's
  h_max; the off-body variant exits non-zero through reify eval printing the off-body message (F3).
  **PASS**. Check: manual — behavioural: message on stderr, code by identity in the e2e.
- `realized-fixture` — producer-self (critic blocker 4) — a body the synthetic box cannot express
  (L-bracket or plate with a hole), as a reify-cli SUBPROCESS e2e fixture
  (`crates/reify-cli/tests/fixtures` + `harness_cli`, the shape #6660 plans), because gmsh is
  reachable from the binary but is not a dependency of reify-eval-fea-tests. **PASS**. Check: path
  present `crates/reify-cli/tests/fixtures/bracket_point_targets.ri`.

## κ (#8259) — docs — FEA chunk, best-practice exemplar, design index, stdlib reference

- `kinds-delivered-upstream` — producer-upstream — intra-batch hard edges to γ, ε, ζ, θ; the
  documented kinds, params and diagnostics exist before the docs are written. **PASS**.
- `fea-chunk-file` — producer-self, CO-OWNED with #7088 (critic blocker 2; Q-C (5)) —
  crates/reify-mcp/src/tools/chunks/ holds 17 chunks today and none for FEA; the registry is
  hand-maintained (language_chunks.rs: include_str! const, TOPICS row, get_chunk arm, the hard-coded
  assert_eq! on available_topics().len(); plus reference_tools_tests.rs ALL_TOPICS). Whichever of κ
  and #7088 lands first creates fea.md with those five edits, the other extends. A crates/**/*.md is
  include_str!'d (lands as code). **PASS**. Check: path present
  `crates/reify-mcp/src/tools/chunks/fea.md`.
- `fea-chunk-documents-point-masses` — producer-self — the chunk documents the three kinds and
  ModalOptions.point_masses (a name the other chunks do not carry). **PASS**. Check: grep
  `point_masses` present in `crates/reify-mcp/src/tools/chunks/fea.md`.
- `chunk-fence-gate` — substrate — PDOCCOVER (crates/reify-audit/src/pdoccover.rs;
  tests/infra/test_reify_audit_pdoccover.sh) checks the chunk's fenced signatures against the
  stdlib. **PASS**. Check: manual — the existing PDOCCOVER gate is the mechanical check.
- `best-practice-exemplar` — producer-self — examples/best_practices/fea_point_targets.ri,
  compile-gated by examples_smoke.rs (examples/ walked recursively; SKIP_SET forbidden for
  best_practices). **PASS**. Check: path present `examples/best_practices/fea_point_targets.ri`.
- `index-row` — producer-self — a row in examples/best_practices/INDEX.md (table rows name the .ri
  file). **PASS**. Check: grep `fea_point_targets\.ri` present in
  `examples/best_practices/INDEX.md`.
- `reify-design-index-line` — producer-self — one index line in .claude/skills/reify-design/SKILL.md
  pointing at the corpus (not an inline playbook). **PASS**. Check: grep
  `fea_point_targets|chunks/fea\.md` present in `.claude/skills/reify-design/SKILL.md`.
- `stdlib-reference-subsection` — producer-self — docs/reify-stdlib-reference.md §15 std.fea exists
  today (stress-invariant result only) and names none of the three kinds; κ adds the subsection.
  **PASS**. Check: grep `PointForce` present in `docs/reify-stdlib-reference.md`.
- `notes-file-generalised` — producer-upstream #7189 — docs/notes/fea-point-supports.md exists only
  on task/7189; κ generalises it to the three kinds. **PASS**. Check: manual — prose generalisation;
  the file's existence is 7189's.
- `p4-d2-pointer` — producer-self —
  docs/prds/naming-convergence/P4-region-ref-fea-selector-unification.md D2 says coordinate loads
  are 'a named future follow-up' and names no PRD; κ points it at this PRD. **PASS**. Check: grep
  `coordinate-target-fea` present in
  `docs/prds/naming-convergence/P4-region-ref-fea-selector-unification.md`.
- `goal-phrased-discoverability` — Docs-truth gate arm 4 — an author searching 'support a plate at
  three points' or 'mass at a position on a beam' lands on the chunk or index line. **PASS**. Check:
  manual — a search judgement, not a grep.

## λ (#8260) — printer_v01 adopts the coordinate kinds (GantryFea head at head_x; EZBed modal on three supports)

- `kinds-upstream` — producer-upstream — intra-batch hard edges to γ (PointMass, point_masses) and ε
  (restrain). **PASS**.
- `no-edges-on-7383-or-5312` — Q-E (Leo, 2026-10-06) — NO add_dependency edge on #7383 (reify check
  prj/printer_v01/printer.ri exits 139 in an OCCT boolean fuse on e5f638d029 — seat B — unrelated to
  λ; the full-file check is noted, not gated, until it lands) and NO edge on #5312 (it rewrites
  GantryFea's String targets in the same block, but the shared prj/printer_v01/printer.ri file lock
  already serialises the two in either order, and #5312 is a long-stall risk gated on in-progress
  #8102). No edge on #8102 either (§3 sub-instance probe: #8102 mints geometry/selector values; a
  point3 from an overridden param is already correct). **PASS**. Check: manual — a dependency-graph
  ruling, verified by the absence of the edges at filing.
- `committed-extracted-fixtures` — producer-self (critic item 11) — the durable signal is two
  COMMITTED extracted fixtures with an e2e (each carries only the structure and its deps, asserting
  the new cells finite and non-Undef), not a one-off probe of the full file. **PASS**. Check: path
  present `crates/reify-eval-fea-tests/tests/fixtures/gantry_fea_head.ri`,
  `crates/reify-eval-fea-tests/tests/fixtures/ezbed_modes.ri`.
- `instance-scope-reuse-holds` — substrate (F8) — an @optimized FEA cell in a sub is reused from the
  template only when every input equals the template's (unfold/optimized_instance_reuse.rs); Printer
  instantiates GantryFea() with no args and EZBed with default-equal values, and λ adds no per-sub
  override reaching an FEA input. The per-instance dispatch gap itself is reported out of the PRD
  (Q-B (ii)). **PASS**. Check: manual — design rule enforced in λ's diff;
  idler_seat_e2e.rs::check_printer stays green.
- `eval-not-check-for-fea-cells` — F3 (seat D advisory) — reify check never dispatches FEA
  trampolines, so check clean is compile-level only; the FEA observation is reify eval of each
  extracted structure printing finite new cells. **PASS**.
- `printer-compile-and-kernel-free-eval-gates` — substrate — printer.ri is compile-gated by
  pin_cell_id_namespace_tests.rs and orientation_constructor_typing_tests.rs and evaluated
  kernel-free by crates/reify-eval/tests/harness_sweep/idler_seat_e2e.rs::check_printer (corrects
  PRD §1). **PASS**.
- `gantry-head-mass-adopted` — producer-self — GantryFea gains head_x (the identifier already occurs
  elsewhere in printer.ri, so the check binds the kind) and a PointMass in the static loads and
  ModalOptions.point_masses. No PointMass( occurs in printer.ri today. **PASS**. Check: grep
  `PointMass\(` present in `prj/printer_v01/printer.ri`.
- `ezbed-three-support-modal` — producer-self — EZBed modal cell on three PointSupports with a cone
  / vee / flat restrain pattern (three z-only points are under-restrained in modal, C5). No
  PointSupport( in printer.ri today. **PASS**. Check: grep `PointSupport\(` present in
  `prj/printer_v01/printer.ri`.

## η (#8265) — static plate on three points — probe first, stop if CG does not converge

_Dependency-gated milestone leaf: asserts nothing numerically until its probe runs (OPEN rows); D3 UNENUMERATED._

- `static-p2-upstream` — producer-upstream — hard add_dependency edge to #7075:
  ElasticOptions.element_order is never read by elastic_static.rs; static meshes are P1 only (seat C
  8). **PASS**.
- `plate-kinds-upstream` — producer-upstream — intra-batch hard edges to ζ (quadratic grid rule), ε
  (cone / vee / flat restrain), γ (weight under Gravity). **PASS**.
- `probe-first-asserts-nothing-numeric` — η asserts nothing numerically until its probe has run (PRD
  §7 G6 note, D11). §3: the 800x500x12 box on linear tets does not converge in 2000 Jacobi-CG
  iterations on main; static P2 and thin-plate CG convergence are unverifiable today. D3 run:
  UNENUMERATED (no probe vector), expected. **OPEN**. Check: manual — the probe's measurements
  (grid, DOFs, CG iterations, residual history, wall time) are the deliverable of η's first step.
- `sag-tolerance-from-probe` — G6 (overlay floors: P1/P2 bending, Dirichlet k~0.67-0.70 for
  pointwise pins) — the strip-vs-overhanging-beam tolerance is stated from the probe's measured
  discretisation error, never guessed. **OPEN**. Check: manual — bound set by η from measurement.
- `no-converge-branch-releases-m2` — seat D HIT row 14 resolved — if CG does not converge η
  escalates with the residual history; Leo's resolution (done with the recorded outcome, or
  cancelled) is what releases M2. **PASS**. Check: manual — an escalation / human ruling, not code.

## M1 (#8261) — prestressed modal analysis — β landed → escalate for a human /prd session

- `human-ruling-gate` — deterministic pure gate (born-at-L2 milestone_gate on dispatch; gate = β
  landed) — the deliverable is Leo's Expand / Re-defer / Decline ruling on prestressed modal
  analysis (C6, §8; Q-N). **PASS**. Check: manual — a human ruling; nothing to grep.
- `geometric-stiffness-substrate` — substrate for the future session, not bound to code here —
  reify-solver-elastic/src/prestress_stability.rs::assemble_geometric_stiffness (pub(crate)) exists;
  the PRD's 'geometric_stiffness' names it loosely. **PASS**.

## M2 (#8262) — coordinate kinds on the MITC3 shell path — η resolved → escalate for a human /prd session

- `human-ruling-gate` — deterministic pure gate (gate = η resolved: done-with-outcome or cancelled)
  — the deliverable is Leo's Expand / Re-defer / Decline ruling on coordinate kinds on the MITC3
  shell path (§8). **PASS**. Check: manual — a human ruling; nothing to grep.
- `shell-owner-cite-completable` — G7 nothing-vacuous-and-unowned (seat D advisory) —
  FeaPointTargetRequiresTet / FeaPointTargetShellFallback messages cite M2; ownership passes to the
  implementing leaf when the /prd session completes (precedent #7177 -> #7457). **PASS**. Check:
  manual — task-record ownership, not code.

## M3 (#8263) — lift D7's radius-0 adaptivity refusal under a far-field QoI — θ + DWR landed → escalate for a human /prd session

- `dwr-estimator-upstream` — producer-upstream — hard add_dependency edges to #7453, #7454, #7455,
  #7456, #7457, #7458 (goal-oriented-error-estimation.md) and intra-batch θ. **PASS**.
- `human-ruling-gate` — deterministic pure gate — Leo's Expand / Re-defer / Decline ruling on
  lifting D7's radius = 0 refusal under a far-field QoI; FeaAdaptiveDeclinedPointTarget's message
  cites this milestone. **PASS**. Check: manual — a human ruling; nothing to grep.

## ω (#8264) — PRD close — terminal stamp, AS-AUTHORED freeze header, matching manifest header

- `freeze-header-precedent` — substrate — docs/prds/v0_6/data-carrying-enums.md and
  docs/prds/kernel-seam-contracts.md headers (as landed in edd9703fae); the PPRDSTATUS detector
  (crates/reify-audit/src/pprdstatus.rs) reads the terminal Status token. **PASS**. Check: manual —
  the committed header is the deliverable; a status-token grep would fire on this manifest's own
  prose.
- `pdrop-handoffs-verified` — G7 (seat D HIT row 2, advisory ω row) — before stamping, ω verifies
  the PDROP declaration rows (in-diff or appended to #7085 / #7084 / #7081) and the #7142 / #7166 /
  #7081 amendments, and names any gap in the header rather than stamping over it. **PASS**. Check:
  manual — task-record inspection, not code.
