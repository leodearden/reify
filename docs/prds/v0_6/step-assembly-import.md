# STEP assembly import — imported assemblies as generated, auto-refreshed modules

**Status:** active — B+H contract PRD. Authored 2026-09-27/28 in an interactive `/team /prd`
session (Leo as design authority; substrate seats on sonnet, adversarial gate review on opus,
lead on fable). Every load-bearing fork in §4 was put to Leo and decided by him in-session.
**Code anchors** verified against main `5340545275` (2026-09-28). Main moves fast —
cite-by-symbol; re-locate lines at implementation time.
**Supersedes:** task ζ of `docs/prds/v0_6/io-export-import-completion.md` (§4.5, §7.4) — the
single-shape `step_import(path) -> Solid` design — and absorbs its task #4289 as leaf α here.
**Substrate evidence:** `docs/prds/v0_6/step-assembly-import.evidence/` (compiled-and-run OCCT
7.8 XDE writer/reader, a multi-product STEP fixture, captured output) plus the probes recorded
in §3.
**Critical path (read first):** nothing in this PRD is usable end to end until the
resolution-unification program's `compile_program` (#5516) and multi-file `build`/`eval`
(#5517) and the stdlib-namespace qualified-reference resolution (#5505) land — today `reify
build` compiles one file and `import lib as l; l.Plate()` is an unknown structure. Leaves
α, β, γ, δ, θ and the native halves of η and λ can proceed now; ε, ζ, κ, ν wait (§7, §8).

---

## 1. Goal — what a user observes

A designer drops a vendor STEP model of an ISO shipping container (an assembly of hundreds of
parts) into their project and builds a design that modifies the container and assembles with it.
The design tracks the real current STEP file with no manual step: when the vendor file changes,
the next `check`/`build`/GUI reload picks it up.

```reify
module site
import "vendor/container.step" as container

structure def Site {
    aux let door_cut = box(900mm, 60mm, 2000mm)          // reference geometry, not exported

    aux sub stock = container.Assembly()                 // the pristine import, hidden

    sub box : container.Assembly {                       // the modified container
        side_wall_1.panel_1.geometry = difference(stock.side_wall_1.panel_1.geometry, door_cut)
        exclude roof_panel_3
    }

    // a datum on an imported face — already in Site's frame, because a sub-member
    // geometry read is scope-posed (placement-relations-belt §7.1.5)
    let door_plane = single(faces_by_normal(stock.side_wall_1.panel_1.geometry, vec3(0.0, 1.0, 0.0), 1deg)).plane

    sub bracket = DoorBracket() at auto
    relate { flush(bracket.seat_plane, door_plane) }
}
```

Every operand above is in `Site`'s frame: `stock.…geometry` is scope-posed (belt §7.1.5, #5436),
`door_cut` is authored in `Site`, and the override value is transported by the engine into the
panel's local frame (D19). No `in_frame` or `.world_frame` appears, so the belt's pose-cycle rule
is never touched.

After this PRD lands, a user can:

1. **Import by naming the file.** `import "vendor/container.step" as container` resolves the path
   relative to the importing file, (re)generates `generated/container.ri` beside it when it is
   missing or stale, and compiles it as an ordinary module. An unchanged STEP costs one hash.
2. **Browse what came in.** The generated module is a committed, human-readable `.ri` file: one
   `structure def` per STEP product, one `sub` per occurrence named from the STEP instance name,
   with its placement. Go-to-definition lands in it; completion after `container.` and after
   `self.box.` lists the imported names.
3. **See regenerations without having to look for them.** When the STEP changes, an info
   diagnostic `I_STEP_MODULE_REGENERATED` summarises parts added, removed, renamed, moved, or
   re-matched.
4. **Modify imported parts from their own design** with a specialization body: dotted override
   paths (`side_wall_1.panel_1.geometry = …`) and `exclude <path>` — re-applied automatically on
   every regeneration, never lost to it, and loud when a named part disappears.
5. **Assemble new parts with it** — `relate` mates against datums taken from imported faces.
6. **Export and account for it.** `reify build -o site.step` writes the modified geometry (a flat
   compound of solids — the writer does not emit product structure today, §9); `reify report
   --bom` lists the STEP import as a provenance row (path, content hash, OCCT version).

---

## 2. Consumers (G1)

- **The container use case (Leo, 2026-09-26/27)** — the motivating consumer: import a
  hundreds-of-parts ISO container STEP, cut openings, remove panels, mount brackets, export.
  Realised in-repo as the integration example `examples/step_import/` (leaf ν).
- **Every future external-geometry import.** The foreign-module provider seam (§5 C3) is the
  single place a future IGES / 3MF-in / point-cloud import plugs in; `io-import-pointcloud.md`
  (deferred) re-targets its "geometry-import seam" reference to this PRD (companion leaf ο).
- **Generic language surfaces with standalone value**, each with this PRD's integration gate as
  its named consumer and native designs as a second: dotted override paths and `exclude` in
  specialization bodies (any nested assembly); scope-level datum operands for `relate`; cross-module
  LSP member completion and go-to-definition through aliased imports (any multi-module project).

---

## 3. Substrate reality (G3 — probe-verified 2026-09-27/28)

### 3.1 Kernel: XDE reading works on system OCCT 7.8 (compiled and run)

Evidence: `step-assembly-import.evidence/` (README lists each file's claim).

- `STEPCAFControl_Reader` with `SetNameMode(true)` into an XCAF document yields the full product
  tree: assemblies, components (references) with `GetReferredShape`, per-component
  `TopLoc_Location`, product names (`TDataStd_Name` on the product label) and **instance names on
  the component label** (the NAUO name, e.g. `CornerCasting-1`), read back byte-identical.
- Shared products are detectable: all four `CornerCasting-n` components refer to one product label.
- A multi-body product (a `TopoDS_Compound` of two solids) reads back as one product shape with
  2 solids under `TopExp_Explorer(TopAbs_SOLID)`.
- **Units:** `Interface_Static::SetCVal("xstep.cascade.unit", "M")` before `ReadFile` converts
  every coordinate and location from the file's declared unit to metres (178 mm → 0.178).
  `STEPControl_Reader::SetSystemLengthUnit` had **no** observable effect for any value tried.
  The static is **process-global** XSTEP state, like the writer's (`g_step_export_mutex` in
  `occt_wrapper.cpp` documents that `XSAlgo` state is shared across kernel threads): the reader
  takes the same mutex and restores nothing it did not set — boundary row B4.
- **Link delta:** only `TKXCAF` and `TKLCAF` join `crates/reify-kernel-occt/build.rs`'s list
  (both present in `/usr/lib/x86_64-linux-gnu` as `.so.7.8`; `STEPCAFControl_*` already live in
  the linked `TKDESTEP`). `TKCAF`/`TKVCAF` are not needed. **Hazard:** `libgmsh.so` from
  `/opt/reify-deps` already pulls `libTKXCAF.so.7.9` and `libTKLCAF.so.7.9` into every reify
  process, so both XCAF versions coexist with unversioned symbols. The evidence programs linked
  7.8 alone; α's signal therefore runs **inside the workspace's gmsh-linked test binary**, and
  `scripts/check-manifold-deps.sh` gains the two toolkits (CLAUDE.md native-deps invariant).
- The plain `STEPControl_Reader` → `TransferRoots` → `OneShape` route that #4289 planned returns
  one anonymous compound of all solids, already placed, with **no** names or hierarchy.
- No reader of any geometry format exists in the workspace today; `occurrence def STEPInput` in
  `crates/reify-compiler/stdlib/io.ri` is a provenance carrier with no result, and `Provenance`
  has no hash field.

### 3.2 Grammar gate (`tree-sitter parse --quiet`, session-isolated cache)

| Fragment | Parses |
|---|---|
| `import "vendor/container.step" as container` | **no** → leaf δ |
| `import vendor.container as container` | yes |
| `sub box : container.Assembly { geometry = difference(…) }` | yes |
| `sub box : container.Assembly { side_wall_1.panel_1.geometry = … }` | **no** → leaf η |
| `sub box : container.Assembly { exclude roof_panel_3 }` | **no** → leaf θ |
| `relate { flush(bracket.seat_plane, door_plane) }` | yes |
| `sub source = STEPInput(source: "vendor/container.step")` | yes |
| `aux sub stock = container.Assembly()` | yes (and aux subs are excluded from export) |
| `sub b = image of stock under identity { exclude roof_panel_3 }` | yes |
| `sub b = image of stock under identity { side_wall_1.geometry = … }` | **no** → leaf ι |

The GUI's lezer mirror (`gui/src/editor/reify.grammar`, pinned by `reifyGrammarCorpus`
`EXPECTED_CLEAN`) carries a legacy `import "foo.ri"` string arm with no `as`; δ, η, θ and ι each
update the mirror and the pin in the same diff.

### 3.3 Compiler / engine reality (probed with `target/debug/reify`, 2026-09-28)

- **Aliased imports do not resolve.** `import lib as l` + `sub p = l.Plate()` → `error:
  sub-component "p" references unknown structure "l.Plate"` at `reify check`. `ImportKind::Aliased`
  has no compiler arm. Owner: stdlib-namespace ν **#5505** (pending; depends on #5516).
- **`build`, `eval`, `report` compile one file.** `import lib.Plate` passes `reify check`, but
  `reify build` prints `warning: import "lib" not resolved by this entry point` and then errors on
  the unknown structure (`parse_and_compile` in `crates/reify-cli/src/main.rs`). Owners:
  resolution-unification β **#5516** (`compile_program` / `CompiledProgram`), γ **#5517**
  (build + eval), δ **#5518** (report/explain/doc), ζ **#5520** (LSP diagnostics multi-file).
- **Specialization-body geometry overrides are silently ignored.** `sub q : Plate { geometry =
  cylinder(50mm, 100mm) }` builds and exports the box: 1 solid, 0 `CYLINDRICAL_SURFACE`, no
  diagnostic. instantiation-value-flow **#6592** (with its geometry-plane test completion
  **#6610**, deferred) owns the fix; η depends on it.
- **A `let` in a specialization body is silently dropped** — parses, creates no cell, and
  `box.w` reports `structure 'Panel' has no member 'w'`. This PRD does not use specialization-body
  lets (D10); the silent drop is filed as an INV-SF-3 bug at decompose (§8, out-of-batch).
- **Module resolution** (`crates/reify-compiler/src/module_dag.rs`): `ModuleResolver` maps a dotted
  `ImportDecl.path` to `<root>/<segments>.ri` or `…/mod.ri`; `ModuleDag::compile_module` and
  `compile_project` hold the two `std::fs::read_to_string` sites. **No extension point exists.**
  `check_module_path_decl` compares the declared `module` line against the dotted path the importer
  used. **Each host roots the resolver differently** — CLI: the entry file's parent; GUI: the entry
  path's parent; LSP: the workspace root — which is why D3 derives the generated path from the
  importing file, never from a root.
- **Hosts** constructing a resolver: `reify-cli`, `reify-lsp` (three sites), `gui/src-tauri`
  engine. `reify-compiler` and `reify-lsp` depend on no kernel crate; the GUI has OCCT behind its
  `gui` feature; the CLI links OCCT through its engine.
- **No design-relative path convention exists**: field-import paths (`FieldSource::Imported`) are
  passed verbatim to the reader, and `ParsedModule` carries a dotted `ModulePath`, not a filesystem
  path. This PRD introduces the first one (D12) and threads the filesystem path (β).
- **LSP completion is single-file** (`completion()` builds its context from the current document
  only); go-to-definition is cross-file via `ModuleResolver` but returns nothing for
  `ImportKind::Aliased`.
- **Geometry-typed params with op defaults** (`param geometry : Solid = box(…)`) compile to a
  `ValueCellDecl` + `RealizationDecl` pair. A one-level sibling reference (`self.stock.geometry`)
  is supported; a two-hop-or-deeper path (`stock.side_wall_1.panel_1.geometry`) fails at eval by
  design (the task-3814 v0.1 boundary) — lifted by **#5427** (uniform-member-access δ) and **#6598**.
- **Predicate selectors work on unseeded handles**: `faces_by_normal`/`faces_by_area`/… are pure
  kernel queries; an op outside `is_seedable_primitive`'s allowlist seeds no topology attributes
  and routes attribute resolution to `AttributeResolution::FallbackToComputed`.
- **Realization caching** is keyed per graph node (`RealizationCache`: entity, repr, options), not
  by op arguments; no file- or document-scoped cache exists in the kernel (§5 C4 adds one).
- **`relate` operands** (`relate_solve.rs::decode_operand`) must be `<sub>.<member>` over a
  `StructureRef`-typed sub, and `realize_operand_datums` builds the operand sub's **structure**
  standalone in its own identity frame — datums are per-structure, not per-instance. A datum
  computed from a *sibling's* geometry cannot be reached that way → leaf κ widens the fixed-side
  operand to a scope-level datum expression (D10).
- **`aux sub` is still realized, tessellated and shipped** to the viewport (spec §4.7), so the
  pristine `stock` copy doubles kernel work; see §9 and Q6.
- **Collections:** `sub xs : List<T>` and `sub xs : Keyed<T>` both evaluate per-element geometry
  to `undef` today, and a per-key `at` does not parse — which is why duplicates become named subs
  (D6), not a collection.
- **BOM:** `Engine::build_bom_report` enumerates `Input`/`Buy`/`Discard` subs only; an `Input`
  (e.g. `STEPInput`) becomes a `ProvenanceEntry` row, surfaced by `reify report --bom`.
- **Export writes a flat compound**: `export_step` emits solids without product names or
  instancing (PRODUCT names are `Open CASCADE STEP translator 7.8 n`).
- **A `Solid`-typed param's realization ops are compiled from its DEFAULT expression** (the
  Solid-param lowering in `crates/reify-compiler/src/entity.rs`), so a value overlay can never
  replace them: the constructor-arg arm `sub q = Plate(geometry: cylinder(…))` is silently
  ignored today exactly like the specialization arm (D3 probe: volume 1e-6 m³, the default box).
  instantiation-value-flow #6592 widens the overlay and the re-realization trigger; it does not
  replace realization ops. **η owns that replacement** (C6).
- **A duplicate specialization-body override is a warning, first assignment wins, exit 0** —
  pinned by `spec_param_override_compile_tests.rs::non_auto_override_duplicate_in_body_warns_first_wins`
  and `spec_param_override_resolution.rs::duplicate_non_auto_override_resolves_to_first_value`
  (task 4694). C6 keeps that rule for dotted paths.
- **Sub-member reads are scope-posed** once placement-relations-belt β #5436 lands (§7.1.5,
  ratified 2026-07-25: an instance value carries its frame; `.world_frame` *is* that carried
  frame). UMA D4's local-frame reading is superseded for instance-valued receivers. This PRD's
  datum and override idioms are written for the post-#5436 rule and never read `.world_frame`.
- **The belt's pose-cycle rule** (§7.1.3 decision 3) rejects any Phase-A-consumed expression
  that reads `.world_frame` of a same-scope sub. The datum idiom (D10) reads geometry, not
  `.world_frame`, so it is admitted.
- **Sub-form constructor keyword arguments are not checked**: `sub x = Ctor(bogus: 1)` silently
  drops the unknown argument (`struct-ctor-field-type-conformance.md` territory); an expression-
  position ctor rejects it with `E_CTOR_UNKNOWN_FIELD`, and an occurrence ctor in expression
  position is only an unresolved-function warning. "Every documented signature
  compiles" is therefore vacuous as an acceptance for occurrence signatures; ξ diffs the
  documented signatures against `io.ri` instead.
- **`examples_smoke` compiles every corpus file single-file** (`ModulePath::single(stem)`, no
  importing-file path); its `SKIP_SET` is forbidden for `best_practices/`. A best-practices
  exemplar therefore cannot contain a file import; the import lives in `examples/step_import/`
  (ν) and the exemplar shows the native-structure idioms.
- **Integer-literal `vec3(0, 1, 0)` is silently undef** as a selector direction (warning only,
  exit 0, face and `.plane` undef); the real-literal form `vec3(0.0, 1.0, 0.0)` works. Every
  example in this PRD uses real literals.
- **`test/corpus` is not CI-run**: only corpus cases `include_str!`-validated by a Rust test in
  `tree-sitter-reify/tests/` are pinned. A fixture pinned in `reifyGrammarCorpus`
  `EXPECTED_CLEAN` (`gui/src/__tests__/reifyGrammarCorpus.test.ts`) must also be registered in
  `scripts/verify.sh` `_GUI_COUPLED_RI_FIXTURES` in the same diff (PG-DRIFT-GUI), and
  `verify.sh` is a verify-pipeline file, so that diff takes the full gate.
- **The unknown-override diagnostic is declaration-order dependent**: a forward-declared child
  (`Plate` declared after the specializing structure) gets no diagnostic in release and a
  `debug_assert` panic in debug (`entity.rs` Case 1 optimistic injection). η closes this.
- **Bare `identity` is not a `Transform<3>`** (`identity(n: Int)` is a tensor builtin);
  `transform3_identity()` is. The copy form uses it.
- **`relate` has no doc chunk and no spec section**; the base at-auto + relate chunk docs belong
  to placement-relations-belt ν (#5446); ξ depends on it.
- **`examples/best_practices/` is also walked by the kernel-less
  `harness_corpus_gates/best_practices_constraint_gate.rs`** (zero Violated, Indeterminate pinned
  via `EXPECTED_INDETERMINATE`) and the corpus-wide eval gates; an exemplar with a
  geometry-consuming constraint needs its `EXPECTED_INDETERMINATE` row (the `clearance_oracle.ri`
  precedent).
- **Probe-binary staleness**: the D3 harness's default binary resolution picks
  `target/release/reify` (2026-09-01) over the current debug build; every probe in this PRD's
  verification ran with `REIFY_BIN` pointed at the 2026-09-30 debug build.

---

## 4. Resolved design decisions (all Leo's calls, 2026-09-27/28, unless marked *review-driven*)

- **D1 — Surface: an import statement naming the file.**
  `import "vendor/container.step" as container` — one widening of `import_declaration` (a string
  path arm beside the dotted `import_path`); the alias is mandatory for a file import. The alias
  binds the generated module. *Rejected:* a compile-time intrinsic `import_step(path: "…")` in sub
  position — it parses today, but its argument must be a compile-time literal (member names come
  from the file and member resolution is static), so it looks dynamic while being static, and
  product types would be reachable only through the generator's file path. Function-in-type-
  position rejected.
- **D2 — The import is sugar for "ensure the generated module is current, then import it".**
- **D3 — Generated path is derived from the importing file, never from a host root**
  (*review-driven*, closes the CLI/GUI/LSP root divergence). File:
  `<dir of importing file>/generated/<STEP basename without extension, sanitised>.ri`. Module
  key and declared `module` line: `<importing module path>.generated.<sanitised stem>`. Two
  imports from one directory whose sources differ but map to one generated path →
  `E_STEP_IMPORT_PATH_COLLISION`. The generated file is **committed** like a lockfile: the LSP and
  CI read names without OCCT, and a vendor change shows up as a reviewable diff.
- **D4 — Freshness key = source content hash + generator version + reader major.minor**
  (*review-driven addition of the last two*). The resolver regenerates when the file is missing or
  any key differs, and does nothing otherwise. Hosts that cannot regenerate (no OCCT) verify only
  and report `E_STEP_MODULE_STALE`. A **locked mode** (`--locked` / `REIFY_GENERATED_LOCKED=1`)
  makes every host verify-only; gate tests and CI run locked so a stale committed module fails
  loudly instead of dirtying the tree.
- **D5 — Generated modules are never hand-edited.** The header hashes the generated body; a
  mismatch → `E_GENERATED_MODULE_EDITED` (pointing at the specialization-body edit form), no
  overwrite. Regeneration writes to a temporary file and renames it into place under a lock file,
  so concurrent CLI and GUI regenerations cannot interleave.
- **D6 — Hierarchy mirrors the STEP product tree**, and the generator also emits a flattened view:
  `container.Assembly` (nested, one structure per product) and `container.Flat` (every leaf
  occurrence as a direct sub with its composed pose, names path-joined with `__`).
- **D7 — Occurrence identity comes from STEP instance names**, sanitised to identifiers
  (`CornerCasting-1` → `corner_casting_1`). Positional suffixes are used only when instance names
  are empty or collide. Duplicates are **named subs**, not an indexed or keyed collection.
- **D8 — Identity-preserving regeneration, loud on re-targeting** (*review-driven strengthening*).
  The previous committed generation is the identity map. Matching order: same product name +
  same instance name; else same product name + nearest placement **within a bounded distance**
  (Q2). Matched occurrences keep their identifier; new ones get fresh identifiers; removed ones are
  retired and never reused. A placement-based re-match is recorded in the header as `rematched`
  and stays there until the next regeneration; the compiler emits `W_STEP_IDENTIFIER_REMATCHED`
  when an override, exclusion or reference names a rematched identifier. A reference to a retired
  or renamed identifier is the ordinary unknown-member error, enriched with the header's record.
- **D9 — Multi-body products** become a structure with one sub per body (`body_1..body_n`), each
  realized by `step_body(…)`; the product structure has no whole-compound geometry of its own. A
  general `solids(g)` selector is **out of scope**: this PRD would be its only consumer and does
  not need it (and `bodies` is already taken by `std.mechanism`).
- **D10 — Edits live in the user's design, as a specialization body**: dotted override paths
  (`a.b.geometry = expr`) and `exclude <path>`. A modify-the-original edit names a hidden pristine
  instance explicitly (`aux sub stock`), because **`default`-as-a-value is out of scope**
  (orthogonal and strictly additive; a later PRD may add it). Datums for mating are **scope-level
  `let`s** in the design computed from scope-posed sub-member geometry reads (belt §7.1.5,
  #5436) — no `in_frame`, no `.world_frame` — and `relate` accepts such a datum as its fixed-side
  operand (κ). Specialization-body `let`s are not used. Derived bodies get the same dotted
  overrides (ι), gated on the derivation lowering and planes.
- **D19 — A geometry override value is expressed in the specializing scope's frame** and the
  engine transports it into the target descendant's local frame (the inverse of the descendant's
  composed pose) when it replaces the descendant's realization (*review-driven*, closes Q5).
  Under §7.1.5 every operand an author can name in that scope is already scope-posed, so
  `difference(stock.wall.panel.geometry, door_cut)` reads exactly as it looks.
- **D20 — δ delivers grammar and AST only; ε owns every import rejection** (*review-driven*, G4):
  the CST→AST lowering's only channel renders as a bare `Parse error:`, so the coded diagnostics
  `E_IMPORT_UNSUPPORTED_FORMAT`, `E_STEP_IMPORT_NEEDS_ALIAS`, `E_STEP_IMPORT_NOT_FOUND`,
  `E_STEP_IMPORT_PATH_COLLISION` all belong to the module-loading path (ε), checked in that order
  (format and alias before the path is resolved).
- **D11 — The compiler never links a kernel** (*crate split review-driven*). The header codec, the
  `ForeignModuleProvider` trait and the module-text emitter live in a new kernel-free crate
  `reify-foreign-module`, used by the compiler; the OCCT-backed provider lives in a new crate
  `reify-step-import` (depends on `reify-kernel-occt`), installed by the CLI and GUI only.
- **D12 — String paths in `.ri` resolve relative to the file containing the literal.** First
  design-relative path convention in the language; applies to the import statement (ε) and to
  `step_solid`/`step_body` paths (β owns threading the filesystem path to builtins).
- **D13 — `#4289` is absorbed as leaf α** (id kept; description, signal, files and deps
  rewritten): XDE reader, not the plain single-shape reader; `step_import(path) -> Solid` is not
  built.
