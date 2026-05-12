# Audit: Pragma implementations for v0.1

**PRD path:** `docs/prds/pragmas.md`
**Auditor:** audit-pragmas
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 8 (10 WIRED)

## Top concerns

- **Stdlib `#no_prelude` invariant is opposite of what the PRD asks.** PRD §1 says "audit that the prelude `.ri` files in `crates/reify-compiler/stdlib/` all carry `#no_prelude` at the top." The actually-shipped invariant (per task 2492 and `stdlib_loader_tests.rs::prelude_modules_carry_no_prelude_pragma`) is "if and only if the module has zero inter-stdlib dependencies." Only 4 of 15 stdlib files carry `#no_prelude` (units, materials_mechanical, analysis, tolerancing). Adding it to the other 11 would silently strip their stdlib dependencies — a hard regression. This is the clearest DRIFT in the PRD.
- **`#kernel` scope quietly expanded from v0.1 to v0.2 wording.** PRD §4 specifies that non-`occt` idents emit error text `"kernel '<other>' is deferred to v0.2; v0.1 supports only #kernel(occt)"`. The implementation now accepts `{fidget, manifold, occt, openvdb}` without error and emits `"unknown kernel '<name>'; v0.2 supports {fidget, manifold, occt, openvdb}"` — different supported-set, different wording, different scope. This was driven by `docs/prds/v0_2/multi-kernel.md` §10.8 but the v0.1 pragmas PRD was never updated.
- **`declared_version` and `kernel_pragma` are write-only.** Both fields are populated correctly on `CompiledModule` but no production code reads them. PRD §Cross-cutting says "The doc generator (separate PRD) reads these fields verbatim" — that consumer doesn't exist yet (`reify-doc` has a generic `pragmas: Vec<PragmaDoc>` collection but no typed-field consumption / no "Module pragmas" section). Storage-only mechanisms are not actively broken, but they are unverified end-to-end.
- **`examples/integration_full_v01.ri` is missing `#precision(0.001m)`.** PRD §Acceptance line 108 says it "carries `#version(0.1)` and `#precision(0.001m)` at the top with no warnings." Source has only `#version(0.1)`. Minor; either the example or the acceptance criterion is stale.
- **`#precision` unit-resolution is fenced from the per-module `UnitRegistry`.** Only built-in `m/mm/cm/in` work; user-declared `unit ft = 0.3048m` is not honoured. This is honestly documented in code as a v0.2 deferral, but the PRD §2 wording ("a `Length` literal such as `0.001m, 1mm, 10um`") implies full Length-unit support. `10um` would in fact be rejected today (not in the built-in set).

## Mechanisms

### M-001: `Pragma` / `PragmaArg` / `PragmaValue` AST populated by parser

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-syntax/src/ts_parser.rs`; `crates/reify-syntax/tests/pragma_tests.rs`; PRD cites task #278 as done.
- **Blocks:** none
- **Note:** Parser-side infrastructure is the pre-existing dependency for everything else in this PRD; explicitly noted in PRD preamble.

### M-002: `KNOWN_BLOCK_PRAGMAS` / `MODULE_ONLY_PRAGMAS` classification

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/annotations.rs:266-269` declares `KNOWN_BLOCK_PRAGMAS = &["precision", "solver", "kernel"]` and `MODULE_ONLY_PRAGMAS = &["no_prelude", "version"]`; `validate_pragmas` at `annotations.rs:318` emits unknown/misplaced warnings.
- **Blocks:** none
- **Note:** Pre-existing infra; matches PRD preamble.

### M-003: `#no_prelude` shadowing of prelude resolution

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/compile_builder/pre_pass.rs:47-60` (`effective_prelude` returns `&[]` when `#no_prelude` is in `parsed.pragmas`); tests at `crates/reify-compiler/tests/pragma_compile_tests.rs:82` (`no_prelude_simple_structure_compiles_clean`) and `:96` (`no_prelude_suppresses_stdlib_units`).
- **Blocks:** none
- **Note:** Mechanism described in PRD §1 as already wired; verified.

