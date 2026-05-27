# Conditional Compilation

> Spec §18 row 13 parks "conditional compilation — conditional imports, platform-specific
> module variants" as deferred. Reify already has a compile-time gating precedent: the
> `#no_prelude` pragma suppresses the implicit prelude import (§7.6), and the module-pragma
> pipeline (`#version`, `#precision`, `#solver`, `#kernel`) reads structured pragma args
> into typed module fields. This PRD builds conditional compilation on that **existing
> pragma substrate** — a `#cfg(key = value)` form that already parses — to gate imports and
> select platform-specific module variants at compile time.

---

## §0 — Purpose & cohesion

**Purpose.** Let an author gate an `import` (or a module's participation in the DAG) on a
compile-time configuration predicate, so a project can ship platform- or feature-specific
module variants and resolve a different import set under a different cfg.

**Cohesion call — why this is a SEPARATE PRD from `module-and-visibility-hardening.md`.**
The cluster brief asked whether path-enforcement + `priv` + conditional-compilation is one
PRD or two. **Two.** The decisive evidence is the G3 grammar gate (§3): path-enforcement
and `priv` **both require new tree-sitter grammar** (a `module` production; a `priv`
keyword), while conditional compilation **requires none** — `#cfg(target = "linux")` and
`#cfg(linux)` already parse on the existing pragma rule (verified 2026-05-27). Coupling a
grammar-free feature to grammar-gated tasks would chain this work behind the grammar
prerequisite for no reason. Conditional compilation also has a *different blast radius*: it
touches the module DAG resolution (`module_dag.rs`) and the prelude/pragma machinery
(`module_pragmas.rs`, `pre_pass.rs`, `annotations.rs::MODULE_ONLY_PRAGMAS`), not the
visibility / member-export path that `priv` touches. The two PRDs share only the high-level
§7 module-system framing, not a single implementation task.

**This PRD introduces NO tuples and no new grammar.** (Per project norms.)

---

## §1 — Spec grounding & current-state evidence

- **§18 row 13:** "Conditional compilation — conditional imports, platform-specific module
  variants. Deferred."
- **§7.6:** "Suppression: the pragma `#no_prelude` suppresses the implicit prelude import."
  — the existing compile-time gating precedent.
- **Current state (verified 2026-05-27):**
  - The pragma grammar (`tree-sitter-reify/grammar.js` `pragma` / `pragma_arg` /
    `_pragma_value`) accepts `#name`, `#name(bareident)`, `#name(key = value)`, with
    `_pragma_value` covering ident / number / string / bool / dimensioned quantity.
    Fixtures `#cfg(target = "linux")`, `#cfg(linux)`, `#when(target = "wasm")` all parse
    (`tree-sitter parse --quiet` exit 0).
  - `crates/reify-compiler/src/annotations.rs`:
    `MODULE_ONLY_PRAGMAS = &["no_prelude", "version"]`; `is_known_module_pragma` gates the
    "unknown pragma" warning — an unrecognized `#cfg` would warn today.
  - `crates/reify-compiler/src/module_pragmas.rs`: `apply_module_pragmas` dispatches to
    `apply_version_pragma` / `apply_precision_pragma` / `apply_solver_pragma` /
    `apply_kernel_pragma` — the exact slot a `#cfg` consumer joins.
  - `crates/reify-compiler/src/compile_builder/pre_pass.rs`: `effective_prelude` already
    suppresses the prelude when `#no_prelude` is present — a per-import gating pass is the
    direct analogue.
  - `crates/reify-compiler/src/module_dag.rs`: `compile_module` walks
    `parsed.declarations`, and for each `Declaration::Import(import)` recursively compiles
    `import.path`. This loop is where cfg-gating filters which imports are followed.
  - `ImportDecl` (`reify-ast/src/decl.rs`) carries `annotations: Vec<Annotation>` but the
    `@cfg(target = "linux")` **annotation** form does NOT parse (`target = "linux"` is not
    a valid expression) — confirming the pragma form, not an annotation form, is the
    correct surface.

---

## §2 — Sketch of approach

### The cfg model

A **cfg key/value set** is supplied to the compiler as compile-time configuration (the
"active cfg"). Default keys: `target` (a platform string, e.g. `"linux"`, `"wasm"`,
`"macos"`), plus arbitrary user feature flags (`#cfg(feature_x)` bare-ident = boolean
true). The active cfg is supplied by the compile driver (CLI flag / GUI / a project default
of the host platform) — see §4 D-2.

A `#cfg(...)` pragma **immediately preceding an `import`** gates that import: if the cfg
predicate is satisfied by the active cfg, the import is followed; otherwise it is skipped
(not resolved, not added to the DAG). The predicate forms (minimal v1, §4 D-1):
- `#cfg(key = "value")` — true iff active cfg has `key == "value"`.
- `#cfg(flag)` — true iff active cfg has boolean `flag` set.
- (negation / and / or are deferred — D-1.)

### Slice A — `#cfg` recognized as a known module pragma (no grammar)

Add `"cfg"` to the recognized-pragma set (`MODULE_ONLY_PRAGMAS` or a new sibling list) so
`#cfg(...)` no longer warns as "unknown pragma", and parse/validate its argument shape in
the `module_pragmas.rs` validation neighbourhood (emit `E_CFG_MALFORMED` for a `#cfg` with
no args or a non-`key=value`/non-bare-ident arg). Signal-bearing but standalone:
malformed `#cfg` now diagnoses; well-formed `#cfg` no longer warns.

### Slice B — cfg predicate evaluation against an active cfg

A pure function `cfg_satisfied(pragma: &Pragma, active: &CfgSet) -> bool` evaluating the
D-1 predicate forms against an active cfg set. `CfgSet` is a typed value
(`{ target: Option<String>, flags: BTreeSet<String>, kv: BTreeMap<String,String> }` or
similar) threaded from the compile driver. Intermediate task feeding C.

### Slice C — cfg-gated import filtering in the module DAG (depends A, B)

Associate each `#cfg(...)` pragma with the `import` declaration it immediately precedes
(positional attachment, mirroring how annotations attach to the following decl). In
`compile_module` / `compile_project_with_entry_source`, before recursing into an import,
evaluate its attached cfg (if any) against the active cfg; **skip the recursion** when the
predicate is unsatisfied. The skipped import contributes nothing to the DAG, the prelude, or
name resolution. A platform-specific variant is expressed as two cfg-gated imports of two
sibling modules.

### Slice D — active-cfg plumbing through the compile driver (depends C)

Thread an active `CfgSet` from `reify check` (a `--cfg key=value` / `--cfg flag` repeated
flag, defaulting `target` to the host platform) down through `compile_with_stdlib` /
`compile_project` to the DAG. GUI dirty-buffer path uses the host default. This is the
user-facing knob.

### Slice E — integration gate (leaf; depends D)

A CI `.ri` fixture set: a module with two cfg-gated imports (`#cfg(target = "linux")` →
`platform_linux`, `#cfg(target = "wasm")` → `platform_wasm`), where each sibling defines a
same-named entity differently. Run `reify check --cfg target=linux` and
`reify check --cfg target=wasm`; assert the resolved entity differs and the off-target
import's module is absent from diagnostics/resolution.

---

## §3 — Substrate (G3) — grammar gate results

Run 2026-05-27, `tree-sitter parse --quiet`, grammar at HEAD:

| Fragment | Fixture | Parses today? | Resolution |
|---|---|---|---|
| `#cfg(target = "linux")` then `import …` | `cc-1-pragma-cfg-bare.ri` | **YES** (exit 0) | use existing pragma grammar |
| `#cfg(linux)` (bare ident) | `cc-2-pragma-bare-ident.ri` | **YES** | use existing pragma grammar |
| `#when(target = "wasm")` (kv form) | `cc-3-pragma-kv.ri` | **YES** | use existing pragma grammar |
| `#no_prelude` precedent | `cc-4-no-prelude.ri` | **YES** | precedent for compile-time gating |
| `@cfg(target = "linux")` annotation form | `cc-5-annotation-import.ri` | **NO** (`(ERROR)`) | rejected — use pragma form, not annotation |

**G3 conclusion: no novel grammar.** `grammar_confirmed=true` for **every** task — the
`#cfg(...)` surface rides the existing pragma production. The only substrate work is
*semantic* (recognize the pragma, evaluate it, gate imports). This is the structural reason
conditional compilation is its own PRD.

---

## §4 — Resolved design decisions

- **D-1 — Minimal predicate grammar in v1: equality + bare-flag, AND-of-adjacent.** A
  single `#cfg(key = "v")` or `#cfg(flag)` per import. Multiple `#cfg` pragmas stacked on
  one import are ANDed. Negation (`not`), `or`, and nested predicates are **deferred** — the
  pragma-arg grammar can't express boolean operators without new grammar, and the 80% case
  (platform select + feature flag) is covered. A future PRD can add a richer predicate if
  demand appears. This keeps v1 grammar-free.
- **D-2 — Active cfg is supplied by the driver, defaulting `target` to the host platform.**
  `reify check` gets a repeatable `--cfg key=value` / `--cfg flag` flag. Absent any flag,
  `target` defaults to the compiling host's platform string; user flags are otherwise
  empty. Rationale: a model that imports `#cfg(target = "linux")` should "just work" when
  checked on Linux without ceremony, matching the platform-variant use case in §18 row 13.
- **D-3 — cfg gates IMPORTS (and thereby module participation), not arbitrary
  declarations.** v1 scope is `#cfg` immediately preceding an `import`. Gating an arbitrary
  structure/fn decl is **out of scope** — it raises hard questions (a gated-out `pub`
  structure referenced elsewhere = dangling reference) that the import-gating model sidesteps
  (a skipped import simply makes its names unavailable, which the existing
  name-resolution-failure diagnostics already handle cleanly). "Platform-specific module
  variants" (§18 row 13) is realized as *two cfg-gated imports of two modules*, not as
  in-module decl gating.
- **D-4 — A `#cfg` not attached to an import is a WARNING.** A `#cfg(...)` at module top
  with no following `import` (or before a non-import decl) has no effect; emit
  `W_CFG_NO_IMPORT` so the author isn't silently surprised. (Mirrors the existing
  block-level-pragma misplacement warnings in `module_pragmas.rs`.)
