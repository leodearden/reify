# Capability manifest — support-realization-policy

PRD: `docs/prds/v0_6/support-realization-policy.md`. Its machine-readable twin is
`docs/prds/v0_6/support-realization-policy.capability-manifest.yaml`, stamped by `commit_planning`.

Filed 2026-10-19 (stamped by `commit_planning`): α₁ #9379, α₂ #9380, β #9381, γ #9382, δ #9383, ε #9384, Ω #9385.

Evidence was measured on main `eb0a855dad` (2026-10-18) with the debug `reify` built on 2026-10-16 at 14:45.
No realization anchor has changed since `04be9302ea` (PRD header).

- The four evidence fixtures are in `tests/prd-gate/fixtures/`. Each parses with `tree-sitter parse --quiet`
  (rc 0, using the session-isolated cache).
- The buckling calibration was measured on 2026-10-19 in an isolated worktree with a release build.
- Every grep delivered check below was measured to FAIL on main, and PASSes once its producer lands.

**Critic pass.** A fable critic seat reviewed the first draft on 2026-10-19. It raised three blockers, seven
majors and twelve minors, all folded into the PRD:

| finding | fix |
|---|---|
| cross-crate `inventory` (blocker) | the rows moved into the module (P9, D12) |
| the body empty-list clamp (blocker) | D13 |
| the e2e crate has no gmsh (blocker) | the body case moved to β's gmsh binary |
| a nonexistent done-criterion script | ε's greps |
| `SupportTarget::Face` | added |
| the shipped point code | D4 fold, with a hand-repoint |
| α split | α₁ / α₂ |
| the buckling corridor | measured: +0.04 pt |
| modal mechanism above the dense ceiling | the realization-time Warning |
| β's discriminating check | `pub const ELASTIC_BODY` |
| the twelve minors | applied in place |

**Fresh review.** A second, fresh fable reviewer read the landed state on 2026-10-19: this PRD, these twins, the
filed tasks and the amended ones. It raised one blocker, seven majors and nine minors, all applied to the PRD
and the task records:

| finding | fix |
|---|---|
| the body implicit clamp would have added a second clamp on the `Faces` path (blocker) | D13 `unless: AnyApplied` |
| D12's coverage test red before γ | moved to ε |
| a vacuous solver-host `nearest_node` row | moved to the S5 member |
| `support-bc-literals` and #7189's literal | 0 or 1 until δ |
| unowned S5-row creation | the first-lands rule |
| α₁ could not resolve `Faces` from the solver crate | `Faces` moved to β |
| `bases` typed on the absent `NodalBasis` | added by whichever of δ / #8256 lands second |
| the unowned point decode | δ owns it |

**Decompose verification.** `scripts/prd-decompose-verify.mjs` ran on 2026-10-19 (run `wf_bfc0888c-04e`) over the
pre-split α, β, γ and ε. It probed 3/4 leaves, executed 45/45 records, and produced 0 malformed records. ε was
unenumerated: its Enumerator returned no premises.

The disposition was BLOCKS. Each blocking item was resolved in the PRD before filing:

