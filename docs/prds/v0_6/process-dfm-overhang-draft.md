# PRD: `std.process` geometry-metrology — auto-measured overhang + draft DFM

**Status:** Ready to decompose · **Author session:** 2026-06-08 · **Milestone:** v0_6
**Approach:** B (vertical slice) + H (contract + two-way boundary tests) per `preferences_implementation_chain_portfolio`.
**Splits from:** `docs/prds/v0_6/process-dfm-geometry-metrology.md` (the deferred geometry-metrology stub), which is superseded by this PRD (the ship-now half) + `process-dfm-thickness-metrology.md` (the deferred research half).
**Consumes (upstream, shipped):** `docs/prds/v0_6/process-dfm-completion.md` — the declarative DFM surface (`Manufacturable`, `DFMRule`, capability params, `dfm::diagnose`, `DFMSeverity`, `fits_build_volume`).

---

## §0 — Supersession + scope split

The deferred stub `process-dfm-geometry-metrology.md` held **all four** auto-measurement queries
(overhang, draft, min-wall, min-feature) as one B+H follow-up to `process-dfm-completion`. A
2026-06-08 feasibility pass (three-agent read-only sweep) found the stub **overstated the substrate
gap** and that the four queries cleave on a sharp tier boundary:

| Tier | Queries | Substrate reality | Home |
|---|---|---|---|
| **Ship-now** | **OverhangFaces, DraftAngle** | Every primitive exists and is exercised today — `FaceNormal` query, `extract_faces`, the `faces_by_normal` dot-product-threshold selector, `tessellate()` with outward facet normals. Zero new kernel/FFI work. | **THIS PRD** |
| **Research-gated** | MinWallThickness, MinFeatureSize | The thickness primitive (`reify-shell-extract::bidirectional_distances`) exists, but an **eval-reachable solid→SDF wire does not** (the kernel primitive #3095 is done + on main, but stranded from the eval production path — a C-17-shape orphan); plus a non-convex sign-monotonicity correctness boundary and a `not(has_openvdb)` degradation contract. | `process-dfm-thickness-metrology.md` (deferred) |

This PRD ships the ship-now tier as a complete end-to-end slice: a designer annotates a part with a
`DFMRule` and `reify check` **auto-measures** the realized solid's overhangs / draft and flags
manufacturability violations — **with no hand-declared measured feature**. It activates
`DFMRule.subject` (decorative in `process-dfm-completion`) as load-bearing.

---

## §1 — Consumer (G1)

**Named consumer: a DFM-minded designer who wants `reify check` to flag manufacturability
violations WITHOUT manually declaring every measured feature.** The user annotates a part with a
`DFMRule` (`rule_name`, `severity`, `applies_to : Process`, and now `subject : Solid`); the engine
realizes the subject solid, measures its overhangs / draft against the process capability, and emits
a `DFMSeverity`-tagged diagnostic. The user-observable surface is the CLI: `reify check` emits
`W_DFM_OVERHANG` / `E_DFM_DRAFT` (etc.) for a part that violates, and nothing for one that conforms.

| Mechanism | Consumer |
|---|---|
| `unsupported_overhang_faces` / `min_draft_angle` eval-level selectors (new, `reify-eval`) | The auto-measurement pass calls them on the realized subject solid; `reify check` reflects the verdict |
| Auto-measurement check-time pass (new, `reify-eval`) | A designer's `DFMRule` annotation — `reify check` auto-runs the matching measurement per process category, no per-feature constraint instantiation |
| `Adding.max_overhang_angle : Angle` capability param (new, `process.ri`) | The overhang pass reads `applies_to.max_overhang_angle` to decide which faces are unsupported |
| `DFMRule.subject : Solid` activation (new, `process.ri`) | The pass realizes `subject` to a solid handle and measures it — the member becomes load-bearing |
| `dfm::diagnose` overhang/draft extension (new arms, `dfm.rs`) | The pass routes each measurement result + the rule's `DFMSeverity` through `diagnose`, emitting `{I,W,E}_DFM_OVERHANG` / `_DRAFT` |

**Engine-integration-norm §3 seam (per `.claude/skills/prd/project.md` G1 sub-check).** The
measurement *selectors* ride the **existing** kernel-query path (§3.1 op-execute — `FaceNormal` /
`tessellate` against the realized handle, exactly as `fits_build_volume` rides `BoundingBox`); no
norm change for them. The **auto-measurement pass is a NEW consumer** — a check-time walk over
`DFMRule` instances in `reify-eval`, structurally a sibling of the `RepresentationWithin`
interception (`engine_constraints.rs:30-90, 654-685`). It does not fit §3.1–§3.7, so this PRD adds a
**new §3 entry** to `engine-integration-norm.md` per that norm's governance (§3 line 151 / §13 Q2
option a: the introducing PRD adds the entry in its own commit). Named call site:
`Engine::check` → a new `measure_dfm_rules` pass in `reify-eval`, invoked alongside
`check_constraints_against_templates` (`engine_constraints.rs:665-685`). The pass realizes the
subject handle from the engine's realized state (same access `engine_build.rs:756/798` already use
to query `BoundingBox` internally) and degrades to `Indeterminate` / no-op when no kernel is present
(the C1 invariant the `RepresentationWithin` path proves).

