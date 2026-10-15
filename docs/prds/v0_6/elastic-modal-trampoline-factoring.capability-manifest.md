# Capability manifest: `elastic-modal-trampoline-factoring`

This manifest mechanizes G3 and G6 per leaf for `docs/prds/v0_6/elastic-modal-trampoline-factoring.md` (program stream S16), per `.claude/skills/prd/project.md`.

Evidence was gathered against main `1b06f93151` on 2026-10-14. Bindings cite by symbol and file, never by `path:line`.

This file is generated from its machine-readable twin, `docs/prds/v0_6/elastic-modal-trampoline-factoring.capability-manifest.yaml`, which carries the same bindings plus their `delivered_check` descriptors. Correcting a check later takes three writes: the `.yaml` descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

**Stamped 2026-10-15 by `commit_planning`:** α #9085, β #9092, γ #9086, δ #9087, ε #9088, ζ #9089, η #9090, θ #9093, ι #9094, κ #9102, λ #9118, μ #9119, ν #9091, ξ #9120, ο #9121, ω #9122. Every grep `delivered_check` (44) was copied into its producer's `metadata.delivered_checks`; `manual` rows were not.

## Verdict at author time

Every `kind: grep` check below was run with `git grep -E` against `HEAD` and is **RED** at main: an `expect: present` check finds nothing, and an `expect: absent` check finds its target.

- **Constructs, not bare tokens.** Each `absent` pattern names the construct that carries the defect (`let body_path = matches!`, `fn handle_morph_stats(`, `^\s*fn value_size_estimate(`), so a comment naming the retired symbol does not wedge the check.
- **No dead pathspecs.** Checks for code that S5 moves into `reify-analysis` scope to `crates`, so they hold before and after the move; no `present` check names a file that does not exist yet.
- **`manual` rows** are excluded from the dispatch gate. Each names the behavioural test or the upstream hard edge that enforces it.

## D3 disposition

μ's premises were probed directly by the lead with `reify eval` (`target/debug/reify` built 2026-10-12; the elastic files are unchanged since `a030ee7528`). The five committed fixtures under `tests/prd-gate/fixtures/` exit 0 at main without naming their extra input, and with `ShellForce.Off` the tet route realizes gravity and pressure. The other leaves (refactors with e2e regression floors, kernel tests, GUI/debug surfaces, docs) are outside the D3 harness's probe reach, so it was not run for them.

## α #9085 — Retire stats.rs, the morph_stats RPC and eligible(); compose_morph the single public entry; eighth telemetry bucket