| blocking item | resolution |
|---|---|
| `printer_print_envelope.ri` fails `reify eval` on #5847's constructor debt, and it and `printer_gantry_modes.ri` carry no analytic band | BT6 and α₂ now band only `simply_supported_beam_modes` and `clamped_clamped_beam_modes.ri`; the structural ones are recorded in process |
| `reify check prj/printer_v01/printer.ri` SIGSEGVs on the current build (open #7383) | the printer.ri verdict check is conditional on #7383 |
| the body fixtures' "solves on the fallback" came from the stale 2026-09-01 release binary, which `prd-capability-check.py` resolves before the newer debug one. On main the pre-hydration guard returns Failed with no diagnostic (#6660) | P7, the fixture headers and `body-silent-today` were corrected; a note was added to S22 ε #8649 |
| γ cannot lower `support-bc-literals` to 0 alone | §6.8: β and γ each lower it, and ε asserts 0 |
| fixed-guided runs at nx = ny = 10, and there is a P2 family | γ's text was corrected |

**ε's premises were verified inline** by the lead (`ls` / `grep` at `eb0a855dad`):

- the `## INV-SF-3 \`declared-intent-consumed-or-diagnosed\`` heading is present (line 135), and the census
  header `| Surface | Gate | Status |` is present;
- `examples/best_practices/INDEX.md` and `.claude/skills/reify-design/SKILL.md` exist;
- `crates/reify-mcp/src/tools/chunks/fea.md`, `crates/reify-eval-fea-tests/tests/support_refusals_e2e.rs` and
  `scripts/tests/test_design_invariants_consistency.py` are absent (the last is why ε's done criteria are
  greps).

Verdict key:

- **PASS** means the binding held at authoring time.
- "producer-self" means the capability is the leaf's own deliverable.
- "manual" checks are recorded but excluded from the dispatch gate.

## α₁ — the module (byte-identical)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| five-sites-on-main | The five realization sites, by symbol (PRD §2.1) | PASS | — |
| boxface-upstream | producer: #9121, upstream (S16 ο `BoxFace`, `"root"` → `XMin`), and through it ξ #9120, ν #9091, ζ₀ #9287 | PASS | — |
| no-cross-crate-registry | reify-eval depends on the solver crate, so a solver test binary cannot link trampoline-side submissions. The rows live in the module (P9) | PASS | — |
| module-table | producer-self | PASS | grep `pub fn constraint_pattern\(` present in `crates/reify-solver-elastic/src` |
| module-composer | producer-self. It also keeps ξ's crates-wide `fn realize_supports\(` check green | PASS | grep `pub fn realize_supports\(` present in `crates/reify-solver-elastic/src` |
| shell-table-absorbed | producer-self (1 on main) | PASS | grep `pub fn build_support_bcs\(` absent in `crates` |
| solver-crate-reify-core-edge | producer-self (R4). There is no cycle: reify-ir and reify-kernel-gmsh already depend on reify-core | PASS | grep `^reify-core` present in `crates/reify-solver-elastic/Cargo.toml` |
| mechanism-code | producer-self | PASS | grep `FeaSupportsUnderRestrained` present in `crates/reify-core/src` |
| totality-matrix | `support_capability_matrix.rs` over `REGISTERED` (BT4) | PASS | manual |

## α₂ — modal

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| supportspec-upstream | producer: #8683, upstream (S15 γ `SupportSpec`) | PASS | — |
| modal-local-realize-supports-upstream | producer: #9120, upstream through α₁ → ο | PASS | — |
| modal-placement-settled | #9121 → #9120 → #9091 → #9003 → #8999: modal is in reify-analysis when α₂ runs | PASS | — |
| lone-pin-clamped-today | `support_policy_modal_lone_pin.ri` rc 0: f1 = 18925.97 Hz plus `I_ModalPinnedFaceRealization` "clamps all 3 translational DOFs". Control `support_policy_modal_unknown_face.ri` (free-free today): rc 0, f1 = 0 Hz, `W_ModalRigidBodyMode` modes 0–2, at 546 DOFs (under the 1024 ceiling). After α₂: 2 free rotations by rank. That the eigensolver reports both is predicted; the realization Warning reports the count in both regimes | PASS | — |
| fixtures-parse | both modal fixtures, tree-sitter rc 0 | PASS | — |
| hinge-numbers-measured | G6: measured in-leaf. Only `simply_supported_beam_modes` (2%) and `clamped_clamped_beam_modes.ri` carry analytic bands. `printer_gantry_modes` and `printer_print_envelope` are structural-only, and the latter is observable in process only (#5847). printer.ri verdicts are recorded only once crash #7383 is fixed. A crossed band escalates. On P1 the analytic-centre anchor ties to today's y=0 edge node (`ny = 1`, lowest index); on P2 it moves | PASS | manual |
| modal-row | producer-self | PASS | grep `pub const MODAL_BOX` present in `crates/reify-solver-elastic/src` |
| face-company-gone | producer-self (1 on main) | PASS | grep `enum FaceCompany` absent in `crates` |

## β — elastic

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| dims-refusal-upstream | producer: #9119, upstream (μ, code `FeaSupportNotRealized` per R1) | PASS | — |
| body-silent-today | `reify eval` on the two body fixtures (main-equivalent debug binary) exits 0. The CLI registers no gmsh kernel (#6660), so the body overload's pre-hydration guard returns Failed with no diagnostic before reading any support, and the result is the hollow default. The stale release binary instead solves on the fallback. The signal is therefore the in-process gmsh binary in `crates/reify-eval/tests` (PRD P7), which records the gmsh before-image | PASS | — |
| fixtures-parse | `support_policy_body_{tip_target,pinned_root}.ri`, tree-sitter rc 0 | PASS | — |
| body-row | producer-self; no other task can satisfy it | PASS | grep `pub const ELASTIC_BODY` present in `crates/reify-solver-elastic/src` |
| dims-clamp-const-gone | producer-self (1 on main). μ may retire it first, which only satisfies the check early | PASS | grep `const SYNTHETIC_CLAMP_FACE` absent in `crates` |
| root-string-check-gone | producer-self (1 on main). μ may retire it first | PASS | grep `fn any_support_targets\(` absent in `crates` |
| kind-blind-union-gone | producer-self (1 on main) | PASS | grep `fn target_node_set\(` absent in `crates` |
| static-numbers-identical | BT9–BT11 | PASS | manual |

## γ — buckling

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| buckling-loops-on-main | the P1 `k_face` loop and the P2 coordinate loop in `buckling.rs` | PASS | — |
| calibration-floor | Measured 2026-10-19, release, `pin_pin_euler_column_within_ten_percent`. Corner anchor (today): 9.17%. Centroid anchor `node_id(4,4,0)`: 9.21%. Bound 10%, so +0.04 pt. Fixed-pin has no single-node u_z anchor, so it is unchanged. No new bound is asserted | PASS | — |
| buckling-row | producer-self | PASS | grep `pub const BUCKLING_COLUMN` present in `crates/reify-solver-elastic/src` |
| loops-gone | producer-self (1 on main) | PASS | grep `for k_face in \[0usize, nz\]` absent in `crates` |
| buckling-numbers-measured | BT12 | PASS | manual |

## δ — points

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| point-sites-upstream | producer: #7189 (unlanded branch; mints `FeaPointSupportsUnderRestrained`) and #8251 (CT α `patch_nodes`; its done-producer check greps that code, which δ hand-repoints), both upstream | PASS | — |
| patch-target | producer-self. α₁ ships `Named` and `Face`; β adds `Faces` | PASS | grep `Patch \{ nodes` present in `crates/reify-solver-elastic/src` |
| point-values-unchanged | BT13. The names δ deletes exist only on `task/7189`, so an absent grep would be vacuous | PASS | manual |

## ε — invariant, e2e, docs (high, gate #9171)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| sf3-text-today | §INV-SF-3 names constraints, relate blocks, objectives and DFM rules only. The census table is in the same file | PASS | — |
| fea-chunk-upstream | producer: #9119 (creates or extends `chunks/fea.md`), reached through β. Co-owners #8259, #7088, #8296 and #8384 extend if it is present | PASS | — |
| e2e-crate-has-no-gmsh | `reify-eval-fea-tests` has no gmsh, so the body family is β's gmsh binary | PASS | — |
| sf3-enforcement-pointer | producer-self | PASS | grep `support_refusals_e2e` present in `docs/legibility/design-invariants.md` |
| refusal-e2e | producer-self | PASS | grep `fn ` present in `crates/reify-eval-fea-tests/tests/support_refusals_e2e.rs` |
| coverage-test | D12's coverage test lands here, after every row-adding leaf. `solve_buckling` takes `List<Support>`, so the test would be red before γ | PASS | manual |
| docs-truth | the chunk fence gate and intent findability | PASS | manual |

## Ω — PRD close

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| close-header | the committed SHIPPED header | PASS | manual |