- **D14 — Parts are realized by `step_solid(path, product_name, source_hash)` /
  `step_body(path, product_name, source_hash, index)`**, public builtins that generated code calls.
  Identity for the op and every cache is `(source_hash, product_name, body)`, never a path. Products
  are addressed by **name** (with a deterministic disambiguator for duplicate product names), never
  by XCAF label entry, which depends on traversal order (*review-driven*).
- **D15 — Provenance via the existing BOM surface, one record.** `Provenance` gains
  `source_hash : String = ""`; the generated root carries `sub source = STEPInput(source: …,
  provenance: Provenance(source_tool: "reify step-import", source_version: "<OCCT SONAME>",
  source_hash: …, …))`, so `reify report --bom` lists it. No second realization-level record.
- **D16 — Zero-solid products are omitted loudly, not fatal** (*review-driven*): a surface-only or
  wire-only product yields `W_STEP_PRODUCT_NO_SOLIDS` naming it, and the rest imports.
- **D17 — `exclude` has one mechanism and one diagnostic set** (*review-driven*): θ delivers the
  descendant-exclusion mechanism for specialization bodies with the assembly-derivation toolbox's
  D5 semantics and diagnostics (`E_DERIVED_SUB_UNKNOWN_DISPOSITION_PATH` generalised to
  `E_DISPOSITION_UNKNOWN_PATH`); the toolbox's derived-body `exclude` (#6618) consumes it.
