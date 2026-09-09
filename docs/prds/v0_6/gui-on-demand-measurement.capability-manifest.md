# Capability manifest — `gui-on-demand-measurement`

**PRD:** `docs/prds/v0_6/gui-on-demand-measurement.md` (landed `4df223e6ac`, 2026-08-26)
**Decomposed:** 2026-08-26 · **Substrate HEAD:** `4df223e6ac` (re-verified against `108d1d9226`;
the four intervening commits are docs-only — `git diff --name-only 4df223e6ac..HEAD -- gui/ crates/`
is empty)
**Batch:** 9 leaves (**#6740–#6748**) · **22 dependency edges** — 19 intra-batch, 2 out-of-batch
(β ← #6667, ε ← #6666), plus 1 repair edge (#6667 ← #6169, amendment A16)

| Label | Task | Prio | Prereqs | Kind |
|---|---|---|---|---|
| α | **#6740** | high | — | leaf · the shared seam |
| β | **#6741** | high | α, **#6667** | leaf · the end-to-end GUI pass |
| γ | **#6742** | medium | β | leaf |
| δ | **#6743** | high | β, γ | leaf · the designer-facing surface |
| ε | **#6744** | medium | β, **#6666** | leaf |
| ζ | **#6745** | high | β, γ | leaf · **integration gate (G5 B+H closure)** |
| η | **#6746** | medium | δ, ε | leaf (docs-truth 1+3+4) |
| θ | **#6747** | medium | α, β | leaf (docs + task records) |
| ι | **#6748** | medium | α β γ δ ε ζ η θ | leaf · PRD close |

**Machine-readable twin:** `docs/prds/v0_6/gui-on-demand-measurement.capability-manifest.yaml`
(9 labels · **58 capability bindings** · **0 FAIL** · 43 mechanical `delivered_check`s, 15 `manual`).

**G3 grammar gate: N/A.** This PRD adds **no novel `.ri` syntax** — the three constraint kinds,
`#precision`, `RepresentationWithin`, geometric `Conforms` and `DFMRule` all ship today and are
exercised by committed fixtures. Only *where* they get measured changes. Per
`references/grammar-gate.md` the gate is a no-op here and is recorded as such rather than as a
fabricated passing fixture. `grammar_confirmed: true` on every leaf.

---

## Verification method, and what it did and did not cover

**Four parallel read-only substrate agents** (engine/CLI symbols · GUI symbols · test-infra
drift guards · manifest schema + house style) plus **direct probe execution** against
`target/release/reify` on `main`. Every mechanical `delivered_check` in the sidecar was
**executed** at decompose time via `git grep -E` with its declared `paths`, and its polarity
compared against `expect`: **43/43 ran, 39 agree with today's tree, 4 are deliberately
deliverable-shaped** (they fail now and pass when their producer lands — α's two, β's
`PURE_ENGINE_SIDE` entry, ι's terminal stamp).

**Three checks were demoted to `manual` during that sweep**, each because a grep would have been
a *false* check rather than a weak one:

- α `inert-refine-flag-carries-a-ptodo-cite` — `#6171` already appears in two
  `crates/reify-eval/src/tolerance_combine.rs` docstrings, so a tree-scoped grep passes
  **vacuously** without the cite ever being written. PTODO enforcement is the real gate.
- δ's two constraint-status rows — the fix direction (normalize the frontend to PascalCase vs
  lowercase the backend vs a shared mapper) is genuinely tactical, so a grep pinned to one side
  **inverts** under the other two and would red a correct fix.

### D3 decompose-verify workflow — disposition

`scripts/prd-decompose-verify.mjs` was **not** run as a workflow, and this is a deliberate,
recorded disposition rather than a skipped gate. Its α probe runner
(`scripts/prd-capability-check.py`) drives exactly three vectors — `tree-sitter parse`,
`reify check`, `reify eval` — and this PRD's leaf premises split cleanly in two:

1. **The `.ri`-observable premises were executed by hand, with captured output** (table below).
   These are the same commands the harness would have issued; running them directly gave the
   verbatim diagnostics now bound into β's and ε's rows.
2. **The GUI-runtime premises are outside the harness's probe-vector set by construction.**
   There is no probe vector that can observe "the constraint panel shows a measured verdict" —
   *building that vector is what leaf β's `measure_constraints` / `measurement_status` are for*.
   Feeding those signals to the Enumerator would return `UNPROVABLE`, which **blocks**, and the
   block would be spurious: it would report the absence of the surface this PRD exists to add.

In-corpus precedent for exactly this split: the `precision-nominal-representation-guarantee`
manifest records "the harness's probe kinds are grammar/check/ir only, so it cannot drive
`reify build`; the export-surface probes below were run by hand alongside it."

### Probes executed (2026-08-26, `target/release/reify`, main `4df223e6ac`)

| Fixture | Command | Observed |
|---|---|---|
| `examples/representation_within.ri` | `reify check` | `VIOLATED CurvedBallCheck#constraint[0]` · `error: RepresentationWithin: sampled facet deviation 6.006e-2 m exceeds bound 1.000e-6 m` · **exit 1** |
| `crates/reify-cli/tests/fixtures/representation_within_satisfied.ri` | `reify check` | `OK SphereCheck#constraint[0]` · `All constraints satisfied.` · **exit 0** |
| `examples/gdt_conformance_violated.ri` | `reify check` | `VIOLATED Conforms#0[0]` · `error: Conforms VIOLATED: measured deviation 0.5000 mm exceeds the 0.1000 mm tolerance zone` · **exit 1** |
| `examples/gdt_conformance_satisfied.ri` | `reify check` | `OK Conforms#0[0]` · **exit 0** |
| `crates/reify-cli/tests/fixtures/dfm_thickness.ri` | `reify check` | `warning: W_DFM_MIN_WALL…` + `warning: W_DFM_MIN_FEATURE…` · **exit 0** |
| `crates/reify-cli/tests/fixtures/dfm_thickness_error.ri` | `reify check` | `error: E_DFM_MIN_WALL…` + `error: E_DFM_MIN_FEATURE…` · **exit 1** |

**BT-1's fixture corpus therefore already exists**, on both polarities, for every kind. That is
the strongest single result of this decompose: the parity oracle needs assertion shapes, not a
fixture-authoring campaign. Two named gaps remain and are written into ζ (no focused
`_DFM_DRAFT` CLI fixture; the nine `pnrg_*` prd-gate probes carry a documented "OK trap" and are
consumed by no automated test).

---

## Binding decompose amendments

Sixteen amendments. Each is written **verbatim into the filed task** and carries a row in the
sidecar. None changes the PRD's direction or any §8 contract's intent; A5, A11 and A13 change
what an implementer would otherwise build.

| # | Leaf | Amendment |
|---|---|---|
| **A1** | α | The four `module_has_*` kind detectors are **private free fns in the `reify-cli` binary crate** — not importable. C-SEAM's "kind gating lives inside the seam" means *moving* them into `reify-eval`. Second-order: `module_has_thickness_dfm_rule`'s body is literally `module_has_dfm_rule(module)`, so `ensure_openvdb_kernel` fires for **every** DFM module today. |
| **A2** | α | `cmd_check --purpose` **never enters the kernel arm** — it calls `check_constraints_with_values` + `run_gdt_check_passes` directly. C-SEAM must say whether that branch routes through the seam. |
| **A3** | α/β | **`tessellate_snapshot` already honours `capture_repr_tol`.** The GUI's existing rebuild surface can populate `achieved_repr_tol` without `tessellate_realizations`; C-SEAM as written makes the GUI tessellate **twice**. Folds into PRD Open Question 4. |
| **A4** | β/γ | **`reset_per_build_state` clears `achieved_repr_tol` on `TessellateSnapshot`**, and `build_gui_state` calls it on every rebuild. D7's retained-stale verdicts **cannot** live in the engine map; the session owns its own result store. C-STATE's "restore on every exit path" must not be read as "clear the map". |
| **A5** | β | **`ConstraintCheckEntry` carries no measured value**, and Conforms/DFM measurements exist **only as diagnostic strings**. C-STATUS's `measured(verdict, values, epoch)` needs a new structured channel, or an explicit v1 scope-cut to verdict-only for those two kinds. Parsing diagnostic prose is not acceptable as the parity-gate surface. **RESOLVED — see the resolution log below.** |
| **A6** | β | **Demand roots have no getter.** `set_demand_selective` replaces wholesale; nothing reads the root set back. C-STATE's "restore the demand roots" is **not achievable with today's public surface** — α adds an accessor or β caches the last `sync_demand` argument. |
| **A7** | β | `measure_constraints` / `measurement_status` are MCP **tools**, not JSON-RPC methods (that table has exactly four arms). Same-diff: `tool_defs()`, an engine-side `dispatch_tool` arm, `run_on_engine`, an in-file `tool_defs_registers_*` test, a **`PURE_ENGINE_SIDE` entry in `gui/src/__tests__/debugParity.test.ts`** (self-checking — omission reds vitest), and `docs/debug-mcp-contract.md` §0. `measurement_status` must use the `demand_dispatch_json` **pure-read** shape, never `engine_state_json` (which re-tessellates). |
| **A8** | β/γ | `ConstraintData` has **two construction paths** — `build_gui_state` and `set_active_fea_case`. A field populated only at the first means an FEA case switch silently blanks measured verdicts. |
| **A9** | β | The solve slot **debug_asserts at most one live handle**, and is lifecycle-only (the trampoline ignores its handle). Sharing it serialises measurement with FEA solves; real mid-pass cancellation needs the handle honoured **inside** the seam. |
| **A10** | γ | **Resolves PRD Open Question 2 outright: nothing to reuse.** No generation/epoch counter exists; the engine's version accessors are `pub(crate)`; `CacheStore::version()` is public but frozen at `VersionId(0)` in production. Add a plain `u64` on `EngineSession`. |
| **A11** | δ | **LIVE DEFECT blocking δ's signal** — see below. |
| **A12** | β | `set_parameter` calls **`Engine::edit_check`, not `check()`**. PRD §2/§3's "the GUI runs `engine.check()` at every mutating entry point" is false for the warm-edit path. The solve-slot lifecycle *is* uniform across all four, so C-CANCEL survives; C-SEAM's framing does not. |
| **A13** | ζ | **The drift-guard registration set the PRD names is wrong** — see below. |
| **A14** | ζ/ε | **BT-1's thickness row is homed on ε**, not ζ. §9 gives ζ a corpus including thickness while §10 wires ζ to β and γ only — ζ would assert a capability its sibling ε produces (`producer-downstream`). The split line is exactly the `ensure_openvdb_kernel` gating boundary, and it keeps ζ off #6666's chain. |
| **A15** | ε | **`has_openvdb` is a build-script cfg, not a cargo feature.** BT-7's "no openvdb" leg gates on `#[cfg(not(has_openvdb))]`. |
| **A16** | θ | The **#6667 → #6169 edge was missing and is wired by this decompose session**, not deferred to θ. |

### A5 resolution log (added 2026-09-01; the row above is the as-authored finding)

A5 was authored as an open choice. It has since been closed twice, and both rulings post-date
this manifest's landing — recorded here rather than by rewriting the row, so the decompose-time
finding stays legible.

**RULED 2026-08-28 (Leo) — the structured channel.** Deliver `measured(verdict, values, epoch)`
as structured data for all three kernel-measured kinds, including GD&T Conforms and DFM. The v1
verdict-only scope-cut is explicitly rejected: a strings-only carrier forces every consumer —
the constraint panel, the cross-driver parity gates, the conformance suite's Ring-2 observation,
#6667's attribution wording — into message-substring parsing, the oracle class the
DiagnosticCode-is-the-contract decision retired; and the C-STALE epoch machinery only composes
uniformly if all three kinds ride one structured payload.

**REFINED 2026-09-01 (A5-R) — the carrier must be dimensioned, and its home was already ruled
elsewhere.** Two things the 08-28 ruling left open:

1. **Home.** `solver-legibility-telemetry` §8.1 item 2 already resolved the
   `ConstraintCheckEntry` collision — a **sibling field**, never a payload on `Satisfaction`
   (whose `content_hash` feeds the incremental cache key) — but that constraint lived only in
   P4's PRD and had not been carried into the task text an implementer reads. Its ε #6722 adds
   `margin` to the same struct; no dependency edge exists in either direction by design, and the
   rule is *whichever lands second extends the first's shape*.
2. **Type.** "Extend the first's shape" cannot mean "copy `Option<f64>`". The values are
   dimensionally heterogeneous — ReprWithin deviation and DFM min-wall/min-feature are **Lengths**,
   DFM overhang and draft are **Angles** — so an undifferentiated `f64` is the silent rad=1 SI
   erasure INV-AD-4 calls a defect *even when the number is correct*, re-committing at the value
   model what this PRD's own G7 addition forbids at the wire boundary. Carrier candidates:
   `Value::Scalar { si_value, dimension }` (exists today) or a typed per-kind enum on the
   `StructuredComputeDetail` precedent. Never a bare f64.

The finding cuts at #6722 symmetrically — `margin: Option<f64>` carries the same latent erasure
for any angle-valued constraint — so P4's one-way coordination note was made **bidirectional**,
and #6722's own internal inconsistency was flagged there (its WORK item 3 specifies
`Option<f64>` while its details already recommend a "dimension-carrying" slot).

**Scope cuts, named as follow-ups rather than silently dropped:** `achieved_repr_tol`
subsumption is out (occurrence-keyed vs `ConstraintNodeId`-keyed); making the diagnostic string
*derived* from the structured record is out (contests P4's C5).

**Where this landed.** Under E3 (see below) the amendment was written to the live coarse task
**#6898**, mirrored to retired standard leaf **#6741** (a §11 rollback target), noted on
**#6722**, and logged as a dated addendum in the experiment's §10.

### E3 re-decomposition — this batch's leaves are retired

On 2026-08-28 this PRD randomized to the **coarse arm** of the E3 decomposition-granularity A/B
(`docs/prds/v0_6/e3-decomposition-granularity-ab.md`), which found reify tasks generally smaller
than optimal. Its 9 leaves became 4: **#6898** (α+β+γ), **#6899** (δ+ε+θ+η), with the
integration gate **#6745** and PRD-close **#6748** preserved as singletons by protocol. Leaves
#6740–#6744, #6746 and #6747 are `x_e3_arm: standard-retired`, deferred, and are rollback
targets — not dead text. **Amendments must be mirrored into both the coarse task and its retired
constituents.** The label→task table at the top of this manifest remains the decompose-time
record; the coarse mapping is in the experiment doc §5.

### A11 in full — the live defect that makes δ's signal vacuous

`build_constraints` (`gui/src-tauri/src/engine.rs`) emits **PascalCase** `"Satisfied"` /
`"Violated"` / `"Indeterminate"`. `ConstraintPanel.tsx`'s `statusIcon` / `statusTitle` /
`isExpandable` / `STATUS_PRIORITY`, `StatusBar.tsx`'s `constraintSummary`, and `ChatPanel.tsx`'s
`hasViolatedConstraints` **all compare lowercase**, and no `toLowerCase` or normalisation exists
anywhere in `gui/src`. Verified independently by direct read of both sides, 2026-08-26.

Today, in the running GUI: every badge renders `?` with the "Indeterminate — not yet evaluated"
tooltip; `data-status="Satisfied"` matches no CSS rule so no colour is applied; every row is
"expandable"; `StatusBar` counts everything indeterminate; and `ChatPanel`'s violated-constraint
context option never enables.

Two consequences for this batch:

1. **δ's user-observable signal is unobservable until this is fixed.** A measured `Satisfied`
   would still render `?`. This is precisely the G2 fake-done shape the gate exists to catch,
   and it would have surfaced at δ's dispatch against a RED test.
2. **PRD §2's premise is true for a broader reason than it states.** "All three kinds render
   Indeterminate in the constraint panel" holds — but so does *every other constraint kind*,
   because of the case mismatch rather than the measurement gap. The measurement gap is real
   and independently verified (§E below); the *rendering* evidence is confounded.

**And the existing pins cannot catch it.** `ConstraintPanel.test.tsx`'s `makeConstraint` factory
defaults `status: 'satisfied'`; `StatusBar.test.tsx`'s takes lowercase; `engineStore.test.ts`'s
fixtures are lowercase. **No test anywhere feeds PascalCase status into a frontend component**,
while the Rust side pins PascalCase hard (`types_tests.rs`, `commands_tests.rs`,
`engine_tests.rs`, `gui_state_parity_tests.rs`). The fix owes a **two-sided pin**, not a fixture
flip.

**Homed in δ, not split out**, because δ already owns the constraint panel and its vitest suite,
and a separate task would be ordered by prose — exactly the shape the overlay's docs-truth and
drift-guard rules forbid. It also blocks **#6667**'s own signal (attributable-Indeterminate
wording rendered in the panel); that is out of this batch and is called out in the hand-back.

### A13 in full — the corrected drift-guard registration set

The PRD's ζ text names "nextest heavy/smoke partition entries" in `.config/nextest.toml`, a
`run-all-classification.manifest` bucket row, and "no new wall-clock upper bounds". All three are
wrong or incomplete:

| PRD says | Reality |
|---|---|
| heavy/smoke partition in `.config/nextest.toml` | The partition is `REIFY_HEAVY_NEXTEST_FILTER` in **`scripts/heavy-test-filter-lib.sh`** — a hand-maintained or-joined filterset, "the one place membership can ever change"; `verify.sh` negates it under `REIFY_GATE_EXCLUDE_HEAVY=1`. `.config/nextest.toml` governs only the `occt` concurrency group, `slow-timeout` ceilings and LPT `priority`. |
| bucket row in `run-all-classification.manifest` | That manifest covers **only `tests/infra/test_*.sh`**. Rust integration tests need no row. Owed only if ζ also adds a shell test. |
| wall-clock registration | `tests/infra/test_no_new_wallclock_upper_bounds.sh` scans **`tests/infra/*.sh` only** and has no allowlist file — the escape is an inline `# wallclock:allow` comment. A Rust elapsed-time assert is **outside the guard entirely**, so this is a ζ *decision*, not a registration. Assert structurally, as precision-δ (#6168) does. |
| — (unstated) | **`gui/src-tauri` tests cannot be heavy-excluded.** The `-p reify-gui --features gui` pass carries **no `-E` filterset by design**, pinned by assertion (b8) of `tests/infra/test_compute_trampoline_registration_wired.sh`; and `test_heavy_filter_atoms.sh` resolves atoms as `crates/$pkg/tests/$bin.rs`, so a `package(reify-gui)` atom would false-fail. **Any BT test landing there is permanently gate-resident and must be fast.** The seconds-class measurement tests (§2: ~10.6 s for one bounded sphere) belong in `crates/reify-eval/tests/`. |
| — (unstated) | **`crates/reify-eval/tests/` rejects standalone files.** `reify-eval` is one of five consolidatable crates, so a standalone `tests/<f>.rs` fails `scripts/check-harness-baseline-registration.sh` with `reason=unregistered-standalone` — an early plan gate, before any compile. Grandfathering a `harness-layout-baseline.manifest` row is **SUPERSEDED** (Leo 2026-07-22, esc-5056-11). BT tests land as a `#[path]` submodule under `harness_<subsystem>/`, under the 20 000-raw-line per-unit cap. |
| — (unstated) | A new `.ri` fixture a compiled test reads must join **`_RUST_COUPLED_RI_FIXTURES`** in `scripts/verify.sh`, or `test_verify_scope.sh`'s PG-DRIFT goes red. Self-enforcing. |

The overlay's own decompose rule — *reject a batch where the registration task is downstream of
or unordered with respect to the test-adding task* — is satisfied structurally: **every one of
these registrations is same-diff inside ζ**, so the esc-4914-162 A3-before-A6 shape is
impossible in this batch.

---

## Per-task bindings

Verdict vocabulary here: `PASS` · `PASS-with-amendment` (substrate falsified or extended the PRD;
the correction is written into the filed task). The sidecar's `verdict` enum is
`PASS`/`FAIL`/`OPEN` only, so amended rows carry `verdict: PASS` there with a leading
`PASS-WITH-AMENDMENT.` token in the `binding` prose. **No row resolves to a FAIL value.**

Full evidence text for all 58 rows lives in the sidecar; this section carries the reasoning that
does not fit a `binding` string.

### α — the shared seam (#6740)

Ten bindings. The reference arm sequence is live and the PRD's `:598-651` citation re-verified
exact. The three substantive findings are **A1** (the detectors are binary-crate-private, so the
extraction is a move, not a re-call), **A2** (the `--purpose` branch bypasses the arm entirely)
and **A3** (`tessellate_snapshot` already captures).

`refine`'s absence is bound as a G6 branch-4 vacuity check and holds strongly: every `refine` hit
in `crates/reify-eval/src/` is trait refinement, FEA a-posteriori adaptive refinement, or FDM
progressive refinement. The only repr-tolerance references are two forward-looking docstrings
naming #6171/#6168 — **no such caller exists**, which is exactly the ordering D2 rules for.

`reset_per_build_state`'s exhaustive no-`..` destructure makes INV-BUILD-1 **self-enforcing** for
the new flag: it lands as a sibling or the crate does not compile.

G7 hit, resolved not waived: the flag ships **inert** until #6171 lands, which is a placeholder
surface under the PTODO grammar and must carry a `#6171` cite (INV-SF-5).

### β — the end-to-end GUI pass (#6741)

Thirteen bindings, the most amended leaf. **A5 is the largest unplanned-work risk in the batch**:
C-STATUS promises `measured(verdict, values, epoch)`, but `ConstraintCheckEntry` is
`{ id, label, satisfaction }` and the only structured measured-value read-back on `main` is
`Engine::achieved_repr_tol(occurrence) -> Option<f64>`, for ReprWithin alone. Geometric-Conforms
and DFM values exist only inside diagnostic message text.

**A4** and **A6** together define C-STATE's real shape: the measurement store must be
session-owned (the engine map is wiped on every rebuild), and restoring demand roots is not
possible with today's public surface. `build_gui_state_full_scene` is the exact in-tree template
for the save/restore-under-`catch_unwind` discipline, and its own doc already argues why a leaked
cache entry is a **correctness** leak, not a perf leak.

**A7** converts the PRD's "JSON-RPC methods" into the real registration set, with
`demand_dispatch` and `set_fea_case` as verbatim precedents, and pins `measurement_status` to the
pure-read shape — a status read that re-tessellates would make BT-3 unobservable through the very
surface the future parity gate drives.

G7 hits, resolved not waived: **INV-AD-4** — overhang and draft measurements are *angular*, and β
carries them across two new boundaries (the Tauri payload, the debug-MCP results), so the
convention must be declared in a greppable contract comment; **INV-SF-6 + INV-SF-2** — every
degradation cause carries a `DiagnosticCode` and must not be Error severity, since kernel-absent
and `not(has_openvdb)` are healthy GUI paths; **INV-SF-4** — each cause classifies expected vs
unexpected per Leo's 2026-08-26 Indeterminate doctrine.

### γ — staleness epochs (#6742)

Four bindings. **A10 answers PRD Open Question 2 outright** — there is nothing to reuse, so the
tactical question is closed at decompose rather than at implementation. The stale flag rides the
generated `diffed keyed` channel with no new event plumbing; `#[serde(default)]` is the in-tree
forward-compat idiom. The four production bump sites are enumerated; `set_active_fea_case` mutates
presentation without re-eval and must **not** bump.

### δ — the designer-facing surface (#6743)

Five bindings, dominated by **A11** above. The idle trigger has an exact in-tree precedent in
`createSelectiveDemandSync` — including the **load-bearing `onCleanup(clearTimeout)`** whose
absence caused esc-4853-42 (task 4856) — combined with `App.tsx`'s idle *gate*; δ wants both.
`engineStore`'s 1 s `applySolverProgress` visibility debounce is the shape C-STATUS's `measuring`
state needs so sub-second passes never flash a spinner.

The stale-render precedent (`ProbeSystem`: grey marker, reduced opacity, "Re-pin" button, boolean
cleared by a fresh sample) supplies the **UX shape only** — `.probe-stale` has no CSS rule
anywhere, so the DOM dim style is authored from scratch. `ConstraintPanel.module.css`'s existing
`.statusBadge[data-status="…"]` selectors are the natural hook.

### ε — the thickness / OpenVDB leg (#6744)

Six bindings. The parity reference is probe-verified on both severities. **A15** corrects the
feature-vs-cfg claim; **A14** homes BT-1's thickness row here. `ensure_openvdb_kernel` has a
second production caller (`cmd_build`'s isosurface arm) and its docstring claiming sole
`cmd_check` ownership is **stale source** — flagged for a drive-by fix. Because the method is
idempotent and registry-driven, the seam calling it after #6666 has registered the kernel is a
no-op, not a conflict.

### ζ — the integration gate (#6745)

Ten bindings, dominated by **A13**. Two structural results make the gate cheap:

- **BT-4 holds by construction.** In `surface_subtree`'s tessellate callback the mesh is produced
  *before* the `if capture_repr_tol` gate and pushed unconditionally; the capture branch only
  reads it. Capture-on/off byte-identity is structural, so BT-4 is a cheap pin, not a risk.
- **BT-8's negative is observable at two independent points** — `cmd_check`'s lightweight
  `Engine::new` else-branch, and `dispatch_constraints`' zero-allocation fast-path guard.

**G5 B+H closure:** ζ is the integration-gate task and names the §9 boundary-test sketch as its
signal (BT-1..BT-4 + BT-8; BT-5 lands with γ, BT-6/BT-7 with β and ε).

### η — docs-truth (#6746)

Four bindings. `constraints.md` and `stdlib.md` are the target chunks (`stdlib.md` already names
`std.process — manufacturing process traits, DFM rules`); neither carries GUI-measurement prose
today, so the discoverability acceptance cannot pass vacuously. **Docs-truth arm 2 (exemplar
corpus) is correctly N/A** — its trigger is conditional on a new authoring idiom, and this PRD
introduces none. Recorded explicitly rather than filed as a vacuous leaf. Arms 1, 3 and 4 are all
carried, and η is wired to δ and ε by **real edges**, never prose.

### θ — companion corrections (#6747)

Four bindings. Both §4.6 texts θ must annotate are present verbatim. **A16**: #6667's own text
tells its implementer to "locate the zeta mechanism … do not invent a parallel attribution
channel", but that mechanism is #6169 and #6667 carried **no dependency edge**. Since β depends
on #6667, deferring the edge to θ would let #6667 dispatch with nothing to reuse — the exact
outcome its own text forbids. Wired at decompose; θ verifies.

θ cites task IDs only and says nothing about status, per the overlay's terminal-status rule.

### ι — PRD close (#6748)

Two bindings, both process capabilities. Decompose-close obligation (1) — backfilling real leaf
IDs into §10 — is discharged by **this decompose session's own commit**, not by ι; ι owns the
terminal stamp, the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, applied to both the
PRD and this manifest. Cancelled sibling leaves count as satisfied for ι's edges.

---

## G7 walk — `docs/legibility/design-invariants.md`

Walked across **all 9** tasks. **No waiver required**; `metadata.g7_waivers` is unset on every
filed task. Four invariants produced design additions rather than clean passes — each is
**resolved by design**, written into the owning task.

| Invariant | Batch-wide verdict |
|---|---|
| `undef-has-provenance` (INV-SF-1) | No task creates a root `Undef` cell. D6 keeps the measurement pass out of the value-cell overlay entirely, so no new cells exist to be undef. **Clear.** |
| `error-severity-exits-nonzero` (INV-SF-2) | **Design addition (β, ε).** Kernel-absent and `not(has_openvdb)` are *healthy* GUI paths, so degradation diagnostics must not be Error severity — the severity-hygiene corollary. α additionally must preserve `cmd_check`'s exit behaviour exactly (C-SEAM), pinned by the unmodified CLI harness suites. **Clear once stamped.** |
| `declared-intent-consumed-or-diagnosed` (INV-SF-3) | This is the PRD's purpose. C-STATUS forbids a bare Indeterminate and D4's kind gating is the required negative. **A11 is itself an INV-SF-3 violation** — a verdict is computed and then silently dropped at the render boundary — and δ closes it. **Clear; this batch closes a hole.** |
| `indeterminate-attributable-transient` (INV-SF-4) | The invariant this PRD serves most directly. **Design addition (β):** each `unmeasured(cause)` must classify **expected vs unexpected** per Leo's 2026-08-26 doctrine, not merely carry a reason string. **Clear once stamped.** |
| `placeholders-owned-and-loud` (INV-SF-5) | **Hit, resolved (α).** The `refine` flag ships inert until #6171 lands — a placeholder surface requiring a live `#6171` cite under the PTODO grammar. Resolved by design, not waived. |
| `diagnostics-carry-codes` (INV-SF-6) | **Design addition (β, ε).** Every new degradation cause carries a `DiagnosticCode`. Note the batch inherits, but does not worsen, the existing `E_DFM_` message-prefix escalation this invariant's own evidence cites. **Clear once stamped.** |
| `parse-is-value-faithful` (INV-SF-7) | No grammar change anywhere in the batch (G3 N/A). **Clear.** |
| `angle-crossings-explicit` (INV-AD-1) | Nothing is retyped to Angle; DFM draft/overhang angles come from the shipped `measure_dfm_rules`. **Clear.** |
| `quotient-pure-derivative-algebra` (INV-AD-2) | No operator row added or retyped. **Clear.** |
| `tensor-single-quantity` (INV-AD-3) | No tensor surface touched. **Clear.** |
| `boundaries-declare-angle-convention` (INV-AD-4) | **Hit, resolved (β).** Overhang and draft measurements are angular and β carries them across two **new** boundaries — the Tauri constraint payload and the debug-MCP tool results. The convention must be declared in a greppable contract comment or schema text. The pre-existing undeclared GUI/MCP channels are chartered elsewhere (angle-dimension-completion ι/υ); this covers only the boundary β adds. Resolved by design, not waived. |

---

## Gate summary

| Gate | Verdict |
|---|---|
| **G1** — consumer named | **PASS.** Every §4 mechanism names a consumer: the seam → `cmd_check` (rewired) + β + the future driver-contract suite; the flag split → #6171 + the GUI capture-only path; the session pass + Tauri command → δ's affordances and the debug-MCP tools; epoch + stale flag → panel rendering; the debug-MCP tools → the parity gate and ζ's tests; the frontend affordances → the designer in the viewport, the G1 primary consumer. Engine-integration sub-check: the seam is a **driver-level** composition of existing engine surfaces, not a new in-engine seam, so `engine-integration-norm.md` §3 does not apply and no norm extension is owed. |
| **G2** — user-observable leaf | **PASS**, with **A11 recorded as the one real threat.** Every leaf names a signal from the overlay's vocabulary (CLI output difference, GUI state via debug MCP, vitest, committed docs). No leaf's only signal is a unit test against synthetic input. δ's signal was **vacuous as written** and is repaired by folding A11 into δ — caught here rather than at δ's dispatch. |
| **G3** — substrate verified | **PASS.** Every §3 row read- or grep-verified at `4df223e6ac`; all four §3 negative claims confirmed, and **stronger** than stated (zero hits across the whole `gui/` tree, untracked files included, with a non-vacuous positive control). Grammar gate N/A. Four §3 imprecisions corrected (A2, A12, A15, and the `run_gdt_check_passes` grouping — it is kernel-free and **already runs in the GUI** via `check()`, so §2's phrasing is the correct one). |
| **G4** — seam ownership | **PASS.** §7's table has a named owner per row and no reciprocal-ownership pattern. The precision PRD keeps owning refine (γ1/γ2) while this PRD owns the flag split; the parity gate stays with the future driver-contract PRD while tool names stay here; #6190 is explicitly not absorbed. **No fourth contested pair introduced** — the three known pairs (persistent-naming↔multi-kernel, imported-field-source↔multi-kernel, topology-selectors↔persistent-naming) are untouched. |
| **G5** — B+H | **PASS.** Contract §8 (C-SEAM/C-FLAGS/C-STATE/C-CANCEL/C-STALE/C-STATUS/C-MCP) and boundary-test sketch §9 both present; integration-gate task **ζ (#6745)** names the §9 matrix as its signal. High-stakes seam triggers present (GUI/engine boundary, cross-PRD consumers ≥ 2). |
| **G6** — premise validity | **PASS.** Every leaf's numeric/capability/rejection premise bound: the measured-verdict premises **probe-verified live on both polarities**; the refine-absence and GUI-never-measures vacuity halves grep-verified with positive controls; `has_openvdb` corrected from feature to cfg. **One anti-inversion repair (A14)** — ζ's BT-1 thickness row moved to its producer ε. No leaf asserts a numeric bound of its own, so branches 1 and 2 do not fire; the batch inherits precision-γ1's floor only where it inherits C-LOOP, which it does not. |
| **G7** — design invariants | **PASS**, no waiver. Four design additions (INV-SF-2, INV-SF-4, INV-SF-5, INV-AD-4) written into their owning tasks. |
| **Manifest** | **0 FAIL.** 58 bindings · 43 mechanical checks all executed at decompose · 16 binding amendments, each written verbatim into its filed task. |
