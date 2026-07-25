# PRD: Resolution Unification — one compile entry point, one definition environment

Status: **active**. Authored 2026-07-24 in an interactive `/prd` session (Leo + investigation
session `investigate-reify-1284379`, spawned from the 2026-07-24 language review). Shape:
**B+H** (contract §3 + boundary-test sketch §7). Milestone: v0_6.

> Every claim marked *(probe)* was verified 2026-07-24 against the 2026-07-22 debug binary
> (`target/debug/reify`); probe files under the investigation session's scratchpad
> (`probe-unify/`, plus the language-review session's `probe-imports/`). Line numbers are
> against `main` at `366c63a679`. Implementers: rebuild before re-probing —
> `reify` embeds the stdlib via `include_str!`, so a stale binary carries stale stdlib.

---

## §0 — Goal and user-observable surface

Reify has **three coexisting template/definition-resolution regimes** and **seven+ compile
entry points**. This PRD collapses them to **one compiler entry point**
(`compile_program` → `CompiledProgram`) consumed by every command and surface
(`check`/`eval`/`build`/`test`/`report`/`explain`/`doc`, GUI, LSP diagnostics), and **one
eval-side definition environment** (`DefEnv`) with a single precedence rule
(entry > imports in declaration order > stdlib) across **all** definition kinds.

After this PRD lands, a user observes:

- `reify eval main.ri` / `reify build main.ri` on an entry that `import parts` — today:
  `warning: import "parts" not resolved by this entry point` +
  `error: sub-component "p" references unknown structure "Pulley"` *(probe)* — instead
  evaluates/builds the cross-file design exactly as `reify check` and the GUI already do.
  Same for `test`, `report`, `explain`, `doc`. All seven commands accept `--cfg` like
  `check` does.
- A cross-file `pub fn` call evaluates to its real value — today it **silently produces
  `undef`** (`op contract failed (OpContractViolation)`, exit 0) *(probe)*.
- A cross-file `pub enum` resolves at the importer for variant construction, type
  positions, and match (task #5387's acceptance).
- A library facade works: `lib.ri` can `pub import parts` and re-export parts' pub
  definitions to whoever imports lib (today `pub import` parses, `is_pub` is recorded at
  `reify-ast/src/decl.rs:725`, and is never consumed).
- The LSP stops emitting false `unknown structure` diagnostics on multi-file projects
  (its diagnostics path is single-file today: `reify-lsp/src/diagnostics.rs:132`).
- The GUI files panel lists imported files, and a module-path mismatch in an *imported*
  file is surfaced with file attribution (both silently dropped today).
- `examples/stdlib/ports_breadth.ri` evaluates the **real** stdlib `ThreadSpec`
  (including `thread_form`) instead of a drifted local mirror.

**Primary consumer:** the standard-parts & materials library program (bookmark task
**#5391**), whose textual gates (2) multi-file eval/build, (3) pub-enum crossing +
re-export story, (4) stdlib re-declaration hazard, are all delivered here. Libraries
cannot be libraries under single-file eval; the dogfood printer project
(`prj/printer_v01`) copy-pastes structures between files today.

---

## §1 — Background: the three regimes (investigation evidence)

**Regime A — merged templates (user imports).** `merge_imported_pub_templates`
(`reify-compiler/src/module_dag.rs:806`) copies direct imports' pub *templates* into
`entry.templates`. Called by exactly two bridges: the `check` bridge
(`compile_entry_with_stdlib_cfg_checked`, module_dag.rs:911) and the GUI's hand-rolled
twin (`compile_entry_with_imports`, `gui/src-tauri/src/engine.rs:885` — whose own doc
comment requests replacement by a compiler API). Covers **1 of ~9 definition kinds**:
`CompiledModule` also carries `enum_defs`, `functions`, `trait_defs`, `units`,
`type_aliases`, `constraint_defs`, `compiled_purposes`, `fields`
(`reify-compiler/src/types.rs:375`) — none cross the eval boundary.

**Regime B — prelude fallback (stdlib).** Stdlib templates are deliberately NOT merged
into `compiled.templates` (presentation purity — entity enumeration must not list
hundreds of stdlib defs). `find_template_with_prelude`
(`reify-eval/src/engine_eval.rs:59`) falls back to the static `Engine::prelude`
(stdlib-only, `reify-eval/src/lib.rs:447`) at ~5 retrofitted call sites (engine_eval.rs
:3968/:5536/:7003, engine_build.rs:4842, bom_report.rs:165). ~12 deeper sites still take
a flat `&[TopologyTemplate]` and cannot see the prelude: unfold.rs:214/:538/:586,
graph.rs:425 (**silent skip**), structural_query.rs:189/:215/:590/:673,
engine_build.rs:586/:872/:10593, engine_eval.rs:1290.

**Regime C — single-file (everything else).** `reify eval/build/test/report/explain/doc`
use `parse_and_compile` (main.rs:173) — no import DAG at all. LSP diagnostics likewise.
GUI `load_from_source` likewise.

**The re-declaration hazard is dead but its scar tissue remains** *(probe)*: stdlib
constructions at eval (`sub`/`let`/port-param positions, enum constructors) all resolve
via Regime B now; a mirror-stripped copy of `ports_breadth.ri` evaluates clean. The
file's local `ThreadSpec` mirror (which shadows stdlib and lacks `thread_form`) and its
"EVAL-TIME RE-DECLARATION NOTE" (lines 16–24) document dead semantics and actively drift.

**Collision-policy fragmentation is documented in-tree** (`prelude_context.rs:32-74`,
task 2776): units **last-wins**+warn, aliases first-wins+warn, functions
first-wins-**silent**, templates first-wins+warn — four policies for one concept.

**Enum-disambiguation wrinkle (root of #5387's check-time symptom):**
`FitClass.Clearance` from an imported module fails at *check* with
`unresolved name: FitClass` because enum-vs-member disambiguation happens at **parse**
(`parse_with_stdlib` pre-seeds stdlib enum names only); imported enums aren't known until
after the import scan, so the reference parses as `MemberAccess`, never `EnumAccess`.
Fix shape is a resolution-phase fixup, not a parse re-run — see D-9.

---

## §2 — Sketch of approach

Three phases inside one PRD, strictly ordered (P2 runs against P1's landed test net;
P3's exposure-policy features build on P2's env):

- **P1 — `compile_program` + consumers.** Promote the check bridge to the canonical
  API returning `CompiledProgram`; point all seven CLI commands, the GUI, and LSP
  diagnostics at it; delete the GUI twin. Eval consumption is unchanged in P1: it reads
  the merged `entry.templates` exactly as check/GUI-fed eval does today (proven in
  production by the GUI). **No new merge logic is written in P1** — the phase
  *net-deletes* one of the two existing callers of the shared merge helper. A `DefEnv`
  skeleton (type + `resolve()`/`declared()` backed by the merged list) ships in P1 so
  P2 is a migration onto an existing API and the namespace-PRD hook exists early.
- **P2 — `DefEnv` for real.** `Engine::load` consumes `CompiledProgram`; one
  precedence-ordered `resolve_*` per definition kind (templates, functions, enums,
  units, aliases, traits, constraint defs, purposes); audit + migrate the flat-slice
  call sites (lookup → `resolve`, iterate → `declared`, silent skips → structured
  diagnostics); then **delete** `merge_imported_pub_templates` consumption and
  `find_template_with_prelude`, restoring `entry.templates` purity. Normalize collision
  policy (D-6).
- **P3 — exposure policy.** `pub import` re-export (transitive through pub edges),
  entity-import narrowing, imported-file module-header mismatch surfacing, GUI files
  panel from `CompiledProgram.sources`.

No novel grammar anywhere: `pub import X`, `import parts.Pulley`, `import parts as pp`
all parse today (tree-sitter fixture, 0 ERROR nodes, verified 2026-07-24). **G3: no
grammar prerequisites.** Qualified references (`pp.Pulley`) are explicitly **out of
scope** (owned by the stdlib-namespace/shadowing PRD, §6).

---

## §3 — Contract (H)

### §3.1 `CompiledProgram` and `compile_program`

```rust
// reify-compiler
pub struct CompiledImportUnit {
    pub import_path: String,        // as written: "parts", "lib.fasteners"
    pub module: CompiledModule,
    pub origin: Option<PathBuf>,    // None for embedded-stdlib-resolved units
}

pub struct CompiledProgram {
    pub entry: CompiledModule,      // diagnostics aggregated here (existing contract)
    pub imports: Vec<CompiledImportUnit>, // followed direct imports, declaration order
    pub sources: Vec<(String /*module key*/, PathBuf, String /*source*/)>,
}

pub fn compile_program(
    entry_path: &Path,
    entry_source: &str,             // dirty-buffer capable; entry_path need not exist
    resolver: &ModuleResolver,
    cfg: &CfgSet,
    checker: &dyn ConstraintChecker,
) -> CompiledProgram
```

Internals = today's `compile_entry_with_stdlib_cfg_checked` (parse via
`parse_with_stdlib`, cfg-gated DAG walk, stdlib + user-import prelude, compile,
`attach_module_path_diag`, merge) — extended to *return* the DAG modules and sources
instead of discarding them.

**Invariants** (each pinned by a test in the phase that establishes it):

- **I1 (single-file equivalence).** Import-free input ⇒ `entry` has identical
  diagnostics and definitions to `compile_with_stdlib_checked` output. This is the P1
  rollout-safety pin.
- **I2 (diagnostics-embedded).** A satisfied-but-broken import surfaces Error
  diagnostics on `entry.diagnostics` with the *imported file* attributed; the entry
  still compiles (preserves module_dag.rs:868-875 contract). Each CLI command's
  existing exit-code policy is preserved as-is in P1 (the error⇒nonzero *invariant*
  belongs to the silent-failure investigation / #5403 — see §6).
- **I3 (cfg inertness).** A cfg-unsatisfied import is never resolved, compiled, merged,
  or listed in `imports`/`sources`.
- **I4 (stdlib skip).** `std`/`std.*` imports are never DAG-walked (stdlib is always in
  the environment).
- **I5 (interim merge, P1 only).** `entry.templates` contains merged direct-import pub
  templates under the existing policy, verbatim. **Post-P2:** `entry.templates` =
  entry-declared only; cross-module exposure moves to `DefEnv`. I5's deletion is a
  named task (κ), not an aspiration.
- **I6 (precedence).** entry > imports in declaration order > stdlib, for every
  definition kind. Collisions: first-wins + `Severity::Warning` naming both origins
  (D-6; P1 keeps per-kind status quo, P2 normalizes).
- **I7 (env shape).** `DefEnv` is keyed internally by `(module_key, name)`; the flat
  precedence view is derived. Qualified lookup (namespace PRD) extends it without
  re-plumbing call sites.

### §3.2 `DefEnv` (skeleton in P1, authoritative in P2)

```rust
// reify-eval (skeleton lives beside Engine; compiler stays presentation-agnostic)
pub struct DefEnv { /* entry + imports + &'static stdlib, precedence-ordered */ }

impl DefEnv {
    pub fn from_program(program: &CompiledProgram) -> Self;
    // Resolution view — the ONLY name-lookup path post-P2:
    pub fn resolve_template(&self, name: &str) -> Option<&TopologyTemplate>;
    pub fn resolve_function(&self, name: &str /* + sig */) -> …;
    pub fn resolve_enum(&self, name: &str) -> Option<&EnumDef>;
    // …units, aliases, traits, constraint defs, purposes: same pattern.
    // Presentation view — entity enumeration, doc, BOM, GUI tree:
    pub fn declared(&self) -> &CompiledModule; // the entry, pure post-P2
}
```

Consumption rule post-P2: a call site either **resolves a name** (→ `resolve_*`) or
**enumerates the user's declarations** (→ `declared()`); passing a raw template slice
across a subsystem boundary is retired. `Engine::prelude`, `prelude_functions`, and
`find_template_with_prelude` are deleted; `merge_functions` folds into env construction.

---

## §4 — Resolved design decisions

- **D-1 (one PRD, three phases).** Leo 2026-07-24: plan P1–P3 in one go; internal hard
  dep edges enforce sequencing so the interim state cannot ossify.
- **D-2 (P1 eval consumption = existing merge).** Not duplication: P1 writes no new
  resolution logic, reuses the single shared helper, deletes its GUI twin caller.
  Everything P2 deletes exists on main today.
- **D-3 (DefEnv skeleton in P1).** P2 migrates onto an API that already exists; the
  namespace PRD gets its hook early.
- **D-4 (--cfg parity).** All seven commands take repeated `--cfg` via the same
  `build_cfg_set` threading as `check`; `target` host-defaults everywhere
  (conditional-compilation PRD §4 D-2 semantics unchanged). GUI stays
  `CfgSet::host_default()` (no cfg selector in v1).
- **D-5 (visibility policy: 1-hop + explicit re-export).** Direct imports expose their
  pub defs; transitivity only via `pub import` chains (D-7). Rust-like, principled,
  matches current 1-hop behavior for plain imports.
- **D-6 (collision normalization, P2).** First-wins + Warning for **all** kinds; entry
  always wins; imports in declaration order; stdlib last. Units flips last-wins →
  first-wins — a deliberate breaking change; the existing pins
  (`prelude_module_unit_collision_emits_warning` et al., `unit_registry_tests.rs`)
  are inverted in the same diff, and the prelude_context.rs policy-divergence doc
  block is replaced by a pointer to DefEnv. Functions gain the warning they never had.
- **D-7 (`pub import` = re-export edge).** In `compile_program`'s frontier walk, a pub
  import in module M extends M's exported surface with the target's pub defs,
  transitively through pub edges (cycles already impossible: DAG check). Non-pub
  imports never leak (boundary test #8).
- **D-8 (entity-import narrowing = exposure filter).** `import parts.Pulley` narrows
  what the *importer* sees to the named def(s); parts' internal resolution (Pulley's
  own params/subs referencing parts-private types) is untouched — narrowing filters
  the exposure set, not the defining module's environment. `ImportKind` already
  carries the data (today consumed only by LSP).
- **D-9 (imported-enum disambiguation is a resolution-phase fixup).** Post-import-scan,
  a `MemberAccess { object: Ident(E), member: V }` where `E` resolves to an enum in the
  env is rewritten to `EnumAccess` during compilation — no parse re-run, no grammar
  change. This is the load-bearing mechanism for λ / #5387.
- **D-10 (`load_from_source` honesty).** Buffer-only compiles (no file anchor) get an
  explicit Warning when the source contains an import ("import requires a file-backed
  buffer"), replacing today's silence. File-backed GUI loads resolve imports (already
  true; preserved through the twin deletion).
- **D-11 (exit-code invariant deferred).** The "any Error diagnostic ⇒ nonzero exit"
  invariant is owned by the placeholder/silent-failure investigation (#5386 notes it;
  dep #5403). P1 preserves each command's current policy; the parity harness (η)
  asserts *diagnostic-set* equality across commands, not exit-code equality.
- **D-12 (doc/report presentation scope).** `declared()`-only in this PRD: `reify doc`
  documents the entry's own defs. Multi-module doc output is a tactical follow-up
  (§10 Q2).

---

## §5 — Pre-conditions for activating

None hard. Substrate: all required syntax parses today (§2); the merge mechanism, DAG
walk, cfg gating, and prelude machinery are landed and production-exercised. Probe
freshness caveat: tasks re-probing behavior must rebuild (`include_str!` stdlib).

---

## §6 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #5391 standard-parts library program (bookmark) | consumes | multi-file eval/build (γ), enum crossing + re-export (λ, μ), mirror-free stdlib eval (α) | this PRD produces; #5391 gates on it | dep edges wired at decompose (`add_dependency` 5391 → γ, λ, μ, α) |
| #5387 pub-enum crossing (narrow fix, filed) | overlaps | enum defs across the import boundary | **this PRD** (λ) owns the mechanism per #5387's own scope note; if #5387 lands first as a narrow merge extension, λ subsumes/replaces its mechanism and keeps its acceptance tests | reference, don't duplicate; resolve at decompose (dep or supersession per task-state at that time) |
| #5386 visibility unenforced + exit-0 (narrow fix, filed; dep #5403) | adjacent | visibility filter in `sub_component_validation`; exit-code invariant | #5386/#5403 own both | this PRD preserves current exit policies (D-11); no edge needed |
| stdlib-namespace / shadowing PRD (parallel 2026-07-24 session; **not yet committed**) | consumes | qualified-name lookup over DefEnv's `(module_key, name)` keying (I7) | namespace PRD owns qualified semantics; this PRD owns env structure | hook shipped in P1 (D-3); nothing here depends on them |
| `conditional-compilation.md` (v0_6, landed) | consumes | `import_cfg_satisfied` gating threaded through `compile_program` (I3) + `--cfg` on six more commands (D-4) | this PRD | extends, semantics unchanged |
| `module-and-visibility-hardening.md` (v0_6) | consumes | `attach_module_path_diag` applied to imported files (ξ) | this PRD (ξ) | extension of landed mechanism |

No new contested-ownership pair; none of the overlay's three known contested seams are
touched.

---

## §7 — Boundary-test sketch (H)

| # | Scenario | Preconditions | Postconditions | Phase |
|---|---|---|---|---|
| 1 | Import-free entry through `compile_program` | any existing single-file example | diagnostics + definitions identical to legacy path (I1) | P1 (β) |
| 2 | Same importing entry through check / eval / GUI load_file | probe-imports `main.ri`+`parts.ri` shape | identical diagnostic sets (text + severity), Pulley sub evaluates in all three | P1 (η) |
| 3 | Broken import (missing file / parse error inside import) | entry imports a broken sibling | Error diagnostic attributed to imported file; entry still compiles; per-command exit policy unchanged (I2) | P1 (η) |
| 4 | cfg-gated import unsatisfied | `#cfg(target=...)` non-host import | inert everywhere: DAG, prelude, merge/env, `imports`, `sources` (I3) | P1 (η) |
| 5 | `load_from_source` buffer containing an import | GUI text-only load | explicit Warning (D-10), no crash, single-file compile proceeds | P1 (ε) |
| 6 | GUI dirty-buffer edit on importing entry | multi-file project open | import graph retained (engine.rs:2250-2260 behavior preserved) | P1 (ε) |
| 7 | LSP diagnostics on importing entry | multi-file workspace | no false `unknown structure`; single-file diagnostics unchanged | P1 (ζ) |
| 8 | `pub import` chain A→B→C vs plain-import chain | facade fixtures | C's pub defs visible at A's importer through pub edges only; plain chain does not leak | P3 (μ) |
| 9 | Entity-import narrowing | `import parts.Pulley` | Pulley resolves + evaluates; sibling pub template from parts errors at check | P3 (ν) |
| 10 | Cross-file pub fn at eval | mathlib probe shape | `let y = double_it(21.0)` prints `42`, not `undef` | P2 (λ) |
| 11 | Cross-file pub enum | #5387 repro | variant construction, type position, match all resolve; non-pub enum invisible (D-9) | P2 (λ) |
| 12 | Cross-file pub unit / alias / trait bound | one fixture each | resolve at importer at check AND eval | P2 (λ) |
| 13 | Collision matrix | entry-vs-import, import-vs-import, import-vs-stdlib, per kind | first-wins per I6, Warning naming both origins; units flip pinned (D-6) | P2 (κ) |
| 14 | Templates purity | any importing entry | post-P2 `entry.templates` contains only entry-declared names; GUI tree / doc unchanged vs P1 for user-visible content | P2 (κ) |
| 15 | Silent-skip retirement | sub naming an unknown structure reaching graph build | structured diagnostic, not silent node omission | P2 (ι) |
| 16 | Imported-file module-header mismatch | import whose file has wrong/missing `module` decl | Warning naming the *imported* file path | P3 (ξ) |
| 17 | GUI files panel | multi-file project | imported files listed (from `sources`), debug-MCP observable | P3 (ο) |
| 18 | Stale-mirror removal | ports_breadth.ri stripped | eval output shows stdlib `thread_form`; examples_smoke green | P0 (α) |

---

## §8 — Decomposition plan

Labels are PRD-relative; real ids at decompose. **Leaf** = no in-batch dependent.
Every new gate-resident test carries its drift-guard registrations in the same diff
(run-all-classification manifest / wallclock-bounds / nextest partitions), per the
overlay rule; η and λ add test binaries and must also bump the THROUGHPUT-COUNTS
sentinel (`docs/notes/verify-scope-throughput.md`) in-diff if they add build_plan poles.

**P0 — independent cleanup**

- **α — Strip stale eval-time mirrors from stdlib examples** *(leaf)*.
  Modules: `examples/stdlib/` only (grep for the "eval-time availability" /
  "RE-DECLARATION" comment pattern; ports_breadth.ri is the known instance; check
  m8_tolerancing lineage). Remove mirrors + dead comment block, keep the imports.
  Signal (#18): `reify eval examples/stdlib/ports_breadth.ri` output contains
  `thread_form` (impossible while the drifted mirror shadows stdlib); examples_smoke
  stays green. Evidence: mirror-stripped copy evaluates clean *(probe)*. Deps: none.

**P1 — compile_program + consumers**

- **β — `compile_program` + `CompiledProgram` + DefEnv skeleton** *(intermediate →
  γ δ ε ζ ξ θ)*. Modules: reify-compiler (module_dag.rs, lib.rs), reify-eval (skeleton).
  Refactor `compile_entry_with_stdlib_cfg_checked` into `compile_program`; keep the old
  name as a thin deprecated wrapper for one phase. I1 equivalence pin + I2/I3/I4 tests
  (#1). Unlocks: every other P1 task.
- **γ — eval + build go multi-file, `--cfg`** *(leaf)*. Modules: reify-cli/main.rs.
  Signal: probe-imports `main.ri` — `reify eval` prints `Rig.p = Pulley {…}` members and
  `reify build` produces geometry, no "import not resolved" warning; `--cfg target=x`
  selects platform modules as in check. Deps: β. (#5391 gate 2.)
- **δ — test/report/explain/doc go multi-file, `--cfg`** *(leaf)*. Modules:
  reify-cli/main.rs. Signal: each command's output difference on an importing entry
  (test discovers a test on an imported structure; report/explain/doc render without
  the unknown-structure error). Deps: β.
- **ε — GUI: delete the twin, wire `compile_program`** *(leaf)*. Modules:
  gui/src-tauri/src/engine.rs. `compile_entry_with_imports` deleted;
  `load_from_source` D-10 diagnostic; dirty-buffer path preserved. Signals: #5, #6 +
  existing GUI suite (`scripts/gui-test.sh`) green + `git grep compile_entry_with_imports`
  empty. Deps: β.
- **ζ — LSP diagnostics multi-file** *(leaf)*. Modules: reify-lsp/src/diagnostics.rs.
  Signal (#7): diagnostics on an importing entry contain no `unknown structure` for an
  imported pub template; single-file snapshot tests unchanged. Deps: β.
- **η — P1 cross-surface parity harness** *(leaf; the P1 integration gate)*. Modules:
  new integration test (harness_ consolidation rules apply). Signals: #2, #3, #4 as a
  gate-resident suite; drift-guard registrations same-diff. Deps: γ, δ, ε, ζ.

**P2 — DefEnv authoritative** (chain: θ → ι → {κ, λ}; θ additionally gated on η so P2
runs against P1's landed net)

- **θ — DefEnv full resolution + `Engine::load(CompiledProgram)` + collision
  normalization** *(intermediate → ι)*. Modules: reify-eval (lib.rs, engine_admin),
  reify-compiler (prelude_context doc block). All kinds resolvable per I6/I7; D-6
  policy incl. units flip with pins inverted in-diff; D-9 enum fixup pass. Deps: β, η.
- **ι — Flat-slice call-site migration** *(intermediate → κ, λ)*. Modules: reify-eval
  (unfold, graph, structural_query, engine_build, engine_eval),
  reify-compiler/conformance (sub_component_validation mirrors the env lookup set).
  Lookup sites → `resolve_*`; iterate sites → `declared()`; graph.rs silent skip →
  diagnostic (#15). Deps: θ.
- **κ — Delete the interim: merge consumption + `find_template_with_prelude` +
  `Engine::prelude` statics; templates purity** *(leaf)*. Modules: reify-eval,
  reify-compiler, gui. Signals: #13, #14; `git grep find_template_with_prelude` empty;
  full multi-file + stdlib suites green. Deps: ι.
- **λ — Cross-kind end-to-end signals** *(leaf)*. Modules: reify-eval tests +
  reify-cli tests (fixtures). Signals: #10 (fn `42` not `undef`), #11 (#5387
  acceptance: construction/type/match, non-pub invisible), #12 (unit/alias/trait).
  Deps: ι. (#5391 gate 3, first half.)

**P3 — exposure policy**

- **μ — `pub import` re-export** *(leaf)*. Modules: reify-compiler
  (compile_program frontier). Signal (#8): facade fixture — entry imports lib,
  lib pub-imports parts; `reify check` + `reify eval` resolve Pulley; plain-import
  chain does not leak. Deps: κ. (#5391 gate 3, second half.)
- **ν — Entity-import narrowing** *(leaf)*. Modules: reify-compiler. Signal (#9).
  Deps: κ.
- **ξ — Imported-file module-header mismatch surfacing** *(leaf)*. Modules:
  reify-compiler (module_dag.rs:527-529 region). Signal (#16). Deps: β.
- **ο — GUI files panel from `sources`** *(leaf)*. Modules: gui (engine.rs source_map,
  frontend files panel). Signal (#17): debug-MCP store_state lists the imported file.
  Deps: ε.

**Companion corrections at decompose:** wire `add_dependency` 5391 → {γ, λ, μ, α};
resolve #5387 relationship per its live status (§6 row 2).

---

## §9 — Out of scope

- **Qualified references** (`pp.Pulley`) and namespace/shadowing semantics — the
  parallel 2026-07-24 namespace PRD owns them; this PRD ships only the I7 keying hook.
- **The error⇒nonzero-exit invariant** as a global policy (D-11; #5386/#5403 territory).
- **Module-level compile caching / incremental recompilation** for GUI keystroke
  performance — current per-edit recompile behavior is preserved; a cache keyed on
  content hash is future work if profiling demands it.
- **Multi-module `reify doc` output** (D-12, §10 Q2).
- **Catalog/table mechanism** for part families — #5391's own PRD.
- **LSP navigation/completion multi-file upgrades** beyond the diagnostics path (goto-def
  already has its own resolver wiring).

## §10 — Open questions (tactical)

1. **`compile_entry_with_stdlib_cfg_checked` wrapper retirement.** Keep the deprecated
   wrapper through P1 or delete immediately and fix all in-tree callers in β?
   **Suggested:** delete in β if callers are few (they are: CLI + tests); decide in β.
2. **`reify doc` on a multi-file project** — should it eventually emit per-module
   sections from `CompiledProgram.imports`? **Suggested:** follow-up task filed when a
   consumer asks; `declared()`-only for now (D-12). Decide post-P1.
3. **`CompiledImportUnit.origin` for embedded-stdlib fallback resolutions** — `None` vs
   a synthetic `embedded:` path for diagnostics. **Suggested:** `None` + diagnostic
   text says "embedded stdlib". Decide in β.
4. **DefEnv function overload precedence** — `resolve_function` must compose with the
   existing signature-based overload table (`merge_functions` shadowing rules). Exact
   merge of "first-wins across modules" with "signature match within module" decided
   in θ against the existing `eval_is_idempotent_for_prelude_functions` pins.
5. **α's sweep breadth** — whether any non-examples `.ri` (prj/, dogfood worktree)
   carries the mirror pattern worth cleaning in the same task. Decide in α via grep.
