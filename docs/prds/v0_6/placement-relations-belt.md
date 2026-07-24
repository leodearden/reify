# PRD — Constraint-driven placement completion, clearance constraints & the belt-path stress test

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-24 · **Author:** /prd session spawned from the 2026-07-24 language review (brief `~/.claude/spawn-briefs/2026-07-24-prd-placement-relations-belt.md`; dogfood session a0d342d4, printer_v01)

**Relationship to `geometric-relations.md`:** direct continuation. The parent PRD's α–θ batch (tasks 4381–4388) landed the relate vocabulary, the `at auto` relate-solve, and grounding; only θ (#4388, DOF ledger + `W_UNDERDETERMINED`) is still pending. This PRD completes the *idiom* (fixing the silent traps that make relate hazardous in practice), gives placements an observable world pose, adds the clearance-constraint family over that substrate, and stress-tests the whole stack with `belt_path`.

**This PRD OWNS the world-pose substrate spec** (§7.1). The parallel sessions of 2026-07-24 (sub-iteration, affine-transforms, entrypoint-unification) cite it — reference this section, do not redesign it.

---

## 1. Goal & user-observable surface

A Reify author assembling parts writes `sub bush : Bushing at auto` + `relate { concentric(…) flush(…) }` as the **default placement idiom**, and the tool — not the author's eyes — catches every mistake in that workflow:

