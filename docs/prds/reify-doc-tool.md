# PRD: `reify doc` documentation generator

**Status:** v0.1 scope.
**Spec reference:** `docs/reify-language-spec.md` §13.
**Pre-existing infrastructure:** Tasks **#214** (doc-comment extraction in the AST), **#215** (doc-string propagation to compiled output — `TopologyTemplate`, `CompiledFunction`, `TraitDef`, `EnumDef` all carry `doc: Option<String>`), **#216** (LSP hover exposes the doc), and **#217** (≥8 tests covering all four declaration kinds) are **done**. The compiler already retains: doc comments per declaration, the `meta { … }` block contents (`TopologyTemplate.meta: HashMap<String, String>`), parameter declarations with types and dimensions, port declarations with kind/role, constraints, trait bounds, and visibility.

This PRD covers the **toolchain feature** that consumes that data and emits human-and-machine-readable documentation. §13.3 is explicit that the output format is implementation-defined; this PRD nails it down.

## Goals

A library author can run

```
reify doc examples/integration_full_v01.ri --format markdown -o doc/
```

and get a complete, navigable, non-trivial reference for every public declaration in the module, with parameter tables, port tables, constraint summaries, `meta` block dumps, and cross-references. Output is faithful — every doc comment ends up where it belongs, every public declaration is listed, no editorialising, no truncation.

## Non-goals

- **Search.** Output is static; no JS, no client-side index. The JSON format exists to let downstream tools (e.g. a future `reify book`) build search.
- **Theming.** HTML uses one minimal embedded stylesheet. v0.2 may add `--theme`.
- **Diff between module versions.** The version pragma exists; cross-version diffing is v0.2 plus.
- **Doc-test execution.** Reify's `@test` annotation already exists for that; the doc tool just lists `@test` declarations under a "Tests" heading.
- **Auto-publishing.** No upload, no GitHub Pages helper, no manifest. Just write files.

## Output formats

Three formats, exhaustive enumeration. CLI flag `--format html|markdown|json`; default `html`.

### Common: structured doc model

A pure-data crate-internal `DocModel` (defined in the new `reify-doc` crate) is built first. All three formatters consume the same model.

```text
DocModel {
    module: ModuleDoc {
        path: ModulePath,
        pragmas: Vec<PragmaDoc>,        // from compiled module pragma fields
        version: Option<(u16, u16)>,    // from #version pragma, if any
        items: Vec<ItemDoc>,            // sorted: traits, then structures, then occurrences,
                                        // then enums, then functions, then constants —
                                        // alphabetical within each kind
    }
    cross_refs: CrossRefs {
        // trait name → list of conforming structure / occurrence names
        // entity name → list of entities that contain it as a sub-component
        // (these are computed once over the whole module)
    }
}
ItemDoc {
    name, kind (Structure | Occurrence | Trait | Enum | Function | Constant),
    visibility, type_params, trait_bounds, doc (joined paragraphs),
    annotations: Vec<AnnotationDoc>,    // @test, @deprecated message, @optimized target
    meta: BTreeMap<String, String>,     // copied from TopologyTemplate.meta
    params: Vec<ParamDoc>,              // value cells with kind=Param or Auto
    lets: Vec<LetDoc>,                  // value cells with kind=Let (in summary form only;
                                        // expressions not rendered, just type + doc)
    ports: Vec<PortDoc>,
    constraints: Vec<ConstraintDoc>,    // bare expressions rendered from source span;
                                        // named constraint instantiations show their target name
    sub_components: Vec<SubComponentDoc>,
    realizations: Vec<RealizationDoc>,
}
ParamDoc { name, type_str, dimension, default_str (None if auto), doc, annotations }
PortDoc  { name, kind, role, port_type, doc }
```

The doc model is **read directly from the existing `CompiledModule`**: `TopologyTemplate.value_cells`, `.ports`, `.constraints`, `.meta`, `.annotations`, `.sub_components`, etc. The formatters never touch the AST — only the compiled output and (optionally) the original source text for verbatim constraint rendering by span.

### Markdown (GitHub-flavored)

Single file by default; `--split` flag emits one file per item plus an index. Layout per item:

