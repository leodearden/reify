# Indexed Sub Instantiation (index-parametric sub collections)

Status: contract. Authored 2026-07-24 in an autonomous `/prd` session spawned from the
2026-07-24 language review (dogfood printer_v01, session a0d342d4); subject and the
two-phase static-N → dynamic-N split pre-approved by Leo in the spawn brief. Gates
G1–G7+META walked; substrate claims probe-verified against the 2026-07-22 debug binary
and `tree-sitter parse` (fixtures quoted in §2, target-surface fixture committed at
`docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri`).

---

## §0 — Purpose

Give Reify a way to place **N sub-structure instances whose constructor arguments and
poses are functions of the instance index**, with the instances individually addressable,
iterable, and realized as first-class geometry.

The motivating gap is the top expressiveness finding of the printer_v01 dogfood review:
`prj/printer_v01/printer.ri` (995 lines) hand-places **38 `LinearTendon` + 34
`IdlerPulley`** subs as one-liners with literal `transform3(...)` coordinates, e.g.:

```reify
sub corner_fr_u = IdlerPulley() at transform3(orient_identity(), vec3(idler_out_r, corner_y, z_a))
sub corner_fr_l = IdlerPulley() at transform3(orient_identity(), vec3(idler_out_r, corner_y, neg_z_a))
sub corner_fl_u = IdlerPulley() at transform3(orient_identity(), vec3(idler_out_l, corner_y, z_b))
sub corner_fl_l = IdlerPulley() at transform3(orient_identity(), vec3(idler_out_l, corner_y, neg_z_b))
```

Each block of 4–8 near-identical declarations differs only in a sign/offset pattern that
is a pure function of an index. Patterns are no substitute: `linear_pattern` /
`circular_pattern` fuse geometry into one body whose instances are unaddressable
(probe-verified: `p[0]` on a pattern result is a type error). `generate(n, |i| geometry)`
is silently broken for geometry elements (**#5385** — a separate bug about geometry
*value lists*; this PRD is about *sub-structure members* and deliberately does not
depend on `generate`).

### §0.1 — What this is NOT

- **NOT keyed membership.** `Keyed<T>` (`docs/prds/v0_6/keyed-collection-identity.md`,
  landed through task 3932) is for **author-curated** member sets where each member is
  individually written and addressed by a stable string key. Indexed collections are
  **generated** member sets where per-member variation is a function of the index. The
  two are siblings on the same collection-sub substrate; neither replaces the other.
