# /review — review-reify-20261015

## Method

```yaml
run_id: review-reify-20261015
as_of_sha: 0e60fa138d8852e1e26999d6dd22d05964848740
since: none
evidence:
  source_files: 2973
  slices: 14
  changed_files: null
  phase2_files: all (mode full)
  carried_findings: 359
  review_all_findings_reobserved: 344
  briefing: review/briefing.yaml (last_updated 2026-09-29)
  read_coverage_by_slice:
  - - GUI1-tauri-backend
    - 6800
    - 9
    - 'engine.rs: only the stub-hit contexts, with_solve_slot/check_with_solve_slot and anchor lines, since S25 owns the whole '
  - - TO-tooling
    - 7000
    - 11
    - 'Partially read: main.rs (main/dispatch, parse_and_compile*, cmd_test, cmd_build head, configured_eval_engine, cmd_gui/cm'
  - - FI2-warm-lanes-infra-tests
    - 7000
    - 10
    - 'Read in part only: run_all.sh (runner, timing, dispatch 636-1000 and 1640-2250), seed-warm-lane.sh (header, parser,
      side'
  - - LP1-compiler-front
    - 5200
    - 5
    - 'Partial reads: forall_elaborate.rs (1-140, 480-560, 680-1051), module_dag.rs (637-998), annotations/schema.rs (1-240),
      l'
  - - KM-kinematics
    - 7600
    - 11
    - 'Bodies read only in part: trampoline.rs (:142-1210 production read; tests not read), simulate.rs (header plus :416-583),'
  - - FI1-verify-pipeline
    - 6000
    - 13
    - 'Partially read: verify.sh (the a030ee7528..HEAD diff, the role/profile/MERGE_HEAD blocks, select_infra_tests, build_plan'
  - - CG2-kernels
    - 7000
    - 12
    - 'Production halves read fully but not listed above: manifold kernel.rs (1-1228), openvdb ingest.rs (30-870), gmsh kernel_'
  - - RA-reify-audit
    - 4800
    - 11
    - 'Read only in part: the production half of bin/reify-audit.rs (lines 82-1111), p5_phantom_done.rs check_with_target
      / che'
  - - GUI2-frontend
    - 5200
    - 11
    - 'Not deep-read: gui/src/debug/bridge.ts (2245 lines; fully owned by S26 sigma/tau), most of App.tsx (only the navigation,'
  - - LP3-core-ir-expr
    - 4200
    - 6
    - "Bodies not read: most of reify-ir value.rs (14k lines, h14 owned by S10 \u03B1\u2084 #8933), expr.rs, constraint.rs,\
      \ ri_literal.rs"
  - - AN1-solvers
    - 7000
    - 11
    - 'Not read: membrane_load.rs, tensegrity_load.rs, buckling_kernel.rs, shell_assembly.rs, eigensolve.rs and form_find_free.'
  - - LP2-eval-core
    - 4200
    - 5
    - 'Not deep-read: graph.rs, dirty.rs, demand.rs, observed_demand.rs, journal.rs, scope_containment.rs, tolerance_promise.rs'
  - - AN2-compute-targets
    - 5200
    - 4
    - 'Partial reads: engine_compute.rs run_compute_dispatch 353-560, modal_ops.rs (1220-1330, 1575-1600, 2640-2760, 650-700), '
  - - CG1-constraints-geometry
    - 6500
    - 8
    - No deep read of cpsat.rs, solver.rs (owned by S14), relate_solve.rs (both crates), sketch.rs, solvespace.rs beyond the
      p
  stub_scan_hits: 1193
  stub_classification:
    allowed: 37
    cited: 33
    fixture: 120
    pattern_text: 970
    unintended_stub: 29
    untracked_marker: 4
  gtool_orphan_candidates:
    now: 811
    baseline: 650
    new_vs_baseline: 239
verification:
  confirmed: 378
  weakened: 80
  refuted: 0
  unverified: 15
cost:
  agents: 35
  subagent_tokens: 7539838
  wall_clock_min: null
  breakdown:
    phase2_workflow:
      agents: 29
      tokens: 6639489
      minutes: 131
    smoke:
      agents: 1
      tokens: 83818
    dedup:
      agents: 4
      tokens: 709531
    titles:
      agents: 1
      tokens: 107000
inputs_consumed:
- review-all-reify-20261010
- hotspot-survey-reify-20261015
- codebook@0e60fa138d8852e1e26999d6dd22d05964848740
- review-all-reify-20261010 (plans/quality-metrics snapshot)
- legacy:docs/notes/bug-hotspot-survey-2026-07-05.md
extra:
  project_id: reify
  mode: full
  full_reason: --since none (no prior committed review-*.json; legacy phase1-/phase2-/summary- files carry no as_of_sha)
  scope: full
  phases_run:
  - 1
  - 2
  - 3
  findings_only: false
  launched_from: null
  worktree:
    path: /home/leo/src/warm-lanes/personal/review-reify-20261015
    seeded_from: /home/leo/src/warm-lanes/base/target.gen.588 (reflink)
    removed: false
  quality_doc: reify carries no docs/code-quality.md; every judging seat read /home/leo/src/dark-factory/docs/code-quality.md
    (the normative copy guidance() renders) in full
  dedup_method: "\xA78 step 1 via a paginated get_tasks scan over the fused-memory HTTP MCP (9128 tasks; 359 review-all keys\
    \ on 331 tasks) per the review-all overlay ruling of 2026-10-12; step 2 via search_tasks>=0.6 confirmed by get_task in\
    \ four sonnet seats; step 3 the curator"
  audit:
    ran: report-only (no filing/escalation; reify-audit release binary from the main checkout, --since 2026-10-01, --no-jcodemunch)
    default_sweep: "timed out at 100 min (exit 124) \u2014 P1/P5 not examined; single-pattern runs P2/PTODO/PDSSENTINEL/PDIAG/PDOCCOVER/PDCHECK/PLINECITE\
      \ each exit 0"
    counts:
      P2ConsumerStub: 52
      PDiag: 1
      PCITE: 14
      PPRDSTATUS: 44
      PTODO: 0
      PDSSENTINEL: 0
      PDOCCOVER: 0
      PDCHECK: skipped (no tasks.db in the pinned tree)
      PLINECITE: 0
    note: "audit findings folded as counts only; the /audit skill's own routing was not applied, so none were filed or escalated\
      \ by this run \u2014 run /audit attended to route the 44 PPRDSTATUS High and 14 PCITE rows"
  hotspot_join: hotspot-survey-reify-20261015 pinned the same as_of_sha; 0 of its 88 keys coincide with this run's 114 new
    keys; 14 coincide with review-all keys (carried with task-store dispositions)
  briefing_defects:
  - '[GUI1-tauri-backend] gui known_gaps omits the AI write-confinement check/write race that docs/debug-mcp-contract.md states
    as a residual (''a directory swapped for a symlink between them can redirect the write''); no task owns it.'
  - "[GUI1-tauri-backend] gui key_scenarios 'The Claude chat panel drives the sidecar \u2026 under landlock' does not say\
    \ that the landlock workspace is fixed at first sidecar spawn, or that the reify-debug AI tool surface is registered only\
    \ in REIFY_DEBUG=1 launches whose bind succeeded (#7816, esc-7049-7). A reviewer cannot tell whether AI write tools belong\
    \ to the release scenario."
  - "[GUI1-tauri-backend] gui purpose says 'Every engine-touching Tauri command \u2026 goes through an EvalQueue'. That is\
    \ true for Tauri commands, but the debug server's engine closures (including #7816's confined_write_target read) still\
    \ bypass it. known_gaps covers this only through '#6351/#6352', while the actual owner is now S25 \u03C1 #8954 (which\
    \ subsumed #7854)."
  - '[TO-tooling] tooling.key_scenarios: ''reify.toml is parsed by reify-config and consumed by reify-eval (...), reify-compiler
    (...) and the CLI cache'' is false. No binary loads reify.toml (fk-677d67cb2ac8, #8764). The CLI cache reads ~/.config/reify/config.toml
    and <cwd>/.reify/config.toml via reify_config::cache, not reify.toml.'
  - "[TO-tooling] tooling.key_decisions: 'CliToolContext does not support geometry export \u2014 reify_export always errors\
    \ in headless mode' and purpose's 'mcp-server' subcommand describe code that in-progress #6665 deletes; #6665's files\
    \ include review/briefing.yaml."
  - '[TO-tooling] tooling.key_scenarios: ''`reify test` ... fails only on Fail'' records today''s behaviour, but #6793 (pending)
    carries Leo''s ruling that @test Indeterminate exits non-zero unless annotated. The scenario will invert when it lands.'
  - '[TO-tooling] tooling.what_working_means has no line for the CLI and GUI sharing one persistent FEA cache, which the shared
    resolver in reify-config exists for. The CLI and GUI resolve it differently today (see the cache-resolver finding).'
  - '[FI2-warm-lanes-infra-tests] The factory-infra key_scenarios docs-only line still carries ''TEMPORARY (Leo 2026-09-26):
    ... route docs through the merge queue on a planning_mode docs-vehicle task''. CLAUDE.md now says a docs-only direct commit
    on main is fine at any time since DF 5070 (it costs an in-flight merge one re-verify), so that briefing clause is stale.'
  - '[FI2-warm-lanes-infra-tests] key_scenarios names scripts/warm-lane-audit.sh''s stash_entries as the backstop but says
    nothing of its degrade-to-0 semantics. key_decisions says ''reify ships the primitive, dark-factory wires the invocation''
    but has no row for primitives whose invocation was never wired: warm-lane-source-integrity.sh (unowned) and warm-lane-lock-guard.sh
    (DF 3071 pending).'
  - '[LP1-compiler-front] assemblies.key_scenarios[0] says a derived sub has ''ZERO runtime effect''. Measured: `reify check`
    fails with ''sub-component "b" references unknown structure ""'' (exit 1).'
  - '[LP1-compiler-front] language-pipeline.known_gaps ''reify-syntax''s transient pub use reify_ast::* re-export'' has tracking:
    null, but #8552 (pending, S7-adopted, carries fk-39797230491b) owns it.'
  - "[LP1-compiler-front] assemblies.known_gaps cites #6617/#6618 as deferred and #6616 as pending. #6616 is still pending,\
    \ but follow-up #8915 already describes A-\u03B2's E_DERIVED_SUB_UNSUPPORTED rejections as if designed. The note 'compiler\
    \ lowering not landed' still holds (no derived_sub_check.rs in the tree)."
  - "[KM-kinematics] kinematics-and-motion known_gaps[0] is stale. It says input_shape returns the input profile unchanged\
    \ and that amplitude_sum/trailing_time/convolve_at are consumed only by tests. In fact trampoline::input_shape_value's\
    \ impulse arm really shapes on the engine path (convolve_at and trailing_time have production callers at trampoline.rs:758,799).\
    \ The echo survives only on the value-eval (check/LSP) path in input_shape::eval_input_shape. The 'confirm a live task\
    \ id' tracker should be S6 \u03C5 #8849."
  - '[KM-kinematics] key_scenarios: ''simulate_trajectory runs FK + inverse dynamics + modal superposition end to end'' overstates
    what runs. FK is q-independent over identity placeholder links (simulate.rs:493-553; #8881), so the briefing should record
    this as a known_gap.'
  - '[KM-kinematics] key_scenarios: ''gcode_import ... returns List<MotionProfile> ...; a Klipper INPUT_SHAPER directive is
    a warning'' is false at the user surface. No warning is ever emitted, and the result is a List of Maps that no Profile
    function evaluates (#7327). Add a known_gap.'
  - '[KM-kinematics] known_gaps has no entry for the S6 stream (docs/prds/v0_6/kinematics-typed-kinds-and-mechanism-record.md,
    leaves #8828-#8856), which owns most of this area''s open cost. A reviewer could otherwise re-report joint-kind string
    dispatch or loop-closure record findings as new.'
  - '[KM-kinematics] Slice scope: crates/reify-stdlib/src/orientation.rs (rotation builtins) and the flexures/ tree sit in
    this area''s slice, but the briefing''s purpose and scenarios never mention orientation or compliant joints. Either add
    a scenario for them or move them to constraints-and-geometry.'
  - '[FI1-verify-pipeline] factory-infra key_scenarios[2] still carries ''TEMPORARY (Leo 2026-09-26): while the train CAS-retry
    fix is unlanded, route docs through the merge queue on a planning_mode docs-vehicle task''. CLAUDE.md (Landing on main)
    and memory feedback_never_direct_commit_main_while_queue_live say that SOP was retired once dark-factory task 5070 landed:
    a docs direct commit is fine any time and costs an in-flight merge one re-verify. The scenario line should drop the TEMPORARY
    clause.'
  - '[FI1-verify-pipeline] factory-infra purpose/key_scenarios do not mention the verify instrumentation ledger (scripts/verify-ledger.py,
    #7424, landed 2026-10-15). It is now a verify-pipeline path, invoked at run-begin, cargo boundaries and _verify_cleanup,
    and writes data/verify-logs/verify-runs.jsonl. Worth one what_working_means line: it is fail-open and must never change
    a verify outcome.'
  - "[CG2-kernels] constraints-and-geometry.known_gaps 'OpenVDB voxel-Boolean execute() ... stubbed' has tracking: null, which\
    \ quality-findings-contract \xA77 counts as a defect (known_gaps keep only a pointer to an accepting task). Its text also\
    \ understates the stub: VOXEL_BOOL_STUB_MSG is returned for every non-Surface/ApplyTransform execute op and for every\
    \ GeometryQuery, not only for voxel Booleans on two grids."
  - "[CG2-kernels] constraints-and-geometry.key_decisions 'Each kernel is detected at build time and fails OPEN to stubs'\
    \ will be false once S10 \u03B2\u2081 #8935 lands (Leo R5: native deps fail closed, stub arms deleted). The briefing should\
    \ point at kernel-interface-typed-seam.md \u03B2\u2081 so a reviewer does not treat the stub arms as sanctioned design."
  - '[CG2-kernels] constraints-and-geometry.known_gaps ''#4263 (pending) Manifold cross-kernel selector wave'' does not mention
    that Manifold''s AdjacentFaces/SharedEdges work in a different face-index space from its extract_faces (raw triangles
    vs coalesced planar faces), so ''selector coverage'' on Manifold is not one gap but two.'
  - "[RA-reify-audit] The reify-audit key_scenarios omit the dark-factory pre-done hook path (reify-audit-predone-wrapper.sh\
    \ \u2192 --tasks-file snapshot \u2192 --task <id> --pre-done \u2192 exit-code refusal of the done-flip). It runs on every\
    \ done transition and is the crate's most operationally consequential consumer."
  - '[RA-reify-audit] The reify-audit known_gaps do not record that P1''s non-test-caller suppression is structurally dead
    (filter_refs_to_file, #6504 pending and unmerged), although that defect breaks key scenario 1.'
  - '[RA-reify-audit] what_working_means says ''P1/P2/P5 findings match the /audit skill''s severity routing (High escalate,
    Medium file, Low log)''. The skill''s routing says P5 never reaches Medium, while four P5 patterns emit Medium (#7734
    pending). The briefing asserts as working something that is not.'
  - "[RA-reify-audit] The key_decision 'PTODO ... read-only sqlite' is not reconciled with CLAUDE.md \xA7Memory & tasks, which\
    \ says direct tasks.db reads are wrong while fused-memory is up."
  - '[GUI2-frontend] gui key_scenarios omit the LSP editor affordances that carry most recent churn (F2 rename, Shift+F12
    find uses, version-skew guard; tasks 7510/7512 and follow-ups #8596/#8603/#8604), so critical-path tracing has no scenario
    line for them.'
  - '[GUI2-frontend] gui known_gaps do not record that the gui/sidecar vitest suite is not gated (#7815), even though what_working_means
    names scripts/gui-test.sh green as the frontend bar and that runner also excludes the sidecar.'
  - '[GUI2-frontend] gui known_gaps do not record the live chat defects: the permission-decision envelope (S26 alpha #9062),
    and the message-id correlation break found in this run.'
  - '[LP3-core-ir-expr] language-pipeline key_decisions (reify-runtime deletion entry) says the crate is ''consumed by reify-ir,
    reify-cli dev and the kernels'' warm_register sites''. reify-ir and the kernels only mention reify_runtime in doc comments:
    reify-runtime depends on reify-eval, so reify-ir cannot depend on it. The sole production consumer is reify-cli/src/dev.rs::render_inspection
    (display only), and reify-eval takes it only as a dev-dependency.'
  - "[LP3-core-ir-expr] language-pipeline known_gaps lists #5023 (async-recalc Phase A) as 'deferred \u2014 confirm its flip\
    \ trigger'. #5023 is now pending."
  - '[LP3-core-ir-expr] conventions/TODO tracking: the briefing treats a cite as tracking, but #7151 shows a cited task can
    be closed by a comment-only landing whose own metadata forbids it. The PTODO liveness lane should have reported units.rs:397
    as orphaned; worth a note that liveness findings route to DF sweeps rather than the ratchet, so an orphaned high-priority
    cite can sit on main unflagged.'
  - '[AN1-solvers] The analysis key_scenarios FDM line claims refinement to R0 when PrusaSlicer is on PATH. R0 is unreachable
    from any .ri file today: as_printed_material routes only to r_fast, and select_rungs has no production caller. The scenario
    should say ''R0 pending #7087''.'
  - '[AN1-solvers] The analysis key_scenarios shell-extraction line ends in ''shell mesh with a shell/tet interface partition''.
    partition_body has no production caller, and the mixed-region consumer is dead_code pending #6371.'
  - '[AN1-solvers] Analysis known_gaps row GR-041/GR-024 (''buckling K_g'') does not record that the shell/hex/wedge K_g stubs''
    promised diagnostics (task #3455, done) were never built. No DiagnosticCode E_BucklingShellNotImplemented / E_BucklingHexWedgeNotImplemented
    exists.'
  - '[AN1-solvers] The analysis subproject has no stability_concerns key, so the deep-read prioritisation step had nothing
    to draw on.'
  - "[AN1-solvers] what_working_means says the reify-eval-fea-tests suite is 'engine-driven \u2026 the proof of user reachability',\
    \ but 5 of its 33 files are trampoline-direct with hand-built Values."
  - "[LP2-eval-core] language-pipeline known_gaps entry tracking '#5023 (deferred \u2014 confirm its flip trigger)' is stale.\
    \ #5023's live status is pending, and the S3 ruling in plans/review-all-program-reify-20261012.md deletes propagate_freshness_only,\
    \ freshness_walk.rs and the gating.rs dead code rather than keeping them retained 'awaiting that phase'. The entry should\
    \ point at S3 \u03B6 #8699."
  - '[LP2-eval-core] The language-pipeline subproject has no stability_concerns key, although the brief''s deep-read ordering
    relies on it.'
  - '[LP2-eval-core] language-pipeline key_scenarios do not cover `reify test`, the only CLI surface that runs trampolines
    and reports verdicts. That path has a known exit-0 vacuity for geometry-bearing @test blocks (#6781), and no scenario
    line draws a reviewer to it.'
  - "[AN2-compute-targets] analysis.key_scenarios[5] states that as_printed_material 'refines to R0 when PrusaSlicer is on\
    \ PATH'. No production path does this: no .ri declares fdm::as_printed_material_r0, and only tests call select_rungs and\
    \ rung_compute_key. Move it to known_gaps (owner #7087 / S5 \u03BA\u2082 #9001) until it is wired."
  - '[AN2-compute-targets] analysis.key_scenarios[1] names ''z_dominant_frequencies'' as if it were a product capability.
    It is a test helper in crates/reify-eval-fea-tests/tests/modal_analysis_e2e.rs:698.'
  - '[AN2-compute-targets] analysis.known_gaps omits the modal Selector-location gap. ModalResult.topology is always Undef
    in production, so displacement_at / StepForce.at with a Selector silently resolve to the fundamental antinode (#7097 owns
    the body path only).'
  - "[AN2-compute-targets] analysis.key_decisions 'inverse_dynamics_at_snapshot \u2026 comment still says deferred to task\
    \ 4146' could not be checked within budget; dynamics/eval.rs was not deep-read."
  - '[CG1-constraints-geometry] constraints-and-geometry known_gaps ''Cross-sub clearance constraints are UNPOSED'' tracks
    #6583, and the key_scenarios line ''Declarative engineering libs evaluate ... cross-sub clearance is UNPOSED, #6583''
    cites it too. #6583 is CANCELLED as a curator duplicate of #5436 (+ #5483), both pending (docs/prds/v0_6/geometry-algebra-solver-unification.md:161;
    e3-decomposition-granularity-ab.md:474). Re-point both lines to #5436/#5483.'
  - '[CG1-constraints-geometry] constraints-and-geometry key_decisions ''CpSatSolver ... SolverRegistry is designed for substitution''
    is accurate, but readers should know that production() now fills every slot (registry.rs:313-320: SolveSpace geometric,
    CpSat logical, DiscreteFirstFallback cross-domain). Some pending task records (#6681) still describe the logical/fallback
    slots as None.'
  - 'smoke seat: subproject names in review/briefing.yaml what_working_means blocks had to be inferred by content order (seat
    note)'
  instrument_gaps:
  - '[GUI1-tauri-backend] No cargo/npm run, per the brief (Phase 1 holds the locks), so no runtime confirmation of the sidecar-workspace
    or confinement-race findings.'
  - '[GUI1-tauri-backend] list_tickets defaults to a 7-day window, and the tkt_ pointers in commands.rs/engine.rs/large_stack.rs
    are older, so whether they became tasks was judged only via search_tasks paraphrase (no hits).'
  - '[GUI1-tauri-backend] gtool_orphans_in_slice was empty, so the dead-code judgment rests on manual grep (InboundMessage::ToolResult).'
  - '[TO-tooling] No cargo build/test was run (Phase 1 holds the locks), so runtime claims (e.g. the -o fallthrough, cmd_test
    stderr content) come from static reading only.'
  - '[TO-tooling] stub_hits came only from TODO/placeholder/unimplemented text patterns. The doc-formatter em-dash columns
    were found that way, but constant-return placeholder bodies without those words would be missed.'
  - '[TO-tooling] gtool_orphans_in_slice lists cfg(test-support) helpers (blocking_work test_support, RecordingSink take_calls/probe_state/take_log_calls)
    and G-allowed public LSP wrappers as orphans. None were genuine dead-code candidates; reify-config default_cache_dir/parse_cache_config
    have in-crate production callers (resolve_cache, load_cache_config_from_path).'
  - '[TO-tooling] Task-store dedup used search_tasks at a 0.5-0.6 threshold; a finding owned by a task with dissimilar wording
    could be missed.'
  - '[FI2-warm-lanes-infra-tests] Per the brief, no tests were run (no cargo or npm in the pinned tree), so every claim is
    static, plus a read-only listing of the live /home/leo/src/warm-lanes/base pool dir.'
  - '[FI2-warm-lanes-infra-tests] The cross-repo wiring checks (dark-factory git_ops.py, orchestrator/src, shared) were run
    against the dark-factory working tree at its current HEAD, not a pinned SHA.'
  - '[FI2-warm-lanes-infra-tests] The G-tool orphan list flags build.rs-only consumers of the reify-build-utils pub fns as
    orphans. All 5 in this slice are false positives.'
  - '[LP1-compiler-front] No cargo build or test was run in the pinned tree, per the brief. Behavioural probes used the pinned
    tree''s prebuilt target/release/reify (dated 2026-10-13), which predates the 2026-10-14 PortPath commits. Every probed
    behaviour was cross-checked against the HEAD source, which has the same deferred-path and as-i64 logic.'
  - '[LP1-compiler-front] fused-memory find_tasks_by_metadata (key lookup) is not used here. Dedup relied on the slice''s
    task_store, the program FILED table and search_tasks.'
  - "[LP1-compiler-front] stub_hits are dominated by the word 'placeholder' in prose and identifiers (pattern-text). The 'RED\
    \ \u2014 todo!() panics' TDD banners in units.rs tests are stale prose, not stubs, and are covered by the step-N label\
    \ ratchet #7353."
  - '[KM-kinematics] The gtool orphan report counts only cross-file references. Of its 35 rows in this slice, most are false
    positives: same-file production helpers (all simulate.rs, trampoline.rs and tots.rs internals except code_str), crate-root
    re-exports consumed by reify-expr (orientation::diagnose, snapshot::diagnose), newton_solve_with_projection (called by
    solve_loop_closure), lower_gcode, chain_transform, value_for_joint and pose_link. Genuine candidates: worst_case_residual_fraction
    (#9087), per_joint_jacobian_local (#8833), code_str (#8546), resample_cubic (new), and newton_solve (test-only plus re-export;
    #8560).'
  - '[KM-kinematics] INV-PD-2 names `reify-audit --pattern PVAC` as its enforcement, but git grep finds no PVAC in crates/reify-audit/src
    at 0e60fa138d. The EndEffectorTrack sentinel fields therefore have no mechanical detector.'
  - "[KM-kinematics] No runtime probing was done: cargo build/test and the reify binary are forbidden in the pinned tree.\
    \ The profile t0 \u2260 0 defect and the gcode NaN acceptance rest on static reading of the spline, sampling and f64::from_str\
    \ semantics, not on executed probes."
  - '[FI1-verify-pipeline] Ran no tests or cargo/npm, and did not run verify-pipeline-guard.sh --list (it forks verify.sh
    --print-plan). Every guard claim is from static reading.'
  - '[FI1-verify-pipeline] Did not reproduce the conflicted-merge path with a real git merge (read-only constraint). The finding
    rests on githooks(5) semantics and the code read.'
  - '[FI1-verify-pipeline] Did not run search_tasks for re-observations. Their task_store dispositions came from the slice
    file, with statuses refreshed via get_statuses.'
  - '[FI1-verify-pipeline] Stub audit: verify.sh:4394 carries `ERROR(#4624)` in a diagnostic string. #4624 is done, but this
    is an error-message provenance cite, not a TODO/FIXME/ignore marker, so it was counted as cited and not reported.'
  - '[FI1-verify-pipeline] gtool_orphans_in_slice and codebook_rows were empty for this slice, so there was no dead-code judgement
    to make.'
  - '[CG2-kernels] No runtime execution in this seat (cargo/reify forbidden while Phase 1 holds the locks). The OpenVDB voxel-Boolean
    routing and the Manifold adjacency index mismatch are static traces, not reproduced.'
  - "[CG2-kernels] stub_hits (8) are all pattern-text. The real unintended stub in the slice (OpenVDB execute/query catch-all,\
    \ VOXEL_BOOL_STUB_MSG) was not among them, because the hit scanner keys on TODO/placeholder/todo!() words and misses stub-shaped\
    \ Err-returning bodies whose owner is a malformed prose cite ('task \u03B5')."
  - '[CG2-kernels] The G-tool''s new_vs_baseline flags extern "C" FFI declarations (16 of the 27 new rows) as orphans. Those
    are alive by rule and are noise for this slice.'
  - '[RA-reify-audit] No cargo build/test was run (Phase 1 holds the locks), so P1''s dead-suppression and runs.db-Medium
    claims rest on code reading, not on a run.'
  - '[RA-reify-audit] The number of `merged` done tasks that a degraded runs.db would turn into per-task Mediums was not counted:
    raw tasks.db reads are prohibited and no corpus query was made through fused-memory.'
  - '[RA-reify-audit] jcodemunch was not live, so the cross-file find_references behaviour is taken from #6504''s recorded
    live measurement (2026-08-24), not re-measured.'
  - '[RA-reify-audit] Whether fused-memory''s get_task returns subtask ids as ''N.M'', which the pre-done subtask-miss claim
    depends on, was not verified against a live server; that sub-claim is stated as a divergence, not as a measured failure.'
  - '[GUI2-frontend] gtool_orphans_in_slice was empty, so dead-code judgment used an ad-hoc word-match export-usage scan over
    gui/ (production vs test importers). It counts comment mentions as uses, so it under-reports; MultiCaseResult was caught
    only through its stub hit.'
  - '[GUI2-frontend] No app run or vitest run (forbidden in the pinned tree). The URI percent-encoding claim rests on url-crate
    parse behaviour that the Editor''s own docblock and test fixtures already assume. It was not reproduced.'
  - '[GUI2-frontend] The chat id-mismatch consequence (tool cards never render, streaming cursor stays on) is inferred from
    MessageGroup''s props and was not observed in the GUI.'
  - '[GUI2-frontend] The gui/sidecar vitest suite (session.test.ts and 7 others) never runs at the merge gate (verify.sh sidecar
    lane is typecheck-only; gui/vitest.config.ts excludes sidecar/**). Already owned by #7815, so not reported, but every
    sidecar finding''s test signal (e.g. S26 zeta #9067) is ungated until it lands.'
  - '[LP3-core-ir-expr] No cargo build/test was run (brief prohibition). The misparse repro used the pinned tree''s existing
    target/release/reify, built 2026-10-13, two days before the pin; grammar.js and ast.rs are unchanged in the interval per
    git log.'
  - '[LP3-core-ir-expr] G-tool orphan counts include doc-comment and same-file references. Production-consumer judgments were
    made by per-symbol git grep with comment lines filtered and tests/ paths excluded, not by a call-graph tool.'
  - '[LP3-core-ir-expr] search_tasks is semantic. Ownership for the dual_eval differentiability tables and _test_at_depth
    was inferred from PRD text, not from a task record naming them.'
  - '[LP3-core-ir-expr] The design-invariants audit covered SF-1, SF-5, SF-6, SF-7 and the umbrella principle against the
    slice. AD-1..4, PD-1..2 and ARCH-1 were not walked beyond what the owned review-all findings already record (ARCH-1 for
    the reify-ir SCC is owned by #8930).'
  - '[AN1-solvers] Static reading only, per the brief. No cargo build or test was run, so I did not verify that the ''linear-solve-fault-injection''
    self dev-dependency compiles the armed hooks into other workspace test binaries (the PRD itself marks this unverified).'
  - '[AN1-solvers] The G-tool counts callers in the defining file as non-callers. compose_morph, assemble_stiffness, is_stalled,
    tet_shape_quality, mark_dorfler, compute_dual_weighted_indicator and marking_weights all have live in-file production
    callers and were judged alive. classify_grid_misses carries a G-allow the tool missed. The fault_injection rows are feature-gated
    test API, ruled by PRD C2.'
  - '[AN1-solvers] search_tasks at score_threshold 0.6 missed obvious owners. A paraphrase of the form-find sign-rule duplication
    did not return #8628, so task_hits empty does not mean unowned.'
  - '[AN1-solvers] PTODO cannot see placeholder promises written as prose (''will replace this placeholder'', ''not implemented''
    in panic! text). Only todo!/unimplemented!/TODO markers are gated.'
  - '[AN1-solvers] A git log --grep over the whole history timed out once (backgrounded); I re-ran it bounded with --since.'
  - '[LP2-eval-core] No runtime reproduction: cargo build/test is forbidden in the pinned tree, so the empty-spacing h-derivation
    divergence in the measure_min copies is a static read, not observed behaviour.'
  - "[LP2-eval-core] search_tasks was run at score_threshold 0.5, not 0.6, for recall. Every hit is reported with its score,\
    \ and none of the new findings has a hit \u22650.6 that owns its exact cost; #8298 at 0.618 owns the on_refine_trigger\
    \ consumer."
  - '[LP2-eval-core] G-tool orphan reference counts came from git grep on the bare symbol name with comment lines excluded.
    Method calls through self. are counted, but cfg(test)-only callers inside the same file were told apart from production
    callers by hand only for the candidates reported.'
  - '[LP2-eval-core] Stub classification counted every hit inside a #[cfg(test)] module as ''fixture''. Most of the 52 are
    the word ''placeholder'' in test names or test prose, not fixture data.'
  - '[AN2-compute-targets] No cargo build/test was run, per the brief. Behavioural claims rest on static reading, except for
    one `reify check` probe with the pinned tree''s target/release/reify. That binary was built 2026-10-13, possibly from
    a commit earlier than 0e60fa138d.'
  - '[AN2-compute-targets] The G-tool orphan list counts in-file production callers as orphans. 8 of the 13 new_vs_baseline
    / zero-external-ref candidates in this slice are alive through same-file callers or fn-pointer registration.'
  - '[AN2-compute-targets] search_tasks recall on comment-rot and dead-code items is weak; an empty result is not proof of
    no owner.'
  - '[AN2-compute-targets] The stub-hit scan covers only the words placeholder/TODO/RED. Production placeholders without those
    words, such as build_modal_topology_value''s unconditional Undef, are found only by reading.'
  - '[CG1-constraints-geometry] stub_hits are keyword hits on the word ''placeholder'' plus unimplemented!/TODO. None of the
    48 is an untracked marker or an unintended stub. The one production soft-stub that a keyword scan cannot see (helpers.rs
    validate_selector_target accepting any String/Map as an FEA selector ''placeholder'' with prose-only ''PRD task 16'' /
    ''task 5'' cites) lives in supports.rs''s consumer path, outside this slice, and was not filed.'
  - '[CG1-constraints-geometry] Most gtool_orphans_in_slice are live: pub(crate) helpers with in-file callers, pub API consumed
    through lib.rs re-exports, or inventory/FFI. The dual_jacobian exports (residual_jacobian, Jacobian::differs_from) and
    solve_sketch have only test callers today but are awaited by pending consumers #6675/#6679/#6680 and #5509/#5512, so they
    were not reported as dead.'
  - '[CG1-constraints-geometry] codebook_rows was empty for this slice, so there were no confusion-dense anchors to prioritise.'
  - '[CG1-constraints-geometry] No cargo build or test was run (static reading only, per the brief), so behavioural claims
    such as asin(0.5mm) and the swept-mesh offset are derived from code paths, not executed probes.'
  - '[CG1-constraints-geometry] The fused-memory task search tops out around 0.5-0.66 cosine for these statements. Ownership
    was confirmed by reading task records (get_task #6681) where a near-match existed.'
  critical_paths:
  - slice: GUI1-tauri-backend
    scenario: Opening a .ri file shows source and tessellated geometry; editing re-evaluates via delta events; status bar
      idle
    traced: true
    outcome: "OK on the command path. open_file_engine \u2192 commands::open_file_evaluation (EvalRequest::snapshot) \u2192\
      \ open_file_engine_impl \u2192 load_initial_file_impl \u2192 load_file_into_engine \u2192 UnresolvedGuiState::resolve,\
      \ then SnapshotPublisher.publish \u2192 TauriEvalObserver.delta/activity ('evaluating'/'idle'), then watch_file. Editor\
      \ edits: update_source \u2192 editor_source_edit (EditIdentity::editor_source) \u2192 reload_for_watch_impl. Residuals:\
      \ the debug-server open bypasses the queue (fk-7212abff873d, owned by \u03C1 #8954), and the evaluation-status phase\
      \ is a String (S26)."
  - slice: GUI1-tauri-backend
    scenario: Property-editor edits trigger constraint re-check and viewport update
    traced: true
    outcome: "OK. set_parameter \u2192 commit_parameter_edit (EditIdentity::commit) \u2192 set_parameter_impl \u2192 EngineSession::commit_parameter.\
      \ A refusal publishes the restored state (RefusedParameterWrite). preview_parameter takes the transient path. The re-check\
      \ over DTO strings is fk-995c30a8a446 (\u03B8 #8944)."
  - slice: GUI1-tauri-backend
    scenario: Rapid edits supersede queued evaluations; the running one still completes
    traced: true
    outcome: "OK. EvalQueue::accept \u2192 EditLedger::admit refuses late stamps \u2192 QueueState::supersede_queued resolves\
      \ queued same-target edits Ok(()) unrun. The running entry is untouched, which is the #5215 known gap. Barriers (evaluations,\
      \ engine calls) are never jumped."
  - slice: GUI1-tauri-backend
    scenario: The Claude chat panel drives the sidecar (--resume continuity) under landlock
    traced: true
    outcome: "GAP. The landlock workspace is resolved per message, but ensure_sidecar_ready's Ready fast path never respawns,\
      \ so the writable directory stays the one from the first message (finding on main.rs::claude_send_message). The inbound\
      \ tool_result arm is dead (finding on claude_bridge.rs::InboundMessage). The permission-decision flat-args defect is\
      \ already S26 \u03B1 #9062."
  - slice: GUI1-tauri-backend
    scenario: REIFY_DEBUG=1 opens the debug MCP listener on 127.0.0.1:${REIFY_DEBUG_PORT:-3939}; screenshot_window captures
      the viewport
    traced: true
    outcome: "OK. bind_debug_listener (before the Builder) \u2192 DebugMcpEndpoint(Some(url)) \u2192 spawn_debug_server(listener)\
      \ via TcpListener::from_std inside async_runtime::spawn, and the sidecar gets REIFY_DEBUG_MCP_URL only when bound. A\
      \ failed bind is eprintln-only by design. screenshot_window \u2192 handle_image_tool \u2192 DebugBridge::query_frontend\
      \ (5s timeout)."
  - slice: GUI1-tauri-backend
    scenario: "Viewport click \u2192 editor scroll; diagnostics-panel row \u2192 jump-to-line incl. cross-file open"
    traced: false
    outcome: The Rust side is only open_file_impl (a canonicalised fs read) and get_source_location/get_entity_at_source_location
      engine_calls, all present and queue-ordered. Scroll and fly-to are frontend-only and outside this slice.
  - slice: GUI1-tauri-backend
    scenario: FS watcher hot reload
    traced: true
    outcome: "OK. create_watcher callback \u2192 evals.submit(disk_reload_edit) (not awaited) \u2192 reload_for_watch_if_changed_impl,\
      \ which skips its own echo via reload_would_be_a_no_op. Trailing-edge debounce covers non-atomic writes."
  - slice: TO-tooling
    scenario: '`reify check` validates constraints, exits non-zero on violations, 0 on Indeterminate unless --strict; realizes
      geometry only when needed'
    traced: true
    outcome: 'main dispatches to cmd_check, then parse_and_compile_with_cfg (follows cfg-gated user imports), then finish_check/check_fails.
      Exit decisions match the contract: SomeViolated fails, SomeIndeterminate fails only under --strict. The Error-exit allowlist
      (CHECK_ERROR_EXIT_ALLOWLIST, #5404) and the double evaluation (#5973 known_gap) remain owned. The --purpose branch disagreement
      is fk-c24f5d7302ed (#9022/#9023). No new gap.'
  - slice: TO-tooling
    scenario: '`reify build --out-dir` exports one STEP per product body, --verbose prints provenance; `-o` dumps every body
      into one file'
    traced: true
    outcome: 'Partially traced through flag parsing, compile and Engine::with_registered_kernel + register_compute_trampolines
      into the -o format dispatch. Two gaps. (1) `-o` given as the last argument with no value gives o_value_pos=None, so
      the command silently runs declarative mode, whereas `--out-dir` without a value warns. (2) An unknown -o suffix (e.g.
      out.obj) writes STEP bytes with only an unprefixed stderr note and exits 0; already owned by #7808. The declarative
      Output-occurrence loop was not read in full.'
  - slice: TO-tooling
    scenario: '`reify test` counts Pass/Fail/Indeterminate and fails only on Fail; flip a constraint FALSE to tell a verdict
      from a shrug'
    traced: true
    outcome: 'Path: cmd_test, parse_and_compile, reify_eval::run_tests, build_test_engine (trampolines registered, no solver),
      Engine::check. The exit contract holds, but TestResult.diagnostics and indeterminate reasons are never printed, so a
      shrug cannot be told from an error. Reported as a finding on main.rs::cmd_test.'
  - slice: TO-tooling
    scenario: LSP live diagnostics, hover, F2 rename (version-stamped edits), Shift+F12 references, document symbols; in-process
      for the GUI
    traced: true
    outcome: Traced the handlers in server.rs. did_open/did_change go through evaluate_and_forward_logs to the sink. Rename
      snapshots versions under the same read lock as the text, then version_stamped_workspace_edit converts changes to documentChanges,
      which is coherent. Hover and completion compile on the async worker (fk-1a772d160168), and goto_def and prepare_rename
      inline the rig (fk-74ce7cce677f). Symbols use symbols.rs via decl_name_and_span. No new wiring gap; bridge.rs handle_request
      was not re-traced (fk-7b9fad8a5d00 owned).
  - slice: TO-tooling
    scenario: An LLM reads/modifies designs via MCP and looks up syntax in the language-reference chunks
    traced: true
    outcome: 'Traced McpServer::run_on_streams through McpDispatcher::dispatch, registry.call_tool and the tools/read.rs closures.
      Tool errors return isError:true in the result body, as required. A notification yields a -32700 reply (fk-bed8cadb429f,
      #8765). The CLI host CliToolContext is being deleted by in-progress #6665; the GUI host is out of slice. Chunk freshness
      was not reviewed.'
  - slice: TO-tooling
    scenario: reify.toml is parsed by reify-config and consumed by reify-eval, reify-compiler and the CLI cache
    traced: true
    outcome: 'No production loader exists (fk-677d67cb2ac8, #8764). The CLI cache does not read reify.toml at all; it reads
      ~/.config/reify/config.toml and <cwd>/.reify/config.toml, and the GUI reads neither. The scenario line is false as written;
      see briefing_defects and the cache-resolver finding.'
  - slice: TO-tooling
    scenario: '`reify doc <file>.ri` emits faithful Markdown/HTML/JSON reference output incl. generic type params, alphabetic
      meta ordering'
    traced: true
    outcome: 'Path: cmd_doc, reify_doc_build build_doc_model + build_cross_refs, fmt_markdown/fmt_html render. render_meta
      sorts keys; CrossRefIndex uses BTreeMap, so output is deterministic. Not faithful in two places: Dimension/Kind columns
      are always em-dash (new finding), and stdlib defaults render as `<default>` (fk-10941c98934e, #8767).'
  - slice: FI2-warm-lanes-infra-tests
    scenario: A warm lane is reseeded CoW at ACQUIRE under a lane flock that refuses a live consumer
    traced: true
    outcome: 'seed-warm-lane.sh takes its own flock only under --lane-lock (default on). Dark-factory passes --assume-lane-lock-held
      on both production acquires, and in that mode seed runs zero checks that the lock is held (parser arm at 400). This
      is the existing fk-60aa699ea8ce, owned by #8624 L1. No new gap was found.'
  - slice: FI2-warm-lanes-infra-tests
    scenario: A released lane is thinned unless a live process still references its target/
    traced: true
    outcome: thin-warm-lane.sh takes flock -n on FD 9 (75 on contention), then runs lib_live_refs.sh live_ref_present (75
      if referenced), then rm -rf target. The live-ref scan fails OPEN with a loud stderr warn when /proc is degraded, by
      design. The path is coherent.
  - slice: FI2-warm-lanes-infra-tests
    scenario: Only the _merge-verify lane's landed-commit target/ may advance the shared base
    traced: true
    outcome: GAP. refresh-warm-base.sh checks cleanliness and HEAD==flag only. Every caller passes the advancing worktree's
      own HEAD, so the guard never verifies that the commit landed or which lane is advancing (new finding at scripts/refresh-warm-base.sh,
      h10).
  - slice: FI2-warm-lanes-infra-tests
    scenario: Base build-config match (RUSTFLAGS and invocation) gates a seed
    traced: true
    outcome: GAP. Seed's guard reads .warm-base-meta, which nothing in production writes. The live pool has no such file,
      so the guard compares env against ''. Preflight reads refresh's target.rustflags and target.invocation (new finding
      at _sidecar_path, h11).
  - slice: FI2-warm-lanes-infra-tests
    scenario: scripts/warm-lane-audit.sh's stash_entries field is the backstop that makes a disarmed stash guard visible
    traced: true
    outcome: GAP. On a failed or unresolved stash query the field reports 0, the clean value. One of the three degrade paths
      emits no warn at all, despite its comment (new finding at scripts/warm-lane-audit.sh).
  - slice: FI2-warm-lanes-infra-tests
    scenario: Native-dep absence is caught before cargo (reify-build-utils side)
    traced: true
    outcome: find(), emit_rpath_*, emit_tbb_pin_* and read_soname_version are fail-soft by design. All five G-tool orphan
      candidates have build.rs consumers (reify-cli, reify-config, gui/src-tauri, reify-kernel-occt). The rerun-env gap is
      existing fk-765ff49d2de5 (#8646). check-manifold-deps.sh is outside this slice and was not traced.
  - slice: FI2-warm-lanes-infra-tests
    scenario: tests/infra run_all.sh members run identically on every branch (merge-gate run_all plan line)
    traced: true
    outcome: '#8681 landed one _ra_run_member and _ra_record_result. The new task-7424 per-member timing is applied on the
      pool branch only (new finding at _ra_timing_pool_lines).'
  - slice: LP1-compiler-front
    scenario: 'language-pipeline: A .ri file is parsed, compiled and evaluated (parse -> compile entry)'
    traced: true
    outcome: 'CLI reify check runs reify_syntax::parse, then module_dag::compile_entry_with_stdlib_cfg_checked (stdlib prelude
      + cfg-gated DAG + pub-template merge), and the wiring holds. module_dag also exports compile_project / compile_project_with_entry_source
      / _cfg, which have no production caller (tests only; the GUI uses its own compile_entry_with_imports). They differ in
      semantics: no stdlib, early return on import error, parse errors stripped of spans. Not filed: resolution-unification
      #5516 and S27 own entry-point collapse.'
  - slice: LP1-compiler-front
    scenario: 'language-pipeline: Multi-module compilation via ModuleDag with topological ordering and cycle detection'
    traced: true
    outcome: Cycle detection lives in ModuleDag::compile_module, which both the production bridge and the test-only compile_project
      use, so the property tested is the production one. No gap found.
  - slice: LP1-compiler-front
    scenario: 'language-pipeline: Builtin dispatch: migrated families resolve through reify_builtins::lookup'
    traced: true
    outcome: Wired. The compiler side goes through builtin_registry::registry_result_type (name_group, deliberately arity-insensitive),
      the eval side through reify-stdlib registry_dispatch::dispatch -> reify_builtins::lookup. Remaining legs are S8-owned.
  - slice: LP1-compiler-front
    scenario: 'assemblies: `sub b = mirror of a across P { }` parses into SubDecl.derivation with zero runtime effect'
    traced: true
    outcome: 'The briefing is inaccurate. The derived sub reaches the compiler with structure_name "" and fails `reify check`
      with ''references unknown structure ""'' (finding filed, #6616 owns the real rejection).'
  - slice: LP1-compiler-front
    scenario: 'assemblies: CompiledConnection consumed only by topology-fingerprint hashing and forall-Connect bookkeeping'
    traced: true
    outcome: Confirmed. The only non-test readers of left_port/right_port are ContentHash in entity.rs and reify-eval graph.rs.
      The deferred forall-connect path emits connections with a literal-true compat constraint and no direction or existence
      check (finding filed).
  - slice: LP1-compiler-front
    scenario: 'language-pipeline: Structure-instance runtime: ctor field types conformance-checked'
    traced: false
    outcome: "Not traced. The conformance/ module is heavily owned by S9 (\u03B8/\u03B9/\u03B7) and review-all findings; reading\
      \ budget went to the changed connect/forall/port_path files."
  - slice: KM-kinematics
    scenario: mechanism().body(...).joint(...) composes; snapshot(mech, bindings) returns a pose Map carrying is_singular;
      over/under-constrained and singular cases emit typed diagnostics
    traced: true
    outcome: "The path is wired, but its known defects are all owned by S6 (\u03BA #8837, \u03BE #8842, \u03BC #8840, \u03BD\
      \ #8841). Snapshot re-runs the plain solve when the wrapper reports over-constraint and discards KinematicUnderconstrained.\
      \ is_singular is a magic key that is absent when false (fk-241b66aa0262). No new finding."
  - slice: KM-kinematics
    scenario: sweep(mech, joint, range, steps) warm-starts loop-closure free variables across steps
    traced: true
    outcome: "Width mismatch in the warm-start check: it uses free_b.len() where the solver uses \u03A3flat_len. Fix in progress\
      \ (#8545, fk-337e4f0bcfa9); \u03C0 #8844 redefines the warm start. No new finding."
  - slice: KM-kinematics
    scenario: Closed loops with multi-DOF joints converge (Tikhonov damping on Spherical's redundant quaternion storage)
    traced: true
    outcome: 'Present. SPHERE_TIKHONOV_DAMPING is applied per component in solve_loop_closure (loop_closure_solver.rs:663-680).
      #7497 landed coupling propagation inside the Newton iterate. A chain_a coupling of a free chain_b joint still keeps
      its seed, documented as #8837 (R8).'
  - slice: KM-kinematics
    scenario: 'Parametric shaper design: a piecewise-cubic profile evaluates through the spline (evaluate_profile/_dot/_ddot,
      profile_duration); simulate_trajectory runs FK + inverse dynamics + modal superposition end to end'
    traced: true
    outcome: 'Spline evaluation is wired. simulate_trajectory runs end to end, but its FK is q-independent over identity placeholder
      links, and the EndEffectorTrack runtime shapes don''t match the declaration. A profile whose first waypoint is not at
      t = 0 is mis-sampled. These are new findings at simulate.rs::simulate_trajectory_core, trampoline.rs::track_data_to_value
      and sampling.rs::to_trajectory_samples. Marshalling half owned by #8881.'
  - slice: KM-kinematics
    scenario: 'TOTS: velocity/accel/force limits + a vibration tolerance drive an in-house SQP over B-spline control points
      to a minimum-time feasible solution'
    traced: true
    outcome: "Wired through the input_shape ComputeNode. The force limit reads a nonexistent force_limit field (#5805/#7083).\
      \ The mechanism model is a placeholder (#8881). Infeasible returns a silent Undef (\u03C6 #8850). The check/LSP path\
      \ solves a stand-in model (\u03C5 #8849). No new finding."
  - slice: KM-kinematics
    scenario: 'gcode_import(file, dialect: KlipperDialect | MarlinDialect) returns List<MotionProfile> in SI units; a Klipper
      INPUT_SHAPER directive is a warning, not a parse error'
    traced: true
    outcome: Units are SI. The result is a List of bare Maps that no Profile function accepts (#7327). The INPUT_SHAPER warning
      and the other diagnostics never reach the user, and a parse error is a cause-less Undef (new finding at gcode_import.rs::eval_gcode_import).
      M83 relative extrusion is misread, and non-finite values are accepted (new low findings).
  - slice: FI1-verify-pipeline
    scenario: 'A task branch lands via the merge queue / land.sh: DF_VERIFY_ROLE=merge forces --scope all --profile both through
      hooks/pre-merge-commit, release-only tests execute, sentinel marked only on success'
    traced: true
    outcome: 'land.sh exports DF_VERIFY_ROLE=merge and runs a real git merge --no-ff. pre-merge-commit runs verify.sh all
      --profile both --scope all and then marks the sentinel. land.sh pre-marks before the merge (fk-638f0a64b948, #8671).
      It defaults REIFY_GATE_EXCLUDE_HEAVY=1 by the documented DA5 decision. Gap found: a conflicted hand merge resolved with
      git commit takes pre-commit/project-checks, which runs a debug-only task-role gate, and is still marked sanctioned (new
      finding on hooks/project-checks).'
  - slice: FI1-verify-pipeline
    scenario: Native-dep absence caught by scripts/check-manifold-deps.sh before cargo runs
    traced: true
    outcome: build_plan emits add_tool ./scripts/check-manifold-deps.sh under RUN_RUST=1, after the manifest/harness gates
      and before tree-sitter and every cargo leaf. The gui-feature cargo check is also gated by RUN_RUST=1, so no cargo leaf
      precedes the preflight. The script body was not re-read; it is owned by S21 (#8634/#8646).
  - slice: FI1-verify-pipeline
    scenario: Docs-only change lands directly on main through hooks/pre-commit -> project-checks (no heavy checks)
    traced: true
    outcome: project-checks runs verify.sh all --profile debug --scope staged --include-infra. reify_path_attribution treats
      docs/*, *.md and *.yaml as inert. select_infra_tests still runs on the changed-file list, so Python code under docs/
      that has rows in verify-pipeline-infra-tests.txt (docs/prds/v0_6/e3-granularity/analysis/*.py -> test_e3_analysis.sh)
      is still tested. No gap. The briefing's TEMPORARY routing note is stale (see briefing_defects).
  - slice: FI1-verify-pipeline
    scenario: Unattended writers land via hook-gated git commit --only, never push
    traced: false
    outcome: The writers live in dark-factory (legibility trickle/census), outside this slice. Not traced.
  - slice: FI1-verify-pipeline
    scenario: Warm lane reseeded CoW at ACQUIRE under lane flock; release thins unless live refs; only _merge-verify advances
      base
    traced: false
    outcome: seed-warm-lane.sh, thin-warm-lane.sh and warm-lane-gc.sh are not in this slice's file list. Only verify-ledger.py's
      seed-stamp consumer was read (read-only over <lane>.seed-stamp.json and target/*/.fingerprint).
  - slice: FI1-verify-pipeline
    scenario: 'A verify-pipeline file changes: merge worker consults verify-pipeline-guard.sh requires-full-gate'
    traced: true
    outcome: Traced the guard's static set (verify.sh anchor, manifest rows, verify.sh source lines, emitted .sh paths, infra
      glob, lazy clause 4b) and dark-factory's caller (verify.py::_verify_pipeline_guard_requires_full_gate, used only for
      config-only diffs, which exclude .py/.rs). scripts/reify_path_attribution.py now carries the scope classifier but is
      in no guard clause. The S18 PRD records this as accepted (met by derivation at kappa; a .py never takes the fast path),
      so it is not reported.
  - slice: FI1-verify-pipeline
    scenario: 'core.hooksPath clobber: hooks symlink + per-worktree config.worktree overrides keep the gate live; warm-lane-audit
      stash_entries is the backstop'
    traced: true
    outcome: hooks-armed-guard.sh check/arm probes reference-transaction behaviourally and checks the config.worktree pin
      via lib_worktree_hooks_pin.sh. setup-main-gate-worktree-config.sh writes it. land.sh re-spells the pin write instead
      of using these (new low finding). warm-lane-audit.sh is outside the slice.
  - slice: CG2-kernels
    scenario: 'Imported fields: elaborate_field''s Imported arm reads a .vdb through reify-kernel-openvdb and surfaces FieldImportFailed
      loudly on error'
    traced: true
    outcome: "Wired: engine_eval.rs:4325-4360 \u2192 read_vdb_file (cfg has_openvdb) \u2192 build_realized_grid_source \u2192\
      \ lower_to_sampled. Errors become a FieldImportFailed-coded Diagnostic::error plus Undef. Gap: unit-prefixed grids ('mm',\
      \ 'MPa', ...) pass validation but are never scaled to SI (finding on ingest.rs::KNOWN_UNITS)."
  - slice: CG2-kernels
    scenario: 'Cross-kernel booleans: Manifold mesh boolean on OCCT-produced shapes via auto-tessellation'
    traced: true
    outcome: "Kernel side intact: ingest_mesh checks %3 buffers \u2192 manifold_from_reify_mesh (weld + INV-GEO-1 contract\
      \ check, posture from a process OnceLock; fk-a0ce1853f2da owned by \u03B6\u2082) \u2192 as_original \u2192 store; Union/Difference/Intersection\
      \ via get_manifold (InvalidReference); tessellate with empty-result guard. The conformance arm occt_manifold_ingest_conformance\
      \ validates\u2192ingests\u2192re-validates 4 fixtures plus the sphere with real assertions."
  - slice: CG2-kernels
    scenario: Sub-shape selection (@face/@edge) and attribute correlation on Manifold results through reify-eval's selector
      pass
    traced: true
    outcome: "Gap: AdjacentFaces/SharedEdges on Manifold use raw triangle indices while the eval consumers pass extract_faces\
      \ (coalesced planar-face) positions, so results are silently wrong once queries reach the owning kernel (finding on\
      \ ManifoldKernel.query). Today the selector pass takes one kernel argument (engine_build.rs:11156), so the gap is latent\
      \ until S10 \u03B9\u2085. Attribute propagation (propagate_attributes \u2192 result_faces store) is wired; per-face\
      \ correlation is still #4263."
  - slice: CG2-kernels
    scenario: "Isosurface over a Boolean (Voxel demand on a Boolean producer) \u2014 not a briefing key scenario; traced as\
      \ the voxel known_gap"
    traced: true
    outcome: 'Gap: OpenVDB declares voxel Booleans that execute() never performs, and the dispatcher''s literal final-stage
      match routes them there (finding on openvdb_capability_descriptor). Not executed.'
  - slice: CG2-kernels
    scenario: "check \u2192 build \u2192 STEP through OCCT (primitives, booleans, fillets, sweeps, patterns, export, mass)"
    traced: false
    outcome: "Only the review-all re-observations were checked: get_shape cause propagation is fixed by #7379, while the repr-by-exit-path\
      \ tail (fk-ba1ea4167dbe, S11 \u03B6) and the spawn-loop op lowering (fk-e6bdef1ef0ae, S11 \u03B31) are still present\
      \ and owned. lib.rs/handle.rs bodies were not re-traced because S11 owns them."
  - slice: CG2-kernels
    scenario: Auto params solved via ResolutionProblem / domain solvers
    traced: false
    outcome: Not in this slice (reify-constraints).
  - slice: RA-reify-audit
    scenario: 'P1: a done task''s commit range is diffed through jcodemunch for new public producers with no consumer, honouring
      G-allow'
    traced: true
    outcome: "BROKEN at the non-test-caller step. The path runs bin bind_jcodemunch \u2192 Seam::Live \u2192 p1_producer_orphan::check\
      \ \u2192 get_changed_symbols, which is fallible (#6928/#8441: Unexamined findings for failures and an unavailable seam)\
      \ \u2192 opts_out (G-allow, allow(dead_code), cfg(test) via enrich_suppression_flags; honoured) \u2192 find_references\
      \ \u2192 filter_refs_to_file. filter_refs_to_file drops every cross-file reference, so the consumer guard is dead and\
      \ cross-file-consumed symbols are reported as orphans; the fix is on unmerged task/6504. find_references also fails\
      \ soft to empty, which yields the same false orphan (#8363). The G-allow and grace-window logic is coherent."
  - slice: RA-reify-audit
    scenario: 'P5: a task marked done whose deliverable is absent from main is surfaced as phantom-done (sweep and dark-factory
      pre-done hook)'
    traced: true
    outcome: "Wired end to end. Sweep: check \u2192 swept_tasks \u2192 check_task \u2192 check_one (runs.db trail, rescues,\
      \ git-diff leg) \u2192 High. Pre-done: wrapper \u2192 --tasks-file snapshot \u2192 check_pre_done \u2192 check_one \u2192\
      \ check_pre_done_landing. Costs found: (1) runs.db degradation emits one Medium P5PhantomDone per merged task, and the\
      \ skill says P5 can never be Medium (new finding). (2) The production pre-done path loads the whole corpus through the\
      \ jq snapshot, which neither flattens subtasks nor tolerates one malformed row, instead of the bin's O(1) get_task path\
      \ (new finding). Known costs: the exact-match coverage predicate (\u03B7), infallible git (\u03B4) and the env-read\
      \ posture (\u03B9)."
  - slice: RA-reify-audit
    scenario: 'PTODO: a new TODO without a live #NNNN cite, or citing a done task, is a High finding; on the real tree the
      ratchet reds on any new structural fingerprint absent from the empty baseline'
    traced: true
    outcome: "Wired. ptodo-baseline-gen \u2192 check_with_stats \u2192 fingerprint (summary-parsed; owned by \u03B31/\u03B3\
      2) \u2192 tests/infra/test_reify_audit_ptodo.sh runs comm in both directions, with the @@PTODO_SCAN@@ files_scanned\
      \ floor and the tasks_db=absent floor. No gap found beyond the review-all-owned summary parsing. The liveness lanes\
      \ read tasks.db directly (new convention finding)."
  - slice: GUI2-frontend
    scenario: Opening a .ri file shows source and geometry; edits re-evaluate via delta events; status bar shows idle, triangle
      and constraint counts
    traced: true
    outcome: "Partially traced on the frontend side: Editor updateListener \u2192 debounced bridge.updateSource; engineStore\
      \ subscriptions; StatusBar reads evalStatus.phase, meshes and constraints (exhaustive over ConstraintStatus since #7465).\
      \ No new wiring gap. The phase-string disagreement ('resolving') is owned by S26 mu #9073 (fk-7262d24da57d)."
  - slice: GUI2-frontend
    scenario: Property-editor edits with unit-aware literals trigger re-check and viewport update
    traced: false
    outcome: "Only traced as far as PropertyEditor \u2192 bridge.setParameter/previewParameter (EDIT_EPOCH/lastEditSeq module\
      \ state). The quantity grammar is owned by S26 theta #9069 and #9147. Not followed into Rust."
  - slice: GUI2-frontend
    scenario: "Viewport click \u2192 editor scroll; group double-click \u2192 fly-to; diagnostics row \u2192 jump-to-line\
      \ incl. cross-file open"
    traced: true
    outcome: "Diagnostics row: cross-file open works through navigateToDiagnostic. Viewport click (navigateToSource) and the\
      \ navigate-to-source event call setScrollToLocation directly and work for the active file only. The Find-uses panel\
      \ (a sibling navigation path) silently drops cross-file references and shows wrong previews. Recorded as findings at\
      \ App.tsx::navigateToDiagnostic and references.ts::previewForLocation. Fly-to goes through onFocusEntity \u2192 flyToEntityFn,\
      \ which exists."
  - slice: GUI2-frontend
    scenario: Rapid edits supersede queued evaluations
    traced: false
    outcome: Backend EvalQueue, outside this frontend slice.
  - slice: GUI2-frontend
    scenario: Claude chat panel drives the sidecar (--resume) under landlock
    traced: true
    outcome: "Traced ChatPanel \u2192 claudeStore \u2192 App onSend \u2192 bridge \u2192 Rust claude_send_message \u2192 sidecar\
      \ invokeSdk \u2192 outbound_to_event \u2192 subscribeToClaudeEvents \u2192 claudeStore. Broken: message ids are minted\
      \ twice and never reconciled, so done, tool_call, tool_result and error never reach the assistant message (finding at\
      \ claudeStore.ts::createClaudeStore). The permission-decision envelope defect is already owned by S26 alpha #9062. sessionId/--resume\
      \ continuity holds inside SidecarSession. The clear-session path has no UI entry (low finding)."
  - slice: GUI2-frontend
    scenario: REIFY_DEBUG=1 debug MCP listener; screenshot_window
    traced: false
    outcome: The frontend half (debug/bridge.ts buildHandlers) is owned in full by S26 rho/sigma/tau/upsilon (#9079-#9082).
      Not re-traced.
  - slice: LP3-core-ir-expr
    scenario: Kleene boolean evaluation over Undef operands; Undef arithmetic propagates quietly
    traced: true
    outcome: "eval_and/eval_or/eval_implies (reify-expr lib.rs:3666-3735) \u2192 KBool::try_from \u2192 kleene_and/or/implies.\
      \ The truth tables match spec \xA79.2.3, and a non-bool operand \u2192 Undef. The duplicated absorption rule is owned\
      \ by #8549. The Int overflow panic on the arithmetic leg is owned by #8550. Both are re-observed, not new."
  - slice: LP3-core-ir-expr
    scenario: 'A .ri file is parsed, compiled and evaluated: param defaults propagate, let bindings compute'
    traced: true
    outcome: "Probed the unit-literal leg with the prebuilt release binary. `50MPa*m^1/2` parses with exponent ^1 and a value-level\
      \ /2 and evaluates to 25 MPa\xB7m with no diagnostic (INV-SF-7 violation). Its owner #7151 is phantom-done; see the\
      \ units.rs::ri_compound_unit_expr finding."
  - slice: LP3-core-ir-expr
    scenario: 'Structure-instance runtime: PascalCase ctors evaluate to a typed Value::StructureInstance'
    traced: true
    outcome: 'The CompiledExprKind::StructureInstanceCtor arm (lib.rs:1109) delegates to eval_structure_instance_ctor and
      is wired. The only gap is a stale test-module comment at lib.rs:6256 claiming the arm returns Undef (prose, owned by
      S23 #9144).'
  - slice: LP3-core-ir-expr
    scenario: 'Builtin dispatch: migrated families resolve through reify_builtins::lookup; the rest still string-dispatch'
    traced: false
    outcome: Not traced end to end. S8 (#8888-#8913) owns the registry migration and the classify_call interception. I noted
      only that dual_eval's differentiability tables are a further string-dispatched dimension (low finding).
  - slice: LP3-core-ir-expr
    scenario: Warm-state / commitment policy (reify-runtime, per the briefing's key_decisions)
    traced: true
    outcome: No production path reaches CommitmentTracker, PriorityPromoter, assert_warm_startable_coextensive or WarmStartableRegistry::from_inventory.
      The only consumer is the display-only reify-cli dev inspect-node, so the B5 invariant gate never runs (medium finding).
  - slice: AN1-solvers
    scenario: "Real .ri FEA cantilever runs @optimized \u2192 ComputeNode \u2192 trampoline \u2192 solver-elastic, bit-identical\
      \ from cache"
    traced: true
    outcome: "Traced only the solver-elastic half. elastic_static still uses resolve_execution_modes + assemble_global_stiffness\
      \ + solve_cg. The new linear_solve seam (SpdSystem/SpdSolver, #8372) and assemble_stiffness (#8373) have no production\
      \ adopter yet; that is \u03B2 #8374 / \u03BE #8375, both pending. SpdSystem::new, the Direct tier and Jacobi-PCG type\
      \ every ending (SystemInvalid, SolveStop, SolveFailure), and the zero-RHS rule holds. Legacy solve_cg still collapses\
      \ Cancelled and CapReached into converged=false (CgRun::into_cg_result). The modal eigensolve still factors through\
      \ faer's global-parallelism API (finding on eigensolve.rs::EuclideanOperator). The trampoline/eval half is outside this\
      \ slice."
  - slice: AN1-solvers
    scenario: 'Mesh morph under a shape edit: classifier picks Laplacian / elasticity / remesh-fallback; elasticity meets
      quality floor and warm-starts FEA'
    traced: true
    outcome: "Wired: register_morph_producer \u2192 MeshMorphProducer.try_morph \u2192 compose_morph \u2192 morph_eligible\
      \ \u2192 compute_dirichlet_bcs (KernelProjector) \u2192 laplacian_smooth or elasticity_morph under catch_unwind \u2192\
      \ quality_check \u2192 rekey_boundary_association. The Laplacian/elasticity choice is made by compose_morph's displacement-magnitude\
      \ rule (LAPLACIAN_DISPLACEMENT_FRACTION=0.05), not by the eligibility classifier. MorphFailure is rendered to strings\
      \ across the engine seam (MorphResult::Ineligible(String) etc.), owned by S16 \u03B2 #9092. The producer uses MorphOptions::default()\
      \ on every call. The morph itself never warm-starts FEA; that is the #2952 measurement gap the briefing already records.\
      \ The skeleton `morph` is still exported (fk-d632489a4c98)."
  - slice: AN1-solvers
    scenario: "Shell extraction T1\u2013T4 + remesh: thin solid \u2192 medial axis \u2192 mid-surface \u2192 pruning \u2192\
      \ segmentation \u2192 shell mesh with shell/tet interface partition"
    traced: true
    outcome: "reify-eval shell_extract_compute_fn wires medial mask \u2192 mid-surface \u2192 pruning \u2192 segmentation\
      \ \u2192 mesher. The last step, the shell/tet interface partition (partition_body), has no production caller; the mixed-region\
      \ consumer is dead_code pending #6371 (new finding). The SDF producer build_slab_sdf transposition is owned by #8711\
      \ (fk-4ac205f76343). The extract_mid_surface index panic on malformed grids is owned by #8714 (fk-90433ba69658)."
  - slice: AN1-solvers
    scenario: 'FDM: as_printed_material returns R-fast immediately and refines to R0 when PrusaSlicer is on PATH'
    traced: true
    outcome: "The R-fast path is wired (@optimized fdm::as_printed_material_r_fast). fdm::slice is wired (discover_slicer/slice_body\
      \ with SIGTERM\u2192SIGKILL). R0 is not reachable from .ri: no stdlib declaration routes to fdm::as_printed_material_r0,\
      \ and select_rungs/rung_compute_key have no production caller. The ladder is tested only through direct trampoline calls.\
      \ Owned by decision gate #7087 (new finding, low)."
  - slice: AN1-solvers
    scenario: Modal analysis populates Mode.shape; z_dominant_frequencies classifies from it
    traced: false
    outcome: Not traced. The path lives in reify-eval modal_ops / stdlib, outside this slice. In-slice I checked only eigensolve.rs's
      factorisation path.
  - slice: AN1-solvers
    scenario: ElasticResult.frame and shell_channels Undef on solid tets by contract
    traced: false
    outcome: 'Not traced: the population is eval-side, outside this slice.'
  - slice: LP2-eval-core
    scenario: 'A .ri file is parsed, compiled and evaluated: param defaults propagate, let bindings compute in dependency
      order, constraints report satisfied/violated/indeterminate'
    traced: true
    outcome: "Wired: eval() \u2192 evaluate_params_and_lets_unified \u2192 commit_cell_result. The cold Let/Param contexts\
      \ use eval_ctx_with_meta without containment or runtime sink, while eval_cached uses cell_eval_ctx: the known cold/warm\
      \ divergence fk-32d11c0fd0ab, owned by S4 \u03C3 #8871. No new gap."
  - slice: LP2-eval-core
    scenario: The unified DAG orders the build; a genuine cycle or an unreachable auto surfaces as E_EVAL_CYCLE / E_EVAL_UNRESOLVED
      rather than a silent Undef
    traced: true
    outcome: "Wired. Cold eval emits DiagnosticCode::EvalCycle (engine_eval.rs ~2669) and EvalUnresolved (~1672). On build,\
      \ run_unified_pass's Tarjan stage emits E_EVAL_CYCLE, and build_with_geometry_output extends pass.diagnostics (engine_build.rs\
      \ ~5090). demand_scoped_unified_pass's selective branch (engine_build.rs:6040) fabricates empty residue and diagnostics.\
      \ That branch is reachable in the GUI after sync_demand, but its consumers (tessellate, build_snapshot) never surface\
      \ pass.diagnostics, and CLI build always runs full scope after check\u2192eval sets full_scope. So no missed surfacing\
      \ today; the branch is a driver-axis difference under fk-f7e4716af878."
  - slice: LP2-eval-core
    scenario: 'Incremental re-evaluation after a parameter edit: dirty/demand cone intersection computes the minimal eval
      set; early cutoff skips unchanged nodes'
    traced: true
    outcome: "Wired: edit_param \u2192 dirty::compute_dirty_cone(changed, reverse_index, graph) \u2192 compute_eval_set(dirty,\
      \ self.demand, trace_map), which is topo-sorted via run_unified_pass_seeded. The walk commits through commit_cell_result\
      \ with CacheLeg::Record, and EvalOutcome::Unchanged adds dependents to the `skipped` set (engine_edit.rs ~1150-1440).\
      \ The phase structure is the known fk-263246810f8f cost."
  - slice: LP2-eval-core
    scenario: 'Auto parameter resolution: two-phase eval resolves undetermined params only on entry points that attach a solver'
    traced: false
    outcome: 'Not traced end to end. Arms are hand-rolled per the fk-de6e6f9e6b14 roster (6 arms, ''Constraint solver made
      no progress'' at 6 sites), owned by S4. Related front-end posture: reify test builds Engine::new(checker, None) via
      test_runner::build_test_engine, with no kernel and no solver.'
  - slice: LP2-eval-core
    scenario: reify test reports constraint verdicts for @test blocks (constraints report satisfied/violated/indeterminate)
    traced: true
    outcome: "Gap, already owned. cmd_test \u2192 reify_eval::run_tests \u2192 build_test_engine = Engine::new(checker, None)\
      \ with no geometry kernel, then engine.check() only, with no build or realize step. A geometry-bearing @test therefore\
      \ returns INDETERMINATE, prints 'test result: ok' and exits 0. Tracked by #6781 (pending)."
  - slice: LP2-eval-core
    scenario: 'Guard-controlled topology: a guard cell''s boolean change rebuilds the active set of cells and constraints'
    traced: true
    outcome: "Wired on the warm path: edit_param's guard re-elaboration phase (engine_edit.rs ~1559) and the post-wave2 driver-ordered\
      \ member reseed (~1962-2076), which uses run_unified_pass_seeded. The dead edit_source copy still diverges: its reseed\
      \ is members \u2229 demand only (4242). Owned by fk-8dd6a16ea574 / S4 \u03B4 #8862."
  - slice: LP2-eval-core
    scenario: Purpose activation injects constraints at runtime without recompilation (reify check --purpose)
    traced: false
    outcome: "Read only the placeholder-expansion step (engine_purposes.rs:220-256: expand_purpose_reflective_placeholders\
      \ over rewritten constraints and objective terms). It is wired; the h8/h7 shape is owned by fk-fb8cf3e787b9 / S1 \u03B7\
      \ #9045."
  - slice: LP2-eval-core
    scenario: 'Build export: Phase-B product bodies and declarative `: Output` export through the owning kernel'
    traced: true
    outcome: 'Product export now routes by owner: export_product_bodies + kernel_owner::export_kernel_for_owners (#6308),
      and a mixed-owner set gives an Error diagnostic. The declarative Output path (engine_build.rs ~5520-5600) still exports
      subject_handle through the default kernel. A subject realized on a non-default kernel therefore silently exports whatever
      shape that id names on the default kernel. Tracked by #6366 (pending, depends on #8995).'
  - slice: AN2-compute-targets
    scenario: "A real .ri (examples/fea_cantilever_smoke.ri) runs @optimized \u2192 ComputeNode \u2192 trampoline and returns\
      \ a non-Undef ElasticResult with Sampled fields; a second run is bit-identical from cache"
    traced: true
    outcome: "Wired: register_production_compute_fns \u2192 register_compute_fns registers solver::elastic_static; run_compute_dispatch\
      \ does persistent lookup \u2192 invoke \u2192 persistent write. Gaps, all owned or recorded: multi-case solves drop\
      \ structured detail (new finding); a persistent HIT drops a pool-held warm state (new low finding); dims overload ignores\
      \ supports beyond the clamp face (S16 \u03BC #9119); buckling ignores supports (#7081). Probe with the 2026-10-13 release\
      \ binary: `gravity(...)` passed as a load is rejected at check time ('does not conform to trait Load'), so it is not\
      \ a silent drop."
  - slice: AN2-compute-targets
    scenario: Modal analysis populates Mode.shape (List<Vector3>) and z_dominant_frequencies classifies modes from it
    traced: true
    outcome: run_modal_analysis writes mode shapes. z_dominant_frequencies is a helper inside crates/reify-eval-fea-tests/tests/modal_analysis_e2e.rs,
      not a production feature (briefing defect). Displacement queries by Selector are unreachable in production (new finding
      on resolve_location_value).
  - slice: AN2-compute-targets
    scenario: ElasticResult.frame and shell_channels are Undef on solid tets by contract
    traced: false
    outcome: 'Not traced beyond confirming that buckling.rs writes frame: Undef (lines 441, 949). The elastic tet arm was
      not re-read (fk-b5dc0cd8ec92 territory).'
  - slice: AN2-compute-targets
    scenario: 'Mesh morph under a shape edit: eligibility classifier picks Laplacian/elasticity/remesh and warm-starts FEA'
    traced: true
    outcome: 'morph_stage_b::stage_b_eligible is alive through reify-mesh-morph eligibility. morph_arm_e2e records that Stage
      B currently rejects boolean-cut B-reps as NamingLayerError (attribution gap, live #6637), so the structural-change path
      is measured as a naming-layer reject, not a bijection verdict. Known and owned.'
  - slice: AN2-compute-targets
    scenario: "Shell extraction T1\u2013T4 + remesh producing a shell mesh with a shell/tet interface partition"
    traced: true
    outcome: "shell-extract::extract is registered through register_production_compute_fns. The FEA shell route feeds it a\
      \ synthetic slab SDF (shell_solve::build_slab_sdf), and the stress solve does not consume the extracted mid-surface\
      \ (PRD \xA711 OQ-2). Owned by #5119 (re-anchored under S5 \u03BD/\u03BF)."
  - slice: AN2-compute-targets
    scenario: 'FDM: as_printed_material returns an R-fast field immediately and refines to R0 when PrusaSlicer is on PATH;
      R0 slicer path deterministic and cancel-clean'
    traced: true
    outcome: 'R-fast is wired from .ri. R0 refinement has no production path: no .ri route to fdm::as_printed_material_r0,
      select_rungs and rung_compute_key are test-only, and the #3791 gate drives the trampolines directly (new critical-path
      finding; owner #7087/#9001). Malformed FDM inputs degrade silently (new finding).'
  - slice: CG1-constraints-geometry
    scenario: 'Auto params are solved: ResolutionProblem decomposes via union-find into components, each routed to its domain
      solver, results merged'
    traced: true
    outcome: 'Wired end to end. SolverRegistry::solve_inner calls decompose_prelude, then decompose_into_components_with_reads
      (union-find plus dependent-cell reads), then solver_for(domain) per component (production() installs Dimensional, SolveSpace,
      CpSat and DiscreteFirstFallback), then merges per component (Infeasible/NoProgress short-circuit the whole resolve).
      Gaps are all owned already: name-based routing with dead std::geo:: prefixes and missing std::angle/std::fasten (#6681);
      a Geometric NoProgress kills the whole resolve (#6659); SolveSpace returns unique:true and discards libslvs dof (#6713,
      deferred); the SolveSpace expression path is retired by #6682. New: the routing tests use fabricated names (finding
      on classifier.rs).'
  - slice: CG1-constraints-geometry
    scenario: "Stdlib trig enforces ANGLE \u2014 sin(5mm) returns Undef"
    traced: true
    outcome: Holds for sin/cos/tan through trig_input. It does not extend to asin/acos/atan/atan2/exp/log/hyperbolics, which
      strip dimension through Value::as_f64 (new finding on trig.rs::eval_trig).
  - slice: CG1-constraints-geometry
    scenario: 'Declarative engineering libs evaluate: stack-up, clearance constraints (cross-sub clearance UNPOSED, #6583)'
    traced: true
    outcome: 'Stack-up traced: eval_stackup to contributor/parse_chain_checked to diagnose with coded diagnostics; error-cause
      conflation is a new low finding. The briefing''s tracking task #6583 is cancelled (a duplicate of #5436 + #5483, both
      pending). GD&T/DFM paths are owned by S12 (#9026/#9027/#9031).'
  - slice: CG1-constraints-geometry
    scenario: 'Cross-kernel booleans: Manifold mesh boolean on OCCT shapes via the dispatcher BFS conversion chain (DEMANDED-REPR
      routing)'
    traced: false
    outcome: 'Partially traced: dispatcher::dispatch BFS (seeded deterministically, lex-min tie-break, prefer_kernel pragma)
      and v03_conversion_projection''s three crossings exist and are called from engine_build. Not followed into the conversion
      executor or the Manifold kernel. The selection axis is owned by fk-80c2f82c17d6 / #8998; DispatchPlan.kernel is a String
      while conversions carry KernelId.'
  - slice: CG1-constraints-geometry
    scenario: Sub-shape selection (@face/@edge) and attribute correlation work on Manifold results through reify-eval's selector
      pass
    traced: false
    outcome: 'Partially traced: topology_attribute_resolver::resolve_unique_by_attribute is called from geometry_ops.rs:11652
      (live). The primitive-seed per-face roles for Cylinder/Cone are placeholders cited to #4263 (pending, known gap). Resolver
      duplication is owned by #8747.'
  - slice: CG1-constraints-geometry
    scenario: "A designer's .ri part goes check \u2192 build \u2192 STEP with fillets via edge selectors, sweeps and patterns"
    traced: false
    outcome: 'Not traced to the OCCT export. Observed only the 6308 change: the Phase-B export now tags each ExportBody with
      its owning kernel (kernel_owner::resolve_owning_kernel_name), which removes one wrong-kernel export path.'
  - slice: CG1-constraints-geometry
    scenario: sweep(profile, path) transports the profile AS POSED
    traced: false
    outcome: Not traced; the compile-side sweep op lives in geometry_ops compile, which was not deep-read. The adjacent sweep_classifier
      was read, and its ExtrudeSymmetric conflation is a new finding.
  phase3_summary: 'Filed 71 mechanical tasks (#4944, #6942, #7398, #8546, #8546, #8766, #8772, #8775, #8926, #9174, #9175,
    #9176, #9177, #9178, #9179, #9180, #9181, #9182, #9183, #9184, #9185, #9186, #9187, #9188, #9189, #9190, #9191, #9192,
    #9193, #9194, #9195, #9196, #9197, #9198, #9199, #9200, #9201, #9202, #9203, #9204, #9205, #9206, #9207, #9208, #9209,
    #9210, #9210, #9211, #9212, #9213, #9214, #9215, #9216, #9217, #9218, #9219, #9219, #9220, #9221, #9222, #9223, #9224,
    #9225, #9226, #9227, #9228, #9229, #9230, #9231, #9232, #9233); 4 covered by existing tasks (keys stamped: #9035, #6504
    (raised to high), #9101, #6616); 39 structural findings left open for deliberation; 18 review-all findings fixed since
    2026-10-10; chain: interim [9242]'
  trigger_chain:
    form: interim
    task_ids:
    - 9242
    superseded: []
    reason: null
    depends_on:
    - 9207
  not_re_verified: '15 review-all findings with slug: anchors (S23 prose rehoming) and 1 unsliced anchor carry task-store
    dispositions only (verdict unverified)'
  phase1_note: 'test_command/lint_command are configured for --scope branch; this run used --scope all --profile both --include-infra
    (the merge gate''s form) in the pinned tree; type_check_command is the configured no-op `true` (task 4063: clippy -D warnings
    is the type gate)'
```

## Review complete: full — review-reify-20261015
as_of 0e60fa138d · since none · mode full

### Phase 1
- test: exit 0 — 38480 passed, 0 failed, 101 skipped (991 binaries; debug 26529/71 skipped, release 10661/29, gui 1290/1; started 2026-10-15T19:xx, finished 21:19:35Z)
- lint: exit 0 (finished 21:51:45Z; clippy -D warnings clean)
- typecheck: exit 0 (configured no-op (task 4063))
- Lint: exit 0 (0 issues) · Type-check: no-op by config (task 4063)
- Smoke: 12/23 — failed: 

### Phase 2
- Scope: mode full — 2973 source files in 14 slices; 359 review-all findings re-observed
- Inputs: review-all-reify-20261010, hotspot-survey-reify-20261015, codebook@0e60fa138d8852e1e26999d6dd22d05964848740, review-all-reify-20261010 (plans/quality-metrics snapshot), legacy:docs/notes/bug-hotspot-survey-2026-07-05.md
- New findings: 3 high · 38 medium · 73 low — by lens: h10: 18, h11: 18, tests: 15, h1: 12, comments: 9, h6: 6, kind:defect: 6, h12: 5, inv-sf-5: 4, h13: 3, h3: 2, inv-nothing-vacuous-and-unowned: 2, inv-sf-3: 2, kind:dead-code: 2, h7: 1, inv-sf-2: 1, h14: 1, h5: 1, h4: 1, inv-sf-1: 1, inv-pd-2: 1, inv-SF-7: 1, inv-SF-5: 1, inv-pd-1: 1
- Carried (review-all) dispositions: filed: 338, fixed: 18, open: 3
- Briefing defects: 47

### Phase 3
Filed 71 mechanical tasks (#4944, #6942, #7398, #8546, #8546, #8766, #8772, #8775, #8926, #9174, #9175, #9176, #9177, #9178, #9179, #9180, #9181, #9182, #9183, #9184, #9185, #9186, #9187, #9188, #9189, #9190, #9191, #9192, #9193, #9194, #9195, #9196, #9197, #9198, #9199, #9200, #9201, #9202, #9203, #9204, #9205, #9206, #9207, #9208, #9209, #9210, #9210, #9211, #9212, #9213, #9214, #9215, #9216, #9217, #9218, #9219, #9219, #9220, #9221, #9222, #9223, #9224, #9225, #9226, #9227, #9228, #9229, #9230, #9231, #9232, #9233); 4 covered by existing tasks (keys stamped: #9035, #6504 (raised to high), #9101, #6616); 39 structural findings left open for deliberation; 18 review-all findings fixed since 2026-10-10; chain: interim [9242]

## Findings

| display_id | key | area | anchor | tags | severity | class | verdict | disposition |
|---|---|---|---|---|---|---|---|---|
| R1 | fk-67feaf8cc7e2 | gui | `gui/src/stores/claudeStore.ts::createClaudeStore` | h11 h10 tests kind:critical-path | high | structural | confirmed | open |
| R2 | fk-f8cb4ed96ae8 | language-pipeline | `crates/reify-core/src/units.rs::ri_compound_unit_expr` | inv-SF-7 inv-SF-5 h10 kind:convention | high | mechanical | confirmed | filed:9207 |
| R3 | fk-4266f4222b24 | reify-audit | `crates/reify-audit/src/jcodemunch_client.rs::filter_refs_to_file` | h1 tests kind:critical-path | high | mechanical | confirmed | filed:6504 |
| R4 | fk-b683dbb8c87b | analysis | `crates/reify-eval/src/compute_targets/as_printed_material.rs::as_printed_material_r_fast_trampoline` | inv-sf-3 inv-sf-1 h10 kind:deep-read | medium | structural | weakened | open |
| R5 | fk-a804f8218ad6 | analysis | `crates/reify-eval/src/compute_targets/as_printed_material_r0.rs::as_printed_material_r0_trampoline` | tests h1 kind:critical-path | medium | structural | confirmed | open |
| R6 | fk-e088a6e9caaa | analysis | `crates/reify-eval/src/compute_targets/multi_case.rs::solve_multi_case_trampoline` | kind:defect h10 inv-sf-6 | medium | mechanical | confirmed | filed:9219 |
| R7 | fk-6d119487b4f1 | analysis | `crates/reify-eval/src/modal_ops.rs::resolve_location_value` | inv-sf-3 h1 tests kind:critical-path | medium | structural | confirmed | open |
| R8 | fk-756b94f946c0 | analysis | `crates/reify-shell-extract/src/partition.rs::partition_body` | h6 kind:critical-path kind:convention | medium | structural | confirmed | open |
| R9 | fk-e653f30b14df | analysis | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs::geometric_element_stiffness_shell` | inv-sf-5 h1 h6 kind:stub | medium | mechanical | confirmed | filed:9213 |
| R10 | fk-98a6b0d91a7f | assemblies | `crates/reify-compiler/src/connect.rs::compile_connection` | h12 h11 kind:cross-module | medium | structural | weakened | open |
| R11 | fk-e30c10d74c04 | assemblies | `crates/reify-compiler/src/forall_elaborate.rs::elaborate_forall_connect` | h10 inv-SF-3 kind:critical-path | medium | mechanical | confirmed | filed:9185 |
| R12 | fk-68442f3e3530 | assemblies | `crates/reify-compiler/src/port_path.rs::parse_root` | h10 kind:defect | medium | mechanical | confirmed | filed:9186 |
| R13 | fk-e5147ccf3fd9 | constraints-and-geometry | `crates/reify-eval/src/sweep_classifier.rs::classify_swept_body` | kind:defect h1 h11 | medium | mechanical | confirmed | filed:9226 |
| R14 | fk-c23fa8fef28c | constraints-and-geometry | `crates/reify-kernel-manifold/src/kernel.rs::ManifoldKernel.query` | h1 h11 h10 kind:cross-module | medium | structural | confirmed | open |
| R15 | fk-6a9547d3d171 | constraints-and-geometry | `crates/reify-kernel-openvdb/src/ingest.rs::KNOWN_UNITS` | h12 h10 kind:defect | medium | mechanical | confirmed | filed:9196 |
| R16 | fk-17a5194418da | constraints-and-geometry | `crates/reify-kernel-openvdb/src/register.rs::openvdb_capability_descriptor` | h1 inv-sf-5 h10 kind:critical-path | medium | structural | confirmed | open |
| R17 | fk-8f8d0ef27b07 | factory-infra | `hooks/project-checks` | h10 h11 kind:critical-path | medium | mechanical | confirmed | filed:9194 |
| R18 | fk-3cf4b09d4b1a | factory-infra | `scripts/refresh-warm-base.sh` | h10 h1 | medium | mechanical | confirmed | filed:9180 |
| R19 | fk-3d9c0153c359 | factory-infra | `scripts/seed-warm-lane.sh::_sidecar_path` | h11 h10 | medium | structural | confirmed | open |
| R20 | fk-912fa2bf5921 | factory-infra | `scripts/warm-lane-audit.sh` | h10 comments | medium | mechanical | confirmed | filed:9181 |
| R21 | fk-91bac0103c72 | factory-infra | `scripts/warm-lane-source-integrity.sh` | h10 comments | medium | mechanical | confirmed | filed:9182 |
| R22 | fk-83fa1aeda5f6 | factory-infra | `tests/infra/run_all.sh::_ra_timing_pool_lines` | h3 h11 | medium | structural | confirmed | open |
| R23 | fk-d645f8a1d95a | gui | `gui/src-tauri/src/claude_bridge.rs::InboundMessage` | h1 h6 h11 kind:dead-code | medium | structural | confirmed | open |
| R24 | fk-bf57b4017285 | gui | `gui/src-tauri/src/main.rs::claude_send_message` | h7 h5 kind:critical-path | medium | structural | confirmed | open |
| R25 | fk-93b720040a52 | gui | `gui/src/App.tsx::navigateToDiagnostic` | h11 h10 kind:critical-path | medium | mechanical | confirmed | filed:9203 |
| R26 | fk-c4858bb86f2f | gui | `gui/src/editor/Editor.tsx::Editor` | h11 h10 kind:defect | medium | mechanical | confirmed | filed:9202 |
| R27 | fk-b252ff943d44 | gui | `gui/src/viewport/ProbeSystem.tsx::createProbeSystem` | inv-nothing-vacuous-and-unowned h9 kind:dead-code | medium | structural | confirmed | open |
| R28 | fk-5e611b84370d | kinematics-and-motion | `crates/reify-stdlib/src/flexures/common.rs::classify_geometry_invalid` | h10 inv-sf-1 h11 | medium | structural | confirmed | open |
| R29 | fk-e348f06cce2e | kinematics-and-motion | `crates/reify-stdlib/src/orientation.rs::eval_orientation` | h12 h11 h3 h4 | medium | structural | weakened | open |
| R30 | fk-5d64b909dfdf | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/gcode_import.rs::eval_gcode_import` | inv-sf-1 h10 kind:critical-path | medium | structural | confirmed | open |
| R31 | fk-73c31b060f80 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/sampling.rs::to_trajectory_samples` | h10 kind:defect | medium | mechanical | confirmed | filed:9190 |
| R32 | fk-6166c9f2a331 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/simulate.rs::simulate_trajectory_core` | h1 kind:critical-path | medium | structural | confirmed | open |
| R33 | fk-c2cca28ae7c5 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/trampoline.rs::track_data_to_value` | inv-pd-2 h12 | medium | structural | confirmed | open |
| R34 | fk-194bafc50401 | language-pipeline | `crates/reify-compiler/src/annotations/schema.rs::SCHEMAS` | tests h6 kind:coverage | medium | structural | confirmed | open |
| R35 | fk-b8a0e7566919 | language-pipeline | `crates/reify-eval/src/measure_min_wall.rs::Engine.measure_min_wall` | h11 comments h10 | medium | mechanical | confirmed | filed:9217 |
| R36 | fk-f8305a739f1a | language-pipeline | `crates/reify-runtime/src/warm_startable_assert.rs::assert_warm_startable_coextensive` | h10 h6 comments kind:dead-code | medium | structural | confirmed | open |
| R37 | fk-5c361cda62de | language-pipeline | `crates/reify-test-support/src/orphan_audit.rs::run_orphan_audit` | tests h10 kind:coverage | medium | mechanical | confirmed | filed:9209 |
| R38 | fk-d6e5b0480df8 | reify-audit | `crates/reify-audit/src/lib.rs::JCodemunchOps.find_references` | h10 h3 | medium | structural | confirmed | open |
| R39 | fk-923f53493f57 | reify-audit | `crates/reify-audit/src/p5_phantom_done.rs::has_task_completed_event` | h10 h11 h3 | medium | structural | confirmed | open |
| R40 | fk-e19e2d1ba1e8 | reify-audit | `crates/reify-audit/src/plinecite.rs::line_number_end` | comments kind:convention | medium | mechanical | confirmed | filed:9188 |
| R41 | fk-7199821bfd6b | tooling | `crates/reify-config/src/cache.rs::CacheResolverInputs` | h11 h10 h1 | medium | structural | confirmed | open |
| R42 | fk-5882b549863f | analysis | `crates/reify-eval-fea-tests/src/lib.rs` | tests kind:coverage | low | structural | weakened | open |
| R43 | fk-aeba170a962b | analysis | `crates/reify-eval/src/compute_targets/buckling_multi_case.rs::solve_buckling_multi_case_trampoline` | h11 h3 | low | mechanical | confirmed | filed:9219 |
| R44 | fk-41d6ac48ae56 | analysis | `crates/reify-eval/src/compute_targets/fea_diagnostics.rs::fea_structured_detail` | kind:dead-code tests h9 | low | mechanical | confirmed | filed:9223 |
| R45 | fk-72c5899f1a45 | analysis | `crates/reify-eval/src/compute_targets/mod.rs::register_compute_fns` | comments h1 | low | mechanical | confirmed | filed:9224 |
| R46 | fk-735c24dd849f | analysis | `crates/reify-eval/src/compute_targets/result_topology.rs` | comments tests | low | mechanical | confirmed | filed:9225 |
| R47 | fk-21ccc7e905aa | analysis | `crates/reify-eval/src/engine_compute.rs::run_compute_dispatch` | kind:defect h7 | low | mechanical | confirmed | filed:9220 |
| R48 | fk-98fc505b592f | analysis | `crates/reify-eval/src/modal_ops.rs::solve_modal_core` | h1 tests kind:dead-code | low | mechanical | confirmed | filed:9221 |
| R49 | fk-0a1125558289 | analysis | `crates/reify-fdm/src/as_printed.rs::select_rungs` | inv-pd-1 h1 kind:critical-path | low | structural | confirmed | open |
| R50 | fk-f590a349dc61 | analysis | `crates/reify-fdm/src/lib.rs` | comments h1 kind:deep-read | low | mechanical | confirmed | filed:9216 |
| R51 | fk-3e6917a7a1ae | analysis | `crates/reify-solver-elastic/src/adaptive.rs::probe_target_accuracy` | h6 kind:dead-code kind:convention | low | mechanical | weakened | filed:4944 |
| R52 | fk-ee0f72937e55 | analysis | `crates/reify-solver-elastic/src/eigensolve.rs::EuclideanOperator` | h11 h1 kind:cross-module | low | mechanical | confirmed | filed:9214 |
| R53 | fk-35f5e0982032 | analysis | `crates/reify-solver-elastic/src/math.rs::MIN_JACOBIAN_DET` | inv-sf-5 comments h1 kind:stub | low | mechanical | confirmed | filed:9215 |
| R54 | fk-54ada9ac9619 | analysis | `crates/reify-stdlib/src/loads.rs::is_load_value` | kind:dead-code comments h6 | low | mechanical | weakened | filed:9222 |
| R55 | fk-e6c1343608c1 | assemblies | `crates/reify-compiler/src/conformance/sub_component_validation.rs::check_sub_structure_existence` | h10 kind:critical-path | low | mechanical | confirmed | filed:6616 |
| R56 | fk-137717ade779 | assemblies | `crates/reify-compiler/src/connect.rs::ConnectAccumulator` | comments kind:deep-read | low | mechanical | confirmed | filed:9189 |
| R57 | fk-b2deb4ebef8b | constraints-and-geometry | `crates/reify-constraints/src/classifier.rs::is_geometry_qualified_name` | tests h12 kind:coverage | low | mechanical | weakened | filed:9227 |
| R58 | fk-5c839728f680 | constraints-and-geometry | `crates/reify-constraints/src/decompose.rs::decompose_into_components` | tests h9 h11 | low | mechanical | weakened | filed:9232 |
| R59 | fk-33bcae9b2dd4 | constraints-and-geometry | `crates/reify-constraints/src/decompose.rs::domain_of_auto` | comments h13 | low | mechanical | confirmed | filed:9233 |
| R60 | fk-7bb7b0b121a5 | constraints-and-geometry | `crates/reify-eval/src/appearance.rs::coating_appearance` | h11 h12 inv-SF-3 | low | mechanical | confirmed | filed:9228 |
| R61 | fk-240270804762 | constraints-and-geometry | `crates/reify-eval/src/realization_read_test_support.rs::seed_kernel_realization` | tests comments h7 | low | mechanical | weakened | filed:9231 |
| R62 | fk-ee411fb016d2 | constraints-and-geometry | `crates/reify-kernel-conformance/tests/occt_gmsh_attributed_conformance.rs` | tests h1 kind:coverage | low | mechanical | confirmed | filed:9199 |
| R63 | fk-ddb0fd4f4ca5 | constraints-and-geometry | `crates/reify-kernel-gmsh/src/ffi.rs::mesh_set_size_at_entity` | h1 comments kind:dead-code | low | mechanical | confirmed | filed:9197 |
| R64 | fk-4334831c82cc | constraints-and-geometry | `crates/reify-kernel-openvdb/src/kernel_real.rs::OpenVdbKernel` | h6 kind:dead-code | low | mechanical | weakened | filed:9198 |
| R65 | fk-67a04cc482f3 | constraints-and-geometry | `crates/reify-kernel-openvdb/src/kernel_real.rs::VOXEL_BOOL_STUB_MSG` | h1 comments kind:deep-read | low | mechanical | confirmed | filed:8926 |
| R66 | fk-655f2584d563 | constraints-and-geometry | `crates/reify-stdlib/src/registry_dispatch.rs` | tests h9 h13 | low | mechanical | weakened | filed:9230 |
| R67 | fk-152e4fb58bd7 | constraints-and-geometry | `crates/reify-stdlib/src/stackup.rs::parse_chain_checked` | h10 h1 h12 | low | mechanical | weakened | filed:9229 |
| R68 | fk-452afde2698e | constraints-and-geometry | `crates/reify-stdlib/src/trig.rs::eval_trig` | h10 inv-AD-1 h11 kind:defect | low | mechanical | weakened | filed:6942 |
| R69 | fk-9fb15d8eaa0a | factory-infra | `scripts/affected-crates-lib.sh::direct_crates_per_path` | tests h6 kind:coverage | low | structural | weakened | open |
| R70 | fk-5d6c6f0f00a4 | factory-infra | `scripts/assert-module-scc.py::lex_crate` | h11 h10 kind:cross-module | low | mechanical | confirmed | filed:8766 |
| R71 | fk-b0abbccfe141 | factory-infra | `scripts/jobserver-balancer.py::main` | h4 comments h12 | low | mechanical | confirmed | filed:9184 |
| R72 | fk-e9db0e38b420 | factory-infra | `scripts/land.sh` | h11 kind:cross-module | low | mechanical | confirmed | filed:9195 |
| R73 | fk-00f86e271fb2 | factory-infra | `scripts/refresh-warm-base.sh` | h5 h10 | low | mechanical | confirmed | filed:9183 |
| R74 | fk-882bde3953cf | factory-infra | `scripts/seed-warm-lane.sh::_close_inherited_fds` | tests h11 | low | structural | weakened | open |
| R75 | fk-3f08a4c422f2 | factory-infra | `scripts/verify-ledger.py::PHASE_RULES` | h12 h11 kind:cross-module | low | mechanical | weakened | filed:9035 |
| R76 | fk-6365d50f0d65 | factory-infra | `scripts/verify.sh::_verify_cleanup` | h11 h10 kind:cross-module | low | mechanical | confirmed | filed:9101 |
| R77 | fk-bf7b2f5e4aad | gui | `gui/src-tauri/src/commands.rs::load_initial_file_impl` | comments h11 kind:convention | low | mechanical | weakened | filed:9175 |
| R78 | fk-19a30824e41b | gui | `gui/src-tauri/src/debug_server.rs::confined_write_target` | h10 kind:defect | low | mechanical | confirmed | filed:9174 |
| R79 | fk-65db0c1c9ef3 | gui | `gui/src-tauri/src/diff.rs` | h13 inv-arch-1 h9 kind:cross-module | low | structural | weakened | open |
| R80 | fk-fa60000e66da | gui | `gui/src-tauri/src/main.rs` | comments h1 kind:deep-read | low | mechanical | confirmed | filed:9176 |
| R81 | fk-05c7548661ba | gui | `gui/src-tauri/src/mcp_context.rs::TauriToolContext.get_eval_status` | inv-sf-5 h1 kind:stub | low | mechanical | weakened | filed:9177 |
| R82 | fk-e701ecedf198 | gui | `gui/src-tauri/src/watcher.rs::FileWatcher` | tests comments h9 kind:coverage | low | structural | confirmed | open |
| R83 | fk-7cc1c482ece2 | gui | `gui/src/bridge.ts::claudeClearSession` | inv-nothing-vacuous-and-unowned kind:dead-code | low | mechanical | confirmed | filed:9205 |
| R84 | fk-0819ab6ab79e | gui | `gui/src/editor/Editor.tsx::Editor` | h6 h9 h5 h4 comments kind:deep-read | low | structural | weakened | open |
| R85 | fk-9097c8a9fab7 | gui | `gui/src/editor/references.ts::previewForLocation` | kind:defect h1 comments | low | mechanical | confirmed | filed:9204 |
| R86 | fk-f24777406f66 | gui | `gui/src/types.ts::MultiCaseResult` | inv-sf-5 h1 comments kind:stub | low | mechanical | confirmed | filed:9206 |
| R87 | fk-f6c709836998 | kinematics-and-motion | `crates/reify-gcode/src/marlin.rs::parse_value` | kind:defect h10 | low | mechanical | confirmed | filed:9193 |
| R88 | fk-a8df59509cdb | kinematics-and-motion | `crates/reify-stdlib/src/flexures/prismatic.rs::SMALL_DEFLECTION_FRACTION` | h11 comments | low | mechanical | confirmed | filed:9191 |
| R89 | fk-2f1e2de126f0 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/gcode_import.rs::lower_commands` | kind:defect h12 | low | mechanical | confirmed | filed:9192 |
| R90 | fk-2fbadf54af70 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/sampling.rs::MotionTrajectory.resample_cubic` | h6 kind:dead-code | low | mechanical | confirmed | filed:8546 |
| R91 | fk-abc8e797fab9 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/simulate.rs` | comments h5 h6 | low | mechanical | weakened | filed:8546 |
| R92 | fk-d2541e0026dd | language-pipeline | `crates/reify-compiler/src/annotations/schema.rs::AnnotationSchema` | h1 h11 comments kind:deep-read | low | mechanical | weakened | filed:9187 |
| R93 | fk-400e69bbca2c | language-pipeline | `crates/reify-compiler/src/lib.rs` | tests h9 kind:coverage | low | structural | weakened | open |
| R94 | fk-ba1aa6d8a3b3 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::Engine.on_refine_trigger` | h10 comments | low | mechanical | confirmed | filed:9218 |
| R95 | fk-e51ded73acb1 | language-pipeline | `crates/reify-expr/src/calculus.rs::validate_differentiable_field` | h10 inv-SF-1 kind:deep-read | low | mechanical | weakened | filed:9208 |
| R96 | fk-819257c68b82 | language-pipeline | `crates/reify-expr/src/dual_eval.rs::builtin_partials` | h11 h12 kind:cross-module | low | structural | weakened | open |
| R97 | fk-168176511f09 | language-pipeline | `crates/reify-expr/src/lib.rs::EvalContext._test_at_depth` | tests h9 kind:coverage | low | mechanical | confirmed | filed:9212 |
| R98 | fk-2b679f63a55a | language-pipeline | `crates/reify-ir/src/arg_acceptance.rs` | h1 comments kind:deep-read | low | mechanical | weakened | filed:9210 |
| R99 | fk-ec1f6ad4538f | language-pipeline | `crates/reify-ir/src/arg_acceptance.rs::mass_spec` | inv-SF-5 kind:dead-code | low | mechanical | confirmed | filed:9210 |
| R100 | fk-60ca11154f6b | language-pipeline | `crates/reify-runtime/src/priority_promotion.rs::PriorityPromoter` | h6 kind:dead-code | low | mechanical | confirmed | filed:9211 |
| R101 | fk-e56835df6288 | reify-audit | `crates/reify-audit/src/bin/reify-audit.rs::load_tasks_from_fused_memory` | h11 h1 h10 | low | structural | weakened | open |
| R102 | fk-d8cb7c46751e | reify-audit | `crates/reify-audit/src/bin/reify-audit.rs::pattern_selects` | h12 | low | mechanical | confirmed | filed:9201 |
| R103 | fk-f8d63189bc4e | reify-audit | `crates/reify-audit/src/plinecite.rs::stale_baseline_entry` | h11 h1 | low | mechanical | confirmed | filed:9200 |
| R104 | fk-3dc9e2858c24 | reify-audit | `crates/reify-audit/src/ptodo.rs::tasks_db_path` | h11 h7 kind:convention | low | structural | weakened | open |
| R105 | fk-954243388ee9 | reify-audit | `crates/reify-audit/src/ratchet_ledger/regenerator.rs::Regenerator` | h11 h1 | low | structural | confirmed | open |
| R106 | fk-a8038b74107f | tooling | `crates/reify-cli/src/main.rs::cmd_gui` | tests h6 | low | mechanical | weakened | filed:9179 |
| R107 | fk-450af715ff4c | tooling | `crates/reify-cli/src/main.rs::cmd_report` | h1 comments | low | mechanical | confirmed | filed:8775 |
| R108 | fk-4cf4e3e2c407 | tooling | `crates/reify-cli/src/main.rs::cmd_test` | inv-sf-2 h10 tests | low | structural | weakened | open |
| R109 | fk-92dcfef08257 | tooling | `crates/reify-doc/src/fmt_html.rs::CrossRefIndex` | h3 h11 h1 | low | structural | confirmed | open |
| R110 | fk-adb958128dd9 | tooling | `crates/reify-doc/src/fmt_markdown.rs::render_params_table` | h1 h11 comments | low | structural | confirmed | open |
| R111 | fk-578105988fe2 | tooling | `crates/reify-lsp/src/analysis.rs` | h13 h9 | low | mechanical | confirmed | filed:8772 |
| R112 | fk-d5dfe97a5613 | tooling | `crates/reify-lsp/src/completion.rs` | h14 h13 | low | structural | weakened | open |
| R113 | fk-dc9c467ba3d2 | tooling | `crates/reify-lsp/src/completion.rs::LENGTH_GATED_EXAMPLES` | h10 comments | low | mechanical | confirmed | filed:7398 |
| R114 | fk-c833e442d9e3 | tooling | `crates/reify-lsp/src/decl_oracle.rs::test_fixtures` | h13 tests | low | mechanical | confirmed | filed:9178 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.1 | fk-cf610e43d544 | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::solve_elastic_static_trampoline` | h7 h3 h1 | high |  | confirmed | filed:8988 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.3 | fk-3f6ee7ecd393 | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::solve_elastic_static_trampoline` | h3 h4 h2 h12 comments | high |  | confirmed | filed:9118 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.2 | fk-34754bed8353 | analysis | `crates/reify-eval/src/modal_ops.rs::read_scalar_si` | h10 h11 h12 | high |  | confirmed | filed:8674 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.1 | fk-63218ffb51cd | analysis | `crates/reify-solver-elastic/src/form_find.rs::form_find_anchored_surfaces` | h11 h10 h3 | high |  | weakened | filed:8628 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.1 | fk-38811e6f7300 | analysis | `crates/reify-stdlib/src/dynamics/eval.rs::closed_chain_inverse_dynamics` | h11 h1 h4 comments | high |  | confirmed | filed:8845 |
| cg-ck.5 | fk-e719c0dc7f0d | constraints-and-geometry | `crates/reify-constraints/src/solver.rs` | h14 h13 tests h6 | high |  | confirmed | filed:8658 |
| stateless.1 | fk-88643a4cc993 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::check` | h7 h10 h5 | high |  | weakened | filed:9017 |
| cg-ck.1 | fk-e9cceaaac89a | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::replace_superseded_constraint_diagnostics` | h12 h11 h10 | high |  | weakened | filed:9024 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.1 | fk-59c46755d24c | constraints-and-geometry | `crates/reify-eval/src/topology_selectors.rs::parse_flat_number_object` | h12 h11 tests | high |  | confirmed | filed:8976 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.1 | fk-ba1ea4167dbe | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::OcctKernel.execute` | h10 h3 h1 h2 | high |  | weakened | filed:8669 |
| factory-infra/tests-infra-2-build-utils.2 | fk-ce79499430e2 | factory-infra | `crates/reify-build-utils/src/lib.rs::NativeDep` | h11 h12 comments h10 | high |  | confirmed | filed:8634 |
| factory-infra/scripts-1-hooks.2 | fk-d75787147427 | factory-infra | `scripts/prd-capability-check.py::run_probe` | h12 h10 h11 | high |  | confirmed | filed:8642 |
| factory-infra/scripts-1-hooks.3 | fk-1922380bc3aa | factory-infra | `scripts/prd-decompose-verify.py::bind_premises` | h10 h3 | high |  | confirmed | filed:8644 |
| factory-infra/scripts-1-hooks.1 | fk-21c29ddc104d | factory-infra | `scripts/reify_overlap_detector.py::_file_to_crate` | h11 h1 tests h10 | high |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| factory-infra/scripts-2.1 | fk-507b17f8adc7 | factory-infra | `scripts/verify.sh` | h12 tests h3 h9 | high |  | confirmed | filed:9098 |
| factory-infra/scripts-2.2 | fk-21149d55fd41 | factory-infra | `slug:verify-role-policy-scatter` | h3 h11 h10 | high |  | unverified | filed:9095 |
| factory-infra/tests-infra-1-cargo.1 | fk-acca603a9628 | factory-infra | `tests/infra/run_all.sh` | h3 h11 h4 h7 | high |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| factory-infra/tests-infra-1-cargo.2 | fk-53a9afc0d2b9 | factory-infra | `tests/infra/test_harness_kloc_cap.sh` | h6 h9 tests comments h13 | high |  | confirmed | filed:8700 |
| factory-infra/tests-infra-2-build-utils.1 | fk-902a5ad9889e | factory-infra | `tests/infra/test_nextest_slow_priority.sh::_role_wall_secs` | h12 tests h11 h7 | high |  | confirmed | filed:9103 |
| tests-infra-3.1 | fk-1417bf006761 | factory-infra | `tests/infra/test_slot_timeout_marker.sh` | h12 h6 h9 h14 comments | high |  | confirmed | filed:8694 |
| gui.1 | fk-05311be442e6 | gui | `gui/src-tauri/src/engine.rs` | h14 h9 h6 tests | high |  | confirmed | filed:8941 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.1 | fk-2be83576bc8b | gui | `gui/src/App.tsx::App` | h11 h10 tests | high |  | confirmed | filed:9063 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.3 | fk-252aa009dcdc | gui | `gui/src/App.tsx::App` | tests h9 h7 | high |  | confirmed | filed:9077 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.2 | fk-e0f49abf4b34 | gui | `gui/src/debug/bridge.ts::buildHandlers` | h10 h11 h4 h9 comments | high |  | confirmed | filed:9080 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.2 | fk-4543a3a88865 | kinematics-and-motion | `crates/reify-stdlib/src/joints.rs::JOINT_KINDS` | h12 h3 h11 h10 | high |  | weakened | filed:8852 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.1 | fk-14c3de3a675c | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure.rs::value_for_joint` | h11 h12 h3 comments | high |  | confirmed | filed:8840 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.2 | fk-6a6bbd16da97 | kinematics-and-motion | `crates/reify-stdlib/src/mechanism.rs::make_loop_closure_record` | h12 h10 h11 h7 | high |  | confirmed | filed:8837 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.1 | fk-337e4f0bcfa9 | kinematics-and-motion | `crates/reify-stdlib/src/snapshot.rs::eval_snapshot` | h11 h10 comments | high |  | confirmed | filed:8545 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.3 | fk-f584626a8627 | kinematics-and-motion | `crates/reify-stdlib/src/snapshot.rs::value_for` | h11 tests comments | high |  | confirmed | filed:8840 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.1 | fk-7bfe957c7f09 | language-pipeline | `crates/reify-compiler/src/datum_projection.rs::datum_projection_result_type` | h11 h10 kind:static-runtime-drift | high |  | confirmed | filed:8906 |
| p9 | fk-3318c3d043d6 | language-pipeline | `crates/reify-compiler/src/entity.rs` | h10 | high |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline.3 | fk-0bdf9598b402 | language-pipeline | `crates/reify-compiler/src/entity.rs::compile_entity` | h11 h5 h10 comments | high |  | confirmed | filed:8731 |
| language-pipeline.2 | fk-5a6c2aff4589 | language-pipeline | `crates/reify-compiler/src/expr.rs::compile_expr_guarded_with_expected_inner` | h1 h10 h11 | high |  | confirmed | filed:8898 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.1 | fk-28fab6d836c8 | language-pipeline | `crates/reify-compiler/src/lib.rs` | h13 h9 h7 | high |  | confirmed | filed:8636 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.2 | fk-d1ebb866f47b | language-pipeline | `crates/reify-eval/src/cell_commit.rs::commit_cell_result` | h10 inv-1 h11 | high |  | weakened | filed:8861 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.2 | fk-3ee0f9a51f25 | language-pipeline | `crates/reify-eval/src/cell_eval_ctx.rs::cell_eval_ctx` | h10 h11 comments tests | high |  | confirmed | filed:8872 |
| p16 | fk-23c462ee8918 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::propagate_freshness_only` | h6 | high |  | confirmed | filed:8699 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.2 | fk-f7e4716af878 | language-pipeline | `crates/reify-eval/src/engine_build.rs::build_with_geometry_output` | h11 h3 h10 | high |  | confirmed | filed:9159 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.1 | fk-53997e96ef7c | language-pipeline | `crates/reify-eval/src/engine_build.rs::execute_realization_ops` | h4 h2 h6 comments | high |  | confirmed | filed:9158 |
| p5 | fk-4696c8f0c6e4 | language-pipeline | `crates/reify-eval/src/engine_build.rs::execute_realization_ops` | h10 | high |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.2 | fk-263246810f8f | language-pipeline | `crates/reify-eval/src/engine_edit.rs::edit_param` | h4 h5 h2 comments | high |  | confirmed | filed:8863 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.1 | fk-8dd6a16ea574 | language-pipeline | `crates/reify-eval/src/engine_edit.rs::edit_source` | h11 h6 h10 kind:dead-parity-twin | high |  | confirmed | filed:8862 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.1 | fk-3ca766d13afa | language-pipeline | `crates/reify-eval/src/engine_eval.rs::Engine.eval_cached` | h11 h3 h4 | high |  | weakened | filed:8867 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.3 | fk-5534150bb053 | language-pipeline | `crates/reify-eval/src/engine_eval.rs::Engine.evaluate_let_bindings` | h11 h4 h12 | high |  | weakened | filed:8864 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.1 | fk-800d61acd5d0 | language-pipeline | `crates/reify-eval/src/lib.rs::Engine` | h13 h9 h5 h11 kind:god-struct | high |  | confirmed | filed:9050 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.2 | fk-32d11c0fd0ab | language-pipeline | `crates/reify-eval/src/lib.rs::eval_ctx_with_meta` | h10 h11 | high |  | confirmed | filed:8871 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.1 | fk-6f316376a4ba | language-pipeline | `crates/reify-eval/src/unfold.rs` | h11 h10 kind:parallel-path | high |  | confirmed | filed:8864 |
| language-pipeline.1 | fk-be87290e126e | language-pipeline | `crates/reify-expr/src/lib.rs::eval_expr` | h11 h3 comments tests | high |  | confirmed | filed:8896 |
| p7 | fk-42181a362efb | language-pipeline | `crates/reify-expr/src/lib.rs::evalcontext` | h10 | high |  | confirmed | filed:8872 |
| language-pipeline.1 | fk-047cb467bd07 | language-pipeline | `crates/reify-ir/src/geometry.rs::GeometryKernel` | h9 h10 tests | high |  | confirmed | filed:8969 |
| reify-audit.2 | fk-0b1d530f0264 | reify-audit | `crates/reify-audit/src/lib.rs::AuditContext` | h1 h5 kind:dead-interface | high |  | confirmed | filed:8807 |
| reify-audit.1 | fk-33f9891ad16c | reify-audit | `crates/reify-audit/src/lib.rs::Finding` | h12 h1 h11 tests | high |  | confirmed | filed:8809 |
| tooling.1 | fk-f5264d76e3ad | tooling | `crates/reify-cli/src/mcp_context.rs::CliToolContext` | h11 h10 h1 | high |  | confirmed | filed:8778 |
| tooling.1 | fk-bb84d4272cfd | tooling | `crates/reify-mcp/src/tools/write.rs::register` | h11 h1 h9 kind:dual-surface | high |  | confirmed | filed:8951 |
| layering.3 | fk-8cd376f72fca | analysis | `crates/reify-compute-contract/src/dispatch.rs::StructuredComputeDetail` | h9 h1 h6 | medium |  | confirmed | filed:9004 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.3 | fk-7d4f2be7b634 | analysis | `crates/reify-eval/src/compute_persist.rs::persistent_lookup` | h12 h11 h3 | medium |  | confirmed | filed:8994 |
| spot.3 | fk-cee09de8be7a | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::SOLVER_MAX_ITER` | h11 h10 h12 | medium |  | weakened | filed:8679 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.7 | fk-9facd9401295 | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::solve_cantilever_fea` | tests h4 h9 comments | medium |  | weakened | filed:9094 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.4 | fk-b5dc0cd8ec92 | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::value_from_elastic_result` | h11 h10 inv-2 | medium |  | confirmed | filed:8690 |
| layering.2 | fk-6e78ea52b691 | analysis | `crates/reify-eval/src/compute_targets/mod.rs::register_compute_fns` | h9 h6 h11 | medium |  | confirmed | filed:9008 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.4 | fk-34bddf1fcbce | analysis | `crates/reify-eval/src/engine_compute.rs::run_compute_dispatch` | h3 h12 h11 | medium |  | confirmed | filed:8994 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.8 | fk-7687550ec80f | analysis | `crates/reify-eval/src/modal_ops.rs` | h14 h6 tests h13 | medium |  | confirmed | filed:9120 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.6 | fk-603bbbab0515 | analysis | `crates/reify-eval/src/modal_ops.rs::build_beam_mesh` | h11 h1 | medium |  | confirmed | filed:9121 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.5 | fk-340a2cbd265b | analysis | `crates/reify-eval/src/modal_ops.rs::degenerate_modal_result` | h11 h1 inv-2 | medium |  | confirmed | filed:8693 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.10 | fk-cecdff4f1373 | analysis | `crates/reify-eval/src/modal_ops.rs::promote_beam_mesh_to_p2` | h1 tests h9 | medium |  | confirmed | filed:9089 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.9 | fk-9082d3765668 | analysis | `crates/reify-eval/src/modal_ops.rs::run_mechanism_modal` | h12 h1 | medium |  | confirmed | filed:8702 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.1 | fk-952285bb817b | analysis | `crates/reify-eval/src/shell_extract_compute.rs::value_to_shell_extraction_result` | h11 h12 h10 | medium |  | confirmed | filed:8996 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.6 | fk-d632489a4c98 | analysis | `crates/reify-mesh-morph/src/lib.rs::morph` | h1 tests | medium |  | weakened | filed:8527 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.5 | fk-acc10f2161f6 | analysis | `crates/reify-mesh-morph/src/stats.rs::snapshot` | h1 h11 h7 | medium |  | confirmed | filed:9085 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.7 | fk-4ac205f76343 | analysis | `crates/reify-shell-extract/src/medial.rs::sample_at_index` | h11 h13 h10 tests | medium |  | confirmed | filed:8711 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.8 | fk-90433ba69658 | analysis | `crates/reify-shell-extract/src/mid_surface.rs::extract_mid_surface` | h10 comments | medium |  | confirmed | filed:8714 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.4 | fk-a0d2e755b215 | analysis | `crates/reify-shell-extract/src/result.rs::read_f64_slab` | h11 h9 | medium |  | weakened | filed:8984 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.6 | fk-188e6a0adc9c | analysis | `crates/reify-solver-elastic/src/assembly/global.rs::AssemblyElement` | h10 h1 | medium |  | weakened | filed:8621 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.2 | fk-8f79315fc7e8 | analysis | `crates/reify-solver-elastic/src/membrane_load.rs::membrane_load_analysis` | h11 h3 | medium |  | confirmed | filed:8626 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.4 | fk-20c32882531a | analysis | `crates/reify-solver-elastic/src/mpc.rs::MpcRow` | h10 h1 comments | medium |  | confirmed | filed:8629 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.2 | fk-edde1bcefb9f | analysis | `crates/reify-solver-elastic/src/shell_assembly.rs::build_shell_frame` | h9 h13 | medium |  | confirmed | filed:8616 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.3 | fk-bc1e40784fe0 | analysis | `crates/reify-solver-elastic/src/shell_assembly.rs::degenerate_stiffness_core` | h11 h3 h4 | medium |  | confirmed | filed:8618 |
| spot.6 | fk-f33ff551c66d | analysis | `crates/reify-solver-elastic/src/shell_boundary.rs::SupportKind` | h11 h3 h12 | medium |  | weakened | filed:8683 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.6 | fk-a28b1b57ed64 | analysis | `crates/reify-stdlib/src/analysis.rs::REGISTRY_FREE_TYPE_ID` | h11 h10 | medium |  | confirmed | filed:8686 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.5 | fk-da538013331f | analysis | `crates/reify-stdlib/src/analysis.rs::compute_stress_invariants_3x3` | h10 h11 | medium |  | confirmed | filed:8705 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.9 | fk-a9b0785fd293 | analysis | `crates/reify-stdlib/src/dynamics/eval.rs::joint_force_value` | h12 h3 h11 | medium |  | weakened | filed:8847 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.3 | fk-dc44c395ee01 | analysis | `crates/reify-stdlib/src/dynamics/eval.rs::make_mass_properties` | h11 h9 | medium |  | confirmed | filed:8697 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.2 | fk-eca29487b2c9 | analysis | `crates/reify-stdlib/src/fea.rs::diagnose` | h10 h11 h7 | medium |  | confirmed | filed:8893 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.7 | fk-25c46b3f4c72 | analysis | `slug:dangling-plan-json-and-stale-line-pointers` | comments h13 | medium |  | unverified | filed:9128 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.5 | fk-95dd3ef3a20a | analysis | `slug:solver-elastic-integration-test-harness-copies` | tests h11 | medium |  | unverified | filed:8630 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.2 | fk-7fbe91c20e82 | constraints-and-geometry | `crates/reify-constraints/src/registry.rs::SolverRegistry` | comments h4 h2 | medium |  | confirmed | filed:9134 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.3 | fk-45c54d189935 | constraints-and-geometry | `crates/reify-constraints/src/relate_solve.rs::residual_dispatch` | h12 h11 h10 | medium |  | confirmed | filed:8830 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.6 | fk-b4af214cc9fe | constraints-and-geometry | `crates/reify-constraints/src/solvespace.rs::SystemBuilder` | h11 h10 | medium |  | weakened | filed:8752 |
| dimensions.1 | fk-80c2f82c17d6 | constraints-and-geometry | `crates/reify-eval/src/dispatcher.rs::dispatch` | h3 h11 h12 | medium |  | confirmed | filed:8998 |
| cg-ck.3 | fk-c944ff1a913b | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.check` | h7 h5 | medium |  | confirmed | filed:9017 |
| cg-ck.4 | fk-083c497dc973 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.collect_active_constraints` | h11 h10 | medium |  | weakened | filed:9029 |
| cg-ck.2 | fk-d98e178717e6 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.has_repr_capable_kernel` | h12 h3 h10 | medium |  | weakened | filed:8992 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.8 | fk-07c78c4ca710 | constraints-and-geometry | `crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector` | h11 h12 h3 | medium |  | weakened | filed:6944 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.3 | fk-e984283444b1 | constraints-and-geometry | `crates/reify-eval/src/geometry_ops/tests.rs` | tests h9 h14 | medium |  | confirmed | filed:8751 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.5 | fk-ca97659733e9 | constraints-and-geometry | `crates/reify-eval/src/relate_solve.rs::is_motion_joint_cell_type` | h12 h11 h3 | medium |  | confirmed | filed:8831 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.1 | fk-0375628ba585 | constraints-and-geometry | `crates/reify-eval/src/selector_vocabulary_v2.rs::extremal_by_bbox` | h12 h11 h9 | medium |  | confirmed | filed:8976 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.7 | fk-d0569c316d46 | constraints-and-geometry | `crates/reify-eval/src/structural_classifier.rs::classify_cell` | comments h13 | medium |  | confirmed | filed:9136 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.8 | fk-9f2a530a6921 | constraints-and-geometry | `crates/reify-eval/src/tolerance_combine.rs` | h6 h1 | medium |  | confirmed | filed:9019 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.10 | fk-e5a129d3cc24 | constraints-and-geometry | `crates/reify-eval/src/tolerance_combine.rs::conforms_to_trait` | h11 h1 comments | medium |  | confirmed | filed:9018 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.9 | fk-925d0004d647 | constraints-and-geometry | `crates/reify-eval/src/tolerance_combine.rs::realization_belongs_to` | h12 h11 tests | medium |  | weakened | filed:9020 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.4 | fk-25474a437b00 | constraints-and-geometry | `crates/reify-eval/src/topology_selectors.rs::resolve_against_carried_topology` | h11 h9 h13 | medium |  | confirmed | filed:8747 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.4 | fk-ef5f5da8f729 | constraints-and-geometry | `crates/reify-geometry/src/lib.rs::SingleKernelHolder` | h10 h1 tests | medium |  | confirmed | filed:8533 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.6 | fk-e6a266a72f41 | constraints-and-geometry | `crates/reify-kernel-fidget/src/kernel.rs::FidgetKernel` | h10 tests | medium |  | weakened | filed:8960 |
| cg-ck.8 | fk-92d313d2986f | constraints-and-geometry | `crates/reify-kernel-gmsh/src/init.rs::lock` | h10 h11 h7 | medium |  | weakened | filed:8285 |
| p1 | fk-24bac89dd7d8 | constraints-and-geometry | `crates/reify-kernel-gmsh/src/lib.rs` | h10 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| cg-ck.7 | fk-10ec3d2a1b76 | constraints-and-geometry | `crates/reify-kernel-manifold/src/kernel.rs::STUB_MSG` | h1 comments | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.2 | fk-e6bdef1ef0ae | constraints-and-geometry | `crates/reify-kernel-occt/src/handle.rs::OcctKernelHandle` | h10 h11 h1 | medium |  | confirmed | filed:8652 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.3 | fk-9eb90a5a82ff | constraints-and-geometry | `crates/reify-kernel-occt/src/handle.rs::OcctRequest` | h3 h9 h14 | medium |  | confirmed | filed:8672 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.5 | fk-d9cc4b340a69 | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::OcctKernel` | h11 h4 | medium |  | confirmed | filed:8666 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.4 | fk-baae0532b743 | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::OcctKernel.get_shape` | h10 h11 h1 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.2 | fk-030602c2fd62 | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::OcctKernel.query` | h12 h11 h9 | medium |  | weakened | filed:8976 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.6 | fk-10c48f52f01c | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::tests` | tests h11 h14 | medium |  | confirmed | filed:8659 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.5 | fk-107e28de9029 | constraints-and-geometry | `crates/reify-kernel-occt/src/stubs.rs::OcctKernel` | h10 tests h6 | medium |  | weakened | filed:8935 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.7 | fk-7e3c3d10ae46 | constraints-and-geometry | `crates/reify-stdlib/src/geometry.rs::diagnose` | h10 h3 h11 comments | medium |  | confirmed | filed:8891 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.7 | fk-76e1dc7c434e | constraints-and-geometry | `crates/reify-stdlib/src/lib.rs` | h3 h10 h11 | medium |  | confirmed | filed:8895 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.8 | fk-7bfc9a5014e2 | constraints-and-geometry | `crates/reify-stdlib/src/sweep.rs::driving_joint_kind` | h12 h11 | medium |  | confirmed | filed:8843 |
| cg-ck.6 | fk-e90cf61da43a | constraints-and-geometry | `slug:constraint-geometry-comment-drift` | comments h1 | medium |  | unverified | filed:9133 |
| factory-infra/tests-infra-1-cargo.5 | fk-e05e85e98f91 | factory-infra | `.cargo/run-with-occt.sh` | h11 comments | medium |  | confirmed | filed:8654 |
| factory-infra/tests-infra-2-build-utils.5 | fk-765ff49d2de5 | factory-infra | `crates/reify-build-utils/src/lib.rs::find` | h11 h10 h9 | medium |  | confirmed | filed:8646 |
| factory-infra/scripts-1-hooks.9 | fk-638f0a64b948 | factory-infra | `hooks/main-gate-lib.sh::main_gate_mark` | h10 h7 | medium |  | confirmed | filed:8671 |
| layering.7 | fk-448317051ff9 | factory-infra | `scripts/assert-crate-dag.sh` | h9 h10 | medium |  | weakened | filed:8958 |
| factory-infra/scripts-1-hooks.7 | fk-0497ab17e729 | factory-infra | `scripts/await-merge-landing.py::MergeStatusClient` | h11 h9 | medium |  | confirmed | filed:8664 |
| factory-infra/scripts-1-hooks.6 | fk-f34f07d2dde4 | factory-infra | `scripts/prd-capability-check.py::evaluate` | tests h7 h11 | medium |  | confirmed | filed:8649 |
| factory-infra/scripts-1-hooks.5 | fk-6a006c36a78d | factory-infra | `scripts/prd-decompose-verify.mjs::runWorkflow` | h1 tests h12 | medium |  | confirmed | filed:8655 |
| factory-infra/scripts-1-hooks.4 | fk-bf4e8bbb7dda | factory-infra | `scripts/prd-decompose-verify.py::_BLOCKING_VERDICTS` | h11 h1 h12 | medium |  | confirmed | filed:8651 |
| factory-infra/scripts-1-hooks.10 | fk-5f3dcd2ae885 | factory-infra | `scripts/seed-warm-lane.sh` | comments h13 | medium |  | confirmed | filed:9124 |
| stateless.4 | fk-60aa699ea8ce | factory-infra | `scripts/seed-warm-lane.sh` | h7 h11 h10 | medium |  | weakened | filed:8624 |
| factory-infra/scripts-1-hooks.8 | fk-fc5ee820d36e | factory-infra | `scripts/verify-pipeline-guard.sh::derive_plan_paths` | h12 h10 comments | medium |  | confirmed | filed:9100 |
| factory-infra/scripts-2.6 | fk-8c68164d0151 | factory-infra | `scripts/verify.sh` | comments h13 h1 | medium |  | confirmed | filed:8540 |
| factory-infra/scripts-2.7 | fk-b072e820e926 | factory-infra | `scripts/verify.sh::add_test_passes` | h11 h4 | medium |  | confirmed | filed:9099 |
| factory-infra/scripts-2.3 | fk-73c909381788 | factory-infra | `scripts/verify.sh::decide_scope` | h11 h10 kind:derived-copy | medium |  | confirmed | filed:9097 |
| dimensions.4 | fk-03d27a868cfd | factory-infra | `scripts/verify.sh::emit_nextest_pass` | h3 h12 h11 | medium |  | confirmed | filed:9095 |
| factory-infra/scripts-2.8 | fk-26e1e3d2eb67 | factory-infra | `scripts/warm-lane-audit.sh::_plan_anchor` | h12 h11 | medium |  | confirmed | filed:8647 |
| factory-infra/scripts-2.5 | fk-6fab601d98c2 | factory-infra | `scripts/warm-lane-audit.sh::_task_status_raw` | h11 comments | medium |  | confirmed | filed:8641 |
| factory-infra/scripts-2.4 | fk-05e33a09a266 | factory-infra | `scripts/warm-lane-lock-guard.sh` | h1 h11 | medium |  | confirmed | filed:8635 |
| tests-infra-3.3 | fk-465b5eec51e2 | factory-infra | `slug:line-number-citations-in-infra-test-prose` | comments h11 | medium |  | unverified | filed:9125 |
| factory-infra/tests-infra-1-cargo.3 | fk-9c87c8ca99b5 | factory-infra | `tests/infra/run-all-classification-lib.sh::classification_discovered_set` | h11 h10 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| factory-infra/tests-infra-1-cargo.7 | fk-91aaee6b8fde | factory-infra | `tests/infra/run_all.sh` | comments h13 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| factory-infra/tests-infra-1-cargo.6 | fk-3988a6996b82 | factory-infra | `tests/infra/test_compute_trampoline_registration_wired.sh` | h1 h6 h14 | medium |  | confirmed | filed:9111 |
| factory-infra/tests-infra-1-cargo.4 | fk-a86e30ad9ecb | factory-infra | `tests/infra/test_cpu_load_governance.sh` | h11 tests h10 h13 | medium |  | confirmed | filed:8707 |
| factory-infra/tests-infra-2-build-utils.6 | fk-7b476f26ad45 | factory-infra | `tests/infra/test_nan_safe_ordering_guard_wired.sh` | h10 h11 tests | medium |  | confirmed | filed:9106 |
| factory-infra/tests-infra-2-build-utils.4 | fk-37507523c365 | factory-infra | `tests/infra/test_no_new_wallclock_rust_deadlines.sh` | h14 h6 comments h13 | medium |  | confirmed | filed:8703 |
| factory-infra/tests-infra-2-build-utils.3 | fk-6ccbfc296ee8 | factory-infra | `tests/infra/test_occt_flock_gate.sh` | h6 h1 tests comments | medium |  | confirmed | filed:9110 |
| tests-infra-3.6 | fk-9c99c0223cb3 | factory-infra | `tests/infra/test_reify_audit_ptodo.sh` | h11 h4 h10 | medium |  | confirmed | filed:8712 |
| tests-infra-3.5 | fk-35b618e9b24d | factory-infra | `tests/infra/test_run_all.sh` | tests h7 h9 | medium |  | confirmed | filed:8692 |
| tests-infra-3.2 | fk-2e56a7e3fcb7 | factory-infra | `tests/infra/test_seed_warm_lane.sh` | h10 h7 tests | medium |  | confirmed | filed:8716 |
| tests-infra-3.4 | fk-34a184f51e6f | factory-infra | `tests/infra/test_test_helpers.sh` | tests h6 comments | medium |  | confirmed | filed:8726 |
| factory-infra/tests-infra-4.4 | fk-3cb220f9cc94 | factory-infra | `tests/infra/test_tree_sitter_pipeline.sh::run_tests` | h10 h11 h4 | medium |  | confirmed | filed:8725 |
| factory-infra/tests-infra-4.1 | fk-d47473e4d8d8 | factory-infra | `tests/infra/test_verify_scope.sh` | h12 h3 tests | medium |  | confirmed | filed:9105 |
| factory-infra/tests-infra-4.3 | fk-763b633b2775 | factory-infra | `tests/infra/test_verify_scope.sh::make_fixture` | h11 h13 | medium |  | confirmed | filed:9108 |
| factory-infra/tests-infra-4.2 | fk-7bc1c1449010 | factory-infra | `tests/infra/test_verify_semaphore_e2e.sh::_ts_ready_check` | h11 h10 | medium |  | confirmed | filed:9109 |
| factory-infra/tests-infra-4.5 | fk-4b80fe8f7de1 | factory-infra | `tests/infra/test_warm_lane_audit.sh::make_plan` | h12 tests | medium |  | confirmed | filed:8709 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.10 | fk-39619f02ae9c | gui | `gui/sidecar/src/session.ts::SidecarSession` | h4 h10 h6 | medium |  | confirmed | filed:9067 |
| gui.5 | fk-e2b409ec7580 | gui | `gui/src-tauri/src/debug_server.rs::dispatch_tool` | h11 h12 | medium |  | weakened | filed:8955 |
| gui.7 | fk-c4da71a0014e | gui | `gui/src-tauri/src/debug_server.rs::handle_set_fea_case` | h10 h7 | medium |  | confirmed | filed:8954 |
| dimensions.2 | fk-7212abff873d | gui | `gui/src-tauri/src/debug_server.rs::run_on_engine` | h3 h7 h10 | medium |  | confirmed | filed:8954 |
| gui.3 | fk-4ecb76174487 | gui | `gui/src-tauri/src/engine.rs::EngineSession` | h3 h11 h10 | medium |  | confirmed | filed:8947 |
| gui.4 | fk-2dcacd59c79d | gui | `gui/src-tauri/src/engine.rs::EngineSession` | h11 h10 h4 | medium |  | confirmed | filed:8942 |
| gui.2 | fk-b28eb5276628 | gui | `gui/src-tauri/src/engine.rs::set_active_fea_case` | h11 h10 | medium |  | confirmed | filed:8943 |
| gui.8 | fk-995c30a8a446 | gui | `gui/src-tauri/src/engine.rs::surface_geometry_derived_cells` | h12 h8 | medium |  | confirmed | filed:8944 |
| gui.6 | fk-58f6f0d58857 | gui | `gui/src-tauri/src/tests/debug_write_tool_routing_tests.rs` | h10 h12 tests | medium |  | confirmed | filed:8951 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.7 | fk-559caca373a7 | gui | `gui/src/App.tsx::App` | h10 h5 h2 | medium |  | confirmed | filed:9064 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.8 | fk-a03c96e5733d | gui | `gui/src/stores/autoViewGenerator.ts::isLetGeometryType` | h12 h11 comments | medium |  | confirmed | filed:9068 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.6 | fk-c8938e374057 | gui | `gui/src/stores/layoutStore.ts::createLayoutStore` | h11 h10 | medium |  | confirmed | filed:9066 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.9 | fk-3ac9bc99b963 | gui | `gui/src/stores/unitLadder.ts` | comments h11 h12 | medium |  | confirmed | filed:9147 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.4 | fk-b2129b18b706 | gui | `gui/src/stores/viewStateStore.ts::createViewStateStore` | h7 h5 comments | medium |  | confirmed | filed:9065 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.5 | fk-7262d24da57d | gui | `gui/src/types.ts` | h12 h11 comments | medium |  | confirmed | filed:9072 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.3 | fk-baa063db2052 | kinematics-and-motion | `crates/reify-eval/src/trajectory_ops.rs::worst_case_residual_fraction` | h1 comments h6 | medium |  | confirmed | filed:9087 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.9 | fk-3cfd0be186c0 | kinematics-and-motion | `crates/reify-stdlib/src/joints.rs::eval_joints` | h12 h13 h7 | medium |  | confirmed | filed:8852 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.5 | fk-22068b67e813 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure.rs` | comments h1 h14 | medium |  | confirmed | filed:9131 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.6 | fk-02810f753a67 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure.rs::extract_loop_closure_chains` | h10 h8 h1 | medium |  | confirmed | filed:8841 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.4 | fk-90837877eee7 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure.rs::strip_world_sentinel` | h11 h1 | medium |  | confirmed | filed:8837 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.7 | fk-67ca81ff9067 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure_solver.rs::SINGLE_LOOP_RESIDUAL_COUNT` | comments h1 | medium |  | confirmed | filed:9131 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.6 | fk-9b8484939308 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure_solver.rs::mechanism_loop_closure_chains` | h11 h10 comments | medium |  | confirmed | filed:8837 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.4 | fk-241b66aa0262 | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure_solver.rs::solve_loop_closure_with_diagnostics` | h11 h12 h9 h7 | medium |  | confirmed | filed:8842 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.5 | fk-691b37115ddf | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/input_shape.rs::eval_input_shape` | h11 h12 h6 | medium |  | confirmed | filed:8849 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.8 | fk-98f62f3f0562 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/tots.rs::TotsOutcome` | h12 h10 | medium |  | confirmed | filed:8850 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.7 | fk-06103e5f06eb | language-pipeline | `crates/reify-cli/src/mcp_context.rs::ensure_engine` | h10 h11 tests | medium |  | confirmed | filed:9039 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.3 | fk-33801a7c7a36 | language-pipeline | `crates/reify-compiler/src/builtin_registry.rs::registry_result_type` | h3 h11 comments | medium |  | confirmed | filed:6014 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.6 | fk-ffa4ba4c2d0b | language-pipeline | `crates/reify-compiler/src/builtin_signatures.rs::builtin_arg_slots` | h11 h3 comments | medium |  | confirmed | filed:6944 |
| language-pipeline.6 | fk-cf6356374fa1 | language-pipeline | `crates/reify-compiler/src/conformance/checker.rs::PreRegisterOutput` | h3 h11 h10 | medium |  | confirmed | filed:8738 |
| language-pipeline.7 | fk-254bc351a8ab | language-pipeline | `crates/reify-compiler/src/conformance/mod.rs::tests` | tests comments | medium |  | confirmed | filed:8736 |
| language-pipeline.9 | fk-37775e022693 | language-pipeline | `crates/reify-compiler/src/conformance/mod.rs::walk_param_against_arg_type` | h11 comments h2 | medium |  | confirmed | filed:8739 |
| language-pipeline.6 | fk-6460b80f5442 | language-pipeline | `crates/reify-compiler/src/expr.rs` | h13 h9 | medium |  | confirmed | filed:8627 |
| language-pipeline.7 | fk-935b6eca2b50 | language-pipeline | `crates/reify-compiler/src/expr.rs::compile_expr_guarded_with_expected_inner` | h3 h11 | medium |  | confirmed | filed:6014 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.5 | fk-27489259525d | language-pipeline | `crates/reify-compiler/src/forall_elaborate.rs::elaborate_forall_connect` | h12 h1 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.6 | fk-ea33d4b5d69d | language-pipeline | `crates/reify-compiler/src/geometry.rs::compile_geometry_call_inner` | h11 h12 h4 | medium |  | confirmed | filed:8901 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.7 | fk-2e03b9202f6d | language-pipeline | `crates/reify-compiler/src/geometry_boolean.rs::compile_boolean_op` | h12 h11 | medium |  | confirmed | filed:8899 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.10 | fk-a494d2285710 | language-pipeline | `crates/reify-compiler/src/geometry_modify.rs::compile_modify_op` | h12 h13 | medium |  | weakened | filed:8902 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.5 | fk-cc676a29f5d9 | language-pipeline | `crates/reify-compiler/src/joint_self_check.rs::check_joint_dof` | h9 h10 tests | medium |  | confirmed | filed:8740 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.8 | fk-6621fbaa0058 | language-pipeline | `crates/reify-compiler/src/member_path.rs::resolve_member_path` | h11 h1 | medium |  | confirmed | filed:5430 |
| spot.2 | fk-4bd71b633033 | language-pipeline | `crates/reify-compiler/src/recursion_guard.rs::MAX_COMPILE_RECURSION_DEPTH` | h11 h10 | medium |  | confirmed | filed:8558 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.6 | fk-cb941b3d058e | language-pipeline | `crates/reify-compiler/src/relation_signatures.rs::relation_delta_dof` | h11 h3 h12 h1 | medium |  | confirmed | filed:8741 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.3 | fk-a9fb39978c17 | language-pipeline | `crates/reify-compiler/src/scope.rs::CompilationScope` | h5 h9 h8 comments | medium |  | confirmed | filed:8650 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.9 | fk-60053402aba5 | language-pipeline | `crates/reify-compiler/src/stdlib_loader.rs::stdlib_sources` | h10 comments h11 | medium |  | confirmed | filed:5499 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.4 | fk-120b6255ec1c | language-pipeline | `crates/reify-compiler/src/termination.rs` | h13 h9 | medium |  | confirmed | filed:8617 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.4 | fk-30505ec518b4 | language-pipeline | `crates/reify-compiler/src/traits.rs::compile_purpose` | h12 h11 | medium |  | confirmed | filed:8733 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.10 | fk-196deab564e7 | language-pipeline | `crates/reify-compiler/src/traits.rs::unsupported_purpose_member_info` | h3 h11 h10 | medium |  | confirmed | filed:8732 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.7 | fk-78d6c776ba64 | language-pipeline | `crates/reify-compiler/src/type_compat.rs` | h13 h9 | medium |  | confirmed | filed:8620 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.6 | fk-fc9a2fe8498b | language-pipeline | `crates/reify-compiler/src/type_compat.rs::type_mentions_conflicted_param` | h11 h3 | medium |  | confirmed | filed:8638 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.4 | fk-eeae7ace09ad | language-pipeline | `crates/reify-compiler/src/type_resolution.rs::RESOLUTION_ENUM_NAMES` | h7 h5 comments h1 | medium |  | confirmed | filed:8643 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.5 | fk-d402f7243603 | language-pipeline | `crates/reify-compiler/src/type_resolution.rs::resolve_parameterized_builtin_type_with_subst` | h3 h11 | medium |  | confirmed | filed:8903 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.9 | fk-bb1ba38a6a46 | language-pipeline | `crates/reify-compiler/src/types.rs` | h10 h1 h11 comments | medium |  | weakened | filed:8661 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.5 | fk-bde20c8c3ed4 | language-pipeline | `crates/reify-compiler/src/units.rs` | h1 h6 h13 | medium |  | confirmed | filed:8622 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.5 | fk-15e94643506d | language-pipeline | `crates/reify-core/src/diagnostics.rs::DiagnosticCode` | comments h11 h14 tests | medium |  | confirmed | filed:9138 |
| spot.5 | fk-4718cc84e52a | language-pipeline | `crates/reify-core/src/diagnostics.rs::DiagnosticInfo` | h11 h12 inv-6 | medium |  | confirmed | filed:9028 |
| p17 | fk-e8a47806e557 | language-pipeline | `crates/reify-eval/src/cache.rs` | h10 | medium |  | confirmed | filed:8868 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.6 | fk-be356be3cbd5 | language-pipeline | `crates/reify-eval/src/cell_commit.rs::commit_cell_result_at` | h12 h1 | medium |  | confirmed | filed:8859 |
| spot.4 | fk-250d45d9d1f4 | language-pipeline | `crates/reify-eval/src/compute_targets/mod.rs::register_compute_fns` | h11 h12 | medium |  | confirmed | filed:8997 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.4 | fk-919c3ad5cc92 | language-pipeline | `crates/reify-eval/src/deps.rs::build_trace_map_and_fields` | h11 h10 | medium |  | confirmed | filed:8691 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.4 | fk-a6cb97b97c14 | language-pipeline | `crates/reify-eval/src/detectors.rs::DetectorRegistry` | h3 h11 comments | medium |  | confirmed | filed:8868 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.11 | fk-0ff49d22cff9 | language-pipeline | `crates/reify-eval/src/detectors.rs::DetectorRegistry` | h10 inv-3 comments | medium |  | confirmed | filed:8868 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.2 | fk-ba15d44904cb | language-pipeline | `crates/reify-eval/src/engine_admin.rs` | h6 h13 h5 h9 | medium |  | confirmed | filed:9051 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.8 | fk-d8fd9ed9be89 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::Engine` | tests h9 h5 | medium |  | confirmed | filed:9047 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.7 | fk-cb6c853cb648 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::cache_store_mut` | tests h9 | medium |  | confirmed | filed:9049 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.3 | fk-750e0c9f3bd6 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::parse_shell_extraction_result` | h11 h12 comments | medium |  | confirmed | filed:8996 |
| stateless.2 | fk-8250a05a9a10 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::trace_undef_causes` | h7 h5 h11 | medium |  | confirmed | filed:9044 |
| layering.4 | fk-4db43199eda2 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::with_registered_kernel` | h9 h7 h11 | medium |  | confirmed | filed:9038 |
| dimensions.5 | fk-4fa7bda0085b | language-pipeline | `crates/reify-eval/src/engine_admin.rs::with_registered_kernels_and_manifest` | h3 h11 h7 | medium |  | confirmed | filed:9036 |
| p6 | fk-c473cc3d0efa | language-pipeline | `crates/reify-eval/src/engine_build.rs::current_eval_version` | h11 | medium |  | weakened | open |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.4 | fk-eb5a9e37eb9f | language-pipeline | `crates/reify-eval/src/engine_build.rs::probe_realization_cache` | comments h1 h14 | medium |  | confirmed | filed:9141 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.3 | fk-17e2aa81f6d6 | language-pipeline | `crates/reify-eval/src/engine_build.rs::tessellate_from_values` | h10 comments h11 | medium |  | confirmed | filed:9160 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.3 | fk-de6e6f9e6b14 | language-pipeline | `crates/reify-eval/src/engine_edit.rs` | h10 h11 comments | medium |  | confirmed | filed:8867 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.7 | fk-a401e42a7b49 | language-pipeline | `crates/reify-eval/src/engine_eval.rs` | comments h13 h1 | medium |  | confirmed | filed:9142 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.5 | fk-d44472dae647 | language-pipeline | `crates/reify-eval/src/engine_eval.rs::Engine.eval_cached` | h10 | medium |  | weakened | filed:8866 |
| p15 | fk-f9ead5ab4db0 | language-pipeline | `crates/reify-eval/src/engine_eval.rs::invoke_compute_trampoline` | h10 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.11 | fk-76bdd62534fe | language-pipeline | `crates/reify-eval/src/engine_eval.rs::is_strict_connector_instance_auto` | h12 h11 | medium |  | confirmed | filed:8735 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.9 | fk-9915c9230418 | language-pipeline | `crates/reify-eval/src/engine_eval.rs::post_solver_re_eval_guard_cells` | h11 h3 h1 | medium |  | confirmed | filed:8865 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.6 | fk-813c018bf3cf | language-pipeline | `crates/reify-eval/src/engine_eval.rs::scope_qualifies_for_robustness_floor` | h11 h1 comments | medium |  | confirmed | filed:8645 |
| dimensions.3 | fk-98754d80a67e | language-pipeline | `crates/reify-eval/src/engine_fixpoint.rs::BuildScheduler` | h3 h12 tests | medium |  | confirmed | filed:4727 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.3 | fk-fb8cf3e787b9 | language-pipeline | `crates/reify-eval/src/engine_purposes.rs::Engine` | h8 h7 h10 h5 | medium |  | confirmed | filed:9045 |
| layering.1 | fk-f0a930933a38 | language-pipeline | `crates/reify-eval/src/geometry_ops.rs::is_geometry_consumer_call` | h9 h13 h6 | medium |  | confirmed | filed:8687 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.2 | fk-f0961047b0a6 | language-pipeline | `crates/reify-eval/src/graph.rs::EvaluationGraph` | h11 h4 h10 kind:identity-hash | medium |  | confirmed | filed:8677 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.10 | fk-420be0954017 | language-pipeline | `crates/reify-eval/src/lib.rs` | comments h13 h14 | medium |  | confirmed | filed:9142 |
| stateless.3 | fk-e9536a3edbe7 | language-pipeline | `crates/reify-eval/src/lib.rs::TessellateResult` | h7 h5 h12 | medium |  | confirmed | filed:9046 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.5 | fk-8188fcb77d66 | language-pipeline | `crates/reify-eval/src/resolve_order.rs::normalize_cell_id` | h12 h11 kind:string-identity | medium |  | weakened | filed:8704 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.8 | fk-d2c64e9d3868 | language-pipeline | `crates/reify-eval/src/snapshot.rs::Snapshot` | h1 h8 comments | medium |  | weakened | filed:9044 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.6 | fk-ecc0b4fb9d6b | language-pipeline | `crates/reify-expr/src/analysis.rs::sample_safety_factor_at_point` | h11 comments | medium |  | confirmed | filed:8908 |
| language-pipeline.8 | fk-50ebd24db52c | language-pipeline | `crates/reify-expr/src/lib.rs::eval_div` | h10 | medium |  | confirmed | filed:8550 |
| language-pipeline.4 | fk-a72f4510f36b | language-pipeline | `crates/reify-expr/src/lib.rs::eval_solve_load_cases` | h11 h6 h9 h10 | medium |  | confirmed | filed:8909 |
| language-pipeline.5 | fk-561a5a490532 | language-pipeline | `crates/reify-expr/src/lib.rs::sample_field_at` | h12 h1 h10 | medium |  | confirmed | filed:8907 |
| layering.5 | fk-66c1d2fe9ff9 | language-pipeline | `crates/reify-ir/src/geometry.rs` | h9 h13 h14 | medium |  | confirmed | filed:8930 |
| language-pipeline.8 | fk-e2730bcb8111 | language-pipeline | `crates/reify-ir/src/geometry.rs::GEOMETRY_OP_DESCRIPTORS` | comments h1 h11 | medium |  | confirmed | filed:9140 |
| language-pipeline.3 | fk-1d857a1e9750 | language-pipeline | `crates/reify-ir/src/geometry.rs::GeometryOp` | h3 h12 h11 | medium |  | confirmed | filed:8983 |
| language-pipeline.4 | fk-fdb4e221350a | language-pipeline | `crates/reify-ir/src/geometry.rs::GeometryQuery` | h12 h11 | medium |  | confirmed | filed:8979 |
| language-pipeline.2 | fk-a01d863c73ba | language-pipeline | `crates/reify-ir/src/geometry.rs::ValidatedMesh` | h10 h9 | medium |  | confirmed | filed:8971 |
| p11 | fk-44b665d07073 | language-pipeline | `crates/reify-ir/src/geometry.rs::topologyattributetable` | h12 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.7 | fk-08aa3fff3378 | language-pipeline | `crates/reify-ir/src/kernel_validation.rs::check_length_field` | h11 comments h1 | medium |  | confirmed | filed:8982 |
| language-pipeline.5 | fk-7b093573458b | language-pipeline | `crates/reify-ir/src/value.rs::StructureInstanceData` | h12 h10 | medium |  | weakened | filed:8934 |
| p14 | fk-7bfef234d79e | language-pipeline | `crates/reify-test-support/src/mocks.rs` | tests | medium |  | confirmed | filed:8964 |
| language-pipeline/eval-freshness-walk-compiler-member-path-expr-field-reductions-ir-constr.12 | fk-d149797f802f | language-pipeline | `tree-sitter-reify/tests/build_logic_tests.rs` | tests h11 h14 | medium |  | confirmed | filed:8553 |
| reify-audit.8 | fk-31eb23b6b93f | reify-audit | `crates/reify-audit/src/bin/reify-audit.rs::high_severity_exit_code` | h1 h12 h10 | medium |  | confirmed | filed:8814 |
| reify-audit.4 | fk-c45888cf175b | reify-audit | `crates/reify-audit/src/lib.rs::GitOps` | h10 h3 h12 | medium |  | confirmed | filed:8810 |
| reify-audit.5 | fk-a26634db4007 | reify-audit | `crates/reify-audit/src/lib.rs::Pattern` | comments h1 | medium |  | confirmed | filed:9145 |
| reify-audit.7 | fk-20ce222ff90c | reify-audit | `crates/reify-audit/src/p5_phantom_done.rs::check_one` | h2 h4 h11 | medium |  | confirmed | filed:8813 |
| reify-audit.6 | fk-bdc845122da0 | reify-audit | `crates/reify-audit/src/ptodo.rs::open_tasks_db` | h9 h7 h1 | medium |  | confirmed | filed:8811 |
| reify-audit.9 | fk-422bbd7986a6 | reify-audit | `crates/reify-audit/src/ptodo.rs::resolve_liveness` | tests h9 h13 | medium |  | confirmed | filed:8819 |
| reify-audit.3 | fk-ec468bfa673a | reify-audit | `slug:reify-audit-source-shape-recognisers` | h11 h10 h13 | medium |  | unverified | filed:8812 |
| p20 | fk-2ec6ef96fa1c | repo | `slug:extract-but-abandon-at-3-of-n-sites` | h11 | medium |  | unverified | filed:8858 |
| spot.1 | fk-d668027f44f2 | repo | `slug:invariant-registry-status-not-derived-from-enforcement` | h11 h10 comments | medium |  | unverified | filed:8821 |
| tooling.4 | fk-c24f5d7302ed | tooling | `crates/reify-cli/src/main.rs::cmd_check` | h3 h4 h11 | medium |  | confirmed | filed:9022 |
| tooling.6 | fk-677d67cb2ac8 | tooling | `crates/reify-config/src/lib.rs::Manifest` | h1 comments h6 | medium |  | weakened | filed:8764 |
| tooling.2 | fk-7b9fad8a5d00 | tooling | `crates/reify-lsp/src/bridge.rs::InProcessLsp` | h12 h11 comments | medium |  | confirmed | filed:8760 |
| p19 | fk-2456b4804912 | tooling | `crates/reify-lsp/src/diagnostics.rs` | h10 | medium |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| tooling.5 | fk-41819ea934bd | tooling | `crates/reify-lsp/src/diagnostics.rs::compute_diagnostics` | h11 tests h1 | medium |  | confirmed | filed:8759 |
| tooling.2 | fk-bf1370c79e89 | tooling | `crates/reify-lsp/src/diagnostics.rs::compute_diagnostics_with_state` | h12 h11 h4 | medium |  | confirmed | filed:9025 |
| tooling.6 | fk-b420a7695f68 | tooling | `crates/reify-lsp/src/diagnostics.rs::compute_diagnostics_with_state` | h11 h10 | medium |  | confirmed | filed:8771 |
| tooling.9 | fk-14195f0a11d2 | tooling | `crates/reify-lsp/src/diagnostics/eval_guard.rs` | comments h13 | medium |  | confirmed | filed:9146 |
| tooling.3 | fk-d9b1ed83ed62 | tooling | `crates/reify-lsp/src/hover.rs::compute_hover_in_context` | h4 h2 h10 tests | medium |  | confirmed | filed:8773 |
| tooling.3 | fk-3a3ae797f979 | tooling | `crates/reify-lsp/src/references.rs::entity_members` | h11 h3 h10 | medium |  | confirmed | filed:8772 |
| tooling.7 | fk-81f949615552 | tooling | `crates/reify-lsp/src/references.rs::is_reserved_keyword` | h1 h11 | medium |  | confirmed | filed:8910 |
| tooling.8 | fk-1a772d160168 | tooling | `crates/reify-lsp/src/server.rs::ReifyLanguageServer` | h3 h10 | medium |  | confirmed | filed:8774 |
| tooling.5 | fk-2da836d68189 | tooling | `crates/reify-mcp/src/tools/language_chunks.rs::get_chunk` | h11 h12 h13 | medium |  | confirmed | filed:8763 |
| tooling.4 | fk-cc4329a75e3e | tooling | `crates/reify-mcp/src/types.rs::ParameterInfo` | h12 h11 | medium |  | confirmed | filed:8952 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.12 | fk-b0a7b8818be4 | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs` | comments h1 | low |  | confirmed | filed:9129 |
| analysis/eval-compute-targets-eval-modal-ops-solver-elastic-lib-solver-elastic-ev.11 | fk-2ac688334abf | analysis | `crates/reify-eval/src/compute_targets/elastic_static.rs::PressureSpec` | h12 h1 | low |  | confirmed | filed:8683 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.9 | fk-43921c86c7d3 | analysis | `crates/reify-eval/src/dynamics_ops.rs::InverseDynamicsRun` | h6 tests h13 | low |  | confirmed | filed:9088 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.8 | fk-d870b2b3bfdf | analysis | `crates/reify-eval/src/engine_compute.rs::OptimizedComputeDispatcher` | h11 h10 | low |  | confirmed | filed:8525 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.11 | fk-e6650962065a | analysis | `crates/reify-eval/src/engine_compute.rs::tests` | h13 tests | low |  | confirmed | filed:8991 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.12 | fk-c02d5d2d8515 | analysis | `crates/reify-eval/src/shell_extract_compute.rs::shell_extract_compute_fn` | h4 h2 | low |  | confirmed | filed:8706 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.7 | fk-12285c864993 | analysis | `crates/reify-eval/src/trajectory_ops.rs::value_size_estimate` | h11 | low |  | confirmed | filed:9086 |
| analysis/solver-elastic-elements-eval-engine-compute-solver-elastic-assembly-fdm.10 | fk-ebfb3161f8cd | analysis | `crates/reify-fdm/src/toolpath.rs::compute_adjacency_with_stats` | h11 h10 comments | low |  | confirmed | filed:8526 |
| stateless.7 | fk-e3acedd1c27a | analysis | `crates/reify-mesh-morph/src/diagnostics.rs::reset` | h7 tests | low |  | confirmed | filed:9092 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.8 | fk-66ffd82ed984 | analysis | `crates/reify-solver-elastic/src/interpolation.rs::TetSpatialIndex` | h7 h10 | low |  | weakened | filed:8637 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.9 | fk-473bb6240e81 | analysis | `crates/reify-solver-elastic/src/qoi.rs` | h14 comments tests | low |  | confirmed | filed:8640 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.11 | fk-824124d1e74e | analysis | `crates/reify-solver-elastic/src/shell_result.rs::ShellStress` | h6 comments | low |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| dimensions.6 | fk-c940cb1edad4 | analysis | `crates/reify-solver-elastic/src/solver.rs::resolve_execution_modes` | h3 h11 | low |  | weakened | filed:8375 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.7 | fk-c497935d4b8a | analysis | `crates/reify-solver-elastic/src/volume_refine.rs::map_geometry_error` | h12 h11 | low |  | weakened | filed:8935 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.10 | fk-9bc22225093b | analysis | `crates/reify-stdlib/src/fea.rs::grids_equal` | h11 h10 | low |  | confirmed | filed:8717 |
| analysis/solver-elastic-qoi-stdlib-modal-eval-compute-persist-solver-elastic-mpc.10 | fk-e4b87a3c5f90 | analysis | `slug:solver-analysis-drifted-pointers-and-stale-status-prose` | comments h1 | low |  | unverified | filed:9128 |
| analysis/shell-extract-mesh-morph-stdlib-dynamics-stdlib-fea-solver-elastic-solve.11 | fk-632e4850d7d0 | analysis | `slug:stale-line-citations-and-reviewer-prose-in-analysis-stdlib` | comments h13 | low |  | unverified | filed:9130 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.9 | fk-227058959fa6 | constraints-and-geometry | `crates/reify-constraints/src/dependent_reads.rs::collect_value_refs` | h1 h11 | low |  | weakened | open |
| layering.8 | fk-6875d651562b | constraints-and-geometry | `crates/reify-constraints/src/lib.rs` | h9 h13 comments | low |  | confirmed | filed:8560 |
| cg-ck.10 | fk-23f520f22f28 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.dispatch_constraints` | h2 h4 comments | low |  | weakened | filed:9016 |
| cg-ck.9 | fk-72f83d1ef8c0 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.measure_dfm_rules` | h12 h3 | low |  | weakened | filed:9027 |
| cg-ck.11 | fk-747e27932117 | constraints-and-geometry | `crates/reify-eval/src/engine_constraints.rs::Engine.measure_gdt_conformance` | h11 comments | low |  | confirmed | filed:9026 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.10 | fk-9e6d548589d8 | constraints-and-geometry | `crates/reify-eval/src/realization_content.rs::project_realization_read_handle` | h11 comments h1 | low |  | confirmed | filed:8534 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.11 | fk-9063f57c1daf | constraints-and-geometry | `crates/reify-eval/src/realization_staleness.rs::sub_consumers_the_reverse_index_drops` | h11 h12 | low |  | confirmed | filed:8701 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.11 | fk-bef6f2b4851c | constraints-and-geometry | `crates/reify-eval/src/warm_pool.rs::WarmStatePool` | h9 h1 tests | low |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| stateless.5 | fk-a0ce1853f2da | constraints-and-geometry | `crates/reify-kernel-manifold/src/kernel.rs::manifold_from_reify_mesh` | h7 h11 | low |  | weakened | filed:8971 |
| constraints-and-geometry/stdlib-sweep-constraints-relate-solve-constraints-registry-kernel-fidget.12 | fk-68d69d7d9ace | constraints-and-geometry | `crates/reify-kernel-occt/src/ffi.rs::ffi` | h3 h11 | low |  | confirmed | filed:8676 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.10 | fk-9e30579d2c52 | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs` | comments h13 | low |  | confirmed | filed:8531 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.9 | fk-69bd53399b08 | constraints-and-geometry | `crates/reify-kernel-occt/src/lib.rs::OcctKernel.warm_state` | h6 comments h5 | low |  | confirmed | filed:8682 |
| layering.9 | fk-65bb1ffb5d60 | constraints-and-geometry | `crates/reify-kernel-occt/src/projector_impl.rs` | h9 h1 | low |  | confirmed | filed:8561 |
| constraints-and-geometry/kernel-occt-lib-eval-geometry-ops-stdlib-geometry-eval-realization-cache.11 | fk-d22418138fa2 | constraints-and-geometry | `crates/reify-kernel-occt/src/stubs.rs::OcctKernel` | h1 h10 | low |  | weakened | filed:8672 |
| constraints-and-geometry/kernel-openvdb-eval-topology-selectors-kernel-occt-handle-eval-tolerance.12 | fk-0f933e75bf7d | constraints-and-geometry | `crates/reify-kernel-openvdb/src/lib.rs` | h1 comments h13 | low |  | confirmed | filed:8935 |
| factory-infra/scripts-1-hooks.12 | fk-09edb7dfde69 | factory-infra | `scripts/cutover-reify-types.py` | h6 | low |  | confirmed | filed:8539 |
| factory-infra/scripts-2.9 | fk-cee9ca6b041a | factory-infra | `scripts/warm-lane-gc.sh::_do_reclaim` | h4 h2 | low |  | confirmed | filed:8668 |
| factory-infra/scripts-2.10 | fk-7dba1d7f3f47 | factory-infra | `scripts/warm-lane-gc.sh::_usage` | h1 | low |  | confirmed | filed:8541 |
| factory-infra/tests-infra-4.6 | fk-cb27e6ba131b | factory-infra | `slug:infra-test-navigation-prose-rot` | comments h13 | low |  | unverified | filed:9126 |
| factory-infra/tests-infra-2-build-utils.8 | fk-4186fb4f7289 | factory-infra | `slug:line-number-citations-in-infra-prose` | comments h11 | low |  | unverified | filed:9123 |
| factory-infra/scripts-1-hooks.11 | fk-d30433df0ed6 | factory-infra | `slug:seed-warm-lane-flag-capability-grep` | h12 h1 | low |  | unverified | filed:8670 |
| factory-infra/tests-infra-4.7 | fk-6e371c1b8c6f | factory-infra | `slug:stale-red-until-tdd-labels` | h1 comments | low |  | unverified | open |
| factory-infra/tests-infra-2-build-utils.7 | fk-c796ce463c73 | factory-infra | `tests/infra/test_jobserver_balancer.sh` | h14 tests | low |  | confirmed | filed:8727 |
| tests-infra-3.8 | fk-4229ece8d26d | factory-infra | `tests/infra/test_run_all.sh` | h11 | low |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| tests-infra-3.7 | fk-1c2e90be3491 | factory-infra | `tests/infra/test_seed_warm_lane.sh` | h11 h4 | low |  | confirmed | filed:8716 |
| tests-infra-3.9 | fk-6213e3dfdbec | factory-infra | `tests/infra/test_seed_warm_lane.sh` | h1 | low |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| factory-infra/tests-infra-4.8 | fk-75e28dab2f08 | factory-infra | `tests/infra/test_warm_lane_gc.sh::_seed_stub_body` | h11 h4 | low |  | confirmed | filed:8721 |
| gui.9 | fk-55213deb2362 | gui | `gui/src-tauri/src/engine.rs` | h12 h11 | low |  | confirmed | filed:8945 |
| gui.10 | fk-70d435aa0170 | gui | `gui/src-tauri/src/engine.rs` | comments h1 | low |  | confirmed | filed:8542 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.12 | fk-5067404e6b75 | gui | `gui/src/__tests__/viewStateStore.test.ts` | tests comments | low |  | confirmed | filed:8544 |
| layering.11 | fk-b131e38ebad7 | gui | `gui/src/panels/DiagnosticsPanel.tsx::DiagnosticEntry` | h9 h13 | low |  | confirmed | filed:8563 |
| gui/gui-stores-gui-root-gui-debug-gui-other-gui-hooks-gui-components-gui-uti.11 | fk-068d91243153 | gui | `gui/src/stores/viewPersistence.ts::isPersistentViewState` | h11 | low |  | confirmed | filed:8543 |
| layering.10 | fk-0b62433f5807 | gui | `gui/src/types.ts::PersistentViewState` | h9 h13 | low |  | confirmed | filed:9072 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.10 | fk-f9623eaac8de | kinematics-and-motion | `crates/reify-gcode/src/klipper.rs::parse_kv_params` | h12 h10 | low |  | confirmed | filed:8548 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.8 | fk-6467cfff64bc | kinematics-and-motion | `crates/reify-stdlib/src/loop_closure.rs::per_joint_jacobian_local` | h6 h12 | low |  | confirmed | filed:8833 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.11 | fk-49105d89fd7b | kinematics-and-motion | `crates/reify-stdlib/src/snapshot.rs::make_binding` | h13 h9 | low |  | confirmed | filed:8839 |
| kinematics-and-motion/stdlib-trajectory-stdlib-joints-stdlib-flexures-stdlib-snapshot-stdlib-o.10 | fk-d49d3ac15f14 | kinematics-and-motion | `crates/reify-stdlib/src/trajectory/tots.rs` | h5 comments h9 | low |  | confirmed | filed:8546 |
| kinematics-and-motion/stdlib-loop-closure-stdlib-mechanism-gcode-eval-trajectory-ops.9 | fk-679840f73698 | kinematics-and-motion | `slug:gcode-untracked-plan-design-decision-pointers` | comments h13 | low |  | unverified | filed:9132 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.9 | fk-75d9313c6ee6 | language-pipeline | `crates/reify-compiler/src/ambient_defaults.rs::AmbientDefaults` | h1 comments | low |  | confirmed | filed:8556 |
| language-pipeline.10 | fk-726aff24a191 | language-pipeline | `crates/reify-compiler/src/conformance/mod.rs::resolve_joint_nominal_type` | h11 h12 | low |  | weakened | filed:8831 |
| spot.8 | fk-57ffd6bd2af2 | language-pipeline | `crates/reify-compiler/src/module_pragmas.rs::KNOWN_V02_KERNELS` | h11 h12 | low |  | weakened | filed:8987 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.8 | fk-e187a67f4b32 | language-pipeline | `crates/reify-compiler/src/type_compat.rs::resolve_binop` | h12 h11 | low |  | confirmed | filed:8904 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.9 | fk-eeebffcc4a83 | language-pipeline | `crates/reify-compiler/src/units.rs::is_surface_producing_arg` | h11 h1 h12 | low |  | confirmed | filed:8905 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.10 | fk-2e1e82c1d1c6 | language-pipeline | `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic` | comments h6 | low |  | confirmed | filed:9139 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.9 | fk-863955f49c19 | language-pipeline | `crates/reify-core/src/ty.rs` | comments h13 h9 | low |  | confirmed | filed:9143 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.8 | fk-013bfbef8746 | language-pipeline | `crates/reify-eval/src/cache.rs::dirty_set` | h11 h6 h1 | low |  | confirmed | fixed:0e60fa138d8852e1e26999d6dd22d05964848740 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.12 | fk-73145187f8a6 | language-pipeline | `crates/reify-eval/src/dirty.rs::compute_dirty_cone_with_realizations` | h11 h10 h1 | low |  | confirmed | filed:8695 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.11 | fk-c88bf8dc4754 | language-pipeline | `crates/reify-eval/src/engine_admin.rs::realization_kernel_provenance` | h12 h11 | low |  | confirmed | filed:8555 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.11 | fk-7378f0be931f | language-pipeline | `crates/reify-eval/src/engine_demand.rs::Engine.add_demand` | h7 h9 tests | low |  | weakened | filed:8680 |
| language-pipeline/test-support-eval-engine-eval-compiler-geometry-modify-spec-conformance.12 | fk-82271f54167a | language-pipeline | `crates/reify-eval/src/engine_eval.rs::emit_param_override_rejection_warning` | tests h12 | low |  | confirmed | filed:8869 |
| language-pipeline/eval-resolve-order-eval-lib-compiler-annotations-compute-contract-eval-g.11 | fk-ee1be91862ca | language-pipeline | `crates/reify-eval/src/engine_purposes.rs::expand_purpose_reflective_placeholders` | h12 h11 | low |  | confirmed | filed:8733 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.7 | fk-6432e37b9f24 | language-pipeline | `crates/reify-eval/src/persistent_cache.rs::PersistentlyCacheable` | h13 kind:reexport-shim | low |  | confirmed | filed:8551 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.8 | fk-4c92d87f92ce | language-pipeline | `crates/reify-eval/src/persistent_cache.rs::tests` | tests h13 h6 | low |  | weakened | filed:8876 |
| stateless.6 | fk-3aeb721eaaa6 | language-pipeline | `crates/reify-eval/src/snapshot.rs::Snapshot` | h7 h10 | low |  | confirmed | filed:8564 |
| language-pipeline/core-eval-engine-build-compiler-type-compat-expr-kleene.11 | fk-2845dd9262b4 | language-pipeline | `crates/reify-expr/src/kleene.rs::KBool` | h11 | low |  | confirmed | filed:8549 |
| language-pipeline.9 | fk-5b6284e261af | language-pipeline | `crates/reify-expr/src/lib.rs` | comments h14 | low |  | confirmed | filed:9144 |
| language-pipeline/eval-cache-eval-engine-admin-compiler-builtin-signatures-compiler-units.10 | fk-3d9aa29e205a | language-pipeline | `crates/reify-ir/src/expr.rs::TAG_STRUCTURE_INSTANCE_CTOR` | h10 comments | low |  | confirmed | filed:8554 |
| spot.7 | fk-d30cf96fc557 | language-pipeline | `crates/reify-ir/src/value.rs::Value` | h11 | low |  | confirmed | filed:8698 |
| language-pipeline/syntax-ts-parser-eval-engine-edit-compiler-type-resolution-compiler-geom.9 | fk-39797230491b | language-pipeline | `crates/reify-syntax/src/lib.rs` | h13 kind:reexport-shim | low |  | confirmed | filed:8552 |
| language-pipeline/compiler-orientation-signatures-eval-compute-cache-key-compiler-si-units.12 | fk-ddfdce77efbc | language-pipeline | `crates/reify-syntax/src/member_continuation.rs::is_member_continuation_message` | h12 | low |  | confirmed | filed:8742 |
| dimensions.7 | fk-ba93c26e42d1 | reify-audit | `crates/reify-audit/src/p5_phantom_done.rs::pre_done_refusal_severity` | h3 h7 h1 | low |  | weakened | filed:8815 |
| tooling.10 | fk-972b7d28a290 | tooling | `crates/reify-cli/src/dev.rs::parse_node_id` | h11 h12 | low |  | confirmed | filed:8768 |
| tooling.9 | fk-10941c98934e | tooling | `crates/reify-doc-build/src/build.rs::build_doc_model` | h3 h12 h1 | low |  | confirmed | filed:8767 |
| tooling.11 | fk-419ebb996480 | tooling | `crates/reify-lsp/src/completion.rs::BUILTIN_FUNCTIONS` | h11 | low |  | confirmed | filed:6012 |
| tooling.8 | fk-3e8bf5666555 | tooling | `crates/reify-lsp/src/convert.rs::convert_diagnostic` | h11 comments h1 | low |  | confirmed | filed:8762 |
| tooling.12 | fk-5854202d4aed | tooling | `crates/reify-lsp/src/diagnostics.rs::EvalState` | tests h9 | low |  | confirmed | filed:8759 |
| tooling.10 | fk-74ce7cce677f | tooling | `crates/reify-lsp/src/server.rs::build_cross_file_rig` | h11 | low |  | weakened | filed:8769 |
| tooling.7 | fk-bed8cadb429f | tooling | `crates/reify-mcp/src/jsonrpc.rs::JsonRpcRequest` | h10 h11 | low |  | confirmed | filed:8765 |

## New findings — statements

### R1 · fk-67feaf8cc7e2 · high · confirmed · open
**gui** `gui/src/stores/claudeStore.ts::createClaudeStore` — h11 h10 tests kind:critical-path

Heuristic 11 (SPOT): one chat turn gets its id from two independent minters that are never reconciled, so the chat panel cannot correlate any lifecycle event with its assistant message. claudeStore.sendMessage mints `msg-${Date.now()}-${n}` (generateId, claudeStore.ts:80) and sets it as currentMessageId and as the assistant message id. App's onSend handler then discards it (`onSend: (_id, text, context) => claudeSendMessage(text, context)`, App.tsx:465). bridge.claudeSendMessage returns Promise<void> and drops the id that the Rust command returns (bridge.ts:323). Rust claude_bridge.rs send_message mints its own `msg-{n}` from MSG_COUNTER, writes it to the sidecar and returns it (claude_bridge.rs:581-596). The sidecar echoes that id on every outbound event, and outbound_to_event re-emits it unchanged. In claudeStore.handleOutboundMessage, text_delta and thinking_delta append to state.currentMessageId, so reply text renders. tool_call, tool_result, done and error all look up findAssistantIdx(msg.id) (claudeStore.ts:173, 195, 218, 254), which can never match, so each breaks out early. As a result: no tool-call card is ever added (MessageGroup renders props.message.toolCalls); `complete` and `thinkingComplete` are never set, so MessageGroup's streaming={!props.message.complete} stays true for every finished reply; and an error is never attached to its turn. This breaks the briefing key scenario 'The Claude chat panel drives the sidecar'. It is separate from the permission-envelope defect the S26 PRD found (alpha #9062). Neither the S25 nor the S26 PRD mentions it. The tests mask it: ChatPanel.test.tsx:114-116 and claudeIntegration.test.ts:81-86 feed store.state.currentMessageId back in as the event id, an id the real producer never sends.

*Proposal:* Make one side the id minter. Either the store passes its id through claude_send_message (the Rust command accepts the id and forwards it), or the store takes the id that claudeSendMessage returns, under the S26 bridge port. Then add one test that drives the real bridge mapping with a producer-shaped id, not the store's own id.

### R2 · fk-f8cb4ed96ae8 · high · confirmed · filed:9207
**language-pipeline** `crates/reify-core/src/units.rs::ri_compound_unit_expr` — inv-SF-7 inv-SF-5 h10 kind:convention

The `TODO(#7151)` at units.rs:397 cites task #7151, which is `done`, so the cite is orphaned (TODO convention). #7151 was closed on the comment-only merge 634a3a0af0 (`Merge task/annotate-fracture-toughness-carveout`, +11 comment lines in units.rs). Its done_provenance names that merge, and its own metadata.x_do_not_infer_done warned that exactly this commit must never mark it done. The two costs #7151 owned are both still live. (1) INV-SF-7 parse-is-value-faithful is violated. The grammar exponent is still `signed_integer: token.immediate(/-?\d+/)` (tree-sitter-reify/grammar.js:1970), and `UnitExpr::Pow(Box<UnitExpr>, i32)` is still integral (reify-ast/src/ast.rs:204). So `50MPa*m^1/2` silently evaluates to 25,000,000 N/m: the `/2` falls out of the unit expression and becomes a value division, with zero diagnostics. (2) INV-SF-5: the FractureToughness carve-out (`let exponent = r.as_i8()?;` silently drops any rational-exponent dimension) and its sibling assertions are unretired, for example the 'fractional-exponent exemption' message at crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2983. Nothing now owns either cost.

*Proposal:* Reopen #7151 (set pending, clear the bogus done_provenance), or file a successor and re-cite the TODO to it in the same commit as its ptodo-baseline row. Its R3 cheaper route (emit `sqrt(1m)` for rational exponents) plus a parse error at the `^`/`/` boundary would close both halves.

### R3 · fk-4266f4222b24 · high · confirmed · filed:6504
**reify-audit** `crates/reify-audit/src/jcodemunch_client.rs::filter_refs_to_file` — h1 tests kind:critical-path

Key scenario 1 (P1 producer-orphan) is broken at its suppression step at the pinned tree (heuristic 1, informative names: the trait contract says the opposite of what jcodemunch does).
- The JCodemunchOps::find_references contract says production impls 'MUST scope the lookup to symbol.file'.
- RealJCodemunchOps::find_references applies filter_refs_to_file, which keeps only references whose file equals the declaring file.
- jcodemunch's find_references is import-based and never lists the declaring file.
- So every cross-file consumer is discarded, and p1_producer_orphan::check's `has_non_test_caller` guard can never see one. A public symbol consumed only from another file is reported as a producer-orphan: Medium past the 14-day grace window, which the /audit skill auto-files as a 'Wire <symbol> consumer' task.
- P5 H2 (check_live_path_stranded) uses the same seam.
- Tests stance: MockJCodemunchOps keys references on (file, name), so it encodes the same wrong contract, and tests/p1.rs could not fail on this defect.
- The fix exists on branch task/6504, which is 5 commits ahead of main (dated 2026-10-15) and not merged. #6504 is pending at priority low although it breaks a briefing key scenario.

### R9 · fk-e653f30b14df · medium · confirmed · filed:9213
**analysis** `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs::geometric_element_stiffness_shell` — inv-sf-5 h1 h6 kind:stub

Placeholders-owned-and-loud (INV-SF-5) and heuristic 1 (informative names and docs): three public geometric-stiffness kernels are always-panicking placeholder bodies with no live owner, and their docs describe a diagnostic path that does not exist. The three functions are geometric_element_stiffness_shell, geometric_element_stiffness_hex_p1 and geometric_element_stiffness_wedge_p1. Each body is an unconditional panic!("… not implemented …"), and all three are re-exported at the crate root (lib.rs:709-711). They have zero non-test callers. The module doc and every panic message say the buckling trampoline filters non-tet cells first and emits E_BucklingShellNotImplemented / E_BucklingHexWedgeNotImplemented. No such DiagnosticCode exists anywhere in crates/*/src: the only buckling code is BucklingOptionUnsupported. The buckling trampoline has no non-tet filter because it only ever meshes P1/P2 tets. The owner the docs cite (PRD buckling-eigensolver §13 task ζ, now task #3455 'Shell + hex/wedge stub diagnostics surfaced through trampoline') is status done, but no commit implementing it exists. Its other cite, 'task 3392', is in prose form, so the PTODO grammar would treat it as malformed. The stubs also escape the PTODO gate entirely, because they use panic! rather than todo!()/unimplemented!(). A next agent adding shell or hex buckling would look for diagnostics and a filter that are not there.

*Proposal:* Delete the three stubs, their re-exports and the 'diagnostic path' prose (heuristic 6: no caller needs them). If shell/hex buckling is planned, file a live owner and make the entry points return a typed error instead. Flag #3455 as phantom-done.

### R8 · fk-756b94f946c0 · medium · confirmed · open
**analysis** `crates/reify-shell-extract/src/partition.rs::partition_body` — h6 kind:critical-path kind:convention

Critical path: the briefing's shell-extraction scenario ends in 'shell mesh with a shell/tet interface partition', but that last step has no production caller. Heuristic 6 (well-defined purpose) and the orphan-producer convention both apply. partition_body is `pub fn` at partition.rs:182 with no `// G-allow:` above it. Outside partition.rs, git grep finds it only in two comments in reify-eval engine_build.rs. The production pipeline, reify-eval shell_extract_compute_fn, runs medial mask → mid-surface → pruning → segmentation → mesher and never partitions. The only ShellTetInterface consumer, engine_build.rs build_mixed_region_mesh, is #[allow(dead_code)] pending #6371. #6371's description names only build_mixed_region_mesh and its stride-drop fix, not partition_body. The T12 task (#3023 'mixed-region body partitioning') is done. The scenario therefore claims a stage that today runs only in unit tests.

*Proposal:* Either widen #6371 (or file a sibling) so the mixed solve consumes partition_body's interfaces, or add a G-allow that cites a live owner. Correct the briefing scenario until then.

### R7 · fk-6d119487b4f1 · medium · confirmed · open
**analysis** `crates/reify-eval/src/modal_ops.rs::resolve_location_value` — inv-sf-3 h1 tests kind:critical-path

The Selector branch of the modal location resolver cannot fire in production, and the drop is silent (INV-SF-3, declared-intent-consumed-or-diagnosed). The branch serves displacement_at and StepForce.at. It reads ModalResult.topology through carried_topology_from_result. Every production writer of that field gets it from build_modal_topology_value (modal_ops.rs:1289), which returns Value::Undef unconditionally, or writes a literal Undef (degenerate_modal_result). So in production a `Value::Selector` location always takes the 'honest fallback' and resolves to the global fundamental antinode. That path deliberately emits no diagnostic and no warning ('stays quiet'), so an author who asks for the response at a face gets the response at a different node with nothing reported. Heuristic 1 (informative names): the function's doc says it reads 'the CarriedTopology the BUILD path baked into the echoed modal_result', but no build path bakes one. The 'capability flip' test, r3b_modal_selector_displacement.rs, never runs the producer. It hand-builds a ModalResult with a populated topology through from_realized_mesh and calls displacement_at_trampoline directly, so it masks the gap (Tests stance). from_realized_mesh has no production caller. build_modal_topology_value's own deferral cites done #4654 ('Real population deferred to R3b follow-up per task 4654 design decision'). #7097 owns populating .topology on the body overload only. Its ratified scope says the dims overload's honest form 'stays the C2′ degraded/Undef one', so a Selector on the dims modal path will keep falling back silently even after #7097 lands. S13's PRD lists resolve_location_value as a 'consumer on main today' of the carried-topology resolver, but only tests reach that consumer.

*Proposal:* When the location is a Selector and the result carries no topology, emit a coded Warning naming the selector and saying it resolved to the fundamental antinode, instead of falling back silently. Have #7097's body-path population feed this resolver. Add one e2e that drives displacement_at with a Selector from a .ri modal solve, not a hand-built ModalResult. Correct the 'BUILD path baked' doc.

### R5 · fk-a804f8218ad6 · medium · confirmed · open
**analysis** `crates/reify-eval/src/compute_targets/as_printed_material_r0.rs::as_printed_material_r0_trampoline` — tests h1 kind:critical-path

The briefing scenario 'as_printed_material(body, proc) returns a Field immediately from R-fast, and refines to R0 when PrusaSlicer is on PATH' has no production path. Only fdm_as_printed.ri declares the R-fast target (`@optimized("fdm::as_printed_material_r_fast")`); no .ri routes to `fdm::as_printed_material_r0`. The R0 trampoline takes a raw G-code String at value_inputs[0], and no production code passes fdm::slice output into it. reify_fdm::select_rungs has zero production callers. as_printed_material::rung_compute_key, the documented 'eval-side bridge from the abstract rung to the concrete registered producer', is called only by tests. The 'integration gate ι' that #3791 (done) shipped, fdm_progressive_refinement_e2e.rs, calls as_printed_material_r_fast_trampoline and as_printed_material_r0_trampoline directly. Its own helper doc says the dispatch 'mirrors what Engine would do', so the test masks the missing wiring (Tests stance). target_fidelity is the knob that would select R0, and it is inert (fdm_slice.rs FdmSliceCacheKey doc; PDROP A10).

*Proposal:* Either wire the progressive ladder (fdm::slice → R0 via the as_printed_material node, gated by target_fidelity under #7087's ruling) and replace the direct-trampoline gate with a .ri-driven e2e, or record R0 refinement as a known_gap in the briefing and mark #3791's signal as test-harness-only.

### R6 · fk-e088a6e9caaa · medium · confirmed · filed:9219
**analysis** `crates/reify-eval/src/compute_targets/multi_case.rs::solve_multi_case_trampoline` — kind:defect h10 inv-sf-6

solve_load_cases (solver::multi_case) discards every per-case structured detail that its elastic_static sub-solves emit. That detail is the FEA overlay payload (StructuredComputeDetail::Fea): the UnderConstrained rigid-body null-space for an empty supports list, and ProblemElements for a degenerate mesh. The Completed arm destructures the sub-outcome with `..`, and the Failed arm rebuilds the outcome with `structured_detail: vec![]`. The text diagnostics are forwarded, so a multi-case solve says 'under-constrained' but never carries the element or DOF payload that the engine's target-agnostic detail channel (engine_eval accumulates structured_detail for every compute dispatch) hands to the GUI. The same scene solved through solve_elastic_static gets the overlay. buckling_multi_case.rs copies the same driver with the same two drops. It is latent there, because solve_buckling_trampoline emits no structured detail today.

*Proposal:* Accumulate each case's structured_detail alongside accumulated_diagnostics and return it in both arms. The two multi-case drivers would share one case-iteration helper that does this once (see the h11 finding on buckling_multi_case).

### R4 · fk-b683dbb8c87b · medium · weakened · open
**analysis** `crates/reify-eval/src/compute_targets/as_printed_material.rs::as_printed_material_r_fast_trampoline` — inv-sf-3 inv-sf-1 h10 kind:deep-read

The FDM compute targets turn malformed or missing declared inputs into silent results, against INV-SF-3 (declared intent consumed or diagnosed), INV-SF-1 (undef has provenance) and the briefing's 'errors must be noisy' convention. (1) as_printed_material_r_fast_trampoline and as_printed_material_r0_trampoline map any failure in their `?`-chained readers to `degraded_field()` with `diagnostics: Vec::new()`. Those failures are: a missing FDMProcess field (walls, top_bottom_layers, layer_height, build_direction, infill_density), a missing AsPrintedOptions.line_width, a missing body realization, or a G-code parse error. degraded_field() is a Field whose lambda is Undef. Pre-hydration absence and an author's malformed process are indistinguishable, and nothing records why. (2) fdm_slice::read_slice_settings fills any missing or ill-typed FDMProcess field with a hard-coded default via `unwrap_or` (0.2 mm layer height, walls 3, top_bottom_layers 4, infill 0.2, Gyroid), so a mistyped process slices at a profile the author never wrote. Those defaults restate the FDMProcess .ri defaults by hand (h11). (3) A degenerate build_direction silently becomes +Z (`unit3(..).unwrap_or([0,0,1])`) in both rungs. The S15 α decoder (#8674) covers only the ElasticMaterial read (read_base_elastic). It does not cover the FDMProcess/AsPrintedOptions/G-code degrade policy or read_slice_settings.

*Proposal:* Route the FDM readers through S15's InputDecodeError → coded Failed (or coded Warning plus Undef with a recorded cause), keeping 'body not yet realized' as the only silent transient. Have read_slice_settings decode FDMProcess once, through the same decoder, rather than restating its defaults.

### R11 · fk-e30c10d74c04 · medium · confirmed · filed:9185
**assemblies** `crates/reify-compiler/src/forall_elaborate.rs::elaborate_forall_connect` — h10 inv-SF-3 kind:critical-path

Heuristic 10, invariants enforced redundantly and uniformly. A `forall v in coll: connect ...` over a collection whose count is deferred (unknown at compile time) is never checked for port direction or undefined ports. The same source with a known count is checked. On the Deferred arm, elaborate_forall_connect only runs PortPath::parse on the two endpoints and pushes a CompiledForallTemplate. It never reaches compile_connection's endpoint_direction or names_undefined_port checks. At runtime, engine_edit.rs's CompiledForallBody::Connect arm emits every element's compatibility constraint as `CompiledExpr::literal(Value::Bool(true), Type::Bool)`, so the constraint reports Satisfied. Every element shares the collection's child template, so the check could run once at capture time. Measured with the pinned tree's target/release/reify. The fixture has a deferred `sub vents : List<Vent>` with `param n : Int` (no default) and `constraint vents.count == n`, plus `forall v in vents: connect air_channel -> v.inlet` (in -> out) and `forall v in vents: connect v.nosuch -> air_channel`. `reify check` prints 'All constraints satisfied.' and exits 0. With `constraint vents.count == 2` (resolved) the same body gives 'incompatible port directions for connect: In -> Out' and 'undefined port vents[i].nosuch' errors and exits 1. The module doc lists what the deferred path drops (connector type, params, where-clauses, chains), but this check is not on the list, and no diagnostic says so.

*Proposal:* At capture time, resolve each PortPath against the collection sub's child template (the element root shares it) and run the same direction and existence predicates compile_connection uses. Or emit an Info/Warning stating the check is skipped. Either way, the runtime compat literal should reflect the compile-time verdict instead of a hard-coded true.

### R10 · fk-98a6b0d91a7f · medium · weakened · open
**assemblies** `crates/reify-compiler/src/connect.rs::compile_connection` — h12 h11 kind:cross-module

Heuristic 12, structured data instead of meaningful strings. Since c225a44d88 a connect endpoint parses to a typed PortPath {root: Element | Literal{sub, index} | Named, members}. compile_connection then calls resolve_port_name, which flattens it straight back to a String (`PortPath::parse(expr, None)?.port_name()`). Every downstream decision re-parses that string with ad-hoc parsers. own_port_name uses strip_prefix("self.") and contains('.')/contains('['). sub_member_endpoint uses rsplit_once('.') then split_once('['). These two decide own-port vs sub-member, direction lookup, undefined-port and auto-match. The file itself concedes the string is ambiguous: chain_element_sub says it dispatches on elem.kind because the serialized name 'cannot tell a keyed element from a dotted port reference (vents["a.b"])'. Known costs: the phantom un-indexed `vents.outlet` connection (#7659, pending) comes from sub_member_endpoint stripping the indexer and accepting a collection name, a fact the typed root already distinguishes. `self` is parsed as PortRoot::Named("self") and is only folded into the own-port case by the string prefix strip. The planned next change in this area, nested port paths `a.b.port` (assembly-modal-connection-graph α), would render as "a.b.port". sub_member_endpoint would then look up sub "a.b", miss both side maps, and fall into the documented silent-pass absence contract. The directions would go unchecked and no undefined-port error would fire.

*Proposal:* Keep the PortPath through compile_connection. Classify own vs sub-member endpoints from PortRoot (Named("self") folded at parse), look up directions and member names by (root sub, members), and render the String only where CompiledConnection/labels need it. Then #7659's collection-name endpoint becomes a typed check (Named root naming a collection sub with members non-empty).

### R12 · fk-68442f3e3530 · medium · confirmed · filed:9186
**assemblies** `crates/reify-compiler/src/port_path.rs::parse_root` — h10 kind:defect

Heuristic 10, informative enforcement, and the convention that errors must be noisy. parse_root accepts any NumberLiteral index as `ElementIndex::Position(*value as i64)`. A fractional index is silently truncated, and nothing checks the index against the collection's count, so a connect to a non-existent element compiles as a satisfied phantom connection. Measured on a fixture with `sub vents : List<Vent>` and `constraint vents.count == 2`. `connect vents[1.7].inlet -> air_channel` printed 'OK connect_compat_vents[1].inlet_air_channel' (1.7 became 1). `connect vents[9].inlet -> air_channel` printed 'OK connect_compat_vents[9].inlet_air_channel' over an element that does not exist. `reify check` exited 0 with 'All constraints satisfied.' The `is_real` flag the AST carries is ignored. This is the same phantom-connection class as #7659 (un-indexed collection name), reached through the literal index. The truncation predates PortPath, carried over from the old resolve_port_name.

*Proposal:* Reject a non-integer literal (`is_real` or a fractional value) at parse with a coded diagnostic. When the collection count is known at compile time, check the Position against it, in the same place #7659's un-indexed check lands.

### R16 · fk-17a5194418da · medium · confirmed · open
**constraints-and-geometry** `crates/reify-kernel-openvdb/src/register.rs::openvdb_capability_descriptor` — h1 inv-sf-5 h10 kind:critical-path

Heuristic 1 (informative names) and INV-SF-5 (placeholders-owned-and-loud): the OpenVDB capability descriptor, which its own doc calls a feasibility table, declares (BooleanUnion, Voxel), (BooleanDifference, Voxel) and (BooleanIntersection, Voxel). No code path performs a voxel Boolean. OpenVdbKernel::execute handles only Surface and ApplyTransform and returns Err(OperationFailed(VOXEL_BOOL_STUB_MSG)) for every other op (kernel_real.rs:590-597). The dispatcher's final-stage probe needs a literal (op, demanded) match with the current repr equal to the demanded one (dispatcher.rs:725-731), and OpenVDB is the only kernel that declares those three pairs. Voxel demand is reachable from .ri: compute_demanded_reprs gives a realization Voxel demand whenever a direct consumer is Voxel-only-input, i.e. isosurface (engine_build.rs op_is_voxel_only_input, plus the demand assignment around :2849). So `let s = union(a, b); let shell = isosurface(s)` should plan union on OpenVDB over Voxel and then fail at execution, with a message about 'Voxel handles on both operands'. The only owner the code names is 'task ε' (register.rs:146-150, kernel_real.rs:495-500/595), a malformed cite. The briefing's known_gaps row for this stub has tracking: null. search_tasks finds no live owner. S10 ι₂ #8989 checks the opposite direction only (every emitted Operation is declared by some kernel), not over-declaration; ι₆ #8998's parity covers queries, exports and projections, not ops.

*Proposal:* Pick one: delete the three Boolean rows, so isosurface(union) plans the union on OCCT/Manifold and then voxelizes. Or implement voxel CSG (openvdb::tools::csgUnion etc.) under a real task id. Either way, record the decision as the known_gaps tracking pointer.

### R14 · fk-c23fa8fef28c · medium · confirmed · open
**constraints-and-geometry** `crates/reify-kernel-manifold/src/kernel.rs::ManifoldKernel.query` — h1 h11 h10 kind:cross-module

Heuristic 1 / heuristic 11: GeometryQuery::AdjacentFaces and ::SharedEdges carry a face index that means two different things across the trait seam. The trait contract (reify-ir geometry.rs:1856-1860) says `face_index` is the 0-based index into the shape's face enumeration. The only production consumers, selector_vocabulary_v2::adjacent_to_face (:796-845) and geometry_ops' shared-edges helper (:9750-9787), compute it as the position of a handle in extract_faces(parent), then map reply indices back through that same list. Since #4262, ManifoldKernel::extract_faces returns coalesced planar faces (6 for a cube), while Manifold's AdjacentFaces/SharedEdges arms interpret the index as a raw mesh triangle (0..12) and return triangle indices (kernel.rs ~588-640; queries.rs module doc :29-35 states the two spaces are 'disjoint'). On a Manifold-owned parent, adjacent_to_face therefore asks about the wrong triangle and maps triangle ids back as planar-face positions. The result is silently wrong neighbours, or 'index out of range for extract_faces' once a triangle index is >= 6. The capability table marks both queries BRepAndMesh (geometry.rs:2438-2439), so eval never gates them, and the Manifold test (tests/queries.rs:840-880) pins the raw-triangle semantics. Today the selector pass hands try_eval_topology_selector a single kernel (engine_build.rs:11156), so the defect is latent. S10 ι₅ #8995 ('handle-served requests go to the handle's owner') will route these queries to Manifold, and its boundary tests do not cover adjacency. No PRD under docs/prds/v0_6 mentions the index-space split.

*Proposal:* Make Manifold's AdjacentFaces/SharedEdges operate on the coalesced planar-face enumeration that extract_faces returns (adjacency = faces sharing a canonical edge). Or declare them BRepOnly in the capability table so eval emits QueryNotSupportedOnRepr. Add a BRepAndMesh parity test that runs adjacent_to_face over both kernels on a cube (expects 4 neighbours).

### R15 · fk-6a9547d3d171 · medium · confirmed · filed:9196
**constraints-and-geometry** `crates/reify-kernel-openvdb/src/ingest.rs::KNOWN_UNITS` — h12 h10 kind:defect

Heuristic 12 (structured data instead of meaningful strings): imported .vdb unit metadata is parsed into a dimension only, and its scale is discarded. KNOWN_UNITS maps 'mm', 'cm', 'km', 'kPa', 'MPa' and 'GPa' to their DimensionVector with no scale factor. lookup_unit_dimension returns just the dimension, and validate_grid_units only compares dimensions. lower_to_sampled then copies grid.data, bounds_min/max and spacing into the SampledField unchanged. The in-language `sampled {}` source builds the same SampledField from SI values (engine_eval.rs extract_point_axes pushes `si_value`). So a grid declaring units 'MPa' with value 250 is accepted for a Pressure codomain and reads as 250 Pa (1e6 low), and a grid declaring 'mm' reads as metres (1e3 high), silently. Voxel-transform bounds and spacing are never converted either. elaborate_field's Imported arm (engine_eval.rs:4325-4360) forwards the outcome as-is, so the key scenario 'Imported fields ... surfaces FieldImportFailed loudly on error' is wired, but a unit-prefixed file yields wrong numbers with no diagnostic. The ingest tests pin only that 'MPa' lowers successfully (ingest_tests.rs:141-148, 752-775), never the lowered values.

*Proposal:* Carry (dimension, si_scale) per KNOWN_UNITS row and multiply data by the scale in lower_to_sampled. Decide (and document) the length unit of the voxel transform, then scale bounds and spacing to metres. Add a value-level test: a 'MPa' grid holding 1.0 samples to 1e6 Pa.

### R13 · fk-e5147ccf3fd9 · medium · confirmed · filed:9226
**constraints-and-geometry** `crates/reify-eval/src/sweep_classifier.rs::classify_swept_body` — kind:defect h1 h11

classify_swept_body maps GeometryOp::ExtrudeSymmetric onto the same SweptKind::Extrude { axis: +Z, length: distance } as a plain Extrude and drops the op's -distance/2 translation. reify-ir documents ExtrudeSymmetric as make_prism(profile, 0, 0, d) followed by translate_shape(0, 0, -d/2), so the realized body spans z in [-d/2, d/2]. The only consumer that reads SweptKind position is the structured hex/wedge mesher. It goes through swept_kind_to_sweep_params and reify-solver-elastic sweep::build_vertices, which lays layer k at z = k/layers * length, i.e. z in [0, d]. A swept mesh of a symmetric extrude would therefore be offset by d/2 from its body, so face-location boundary conditions would miss it. Today this is latent: the one production call of dispatch_volume_mesh (engine_build.rs:9470) passes swept_kind None and force_tet true, and the datum projection reads only Extrude's axis (feature_datum.rs:607). Pending #4746 is the leaf that activates the hex/wedge production edge, so that change will produce a misplaced mesh unless SweptKind carries the offset (heuristic 1: the variant's name and doc claim the two ops are 'identical for classification purposes', which is untrue once position is consumed).

*Proposal:* Give SweptKind::Extrude an explicit start offset along the axis (0 for Extrude, -d/2 for ExtrudeSymmetric) or a separate ExtrudeSymmetric variant, and make SweepParams::Extrude carry it into build_vertices before #4746 lands. Pin it with a test that compares the swept mesh's z-range with the realized body's bounding box.

### R18 · fk-3cf4b09d4b1a · medium · confirmed · filed:9180
**factory-infra** `scripts/refresh-warm-base.sh` — h10 h1

Heuristic 10 (invariants enforced) and heuristic 1 (informative names). The refresh guard is the only reify-side enforcement of §9.5 inv.9: only the _merge-verify lane's clean landed-commit target/ may advance the shared base. It never checks that the commit has landed, and it never checks which lane is advancing. The guard checks four things: the advancing dir is inside a worktree (259), `git status --porcelain --untracked-files=no` is empty (265), --landed-commit is non-empty (287), and `git rev-parse HEAD` == --landed-commit (294). No `merge-base --is-ancestor <sha> refs/heads/main` and no lane-identity test exists anywhere in the script. Every caller derives the flag from the advancing worktree's own HEAD: dark-factory GitOps.refresh_warm_base ('derived from git rev-parse HEAD in the _merge-verify worktree'), ensure-warm-base.sh:209 (_r2_head), and seed-warm-base-initial.sh:253-255. The refusal hint at 289 tells an operator to pass `--landed-commit $(git -C <lane_worktree> rev-parse HEAD)`. The 'landed' assertion therefore reduces to HEAD==HEAD. A clean task lane holding committed but unlanded work passes, and the guard promotes it. Dark-factory's docstring calls this guard 'defense-in-depth', and the PRD names it as inv.9's enforcement. The commit-cleanliness half is real; the 'landed' half that the flag's name promises is not checked.

*Proposal:* Make the guard check what the flag's name says: refuse unless `git merge-base --is-ancestor $LANDED_COMMIT refs/heads/main`, and fix the hint to name a main commit. Coordinate with #8590, which amends inv.9 to admit _spec- lanes, so the check keys on landed-ness rather than lane name.

### R19 · fk-3d9c0153c359 · medium · confirmed · open
**factory-infra** `scripts/seed-warm-lane.sh::_sidecar_path` — h11 h10

Heuristic 11 (SPOT). The warm base's build configuration (RUSTFLAGS and the invocation fingerprint) is recorded in two homes, and the two fail-closed guards each read a different one. refresh-warm-base.sh, the production promoter, writes `<base>/target.rustflags` and `<base>/target.invocation` (575-576), and warm-lane-preflight.sh Checks 4/5 read those (135, 154). seed-warm-lane.sh's B5/D4 RUSTFLAGS guard and S1 invocation guard (871-890) read RUSTFLAGS= and INVOCATION= from `$(dirname base)/.warm-base-meta` (_sidecar_path, 490-494). Only `seed-warm-lane.sh --record-base` writes that file, and it has no production caller: the git grep hits are tests only (test_warm_lane_pool.sh, test_target_per_lane_independence.sh). The live pool confirms it. /home/leo/src/warm-lanes/base holds target.rustflags and target.invocation (both empty, written 2026-10-13 22:22) and no .warm-base-meta. _sidecar_read returns '' for an absent file, so the seed guard in production asserts 'env RUSTFLAGS is empty', not 'env matches the base'. A base promoted with --rustflags X would pass preflight under X and be refused by seed under X, and the reverse also holds. The stamps differ in a second way. refresh writes them to the symlink path after the atomic swap, last-writer-wins. Its own .basecommit and .buildroot, by contrast, are written per generation, before the swap, so that they stay 'TOCTOU-free under concurrent refreshes (inv.8)'.

*Proposal:* Keep one per-generation build-config stamp, written by refresh before the swap beside .basecommit. Both seed and preflight should read it, and the seed guard should fail closed when the stamp is absent rather than default to ''. #5614's --build-fingerprint generalisation is the natural vehicle.

### R20 · fk-912fa2bf5921 · medium · confirmed · filed:9181
**factory-infra** `scripts/warm-lane-audit.sh` — h10 comments

Heuristic 10, 'informatively', and the Comments stance. The briefing's key scenario names `stash_entries` as 'the backstop that makes a disarmed guard visible'. When it cannot measure, it reports the all-clear value. The figure starts at STASH_ENTRIES=0 (1543). With no resolvable repo, or a non-worktree repo, it stays 0 and a stderr warn is printed (1548-1551). When `git -C <repo> stash list` itself fails, the output is `2>/dev/null || _stash_out=""` (1556): the figure stays 0 with no warning at all. The comment directly above the code says 'A failure degrades to 0 with a note', and no note is emitted. The audit already has explicit could-not-measure counters for its other axes (state_unknown, leak_unknown, plan_unknown in the same HEADROOM line), but this axis has none. In both table and JSON output, an unmeasured stash stack is therefore byte-identical to a clean one, on the field whose job is to surface what the hook guard missed.

*Proposal:* Add a distinct could-not-measure outcome, for example stash_entries=-1 or a stash_unknown=1 flag, set on all three degrade paths, and emit the promised warn on the git-failure path. Append it per the HEADROOM append-only rule (S20 D8 field list).

### R21 · fk-91bac0103c72 · medium · confirmed · filed:9182
**factory-infra** `scripts/warm-lane-source-integrity.sh` — h10 comments

Umbrella principle nothing-vacuous-and-unowned, and heuristic 10. The vanished-tracked-file detector shipped by task 7227 on 2026-09-10 has no production invoker and no live owner for one. Its header says 'Reify ships this primitive; dark-factory owns the agent-session-start invocation'. At this tree no reify hook, script or timer calls it: the git grep hits are its own test, CLAUDE.md, the seed test and the direct-sets TSV. git grep over dark-factory orchestrator/src and shared finds 0 references. Task 7227's deliverable 4 assigned the invocation to dark-factory, but no dark-factory or reify task to wire it was found (search_tasks on both stores, threshold 0.45-0.5). CLAUDE.md's Warm lanes section cites it as the 'vanished-file detector' for the esc-7106-5 class. The incident that motivated it is still unattributed and waiting for a second sighting, and an unrun detector cannot produce one.

*Proposal:* Pick one of the three exits. File the dark-factory wiring task, run at acquire tail or session start, fail-open, exit 3 to an info escalation. Or have seed-warm-lane.sh --fresh-checkout's tail run it as it does hooks-armed-guard. Or delete the detector and the CLAUDE.md pointer.

### R22 · fk-83fa1aeda5f6 · medium · confirmed · open
**factory-infra** `tests/infra/run_all.sh::_ra_timing_pool_lines` — h3 h11

Heuristic 3 (one mechanism per axis) and heuristic 11. Task 7424's per-member timing was placed at call sites instead of inside the member runner that #8681 just unified, so a cross-cutting concern is again per-branch. `_ra_mark` .tq/.ts/.te pairs wrap `_ra_run_member` at the three pool call sites only (2135-2139, 2176-2178, 2207-2209). `_ra_run_serial_member` (810-820), which serves the host-infra, member-subset and legacy-serial paths, records none. `_ra_timing_write` therefore emits member rows for the pool path only, and a summary-only line for the others. That includes member-subset, dark-factory's merge-gate retry. docs/notes/verify-instrumentation-ledger.md lists this under 'Known blind spots' with no owning task. Second, S19 γ #8685 (pending, high) adds REIFY_VERIFY_EVENT_LOG with member_start and member_finish records written from _ra_run_member and _ra_record_result. Its task text never mentions REIFY_RUN_ALL_TIMING_OUT. Two structured per-member start/end/rc channels will then exist side by side, one hand-built with printf (_ra_timing_member) and one through lib_event_log.sh's escaper, with different coverage.

*Proposal:* Fold the timing record into γ's event channel. member_start and member_finish already carry ts_ns, and queued_at becomes a field. Derive verify-ledger's run_all block from the event log, or move the marks inside _ra_run_member (queue mark passed in) so every branch records them. Either way there is one per-member record and one emitter.

### R17 · fk-8f8d0ef27b07 · medium · confirmed · filed:9194
**factory-infra** `hooks/project-checks` — h10 h11 kind:critical-path

Heuristic 10, clear invariants uniformly enforced: the invariant 'a merge into main runs the merge gate (--profile both, role=merge)' has two enforcement points that disagree, and the weaker one marks the move as sanctioned. When a hand `git merge <branch>` on main hits conflicts, the user resolves them and runs `git commit`. Git then runs pre-commit, not pre-merge-commit, which per githooks only fires for an automatically created merge commit. pre-commit calls hooks/project-checks, which runs `verify.sh all --profile debug --scope staged --include-infra` with DF_VERIFY_ROLE unset, so the role is `task`. verify.sh's MERGE_HEAD defence (the top-level block before `cd "$REPO_ROOT"`) forces only SCOPE=all. PROFILE stays `debug`: --profile was explicit (PROFILE_EXPLICIT=1), and the role-based `both` default fires only for merge/background. As a result the release-only tests that hooks/pre-merge-commit's header says run 'exactly once, at merge' (reify-solver-elastic's #[cfg_attr(debug_assertions, ignore)] buckling/eigensolve tests) never execute. pre-commit then calls main_gate_mark, and hooks/reference-transaction logs 'sanctioned main move'. scripts/land.sh aborts on conflict, and DF's merge worker verifies with role=merge, so neither sanctioned path produces this. But a conflicted hand merge is exactly the class of move the tripwire exists to classify, and it records it as sanctioned after a debug-only gate.

*Proposal:* In verify.sh's MERGE_HEAD block, also force PROFILE=both and the merge role (or refuse a debug profile while MERGE_HEAD is present). Alternatively, have hooks/pre-commit detect MERGE_HEAD and run the pre-merge-commit gate instead of project-checks. In S18 this is one row of the role/scope record (D3/D5).

### R24 · fk-bf57b4017285 · medium · confirmed · open
**gui** `gui/src-tauri/src/main.rs::claude_send_message` — h7 h5 kind:critical-path

Key scenario 'The Claude chat panel drives the sidecar … under landlock' breaks on heuristic 7 (temporal coupling): which directory the sidecar may write is decided by whichever design was open when the FIRST chat message was sent. Path: claude_send_message recomputes the landlock workspace on every message with claude_bridge::resolve_workspace_dir. Its inputs are context.current_file, then AppState.initial_file, then cwd. The result is captured only in the spawn closure passed to ensure_sidecar_ready. ensure_sidecar_ready's Phase-1 fast path returns Ok(()) as soon as the slot holds a Ready handle, without comparing workspaces, so the closure never runs again. REIFY_WORKSPACE (sidecar index.ts: 'parent dir of the active editor file at sidecar-spawn time') therefore stays fixed for the sidecar's lifetime. After the user opens a design in another directory, through open_file_engine (which never updates AppState.initial_file; only the argv load sets it), the claude CLI keeps --writable on the OLD directory, and its edits to the newly open design are refused by landlock. The per-message resolve_workspace_dir result is a transient value whose effect is pinned to the first call (heuristic 5). No test covers a workspace change after spawn: claude_bridge_tests' workspace_resolution tests the pure resolver only.

*Proposal:* Key the sidecar on its workspace: respawn the sidecar, or send it a workspace-change message, when the resolved workspace differs from the one it was spawned with. Or state in the briefing and the sidecar contract that the workspace is pinned per session, and give the UI a signal for it.

### R23 · fk-d645f8a1d95a · medium · confirmed · open
**gui** `gui/src-tauri/src/claude_bridge.rs::InboundMessage` — h1 h6 h11 kind:dead-code

The host-side half of the deleted in-process tool interception (gui-state-sync L8) is still in the code, and its names lie (heuristic 1). InboundMessage::ToolResult ('Tool result sent back to the sidecar after in-process MCP tool execution') has no production producer: `git grep InboundMessage::ToolResult` finds it constructed only in tests/claude_bridge_tests.rs:1767. Yet the sidecar keeps a whole consumer path for it: ipc.ts validates 'tool_result' inbound; session.ts handleToolResult, the pendingToolUseIds FIFO fallback, and the stdin kept open 'so tool_result blocks can follow'. The Rust side also keeps OutboundMessage::ToolCall.tool_use_id 'enabling id-based correlation' and warn_if_stale_tool_call, which warns about a correlation fallback that no host reply exercises. Names still describe the removed feature: `McpConfig` holds only an event emitter; `SidecarHandle::from_parts_with_mcp` means 'with emitter'; the SharedStdin alias doc says 'so the reader task can write back tool results', which the reader never does. Two languages carry a protocol arm with no producer, and a reader of either side will take it for a live path (heuristic 6, no single purpose).

*Proposal:* Delete the inbound tool_result arm on both sides of the stdio protocol (Rust enum variant, ipc.ts validation, session.ts handleToolResult and its FIFO bookkeeping), and rename McpConfig/from_parts_with_mcp after what they carry (an event emitter). If ai-native-editing ever reintroduces host tool results, it re-adds them with a producer.

### R26 · fk-c4858bb86f2f · medium · confirmed · filed:9202
**gui** `gui/src/editor/Editor.tsx::Editor` — h11 h10 kind:defect

Heuristic 11 (SPOT) and heuristic 10 (uniform enforcement): the editor has no single rule for document identity across the two spellings of one file. Editor's own versionKey comment (Editor.tsx:95-105) records that store paths are decoded while every URI the backend mints is percent-encoded by the url crate, and only the version counters canonicalize (versionKey = pathToUri(canonicalizeKey(...))). pathToUri does not encode, so currentUri for '/p/hello world.ri' is 'file:///p/hello world.ri'. Two other identity checks compare raw strings. (1) The LSP diagnostics listener drops every event where `event.uri !== currentUri` (Editor.tsx:685). TauriNotificationSink.publish_diagnostics emits `uri.as_str()` of the server's parsed Url (main.rs:99-109), so a file whose path has a space or non-ASCII character never shows LSP diagnostics. (2) applyWorkspaceEditAcrossFiles routes an edit to the live view only if `uri === activeUri` (rename.ts:204), where activeUri is the decoded currentUri. A fresh F2 rename in such a file therefore fails that test. isOpen(canonicalizeKey(uri)) returns true, so the edit goes to applyOpenInactive on the ACTIVE file (Editor.tsx:395-419). That sink applies the edits to the store's file.content, which the Mod-s branch's own comment says is stale once the user types, writes it to disk with saveFile, and calls updateFileContent. The store-sync effect then replaces the whole view buffer with a sync.external transaction. Any unsaved typing is discarded, and the rename lands on the pre-typing text. Editor.test.tsx covers the spaced spelling only for the stale-refusal path (2433-2489) and the inactive-buffer path (2491-2542). It has no fresh-rename-on-active or encoded-diagnostics case.

*Proposal:* Route every document-identity comparison in the editor (diagnostics listener, applyWorkspaceEditAcrossFiles' active check, fileStates keys, the uriGetter compares) through the one key function versionKey already uses. Add the fresh-rename-on-active and encoded-diagnostics cases with the spaced fixture.

### R25 · fk-93b720040a52 · medium · confirmed · filed:9203
**gui** `gui/src/App.tsx::navigateToDiagnostic` — h11 h10 kind:critical-path

Heuristic 11 (SPOT): the cross-file navigation policy (activate an open tab, or read the file from disk and open it, then scroll) exists only inside navigateToDiagnostic (App.tsx:135-178), which #4403 re-landed after #3358 regressed. Every other producer of a source location calls setScrollToLocation directly: the FindUsesPanel onNavigate (App.tsx:2348-2363), the viewport-click navigateToSource scrollEditor dep (App.tsx:2055-2058), and the navigate-to-source listener (App.tsx:1647-1650). The Editor's scroll effect silently returns for any location outside the active file (Editor.tsx:883). Since #4210 (kappa) the server returns cross-file references: lsp_bridge.rs's own test asserts 3 Locations spanning parts.ri and main.ri. So in the Find-uses panel, clicking a reference in an imported file does nothing, which is the same class of defect as #3358 on a sibling path. references.ts's header still says results are 'scoped to the open document's enclosing entity body', a comment the κ change made false.

*Proposal:* Make the open/activate/scroll sequence the one navigation entry point (rename navigateToDiagnostic to a location-generic navigateToLocation) and route FindUsesPanel, navigateToSource and navigate-to-source through it.

### R27 · fk-b252ff943d44 · medium · confirmed · open
**gui** `gui/src/viewport/ProbeSystem.tsx::createProbeSystem` — inv-nothing-vacuous-and-unowned h9 kind:dead-code

Design-invariants umbrella principle `nothing-vacuous-and-unowned`, and the briefing convention that every producer has a production consumer: the FEA probe chain has no production consumer. createProbeSystem and ProbePopup (ProbeSystem.tsx, 317 lines), createProbeStore (probeStore.ts, 145 lines, re-exported from stores/index.ts), and meshManager's computeBarycentric/sampleProbe (meshManager.ts, ~150 lines, on the public MeshManagerContext) are imported only by tests. Viewport, DualViewport, MultiViewport, scene.ts and App.tsx contain no 'probe' reference. Task #2964 ('probe-point query system ... pinnable probes') and #3001's probe error readout are both done, yet a user cannot place a probe. This is the same shape as #4885, where feaModeStore was shipped but never instantiated. #6204 (deferred) plans further edits to the unwired ProbePopup without wiring it.

*Proposal:* Pick an exit: wire createProbeSystem into the Viewport FEA mode (as #4885 did for feaModeStore) under a live task, or delete the probe chain and meshManager's probe surface. Re-check #6204's premise either way.

### R30 · fk-5d64b909dfdf · medium · confirmed · open
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/gcode_import.rs::eval_gcode_import` — inv-sf-1 h10 kind:critical-path

Key scenario 'gcode_import(file, dialect) ... a Klipper INPUT_SHAPER directive is a warning, not a parse error' breaks at the eval boundary (INV-SF-1 undef-has-provenance; heuristic 10, informatively enforced). lower_gcode collects typed outcomes: a ParseError carrying its line, plus GcodeImportWarning::{ShaperConflict, ArcFlattened, DialectUnsupported}. eval_gcode_import, the only production caller, discards all of them. A parse error becomes a bare Value::Undef with no diagnostic or UndefCause (gcode_import.rs:404-406). An unrecognised dialect becomes an empty List, which looks the same as an empty program. Every warning is dropped (the doc at :377 says 'undeliverable here'). The trajectory family has no diagnose hook; snapshot, orientation, flexure, fea, geometry, stackup, dfm and tolerancing each register one (reify-stdlib lib.rs). So none of the PRD's three codes, E_GcodeParseError, W_GcodeDialectUnsupported and W_GcodeDialectShaperConflict, exists in reify-core or reaches a user: git grep finds no reference outside gcode_import.rs and reify-gcode. The module-wide #![allow(dead_code)] at :39 suppresses the never-read-warning lint that would otherwise flag this. S8's typed-outcome PRD keeps the trajectory family `legacy` (builtin-registry-migration-and-interception-classifier.md D4), and S6 χ covers only TOTS outcomes, so no stream owns this.

*Proposal:* Make lower_gcode's ParseError and warnings coded DiagnosticCodes and emit them from the gcode_import arm, either through a trajectory diagnose hook today or through S8's Evaluated{value, advisories} once the trajectory family converts. A parse error should become a coded rejection. An unsupported dialect should become a coded error, not an empty list.

### R32 · fk-6166c9f2a331 · medium · confirmed · open
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/simulate.rs::simulate_trajectory_core` — h1 kind:critical-path

Key scenario 'simulate_trajectory runs FK + inverse dynamics + modal superposition end to end': the FK step does not depend on the trajectory, so the 'nominal pose' the result reports is a misnomer (heuristic 1, informative names). simulate_trajectory_core builds the FK chain once from each link's fixed parent_to_child (simulate.rs:493). It calls nominal_fk_chain once and replicates that single pose across every time sample (:550-553). The joint positions q(t) from the sampled profile never enter forward kinematics, so EndEffectorTrack.nominal_pose is constant for any profile. Vibration is a scalar mapped onto Z (:528-532). Upstream, value_to_mechanism_model gives every link an identity transform (#8881), so the reported nominal pose is the identity for every mechanism. The only physically meaningful output is the scalar vibration magnitude. The module doc (simulate.rs:14-18) still lists 'FK-snapshot integration' as deferred to the π Value-wiring task, which is done, so no live task owns it.

*Proposal:* Either compute nominal FK per sample from q(t) through the joint transforms (snapshot / transform_at), or rename and document the output as a vibration-only track and drop the nominal and combined pose claims until FK is wired. Fold this into #8881's re-scope.

### R33 · fk-c2cca28ae7c5 · medium · confirmed · open
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/trampoline.rs::track_data_to_value` — inv-pd-2 h12

The runtime EndEffectorTrack does not match its .ri declaration field for field (INV-PD-2 result-fields-populated-or-owned; heuristic 12). trajectory.ri:359-366 declares modal_result: ModalResult, nominal_pose and combined_pose: List<List<Pose3>>, and vibration_offset: List<List<Vec3<Length>>>. track_data_to_value writes modal_result and mechanism as the sentinel Value::Real(0.0) (trampoline.rs:583-584). It writes the three pose and offset matrices as List<List<Real>>: the Z component only, dimensionless. The accessors follow suit. end_effector_track is declared -> List<Pose3> but returns List<Real>. deviation_from_nominal is declared -> List<Length> but returns List<Real>. peak_deviation is declared -> Length but returns Real (trajectory.ri:448/460/473; trampoline.rs:1167-1209). peak_deviation_at returns 0.0 ('perfect') for a malformed track or an out-of-range location (fold seed at :1207). That is a plausible well-formed fake, not an honest Undef, and the 'errors must be noisy' convention prohibits it. Any FaceSelector location collapses to index 0 (read_location_index). #8119 covers only the static typing of the *_at intrinsics. No task owns the runtime shape, and reify-audit has no PVAC pattern to catch it.

*Proposal:* Either populate the declared types: Pose3 lists, Vec3<Length> offsets, Length deviations, modal_result passed through. Or narrow the declarations to what the runtime produces, and make the malformed and out-of-range accessor cases return Undef with a cause.

### R31 · fk-73c31b060f80 · medium · confirmed · filed:9190
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/sampling.rs::to_trajectory_samples` — h10 kind:defect

A PiecewisePolynomialProfile whose first waypoint is not at t = 0 is silently mis-evaluated by simulate_trajectory and by input_shape's impulse arm. The precondition 'profiles start at t = 0' is unenforced (heuristic 10): it is stated only in a doc 'Assumptions' paragraph (sampling.rs:115-118, 'for non-zero-origin profiles the caller must adjust'), and no caller adjusts. The splines keep absolute knots: CubicSpline::eval subtracts knots[i], and segment() clamps t <= knots[0] onto segment 0 (spline.rs:174-196). But duration() is the span t_last − t_first (:364-367). to_trajectory_samples samples [0, duration] (sampling.rs:121-167), and input_shape_value resamples [0, duration + Δ] with convolve_at clamping τ into [0, t_domain] (trampoline.rs:796; impulse_shaper.rs:355). With waypoints at t = 2 s and 3 s, both paths evaluate the segment-0 cubic extrapolated backwards over [0, 1] s. They never sample the real motion. The shaped profile is also re-timed to start at 0. trajectory.ri imposes no first-waypoint-at-zero constraint, and nothing reports the problem.

*Proposal:* Either shift by the first knot inside MultiJointSpline (an eval over a [start, end] domain), or reject a non-zero-origin profile with a coded diagnostic at value_to_multijoint_spline. Pin the case with a test that uses t0 = 2 s.

### R29 · fk-e348f06cce2e · medium · weakened · open
**kinematics-and-motion** `crates/reify-stdlib/src/orientation.rs::eval_orientation` — h12 h11 h3 h4

The Euler-convention axis of variation has no typed home in Rust (heuristic 12, structured data; heuristic 11, SPOT; heuristic 3). The .ri declares enum EulerConvention with 12 variants (geometry_traits.ri:162). eval_orientation reads the variant back as a string in two independent 12-arm tables. orient_euler maps the string to an axis triple (orientation.rs:82-95). orient_to_euler re-implements all 12 conventions as hand-written matrix-element formulas, each with its own gimbal-lock branch (:327-460). Both end in `_ => Undef`. A third copy is the test list ALL_CONVENTIONS (:848). Nothing derives the decomposer from the constructor's axis triple, so the two can disagree silently. The history shows that drift: 11fb37b8eb fixed four proper-Euler decomposer arms at their singularities, and #6082 found the enum under-declaring eval by six variants. eval_orientation is one 627-line match over 13 builtins (heuristic 4), with the 180-line decomposer inline.

*Proposal:* Parse the variant once into a Rust EulerConvention enum with an exhaustive axes() method. Have both the constructor and the decomposer dispatch on it, and derive the decomposition generically from the axis triple (Tait-Bryan vs proper). Lift each builtin arm into its own function.

### R28 · fk-5e611b84370d · medium · confirmed · open
**kinematics-and-motion** `crates/reify-stdlib/src/flexures/common.rs::classify_geometry_invalid` — h10 inv-sf-1 h11

PRB flexure constructor rejections are silent for every class except one degenerate-geometry check, and that check is enforced by redundant reconstruction (heuristic 10: 'a census that re-derives state ... is the smell'; INV-SF-1). Each family's parser returns Option: parse_beam_inputs, parse_notch_inputs, parse_bending_hinge_inputs, parse_let_inputs, parse_prismatic_inputs, parse_compound_inputs and parse_cartwheel_inputs. Every rejection collapses to Value::Undef with no reason. flexure_diagnose then re-derives the geometry from the raw args through classify_geometry_invalid. That function holds a second copy of every constructor's positional argument layout, keyed by 13 name strings (common.rs:449-479), and a second copy of the t ≥ L / t ≥ 2r rules the parsers already apply (e.g. beam.rs:75). A missing youngs_modulus, a non-positive width or E, a non-unit axis, or a wrong arity stays silent by design (diagnostics.rs:124-137; common.rs:441-443). S8 α2 #8889 moves the flexure advisories with 'byte-identical' signals, so it does not by itself give the silent classes a code.

*Proposal:* Have each parser return Result<Inputs, FlexureInputError>, with one error enum naming the failed field and its value. Have the constructor arm return it as a coded rejection (S8's Rejection) and delete classify_geometry_invalid's layout table.

### R34 · fk-194bafc50401 · medium · confirmed · open
**language-pipeline** `crates/reify-compiler/src/annotations/schema.rs::SCHEMAS` — tests h6 kind:coverage

Tests stance and heuristic 6, a well-defined purpose for each entity. The production annotation registry SCHEMAS registers `@test_eval`, which its own comment calls a 'test-only annotation for annotation-args ε (#3556)'. It is valid on structure and occurrence in every shipped binary. It is the ONLY schema entry with an `EvalTime::AtMaterialization` arg, so the whole materialization pipeline exists in production only to serve it: compile_materialization_annotation_args in annotations/materialize.rs, which reify-compiler lib.rs re-exports, and its reify-eval consumer engine_eval.rs MaterializationArgType matching. The PRD leaf asked for it to be 'declared in the test harness registry' (task #3556 description), not the production table. A user who writes `@test_eval(value = ...)` gets real evaluation and AnnotationEvalFailed diagnostics. The tests reach the materialization path through a production entry that exists for them, so no production annotation pins that path.

*Proposal:* Move @test_eval behind a test-support-gated schema injection, or replace it with the first real AtMaterialization consumer (the @shell(thickness = <expr>) form the annotation-args PRD names). That gives the materialization path a production caller and keeps test-only surface out of the user language.

### R36 · fk-f8305a739f1a · medium · confirmed · open
**language-pipeline** `crates/reify-runtime/src/warm_startable_assert.rs::assert_warm_startable_coextensive` — h10 h6 comments kind:dead-code

The PRD §5 B5 warm-startable coextension gate (M-013) runs nowhere, and it would fail if it ran (heuristic 10: an invariant with no enforcement point). Its doc says it is 'Invoked once a WarmStartableRegistry has been assembled', and warm_registry.rs says 'The runtime calls WarmStartableRegistry::from_inventory at scheduler init'. But the scheduler was deleted by #5065 (c1b8dba3f7). git grep finds no production call of `assert_warm_startable_coextensive(` or `WarmStartableRegistry::from_inventory()`; the only callers are the assert's own unit tests and two per-crate inventory tests. Neither of those tests runs the assert over a real inventory. NodeKind::Resolution declares WARM_STARTABLE (node_traits.rs:238), but only Realization (kernel-occt) and Compute (solver-elastic) are submitted. node_traits.rs:181-193 itself says the assert would fail on the declared-without-registered direction. It cites `reify_runtime::concurrent::SchedulerConfig.warm_startable_registry`, a symbol #5065 deleted. The module doc still links `reify_types::WarmStartableRegistry`, a retired crate. Unlike commitment.rs's orphans (G-allow → #5023), none of this carries a G-allow marker.

*Proposal:* Delete the gate, from_inventory and the two inventory submissions, or wire a workspace-level test that links both producers and asserts coextension with Resolution explicitly allowlisted against a live task. Either way, fix the node_traits.rs/warm_registry.rs prose that names the deleted scheduler.

### R37 · fk-5c361cda62de · medium · confirmed · filed:9209
**language-pipeline** `crates/reify-test-support/src/orphan_audit.rs::run_orphan_audit` — tests h10 kind:coverage

The G-allow pin tests, which enforce the briefing's dominant 'scaffold without a caller' convention, all pass vacuously when the orphan audit cannot run. Nothing in the repo can force them to run. run_orphan_audit collapses EnvUnavailable into `None`. EnvUnavailable covers python3 or git missing, the script missing (`scripts/audit-orphan-producers.sh not found at …`), and a repo_root outside a work tree. Callers return early on `None`. Four pin files say the authoritative lane 'MUST set REIFY_REQUIRE_ORPHAN_AUDIT=1' to turn that skip into a failure, but no script, hook or orchestrator config sets it (git grep: reads only, in reify-audit tests; none in scripts/, hooks/ or dark-factory-orchestrator.yaml, and none in dark-factory). Two more callers (baseline_report_freshness.rs:50, engine_seam_orphans_g_allow.rs:94) do not read the flag at all. So renaming or moving the script during the S19/S20 script work would turn every G-allow pin green with only an stderr note.

*Proposal:* Make 'script absent' a hard failure (the script is tracked, so its absence is never environmental), and either set REIFY_REQUIRE_ORPHAN_AUDIT in verify.sh's gate or drop the flag and fail closed on the gate host. Fold this into #8240's shared helper.

### R35 · fk-b8a0e7566919 · medium · confirmed · filed:9217
**language-pipeline** `crates/reify-eval/src/measure_min_wall.rs::Engine.measure_min_wall` — h11 comments h10

Engine::measure_min_wall (measure_min_wall.rs) and its twin Engine::measure_min_feature (measure_min_feature.rs) have no production caller. Each is ~282 lines with its own tests and carries `#[allow(dead_code)] // consumed by ζ=4426`. That claim is false (heuristic 1 and the Comments stance): ζ #4426 is done, and it implemented the live path as a third copy of the same recipe, engine_constraints.rs::measure_thickness_pair (realize_solid_sdf → derive h from spacing → reify_shell_extract extractor → .ok()), called once at engine_constraints.rs:1992. Heuristic 11 (SPOT): the three copies already disagree on the one policy line they share, how h is derived from an empty spacing vector. measure_min_feature uses `spacing[0].min(spacing[1]).min(spacing[2])` with a comment that says the fold form would 'silently yield h=+INF and masquerade every result as BelowResolution'. measure_min_wall and the live measure_thickness_pair both use exactly that fold(f64::INFINITY, f64::min), and the live copy's comment calls it safe. The test fixtures box_2mm and TessellatingBoxKernel are copied three times (realize_solid_sdf.rs:299/318, measure_min_wall.rs:77/96, measure_min_feature.rs:81/100). The next change is likely to be wasted: three decomposed PRD leaves list both dead files as live geometry readers to migrate. These are S10 ι₃ #8992 and ι₆ #8998 (kernel-interface-typed-seam.md, 'Files' and 'openvdb_kernel_name and its 14 call sites go (… measure_min_feature, measure_min_wall …)'), and S2 ε #9157 (realization-pipeline-one-walk-typed-executor.md §5.6 / D8 'Readers … measure_min_feature/measure_min_wall').

*Proposal:* Delete measure_min_wall.rs and measure_min_feature.rs with their tests, and repoint the three PRD leaves (#8992, #8998, #9157) at measure_thickness_pair as the one thickness reader. Keep one h-derivation policy with a stated empty-spacing behaviour.

### R40 · fk-e19e2d1ba1e8 · medium · confirmed · filed:9188
**reify-audit** `crates/reify-audit/src/plinecite.rs::line_number_end` — comments kind:convention

Comments stance and the 'cite code by symbol, never by file:line' convention. PLINECITE, the merge-gated detector that S23's burn-down treats as its done-state ('no plinecite-baseline.txt row in this leaf's path set'), recognises only `<file>.rs:<digits>` (or ` esc-<id>:<digits>`): line_number_end returns None unless the extension is followed by ':'. The other line-cite spellings are invisible to it, namely `entity.rs ~2145`, `(entity.rs ~2249)`, `entity.rs (~2726-2790)`, `kernel.rs lines 178–215` and `lib.rs line 444`. 21 such comment cites exist in crates/*/src today, 13 in reify-compiler. Examples: connect.rs:728,732,733,740 (graph.rs ~429, unfold.rs ~308, entity.rs ~2145/~2249/~2726-2790), entity.rs:1030,1031,4224,4225,5703, expr.rs:585,946,950,4328, reify-core diagnostics.rs:483-484. They are the same stale-coordinate defect class but have no ledger row. S23's leaves will reach 'done' with them in place, and nothing stops new ones.

*Proposal:* Extend line_number_end's grammar to accept ` ~<digits>`, ` (~<digits>`, ` line(s) <digits>` after a .rs/.ri stem, and regenerate the ledger with --admit-new so S23's burn-down sees these rows. Alternatively, fold the 21 sites into the S23 leaves owning those paths.

### R38 · fk-d6e5b0480df8 · medium · confirmed · open
**reify-audit** `crates/reify-audit/src/lib.rs::JCodemunchOps.find_references` — h10 h3

The JCodemunchOps failure semantics are split (heuristic 10, uniformly enforced invariants; heuristic 3, one mechanism per axis).
- get_changed_symbols returns Result<_, JCodemunchCallFailed>, and P1/H2 turn an Err into a Low *Unexamined finding.
- The other four methods return bare Vecs. In RealJCodemunchOps each failed call_tool is logged to stderr and answered with `Vec::new()`.
- For find_references that empty answer means 'no callers'. On the P1 key-scenario path, a transient jcodemunch failure (timeout, reset, error envelope) after a successful get_changed_symbols becomes a P1ProducerOrphan false positive: Medium past grace, so the skill auto-files a task.
- A `None` decode (no table carried references) also returns empty.
- get_dead_code, get_untested_symbols and get_layer_violations fail soft to 'clean' in the same way (PDEAD, PUNTESTED and PLAYER are Low-only).
- S24 δ makes only GitOps fallible; the jcodemunch trait is outside it.

### R39 · fk-923f53493f57 · medium · confirmed · open
**reify-audit** `crates/reify-audit/src/p5_phantom_done.rs::has_task_completed_event` — h10 h11 h3

On key scenario 2 (P5 phantom-done), runs.db is the second database seam, and it has no run-level degraded outcome (heuristic 10, informatively and uniformly enforced; heuristic 11, SPOT).
- The bin opens it with `rusqlite::Connection::open`, which is read-write and creates a missing file. The default path `data/orchestrator/runs.db` resolves against the CWD, while every other path resolves against --project-root.
- Any query failure (missing table, a freshly created empty file, a locked DB) is handled per task inside check_one. Every `merged` done task gets its own Medium P5PhantomDone 'runs.db unreadable' finding, with the same Pattern as a real phantom-done, so one bad input yields one Medium per merged task in the corpus.
- Contrast: tasks.db degradation gets one breadcrumb, the --require-tasks-db guard and (S24 ε) a dispatcher-level DbUse.
- The /audit skill's routing says 'P5 findings never reach Medium in the periodic sweep context, so no Medium title template is needed'. So these per-task Mediums, and the other Medium P5 patterns (P5TestsAssertEmpty, P5MetadataFilesGitignored, P5LivePathStranded), reach a routing table that says they cannot exist.

*Proposal:* Open runs.db read-only and resolve its default against --project-root. Give runs.db the same degraded-outcome channel S24 ε designs for tasks.db: one run-level finding or breadcrumb plus a refusal flag, not one Medium per task. Correct the skill's P5 note; #7734 owns the stale 'P5 Medium kinds' claim.

### R41 · fk-7199821bfd6b · medium · confirmed · open
**tooling** `crates/reify-config/src/cache.rs::CacheResolverInputs` — h11 h10 h1

Under heuristic 11 (SPOT), reify-config's persistent-cache resolver is pure: the precedence lives in resolve_cache. Turning the process environment and config files into CacheResolverInputs, though, is written by each caller, and the two production callers disagree. reify-cli cache.rs::resolve_cache_root_layered and resolve_cache_root_with_cli read REIFY_CACHE_DIR, REIFY_CACHE_MAX_BYTES, XDG_CACHE_HOME and HOME, and load `$XDG_CONFIG_HOME|~/.config/reify/config.toml` plus `<cwd>/.reify/config.toml`. gui/src-tauri engine.rs::bootstrap_persistent_cache_sweep reads the same four env vars but passes user_config: None and project_config: None. Its doc nonetheless claims it 'mirrors reify-cli's resolve_cache_root pipeline'. With `[cache] dir` set in the user config, the CLI caches in that directory while the GUI sweeps `$XDG_CACHE_HOME/reify/fea`. The next change, pending #6799 (DC-rho: the GUI installs the FEA persistent cache it sweeps), is premised on 'it already calls the shared config-crate resolver, which is the same pipeline'. Implemented as written, it would point GUI engines at a different cache from the CLI's. The CLI side carries two more heuristic-1/10 costs. First, resolve_cache_root's doc says 'Config-file layers are deliberately not plumbed in', yet it delegates to resolve_cache_root_with_cli(None), which loads both files. Second, a malformed config file is something parse_cache_config treats as a loud hard error (CacheError::EmptyDir: 'a TOML file entry is always intentional'). In configured_eval_engine and mcp_context ensure_engine, that error silently disables the persistent cache with only a tracing::debug line.

*Proposal:* Give reify-config one `CacheResolverInputs::from_process` (or `resolve_cache_from_env(cli_dir)`) that owns the env reads and config-file discovery. Have the CLI and GUI call it, and correct #6799's premise before it lands.

### R49 · fk-0a1125558289 · low · confirmed · open
**analysis** `crates/reify-fdm/src/as_printed.rs::select_rungs` — inv-pd-1 h1 kind:critical-path

Critical path: the briefing's FDM scenario says as_printed_material returns R-fast and refines to R0 when PrusaSlicer is on PATH. The R0 rung cannot be reached from a .ri file (INV-PD-1, declared-param-reaches-kernel; heuristic 1). The stdlib routes `@optimized("fdm::as_printed_material_r_fast")` only (fdm_as_printed.ri:61). No .ri declares fdm::as_printed_material_r0, although reify-eval registers it (compute_targets/mod.rs:615). The rung ladder has zero production callers: reify-fdm select_rungs and reify-eval rung_compute_key are called only by tests. AsPrintedOptions.target_fidelity is documented as a 'placeholder' and reaches nothing. The only test of the ladder, fdm_progressive_refinement_e2e.rs, calls both trampolines directly, so it would stay green if the DSL never reached R0. One step along the same path: as_printed_material_r0_trampoline degrades malformed G-code or a malformed FDMProcess to an Undef-lambda field with `diagnostics: Vec::new()` (silent fail-soft). That part is reify-eval's, outside this slice. The ruling is already a filed decision gate.

*Proposal:* No new task. Correct the briefing scenario to 'R-fast; R0 pending #7087'. When #7087 is ruled, the silent R0 degrade gets a coded diagnostic.

### R52 · fk-ee0f72937e55 · low · confirmed · filed:9214
**analysis** `crates/reify-solver-elastic/src/eigensolve.rs::EuclideanOperator` — h11 h1 kind:cross-module

Heuristic 11 (SPOT) and heuristic 1 (informative docs): the crate has two sparse-Cholesky drivers, while the new seam's docs say it has one. linear_solve/direct.rs opens with 'The one sparse-Cholesky driver of reify-solver-elastic … every faer call passes Par::Seq', and the solver-performance PRD's C1 says the seam is the only Cholesky caller besides SplitCholesky. However, eigensolve.rs EuclideanOperator factors through faer's high-level `sp_cholesky(Side::Lower)` (eigensolve.rs:1560, :1566) and `sp_lu()` (:1780). In faer 0.24 those methods read get_global_parallelism() internally (sparse/solvers.rs imports it and calls it at many sites). The PRD's BT6 grep passes only because the read is inside faer. As a result, modal shift-invert stays bit-stable only by feature unification (faer has no rayon feature today), which is the condition C2 was written to end. The PRD puts the eigensolves out of scope (§8), but nothing tells the next agent so: direct.rs's header contradicts the code.

*Proposal:* Route EuclideanOperator's two SPD factorisations through linear_solve::direct::analyse/factorize (as SplitCholesky already does). Otherwise, narrow direct.rs's header and C1 to say the eigensolve's own factorisation still reads the faer global.

### R53 · fk-35f5e0982032 · low · confirmed · filed:9215
**analysis** `crates/reify-solver-elastic/src/math.rs::MIN_JACOBIAN_DET` — inv-sf-5 comments h1 kind:stub

Placeholders-owned-and-loud (INV-SF-5) and the Comments stance: two degeneracy-guard constants are labelled placeholders whose replacement is promised to 'PRD task #21', a PRD-relative index that no live task owns. One is solver-elastic math.rs MIN_JACOBIAN_DET ('PRD task #21 (diagnostics) will replace this placeholder with a … mesh-scale-aware degeneracy detector'). The other is mesh-morph elasticity.rs MIN_VOLUME ('PRD task #21 will replace this placeholder … until then we fail-finite-but-garbage on degenerate input'), stated twice. In structural-analysis-fea.md, item 21 is the diagnostic-mapping task, which shipped (diagnostics.rs, fea_diagnostics_e2e.rs) without replacing either constant. mesh-morphing.md has no item 21 at all. The PTODO grammar classes PRD-relative indices as malformed-cite, but 'placeholder' is not a TODO marker, so no gate sees these promises. The census row for prose task-promises is unenforced (bookmark #7098). elasticity.rs also cites `reify-solver-elastic/src/result.rs:39` by file:line, against the briefing convention.

*Proposal:* Either file a live owner for a mesh-scale-aware degeneracy check and cite it as #NNNN, or reword both constants as deliberate floors and drop the 'will replace' promise and the file:line cite.

### R42 · fk-5882b549863f · low · weakened · open
**analysis** `crates/reify-eval-fea-tests/src/lib.rs` — tests kind:coverage

Tests stance: the crate's doc presents it as the 'engine-driven synthetic-mesh FEA e2e suite', and the briefing names it as proof of user reachability. Yet 5 of its 33 test files construct no Engine and call trampolines directly with hand-built Values. These tests stay green if the stdlib .ri declaration, the compiler's ctor lowering or compute dispatch stops delivering what they hand-build. buckling_element_order_dispatch.rs is an example. Its Run A depends on an absent `element_order` field silently defaulting to P1. The compiler never produces that shape, because ctor-built instances carry every declared field (trampoline-decode-and-result-build.md §2), and that PRD's D1 turns field absence into an Error.

*Proposal:* For each trampoline-direct file, either add an engine-driven sibling from a .ri fixture or build its inputs through S15's stdlib_default_instance. Reword the crate doc to say which files are seam-level.

### R51 · fk-3e6917a7a1ae · low · weakened · filed:4944
**analysis** `crates/reify-solver-elastic/src/adaptive.rs::probe_target_accuracy` — h6 kind:dead-code kind:convention

Possibly dead, so verify before removing. probe_target_accuracy is a public producer with no production consumer and no G-allow marker, which breaks the orphan-producer convention (heuristic 6, purpose without a consumer). Its references are adaptive.rs, the lib.rs re-export and tests/adaptive_refinement_tests.rs. Its own doc says 'a live call site … is deferred future work. Today the auto-resolve loop … has no FEA-per-probe call site to wire this into', with no task cited. The G-tool flags it new_vs_baseline. It and NEAR/FAR_FROM_BOUNDARY_TARGET_ACCURACY were added by task #3000 (6e1334b7fc, 2026-07-01).

*Proposal:* Delete it with its constants, or add a G-allow citing a live owner for per-probe FEA accuracy in auto-resolve.

### R50 · fk-f590a349dc61 · low · confirmed · filed:9216
**analysis** `crates/reify-fdm/src/lib.rs` — comments h1 kind:deep-read

Heuristic 1 and the Comments stance: reify-fdm's crate doc misdescribes the crate. It says it 'hosts two pure, dependency-free pieces', then lists three modules (zone, correlation, toolpath). It omits as_printed, slice and r0, although lib.rs declares all of them. It also states 'the crate has **no** external dependencies' apart from reify-gcode. Cargo.toml contradicts this: it lists tempfile as a normal dependency and libc under cfg(unix), and its own comments mark both as production dependencies of the slice module. The doc's 'Real-body integration … is owned by the downstream δ-task' is also stale, since δ (#3786) is done. The briefing's key_decisions have to correct this doc ('reify-fdm is no longer dependency-free'), which is the drift the Comments stance warns about.

*Proposal:* Rewrite the crate doc to list the modules and dependencies as they are, or reduce it to one sentence that points at the PRD.

### R43 · fk-aeba170a962b · low · confirmed · filed:9219
**analysis** `crates/reify-eval/src/compute_targets/buckling_multi_case.rs::solve_buckling_multi_case_trampoline` — h11 h3

Heuristic 11 (SPOT): the LoadCase iteration driver exists twice, once in multi_case.rs::solve_multi_case_trampoline and once in buckling_multi_case.rs::solve_buckling_multi_case_trampoline. Each copy has the same per-case extraction of name, loads and supports with the same Failed messages; the same cooperative-cancel check; the same duplicate-name Warning text; the same `..`/`vec![]` structured-detail drop; and the same `Value::Map{"cases"→Map}` wrap. The only differences are the sub-trampoline called and the options rule (per-case ElasticOptions override vs. always-shared BucklingOptions). The copies have already drifted: only multi_case has the body overload. The structured-detail defect recorded against multi_case.rs therefore has to be fixed twice.

*Proposal:* Extract one `run_load_cases(cases, per_case_inputs_fn, sub_trampoline, ..)` driver that both targets call.

### R47 · fk-21ccc7e905aa · low · confirmed · filed:9220
**analysis** `crates/reify-eval/src/engine_compute.rs::run_compute_dispatch` — kind:defect h7

run_compute_dispatch takes the prior warm state out of the pool before the persistent-cache lookup. On a persistent HIT it then drops that state and the prior cost. It sources prior_warm_state as `self.cache.get_warm_state(..).or_else(|| self.warm_pool.checkout(..))`, and checkout REMOVES the pool entry (as the in-file tests document). It then runs the persistent lookup. The HIT arm completes with `new_warm_state: None` and cost 0.0 and returns, never restoring the prior. The Cancelled, Failed and unregistered arms do restore it, under PRD §5's 'the prior must survive' rule. So after an edit parks a warm state in the pool, a persistent HIT throws that warm-start seed away and the next real solve runs cold. The HIT arm diverges from the other arms by convention only, the same class as fk-34bddf1fcbce's mirror-by-convention cost (heuristic 7: temporal coupling on the order of checkout vs. lookup).

*Proposal:* Do the persistent lookup before taking the prior out of the pool, or donate the prior back on the HIT arm. S5 ε's single completion hook can carry this.

### R48 · fk-98fc505b592f · low · confirmed · filed:9221
**analysis** `crates/reify-eval/src/modal_ops.rs::solve_modal_core` — h1 tests kind:dead-code

solve_modal_core is production code that only tests use, and its G-allow marker says the opposite of the doc above it (heuristic 1, a name or marker that lies). The rustdoc says '#[allow(dead_code)]: since task κ the production trampoline composes assemble_modal_km + eigensolve_modal directly … this convenience wrapper is exercised only by the modal_ops unit tests'. The G-allow line directly below says it is 'reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140)'. That is false: run_modal_analysis calls assemble_modal_km and eigensolve_modal itself, and register_compute_fns is now at mod.rs:534. promote_beam_mesh_to_p2's doc likewise says the trampoline 'feeds the promoted (nodes, tets) into … solve_modal_core'. reify-audit's G-allow test fixture (new_orphans_2026_05_30_g_allow.rs:264) lists it, so the orphan audit counts it as justified.

*Proposal:* Move solve_modal_core into the test module (or delete it in favour of the composed calls), remove its G-allow and audit-fixture row, and correct promote_beam_mesh_to_p2's doc. S23 β₂ only re-points the mod.rs:140 cite, which would leave the false reachability claim in place.

### R54 · fk-54ada9ac9619 · low · weakened · filed:9222
**analysis** `crates/reify-stdlib/src/loads.rs::is_load_value` — kind:dead-code comments h6

The Map-based load and support kind registries in reify-stdlib are dead remnants that #[allow(dead_code)] hides, and their docs carry an expired promise (Comments stance). is_load_value, LOAD_KINDS (now just ["gravity"]), is_support_value and SUPPORT_KINDS are all #[allow(dead_code)] and called only by their own tests. Their docs say 'Used by the FEA solver (PRD task 16) once it lands; not yet called from any external module'. The FEA solver landed long ago and consumes the typed `PointLoad`/`PressureLoad`/`Gravity`/support StructureInstances, never these Maps. loads.rs's module doc says selector targets are validated 'as a narrow placeholder set … until the topology-selector variants land' through helpers::validate_selector_target. Typed selectors have landed, and validate_selector_target has no production caller either.

*Proposal:* Delete is_load_value / LOAD_KINDS / is_support_value / SUPPORT_KINDS (and validate_selector_target if no other caller) with their tests, and correct loads.rs's module doc.

### R44 · fk-41d6ac48ae56 · low · confirmed · filed:9223
**analysis** `crates/reify-eval/src/compute_targets/fea_diagnostics.rs::fea_structured_detail` — kind:dead-code tests h9

Possibly dead; verify before removing. Several `pub` functions in this slice's compute targets have no production caller and are exported only so tests can reach them (Tests stance; heuristic 9, an interface wider than its function). fea_structured_detail is a one-line wrapper over FeaFailure::structured_detail. It was built for GUI consumer task 2966, but production calls failure.structured_detail() directly and only the wrapper's own test calls the wrapper. bc_resolve::resolve_selector_faces is a one-line wrapper over topology_selectors::resolve, called only by bc_resolve's tests. as_printed_material::rung_compute_key is test-only (see the R0 critical-path finding). modal::transient::duhamel_solve is test-only; production uses duhamel_coefficients and duhamel_solve_with_coeffs. result_topology::from_realized_mesh is test-only (see the modal Selector finding). reify-eval's lib.rs also has `#[doc(hidden)] pub use compute_targets::elastic_static::PROGRESS_STRIDE` for a single integration test, and the constant's doc says 'Exposed pub(crate)' while it is declared `pub const`.

*Proposal:* Make the test-only wrappers private test helpers or delete them. For PROGRESS_STRIDE, have the progress test assert cadence through the public sink instead of importing the constant.

### R45 · fk-72c5899f1a45 · low · confirmed · filed:9224
**analysis** `crates/reify-eval/src/compute_targets/mod.rs::register_compute_fns` — comments h1

Comments stance / heuristic 1: the registration code carries rationale that has become false. register_compute_fns says twice that the VolumeMesh and boundary demand hooks for solver::elastic_static have 'No production effect on the current FEA signature (no geometry arg → the demand override never fires)'. Since #4870, solver_elastic.ri has a `body : Solid` overload of solve_elastic_static that drives exactly that demand. shell_extract_compute.rs::register_shell_extract_compute_fns justifies itself with 'no workspace-wide register_compute_fns function exists today'. Both compute_targets::register_compute_fns and the canonical Engine::register_production_compute_fns bundler exist and call it. The module doc's 'Placement rationale' still proposes moving ComputeFn/ComputeOutcome into reify-ir. Those types now live in reify-compute-contract (#4934), and S5 has ruled a different home.

*Proposal:* Delete the 'no production effect' clauses and the shell-extract design note. Replace the placement rationale with a pointer to docs/prds/v0_6/analysis-stratum.md.

### R46 · fk-735c24dd849f · low · confirmed · filed:9225
**analysis** `crates/reify-eval/src/compute_targets/result_topology.rs` — comments tests

Comments stance: TDD-phase prose survives in this slice's tests after GREEN, and some of it is now false. result_topology.rs's tests carry five 'RED: … does not exist yet' / '(not implemented yet)' doc lines. They cover all_finite, is_empty, from_realized_mesh, and from_value's non-finite rejection, all of which exist and are tested in the same file. 'RED' or 'not yet defined' text appears in 64 lines of elastic_static.rs, 21 of modal_ops.rs, 13 of dynamics_ops.rs, 7 of compute_targets/mod.rs and 6 of compute_persist.rs. Sweeps of the same class have been filed one file at a time (#8538, #7062, #9034, #3715, #3299) with no detector, so it re-accretes.

*Proposal:* Delete the TDD-history prose from these test docs. A reify-audit comment detector (not a per-crate test) would stop re-accretion.

### R55 · fk-e6c1343608c1 · low · confirmed · filed:6616
**assemblies** `crates/reify-compiler/src/conformance/sub_component_validation.rs::check_sub_structure_existence` — h10 kind:critical-path

Heuristic 10, informatively. The assemblies key scenario says a derived sub `sub b = mirror of a across P { }` has 'ZERO runtime effect'. Measured, it is a hard compile error that names an empty structure. ts_parser::lower_sub leaves structure_name as String::new() on the derived arm by design (the discriminator invariant), and nothing in reify-compiler reads SubDecl.derivation. So check_sub_structure_existence reports `error: sub-component "b" references unknown structure ""` and `reify check` exits 1. The message names neither the derivation nor the fact that derived subs are not yet lowered. The compile-scope rejection E_DERIVED_SUB_UNSUPPORTED is designed under #6616 (A-β, pending) but has not landed, and no compiler test pins today's behaviour (derived-sub tests exist only in reify-syntax).

*Proposal:* Until #6616 lands, have check_sub_structure_existence (or the Sub pre-pass) recognise `derivation.is_some()` and emit a coded 'derived subs are not yet compiled' error rather than an unknown empty structure. Correct the briefing's assemblies key_scenario wording.

### R56 · fk-137717ade779 · low · confirmed · filed:9189
**assemblies** `crates/reify-compiler/src/connect.rs::ConnectAccumulator` — comments kind:deep-read

Comments stance (comments drift). The doc on ConnectAccumulator.value_cells says it is 'Unused by `compile_connection` until task ε wires the connect-param producer (step-6)'. compile_connection pushes scoped Auto ValueCellDecls into acc.value_cells (Case 3 of the connect-param auto lookup), so the field comment states the opposite of the code. The same block cites four call-sites by approximate line ('graph.rs ~429, unfold.rs ~308', 'entity.rs (~2145)', 'entity.rs ~2249', 'entity.rs (~2726-2790)'), which also breaks the cite-by-symbol convention (see the PLINECITE finding).

*Proposal:* Rewrite the field doc to say what the sink receives, and replace the '~line' cites with symbol names (PendingSubOverrideAuto, the entity.rs Sub paren-form auto-args lookup function).

### R65 · fk-67a04cc482f3 · low · confirmed · filed:8926
**constraints-and-geometry** `crates/reify-kernel-openvdb/src/kernel_real.rs::VOXEL_BOOL_STUB_MSG` — h1 comments kind:deep-read

Heuristic 1 (a message that lies): VOXEL_BOOL_STUB_MSG says 'OpenVDB voxel-Boolean execution requires Voxel handles on both operands ... remains future work (task ε)'. OpenVdbKernel returns it for every execute op other than Surface/ApplyTransform (kernel_real.rs:597) and for every GeometryQuery variant (query() at :716-718). A Volume, BoundingBox or Centroid query on an OpenVDB-owned isosurface handle is therefore answered with a message about voxel Booleans. This is the same cost review-all recorded for Manifold's STUB_MSG (fk-10ec3d2a1b76), which #8529 fixed for Manifold only (e38074ad09: catch-alls name the variant and kernel). It becomes user-visible once S10 ι₅ #8995 routes handle-served queries to the owning kernel. gmsh's TRAIT_DISPATCH_MSG (kernel_real.rs:631-656, 'not yet routed for Mesh→VolumeMesh' although mesh_surface_to_volume is routed) has the same shape, but meshing-service ε #8288 deletes that impl.

*Proposal:* Match the Manifold fix: execute → 'unsupported op {kind_name} on openvdb kernel', query → 'unsupported query {kind_name} on openvdb kernel'; drop the 'task ε' text.

### R63 · fk-ddb0fd4f4ca5 · low · confirmed · filed:9197
**constraints-and-geometry** `crates/reify-kernel-gmsh/src/ffi.rs::mesh_set_size_at_entity` — h1 comments kind:dead-code

`pub fn mesh_set_size_at_entity` has had no caller since 30128ffaf9 (feat(7447), 2026-09-19), which replaced the per-corner size loop in refine_volume_with_size_field with a background size field. `git grep -w mesh_set_size_at_entity` finds only its definition (ffi.rs:977) and the comments at ffi.rs:279-282. The `// G-allow:` marker on the extern `gmshModelMeshSetSize` (ffi.rs:282) still names 'the per-vertex size-hint loop in refine_volume_with_size_field' as the live consumer. That makes the exemption false (heuristic 1), and it blinds the orphan-producer audit to two dead symbols (the wrapper and the extern binding). The 2026-10-15 G-tool run already flags the wrapper as new_vs_baseline.

*Proposal:* Delete mesh_set_size_at_entity and the gmshModelMeshSetSize binding, along with its G-allow. Also reword the ffi.rs:288-293 'Unlike gmshModelMeshSetSize above' prose.

### R64 · fk-4334831c82cc · low · weakened · filed:9198
**constraints-and-geometry** `crates/reify-kernel-openvdb/src/kernel_real.rs::OpenVdbKernel` — h6 kind:dead-code

The public inherent methods write_vdb_grid, sample_sdf_at and active_voxel_count have no production caller. git grep -w over crates/*/src and gui/src-tauri finds only their definitions; every other use is under tests/ (grid_io_tests, realize_voxel_tests, mesh_to_voxel_resolution_tests, ingest_mesh_densify_tests, and imported_field_e2e tests that use write_vdb_grid as a fixture writer). The module doc says they are 'exposed for the v0.4 shells use-case (direct callers)', but no such caller exists. Unlike their siblings open_vdb_grid_for_test and grid_name_for_test, they are not gated behind `test-fixtures`, and they carry no G-allow, contrary to the briefing's every-public-producer-has-a-consumer convention. This is possibly dead; verify against v0.4-shells plans before removing.

*Proposal:* Gate the three behind `test-fixtures` (they are test probes), or add G-allow markers that cite the owning shells task.

### R62 · fk-ee411fb016d2 · low · confirmed · filed:9199
**constraints-and-geometry** `crates/reify-kernel-conformance/tests/occt_gmsh_attributed_conformance.rs` — tests h1 kind:coverage

Tests stance (a conformance claim that cannot fail): occt_box_attributed_volume_preserves_face_attribution asserts only `vm.boundary.is_some()` plus the structural volume-mesh checks. GmshKernel::mesh_surface_to_volume_attributed unconditionally sets `volume.boundary = Some(report.boundary)` (gmsh kernel_real.rs ~830), so the assertion is always true. A producer that matched zero faces (Some(empty), the case #8113 described) or attributed nodes to the wrong faces would pass. The test name promises face-attribution preservation that it never checks. This is the 'conformance passes but is vacuous' pattern the briefing's kernel-seam key_decision warns about. The attributed arm is slated for deletion by meshing-service ε #8288 (#8113's content was ported there), so the replacement conformance arm should assert the C5 invariants (every face ordinal has ≥1 triangle, every boundary node exactly one entry).

*Proposal:* Until #8288 lands, assert that each of the box's 6 face anchors owns at least one attributed boundary node. Make #8288's conformance replacement assert C5's invariants.

### R68 · fk-452afde2698e · low · weakened · filed:6942
**constraints-and-geometry** `crates/reify-stdlib/src/trig.rs::eval_trig` — h10 inv-AD-1 h11 kind:defect

Heuristic 10 (one invariant, enforced uniformly) is broken at runtime in the transcendental builtins. Forward trig checks its argument's dimension: trig_input (helpers.rs:102-118) rejects any non-ANGLE Scalar, so sin(5mm) is Undef as the briefing scenario says. The inverse trig, exponential/log and hyperbolic arms take a different path. asin, acos, atan, atan2, log, log10, exp, sinh, cosh and tanh go through unary_f64/binary_f64 (helpers.rs:31-49), which call Value::as_f64. as_f64 (reify-ir value.rs:1638-1645) returns si_value for ANY Scalar and discards its dimension. So asin(0.5mm) silently returns an Angle of asin(0.0005), exp(5mm) returns exp(0.005), and atan2(3mm, 2s) returns a value from mixed dimensions, all with no diagnostic. The compiler documents these arguments as 'accept dimensionless Real' (math_signatures.rs:120, :406) but never checks them; that compile-time half is already #6249. Under INV-AD-1 this is an unchecked path that turns a Length into an Angle.

*Proposal:* Add one dimensionless_input(v) helper beside trig_input (Real/Int, or a DIMENSIONLESS Scalar) and route the inverse-trig, log/exp and hyperbolic arms through it, so asin(5mm) is Undef just as sin(5mm) is. Fold this into #6249's scope as its runtime half.

### R57 · fk-b2deb4ebef8b · low · weakened · filed:9227
**constraints-and-geometry** `crates/reify-constraints/src/classifier.rs::is_geometry_qualified_name` — tests h12 kind:coverage

Tests stance: the routing that sends constraints to the geometric (SolveSpace) slot is tested only with qualified names the compiler never emits. classifier_tests.rs and solvespace_tests.rs both build their FunctionCall fixtures with a shared helper that formats `std::geo::{name}`. The compiler builds every stdlib call as `format!("std::{}", name)` (expr.rs:3487). The production vocabulary therefore takes untested paths: std::angle is not matched (only the nonexistent std::angle_between is), and std::fasten is missing. Both suites would stay green if every compiler-emitted geometry name stopped routing Geometric. Separately, #6681's description says production() leaves the logical and fallback slots None and calls #5469 a race. #5469 is done, and registry.rs:313-320 now installs CpSatSolver and DiscreteFirstFallback, so the cpsat::can_enumerate inversion that #6681 calls latent is now live.

*Proposal:* When #6681 replaces name routing, drive the routing tests from compiled .ri sources (or from names taken from the compiler's own tables) instead of hand-built std::geo:: calls. Also update #6681's description: the production-slot premise is stale.

### R60 · fk-7bb7b0b121a5 · low · confirmed · filed:9228
**constraints-and-geometry** `crates/reify-eval/src/appearance.rs::coating_appearance` — h11 h12 inv-SF-3

Heuristics 11 and 12: appearance.rs restates the stdlib's CoatingProcess and FinishProcess enums (surface_finish.ri:44, :78) as bare variant-name strings. coating_appearance matches "Anodize"/"Electroplate"/"PowderCoat"/"Paint"/"Passivate" with a `_ =>` arm that returns a generic Satin appearance. finish_modulation matches "Polished"/"Lapped"/"Ground"/"Brushed"/"BeadBlasted"/"AsCast" with `_ => return base_appearance.clone()`. Neither arm emits a diagnostic. A variant added to either .ri enum therefore renders as the default look with no compile error and no warning, so a declared intent is silently dropped (INV-SF-3). The two lists match today; nothing enforces that they keep matching.

*Proposal:* Make the variant tables exhaustive against the stdlib enum (e.g. a test that loads surface_finish.ri and asserts every CoatingProcess/FinishProcess variant has an arm), or emit a coded warning from the default arms.

### R67 · fk-152e4fb58bd7 · low · weakened · filed:9229
**constraints-and-geometry** `crates/reify-stdlib/src/stackup.rs::parse_chain_checked` — h10 h1 h12

Heuristic 10, informatively: parse_chain_checked folds three unrelated failures into StackupError::DimMismatch. They are a chain item that is not a Map, a 'distribution' value that is not a Distribution enum, and a Distribution enum with an unknown variant. diagnose reports all three as 'E_StackupDimMismatch: contributor field must be a finite LENGTH scalar', which names the wrong cause and no offending value or index. Heuristic 12: the distribution also round-trips through strings. contributor_asym's parse_distribution returns a &str ('Normal'/'Uniform'/'Triangular'), make_contributor_map stores it, and parse_chain_checked re-matches the same three strings into the private Distribution enum, so the variant list is written twice in one file.

*Proposal:* Add StackupError variants for a malformed item and an unknown distribution, each carrying the item index (and variant name), and make parse_distribution return the Distribution enum used by both paths.

### R66 · fk-655f2584d563 · low · weakened · filed:9230
**constraints-and-geometry** `crates/reify-stdlib/src/registry_dispatch.rs` — tests h9 h13

Tests stance: reify-stdlib exports two #[doc(hidden)] pub shims behind a test-support feature, __registry_dispatch_for_test and __try_dispatch_for_test (lib.rs:358-400). Their only purpose is to let one integration test (tests/registry_dispatch_seed_parity.rs:65-66) reach the private registry_dispatch module's dispatch(EvalBuiltinId) and try_dispatch(name). That test's own header says no layer of it 'can distinguish "the registry answered" from "a deleted legacy matcher would have answered identically"'. The private seam is held open for a test that cannot assert the property the seam exists for, while the public eval_builtin path (its layer 2) already covers the observable behaviour.

*Proposal:* Keep layer 2 (public eval_builtin parity with hand-written expectations) and move layers 1 and 3 into registry_dispatch.rs's own #[cfg(test)] module, then delete the shims and the feature.

### R61 · fk-240270804762 · low · weakened · filed:9231
**constraints-and-geometry** `crates/reify-eval/src/realization_read_test_support.rs::seed_kernel_realization` — tests comments h7

Tests stance: the realization-read γ seam tests (realization_read_gamma.rs and realization_content's tests, through realization_read_test_support) seed Engine's private realization_handles map and graph.realizations directly, instead of building a realization through Engine. realization_read_gamma.rs:7-13 justifies this as 'the production build path can't deliver a Value::GeometryHandle to a compute target until task 4091', and realization_content.rs:31,41 treat 4091 as future work. Task #4091 is done: it merged 2026-06-26 (dddf42b693), wiring elastic_static's VolumeMesh demand. So the seeded private state stands in for a path that now exists, and the prose justifying it is stale.

*Proposal:* Re-drive the γ seam test through a compiled .ri whose @optimized compute target receives a realized VolumeMesh (the path #4091 delivered), delete the private-state seeding helpers, and drop the stale 4091 deferral prose. S2 ι/ζ (#8534) is touching the same module's prose.

### R58 · fk-5c839728f680 · low · weakened · filed:9232
**constraints-and-geometry** `crates/reify-constraints/src/decompose.rs::decompose_into_components` — tests h9 h11

decompose_into_components is a pub, re-exported crate API (lib.rs:22) with no production caller. Its own doc tells production callers to use decompose_into_components_with_reads, and the only production path is SolverRegistry's decompose_prelude (registry.rs:131-260). That path derives the objective refs itself, from ObjectiveSet terms via dependent_reads::collect_value_refs. The public wrapper instead takes a caller-built objective_refs HashSet. Seven test files across reify-constraints and reify-eval decompose through the wrapper, so they exercise a second objective-ref derivation that production never runs. A change to decompose_prelude's objective handling would not be seen by them (tests stance; heuristic 11, two homes for 'which refs an objective contributes').

*Proposal:* Expose decompose_prelude's derivation through a public function that takes a ResolutionProblem (as objective_consumption already does), point the tests at it, and make the hand-built-refs wrapper pub(crate) or delete it.

### R59 · fk-33bcae9b2dd4 · low · confirmed · filed:9233
**constraints-and-geometry** `crates/reify-constraints/src/decompose.rs::domain_of_auto` — comments h13

Comments stance: decompose.rs's production half (lines 1-640) is 56% comment lines (358 of 641), and much of it answers reviewers. Examples: 'review round 3, suggestion 2' (line 240), 'This ONE pass replaces the two that stood here (review round 3, suggestion 4)' (line 595), the multi-paragraph 'Why the match is EXHAUSTIVE' defence on widen_domain, and the arm-order defence inside domain_of_auto. S23 δ₁'s paths cover reify-constraints solver/**, trial.rs and cpsat.rs, and δ₂ covers registry.rs. decompose.rs is in neither leaf, though it is the routing core of the auto-param key scenario.

*Proposal:* Add decompose.rs to S23's residual sweep (θ) or δ₁. Move the review-round history and alternatives to commit messages, keeping each function's routing contract plus a pointer to discrete-cost-minimisation.md.

### R74 · fk-882bde3953cf · low · weakened · open
**factory-infra** `scripts/seed-warm-lane.sh::_close_inherited_fds` — tests h11

Tests stance (test access to internals is an interface smell) and heuristic 11. test_seed_warm_lane.sh reaches seed-warm-lane.sh's private helper by source text. V18 extracts the shipped `_close_inherited_fds` body with `sed -n '/^_close_inherited_fds() {$/,/^}$/p' "$SCRIPT"` (5214). It sources the body into a probe, then builds its mutation control by awk substring replacement of the eval line. V8, V13 and V17 (4896-5190) lint seed's whole source with an awk DETACH predicate that requires every backgrounded `&` line to carry a stdout redirect, `_close_inherited_fds` and `</dev/null`. That predicate, `is_detach` with `/[^&>|]&[[:space:]]*$/`, is a hand copy of `_flock_fork_offenders`' is_detach in test_flock_detached_fork_guard.sh:162-164, kept in agreement by a comment saying it 'borrows' the definition. The private helper has no public seam, such as a sourced lib function, so its contract is pinned through regexes over the production file's layout. Moving the helper into a lib or reformatting its closing brace would silently empty V18's fixture, which only a grep sentinel guards.

*Proposal:* Move _close_inherited_fds, which a future thin and gc consumer would also need, into a small sourced lib with its own hermetic test. Move the DETACH predicate into one shared test lib used by both guards.

### R73 · fk-00f86e271fb2 · low · confirmed · filed:9183
**factory-infra** `scripts/refresh-warm-base.sh` — h5 h10

Heuristic 5 (minimum lifetimes). Per-generation sidecars outlive their generation and are never reaped. The retired-gen GC loop iterates `${BASE_DIR}.gen.*`, skips anything that is not a directory (`[ -d "$_gc_gen" ] || continue`), and removes `.lock`, `.basecommit` and `.buildroot` only after reaping that gen's dir. A sidecar whose dir is already gone is invisible to every later sweep. warm-lane-gc.sh:500 opens the gen lock with `exec 9>>"${gen}.lock"`, which creates it, before re-checking `[ -d "$gen" ]`. A gc reader racing a refresh GC therefore re-creates the lock of a reaped gen. On the live pool, /home/leo/src/warm-lanes/base holds 18 .lock files for gens 3-550 whose directories no longer exist (dated 2026-08-18 to 09-30), plus gen.125's orphaned .basecommit and .buildroot. Only gen.588 is live.

*Proposal:* Sweep `${BASE_DIR}.gen.*.{lock,basecommit,buildroot}` whose gen dir is absent and whose lock is free. Have readers open the gen lock without creating it, for example `9<` on an existing file with a missing-file arm.

### R71 · fk-b0abbccfe141 · low · confirmed · filed:9184
**factory-infra** `scripts/jobserver-balancer.py::main` — h4 comments h12

Heuristics 4 and 12, and the Comments stance. main() is 224 lines (594-818). FIFO lifecycle, owner stamps, seeding, signal setup, the whole per-tick control loop (sense, pressure stage, idle counter, decide, give-back suppression, transfer) and shutdown cleanup are all inline. The tick therefore has no seam a unit test can drive. Its stage outputs are bare strings ('hold'/'release' from pressure_decide; 't2m'/'m2t'/'none' from decide) dispatched by if/elif equality. About half the body is prose, including reviewer-answering text such as 'Confirmed safe against jobserver-canary.sh (review comment 4, round 2)', a sub-millisecond ORDER NOTE race defence, and 'Tests assert the PARTITION PROPERTY, not a numeric value'.

*Proposal:* Extract a `tick(state, fds) -> state` function with enum actions, and keep main() as setup, the loop and teardown. Move the review-defence prose to the commit and PRD homes. Pair with ρ (#8727), which already moves the balancer's unit oracle to stdlib unittest.

### R76 · fk-6365d50f0d65 · low · confirmed · filed:9101
**factory-infra** `scripts/verify.sh::_verify_cleanup` — h11 h10 kind:cross-module

Heuristic 11, SPOT: _verify_cleanup feeds two run ledgers from one exit, and they derive the run's exit code by different rules. #7424 added _vl_finalize. It deliberately prefers _VL_EXIT_RC, the last plan step's rc, over the trap's $?, 'which the Node-lane EXIT trap resets before calling _verify_cleanup', and it maps INT/TERM/HUP to 130/143/129 from a new trap argument. The adjacent _record_background_verdict still reads the raw $? captured as _exit_rc and ignores the signal argument. So the S18 §2.4 defect 3 (the background sweep ledger records pass/exit_code 0 for failed runs) was worked around for the new ledger only. The two ledgers can now disagree about the same run.

*Proposal:* Compute the run's exit code once in _verify_cleanup (last-step rc, then signal mapping) and pass it to both recorders; or leave it to S18 epsilon's port.

### R75 · fk-3f08a4c422f2 · low · weakened · filed:9035
**factory-infra** `scripts/verify-ledger.py::PHASE_RULES` — h12 h11 kind:cross-module

Heuristic 12, structured data instead of meaningful strings. The verify instrumentation ledger, new since review-all (#7424, 1,065 lines), recovers each plan step's kind by substring and regex rules over verify.sh's emitted shell text. Examples are 'bash tests/infra/run_all.sh', '_VERIFY_NODE_BG_PID=$!', 'reify-audit binary missing' and 'RELEASE-PASS: skipped (delta-clean)'. The fallbacks CARGO_OTHER, PREFLIGHT and OTHER mean a reshaped add() line silently loses its sccache/concurrency snapshots, its nextest JUnit split, or the release_pass decision. This is the same cost as fk-507b17f8adc7 (plan as text, behaviour routed on substrings), now with a second consumer at a new anchor.

*Proposal:* Done by #9035: verify.sh (or the S18 typed plan) hands each step's kind to run-begin, and PHASE_RULES is deleted.

### R70 · fk-5d6c6f0f00a4 · low · confirmed · filed:8766
**factory-infra** `scripts/assert-module-scc.py::lex_crate` — h11 h10 kind:cross-module

Heuristic 11, SPOT: the locale the shared Rust lexer (scripts/lib_rust_production_view.sh, RUST_LEXER_AWK) must run under is part of its correctness, but the lib states no locale contract, and its two Python consumers decide it oppositely. audit-orphan-producers.sh's _run_lexer is 'Deliberately not pinned to LC_ALL=C', because byte mode splits a non-ASCII char literal and its quotes read as lifetimes. assert-module-scc.py's lex_crate pins LC_ALL=C. Measured with gawk on a probe file: under C.UTF-8, `['é','{']` lexes to `['','']`; under LC_ALL=C it lexes to `['é''{']`. The `{` survives into the production view, and so does a `}` after '—'. assert-module-scc.py runs matching_close and INLINE_MOD_RE over exactly that view. There is no live instance today (no non-ASCII char literal sits beside a brace literal in crates/). There are 40 non-ASCII char literals in crates/*/src, so the hazard is latent. Each Python consumer also writes its own awk driver and record protocol around the lexer: \x01/|/\x02 marks versus _FRAME_END frames.

*Proposal:* State the locale in lib_rust_production_view.sh's consumer contract (a UTF-8 locale) and drop assert-module-scc.py's LC_ALL=C pin. Better, give the lib one Python driver (path list in, (code_view, open_state) per file out) that both Python consumers and the incoming S9/S27/S1 ratchets import.

### R72 · fk-e9db0e38b420 · low · confirmed · filed:9195
**factory-infra** `scripts/land.sh` — h11 kind:cross-module

Heuristic 11, SPOT: the per-worktree core.hooksPath pin has a canonical home. scripts/lib_worktree_hooks_pin.sh defines WORKTREE_HOOKS_PIN=hooks and worktree_hooks_pin. scripts/setup-main-gate-worktree-config.sh is the writer, with the core.bare/core.worktree safety refusals and the extensions.worktreeConfig enable. scripts/hooks-armed-guard.sh `arm` is the pin plus a behavioural probe. scripts/land.sh, the only sanctioned manual landing path, re-spells both the value and the write: `git config --worktree core.hooksPath hooks`, failing if worktreeConfig is off. It bypasses the shared preconditions and the liveness probe at the exact point the gate must be live.

*Proposal:* Have land.sh call `scripts/hooks-armed-guard.sh arm "$ROOT"` (exit 0 required) instead of its own git config write.

### R69 · fk-9fb15d8eaa0a · low · weakened · open
**factory-infra** `scripts/affected-crates-lib.sh::direct_crates_per_path` — tests h6 kind:coverage

Tests stance: test_affected_crates_lib.sh reaches the lib through private names. It references `_reverse_closure` 19 times and occt-scope-lib.sh's `_reify_compile_closure` 15 times. The lib also grew a public function whose only consumer is a test: direct_crates_per_path. Its header says it is public 'so that check reads the precedence through the lib's interface and never through _routed_record'. Its route loop duplicates affected_crates'. That consumer is B13, a 4,956-row golden corpus (tests/infra/affected-crates-direct-sets.tsv). The corpus was generated at 1cdd3577c0 from the pre-theta bash rules as S22 theta's migration parity check. theta is done, and those rules are deleted. The corpus header's regenerate command is the implementation under test (`direct_crates_per_path <paths>`), so any intended routing change is checked only by regenerating from itself. The lasting value is the reviewable TSV diff, not a behavioural assertion.

*Proposal:* After S18 beta, retire B13's corpus and direct_crates_per_path. Pin routing with a small set of named boundary cases through affected_crates and reify_path_attribution.attribute, and give the closure a public entry the tests use, instead of _reverse_closure. #7766 (pending) would add more private reach into _reverse_closure.

### R78 · fk-19a30824e41b · low · confirmed · filed:9174
**gui** `gui/src-tauri/src/debug_server.rs::confined_write_target` — h10 kind:defect

The #7816 confinement of AI write targets is checked separately from the write, and the window between them is unbounded. confined_write_target resolves the target with no lock held. The write happens later inside the seam's run_on_engine closure, which first waits for the engine lock, and that wait can last a whole queued evaluation. For reify_save_file the write is crate::commands::save_file_impl (std::fs::write), which follows a symlink at the resolved path. For reify_export it is the kernel exporter, opening the path. So a symlink created at a not-yet-existing confined path, or a directory swapped for one, during that wait redirects the write outside the design directory. The sidecar's claude CLI can write inside the design directory (it is the landlock workspace), so the requester itself can create that symlink. docs/debug-mcp-contract.md states this residual ('a directory swapped for a symlink between them can redirect the write'), but no task owns it: search_tasks finds only #8471 (screenshot save_path), and briefing known_gaps does not list it. The invariant is therefore enforced at check time only, not at use (heuristic 10, redundantly), and the residual is documented but has no owner, against the umbrella principle 'nothing-vacuous-and-unowned'.

*Proposal:* Re-check at use: open the resolved target with O_NOFOLLOW (or write to a temp file in the confined directory and renameat into it), then fstat-compare against the design directory. Or file a task that owns the documented residual.

### R77 · fk-bf7b2f5e4aad · low · weakened · filed:9175
**gui** `gui/src-tauri/src/commands.rs::load_initial_file_impl` — comments h11 kind:convention

Follow-up work in this slice is recorded as curator-ticket ids (`tkt_…`) in production prose instead of `#NNNN` task cites. That breaks the briefing's TODO/citation convention and the Comments stance: a pointer the next agent cannot resolve by path does not count. There are six production sites: commands.rs:275 and :910, engine.rs:2955, :3121 and :5498, and large_stack.rs:20. Three more are in tests. commands.rs:906-912 states the evasion outright: the argv-launch residual of #5193 ('an argv-LAUNCHED GUI still paints stem-only files[].path') is 'Filed as a follow-up under ticket tkt_0RSMMJKNAYC6YCYNNNA86P9AYQ, not as a tracked-pattern comment, since the curator assigns the task id asynchronously'. A search_tasks paraphrase of that residual at 0.6 returns nothing, so from the code there is no way to tell whether the follow-up exists or is live, and the PTODO detector cannot see the marker.

*Proposal:* Resolve each ticket to its task id and rewrite the pointer as `#NNNN` (or delete it when the ticket was refused or combined). A reify-audit lane could flag `tkt_` in tracked source as an unresolvable cite.

### R80 · fk-fa60000e66da · low · confirmed · filed:9176
**gui** `gui/src-tauri/src/main.rs` — comments h1 kind:deep-read

main.rs doc prose has drifted against the code it sits on, a heuristic 1 / Comments-stance cost of the same class #8542 fixes in engine.rs, but outside that task's file:
- `/// Return the current kernel availability status.` sits on read_view_sidecar, so get_kernel_status, the command it describes, has no doc.
- cancel_solve's doc says 'The engine-side wiring that publishes the handle is a follow-on task', and AppState.pending_solve_cancel (commands.rs:35) says 'Published by the engine-side dispatch wiring (follow-on task)'. Both are false: PendingSolveCancelSink (#4086, done) is installed in setup(), and EngineSession::with_solve_slot calls solve_started/solve_finished.
- setup()'s fea-case emitter comment says 'The emitter is a no-op until task 3026 lands solve_load_cases'; #3026 is done.
- claude_send_message's event list omits claude-notice, claude-permission-request and claude-sidecar-crashed, which outbound_to_event and on_sidecar_exit emit.

*Proposal:* Move the kernel-status doc onto get_kernel_status, and delete the three stale 'follow-on'/'until 3026' clauses. If S26 ι #9070 moves the commands to tauri_commands.rs first, do this in that diff.

### R81 · fk-05c7548661ba · low · weakened · filed:9177
**gui** `gui/src-tauri/src/mcp_context.rs::TauriToolContext.get_eval_status` — inv-sf-5 h1 kind:stub

TauriToolContext::get_eval_status is a constant body, `EvalStatusInfo { phase: "idle", progress: None, dirty_count: 0 }`, which reports idle even while the EvalQueue is evaluating. That is the INV-SF-5 'body returns a value it knows is wrong' shape, with no #NNNN owner on it. It is unreachable from the frontend today: it is reached only through the mcp_tool_call Tauri command, which S26 §2.4 records has no frontend caller and S25 ο #8951 deletes. The S25 PRD §12 Q2 leaves its fate as an open tactical question ('ν may propose it in its task record'), not a leaf. The real value exists: EvalQueue::progress() / engine_activity::probe already answer it lock-free.

*Proposal:* Either delete reify_get_eval_status from the GUI host when ο removes mcp_tool_call, or back it with EvalQueue::progress().

### R79 · fk-65db0c1c9ef3 · low · weakened · open
**gui** `gui/src-tauri/src/diff.rs` — h13 inv-arch-1 h9 kind:cross-module

diff.rs is a re-export shim by its own account (heuristic 13). Lines 10-12 say `pub use crate::types::{StateDelta, delta_to_events, diff_gui_state}` exists 'so every existing crate::diff::{…} import path … is unchanged'. It also closes the reify-gui module cycle that INV-ARCH-1 carries as a baseline row (scripts/module-scc-baseline.json reify-gui: [diff, gui_state_schema, types]). The cycle runs: types invokes gui_state!, whose expansion calls `$crate::diff::push_serialized_event` (gui_state_schema.rs:328,367); diff imports crate::types::GuiState and re-exports types' generated items. push_serialized_event is macro support ('pub (bumped from pub(crate) for L5)'), not diffing, so it lives a layer above its only user. main.rs, eval_queue.rs, debug_server.rs and tests import the generated types through the shim path.

*Proposal:* Move push_serialized_event into gui_state_schema (beside diff_keyed/diff_whole). Import StateDelta/delta_to_events/diff_gui_state from crate::types, and keep diff.rs to advance_baseline/compute_delta over crate::types. Lower the baseline row in the same diff.

### R82 · fk-e701ecedf198 · low · confirmed · open
**gui** `gui/src-tauri/src/watcher.rs::FileWatcher` — tests comments h9 kind:coverage

Under the Tests stance, FileWatcher's tests reach its private state through two #[cfg(test)] pub(crate) hooks. pending_paths reads Debouncer.pending through the private `shared`. record_pending_for_test writes into it with a caller-chosen Instant, so a future stamp makes an entry 'structurally un-drainable'. The hooks exist because FileWatcher hard-wires both its clock (Instant::now() in the notify closure and the worker loop) and its event source (notify::RecommendedWatcher), although the inner Debouncer is already clock-injected. Per the hook's own 30-line docstring, this design caused 'four separate reported flakes in watcher_tests.rs' (#5143, #6438, #6462, all done). That docstring is reviewer-answering prose and flake history (Comments stance). watcher.rs is 47% comment lines, and its tests drive behaviour through the private seam rather than a public one.

*Proposal:* Give FileWatcher an injectable clock and event source (e.g. FileWatcher::with_source(events: Receiver<(PathBuf, ChangeKind)>, clock) beside the notify-backed new()). The drop/discard tests then drive the public seam, the two hooks are deleted, and the flake rationale moves to the commit history.

### R84 · fk-0819ab6ab79e · low · weakened · open
**gui** `gui/src/editor/Editor.tsx::Editor` — h6 h9 h5 h4 comments kind:deep-read

Heuristic 6 (well-defined purpose) and heuristic 9 (deep modules): the client half of the LSP document lifecycle has no module of its own. It lives as closure locals of the Editor component: docVersions, versionKey, currentUri, fileOpsPromise, lspDebounceTimer, destroyed, sendDidChange, flushPendingLspChange and onceServerIsCurrent. Those locals are mutated from onMount (lines 218-705, a 488-line closure), three createEffects, the keymap, the rename sinks and onCleanup. The same component also builds the rename overlay DOM, the three WorkspaceEdit disk/store/LSP sinks, the two-channel diagnostics merge, the save keymap, nav history, and the store→view content sync. 322 of 943 lines are comments, many explaining ordering between those effects (comments stance). The cost is measured: in the last 3 days #7510/#7512 landed 12 commits here, and they left three pending point-fix follow-ups in this same lifecycle: #8596 (rename re-issues at stale coordinates), #8603 (a didChange can precede initialize/didOpen because sendDidChange is not chained on fileOpsPromise), and #8604 (didClose sent for documents never opened). The URI-spelling defect recorded separately here is a fourth. Each follow-up has to reason about the whole closure because the ordering invariant (open→changes→close per URI, one version sequence) has no owner that enforces it.

*Proposal:* Extract an LSP document session (for example editor/lspDocumentSession.ts) that owns open/change/close ordering, per-document versions under one key function, and flush-before-request. Editor then calls open(uri,text), change(uri,text), close(uri), syncForRequest() and keeps only the view wiring. Move the rename overlay and the WorkspaceEdit sinks into their own modules beside rename.ts. #8603 and #8604 then become invariants of that module rather than separate patches.

### R85 · fk-9097c8a9fab7 · low · confirmed · filed:9204
**gui** `gui/src/editor/references.ts::previewForLocation` — kind:defect h1 comments

previewForLocation reads `view.state.doc.line(location.range.start.line + 1)` from the active EditorView for every Location, whatever location.uri says. Because references are cross-file since #4210, a reference in an imported file shows whichever line of the active document has the same index. The Find-uses panel therefore shows fabricated preview text for every cross-file row. The function's docblock ('the source line a reference sits on') names behaviour the code does not have for those rows.

*Proposal:* Preview only Locations whose uri is the active document (under the editor's one identity key) and leave the others undefined, or ask the backend for the preview text.

### R83 · fk-7cc1c482ece2 · low · confirmed · filed:9205
**gui** `gui/src/bridge.ts::claudeClearSession` — inv-nothing-vacuous-and-unowned kind:dead-code

The clear-session chain has no production entry point. bridge.claudeClearSession and claudeStore.clearSession have no caller outside tests: no panel, menu, shortcut or command-palette entry invokes them. The Rust claude_clear_session command and the sidecar's SidecarSession.handleClearSession, which resets --resume continuity, are therefore unreachable from the UI. A user cannot start a fresh conversation without restarting the app. Possibly dead, or possibly a missing affordance; verify intent before removing.

*Proposal:* Add the chat-panel affordance (claudeStore.clearSession + claudeClearSession), or delete the chain end to end.

### R86 · fk-f24777406f66 · low · confirmed · filed:9206
**gui** `gui/src/types.ts::MultiCaseResult` — inv-sf-5 h1 comments kind:stub

INV-SF-5 `placeholders-owned-and-loud`: types.ts exports a placeholder interface MultiCaseResult { cases: Record<string, unknown> }. Its docblock calls it 'a deliberate placeholder' to be narrowed 'when 3026 lands'. Task 3026 (GUI case-picker for MultiCaseResult) is done, and the type has zero importers in gui/: every other mention is in a comment. The placeholder's owner is terminal, its promise is a prose task-promise (the #7098 convention-only surface), and the exported name implies a wire type that nothing produces or consumes.

*Proposal:* Delete the interface (lambda #9072 removes the types.ts hand mirrors and moves TS-only shapes beside their consumers, and this one has none).

### R88 · fk-a8df59509cdb · low · confirmed · filed:9191
**kinematics-and-motion** `crates/reify-stdlib/src/flexures/prismatic.rs::SMALL_DEFLECTION_FRACTION` — h11 comments

The flexure submodules restate shared facts instead of importing them from flexures/common.rs (heuristic 11, SPOT). prismatic.rs redeclares SMALL_DEFLECTION_FRACTION = 0.1 with a near-identical doc, although common::SMALL_DEFLECTION_FRACTION is pub(super) (prismatic.rs:41 vs common.rs:31). neutral_length_si is copied byte-for-byte in beam.rs:253 and prismatic.rs:138. diagnostics.rs hard-codes PRB_ANGLE_LIMIT_DEG = 5.0 beside the imported PRB_ANGLE_LIMIT_RAD = 5°, instead of deriving one from the other (diagnostics.rs:27 vs common.rs:14). The 13 constructor names live in three runtime lists: diagnostics::PRB_CTOR_NAMES, the classify_geometry_invalid match, and the per-module eval_* dispatch arms. A fourth list in the compiler, flexure_signatures::FLEXURE_CTOR_FN_NAMES, is kept in step only by a fixture test. Both neutral_*_si readers also turn a present-but-unreadable neutral argument into 0.0 (common.rs:212-219; beam.rs:253-259), a default-on-error pattern the briefing's 'errors must be noisy' convention prohibits.

*Proposal:* Import SMALL_DEFLECTION_FRACTION and one neutral_length_si from common.rs, and derive PRB_ANGLE_LIMIT_DEG from PRB_ANGLE_LIMIT_RAD. Have neutral_*_si return None on an unreadable present argument so the constructor rejects it.

### R91 · fk-abc8e797fab9 · low · weakened · filed:8546
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/simulate.rs` — comments h5 h6

The cost review-all recorded for tots.rs (fk-d49d3ac15f14, #8546) also applies to five sibling trajectory modules, which #8546 does not cover: module-wide #![allow(dead_code)] justified by prose that is now false (Comments stance). gcode_import.rs:39, sampling.rs:24, simulate.rs:44, spline.rs:14 and impulse_shaper.rs:25 each say the API is 'tested ahead of the θ/π consumer that will wire it'. Those consumers (simulate_trajectory_value, input_shape_value, eval_gcode_import) have been wired for months. In gcode_import.rs the allow hides exactly the never-read warning variants (finding above). In sampling.rs it hides MotionTrajectory::resample_cubic, which has only test callers. simulate.rs:19 carries a `// ptodo:allow` asserting that trajectory.ri's accessor bodies are 'currently stub TODO(θ) bodies'. That is false: they delegate to the real *_at intrinsics (trajectory.ri:448-473). trampoline.rs puts item-level #[allow(dead_code)] on four production-called functions (flatten_shape, body_mass, placeholder_link, track_data_to_value; :368/:433/:453/:548). Seven same-file-only pub(crate) helpers carry '// G-allow: same-file caller only' (e.g. :198, :393, :486).

*Proposal:* Widen #8546 to all six trajectory modules. Drop the module-wide allows and the false headers, make same-file helpers private, and let rustc report what is genuinely dead.

### R90 · fk-2fbadf54af70 · low · confirmed · filed:8546
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/sampling.rs::MotionTrajectory.resample_cubic` — h6 kind:dead-code

MotionTrajectory::resample_cubic has no production caller: all five references are in sampling.rs's own test module (:357-460). Its G-allow comment claims it is a 'permanent internal helper called only within sampling.rs' wired through simulate_trajectory_value, but simulate_trajectory_core uses only to_trajectory_samples, times() and the samples. The module's #![allow(dead_code)] keeps rustc from flagging it. Possibly dead: verify before removing. It may be meant as the γ round-trip test oracle, in which case it belongs under #[cfg(test)].

*Proposal:* Move it into the test module, or delete it with the G-allow line.

### R89 · fk-2f1e2de126f0 · low · confirmed · filed:9192
**kinematics-and-motion** `crates/reify-stdlib/src/trajectory/gcode_import.rs::lower_commands` — kind:defect h12

lower_commands treats M82/M83 (absolute/relative extrusion) as segment splitters only, and always reads E as an absolute coordinate. After an M83, which is common in slicer output, each waypoint's `e` is the per-move delta mislabelled as the absolute extruder position. waypoint_to_value then publishes it as a Length 'absolute coordinate' (gcode_import.rs:114-118, 466-480). The AST already distinguishes the mode: IgnoredMCode keeps code 82/83. The lowering discards that fact, the waypoint record has no field saying which regime `e` is in, and nothing warns.

*Proposal:* Track the extrusion mode in PosState: accumulate E under M83 and treat it as absolute under M82. Alternatively, raise a coded warning when M83 appears.

### R87 · fk-f6c709836998 · low · confirmed · filed:9193
**kinematics-and-motion** `crates/reify-gcode/src/marlin.rs::parse_value` — kind:defect h10

parse_value accepts any string Rust's f64::from_str accepts, which includes 'inf', 'infinity' and 'NaN' (case-insensitive). So `G1 XNaN Yinf` parses successfully, although the grammar the crate documents is `<letter><number>`. The non-finite value then flows through lower_gcode into waypoint_to_value as a NaN or ±inf Length nested inside a List of Maps. reify-stdlib's sanitize_value checks only the top-level Value (helpers.rs:83-95), so it never cleans it. The crate's round-trip contract (lib.rs:18-22) also fails for NaN, because NaN != NaN under the derived PartialEq. display.rs's rule 3 already scopes itself to 'every finite x'. No test covers non-finite input.

*Proposal:* Reject non-finite values in parse_value as InvalidParameter (`parsed.is_finite()`), and add a parse_errors test.

### R92 · fk-d2541e0026dd · low · weakened · filed:9187
**language-pipeline** `crates/reify-compiler/src/annotations/schema.rs::AnnotationSchema` — h1 h11 comments kind:deep-read

Heuristic 1, informative names (a declaration that lies), with heuristic 11. Every SCHEMAS entry declares `on_extra: ExtraArgsPolicy::WarnIgnore`, and ArgSchema entries declare `required: true`. Neither field, nor `flag_set`, is read anywhere in the workspace. ExtraArgsPolicy::Error is never constructed. The real arg-shape and severity policy lives in the per-annotation `arg_check` fn pointers. A reader of the declarative table is therefore told a policy the code does not enforce, and the in-file comments keep two homes in sync by hand ('matching on_extra: WarnIgnore'). Five `#[allow(dead_code)]` attributes suppress the compiler's own report of this. Each cites 'later phases (δ, ζ, ι)' of docs/prds/annotation-args.md, Greek PRD-relative aliases the TODO convention rejects as malformed cites, and phase δ (#3555) and ε (#3556) are done. The allow on EvalTime/ArgType/ArgSchema is also stale: materialize.rs now reads eval_time, positional_index and ty.

*Proposal:* Delete the unread fields and the never-constructed variant, or make validate_via_schema enforce them (and drop the per-helper duplication). Remove the stale #[allow(dead_code)]s, and cite a live #NNNN for any phase still owed.

### R93 · fk-400e69bbca2c · low · weakened · open
**language-pipeline** `crates/reify-compiler/src/lib.rs` — tests h9 kind:coverage

Tests stance: test access to a module's internals is an interface-design smell. reify-compiler's crate root carries four `#[cfg(any(test, feature = "test-support"))] #[doc(hidden)] pub fn __*_for_test` shims. They exist only so integration tests can call private functions directly: annotations::validate_annotations, type_compat::infer_mul_div_result, builtin_registry::registry_result_type and builtin_registry::registry_owns. Each G-allow cites a DONE task (#3530, #5063, #6001) as its reason. The same file widens eight builtin-name families from pub(crate) to `pub use units::{...}` so reify-eval's registry_drift_tests can read them (lib.rs comment). reify-syntax repeats the pattern with `#[doc(hidden)] pub fn visit_structure_member_root_exprs`, which its doc calls 'a test-support helper and not part of the parser's production API'. The parity and registry-seam properties these tests guard are therefore pinned at private functions, not at compile()'s observable output.

*Proposal:* Drive the parity and seam tests through compile_source on .ri fixtures (static result type is observable in CompiledModule), or give the registry seam a narrow public query surface that production also uses. Then delete the __ shims and the test-only widening.

### R95 · fk-e51ded73acb1 · low · weakened · filed:9208
**language-pipeline** `crates/reify-expr/src/calculus.rs::validate_differentiable_field` — h10 inv-SF-1 kind:deep-read

reify-expr reports an op-contract failure through two unrelated mechanisms, which breaks the 'uniformly' clause of heuristic 10 and INV-SF-1 undef-has-provenance. lib.rs has `push_op_contract_failure(ctx, DiagnosticCode)` plus UndefCause recording (13 sites). calculus.rs (13 sites) and analysis.rs (5 sites) instead return `None`/`Value::Undef` after a `#[cfg(debug_assertions)] eprintln!("[reify-expr] …")`. lib.rs adds 3 more of these (match on a non-enum, sample() on a non-Field or non-Lambda slot). Those 21 sites record no UndefCause and push nothing to the EvalContext runtime-diagnostics sink. In release builds they are completely silent. In debug builds they write to stderr, which neither the GUI nor the CLI diagnostic stream carries. The only remaining signal is the generic W_UNDEF_UNEXPLAINED backstop. calculus.rs and analysis.rs contain zero push_op_contract_failure or UndefCause sites.

*Proposal:* Route each site through push_op_contract_failure with a DiagnosticCode (or record an UndefCause via the γ sink) and delete the debug eprintln channel.

### R98 · fk-2b679f63a55a · low · weakened · filed:9210
**language-pipeline** `crates/reify-ir/src/arg_acceptance.rs` — h1 comments kind:deep-read

arg_acceptance.rs, the S15-owned single home of the dimension-acceptance rule, has three docs that lie (heuristic 1). (1) `pressure_spec`'s doc block (lines 299-307) has no item after it. #6924 (875c0b1c62) inserted `angle_spec` there, so rustdoc renders angle_spec's doc opening with 'Returns the ArgSpec for a PRESSURE-semantic reader position', and `pressure_spec` (line 345) has no doc. (2) The Leg-B banner says every constructor in the section sets `migration_hint: None`, but angle_spec, now inside that section, sets `Some(ANGLE_MIGRATION_HINT)`. (3) `ArgRejection` and `ArgRejection::message` say they format a `Severity::Warning` diagnostic, but geometry_ops emits every DimensionedArgRejected as `Diagnostic::error` (geometry_ops.rs:427, :714; table at :310).

*Proposal:* Move the pressure doc onto pressure_spec, move angle_spec out of the Leg-B section, and correct the severity wording.

### R100 · fk-60ca11154f6b · low · confirmed · filed:9211
**language-pipeline** `crates/reify-runtime/src/priority_promotion.rs::PriorityPromoter` — h6 kind:dead-code

PriorityPromoter (promote_for_demand, effective_priority, batch_remove) has had no production caller since #5065 deleted the concurrent scheduler, and it carries no G-allow marker. The same reasoning made commitment.rs's CommitmentTracker / check_commitment carry `// G-allow: no production caller pending #5023`. reify-runtime's only production consumer is reify-cli dev.rs::render_inspection, which uses traits_to_priority, Priority, NodePolicyOverrides and NodeCommitmentOverride for display only. reify-eval depends on reify-runtime only as a dev-dependency. Possibly dead — verify before removing.

*Proposal:* Add `// G-allow: … pending #5023` alongside commitment.rs's, or delete the module with its tests.

### R99 · fk-ec1f6ad4538f · low · confirmed · filed:9210
**language-pipeline** `crates/reify-ir/src/arg_acceptance.rs::mass_spec` — inv-SF-5 kind:dead-code

Ten of the eleven PRD-5 Leg-B dimensioned spec constructors that #5791 added on 2026-09-01 still have no consumer six weeks later, and carry no G-allow marker as the producer convention requires: pressure_spec, mass_spec, frequency_spec, time_spec, velocity_spec, acceleration_spec, force_density_spec, moment_of_inertia_spec, translational_stiffness_spec, rotational_stiffness_spec. Only force_spec has a caller (reify-stdlib helpers.rs:2161). The consumers are named and live: dimension-checked-readers leaves #6922 / #6941 / #5802 and S15's typed decoders. So this is vacuous-with-an-owner whose ownership is not recorded at the producer, rather than abandoned scaffold. Possibly dead — verify before removing.

*Proposal:* Add one G-allow line per constructor citing #6922/#6941/#5802, or have S15's decoders adopt them.

### R96 · fk-819257c68b82 · low · weakened · open
**language-pipeline** `crates/reify-expr/src/dual_eval.rs::builtin_partials` — h11 h12 kind:cross-module

dual_eval.rs keeps three more hand-written (name, arity) string tables of builtin semantics: is_differentiable_builtin (13 smooth builtins), builtin_partials (their derivatives) and is_kink_builtin (non-smooth min/max/abs/sign/floor/ceil/round/clamp/mod and reductions). They are separate from the reify-builtins registry, reify-stdlib's evaluators (e.g. trig.rs) and reify-compiler math_signatures.rs (heuristic 11 SPOT; heuristic 12 routing on name strings). Renaming a builtin or changing its arity in the registry or stdlib leaves the dual path silently treating it as non-differentiable or smooth, and nothing fails at compile time. S8's classify_call covers only the interception half of dual_eval's mirroring; these tables are a separate dimension it does not name.

*Proposal:* Carry differentiability (smooth / kink / none) as a reify-builtins row column that dual_eval reads, and keep the derivative closures keyed by BuiltinId.

### R97 · fk-168176511f09 · low · confirmed · filed:9212
**language-pipeline** `crates/reify-expr/src/lib.rs::EvalContext._test_at_depth` — tests h9 kind:coverage

EvalContext exposes `#[doc(hidden)] pub fn _test_at_depth(values, depth)`, a constructor that exists only so an integration test can preset the private recursion_depth (Tests stance: test access to internals is an interface smell). Its only callers are lambda_eval_tests.rs:710/719, which also hard-code 256 = MAX_RECURSION_DEPTH. The recursion-limit behaviour could instead be driven through the public seam with a genuinely deep lambda nest. S4 τ's closed constructor set (#8872) does not list this constructor.

*Proposal:* Drive the depth limit through a real nested-lambda fixture and delete the hidden constructor, inside #8872's constructor closure.

### R94 · fk-ba1aa6d8a3b3 · low · confirmed · filed:9218
**language-pipeline** `crates/reify-eval/src/engine_admin.rs::Engine.on_refine_trigger` — h10 comments

Engine::on_refine_trigger and Engine::invalidate_morph_source are `pub fn` with no production caller; only morph_producer.rs unit tests call them. Neither carries the `// G-allow:` marker the briefing convention requires for a public producer without a production consumer. Their dormancy is instead stated in rustdoc prose, under the 'Wiring status' and 'no such call site exists yet' sections, which name no task id. Heuristic 10 (uniformly enforced): the orphan-producer invariant has one marker mechanism that scripts/audit-orphan-producers.sh and reify-audit P1 read, and this site bypasses it. The G-tool orphan report flags on_refine_trigger as new versus its baseline. The consumer is filed: #8298 (pending) wires the GUI to call it at settled moments.

*Proposal:* Add `// G-allow: consumer pending #8298 (GUI settled-moment refine trigger)` above both fns, or fold that into #8298's scope so the marker goes when the call site lands.

### R101 · fk-e56835df6288 · low · weakened · open
**reify-audit** `crates/reify-audit/src/bin/reify-audit.rs::load_tasks_from_fused_memory` — h11 h1 h10

Two implementations map a fused-memory task record to TaskMetadata (heuristic 11, SPOT), and the production callers use the one the bin labels test-only (heuristic 1).
- The Rust live loader: load_tasks_from_fused_memory → FusedMemoryClient::get_task/get_tasks → task_metadata_from_wire.
- The jq sidecar scripts/reify-audit-snapshot-filter.jq, fed through `--tasks-file`.
- Both production callers use the jq path: the /audit skill and the dark-factory pre-done hook wrapper. The bin's usage calls `--tasks-file` 'for tests' and the live loader 'production'.

The two adapters disagree:
- **Subtasks:** the Rust loader requests `with_subtasks: true` and flattens subtasks (collect_tasks_recursive). The wrapper and skill call get_tasks without it, and jq maps only top-level `.tasks`, so subtasks are never audited and a pre-done flip of a subtask finds no row (check_pre_done returns [] and the flip is allowed).
- **Per-row tolerance:** Rust defaults a missing title to "" and drops a malformed done_provenance per row. In the jq path TaskMetadata.title is a non-defaulted String and serde decodes the whole file, so one null title or malformed row fails the snapshot. The wrapper then exits 125 and every done-flip in the project is blocked.
- **done_at:** jq prefers .metadata.done_at; the Rust loader reads only updatedAt.
- **Pre-done cost:** the Rust pre-done path is a deliberate O(1) get_task. The wrapper pulls the whole corpus with get_tasks inside fused-memory's write lock.

The wrapper's header premise, 'reify-audit is a pure-logic library (no MCP client...)', is false: the bin has had a live client since #3736.

*Proposal:* Pick one adapter. Either the wrapper and skill call the bin's live loader (drop --tasks-file from production, keeping it for fixtures), or the Rust loader is deleted and the jq contract is made total per row. Whichever survives owns subtask flattening, per-row tolerance and the done_at tiers.

### R104 · fk-3dc9e2858c24 · low · weakened · open
**reify-audit** `crates/reify-audit/src/ptodo.rs::tasks_db_path` — h11 h7 kind:convention

reify-audit reads the task store directly. This conflicts with the project invariant, and it gives one binary two sources for one fact, task status (heuristic 11, SPOT; heuristic 7, stateless module interactions).
- PTODO's liveness/inverse/G-allow lanes and PDCHECK open `.taskmaster/tasks/tasks.db` with rusqlite (READ_ONLY) through ptodo::tasks_db_path/open_tasks_db.
- CLAUDE.md §Memory & tasks says direct reads 'are also wrong while fused-memory is up: it holds its own write queue and cache, so even "just a quick read" races the pipeline and can return stale rows'.
- The same run loads the task corpus from fused-memory for P1, P2, P5 and PPRDSTATUS (modes.md: PPRDSTATUS reads the corpus, 'never tasks.db').
- The PTODO PRD's premise for accepting direct reads (§13 Q2: 'the DB write is synchronous via fused-memory') contradicts the current CLAUDE.md statement. The briefing records the read-only sqlite choice as a key decision without that reconciliation.
- S24 ε #8811 (TasksDb) and ξ #8821 (PREGISTRY's liveness lane) extend the direct-read surface.
- #8780 adds a no-direct-tasks.db lint for scripts/ only.

*Proposal:* Decide deliberately, as a human ruling recorded in the PTODO PRD and briefing. Either reify-audit's DB lanes read through fused-memory (get_statuses, or a query shape added for cite liveness), or CLAUDE.md names reify-audit's read-only, degradation-reporting lanes as a sanctioned exception, with the staleness bound stated. Then make S24 ε's TasksDb follow that ruling.

### R105 · fk-954243388ee9 · low · confirmed · open
**reify-audit** `crates/reify-audit/src/ratchet_ledger/regenerator.rs::Regenerator` — h11 h1

The four committed-baseline generators follow two protocols (heuristic 11, SPOT), and the shared module's doc names a sharing that does not exist (heuristic 1).
- ratchet_ledger says it is 'What every committed-ratchet ledger shares', and regenerator calls itself 'The driver every ratchet ledger's `*-baseline-gen` binary shares'.
- Only pdoccover-baseline-gen and plinecite-baseline-gen use the driver.
- ptodo-baseline-gen and pdiag-baseline-gen hand-roll argv. They accept a bare positional project root, which the driver deliberately refuses ('a stray positional is refused ... never silently taken as a different tree'). They emit only to stdout, with a documented `> baseline.txt` redirect recipe, and each has its own exit-code contract. ptodo-baseline-gen's `--help` exits 0 under that redirect, the hazard pdiag-baseline-gen and the driver both document guarding against.
- Committed-ledger reads differ too: ratchet_ledger::committed_rows reads only a git-tracked ledger ('an untracked copy is inert'), while pdiag::check reads pdiag-baseline.txt from the working tree whether tracked or not.
- Pending task #8925 ('Move pdoccover-baseline-gen onto the shared driver') is already satisfied on main by #6935's amend 651128f46b, so it is stale.

*Proposal:* Either move PDIAG's count manifest and PTODO's fingerprint set onto Regenerator/committed_rows, giving Ledger<R> or the driver a count-manifest variant, or narrow the module docs to name the lanes the driver serves. Cancel #8925 as already delivered.

### R103 · fk-f8d63189bc4e · low · confirmed · filed:9200
**reify-audit** `crates/reify-audit/src/plinecite.rs::stale_baseline_entry` — h11 h1

The finding text gives a regeneration command that does nothing (heuristic 11, the regeneration command has several homes and they have drifted).
- PLINECITE's and PDOCCOVER's `stale-baseline-entry` remedy says 'regenerate with `cargo run -p reify-audit --bin plinecite-baseline-gen`' (pdoccover: `pdoccover-baseline-gen`), with no `-- --project-root . --write`.
- Under the shared driver, a run without --write only prints the ledger to stdout and changes nothing.
- The ledger header, the /audit skill, the infra test and the generator docs all give the `--write` form.
- An agent following the finding verbatim gets a no-op. One that adds the 'obvious' redirect is refused (exit 3) by the driver's truncation guard.

*Proposal:* Render the command once, from a const beside BASELINE_PATH or a Regenerator-owned string, as the full `--project-root . --write` form, and use it in both detectors' remedy text and the ledger header.

### R102 · fk-d8cb7c46751e · low · confirmed · filed:9201
**reify-audit** `crates/reify-audit/src/bin/reify-audit.rs::pattern_selects` — h12

--pattern is validated token by token at parse time, then stored as the raw comma-separated String in Args.pattern (heuristic 12, structured data instead of meaningful strings).
- Every consumer re-parses it through pattern_selects (`pattern.split(',').map(str::trim)`): Detector::selected_by, reached via selected_detectors from needs_jcodemunch, jcodemunch_only_run_set, task_corpus_only_run_set, run_set_reads_tasks_db, seam_unavailable_breadcrumb and the dispatch loop.
- It is an internal value re-parsed at each use rather than parsed once at the boundary into a typed set of DETECTORS rows or pattern_flag tokens.

*Proposal:* Parse --pattern into the selected Detector rows (or a set of pattern_flag tokens) once in parse_args, and let every predicate iterate that set.

### R108 · fk-4cf4e3e2c407 · low · weakened · open
**tooling** `crates/reify-cli/src/main.rs::cmd_test` — inv-sf-2 h10 tests

Under INV-SF-2 (error-severity-exits-nonzero) and heuristic 10 (invariants informatively enforced), `reify test` throws away every diagnostic its test runs produce. reify_eval::run_tests returns TestResult { status, diagnostics, constraint_results }, filling diagnostics from engine.check's CheckResult. cmd_test reads only result.status and result.name. It never prints result.diagnostics or the per-entry indeterminate_reason that ConstraintCheckEntry has carried since e12e63c2ea. It exits FAILURE only when failed > 0. So an Error-severity diagnostic raised while checking a @test template, such as a kernel or eval failure, is never shown, and the test reads INDETERMINATE with exit 0 and no reason. That defeats the briefing scenario's 'tell a verdict from a shrug'. cmd_check, cmd_build, cmd_eval and cmd_report all print eval diagnostics to stderr; cmd_test is the only engine-running command that prints none. crates/reify-cli/tests/harness_cli/cli_test.rs asserts exit codes and summary lines only; no case checks stderr for an eval diagnostic. Pending #6694 (P1-zeta) names this drop in its description, but its files_to_modify lists only crates/reify-eval/src/test_runner.rs, and the drop is in main.rs::cmd_test. Its premise that build_test_engine registers 'no compute trampolines' is also stale: build_test_engine now calls register_production_compute_fns (MorphRegistration::Unavailable). Pending #6793 owns the Indeterminate exit ruling but not the diagnostics output.

*Proposal:* Have cmd_test print each TestResult's diagnostics and indeterminate reasons, using the same report_eval_output ordering as check/build. Fold that, plus whether an Error-severity diagnostic gates the exit, into #6793/#6694, and widen #6694's files to main.rs.

### R107 · fk-450af715ff4c · low · confirmed · filed:8775
**tooling** `crates/reify-cli/src/main.rs::cmd_report` — h1 comments

Under heuristic 1 (informative names; a doc that lies), cmd_eval's 37-line rustdoc is attached to cmd_report. It begins '`reify eval <file>` — parse, compile, evaluate, and print every top-level value cell', and covers geometry modules, OCCT-absent degradation and Error propagation. Commit 2b9a48f9b2 (task 4292, 2026-06-26) inserted cmd_report's own doc and fn between that block and the configured_eval_engine function it followed. The two doc blocks now run together, so cmd_report's rendered doc opens by describing `reify eval`, and cmd_eval at its own definition has no doc at all. Pending #8775 (υ) moves both functions move-only into cmd/<name>.rs, which would carry the eval prose into cmd/report.rs.

*Proposal:* Move the eval doc block back onto cmd_eval. Do it in #8775's diff or before it.

### R113 · fk-dc9c467ba3d2 · low · confirmed · filed:7398
**tooling** `crates/reify-lsp/src/completion.rs::LENGTH_GATED_EXAMPLES` — h10 comments

This breaks the TODO citation convention (cited ≠ tracked). LENGTH_GATED_EXAMPLES' doc carries `TODO(#5752): once ι lands, re-check this table against its allowlist`, and the test length_gated_tables_stay_in_sync's doc refers to 'the TODO(#5752) residual'. Task #5752 (units-length ι closure guard) has been done since 2026-09-20; it was merged as 646320f1af. The cite is therefore orphaned, and the re-check it asks for was never recorded as done. The same doc also cites `crates/reify-eval/src/arg_acceptance.rs`, a module that moved to crates/reify-ir (task 5791). That is the stale-citation class pending #8225 sweeps outside reify-compiler.

*Proposal:* Re-check the table against units_length_closure_guard.rs's ALLOWLIST. Then either delete the TODO or re-cite a live task, and repoint the arg_acceptance path.

### R110 · fk-adb958128dd9 · low · confirmed · open
**tooling** `crates/reify-doc/src/fmt_markdown.rs::render_params_table` — h1 h11 comments

Under heuristic 1 and the briefing key_decision that doc-build is 'faithful reference output', both reify-doc formatters render a column whose header names a fact the model never carries. The Parameters table is headed 'Name | Type | Dimension | Default | Description', and every Dimension cell is a hard-coded em-dash in both fmt_markdown::render_params_table and fmt_html::render_params_table. ParamDoc has no dimension field: it has name, doc, type_repr, default_repr and annotations. Likewise every Ports table's Kind cell is a hard-coded em-dash in both formatters, because PortDoc carries only name, direction, type_name and members. These placeholder bodies have been in production since the formatters landed (a42d7c5750, task 2359, 2026-04-27). No task or PTODO cite owns them. fmt_html_tests and fmt_markdown_tests pin the header text (`<th>Dimension</th>`, the GFM header row), so the empty column is locked in. The same choice is made twice, and the HTML copy is commented 'mirroring markdown'.

*Proposal:* Either drop the two columns, or add the dimension (derivable from type_repr's resolved type) and port kind to the doc model in reify-doc-build. Decide once for both formatters.

### R109 · fk-92dcfef08257 · low · confirmed · open
**tooling** `crates/reify-doc/src/fmt_html.rs::CrossRefIndex` — h3 h11 h1

Under heuristic 3 (orthogonal dimensions) and heuristic 11, reify-doc varies along two axes, output format and item kind, but only the format axis has a mechanism. The format-independent decisions are written out once per formatter. CrossRefIndex (the conformer→traits inverse of CrossRefs) is a verbatim copy in fmt_html.rs and fmt_markdown.rs; the HTML doc says 'Mirrors fmt_markdown::CrossRefIndex'. find_annotation is a verbatim copy. The params and ports column schema, including the placeholder columns, is chosen twice. Each formatter has its own per-kind render_*_body family: field, purpose, unit, type_alias, constraint_def, trait_members, enum_variants and function_signature exist in both. util.rs, the crate's own 'helpers shared across the HTML and Markdown formatters' module, holds only unquote and join_type_params. So a new ItemDoc kind or column needs lockstep edits in both files. Separately, the crate root doc in reify-doc/src/lib.rs says 'pure data, no formatters yet', but the crate declares fmt_html, fmt_json and fmt_markdown.

*Proposal:* Lift CrossRefIndex, annotation lookup and the table column schema into util.rs (or a format-neutral view model). Have each formatter keep only its markup emitters, and correct the crate doc.

### R112 · fk-d5dfe97a5613 · low · weakened · open
**tooling** `crates/reify-lsp/src/completion.rs` — h14 h13

Under heuristic 14, the review-all record's h14 measurement for completion.rs is `stays_whole: false`. The file is 3132 raw lines (prose_share 0.19). The record names a 4-unit partition: context detection; completion assembly; the BuiltinFunctionInfo/BUILTIN_FUNCTIONS/TYPE_NAMES/COMPLEX_METHODS/LENGTH_GATED_EXAMPLES data table of about 600 production lines; and the keyword tables, which it marks 'misplaced' because references.rs imports them. Unlike the other four stays_whole:false files in this slice (main.rs, analysis.rs, references.rs, diagnostics.rs), no S27 partition leaf was filed for it. tooling-front-end.md §1 lists only those four, and its §11 mentions completion.rs only for the KEYWORDS dependency. Two pending tasks reshape most of the units: #6012 replaces BUILTIN_FUNCTIONS with registry-generated rows, and #8910 moves the reserved keyword table to reify-syntax. What remains unowned is the context-detection and assembly split, plus the decision whether the file stays whole once those land.

*Proposal:* Record the completion.rs disposition as a note on #6012 or #8910, saying whether it stays whole after they land or gets a ψ-style move-only leaf.

### R114 · fk-c833e442d9e3 · low · confirmed · filed:9178
**tooling** `crates/reify-lsp/src/decl_oracle.rs::test_fixtures` — h13 tests

Under heuristic 13 (files make sense in isolation) and the Tests stance, the crate-wide test-fixture file crates/reify-lsp/src/test_fixtures.rs is a child of a production sibling. decl_oracle.rs mounts it with `#[cfg(test)] #[path = "test_fixtures.rs"] pub(crate) mod test_fixtures;`, and lib.rs has no mod line for it. Its consumers therefore import a URL helper and declaration tables through an unrelated production module: server.rs, diagnostics.rs, symbols.rs, goto_def.rs and references.rs tests all write `crate::decl_oracle::test_fixtures::...`. A reader of lib.rs cannot find the file, and a reader of test_fixtures.rs learns its own path only from prose ('Declared from decl_oracle.rs rather than the crate root'). Both landed in #8756 χ (done) and amend 79f4aadd40.

*Proposal:* Declare `#[cfg(test)] mod test_fixtures;` in lib.rs and import it as crate::test_fixtures. #8757 ψ, which already lists test_fixtures.rs and decl_oracle.rs, can carry this.

### R111 · fk-578105988fe2 · low · confirmed · filed:8772
**tooling** `crates/reify-lsp/src/analysis.rs` — h13 h9

Under heuristic 13 (re-export shims) and heuristic 9 (layering), analysis.rs carries `pub use reify_ast::{MemberSpanInfo, find_named_member_span};`. Its only in-crate user, goto_def.rs, imports `crate::analysis::find_named_member_span` through it. goto_def needs only a parse: its own comment says 'Only declaration spans are needed, so no compile/check runs'. Yet it imports from analysis, the module that owns AnalysisContext and its compile_with_stdlib_checked + Engine::check pipeline, when the function lives in reify_ast. After χ (#8756) moved every other syntax-only oracle into decl_oracle, this re-export is the remaining edge from goto_def onto analysis.

*Proposal:* Import find_named_member_span from reify_ast in goto_def.rs and drop the pub use. #8772 η edits both files.

### R106 · fk-a8038b74107f · low · weakened · filed:9179
**tooling** `crates/reify-cli/src/main.rs::cmd_gui` — tests h6

Under the Tests stance, production cmd_gui carries two env-var test seams that only the test harness sets. REIFY_GUI_SKIP_LAUNCH makes the command print 'could not launch reify-gui (launch skipped via REIFY_GUI_SKIP_LAUNCH)' and exit FAILURE. REIFY_GUI_DEBUG_PROBE additionally prints `REIFY_GUI_DEBUG_PROBE: debug=<bool>`. Both are read only in main.rs, and their only setter is crates/reify-cli/tests/harness_cli_surface/cli_gui.rs (20 references). The behaviour they let the tests observe, flag parsing and the REIFY_DEBUG env on the child command, already has a pure in-process seam in build_gui_command(gui_path, file, debug) -> Command. So the shipping binary's control flow carries a branch whose purpose is test observation.

*Proposal:* Split argument parsing from the launch into a parse_gui_args -> (file, debug) function. Test it and build_gui_command in-process, and drop the env hooks.