---

## §2 — Sketch of approach (the "what changes")

The slice is mechanically small because the measurement compute, the severity bridge, the
declarative DFM surface, and the kernel-backed check-time evaluation pattern **all already exist**.

### 2.1 Overhang + draft selectors (`reify-eval/src/topology_selectors.rs`, the producer)

Siblings of `faces_by_normal` (`topology_selectors.rs:583`), reusing its
`extract_faces` → per-face `FaceNormal` → `filter_by_value` / `normalize3` / `dot3` machinery
verbatim. No new kernel query, no `reify-ir`/OCCT change.

- **`unsupported_overhang_faces(handle, build_dir, max_overhang_angle) -> (Vec<face>, worst_dip)`**
  — for a closed solid and unit build direction `b` (default +Z), a face with outward unit normal
  `n` is an **unsupported overhang** iff `n · b < −sin(max_overhang_angle)` (i.e. its underside dips
  more than `max_overhang_angle` below the horizontal build plane). The measured **dip angle** of a
  face is `asin(−n · b) ∈ [0°, 90°]`; the rule's measured value is the worst (max) dip across faces.
- **`min_draft_angle(handle, pull_dir) -> (signed_min_draft, has_undercut)`** — for unit pull
  direction `p` (default +Z), a wall-like face (`|n · p|` below the wall window) has signed draft
  `δ = 90° − angle(n, p)`; the measured value is the minimum δ over wall faces. A face whose normal
  opposes the pull on the near side (`n · p < 0` within the wall window) is flagged an **undercut**
  (negative draft — the part locks in the mold).

Planar/conical faces are **exact** (single BRep-face normal). Curved faces are sampled **per-facet**
via `tessellate()` (which emits outward per-vertex normals), and the **worst** facet is reported —
a conservative bound, never an optimistic one (§3 G6). v1 ships the per-BRep-face path; the example
(§8 ζ) uses planar fixtures.

### 2.2 Auto-measurement check-time pass (`reify-eval`, the new consumer)

A new `Engine::measure_dfm_rules` pass, invoked from `Engine::check` (`engine_constraints.rs:665`),
modeled on the `RepresentationWithin` interception:

1. Enumerate the module's `DFMRule` structure-instances (duck-typed on the `rule_name`/`severity`/
   `applies_to`/`subject` fields, same way `dfm.rs::parse_dfm_severity` duck-types on `severity`).
2. For each rule, realize `subject : Solid` to its kernel handle from the engine's realized state.
3. Determine the process category from `applies_to`'s concrete conformer type (`Adding` →
   overhang; `Forming` → draft); read the capability param (`max_overhang_angle` / `draft_angle`)
   and the direction (default +Z).
4. Run the matching selector, compare the measured value vs the capability, and route the result +
   the rule's `DFMSeverity` through `dfm::diagnose` (§2.3).
5. **No kernel → no realized handle → `Indeterminate` / no-op** (never a false `Violated`), exactly
   as the `RepresentationWithin` empty-`achieved_repr_tol` path (C1).