```markdown
## `pub structure HexBolt : Fastener` <a id="HexBolt"></a>

A standard hex-head bolt per ISO 4014.

The bolt length includes the head…

### Parameters

| Name | Type | Dimension | Default | Description |
| --- | --- | --- | --- | --- |
| `diameter` | `Length` | length | — | Nominal bolt diameter (shank diameter). |
| `length` | `Length` | length | — | Total bolt length including head. |

### Ports

| Name | Kind | Role | Type | Description |
| --- | --- | --- | --- | --- |
| `head` | mechanical | out | `MechanicalPort` | … |

### Constraints

- `length >= diameter` *(line 42)*
- `MinThread(bolt: this)`

### Meta

- **part_number**: ISO-4014-M8x25
- **revision**: B

### Conforms to

- [`Fastener`](#Fastener) — *trait*
```

Tables for parameters and ports. Bullet lists everywhere else. Dimension is rendered as the canonical SI dimension string (`length`, `mass · length / time^2`, etc.) — the `Type` column shows the language-level type (`Length`, `Force`).

The TOC at the top groups by kind (Traits / Structures / Occurrences / Enums / Functions / Constants). Cross-refs use markdown anchor links (`<a id="…">`) and `[name](#anchor)`. GFM tables, GFM auto-anchors used as a fallback when explicit anchors aren't generated.

### HTML

Single self-contained file with embedded `<style>`. No external CSS. No JS.

- Top: TOC nav (`<nav>` element, sticky position via simple CSS `position: sticky; top: 0;`).
- Per item: same sections as markdown, rendered with semantic HTML (`<table>`, `<dl>` for the meta block, `<section id="HexBolt">` for anchor targeting).
- Code spans use `<code>`. Doc comment paragraphs use `<p>`. No syntax highlighting in v0.1.
- The `<style>` block is hand-written, ≤100 lines, monospace fallback for code, max-width 900px on body, generous line height. Aim for legibility, not branding.

### JSON

Direct serde serialisation of `DocModel`. Pretty-printed by default; `--compact` flag for single-line. Field naming: `snake_case`. Schema is the doc model definition above; consumers can rely on it being stable across v0.1 patch releases. **No backward-compat promise across v0.1 → v0.2** — bump the version pragma to signal.

## CLI

New subcommand in `crates/reify-cli/src/main.rs`:

```
reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]
```

- `<input.ri>` — required positional. Single file in v0.1; multi-file modules deferred (the existing `reify check` only takes one file too — this matches).
- `-o <path>` — output path. For single-file formats: defaults to stdout when omitted. For `--split` markdown: required (must be a directory).
- `--format` — defaults to `html`.
- `--split` — markdown only; emit one file per item plus `index.md`. Errors out for `html` and `json`.
- `--compact` — JSON only; emit single-line JSON.

Exit codes: `0` if compilation succeeded, `1` if compile errors prevented doc generation (errors printed to stderr exactly as `reify check` does today), `2` for CLI usage errors.

## Crate structure

New crate `crates/reify-doc/` with workspace member entry in `Cargo.toml`. Library + thin module structure:

```
crates/reify-doc/
  Cargo.toml
  src/
    lib.rs           // pub fn build_doc_model(&CompiledModule, &str) -> DocModel
    model.rs         // DocModel + child types
    cross_refs.rs    // trait → conformers, entity → containers
    fmt_markdown.rs  // pub fn render_markdown(&DocModel, MarkdownOptions) -> String or Vec<(name, content)>
    fmt_html.rs      // pub fn render_html(&DocModel) -> String
    fmt_json.rs      // pub fn render_json(&DocModel, compact: bool) -> String
  tests/
    snapshot_tests.rs   // golden-file snapshots per format for examples/integration_full_v01.ri
    model_tests.rs      // unit tests on the doc model builder
```

Dependencies: `reify-compiler`, `reify-syntax` (for span-based source rendering only — strictly read-only access to the source text), `serde`, `serde_json`. No new external deps beyond what the workspace already pulls in.

`reify-cli` adds `reify-doc` as a workspace dep and routes `reify doc` to it via a new `cmd_doc` function (mirroring `cmd_test`, `cmd_check`).

## Source text rendering for constraints

Constraint summaries (`ConstraintDoc`) include the original constraint expression. v0.1 renders them by re-slicing the source text using the span attached to each `CompiledConstraint`. This keeps verbatim fidelity (units, precedence, identifier casing) without round-tripping through a pretty-printer that doesn't yet exist. For named constraint instantiations (`constraint MinWall(wall: thickness)`), render the constraint def name and arguments using the same span-slice approach.

## meta block rendering

`TopologyTemplate.meta` is already a `HashMap<String, String>`. Render keys in alphabetical order in all three formats (deterministic output). v0.1 treats every value as a plain string; richer typed `meta` values are deferred.

