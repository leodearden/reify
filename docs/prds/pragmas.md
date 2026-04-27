# PRD: Pragma implementations for v0.1

**Status:** v0.1 scope.
**Spec reference:** `docs/reify-language-spec.md` §12.2.
**Implementation reference:** `docs/reify-implementation-architecture.md` §10.3 (multi-kernel context for `#kernel`).
**Pre-existing infrastructure:** Task #278 (parser framework) is **done** — `Pragma`, `PragmaArg`, `PragmaValue` ASTs are populated by `crates/reify-syntax/src/ts_parser.rs`, attached to `ParsedModule.pragmas` and to each block-level scope. `crates/reify-compiler/src/annotations.rs` already classifies pragma names via `KNOWN_BLOCK_PRAGMAS` / `MODULE_ONLY_PRAGMAS` and emits unknown / misplaced warnings. `#no_prelude` already short-circuits the prelude (`compile_builder/pre_pass.rs`).

This PRD covers the five v0.1 pragma **implementations** that consume the parsed pragma data. `#no_prelude` is already wired (kept here only for completeness and for the bootstrap-criticality flag); the other four need real semantics, not just storage.

## Goals

A pragma always validates and either takes effect on the toolchain or emits an actionable diagnostic. Pragmas are **toolchain directives**: they never change program meaning, only implementation characteristics (§12.2 invariant). v0.1 must keep that invariant — a missing or invalid pragma must never silently change semantics.

## Non-goals (deferred to v0.2)

- The `#version` migration tool (rewrite-on-bump). v0.1 records the target version and warns if it disagrees with the compiler's supported version.
- Multi-kernel dispatch beyond OCCT. v0.1 only validates `#kernel(occt)`; other kernel names produce an explicit "deferred to v0.2" diagnostic, never a silent fallback.
- Per-block override of `#precision` and `#solver` (block-level pragmas already store, but v0.1 only honours module-level values; block-level emits an "ignored in v0.1" warning).

## Items

### 1. `#no_prelude` — bootstrap-critical

**Status:** wired in `compile_builder/pre_pass.rs::resolve_prelude_with_pragmas`. The prelude itself uses `#no_prelude` to bootstrap (otherwise it would import itself). Tests at `crates/reify-compiler/tests/pragma_compile_tests.rs::no_prelude_simple_structure_compiles_clean` and `constant_compile_tests.rs::pi_works_under_no_prelude` exist.

**Outstanding work in scope of v0.1:**
- Audit that the prelude `.ri` files in `crates/reify-compiler/stdlib/` all carry `#no_prelude` at the top (they currently rely on the pre-pass list-based bootstrap; making the source explicit makes the convention self-documenting).
- Add a regression test that compiling a non-prelude module **without** `#no_prelude` still gets the prelude (currently only the negative case is tested explicitly).

This is small enough to be a single task.

### 2. `#precision(0.001m)` — global default tolerance

**Spec:** §12.2 example uses `#precision(float64)` (a numeric format hint). The architectural decision for v0.1 narrows this to a **single global default geometric tolerance** for the module/file (architectural decision in this scope: v0.1 uses a single global tolerance per §10.4 simplifications).

**Argument grammar:**
- `#precision(<dim-literal>)` — a `Length` literal such as `0.001m`, `1mm`, `10um`. Stored as `f64` metres.
- `#precision(float64)` — accepted as a **legacy/numeric-format spelling** for compat with §12.2; v0.1 emits a `"#precision(float64) recognised but ignored — v0.1 always uses float64; use a Length literal to set the default tolerance"` info-level diagnostic.
- Anything else: warning + ignored.

**Where the value lives:**
- New field on `CompiledModule`: `pub default_tolerance: Option<DimensionedValue>` (or, more simply, `Option<f64>` in metres). Set by a new pass that reads `parsed.pragmas`. Default when absent is the existing hard-coded value (locate it; expected in `reify-geometry` or `reify-runtime`; tighten the location during architect phase).
- `reify-geometry::DispatchPlanner` and the OCCT kernel binding read `compiled_module.default_tolerance` (or the absence-default) when constructing kernel inputs.

**Validation:**
- Multiple `#precision` pragmas at module level: warning on subsequent ones, first wins.
- Block-level `#precision` is in `KNOWN_BLOCK_PRAGMAS` already; v0.1 emits an `"ignored in v0.1; per-block tolerance deferred to v0.2"` warning.
- Negative or non-finite values: error.

### 3. `#solver(libslvs)` — solver dispatch override

**Spec:** §12.2 example is `#solver(nlopt, algorithm = LD_SLSQP)`. v0.1 narrows to selecting between the available solver back-ends and falling back to the existing default if the named back-end is not registered.

**Argument grammar:**
- `#solver(<ident>)` where `<ident>` is the back-end name. v0.1 known names: `libslvs`, `argmin`. Anything else: warning + use default.
- `#solver(<ident>, key = value, …)` — the `key = value` pairs are stored verbatim on the compiled module as a `BTreeMap<String, PragmaValue>` and forwarded to the solver back-end for back-end-specific tuning. Unknown keys: solver-back-end-level warning, not a compile error (matches §12.2 "implementation characteristics only").