### 2.3 `dfm::diagnose` overhang/draft extension (`reify-stdlib/src/dfm.rs`)

Extend the existing `diagnose` (`dfm.rs:249`) with arms for the two new measurements (or add sibling
classifiers reusing `parse_dfm_severity`): on a violation, push one `{I,W,E}_DFM_OVERHANG` /
`{I,W,E}_DFM_DRAFT` diagnostic at the rule's declared severity (default `Warning`), mirroring
`build_volume_violation`. Undercut faces get a dedicated `E_DFM_UNDERCUT` (an undercut is always an
error — the part cannot release). A conforming part surfaces nothing.

### 2.4 `process.ri` declarative surface (`crates/reify-compiler/stdlib/process.ri`)

- Add `Adding.max_overhang_angle : Angle` to the `Adding` trait (required-member convention,
  precedent: `Forming.draft_angle : Angle`). `Forming.draft_angle` already exists — no draft
  capability param is added.
- Add `subject : Solid` to the `DFMRule` trait (precedent: `Subtracting.tool_access : Solid`).
  Activates the stub's deferred member; `Solid` resolves to `Type::Geometry`
  (`type_resolution.rs:563`).

---

## §3 — Pre-conditions / substrate verification (G3 + G6)

### G3 — substrate (verified during the 2026-06-08 feasibility sweep, file:line cited)

| Construct / capability | Verdict | Evidence |
|---|---|---|
| Overhang/draft compute (face-normal vs direction, dot-product-threshold) | **EXISTS as reusable machinery** | `faces_by_normal` (`topology_selectors.rs:583`) does exactly this — `extract_faces` → `FaceNormal` query → `normalize3`/`dot3`/`acos` → threshold; `edges_parallel_to` (`:647`) shows the `\|dot\| >= cos(tol)` variant |
| `FaceNormal` per-BRep-face query | **EXISTS** | `GeometryQuery::FaceNormal` (`reify-ir/src/geometry.rs:1068`), orientation-aware outward normal; OCCT arm `reify-kernel-occt/src/lib.rs:2972` → `query_face_normal` (`ffi.rs:710`) |
| Curved-face facet sampling (outward normals) | **REACHABLE** | `GeometryKernel::tessellate` (`geometry.rs:2400`; OCCT `lib.rs:3158`) returns `Mesh` (`geometry.rs:1561`) with `normals: Option<Vec<f32>>`, outward-oriented per `occt_wrapper.cpp:4470-4514` |
| Direction arg resolution (`Value::Vector` of 3 dimensionless) | **EXISTS** | `resolve_vec3_arg` (`geometry_ops.rs:4038`); angle via `resolve_angle_scalar_arg` (`geometry_ops.rs:4066`, ANGLE-dimensioned). Used by the topology selectors today |
| Kernel-backed check-time evaluation pass with no-kernel degradation | **EXISTS as template** | `RepresentationWithin` interception in `dispatch_constraints` (`engine_constraints.rs:30-90`) + `Engine::check` ordering/degradation invariant (`engine_constraints.rs:654-685`): empty realized data → `Indeterminate`, never false `Violated` (C1) |
| Realize a subject solid handle at check time | **EXISTS** | `tessellate_realizations` populates realized data before `check()`; internal `BoundingBox` queries on realized handles at `engine_build.rs:756/798` |
| Declarative DFM surface (`DFMRule`/`Manufacturable`/`dfm::diagnose`/`DFMSeverity`/`fits_build_volume`/`eval_dfm`) | **SHIPPED** | `process.ri:28,117-193`; `dfm.rs:30,249`; `eval_dfm` arm at `reify-stdlib/src/lib.rs:215`. `fits_build_volume`→`BoundingBox`→OCCT trace is the geometry-backed template |
| `param max_overhang_angle : Angle` (trait param) | **parses + resolves** | precedent `Forming.draft_angle : Angle` (`process.ri:76`) — `Angle` is a registered `NAMED_DIMENSIONS` type; no novel grammar |
| `param subject : Solid` (trait param) | **parses + resolves** | precedent `Subtracting.tool_access : Solid` (`process.ri:54`); `"Solid" => Type::Geometry` (`type_resolution.rs:563`); no novel grammar |