### M-004: Positive-prelude regression test ("no #no_prelude → prelude applied")

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/tests/pragma_compile_tests.rs:107` (`without_no_prelude_stdlib_units_resolve`) and `:131` (`without_no_prelude_stdlib_traits_resolve`) — both explicitly described in source as the positive-prelude regression that PRD §1 asks for.
- **Blocks:** none
- **Note:** PRD §1 work item ("Add a regression test that compiling a non-prelude module without `#no_prelude` still gets the prelude") is satisfied.

### M-005: Stdlib `.ri` files carry `#no_prelude` at the top (PRD §1 polish)

- **State:** DRIFT
- **Failure mode:** F? (PRD describes a different shape than what landed — see audit-brief F-catalog DRIFT row)
- **Evidence:** PRD §1: "Audit that the prelude `.ri` files in `crates/reify-compiler/stdlib/` all carry `#no_prelude` at the top." Actual: only 4 of 15 carry it (`units.ri`, `materials_mechanical.ri`, `analysis.ri`, `tolerancing.ri`). The invariant enforced by `stdlib_loader_tests.rs::prelude_modules_carry_no_prelude_pragma` and task 2492 is "iff zero inter-stdlib dependencies" — adding `#no_prelude` to e.g. `materials_thermal.ri` would silently strip `MaterialSpec` resolution from `materials_mechanical.ri`.
- **Blocks:** none (the actual invariant ships)
- **Note:** PRD has not been reconciled with the bidirectional bootstrap-invariant decision (task 2492). Recommend updating PRD §1 to describe the actual zero-dep bootstrap criterion.

### M-006: `apply_module_pragmas` post-pass module + invocation site

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/module_pragmas.rs` (the file PRD §Cross-cutting requested); invoked from `crates/reify-compiler/src/lib.rs:362`. Initialized `None` fields on `CompiledModule` at `compile_builder/ctx.rs:179-185` and `compile_builder/defs_phase.rs:292-295`.
- **Blocks:** none
- **Note:** PRD §Cross-cutting + §Task slicing item 6 are both satisfied.

### M-007: `CompiledModule.default_tolerance: Option<f64>` field

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:240` declaration; populated at `module_pragmas.rs:317`.
- **Blocks:** none
- **Note:** Field shape matches PRD §2 (simpler `Option<f64>` rather than `Option<DimensionedValue>`).

### M-008: `#precision(<Length-literal>)` parse + store on module

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `module_pragmas.rs:247-384` (`apply_precision_pragma`); validation arms for legacy `float64`, bare-number, key=value, multi-dimension, non-finite, negative, zero, above-cap (1m). Test file `pragma_compile_tests.rs` has dozens of tests on this path.
- **Blocks:** none
- **Note:** "first wins on multi-#precision" + diagnostic severities match PRD §2 closely.

### M-009: `#precision` plumbing to OCCT tessellation tolerance

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_build.rs:1070` `effective_tessellation_tolerance(module)` returns `module.default_tolerance.unwrap_or(Engine::DEFAULT_TESSELLATION_TOLERANCE)`; consumed at `engine_build.rs:1306`. E2E coverage in `crates/reify-eval/tests/tolerance_wiring_e2e.rs` + unit tests at `engine_build.rs:3548-3712`.
- **Blocks:** none
- **Note:** PRD §2 §"Where the value lives" mentions `reify-geometry::DispatchPlanner`; actual wiring is via `Engine`'s tessellation pass. Functionally equivalent — minor PRD wording drift but not architectural.

### M-010: `#precision` plumbing through per-module `UnitRegistry` (user-declared units)

- **State:** PARTIAL
- **Failure mode:** F? (mechanism partial; documented gap)
- **Evidence:** `module_pragmas.rs:224-233` docstring: "Only the built-in SI/imperial length units understood by `unit_to_scalar` are accepted: `m`, `mm`, `cm`, `in`. The per-module/per-prelude `UnitRegistry`... is **not** queried here... Plumbing the prelude / in-module `UnitRegistry` into this pass is deferred to v0.2." PRD §2 wording ("`0.001m`, `1mm`, `10um`") implies the broader set — `10um` would emit an "unrecognised unit" warning today because `um` is not in the built-in fallback.
- **Blocks:** any v0.1 user who writes `#precision(10um)` or `#precision(1ft)` after a custom `unit ft = ...`. No tasks tracked.
- **Note:** Honestly documented as deferred to v0.2 in source; PRD does not call out the limitation.

