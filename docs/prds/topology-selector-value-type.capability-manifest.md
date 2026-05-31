# Capability manifest — topology-selector-value-type

Mechanizes G3 + G6 per leaf for `docs/prds/topology-selector-value-type.md`.
Built at decompose time (2026-05-31). Evidence verified against main @ `45472174cf`.

**Empty-value sentinel:** `Value::Undef`. **Grammar:** N/A (constructors are plain
function calls — no novel syntax; D7/§6). **Numeric floors:** N/A (no numeric/exactness
premises — resolution is deferred, not approximated).

Legend: PASS = exists+wired / `producer:task-X upstream`. FAIL values that block:
`declared-only · test-only · producer-downstream · producer-absent · fixture-ERROR · bound≤floor`.

---

## α — Selector value + type substrate  (intermediate; unlocks β/γ/δ; roped to ε)

| Capability | Evidence | Verdict |
|---|---|---|
| `Value` enum extensible | `grep:crates/reify-ir/src/value.rs` (29 variants; `GeometryHandle` payload @593–601 reused as the leaf `target`) | PASS |
| `Type` enum extensible | `grep:crates/reify-core/src/ty.rs:30` | PASS |
| `SourceSpan`/`Diagnostic` for the K1 construct-time rejection | `grep:crates/reify-core` (Diagnostic), used pervasively | PASS |
| New `Value::Selector`/`Type::Selector`/`SelectorKind` = this task's deliverable | consumers β/γ/δ/ε are **downstream in-batch** → correct DAG direction (α upstream) | PASS (not orphan) |

## β — type-name resolution + conformance + coercion-acceptance  (intermediate; unlocks γ/δ)

| Capability | Evidence | Verdict |
|---|---|---|
| `type_compatible` (compile-time conformance) wired into overload resolution | `grep:crates/reify-compiler/src/type_compat.rs:220` (`type_compatible`) + `:281` (`resolve_function_overload`) — production path | PASS (wired-on-main) |
| `value_type_kind_matches` (runtime conformance) | `grep:crates/reify-eval/src/lib.rs:209` — production | PASS |
| identifier-type-annotation → `Type` resolver to extend (`FaceSelector` → `Type::Selector(Face)`) | existing type-annotation resolution accepts identifier type names; this task adds the three mappings | PASS (extends existing) |
| `Type::Selector` producer | `producer:α upstream` | PASS |

## γ — predicate constructors + `resolve()` + `ResolveSelector` coercion  (intermediate; unlocks ε; **own user-observable signal**)

**Signal (re-scoped at decompose — see G6 note):** a committed `.ri` fixture exercises a
predicate selector through `Selector → List<Geometry>` coercion and `single(...)` / `fillet(...)`
realizes the **asserted** geometry (fixture golden); the working trio (`closest_point`/`is_on`/
`angle_between_surfaces`, task 2324 *done*) and the `*_with_tags` resolver unit tests stay green.

| Capability | Evidence | Verdict |
|---|---|---|
| predicate resolvers `faces_by_normal`/`faces_by_area`/`edges_by_length`/`edges_at_height`/`edges_parallel_to` | `grep:crates/reify-eval/src/topology_selectors.rs:573/349/272/777/637`; recognized by production dispatch `geometry_ops.rs:try_eval_topology_selector` | PASS (wired-on-main) |
| coercion consumer `single` (takes `List`) | `grep:crates/reify-stdlib/src/list.rs:23` — production builtin | PASS |
| coercion consumer `fillet` | `grep:crates/reify-compiler/src/geometry_modify.rs:115`; edge-arg `List<Geometry>` (Solid/Surface/Curve all `Type::Geometry`) confirm at impl — **non-blocking** (`single` is the confirmed coercion target) | PASS |
| `ResolveSelector` IR node + compiler insertion + eval arm = this task's deliverable | `producer:γ`; consumed by its own eval arm + δ/ε in-batch | PASS (not orphan) |
| **G6 field-population / branch-3** — `resolve()` must not inherit an `Undef` baseline | predicate List-selectors currently fall through to the unimplemented complement arm (`geometry_ops.rs` ~1894+; task **2691 cancelled**) ⇒ baseline cell = `Value::Undef`. **Resolved:** signal re-scoped to forward-looking fixture-golden (above), not "identical to baseline". `resolve()` returns real handles via `topology_selectors.rs`. | PASS (after re-scope) |

## δ — composition algebra + Named constructors  (intermediate; unlocks ε; **own user-observable signal**)

**Signal:** `.ri` example `union(faces(b), edges(b))` → exactly one `E_SELECTOR_KIND_MISMATCH`
(names both kinds, span at `union`); same-kind `union`/`intersect`/`difference` resolve to the
set result (canonical-ordered, dedup'd).

| Capability | Evidence | Verdict |
|---|---|---|
| composition over `Value::Selector` + K1 kind closure | `producer:α` (the value + kind); new combinator code | PASS |
| `E_SELECTOR_KIND_MISMATCH` diagnostic code | `DiagnosticCode` enum extensible + production-emitted via `Diagnostic` — precedent `TopologyTagStale` @ `crates/reify-types/src/diagnostics.rs` | PASS (new code, established mechanism) |
| Named-leaf interim resolution | `grep:crates/reify-eval/src/topology_selectors.rs:864` (`resolve_unique_by_tag`); full name→handle is the **soft seam** to persistent-naming-v2 (D8 / G4) — non-blocking, interim `W_TOPOLOGY_TAG_STALE` | PASS (interim path exists) |

## ε — boundary-test integration gate (G5 H)  (**leaf**)

**Signal:** §5 BT1–BT8 green end-to-end (both producer + consumer sides).

| Capability | Evidence | Verdict |
|---|---|---|
| every asserted capability (types, conformance, resolve, coercion, composition, kind errors) | `producer:γ, δ upstream` (and transitively α/β) | PASS (all upstream-in-batch) |
| `tests/` `.ri` fixture harness | existing eval test-harness pattern (`crates/reify-eval/tests/*`) | PASS |

## ζ — companion prose corrections  (**leaf**)

**Signal:** `fea_multi_case.ri` field comments + root `topology-selectors.md` cite this PRD's
`String → FaceSelector`/`BodySelector` migration path; no code change.

| Capability | Evidence | Verdict |
|---|---|---|
| target files exist | `grep:crates/reify-compiler/stdlib/fea_multi_case.ri:336/382`; `docs/prds/topology-selectors.md` | PASS |

---

**Gate result:** no binding resolves to a blocking FAIL value. One G6 resolution applied at
decompose: **γ's signal re-scoped** off the `Undef`-baseline premise (task 2691 cancelled) to a
forward-looking fixture-golden. No prerequisite tasks needed to be queued; all dependencies are
intra-batch. Soft seam to persistent-naming-v2 (Named resolution, D8) is intentionally **not** a
hard dependency edge — interim behavior keeps the batch self-contained.
