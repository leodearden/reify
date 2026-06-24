# P0 — Capability manifest

Mechanizes G3 + G6 per leaf for `./P0-region-reference-layer-model.md`. Each binding pairs an
asserted capability with evidence verified against current `main` (2026-06-24). Any **FAIL** blocks
the batch. Evidence is `grep:<file>:<line>` (wired on main) or `producer:task-<label>` (in the
transitive dependency closure). Empty-value sentinel for reify = `Value::Undef`.

**G3-grammar:** P0 introduces **no novel syntax** (D5). The two fixture forms the leaf signals rely
on were grammar-gated 2026-06-24: `faces_by_normal(body, vec3(...), 1deg)` and `#kernel(fidget)` +
`faces(body)` — both `tree-sitter parse --quiet` **exit 0, 0 ERROR nodes**. No grammar-producer task
is required.

---

## task α — Region-reference contract realized in code (intermediate)

| Capability | Evidence | Verdict |
|---|---|---|
| `Value::Selector(SelectorValue)` exists to declare canonical | `grep:crates/reify-ir/src/value.rs:1056` (`Selector(SelectorValue)`) | **PASS** (wired; task 4116) |
| `Type::Selector(SelectorKind)` / `SelectorKind` in core to reframe | `grep:crates/reify-core/src/ty.rs:39` (enum), `:53` (`dimensionality()` already 0/1/2/3) | **PASS** |
| Content hash excludes ephemeral `kernel_handle` (re-eval stability claim) | `grep:crates/reify-ir/src/value.rs:724` (`hash_ghr` — `kernel_handle excluded`) | **PASS** |
| Reserved type names to reframe as dimensionality views | `grep:crates/reify-compiler/src/type_resolution.rs:578-589` | **PASS** |

No novel substrate; α is a reframe/alias/doc change over existing types. DAG-direction: α is upstream
of β/γ. **PASS.**

---

## task β — Fail-closed region/selector resolution per representation (LEAF)

**Signal:** resolving a function-call selector over a non-BRep-realized body → `reify eval`/`check`
emits `E_QUERY_NOT_SUPPORTED_ON_REPR` + cell `Value::Undef`. **Assertion class: negative/rejection
(G6 branch-4).**

| Capability | Evidence | Verdict |
|---|---|---|
| The rejection mechanism exists and fires (anti-silent-accept) | `rejection-check:E_QUERY_NOT_SUPPORTED_ON_REPR` — **live**: `grep:crates/reify-eval/src/geometry_ops.rs:57-143` (`gate_query_capability` emits exactly one `Diagnostic::error(...).with_code(QueryNotSupportedOnRepr)` then routes `Unsupported → None → Value::Undef`); diagnostic code `grep:crates/reify-core/src/diagnostics.rs:1402`; already pinned for geometry queries `grep:crates/reify-eval/tests/query_capability_gating.rs:127` | **PASS** (mechanism live; β extends it to the region path) |
| The region/selector resolution dispatch site to route through the gate | `grep:crates/reify-eval/src/topology_selectors.rs:1487` (match on `SelectorKind` → kernel `extract_*`; today returns generic `QueryError::QueryFailed`) | **PASS** (the site to convert) |
| Non-BRep kernels return a *failure* to convert (not a fake) | `grep:crates/reify-ir/src/geometry.rs:3194` (trait-default `extract_*` → `Err("topology extraction not supported by this kernel")`); Fidget/OpenVDB/Gmsh use the default | **PASS** |
| `Value::Undef` is the fall-through sentinel on `Unsupported` | doc-contract `grep:crates/reify-core/src/diagnostics.rs:1391-1401` (`Unsupported → None → Value::Undef`) | **PASS** |

DAG-direction: producer (the gate) is **live**, β is the consumer wiring; β depends on α (upstream).
No numeric bound asserted (G6 branches 1/2 N/A). **PASS.**

---