### M-011: Block-level `#precision` ignored-with-warning

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `module_pragmas.rs:80-104` (`warn_block_level_precision`); walks 4 pragma-bearing containers (`templates`, `trait_defs`, `compiled_purposes`, `constraint_defs`); tests at `pragma_compile_tests.rs::block_level_kernel_pragma_on_*` and the precision analogues.
- **Blocks:** none
- **Note:** PRD §2 ignore-with-warning verbiage matches; explicit "container-set invariant" doc-comment block documents the four-container walk that all three block-warners share.

### M-012: `CompiledModule.declared_version: Option<(u16, u16)>` field + parse/validate

- **State:** PARTIAL
- **Failure mode:** F? (ORPHAN-adjacent — stored but unread)
- **Evidence:** `crates/reify-compiler/src/types.rs:249`; populated at `module_pragmas.rs:526`. Field is populated correctly, validation diagnostics match PRD §5 (too-new error, too-old warning, duplicate error). However, no production consumer reads `declared_version`: a workspace `grep -n 'declared_version' --include='*.rs'` returns only the field's declaration, initialisation, the post-pass writer, and tests. PRD §Cross-cutting says "the doc generator (separate PRD) reads these fields verbatim" — doc generator does not yet consume any typed pragma fields.
- **Blocks:** future doc generator; future v0.2 migration toolchain (PRD `docs/prds/v0_2/migration-toolchain.md`)
- **Note:** Compile-time validation is wired; downstream consumption is not. Functionally fine for v0.1 since the only required behavior was the diagnostic emission, but the "stored for round-trip" promise has no read side yet.

### M-013: `#version` multi-form acceptance (number vs string)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `module_pragmas.rs:423-516`: `Number(0.1)` parsed via Display split on '.'; `String("0.1")` parsed via strict 2-component split; catch-all warning for other shapes. Multiple tests: `version_pragma_with_number_form_zero_one_sets_declared_version`, `version_pragma_with_string_form_zero_one_sets_declared_version`.
- **Blocks:** none
- **Note:** A subtle quirk noted in source: `#version(0.10)` and `#version(0.1)` are identical at the lex layer (both `f64 == 0.1`), so users needing MINOR=10 must use the string form. Documented in code (`module_pragmas.rs:426-432`) but not in the PRD.

### M-014: `CompiledModule.solver_pragma: Option<SolverPragma { name, options }>` field + parse

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:261, 295-300` (struct decl with `BTreeMap<String, PragmaValue>` for options as PRD §3 specifies); populated at `module_pragmas.rs:636`.
- **Blocks:** none
- **Note:** Storage-reflects-declared policy (back-end name stored even when unknown, with warning) explicitly documented; mirrors `#version` policy.

### M-015: `#solver` runtime dispatch (named-solver registry lookup)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_admin.rs:537+` (`resolve_solver_for_module` consults `module.solver_pragma.name` against registry, falls through to default on miss); `crates/reify-eval/src/engine_eval.rs:1785, 2089, 2583` invocation sites; e2e tests in `crates/reify-eval/tests/solver_pragma_dispatch.rs`.
- **Blocks:** none
- **Note:** PRD §3 §"Where the value lives" satisfied — `reify-eval` solver dispatch routes through `solver_pragma` before falling back to default.

### M-016: `CompiledModule.kernel_pragma: Option<String>` field + parse

- **State:** PARTIAL
- **Failure mode:** F? (ORPHAN-adjacent — stored but unread)
- **Evidence:** `crates/reify-compiler/src/types.rs:275`; populated at `module_pragmas.rs:716, 733`. Field is populated; validation diagnostics fire. However, the only production read is during initialisation (`None`). DispatchPlanner / OCCT kernel binding does not consult `kernel_pragma`. PRD §4 §"Where the value lives" anticipates this: "v0.1 dispatch always uses OCCT regardless of value; the field is for round-tripping and for the doc tool." Doc tool doesn't consume it yet — see M-012.
- **Blocks:** future multi-kernel dispatch (`docs/prds/v0_2/multi-kernel.md`); future doc generator.
- **Note:** Same shape as `declared_version` — validation wired, downstream consumption empty.