- **D18 — Scope split-off:** the face position-threshold selector (“all +Z faces above height
  h”, gap 10 of the 2026-09-26 gap analysis) and the specialization-body-`let` silent-drop bug are
  filed as standalone tasks, not part of this PRD.

---

## 5. Contract (B+H)

### C1 — Kernel reader (reify-kernel-occt)

- New FFI `read_step_document(path) -> StepDocument` on the OCCT kernel thread, holding
  `g_step_export_mutex` for the duration of the read: sets `xstep.cascade.unit = "M"` under the
  mutex, runs `STEPCAFControl_Reader` with name mode on, keeps the XCAF document alive inside the
  kernel, keyed by source content hash (C4).
- `StepDocument` exposes a plain Rust **product tree** (no OCCT types cross the FFI):
  `ProductNode { name: String, dedupe_index: u32, kind: Assembly | Part { solid_count: u32 },
  components: Vec<Component> }`, `Component { product: (name, dedupe_index), instance_name:
  String, location: Placement }`, `Placement { translation: [f64; 3] /* metres */, rotation:
  [[f64; 3]; 3] }`. Free roots are all reported; the generator picks the root (Q7).
- Shape access by `(document, product, body_index)` returning kernel handles in **product-local**
  coordinates (the component placement is the sub's pose).
- Failure: unreadable file or zero free roots → typed error naming the file. A product with zero
  solids is reported in the tree (`solid_count: 0`) and skipped by the generator with
  `W_STEP_PRODUCT_NO_SOLIDS` (D16).

### C2 — Generated module (reify-foreign-module: codec + emitter; pure text)

1. **Input:** a product tree (C1), the STEP path relative to the generated file, the freshness key
   (D4), and the previous generated text if any. **Output:** module text + a structured delta.
2. **Header** (comment lines, machine-parsed by one codec shared with the compiler): generator
   version, reader major.minor, source path, `source-sha256`, `body-sha256` (covers every line
   after the header, identity table included), OCCT SONAME, and the identity table
   (`identifier = instance name @ product name#dedupe`, plus `retired`, `renamed`, `rematched`
   records with the regeneration timestamp).
3. **Body:** `import std.io.{STEPInput, Provenance}` (explicit, so strict visibility never breaks
   it); `pub structure def Assembly` (root); `pub structure def Flat`; one `pub structure def` per
   product (sanitised product name, collision-suffixed; all members `pub`). Part products:
   `param geometry : Solid = step_solid("<path>", "<product>", "<source-sha256>")`. Assembly
   products: one `sub <ident> = <Product>() at transform3(orient_basis(x, y, z), vec3(…))` per
   component. Multi-body products: `sub body_k = <Product>__Body<k>()` (each a one-param
   structure over `step_body`). Root adds `sub source = STEPInput(…)` (D15).
4. **Poses:** translation in millimetres with 12 significant digits; rotation as three orthonormal
   basis vectors with 15 significant digits, orthonormalised (Gram–Schmidt) by the generator so
   `orient_basis`'s guards never reject a vendor matrix; a det<0 placement (forbidden by AP242 and
   unrepresentable as a rigid pose) → `E_STEP_IMPROPER_PLACEMENT` naming the occurrence.