- **NOT the geometry-list path.** `generate`/`map`/list-of-geometry evaluation (#5385)
  is a value-level bug with its own task. No task in this PRD may be closed by fixing
  or depending on `generate`.
- **NOT constraint-driven placement.** Poses here are explicit `Transform3` expressions
  of `i`. Solver-placed instances (`at auto` + `relate` per element) are a declared
  follow-up seam with the parallel placement/relations/belt PRD (§4, §8).

## §1 — Consumers (G1)

| Mechanism | Consumer (user surface or PRD) |
|---|---|
| Indexed sub declaration (`sub idlers[i in 0..4] = IdlerPulley(...) at pose(i)`) | printer_v01 idler/tendon arrays (and every array-shaped assembly). User-observable leaf: a committed `examples/` file distills the printer corner-idler + tendon-span blocks into two indexed declarations, realizes the same body set at the same world poses as the hand-placed baseline, CI-run (§7 task ζ). |
| Per-instance realization + posing | The **sub-placement surfacing walk** (`docs/prds/v0_6/sub-placement-and-surfacing.md` §4/§5): closes its §10 deferral #1 ("collection-sub per-element placement … requires per-instance realization handles"). Viewport, STEP export, interference/distance queries, mass properties all see placed instances through the existing walk — no new consumer surface invented. |
| Ordered instance collection in expression position | The parallel **placement/relations/belt PRD** (authoring 2026-07-24): `belt_path` v1 takes an explicit pulley list; an indexed collection reference supplies it (§4 seam). |
| `forall i in <range>` iteration | Cross-instance constraints (`idlers[i]` vs `idlers[i+1]` — the chained-tangency shape) and index-parametric checks; consumed by the belt PRD's per-pulley reasoning and by ordinary `.ri` authors. |
| Doc chunks + skill cheatsheet | `reify-mcp` chunks (`collections.md`, `structures.md`) and `.claude/skills/reify-design/SKILL.md` — the in-GUI assistant and the design skill must surface the idiom (project PRD gate, §7 task θ). |

**Engine-integration sub-check.** Every in-engine mechanism extends an existing seam:
schema elaboration of collection subs (`EvaluationGraph::from_templates` +
`CollectionSubInfo`), the `engine_edit.rs` collection-count re-elaboration phase (tasks
2629/2690), the sub-placement realization/surfacing walk (ApplyTransform,
`engine-integration-norm.md` §3.6 freshness walk downstream). **No new seam** is
introduced.

## §2 — Substrate reality (G3 — probe-verified 2026-07-24)

Probes run with `tree-sitter parse --quiet` (tree-sitter-reify) and the 2026-07-22
`target/debug/reify`. Fixture set preserved in the session scratchpad; target-surface
fixture committed beside this PRD.

| # | Premise | Verdict | Evidence |
|---|---|---|---|
| 1 | `sub xs : List<T>` + `constraint xs.count == N` elaborates N members with per-member value cells at positional NodeId paths | **EXISTS** | eval prints `Rig.idlers[0].od = 0.036 m`, `Rig.__count_idlers = 2`; `examples/structural_query_children_members.ri` et al. |
| 2 | Per-member **geometry** realization for collection subs | **ABSENT — silently** | eval prints `Rig.idlers[0].body = undef`, `Rig.__list_idlers__body = [undef, undef]`, zero diagnostics. The silent-undef shape the 2026-07-24 placeholder-eradication review targets; §3.6 makes the v1 replacement loud where it must be. |
| 3 | `at` on the collection arm | parses; compiler rejects **loudly** | `error: 'at' placement is not supported on collection subs; per-element placement is out of scope in v1` — the diagnostic that anticipates this PRD. |
| 4 | Target surface `sub idlers[i in 0..4] = Pulley(od: 30mm + i * 2mm) at transform3(...)` | **FAILS to parse** (ERROR at `[`) | grammar work — task α. |
| 5 | `forall v in <coll-sub> : constraint v.od > 1mm` | **EXISTS** | check prints `OK forall@v[0]`, `OK forall@v[1]` (task 2364 lowering). |
| 6 | `forall i in 0..4 : constraint ...` | parses; compiler rejects **loudly** | `error: cannot iterate over non-collection type 'Range<Int>' in forall: expected List<_> or Set<_>` — task δ lifts this. |
| 7 | Bare `idlers[0].od` in a `let` | **EXISTS** | eval prints `Rig.d0 = 0.036 m` (task 2871 indexed-access lookup). |
| 8 | `self.idlers[0].od` | **BROKEN — misrouted** | `error: Geometry has no projection '.od'` (routed into cross-sub-geometry access instead of member access). Hygiene task ε. |
| 9 | `self.idlers.count` | rejected loudly with redirect | `error: cannot access aggregation 'count' … use 'idlers.count' directly`. ε aligns #8 with this diagnostic quality. |
| 10 | Bare collection ref in value position (`let coll = idlers`) | **PARTIAL** | evaluates to a per-element list — today `[undef, undef]` (the element bodies). Becomes real when #2 lands; belt seam consumes it (§4). |
| 11 | Runtime re-elaboration on count change (warm `edit_param`) | **EXISTS** for member value cells + forall Constraint (task 2629) **and** Connect (task 2690) decls | `engine_edit.rs` collection-count phase (~1963–2400): re-elaborates `CollectionSubInfo.child_value_cells`, re-emits `forall_templates` with the `forall_emitted` ledger. Chain bodies remain compile-time silent-skip-with-info. Phase 2 extends this proven path to realization/poses; it does **not** wait on the (still-absent) SchemaNode-style re-elaboration that test task #4684 is deferred for. |
| 12 | Count cells classified Structural (morph ineligible, re-elaboration path) | **EXISTS** | `structural_classifier.rs` Rule 3 (`collection_subs.count_cell`). |
| 13 | Rigid posing primitive + composed surfacing | **EXISTS** | `ApplyTransform` + composition walk (sub-placement PRD, landed via tasks 4147/3903 lineage; grammar `at` clause present on all three sub arms). |

Every novel-syntax fragment in this PRD is either covered by fixture #4 (task α names it
as its signal) or parses today (#5, #6, #7 contexts). **G3 resolution: one grammar
prerequisite task (α); all other mechanisms extend probe-verified existing substrate.**

## §3 — Resolved design decisions

### §3.1 — Surface syntax: the indexed-instantiation form

```reify
sub idlers[i in 0..4] = IdlerPulley(sheave_od: r_pitch * 2)
    at transform3(orient_identity(), vec3(x_of(i), corner_y, z_of(i)))

sub tendons[i in 0..4] = LinearTendon(length: span(i))
    at transform3(rot_to_y, vec3(0mm, span(i) / 2, z_of(i)))
```

Grammar delta (task α): the **instantiation arm** of `sub_declaration` gains one
optional indexer clause between the name and `=`:

```
seq('[', field('binder', $.identifier), 'in', field('domain', $._expression), ']')
```

Everything after `=` is the existing instantiation grammar **unchanged** — named
ctor arguments, `at <pose>`, and (syntactically) the trailing `relate` block — with the
binder in scope throughout. The domain expression must type as `Range<Int>`
(`0..n` is existing `range_expression` grammar; `0..4` = indices 0,1,2,3, matching the
established exclusive-upper Range semantics).

Declared meaning: `idlers` is a collection sub of element type `IdlerPulley` with
derived count cell `__count_idlers = |domain|`; element `i` is the template instantiated
with the binder bound to `i`.

**Why this form** (alternatives considered and rejected):

- *Lambda-pose on the collection arm* (`sub idlers : List<IdlerPulley> at |i| pose(i)` +
  separate count constraint) parses **today** (probe #3) and needs no grammar work — but
  it answers only the pose axis. Index-dependent **ctor args** (`IdlerPulley(sheave_od:
  f(i))`) have no home, and bolting on a second lambda surface per argument would be a
  worse grammar than one indexer clause. Rejected: solves half the problem.
- *`forall i in 0..4 : sub ...`* — forall statement bodies are connect/chain/constraint;
  adding member-introducing bodies gives each iteration an unnameable declaration, leaves
  no collection entity to address (`idlers[i]`, `.count`, iteration), and puts structure
  introduction inside a statement form whose lowering was designed for per-element
  *decls about existing members*. Rejected: destroys addressability (brief Q3).
- *Count-in-type* (`List<IdlerPulley>[4]`) — a second count spelling competing with the
  established count-cell/constraint mechanism, and no binder for args. Rejected.

The indexed form and the existing bare collection form coexist: `sub xs : List<T>` +
count constraint remains valid (unposed, default-ctor members — the BOM/structural-query
use). The indexed form is the way to get per-element args/poses.

**v1 restriction (loud, not silent):** the domain must resolve to a compile-time
constant (literal bounds or const-foldable expressions of literals/defaults). A dynamic
bound in phase 1 emits `E_INDEXED_SUB_DYNAMIC_COUNT` naming phase 2 — never a silent
zero-element elaboration. Phase 2 (§3.7) lifts this.

**v1 restriction (relate):** a per-element `relate` block on an indexed sub parses (it
rides the instantiation arm) but emits `E_INDEXED_SUB_RELATE_UNSUPPORTED` in v1,
pointing at the placement/relations seam (§4). Explicit `at` transforms only.

### §3.2 — Identity: ordinal, by design

The element's **index is its identity**. NodeId paths reuse the existing positional
scheme (probe #1: `Rig.idlers[0].od` — nothing new to invent). Contract under N change
(phase 2, but fixed now because it shapes everything):

- **Grow (`0..4` → `0..5`):** instance 4 is appended; instances 0–3 keep their NodeId
  paths, cells, realizations, GUI entity paths, and warm-eval cache entries untouched.
- **Shrink (`0..4` → `0..3`):** instance 3 and everything derived from it (cells,
  realization, forall-emitted decls, meshes) is removed. Any surviving reference to a
  dropped index (`idlers[3]` in a constraint or let) becomes a **loud eval-graph
  failure** (`E_INDEXED_SUB_INDEX_OUT_OF_RANGE`), never a silent undef.
- **No renumbering, ever.** A generated collection has no "insert in the middle" edit —
  members are not individually authored, so the keyed PRD's instability problem (its §0)
  cannot arise. If a design needs author-curated stable membership, that is what
  `Keyed<T>` is for; the doc chunk states this contrast explicitly (task θ).

**No per-instance external overrides in v1.** Specializing one generated member from
outside (`idlers[2]` gets a different sheave) would reintroduce index-targeted state
whose meaning under N change needs the keyed machinery. The template is the single
source of per-element variation; in-template `match i { ... }` / conditionals are the
escape hatch. Revisit only with a concrete consumer (§8).

### §3.3 — Addressing and iteration

- `idlers[i]` (literal or expression index) in expression position; `idlers[i].<member>`
  chains — probe #7 shows the bare path works today; task ε fixes the `self.`-prefixed
  misroute (probe #8) so both spellings behave identically (work, or emit the same
  redirect diagnostic as `.count` — probe #9 — decided at ε; silent divergence is the
  only forbidden outcome).
- `idlers.count` — existing count-cell read, unchanged.
- `forall v in idlers : constraint/connect/chain ...` — existing statement forms
  (probe #5), now over indexed collections too (same lowering; the collection is a
  collection sub either way).
- **New: range iteration** (task δ): `forall i in <Range<Int>> : <statement body>` and
  the `forall`/`exists` quantifier expressions accept `Range<Int>` domains, binding the
  index as `Int`. Literal bounds unroll at compile time exactly like literal-count
  collections; the per-element replacement is the binder's literal value. This is what
  makes cross-instance constraints expressible:

  ```reify
  forall i in 0..3 : constraint distance(idlers[i].axis, idlers[i + 1].axis) > min_gap
  ```

  Non-literal bounds in phase 1 get the same loud `E_INDEXED_SUB_DYNAMIC_COUNT`
  treatment as §3.1 (one diagnostic, one phase-2 lift for both sites).

### §3.4 — Per-instance realization and posing

The heart of the PRD — closes sub-placement §10 deferral #1 and replaces probe #2's
silent undef:

1. Element `i`'s geometry-typed members realize exactly as a single sub's do today,
   producing **per-instance realization handles** (element cells already exist; the
   realization walk gains the per-element iteration).
2. The element pose is the evaluated `at` expression with the binder bound to `i`
   (a rigid `Transform3`; `Frame` lowers per the sub-placement convention). Absent
   `at`, identity — all instances coincide, which is legal but almost always wrong;
   `W_INDEXED_SUB_NO_POSE` warns when an indexed sub with count > 1 has no `at` clause
   and no per-element pose variation.
3. Posing/surfacing/export go through the **existing** ApplyTransform + composed
   containment walk (sub-placement §4/§5). Instances participate in STEP export,
   interference/distance queries, mass properties, and the viewport exactly as N
   hand-written subs would — that equivalence is the integration gate's signal (ζ).
4. `Rig.idlers[i].body` in eval output shows a real geometry handle; the
   `__list_<name>__<member>` aggregate mirrors it. The undef-elements state (probe #2)
   ceases to exist for geometry members of indexed subs.

Cold elaboration and warm re-elaboration **share one expansion helper** (template ×
index → cells/pose/realization requests). One implementation, called from
`from_templates` and from the `engine_edit.rs` collection-count phase
(no-lockstep-duplication, G7 INV-5).

### §3.5 — Count interplay

The indexed form derives `__count_<name>` from the domain (v1: a compile-time constant;
phase 2: a dependent cell). A user-written `constraint <name>.count == <expr>` on an
indexed sub stays legal and is checked like any constraint against the derived value —
it does not *drive* elaboration (the domain does). The classifier needs no change
(probe #12: count cells are already Structural).

### §3.6 — Loud-failure policy

Aligned with the 2026-07-24 silent-undef eradication review: every unsupported or
out-of-domain condition in this PRD's surface has a **named diagnostic** —
`E_INDEXED_SUB_DYNAMIC_COUNT` (§3.1, §3.3), `E_INDEXED_SUB_RELATE_UNSUPPORTED` (§3.1),
`E_INDEXED_SUB_INDEX_OUT_OF_RANGE` (§3.2), `W_INDEXED_SUB_NO_POSE` (§3.4). No code path
introduced by this PRD may materialize undef silently; task-level tests assert each
diagnostic **fires** (G6 branch-4: the rejection is observed, not assumed).

### §3.7 — Phase 2: dynamic N (own phase, own risks)

Static-N (phase 1) delivers full value alone — all printer_v01 counts are architectural
constants. Phase 2 makes the domain bound a live parameter:

- `sub idlers[i in 0..n_idlers]` with `param n_idlers : Int` — the derived count cell
  depends on the param; a warm `edit_param` rides the **existing, landed**
  collection-count re-elaboration phase (probe #11), extended to (a) run the §3.4
  expansion helper for added/removed indices, (b) evict dropped-tail realizations and
  meshes, (c) re-emit range-`forall` templates whose bounds involve the edited cell
  (mirroring the `count_cell`-keyed `forall_templates` ledger).
- Identity continuity per §3.2: grow appends, shrink truncates, surviving instances
  keep NodeIds/entity paths/cache entries — asserted by the phase-2 integration gate.
- **Out of scope even in phase 2:** solver-driven counts (`n_idlers` as an `auto`).
  Integer/discrete solve is under active investigation (2026-07-24 discrete-solve
  spike: CpSat unwired, discrete enumeration inexpressible); gating count changes on
  solver output is that program's territory. The count source here is params/defaults
  edited by a human or the GUI.

Known phase-2 risks, stated honestly: eviction correctness (dropped instances must not
leak realization handles or stale meshes), interaction with `_merge`-style warm caches,
and GUI selection continuity across count edits. Each has a named task (§7) rather than
an assumed outcome. Related-but-not-blocking: #4684 (deferred test task waiting on
SchemaNode-style re-elaboration) — phase 2 deliberately builds on the proven
`engine_edit` count phase instead of waiting for that machinery.

## §4 — Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/sub-placement-and-surfacing.md` | consumes + closes | ApplyTransform + composed surfacing walk; this PRD closes its §10 deferral #1 (per-element placement / per-instance realization handles) | **this PRD** (the deferral was explicitly parked for a successor) | queued (γ) |
| `v0_6/keyed-collection-identity.md` | sibling on shared substrate | `CollectionSubInfo`/`KeyedSubInfo` elaboration path, structural classifier; ordinal-vs-keyed identity contrast documented in chunks | this PRD (no keyed behavior change; contrast text in θ) | wired (landed substrate) |
| Placement/relations/belt PRD (parallel session, 2026-07-24) | produces to it | **This PRD owns** the indexed-collection surface: `idlers[i]` addressing, `forall i in range` iteration, ordered collection-in-expression-position (probe #10). **They own** the world-pose substrate (`.world_frame`, poses into snapshots/queries), `relate` idiom completion, and `belt_path` itself (explicit pulley list v1). Per-element `relate`/`at auto` on indexed instances = follow-up gated on their world-pose substrate, owned by a joint follow-up PRD (§8) — neither PRD lands it unilaterally. | split as stated; no reciprocal ambiguity | parallel-authoring; seam declared identically in their brief ("coordinate the collection-of-subs surface, don't redesign") |
| #5385 (generate-of-geometry silent undef) | disjoint | none — value-list path vs member path; both PRD and task text carry the mutual disclaimer | n/a | filed, pending |
| `v0_6/tuple-type.md` | optional future consume | `(i, v)` pair-binder iteration over indexed collections — same deferral the keyed PRD made (its §3); range iteration (δ) covers the index-needing cases without tuples | tuple-type, if/when | deferred, off critical path |
| Discrete-solve program (spike 2026-07-24) | boundary declared | solver-driven counts excluded (§3.7) | discrete-solve program | out of scope here |

No contested pairs touched (overlay §G4 list checked: persistent-naming, OpenVDB,
topology-selectors all untouched).

## §5 — Approach: B + H (G5)

Blast radius: tree-sitter-reify, reify-ast, reify-syntax (hand parser), reify-compiler,
reify-eval, GUI — ≥ 5 crates; touches the **grammar/parser** load-bearing seam and
elaboration identity. B + H. The H component is §3's elaboration contract (expansion
helper, identity rules, diagnostics) plus the §6 boundary-test sketch; the integration
gate (ζ) names §6 rows as its signal.

## §6 — Boundary-test sketch (H)

### 6.1 compiler ↔ eval (template expansion)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Indexed declaration elaborates | α+β | `sub ps[i in 0..3] = Pulley(od: 20mm + i * 2mm)` yields cells `ps[0].od = 20mm`, `ps[1].od = 22mm`, `ps[2].od = 24mm`; `ps.count == 3`. |
| Binder scoping | α+β | binder `i` resolves inside ctor args and pose; referencing `i` outside the declaration is an unresolved-name error. |
| Dynamic bound rejected loudly (v1) | β | `[i in 0..n]` with `param n` ⇒ `E_INDEXED_SUB_DYNAMIC_COUNT` observed; zero silent elements. |
| relate block rejected loudly (v1) | β | indexed sub with `relate {}` ⇒ `E_INDEXED_SUB_RELATE_UNSUPPORTED` observed. |
| Range forall unrolls | δ | `forall i in 0..2 : constraint ps[i].od < ps[i + 1].od` emits 2 per-element constraints (`forall@i[0..1]`), all checked. |

### 6.2 eval ↔ kernel/surfacing (per-instance realization)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Instances realize + pose | γ | eval shows `ps[i].body` = real handle (not undef); `reify build` mesh-dump has one mesh per instance at golden per-index AABBs. |
| Parity with hand-placed baseline | γ (gate ζ) | indexed file and its hand-written N-sub twin produce the same body count, per-body AABBs and volumes within kernel tolerance, in STEP export and mesh dump. |
| No-pose warning | γ | count > 1, no `at` ⇒ `W_INDEXED_SUB_NO_POSE` observed. |
| Queries see posed instances | γ | `distance(ps[0].body, ps[1].body)` equals the composed-transform expected value. |

### 6.3 warm edit ↔ identity (phase 2)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Grow keeps survivors | σ1+σ2 | `n 3→4`: `ps[0..2]` NodeIds/entity paths/realizations unchanged (cache hits asserted); `ps[3]` appears posed. |
| Shrink truncates loudly | σ1+σ2 | `n 4→3`: `ps[3]` cells/mesh/realization gone; a `let` referencing `ps[3]` ⇒ `E_INDEXED_SUB_INDEX_OUT_OF_RANGE`; no undef. |
| Range-forall re-elaborates | σ3 | count edit re-emits per-element range-forall decls via the template ledger (constraint + connect arms). |

### 6.4 this PRD ↔ belt PRD (collection surface)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Collection consumable as ordered list | γ | `let bodies = idlers` yields the per-instance list in declaration order with real elements (today `[undef, undef]` — probe #10); indexing/iteration order == index order. |

## §7 — Decomposition plan

Labels α…θ (phase 1), σ1…σ3 (phase 2); real ids at decompose time. Every diagnostic
signal below means *the test observes the diagnostic fire* (G6 branch 4). No task adds
a new standalone gate-resident test file — assertions extend the existing suites
(`forall_statement_lower_tests.rs`, the reify-eval collection/engine-edit tests, the
examples CI runner), so the overlay's drift-guard registration trigger is not tripped;
any task that ends up needing a new gate-resident file must carry the manifest/nextest
registrations in its own diff per CLAUDE.md.

### Phase 1 — static N

- **α — Grammar + parsers: indexer clause on the instantiation arm.**
  tree-sitter production (§3.1 delta), hand-parser (reify-syntax) support, AST
  `SubDecl` binder/domain fields. Signal: committed fixture
  `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri` parses
  (`tree-sitter parse --quiet` exit 0, 0 ERROR nodes) — it fails today (probe #4);
  corpus test pins the CST; the three existing sub arms unaffected (regression corpus
  green). `grammar_confirmed=false` (this IS the grammar work). Crates:
  tree-sitter-reify, reify-syntax, reify-ast.
- **β — Compiler lowering: indexed template → collection-sub expansion.** Depends α.
  Binder-scoped compilation of ctor args + pose; derived count cell; the shared
  expansion helper (§3.4) on the compile path; `E_INDEXED_SUB_DYNAMIC_COUNT`,
  `E_INDEXED_SUB_RELATE_UNSUPPORTED`. Signal: `reify check`/`eval` on a 3-element
  indexed sub shows per-index arg values (§6.1 row 1); both diagnostics observed on
  negative fixtures. Intermediate — unlocks γ, δ, ε. Crates: reify-compiler.
- **γ — Per-instance realization + posing + surfacing.** Depends β. §3.4 items 1–4;
  `W_INDEXED_SUB_NO_POSE`. Signal: §6.2 rows 1, 3, 4 — eval shows real `body` handles;
  `reify build` mesh-dump/STEP contain N instances at golden AABBs; warning observed.
  Crates: reify-eval (+ kernel walk touchpoints).
- **δ — Range iteration in forall/exists.** Depends β (independent of γ). `Range<Int>`
  domains in the forall statement lowering (literal-bound unroll, per-element binder
  substitution) and quantifier expressions (`Int` element type). Signal: §6.1 row 5 —
  the neighbor-constraint fixture checks green with per-element `OK forall@i[k]` lines;
  non-literal bound ⇒ `E_INDEXED_SUB_DYNAMIC_COUNT` observed. Crates: reify-compiler.
- **ε — Addressing hygiene: `self.`-path parity + out-of-range access.** Depends β.
  Fix probe #8's misroute (`self.<coll>[k].<member>`), align with probe #9's redirect
  quality; literal out-of-range index on a static count ⇒ compile-time error, computed
  index ⇒ `E_INDEXED_SUB_INDEX_OUT_OF_RANGE` at eval. Signal: the probe-#8 fixture
  stops mis-diagnosing (either evaluates equal to the bare path or emits the redirect);
  out-of-range diagnostics observed. Crates: reify-compiler, reify-eval.
- **ζ — Integration gate: printer-pattern example, parity-verified.** Depends γ, δ, ε.
  `examples/indexed_idler_array.ri`: the printer corner-idler + tendon-span pattern (4
  `IdlerPulley` + 4 `LinearTendon` as two indexed declarations with sign/offset
  functions of `i`, replacing 8 hand-placed one-liners) **plus** its hand-placed twin
  structure in the same file; asserts §6.2 row 2 parity (same body count, per-body
  AABB/volume within kernel tolerance in mesh dump + STEP) and §6.4 (ordered collection
  value). CI-run example. **Leaf — the phase-1 integration gate.**
- **η — GUI: instance arrays in tree + inspector.** Depends γ. Collection node with
  indexed children (`idlers[0]`…) using the existing entity-path scheme; inspector
  shows the instance's resolved params/pose read-only. Signal: reify-debug MCP —
  `viewport_state.meshCount` counts N instance meshes; selecting `idlers[2]` in the
  tree highlights exactly instance 2's mesh (screenshot + store_state assertion).
  Crates: gui/src-tauri + frontend.
- **θ — Docs, chunks, skill, discoverability (project PRD gate).** Depends ζ.
  Spec update (§3.4 collection subs gain the indexed form; §4.7 `sub`; §15 grammar);
  `reify-mcp` chunks `collections.md` + `structures.md` updated with
  **registry-verified signatures** and the indexed-vs-keyed contrast (§3.2);
  `.claude/skills/reify-design/SKILL.md` cheatsheet gains the idiom (indexed arrays +
  range forall + the "patterns fuse geometry; indexed subs stay addressable" rule).
  Signal — discoverability acceptance: querying the chunk corpus by intent ("place N
  pulleys in a row", "array of subs with varying position") surfaces the indexed-sub
  chunk section; chunk lint/signature check green; spec `.ri` snippets parse.

### Phase 2 — dynamic N (files with phase-1 batch, depends on its gate)

- **σ1 — Dynamic domain bound → warm re-elaboration of instances.** Depends ζ.
  Lift `E_INDEXED_SUB_DYNAMIC_COUNT` for param-driven bounds; expansion helper joins
  the `engine_edit.rs` collection-count phase; tail eviction of realizations/meshes.
  Signal: GUI/debug-MCP or eval-harness count edit `3→4→2` shows §6.3 rows 1–2
  behavior, diagnostics observed for dangling refs. Crates: reify-compiler, reify-eval.
- **σ2 — Identity/GUI continuity under count edits.** Depends σ1, η. Entity-path
  stability for survivors (selection/visibility persist across a grow), mesh removal on
  shrink. Signal: debug-MCP scenario — select `idlers[1]`, edit count 4→5→3, selection
  survives while `idlers[4]`/`idlers[3]` meshes appear/vanish; `meshCount` tracks.
  **Leaf — the phase-2 integration gate.**
- **σ3 — Range-forall dynamic bounds.** Depends σ1, δ. Range templates keyed on their
  bound cells in the `forall_templates` ledger (constraint + connect arms — chain stays
  compile-time-info, matching collection forall). Signal: count edit re-emits
  per-element decls (§6.3 row 3), asserted in the engine-edit test suite alongside the
  2629/2690 twins.

### Dependency view

```
α → β ─┬─→ γ ─┬────────────→ ζ ─→ θ
       ├─→ δ ─┤              ↑
       └─→ ε ─┘              │
             γ ─→ η ─────────┤ (η feeds σ2, not ζ)
ζ ─→ σ1 ─┬─→ σ2 (gate 2, also ← η)
         └─→ σ3 (also ← δ)
```

## §8 — Out of scope

- **Per-element `relate` / `at auto` on indexed instances** (solver-placed arrays).
  Deferred to a joint follow-up with the placement/relations PRD once its world-pose
  substrate lands; v1 = the named diagnostic (§3.1). The belt case does not need it:
  `belt_path` v1 consumes explicitly-posed pulley lists.
- **Per-instance external overrides** on generated members (§3.2) — use in-template
  conditionals or `Keyed<T>`.
- **Solver-driven counts** (`auto` N) — discrete-solve program territory (§3.7).
- **`(i, v)` pair-binder** iteration over collections — tuple-type PRD (same deferral
  as keyed §3); range iteration covers the indexed cases.
- **`generate`/geometry-value-list repair** — #5385.
- **Nested indexed subs / multi-dimensional index domains** (`[i in .., j in ..]`) —
  no consumer named today; a future PRD may generalize the domain clause.
- **Pattern-op changes** — `linear_pattern`/`circular_pattern` stay as-is (fused-body
  bulk features); chunk text contrasts the two idioms (θ).

## §9 — Open questions (tactical)

1. **Binder spelling for count-only arrays** — allow omitting the binder
   (`sub legs[in 0..4]` or `[4]`) when args/pose don't use `i`? Suggested: no — one
   form, `W_UNUSED` conventions apply to an unused binder. Decide at α.
2. **`W_INDEXED_SUB_NO_POSE` exact trigger** — warn on "no `at` clause" only, or also
   on a pose expression that doesn't reference the binder (constant pose for all
   instances)? Suggested: both, second as a distinct note. Decide at γ.
3. **Out-of-range literal index** — compile error vs eval-graph failure when count is
   static and index is a literal (ε): suggested compile-time (matches keyed PRD's
   literal-key plan). Decide at ε.
4. **Mesh-dump golden format for ζ parity** — reuse the sub-placement T5 golden-AABB
   harness or add a body-set comparator. Decide at ζ.
5. **Entity-path rendering of indices in the GUI tree** (`idlers[2]` vs `idlers · 2`).
   Decide at η with existing outline conventions.

---

*Decompose note:* file with `planning_mode=True`, per-task `user_observable_signal` /
`consumer_ref` / `grammar_confirmed` metadata, wire the §7 edges via `add_dependency`
(σ tasks depend on the phase-1 gate — real edges, not prose), commit the capability
manifest + YAML sidecar beside this PRD, flip the batch with one `commit_planning`.