### M-017: `#kernel` validation scope drift (v0.1 vs v0.2)

- **State:** DRIFT
- **Failure mode:** F? (mechanism exists but PRD describes a different shape than what landed)
- **Evidence:** PRD §4: `#kernel(<other>)` is error with text `"kernel '<other>' is deferred to v0.2; v0.1 supports only #kernel(occt)"`. Actual: `module_pragmas.rs:47` declares `KNOWN_V02_KERNELS = &["fidget", "manifold", "occt", "openvdb"]` — all four are accepted silently; only non-list idents error with `"unknown kernel '<name>'; v0.2 supports {fidget, manifold, occt, openvdb}"`. Wording, supported-set, and scope (v0.1-only vs anticipating v0.2) all differ.
- **Blocks:** none
- **Note:** The expansion is driven by `docs/prds/v0_2/multi-kernel.md` §10.8, but the v0.1 pragmas PRD has not been reconciled. Users today who write `#kernel(fidget)` will get NO diagnostic at all — and v0.1 will still silently use OCCT (M-016 confirms `kernel_pragma` is not read by dispatch). PRD-stated user discoverability of the v0.1 limitation is therefore weakened: only typos (`#kernel(occxx)`) surface; well-formed-but-v0.2-only kernels are accepted with no signal.

### M-018: `examples/integration_full_v01.ri` carries `#version(0.1)` AND `#precision(0.001m)` (acceptance criterion)

- **State:** PARTIAL
- **Failure mode:** F? (acceptance criterion partially met)
- **Evidence:** `examples/integration_full_v01.ri:22` has `#version(0.1)`; no `#precision(0.001m)` in the file (verified by grep). PRD §Acceptance: "examples/integration_full_v01.ri carries `#version(0.1)` and `#precision(0.001m)` at the top with no warnings."
- **Blocks:** none
- **Note:** Either the example needs `#precision(0.001m)` added or the PRD line softened. Trivial to fix but tracked nowhere.

## Cross-PRD breadcrumbs

- **`docs/prds/v0_2/multi-kernel.md`** — M-017 (kernel scope drift) and M-016 (unread `kernel_pragma`) are gated by this v0.2 PRD. The v0.1 PRD already accepts v0.2 kernel idents in code but doesn't dispatch them, creating a half-finished surface that a v0.2 audit should catch.
- **`docs/prds/v0_2/migration-toolchain.md`** — M-012 (`declared_version` write-only) gates on this v0.2 PRD; the §14.2 auto-migration tool is explicitly deferred per the v0.1 PRD non-goals.
- **`docs/prds/v0_2/per-purpose-tolerance.md`** — likely consumer for block-level `#precision` (currently warned-and-ignored); related to M-011.
- **Doc generator PRD (referenced in PRD §Cross-cutting but path not given)** — primary consumer for `declared_version`, `solver_pragma`, `kernel_pragma`, `default_tolerance` typed fields. Without that PRD landing, M-012 and M-016 stay write-only.
- **GR-001 (struct-ctor runtime eval)** — not applicable to this PRD; no pragma value type depends on struct-constructor evaluation.

## Notes for Phase 3

- The pragmas PRD is one of the most-completed v0.1 PRDs audited. 10/18 mechanisms fully WIRED, with rich diagnostic-shape test coverage. Most gaps are downstream-consumer absences (doc generator) or PRD-vs-code wording drift, not missing implementation.
- The DRIFT in M-005 (stdlib `#no_prelude`) and M-017 (`#kernel` v0.2 scope) suggest a recurring pattern: implementation evolves to better-informed designs (bidirectional bootstrap invariant; anticipating v0.2 kernels) but the PRDs ossify at the earlier point. A PRD-update gate on architect-design-decision tasks would catch these.
- The "stored-for-round-trip, doc-tool-will-read-later" pattern (M-012, M-016) is a soft form of FICTION: the PRD asserts a consumer exists ("the doc generator reads these fields verbatim") but the consumer doesn't. Compared to GR-001 it's less severe (the writer side is correct and the round-trip works in-process), but it's worth tracking as a pattern.
