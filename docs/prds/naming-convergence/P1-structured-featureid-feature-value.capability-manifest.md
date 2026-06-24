# P1 capability manifest — structured `FeatureId` + `Feature` value + fallible codec

> Mechanizes G3 + G6 for [`./P1-structured-featureid-feature-value.md`](./P1-structured-featureid-feature-value.md).
> One block per task; each asserted capability bound to evidence. **No novel `.ri` syntax** → the
> grammar gate is N/A (no grammar-fixture rows). All `grep:` evidence verified against `main` HEAD
> `f2e04933db` during authoring (four parallel substrate sweeps). Any `FAIL` binding blocks the batch.

**Evidence legend.** `grep:<file>:<line>` = wired on main (verified present). `producer:task-X
upstream` = capability delivered by an upstream task in the dependency closure. `rejection-check` =
G6 branch 4 (author X, observe the diagnostic fires). `field-pop` = producer writes a non-`Undef`
(non-sentinel) value on the production path. `floor` = G6 numeric pin.

DAG: `α ─┬─► β ─────────► ε` / `└─► γ ─► δ ─► ε`.

---

## α — Structured `FeatureId` enum + `DerivedKind` + accessors + `FromStr` + content-hash; full migration  *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `RealizationNodeId { entity: String, index: u32 }` exists to wrap losslessly | `grep:crates/reify-core/src/identity.rs:163` | PASS |
| `From<&RealizationNodeId>` + `derived_mid_surface` are the only constructors (signatures preserved) | `grep:crates/reify-ir/src/geometry.rs:3682` · `:3671` | PASS |
| No `FromStr`/`TryFrom`/`parse` for `FeatureId` exists today → α *introduces* the parse-back (not modifies) | workspace grep = 0 hits (substrate sweep D) | PASS |
| Migration extent is bounded & known (the 4 String-reconstruction sites + all `&FeatureId` consumers) | `grep:result.rs:556/564` · `shell_extract_compute.rs:757/815`; inventory table (sweep D) | PASS (extent covered) |
| `Role::content_hash_bytes` discipline exists to mirror (pinned discriminants, frozen, append-only) | `grep:crates/reify-ir/src/geometry.rs:3850` | PASS |
| Malformed-string rejection (`FromStr → Err`) | rejection-check: α authors `{"", "Foo", "Foo#realization[]", ".../bogus"}`, observes `Err` — mechanism delivered by α (self) | PASS |
| **I2 round-trip premise** (Display↔FromStr) holds only if `entity` excludes `#/[]/` | α confirms entity charset; **fallback named in PRD** (structured `FeatureIdOnDisk` record) if not | PASS (contingency bound, not latent) |

## β — Fallible codec + `FORMAT_VERSION` bump  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `topology_attribute_from_disk` already returns `io::Result` (so `?` fits, no signature change) | `grep:crates/reify-shell-extract/src/result.rs:554` | PASS |
| `role_from_u8` InvalidData precedent to mirror | `grep:result.rs:504-535` (rejects unknown tag → `io::ErrorKind::InvalidData`) | PASS |
| `ShellExtractionResult::FORMAT_VERSION` (=1) + pin test exist to bump | `grep:result.rs:890` · pin test `:1269` | PASS |
| **Corrupt on-disk `feature_id` → `InvalidData`** (B7, negative assertion) | rejection-check: author on-disk record w/ `feature_id="@@bad@@"`, observe `from_disk(..).unwrap_err().kind()==InvalidData`; rejection mechanism = α's fallible `FromStr` (`producer:task-α upstream`) wired into β's `?` | PASS (rejection bound) |

## γ — `Value::Feature(FeatureId)` + `Type::Feature` + all exhaustive arms  *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `FeatureId` (structured) exists to wrap | `producer:task-α upstream` | PASS |
| Every exhaustive `Value` matcher exists (compile-forced arms) | `grep:value.rs:1273,1759,1865,2045,2347,2605,2649,2924`; `reify-eval/src/lib.rs:258`; `reify-constraints/src/lib.rs:101`; `engine_eval.rs:101` (sweep B) | PASS (wired on main) |
| `content_hash` next free tag = 31 (current max Selector=30) | `grep:value.rs:1273` (sweep B) | PASS |
| Exhaustiveness oracle exists (the γ signal) | `grep:crates/reify-eval/tests/m8_m11_regression_checkpoint.rs:435` (Value) · `:373` (Type) | PASS |
| `Type::Feature` is a defensible *new* variant (no existing Type to reuse) | precedent split: `GeometryHandle→Type::Geometry` reuse vs `Selector→Type::Selector(kind)` dedicated; Feature is neither geometry nor selector (sweep B) | PASS |

## δ — Wire `Value::Feature` into the production shell-extract path  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `Value::Feature` exists to carry | `producer:task-γ upstream` (DAG γ→δ, upstream ✓) | PASS |
| shell-extract structure-instance produces the `feature_id` field on the **production** path | `grep:crates/reify-eval/src/shell_extract_compute.rs:757` · `:815` (production construction-from-String, sweep D) | PASS (wired on main) |
| `engine_admin` consumes that structure-instance | `grep:crates/reify-eval/src/engine_admin.rs:2266` · `:2317-2328` (sweep C) | PASS |
| field-population: the field carries a **real** `FeatureId` (non-`Undef`) | `field-pop grep:shell_extract_compute.rs:757` writes a constructed `FeatureId` (→ `Value::Feature`), not `Value::Undef` | PASS |
| end-to-end (G6 branch 3): all required capabilities upstream of δ | `producer:task-α,γ upstream`; `grep:crates/reify-eval/tests/topology_attribute_e2e.rs` exists | PASS (no downstream-owner inversion) |

## ε — Boundary-test suite B1–B11 (B+H integration gate)  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| All exercised capabilities delivered upstream | `producer:tasks α,β,γ,δ upstream` (DAG: all → ε) | PASS |
| B5 content-hash **golden bytes** are achievable (self-consistent pin) | `floor`: golden bytes are defined by α's pinned `content_hash_bytes`; the test pins the implementation's own output (no external accuracy claim) | PASS |
| B7 corrupt→InvalidData rejection fires | rejection-check bound via β (above) | PASS |
| B11 no-regression: existing engine e2e suite is non-synthetic & present | `grep:crates/reify-eval/tests/topology_attribute_{e2e,extrude_revolve_e2e,sweep_loft_e2e,resolver_e2e}.rs` (sweep D) | PASS |
| one-time content-hash change is safe (no persisted prod cache) | shell-extract codec has **zero** `compute_persist.rs` dispatch arms → no on-disk entries to invalidate (sweep C) | PASS |

---

**Result:** 0 FAIL bindings. The batch clears the G3+G6 manifest gate. The only premise needing
runtime confirmation (I2 entity-charset) is bound with a named structured-record fallback, so it is
a tactical contingency in α — not a latent false premise frozen into a RED test.
