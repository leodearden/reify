# compute-fea-hardening: canonical compute registration, LSP FEA posture, panic-safe compute boundary, NaN-safe ordering

**Status:** active. Authored 2026-07-06 from the bug-hotspot survey (`docs/notes/bug-hotspot-survey-2026-07-05.md` §H7 + latent bugs 8/9) and Leo's ratified hotspot-program decisions. Establishes **INV-FEA-1/2/3** (`docs/invariants.md`).

## Goal

A user running `reify build`/`reify test`/`reify eval` or driving the GUI never silently loses FEA-result registration and never crashes the process on malformed FEA input; an FEA-bearing `.ri` file open in the editor tells the user when a constraint's truth is undetermined instead of staying silent; and no sort/comparison in the numeric solve path treats `NaN` as `Ordering::Equal` without an explicit, reasoned policy.

Four independently-landable hardening slices, all long-standing repeated-omission classes surfaced by the survey:

1. **INV-FEA-1 (registration).** One canonical bundling point for compute-trampoline registration, replacing three independently hand-assembled, already-drifted bundlers (CLI, GUI, test_runner).
2. **INV-FEA-1 (LSP posture).** The LSP's trampoline-free posture becomes documented + locked (mirroring `cmd_check`'s existing posture) instead of silently dropping FEA-constraint truth; a hint diagnostic tells the user why.
3. **INV-FEA-2 (panic boundary).** The compute-dispatch boundary converts a trampoline panic into a structured `ComputeOutcome::Failed` instead of an uncaught process crash (CLI has **zero** `catch_unwind` today), then works down the ~30 panic-based Value-shape asserts that make the panic reachable in the first place.
4. **INV-FEA-3 (NaN ordering).** Every `partial_cmp(...).unwrap_or(Ordering::Equal)` site in the numeric crates gets an explicit, reasoned NaN policy (fail-closed guard or `total_cmp`), plus a grep gate so a new unguarded site can't reappear silently.

## Background