- **D-5 — cfg is attached positionally (pragma immediately precedes import), reusing the
  annotation-attachment idiom.** Annotations already attach to the following declaration via
  a pending-accumulator in lowering; cfg pragmas use the same "pending pragma → next import"
  attachment. No grammar change; a small lowering change to associate a leading `#cfg` with
  the `ImportDecl` that follows it (new optional `cfg: Vec<Pragma>` or `cfg_predicates` field
  on `ImportDecl`).
- **D-6 — Skipped imports are fully inert.** A cfg-gated-out import is not resolved (no file
  I/O), not added to the DAG, not part of the prelude, and contributes no names. This makes
  the off-platform branch genuinely absent rather than compiled-then-discarded — so a
  `platform_wasm` module that wouldn't even *parse* on a non-wasm toolchain is never touched.

---

## §5 — Out of scope

- Path enforcement + `priv` — `v0_6/module-and-visibility-hardening.md`.
- Boolean predicate operators (`not`/`or`/nesting) in `#cfg` — deferred (D-1).
- Conditional gating of arbitrary declarations (not imports) — deferred (D-3).
- A cfg *manifest* / project-file source for the active cfg — v1 uses CLI flags + host
  default; a project-config source is a separate concern.
- Cross-compilation toolchain selection — cfg is a compile-time *source-selection* feature,
  not a build-target/codegen feature.

