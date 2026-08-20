# Capability Manifest — FEA Load/Support String→Selector Migration

Mechanizes G3 + G6 per **leaf** task for `docs/prds/v0_6/fea-load-support-selector-migration.md`. Built at decompose time (2026-06-08). Every binding must resolve to a PASS form (`grep:…wired`, `producer:task upstream-wired`, `grammar-fixture:…parses`, `parity:no-new-bound`) — any `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor` blocks the batch.

**Substrate status snapshot:** 4116/4117 **done** (selector value/type substrate + conformance + `E_SELECTOR_KIND_MISMATCH`); 4118/4119/4120 **pending** (constructors + `resolve()` + `ResolveSelector` + boundary gate); 4092 **pending** (selector→FE node-set). This PRD is **deferred** behind those; all "producer:…upstream" bindings below are wired as real `add_dependency` edges (so the leaf cannot dispatch before its producer is `done`).

## Leaf: Bmig — FEA field migration + example migration

Signal: `PressureLoad(face: faces_by_normal(b,+Z,1deg))` compiles + applies to the +Z face node set; `PressureLoad(face: body(b,"x"))` → compile-time `E_SELECTOR_KIND_MISMATCH`; migrated `examples/fea_cantilever_smoke.ri` `reify check`s clean.

| Capability asserted | Binding | Evidence / form | Verdict |
|---|---|---|---|
| Predicate ctor `faces_by_normal(b,…)` → `Value::Selector(Face)` | `producer:4118` | task 4118 (γ) re-types predicate selectors; wired dep | PASS (upstream-wired) |
| Named ctor `body(b,"x")`/`face(b,"r")` → `Value::Selector(kind)` | `producer:4119` | task 4119 (δ) named constructors; wired dep | PASS (upstream-wired) |
| `VertexSelector` + `vertex(b,"tip")` | `producer:A1` | intra-batch task A1; wired dep | PASS (upstream-wired) |
| Kind-agnostic param accepts any `Selector(k)` | `producer:A2` | intra-batch task A2; wired dep | PASS (upstream-wired) |
| `resolve(selector,kernel)→Vec<GeometryHandleId>` | `producer:4118` | task 4118 single executor; wired dep | PASS (upstream-wired) |
| Selector → FE node-set on realized mesh | `producer:4092` | task 4092 (pending); wired dep | PASS (upstream-wired) |
| `E_SELECTOR_KIND_MISMATCH` diagnostic emitted on wrong kind | `grep:reify-*/diagnostics + 4116/4117 area` | introduced by topo-selector α/β (done); re-verify on main at activation | PASS (exists; re-verify) |
| Field type change `String → selector` populates the *real* selector value (not `Undef`) into the trampoline | `grep:crates/reify-stdlib/src/loads.rs,supports.rs validate_selector_target` | the migrated structure-def field carries the constructed `Value::Selector`; `validate_selector_target` accept-set updated from opaque-String/Map → typed selector (task 3076 narrowed it) | PASS (production path; not a tests/-only construction) |
| `face(b,"top")`, `vertex(b,"tip")`, `PressureLoad(face: …)` parse | `grammar-fixture:plain-function-call + named-args + type-name-identifier` | topology-selector PRD §6/D7 — no novel syntax; `grammar_confirmed: true` | PASS (grammar N/A) |

No FAIL binding. (No new numeric claim — the elastic solve result is *parity* with the pre-migration String-based BC path; the cantilever tolerance is the existing one, so no `bound≤floor` exposure.)

## Leaf: BT — boundary-test integration gate

Signal: §5 BT1–BT7 green end-to-end (compile-fail fixtures + resolving `.ri` examples + migrated cantilever).

| Capability asserted | Binding | Evidence / form | Verdict |
|---|---|---|---|
| All §5 boundary tests exercise the migrated path | `producer:Bmig` | intra-batch task Bmig; wired dep | PASS (upstream-wired) |
| Wrong-kind compile-fail fixtures emit exactly one `E_SELECTOR_KIND_MISMATCH` | `grep:E_SELECTOR_KIND_MISMATCH (4116/4117) + producer:Bmig` | diagnostic exists; fixtures authored by BT | PASS |
| Migrated cantilever end-to-end (BT5) tip-deflection within existing FEA tolerance | `parity:no-new-bound` | the migration changes BC *specification* (String→selector), not the numerics; assert equality-to-pre-migration-result, not a new accuracy bound — no G6 bending-lock-floor exposure | PASS (parity, no new bound) |

