# Capability manifest — prose-rehoming-and-symbol-citations

PRD: `docs/prds/v0_6/prose-rehoming-and-symbol-citations.md` (review-all S23).

- **Binding.** Evidence was bound 2026-10-15 against main `9bdef68093` by six read-only measurement seats; the review's anchors are `a030ee7528`.
- **Pass 1.5 fold.** It re-measured every touched check at main with its exact pattern and path, and each one is red today (count ≥ 1 for a present→absent check).
- **No `.ri` fixtures.** The PRD touches no language surface, so the grammar gate and the docs-truth gate do not fire.
- **Task ids** are stamped at decompose.
- **Adopted tasks.** Three leaves adopt filed tasks: δ₃ = #8531, ζ₃ = #8542, ζ₄ = #8316. Their `metadata.delivered_checks` are written at pass 2 (three writes each).

Ledger-absent checks grep `crates/reify-audit/plinecite-baseline.txt` for a path prefix, `.rs` rows only.
- They become meaningful only after S24 ο #8822 regenerates the widened ledger, and every such leaf depends on #8822.
- Phrase checks use single-line tokens, because several quoted phrases wrap across comment lines.
- Every check is pattern-anchored, never `file:line`.

Machine-readable twin: `prose-rehoming-and-symbol-citations.capability-manifest.yaml`.

## α₁ — jcodemunch scripts and tests — symbol cites and the external-package pointer form

- `jcodemunch-cites-exist-today`:
  - Binding: signal premise — tests/infra/test_jcodemunch_index_reify.sh cites jcodemunch-index-reify.sh:496-503 (stale; seam is REIFY_JC_INDEXER_CMD -> INDEXER_CMD); test_jcodemunch_index_units.sh cites with-jcodemunch-serve.sh:248-257 "enumerates the four sites" while lib_jcodemunch_pin.sh calls itself the SINGLE definition site; 14 external cites in jcodemunch-index-reify.sh, 7 in lib_jcodemunch_pin.sh (measured 2026-10-15). Prereq #8540 is a real edge (its files include with-jcodemunch-serve.sh).
  - Delivered check: grep, pattern `enumerates the four sites`, expect `absent`, paths `tests/infra/test_jcodemunch_index_units.sh`.
  - PASS.

## α₂ — seed and gc script prose — rehome measurements and amendments; symbol cites; Greek-letter comment aliases

- `seed-reviewer-prose-present`:
  - Binding: signal premise — scripts/seed-warm-lane.sh comment share 0.599; "measured at 0.249s" and "amendment 2" present; _close_inherited_fds carries a 53-line block over a 7-line fn; warm-lane-gc.sh:648-649 and git-rerere-guard.sh:735-758 cites stale (targets: the `2>&1 | while` consumer, cmd_arm). warm-lane-gc.sh 20 + warm-lane-gc-sweep.sh 18 Greek-letter comment lines outside _usage (2026-10-15). Check token count 1 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Paths widened to lib_test_semaphore.sh (two cargo-test-occt-gated.sh cites, OQ-3 ruling); prereq S18 ε #9101.
  - Delivered check: grep, pattern `0\.249s`, expect `absent`, paths `scripts/seed-warm-lane.sh`.
  - PASS.

## α₃ — warm-lane infra tests — symbol and Block cites, header indexes dropped, esc pointers rehomed

- `seed-test-cites-stale-today`:
  - Binding: signal premise — test_seed_warm_lane.sh carries seed-warm-lane.sh:1133/:563/:663/:171/:1666 and warm-lane-gc.sh:648-649/:559-565, all stale at 9bdef68093; header block indexes cover audit 1/20, pool 6/16, gc missing H; 8 esc pointers in the audit test.
  - Delivered check: grep, pattern `seed-warm-lane\.sh:[0-9]`, expect `absent`, paths `tests/infra`.
  - PASS.

