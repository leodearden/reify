# PRD: `std.process` §8 — process-category parameter surface + declarative DFM evaluation engine

**Status:** Draft · **Author session:** 2026-06-03 · **Milestone:** v0_6
**Closes:** gap-register `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` cluster **P14 process-dfm** (all 3 rows) + the Bucket-B §8 DFMRule doc-reconcile row.
**Source doc:** `docs/reify-stdlib-reference.md` §8 (`std.process`).
**Substrate file under change:** `crates/reify-compiler/stdlib/process.ri` (+ a new `crates/reify-stdlib/src/dfm.rs` builtin module).
**Forward stub for the deferred half:** `docs/prds/v0_6/process-dfm-geometry-metrology.md` (the geometry-metrology DFM engine — kept out of this PRD; tracked, not dropped).

---

## §0 — Scope boundary: this PRD owns the *declarative DFM surface*, NOT geometry metrology

`std.process` has two cleanly separable layers. This PRD owns the first; the second is a
substrate-heavy B+H follow-up captured in the forward stub PRD and held by deferred tracker tasks.

| Layer | What | This PRD? |
|---|---|---|
| **Declarative DFM surface** — the §8 *process-category capability params*, the `DFMRule` trait, and DFM checks expressed as `constraint def`s over **design-declared / user-supplied** measured values + **geometry queries that already exist** (`bounding_box`, `min_clearance`), surfaced via `reify check` and a `DFMSeverity`-aware diagnostic classifier | **YES** |
| **Geometry-metrology DFM engine** — NEW kernel queries (overhang-angle scan vs build direction, draft detection, min-wall-thickness via medial axis, min-feature-size) + an auto-extraction pass that realizes the subject solid, measures features, and auto-runs all applicable rules | **NO — deferred**, `process-dfm-geometry-metrology.md` + deferred trackers |

The split mirrors how the just-shipped `std.tolerancing` §7 PRD
(`tolerancing-gdt-surface-completion.md`) made `Conforms` a real, non-tautological constraint over
a **user-supplied** `measured_deviation` and explicitly deferred the metrology (we cannot measure a
manufactured/realized feature generically at design time without new kernel ops). DFM is the same
shape: a process declares a *capability* (`min_feature_size`, `build_volume`, `min_bend_radius`,
…); a rule checks a design's *declared or cheaply-measured* feature against that capability.

The new builtin module `reify-stdlib/src/dfm.rs` is a **sibling of `stackup.rs`/`tolerancing.rs`**,
wired into the same `eval_builtin` dispatch chain (`reify-stdlib/src/lib.rs`); it neither imports
nor is imported by the kernel-budget machinery, `stackup.rs`, or `tolerancing.rs`.

---

## §1 — Consumer (G1)

**Named consumer: a manufacturing/DFM-minded designer annotating a `.ri` part model with a
process + DFM rules and running `reify check`.** The user-observable surface is the CLI: `reify
check` reports each constraint `OK`/`VIOLATED` and emits `DFMSeverity`-tagged diagnostics.

Every mechanism this PRD introduces names its consumer:

| Mechanism | Consumer |
|---|---|
| 14 capability params on the 7 category traits (`Subtracting.min_feature_size`, `Adding.build_volume`, `Forming.min_bend_radius`, …) | A designer writes `structure def MilledBracket : Subtracting { … }` supplying the process capabilities; `reify eval` reads `proc.min_feature_size`; a DFM `constraint` checks against it |
| `Manufacturable` + per-category DFM `constraint def`s (new, `process.ri`) | A designer writes `constraint Manufacturable(rule: r, measured: wall, capability: proc.min_feature_size)`; `reify check` reports `VIOLATED` when the design under-runs the process capability |
| `fits_build_volume(part, envelope) -> Bool` builtin (new, `dfm.rs`) | The `Adding`/build-volume DFM rule: `reify check` flips pass→fail when a part is scaled past the printer envelope — a **geometry-backed** check using the *existing* `BoundingBox` query (no new kernel work) |
| `dfm::diagnose` severity classifier (new, `dfm.rs`) | `reify check` emits `W_DFM_*` (Warning) / `E_DFM_*` (Error) / `I_DFM_*` (Info) per a rule's `DFMSeverity` on the violation path — so the `DFMSeverity` enum does real work (mirrors `flexures::flexure_diagnose`, which fires on the **success** path) |
| `DFMRule` doc-reconcile (`rule_name`/`severity`/`applies_to` kept; doc `subject`/`process` updated) | The §8 doc matches the shipped trait; `applies_to : Process` names the process a rule targets |