**Grammar gate (`references/grammar-gate.md`):** PRD A introduces **no novel syntax** — both new
params reuse param-type forms proven on main (`Angle`, `Solid`). `grammar_confirmed = true` for the
`.ri` work; the capability manifest cites the existing-precedent fixtures rather than queueing a
grammar producer.

**`subject : Structure` correction (the brief's flagged hazard, verified).** `"Structure"` is
**not** a registered surface type — it is a purpose-only wildcard sentinel
(`WILDCARD_STRUCTURE_KIND`, `expr.rs:551`), carried as a bare string outside `resolve_type_name`
(`type_resolution.rs:560-589`, which has `"Solid"`/`"Geometry"` arms but no `"Structure"` arm).
A metrology query consumes a geometry handle, so `subject` is typed **`Solid`**, not `Structure`.
No `"Structure"` resolver arm is added (it would be the wrong tool and is unowned by #3116).

**#3116 (tolerancing `Geometry`-registration) — SPURIOUS dependency, NOT wired.** The
`"Geometry" => Type::Geometry` arm #3116 owns already landed (`type_resolution.rs:564`); `Solid`
already resolves; the `Structure` arm this PRD declines is outside #3116's scope. No dependency.

### G6 — numeric premise (domain = numerical, per overlay)

This PRD asserts **no closed-form accuracy floor and freezes no guessed numeric bound** (the
esc-3453/esc-3770 failure mode is structurally avoided):

- **The measurements are angle comparisons, not iterative estimates.** Overhang dip
  `asin(−n·b)` and draft `90° − angle(n, p)` are **exact** for planar/conical faces (a single exact
  BRep-face normal and an exact dot product). There is no tolerance, iteration, or formula to
  mis-calibrate.
- **Curved faces are a sampled bound, reported conservatively.** Per-facet `tessellate()` sampling
  inherits the tessellation chord tolerance; the **worst** (most-overhanging / least-drafted) facet
  is reported, so the verdict is conservative — a part flagged as violating is never a false alarm
  hidden by under-sampling. The RED tests assert **inequalities on planar fixtures** (a 30° wedge's
  measured dip equals 30° to within the face-normal precision; a 60°-from-horizontal face is *not*
  flagged at `max_overhang_angle = 45°`), never an exact float on a curved surface.
- **Definition-ambiguity (the brief's MinFeatureSize-style hazard, here for overhang/draft) is
  pinned to ONE crisp, measurable convention** (§2.1): overhang dip is measured from the horizontal
  build plane via `n·b < −sin(max_angle)`; draft is the signed `90° − angle(n, p)` over wall-window
  faces with undercut = the `n·p < 0` sign condition. The exact wall-window half-angle is a contract
  constant fixed with a fixture (§7 / §9 Q1).

---

## §4 — Resolved design decisions

1. **The deliverable is the auto-measurement pass (the brief's MET-2), not just more geometry-backed
   builtins.** A designer annotates a part with a `DFMRule`; the engine auto-measures — this is what
   makes `DFMRule.subject` load-bearing and is the differentiated value over the already-shipped
   declarative surface. The pass is a check-time walk modeled on `RepresentationWithin`.

2. **`subject : Solid`, not `Structure`** (§3). A metrology query needs a geometry handle; `Solid`
   resolves today, `Structure` does not exist as a surface type. Corrects the stub's `subject :
   Structure` framing.

3. **Overhang/draft are eval-level selectors reusing `faces_by_normal`, not new `GeometryQuery`
   variants.** The compute is identical to an existing selector and the consuming pass lives in
   `reify-eval`; a kernel `GeometryQuery` variant would touch `reify-ir` + OCCT + two exhaustive
   matches for no benefit. Corrects the stub's "new GeometryQuery variants" framing (the same
   over-statement as its substrate gap). Promoting a selector to a user-facing `.ri` query builtin
   is a noted follow-up (§9), not in scope.

4. **Build/pull direction defaults to part-local +Z; explicit override is a tactical follow-up.**
   The build/pull axis is a setup choice external to the geometry and must be declared or defaulted;
   +Z is the conventional AM/molding axis. An explicit per-rule override needs a `Vector`-typed
   `.ri` param whose grammar is unverified — deferred (§9 Q2) to keep PRD A grammar-clean. The
   default is a **convention** (the supplying user must orient the part +Z-up), not a numeric
   premise — no accuracy floor. The diagnostic states the assumed axis so a mis-oriented part is
   diagnosable.

5. **Reuse `dfm::diagnose` / `DFMSeverity` / `Manufacturable` / `DFMRule` from
   `process-dfm-completion`; do not re-author the severity bridge.** This PRD only *extends*
   `diagnose` with overhang/draft/undercut arms.

6. **The pass measures overhang (`Adding`) + draft (`Forming`).** Build-volume stays the shipped
   user-instantiated `FitsBuildVolume` constraint-def; folding it into the auto-pass is a noted
   follow-up (§9 Q3), not in scope.

7. **The new check-time-walk seam is recorded as a new `engine-integration-norm.md` §3 entry in this
   PRD's commit** (per that norm's governance). The selectors ride the existing §3.1 query path and
   need no norm change.

---

## §5 — Out of scope

- **MinWallThickness / MinFeatureSize** + the eval-reachable solid→SDF wire — deferred to
  `process-dfm-thickness-metrology.md` (research-gated; owns the SDF wire as a G4 seam over the done
  #3095 kernel primitive).
- **Curved-overhang facet sampling beyond the conservative-worst-facet v1** (e.g. per-region
  overhang maps) — the v1 contract is per-BRep-face exact + per-facet conservative bound.
- **Explicit per-rule build/pull direction override** (needs a `Vector` `.ri` param — G3-unverified;
  §9 Q2).
- **A user-facing `.ri` overhang/draft *query builtin*** (the auto-pass is the surface; §9 Q4).
- **Auto-running build-volume in the pass** — stays the shipped `FitsBuildVolume` constraint-def
  (§9 Q3).
- **Registering a `"Structure"` surface type** — declined (§3); `subject : Solid`.
- **Registering `Geometry`** — owned by tolerancing #3116; already landed; not touched.

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Resolution |
|---|---|---|
| Declarative DFM surface (`Manufacturable`, `DFMRule`, `dfm::diagnose`, `DFMSeverity`, capability params, `fits_build_volume`) | `process-dfm-completion.md` (**upstream, shipped**) | This PRD consumes + extends (`diagnose` arms, `Adding.max_overhang_angle`, `DFMRule.subject`). Additive — `process-dfm-completion` is done; no contested ownership. |
| Overhang/draft selectors + auto-measurement check-time pass | **THIS PRD** | New `reify-eval` producer + consumer. |
| New `engine-integration-norm.md` §3 entry (check-time DFM measurement walk) | **THIS PRD** | Added in this PRD's commit per norm §3 governance. Selectors ride existing §3.1 (no norm change). |
| `FaceNormal` / `tessellate` kernel queries (existing) | `reify-ir` / `reify-kernel-occt` | Read-only reuse; no change. |
| MinWall/MinFeature + solid→SDF wire | `process-dfm-thickness-metrology.md` (deferred) | This PRD's overhang/draft is **independent** of the SDF tier — no shared substrate, no dependency. |
| `Geometry` type registration | tolerancing `#3116` (done) | Not touched; `Solid` resolves. Dependency SPURIOUS (§3). |
| `DFMRule.subject` activation | **THIS PRD** | Activates the member `process-dfm-completion` §4 decision 6 left decorative; recorded there as becoming load-bearing here. |

---

## §7 — Contract (B + H)

### §7.1 — Measurement contracts

**`unsupported_overhang_faces(handle, build_dir, max_overhang_angle)`**
- Input: a realized closed-solid handle; unit `build_dir` (default `[0,0,1]`); `max_overhang_angle :
  Angle ∈ [0°, 90°]`.
- Output: the set of faces with outward normal `n` s.t. `n · build_dir < −sin(max_overhang_angle)`,
  plus `worst_dip = max over faces of asin(−n·build_dir)`.
- Errors: zero/non-finite `build_dir` or out-of-range angle → `QueryError::QueryFailed` (mirrors
  `faces_by_normal`'s `validate_angular_tol`).
- Exactness: planar/conical faces exact; curved faces per-facet via `tessellate`, worst facet
  reported (conservative bound).

**`min_draft_angle(handle, pull_dir)`**
- Input: realized closed-solid handle; unit `pull_dir` (default `[0,0,1]`).
- Output: `signed_min_draft = min over wall-window faces of (90° − angle(n, pull_dir))`;
  `has_undercut = ∃ wall-window face with n · pull_dir < 0`.
- Wall window: faces with `|n · pull_dir|` below `sin(WALL_WINDOW)` (a fixed contract constant,
  default 45°; §9 Q1).
- Exactness: as above.

**Auto-measurement pass**
- Pre: kernel-backed engine, `subject` realized. Post: ≤1 diagnostic per rule at the rule's
  `DFMSeverity` (overhang/draft violation); `E_DFM_UNDERCUT` for any undercut. No kernel / no
  realized subject → `Indeterminate`, **never `Violated`** (C1). Conforming part → nothing.

### §7.2 — Two-way boundary tests (H)

**Producer side (does the measurement match geometry?):**
| Scenario | Precondition | Postcondition |
|---|---|---|
| Planar overhang exact | A wedge with a known 30°-from-horizontal underside | `worst_dip == 30°` to face-normal precision; face is in the unsupported set iff `max_overhang_angle < 30°` |
| Self-supporting not flagged | A 60°-from-horizontal face, `max_overhang_angle = 45°` | face **not** in the unsupported set |
| Draft sign + undercut | A box with one tapered wall (+5°) and one re-entrant wall (−3°) | `signed_min_draft == −3°`; `has_undercut == true` |
| Curved conservative bound | A cylinder/fillet whose worst facet dips θ | reported dip `≥ θ_true − chord_tol`; never optimistic |

**Consumer side (does the pass wire to the user surface?):**
| Scenario | Precondition | Postcondition |
|---|---|---|
| Auto-flag, no hand-declared feature | `.ri` with a part + `DFMRule{severity: Warning, applies_to: printer, subject: bracket}`, bracket has an unsupported overhang | `reify check` emits exactly one `W_DFM_OVERHANG`; the `.ri` declares **no** overhang angle |
| Severity honored | same with `severity: Error` | diagnostic is `E_DFM_OVERHANG` |
| No-kernel degradation (C1) | the lightweight (no-OCCT) `reify check` path | overhang/draft rules → `Indeterminate`, exit 0, no `Violated` |
| Conformer | a part with all overhangs ≤ limit | no DFM diagnostic |
| Anti-orphan | — | `rg` shows `measure_dfm_rules` invoked from `Engine::check` and the new `diagnose` arms reached from the pass |

---

## §8 — Decomposition plan (one leaf per task → observable signal)

**Spine:** **α (selectors) ‖ β (.ri surface) → γ (auto-measurement pass) ‖ δ (diagnose arms) → ζ
(e2e example, the leaf)**; **ε (norm §3 entry)** rides on γ. α/β are independent (different crates).
γ depends on α + β. δ depends on α (needs the result types). ζ (the single user-observable leaf /
B integration gate) depends on γ + δ. The `reify-eval` work (α in `topology_selectors.rs`, γ in
`engine_constraints.rs`) touches different files — narrow-lock-safe.

- **α — overhang + draft eval-level selectors** (`reify-eval/src/topology_selectors.rs`).
  `unsupported_overhang_faces` + `min_draft_angle` reusing the `faces_by_normal` machinery; `Value`
  arg resolution via `resolve_vec3_arg`/`resolve_angle_scalar_arg`.
  **Signal:** `cargo test -p reify-eval` — a 30° wedge returns the expected dip + face set; a 60°
  face is not flagged at 45°; a re-entrant wall sets `has_undercut`; `rg` shows the new fns.
  *Deps: none. grammar_confirmed: n/a (Rust).*

- **β — `process.ri` surface** (`crates/reify-compiler/stdlib/process.ri`). Add
  `Adding.max_overhang_angle : Angle` and `DFMRule.subject : Solid`.
  **Signal:** a `reify check` conformance test — a conformer supplying both compiles; one omitting
  `max_overhang_angle` fails with a `required_members` diagnostic.
  *Deps: none. grammar_confirmed: true (Angle/Solid precedents).*

- **γ — auto-measurement check-time pass** (`reify-eval`, `Engine::measure_dfm_rules` invoked from
  `Engine::check`). Enumerate `DFMRule` instances, realize `subject`, match category → selector,
  compare vs capability, route through `dfm::diagnose`; degrade to `Indeterminate` with no kernel.
  **Signal:** `reify check` on a part with a `DFMRule` + unsupported overhang emits `W_DFM_OVERHANG`
  with no hand-declared overhang; the no-OCCT path returns `Indeterminate` (no false `Violated`).
  *Deps: α, β.*

- **δ — `dfm::diagnose` overhang/draft/undercut arms** (`reify-stdlib/src/dfm.rs`). Extend
  `diagnose` with `{I,W,E}_DFM_OVERHANG` / `_DRAFT` (severity from the rule) + `E_DFM_UNDERCUT`.
  **Signal:** `cargo test -p reify-stdlib` — an overhang violation with `DFMSeverity.Error` →
  one `E_DFM_OVERHANG`; an undercut → `E_DFM_UNDERCUT`; a conformer → empty.
  *Deps: α (result types).*

- **ε — `engine-integration-norm.md` §3 entry** for the check-time DFM measurement walk; cite γ's
  call site; note the selectors ride §3.1.
  **Signal:** `git log -- docs/prds/v0_3/engine-integration-norm.md` returns the commit;
  `rg "DFM measurement" docs/prds/v0_3/engine-integration-norm.md` returns the new §3 entry.
  *Deps: γ (cites its call site).*

- **ζ — end-to-end CI example + doc reconcile (user-observable leaf, B integration gate).** Commit
  `examples/process/std_process_dfm_metrology.ri`: an `Adding` printer with `max_overhang_angle`, a
  `Forming` process with `draft_angle`, parts with an unsupported overhang / insufficient draft /
  undercut, `DFMRule`s at each `DFMSeverity` with `subject`. Reconcile
  `docs/reify-stdlib-reference.md` §8 (document the auto-measurement engine; `subject : Solid`).
  **Signal:** `reify check examples/process/std_process_dfm_metrology.ri` (CI) shows the expected
  auto-emitted `W_/E_DFM_OVERHANG` / `_DRAFT` / `_UNDERCUT` set, with no hand-declared measured
  feature; CI green.
  *Deps: γ, δ.*

**Tracker disposition (at decompose):** the existing deferred trackers **4276 (MET-1)** and **4277
(MET-2)** are superseded — the overhang/draft slice of MET-1 + the pass of MET-2 land here (α–ζ);
the min-wall/min-feature slice of MET-1 moves to `process-dfm-thickness-metrology.md`. Per the
subtask-deprecation norm, the batch is filed as **top-level** tasks (no subtasks).

---

## §9 — Open (tactical / implementation-time) questions

1. **Wall-window half-angle for draft (α).** Which faces count as "walls" for `min_draft_angle`
   (the `|n·pull|` window)? Default 45°; pin the exact constant with the wedge/box fixture. Tactical.
2. **Explicit per-rule direction override (β).** v1 defaults +Z. Adding `build_dir`/`pull_dir` to
   the rule needs a `Vector`-typed `.ri` param — grammar-verify before committing to it; until then
   the part must be modeled +Z-up. Follow-up.
3. **Fold build-volume into the auto-pass (γ).** The pass could also auto-run the build-volume rule
   (it has `Adding.build_volume`), retiring the user-instantiated `FitsBuildVolume`. Out of scope;
   noted.
4. **User-facing `.ri` overhang/draft query builtin (α).** Promoting a selector to a `fits_build_volume`-style
   `.ri` builtin (for explicit constraints) is additive; the auto-pass is the v1 surface.
5. **`DFMSeverity.Info` rendering (δ).** Confirm `reify check` renders an `Info`-level DFM
   diagnostic (or downgrade `Info` rules to a no-op pass) — mirrors `process-dfm-completion` §8 Q4.
6. **`applies_to` multiplicity.** A rule targets one `Process`; per-process rule duplication is the
   v1 idiom (same as `process-dfm-completion` §8 Q5).
