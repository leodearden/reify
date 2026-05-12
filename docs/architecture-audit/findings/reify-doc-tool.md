# Audit: `reify doc` documentation generator

**PRD path:** `docs/prds/reify-doc-tool.md`
**Auditor:** audit-reify-doc-tool
**Date:** 2026-05-12
**Mechanism count:** 24
**Gap count:** 17

## Top concerns

- **The PRD's preamble claim that `TopologyTemplate` / `CompiledFunction` / `TraitDef` / `EnumDef` "all carry `doc: Option<String>`" is FICTION.** None of these compiled types has a `doc` field (verified `crates/reify-compiler/src/types.rs:21,105,136,503,747,1118,1133,1163` and `crates/reify-types/src/expr.rs:208`, `crates/reify-types/src/traits.rs:7,86`). The AST (`reify_syntax::Declaration` variants in `crates/reify-syntax/src/lib.rs:37,63,154,167,261,543,556,572,683`) carries `doc`, and LSP reads doc directly off the AST (`crates/reify-lsp/src/analysis.rs:166,179,182,185,187,188`), but the compiler drops it at lowering time. Task #215 ("propagation to compiled output") is marked `status=done` in fused-memory yet the field doesn't exist. Any formatter trying to read `template.doc` would not compile. This is a precise mirror of GR-001's pattern: tasks-marked-done that don't deliver the runtime contract subsequent PRDs assume.
- **The central lowering step `build_doc_model(&CompiledModule, &str) -> DocModel` is absent.** The crate scaffold (`reify-doc`) and all three formatters exist and pass tests (~3,500 LOC including snapshots), but no compiler→DocModel function exists in `reify-doc-build/src/lib.rs` (only `cross_refs` lives there). The CLI uses a `minimal_doc_model_from_compiled` stub that returns a `DocModel` with one `ModuleDoc { path, ..Default::default() }` and nothing else (`crates/reify-cli/src/main.rs:319-341`, `:519`). Snapshot tests inline a hand-built fixture rather than compile the example (`crates/reify-doc/tests/fmt_html_tests.rs:1937-1951` and the matching markdown tests). End-to-end the PRD's `reify doc examples/integration_full_v01.ri --format markdown` produces an empty document.
- **The DocModel schema has already drifted from the PRD's spec.** PRD §"Common: structured doc model" describes `DocModel { module, cross_refs }` with `ItemDoc { name, kind, visibility, type_params, trait_bounds, doc, ... params/lets/ports/constraints/sub_components/realizations/... }`. The implemented schema is `DocModel { modules: Vec<ModuleDoc> }` (multi-module wrapper; absent from PRD), `ItemDoc { header, kind }` with header/kind flatten via serde, and the `cross_refs` field is split between `model::ModuleCrossRefs` (per-module outgoing refs — invented post-PRD) and `cross_refs::CrossRefs` (the PRD's two indices). Several PRD-mandated ItemDoc fields are not modelled at all (`type_params`, `trait_bounds`, `lets: Vec<LetDoc>`, `realizations` exists, but no `visibility` separate from `is_pub`). PortDoc lacks `role`; ParamDoc lacks `dimension`. CLI consumers who parse JSON against the PRD will break.
- **HTML formatter exists but the CLI doesn't use it.** `reify_doc::fmt_html::render_html` is fully implemented (750 LOC, tested) but `cmd_doc` in `reify-cli` routes through `render_html_stub` that wraps markdown in `<pre>` (`main.rs:368-391,519,521-528`). The `--format html` acceptance criterion ("produces a single self-contained file that renders in a browser") is technically satisfied by the stub but produces nothing like what the PRD describes.

## Mechanisms