These are **not in-engine seams** (no kernel module / dispatcher / realization-kind / hook), so the
`engine-integration-norm.md` §3 sub-check does not apply. The consumer is the `reify check`/`eval`
user surface — first-class user-observable. (The deferred metrology engine *does* add an in-engine
seam — new `GeometryQuery` variants + a measurement pass — which is exactly why it is split out.)

---

## §2 — Sketch of approach (the "what changes")

Two producer layers, mechanically minimal because category-trait params, trait-typed `constraint
def` params, member-access predicates, and the `eval_builtin` dispatch seam **all already work**
(verified end-to-end with the real `reify check` binary — see §3).

### 2.1 New Rust builtin module `reify-stdlib/src/dfm.rs` (the producer)

Mirrors `stackup.rs`/`tolerancing.rs`: a `pub fn eval_dfm(name, args) -> Option<Value>` added to
the `eval_builtin` dispatch chain in `lib.rs`, plus a `pub fn diagnose(...)` classifier re-exported
and called at the reify-expr builtin fallthrough.

- **`fits_build_volume(part: Solid, envelope: Solid) -> Bool`** — true iff the part's axis-aligned
  bounding box fits inside the envelope's axis-aligned bounding box (component-wise extent compare).
  Built on the **existing** `GeometryQuery::BoundingBox` (`reify-ir/src/geometry.rs:877`) — **no new
  kernel query**. Returns `Value::Undef` + a diagnostic for non-Solid args. This is the one
  geometry-backed rule that proves the engine reads real geometry, not just declared scalars.
- **`dfm::diagnose(name, args, ctx)`** — the `DFMSeverity` → diagnostic-severity bridge. On a DFM
  builtin's violation result it pushes a `Severity::{Info,Warning,Error}` diagnostic
  (`I_DFM_*`/`W_DFM_*`/`E_DFM_*`) into the `EvalContext` sink, severity taken from the rule's
  `DFMSeverity`. Fires on the **success** path like `flexure_diagnose` (the rule constructs fine but
  the design violates it). This is what makes a rule "run" and produce severity-aware output.

(The declarative *checks* themselves are `.ri` `constraint def`s — they need no Rust builtin. Rust
is needed only for the geometry-backed `fits_build_volume` and the severity classifier.)

### 2.2 `crates/reify-compiler/stdlib/process.ri` (the declarative surface)

- **Capability params** — add the 14 documented params to the 7 category traits (currently empty
  markers), keeping the module's existing *required-member* convention (no defaults, matching
  `Process { duration; cost }`):
  `Subtracting { tool_access : Solid; min_feature_size : Length; achievable_finish : Length }`,
  `Adding { layer_thickness : Length; min_feature_size : Length; build_volume : Solid }`,
  `Forming { min_bend_radius : Length; max_draw_depth : Length; draft_angle : Angle }`,
  `Joining { joint_strength : Pressure; reversible : Bool }`,
  `Parting { kerf_width : Length; min_feature_size : Length }`,
  `SurfaceTreating { coating_thickness : Length; achievable_finish : Length }`,
  `HeatTreating { treatment_temperature : Temperature; hold_duration : Time }`.
- **DFM `constraint def`s** — a universal `Manufacturable { param measured : Length; param
  capability : Length; measured >= capability }` plus per-category sugar
  (`FeatureManufacturable(proc : Subtracting, feature : Length)`, `BendManufacturable`, etc.) that
  read the process's own capability params. The build-volume rule calls `fits_build_volume`.