## task γ — Two-way region-resolution boundary test (LEAF; integration gate)

**Signal:** committed boundary-test file under `crates/reify-eval/tests/` whose §6 rows pass.

| Capability | Evidence | Verdict |
|---|---|---|
| Engine-level integration test harness exists | `grep:crates/reify-eval/tests/` (existing dir; e.g. `query_capability_gating.rs`, `topology_selector_runtime.rs`) | **PASS** |
| OCCT + Manifold resolve (producer rows) | `grep:crates/reify-kernel-occt/src/handle.rs:1818` (`extract_faces`), `grep:crates/reify-kernel-manifold/src/kernel.rs:729` (coalesce) | **PASS** |
| Fidget/OpenVDB/Gmsh fail-closed (producer rows) | trait-default `Err` `grep:crates/reify-ir/src/geometry.rs:3194`; per-kernel confirmed in §2 evidence | **PASS** |
| `@point` → eager `Value::Frame`, kernel-free (consumer row) | `grep:crates/reify-expr/src/lib.rs:1194` | **PASS** |
| Construct-time kind discipline (FaceSelector vs BodySelector = 2- vs 3-manifold) | `grep:crates/reify-compiler/src/type_compat.rs` (selector-kind coercion rules) | **PASS** |

Depends on α, β (upstream). **PASS.**

---

## task δ — Drop user-label orphan helpers (LEAF)

**Signal:** `reify-audit` reports the C-10 `selector_vocabulary_v2` cluster shrunk by 2; suite green.

| Capability | Evidence | Verdict |
|---|---|---|
| `has_user_label`/`user_label_eq` exist and are removable | `grep:crates/reify-eval/src/selector_vocabulary_v2.rs:774` / `:799` | **PASS** |
| They are test-only orphans (net-zero behaviour) | callers only in `crates/reify-eval/tests/selector_vocabulary_v2_mock.rs` + `_e2e.rs`; re-exports `crates/reify-eval/src/lib.rs:118/119` — **zero production callers** (verified C-10) | **PASS** (`test-only` is the removal warrant, not a failure here) |
| Removal does **not** touch the entangled substrate (§7 / P2) | helpers are isolated from `TopologyAttribute.user_label` (`geometry.rs:3903`), the resolver branch (`topology_attribute_resolver.rs:220`), and `LeafQuery::Named` — those are explicitly **excluded** | **PASS** |

No premise beyond "verified dead." **PASS.**

---

## task ε — Spec + v0_2 PRD prose corrections (LEAF)

**Signal:** edits + cross-refs present; doc-lint passes.

| Capability | Evidence | Verdict |
|---|---|---|
| Spec §6.1.3 (ad-hoc selector table) exists to correct | `grep:docs/reify-language-spec.md` §6.1.3 (`@face`/`@region`/`@point`/`@edge`/`@body` table) | **PASS** |
| Spec §8.12 (reserved type names) exists to reframe | `grep:docs/reify-language-spec.md` §8.12 (`Selector`/`FaceSelector`/`EdgeSelector`/`BodySelector`) | **PASS** |
| v0_2 persistent-naming sigil-zoo + user-label surface exist to supersede | `grep:docs/prds/v0_2/persistent-naming-v2.md:81-89` (selector-vocabulary-v2 sigils) + the `name="..."` user-label surface | **PASS** |

Docs-only; no runtime capability. **PASS.**

---

## Summary

All leaf bindings **PASS**. No FAIL (`declared-only` / `test-only`-as-orphan / `producer-absent` /
`producer-downstream` / `fixture-ERROR` / `bound≤floor` / `rejection-absent`). The keystone
rejection capability (`E_QUERY_NOT_SUPPORTED_ON_REPR`) is **live code** extended to a new dispatch
path, not a fiction — the highest-risk binding (β's negative assertion) is backed by a wired,
test-pinned mechanism. Batch is clear to queue.