---

## §6 — Boundary-test sketch (G5 = bare B)

Bare-B shaped: 2 crates (`reify-compiler` + `reify-cli`); ~6 mechanisms; not a load-bearing
seam per the overlay (the module DAG resolution is touched but the change is additive
filtering, not a contract rewrite). One end-to-end two-way scenario, named as Slice E's
signal:

| Scenario | Driver input | Resolution effect | Expected |
|---|---|---|---|
| linux variant selected | `reify check --cfg target=linux` on a model with both gated imports | `platform_linux` followed, `platform_wasm` skipped | the linux entity resolves; wasm module absent from DAG |
| wasm variant selected | `reify check --cfg target=wasm` | `platform_wasm` followed, `platform_linux` skipped | the wasm entity resolves; linux module absent |
| unsatisfied gate | `#cfg(feature_x)` import, no `--cfg feature_x` | import skipped | names from gated import unavailable; clean name-resolution error if referenced |
| malformed cfg | `#cfg()` | validation | `E_CFG_MALFORMED` |
| dangling cfg | `#cfg(linux)` with no following import | validation | `W_CFG_NO_IMPORT` |

---

## §7 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/module-and-visibility-hardening.md` | sibling | shares §7 module-system framing only; disjoint mechanisms (`#cfg` pragma vs `module`/`priv` grammar) | independent | no shared task |
| spec §7.6 `#no_prelude` / module-pragma pipeline | consumes | `MODULE_ONLY_PRAGMAS`, `apply_module_pragmas`, `effective_prelude` (precedent + extension point) | this-prd | wired Slices A/C |
| spec §7.3 import forms / `module_dag.rs` resolution | extends | the `compile_module` import-recursion loop (cfg filters which imports are followed) | this-prd | wired Slice C |