## Cross-references

Two cross-reference indices, computed once over the whole compiled module:

1. **Trait → conformers.** Walk every `TopologyTemplate`; for each entry in `trait_bounds`, push the template's name onto the trait's bucket. Render under "Implementations" on each trait page.
2. **Entity → containers.** Walk every `TopologyTemplate.sub_components`; record the parent. Render under "Used by" on each structure / occurrence page.

Both are `BTreeMap<String, Vec<String>>` for deterministic ordering. **Out of scope for v0.1:** function call graphs, geometry op references, port-connection graphs.

## Annotations rendering

- `@deprecated("msg")` → render at the top of the item with a "Deprecated" callout containing the message.
- `@test` → group `@test`-annotated items under their own "Tests" subsection at the bottom of the module page (still rendered in full; just visually grouped).
- `@optimized("target")` → small italic note under the item: *Optimized: `target`*.
- `@solver_hint(…)` on a parameter → render in the parameter's "Description" cell after the doc comment, e.g. `*hint: discrete_set(standard_bolt_lengths)*`.

## Acceptance

- `cargo build -p reify-doc` succeeds.
- `cargo test -p reify-doc` passes, including snapshot tests for markdown and HTML against `examples/integration_full_v01.ri`. Snapshots are committed to `crates/reify-doc/tests/snapshots/`.
- `reify doc examples/integration_full_v01.ri --format markdown` produces a non-trivial readable document covering at least all `pub` declarations in the example, with no panics, no `Display` placeholders, no missing-doc-comment crashes.
- `reify doc examples/integration_full_v01.ri --format json --compact | jq .` parses cleanly.
- `reify doc examples/integration_full_v01.ri --format html -o /tmp/doc.html` produces a single self-contained file (no external resource references) that renders in a browser.
- Running `reify doc` against a stdlib module (`crates/reify-compiler/stdlib/units.ri`) produces useful output — used as a manual smoke check, not a snapshot.

## Risks and open design questions

- **Sub-component recursion.** `TopologyTemplate.is_recursive` is set by SCC analysis. The doc tool should not chase recursive sub-components into an infinite expansion — the cross-reference index already handles this (it's an adjacency map, not a tree walk).
- **Generic instantiation.** `Box<Bolt>` etc. — render as the source-span text rather than re-pretty-printing. Same approach as constraints; v0.1 doesn't have a unified pretty-printer.
- **Doc on `let` cells.** Doc comments on internal lets do exist. Render in a "Computed values" subsection but compactly (name + type + doc), no expression body. Avoids leaking implementation detail in v0.1; library authors who want that detail can read the source.
- **Cross-module references.** v0.1 takes one file; cross-module xrefs are deferred. The `ModulePath` field is captured for forward compat.
- **Stdlib doc bundling.** Generating docs for the entire stdlib as a single bundle is tempting but out of scope for v0.1. The cross-ref index is already sized for it; a v0.2 task can add `reify doc --stdlib`.

## Task slicing

Six tasks. Touching each module crisply rather than one mega-task makes the work parallelisable and the review surface smaller per task.

1. **`reify-doc` crate scaffold + `DocModel` types.** Add the crate to the workspace, define `DocModel` and its sub-types in `model.rs`, no formatters yet, `cargo test -p reify-doc` empty but green. **Medium priority.**
2. **`build_doc_model` over `CompiledModule`.** Implement `build_doc_model(&CompiledModule, source: &str) -> DocModel`, including spans-to-source-slice for constraints. Tests: model contains the expected items for a small fixture. **Medium priority.**
3. **Cross-ref index.** `cross_refs.rs` with the two indices defined above; tests over a synthetic module with trait conformance and sub-components. **Medium priority.**
4. **Markdown formatter.** `fmt_markdown.rs`, snapshot test against `examples/integration_full_v01.ri`. **Medium priority.**
5. **HTML formatter.** `fmt_html.rs` with embedded stylesheet, snapshot test against the same example. **Medium priority.**
6. **JSON formatter + CLI subcommand.** `fmt_json.rs` plus the new `cmd_doc` in `reify-cli`, end-to-end test that runs `reify doc` as a subprocess via `reify-test-support` test harness. **Medium priority.**

The `--split` markdown variant can ride along with task 4.

The whole feature is **medium priority** in the v0.1 backlog — library-author UX, ships slightly behind core lang. None of the six tasks block the lang spec; they sit downstream of the already-done #214–#217 and the pragma PRD's `#version` field (which the doc tool reads but does not require).