**Where the value lives:**
- `CompiledModule::solver_pragma: Option<SolverPragma>` (`name: String`, `options: BTreeMap<String, PragmaValue>`).
- `reify-eval` solver dispatch reads it before falling back to the domain default; back-end registration happens at runtime startup.

**Validation:**
- Multiple `#solver` pragmas at module level: warning, first wins.
- Block-level `#solver`: ignored in v0.1 with "deferred to v0.2" warning.
- Unknown back-end name: warning, fall through to default; **must not** error (matches §12.2 invariant — pragmas don't change meaning).

### 4. `#kernel(occt)` — kernel selection

**Spec:** §12.2; architectural reference §10.3.

**Argument grammar:**
- `#kernel(occt)` — the only valid value in v0.1; recorded for symmetry with future kernel selection.
- `#kernel(<other>)` where `<other>` is any other identifier: error-level (not warning) diagnostic with text **"kernel '<other>' is deferred to v0.2; v0.1 supports only #kernel(occt)"**. The error makes the v0.1 limitation discoverable rather than hidden.
- `#kernel()` (no arg): warning, ignored.

**Where the value lives:**
- `CompiledModule::kernel_pragma: Option<String>` for symmetry, but v0.1 dispatch always uses OCCT regardless of value; the field is for round-tripping and for the doc tool.
- Block-level `#kernel`: in `KNOWN_BLOCK_PRAGMAS` already; v0.1 emits "ignored in v0.1" warning.

### 5. `#version(0.1)` — language version declaration

**Spec:** §12.2 with cross-ref to §14.

**Argument grammar:**
- `#version(<number>)` where `<number>` is a `PragmaValue::Number` interpreted as a `MAJOR.MINOR` decimal (so `0.1`, not `0.1.0`).
- `#version("0.1")` (string form): also accepted; parsed as a `MAJOR.MINOR` semver-like literal.

**Where the value lives:**
- `CompiledModule::declared_version: Option<(u16, u16)>`.

**Validation:**
- v0.1 compiler's supported version is `0.1`. Declared version above the compiler's supported version: error ("module declares version X.Y; this compiler supports up to 0.1").
- Declared version below 0.1 (e.g. `0.0`): warning ("declared version 0.0 predates the first stable language; treating as 0.1"), no migration in v0.1.
- Multiple `#version` pragmas: error ("at most one #version declaration per module").
- Module-only — already in `MODULE_ONLY_PRAGMAS`; existing block-level warning suffices.

The auto-migration tool referenced in §14.2 is **explicitly v0.2 scope** — defer.

## Cross-cutting design

- All five values live as new `Option<...>` fields on `CompiledModule` (in `crates/reify-compiler/src/types.rs`). A single new pass `apply_module_pragmas(parsed, module, diagnostics)` consumes `parsed.pragmas`, validates, and populates the fields. Add a `module_pragmas.rs` file alongside `annotations.rs`.
- Validation diagnostics use the existing `Diagnostic` infrastructure with the pragma's `span` for the label. Severities follow this PRD's per-pragma rules; a misnamed pragma still goes through `validate_pragmas` (already in place) at warning level.
- The doc generator (separate PRD) reads these fields verbatim to render a "Module pragmas" section. **No new fields beyond the five above.**

## Acceptance

- `cargo test -p reify-compiler --test pragma_compile_tests` adds tests for each pragma covering: valid value stored on `CompiledModule`, every diagnostic path (multiple, malformed, unknown back-end, deferred-to-v0.2 kernel, etc.), and the `#kernel(brep_xyz)` error message text.
- `examples/integration_full_v01.ri` carries `#version(0.1)` and `#precision(0.001m)` at the top with no warnings.
- `compile_with_stdlib` smoke test asserts the prelude modules all compile under `#no_prelude` (regression guard for the bootstrap path).

## Task slicing

Five tasks plus an optional sixth:

1. **`#precision`** — extract Length literal, store on `CompiledModule`, plumb to OCCT kernel; warnings for legacy `float64` form and block-level use.
2. **`#solver`** — store back-end name + options map on `CompiledModule`, integrate with `reify-eval` solver dispatch, fall-through-to-default for unknown back-end names.
3. **`#kernel`** — record on `CompiledModule`, error for non-occt values with explicit v0.2 message, ignore block-level with warning.
4. **`#version`** — parse number-or-string literal, error on too-new declared version, warning on too-old, error on duplicates.
5. **`#no_prelude` polish** — ensure each prelude `.ri` carries the pragma explicitly; add the positive-prelude regression test described above. (Smaller than the other four — could be folded into #1 if scope is tight.)
6. **`module_pragmas.rs` consolidation** — collect the five validators behind `apply_module_pragmas`. May be done as part of task 1; otherwise a tiny clean-up task.

Tasks 1-4 each touch `crates/reify-compiler/src/types.rs` (new field), `crates/reify-compiler/src/module_pragmas.rs` (new), and a per-pragma test file. Tasks 2 and 3 additionally touch `reify-eval` and `reify-geometry` / `reify-kernel-occt` respectively.

Priority: **high** for all four (lang-spec features that downstream tools, including the doc generator, will reference). `#no_prelude` polish is high (bootstrap-critical even if minor).