No contested-ownership pair from the overlay's catalogue is touched.

---

## §8 — Decomposition plan (per-task observable signal)

- **Task α — Recognize + validate `#cfg` pragma (no grammar).** Add `"cfg"` to the known
  module-pragma set; validate arg shape in `module_pragmas.rs`. Signal: `reify check` on a
  file with a well-formed `#cfg(target = "linux")` no longer emits "unknown pragma"; a
  malformed `#cfg()` emits `E_CFG_MALFORMED`. `grammar_confirmed=true`.
- **Task β — cfg predicate evaluator + `CfgSet` type.** `cfg_satisfied(&Pragma, &CfgSet)`
  for the D-1 forms. Signal: unit-level evaluator behaviour is exercised, but its
  user-observable proof is deferred to ε (β is an **intermediate** task unlocking γ/δ —
  named consumer: Task γ). `grammar_confirmed=true`.
- **Task γ — Attach `#cfg` to following import + DAG gating (depends α, β).** Positional
  attachment of a leading `#cfg` to the next `ImportDecl` (new field on `ImportDecl`); skip
  recursion in `compile_module` when the predicate is unsatisfied against a (test-supplied)
  active cfg. Signal: with an active cfg passed programmatically, `reify`'s compiled module
  set omits the gated-out import's module and includes the matching one (observable via
  `reify check` diagnostics referencing only the selected variant). `grammar_confirmed=true`.
- **Task δ — `--cfg` driver plumbing (depends γ).** Repeatable `--cfg key=value` / `--cfg
  flag` on `reify check`, defaulting `target` to host platform; thread `CfgSet` into the DAG.
  Signal: `reify check --cfg target=wasm <file>` and `reify check --cfg target=linux <file>`
  on the same file produce different resolution outcomes (one errors on a missing name the
  other resolves, or both succeed selecting different variants). `grammar_confirmed=true`.
- **Task ε — Integration gate (leaf; depends δ).** CI `.ri` fixture set with linux/wasm
  cfg-gated imports of sibling modules defining a same-named entity differently. Signal: the
  CI example, run under both `--cfg target=linux` and `--cfg target=wasm` through
  `reify check`, resolves the platform-correct entity and shows the off-target module absent
  — matching the §6 table. `grammar_confirmed=true`.

Intra-batch deps: γ→{α, β}, δ→γ, ε→δ.

---

## §9 — Open (tactical) questions

- Final `--cfg` flag spelling and whether `target` is special-cased or just another key.
  Tactical: D-2 fixes the semantics; the surface string is implementer's choice.
- Whether `CfgSet` lives in `reify-compiler` or a small shared types crate — tactical,
  follow the existing pragma-types placement.
- Whether the GUI exposes a cfg selector or always uses host default in v1 — default to host
  (D-2); a GUI selector is a separate UX task.
- Pragma name: `#cfg` vs `#when` vs `#config`. Default `#cfg` (Rust-familiar, terse). Pure
  naming; no design impact.
