# Capability manifest — stdlib-surface-type-substrate.md (2026-06-12)

Per-leaf capability→evidence bindings (G3+G6 mechanized). All bindings verified against the working tree 2026-06-12 (post-task-4548 Phase-A: `Impulse`/`Momentum` registered, `ImpulseForce.impulse`→`Impulse`, `Mode.frequency`→`Frequency`). **No FAIL bindings.** Leaf labels match PRD §10. **Filed task IDs (4548 step-8):** F=#4574, V=#4575, R=#4576, P=#4577, Pt=#4578, M=#4579, S=#4580. **Status at 4548 closeout (2026-06-13, step-16):** F/R/Pt/M/S = `done` (landed on main; markers resolved/deleted upstream); V/P = `blocked` (non-terminal/LIVE). V/R/P/M depend on F=#4574. _The per-leaf evidence table below is the authoring-time (2026-06-12) snapshot; the `done`-sibling grep evidences (R/Pt/M/S) have since been resolved upstream — see the closeout section for current disposition._

| Leaf | Capability asserted by signal | Evidence | Verdict |
|---|---|---|---|
| F | linear stdlib loader is the blocker (position-enforced order) | `stdlib_loader.rs::load_stdlib` `for (module_name, source) in &sources` growing-prelude loop; `std.kinematic` ordering comment "Depends on std.trajectory (Vec3 and JointValue aliases)"; `std.fea.multi_case` listed before `std.trajectory` | PASS wired |
| F | user-project DAG loader already exists (the narrower-than-from-scratch claim) | `crates/reify-compiler/src/module_dag.rs` present (DFS + cycle detection + post-order topo sort + embedded-stdlib fallback) | PASS wired |
| F | forward-reference pre-pass precedent exists (approach 2) | same-module skeleton pre-pass (task 3895) makes a `structure_def` visible to an accessor fn body in the same module (cited in stdlib_loader.rs `std.flexures` comment) | PASS wired |
| F | `import` grammar available for the ModuleDag-migration branch (approach 1) | `module-and-visibility-hardening.md` §8 task α (`module`/`import` grammar) — consumed read-only via G4 seam | PASS producer-upstream |
| F | foundation is unowned (no sibling owns stdlib load-order) | `module-and-visibility-hardening.md` §5 out-of-scope is silent on prelude load-order / forward-refs; owns only module-PATH semantics (§7.1, §8 α/γ) | PASS wired |
| V | Vec3 placeholder corpus exists to tighten | grep: `pub type Vec3 = Real` (trajectory.ri:96); `param axis : Vec3` ×5 (kinematic.ri:99/:113/:122/:129/:130); TODO(vec3-type) (fea_multi_case.ri ×2, trajectory.ri, fdm.ri) | PASS wired |
| V | owning PRD homes exist for the vector type | `affine-map-type.md` §4.2 (`Vector3<Length>`/`vec3()`); `math-linalg-n-generality-and-signatures.md` §2/§3 (`vec`/`vec2`/`vec3`); `kinematic-inter-joint-offsets.md` §3/§7.1 (`point3`/`vec3`) | PASS producer-upstream |
| V | `Vector3<Q>` parametric resolver arm present | audit note §"Resolver capability reference": `Vector3<Q>`, `Point3<Q>` in `type_resolution.rs` parametric arms | PASS wired |
| V | alias-collapse hazard is real (negative boundary test motivation) | trajectory.ri:50-62 η-phase note: "compiler will silently accept a `Pose3` value where a `LocationId` index is expected, or a `Vec3` … where a `Pose3` … is required" | PASS wired |
| R | Range placeholder corpus exists | grep: TODO(range-type) (kinematic.ri SweepDim.range ×2); TODO(range-angle-type) (flexures.ri ×2) | PASS wired |
| R | owning PRD owns `Range<T>` + methods | `numeric-and-range-literal-forms.md` §1/§2 (`Range<T>`, `.contains`/`.lower`/`.upper`); `tolerancing-gdt-surface-completion.md` §4 decision 6 scopes `Range<Length>` OUT to it | PASS producer-upstream |
| P | Pose3/LocationId placeholders exist | grep: `pub type Pose3 = Real` (trajectory.ri:87), `pub type LocationId = Real` (trajectory.ri:106); FIXME(location-id-type) ×4 (modal_analysis.ri at-params) | PASS wired |
| P | Selector substrate LANDED (LocationId rides it, not a new alias) | task 4116 (DONE): `Value::Selector`/`Type::Selector`/`SelectorKind{Face,Edge,Body}`; modal_analysis.ri:441-442 "the future `LocationId` topology-selector type" | PASS producer-upstream |
| P | Pose3 is genuinely unowned but adjacent to Transform3 | `affine-map-type.md` §4.4 owns `Transform3 { rotation: Orientation, translation: Vector3<Length> }`, does NOT own `Pose3` → unowned, designed adjacent | PASS wired |
| Pt | Part placeholder corpus exists (`String`, not `Real`) | grep: FIXME(part-structdef) ×3 (modal_analysis.ri ModalResult.part:216 / ForcingTimeHistory.part:682 / TransientResponse.part:754); `String` placeholder per modal_analysis.ri header §1.2 | PASS wired |
| Pt | producer echo path to migrate exists | modal_analysis.ri:758 "trampoline echoes it from the modal result (currently the `""` placeholder)"; `modal_ops.rs` producer | PASS wired |
| M | ModalResult/loop-closure/Map placeholders exist | grep: TODO(modal-result-type) (trajectory.ri EndEffectorTrack:322); `Map<String, Real>` + `List<Real>` (kinematic.ri Mechanism:190/:191/:194/:195) | PASS wired |
| M | ModalResult nominal type exists to point to (cross-module) | `std.modal.analysis` declares `ModalResult` (modal_analysis.ri header §"Provides"); consumed in `std.trajectory` (later in loader) → the canonical cross-module case requiring F | PASS wired |
| S | force/velocity/acceleration placeholders exist | grep: TODO(force-scalar) (trajectory.ri JointLimit.max_force:490), TODO(velocity-scalar):550, TODO(acceleration-scalar):553 | PASS wired |
| S | `Force`/`Acceleration` named; `Velocity` NOT named (new dim needed) | grep: `(FORCE, "Force")` dimension.rs:491, `(ACCELERATION, "Acceleration")` :513; NO `"Velocity"` entry (only `AngularVelocity` :509) — confirms trajectory.ri:551 | PASS wired |
| S | new-named-dimension procedure proven (mirror) | task 4548 Phase-A `IMPULSE`/`Momentum` registration (dimension.rs:555-556) + `from_exps` const helper + table-driven `NAMED_DIMENSIONS` | PASS wired |
| S | dimensioned-zero constraint convention (polymorphic-zero NOT landed) | modal_analysis.ri HarmonicForce `frequency > 0Hz`; 4548 Phase-A `impulse > 0 * 1N * 1s`; type-hygiene.md β owns the eventual polymorphic-zero | PASS wired |
| S | tightenings non-breaking on read side | `modal_ops.rs` tolerant `read_scalar_si` (4548 Phase-A precedent: impulse read needed no change) | PASS wired |
| (all) | re-citation detector contract | `crates/reify-audit/src/ptodo.rs` `has_canonical_cite`/`extract_cites` (`#` + 1..=5 non-zero ASCII digits, anywhere on line); liveness lane β requires live non-terminal task | PASS wired |