- **stats-module-gone** — producer:α — `pub mod stats` declared in reify-mesh-morph lib.rs at main. **PASS**. Check: grep `pub mod stats` absent in `crates/reify-mesh-morph/src`.
- **morph-stats-rpc-gone** — producer:α — handle_morph_stats serves stats::snapshot (always zero) at main. **PASS**. Check: grep `fn handle_morph_stats\(` absent in `gui/src-tauri/src`.
- **eligible-wrapper-gone** — producer:α — bool wrapper with no production caller at main. **PASS**. Check: grep `pub fn eligible\(` absent in `crates/reify-mesh-morph/src`.
- **solver-error-bucket** — producer:α — compose_morph records nothing for Unsupported / non-panic SolverError at main (#7835 part 2). **PASS**. Check: grep `remeshed_solver_error` present in `crates/reify-mesh-morph/src`.
- **recorders-crate-private** — producer:α — record_* are pub and re-exported at the crate root at main. **PASS**. Check: grep `pub fn record_morphed` absent in `crates/reify-mesh-morph/src`.
- **seed-for-test-entry** — producer:α — gui handler tests seed the counters with the pub record_morphed() at main. **PASS**. Check: grep `pub fn seed_for_test` present in `crates/reify-mesh-morph/src`.
- **morph-deleted-upstream** — producer:#8527 upstream — morph() and its stub test deleted on task/8527 (unmerged). **PASS**. Check: manual — hard edge α ← #8527; #8527's own delivered checks enforce it.

## β #9092 — Per-build morph tally on BuildResult; typed engine-side morph outcome; CLI and e2e read the result

- **build-result-tally** — producer:β — BuildResult has no telemetry field at main. **PASS**. Check: grep `pub morph_tally:` present in `crates/reify-eval/src`.
- **typed-morph-outcome** — producer:β — MorphResult carries reason strings at main (heuristic 12). **PASS**. Check: grep `Ineligible\(String\)` absent in `crates/reify-eval/src`.
- **one-outcome-enum** — producer:β — reify-mesh-morph keeps a private MorphOutcome beside the engine seam at main. **PASS**. Check: grep `enum MorphOutcome` absent in `crates/reify-mesh-morph/src`.
- **e2e-lock-gone** — producer:β — morph_arm_e2e serialises on MORPH_DIAG_LOCK at main. **PASS**. Check: grep `MORPH_DIAG_LOCK` absent in `crates/reify-eval/tests`.
- **cli-reads-result** — producer:β — reify-cli prints the process-global snapshot after builds at main. **PASS**. Check: grep `&reify_mesh_morph::diagnostics::snapshot\(\)` absent in `crates/reify-cli/src`.
- **no-testing-dev-dep** — producer:β — reify-eval dev-depends on reify-mesh-morph with features = ["testing"] for morph_arm_e2e's reset_for_test at main. **PASS**. Check: grep `features = \[\"testing\"\]` absent in `crates/reify-eval/Cargo.toml`.
- **s1-accumulator-upstream** — producer:#9046 upstream (S1 θ per-call outputs accumulator). **PASS**. Check: manual — hard edge β ← #9046.
- **two-engines-independent** — behavioural: BT8. **PASS**. Check: manual — BT8 (two engines, independent tallies) is the behavioural cover.

## γ #9086 — One value_size_estimate in reify_ir::warm; three copies deleted; ratchet row

- **warm-estimator** — producer:γ — reify_ir::warm holds OpaqueState only at main. **PASS**. Check: grep `pub fn value_size_estimate` present in `crates/reify-ir/src`.
- **private-copies-gone** — grep: three private copies at main (trajectory_ops, dynamics_ops, fdm_slice). **PASS**. Check: grep `^\s*fn value_size_estimate\(` absent in `crates`.
- **moves-upstream** — producer:#9001, #9002 upstream (S5 κ₂, λ move the three copies' files). **PASS**. Check: manual — hard edges γ ← #9001, #9002.
- **ratchet-member-upstream** — producer:#8984 upstream (S5 α creates tests/infra/test_analysis_stratum_ratchet.py). **PASS**. Check: manual — hard edge γ ← #8984.
- **ratchet-row** — producer:γ on S5 α #8984's member. **PASS**. Check: manual — a bare row token over tests/infra is satisfiable by a docstring; the ratchet member enforces its own rows.

## δ #9087 — Delete worst_case_residual_fraction, its tests and audit pin; correct every false wiring claim

- **helper-gone** — grep: pub fn with 0 production callers at main. **PASS**. Check: grep `fn worst_case_residual_fraction` absent in `crates`.
- **audit-pin-gone** — grep: new_orphans_2026_06_02_g_allow.rs pins it at main. **PASS**. Check: grep `\"worst_case_residual_fraction\"` absent in `crates/reify-audit`.
- **move-upstream** — producer:#9002 upstream (S5 λ moves trajectory_ops). **PASS**. Check: manual — hard edge δ ← #9002.
- **trajectory-outputs-unchanged** — behavioural: trajectory e2e and examples/trajectory/*.ri print unchanged. **PASS**. Check: manual — regression floor; no value changes by construction (no production caller).

## ε #9088 — Delete InverseDynamicsRun.reused; poisoned-prior cache-hit test

- **reused-field-gone** — grep: test-only field with #[allow(dead_code)] at main. **PASS**. Check: grep `^\s*(pub(\(crate\))? )?reused: bool` absent in `crates`.
- **dynamics-moved-upstream** — producer:#9002 upstream — S5 λ moves the trampoline half into reify-analysis. **PASS**. Check: manual — hard edge ε ← #9002.
- **poisoned-prior-probe** — behavioural: BT11. **PASS**. Check: manual — BT11 is the behavioural cover.

## ζ #9089 — P2 promotion production home in reify_solver_elastic::p2_tet; test_support fixtures only; ratchet row

- **p2-promotion-production** — producer:ζ — p2_tet.rs holds the P2 element kernel only at main. **PASS**. Check: grep `pub fn promote_tets_to_p2` present in `crates/reify-solver-elastic/src/p2_tet.rs`.
- **test-support-promotion-gone** — grep: defined in assembly/test_support.rs at main. **PASS**. Check: grep `pub fn promote_tets_to_p2` absent in `crates/reify-solver-elastic/src/assembly`.
- **no-test-support-promotion-import** — grep: 8 test_support::promote_tets_to_p2 paths at main (2 production). **PASS**. Check: grep `test_support::promote_tets_to_p2` absent in `crates`.
- **moves-upstream** — producer:#9003, #9004 upstream (S5 μ, ν move the two callers). **PASS**. Check: manual — hard edges ζ ← #9003, #9004.
- **ratchet-member-upstream** — producer:#8984 upstream (S5 α ratchet member). **PASS**. Check: manual — hard edge ζ ← #8984.
- **ratchet-row** — producer:ζ on S5 α #8984's member. **PASS**. Check: manual — the ratchet member enforces its own rows.

## η #9090 — sampled_channels.rs leaf in the FEA family

- **sampled-channels-module** — producer:η — helpers live in compute_targets/mod.rs at main. **PASS**. Check: grep `mod sampled_channels;` present in `crates`.
- **fea-core-moved-upstream** — producer:#9004 upstream — S5 ν moves the FEA core. **PASS**. Check: manual — hard edge η ← #9004.

## θ #9093 — Elastic partition (directory module), AdaptiveSummary seam, shared test fixtures, stale allows removed

- **adaptive-summary** — producer:θ — the trampoline reads private adaptive fields at main. **PASS**. Check: grep `struct AdaptiveSummary` present in `crates`.
- **no-private-adaptive-read** — grep: three `= problem.grid;` / `= problem.iterates;` reads in the trampoline at main (tests use other forms). **PASS**. Check: grep `= problem\.(grid|iterates);` absent in `crates`.
- **dead-realized-mesh-gone** — grep: realized_solver_mesh has no caller at main. **PASS**. Check: grep `fn realized_solver_mesh\(` absent in `crates`.
- **no-module-scc** — INV-ARCH-1: S3's assert-module-scc.py reds a new SCC. **PASS**. Check: manual — the S3 module-SCC tool is the enforcement.
- **reshapers-upstream** — producer:#8374, #8248 upstream (§13 R2). **PASS**. Check: manual — hard edges θ ← #8374, #8248.

## ι #9094 — CantileverSolveRequest replaces the 14 positional parameters

- **request-struct** — producer:ι. **PASS**. Check: grep `struct CantileverSolveRequest` present in `crates`.
- **stale-ten-args-comment-gone** — grep: the '10 args' justification at main. **PASS**. Check: grep `// 10 args: the helper threads` absent in `crates`.
- **single-arg-calls** — behavioural: BT6. **PASS**. Check: manual — BT6; every call passes one request.

## κ #9102 — Explicit overload entries: five descriptor rows, three decoders, multi_case per-case typed requests

- **body-row** — producer:κ — all three overloads share solver::elastic_static at main. **PASS**. Check: grep `@optimized\(\"solver::elastic_static_body\"\)` present in `crates/reify-compiler/stdlib`.
- **field-row** — producer:κ. **PASS**. Check: grep `@optimized\(\"solver::elastic_static_field\"\)` present in `crates/reify-compiler/stdlib`.
- **multi-case-body-row** — producer:κ — both multi_case overloads share solver::multi_case at main. **PASS**. Check: grep `@optimized\(\"solver::multi_case_body\"\)` present in `crates/reify-compiler/stdlib`.
- **body-path-sniff-gone** — grep: two `let body_path = matches!` sites at main (elastic_static, multi_case). **PASS**. Check: grep `let body_path = matches!` absent in `crates`.
- **decoders-upstream** — producer:#8679, #8683 upstream (S15 β options decoders, γ loads/supports decoders). **PASS**. Check: manual — hard edges κ ← #8679, #8683; κ ← θ, ι intra-batch.
- **identity-parity** — S5 η #8997's identity parity test (reached transitively through ν #9004) covers the new rows. **PASS**. Check: manual — BT1.

## λ #9118 — Route enum, one planner, per-route solvers, one build site

- **route-planner** — producer:λ. **PASS**. Check: grep `fn plan_elastic_route` present in `crates`.
- **route-enum** — producer:λ. **PASS**. Check: grep `enum ElasticRoute` present in `crates`.
- **planner-agreement** — behavioural: BT2 (red at main for a non-isotropic thin scene under Auto). **PASS**. Check: manual — BT2.
- **one-builder** — producer:#8690 upstream (S15 ε build_elastic_result). **PASS**. Check: manual — hard edge λ ← #8690; S15's result-type-literal row.

## μ #9119 — Capability routing on the shell route; dims-support policy; FEA chunk

- **shell-fallback-code** — producer:μ — no input-neutral shell-capability code at main. **PASS**. Check: grep `FeaShellRouteInputFallback` present in `crates/reify-core/src`.
- **shell-requires-tet-code** — producer:μ. **PASS**. Check: grep `FeaShellRouteInputRequiresTet` present in `crates/reify-core/src`.
- **dims-support-code** — producer:μ — dims supports other than the root clamp are silent on tet at main. **PASS**. Check: grep `FeaDimsSupportNotRealized` present in `crates/reify-core/src`.
- **supports-present-arm-gone** — grep: FeaUnderConstrained's supports-present-none-root arm at main (elastic_static trampoline). **PASS**. Check: grep `!any_support_targets\(supports, SYNTHETIC_CLAMP_FACE\)` absent in `crates`.
- **multi-load-bracket-root** — producer:μ (R3) — the example's dims support targets "mount_face" at main. **PASS**. Check: grep `FixedSupport\(\"mount_face\"\)` absent in `examples/multi_load_bracket.ri`.
- **supports-decoder-upstream** — producer:#8683 upstream (S15 γ SupportSpec / LoadSpec). **PASS**. Check: manual — hard edge μ ← #8683; μ ← λ intra-batch.
- **bt4-bt5-fixtures** — grammar-fixture + behavioural: tests/prd-gate/fixtures/{shell_route_gravity_thin_plate,shell_route_pressure_thin_plate,dims_support_root_plus_xmax,dims_support_root_plus_xmax_thick,dims_support_xmax_only}.ri committed with this PRD; reify eval exits 0 on each at main, and none names its extra input (thin: max_vm 2.143e9 Pa as the tip-only solve; thick twin: 2.518e6 Pa as root-only) — the red premise of BT4/BT5a; after μ BT5a/b exit 1 (R3); BT5b: FeaUnderConstrained's supports-present arm replaced; BT5c: empty-list arm unchanged. **PASS**. Check: manual — BT4, BT5.
- **docs-truth** — docs-truth items 1 and 4: FEA chunk routing and support paragraphs. **PASS**. Check: manual — chunk fence gate + intent findability.

## ν #9091 — Modal partition (directory module) along the measured partition

- **modal-damping-module** — producer:ν — modal_ops.rs is one 12,835-line file at main; damping is a partition S15 does not empty. **PASS**. Check: grep `mod damping;` present in `crates`.
- **modal-moved-upstream** — producer:#9003 upstream — S5 μ moves modal. **PASS**. Check: manual — hard edge ν ← #9003.
- **s15-modal-upstream** — producer:#8693, #8702 upstream (S15 ζ modal builders, ι modal codes). **PASS**. Check: manual — hard edges ν ← #8693, #8702.
- **no-module-scc** — INV-ARCH-1. **PASS**. Check: manual — S3's module-SCC tool.

## ξ #9120 — Modal supports and damping seams; tests onto seams; modal reused_* flags deleted

- **supports-seam** — producer:ξ — build_dirichlet_bcs takes the options Value at main. **PASS**. Check: grep `fn realize_supports\(` present in `crates`.
- **lumped-damping-plan** — producer:ξ — mechanism path calls classify_damping directly at main. **PASS**. Check: grep `fn plan_lumped_damping\(` present in `crates`.
- **modal-reused-flags-gone** — grep: reused_assembly / reused_setup test-only fields at main. **PASS**. Check: grep `reused_assembly: bool|reused_setup: bool` absent in `crates`.
- **seam-tests** — behavioural: BT7. **PASS**. Check: manual — BT7.

## ο #9121 — Structured box mesh and BoxFace vocabulary in reify-solver-elastic; adopted by modal, elastic and tests

- **structured-box-mesh** — producer:ο — no production structured mesh generator at main. **PASS**. Check: grep `pub struct StructuredBoxMesh` present in `crates/reify-solver-elastic/src`.
- **box-face-enum** — producer:ο — face names written twice (face_bound, box_face_plane) at main. **PASS**. Check: grep `pub enum BoxFace` present in `crates/reify-solver-elastic/src`.
- **s14-ratchet-host** — producer:#8614 upstream (S14 α: crates/reify-solver-elastic/tests/adoption_ratchets.rs hosts freudenthal-table-copy). **PASS**. Check: manual — hard edge ο ← #8614; the ratchet binary enforces its own rows.
- **counts-from-caller** — D7: StructuredBoxMesh takes counts; #8252's synthetic_grid adapts second. **PASS**. Check: manual — BT9 byte-identity is the cover.
- **solver-test-copies-gone** — grep: the six-tet literal in three reify-solver-elastic test sites at main. **PASS**. Check: grep `\[c\[0\], c\[5\], c\[1\], c\[6\]\]` absent in `crates/reify-solver-elastic/tests`, `crates/reify-solver-elastic/src/boundary/patch_load.rs`, `crates/reify-solver-elastic/src/diagnostics.rs`.
- **modal-split-upstream** — producer:ξ (intra-batch; ξ rewrites the supports partition ο converts); θ and ν (intra-batch, the partitioned files). **PASS**. Check: manual — hard edges ο ← θ, ν, ξ.
- **ratchet-member-upstream** — producer:#8984 upstream (S5 α ratchet member hosts freudenthal-table-consumer and box-face-literal). **PASS**. Check: manual — hard edge ο ← #8984.
- **byte-identical-meshes** — behavioural: BT9. **PASS**. Check: manual — BT9; the freudenthal-table-copy and box-face-literal ratchet rows.

## ω #9122 — PRD close: terminal stamp

- **terminal-stamp** — producer:ω. **PASS**. Check: manual — committed SHIPPED header with landed leaf ids, AS-AUTHORED freeze, LIVE/AS-AUTHORED map.