### M-001: `reify-doc` crate scaffold with `DocModel` types

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-doc/Cargo.toml`; `crates/reify-doc/src/lib.rs:1-13`; `crates/reify-doc/src/model.rs:1-1200` (full type graph + 11 serde round-trip tests); workspace member entry in root `Cargo.toml`.
- **Blocks:** none
- **Note:** Task slicing #1 ("crate scaffold + `DocModel` types") landed. The crate is serde-only with no compiler dep, matching the PRD's "pure-data crate-internal" intent.

### M-002: DocModel field shape matches PRD

- **State:** DRIFT
- **Failure mode:** F5 (PRD describes a different shape than what landed)
- **Evidence:** PRD §"Common: structured doc model" describes `DocModel { module: ModuleDoc, cross_refs: CrossRefs }` (single module) with `ItemDoc { name, kind, visibility, type_params, trait_bounds, doc, annotations, meta, params, lets, ports, constraints, sub_components, realizations }`. Implementation in `crates/reify-doc/src/model.rs:12-14` has `DocModel { modules: Vec<ModuleDoc> }` (plural) with no top-level `cross_refs`; cross-refs is split between per-module `ModuleCrossRefs` and a separate `cross_refs::CrossRefs`. `ItemDoc` lacks `visibility` (only `is_pub`), `type_params`, `trait_bounds`, `lets: Vec<LetDoc>` (no `LetDoc` type at all). `ItemKind::Function` carries a single `signature: String` rather than a structured surface.
- **Blocks:** any downstream consumer parsing JSON against the PRD's schema
- **Note:** Multi-module wrapper is a forward-compat move (the PRD mentions v0.2 stdlib bundling) but is not in v0.1 scope per the PRD. The "no LetDoc" decision elides §"Doc on `let` cells" entirely.

### M-003: PortDoc with `role` field

- **State:** DRIFT
- **Failure mode:** F5
- **Evidence:** PRD §"Common" specifies `PortDoc { name, kind, role, port_type, doc }`. Implementation `crates/reify-doc/src/model.rs:84-93` has `PortDoc { name, direction, type_name, members: Vec<String> }` — no `role`, no `doc`, no `kind` (replaced by `direction`), plus an added `members` array.
- **Blocks:** none (no downstream consumer yet)
- **Note:** The compiled `CompiledPort` (`types.rs:617-624`) carries `direction: PortDirection`, `type_name`, `members: Vec<ValueCellDecl>`, `constraints`, `frame_expr` — but no "role" attribute. So the PRD's `role` field has no compiler source to render from. Either the PRD names a non-existent field or `role` is a synonym for `direction`.

### M-004: ParamDoc `dimension` field

- **State:** DRIFT
- **Failure mode:** F5
- **Evidence:** PRD §"Markdown (GitHub-flavored)" specifies a Parameters table with columns: Name / Type / **Dimension** / Default / Description, and notes "Dimension is rendered as the canonical SI dimension string (`length`, `mass · length / time^2`)". Implementation `crates/reify-doc/src/model.rs:68-79` has `ParamDoc { name, doc, type_repr, default_repr, annotations }` — no `dimension`. `fmt_markdown.rs` emits a four-column table without a Dimension column.
- **Blocks:** none
- **Note:** Building dimension strings would need to inspect `ValueCellDecl.cell_type` and decompose to SI base dimensions — `reify-types` has `DimensionVector` (used on `CompiledUnit`) so the infrastructure exists, but the model field that would carry the rendered string does not.

### M-005: `build_doc_model(&CompiledModule, &str) -> DocModel` lowering

- **State:** FICTION
- **Failure mode:** F2 (PRD assumes mechanism exists; code provides nothing — the function does not exist)
- **Evidence:** PRD §"Crate structure" lines: `lib.rs           // pub fn build_doc_model(&CompiledModule, &str) -> DocModel`. No such function in `crates/reify-doc-build/src/` (only `cross_refs.rs`); `reify-doc-build/src/lib.rs:1-11` says "natural home for future compiler→doc-model transforms (e.g., `build_doc_model`, formatter/CLI lowering stages)". CLI uses placeholder `minimal_doc_model_from_compiled` returning effectively-empty `DocModel` (`reify-cli/src/main.rs:319-341,519`).
- **Blocks:** end-to-end CLI output. PRD task slicing #2 ("build_doc_model over CompiledModule") not landed.
- **Note:** Even if the function were a one-day walk, it cannot be written as-specified because the compiled types lack the `doc` fields the PRD assumes — see M-006.