1. A `relate` block that solves nothing (no `at auto` sub in scope) **says so** instead of silently no-opping.
2. A relate operand type error makes `reify check` **exit nonzero** (today it prints the error and exits 0).
3. An under-constrained placement surfaces the DOF ledger (`W_UNDERDETERMINED`, parent-PRD θ #4388 — referenced, not re-owned).
4. `connect` between two `LocatedPort` ports **actually places** the `at auto` side (today frame alignment is a doc-only fiction).
5. `min_clearance(a, b)` / `self_clearance(a)` / `not self_interferes(a)` accept **Structure operands in the natural assembly style** — sub instances, world-posed — and `constraint no_undeclared_interference(self)` machine-verifies "nothing touches unless a relation or connect says it should" under `reify build`.
6. `belt_path(...)` (stdlib, written in `.ri`) produces tangent-line + wrap-arc path geometry around pulley pitch circles that **stays tangent when a pulley moves**, with derived length and per-pulley wrap angles.

**Consumer (G1):** printer_v01 tendon routing (`prj/printer_v01/printer.ri`, on main, 995 lines). **Leaf (G2):** rear-drive routing v2 rebuilt on relate + `belt_path` with ZERO undeclared interferences, machine-verified by the clearance constraints under `reify build` — replacing the hand-solved literals (e.g. `printer.ri:341–383`: `w1_in = winding_z + lead_split + r_pitch`, tangency arithmetic in comments) and the eyeball interference oracle that shipped real interferences in the dogfood.

## 2. Background — why (G6 premise, probe- and dogfood-verified)

The printer_v01 dogfood session hand-solved ~40 tangency relations into literal `transform3(...)` calls across 5–6 discarded layouts and still shipped interferences only human eyes caught. "A belt is one object modelled as fifteen disconnected primitives… All continuity lives in comments and my arithmetic."

Probe ground truth (2026-07-24, artifacts `probe-relations/` in the review scratchpad; fixtures are re-committed as test fixtures by the leaves below):

- `relate{}` **works** as a 6-DOF placement solver for static mates: p2a (`at auto` + concentric + flush) solved to ~1.5e-8 m and the pose lands in STEP export.
- **Trap (a):** `sub` without `at auto` + `relate` = complete silent no-op. Seam: `crates/reify-eval/src/relate_solve.rs:836` filters every scope with `auto_unknowns.is_empty()` — relations in such scopes are neither solved **nor verified**.
- **Trap (b):** under-constrained DOFs silently sit at gauge-seeded values. The residual-DOF data is already computed (`RelateSolution::free`); the `W_UNDERDETERMINED` ledger renderer is parent-θ #4388 (pending).
- **Trap (c):** relate operand-type errors print but `reify check` exits 0. Seam: `cmd_check` (`crates/reify-cli/src/main.rs:475+`) fails on compile-phase `Severity::Error`, but engine-phase diagnostics flow through `report_eval_output`/`finish_check` without flipping the exit code.
- **connect frame alignment is doc-only:** `crates/reify-compiler/src/connect.rs:584–622` generates a frame constraint only for double ad-hoc `@` selectors, as a plain `Bool Eq` `CompiledConstraint` (permanently INDETERMINATE — never enters the relate-solve); the compatibility constraint is a compile-time literal. Content-fix for the doc claim is #5389 (docs-truth territory); the *implementation* is this PRD's ζ.
- **The interference oracle can't see assemblies:** the kinematic trio (`interferes`/`interferes_with`/`min_clearance`, `units.rs:131`) works only over a mechanism `snapshot()` let-binding under eval/build; snapshot bodies resolve `solid` names against `named_steps` (top-level geometry lets — `crates/reify-eval/src/geometry_ops.rs:4248–4479`). **Sub instances are unreachable and sub `at` poses are not carried into snapshots** — the natural assembly style has no oracle.

## 3. Sketch of approach — four blocks

**Block 1 — world-pose substrate (owned spec, §7.1).** One shared primitive: `RealizedBodySet(scope) → [(body, world_transform, provenance-path, aux)]` — the recursive walk that realizes a structure instance subtree's solid bodies with composed poses (explicit `at` ∘ relate-solved autos), excluding `aux let` reference geometry. Consumers: the clearance family (block 3), snapshot enrichment, and (future) mass properties — one walk, no lock-step duplicates. On top of it: a read-only `.world_frame` member on sub instance refs, an `in_frame(value, frame)` conversion builtin, and staged placement evaluation with cycle detection (§7.1.3).

**Block 2 — relate idiom completion.** Fix traps (a) and (c) at their named seams; reference θ #4388 for trap (b); implement `connect` frame alignment for `LocatedPort` pairs by emitting a frame-mate into the enclosing scope's relate-solve (the decision the review recommended: *implement* rather than deprecate the doc claim — this collapses the three-placement-idioms problem: `at <transform>` stays as the escape hatch, `at auto`+relate is the default, `connect` now composes with it instead of being a third half-working idiom).

**Block 3 — clearance-constraint family (Leo's design, refined).** `min_clearance(a, b)` accepting Structure or Geometry; list forms `min_clearance(l)` (N² with AABB prefilter) and `min_clearance(l, m)` (M×N); `self_clearance(a)` = pairwise-over-bodies excluding adjacent/declared-contact pairs (NOT naive self-distance, which is 0 for any connected solid) plus a curve-domain form for paths (min distance between points whose arc-length separation exceeds δ — the tendon case); `not self_interferes(a)`. Structure→geometry semantics: the realized solid set of the subtree, world-posed, EXCLUDING `aux let` reference geometry; min over body pairs. Undef bodies ⇒ **loudly indeterminate**, never a silent pass. **"No undeclared interference":** pairs connected via relate `coincident`/`flush`/`fasten` or `connect` edges are auto-whitelisted declared contacts; within the scope of the (opt-in, v1) assertion `constraint no_undeclared_interference(self)`, everything else must not interfere — this is how the tool discriminates intent. A read-only `reify report --contacts` triage listing is a follow-on, not this PRD's leaf.

**Block 4 — `belt_path` as a language-level stdlib feature (`.ri`).** Tangent lines + wrap arcs around pulley pitch circles with wrap-direction choices, producing sweepable path geometry + derived length + per-pulley wrap angles, staying tangent when a pulley moves. Deliberately written in `.ri` as a **language-power stress test**: every place it cannot be expressed in `.ri` and forces a Rust escape is a measured finding, recorded per the protocol in §8.

## 4. G3 — substrate verification record (2026-07-24, verified against main 366c63a679)

| Assumed capability | Status | Evidence |
|---|---|---|
| Relation vocabulary (10 pure names + arity-3 `angle`/`distance`) | EXISTS | `crates/reify-compiler/src/relation_signatures.rs:54–68` |
| `at auto` + relate-solve + `ApplyTransform` placement + STEP export | EXISTS (probe p2a) | `crates/reify-eval/src/relate_solve.rs` (ζ #4386 done) |
| `Type::Frame(3)` / `Value::Frame`; `fasten` over Frames | EXISTS | relation_signatures.rs:186–189; relate_solve writes `Value::Frame` poses |
| `trace_to_ground` / B6 floating diagnostic | EXISTS | relate_solve.rs:504–579 |
| DOF ledger / `W_UNDERDETERMINED` | QUEUED — **#4388 (pending)**, parent PRD θ | `reify-core/src/diagnostics.rs:3499–3515` (code exists; renderer pending) |
| `LocatedPort : Port { param frame : Frame3 }` + asymmetric-LocatedPort check | EXISTS | `crates/reify-compiler/stdlib/ports.ri`; connect.rs:217, 353–401 |
| Kinematic trio over snapshots (FK `world_transform` per body, ApplyTransform, pose cache) | EXISTS | units.rs:131–159; geometry_ops.rs:4248–4479 |
| `intersects`/`distance` geometry queries on geometry lets | EXISTS | units.rs GEOMETRY_QUERY_NAMES |
| `aux` marking distinguishable at compile time | EXISTS | `is_aux` on `ValueCellDecl`/`SubComponentDecl` |
| 3D curve ops: `LineSegment`, `Arc{center,radius,start_angle,end_angle,axis}`, `Helix`; `Sweep`/`SweepGuided` | EXISTS | `crates/reify-ir/src/geometry.rs:883–933` |
| **Wire-join primitive (segments → one contiguous wire)** | **ABSENT** — queued as κ | verified: no MakeWire/JoinWire/concat op anywhere in reify-ir/reify-eval |
| Trig/vector math for tangent construction (`atan2`, `acos`, `sqrt`, …) | EXISTS | math_signatures.rs:49, 97–99 |
| List iteration producing geometry (`generate(n, \|i\| geom)`) | **BROKEN — #5385** (silent Undef; pending, high) | list_helpers.rs:62; λ hard-depends on it |
| `helix()` as sweep spine | IN-PROGRESS #5342 (not needed for belt — flat paths) | task text |
| `pipe()` non-+Z paths | QUEUED #5343 (soft; `sweep` with a posed profile is the fallback) | reify-kernel-occt/src/lib.rs:236–273 |
| `Type::Set` / `Type::List` | EXIST (List is the v1 surface) | reify-core/src/ty.rs:132,134 |
| printer_v01 on main | EXISTS | `prj/printer_v01/` (tracked; last touched 361706754e) |

**Grammar gate:** no novel grammar productions. Every new surface is a function call (`in_frame`, `min_clearance`, `self_clearance`, `self_interferes`, `no_undeclared_interference`, `wire`, `belt_path`) or a member access (`.world_frame`) — both parse today (probe p2a exercises `sub.member` in relate operands; printer.ri exercises chained member access). `grammar_confirmed=true` for every leaf; decompose re-verifies with tree-sitter fixtures per the overlay.

## 5. Resolved design decisions

1. **`.world_frame` semantics are scope-relative ("scope-world").** `s.world_frame` = the frame of sub instance `s` expressed in the coordinate frame of the structure instance whose scope the expression evaluates in (`self`), composing through intermediate levels for chained access (`a.b.world_frame` = pose(a) ∘ pose(b)). At the model root this **is** the global frame. *Rejected alternative — global-root-absolute:* a template can be instantiated many times; a cell expression inside it cannot have a single global pose at template-eval time. Scope-relative is well-defined per instance, cacheable, and is exactly what sibling-relative derivations (clearance, belt tangency between sibling pulleys) need. The name `world_frame` is kept (per review agreement); the doc chunk states the scope-world meaning prominently.
2. **`in_frame(v, f)` = express `v` (given in frame `f`'s coordinates) in the current scope frame:** returns `f · v`. Typical use: `in_frame(bush.bore_axis, bush.world_frame)` — a sub's local datum in assembly coordinates. The inverse direction composes with existing frame ops (`frame_to_frame`, `reify-stdlib/src/geometry.rs:3183`); no second builtin in v1.
3. **Placement evaluation is staged; cycles are errors.** Within a scope: Phase A = relate-solve (determines `at auto` poses); Phase B = `at <expr>` evaluation, which MAY read `.world_frame` of subs whose pose Phase A (or a plain `at`) determined. An expression **consumed by** Phase A (a relate operand datum, an auto seed) that reads `.world_frame` of any same-scope sub is a cycle → `E_POSE_CYCLE` diagnostic naming the cycle path, at check time where statically detectable and at build time otherwise. Never silently accepted (G6 branch 4: the rejection is mechanism-backed — the leaf test observes the diagnostic fire).
4. **Trap (a) fix — zero-auto relate scopes verify instead of vanishing.** A scope with relations but no auto unknowns: relations are *verified* at the fixed poses (they are directives; over fully-fixed operands they degenerate to assertions). Violated ⇒ error (build fails, check exits nonzero); additionally `W_RELATE_NO_AUTO` names the relate block and the fixed subs so the "forgot `at auto`" case is loud even when the assertion happens to hold. Removes the `relate_solve.rs:836` empty-auto filter's silent branch.
5. **Trap (c) fix — engine-phase error diagnostics flip the `check` exit.** Scoped here to the relate family (operand-type errors, `E_POSE_CYCLE`); the *general* invariant ("any error-severity diagnostic ⇒ nonzero exit") is being drafted by the 2026-07-24 silent-undef/placeholder-eradication session — this PRD conforms to those invariants and its leaf narrows to the relate family so the two batches compose instead of colliding (§6 seam row).
6. **connect frame alignment: implement for `LocatedPort` pairs, full-lock mate.** When both connect operands satisfy `LocatedPort` (directly or via refinement — the existing connect.rs:353–401 machinery), lower the connection to a frame-mate emitted into the enclosing scope's relation set: mate convention = origins coincident, z-axes antiparallel (ports face each other), x-axes aligned — a deterministic full 6-DOF lock (`fasten` against the flipped right frame). *Rejected for v1:* leaving spin free (5-DOF mate) — predictable full-lock doesn't depend on pending θ #4388 for comprehensibility; a `free`-spin opt-out is a recorded follow-on. `Frame3` (a params struct of four `Vec3<Length>`) converts to a `Value::Frame` at lowering; ill-conditioned frames (non-orthonormal axes) get a typed error. The ad-hoc-`@` `Bool Eq` branch (connect.rs:584–622) is superseded by the same lowering.
7. **Clearance family semantics.** Structure operand ⇒ `RealizedBodySet` of the instance subtree (world-posed, aux-excluded), min over body pairs; Geometry operand ⇒ that body alone. Any Undef body in the set ⇒ the query is **loudly indeterminate** (diagnostic naming the undef body's provenance path; constraint verdict INDETERMINATE, never a silent pass). Under bare `reify check` (no kernel by design): same loud-indeterminate shape. AABB prefilter before exact `BRepExtrema` distance; the existing pose cache pattern (geometry_ops.rs:4457–4479) reused.
8. **Declared contacts derive from the model, not from annotations.** A body pair is a *declared contact* iff their owning subs are linked by a relate `coincident`/`flush`/`fasten` edge or a `connect` edge (transitively at the sub-pair level, not body level). `no_undeclared_interference(self)` asserts: every non-whitelisted body pair has distance > 0. **Opt-in in v1** (an explicit `constraint` the author writes) — making it an always-on default would break every existing intentionally-interpenetrating model; a strictness-level default is a recorded follow-on. Declared pairs are exempt entirely in v1 (a fastened bolt interpenetrates its thread at model fidelity); distinguishing touch-vs-overlap for declared pairs is a follow-on.
9. **`self_clearance` excludes adjacency by the same whitelist** plus same-body pairs; its curve-domain form `self_clearance(path, min_separation: Length, window: Length)` samples the path and reports the min distance between sample pairs whose arc-length separation exceeds `window` — the tendon-crossing case.
10. **`belt_path` v1 takes explicit lists** (`centers : List<Point3<Length>>` in a named plane, `radii : List<Length>`, `wrap : List<Bool>` (true = CCW), `closed : Bool`) and is a stdlib **structure** (`BeltPath`) exposing `path : Geometry` (joined wire), `length : Length` (closed-form: Σ tangent segments + Σ arc lengths), `wrap_angles : List<Angle>`. Tangency is exact by construction (tangent points and arc endpoints are the same computed expressions). The collection-of-subs input surface belongs to the parallel sub-iteration PRD — v1 consumers feed centers from `.world_frame` origins explicitly; coordinate, don't block (§6).
11. **`wire(segments : List<Geometry>) → Geometry`** is the one pre-registered Rust escape: an op-execute kernel op (OCCT `BRepBuilderAPI_MakeWire`) joining contiguous curve segments into a single sweepable wire, with a typed error on non-contiguity (OCCT's tolerance is the contiguity oracle). Engine seam: §3.1 op-execute (`engine-integration-norm.md`).
12. **Signatures use `List<T>`** (the brief's "Set" reads as unordered-semantics intent; `Type::Set` exists but list literals/helpers are the ergonomic surface today). Order does not affect any result defined here.

## 6. Cross-PRD relationship (G4)

| Other PRD / session | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/geometric-relations.md` (parent) | consumes | relate-solve, `RelateSolution`, vocabulary; θ #4388 DOF ledger | parent owns θ; this PRD depends on landed ζ/η, references θ | wired (θ pending) |
| sub-iteration PRD (parallel session 2026-07-24) | produces→them | world-pose spec §7.1 (they cite); collection-of-subs surface (they own; `belt_path` v2 consumes) | each owns its side | queued |
| affine-transforms PRD (parallel) | produces→them | §7.1 world-pose spec (rigid `Transform`/`Frame` only; non-rigid maps never reach placement per their own recommendation) | this PRD owns §7.1 | queued |
| silent-undef / placeholder-eradication session (parallel) | consumes | silent-failure invariants draft (error-diag ⇒ nonzero exit; loud-indeterminate); this PRD's ε narrows to the relate family | they own the general invariant; ε owns the relate-family slice | queued (draft doc not yet landed — ε's text re-checks at dispatch and shrinks to test-only if the general fix landed first) |
| docs-truth session (parallel) + #5389 | both | doc chunks: #5389 documents the EXISTING kinematic trio; this PRD's ν documents the NEW surfaces; ζ makes connect.md's frame-alignment claim true (docs-truth may first correct it to "not implemented" — ν restores it post-ζ) | #5389 owns existing-oracle docs; ν owns new-surface docs | queued |
| stdlib-module-arch session (parallel) | consumes | `belt.ri` module placement conforms to whatever module architecture lands; content is theirs to move, not to specify | they own layout | queued |
| `geometric-joints.md` / KIN-OFFSET-1 | none new | no new seam: clearance family reads poses, never writes them; mechanism/joint FK stays owned there | — | n/a |
| kernel tasks #5342 (helix), #5343 (pipe orientation), #5385 (generate-of-geometry) | consumes | λ ← #5385 (hard); μ ← #5343 (soft — `sweep` with posed profile is the fallback); #5342 not needed (belt paths are planar) | kernel tasks own their fixes | #5342 in-progress, #5343/#5385 pending |

Engine-integration norm (G1 sub-check): clearance family evals plug into the established eval-time query dispatch (sibling of `try_eval_kinematic_query`; norm §3.1 op-execute for kernel probes); `wire` is a §3.1 `GeometryOp`; connect frame-mates enter §3.5 (ConstraintSolver) via the relate-solve. No new seam is introduced.

## 7. Contract section (B+H)

### 7.1 World-pose substrate (THE OWNED SPEC — parallel PRDs cite this section)

**7.1.1 `RealizedBodySet`.** `realized_bodies(scope_instance, opts) → Vec<RealizedBody>` where `RealizedBody = { handle: GeometryHandleId, world_transform: Frame, path: InstancePath (e.g. "drive.unit_a.drum"), owning_sub: top-level-sub-of-scope, aux: bool }`. Semantics: recursive walk of the instance subtree; each level composes the sub's pose (explicit `at` value or relate-solved auto pose) onto the accumulated transform; leaf solid geometry cells produce bodies; `aux let` cells and `aux` subs are marked (default-excluded by clearance consumers). A body whose geometry evaluates Undef yields a **placeholder entry with its provenance path** (never silently dropped) so consumers can go loudly indeterminate. Invariant: exactly one walk implementation; snapshot enrichment (γ) and the clearance family (η/θ/ι) both consume it (no-lockstep-duplication).

**7.1.2 `.world_frame` / `in_frame`.** `sub_ref.world_frame : Frame<3>` read-only member per decision 1 (scope-world). `in_frame(v, f)` per decision 2; `v` ranges over `Point3`/`Vec3`/`Axis`/`Plane`/`Direction`/`Frame`. Type errors for other operand kinds are ordinary compile diagnostics.

**7.1.3 Staging & cycles.** Per decision 3. The dependency edge is `pose(s) → every .world_frame read of s`; the cycle check runs on the existing cell-dependency graph extended with these edges. Diagnostic contract: `E_POSE_CYCLE` (error) with the cycle path (`bush.world_frame → relate operand concentric(…) → pose(bush)`).

**7.1.4 Snapshot enrichment (γ).** `snapshot()` bodies additionally enumerate sub-instance bodies via 7.1.1, carrying `world_transform` = FK pose ∘ static composed pose, so the kinematic trio reaches assemblies. Existing top-level-let bodies keep byte-identical behavior (C2).

### 7.2 Clearance family signatures

```
min_clearance(a, b) -> Scalar<Length>        // a, b: Structure | Geometry (arity-2; snapshot trio keeps arity-3)
min_clearance(l : List<...>) -> Scalar<Length>          // N² over the list, AABB-prefiltered
min_clearance(l : List<...>, m : List<...>) -> Scalar<Length>   // M×N
self_interferes(a) -> Bool                   // any non-whitelisted body pair with distance <= 0
self_clearance(a) -> Scalar<Length>          // min over non-adjacent, non-declared pairs
self_clearance(path : Geometry, window : Length) -> Scalar<Length>  // curve-domain (arc-length-separated samples)
no_undeclared_interference(a) -> Bool        // constraint surface; whitelist per decision 8
```

Verdict semantics in `constraint` position: Kleene — TRUE/FALSE when all bodies realize; INDETERMINATE + diagnostic naming the unrealized body's path otherwise (decision 7). All are eval/build-phase queries (check attaches no kernel by design — same posture as the existing trio, but *loud*).

### 7.3 connect → relate lowering

For `connect a.p <-> b.q` where both port types satisfy `LocatedPort`: emit `RelationInstance(fasten, frame_of(a.p) , flip_x(frame_of(b.q)))` into the enclosing scope's relation set before the relate-solve partitions (flip = π about the port frame's x-axis, realizing origins-coincident + z-antiparallel + x-aligned). `frame_of` converts the port's `Frame3` params struct (evaluated in the sub's local frame) to a `Value::Frame`; non-orthonormal ⇒ typed error `E_PORT_FRAME_ILL_FORMED`. Interaction rules: the mate participates in the driving-set rank partition exactly like an authored `fasten`; if the target sub is not `at auto`, the mate degenerates to a verified assertion (consistent with decision 4). The connector-sub path (`SubComponentDecl` synthesized connectors) is out of scope for the mate in v1 (recorded follow-on).

### 7.4 `belt_path` (stdlib)

```
structure BeltPath {
    param plane   : Plane                      // belt plane (all centers projected onto it)
    param centers : List<Point3<Length>>
    param radii   : List<Length>
    param wrap    : List<Bool>                 // true = CCW wrap seen from plane normal
    param closed  : Bool = true
    let path        : Geometry                 // wire([tangent_0, arc_0, tangent_1, ...])
    let length      : Length                   // closed-form sum
    let wrap_angles : List<Angle>
}
```

Construction: consecutive pulley pairs get the external/internal common tangent selected by their wrap flags (crossed tangent iff wrap flags differ); wrap arc per pulley spans incoming→outgoing tangent points in the wrap direction. Degenerate inputs (overlapping pitch circles, tangent nonexistence) ⇒ error diagnostics naming the pulley indices — never Undef geometry.

## 8. Language-power findings protocol (the stress-test contract)

Every place `belt_path` cannot be expressed in `.ri` and forces a Rust escape is a **measured finding**. λ's deliverable includes `docs/notes/belt-path-language-findings.md`: one row per escape — what was being expressed, why `.ri` couldn't, the escape taken, and the language feature that would remove it. Pre-registered findings from this authoring session's substrate verification:

- **F0** — `generate(n, |i| geometry)` silently yields Undef (#5385): list-of-geometry construction, the core of any N-pulley loop, is broken. λ hard-depends on the fix; if `flat_map`'s geometry path shares the defect, that extends #5385 (report, don't work around).
- **F1** — no wire-join primitive: path composition forces the κ kernel op. (A curve algebra in `.ri` would remove this class.)
- **F2 (candidate)** — `pipe` +Z restriction (#5343): sweeping the flat belt path into a solid needs a posed-profile `sweep` workaround until #5343 lands.

The findings doc is input to the next language-review cycle; it is a deliverable, not a scratch note.

## 9. Boundary-test sketch (B+H)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Zero-auto relate scope, violated relation | p2b-shape fixture: two fixed subs, `flush` that does not hold | build fails; `reify check` exit ≠ 0; `W_RELATE_NO_AUTO` present |
| B2 | Zero-auto relate scope, satisfied relation | same, poses satisfy the relation | build succeeds; `W_RELATE_NO_AUTO` still emitted |
| B3 | Relate operand type error exits nonzero | p2d-shape fixture (`concentric` on a Scalar) | diagnostic + `reify check` exit ≠ 0 |
| B4 | `.world_frame` after solve | p2a fixture + `let f = bush.world_frame` | `reify eval` prints the solved frame (matches the STEP pose) |
| B5 | Pose cycle rejected | relate operand reads same-scope `.world_frame` | `E_POSE_CYCLE` with cycle path; exit ≠ 0 |
| B6 | connect places an `at auto` sub | two structures with mated `LocatedPort`s, right side `at auto` | build: right sub posed per §7.3 convention (STEP assertion) |
| B7 | Structure-operand clearance | assembly of two relate-placed subs | `min_clearance(a, b)` = known analytic gap ± kernel tol under `reify eval` |
| B8 | Undef body ⇒ loud indeterminate | one sub's geometry Undef | clearance constraint INDETERMINATE + diagnostic naming the body path; never TRUE |
| B9 | Declared-contact whitelist | fastened pair touching + un-declared pair overlapping | `no_undeclared_interference(self)` FALSE naming only the un-declared pair; after declaring it, TRUE |
| B10 | Aux exclusion | aux let sketch geometry overlapping a body | clearance ignores the aux geometry |
| B11 | `wire` + sweep | `wire([line_segment, arc])`, tangent-continuous | `sweep(posed profile, wire)` produces non-Undef solid |
| B12 | Belt tangency under motion | 3-pulley `BeltPath`; move one center between evals | both evals: wire join succeeds; `length` matches closed-form; wrap_angles update |
| B13 | Snapshot sees subs | mechanism with sub instances, no top-level lets | `interferes(snap)` returns a real list (today: bodies skipped) |

## 10. Decomposition plan (G2 signal per task; Greek labels → task IDs at decompose)

Phases: **1 = world-pose substrate** (α–γ) · **2 = idiom completion** (δ–ζ) · **3 = clearance** (η–ι) · **4 = belt** (κ–μ) · **5 = docs gate** (ν–ο).

- **α — `RealizedBodySet` walk primitive** (§7.1.1). Modules: `crates/reify-eval` (new module beside geometry_ops). *Signal (intermediate → γ, η, ι, μ):* unit + integration test: a two-level assembly's body set carries composed world transforms and aux flags; Undef leaf yields placeholder-with-path. *Prereqs:* none. Manifest: producer-self; anchors relate_solve.rs poses, `is_aux`.
- **β — `.world_frame` + `in_frame` + staging/cycle diagnostic** (§7.1.2–7.1.3). Modules: reify-compiler (member lowering, dep edges), reify-eval. *Signal (leaf-grade, B4/B5):* CI `.ri` example prints the solved frame; cycle fixture emits `E_POSE_CYCLE`, exit ≠ 0. *Prereqs:* none intra-batch (relate-solve landed). `grammar_confirmed=true`.
- **γ — sub poses into snapshots** (§7.1.4). Modules: reify-eval snapshot builder + geometry_ops. *Signal (B13):* `.ri` example where `interferes(snapshot)` over an assembly of subs returns a real pair list. *Prereqs:* α.
- **δ — zero-auto relate verification (trap a).** Modules: reify-eval/relate_solve.rs (the :836 filter), diagnostics. *Signal (B1/B2):* fixture emits `W_RELATE_NO_AUTO`; violated ⇒ error, exit ≠ 0. *Prereqs:* none.
- **ε — relate-family engine-phase errors flip `check` exit (trap c).** Modules: reify-cli finish_check seam (narrowed to relate-family codes). *Signal (B3):* p2d-shape fixture exits nonzero. *Prereqs:* none. **Dispatch note:** if the silent-undef batch's general invariant landed first, re-scope to a regression test binding the relate family (task text carries this instruction).
- **ζ — connect `LocatedPort` frame alignment** (§7.3). Modules: reify-compiler/connect.rs, relate-solve threading. *Signal (B6):* CI example: connect + `at auto` places the sub; STEP pose assertion. *Prereqs:* β (frame conversion helpers). Coordinates #5389 (docs) — ζ's diff updates connect.md's claim to match the now-real behavior if #5389 already landed the correction.
- **η — `min_clearance(a, b)` Structure/Geometry overload** (§7.2, decision 7). Modules: reify-eval query dispatch (sibling of `try_eval_kinematic_query`), units.rs signatures. *Signal (B7/B8/B10):* `.ri` example prints a real Length under eval; Undef-body fixture goes loudly indeterminate. *Prereqs:* α.
- **θ — list forms** (`min_clearance(l)`, `min_clearance(l, m)`, AABB prefilter). *Signal:* N-body example returns the analytic min; prefilter unit test. *Prereqs:* η.
- **ι — declared contacts + `self_interferes`/`self_clearance` + `no_undeclared_interference`** (decisions 8–9). Modules: reify-eval (whitelist derivation off relate/connect edges), constraint verdicts. *Signal (B9):* whitelist fixture per B9; curve-domain `self_clearance` fixture (crossing path FALSE at δ). *Prereqs:* α, η (+ζ for connect-edge whitelist arm).
- **κ — `wire()` kernel op** (decision 11). Modules: reify-ir GeometryOp, reify-kernel-occt (MakeWire), units.rs. *Signal (B11):* sweep-along-wire example builds. *Prereqs:* none.
- **λ — `belt_path` stdlib** (§7.4, §8). Modules: `crates/reify-compiler/stdlib/belt.ri` (placement per stdlib-module-arch outcome), findings doc. *Signal (B12):* CI example: 3-pulley closed belt; center moved between evals stays tangent; `length`/`wrap_angles` match closed form. *Prereqs:* κ; **out-of-batch hard: #5385**.
- **μ — printer_v01 rear-drive routing v2 (THE LEAF).** Modules: `prj/printer_v01/printer.ri` (+ a small routing module if it decomposes). *Signal (the consumer signal):* rear-drive routing rebuilt on relate + `belt_path`; `constraint no_undeclared_interference(self)` active; `reify build` exits 0; a deliberately perturbed pulley position makes the build FAIL naming the pair (regression guard for the eyeball-oracle failure class). *Prereqs:* β, δ, η, ι, λ (+ soft #5343 — fallback: per-segment sweep + union for the tendon solid).
- **ν — doc-chunk updates** (project gate leaf 1). Modules: `crates/reify-mcp/src/tools/chunks/{constraints,connect,geometry,stdlib}.md`. *Signal:* chunks document at-auto+relate as the default idiom, the clearance family, `wire`, `belt_path`, `.world_frame`/`in_frame` — each signature spot-verified against the compiler registries in the task's own diff (grep evidence in the PR description); no duplication of #5389's existing-trio content. *Prereqs:* β, ζ, η, ι, λ.
- **ξ — reify-design skill cheatsheet update** (project gate leaf 2). Modules: `.claude/skills/reify-design/SKILL.md`. *Signal:* cheatsheet carries the at-auto+relate default idiom, clearance assertion recipe, belt recipe. *Prereqs:* same as ν.
- **ο — discoverability acceptance** (project gate leaf 3). *Signal:* scripted acceptance: intent queries ("place a bushing on a boss", "check parts don't collide", "belt around pulleys") against the chunk topics surface the new idioms (documented transcript committed with the task). *Prereqs:* ν, ξ.

**DAG:** α→{γ,η,ι,μ} · β→{ζ,μ,ν,ξ} · δ→μ · ε standalone · ζ→{ι,ν,ξ} · η→{θ,ι,μ} · ι→{μ,ν,ξ} · κ→λ · λ→{μ,ν,ξ} · {ν,ξ}→ο. Out-of-batch: λ←#5385 (hard) · μ←#5343 (soft, no edge — fallback documented in μ's text) · trap (b) referenced to #4388 (no edge — no leaf here depends on the ledger).

**G7 advisory walk** (normative doc `docs/legibility/design-invariants.md` not yet landed; slugs applied by judgment): no silent fail-soft — δ/ε/decision 7 exist precisely to remove them; structured-facts-at-failure — every new diagnostic carries the provenance path / pulley index / cycle path; no-lockstep-duplication — α is the single body-walk both γ and η–ι consume; contracts-machine-checked — the contact whitelist derives from model edges, not prose annotations; storm-escape / corroborate-before-acting — no detectors or snapshot-actors introduced. No waivers needed.

## 11. Out of scope

- `reify report --contacts` triage listing (follow-on named in the brief).
- Touch-vs-overlap discrimination for declared contacts; always-on/strictness-level `no_undeclared_interference` default (decision 8 records both).
- Spin-free (5-DOF) connect mates; connector-sub frame mates (decision 6 / §7.3).
- Collection-of-subs `belt_path` input surface (sub-iteration PRD; `belt_path` v2).
- Belt dynamics, tension, elasticity; non-planar belt paths (crossed/twisted 3D routing).
- BVH acceleration beyond AABB prefilter (perf follow-on if N² bites).
- The general error-diag⇒nonzero-exit invariant (silent-undef session's PRD; ε takes the relate-family slice only).
- Global-absolute pose queries across instantiations (scope-world only, decision 1).

## 12. Open questions (tactical)

1. **`W_RELATE_NO_AUTO` severity** — warning vs info when the assertion holds (B2). Suggested: warning. Decide during δ.
2. **AABB prefilter margin + sample density for curve-domain `self_clearance`** — decide during θ/ι with a measured default.
3. **`belt.ri` module name/placement** — conform to stdlib-module-arch outcome at λ time.
4. **`wrap : List<Bool>` vs a two-variant enum** — enum reads better (`Wrap.CW`); decide during λ against enum ergonomics in param lists.
5. **μ tendon-solid route** — `sweep` posed-profile vs waiting on #5343 `pipe`; decide at μ dispatch by checking #5343 status.

## 13. Notes for decompose mode

- File α–ο with `planning_mode=True`; wire the §10 DAG intra-batch edges + the out-of-batch hard edge λ←#5385 via `add_dependency` while deferred; flip the whole batch in one `commit_planning`.
- Build the capability manifest + YAML sidecar beside this PRD (`placement-relations-belt.capability-manifest.md`/`.yaml`). Binding anchors are pre-verified in §4's table (file:line as of main 366c63a679); the negative-assertion signals (B3, B5, B8, B9-FALSE arms) each need a rejection-observed probe per the overlay's D3 workflow.
- Gate-test drift-guard rule: any leaf adding a gate-resident `crates/*/tests/*.rs` or `tests/infra/test_*.sh` carries its classification/wallclock registrations in the same diff (overlay rule; esc-4914-162 precedent).
- `metadata.files`: tight-or-empty per the overlay (κ names the occt files; μ names `prj/printer_v01/printer.ri`; broad-footprint leaves file `[]`).
- Re-run the tree-sitter grammar fixtures for the §4 "no novel grammar" claim (cheap; all call/member shapes).
- Probe fixtures p2a/p2b/p2d live in an ephemeral scratchpad — leaves commit their own fixture copies; do not reference the /tmp paths in task text.
