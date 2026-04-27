//! Module-pragma post-pass.
//!
//! Runs after [`crate::compile_builder::ctx::CompilationCtx::into_compiled_module`]
//! has assembled the [`CompiledModule`]. Extracts semantics from KNOWN module-level
//! pragmas (currently only `#precision`) into typed fields on the module, and emits
//! diagnostics for malformed or out-of-scope forms.
//!
//! The complementary pre-pass `validate_module_pragmas`
//! ([`crate::compile_builder::pre_pass`]) only warns on UNKNOWN module-pragma names;
//! the two passes run at different phases and emit non-overlapping diagnostic sets.

use std::collections::BTreeMap;

use reify_syntax::{ParsedModule, Pragma, PragmaArg, PragmaValue};
use reify_types::{Diagnostic, DiagnosticLabel, DimensionVector, SourceSpan, Value};

use crate::types::{CompiledModule, SolverPragma};
use crate::units::unit_to_scalar;

/// Apply every known module-level pragma to the assembled `CompiledModule`,
/// mutating typed fields and pushing diagnostics in place.
pub(crate) fn apply_module_pragmas(parsed: &ParsedModule, module: &mut CompiledModule) {
    apply_precision_pragma(parsed, module);
    apply_version_pragma(parsed, module);
    apply_solver_pragma(parsed, module);
    apply_kernel_pragma(parsed, module);
    warn_block_level_precision(module);
    warn_block_level_solver(module);
}

/// The maximum target language version this compiler can compile.
///
/// The too-new error, too-old warning, and their span labels in
/// `apply_version_pragma` all derive their `MAJOR.MINOR` literal from this
/// constant via `format!`, so bumping it (e.g. to `(0, 2)`) automatically
/// updates the user-facing wording. The PRD (`docs/prds/pragmas.md` §5)
/// specifies the wording template, not a literal version string.
const COMPILER_SUPPORTED_VERSION: (u16, u16) = (0, 1);

/// Append the spans of every pragma whose `name` matches `target_name` in
/// `pragmas` to `out`. Shared by `warn_block_level_precision` and
/// `warn_block_level_solver` so a future container-set change updates both
/// passes from a single edit.
fn collect_named_pragma_spans(target_name: &str, pragmas: &[Pragma], out: &mut Vec<SourceSpan>) {
    for pragma in pragmas {
        if pragma.name == target_name {
            out.push(pragma.span);
        }
    }
}

/// Walk every block-level pragma container on the assembled module and emit
/// one "ignored in v0.1; per-block tolerance deferred to v0.2" warning per
/// `#precision` pragma found.
///
/// PRD §2: per-block tolerance is deferred to v0.2. The complementary
/// `validate_pragmas` pre-pass deliberately does NOT warn on `#precision` (it
/// is in `KNOWN_BLOCK_PRAGMAS`), so this is the single site that flags
/// block-level usage.
///
/// **Invariant — keep these four containers in sync with `CompiledModule`.**
/// Every field on `CompiledModule` whose element type carries
/// `pub pragmas: Vec<reify_syntax::Pragma>` must be walked here. Today those
/// fields are: `templates` (`TopologyTemplate`), `trait_defs` (`CompiledTrait`),
/// `compiled_purposes` (`CompiledPurpose`), and `constraint_defs`
/// (`CompiledConstraintDef`). If a future PR adds a fifth pragma-bearing
/// container (e.g. `compiled_functions`), append a matching loop below — and
/// also update the sibling `warn_block_level_solver`, which walks the same
/// four containers for `#solver`.
fn warn_block_level_precision(module: &mut CompiledModule) {
    let mut spans: Vec<SourceSpan> = Vec::new();

    for tmpl in &module.templates {
        collect_named_pragma_spans("precision", &tmpl.pragmas, &mut spans);
    }
    for trait_def in &module.trait_defs {
        collect_named_pragma_spans("precision", &trait_def.pragmas, &mut spans);
    }
    for purpose in &module.compiled_purposes {
        collect_named_pragma_spans("precision", &purpose.pragmas, &mut spans);
    }
    for constraint_def in &module.constraint_defs {
        collect_named_pragma_spans("precision", &constraint_def.pragmas, &mut spans);
    }

    for span in spans {
        module.diagnostics.push(
            Diagnostic::warning(
                "#precision is ignored in v0.1; per-block tolerance deferred to v0.2",
            )
            .with_label(DiagnosticLabel::new(span, "ignored in v0.1")),
        );
    }
}

