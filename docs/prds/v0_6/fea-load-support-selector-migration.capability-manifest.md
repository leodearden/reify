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