5. **Identity** (D7, D8) and **determinism:** same inputs → byte-identical text.

### C3 — Foreign-module provider (reify-foreign-module trait; reify-compiler resolution; hosts)

- `trait ForeignModuleProvider { fn materialize(&self, req: &ForeignImport) -> Result<Materialized, ForeignError>; }`
  with `ForeignImport { importing_file, importing_module, literal_path, resolved_source_path,
  generated_path, module_key, freshness: FreshnessKey }` and `Materialized { generated_path,
  delta: Option<Delta> }`. A provider is optional; `locked: bool` is set by the host.
- Resolution of a string-path import, in the module-loading path owned by `compile_program`
  (#5516) — until it lands, the same steps hook `ModuleDag::compile_module`, and every rejection
  below is a coded compile diagnostic emitted here (D20), never a bare parse error: (0) extension
  not in {`step`, `stp`} → `E_IMPORT_UNSUPPORTED_FORMAT`; missing alias →
  `E_STEP_IMPORT_NEEDS_ALIAS` (both checked before the path is touched, so a fixture need not
  exist on disk); (1) resolve the literal against the importing file's directory (D12) →
  `E_STEP_IMPORT_NOT_FOUND`; (2) derive the generated path and module
  key (D3) → `E_STEP_IMPORT_PATH_COLLISION` on conflict; (3) compute the freshness key (source hash
  memoised per process by path, size and mtime); (4) if the generated file exists, its header
  parses, its freshness key matches and its body hash verifies → compile it, no provider call;
  (5) else if unlocked and a provider is installed → `materialize`, emit
  `I_STEP_MODULE_REGENERATED` with the delta, compile the result; (6) else → `E_STEP_MODULE_STALE`
  (or `E_STEP_MODULE_MISSING`). Body hash mismatch → `E_GENERATED_MODULE_EDITED`, no overwrite.
- Hosts: CLI and GUI install the OCCT provider; the LSP installs none; all honour locked mode.
  The GUI watches every foreign source path an open design imports (its watcher is non-recursive
  and `.ri`-only today) and reloads on change (ζ).

### C4 — Realization of imported parts (reify-ir, reify-compiler, reify-eval, reify-kernel-occt)

- `GeometryOp::StepSolid { source_hash, product, body: Option<u32>, path }` (identity excludes
  `path`) with its descriptor row, compiler arm for `step_solid`/`step_body` (the literal path is
  resolved against the calling file, D12, at compile time), and an explicit OCCT execute arm;
  non-OCCT kernels reject it by name (routing sends it to OCCT via its capability descriptor).
- **Document cache** in the OCCT kernel keyed by source content hash, lifetime = kernel lifetime
  (Q3): N parts from one file parse it once. A `source_hash` argument that does not match the file
  on disk → `E_STEP_SOURCE_CHANGED` (belt-and-braces behind C3).
- Units: every returned shape and location is in metres (C1); export after import still writes
  millimetre STEP (#6186 unchanged).

### C5 — Import grammar and AST (tree-sitter-reify, lezer mirror, reify-syntax, reify-ast)

- `import_declaration.path` becomes `choice($.import_path, $.string_literal)`; the alias stays
  optional in the grammar so the compiler (ε, C3 step 0) can emit `E_STEP_IMPORT_NEEDS_ALIAS`.
- `ImportDecl` gains a source discriminator (`Dotted(String) | File(String)`); `lower_import`
  produces it and emits **no** diagnostics of its own; binding collection binds the alias;
  go-to-definition and completion resolve `ImportKind::Aliased` (λ). Until ε lands, a File import
  compiles to the same unresolved-import diagnostic an unknown dotted module gets today.

### C6 — Specialization-body edits (tree-sitter-reify, lezer mirror, reify-compiler, reify-eval)

- **Grammar:** `param_assignment.name` (and `derived_param_assignment.name`, leaf ι) widens to a
  dotted path.
- **Resolution:** the path resolves through the uniform member-path resolver (uniform-member-access
  C1, #5424 landed): every hop but the last is a sub of the previous hop's structure; the last is a
  param. Unknown hop → a diagnostic naming the hop and its concrete type; `priv` enforced per hop
  (generated members are all `pub`).
- **Scope and frame (D19):** the right-hand side is evaluated in the scope of the structure that
  contains the `sub` declaration (`Site` above), so it may read sibling subs (`stock.…`, needing
  #5427 for depth ≥ 2), and every sub-member read it makes is scope-posed (belt §7.1.5). A
  geometry-typed override value is therefore expressed in that scope's frame; when it replaces the
  target descendant's realization the engine transports it into the descendant's local frame
  (inverse of the descendant's composed pose, #5436's carried frame). No author-side `in_frame`.
- **Geometry override replaces realization ops (η owns this).** A `Solid`-typed param's
  realization ops are compiled from its default expression (§3.3), so an override must compile
  the override expression's ops and substitute them for the descendant's — on the specialization
  arm (dotted and flat) and on the constructor-arg arm, since both are silently ignored today.
  #6592 supplies the recursive overlay and re-realization trigger the substitution rides on.
- **Semantics:** `a.b.p = e` in the body of `sub s : T` ≡ `s`'s descendant `a.b` instantiated with
  `p = e`; the override threads to the descendant's realization (instantiation-value-flow, #6592).
  A duplicate override of one path keeps the house rule — a warning, first assignment wins (task
  4694 pins) — for any path length. An override under an excluded ancestor → error. The
  unknown-hop / unknown-param diagnostic fires in **both declaration orders** (η retires the
  optimistic forward-declaration injection in `entity.rs` Case 1). The AST carries the path as
  structured data: `SubParamOverride.name` widens from a single `SpannedIdent` to a path
  (`crates/reify-ast/src/decl.rs`; consumers such as `crates/reify-lsp/src/references.rs` follow),
  never a dotted string (heuristic 12).
- **`exclude <path>`** removes that descendant from the instance: no realization, not surfaced,
  not exported, not in the BOM. Unknown path → `E_DISPOSITION_UNKNOWN_PATH`; a reference elsewhere
  (override, relate operand, member read) to an excluded descendant → `E_REFERENCE_TO_EXCLUDED`
  naming the exclusion (D17).

### C7 — Scope-level datum operands for `relate` (reify-eval relate_solve, reify-compiler)

- `decode_operand` additionally accepts, on the **fixed** side only, a bare identifier or member
  path whose static type is `Plane` or `Axis` and which is a `let` of the enclosing structure
  (not a sub member). It is evaluated per instance in the enclosing scope's frame (already the
  frame `relate` poses `at auto` subs in); a datum computed from a sub-member geometry read is in
  that frame by §7.1.5 without any transport. The belt's pose-cycle rule is unchanged: a datum
  that reads `.world_frame` of any same-scope sub is `E_POSE_CYCLE`; the idiom in §1 reads
  geometry, not `.world_frame`. The unknown side is unchanged.
- **Discriminating fixture:** the datum's face lives on a nested sub whose composed pose includes
  a rotation, and the test asserts the solved bracket pose against an independently computed
  expected plane (pose ∘ local face plane), not merely against `door_plane` itself.

### C8 — Cross-module member completion and aliased go-to-definition (reify-lsp)

- `completion()` resolves the document's imports through `ModuleResolver` (as go-to-definition
  does) and offers members of imported modules after `alias.` and members of imported structures
  after `self.<sub>.`, without OCCT (reads committed generated files). `goto_definition` resolves
  `ImportKind::Aliased` targets.

---

## 6. Boundary-test sketch (two-way; ν's signal is the union)

| # | Scenario | Pre | Post |
|---|---|---|---|
| B1 | kernel reads the evidence fixture **inside the gmsh-linked workspace test binary** | α | tree has root `Container`, 4 `CornerCasting` components on one product, `SideWall` sub-assembly, `Weldment` with 2 solids; casting bbox 0.178×0.162×0.118 m; a rotated occurrence's rotation matrix round-trips |
| B2 | `step_solid` re-export round trip | β | `reify build -o out.step` of a `.ri` calling `step_solid` on one planar product re-exports; parsed AABB matches the product within 1e-6 m |
| B3 | one parse for many parts | β | realizing every part of the fixture parses the file once (kernel document-cache build count = 1) |
| B4 | reader restores the unit static | β | after `step_solid` realizes, `xstep.cascade.unit` reads back as its pre-read value (test-only FFI getter, same test fn); the writer itself is immune because it sets per-model units (#6186), so no writer canary is claimed. The stale-hash rejection (`E_STEP_SOURCE_CHANGED`) has its own fixture |
| B5 | first import generates | ε | `reify check site.ri` (minimal ε example) with no generated file writes `generated/container.ri`, emits one `I_STEP_MODULE_REGENERATED`, exits 0 |
| B6 | unchanged STEP is free | ε | second `reify check` makes no provider call, leaves the file byte-identical |
| B7 | vendor change with identity preserved | ε | moving one casting within the bound regenerates; delta names it; identifiers unchanged; moving it beyond the bound → `rematched` record and `W_STEP_IDENTIFIER_REMATCHED` on a design that overrides it |
| B8 | locked and edited | ε | `--locked` with a stale module → `E_STEP_MODULE_STALE`, no write; a hand-edited module → `E_GENERATED_MODULE_EDITED`, no write |
| B9 | rejections fire | ε | `E_STEP_IMPORT_NOT_FOUND`, `E_IMPORT_UNSUPPORTED_FORMAT`, `E_STEP_IMPORT_NEEDS_ALIAS`, `E_STEP_IMPORT_PATH_COLLISION`, `E_STEP_SOURCE_CHANGED` each observed on its own fixture (negative assertions) |
| B10 | no-OCCT host, stale file | ζ | LSP publishes `E_STEP_MODULE_STALE` after the STEP changes; GUI reload after a STEP edit shows the regenerated module's parts |
| B11 | geometry override takes effect, flat and dotted | η | (a) flat twin `sub q : Plate { geometry = cylinder(50mm, 100mm) }` and the ctor-arg twin `Plate(geometry: cylinder(…))` build to the cylinder (STEP `CYLINDRICAL_SURFACE` present, `reify eval` volume ≈ π·0.05²·0.1 m³, today 1e-6 m³); (b) `sub s : Outer { mid.leaf.geometry = cylinder(…) }` builds the same; unknown hop → `reify check` exit 1 with a diagnostic naming `bogus` and the hop's type, **in both declaration orders** (a forward-declared child is silently accepted in release and panics in debug today); duplicate path → the pinned first-wins warning |
| B12 | exclude, depth 1 with a sibling solid | θ | `sub s : Outer { exclude leaf }` where `Outer { sub leaf; sub other }`: STEP export has one solid (control two) and GUI `mesh_stats` one body; `exclude nope` → `E_DISPOSITION_UNKNOWN_PATH`; a bare sub reference `let r = s.leaf` declared inside the containing structure (the only descendant read that resolves today; there is no module-level `let`) → `E_REFERENCE_TO_EXCLUDED`; dotted exclusion paths are exercised once #5427 lands |
| B13 | mating to a scope-level datum, off-axis | κ | a bracket `at auto` with `relate { flush(bracket.seat_plane, door_plane) }`, `door_plane = single(faces_by_normal(stock.wall.panel.geometry, vec3(0.0, 1.0, 0.0), 1deg)).plane` on a **rotated, nested** sub, solves; the solved seat plane (read as `in_frame(bracket.seat_plane, bracket.world_frame)`, since a sub's datum members stay local) has signed point-to-plane distance < 1e-6 m from the independently computed expected plane (pose ∘ local face) and a parallel or antiparallel normal — never origin coincidence, which `flush` does not fix; `E_POSE_CYCLE` observed on `reify check` stderr |
| B14 | completion and go-to-definition across imports | λ | completion after `container.` lists `Assembly`, `Flat`, product structures; after `self.box.` lists occurrence subs; go-to-definition on `container.Assembly` lands in the generated file |
| B15 | the container site end to end | ν | `examples/step_import/site.ri` against a container-shaped fixture: `reify build -o site.step` exports the cut wall, no roof panel 3, the bracket on the door plane; `reify report --bom` lists the STEP row with its hash |
| B16 | scale | ν | a generated 500-occurrence fixture imports: parse count 1, hash computed once per compile, compile and build succeed; timings recorded, not bounded |

---

## 7. Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam | Owner |
|---|---|---|---|
| `resolution-unification.md` β #5516, γ #5517, δ #5518, ζ #5520 | **hard prerequisite** | one compile entry point; multi-file `build`/`eval`/`report`/LSP diagnostics | resolution-unification; C3 hooks its module-loading path |
| `stdlib-namespace` ν #5505 | **hard prerequisite** | `alias.Name` qualified-reference resolution | stdlib-namespace |
| `io-export-import-completion.md` ζ (#4289) | supersedes | single-shape STEP import | **this PRD** (α absorbs #4289; ο annotates the old PRD) |
| `uniform-member-access.md` (#5424 landed; #5427, #5430 pending) | consumes | dotted-path resolution for C6; two-hop geometry reads in edits | UMA owns the resolver and #5427; **this PRD** owns override-path semantics |
| `instantiation-value-flow.md` (#6592, #6586; #6610 deferred) | consumes / **extends** | IVF supplies the recursive overlay and re-realization trigger; **η owns** replacing a `Solid` param's default-compiled realization ops with the override expression's (both arms), which IVF's scope does not cover (§3.3) | IVF (overlay) / **this PRD** (op substitution) |
| #6598 | consumes | re-realization of an overridden child that reads its own sub's body | #6598 |
| `placement-relations-belt.md` β #5436 | consumes | §7.1.5 carried frames (scope-posed sub-member reads) for the datum idiom and D19's transport; the pose-cycle rule is untouched | the belt |
| `assembly-derivation-toolbox.md` (#6616, #6617, #6618 deferred) | **produces** (θ) / extends (ι) | θ delivers the descendant-exclusion mechanism + diagnostics that #6618's derived-body `exclude` consumes (D17); ι adds dotted overrides to derived bodies | **this PRD** owns the mechanism; the toolbox owns derived-body lowering |
| `naming-convergence/P0` (D1 selectors-as-names, D4 no labels) | conforms | faces of imported parts are addressed by predicate selectors; part names are sub identifiers, not labels | no contest |
| `io-import-pointcloud.md` (deferred) | re-points | its "geometry-import seam" is C3 here | **this PRD** (ο edits the stub) |
| `indexed-sub-instantiation.md`, `keyed-collection-identity.md` | declined | duplicates are named subs (D7) | no seam |

No new contested-ownership pair: topology-selectors ↔ persistent-naming is untouched (imported
geometry is unseeded and resolves through the existing computed fallback).

---

## 8. Decomposition plan

Task ids assigned at decompose (2026-09-30): α #4289 (rewritten), β #8055, γ #8056, δ #8057, θ #8058, ε #8059, ζ #8060, η #8061, ι #8062, κ #8063, λ #8064, ν #8065, ξ #8066, ο #8067, π #8068. Standing rules for every leaf: a new gate-resident
Rust integration test or `tests/infra` member ships its drift-guard registrations in the same diff
(nextest partition entries, `scripts/check-harness-baseline-registration.sh`, the
`run-all-classification.manifest` row); a `tests/prd-gate/fixtures/*.ri` read by a Rust test is
registered in `_RUST_COUPLED_RI_FIXTURES`, and one pinned in `reifyGrammarCorpus` `EXPECTED_CLEAN`
(`gui/src/__tests__/reifyGrammarCorpus.test.ts`) in `_GUI_COUPLED_RI_FIXTURES`, both in
`scripts/verify.sh` in the same diff (a verify-pipeline file: that diff takes the full gate);
every grammar leaf updates the lezer mirror and that pin, and pins its corpus case with a Rust
`include_str!` test in `tree-sitter-reify/tests/` (the `derived_sub_grammar_tests.rs` pattern),
because `test/corpus` is not CI-run. One fixture per leaf — no leaf's fixture may need another
leaf's grammar.

### Phase 1 — Foundations (no unlanded prerequisites)

- **α (#4289, rewritten) — OCCT XDE STEP reader + product tree + units.** *Modules:*
  `crates/reify-kernel-occt/cpp/occt_wrapper.cpp`, `…/occt_wrapper.h`, `…/src/ffi.rs`,
  `…/src/lib.rs`, `…/build.rs` (TKXCAF, TKLCAF), `scripts/check-manifold-deps.sh`, fixtures
  `crates/reify-kernel-occt/tests/fixtures/step_assembly_small.step` (the evidence fixture) and a
  second one with a rotated occurrence and a rotated sub-assembly (both produced with
  `step-assembly-import.evidence/writer.cpp`, extended). *Intermediate* (unlocks β, γ, ε).
  *Unlock signal:* B1 as an OCCT-gated kernel integration test in the workspace. *Prereqs:* —.
- **β (#8055) — `step_solid`/`step_body` op, kernel document cache, path threading, provenance field.**
  *Modules:* `crates/reify-ir/src/geometry.rs`, `crates/reify-compiler/src/geometry.rs`,
  `crates/reify-eval/src/geometry_ops.rs`, `crates/reify-kernel-occt/src/lib.rs`, the
  module→filesystem-path threading (D12), `crates/reify-compiler/stdlib/io.ri`
  (`Provenance.source_hash`, D15 — ξ documents it). *Leaf.* *Signal:* B2 via `reify build` on a
  committed single-file `.ri` that calls `step_solid` directly (no import statement needed); B3;
  B4; `E_STEP_SOURCE_CHANGED` observed on a stale hash. *Prereqs:* α.
- **γ (#8056) — Foreign-module codec, provider trait, and module generator.** *Modules:* new
  `crates/reify-foreign-module/` (header codec, `ForeignModuleProvider`, emitter), new
  `crates/reify-step-import/` (OCCT-backed provider over α's tree). *Intermediate* (unlocks ε).
  *Unlock signal:* golden-text tests over the fixture trees: nested + Flat views, instance-name
  identifiers, multi-body subs, rotated poses, header + identity table; D8 preservation over
  moved-within-bound, moved-beyond-bound (rematched), removed and added occurrences; byte-identical
  regeneration. *Prereqs:* α.
- **δ (#8057) — String-path import grammar + AST.** *Modules:* `tree-sitter-reify/grammar.js` + corpus,
  `gui/src/editor/reify.grammar` + corpus pin, `crates/reify-syntax/src/ts_parser.rs`
  (`lower_import`), `crates/reify-ast/src/decl.rs`. *Intermediate* (unlocks ε, λ). *Unlock
  signal:* `tests/prd-gate/fixtures/step_import_stmt.ri` (the import line only) parses with 0
  ERROR nodes; a Rust `include_str!` test in `tree-sitter-reify/tests/` pins the CST; the lezer
  mirror, its pin and the `_GUI_COUPLED_RI_FIXTURES` row updated; the AST carries `File(path)` +
  alias (unit test). No diagnostics are δ's (D20). *Prereqs:* —.
- **θ (#8058) — `exclude <path>` in specialization bodies (the shared disposition mechanism).**
  *Modules:* grammar + lezer mirror, `ts_parser.rs`, specialization lowering, surfacing/export
  walk, diagnostics. *Leaf.* *Signal:* B12 on native structures at depth 1 with a sibling solid
  (a descendant param read through a sub-of-sub does not resolve today, so the
  reference-to-excluded probe uses a bare sub reference). *Prereqs:* —.

### Phase 2 — Vertical slice (gated on the resolution programs)

- **ε (#8059) — Provider wiring in the compiler, design-relative resolution, freshness, locked mode,
  CLI host.** *Modules:* `crates/reify-compiler/src/module_dag.rs` (or the #5516
  `compile_program` loading path), `crates/reify-cli/src/main.rs`, `crates/reify-step-import/`.
  *Leaf.* *Signal:* B5–B9 on a **minimal** `examples/step_import/minimal.ri` (`sub c =
  container.Assembly()`, no edits) against the small fixture, via `reify check` and `reify build`;
  B9's five rejections (including `E_IMPORT_UNSUPPORTED_FORMAT` and `E_STEP_IMPORT_NEEDS_ALIAS`,
  D20) are each observed on stderr with a non-zero exit. *Prereqs:* α, γ, δ, #5516, #5517, #5505.
- **ζ (#8060) — GUI and LSP host policy.** *Modules:* `gui/src-tauri/src/engine.rs` (+ watcher),
  `crates/reify-lsp/src/server.rs`. *Leaf.* *Signal:* B10. *Prereqs:* ε, #5520.

### Phase 3 — Editing and assembling

- **η (#8061) — Geometry overrides take effect; dotted override paths in specialization bodies.**
  *Modules:* grammar + lezer mirror + pin + `_GUI_COUPLED_RI_FIXTURES`, `ts_parser.rs`,
  `crates/reify-ast/src/decl.rs` (`SubParamOverride.name` as a path) and its consumers,
  `crates/reify-compiler/src/entity.rs` (Solid-param lowering, specialization lowering, the
  order-independent unknown-override diagnostic), eval threading and the D19 transport.
  *Leaf.* *Signal:* B11 — the flat and ctor-arg twins first (they parse today and are silently
  ignored), then the dotted form. *Prereqs:* #6592 (overlay + trigger), #5436 (carried frame for
  the transport).
- **ι (#8062) — Dotted overrides in derived bodies.** *Modules:* grammar (`derived_param_assignment`) +
  lezer mirror + `EXPECTED_CLEAN` pin + `_GUI_COUPLED_RI_FIXTURES`, `ts_parser.rs`, the shared
  path-typed AST override (from η), derivation lowering. *Leaf.* *Signal:* a copy-form fixture
  `aux sub stock = Outer(); sub c = image of stock under transform3_identity() { mid.leaf.geometry
  = … }` builds identically to its specialization-form twin (STEP grep + `mesh_stats`).
  *Prereqs:* η, #6616, #6617, #6618.
- **κ (#8063) — `relate` accepts scope-level datum operands.** *Modules:*
  `crates/reify-eval/src/relate_solve.rs` (`decode_operand`, operand realization), compiler
  typing of relate members. *Leaf.* *Signal:* B13 via `reify eval`, on native structures with a
  rotated nested sub (no import needed): point-to-plane distance and normal parallelism against
  an independently computed expected plane, the solved seat plane read through
  `in_frame(bracket.seat_plane, bracket.world_frame)`. *Prereqs:* #5436 (§7.1.5 scope-posed
  reads, `in_frame`), #5427.
- **λ (#8064) — Cross-module LSP member completion + aliased go-to-definition.** *Modules:*
  `crates/reify-lsp/src/server.rs`, `…/completion.rs`, `…/goto_def.rs`. *Leaf.* *Signal:* B14 on a
  plain two-module fixture (native), and on the committed generated module once ε has landed.
  *Prereqs:* δ, #5505.

### Phase 4 — Integration gate, docs, corrections, close

- **ν (#8065) — Container site integration gate (H) + scale.** *Modules:* `examples/step_import/site.ri`,
  `examples/step_import/vendor/container.step` (container-shaped: 4 castings, 2 side walls of 2
  panels each, 3 roof panels, a rotated sub-assembly; from the extended writer), a generated
  500-occurrence fixture, the committed `examples/step_import/generated/container.ri`, e2e tests,
  `examples_smoke` registration (its multi-file path or `SKIP_SET`). *Leaf.* *Signal:* B15, B16.
  *Prereqs:* β, ε, η, θ, κ, #5427, #6598, #5518.
- **ξ (#8066) — Docs-truth bundle.** *Modules:* `docs/reify-language-spec.md` (§7 file imports, D12 path
  rule; specialization-body dotted overrides, D19 frame rule and `exclude`; scope-level relate
  datums), `crates/reify-mcp/src/tools/chunks/` (`syntax.md`, `structures.md`, `geometry.md`,
  `connect.md` for relate), `docs/reify-stdlib-reference.md` §9 (the `STEPInput` and `Provenance`
  rows only — the phantom `result : Structure` and the `source_hash` field β adds; the rest of §9's
  drift, e.g. `DisplayStyle.color` and the deferred `PointCloudInput`, is out of scope and named
  in ο for the docs-truth program), `examples/best_practices/modify_assembly.ri` (native-structure
  idioms: dotted override, `exclude`, scope-level datum + `relate`; no file import, because
  `examples_smoke` is single-file — the import lives in ν's `examples/step_import/`) + `INDEX.md`
  line, `.claude/skills/reify-design/SKILL.md` index line. *Leaf.* *Signal:* every documented
  signature is diffed against its registry (`io.ri` for occurrences, the compiler registries for
  builtins) — not compile-success, which is vacuous for occurrence ctor kwargs (§3.3); the exemplar
  builds; an author searching the chunks for "use a vendor CAD model" / "modify an imported part"
  / "mate to an imported face" lands on the right chunk and corpus file; the exemplar carries its
  `EXPECTED_INDETERMINATE` row in `harness_corpus_gates/best_practices_constraint_gate.rs`.
  *Prereqs:* δ, ε, η, θ, ι, κ, λ, β, placement-relations-belt ν #5446 (the base at-auto + relate
  chunk docs this leaf extends — `relate` has no chunk today).
- **ο (#8067) — Companion corrections.** *Modules:* `docs/prds/v0_6/io-export-import-completion.md`
  (ζ superseded note), `docs/prds/v0_6/io-import-pointcloud.md` and
  `gdt-measured-feature-import.md` (seam re-point), `docs/prds/v0_6/assembly-derivation-toolbox.md`
  §10 (D17 seam note on #6618), `docs/prds/v0_6/instantiation-value-flow.md` §7 (η owns
  Solid-param realization-op substitution; IVF owns the overlay), and a note for the docs-truth
  program naming every §9 row ξ leaves untouched: `STEPOutput.path`, `STLOutput.path`,
  `ThreeMFOutput.path`, `STEPOutput.subject`, `ThreeMFOutput.subject`, `DisplayOutput.subject`
  (`Structure`/`Geometry` vs `Solid`), `DisplayStyle.color`, `DisplayStyle.finish`,
  `PointCloudInput`/`PointCloudFormat` (deferred PRD). *Leaf.* *Signal:* the edits are present and cross-reference this PRD.
  *Prereqs:* —.
- **π (#8068) — PRD close.** *Leaf.* *Signal:* the committed terminal header on this PRD and its
  capability manifest, in the ratified three-part freeze shape (terminal token + landed ids; the
  AS-AUTHORED sentence; the LIVE vs AS-AUTHORED map), with a cancelled sibling counting as
  satisfied. *Prereqs:* every other leaf.

Filed **out of batch** at decompose (own tasks, `prd_path` set, no label): the face
position-threshold selector; the specialization-body-`let` silent-drop bug (INV-SF-3).

### Dependency view

```
α ─┬─ β ────────────────────────────┐
   └─ γ ─┐                          │
δ ───────┼─ ε ─ ζ (+#5520)          │
#5516 #5517 #5505 ──┘               │
#6592 ─ η ─ ι (+#6616 #6617 #6618)  │
θ                                   │
#5436 #5427 ─ κ                     │
δ #5505 ─ λ                         │
β ε η θ κ #5427 #6598 #5518 ────────┴─ ν ─┐
δ ε η θ ι κ λ β ───────────────────── ξ ─┤
ο ────────────────────────────────────────┴─ π (depends on all)
```

---

## 9. Out of scope

- `default` as a value in override expressions (D10) — additive, later PRD. Until then the
  pristine `aux sub stock` costs a second realization and tessellation of the whole assembly
  (spec §4.7); `default`-as-value is the mechanism that would remove it.
- A general `solids(g)` / Body-kind selector and selector-results as boolean operands (D9).
- IGES, STL/OBJ/3MF-in, glTF, point clouds — later formats plug into C3.
- STEP-level face/edge names (AP242 semantic PMI) and colours/layers; faces of imported parts are
  addressed by predicate selectors (P0 D1).
- Product structure and instancing on **export** — the writer emits a flat compound today; a
  round-tripping writer is its own PRD.
- A GUI design-tree panel (the design-tree PRD is its home).
- Specialization-body `let`s (silent-drop bug filed separately, D18); the face position-threshold
  predicate (D18).
- Document-cache eviction policy beyond kernel lifetime (Q3).

---

## 10. Open questions (tactical)

1. **Sanitiser collisions across products** (`Side Wall` vs `Side-Wall`). Suggested: suffix by
   first-seen order within the previous generation's identity table. Decide at γ.
2. **Re-match distance bound** (D8). Suggested: the occurrence's own bounding-box diagonal.
   Decide at γ.
3. **Document-cache eviction.** Per-kernel lifetime vs LRU by bytes. Suggested: lifetime, with an
   LRU cap only if B16's recorded numbers show pressure. Decide at β.
4. **Duplicate product names inside one file** (two distinct products both named `Panel`).
   Suggested: `#2`, `#3` dedupe index in traversal order, recorded in the identity table so a
   re-export that swaps their order is reported as a rename. Decide at γ.
5. ~~Inverse frame transport for cut geometry~~ — **resolved by D19** (2026-09-30): the engine
   transports a scope-frame override value into the descendant's local frame; no author-side idiom.
6. **Where `--bom` shows the OCCT version.** `source_version` (D15) vs a separate field. Decide at β.
7. **Multiple free roots** in one STEP. Suggested: the first is `Assembly`; every root is also
   exported as its own structure; the delta reports the count. Decide at γ.