/// Walk every block-level pragma container on the assembled module and emit
/// one "ignored in v0.1; per-block solver tuning deferred to v0.2" warning
/// per `#solver` pragma found.
///
/// PRD §3: per-block solver selection is deferred to v0.2. The complementary
/// `validate_pragmas` pre-pass deliberately does NOT warn on `#solver` (it
/// is in `KNOWN_BLOCK_PRAGMAS`), so this is the single site that flags
/// block-level usage.
///
/// Mirrors `warn_block_level_precision`; see that function's container-set
/// invariant doc for the contract on adding a fifth pragma-bearing container.
fn warn_block_level_solver(module: &mut CompiledModule) {
    let mut spans: Vec<SourceSpan> = Vec::new();

    for tmpl in &module.templates {
        collect_named_pragma_spans("solver", &tmpl.pragmas, &mut spans);
    }
    for trait_def in &module.trait_defs {
        collect_named_pragma_spans("solver", &trait_def.pragmas, &mut spans);
    }
    for purpose in &module.compiled_purposes {
        collect_named_pragma_spans("solver", &purpose.pragmas, &mut spans);
    }
    for constraint_def in &module.constraint_defs {
        collect_named_pragma_spans("solver", &constraint_def.pragmas, &mut spans);
    }

    for span in spans {
        module.diagnostics.push(
            Diagnostic::warning(
                "#solver is ignored in v0.1; per-block solver tuning deferred to v0.2",
            )
            .with_label(DiagnosticLabel::new(span, "ignored in v0.1")),
        );
    }
}

/// Solver back-end names recognised by the v0.1 compiler.
///
/// PRD §3 enumerates the v0.1 known back-ends: `libslvs` (geometric / 2D
/// dimensional constraints) and `argmin` (numerical optimisation). Any other
/// name is allowed through `apply_solver_pragma` — `solver_pragma` is still
/// populated per the storage-reflects-declared design decision (mirroring
/// `#version`'s policy) — but emits a compile-time warning surfaced via the
/// LSP diagnostic layer to catch typos like `#solver(libslsv)` early.
///
/// This list is intentionally hardcoded: the runtime `Engine.solvers` registry
/// (registered via `register_solver`) is a separate axis. An embedder can run
/// reify-eval with only `argmin` registered, in which case `#solver(libslvs)`
/// parses cleanly here (no compile warning) but still falls through to the
/// default at solve time. Both warnings are independently useful.
const KNOWN_SOLVER_BACKENDS: &[&str] = &["libslvs", "argmin"];

/// Sane upper bound for the global tessellation tolerance, in SI metres.
///
/// Values larger than this are almost certainly a unit mistake (e.g. the user
/// wrote `1m` thinking millimetres) and would push OCCT into a regime where it
/// either errors, hangs, or produces meaningless meshes. The default
/// (`Engine::DEFAULT_TESSELLATION_TOLERANCE`, 0.0001 m = 0.1 mm) is four orders
/// of magnitude tighter than this cap, so users who genuinely need a coarser
/// tolerance can still pick anything up to and including 1 m.
const MAX_PRECISION_TOLERANCE_M: f64 = 1.0;