### M-006: Doc-string propagation through compilation

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no runtime backing)
- **Evidence:** PRD preamble claim: "`TopologyTemplate`, `CompiledFunction`, `TraitDef`, `EnumDef` all carry `doc: Option<String>`". Reality:
  - `TopologyTemplate` (`crates/reify-compiler/src/types.rs:503-576`) — no `doc` field
  - `CompiledTrait` (`types.rs:21-37`) — no `doc` field
  - `CompiledFunction` (`crates/reify-types/src/expr.rs:208-227`) — no `doc` field
  - `TraitDef` (`crates/reify-types/src/traits.rs:86-95`) — no `doc` field
  - `EnumDef` (`crates/reify-types/src/traits.rs:7-12`) — no `doc` field
  - `CompiledField`, `CompiledPurpose`, `CompiledUnit`, `CompiledTypeAlias`, `CompiledConstraintDef`, `CompiledPort`, `ValueCellDecl`, `SubComponentDecl`, `RealizationDecl` — none have `doc` fields
  - `TopologyTemplate` construction at `entity.rs:2189-2218` omits doc entirely
  - AST `reify_syntax::Declaration::*.doc: Option<String>` IS populated (parser side, task #214 actually wired); LSP reads from AST not compiled output (`reify-lsp/src/analysis.rs:166,179,182,185,187,188`).
- **Blocks:** every "doc rendering" mechanism downstream. Tasks #214/#215/#216/#217 marked `done` in fused-memory but #215 is unsatisfied. This is structurally a sibling of GR-001 (task marked done; contract subsequent PRDs assume is absent).
- **Note:** Without this, `build_doc_model` cannot render ItemDoc.doc, ParamDoc.doc, PortDoc.doc, ConstraintDoc — every doc-comment cell in the PRD's tables and bullets is unfillable. The fix is either (a) re-open task #215 and add `doc` to ~10 compiler-output structs and their construction sites, or (b) re-architect the doc tool to walk AST + compiled module together. The PRD explicitly forbids (b): "The doc model is read directly from the existing `CompiledModule` ... The formatters never touch the AST — only the compiled output and (optionally) the original source text for verbatim constraint rendering by span."

### M-007: Markdown formatter (`render_markdown`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-doc/src/fmt_markdown.rs` (1,019 LOC); `crates/reify-doc/tests/fmt_markdown_tests.rs` with snapshot `tests/snapshots/integration_full_v01.single.md` and 7 split-mode snapshots; `crates/reify-cli/src/main.rs:530-559` wires `--format markdown` end-to-end through the stub model.
- **Blocks:** none on its own — but with empty input model (M-005) it emits empty output.
- **Note:** Task slicing #4 ("Markdown formatter") landed. The `--split` variant is wired; cross-ref index integration too (`NameIndex::unique_resolve`).

### M-008: HTML formatter (`render_html`)

- **State:** PARTIAL
- **Failure mode:** F4 (mechanism implemented but not integrated)
- **Evidence:** `crates/reify-doc/src/fmt_html.rs:149` `pub fn render_html(model: &DocModel, cross_refs: Option<&CrossRefs>) -> String`; 750 LOC with embedded stylesheet; snapshot test against integration_full_v01 fixture passes. BUT `reify-cli` does NOT call it: `main.rs:368-391` defines `render_html_stub` (markdown wrapped in `<pre>`) and `main.rs:521-528` routes html to the stub. The `TODO(post-2361): replace with reify_doc::fmt_html::render_html when task 2359 lands` comment is stale — `fmt_html.rs` is fully landed.
- **Blocks:** "produces a single self-contained file (no external resource references) that renders in a browser" acceptance criterion. The stub satisfies it textually (it does produce a single HTML file) but the rich rendering specified in the PRD is bypassed.
- **Note:** Task slicing #5 ("HTML formatter") landed in the library but the CLI integration was never updated. Trivial one-line fix in `cmd_doc`, gated on whether the model passed in actually carries any content (currently it doesn't — M-005).

### M-009: JSON formatter (`render_json`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-doc/src/fmt_json.rs` (174 LOC including tests); `reify-cli/src/main.rs:521-528` wires `--format json` end-to-end; `--compact` flag honoured.
- **Blocks:** none — but ditto M-005, output is empty.
- **Note:** Task slicing #6 ("JSON formatter") for the formatter portion landed; consumers can rely on snake_case kind tags (`type_alias`, `constraint_def`) regression-tested at `fmt_json.rs:117-173`.

### M-010: `reify doc` CLI subcommand

- **State:** PARTIAL
- **Failure mode:** F4 (CLI flag parsing + plumbing exists but downstream operations are stubbed)
- **Evidence:** `crates/reify-cli/src/main.rs:74` dispatches `"doc" => cmd_doc`; `:393-690` implements arg parsing, format/split/compact validation, `-o` plumbing, stdout fallback, exit codes 0/1/2. Tests `crates/reify-cli/tests/cli_doc.rs` (15+ test functions) cover usage errors and basic stdout/stdin paths.
- **Blocks:** none directly — but content quality depends on M-005, M-008.
- **Note:** Task slicing #6 ("CLI subcommand") landed for the CLI shell; the lowering glue is the missing piece. `--split` is markdown-only and validates this before parsing (`main.rs:484-488`).

### M-011: Cross-ref index — trait → conformers

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-doc/src/cross_refs.rs:59-73` defines `CrossRefs::trait_to_conformers: BTreeMap<String, Vec<String>>`; `crates/reify-doc-build/src/cross_refs.rs:22-54` implements `build_cross_refs(&[TopologyTemplate]) -> CrossRefs` walking `template.trait_bounds`. Tests at `:64-238` cover empty, multi-trait, nested, dedup, and order-independence.
- **Blocks:** none
- **Note:** Task slicing #3 ("Cross-ref index") landed. BTreeSet-backed accumulation gives deterministic, deduplicated output.

### M-012: Cross-ref index — entity → containers

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Same files as M-011; walks `template.sub_components[*].structure_name`. Same tests cover nested and dedup.
- **Blocks:** none
- **Note:** Both indices live in the doc-build crate (not reify-doc) so the type crate remains compiler-free, mirroring the PRD's note that the model crate should be embeddable.

### M-013: Cross-ref rendering in markdown / HTML

- **State:** PARTIAL
- **Failure mode:** F4
- **Evidence:** `fmt_markdown.rs` accepts `Option<&CrossRefs>` and renders "Conforms to" / "Used by" bullets when provided. CLI does NOT pass cross_refs through: `main.rs:531-538` says `TODO(post-2361): once reify_doc_build::build_doc_model lands, wire reify_doc_build::cross_refs::build_cross_refs(&compiled.templates) here and pass Some(&xrefs) instead of None`. So in production the cross-ref index is computed nowhere and rendered nowhere.
- **Blocks:** "Used by" and "Implementations" lists in generated docs.
- **Note:** Trivial wiring fix once M-005 exists; the CLI already references `compiled.templates`.

### M-014: Source-text span slicing for constraint expressions

- **State:** PARTIAL
- **Failure mode:** F1 / F2 (compiled side has spans; doc model has them as `line: Option<u32>` only)
- **Evidence:** PRD §"Source text rendering for constraints" mandates "v0.1 renders them by re-slicing the source text using the span attached to each `CompiledConstraint`". `CompiledConstraint.span: SourceSpan` exists (`types.rs:819`). Doc-model `ConstraintDoc` (`model.rs:96-111`) has `expr_repr: String` (pre-rendered) and `line: Option<u32>` — no span byte offsets, no source-text re-slice. The `// TODO(post-2361)` comment in the markdown formatter doesn't surface this gap; the formatter just renders the pre-baked `expr_repr`.
- **Blocks:** "verbatim fidelity (units, precedence, identifier casing)" of constraint output.
- **Note:** A real `build_doc_model` would need the source-text `&str` parameter the PRD prescribes; the existing CLI passes nothing of the sort to the stub. With `expr_repr` already a `String`, the slicing happens (or doesn't) at lowering time. Currently it doesn't happen — and no helper exists.

### M-015: `meta` block alphabetic rendering

- **State:** PARTIAL
- **Failure mode:** F5
- **Evidence:** PRD §"meta block rendering" mandates alphabetic key order. `TopologyTemplate.meta: HashMap<String, String>` (`types.rs:522`) — HashMap not BTreeMap. Doc-model `ItemKind::Structure.meta: Vec<(String, String)>` (`model.rs:206-208`) — explicitly insertion-ordered, "preserves duplicate keys and source insertion order". Markdown rendering sorts before emission? Need to verify in formatter.
- **Blocks:** PRD's "deterministic output" guarantee.
- **Note:** PRD says "alphabetical order in all three formats (deterministic output)". Vec<(String, String)> in the model is at odds with PRD intent; if the formatter sorts at render time the output is deterministic but the model's "insertion order" comment becomes a lie. Not fully checked — listed as PARTIAL.

### M-016: Annotation rendering — `@deprecated`, `@test`, `@optimized`, `@solver_hint`

- **State:** PARTIAL
- **Failure mode:** F4
- **Evidence:** `AnnotationDoc { name, args: Vec<String> }` exists generically (`model.rs:40-46`); formatters render annotations. PRD §"Annotations rendering" specifies *behavioural* differences:
  - `@deprecated("msg")` → "Deprecated" callout at top of item
  - `@test` → grouped under bottom "Tests" subsection
  - `@optimized("target")` → "*Optimized: `target`*" italic note
  - `@solver_hint(...)` on a parameter → in the parameter's Description cell
  None of these special-cases is enforced by the model schema (an `AnnotationDoc` is opaque to the formatter). Whether the formatter implements them needs inspection — surface scan of `fmt_markdown.rs:13` shows it imports `AnnotationDoc` and there's a `format_annotations`-style helper, but the `@test` grouping ("at the bottom of the module page") would need a separate pass at module level.
- **Blocks:** the four PRD-named annotation rendering behaviours.
- **Note:** I didn't pin down each individually within the audit budget. Flagging because the rendering rules are specified in the PRD but no test mentions "Tests" subsection grouping in snapshot filenames.

### M-017: `@solver_hint` annotation on parameters

- **State:** TODO
- **Failure mode:** N/A (TODO category)
- **Evidence:** PRD specifies `*hint: discrete_set(standard_bolt_lengths)*` in the parameter description. `SolverHint`/`SolverHintKind` in `crates/reify-compiler/src/types.rs:759-779` capture the data; `ValueCellDecl.solver_hints: Vec<SolverHint>` (`:753`). `ParamDoc` in the doc model has `annotations: Vec<AnnotationDoc>` not `solver_hints: Vec<...>` — solver hints would have to be flattened into the generic annotations vec at lowering time. No tests covering this.
- **Blocks:** the "solver_hint in parameter description cell" PRD requirement.
- **Note:** Once `build_doc_model` exists, this is a minor lowering decision (annotations vs. structured field) — but the model schema doesn't currently support a typed solver-hint cell.

### M-018: Sub-component recursion safety (`is_recursive`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** PRD §"Risks": "TopologyTemplate.is_recursive is set by SCC analysis. The doc tool should not chase recursive sub-components". `TopologyTemplate.is_recursive: bool` exists (`types.rs:547`) and is set by `detect_recursive_structures` in `scc.rs`. The cross-ref index is adjacency-based not tree-walk, so it naturally terminates.
- **Blocks:** none
- **Note:** PRD lists this as "already handled" — confirmed.

### M-019: `--split` markdown output

- **State:** PARTIAL
- **Failure mode:** F4
- **Evidence:** `MarkdownOptions { split: bool }` (`fmt_markdown.rs:113-118`); `MarkdownOutput::Split(Vec<(name, content)>)`. CLI `main.rs:547-559` writes split files via `write_split_files`. Snapshot files exist for split mode. Item filenames embed kind slug (`split.structure-Board.md`, `split.trait-Fastener.md`) — handles multi-kind name collisions.
- **Blocks:** none on its own; degraded by upstream M-005 (empty model → empty split output).
- **Note:** WIRED at the library/CLI seam; just produces empty output today.

### M-020: `#version` pragma reading

- **State:** PARTIAL
- **Failure mode:** F5
- **Evidence:** PRD §"Common: structured doc model" lists `version: Option<(u16, u16)>` on `ModuleDoc`. Compiler captures it: `CompiledModule.declared_version: Option<(u16, u16)>` (`types.rs:249`). Doc-model `ModuleDoc` (`model.rs:19-33`) has NO `version` field — the doc model captures `pragmas: Vec<PragmaDoc>` (untyped name+args list) instead. A consumer wanting the version must parse it back out of the pragmas vec.
- **Blocks:** the "JSON consumers can rely on the schema" promise — version is buried in untyped pragmas.
- **Note:** Forward-compat acceptable, but PRD explicitly listed a typed field.

### M-021: Pragma rendering

- **State:** PARTIAL
- **Failure mode:** F5
- **Evidence:** `ModuleDoc.pragmas: Vec<PragmaDoc>` (`model.rs:29`); `PragmaDoc { name, args: Vec<String> }`. `CompiledModule` carries five distinct typed pragmas (`pragmas: Vec<reify_syntax::Pragma>`, `default_tolerance: Option<f64>`, `declared_version`, `solver_pragma: Option<SolverPragma>`, `kernel_pragma: Option<String>`). Lowering would need to fold these back into a uniform `PragmaDoc` list — losing structured fields like `default_tolerance: f64`.
- **Blocks:** none structurally; quality of pragma rendering.
- **Note:** Marker for the lowering pass (M-005) — decision needed about whether to expose the structured typed pragmas or stringify them.

### M-022: `examples/integration_full_v01.ri` end-to-end smoke

- **State:** PARTIAL
- **Failure mode:** F4
- **Evidence:** PRD Acceptance: "Running `reify doc examples/integration_full_v01.ri` against a stdlib module produces useful output". File exists at `examples/integration_full_v01.ri`. Snapshots exist at `crates/reify-doc/tests/snapshots/integration_full_v01.{html,single.md,split.*.md}` and pass. BUT the snapshots are built from a hand-coded inline `DocModel` fixture (`fmt_html_tests.rs:1937-1951`), NOT from compiling the `.ri` file. The TODO at `:1939` is explicit: "replace with `build_doc_model(load_str!("examples/integration_full_v01.ri"))` once that function lands".
- **Blocks:** PRD acceptance criterion validity. Today running `reify doc examples/integration_full_v01.ri` produces a `DocModel { modules: [{ path, ..Default::default() }] }` and renders an empty page.
- **Note:** Snapshot tests prove the formatter renders the *fixture* correctly. They do not prove the compiler-to-doc lowering works (because the lowering doesn't exist).

### M-023: stdlib smoke check on `crates/reify-compiler/stdlib/units.ri`

- **State:** TODO
- **Failure mode:** N/A
- **Evidence:** PRD Acceptance: "Running `reify doc` against a stdlib module (`crates/reify-compiler/stdlib/units.ri`) produces useful output — used as a manual smoke check, not a snapshot." No test or script exercises this. Today same as M-022: empty output.
- **Blocks:** none enforced; manual smoke check.
- **Note:** Not blocked on the doc tool's own scope — but it depends transitively on M-005 / M-006.

### M-024: `cargo test -p reify-doc` passes with snapshots

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** PRD Acceptance: snapshot tests exist and presumably pass (snapshot file count and tests file size suggest active maintenance). Tests at `crates/reify-doc/tests/fmt_html_tests.rs` and `fmt_markdown_tests.rs`. Not exercised live in this audit.
- **Blocks:** none
- **Note:** The acceptance is met *for the fixture*; it doesn't prove end-to-end correctness for arbitrary `.ri` files.

## Cross-PRD breadcrumbs

- **Pragma PRD (`pragmas.md`)** — Doc tool reads `#version` (M-020), `#precision`, `#solver`, `#kernel` pragmas; the CompiledModule fields exist. If pragma PRD changes the lowered shape, the doc model breaks.
- **All v0.1–v0.4 PRDs that define structures, occurrences, fns, etc.** — every PRD that adds a declaration kind needs to be reflected in `ItemKind`. The doc model has 10 variants (`structure`, `occurrence`, `trait`, `function`, `field`, `purpose`, `enum`, `unit`, `type_alias`, `constraint_def`); any future declaration shape (e.g. `domain`, `signature`) would need extension.
- **GR-001 sibling** — M-006 (compiler-side `doc` field on TopologyTemplate / CompiledFunction / etc.) is structurally identical to GR-001: a task marked done in the task tracker whose runtime/compile-time contract is empirically absent from the codebase. Phase 3 should treat M-006 as a candidate for the gap-register's first promotion alongside GR-001.

## Footnotes (audit notes)

- The compiled-types deficit on doc-comments (M-006) is the most consequential finding because every other doc-rendering mechanism downstream depends on it. Two architects ran past it: the original task #215 closer (didn't add the field) and the reify-doc PRD author (asserted it was already there).
- The CLI HTML stub (M-008) is a small embarrassing regression: the real renderer is wired into tests but the production CLI still uses the temporary scaffold.
- No mechanism counted as ORPHAN. The `model::ModuleCrossRefs` type is the only "extra" relative to the PRD — it's not orphan, just an additive forward-compat field for future cross-module lowering.
- Did not exhaustively chase whether `@test` grouping and `@deprecated` callout are implemented in the markdown formatter — counted as PARTIAL based on absence of structured affordances in the model + lack of named tests.