No FAIL binding.

## Intermediate tasks (A1, A2) — not leaf, listed for completeness

- **A1** (`SelectorKind::Vertex` + `vertex()/vertices()` + `extract_vertices` + resolve arm): producer of the VertexSelector capability Bmig consumes; deps `producer:4118`, `producer:4119` (constructor + resolve machinery it mirrors). Unit-covers K1 rejection + K2 kernel-free construction.
- **A2** (kind-agnostic selector param acceptance in `type_compat`): producer of the kind-agnostic capability Bmig consumes; dep `producer:4117` (done — the selector type-compat rules it extends).

---

# 2026-07-20 residual decompose — Bmig2 + Bmig2-consume (+ re-deps 4371/4833)

Re-gated per the PRD's 2026-07-20 revision (D8/D9/D10 ratified; D11 chokepoint dependency). A1/A2/Bmig
landed (4368/4369/4370 — Bmig **narrowed** to `PressureLoad.face` only, esc-4370-24). Evidence snapshot:
branch `prd/fea-selector-conformance` @ `ef95c78bd4` (= main `56380a8f8a` + PRD ratify commit) —
**re-locate at dispatch**. Enforcement producer: the struct-ctor field-type conformance chokepoint batch
**5302–5307** (`docs/prds/struct-ctor-field-type-conformance.md`, decomposed 2026-07-20; α=5302
in-progress, **δ=5306 = the Warning→Error flip**). Rejection baselines (silent-accept on main) were
re-verified same-day by the chokepoint decompose's probe run `wf_801582f3-59d`; no fresh binary exists in
this worktree, so no new live probes were run this session — every rejection binding below is
**producer-upstream with a hard `add_dependency` edge** (G6 branch 4: the rejection is structurally
unobservable pre-chokepoint; it is asserted post-δ by the consumer leaves 4371/4833).

## Leaf: Bmig2 — complete residual field migration + disallow string (source-breaking)

Signal: `PointLoad(point: vertex(b,"tip"))` compiles; `PointLoad(point: faces(b))` →
`E_SELECTOR_KIND_MISMATCH`; a bare string at any selector field → compile error; migrated
`examples/fea_cantilever_smoke.ri` `reify check`s clean; existing FEA solve e2e stays green (interim
consumption shim).

| Capability asserted | Binding | Evidence (snapshot @ ef95c78bd4) | Verdict |
|---|---|---|---|
| The 5 residual fields are still `String` (the thing to flip) | grep:wired | `fea_multi_case.ri:316` (`point`), `:355` (`FixedSupport.target`), `:380` (`PinnedSupport.target`), `:447` (`TractionLoad.face`), `:477` (`BodyForce.body`); `:420` `face` already `Option<FaceSelector>` (4370 — untouched) | PASS |
| Required (no-default) selector params are expressible (D10) | grep:wired | no-default params are established corpus-wide (`param geometry : Solid`, `param material : Material`, …) | PASS |
| `VertexSelector` + `vertex()/vertices()` ctors | producer:4368 done | `reify-core/src/ty.rs:78` (+ `dimensionality→0` `:90`); `geometry_ops.rs:5352` `"vertex"` / `:5324` `"vertices"`; `topology_selectors.rs:1198` `extract_vertices` | PASS (wired) |
| Kind-agnostic `Selector` param (A2) | producer:4369 done | `Type::AnySelector` `ty.rs:284` (Display→`"Selector"` `:620`); `type_compat.rs:249` | PASS (wired) |
| Named/predicate ctors `face()/edge()/body()` | producer:4118/4119 done | committed production idiom in CI: `face(body, "x_max")` — `examples/fea_pressure_smoke.ri:36` | PASS (wired) |
| Wrong-kind at ctor field → `E_SELECTOR_KIND_MISMATCH` at Error | producer:5302→5306 upstream | code exists (`diagnostics.rs:582-584`); at-ctor-field re-code = chokepoint α (5302 item (c)); Error severity = δ (5306); dep edge Bmig2→5306 | PASS (upstream-wired; observation post-δ) |
| Bare string at selector field → compile error | producer:5306 upstream | disallow-string is δ's own negative fixture (`PressureLoad(face: "x_max")`); baseline silent-accept re-verified by `wf_801582f3-59d` | PASS (upstream-wired; rejection-mechanism-backed) |
| Legacy `String` transition arms exist to REMOVE (D8) | grep:wired | `extract_pressure_face_name` `elastic_static.rs:4118-4120` (`Value::String(s)` arm); `any_support_targets` `:345-353` (`Value::String` match — becomes a typed Named-name interim shim, **not** retained String acceptance) | PASS |
| Call-site extent measured, not guessed | grep:wired | **154 sites / 49 files**: `PointLoad(point:"` ×67, `FixedSupport(target:"` ×83, Pinned ×2, Traction ×2, BodyForce ×2 — regen: `git grep -E 'PointLoad\(point: "\|FixedSupport\(target: "\|PinnedSupport\(target: "\|TractionLoad\(face: "\|BodyForce\(body: "' -- examples/ crates/` | PASS |
| Grammar | fixture:committed | no novel syntax — typed-ctor named-arg call form already committed + CI-exercised (`fea_pressure_smoke.ri:36`) | PASS (N/A) |