/// Process the first well-formed module-level `#precision(<Length-quantity>)` pragma:
/// store its SI-metres value on `module.default_tolerance`. All other shapes emit a
/// warning (or info, for the legacy `#precision(float64)` form) and leave
/// `default_tolerance` unset. Subsequent `#precision` pragmas (regardless of arg
/// shape) emit a "subsequent pragma ignored; first one wins" warning.
///
/// **Unit-resolution scope (v0.1).** Only the built-in SI/imperial length units
/// understood by [`unit_to_scalar`] are accepted: `m`, `mm`, `cm`, `in`. The
/// per-module/per-prelude `UnitRegistry` that compiled expressions consult via
/// `lookup_unit_in_registry` (see `expr.rs::QuantityLiteral`) is **not** queried
/// here because the registry is owned by `CompilationCtx` and is consumed before
/// this post-pass runs. As a consequence, `#precision(1ft)` after a user
/// declaration of `unit ft = 0.3048m` will emit an "unrecognised unit" warning
/// even though the rest of the language accepts it. Plumbing the prelude /
/// in-module `UnitRegistry` into this pass is deferred to v0.2; see PRD
/// `docs/prds/pragmas.md` §2.
///
/// **Range bounds (v0.1).** The accepted SI-metres value must be finite,
/// strictly positive, and ≤ [`MAX_PRECISION_TOLERANCE_M`]. Values outside this
/// range emit a warning and leave `default_tolerance` unset (so the engine
/// falls back to its built-in default). The grammar's `number_literal` regex
/// (`\d+(\.\d+)?`) currently produces only non-negative finite f64 values, so
/// the non-finite / negative branches are unreachable from source today; they
/// remain as defence-in-depth against a future grammar relaxation.
fn apply_precision_pragma(parsed: &ParsedModule, module: &mut CompiledModule) {
    let mut first_seen = false;
    for pragma in &parsed.pragmas {
        if pragma.name != "precision" {
            continue;
        }

        if first_seen {
            module.diagnostics.push(
                Diagnostic::warning("subsequent #precision pragma ignored; first one wins")
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
            );
            continue;
        }
        first_seen = true;

        // First-seen pragma: interpret its args.
        match pragma.args.as_slice() {
            [PragmaArg::Bare(PragmaValue::Quantity { value, unit })] => {
                match unit_to_scalar(*value, unit) {
                    Some((Value::Scalar { si_value, dimension }, _))
                        if dimension == DimensionVector::LENGTH =>
                    {
                        // Range gate: tolerance must be finite, > 0, and within
                        // a sane upper bound. Out-of-range values warn and leave
                        // default_tolerance unset so the engine falls back to
                        // Engine::DEFAULT_TESSELLATION_TOLERANCE.
                        //
                        // Negative / non-finite cases are currently unreachable
                        // from source (see fn-level docstring) but the check
                        // stays as a safety net.
                        if !si_value.is_finite() {
                            module.diagnostics.push(
                                Diagnostic::warning(
                                    "#precision: tolerance is not finite; ignored",
                                )
                                .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                            );
                        } else if si_value <= 0.0 {
                            module.diagnostics.push(
                                Diagnostic::warning(format!(
                                    "#precision: tolerance must be positive (got {si_value}m); \
                                     ignored"
                                ))
                                .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                            );
                        } else if si_value > MAX_PRECISION_TOLERANCE_M {
                            module.diagnostics.push(
                                Diagnostic::warning(format!(
                                    "#precision: tolerance {si_value}m exceeds the v0.1 cap of \
                                     {MAX_PRECISION_TOLERANCE_M}m; ignored"
                                ))
                                .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                            );
                        } else {
                            module.default_tolerance = Some(si_value);
                        }
                    }
                    Some(_) => {
                        // unit_to_scalar matched, but the dimension is not LENGTH
                        // (e.g. `0.001s`).
                        module.diagnostics.push(
                            Diagnostic::warning(
                                "#precision: expected a Length quantity (e.g. 0.001m); ignored",
                            )
                            .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                        );
                    }
                    None => {
                        // Unrecognised unit (e.g. `1foo`). See fn-level docstring
                        // for the v0.1 unit-resolution scope.
                        module.diagnostics.push(
                            Diagnostic::warning(format!(
                                "#precision: unrecognised unit '{unit}'; v0.1 supports m/mm/cm/in"
                            ))
                            .with_label(DiagnosticLabel::new(pragma.span, "unrecognised unit")),
                        );
                    }
                }
            }
            [PragmaArg::Bare(PragmaValue::Ident(s))] if s == "float64" => {
                module.diagnostics.push(
                    Diagnostic::info(
                        "#precision(float64) recognised but ignored \u{2014} v0.1 always uses \
                         float64; use a Length literal (e.g. 0.001m) to set the default tolerance",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored legacy form")),
                );
            }
            [PragmaArg::Bare(PragmaValue::Number(_))] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m), got a bare \
                         number; ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            [PragmaArg::KeyValue { .. }] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m); key=value form \
                         not recognised in v0.1; ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            _ => {
                // Catch-all for any other shape: zero args, multiple args, a
                // bare String/Bool, or an Ident other than `float64`. All emit
                // the same generic "expected a Length literal" warning and
                // leave default_tolerance unset.
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
        }
    }
}

