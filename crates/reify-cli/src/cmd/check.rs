//! `reify check <file>`: the static constraint checker.

use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;

use crate::args::{CHECK_USAGE, PurposeActivation, build_cfg_set, parse_purpose_flag};
use crate::check_verdict::{
    check_gating_error, dedup_diagnostics, drop_falsified_indeterminate_diagnostics,
    merge_build_diagnostics, merge_post_build_verdicts, strip_diagnostics_reproduced_by,
};
use crate::constraint_report::{finish_check, report_achieved_deviations, report_eval_output};
use crate::module_capabilities::{
    module_has_dfm_rule, module_has_geometric_conforms, module_has_geometry,
    module_has_representation_within, module_has_thickness_dfm_rule,
};
use crate::parse_and_compile_with_cfg;

/// `reify check <file>` — lightweight static constraint checker.
///
/// `--verbose` adds a full-precision achieved-deviation readout to stdout
/// ([`report_achieved_deviations`]) and changes nothing else.
///
/// ## Engine posture: deliberately NO compute trampolines
///
/// The non-[`RepresentationWithin`] path uses `Engine::new(None) + check()`;
/// the [`RepresentationWithin`] path uses `Engine::with_registered_kernel +
/// check()`.  Neither path calls
/// [`configured_eval_engine`](crate::configured_eval_engine) nor registers the
/// FEA/buckling/modal compute trampolines
/// ([`register_compute_trampolines`](crate::register_compute_trampolines)).
///
/// Consequence: `@optimized("solver::elastic_static")` FEA-result constraints
/// (e.g. `constraint peak_stress < limit` over `result.max_von_mises`) evaluate
/// against the body-inline `undef` fallback and report **Indeterminate** under
/// `reify check`.  They are NOT a gate; `reify build` or `reify eval` are the
/// FEA exit-code gate.
///
/// **Rationale:** registering compute trampolines here would run a potentially
/// slow FEA solve inside the lightweight static-check path, violating the design
/// intent that *check attaches no kernel by design*.  The trampoline-free posture
/// is an executable contract locked by `check_fea_violated_constraint_is_not_gated`
/// in `cli_build_fea.rs`; changing it requires updating that test intentionally.
///
/// **Severity of the missing-trampoline diagnostic (task 5311):** `reify check`
/// surfaces the engine-owned "no registered compute trampoline (falling back to
/// body-inlining)" diagnostic on stderr for `@optimized` FEA solves at
/// `Severity::Warning`, carrying
/// `DiagnosticCode::NoRegisteredComputeTrampoline`.  The engine conditions that
/// severity on its compute registry being entirely EMPTY, which is exactly this
/// function's posture — `cmd_check` never calls `register_compute_trampolines`,
/// so a missing trampoline here is the declared posture rather than a defect.
/// `reify eval` and `reify build` DO register the production bundle, so the
/// same diagnostic stays `Severity::Error` there and keeps gating their exit
/// codes.  The contrast is pinned by
/// `check_downgrades_unregistered_trampoline_fallback_to_warning_while_eval_and_build_keep_erroring`
/// in `crates/reify-cli/tests/harness_cli/cli_check.rs`.
///
/// **Other `error:` lines.** Every other `Severity::Error` diagnostic makes
/// `check` exit non-zero through [`check_gating_error`] unless a
/// `check_verdict::CHECK_ERROR_EXIT_ALLOWLIST` entry excuses it.  The residual shapes across
/// `examples/**/*.ri` are inventoried in #7308 (with the sweep command that
/// measures them), which owns triaging them — never by adding allowlist
/// entries.
pub(crate) fn cmd_check(args: &[String]) -> ExitCode {
    // Flag walk modeled on cmd_doc/cmd_gui: explicit handling of known flags
    // and explicit rejection of unknown `--`-prefixed tokens so a typo like
    // `--purpouse` fails loud instead of being silently treated as a file path.
    let mut purpose_values: Vec<String> = Vec::new();
    let mut cfg_values: Vec<String> = Vec::new();
    let mut strict = false;
    let mut verbose = false;
    let mut file: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--strict" => {
                strict = true;
                i += 1;
            }
            "--verbose" => {
                verbose = true;
                i += 1;
            }
            "--purpose" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --purpose requires a value");
                    eprintln!("{}", CHECK_USAGE);
                    return ExitCode::FAILURE;
                }
                purpose_values.push(args[i + 1].clone());
                i += 2;
            }
            "--cfg" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --cfg requires a value");
                    eprintln!("{}", CHECK_USAGE);
                    return ExitCode::FAILURE;
                }
                cfg_values.push(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `check`: {}", flag);
                eprintln!("{}", CHECK_USAGE);
                return ExitCode::FAILURE;
            }
            _ => {
                if file.is_none() {
                    file = Some(a);
                }
                i += 1;
            }
        }
    }

    let file = match file {
        Some(f) => f,
        None => {
            eprintln!("{}", CHECK_USAGE);
            return ExitCode::FAILURE;
        }
    };

    // Build the active cfg from the repeated `--cfg` values: target is
    // host-defaulted and overridable only by `--cfg target=<v>` (PRD §4 D-2).
    let cfg = match build_cfg_set(&cfg_values) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let compiled = match parse_and_compile_with_cfg(file, &cfg) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return ExitCode::FAILURE;
    }

    if purpose_values.is_empty() {
        // No --purpose flag: route through the appropriate check path.
        //
        // Four routing kinds need live kernel state, and a single module may
        // carry any combination:
        //   * RepresentationWithin (task-4199 γ) — needs
        //     `set_capture_repr_tol(true)` + `tessellate_realizations` to
        //     populate `achieved_repr_tol`, which `dispatch_constraints` reads.
        //   * geometric GD&T `Conforms` (η/4480) — needs
        //     `build(ExportFormat::Step)` to realize live B-rep handles into
        //     `realization_handles` (a `MaxDeviation` query is BRepOnly; only
        //     `build()` — not `tessellate_realizations` — populates that map),
        //     which `measure_gdt_conformance` reads.
        //   * DFMRule (task-4600, γ/4408) — also needs
        //     `build(ExportFormat::Step)` to populate `realization_handles`.
        //     `measure_dfm_rules` (engine_constraints.rs) reads
        //     `self.realization_handles` to assign each rule's `subject_handle`;
        //     without `build()` the handle is None and the rule is silently
        //     skipped.  C1 (no OCCT → None-kernel → build realizes nothing →
        //     measure_dfm_rules C1 guard fires → no output, exit 0).
        //     C2 (modules without DFMRule see has_dfm_rule=false →
        //     byte-identical to their previous path).
        //   * module has geometry (task 5748, PRD
        //     docs/prds/v0_6/check-diagnostic-truthfulness.md leaf β D1) — a
        //     module whose templates carry a realization op or a
        //     `Type::Geometry` value cell ([`module_has_geometry`], whose doc
        //     contract spells out both signals) needs
        //     `build(ExportFormat::Step)` for a DIFFERENT reason than the three
        //     above: geometry-query value cells (`centroid`,
        //     `moment_of_inertia`, `mass`, …) are populated only by
        //     `run_post_processes`/`post_process_geometry_queries`, which run
        //     on the build()/tessellate() path. Without it those cells stay
        //     `undef` and every constraint reading one degrades to
        //     Indeterminate — a check that is silent about geometry it never
        //     realized. `cmd_eval` already routes this way (its
        //     `module_has_geometry` branch is the precedent); this makes
        //     `check` agree. Same predicate, no new detection logic.
        //
        // amend (reviewer suggestion: robustness_routing) — these were
        // previously two mutually-exclusive `else if` arms with geometric
        // `Conforms` first, so a module carrying BOTH kinds ran only `build()`;
        // `set_capture_repr_tol`/`tessellate_realizations` never fired and every
        // RepresentationWithin silently degraded to Indeterminate. They are now
        // a single kernel-backed arm that runs EACH kind's side effect when that
        // kind is present. The side effects touch DISJOINT engine maps and
        // neither clears the other's (`build()` clears+repopulates
        // `realization_handles` but never touches `achieved_repr_tol`;
        // `tessellate_realizations()` is the exact converse), so a combined
        // module gets a correct verdict for each kind. Each single-kind module
        // runs the identical sequence it did before (C2 — byte-identical for all
        // existing inputs).
        //
        // C1 graceful degradation: with no OCCT kernel,
        // `with_registered_kernel` returns a None-kernel engine → build /
        // tessellate realize nothing → all three kinds yield Indeterminate or
        // are skipped (never a false Violated or false W_DFM_OVERHANG) → exit 0.
        //
        // When the module has NONE of the four kinds, keep the existing
        // `Engine::new(None)+check()` path verbatim (C2).
        let checker = SimpleConstraintChecker;
        let has_geometric_conforms = module_has_geometric_conforms(&compiled);
        let has_representation_within = module_has_representation_within(&compiled);
        let has_dfm_rule = module_has_dfm_rule(&compiled);
        let has_thickness_dfm = module_has_thickness_dfm_rule(&compiled);
        let has_geometry = module_has_geometry(&compiled);
        // BOTH nested gates below learn `has_geometry`, and both are
        // load-bearing (task 5748): extending only the outer one would put a
        // geometry-only module on the kernel-backed engine but never call
        // `build()`, so `run_post_processes`/`post_process_geometry_queries`
        // would never fire and the geometry-query cells would stay `undef` —
        // the routing change would be observably inert.
        // Captured by the kernel-backed arm's `build()` call below (None on the
        // lightweight arm, and on a kernel-backed module that takes only the
        // `tessellate_realizations` side effect). Consumed by
        // `merge_post_build_verdicts` after the authoritative `check()`.
        let mut build_result: Option<reify_eval::BuildResult> = None;
        let (result, engine) = if has_geometric_conforms
            || has_representation_within
            || has_dfm_rule
            || has_geometry
        {
            let mut engine = reify_eval::Engine::with_registered_kernel(reify_eval::SolverPosture::SolverFree, Box::new(checker));
            if has_representation_within {
                // Record deviation during tessellation.
                engine.set_capture_repr_tol(true);
            }
            if has_geometric_conforms || has_dfm_rule || has_geometry {
                // Realize live B-rep handles into `realization_handles`. The
                // build result is discarded; only its handle-population side
                // effect matters. Run BEFORE `tessellate_realizations` —
                // `build()` clears+repopulates `realization_handles` but does
                // not touch `achieved_repr_tol`, so the tessellate pass below
                // leaves these handles intact.
                //
                // DFMRule (has_dfm_rule): `measure_dfm_rules` reads
                // `realization_handles` to set each rule's `subject_handle`
                // and skips rules where the handle is None — the same
                // precondition as geometric Conforms.
                //
                // has_geometry (task 5748): `build()` additionally runs
                // `run_post_processes`/`post_process_geometry_queries`, which
                // is the ONLY thing that resolves geometry-query value cells
                // (`centroid`, `moment_of_inertia`, …).
                //
                // The `BuildResult` is CAPTURED, not discarded: its
                // `constraint_results` are the only post-realization verdicts
                // available here, and `merge_post_build_verdicts` below adopts
                // them where check() had nothing better. See that helper.
                //
                // `realize_for_check`, NOT `build()` (esc-5748-6): `build()`
                // also runs the Phase-B product-export walk, whose EXPORT-ONLY
                // diagnostics were harmless while the whole `BuildResult` was
                // discarded but are false errors now that the merge is live —
                // `check` exports nothing.
                //
                // COST, recorded not paid down (task 5748 → #5973): adding
                // `has_geometry` to this gate means a plain geometry module —
                // which previously took the lightweight `Engine::new(None) +
                // check()` path — now evaluates the module at least TWICE per
                // `reify check`: `realize_for_check` →
                // `build_with_geometry_output` opens with `self.check(module)`
                // (which opens with `self.eval`), and the authoritative
                // `engine.check(&compiled)` below opens with another fresh
                // `self.eval`. That second eval is also the sole reason
                // `merge_post_build_verdicts` exists — it discards the
                // post-processed value map the realization just produced, and
                // the merge papers over exactly that discard. Sub-path (c)
                // shows the alternative: it hands the realization's own values
                // to `check_constraints_with_values` and needs no verdict merge
                // at all. Moving this arm onto that shape would delete the
                // redundant eval, the verdict merge AND the (b)/(c)
                // composition asymmetry together, but it changes which passes
                // run on the check path — a behaviour change this leaf is not
                // scoped to make.
                //
                // OWNER: #5973 ("push the post-realization constraint re-check
                // down into Engine::check(), retiring cmd_check's CLI-side
                // merge_post_build_verdicts"), which is option (C) from
                // esc-5748-1's steward ruling — the ruling took the CLI-side
                // merge precisely BECAUSE (C) fell outside 5748's file scope.
                // The double eval, the verdict merge and the (b)/(c) asymmetry
                // all retire together there.
                build_result = Some(engine.realize_for_check(&compiled));
            }
            if has_representation_within {
                // Populate `achieved_repr_tol`. Does not touch
                // `realization_handles`, so the build handles above survive.
                engine.tessellate_realizations(&compiled);
            }
            if has_thickness_dfm {
                // Lazily acquire the OpenVDB kernel for the thickness-DFM
                // sub-case.  `module_has_thickness_dfm_rule` detected a
                // process conformer carrying `min_feature_size : Length` →
                // the thickness arm of `measure_dfm_rules` needs the SDF.
                // Registry-driven (§3.3 "registry not ad-hoc"), idempotent,
                // leaves `default_kernel_name` = OCCT (realize_solid_sdf
                // tessellates via default, voxelizes via openvdb_kernel_name).
                // cfg(not(has_openvdb)) → registry lacks "openvdb" → returns
                // false → no-op (C1/D5: Indeterminate, no false violation).
                engine.ensure_openvdb_kernel();
            }
            // `check()` runs `measure_gdt_conformance` (overrides the matching
            // scalar `Conforms` entry with the measured verdict),
            // `dispatch_constraints`' RepresentationWithin interception, and
            // `measure_dfm_rules` (emits W_/E_DFM_OVERHANG, W_/E_DFM_DRAFT,
            // W_/E_DFM_MIN_WALL, W_/E_DFM_MIN_FEATURE diagnostics) — each
            // reads the map its side effect populated.
            (engine.check(&compiled), engine)
        } else {
            // Existing lightweight path: no kernel, no tessellation (C2).
            let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), None);
            (engine.check(&compiled), engine)
        };

        let mut result = result;
        merge_post_build_verdicts(&mut result, build_result.as_ref());

        // D2 (task 5748): `build()`'s diagnostics are no longer discarded.
        // `check()` alone never produces the realization-only entries
        // (`compile_geometry_op` gating, kernel-dispatch failures), so without
        // this merge a module whose geometry cannot compile at all still
        // reported "All constraints satisfied." under `check`.
        //
        // ORDERING IS LOAD-BEARING and both legs run AFTER
        // `merge_post_build_verdicts`, so the filter reads the POST-upgrade
        // verdicts and the merge cannot re-append a warning the upgrade just
        // falsified. Composed and pinned in that order by
        // `d2_pass_ordering_tests::upgraded_constraints_warning_survives_in_
        // neither_list`.
        //
        // A `None` build result (the lightweight arm, and the kernel-backed
        // sub-case that takes only the `tessellate_realizations` side effect)
        // yields an empty slice, so those paths stay byte-identical (C2).
        let build_diagnostics: Vec<reify_core::Diagnostic> =
            drop_falsified_indeterminate_diagnostics(
                build_result
                    .as_ref()
                    .map(|b| b.diagnostics.as_slice())
                    .unwrap_or(&[]),
                &result.constraint_results,
            );
        let merged_diagnostics = merge_build_diagnostics(&result.diagnostics, &build_diagnostics);

        let outcome = report_eval_output(
            &result.constraint_results,
            &merged_diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        if verbose {
            report_achieved_deviations(
                engine.achieved_repr_tol_by_occurrence(),
                &mut std::io::stdout(),
            );
        }

        let exit = finish_check(
            &outcome,
            &result.constraint_results,
            strict,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // INV-SF-2: any `Severity::Error` in the set `report_eval_output` just
        // showed the user makes `check` exit non-zero, unless a
        // `CHECK_ERROR_EXIT_ALLOWLIST` entry excuses it.
        //
        // Gated on `merged_diagnostics`, NOT `result.diagnostics`, so a
        // realization-only Error moves the exit code too — e.g. the
        // post-geometry harvest's `E_DFM_BUILD_VOLUME`, which check()'s own
        // list never carries. Pinned end to end by
        // `cli_check.rs::check_exits_nonzero_on_a_realization_only_build_volume_error`.
        //
        // Runs AFTER `finish_check`, so stdout is unchanged and only the exit
        // code escalates. Warnings (GdtRemoved2018, W_DFM_*) never gate (C1).
        if check_gating_error(&merged_diagnostics).is_some() {
            return ExitCode::FAILURE;
        }

        exit
    } else {
        // --purpose path: replicates the canonical
        // eval → activate_purpose → check_constraints_with_values sequence.
        // engine.check() does NOT visit purpose-injected constraints —
        // they live in snapshot.graph.constraints, visited only by
        // check_constraints_with_values.
        //
        // GD&T legality is enforced on BOTH paths via `engine.run_gdt_check_passes`
        // (task 4589): diagnostics are folded in before `report_eval_output` below,
        // so the exit gate after `finish_check` sees a `GdtIllegalModifier` Error
        // exactly as the no-purpose path does.

        // Parse all --purpose values up front so a malformed value fails
        // before we touch the engine.
        let mut activations: Vec<PurposeActivation> = Vec::with_capacity(purpose_values.len());
        for value in &purpose_values {
            match parse_purpose_flag(value) {
                Ok(a) => activations.push(a),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return ExitCode::FAILURE;
                }
            }
        }

        let checker = SimpleConstraintChecker;
        // D1 item 2 (task 5748): a geometry-bearing module goes through
        // `build()` here, exactly as `cmd_eval` already does (cmd/eval.rs, its
        // `module_has_geometry`-gated branch). Without it this branch realizes
        // nothing, so `compile_geometry_op` diagnostics are never even PRODUCED
        // and geometry-query value cells stay `undef`.
        //
        // `Engine::with_registered_kernel` (engine_admin.rs) attaches a
        // GEOMETRY kernel only. `configured_eval_engine` is deliberately NOT
        // used: it calls `register_compute_trampolines` and wires a solver +
        // persistent FEA cache, and `check` stays compute-trampoline-free by
        // design (see the design-intent comment on `cmd_check` above, and the
        // regression lock `check_fea_violated_constraint_is_not_gated`). The
        // geometry-kernel axis and the compute-trampoline axis are independent;
        // this leaf moves only the former.
        //
        // The engine must outlive the branch: `activate_purpose` /
        // `activate_purpose_with_bindings` / `is_purpose_active` /
        // `check_constraints_with_values` / `run_gdt_check_passes` are all
        // called on it below.
        let used_build = module_has_geometry(&compiled);
        let (values, front_end_diagnostics, mut engine) = if used_build {
            let mut engine = reify_eval::Engine::with_registered_kernel(reify_eval::SolverPosture::SolverFree, Box::new(checker));
            // `realize_for_check`, not `build()` — same esc-5748-6 reason as
            // sub-path (b) above: `check` writes no artifact, so the Phase-B
            // export walk's diagnostics would be user-visible false errors.
            let r = engine.realize_for_check(&compiled);
            (r.values, r.diagnostics, engine)
        } else {
            let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), None);
            let r = engine.eval(&compiled);
            (r.values, r.diagnostics, engine)
        };

        // Activate each purpose in flag order; one check_constraints_with_values
        // call after the loop collects results for ALL injected constraints.
        for activation in &activations {
            // Single-binding form (name=entity, param==None): route through the
            // activate_purpose(name, entity) shim — byte-identical @entity prefix,
            // preserves existing single-param CLI tests (C6).
            // Everything else (len>=2, or len==1 with a named param like part:PartA):
            // route through activate_purpose_with_bindings for C2/C3 validation.
            let is_bare_single =
                activation.bindings.len() == 1 && activation.bindings[0].param.is_none();

            if is_bare_single {
                engine.activate_purpose(&activation.name, &activation.bindings[0].entity);

                // activate_purpose is silent on unknown-purpose, missing eval_state,
                // and the C2 multi-param refusal. is_purpose_active is the only
                // programmatic signal — a false result surfaces all failure modes.
                if !engine.is_purpose_active(&activation.name) {
                    eprintln!(
                        "Error: could not activate purpose '{}' (no such purpose in the file, or it requires per-param bindings)",
                        activation.name
                    );
                    return ExitCode::FAILURE;
                }
            } else {
                // Multi-binding requires every binding to name its param
                // (`part:PartA`). A bare segment mixed in (`PartA,envelope:BoxB`)
                // — or an all-bare multi value (`PartA,BoxB`) — would forward an
                // empty param string below and surface as the unactionable
                // "has no parameter ''" engine diagnostic. parse_purpose_flag is
                // the first line of defense (it rejects a bare segment in a
                // len>=2 value), so this is currently unreachable via the CLI;
                // we guard here too so cmd_check stays self-consistent and never
                // forwards an empty param if the parser is ever loosened.
                if activation.bindings.iter().any(|b| b.param.is_none()) {
                    eprintln!(
                        "Error: purpose '{}' has multiple bindings; name every parameter (e.g. 'part:PartA,envelope:BoxB')",
                        activation.name
                    );
                    return ExitCode::FAILURE;
                }
                let pairs: Vec<(String, String)> = activation
                    .bindings
                    .iter()
                    .map(|b| (b.param.clone().unwrap_or_default(), b.entity.clone()))
                    .collect();
                if let Err(e) = engine.activate_purpose_with_bindings(&activation.name, &pairs) {
                    eprintln!("Error: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }

        // `check_constraints_with_values` takes the value map as an ARGUMENT and
        // never re-runs eval, so on the geometry branch the verdicts below are
        // computed directly against the realization's POST-processed values.
        // The staleness gap that forced
        // `merge_post_build_verdicts` on sub-path (b) — where `Engine::check()`
        // opens with a fresh `self.eval(module)` and throws build()'s values
        // away — simply does not exist here, so no verdict merge is needed.
        let (constraint_results, check_diags) =
            match engine.check_constraints_with_values(&values) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return ExitCode::FAILURE;
                }
            };

        // GD&T legality pass (task 4589): runs over post-eval values identically
        // to the non-purpose branch.  Computed HERE, ahead of the diagnostic
        // assembly, because the `used_build` arm needs it to identify the
        // realization's redundant copy of the same pass (see
        // `strip_diagnostics_reproduced_by`).  Folded in below, before
        // `report_eval_output`, so the error prints to stderr alongside the
        // others.
        let gdt_diagnostics = engine.run_gdt_check_passes(&compiled, &values);

        // Front-end diagnostics first, then check diagnostics — chronological order.
        let mut diagnostics = if used_build {
            // D2 (task 5748) for sub-path (c). The realization re-runs the eval
            // front-end internally, so its list BOTH carries internal
            // duplicates (measured on `tests/fixtures/mirror_bare_origin.ri`:
            // `failed to compile geometry operation: mirror: expected a Plane
            // value, got undef` twice for one call site — re-measured at task
            // 5746, which moved that fixture's rejection to the producer and
            // with it the text of the duplicated line, not the duplication)
            // AND overlaps what
            // `check_constraints_with_values` reports; the build entries seed
            // the list and the check entries merge in behind them, so an entry
            // produced by both passes is reported exactly once.
            //
            // Same return leg as sub-path (b), applied symmetrically so the two
            // cannot drift: build's stale `ConstraintIndeterminate` claims are
            // filtered against the AUTHORITATIVE `constraint_results` first. The
            // known divergence path here is `engine_fixpoint`'s UnifiedDag
            // `declined` set, whose constraints keep build's Indeterminate
            // warning while the CLI-side check still reaches a definite verdict.
            //
            // Composition pinned by `d2_pass_ordering_tests::
            // run_in_cmd_check_purpose_order`'s tests.
            let front_end_diagnostics = drop_falsified_indeterminate_diagnostics(
                &front_end_diagnostics,
                &constraint_results,
            );
            // The realization's list also carries a COPY of the GD&T legality
            // pass appended below. Withdraw it before the dedup rather than
            // letting the dedup collapse the two runs: that pass can emit two
            // byte-identical lines for two distinct callouts, which the dedup
            // key cannot tell from a re-run, so deduping printed ONE line where
            // the non-geometry arm printed two. With the copy withdrawn, the
            // fold below is the single source and multiplicity is right by
            // construction (see `strip_diagnostics_reproduced_by`).
            let realization_only =
                strip_diagnostics_reproduced_by(&front_end_diagnostics, &gdt_diagnostics);
            let deduped_build = dedup_diagnostics(&realization_only);
            merge_build_diagnostics(&deduped_build, &check_diags)
        } else {
            // Non-geometry: plain chronological concatenation, byte-identical
            // to pre-5748 (C2). Deliberately NOT routed through the merge —
            // `eval()` does not re-run itself, so there is no duplication to
            // collapse, and running the dedup here could only ever REMOVE a
            // line this branch prints today.
            let mut d = front_end_diagnostics.clone();
            d.extend(check_diags);
            d
        };

        // BOTH arms append the legality pass's output the same plain way, so
        // multiplicity is whatever this one authoritative run produced —
        // one line per callout, two distinct callouts of the same
        // characteristic included. The `used_build` arm's realization carried a
        // second copy of the same pass (`realize_for_check` →
        // `build_with_geometry_output` seeds its diagnostic list from
        // `Engine::check`, which ends by extending with `run_gdt_check_passes`);
        // that copy was already withdrawn above, so this `extend` cannot double
        // anything. esc-5748-7 first closed the doubling with an identity merge
        // instead, which was over-strong for the same reason the dedup is.
        diagnostics.extend(gdt_diagnostics);

        let outcome = report_eval_output(
            &constraint_results,
            &diagnostics,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        if verbose {
            report_achieved_deviations(
                engine.achieved_repr_tol_by_occurrence(),
                &mut std::io::stdout(),
            );
        }

        // Same outcome → summary + exit-code mapping as the no-purpose path,
        // so a purpose-injected violation behaves identically to a structure
        // constraint violation in stdout and shell exit semantics.
        let exit = finish_check(
            &outcome,
            &constraint_results,
            strict,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );

        // INV-SF-2: the same gate as the no-purpose branch, over this branch's
        // already-merged, already-reported list — the build/eval front end,
        // `check_constraints_with_values` and `run_gdt_check_passes` folded
        // together above — and placed after `finish_check` for the same
        // reason. `cli_check.rs::check_purpose_gate_matches_the_no_purpose_gate`
        // asserts the two paths' exit codes agree.
        if check_gating_error(&diagnostics).is_some() {
            return ExitCode::FAILURE;
        }

        exit
    }
}