No FAIL binding. Bmig2 asserts **check-level** signals only; the solve half of BT5 is Bmig2-consume's.

## Leaf: Bmig2-consume — truly consume selectors in the solver (D9)

Signal: a migrated FEA solve applies the BC/load at the typed-selected node-set; cantilever
tip-deflection within the EXISTING tolerance (parity — no new bound).

| Capability asserted | Binding | Evidence (snapshot @ ef95c78bd4) | Verdict |
|---|---|---|---|
| Selector → FE node-set substrate | producer:4092 done + grep:wired | `loads_supports_to_bc_node_sets` `elastic_static.rs:1666` → `target_node_set` `:1689` → `bc_resolve::boundary_node_set`; `resolve_selector_faces` `bc_resolve.rs:72`; production caller `:962` | PASS |
| `PointLoad.point` genuinely unconsumed today (the gap) | grep:wired | zero `"point"` reads in `elastic_static.rs`; `extract_loads` `:4013` consumes force/direction only; `target_node_set` reads only the literal `"target"` field, so its `kind="PointLoad"` arm (`:1674`) can never fire on `point` | PASS (gap confirmed) |
| Kernel-less FixedSupport path exists (to upgrade) | grep:wired | `any_support_targets` `:345` `Value::String` match; trampoline resolves only the 6 named box faces (`:4173`) | PASS |
| Vertex handles resolvable | producer:4368/4092 done | `topology_selectors.rs:1198` `SelectorKind::Vertex => kernel.extract_vertices(handle)`; the vertex-handle→mesh-node mapping is **this leaf's own deliverable** (`boundary_node_set` today maps face ids) — extent self-owned, not `producer-extent-short` | PASS |
| Tip-deflection bound | parity:no-new-bound | equality-to-pre-migration-result within the existing FEA tolerance; no new accuracy claim ⇒ no bending-lock-floor exposure | PASS |
| Fields selector-typed at dispatch | producer:Bmig2 intra-batch | dep edge Bmig2-consume→Bmig2 | PASS (upstream-wired) |

No FAIL binding.

## Re-deps: BT (4371) + P4-π (4833)

- **4371 (BT)** gains deps **Bmig2, Bmig2-consume, 5306** — BT3/BT4/BT5 need the migrated+consumed
  fields; BT1/BT4 negatives need the chokepoint at Error. Its original manifest section above is
  otherwise unchanged.
- **4833 (P4-π)** gains dep **Bmig2** (kind-specific pose-vs-set message on `point`/`target`); 5306 was
  already wired. Re-scoped to the chokepoint-verifier framing — see the P4 manifest's 2026-07-20
  revision section (same commit).