- `docs/notes/bug-hotspot-survey-2026-07-05.md` §H7 (diagnosis + proposals) and latent bugs #8 (LSP drops FEA violations) / #9 (CLI panic-crash).
- `docs/invariants.md` INV-FEA-1/2/3 (this PRD is their named owner).
- Three confirmed repeated-omission incidents: esc-2962-66 (GUI), task 4458 (`cmd_build`), task 4468 (`run_tests`) — each fixed by an independent hand-rolled bundler; the GUI bundler has **already drifted again** relative to CLI (missing morph-producer registration), which is itself the strongest evidence that "fix the instance" without "fix the class" doesn't hold.
- Async-recalc Phase A (task **#5023**, deferred bookmark) is the long-term consumer of both the panic boundary (a background compute panic must resolve to `Failed` freshness, never an eternal spinner) and the LSP FEA answer (per-constraint computing/not-evaluated states supersede the hint diagnostic this PRD adds). This PRD wires a real dependency: **#5023 depends on** the panic-boundary task (D1) below.

**Anchors re-verified against `main` @ 2026-07-06** (survey anchors were dated 2026-07-05; `crates/reify-eval/src/compute_targets/elastic_static.rs` had 9 intervening commits — task #4870/#5008 body-arg work — that shifted line numbers materially; corrected anchors are used throughout this PRD, not the survey's).

## Sketch of approach

### 1. Canonical constructor (INV-FEA-1)

**Dependency-direction constraint (why this isn't a bare `Engine::new_production() -> Self`).** `reify-mesh-morph` normal-deps `reify-eval` (the `MorphProducer` seam lives in `reify-eval`); `reify-eval` can only take `reify-mesh-morph` back as a **dev-dependency** (`crates/reify-eval/Cargo.toml:166-175`, comment explicit about the cycle) — used today only by `test_runner.rs`'s own `#[cfg(test)]`-reachable code, but `build_test_engine` is *not* test-gated (it's called by production `run_tests`), so it **cannot** reference `reify_mesh_morph::register_morph_producer` at all without breaking any binary that links `reify-eval` as a library outside a `cargo test` build. CLI and GUI, by contrast, both normal/optional-dep `reify-mesh-morph` directly and already call `register_morph_producer` themselves. So the canonical bundler must live in `reify-eval`, own the two registrations `reify-eval` *can* make (`compute_targets::register_compute_fns`, `register_shell_extract_compute_fns`), and take the morph registration as an **opaque, caller-supplied fn pointer** — never a direct `reify_mesh_morph` reference — resolved into a **required enum arg** (see Contract §C1) so a caller cannot silently omit it the way GUI's bundler did.

Because GUI and CLI each build the underlying `Engine` differently (`Engine::new` vs `Engine::with_registered_kernel`) *before* wiring compute fns, the canonical mechanism is a **method on an already-constructed `&mut Engine`**, not a fresh top-level constructor — despite the survey's `Engine::new_production(...)` phrasing. See **Resolved design decisions** for the exact name.

Migrate the three hand-rolled bundlers (CLI `register_compute_trampolines`, GUI `EngineSession::from_engine`, test_runner `build_test_engine`) to delegate to it; add a grep-based architecture test that all three (and no fourth undelegated site) call it. GUI's migration fixes its missing morph-producer registration **as a side effect** — the actual esc-2962-66-class bug this PRD closes.

`cmd_check`'s deliberate trampoline-free opt-out (`crates/reify-cli/src/main.rs:450-471`, locked by `check_fea_violated_constraint_is_not_gated`, `crates/reify-cli/tests/cli_build_fea.rs:147`) needs **no change** — it is already documented and already locked. It is the pattern the LSP posture work (§2) copies.

### 2. LSP posture (INV-FEA-1)

The LSP constructs 11 bare `reify_eval::Engine::new(...)` engines (`crates/reify-lsp/src/analysis.rs:99`, `diagnostics.rs:26/150/367/611/1154/1211/1229/1337/1443/1557/1631`), none registering compute fns — today **undocumented and untested**. `Satisfaction::Indeterminate` constraints are filtered out of the diagnostics list at exactly two sites (`crates/reify-lsp/src/diagnostics.rs:190,236`, both gated on `entry.satisfaction == Satisfaction::Violated`), so an FEA-dependent constraint produces **no diagnostic at all** — indistinguishable from "no constraint exists."

Two-part fix, Leo-ratified middle path (no keystroke-time solves under any guard scheme — rejected outright):
- **(a)** Document the posture (mirror `cmd_check`'s comment) + a locking test on a newly-authored FEA-bearing fixture (LSP tests use inline `&str` sources, not `.ri` files on disk — no existing FEA fixture in the LSP suite today).
- **(b)** Add a **hint diagnostic**: an FEA-dependent constraint that is `Indeterminate` under the trampoline-free posture gets a distinct, non-noisy `Severity::Info` diagnostic — "FEA constraint not evaluated in editor — run `reify test`" — once per document, per constraint. This is a **superseded-in-place** stopgap: async-recalc Phase A (#5023) is the long-term resolution (per-constraint computing/not-evaluated states); cited here as the consumer that will replace this hint.

### 3. Panic boundary (INV-FEA-2)

`invoke_compute_trampoline` (`crates/reify-eval/src/engine_compute.rs:102-119`) is the single generic dispatch point for **every** registered `ComputeFn` (elastic_static, buckling, modal, shell-extract — looked up by string `target` from a shared registry), so wrapping it here protects all of them, not just FEA. Today a panic inside any trampoline unwinds uncaught: GUI is saved incidentally by `engine_lock.rs`'s poison-safe wrapper; **CLI has none** — a reachable process crash on malformed/`Undef` FEA input.

Immediate fix (mandatory, ~10 lines): `catch_unwind` at `invoke_compute_trampoline`, converting a caught panic into `ComputeOutcome::Failed` + a regression test feeding `Value::Undef` at each positional arg of `solve_elastic_static_trampoline` in turn, asserting `Failed`, not a panic. Verified premise: today, `Value::Undef` at any of `material`/`length`/`width`/`height`/`loads` panics via the `other => panic!(...)` arms in `classify_material`, `extract_scalar_si`, and `extract_loads` respectively (the existing body-path and `AsPrintedZones`-Undef guards only intercept *those specific* shapes, not a bare `Undef` in the ordinary box-args path) — the RED test is real, not vacuous.

Per the placeholder-followups norm, the wrapper task is not "done" until its follow-ups are filed naming the sites — this PRD files them in the same batch (D2–D9 below): a **Result-based refactor** of the ~30 panic-based Value-shape asserts across `classify_material`, `classify_material_as_printed_zones`, `extract_material`, `extract_scalar_si`, `extract_loads`, `anisotropic_material_from_value`, `extract_point3_si`, `extract_vec3_si`, `extract_zone_process_params`, `scalar_si_field`, `real_field` (all in `crates/reify-eval/src/compute_targets/elastic_static.rs`, corrected span **3365–3873** — see Background), landing as one **early validate-all-inputs gate** at the top of `solve_elastic_static_trampoline`, generalizing the existing `AsPrintedZones`-Undef guard (corrected anchor **elastic_static.rs:457–470**, task #3787 precedent) so a malformed/`Undef` arg produces a diagnostic naming the offending arg instead of reaching the panic-based helpers at all.

### 4. NaN ordering (INV-FEA-3)

Fix the three sites the survey named plus two more the PRD's own repo-wide grep turned up in the survey's explicitly-named module list (`modal_ops.rs`, listed alongside solver-elastic/kernel-gmsh/fdm/shell-extract/mesh-morph/compute_targets):

| Site | File | What it orders | Policy chosen |
|---|---|---|---|
| Named (survey) | `crates/reify-kernel-gmsh/src/mesh_boundary.rs:519-539` (`find_closest_anchor`), unguarded `partial_cmp` at :537 | Anchor-matching by nearest distance | **Fail-closed** (finite-guard + `tracing::warn` + exclude non-finite candidates) |
| Named (survey) | `crates/reify-solver-elastic/src/adaptive.rs:266-292` (`mark_dorfler`), unguarded `partial_cmp` at :273-278 | Dörfler error-indicator marking | **Fail-closed** (finite-guard + `tracing::warn` once + treat non-finite indicators as zero-contribution/unmarked) |
| Named (survey) | `crates/reify-solver-elastic/src/interpolation.rs:337-...` (`TetSpatialIndex::build_recursive`), unguarded `partial_cmp` at :387-391 | kd-tree median split (BVH build) | **`f64::total_cmp`** |
| Found (grep sweep) | `crates/reify-eval/src/modal_ops.rs:441-446` | Ascending-frequency mode-order enforcement | **Fail-closed** (finite-guard + `tracing::warn` + skip the corrective resort, keeping the eigensolver's own ascending-\|λ\| order) |
| Found (grep sweep) | `crates/reify-eval/src/modal_ops.rs:2784-2801` (`nearest_node`) | Simply-supported-beam BC anchor placement | **Fail-closed w/ `total_cmp` fallback** (finite-guard + `tracing::warn`; function returns a bare `usize` so it cannot gracefully abstain — falls back to `total_cmp` for a deterministic pick rather than a hard panic) |

Per-site rationale is in **Resolved design decisions**. Repo-wide grep across the named crates found no further sites (checked `reify-fdm`, `reify-shell-extract`, `reify-mesh-morph` — zero matches; `reify-eval/{persistent_cache.rs, engine_build.rs, warm_pool.rs}` also have sites but are **out of the survey's named-crate list** — see Out of scope).

A grep-gate script (mirroring `scripts/check_event_inventory.sh`'s established pattern) wired into `scripts/verify.sh`'s lint step asserts no new unguarded `partial_cmp(...).unwrap_or(...Ordering::Equal...)` appears in the five named crates/modules going forward.

## Resolved design decisions

1. **Canonical bundler is a method, not a constructor.** `Engine::register_production_compute_fns(&mut self, morph: MorphRegistration)` in `crates/reify-eval/src/compute_targets/mod.rs` (beside `register_compute_fns`, line ~248) — **not** `Engine::new_production(...) -> Self`, because CLI/GUI build the underlying `Engine` via different constructors (`new` vs `with_registered_kernel`) before wiring compute fns; a fresh top-level constructor can't express that variance without duplicating each kernel-construction path. Named to read naturally at all three call sites: `engine.register_production_compute_fns(MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer))`.

2. **`MorphRegistration` is a required enum arg, not an optional flag** (the omission-preventing mechanism):
   ```rust
   pub enum MorphRegistration {
       /// Caller normal/optional-deps reify-mesh-morph (CLI, GUI): supply the
       /// concrete fn pointer directly — reify-eval never names the type.
       Enabled(fn(&mut Engine)),
       /// Caller structurally cannot link reify-mesh-morph in production
       /// builds (reify-eval's own test_runner — dev-dep-only, see Sketch §1).
       /// `reason` is asserted non-empty by a unit test so this can't silently
       /// decay into an unexplained no-op.
       Unavailable { reason: &'static str },
   }
   ```
   No `Default`, no `Option<fn(&mut Engine)>` — a caller must name one variant explicitly. This is the direct fix for the GUI-drift class: an `Option`-shaped API lets a forgetful caller pass `None` and compile silently; the enum has no "forgot" state.

3. **Scoped down: no `#[must_use]`-wrapper / rename of `Engine::new`/`with_registered_kernel`.** The survey/brief floated renaming or `#[must_use]`-annotating the raw constructors so bypassing registration is visually distinct. Rejected as written: `Engine::new`/`with_registered_kernel` are long-standing public APIs with hundreds of call sites across the workspace's own tests; wrapping them in a must-use marker type is a blast-radius-disproportionate refactor for what the brief's own G2(c) signal already asks for structurally (**"the architecture test fails if a fourth engine-construction site bypasses [the bundler]"** — a grep-based test, not a type-level guarantee). Resolution: a prominent rustdoc comment on both raw constructors ("NO compute trampolines registered — call `register_production_compute_fns` unless deliberately building a trampoline-free engine; see `cmd_check`'s posture doc") **plus** the grep architecture test as the real enforcement mechanism (task A5). Recorded here as a conservative in-session call, not re-litigated per-task.

4. **NaN-ordering fail-closed vs `total_cmp` policy** (the brief explicitly left this open, "choose per site"): fail-closed (finite-guard + `tracing::warn` + graceful degrade, mirroring the `through_thickness.rs:170-200` #3178 precedent) where the ordered quantity feeds a **correctness-relevant decision** the survey specifically flagged (anchor-matching mis-attribution; Dörfler-marking wrong elements; fundamental-mode identification — the frequency sort's own doc comment states its entire purpose is enforcing this contract). `total_cmp` where the sole purpose is a **deterministic tie-break for a spatial partition** whose result would already be geometrically meaningless on non-finite input for reasons upstream of the sort itself (the kd-tree median split — a NaN centroid already indicates upstream mesh corruption that manifests elsewhere in the solve; `total_cmp` keeps the recursive BVH build panic-free and deterministic without adding branch/log overhead to a hot recursive path). `nearest_node` is the one signature-constrained exception: it returns a bare `usize` and cannot abstain, so it gets both — a warn telemetry guard **and** a `total_cmp` fallback (never a hard panic; see next).

5. **No new dependency from `E5` (`nearest_node`) on `D1` (the panic boundary).** `nearest_node` is reached through the same generic `invoke_compute_trampoline` dispatch D1 wraps (modal analysis is a registered `ComputeFn` target too), so once D1 lands, a hard panic there would already resolve to `Failed` rather than a crash. But E5 and D1 are filed in the same batch with no ordering guarantee between them; rather than take a dependency edge purely to make "panic loudly" safe, E5 uses the graceful `total_cmp`-with-telemetry pattern unconditionally, so it's correct regardless of landing order.

6. **Grep-gates are standalone scripts, not new `reify-audit` `Pattern` variants.** Both A5 (compute-trampoline-registration drift) and F1 (NaN-ordering drift) are implemented as `scripts/check-*.sh` wired into `scripts/verify.sh`'s lint step + a `tests/infra/test_*_wired.sh` wiring test — mirroring the established `check_event_inventory.sh` precedent — rather than new `reify-audit` `Pattern` enum variants. `reify-audit`'s Pattern framework (CLI wiring, fixture corpus, `--pattern` flag plumbing) is heavier machinery than either check needs; `INV-FEA-3`'s registry entry itself says "lint+test," not "audit pattern." Recorded as a conservative choice — if a third such drift-guard is ever needed, revisit consolidating into `reify-audit` then.

7. **Result-refactor task granularity follows the real call graph, not one-task-per-function.** Grep-verified call graph (single call site per helper, confirmed against current `main`): `extract_scalar_si`/`scalar_si_field`/`real_field`/`extract_point3_si`/`extract_vec3_si` are call-graph leaves (no calls to sibling helpers); `extract_material`, `extract_zone_process_params`, `extract_loads` are also leaves; `anisotropic_material_from_value` calls `scalar_si_field`/`real_field`; `classify_material_as_printed_zones` calls `extract_point3_si`/`extract_zone_process_params`/`anisotropic_material_from_value`; `classify_material` calls `scalar_si_field`/`real_field`/`extract_material`/`classify_material_as_printed_zones`. Because Rust requires a caller to handle a newly-`Result`-typed callee in the **same commit** it changes (no partial-conversion compiles), and every one of these functions lives in the **same single file** (so the orchestrator's narrow-file-lock serializes them regardless of task count — splitting here buys review-size and rollback granularity, not parallelism), tasks are split by **call-graph layer** (8 tasks: D2–D9) rather than by individual function — grouping the five true leaf-primitives with near-identical single-match-arm bodies into one task (D2) to avoid five near-duplicate one-line-body tasks, while still cutting the ~500-line monolithic refactor the brief describes into 8 independently-reviewable, dependency-ordered pieces (down from a single task, up from the naive "5 tiny + 3 medium = 8" count only by the D2 grouping). This is the conservative reading of "split as much as reasonably possible": maximal splitting bounded by "each piece still has a coherent, reviewable, non-duplicate diff."

8. **D9 (the validate-all-inputs gate) slots in after the existing early guards, not before.** `solve_elastic_static_trampoline` already has a body-path pre-hydration guard (~429-455) and the `AsPrintedZones`-Undef guard (457-470) ahead of the `classify_material` call at line 478. D9 inserts its new gate **between line 470 and line 472** (immediately before "`// (1) Classify material`"), operating on the (possibly body-path-normalized) `value_inputs` — it must not disturb the two existing guards' behavior or ordering.

## Pre-conditions for activating

None — no upstream PRD/task gates this work. All referenced substrate (`ComputeOutcome::Failed`, `Severity::Info`, `DiagnosticCode`, `std::panic::catch_unwind`, `f64::total_cmp`) already exists and is wired on `main` today (verified: `ComputeOutcome::Failed` is already constructed at `elastic_static.rs:437,469`; `Severity::Info` already maps to LSP `DiagnosticSeverity::INFORMATION` at `crates/reify-lsp/src/convert.rs:148`). G3 (assumed-substrate) is **N/A beyond this** — no novel `.ri` grammar or new-endpoint assumptions in any of the four scopes.

## Cross-PRD relationship

| Other PRD/task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| Task **#5023** (async-recalc Phase A, deferred bookmark) | consumes | `ComputeOutcome::Failed` from the panic boundary (D1) — "a background panic must become Failed freshness, never an eternal spinner" (5023's own metadata) | 5023 (downstream) | **wired** — real `add_dependency(5023, depends_on=D1)` added by this PRD's decompose |
| Task **#5023** | supersedes | The LSP hint diagnostic (C2) — 5023's per-constraint computing/not-evaluated state design subsumes the hint; cited as C2's superseding consumer, no dependency edge (5023 doesn't depend on C2 to be buildable) | 5023 (downstream) | reference-only |
| `docs/prds/reify-eval-fea-decomposition.md` (deferred, measure-gated) | future-touches | If/when activated, its Design-X crate split re-homes `compute_targets/elastic_static.rs` into a new `reify-eval-fea` crate — every anchor in this PRD (elastic_static.rs, compute_targets/mod.rs) would need re-verification against the new location | reify-eval-fea-decomposition (if/when activated) | not active — no conflict today |
| Task 4876 (gmsh segfault) / kernel-seam-contracts PRD (concurrent session) | reference only | Explicitly out of scope here (see below); no seam owned by this PRD | kernel-seam-contracts | n/a |

No contested-ownership pair from `docs/architecture-audit/phase-3-breadcrumb-map.md` §3 is touched.

## Contract (B+H — blast radius ≥3 crates: reify-eval, reify-cli, gui/src-tauri, reify-lsp, reify-solver-elastic, reify-kernel-gmsh, scripts/verify infra; mechanism count well over 8; FEA is a named load-bearing seam)

### C1 — `MorphRegistration` + `register_production_compute_fns`

```rust
// crates/reify-eval/src/compute_targets/mod.rs (beside register_compute_fns)
pub enum MorphRegistration {
    Enabled(fn(&mut crate::Engine)),
    Unavailable { reason: &'static str },
}

impl crate::Engine {
    pub fn register_production_compute_fns(&mut self, morph: MorphRegistration) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
        match morph {
            MorphRegistration::Enabled(f) => f(self),
            MorphRegistration::Unavailable { reason } => {
                debug_assert!(!reason.is_empty(), "Unavailable reason must be non-empty");
                tracing::debug!(reason, "mesh-morph producer not registered on this Engine");
            }
        }
    }
}
```
Invariant: calling this twice on the same `Engine` panics (inherited from `register_compute_fns`'s existing duplicate-registration guard, `compute_targets/mod.rs:89`) — callers must call it exactly once, immediately after construction, exactly as today's three bundlers do.

### C2 — panic-boundary conversion

```rust
// crates/reify-eval/src/engine_compute.rs, invoke_compute_trampoline body
let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    f(value_inputs, realization_inputs, options, prior_warm_state, cancellation)
}));
match result {
    Ok(outcome) => outcome,
    Err(payload) => ComputeOutcome::Failed {
        diagnostics: vec![Diagnostic::error(format!(
            "compute trampoline '{target}' panicked: {}",
            panic_payload_message(&payload)
        ))],
        structured_detail: vec![],
    },
}
```
`AssertUnwindSafe` is sound here: every argument is a shared reference (`&[Value]`, `&Value`, `Option<&OpaqueState>`, `&CancellationHandle`); none are mutated across the unwind boundary. The default Rust panic hook still prints to stderr (not suppressed) — the behavioral floor is "process survives with a diagnostic," not "silent"; suppressing the hook would also hide genuine-bug backtraces needed for debugging (resolved decision, not reopened per-task).

### C3 — Result-refactor error type

```rust
// crates/reify-eval/src/compute_targets/elastic_static.rs
enum FeaValueShapeError {
    ExpectedStructureInstance { context: &'static str, got: String },
    ExpectedScalar { context: &'static str, got: String },
    ExpectedReal { context: &'static str, got: String },
    ExpectedList { context: &'static str, got: String },
    MissingField { context: &'static str, field: &'static str },
}
```
Every extract/classify helper returns `Result<T, FeaValueShapeError>`; the top-level gate (D9) collects the first error and renders it into the diagnostic naming the offending arg (G2(a)'s signal).

### C4 — NaN-ordering policy (tabulated in Sketch §4) is itself part of the contract — a future 6th/7th site in one of the five named crates/modules must pick one of {fail-closed guard, `total_cmp`} and state which, not add a third silent-`Ordering::Equal` pattern; the grep gate (F1) enforces the *absence* of the old pattern, not the *presence* of a specific new one.

## Boundary-test sketch

| Scenario | Preconditions | Postconditions (producer side) | Postconditions (consumer side) |
|---|---|---|---|
| CLI `reify build` on an FEA fixture | fixture has `solver::elastic_static` target | `register_production_compute_fns` called once at engine construction (A2) | morph producer registered too (drift-guard A5 green) |
| GUI loads an FEA fixture | GUI engine constructed via `from_engine` | same bundler call (A3) | morph-producer registration present — the esc-2962-66-class gap closed |
| `reify test` on a `.ri` module with `@test` FEA assertions | test engine via `build_test_engine` | bundler called with `MorphRegistration::Unavailable{reason}` (A4) | FEA assertions evaluate against real solve output, not Undef fallback (pre-existing behavior preserved, not regressed) |
| LSP opens an FEA-bearing `.ri` file | new FEA fixture (C1) | trampoline-free `Engine::new`, undocumented today → documented + locked | hint diagnostic (C2) appears once per FEA constraint, `Severity::Info`, no keystroke-time solve |
| `reify build` fed `Value::Undef` for `solve_elastic_static`'s `material` arg | no trampoline registered issue — registration present, input malformed | `invoke_compute_trampoline` catches the panic (D1) | exits 1 with a `Failed`-shaped diagnostic, no backtrace/crash |
| Same, post D9 | D2-D9 landed | `classify_material` returns `Err`, gate returns `Failed` before reaching the panic-based helper at all | diagnostic **names** `material` specifically (upgrade over D1's generic message) |
| Corrupt VolumeMesh feeds `find_closest_anchor` a NaN candidate distance | mesh has a NaN vertex (upstream pathology) | E1's finite-guard excludes the candidate + warns | no silent mis-attribution; `RUST_LOG` shows the warn |
| A future PR adds a new unguarded `partial_cmp(...).unwrap_or(Ordering::Equal)` in `reify-solver-elastic` | F1 landed | grep gate fails the lint step | PR author sees the failure before merge, not a NaN bug months later |

The integration-gate leaf for scope 3 is **D9** (names this boundary-test sketch's 5th/6th rows as its signal); for scope 1, **A5**; for scope 4, **F1**.

## Decomposition plan

Labels are intra-batch (Greek/alphanumeric); real task IDs assigned at decompose time. `files` are the exact narrow anchors verified against `main` @ 2026-07-06.

### Scope 1 — canonical constructor (INV-FEA-1)

- **A1** — Define `MorphRegistration` + `Engine::register_production_compute_fns` in `crates/reify-eval/src/compute_targets/mod.rs`; add the "NO compute trampolines registered" rustdoc to `Engine::new`/`Engine::with_registered_kernel`. *Intermediate* (unlocks A2/A3/A4). Files: `crates/reify-eval/src/compute_targets/mod.rs`, `crates/reify-eval/src/lib.rs`.
- **A2** — Migrate `reify-cli`'s `register_compute_trampolines` (`crates/reify-cli/src/main.rs:1229-1241`) to delegate to A1, passing `MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer)`. Depends: A1. *Intermediate* (unlocks A5). Files: `crates/reify-cli/src/main.rs`.
- **A3** — Migrate GUI's `EngineSession::from_engine` (`gui/src-tauri/src/engine.rs:1031-1042`) to delegate to A1, same `Enabled(...)` variant — **this is the actual esc-2962-66-class fix** (closes the missing morph-producer gap). Depends: A1. *Intermediate* (unlocks A5). Files: `gui/src-tauri/src/engine.rs`.
- **A4** — Migrate `reify-eval`'s `test_runner::build_test_engine` (`crates/reify-eval/src/test_runner.rs:109-114`) to delegate to A1, passing `MorphRegistration::Unavailable{reason: "reify-mesh-morph is a dev-only dep of reify-eval (task 4744) — a normal dep back would cycle"}`. Depends: A1. *Intermediate* (unlocks A5). Files: `crates/reify-eval/src/test_runner.rs`.
- **A5** — Grep-based architecture test + `scripts/check-compute-trampoline-registration.sh` wired into `scripts/verify.sh`'s lint step (mirrors `check_event_inventory.sh`), asserting all three known call sites delegate to `register_production_compute_fns` and no fourth undelegated bare-`Engine::new`-plus-manual-registration site exists. Depends: A2, A3, A4. **Leaf.** User-observable signal: **G2(c)** — "the architecture test fails if a fourth engine-construction site bypasses the bundler" (verified by temporarily reverting one migration in CI-local testing during task execution). Files: `scripts/check-compute-trampoline-registration.sh` (NEW), `tests/infra/test_compute_trampoline_registration_wired.sh` (NEW), `scripts/verify.sh`.

### Scope 2 — LSP posture (INV-FEA-1)

- **C1** — Document the LSP's trampoline-free posture (mirror `cmd_check`'s comment at `crates/reify-cli/src/main.rs:450-471`) in `crates/reify-lsp/src/diagnostics.rs`; author an inline FEA-bearing `.ri` source fixture (LSP tests use inline `&str`, not files — no shared helper needed unless C2 wants one); add a locking test mirroring `check_fea_violated_constraint_is_not_gated` (`reify-cli/tests/cli_build_fea.rs:147`) asserting the FEA constraint surfaces as `Indeterminate`/no-violation under the LSP's engine. **Leaf.** User-observable signal: a committed test pins that an FEA-bearing `.ri` file produces no false violation/false pass under `compute_diagnostics` — the documented-posture contract itself, locked. Files: `crates/reify-lsp/src/diagnostics.rs`.
- **C2** — Add the hint diagnostic: an FEA-dependent constraint that is `Indeterminate` gets a `Severity::Info` diagnostic ("FEA constraint not evaluated in editor — run `reify test`"), once per document per constraint (dedup by constraint id). Cite task **#5023** as the superseding long-term consumer in the task text. Depends: C1 (reuses its fixture). **Leaf.** User-observable signal: **G2(b)** — "an FEA-bearing `.ri` open in the LSP shows the hint diagnostic on its FEA constraint (was: nothing)." Files: `crates/reify-lsp/src/diagnostics.rs`.

### Scope 3 — panic boundary (INV-FEA-2)

- **D1** — `catch_unwind` wrapper at `invoke_compute_trampoline` (`crates/reify-eval/src/engine_compute.rs:102-119`) per Contract §C2 + regression tests feeding `Value::Undef` at each positional arg of `solve_elastic_static_trampoline` in turn, asserting `ComputeOutcome::Failed`, never a panic. **Leaf** (independent of D2-D9). User-observable signal: `reify build`/`reify eval` on a fixture that trips a compute-trampoline panic exits 1 with a `Failed`/error diagnostic instead of a Rust panic backtrace + process abort. Files: `crates/reify-eval/src/engine_compute.rs`.
- **D2** — Result-ify the primitive SI/Real leaf extractors: `extract_scalar_si`, `scalar_si_field`, `real_field`, `extract_point3_si`, `extract_vec3_si` (all in `elastic_static.rs`, corrected anchors: 3496-3515, 3520-3539, 3684-3695ish, 3696-3708ish, 3741-3749) → `Result<_, FeaValueShapeError>`; shim call sites with `.unwrap_or_else(|e| panic!("{e}"))` to preserve today's exact panic behavior until D9 removes the shims. *Intermediate* (unlocks D5, D6, D7, D9). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D3** — Result-ify `extract_material` (3709-3738) → `Result<IsotropicElastic, FeaValueShapeError>`; shim its one call site (in `classify_material`). *Intermediate* (unlocks D7). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D4** — Result-ify `extract_zone_process_params` (3546-3577) → `Result<ZoneProcessParams, FeaValueShapeError>`; shim its one call site (in `classify_material_as_printed_zones`). *Intermediate* (unlocks D6). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D5** — Result-ify `anisotropic_material_from_value` (3585-3681) → `Result<AnisotropicMaterial, FeaValueShapeError>`, using D2's now-`Result` `scalar_si_field`/`real_field` via `?`; shim its 3 call sites (in `classify_material_as_printed_zones`). Depends: D2. *Intermediate* (unlocks D6). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D6** — Result-ify `classify_material_as_printed_zones` (3446-3491), using D2/D4/D5 via `?`; shim its one call site (in `classify_material`). Depends: D2, D4, D5. *Intermediate* (unlocks D7). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D7** — Result-ify `classify_material` (3365-3429), using D2/D3/D6 via `?`; shim its one call site (in `solve_elastic_static_trampoline`, line 478). Depends: D2, D3, D6. *Intermediate* (unlocks D9). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D8** — Result-ify `extract_loads` (3788-3873) → `Result<([f64;3], Vec<PressureSpec>, [f64;3]), FeaValueShapeError>`; shim its one call site (line 501-502). *Intermediate* (unlocks D9), independent of D2-D7. Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.
- **D9** — Top-level validate-all-inputs gate: insert between the existing `AsPrintedZones`-Undef guard (457-470) and the `classify_material` call (478) — see Resolved decision 8 for exact placement — calling the now-`Result`-returning `classify_material` (D7), `extract_scalar_si` ×3 (D2), and `extract_loads` (D8), returning `ComputeOutcome::Failed` with a diagnostic naming the specific offending arg on the first `Err`, removing all D2/D3/D7/D8 shims. Depends: D2, D7, D8. **Leaf.** User-observable signal: **G2(a)** — `reify build` on an FEA fixture with a malformed/Undef input exits with a diagnostic naming the offending constraint/arg instead of a raw panic backtrace (the full-quality version; D1 delivers the crash-safety floor immediately, D9 delivers the naming quality). Files: `crates/reify-eval/src/compute_targets/elastic_static.rs`.

### Scope 4 — NaN ordering (INV-FEA-3)

- **E1** — Fail-closed guard at `find_closest_anchor` (`crates/reify-kernel-gmsh/src/mesh_boundary.rs:519-539`, unguarded `partial_cmp` at :537): finite-guard + `tracing::warn` + exclude non-finite candidates, mirroring the `through_thickness.rs:170-200` (#3178) template. **Leaf.** Signal: a regression test feeding a NaN candidate distance asserts the candidate is excluded (not silently ordered as equal) + a warn log line. Files: `crates/reify-kernel-gmsh/src/mesh_boundary.rs`.
- **E2** — Fail-closed guard at `mark_dorfler` (`crates/reify-solver-elastic/src/adaptive.rs:266-292`, unguarded `partial_cmp` at :273-278): finite-guard + `tracing::warn` once + treat non-finite indicators as zero-contribution/unmarked. **Leaf.** Signal: a regression test feeding a NaN indicator asserts it is never marked + a warn log line, and finite indicators still mark correctly. Files: `crates/reify-solver-elastic/src/adaptive.rs`.
- **E3** — `f64::total_cmp` at `TetSpatialIndex::build_recursive`'s median split (`crates/reify-solver-elastic/src/interpolation.rs:337-...`, unguarded `partial_cmp` at :387-391). **Leaf.** Signal: a regression test feeding a NaN centroid coordinate asserts the BVH build completes without panicking (deterministic ordering, not "correct" geometry — the contract is panic-freedom + determinism, documented as such). Files: `crates/reify-solver-elastic/src/interpolation.rs`.
- **E4** — Fail-closed guard at the modal frequency-ascending sort (`crates/reify-eval/src/modal_ops.rs:441-446`): finite-guard + `tracing::warn` + skip the corrective resort on non-finite input, preserving the eigensolver's own ascending-\|λ\| order. **Leaf.** Signal: a regression test feeding a NaN frequency asserts the resort is skipped (order preserved) + a warn log line. Files: `crates/reify-eval/src/modal_ops.rs`.
- **E5** — Fail-closed-with-`total_cmp`-fallback guard at `nearest_node` (`crates/reify-eval/src/modal_ops.rs:2784-2801`): finite-guard on `nodes`/`target` + `tracing::warn` + `total_cmp` for the actual pick (never a hard panic — see Resolved decision 5). **Leaf.** Signal: a regression test feeding a NaN node coordinate asserts a deterministic (not silently-Equal) pick + a warn log line. Files: `crates/reify-eval/src/modal_ops.rs`.
- **F1** — Grep-gate script `scripts/check-nan-safe-ordering.sh` (mirrors `check_event_inventory.sh`) asserting no unguarded `partial_cmp(...).unwrap_or(...Ordering::Equal...)` pattern remains in `reify-solver-elastic`, `reify-kernel-gmsh`, `reify-fdm`, `reify-shell-extract`, `reify-mesh-morph`, or `reify-eval`'s `compute_targets/`+`modal_ops.rs`; wired into `scripts/verify.sh`'s lint step + `tests/infra/test_nan_safe_ordering_guard_wired.sh`. Depends: E1, E2, E3, E4, E5 (gate must start clean). **Leaf.** Signal: the boundary-test sketch's last row — a synthetic unguarded site added to a scratch copy is caught by the gate during task execution; the committed gate fails CI on any future regression. Files: `scripts/check-nan-safe-ordering.sh` (NEW), `tests/infra/test_nan_safe_ordering_guard_wired.sh` (NEW), `scripts/verify.sh`.

**Cross-scope, out-of-batch edge:** `add_dependency(id=5023, depends_on=D1)` — wired at decompose time (5023 already exists as a deferred bookmark task; this PRD doesn't own filing it, only wiring the edge its own metadata calls for).

## Out of scope for this PRD

- `reify-constraints/src/solver.rs` tolerance constants (survey B5: inherent Nelder-Mead numerical tuning, not architecture — existing task-by-task pattern is correct). No tolerance work filed.
- Keystroke-time FEA solves in the LSP — rejected outright, not merely deferred.
- Task 4876 (gmsh segfault / attributed producer vertex-merging) — owned by the concurrent kernel-seam-contracts PRD; referenced only.
- NaN-ordering sites outside the survey's five named crates/modules found incidentally during the grep sweep (`reify-eval::persistent_cache.rs:1224`, `engine_build.rs:10710,10774`, `warm_pool.rs:449` — cache-eviction/version-ordering/event-ordering, not physical/geometric numeric domain) — noted for a possible future audit sweep, not filed here (scope discipline: the brief named specific crates/modules; expanding to all of `reify-eval` is a materially larger, un-ratified scope).
- `reify-audit` `Pattern`-framework consolidation of the two new grep gates (Resolved decision 6) — a future task if a third such gate appears.
- Any work on `reify-eval-fea-decomposition.md`'s crate-split (deferred/measure-gated; noted only as a future re-verification trigger for this PRD's anchors).

## Open questions (surfaced but not decided in this session)

1. **Exact wording/format of the LSP hint diagnostic message and its dedup key.** Suggested resolution: `"FEA constraint not evaluated in editor — run `reify test`"`, keyed per-constraint by `ConstraintCheckEntry.id`/label pair (same identity `diagnostics.rs:190/236` already use to filter). Decide during task C2.
2. **`FeaValueShapeError`'s exact `Display` wording per variant** (used verbatim in D9's user-facing diagnostic). Suggested resolution: `"expected {expected} for {context}, got {got}"` shape, matching today's panic-message wording so the upgrade is additive (same information, no `panic!`/backtrace). Decide during task D9 (informed by D2-D8's variant choices).
3. **Whether `A5`/`F1`'s grep scripts should also self-test against a synthetic fixture directory (like PTODO's `tests/fixtures/ptodo/`) vs. a purely regex-based CI-time check with no fixture corpus.** Suggested resolution: no fixture corpus needed at this scale (3-4 known call sites / 5 known crates) — a direct grep over `git ls-files` is sufficient, matching `check_event_inventory.sh`'s own no-fixture precedent. Decide during A5/F1.