## α₄ — infra tests citing verify.sh — retarget at scripts/reify_verify symbols after the S18 cutover

- `verify-sh-cites-outside-verify-sh`:
  - Binding: signal premise — ~70 lines in 25 tests/infra files plus 5 in scripts cite verify.sh:N (2026-10-15); after S18 zeta #9112 verify.sh is a 2-line shim, so every such cite is stale by construction. Targets are scripts/reify_verify/ modules (S18 PRD §4 D1). Check token count 68 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Code-line literals (test_gui_vitest_rpc_hardening.sh assert label 'verify.sh:328') are not comment lines, need no plinecite:allow, and do not match the check.
  - Delivered check: grep, pattern `^\s*#.*verify\.sh:[0-9]`, expect `absent`, paths `tests/infra`, `scripts`.
  - PASS.
- `reify-verify-package-exists-at-dispatch`:
  - Binding: G3 — scripts/reify_verify/ is produced by S18 (gamma1 #9098 .. zeta #9112); alpha4 depends on zeta, so the targets exist when it dispatches. Not present on main today (by design).
  - PASS.

## α₅ — infra and scripts long tail — remaining ledger rows under tests/infra and scripts

- `long-tail-population`:
  - Binding: signal premise — after alpha1..alpha4 and the exclusions (#8540, #7682, S18 nu/theta2/lambda, S19 eta/theta), the tail includes compute-trampoline Part A (13 crate cites), check-compute-trampoline-registration.sh (6), reify-audit-freshness.sh (4), flake-density-report.py docstring cites, test_sandbox_cache_writability_seam.sh (13 dark-factory roles.py) (2026-10-15). Prereq S19 Ω #8729 (σ/π/ρ/γ partition files in the tail).
  - PASS.

## β₁ — reify-solver-elastic — plan.json pointers, symbol cites, reviewer prose, two_tet_fan_mesh volume fix

- `plan-json-pointers-present`:
  - Binding: signal premise — git grep -c plan\.json over crates/reify-solver-elastic/src = 14 lines in 9 files; repo tracks 0 plan.json (git ls-files) (2026-10-15). Prereqs widened: S14 ρ #8660, S16 ο #9121, #8380, #8381.
  - Delivered check: grep, pattern `plan\.json`, expect `absent`, paths `crates/reify-solver-elastic`.
  - PASS.
- `two-tet-volume-claim-false`:
  - Binding: G6 — error_estimator.rs two_tet_fan_mesh doc says "Both tets have volume 1/6"; tet1 determinant is 2, volume 1/3; qoi.rs test copy documents the discrepancy (read 2026-10-15). No other leaf fixes it.
  - Delivered check: grep, pattern `Both tets have volume 1/6`, expect `absent`, paths `crates/reify-solver-elastic`.
  - PASS.

## β₂ — eval/analysis trampolines — elastic_static and modal_ops drift, engine_compute residue

- `elastic-static-drift-present`:
  - Binding: G6 — elastic_static module doc "36 tets, 28 nodes" vs synthetic_grid_counts nx=60 for L=1 m, h=0.1 m; element_von_mises_anisotropic named 9 times, deleted in 8de74b4c3b (git cat-file -t commit); ptodo:allow TODO premise stale (ComputeFn/ComputeOutcome live in reify-compute-contract) (2026-10-15). "10 args" and "(6 fields" are excluded (S16 iota #9094, S15 zeta #8693). Check token count 11 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `6 tets per hex = 36 tets|nz=6 → 28 nodes|element_von_mises_anisotropic`, expect `absent`, paths `crates`.
  - PASS.

## β₃ — stdlib FEA and dynamics, shell-extract medial, mesh-morph module doc

- `analysis-stdlib-cites-stale`:
  - Binding: signal premise — fea.rs 17 cites + 2 bare, dynamics/eval.rs 13, medial.rs 5 bare ("~:617", "line 470/475"), all stale where checked; min_max_stress two-call defence and envelope_tensor_projection two-pass prose present; compliance_cell_f64 block is INV-AD-4 (kept) (2026-10-15). Check token count 4 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Bare cites are invisible to PLINECITE, so the check greps the bare tokens. Prereq S8 ω #8914 added.
  - Delivered check: grep, pattern `~:617|line 47[05]`, expect `absent`, paths `crates/reify-shell-extract/src/medial.rs`.
  - PASS.

## γ₁ — kinematics residue — loop-closure, solver, snapshot, joints and mechanism prose

- `kinematics-residue-present`:
  - Binding: signal premise — loop_closure.rs "future task 2585" (line 4) and 5 reify_constraints::loop_closure pointers (module absent); mechanism.rs 4 plan.json pointers, "measured at 1.3 m", "#2538 will reconcile" (#2538 done) (2026-10-15). The 11 multi-DOF sites of fk-67ca81ff9067 are S6/#8545-owned (PRD §8). Check token count 11 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Scoped to γ₁ files (sweep.rs plan.json is β₃).
  - Delivered check: grep, pattern `plan\.json|future task 2585|reify_constraints::loop_closure`, expect `absent`, paths `crates/reify-stdlib/src/joints.rs`, `crates/reify-stdlib/src/joints`, `crates/reify-stdlib/src/loop_closure.rs`, `crates/reify-stdlib/src/loop_closure_solver.rs`, `crates/reify-stdlib/src/loop_closure_solver`, `crates/reify-stdlib/src/newton.rs`, `crates/reify-stdlib/src/snapshot.rs`, `crates/reify-stdlib/src/snapshot_queries.rs`, `crates/reify-stdlib/src/mechanism.rs`.
  - PASS.

## γ₂ — reify-gcode — drop the colliding plan-decision numbers; inline contracts

- `gcode-plan-pointers-present`:
  - Binding: signal premise — ast.rs 4, klipper.rs 6, marlin.rs 5, tests/parse_klipper.rs 2 untracked-plan pointers; trajectory-input-shaping.md §8 (8.1–8.8) holds none of the rationales; no stream touches reify-gcode src (2026-10-15). Check token count 17 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `plan design decision|plan reuse-item|design decision #|plan step-|decision #[0-9]+ in plan|plan design$`, expect `absent`, paths `crates/reify-gcode`.
  - PASS.

## δ₁ — reify-constraints solver tree and cpsat — history prose rehomed, symbol cites

- `solver-history-prose-present`:
  - Binding: signal premise — solver.rs "History (task #4700 → #4710)" and "RE-MEASURED (task #5711 step-7" present; cpsat.rs 6 stale cites (targets SolveResult::into_ranked_pass_through, ConstraintSolver::solve_ranked default, RankedCandidate::unique verified) (2026-10-15). S14 xi/omicron/pi split solver.rs into solver/.
  - Delivered check: grep, pattern `History \(task #4700|RE-MEASURED \(task #5711`, expect `absent`, paths `crates/reify-constraints/src`.
  - PASS.

## δ₂ — SolverRegistry::solve_inner — rehome the 150-line block and review/history prose (prose only; refactor → #6903, OQ-2 (b))

- `solve-inner-prose-present`:
  - Binding: signal premise — registry.rs lines 349-794 are 261/446 comment; the block over the ResolutionProblem literal carries "ORDERS OF MAGNITUDE", "When task #5720 landed", "LAYER 2". Prose-only per OQ-2 (b); prereqs S14 ν #8645, ξ #8653, #6677, #8822. Check token count 1 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `ORDERS OF MAGNITUDE`, expect `absent`, paths `crates/reify-constraints/src/registry.rs`.
  - PASS.

## δ₃ (task #8531) — reify-kernel-occt lib.rs — symbol cites (deps added at pass 2: S11 iota #8684, #8822)

- `occt-cites-stale`:
  - Binding: signal premise — 5 production cite lines (6 cites) stale: geometry_ops.rs:482/:523, lib.rs:1597 (draft_faces), lib.rs:2167-2174 / 2398-2418 / 2742 (Fillet/Draft/InterpCurve arms) (2026-10-15). S11 gamma1/iota move the cited code. Check narrowed to lib.rs (8 ledger rows at 9bdef68093); register.rs / tessellate_options.rs rows are θ₄.
  - Delivered check: grep, pattern `^crates/reify-kernel-occt/src/lib\.rs `, expect `absent`, paths `crates/reify-audit/plinecite-baseline.txt`.
  - PASS.

## δ₄ — kernel crates — manifold, gmsh, openvdb cites into OCCT and sibling crates

- `kernel-cross-cites-stale`:
  - Binding: signal premise — manifold kernel.rs 8 prod + 5 test cites (OCCT lib.rs:456-466/460-461/516-523/677-710 all stale; mocks.rs:889; kernel_registry.rs:329-353; "PRD line 70" now 85); gmsh kernel_real "line ~120" (clear at 473); openvdb lib.rs:2783-2795 stale (2026-10-15).
  - Delivered check: grep, pattern `^crates/reify-kernel-manifold/src/kernel\.rs `, expect `absent`, paths `crates/reify-audit/plinecite-baseline.txt`.
  - PASS.

## δ₅ — reify-eval dispatch and classifier prose — dispatcher, warm_pool, structural_classifier, sweep_classifier, relate_solve, topology_attribute_propagation

- `dispatcher-variant-claim-false`:
  - Binding: G6 — dispatcher.rs module doc "(4 variants), so the algorithm terminates after at most 4 expansions"; ReprKind has 5 (reify-ir geometry.rs: BRep, Mesh, Sdf, Voxel, VolumeMesh) (2026-10-15). Check token count 4 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `ReprKind`\]? \(4|reify_types::ReprKind`, expect `absent`, paths `crates/reify-eval/src/dispatcher.rs`.
  - PASS.
- `classifier-prose-share`:
  - Binding: signal premise — structural_classifier 0.79 (classify_cell 181 doc / 23 code), sweep_classifier 0.59, warm_pool 0.53, reify-eval relate_solve 0.48; quotes "verified 2026-06-27", "Measured, not assumed, by", "Do NOT \"fix\" the extra level" present (2026-10-15).
  - PASS.

## δ₆ — check-surface remainder — tolerance_combine, representation_within, output_spec, two engine_constraints cites

- `check-surface-residue`:
  - Binding: signal premise — engine_constraints.rs lib.rs:163 (-> SimpleConstraintChecker::check) and engine_constraints.rs:42-62 (-> RepresentationWithin ordering invariant doc) stale; unenforced_representation_bound_diagnostic 84/21, compute_representation_bounds 75/21 doc/code (2026-10-15). S12 lambda splits tolerance_combine.rs.
  - PASS.

## ε₁ — DiagnosticCode — one line per variant, one strum round-trip test, ratchet rows

- `strum-available-in-reify-core`:
  - Binding: G3 — strum is a reify-core dependency; ty.rs derives EnumDiscriminants with EnumIter/EnumCount on TypeDiscriminants; serde_json is a dev-dependency (2026-10-15).
  - PASS.
- `diagnostic-doc-population`:
  - Binding: signal premise — DiagnosticCode body: 214 variants, 4,423 /// lines, 204 `///…Origin:` lines (206 Origin tokens), 25 .rs/.ri coordinates, 166 mnemonic lines, 9 Minting rationale paragraphs; 151 #[test], 136 per-variant derive checks (2026-10-15; file unchanged since a030ee7528). ε₁ owns every Origin line including ScopeCoupling (critic M2).
  - Delivered check: grep, pattern `^ *///.*Origin:`, expect `absent`, paths `crates/reify-core/src/diagnostics.rs`.
  - PASS.
- `ratchet-engine-available`:
  - Binding: G3 — tests/infra/adoption-ratchet.manifest + adoption_ratchet.py gated by test_infra_adoption_ratchet.sh; row grammar pattern-id | ERE | path | ceiling | owner; /// lines count as code lines (2026-10-15).
  - Delivered check: grep, pattern `^diagnostic-code-origin-pointer \|`, expect `present`, paths `tests/infra/adoption-ratchet.manifest`.
  - PASS.

## ε₂ — HexWedgeMeshOutcome beside p2_substitution_diagnostic; re-cite to #4746

- `hexwedge-unwired-and-miscited`:
  - Binding: signal premise — 0 callers outside diagnostics.rs; doc cites #2947 (cancelled) and #2991 (done); #4746 (pending) names these diagnostics as its already-built emitters, so the helper is kept, not deleted (get_task 4746, 2026-10-15).
  - Delivered check: grep, pattern `fn hex_wedge_mesh_diagnostic`, expect `absent`, paths `crates/reify-core/src`.
  - PASS.
- `relocation-target-exists-at-dispatch`:
  - Binding: G3 — p2_substitution_diagnostic is at engine_build.rs today; S2 beta2 (unfiled) moves it to volume_mesh_assembly.rs (S2 draft §5.8). epsilon2 depends on that leaf; id at S2 filing (else x_seam_wait S2).
  - PASS.

## ε₃ — reify-ir geometry — the five false comments, PRD-line cites, symbol cites

- `geometry-false-comments-present`:
  - Binding: G6 — "Descriptor table for all 49" with 55 rows/55 variants; "unverified copy until L2" while engine_build reads descriptor_for; ParentRole "authoritative source"; content_hash_bytes allow(dead_code) with production callers in value.rs; lookup_result_face "exercised only by tests" (manifold debug_assert caller; #4263 still pending); G-allow citing done #3623 (2026-10-15). Prereq S10 ω #9000 added (δ₃ #8961, θ₁–θ₄ #8980–#8983 edit the same files).
  - Delivered check: grep, pattern `Descriptor table for all 49|unverified copy until L2`, expect `absent`, paths `crates/reify-ir/src`.
  - PASS.

## ε₄ — engine_build residue — INV-BUILD pointers; reviewer and measurement prose

- `inv-build-claims-false`:
  - Binding: G6 — probe_realization_cache rustdoc "leans on INV-BUILD-1, still `proposed`" and the "Release-mode reliance" block "status `proposed` … future work"; docs/invariants.md INV-BUILD-1 = enforced(type) (2026-10-15). The registry row is correct; only code comments change (D10).
  - Delivered check: grep, pattern `INV-BUILD-1, still`, expect `absent`, paths `crates/reify-eval/src`.
  - PASS.

## ε₅ — reify-eval lib.rs, results.rs and engine_eval residue

- `lib-and-engine-eval-drift`:
  - Binding: signal premise — lib.rs:1 "reify-types::value::SampledField"; cell_commit/detectors "no external callers yet" (4 and 2 caller files); engine_eval.rs coordinates engine_eval.rs:609-614, "~line 5270", "L3119", "(`:472`)"; nondriving_joint_compile_spans / detect_error_map_diagnostics mis-stitch (2026-10-15). Compat re-exports stay (S5 D12). Check token count 1 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `external callers yet`, expect `absent`, paths `crates/reify-eval/src/lib.rs`.
  - PASS.

## ε₆ — ty.rs — quantity-slot rule to a docs/decisions record; short //!

- `ty-module-doc-and-home`:
  - Binding: G6 — ty.rs //! is 305 lines and declares itself "the single normative home"; dimensioned-construction-strictness.md §12 is "Out of scope" and defers to ty.rs, so no PRD section is a home; docs/decisions/ precedent exists (2522-, 883-); ~10 inbound pointers in conformance/mod.rs (partitioned by S9 omicron #8745) and units.rs (2026-10-15).
  - Delivered check: grep, pattern `single normative home`, expect `absent`, paths `crates/reify-core/src/ty.rs`.
  - PASS.

## ε₇ — reify-expr — stack-budget rule stated once; pin lists and self-cites gone

- `stack-budget-repeated`:
  - Binding: signal premise — "3 MiB" 17 times (15 production) in reify-expr lib.rs; eval_worst_case_dispatch doc lists 7 pinning tests; lib.rs:NNN self-cites at 2848/2737/206/2185/2868 (2026-10-15). S8 tau #8911 splits lib.rs first.
  - Delivered check: manual. the post-split home of MAX_RECURSION_DEPTH is chosen by S8 tau; the leaf records git grep -c "3 MiB" -- crates/reify-expr/src = 1 in its commit.
  - PASS.

## ζ₁ — reify-audit — correct the ten false statements; rehome measurement and rejected-alternative prose

- `audit-false-statements-present`:
  - Binding: G6 — all nine present at 9bdef68093: PTodo "all Medium severity" and "exit-neutral" vs ptodo::Kind::severity High arms; ptodo.rs "all emitting Medium-severity"; pdiag "warn-first Medium lanes"; "currently ships three detectors" (19 patterns); P<N> convention (11 of 19 lack a digit); Medium "reserved for metadata-cleanliness"; AuditContext "never owns a connection" (ptodo::open_tasks_db); RealGitOps "Untested by the slice-1" (13 tests in tests/real_git_ops.rs); pdoccover "stubs — filled in by the TDD". PLINECITE excludes crates/reify-audit/. Check token count 7 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Tenth claim: bin/reify-audit.rs "all three detectors".
  - Delivered check: grep, pattern `three detectors:|all three detectors|slice-1 integration suite|all Medium severity|stubs — filled in by the TDD|never owns a connection`, expect `absent`, paths `crates/reify-audit/src`.
  - PASS.

## ζ₂ — reify-lsp and reify-cli front-end prose

- `lsp-cites-stale`:
  - Binding: signal premise — reify-lsp diagnostics.rs cites cache.rs:482-513 twice (CacheStore::mark_pending is at reify-eval cache.rs:989); task-number citations main 53, references 29, diagnostics 19 (2026-10-15). The "outside this task's file scope" sentences are S27 beta/gamma-owned.
  - Delivered check: grep, pattern `cache\.rs:482-513`, expect `absent`, paths `crates/reify-lsp/src`.
  - PASS.

## ζ₃ (task #8542) — gui engine.rs pointers and the two misplaced doc blocks

- `engine-rs-drift-present`:
  - Binding: signal premise — 15 cites on 14 lines + 3 bare in gui/src-tauri/src/engine.rs; build_values doc sits on display_scalar; build_template_node doc runs into collect_consumed_sibling_names (2026-10-15; file unchanged since anchor). emit_auto_resolve_if_any doc is S25 lambda #8947's.
  - PASS.

## ζ₄ (task #8316) — reifyGrammarCorpus corpus cites + reify.grammar (dep added at pass 2: S18 pi #9096, #8822)

- `corpus-cites-present`:
  - Binding: signal premise — reifyGrammarCorpus.test.ts 133 cites (119 examples/*.ri:N, 9 bare-stem .ri:N, 2 tests/prd-gate, 2 ts_parser.rs:299, 1 other); reify.grammar 2 (2026-10-15). Check token count 111 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers.
  - Delivered check: grep, pattern `examples/[^ ]+\.ri:[0-9]`, expect `absent`, paths `gui/src/__tests__/reifyGrammarCorpus.test.ts`.
  - PASS.

## ζ₅ — GUI residual — gui-ts and gui-rs cites; unitLadder display-helper residue

- `gui-residual-population`:
  - Binding: signal premise — gui-ts 182 (49 outside the corpus test), gui-rs 59 (engine_tests.rs 26, write_tools.rs 5, commands_tests.rs 5, types.rs 3); unitLadder.ts 84% comments, grammar prose owned by S26 theta #9069 (2026-10-15). Check token count 254 measured at main 9bdef68093 (2026-10-15, pass 1.5 fold); red until the leaf delivers. Ledger has 0 ^gui/ rows before #8822, so the check greps live comment cites. Prereqs add S1 ω #9052, S6 ω₂ #8856, S16 ω #9122.
  - Delivered check: grep, pattern `^\s*(//|\*|/\*).*[A-Za-z0-9_-]\.(rs|ts|tsx|ri|sh|py|toml|yaml|yml):[0-9]`, expect `absent`, paths `gui/src`, `gui/src-tauri/src`.
  - PASS.

## η — docs naming retired mechanisms — architecture §3.5, freshness PRD, invariants closing paragraph

- `retired-mechanism-docs`:
  - Binding: signal premise — docs/reify-implementation-architecture.md §3.5 "Early Cutoff" region describes freshness propagation inside the dirty-cone walk; docs/prds/freshness-4-variant.md cites it; S3 zeta #8699 deletes propagate_freshness_only/freshness_walk.rs; docs/invariants.md closing paragraph cites reset_dispatch_tallies() bundling, deleted by S1 theta #9046 (2026-10-15).
  - Delivered check: grep, pattern `reset_dispatch_tallies`, expect `absent`, paths `docs/invariants.md`.
  - PASS.

## θ₁ — residual ledger sweep — reify-compiler

- `compiler-residual`:
  - Binding: signal premise — reify-compiler 190 src + 346 test cite occurrences at 9bdef68093 by the program rule; .ri rows excluded (#8305). 168 .rs rows measured at main 9bdef68093 (2026-10-15, pass 1.5 fold). Prereq S19 θ #8700 added.
  - Delivered check: grep, pattern `^crates/reify-compiler/[^ ]*\.rs `, expect `absent`, paths `crates/reify-audit/plinecite-baseline.txt`.
  - PASS.

## θ₂ — residual ledger sweep — reify-eval and reify-eval-fea-tests

- `eval-residual`:
  - Binding: signal premise — reify-eval 288 src + 300 tests, reify-eval-fea-tests 17 cite occurrences at 9bdef68093. Scope adds crates/reify-analysis/**; .rs rows only (.ri rows are #8305). 222 rows measured at main 9bdef68093 (2026-10-15, pass 1.5 fold).
  - Delivered check: grep, pattern `^crates/reify-(eval|eval-fea-tests|analysis)/[^ ]*\.rs `, expect `absent`, paths `crates/reify-audit/plinecite-baseline.txt`.
  - PASS.

## θ₃ — residual ledger sweep — language and tooling crates

- `language-residual`:
  - Binding: signal premise — reify-expr tests 153, reify-core 37, reify-ir 25, reify-test-support 39, reify-builtins 14, reify-syntax 13, reify-ast 6, reify-doc 4+, reify-cli 30 cite occurrences at 9bdef68093. Scope adds reify-lsp tests.
  - PASS.

## θ₄ — residual ledger sweep — analysis and geometry crates

- `analysis-residual`:
  - Binding: signal premise — reify-stdlib 77, reify-shell-extract 30, reify-constraints tests 34, kernel crates ~60, reify-mesh-morph 21 cite occurrences at 9bdef68093. Scope adds kernel src residue (26 rows), mesh-morph src residue (9), reify-constraints dual_jacobian.rs (1), reify-kernel-conformance (incl. build.rs) and reify-mesh-morph build.rs. Prereqs add S8 ω #8914, S21 ω #8663.
  - PASS.

## ω — PRD close — id backfill, terminal stamp, AS-AUTHORED freeze, INV-DOC-1 ledger remainder

- `close-header`:
  - Binding: overlay decompose-close obligation; signal is the committed header.
  - PASS.
