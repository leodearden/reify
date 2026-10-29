//! `reify build <file>`: realizes the design and writes its output artifacts.

use std::process::ExitCode;

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::ExportFormat;

use crate::artifact_triangles::{stl_triangle_count, threemf_triangle_count};
use crate::check_verdict::{ConstraintOutcome, build_is_success, has_error_diagnostic};
use crate::constraint_report::report_eval_output;
use crate::module_capabilities::module_has_isosurface;
use crate::{configured_check_build_engine, parse_and_compile};

pub(crate) fn cmd_build(args: &[String]) -> ExitCode {
    // Shared usage text for the empty-args and no-positional-file guards.
    const USAGE: &str = "Usage: reify build <file.ri> [-o <output>] [--out-dir <dir>] [--verbose]\n  \
        With -o:    write a single file in that format (imperative).\n  \
        Without -o: every `: Output` occurrence in the design drives its own file\n              \
        (declarative); each relative path resolves against the .ri file's\n              \
        directory, or against --out-dir when given.";

    if args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }

    // Detect `--verbose` anywhere in the args.
    let verbose = args.iter().any(|a| a == "--verbose");

    // Pre-compute the index of the value that follows `-o` (if present and
    // followed by an argument).  This is reused both to build `output_path`
    // and to exclude the `-o` value from the positional-file scan below so
    // that `reify build -o out.step file.ri` doesn't mistakenly treat
    // `out.step` as the input file.
    let o_value_pos: Option<usize> = args.iter().position(|a| a == "-o").and_then(|i| {
        if i + 1 < args.len() {
            Some(i + 1)
        } else {
            None
        }
    });

    // Likewise for `--out-dir <dir>` (declarative mode's CI escape hatch): its
    // value must be excluded from the positional-file scan so it is never
    // mistaken for the input file. The following token only counts as the
    // directory value when it is NOT itself a flag, so `--out-dir` immediately
    // followed by another flag (or appearing as the last token) is treated as
    // "no value given": `out_dir_value_pos` stays None, and the malformed
    // override is warned about + dropped below rather than silently consuming a
    // flag like `--verbose` as the directory.
    let out_dir_present = args.iter().any(|a| a == "--out-dir");
    let out_dir_value_pos: Option<usize> =
        args.iter().position(|a| a == "--out-dir").and_then(|i| {
            if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                Some(i + 1)
            } else {
                None
            }
        });

    // Pick the first positional token: not a flag (`-`-prefixed) and not the
    // value following `-o` or `--out-dir`.  This makes flag ordering irrelevant,
    // so both `reify build file.ri --verbose` and `reify build --verbose file.ri`
    // correctly identify the input file.
    let file = match args.iter().enumerate().find(|(i, a)| {
        !a.starts_with('-') && Some(*i) != o_value_pos && Some(*i) != out_dir_value_pos
    }) {
        Some((_, f)) => f,
        None => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    // `-o` present selects imperative single-output mode; its absence selects
    // the declarative occurrence-driven driver (which writes nothing when the
    // design declares no `: Output` occurrences). Either mode is valid with or
    // without `--verbose` — the historical "no -o requires --verbose" guard is
    // gone now that a bare `reify build f.ri` runs the driver.
    let output_path: Option<&String> = o_value_pos.map(|i| &args[i]);

    // `--out-dir` only affects the declarative (no-`-o`) driver. Warn rather than
    // silently discard the user's intent when it can have no effect (`-o` present)
    // or is malformed (no directory argument follows).
    if out_dir_present {
        if output_path.is_some() {
            eprintln!(
                "warning: --out-dir is ignored when -o is given \
                 (imperative single-output mode writes to the -o path)"
            );
        } else if out_dir_value_pos.is_none() {
            eprintln!(
                "warning: --out-dir was given without a directory argument; ignoring it \
                 (relative output paths resolve against the design file's directory)"
            );
        }
    }

    let compiled = match parse_and_compile(file) {
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

    let checker = SimpleConstraintChecker;
    let mut engine = configured_check_build_engine(|posture| {
        reify_eval::Engine::with_registered_kernel(posture, Box::new(checker))
    });
    // Lazily acquire the OpenVDB kernel when the module contains an
    // `isosurface(...)` realization op (task δ/5002). Without OpenVDB
    // registered, the operand's Mesh→Voxel voxelize stage and the terminal
    // Voxel→Mesh marching-cubes stage (task γ/5001) have no kernel to
    // dispatch to, so the build degrades to an Error diagnostic and exits
    // non-zero. Mirrors cmd_check's `module_has_thickness_dfm_rule` →
    // `ensure_openvdb_kernel()` gate: a static pre-eval detector keeps every
    // non-isosurface build byte-identical and preserves the single-pick
    // OCCT alloc-cost posture (engine_admin.rs). cfg(not(has_openvdb)) →
    // registry lacks "openvdb" → returns false → no-op (an isosurface build
    // would still degrade in that configuration).
    if module_has_isosurface(&compiled) {
        engine.ensure_openvdb_kernel();
    }
    match output_path {
        // ===== Mode (A): imperative single-output (`-o` present). UNCHANGED
        //       back-compat path (B10): the `-o` extension selects the format,
        //       build() serializes the product bodies, and the bytes are written
        //       verbatim to the `-o` target. =====
        Some(path) => {
            // Export refusal (task η, PRD
            // `docs/prds/v0_6/precision-nominal-representation-guarantee.md`
            // §1.1 / C-SURFACE (2)): a design declaring a `RepresentationWithin`
            // bound this path cannot demonstrate it honours must REFUSE to write
            // the artifact rather than write it and report success.
            //
            // WHY THE GATE PRECEDES THE WRITE RATHER THAN RIDING THE DIAGNOSTIC
            // STREAM: `std::fs::write(path, &data)` below runs BEFORE
            // `has_error_diagnostic` is evaluated, so an Error diagnostic ALONE
            // cannot withhold the file — it would exit non-zero having already
            // created (or truncated) the `-o` target. Returning here is what
            // makes the refusal gate the WRITE itself. (Mode B needs no such
            // care: the engine withholds the file by emitting an empty-bytes
            // artifact, which the writer below skips.)
            //
            // Refusing BEFORE `engine.build()` also skips realization and OCCT
            // tessellation entirely, honouring PRD §6's gate-cost rule: η's
            // refusal is a static module-shape decision taken before any
            // measurement. The same helper backs the engine-side Mode-B refusal,
            // so the two surfaces cannot disagree about what counts as bounded.
            //
            // Follow-on task θ narrows this refusal to genuinely UNACHIEVABLE
            // bounds once the export path gains a real measurement (hard-dep on
            // task 6085); until then it fires for any declared bound.
            //
            // KNOWN REPORTING ASYMMETRY WITH MODE B — deliberate, not an
            // oversight. Returning here means a refused `-o` build prints the
            // refusal and NOTHING ELSE: no constraint results, no
            // "No constraints violated (N indeterminate)." summary. Mode B
            // below prints both, because it MUST realize anyway (it needs the
            // elaborated `StructureInstance`s to enumerate `: Output`
            // occurrences at all) and gets the constraint results for free from
            // the same `build_outputs_with_result` call. Mode A has no such
            // obligation: `-o` names its destination outright, so realization
            // buys nothing the refusal needs.
            //
            // The two surfaces are unified on WHAT IS REFUSED and on the
            // diagnostic itself — one shared
            // `unenforced_representation_bound_diagnostic`, one E_* token, one
            // typed code, one exit gate — and that is the property η contracts
            // for (C-SURFACE (2)). They are NOT unified on the surrounding
            // report, and buying that symmetry costs a full realization plus
            // OCCT tessellation (5-20 s on the PRD §2.2 measurements) on a build
            // that is about to write nothing, which is exactly what PRD §6's
            // gate-cost rule forbids. A user who wants the constraint state runs
            // `reify check` — which the refusal message itself tells them to do.
            if let Some(diag) =
                reify_eval::tolerance_combine::unenforced_representation_bound_diagnostic(&compiled)
            {
                // Report through the EXISTING report_eval_output path rather
                // than a bespoke stderr write, so the refusal lands in the same
                // order as every other diagnostic (PRD INV-SF-2: "η rides
                // `cmd_build`'s existing gate rather than adding a per-code
                // bolt-on").
                //
                // THE EXIT CODE IS UNCONDITIONAL, AND THAT IS THE POINT: reaching
                // this arm means the artifact is BEING REFUSED and the write below
                // is being skipped outright, so write-suppression and exit code
                // must not be able to decouple. Deriving it from
                // `build_is_success(&outcome, has_error_diagnostic)` instead would
                // couple them to the builder's severity — a contract pinned only
                // by a unit test in another crate. Were
                // `unenforced_representation_bound_diagnostic` ever to return a
                // Warning/Info, `cmd_build` would write NO file at the `-o`
                // target, print nothing on stdout (`report_constraint_results(&[])`
                // writes nothing) and exit 0: a silent no-op export, the worst
                // failure mode a build command has. This is behaviour-preserving
                // today — the builder returns `Severity::Error` and
                // `constraint_results` is empty, so `build_is_success` is already
                // `false` here — and it stays correct if that ever changes.
                //
                // The `debug_assert` is the loud half of the same guard: a
                // severity regression fails the test suite at this line instead of
                // silently degrading the report (a non-Error diagnostic would print
                // as a warning while the build still exits non-zero — accurate exit
                // code, misleading message).
                debug_assert_eq!(
                    diag.severity,
                    Severity::Error,
                    "the export refusal must be Error-severity: it is what the rest of \
                     the CLI's reporting treats as a refusal rather than an advisory"
                );
                let diagnostics = [diag];
                let _ = report_eval_output(
                    &[],
                    &diagnostics,
                    &mut std::io::stdout(),
                    &mut std::io::stderr(),
                );
                return ExitCode::FAILURE;
            }

            let format = match path {
                p if p.ends_with(".step") || p.ends_with(".stp") => ExportFormat::Step,
                p if p.ends_with(".stl") => ExportFormat::Stl,
                p if p.ends_with(".3mf") => ExportFormat::ThreeMF,
                _ => {
                    eprintln!("Unknown output format, defaulting to STEP");
                    ExportFormat::Step
                }
            };

            let result = engine.build(&compiled, format);

            let outcome = report_eval_output(
                &result.constraint_results,
                &result.diagnostics,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );

            // Under --verbose, print per-realization kernel provenance to stdout.
            if verbose {
                let provenance = engine.realization_kernel_provenance();
                for entry in &provenance {
                    println!(
                        "  {}: kernel: {}, repr: {:?}",
                        entry.realization,
                        entry.kernel.as_registry_name(),
                        entry.repr,
                    );
                }
                // Task 4744 β (step-22): mesh-morph activity for this build
                // (morphed / remeshed / ineligible — the morph_stats counters,
                // also exposed via the mesh_morph_stats debug RPC).
                println!(
                    "  {}",
                    reify_mesh_morph::diagnostics::format_summary(
                        &reify_mesh_morph::diagnostics::snapshot()
                    )
                );
            }

            match result.geometry_output {
                Some(data) => {
                    if let Err(e) = std::fs::write(path, &data) {
                        eprintln!("Error writing {}: {}", path, e);
                        return ExitCode::FAILURE;
                    }
                    println!("Wrote {} ({} bytes)", path, data.len());
                    // Task δ/5002: print the exported triangle count for mesh
                    // formats (Stl/ThreeMF); STEP/BRep builds are unchanged.
                    // `ExportFormat::Obj` cannot reach this match: the format
                    // matcher above (this Mode-A `-o` arm) has no `.obj`
                    // case, so an `.obj` path falls through to the `_ =>`
                    // STEP default before `format` is ever `Obj` here. Both
                    // arms read the count from `data` — the bytes actually
                    // written to `path` — rather than re-deriving it by
                    // tessellating again, so the printed count can never
                    // disagree with the file on disk; see
                    // `stl_triangle_count` / `threemf_triangle_count` for the
                    // per-format byte layout and `triangle_count_tests` for
                    // coverage.
                    //
                    // Mode-A (`-o`) only: Mode-B's declarative `: Output`
                    // loop below prints no per-artifact count — see the note
                    // there for the scope rationale.
                    match format {
                        ExportFormat::Stl => println!("Triangles: {}", stl_triangle_count(&data)),
                        ExportFormat::ThreeMF => {
                            println!("Triangles: {}", threemf_triangle_count(&data))
                        }
                        _ => {}
                    }
                    // Emit the per-outcome status message (unchanged from
                    // pre-4458), then decide exit via build_is_success — which
                    // also gates on Severity::Error diagnostics, matching
                    // cmd_eval's Error gate (task 4458 fix (c)).
                    match &outcome {
                        ConstraintOutcome::AllSatisfied => {}
                        ConstraintOutcome::SomeIndeterminate(n) => {
                            println!("No constraints violated ({n} indeterminate).");
                        }
                        ConstraintOutcome::SomeViolated => {
                            println!("Some constraints violated.");
                        }
                    }
                    if build_is_success(&outcome, has_error_diagnostic(&result.diagnostics)) {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                None => {
                    eprintln!("No geometry output produced");
                    ExitCode::FAILURE
                }
            }
        }

        // ===== Mode (B): declarative occurrence-driven export (no `-o`). The
        //       DSL `: Output` occurrences drive the format(s) + path(s); the
        //       CLI is a thin writer. =====
        None => {
            // CI escape hatch: --out-dir overrides the design-file directory as
            // the base for relative occurrence paths.
            let out_dir_override =
                out_dir_value_pos.map(|i| std::path::Path::new(args[i].as_str()));
            // design_dir = the .ri file's parent (B7: occurrence paths are
            // design-file-relative, not cwd-relative). A bare "foo.ri" (empty
            // parent) resolves against ".".
            let design_dir = std::path::Path::new(file)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));

            // Realize the module ONCE: build_outputs_with_result runs a single
            // realization (Phase-B product serialization DISABLED) and returns
            // both the per-occurrence artifacts AND that realization's constraint
            // results + diagnostics. This replaces the earlier
            // `engine.build()` + `engine.build_outputs()` pair, which realized,
            // constraint-checked, and serialized the discarded Phase-B STEP output
            // twice. Kernel provenance is read from engine state below, populated
            // by this single realization.
            let outputs = engine.build_outputs_with_result(&compiled, design_dir, out_dir_override);
            let artifacts = &outputs.artifacts;

            // Surface BOTH the build diagnostics AND every per-artifact
            // diagnostic (an I_DISPLAY_OUTPUT_DEFERRED info, or a per-occurrence
            // export error) through the shared reporter + exit gate.
            let mut all_diagnostics = outputs.diagnostics.clone();
            for artifact in artifacts {
                all_diagnostics.extend(artifact.diagnostics.iter().cloned());
            }

            let outcome = report_eval_output(
                &outputs.constraint_results,
                &all_diagnostics,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            );

            // Under --verbose, print per-realization kernel provenance to stdout.
            // Preserved for a no-`-o` build even when ZERO Output occurrences are
            // declared, so `build bracket.ri --verbose` still reports
            // 'kernel: occt' and exits 0 (cli_build_verbose regression guard).
            if verbose {
                let provenance = engine.realization_kernel_provenance();
                for entry in &provenance {
                    println!(
                        "  {}: kernel: {}, repr: {:?}",
                        entry.realization,
                        entry.kernel.as_registry_name(),
                        entry.repr,
                    );
                }
                // Task 4744 β (step-22): mesh-morph activity for this build
                // (morphed / remeshed / ineligible — the morph_stats counters,
                // also exposed via the mesh_morph_stats debug RPC).
                println!(
                    "  {}",
                    reify_mesh_morph::diagnostics::format_summary(
                        &reify_mesh_morph::diagnostics::snapshot()
                    )
                );
            }

            // Write one file per artifact. Gate on non-empty bytes (NEVER on
            // format): a DisplayOutput-deferred or failed-occurrence artifact
            // carries empty bytes and must write no file.
            //
            // Unlike Mode-A above, this declarative per-artifact loop does
            // NOT print a `Triangles: N` count — task δ/5002's PRD scope is
            // the imperative `-o` CLI path only (proven by
            // `cli_build_voxel_to_mesh.rs`); see the Mode-A `-o` arm's
            // comment above for the full scope rationale. Each `artifact`
            // here does carry `format` + `bytes`, so `stl_triangle_count` /
            // `threemf_triangle_count` are reusable directly, keyed on
            // `artifact.format`, if Mode-B observability is ever wanted.
            let mut files_written = 0usize;
            for artifact in artifacts {
                if artifact.bytes.is_empty() {
                    continue;
                }
                if let Some(parent) = artifact.path.parent()
                    && !parent.as_os_str().is_empty()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    eprintln!("Error creating {}: {}", parent.display(), e);
                    return ExitCode::FAILURE;
                }
                if let Err(e) = std::fs::write(&artifact.path, &artifact.bytes) {
                    eprintln!("Error writing {}: {}", artifact.path.display(), e);
                    return ExitCode::FAILURE;
                }
                println!(
                    "Wrote {} ({} bytes)",
                    artifact.path.display(),
                    artifact.bytes.len()
                );
                files_written += 1;
            }

            // Explain a silent zero-file success: without this, a bare
            // `reify build f.ri` whose design declares no `: Output` occurrence
            // (or only deferred/failed ones) exits SUCCESS having printed nothing
            // about output — a likely "I forgot -o / forgot to declare an Output"
            // confusion. Print one informational line so the no-write outcome is
            // never unexplained. (Per-occurrence deferral/failure diagnostics were
            // already surfaced above via report_eval_output.)
            if files_written == 0 {
                if artifacts.is_empty() {
                    println!(
                        "No output files written: the design declares no `: Output` \
                         occurrences. Use `-o <file>` to write a single file, or add an \
                         output occurrence (e.g. `sub o = STLOutput(subject: <part>, \
                         path: \"out.stl\")`)."
                    );
                } else {
                    println!(
                        "No output files written: all {} `: Output` occurrence(s) were \
                         deferred or failed (see diagnostics above).",
                        artifacts.len()
                    );
                }
            }

            // Same status message + exit gate as the imperative path: 0 file
            // artifacts + no Error diagnostic + no violated constraint => SUCCESS.
            match &outcome {
                ConstraintOutcome::AllSatisfied => {}
                ConstraintOutcome::SomeIndeterminate(n) => {
                    println!("No constraints violated ({n} indeterminate).");
                }
                ConstraintOutcome::SomeViolated => {
                    println!("Some constraints violated.");
                }
            }
            if build_is_success(&outcome, has_error_diagnostic(&all_diagnostics)) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
