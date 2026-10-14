# Capability manifest — step-assembly-import

PRD: `docs/prds/v0_6/step-assembly-import.md`. Evidence bound 2026-09-28/30 against main
`5340545275`; symbol evidence grep-verified in the authoring session; kernel evidence compiled
and run (`step-assembly-import.evidence/`); grammar probes run directly (`tree-sitter parse
--quiet`, session-isolated cache, captured exit codes); compiler/eval probes run against
`target/debug/reify` (2026-09-25 build). Adversarial gate review (opus, 2026-09-28) folded in.
Re-walked by the D3 decompose workflow twice (run ids in the sidecar header). Round 1 blocked on
45 findings, all folded into the PRD (§3.3, D19, D20, C6, C7, §6, §8). Round 2 blocked on 42:
eight leaves (α, γ, ε, ζ, λ, ν, ο, π) have no α probe vector at all (kernel tests, GUI, LSP,
docs) and were UNENUMERATED both rounds; a further set are prover mis-bindings (match keys the
harness ignores; the stale default `target/release/reify`); the 14 real findings — the B4 guard
vacuity, integer-literal `vec3` undef, corpus not CI-run, the `_GUI_COUPLED_RI_FIXTURES` row,
order-dependent unknown-override diagnostic, bare `identity`, path-typed AST override, the
flush measurement, `relate` having no chunk (#5446), the corpus gate row, the full §9 drift list
— were folded on 2026-09-30. Disposition recorded honestly: **BLOCKS** on harness reach, not on
a standing falsification; the batch was filed on the folded PRD.
Machine-readable twin: `step-assembly-import.capability-manifest.yaml`.

Binding vocabulary: **producer-self** = the leaf itself delivers the capability and its own
signal observes it; **producer-upstream** = a hard `add_dependency` edge to a named task delivers
it; **substrate** = present on main today, cited by symbol.

## α — OCCT XDE reader + product tree + units (absorbs #4289)

- `xde-reader-substrate` — substrate (evidence): `reader.cpp` reads `test_assembly.step` through
  `STEPCAFControl_Reader` with names, locations, shared products and solid counts
  (`reader_output_default_units.txt`). PASS.
- `unit-conversion-call` — substrate (evidence): `Interface_Static::SetCVal("xstep.cascade.unit",
  "M")` converts to metres (`reader_output_metres.txt`); `SetSystemLengthUnit` does not. PASS.
- `link-toolkits-absent-today` — deliberate absence: `crates/reify-kernel-occt/build.rs` lists no
  `TKXCAF`/`TKLCAF`; both `.so.7.8` present on the host; α is the producer. PASS (producer-self).
- `dual-xcaf-in-process` — hazard acknowledged: `libgmsh.so` pulls `TKXCAF`/`TKLCAF` 7.9 into
  every reify process; α's test runs inside the gmsh-linked workspace binary, and
  `scripts/check-manifold-deps.sh` gains the toolkits. PASS (producer-self; manual).
- `reader-under-export-mutex` — substrate: `g_step_export_mutex` (`occt_wrapper.cpp`) documents
  process-global XSTEP state; the reader takes it. PASS (producer-self).

## β — `step_solid`/`step_body`, document cache, path threading, provenance field

- `geometryop-extension-guarded` — substrate: `GEOMETRY_OP_DESCRIPTORS` completeness test and
  the compile-dispatch cross-check (`all_dispatch_functions_accounted_for`,
  `crates/reify-compiler/src/geometry.rs`) fail until a new variant is fully wired. PASS.
- `occt-execute-arm` — substrate: `OcctKernel::execute` is an exhaustive match, so the arm cannot
  be forgotten. PASS (producer-self).
- `document-cache-single-parse` — producer-self: no file-scoped cache exists in the kernel; B3's
  build-count assertion is the observation. PASS (manual).
- `reader-restores-unit-static` — producer-self: B4 asserts `xstep.cascade.unit` reads back as
  its pre-read value after `step_solid` (test-only getter); the writer is immune (per-model
  units, #6186; verified by a C++ replica in round 2), so no writer canary is claimed. PASS.
- `source-hash-rejection` — G6 branch 4, producer-self: `E_STEP_SOURCE_CHANGED` is delivered and
  observed by β's own fixture. PASS.
- `design-relative-path-first-convention` — deliberate absence: field-import paths are verbatim
  and `ParsedModule` carries no filesystem path; β threads it. PASS (producer-self).
- `provenance-hash-field` — deliberate absence: `Provenance` (`io.ri`) has no hash field; β adds
  `source_hash`, ν's BOM row reads it. PASS (producer-self).

## γ — Foreign-module codec, provider trait, generator

- `crates-absent-today` — deliberate absence: no `reify-foreign-module` / `reify-step-import`
  crate exists; γ creates both (compiler depends only on the kernel-free one). PASS.
- `orient-basis-constructor` — substrate: `orient_basis` (`crates/reify-stdlib/src/orientation.rs`,
  `geometry.rs`) with orthonormality guards; the generator orthonormalises before emitting. PASS.
- `identity-preservation` — producer-self: golden tests over moved-within, moved-beyond, removed,
  added occurrences. PASS (manual).
- `improper-placement-rejection` — G6 branch 4, producer-self: `E_STEP_IMPROPER_PLACEMENT`
  observed on a det<0 fixture placement. PASS.

## δ — String-path import grammar + AST

- `string-import-absent-today` — grammar-fixture (deliberate failure): `import "vendor/container.step"
  as container` FAILS `tree-sitter parse` (exit 1, 2026-09-28); δ is the grammar producer. PASS.
- `dotted-import-regression-floor` — grammar-fixture: `import vendor.container as container` parses
  today (exit 0). PASS.
- `lezer-mirror-and-pin` — substrate: `gui/src/editor/reify.grammar` carries a legacy string import
  arm; `reifyGrammarCorpus` `EXPECTED_CLEAN` pins it; δ updates both. PASS (producer-self).
- `corpus-pinned-by-rust-test` — substrate: `test/corpus` is not CI-run; a Rust `include_str!`
  test in `tree-sitter-reify/tests/` pins the case; the `EXPECTED_CLEAN` pin needs a
  `_GUI_COUPLED_RI_FIXTURES` row in `scripts/verify.sh`. PASS.
- `no-diagnostics-in-lowering` — D20: `lower_import`'s only channel renders as a bare
  `Parse error:`; δ emits no diagnostics; every import rejection is ε's. PASS.

## θ — `exclude <path>` in specialization bodies

- `exclude-absent-in-spec-body-today` — grammar-fixture (deliberate failure): `sub b : T { exclude
  x }` FAILS parse (2026-09-28); the same token exists only inside `derived_body`. PASS.
- `disposition-semantics-source` — substrate: assembly-derivation-toolbox D5 defines unresolved
  disposition paths; θ generalises the diagnostic to `E_DISPOSITION_UNKNOWN_PATH` and #6618
  consumes the mechanism (§7). PASS.
- `disposition-rejections` — G6 branch 4, producer-self: `E_DISPOSITION_UNKNOWN_PATH`,
  `E_REFERENCE_TO_EXCLUDED` observed by θ's fixtures. PASS.
- `export-walk-site` — substrate: the surfacing/export walk (`surface_export_bodies`, sub-placement
  #3905) is the single site an exclusion filters. PASS.

## ε — Provider wiring, freshness, locked mode, CLI host

- `provider-seam-absent-today` — deliberate absence: `ModuleDag::compile_module` has no trait or
  callback; ε adds `ForeignModuleProvider` resolution. PASS (producer-self).
- `multi-file-build-upstream` — producer-upstream: resolution-unification β #5516 + γ #5517
  (hard edges). `reify build` compiles one file today (probe 2026-09-28). PASS (with edges).
- `qualified-references-upstream` — producer-upstream: stdlib-namespace ν #5505 (hard edge).
  `import lib as l; l.Plate()` is an unknown structure today (probe 2026-09-28). PASS (with edge).
- `freshness-and-locked` — producer-self: `E_STEP_MODULE_STALE`, `E_STEP_MODULE_MISSING`,
  `E_GENERATED_MODULE_EDITED`, `REIFY_GENERATED_LOCKED`/`--locked` delivered and observed. PASS.
- `regeneration-diagnostic` — producer-self: `I_STEP_MODULE_REGENERATED` carries the delta. PASS.
- `import-rejections` — G6 branch 4, producer-self: `E_STEP_IMPORT_NOT_FOUND`,
  `E_STEP_IMPORT_PATH_COLLISION` observed on their own fixtures. PASS.
- `format-and-alias-rejections` — G6 branch 4, producer-self: `E_IMPORT_UNSUPPORTED_FORMAT`
  and `E_STEP_IMPORT_NEEDS_ALIAS` are coded compile diagnostics emitted in the module-loading
  path before the path is resolved; observed on stderr by ε's fixtures. PASS.

## ζ — GUI and LSP host policy

- `lsp-multifile-upstream` — producer-upstream: resolution-unification ζ #5520 (hard edge). PASS.
- `gui-watcher-extension` — deliberate absence: the GUI watcher is non-recursive and `.ri`-only
  (`watcher_ignores_non_ri_file_changes`); ζ registers foreign source paths. PASS (producer-self).

## η — Geometry overrides take effect; dotted override paths in specialization bodies

- `dotted-name-absent-today` — grammar-fixture (deliberate failure): a dotted
  `param_assignment.name` FAILS parse (2026-09-28). PASS (producer-self).
- `override-threading-upstream` — producer-upstream: instantiation-value-flow #6592 (hard edge)
  supplies the recursive overlay and re-realization trigger; it does not replace a `Solid`
  param's default-compiled realization ops (§3.3). PASS (with edge).
- `solid-param-op-substitution` — producer-self: on main both the specialization arm and the
  ctor-arg arm of a `Solid` param override are silently ignored (probe 2026-09-30: volume
  1e-6 m³, 0 `CYLINDRICAL_SURFACE`); η compiles the override expression's ops and substitutes
  them on both arms. B11(a) is the positive premise (volume ≈ 7.854e-4 m³). PASS (manual).
- `scope-frame-transport-upstream` — producer-upstream: placement-relations-belt β #5436
  (carried frames, §7.1.5) for D19's transport into the descendant's local frame. PASS.
- `order-independent-unknown-override` — producer-self: a forward-declared child's unknown
  override is silently accepted in release / panics in debug today (`entity.rs` Case 1); η's
  diagnostic fires in both orders; the AST carries the path as structured data. PASS.
- `member-path-resolver-landed` — substrate: `resolve_member_path`
  (`crates/reify-compiler/src/member_path.rs`, #5424) walks dotted chains with per-hop types. PASS.
- `override-rejections` — G6 branch 4, producer-self: the unknown-hop diagnostic names the hop on
  stderr; a duplicate path keeps the pinned first-wins warning (task 4694), no new rejection. PASS.

## ι — Dotted overrides in derived bodies

- `derived-body-name-absent-today` — grammar-fixture (deliberate failure): a dotted
  `derived_param_assignment.name` FAILS parse. PASS (producer-self).
- `derivation-lowering-upstream` — producer-upstream: #6616 (lowering), #6617, #6618 (planes;
  deferred) — hard edges. PASS (with edges).

## κ — `relate` scope-level datum operands

- `operand-shape-today` — substrate: `decode_operand` (`crates/reify-eval/src/relate_solve.rs`)
  accepts only `<sub>.<member>`; `realize_operand_datums` builds the structure standalone. κ widens
  the fixed side. PASS (producer-self).
- `scope-posed-reads-upstream` — producer-upstream: placement-relations-belt β #5436 (§7.1.5:
  sub-member geometry reads are scope-posed, so the datum needs no transport and reads no
  `.world_frame`; the pose-cycle rule is untouched) and uniform-member-access δ #5427 — hard edges. PASS.
- `discriminating-expected-plane` — G6: B13 measures signed point-to-plane distance and normal
  parallelism against an independently computed expected plane on a rotated nested sub, reading
  the solved seat plane via `in_frame(bracket.seat_plane, bracket.world_frame)`; never origin
  coincidence. Selector directions are real literals. PASS.
- `pose-bound-floor` — floor: existing relate e2e asserts 1e-6 m (`relate_solve_e2e.rs`); B13 uses
  the same bound. PASS (bound = precedent, not tighter).

## λ — Cross-module completion + aliased go-to-definition

- `completion-single-file-today` — substrate: `completion()` builds its context from the current
  document only (`crates/reify-lsp/src/server.rs`). PASS (producer-self).
- `aliased-goto-def-absent` — substrate: `goto_def.rs` maps `ImportKind::Aliased` to `None`. PASS
  (producer-self).
- `qualified-references-upstream` — producer-upstream: #5505 (hard edge). PASS.

## ν — Container site integration gate + scale

- `report-bom-exists` — substrate: `reify report --bom` (`cmd_report`) and
  `Engine::build_bom_report` emit `Input` provenance rows. PASS.
- `provenance-hash-upstream` — producer-upstream: β (`Provenance.source_hash`). PASS (with edge).
- `examples-smoke-registration` — substrate: `examples_smoke.rs` has `SKIP_SET` for multi-file
  examples; ν registers or uses the multi-file path. PASS.
- `container-fixture` — producer-self: the extended evidence writer produces the container-shaped
  and 500-occurrence fixtures. PASS (manual).
- `end-to-end-reachability` — G6 branch 3: B15 needs β, ε, η, θ, κ, #5427, #6598, #5518 — all are
  upstream edges of ν, none downstream. PASS.

## ξ — Docs-truth bundle

- `chunk-files-exist` — substrate: `crates/reify-mcp/src/tools/chunks/{syntax,structures,
  geometry,connect}.md`, `examples/best_practices/INDEX.md`, `.claude/skills/reify-design/SKILL.md`.
  PASS.
- `stdlib-reference-drift` — substrate: `docs/reify-stdlib-reference.md` §9 sketches a
  `result : Structure` param `io.ri` lacks; ξ reconciles only the `STEPInput`/`Provenance` rows
  (the `DisplayStyle.color` / `STEPOutput.path` / `PointCloudInput` drift is named in ο). PASS.
- `signature-check-is-registry-diff` — G6: "every documented signature compiles" is vacuous
  (ctor kwargs are silently dropped, §3.3); ξ diffs documented signatures against `io.ri` and the
  compiler registries. PASS (manual).
- `relate-docs-upstream` — producer-upstream: placement-relations-belt ν #5446 owns the base
  at-auto + relate chunk docs (`relate` has no chunk today); ξ extends them. PASS.
- `corpus-gate-row` — substrate: `harness_corpus_gates/best_practices_constraint_gate.rs` walks
  `best_practices/` kernel-less; the exemplar carries its `EXPECTED_INDETERMINATE` row. PASS.
- `exemplar-single-file` — substrate: `examples_smoke` compiles corpus files single-file and
  forbids `SKIP_SET` for `best_practices/`; the exemplar (`modify_assembly.ri`) uses
  native-structure idioms only. PASS.

## ο — Companion corrections

- `target-docs-exist` — substrate: the sibling PRDs exist at the cited paths, including
  `instantiation-value-flow.md` (η/IVF ownership note). PASS.

## π — PRD close

- `freeze-header-precedent` — substrate: `docs/prds/v0_6/data-carrying-enums.md`,
  `docs/prds/kernel-seam-contracts.md` headers. PASS (manual).