/// Process the first well-formed module-level `#version(MAJOR.MINOR)` pragma:
/// store its (MAJOR, MINOR) tuple on `module.declared_version` and validate
/// against [`COMPILER_SUPPORTED_VERSION`]. See `docs/prds/pragmas.md` §5.
///
/// Per design decision, `declared_version` reflects what the user wrote
/// regardless of validation outcome (too-new error, too-old warning, or
/// in-range silent), so downstream tooling can render the user's intent
/// verbatim. Only malformed args and duplicates leave it `None`.
fn apply_version_pragma(parsed: &ParsedModule, module: &mut CompiledModule) {
    let mut first_seen = false;
    for pragma in &parsed.pragmas {
        if pragma.name != "version" {
            continue;
        }

        if first_seen {
            // PRD §5: duplicate `#version` is an error (not a warning, unlike
            // #precision). The first pragma's stored `declared_version` and
            // its validation diagnostic stay; only the redundant pragma is
            // flagged here.
            module.diagnostics.push(
                Diagnostic::error("at most one #version declaration per module")
                    .with_label(DiagnosticLabel::new(pragma.span, "duplicate #version")),
            );
            continue;
        }
        first_seen = true;

        // First-seen pragma: interpret its args.
        let parsed_version: Option<(u16, u16)> = match pragma.args.as_slice() {
            [PragmaArg::Bare(PragmaValue::Number(n))] => {
                // Render via Display (shortest round-tripping repr) and split
                // on '.' to extract MAJOR / MINOR. See task design decision:
                // `0.10` lexes to the same f64 as `0.1` (printed as "0.1");
                // users who need MINOR=10 must use the string form.
                //
                // Integer-valued numbers (e.g. `0.0`, `1.0`) lose their `.0`
                // under f64 Display, yielding `"0"` / `"1"`; treat those as
                // MAJOR with MINOR=0 so `#version(0.0)` parses cleanly.
                //
                // Non-finite / negative short-circuit: f64 Display renders
                // NaN/inf as "NaN"/"inf" and negatives with a leading '-',
                // none of which parse as u16. Catching them up-front lets us
                // give a more informative warning than the generic catch-all
                // and avoids wasting work on hopeless inputs. The grammar's
                // `\d+(\.\d+)?` regex currently produces only non-negative
                // finite f64s from source, so this branch is unreachable
                // today; it stays as defence-in-depth (mirroring the
                // analogous guard in `apply_precision_pragma`).
                if !n.is_finite() || *n < 0.0 {
                    module.diagnostics.push(
                        Diagnostic::warning(
                            "#version: version number must be non-negative and finite; \
                             expected MAJOR.MINOR (e.g. 0.1); ignored",
                        )
                        .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                    );
                    None
                } else {
                    let rendered = format!("{n}");
                    let result = match rendered.split_once('.') {
                        Some((maj_s, min_s)) => {
                            match (maj_s.parse::<u16>(), min_s.parse::<u16>()) {
                                (Ok(maj), Ok(min)) => Some((maj, min)),
                                _ => None,
                            }
                        }
                        None => match rendered.parse::<u16>() {
                            Ok(maj) => Some((maj, 0)),
                            Err(_) => None,
                        },
                    };
                    if result.is_none() {
                        // Finite non-negative number that fails to parse as
                        // u16.u16 (e.g. > 65535).
                        module.diagnostics.push(
                            Diagnostic::warning(
                                "#version: expected MAJOR.MINOR (e.g. 0.1); ignored",
                            )
                            .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                        );
                    }
                    result
                }
            }
            [PragmaArg::Bare(PragmaValue::String(s))] => {
                // Strict MAJOR.MINOR — exactly two components, each parseable
                // as u16. This is the form to use when the Number form would
                // round-trip ambiguously (e.g. `0.10` vs `0.1`).
                let parts: Vec<&str> = s.split('.').collect();
                let result = if parts.len() == 2 {
                    match (parts[0].parse::<u16>(), parts[1].parse::<u16>()) {
                        (Ok(maj), Ok(min)) => Some((maj, min)),
                        _ => None,
                    }
                } else {
                    None
                };
                if result.is_none() {
                    // String that didn't split into exactly 2 u16 components
                    // (e.g. "foo", "0.1.2", "a.b").
                    module.diagnostics.push(
                        Diagnostic::warning("#version: expected MAJOR.MINOR (e.g. 0.1); ignored")
                            .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                    );
                }
                result
            }
            _ => {
                // Catch-all for zero args, multiple args, bare Bool/Ident/
                // Quantity, and KeyValue. The wording is more explicit than
                // the form-specific arms because the user hasn't picked a
                // form yet.
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#version: expected MAJOR.MINOR number or string \
                         (e.g. #version(0.1) or #version(\"0.1\")); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
                None
            }
        };

        if let Some((maj, min)) = parsed_version {
            // Storage reflects the user-declared tuple regardless of
            // validation outcome (see task design decision).
            module.declared_version = Some((maj, min));

            let (sup_maj, sup_min) = COMPILER_SUPPORTED_VERSION;
            if (maj, min) > COMPILER_SUPPORTED_VERSION {
                module.diagnostics.push(
                    Diagnostic::error(format!(
                        "module declares version {maj}.{min}; this compiler supports up to \
                         {sup_maj}.{sup_min}"
                    ))
                    .with_label(DiagnosticLabel::new(pragma.span, "unsupported version")),
                );
            } else if (maj, min) < COMPILER_SUPPORTED_VERSION {
                module.diagnostics.push(
                    Diagnostic::warning(format!(
                        "declared version {maj}.{min} predates the first stable language; \
                         treating as {sup_maj}.{sup_min}"
                    ))
                    .with_label(DiagnosticLabel::new(
                        pragma.span,
                        format!("predates v{sup_maj}.{sup_min}"),
                    )),
                );
            }
        }
    }
}

/// Process the first well-formed module-level
/// `#solver(<back-end-ident>, [key=value, ...])` pragma: store the back-end
/// name + options on `module.solver_pragma`. Subsequent `#solver` pragmas
/// emit a "subsequent pragma ignored; first one wins" warning. See
/// `docs/prds/pragmas.md` §3.
///
/// Per design decision, `solver_pragma` reflects the user-declared back-end
/// name regardless of whether the name is in the v0.1 known list, mirroring
/// `#version`'s storage-reflects-declared policy. Only malformed argument
/// shapes (no Bare-Ident first arg, KeyValue-only, etc.) leave it `None`.
fn apply_solver_pragma(parsed: &ParsedModule, module: &mut CompiledModule) {
    let mut first_seen = false;
    for pragma in &parsed.pragmas {
        if pragma.name != "solver" {
            continue;
        }

        if first_seen {
            module.diagnostics.push(
                Diagnostic::warning("subsequent #solver pragma ignored; first one wins")
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
            );
            continue;
        }
        first_seen = true;

        // First-seen pragma: interpret its args. The well-formed shape is
        // `[Bare(Ident(name)), KeyValue*]`. The single bare-ident case is a
        // degenerate of that shape (empty tail). Every other shape leaves
        // `solver_pragma` as `None` and emits a single "expected #solver(...)"
        // warning whose message contains both "expected" and "ident" so the
        // form-hint substring assertion in
        // `solver_pragma_malformed_args_emit_warning_and_leave_solver_pragma_none`
        // holds.
        match pragma.args.as_slice() {
            [PragmaArg::Bare(PragmaValue::Ident(name)), tail @ ..] => {
                let mut options: BTreeMap<String, PragmaValue> = BTreeMap::new();
                for (idx, arg) in tail.iter().enumerate() {
                    match arg {
                        PragmaArg::KeyValue { key, value } => {
                            // Last-wins on duplicate keys, matching BTreeMap::insert
                            // semantics. The compiler does not warn on duplicates
                            // in v0.1 (deferred to back-end-level validation per
                            // PRD §3).
                            options.insert(key.clone(), value.clone());
                        }
                        PragmaArg::Bare(_) => {
                            // The user wrote a second bare value where a key=value
                            // was expected (e.g. `#solver(argmin, foo)`). Skip the
                            // arg and continue processing the rest. Position is
                            // 1-based so users see the arg number after the
                            // back-end ident.
                            let pos = idx + 2;
                            module.diagnostics.push(
                                Diagnostic::warning(format!(
                                    "#solver: option arguments must be `key = value`; \
                                     got bare value at position {pos}; ignored"
                                ))
                                .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                            );
                        }
                    }
                }
                // Compile-time check: warn if the back-end name is not in the
                // v0.1 known list. Storage of `solver_pragma` happens
                // unconditionally, mirroring `#version`'s
                // storage-reflects-declared policy — downstream tooling and the
                // runtime registry still need to see the user's verbatim name.
                if !KNOWN_SOLVER_BACKENDS.contains(&name.as_str()) {
                    module.diagnostics.push(
                        Diagnostic::warning(format!(
                            "#solver: unknown back-end '{name}'; v0.1 supports \
                             {{libslvs, argmin}} \u{2014} falling back to default at runtime"
                        ))
                        .with_label(DiagnosticLabel::new(pragma.span, "unknown back-end")),
                    );
                }
                module.solver_pragma = Some(SolverPragma {
                    name: name.clone(),
                    options,
                });
            }
            // Zero args: `#solver` with no arg list.
            [] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#solver: expected #solver(<back-end-ident>, [key=value, ...]); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // Bare Number / Bool / String / Quantity as the (only) first arg.
            // The Ident case was already matched above; this arm catches the
            // remaining bare scalar variants.
            [PragmaArg::Bare(_)] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#solver: expected #solver(<back-end-ident>, [key=value, ...]); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // KeyValue first (e.g. `#solver(method="gradient")`): the back-end
            // ident is required as the leading positional argument in v0.1.
            [PragmaArg::KeyValue { .. }, ..] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#solver: expected #solver(<back-end-ident>, [key=value, ...]); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // Catch-all for any remaining shape (e.g. multi-bare-arg lists
            // like `#solver(libslvs, argmin)` whose first arg isn't an
            // Ident — actually unreachable today because the first arm
            // handles `[Bare(Ident(_)), ..]`, but leave the catch-all as a
            // future-proofing safety net).
            _ => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#solver: expected #solver(<back-end-ident>, [key=value, ...]); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
        }
    }
}

/// Process the first well-formed module-level `#kernel(<ident>)` pragma: store
/// the user-declared kernel name on `module.kernel_pragma`. See
/// `docs/prds/pragmas.md` §4.
///
/// In v0.1, only `#kernel(occt)` is accepted; any other ident emits an
/// error-level diagnostic ("kernel '<other>' is deferred to v0.2; v0.1
/// supports only #kernel(occt)") to make the v0.1 limitation discoverable.
/// Per the storage-reflects-declared design decision, `kernel_pragma` mirrors
/// the user's verbatim ident regardless of validation outcome — downstream
/// tooling (doc generator, future kernel-registry lookup) needs the verbatim
/// name. Only malformed shapes (zero args, key=value-first, non-Ident bare
/// values) leave the field as None.
fn apply_kernel_pragma(parsed: &ParsedModule, module: &mut CompiledModule) {
    for pragma in &parsed.pragmas {
        if pragma.name != "kernel" {
            continue;
        }

        match pragma.args.as_slice() {
            // Happy path: `#kernel(occt)` — the only valid v0.1 form.
            [PragmaArg::Bare(PragmaValue::Ident(name))] if name == "occt" => {
                module.kernel_pragma = Some(name.clone());
            }
            // Non-occt ident: PRD §4 — error-level (not warning) so the v0.1
            // limitation is discoverable. Storage still mirrors the user's
            // verbatim ident, mirroring `apply_version_pragma`'s policy of
            // writing `module.declared_version` even when emitting an error
            // for a too-new version.
            [PragmaArg::Bare(PragmaValue::Ident(name))] => {
                module.diagnostics.push(
                    Diagnostic::error(format!(
                        "kernel '{name}' is deferred to v0.2; v0.1 supports only #kernel(occt)"
                    ))
                    .with_label(DiagnosticLabel::new(pragma.span, "deferred to v0.2")),
                );
                module.kernel_pragma = Some(name.clone());
            }
            // Zero args: `#kernel` with no arg list. Warning per PRD §4 (NOT
            // error — the error-level diagnostic is reserved for non-occt
            // idents to keep the v0.1 limitation discoverable). Leaves
            // `kernel_pragma` as None.
            [] => {
                module.diagnostics.push(
                    Diagnostic::warning("#kernel: expected #kernel(occt); ignored")
                        .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // Bare Number / Bool / String / Quantity as the (only) first arg.
            // The Ident cases were matched above; this arm catches the
            // remaining bare scalar variants. v0.1 deliberately rejects the
            // string form `#kernel("occt")` — only the Ident grammar is valid.
            [PragmaArg::Bare(_)] => {
                module.diagnostics.push(
                    Diagnostic::warning("#kernel: expected #kernel(occt); ignored")
                        .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // KeyValue first (e.g. `#kernel(name="occt")`): the kernel ident
            // is required as the leading positional argument in v0.1.
            [PragmaArg::KeyValue { .. }, ..] => {
                module.diagnostics.push(
                    Diagnostic::warning("#kernel: expected #kernel(occt); ignored")
                        .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            // Catch-all for any remaining shape (e.g. multi-bare-arg lists).
            // Currently unreachable because the prior arms cover every
            // single-arg shape; left as a future-proofing safety net.
            _ => {
                module.diagnostics.push(
                    Diagnostic::warning("#kernel: expected #kernel(occt); ignored")
                        .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
        }
    }
}