- **`DFMRule` reconcile** — keep `rule_name : String`, `severity : DFMSeverity`, `applies_to :
  Process` (the shipped, deliberate shape — task 4024); the doc's `subject : Structure` / `process`
  become a doc-reconcile. `applies_to` already compiles on main.

### 2.3 What stays user-supplied (the deferred boundary)

The *measured* side of every non-build-volume rule (`wall`, `feature`, `bend_radius`) is a
**design-declared param or a constraint-site scalar** — exactly the tolerancing `Conforms`
`measured_deviation` pattern. Auto-deriving these from the realized solid (min-wall medial axis,
overhang scan, draft detection) is the deferred metrology engine (§0, §5, stub PRD).

---

## §3 — Pre-conditions / substrate verification (G3 + G6)

All novel syntax was compiled with the real `reify check` binary (memory: the tree-sitter CLI can
drift from the compiler grammar; the compiler is ground truth). Fixtures live under
`/tmp/prd-gate-fixtures/process_dfm_*.ri`.

| Construct | Verdict | Evidence |
|---|---|---|
| Category trait with capability params (`Subtracting { tool_access:Solid; min_feature_size:Length; … }`) + a conforming `structure def` | **parses + compiles** | `process_dfm_2.ri` → `reify check` exit 0; params are plain trait params (precedent: `Process { duration; cost }` on main) |
| `DFMRule` conformer with `severity : DFMSeverity = DFMSeverity.Warning` and `applies_to : Process = MilledBracket()` (trait-typed param ← structure instance) | **parses + compiles** | `process_dfm_2.ri` exit 0; `applies_to : Process` already in `process.ri` on main |
| `constraint <feature> >= proc.min_feature_size` (member access on a let-bound structure instance, evaluated) | **parses + compiles + EVALUATES + REPORTS** | `process_dfm_2.ri` → `reify check` prints `VIOLATED Part#constraint[0]` for wall 0.5 mm < min 1 mm — the user-observable DFM signal |
| `constraint def Manufacturable { param measured; param capability; measured >= capability }` | **parses + compiles** | `process_dfm_3.ri` → `reify check`: `OK Manufacturable#0[0]` |
| `constraint def ProcessFeatureOk { param proc : Subtracting; … proc.min_feature_size }` (trait-typed **constraint** param + member-access predicate) | **parses + compiles** | `process_dfm_3.ri` → `OK ProcessFeatureOk#0[0]`; same form the tolerancing PRD verified for `Conforms` |
| Named-arg constraint application `constraint ProcessFeatureOk(proc: proc, feature: wall)` | **works** | `process_dfm_3.ri` exit 0 |
| Unknown builtin `fits_build_volume(...)` in a `let` (pre-impl) | **PARSES** (resolves `Undef` until α lands) | `process_dfm_3.ri` exit 0 — a `.ri` call to an unregistered fn parses; α registers it |
| New builtins wired into dispatch | **available seam** | `if let Some(v) = stackup::eval_stackup(...)` / `tolerancing::eval_tolerancing(...)` chain in `reify-stdlib/src/lib.rs`; add a `dfm::eval_dfm` arm |
| `DFMSeverity` → diagnostic severity on the **success** path | **precedent exists** | `flexures::flexure_diagnose` "runs on BOTH the success and `Undef` paths" pushing `Severity::Warning`/`Error` into the sink (`reify-stdlib/src/lib.rs` doc-comment) — `dfm::diagnose` copies the pattern |