Numeric-floor branch: N/A — no leaf asserts a tuned numeric bound (S's `> 0 * 1N` is a structural positivity contract; all signals are type-resolution / name-resolution / detector-recognition, not tolerance floors).

## Re-citation closeout (task 4548 steps 9–16)

Phase B (steps 9–14) re-cited *every* in-family family to its owning child task. The step-15 rebase onto current main then **superseded most of those re-citations**: four owning siblings (#4576/#4578/#4579/#4580) had landed and gone **`done`**, deleting their own `.ri` markers upstream — so a surviving cite to any of them would be an *orphan* finding. Step-16 re-ran `reify-audit --pattern PTODO` with the **liveness lane active** (real tasks DB via `REIFY_PTODO_TASKS_DB`) and confirmed the final two-bucket disposition. Per-family final state:

| Leaf | Owning task (status) | Final state in tree | Detector verdict (step-16, live lane) |
|---|---|---|---|
| V | **#4575** (`blocked`, LIVE) | **CITED** `TODO(vec3-type, #4575)` ×8 (kinematic.ri ×5, fea_multi_case.ri ×2, trajectory.ri ×1) | PASS — cited + live, not flagged |
| P | **#4577** (`blocked`, LIVE) | **CITED** `TODO(pose3-type/location-id-type, #4577)` ×2 + `FIXME(location-id-type, #4577)` ×4 | PASS — cited + live, not flagged |
| R | **#4576** (`done`) | **RESOLVED UPSTREAM** — `Range<…>` landed; markers deleted on main | n/a — no marker survives to cite (not orphaned) |
| Pt | **#4578** (`done`) | **RESOLVED UPSTREAM** — `structure def Part` + `param part : Part` landed; FIXMEs deleted | n/a — no marker survives to cite |
| M | **#4579** (`done`) | **RESOLVED UPSTREAM** — nominal `ModalResult` + `JointParent` + `Map<BodyId,JointParent>` landed; markers deleted | n/a — no marker survives to cite |
| S | **#4580** (`done`) | **RESOLVED UPSTREAM** — `Scalar<Force/Velocity/Acceleration>` + `VELOCITY` named dim landed; markers now `RESOLVED(...)` | n/a — no marker survives to cite |
| (land-now) | resolved by 4548 itself | **DELETED** — `FIXME(impulse-dim)`, `TODO(frequency-scalar)` ×2 | absent from tree |

Step-16 detector run (liveness lane active): **zero in-family findings** across the four touched stdlib `.ri` modules (`modal_analysis.ri`, `trajectory.ri`, `kinematic.ri`, `fea_multi_case.ri`), and **no `orphaned`/`unknown-id` finding cites `#4574`–`#4580`**. The out-of-scope doc-comment mirrors that Phase B had catalogued in `trajectory_stdlib_compile.rs` + `flexures/common.rs` were also removed upstream by the `done` siblings (verified absent on the rebased tree). **4548 project invariant satisfied: every task-listed marker is DELETED/RESOLVED or CITED to a live non-terminal owner.**
