# Uniform member access — general instance-member references in expressions (including Geometry)

**Status:** authored 2026-07-24 (PRD session spawned from the threads/holes design session; Leo directive: "general access to all instance member refs (including geometry)").
**Milestone:** v0_6.
**Approach:** **B + H** (contracts + two-way boundary tests) — high-stakes seam: compiler expression/geometry lowering, ≥4 crates (reify-compiler, reify-ir, reify-eval, reify-expr), ≥3 cross-PRD consumers (threads/holes, #5391 standard-parts, placement-relations-belt, sub-iteration).

---

## 0. Provenance & design mandate

Spawned 2026-07-24 from the threads/holes PRD session, where `difference(plate, h.body)` on a `let`-constructed instance was the load-bearing blocker. Leo's rulings (this session, 2026-07-24):

1. **Uniformity mandate.** "Everything that could be accessed via `.` should be handled uniformly. `foo.bar` (and `foo.bar...baz.bam`) should work regardless of what types of entities are being referenced (or referenced through)." Uniform **access** across subs, `let`-constructed instances, structure-typed members, at arbitrary chain depth.
2. **Inert-until-consumed.** A `let`-constructed instance is a *value*: its geometry members never auto-join the parent's rendered/exported solid set; they realize on demand where consumed.
3. **Type-driven geometry position.** The unifying goal: *any expression of static type Geometry is legal in geometry-expression position*. `.ri` functions returning Geometry are in scope as a rung of this PRD.
4. **Enum-typed ctor params on user-module structures** are fixed here; the `sub`-of-stdlib-structure re-declaration hazard is **referenced out** to #5391 gate (4).

## 1. Consumers + user-observable surface (G1)

| Consumer | What it consumes | Surface |
|---|---|---|
| **threads/holes PRD** (parallel session, blocked on this PRD's task ids) | `difference(body, h.cutter)` where `h` is a `let`-constructed hole-feature instance exposing `cutter : Geometry` | `reify check/eval/build` on hole-feature designs |
| **#5391 standard-parts library program** (live milestone) | pervasive member-geometry consumption (`screw.clearance_cutter` etc.) | stdlib part structures usable in booleans |
| **`docs/prds/v0_6/placement-relations-belt.md`** | its synthesized `.world_frame` member *rides* this PRD's resolver (semantics stay theirs — §7) | member reads on sub refs |
| **sub-iteration PRD** (parallel session) | `self.idlers[i].<member>` — after their IndexAccess produces a typed instance ref, member resolution is this PRD's resolver (§7) | indexed assembly addressing |
| **printer_v01 dogfood** | chained derives (`self.rear_drive.unit_a.cap_pitch_r`, the #5360 shape) | assembly single-source idiom |
| **Every author** (ergonomics) | any structure member — scalar, vector, geometry, option — usable wherever an expression of that type is legal | the language itself |

Engine-integration sub-check (`engine-integration-norm.md` §3): per-instance realization consumption plugs into the **op-execute** (§3.1) and **realization-kind dispatch** (§3.2) seams — instance-member geometry endpoints become ordinary realization references consumed by the existing boolean/transform op pipeline; no new in-engine seam is introduced.

## 2. Substrate evidence (G3 baseline — probed 2026-07-24 against main `ef6863e770`, debug binary; fixtures committed at `docs/prds/v0_6/fixtures/member_*.ri`)

**Works today (regression floor — these must stay green):**
- Named-arg AND positional ctor construction of structures (`let`/StructureInstance path); scalar member + derived-`let` reads (`member_let_read.ri`: `s.tap` = 8mm with ctor override threaded).
- **Chained scalar reads through nested `let`-instances** (`member_chain_scalar.ri`: `m.inner.doubled` = 14mm, `m.inner.x` = 7mm — nested StructureInstance values carry evaluated fields; projection chains work). Sharper than the spawning brief assumed.
- **Cross-sub geometry member in geometry position** (`member_sub_geom_baseline.ri`: `difference(plate, self.c.body)` where `c` is a `sub` — checks clean) via the task-3441/3512/GHR-γ path: syntactic `self.<sub>.<member>` match → `GeomRef::Sub` → scoped-cell elaboration (`elaborate_child_instance`, ctor overrides threaded per task 3814).
- Grammar: every target shape (chained dotted paths, fn-call-in-geometry-position, enum-valued named ctor args) parses today — all probes reached *semantic* errors, never parse errors. **No novel syntax; grammar gate N/A; `grammar_confirmed=true` batch-wide** (fixtures double as tree-sitter evidence at decompose).

**Fails today (the gaps this PRD closes):**
- `difference(plate, c.body)` on a `let`-instance → `difference() argument 2 must be a geometry expression` (`member_geom_let_instance.ri`); a `let` alias does not launder it (`member_geom_alias.ri`).
- Chained geometry endpoint (`member_chain_geom.ri`: `k.cutter.body`) → same rejection.
- **Geometry members are absent from StructureInstance values entirely** (`member_instance_geom_field.ri` eval: `Test.c = Cutter { d, half_d }` — no `body` field). Root cause: geometry `let`s lower to realizations with **no value cell** (`entity.rs:1327-1330`), so `materialize_template_lets` (`crates/reify-expr/src/lib.rs:1123-1179`) has nothing to materialize. The gap is two-sided: compile-time acceptance AND per-instance realization semantics.
- `.ri` fn returning Geometry → `unsupported geometry function: clearance_cutter` (`member_fn_geometry.ri`; wildcard arm `geometry.rs:2620`).
- Enum-typed param ctor binding on **user-module** structures → `argument 'fit' has type 'Enum(Fit)' but param 'fit' requires structure type 'Fit'` (`member_enum_ctor.ri`); stdlib-structure enum params (ThreadSpec) bind fine.
- Nested-**sub** scalar chains silently undef (**#5360**, pending-high — eval-side elaboration of a child's own sub-scope; owned there, hard dep of task δ here).

**Compiler anchors:** ctor lowering `CompiledExprKind::StructureInstanceCtor` (`crates/reify-compiler/src/expr.rs:2837`; IR `crates/reify-ir/src/expr.rs:184-210`); member read-back via `template.value_cells` (`expr.rs:6506-6560`); geometry-expression class (`crates/reify-compiler/src/geometry.rs` + `geometry_boolean.rs:40-107` — syntactic two-level `self.<sub>.<member>` matchers, geometry-let ident allowlist, builtin dispatch with wildcard rejection); `CompiledExprKind::CrossSubGeometryRef` (typed discriminator, bare-let-top-level-only invariant, `expr.rs:773-870`); privacy `E_PRIV_MEMBER_ACCESS` (task 3978: **default-public, explicit `priv` opt-out** for param/sub/port).

**R3f constraint (load-bearing, from `docs/design/symbolic-eval-nested-selector-resolution.md` §R3f):** post-walk geometry-handle mints produce placeholder `GeometryHandle { kernel_handle: None }` for bare geometry lets, and those flips are *deliberately never fed into re-eval* — `build()`'s kernel pass revisits only cells still `Undef`. Pinning symbolic handles into instance-value fields would permanently freeze consumers at symbolic results (the "R3f clobber risk"; regression witness `restrict_field_b5_integration`). This forecloses the "geometry members as GeometryHandle fields on StructureInstance" design — see decision D5.

## 3. Resolved design decisions

- **D1 — One resolver, type-driven (Leo).** A single compile-time member-path resolver walks a dotted chain segment-by-segment over static types (structure template → member kind → next hop), replacing shape-matched special cases. Geometry-expression positions accept **any expression whose static type is Geometry** — the acceptance criterion moves from syntax shape to type.
- **D2 — Access is uniform; realization policy is per-declaration-kind.** Uniform `.` access does NOT collapse the `sub`/`let` semantic distinction: a `sub` is an assembly component (auto-realized, posed, rendered — placement PRD's world); a `let`-constructed instance is a **value** (inert; its geometry realizes only where consumed). This is what keeps a hole-cutter out of exports while `difference(plate, c.body)` works.
- **D3 — Realization-site rule (makes "inert-until-consumed" precise).** Realization sites are: geometry `let`s, `sub`s, and geometry-op consumption positions. Instance *construction* is never a realization site. Corollaries: consuming `c.body` inside `difference(...)` realizes the cutter's geometry as an operand only (not a shown body); a top-level geometry-typed alias `let cut = c.body` is itself a geometry let and therefore realizes/renders (consistent with task 3891's cross-sub-alias semantics) — that IS the explicit opt-in surface for showing an instance's geometry.
- **D4 — Local frame only.** Instance-member geometry reads are LOCAL-frame. World-posing (sub poses, `.world_frame`, `RealizedBodySet`) is `placement-relations-belt.md` §7.1 territory (§7 table).
- **D5 — Mechanism: generalize the sub path's scoped-cell elaboration; do NOT add symbolic-handle instance fields.** Consuming a `let`-instance's geometry member elaborates that instance's geometry subtree into scoped realization state (scoped-entity stamp `<parent>.<let_name>` mirroring the sub convention `format!("{}.{}", entity, sub)`), reusing `elaborate_child_instance` semantics (`crates/reify-eval/src/unfold.rs:287`) — proven ctor-override threading (3814), and per-instance identity for caching. *Rejected alternative:* materializing `GeometryHandle` fields into StructureInstance values — collides with the R3f no-clobber contract (§2) and build()'s Undef-only revisit policy. Breadcrumb the rejected variant at the impl site.
- **D6 — Visibility uniform.** Default-public with explicit `priv` opt-out (the task-3978 model), enforced by the resolver at **every hop of every path shape** (sub hop, instance hop, deep chains). No new visibility semantics; existing `E_PRIV_MEMBER_ACCESS` diagnostics fire uniformly.
- **D7 — fn-returning-Geometry rides D1.** A call to a `.ri` fn with declared Geometry return type is a Geometry-typed expression, hence legal in geometry position; lowering inlines/elaborates the fn's geometry result through the same machinery as M3. (Adjacent-but-distinct: geometry inside `generate` lambdas is #5385; inline geometry args to *query builtins* is #5345 — both referenced, not owned.)
- **D8 — Enum ctor binding.** User-module `enum` defs must resolve as enum types in ctor param positions (the current lowering conflates `Enum(Fit)` with "structure type `Fit`"). Fix is in this PRD's ctor/member type-resolution blast radius. Note (scope-bounded): overall ctor field-type *conformance* is globally unenforced at check time (verified 2026-07-20 — `Widget(label: 42)` checks clean); this PRD fixes the enum **over-rejection** wart only and does not take on the general conformance gap (silent-accept class — eradicate-silent-undef territory / follow-up).
- **D9 — No silent Undef at the member seam.** Any member projection this PRD makes newly legal must, on failure, produce a diagnostic or a provenance-carrying Undef — never a bare silent Undef. Coordinates with `eradicate-silent-undef.md` γ (`UndefCause::MemberResolutionFailed` at this very seam — additive provenance, disjunctive e2e, order-independent by design).

## 4. Sketch of approach — mechanisms

- **M1 — Uniform member-path resolver (reify-compiler).** One typed resolution walk for dotted paths of arbitrary depth. Input: AST MemberAccess chain; output: a typed terminal classification — `ValueCell` (solver-visible scoped cell), `InstanceField` (StructureInstance projection), `Realization` (geometry endpoint), `Synthesized` (`.count` today; placement's `.world_frame` tomorrow — extension point). Enforces `priv` per hop (D6). Absorbs the two-level `self.<sub>.<member>` matchers and the instance read-back path as callers.
- **M2 — Type-driven geometry-position acceptance (reify-compiler, reify-ir).** `compile_geometry_call`/boolean-arg routing accepts typed-Geometry expressions; `GeomRef` gains an instance-member endpoint variant (alongside `Step`/`Sub`) carrying the resolved instance path + member. The `geometry.rs` registry cross-check test (task-1733 pattern) extends to the new acceptance class so arm loss stays impossible.
- **M3 — Per-instance lazy realization (reify-eval).** A consumed instance-geometry endpoint elaborates the instance's geometry subtree scoped to `<parent>.<let_name>` (D5), lazily (D3), local-frame (D4), cached per (instance path, ctor bindings). Non-assembly: never enters the parent's shown/exported body set.
- **M4 — fn-returning-Geometry lowering (reify-compiler, reify-eval).** D7; retires the wildcard `unsupported geometry function` rejection for declared-Geometry-returning user fns.
- **M5 — Enum ctor binding fix (reify-compiler type_resolution).** D8.
- **M6 — Diagnostics.** Type-aware rejections replace the generic `must be a geometry expression` where a member path was *resolved but ill-typed* (name the actual type); D9 at the projection seam.
- **M7 — Consolidation.** Sub-path member shapes route through M1; bespoke matchers (`match_self_sub_member` twins, `try_resolve_cross_sub_geom_ref` shape probes) retire behind it. Existing cross-sub suites (`cross_sub_geometry_diagnostic_tests.rs`, `solid_param_tests.rs`, priv suites) stay green — INV-5 (`no-lockstep-duplication`) is the driver.
- **M8 — Docs & discoverability (project PRD gate).** Language-spec section for uniform member access + inertness rule; `crates/reify-mcp/src/tools/chunks/` doc-chunk updates verified against compiler registries; `reify-design` skill cheatsheet idiom (`difference(plate, h.cutter)`); discoverability acceptance (idiom findable from chunks by intent).

## 5. Contract (B+H)

**C1 — Resolver.** `resolve_member_path(expr, scope) → Resolved { segments: Vec<Hop>, terminal: TerminalKind, ty: Type }` where `Hop` records (object type, member name, member decl kind, visibility verdict) and `TerminalKind ∈ {ValueCell(ValueCellId), InstanceField(projection), Realization(GeomEndpoint), Synthesized(kind)}`. Invariants: (i) resolution is purely static — no evaluation; (ii) `priv` violation at hop k fails with `E_PRIV_MEMBER_ACCESS` naming hop k's member, regardless of chain depth; (iii) unknown member at hop k names the *concrete* type at hop k (never "not a geometry expression"); (iv) the resolver is the ONLY member-shape authority — no caller re-matches AST shapes (INV-5).

**C2 — Geometry-position acceptance.** An argument in geometry position is accepted iff its static type is Geometry; classification of HOW it lowers (builtin call chain / geometry-let ident / `GeomRef::Sub` / instance-member endpoint / Geometry-returning fn call / control-flow expression of Geometry type) happens AFTER acceptance, each with a total lowering or a specific diagnostic. The generic rejection fires only for genuinely non-Geometry types and names the actual type.

**C3 — Instance realization.** For instance path P with ctor bindings B consuming member m: (i) identity — realization state is keyed `(P, B, m)`; two consumptions of the same key share one realization; (ii) inertness — no realization for members never consumed; (iii) frame — identity pose (local); (iv) isolation — instance realizations never join the parent's shown-body set or STEP/manifest export set unless aliased into a geometry let (D3); (v) failure — an unresolvable projection yields a diagnostic or provenance-carrying Undef (D9), never silent; (vi) R3f-compat — no `GeometryHandle` is pinned into StructureInstance fields; kernel-less surfaces (`reify check`, pure value-eval) see the same acceptance but go loudly-indeterminate where a kernel value would be required (consistent with `value-eval-geometry-addressing.md` Rung 1 semantics).

**C4 — Order-independence.** This PRD's tasks are landing-order-independent w.r.t. #5360 (eval-side nested-sub elaboration) and eradicate-silent-undef γ (provenance at the same seam): δ hard-depends on #5360; γ-their-side is disjunctive by their design. No task here may change `unfold.rs` nested-sub elaboration semantics (that is #5360's file territory: `crates/reify-eval/src/unfold.rs`, `engine_eval.rs`).

## 6. Boundary-test sketch (two-way; the ι integration-gate's observable signal)

| # | Scenario (fixture) | Preconditions | Postconditions |
|---|---|---|---|
| 1 | `difference(plate, c.body)`, `c` a let-instance (`member_geom_let_instance.ri`) | α β γ landed | `reify check` exits 0; `reify eval` prints `Test.drilled` non-undef; drilled volume ≈ 2.0e-6 − π·(1.7e-3)²·8e-3 m³ within 1% (basis: task-325 volume-vs-closed-form precedent; OCCT boolean volume accuracy ≫ 1% on prism∖cylinder) |
| 2 | alias `let cut = c.body; difference(plate, cut)` (`member_geom_alias.ri`) | same | identical result; `cut` itself realizes as a geometry let (D3 corollary) |
| 3 | inertness: instance constructed, member NOT consumed (`member_instance_geom_field.ri`) | γ | no realization of `Cutter.body` under `Test`'s shown set; export/manifest body count unchanged vs a no-instance control |
| 4 | chain ending in geometry (`member_chain_geom.ri`: `k.cutter.body`) | γ δ | check exits 0; drilled evaluates (same volume assertion as row 1) |
| 5 | scalar chains keep working (`member_chain_scalar.ri`, `member_let_read.ri`) | α (regression) | eval values byte-identical to §2 baselines (14mm / 7mm / 8mm) |
| 6 | sub baseline preserved (`member_sub_geom_baseline.ri`) + full cross-sub suites | M7 | check exits 0; existing cross-sub/priv suites green |
| 7 | `priv` enforced on new paths: `priv param`/`priv sub` read via let-instance chain | α | exactly one `E_PRIV_MEMBER_ACCESS` naming the priv member (extends `priv_member_visibility_tests.rs`) |
| 8 | fn→Geometry (`member_fn_geometry.ri`) | ε | check exits 0; drilled evaluates; wildcard rejection no longer fires for declared-Geometry fns |
| 9 | enum ctor (`member_enum_ctor.ri`) | ζ | check exits 0 (constraint `hd > 4mm` satisfied: 4mm + 0.2mm); eval shows `Test.h` with bound variant |
| 10 | non-Geometry member in geometry position (`c.half_d` in `difference`) | β | specific diagnostic naming the actual type (`Scalar[m]`), not the generic sentence; **negative-assertion**: the diagnostic is observed to fire |
| 11 | unknown member mid-chain (`c.nope.x`) | α | diagnostic naming `nope` and the concrete type at that hop; never silent undef (D9) |
| 12 | kernel-less surface: row-1 fixture under bare `reify check` | γ | accepted statically; no kernel invocation; no silent Undef leak into constraint verdicts |

## 7. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **threads/holes PRD** (unfiled; session waiting on this batch) | consumes | geometry-member-in-geometry-ops capability (task γ here) | this PRD | they wire real `add_dependency` edges onto γ/δ after filing |
| `docs/prds/v0_6/placement-relations-belt.md` | produces→them / consumes←them | **access mechanism** (resolver, synthesized-member extension point) = this PRD; **`.world_frame` semantics, sub poses, `RealizedBodySet`, world-posing** = theirs (§7.1) | split as stated | both PRDs record it; no double-ownership |
| **sub-iteration PRD** (parallel session) | produces→them | member resolution AFTER their `IndexAccess` yields a typed instance ref; indexed collection addressing (`subs[i]`) itself | indexed access = theirs; `.member` resolution = this PRD | coordinate, don't block (their open Q3) |
| **#5360** nested-sub chained reads (pending-high) | consumes (upstream dep) | eval-side elaboration of child's own sub-scope (`unfold.rs`) | #5360 | hard `add_dependency`: #5360 → δ |
| `docs/prds/v0_6/eradicate-silent-undef.md` γ | adjacent | `UndefCause::MemberResolutionFailed` provenance at the member-projection seam | theirs (provenance), ours (functional legality) | disjunctive e2e both sides; landing-order-independent (C4) |
| `docs/prds/v0_6/value-eval-geometry-addressing.md` | constrains | R3f no-clobber contract; kernel-less eval stays symbolic/declining | theirs | D5/C3(vi) honor it; no changes to their machinery |
| **#5391** standard-parts program | consumed-by + references-out | member-geometry consumption (consumer); `sub`-of-stdlib re-declaration hazard | consumer edge onto γ; hazard = #5391 gate (4) | referenced out per Leo ruling 4 |
| **#5385** generate-lambda geometry, **#5345** inline geometry query args | adjacent | different geometry-arg gaps (lambda eval; query-builtin arg hoisting) | those tasks | referenced, not owned |

## 8. Decomposition plan (G2 signals inline; Greek labels → task ids at decompose)

Blast radius per task is tight; `metadata.files` follows tight-or-empty.

- **α — Uniform member-path resolver in reify-compiler** (M1, D1, D6). Intermediate → unlocks β, η; regression floor: boundary rows 5, 7, 11. Crates: reify-compiler. *Consumer:* β/η + every leaf below.
- **β — Type-driven geometry-position acceptance + GeomRef instance-member endpoint** (M2, C2). Intermediate → unlocks γ, ε. Crates: reify-compiler, reify-ir. Registry cross-check extension same-diff. *Consumer:* γ/ε.
- **γ — Per-instance lazy realization of let-instance geometry members** (M3, C3, D2/D3/D5). **LEAF — the geometry-member-in-geometry-ops capability the threads/holes PRD waits on.** Signal: `member_geom_let_instance.ri` + `member_geom_alias.ri` pass `reify check` AND `reify eval` shows `Test.drilled` non-undef with volume within 1% of closed form (boundary rows 1–3, 12); e2e test in `crates/reify-eval/tests/` (nextest heavy/smoke classification same-diff if kernel-bound). Deps: α, β.
- **δ — Chained member paths ending in geometry** (deep chains). **LEAF.** Signal: `member_chain_geom.ri` passes check+eval (boundary row 4); a nested-sub chain e2e (the #5360 repro shape reaching a geometry endpoint) passes once #5360 lands. Deps: γ, **#5360** (cross-batch, real edge).
- **ε — `.ri` fn returning Geometry legal in geometry position** (M4, D7). **LEAF.** Signal: `member_fn_geometry.ri` passes check+eval (boundary row 8). Deps: β (γ for realization plumbing of fn-result geometry).
- **ζ — Enum-typed ctor param binding for user-module structures** (M5, D8). **LEAF.** Signal: `member_enum_ctor.ri` — check exits 0, constraint satisfied, eval shows bound variant (boundary row 9). Deps: none (parallel-safe; type_resolution seam).
- **η — Route sub member access through the uniform resolver; retire bespoke matchers** (M7). Intermediate → unlocks ι. Signal: cross-sub + priv suites green with matchers deleted (boundary row 6). Deps: α, γ (behavioral parity target must exist first). G7 driver: `no-lockstep-duplication`.
- **θ — Docs, doc-chunks, cheatsheet, discoverability** (M8; project PRD gate). **LEAF.** Signal: doc-chunks updated with signatures verified against compiler registries; `reify-design` cheatsheet shows the `difference(plate, h.cutter)` idiom; discoverability acceptance — the idiom findable from chunks by intent query. Deps: γ, ε, ζ.
- **ι — Two-way boundary-test suite** (§6 table implemented; integration gate). **LEAF.** Signal: the 12-row suite exists and is green on the merge gate; any new gate-resident `tests/infra` or wallclock assertion carries its drift-guard registration same-diff. Deps: γ, δ, ε, ζ, η.

DAG: α → β → {γ → δ, ε}; α → η → ι; {γ,δ,ε,ζ,η} → ι; {γ,ε,ζ} → θ; #5360 → δ.

**G7 walk (advisory here, blocking at decompose):** silent-fail-soft — D9/C3(v) address it (γ, δ); lock-step duplication — η exists to remove it, α forbids new copies (C1-iv); contracts-machine-checked — C1–C4 back the registry cross-check + boundary suite rather than prose-only; no detector/suppressor loops introduced (storm-escape N/A); no log-scraping (structured diagnostics throughout).

## 9. Out of scope

- World-posed realization, sub poses in snapshots, `.world_frame` semantics → `placement-relations-belt.md`.
- Indexed collection member paths (`subs[i].member`) → sub-iteration PRD (this resolver is its substrate).
- Nested-sub eval elaboration fix → #5360 (hard dep of δ).
- `sub`-of-stdlib-structure re-declaration → #5391 gate (4).
- Geometry inside `generate` lambdas → #5385; inline geometry args to query builtins → #5345.
- Value-eval symbolic-descriptor semantics → `value-eval-geometry-addressing.md` (this PRD only honors its contracts).
- General ctor field-type conformance enforcement (the silent-accept gap) → follow-up outside this PRD (D8 note).

## 10. Open questions (tactical)

1. **GUI debug visibility of inert instance realizations.** Should the GUI offer a toggle to peek at un-consumed instance geometry? Suggested resolution: no v1 surface; the D3 alias idiom covers inspection. Decide during γ.
2. **Cache eviction for instance realizations.** `(P, B, m)` keying interacts with `selective-realization-eviction.md`; suggested resolution: reuse its policy unchanged. Decide during γ.
3. **Diagnostic wording** for type-aware geometry-position rejections (M6). Decide during β; follow `diagnostic-severity-policy.md` once eradicate-silent-undef η lands.
4. **fn-result realization identity** — key by (fn, args) or by call site? Suggested: call site (simpler, matches realization-step semantics). Decide during ε.