**Substrate finding — `Geometry` is not a resolvable type; `Solid` is.** `param tool_access :
Geometry` fails (`error: unresolved type: Geometry`, verified `process_dfm_1.ri`); `Solid` resolves
(`type_resolution.rs:563`). The doc's `Subtracting.tool_access : Geometry` and `Adding.build_volume
: Solid` are therefore reconciled to **`Solid`** (a tool-access / build envelope *is* a solid). This
PRD does **not** register `Geometry` — that lift is **owned by the tolerancing PRD's task #3116**
(`tolerancing-gdt-surface-completion.md` §6, set `pending`), so re-claiming it here would contest
ownership. No dependency on #3116 is needed because `Solid` already resolves.

**Substrate finding — `= undef` is trait-only, but the module convention is required-members.**
`param x = undef` works on **trait** params (the category traits *are* traits) but the existing
`process.ri` deliberately gives `Process` no defaults so members are required. β keeps that
convention (the doc's `coating_thickness = undef` is reconciled to required — already flagged as P14
row 3 "all `= undef` defaults dropped"). Making `coating_thickness` optional via the trait-legal `=
undef` is an additive ergonomic noted in §8.

**G6 — no false numeric premise.** This PRD asserts **no closed-form numeric floor**: the DFM checks
are inequality comparisons of user-declared/queried `Length`s, and `fits_build_volume` is an
exact bounding-box extent compare (no tolerance, no iteration, no formula to mis-calibrate). The one
geometry value (`BoundingBox`) is an existing, tested kernel query. The hazard-laden numeric
measurements (medial-axis min-wall, overhang angle) are precisely what is **deferred** — keeping
this PRD off the G6 numeric-bound branch entirely.

---

## §4 — Resolved design decisions

1. **Scope = declarative DFM surface + ONE geometry-backed rule that uses existing queries.** The
   full geometry-metrology engine (auto-measure features from the realized solid) is deferred to a
   forward stub PRD with deferred tracker tasks (user-ratified scope: "stub PRD(s) with deferred
   tracking tasks for the excluded scope"). Rationale: min-wall-thickness is a research-grade medial
   axis query; overhang/draft are facet-scan kernel ops that don't exist; bundling them makes this a
   multi-PRD B+H effort. The declarative form closes all 3 P14 rows *and* the §8 doc-reconcile
   today, substrate-safe, consistent with the sibling tolerancing PRD.

2. **DFM rules are real, non-tautological `constraint def`s — not param restoration alone.** A rule
   evaluates (`measured >= capability`) and `reify check` reports `VIOLATED`; the build-volume rule
   reads real geometry. This answers the gap-register's "trait-surface-only, nothing evaluates":
   after this PRD, a DFM rule *runs* and produces user-observable output.

3. **`DFMSeverity` is wired to diagnostic severity via `dfm::diagnose`.** Without the classifier the
   `severity` param is inert decoration. With it, an `Info` rule informs, a `Warning` rule warns, an
   `Error` rule errors — the enum does real work. Mirrors `flexure_diagnose`'s success-path firing.

4. **`fits_build_volume` is a Rust builtin over the existing `BoundingBox` query**, not new kernel
   work and not `.ri` bbox-member arithmetic (the bbox value's `.ri` member shape is unverified;
   Rust is unambiguous and unit-testable, matching the `buckling.rs`/`tolerancing.rs` precedent).
   Bbox-vs-bbox (not point-in-arbitrary-solid) keeps it exact and dependency-free.

5. **`tool_access`/`build_volume` typed `Solid`, not `Geometry`** (§3 substrate finding) — avoids
   contesting the tolerancing PRD's #3116 `Geometry`-registration. Recorded as a doc-reconcile.

6. **`DFMRule` keeps the shipped `rule_name`/`severity`/`applies_to`; the doc is reconciled.** The
   documented `subject : Structure` is **not** added: generically reading a measured feature off an
   opaque `Structure` is impossible without the deferred metrology engine, so `subject` is decorative
   today. It becomes load-bearing in the metrology engine (auto-measure the subject) — recorded there.

7. **Scope = full P14 cluster (all 3 rows) + the Bucket-B §8 DFMRule doc row**, plus the forward
   stub PRD for the deferred metrology engine so the excluded scope is tracked.

---

## §5 — Out of scope

- **The geometry-metrology DFM engine** — new `GeometryQuery` variants (overhang scan, draft
  detection, min-wall via medial axis, min-feature) + an auto-extraction measurement pass. Deferred
  to `process-dfm-geometry-metrology.md`; held by deferred tracker tasks (§7).
- **`DFMRule.subject : Structure`** as a load-bearing auto-measured member (needs the metrology
  engine; decorative until then).
- **`std.io` §9** (Source/Sink/Buy/Discard/Output/Provenance, OutputFormat export) — separate cluster
  P15. The `Process.cost : Money` / `Scalar<Money>` doc-reconcile row (P15 low) is **not** touched.
- **Registering the `Geometry` type** — owned by the tolerancing PRD's #3116.
- **Real manufacturing-process simulation / cost estimation** — `Process.duration`/`cost` stay
  user-declared params; no process-physics model.

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Resolution |
|---|---|---|
| `stdlib/process.ri` category-trait params + DFM `constraint def`s | **this PRD** | — |
| `reify-stdlib/src/dfm.rs` builtins + `eval_builtin` dispatch arm | **this PRD** | Sibling of `stackup.rs`/`tolerancing.rs`; no coupling to either (§0). |
| `Geometry`/`DatumRef` resolver typing | `tolerancing-gdt-surface-completion.md` task **#3116** | This PRD does **not** touch it — uses `Solid` (already resolves). No contested ownership: resolved by *not* claiming it. |
| Geometry-metrology DFM engine (overhang/draft/min-wall queries + measurement pass) | `process-dfm-geometry-metrology.md` (forward stub) | This PRD's declarative surface is the **upstream** the metrology engine consumes (rules + severity). Deferred trackers depend on this PRD's δ. |
| `GeometryQuery::BoundingBox` (existing) | `reify-ir`/`reify-kernel-occt` | Read-only reuse by `fits_build_volume`; no change. |
| `Process.cost : Money` / `Scalar<Money>` doc-reconcile (P15) | io-export-import cluster | Not in scope; flagged for the io PRD. |

No new in-engine seam is introduced **by this PRD**, so no `engine-integration-norm.md` extension is
needed. (The deferred metrology engine *will* need one — recorded in the stub PRD.)

---

## §7 — Decomposition plan (one bullet per task → observable signal)

**In-scope spine:** **α (dfm.rs builtins) ‖ β (.ri capability params) → γ (.ri DFM constraint-defs)
→ δ (CI example + §8 doc reconcile, the user-observable leaf).** α and β are independent (different
files: `dfm.rs`/`lib.rs` vs `process.ri`) and run in parallel; γ depends on both; δ on γ. All
`.ri` work (β→γ→δ) serializes on the single file `process.ri` to respect the orchestrator's narrow
file locks. The single user-observable leaf is **δ**.

- **α — `reify-stdlib/src/dfm.rs` builtins + dispatch + severity classifier.** Implement
  `fits_build_volume(part: Solid, envelope: Solid) -> Bool` (bbox-vs-bbox via the existing
  `GeometryQuery::BoundingBox`; `Undef`+diagnostic on non-Solid) and `dfm::diagnose` (emit
  `I_/W_/E_DFM_*` per a rule's `DFMSeverity` on the violation path). Add the `dfm::eval_dfm` arm to
  the `eval_builtin` chain in `lib.rs` and re-export `diagnose` for the reify-expr fallthrough.
  **Signal:** `cargo test -p reify-stdlib` asserts `fits_build_volume` true when part bbox ⊂ envelope
  bbox and false when scaled past it; `diagnose` emits a `Warning` for a `DFMSeverity.Warning` rule
  and `Error` for `DFMSeverity.Error`; `grep` shows the new `dfm::eval_dfm` arm in `lib.rs` dispatch
  (anti-orphan). *Deps: none.*

- **β — `process.ri` capability params on the 7 category traits.** Add the 14 documented params
  (§2.2), required-member convention, `tool_access`/`build_volume : Solid`.
  **Signal:** a `reify check` conformance test: `structure def MilledBracket : Subtracting { … all
  params … }` compiles and `reify eval` reads `MilledBracket().min_feature_size` = the value; a
  conformer **omitting** a required param fails conformance (`required_members` diagnostic). *Deps:
  none.*

- **γ — `process.ri` DFM constraint-defs (the declarative evaluation surface).** Add `Manufacturable`
  + the per-category DFM `constraint def`s (feature ≥ min_feature_size; bend_radius ≥ min_bend_radius;
  draw_depth ≤ max_draw_depth; draft ≥ draft_angle; build-volume via `fits_build_volume`); reconcile
  `DFMRule` (keep `rule_name`/`severity`/`applies_to`).
  **Signal:** `reify check` on a part with an under-spec feature reports the DFM constraint
  `VIOLATED`; the build-volume rule (calling α's `fits_build_volume`) flips `OK`→`VIOLATED` when the
  part is scaled past the envelope; a `DFMSeverity`-tagged rule emits the matching `W_/E_DFM_*`
  diagnostic via `dfm::diagnose`. *Deps: α (`fits_build_volume` + `diagnose`), β (capability params).*

- **δ — end-to-end CI example + §8 doc reconcile (user-observable leaf, B integration gate).** Commit
  `examples/process/std_process_dfm.ri` exercising: a process per category with capability params,
  `DFMRule` instances at each `DFMSeverity`, the `Manufacturable` + per-category constraints, and the
  geometry-backed build-volume-fit that flips when the part exceeds the envelope — run green in CI via
  `reify check`. Reconcile `docs/reify-stdlib-reference.md` §8 (`DFMRule` `subject`/`process` →
  `rule_name`/`severity`/`applies_to`; `tool_access : Geometry` → `Solid`; `= undef` defaults →
  required; document the declarative-engine scope + a pointer to the deferred metrology engine). Mark
  the 3 P14 rows + the Bucket-B §8 DFMRule row closed.
  **Signal:** `reify check examples/process/std_process_dfm.ri` (CI) shows the expected `OK`/`VIOLATED`
  set including the build-volume flip and the severity-tagged DFM diagnostics; the gap-register P14
  rows show closed. *Deps: γ (α, β transitively).*

**Deferred trackers (filed `deferred`, NOT activated — hold the excluded scope; see
`process-dfm-geometry-metrology.md`):**

- **MET-1 — Kernel geometry-metrology queries for DFM** (overhang-angle scan vs build direction,
  draft detection vs pull direction, min-wall-thickness via medial axis, min-feature-size). Deferred;
  B+H; depends on this PRD's δ (consumes the rule/severity surface).
- **MET-2 — Auto-measured DFM evaluation pass** (realize subject solid → measure features → auto-run
  applicable rules from `applies_to` → emit per-violation diagnostics; activates
  `DFMRule.subject : Structure`). Deferred; depends on MET-1 + this PRD's δ.

---

## §8 — Open (tactical / implementation-time) questions

1. **Per-category constraint-def granularity (γ):** one universal `Manufacturable(measured,
   capability)` only, or also per-category sugar (`FeatureManufacturable`, `BendManufacturable`,
   `FitsBuildVolume`)? The universal form covers every scalar ≥/≤ check; the geometry-backed
   build-volume rule needs its own def. Tactical — decide at γ; the example (δ) drives which read best.
2. **`coating_thickness = undef` (β):** the category traits are *traits*, so `= undef` is grammar-legal
   (unlike structures). Honor the doc's optional default, or keep the module's required-member
   convention? Current plan: required (consistency + the reconcile is already a known P14 row). Making
   it optional is additive.
3. **`fits_build_volume` orientation (α):** v1 compares axis-aligned bboxes (printer envelopes are
   AABB in practice). A future enhancement could allow an arbitrary build-orientation transform before
   the compare — out of scope, noted.
4. **`DFMSeverity.Info` diagnostic channel (α):** `reify check` surfaces `Warning`/`Error` clearly;
   confirm an `Info`-level diagnostic is rendered (or downgrade `Info` rules to a no-op pass). Tactical
   at α — the test asserts whichever the diagnostic sink supports.
5. **`applies_to` multiplicity:** a rule targets one `Process`. If a design needs the same rule across
   several processes, the designer writes one `DFMRule` per process. A `List<Process>` `applies_to` is
   a possible ergonomic — out of scope, noted.
