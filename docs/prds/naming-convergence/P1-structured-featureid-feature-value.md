# P1 — Structured `FeatureId` + first-class `Feature` value + fallible codec

> **Status:** active (Wave 1, independent foundation). Naming & Selection Convergence program,
> P1 of P0–P4. Date: 2026-06-24. Approach **B + H** (contract + two-way boundary tests).
> Evidence base: [`./00-findings.md`](./00-findings.md) §2 (stringly-typed model). Authored from
> [`./P1-structured-featureid-feature-value.brief.md`](./P1-structured-featureid-feature-value.brief.md).
>
> **Do NOT touch task 3523 or esc-3523-75/76** — the `/unblock 3523` session owns them.

## Goal

Replace the worst stringly-typed offender in the geometry subsystem — `FeatureId(String)` — with a
**structured, lossless** representation, promote it to a first-class `Value::Feature` (+ `Type::Feature`)
value, and make its on-disk (de)serialization **fallible + validating** so corrupt ids are rejected
at `InvalidData` instead of silently constructing a valid-looking id. This is a **foundation
refactor**: the user-facing query surface (`feature()` accessor + provenance selectors) is **out of
scope** and owned by **P3**. P1 fixes the *representation* before P3 surfaces it.

No new `.ri` surface syntax is introduced — this is internal Rust type work.

## Background

`FeatureId(String)` (`crates/reify-ir/src/geometry.rs:3653`) is the worst stringly-typed type in the
subsystem (findings §2). It:
- **Lossily flattens** the structured `RealizationNodeId { entity: String, index: u32 }`
  (`crates/reify-core/src/identity.rs:163`) via `id.to_string()` in `From<&RealizationNodeId>`
  (`geometry.rs:3682`), producing `"Foo#realization[0]"`.
- **Has no parse-back** — no `FromStr`, `TryFrom`, accessor, or `.as_str()`; the entire identity
  surface is the inner `String` (derived `PartialEq`/`Eq`/`Hash`). *(verified — zero `FromStr`/
  `TryFrom`/`parse` hits workspace-wide.)*
- **Builds derived ids by raw `format!` concat**: `derived_mid_surface(parent)` →
  `FeatureId::new(format!("{parent}/mid_surface"))` (`geometry.rs:3671`) — the sole derived
  constructor.

The **smoking gun** (findings §2) is the on-disk codec
(`crates/reify-shell-extract/src/result.rs`): the sibling `role` field gets a pinned, *fallible*
`u8` codec — `role_from_u8` rejects unknown discriminants with `io::ErrorKind::InvalidData`
(`result.rs:504-535`) — while `feature_id` is an **unvalidated `String` passthrough**
(`to_string()` on write `:539`/`:547`, `FeatureId::new(String)` on read `:556`/`:564`). Same
function, two concepts, opposite rigor. The right move is already in-tree: `Value::GeometryHandle`
keeps the structured `RealizationNodeId` (`value.rs:1042`), and `Role::content_hash_bytes`
(`geometry.rs:3850`) is a deliberate, frozen, discriminant-pinned content-hash encoding that
explicitly avoids the `format!`/`Debug`-as-contract hazard. P1 applies that same discipline to
`FeatureId`.

Promoting today's flattened string to a `Value::Feature` would entrench the lossiest type in the
language. Fix the representation first.

## Pre-conditions for activating

- **Upstream:** none. P1 is an **independent Wave-1 foundation** — no PRD blocks it.
- **Substrate (G3):** no novel `.ri` grammar (G3 grammar gate **N/A**). All assumed Rust substrate
  was verified to exist against `main` (HEAD `f2e04933db`) during authoring; see
  [`./P1-structured-featureid-feature-value.capability-manifest.md`](./P1-structured-featureid-feature-value.capability-manifest.md).
  Substrate corrections to the brief, confirmed during authoring:
  - `role_from_u8` lives in `crates/reify-shell-extract/src/result.rs:504`, **not** `geometry.rs`.
  - `mid_surface_naming.rs` is in **`crates/reify-shell-extract/`**, not `reify-eval`; its
    production derivation is `derived_mid_surface(parent)` at `:146`.
  - `primitive_attribute_seed.rs` is a **consumer** of `&FeatureId`, not a producer.
  - A second production producer exists the brief did not name: `engine_build.rs:6449`.

## Sketch of approach

Six coupled deliverables:

1. **Structured `FeatureId`** — a recursive enum (Q1):
   `enum FeatureId { Realization(RealizationNodeId), Derived { base: Box<FeatureId>, kind: DerivedKind } }`,
   `enum DerivedKind { MidSurface }` (closed, append-only). `entity()`/`index()` accessors walk to
   the realization root. `Display` is preserved for diagnostics (`"Foo#realization[0]/mid_surface"`)
   but is **output, not identity**.
2. **Fallible `FromStr`/`TryFrom<&str>`** that parses the canonical Display grammar back into
   structure, rejecting malformed input — the parse-back the brief calls for (mirror `role_from_u8`).
3. **Structural content-hash** with pinned, append-only discriminants mirroring
   `Role::content_hash_bytes` — equality/hash/content-hash on **structure**, never on a formatted
   string.
4. **First-class `Value::Feature(FeatureId)` + dedicated `Type::Feature`** (a bare unit variant —
   Feature carries no type-level kind to discriminate, but is semantically neither geometry nor
   selector, so it gets its own `Type` rather than reusing `Type::Geometry`).
5. **Production wiring of `Value::Feature`** (Q2) — the shell-extract structure-instance projection
   carries `Value::Feature` instead of the lossy `Value::String` it flattens and re-parses today
   (`shell_extract_compute.rs:757/815` + `engine_admin.rs`). This gives `Value::Feature` a real
   in-P1 producer/consumer and removes a lossy round-trip on the production path.
6. **Fallible on-disk codec** — `topology_attribute_from_disk` validates `feature_id` (and nested
   `splitting_feature_id`) via the fallible parse, mapping failure to `InvalidData` (mirror
   `role_from_u8`'s `?`); version-bump `ShellExtractionResult::FORMAT_VERSION 1→2`.

`ModEntry.splitting_feature_id` (`geometry.rs:3731`) and `TopologyAttribute.feature_id`
(`geometry.rs:3900`, `Option<FeatureId>` at `topology_attribute_resolver.rs:76`) ride along as
typed fields — no signature change, only the underlying type.

## Resolved design decisions

1. **Shape = recursive enum (Q1).** `FeatureId::{ Realization(RealizationNodeId),
   Derived { base: Box<FeatureId>, kind: DerivedKind } }`. Recursion bottoms out at `Realization`;
   `entity()`/`index()` recurse to the root. Faithful to the current string form's open-ended
   `{parent}/…` nesting. `DerivedKind` is a **closed** enum (`MidSurface` only today); adding a kind
   is a deliberate, compile-checked extension.
2. **`Display` is a pinned, owned grammar — output, not identity.** Format:
   `"<entity>#realization[<index>]"` for `Realization`, suffixed `"/<derived_kind>"` per `Derived`
   step (`/mid_surface`). Unlike the `Debug`-as-contract hazard `Role::content_hash_bytes` warns
   against, this Display grammar is *deliberately owned and pinned*: a `Display↔FromStr` round-trip
   test fixes it, and changing it requires a `FORMAT_VERSION` bump. It is therefore a legitimate
   serialization contract for the codec's String wire — **but never the equality/hash identity**
   (decision 3).
3. **Content-hash / equality on structure, pinned discriminants.** `FeatureId` gets a hand-written
   `content_hash_bytes`-style encoding mirroring `Role::content_hash_bytes`
   (`geometry.rs:3850`): byte-0 variant discriminant (`Realization=0`, `Derived=1`), payload bytes
   for `RealizationNodeId`/`DerivedKind`, recursing through `Derived.base`; wildcard-free match
   (adding a variant is a compile error until a fresh discriminant is assigned); **INVARIANT:
   never renumber an existing discriminant — append only.** `PartialEq`/`Eq`/`Hash` are structural
   (recursing through `Box`), never via `Display`. **One-time cache effect:** the structural hash
   differs from today's String hash, so every `FeatureId` content-hash changes once. In-memory
   selector-resolution caches simply re-solve (correct, marginally slower once); **no persisted
   production cache is affected** — the shell-extract codec has no production callers and is not
   wired into `compute_persist.rs` dispatch (so no on-disk cache entries exist to invalidate).
4. **`Type::Feature` is dedicated, bare-unit.** Precedents: `Value::GeometryHandle → Type::Geometry`
   (generic reuse) vs `Value::Selector → Type::Selector(kind)` (dedicated, parametric). Feature has
   no kind/arity to discriminate at the type level (argues against parametric) but is semantically
   neither geometry nor selector (argues against reuse) → a dedicated **bare** `Type::Feature` unit
   variant; cell-type checks are `matches!(ty, Type::Feature)`.
5. **`Value::Feature` is wired into production in P1 (Q2).** It is not a foundation-only orphan: the
   shell-extract structure-instance projection produces and `engine_admin` consumes it. This also
   satisfies G2 (the variant has a non-synthetic, engine-level signal) and resolves the G1 concern
   (a real in-P1 consumer, in addition to P3 downstream).
6. **On-disk wire stays `feature_id: String`; validation is the fallible parse.** Minimal wire
   change; the "fallible + InvalidData" requirement is met by `FromStr` rejecting malformed input
   inside the codec's `?`. `ShellExtractionResult::FORMAT_VERSION 1→2` (`result.rs:890`) + update
   the pin test `shell_extraction_result_format_version_is_one` (`result.rs:1269`). The
   `mod_history` `.map().collect()` closure (`result.rs:560-567`) becomes a `?`-propagating loop.

## §Contract (B + H)

The seam this PRD specifies is **the `FeatureId` identity/serialization boundary** — touched by
producers (kernel/engine construction), the value model (`Value`/`Type` exhaustive matchers), and
the round-trip boundary (codec + structure-instance projection). The contract pins it so the
cross-cutting round-trip/stability properties land as a first-class integration task rather than
starving under the narrow-lock orchestrator.

### C1 — Type & accessors (`crates/reify-ir/src/geometry.rs`)

```rust
pub enum FeatureId {
    Realization(reify_core::identity::RealizationNodeId),
    Derived { base: Box<FeatureId>, kind: DerivedKind },
}
pub enum DerivedKind { MidSurface }            // closed; append-only

impl FeatureId {
    pub fn realization(entity: impl Into<String>, index: u32) -> Self;  // structured ctor (replaces ::new(String))
    pub fn entity(&self) -> &str;              // walks to Realization root
    pub fn index(&self) -> u32;                // walks to Realization root
    pub(crate) fn content_hash_bytes(&self, /* hasher/sink */) ;        // structural, pinned discriminants
}
impl From<&reify_core::identity::RealizationNodeId> for FeatureId;       // signature preserved; now lossless (Realization variant)
impl FeatureId { pub fn derived_mid_surface(parent: &FeatureId) -> FeatureId; }  // signature preserved; now Derived{ kind: MidSurface }
impl core::str::FromStr for FeatureId { type Err = FeatureIdParseError; } // fallible parse-back of the Display grammar
impl core::fmt::Display for FeatureId;          // pinned grammar (decision 2); OUTPUT, not identity
```

**Invariants.**
- **I1 (lossless):** `FeatureId::from(&rn)` preserves `rn` exactly — `fid.entity() == rn.entity &&
  fid.index() == rn.index`. No flattening to `String` at construction.
- **I2 (Display↔FromStr round-trip):** for every constructible `FeatureId` `x`,
  `FeatureId::from_str(&x.to_string()) == Ok(x)`. **Caveat (verify in α):** this holds only if
  `RealizationNodeId.entity` cannot contain the reserved delimiters `#`/`[`/`]`/`/`. α must confirm
  the entity-name charset (entities are scoped identifiers; expected safe). **If entities can carry
  reserved chars, fall back** to a pinned *structured* on-disk record for the codec (`FeatureIdOnDisk`
  with a discriminant byte + length-prefixed `entity` + `index` + recursive derived steps, fallible-
  decoded à la `role_from_u8`) instead of the String/Display wire — `Display` then stays diagnostic-
  only and decision 6's String wire is replaced. This is a tactical contingency, not a blocker.
- **I3 (parse is fallible & validating):** `FeatureId::from_str(s)` returns `Err` for any `s` not
  matching the pinned grammar; callers in the codec map `Err → io::ErrorKind::InvalidData`.
- **I4 (structural identity):** `PartialEq`/`Eq`/`Hash`/`content_hash_bytes` depend only on
  structure, never on `Display`. Discriminants are frozen & append-only.
- **I5 (Display ≠ identity):** changing the `Display`/`FromStr` grammar requires a
  `FORMAT_VERSION` bump; it never affects `content_hash_bytes`.

### C2 — Value/Type integration (the exhaustive-match wiring obligation)

`Value` is `#[derive(Debug, Clone)]` only (no serde, no derived eq/hash). Adding
`Value::Feature(FeatureId)` is a **closed, compile-forced** change — every exhaustive matcher below
must gain an arm (the compiler is the orphan-detector here, unlike a dispatch-table function):

| Site | `file:line` | Obligation |
|---|---|---|
| `Value::content_hash` | `reify-ir/src/value.rs:1273` | new tag = **31** (current max Selector=30); hash `FeatureId::content_hash_bytes` |
| `Value::try_infer_type` | `value.rs:1759` | `Value::Feature(_) => Some(Type::Feature)` |
| `Value::format_hover` | `value.rs:1865` | hover string |
| `Value::format_display` | `value.rs:2045` | display string |
| `impl Display for Value` | `value.rs:2924` | `write!` the `FeatureId` Display |
| `impl PartialEq for Value` | `value.rs:2347` | structural arm (catch-all is `_ => false`) |
| `impl Ord :: type_tag` | `value.rs:2605` | new ordinal tag |
| `impl Ord :: same-type match` | `value.rs:2649` | arm (catch-all is `unreachable!`) |
| `value_type_kind_matches` | `reify-eval/src/lib.rs:258` | `Value::Feature(_) => matches!(ty, Type::Feature)` |
| `value_kind_label` | `reify-constraints/src/lib.rs:101` | label arm |
| `is_representable_cell_type` | `reify-eval/src/engine_eval.rs:101` | list `Type::Feature` representable (so Feature values may live in cells) |
| `assert_all_value_variants_listed` / `assert_all_type_variants_listed` | `reify-eval/tests/m8_m11_regression_checkpoint.rs:435`/`373` | the exhaustiveness oracle — passes only when every arm above is added |
| `impl Display for Type` | `reify-core/src/ty.rs:519` | `Type::Feature` arm |

### C3 — Round-trip boundary

- **Codec (test-only today; future `compute_persist.rs` dispatch is the consumer-of-record).**
  `TopologyAttributeOnDisk.feature_id: String` (write = `Display`, read = `FromStr` with
  `? → InvalidData`). `ModEntryOnDisk.splitting_feature_id` likewise; the `mod_history` closure
  becomes a `?`-loop. `ShellExtractionResult::FORMAT_VERSION 1→2`.
- **Structure-instance projection (production).** `shell_extract_compute.rs:757/815` produce
  `Value::Feature` (not `Value::String`); `engine_admin.rs` consumes `Value::Feature`. After this,
  no `Display`/`FromStr` round-trip occurs on the production value path — it carries structure.

## §Boundary-test sketch (B + H — faces both sides)

The integration-gate leaf (ε) names this suite as its observable signal.

| # | Scenario | Preconditions | Postcondition (asserted) | Faces |
|---|---|---|---|---|
| B1 | Realization round-trips losslessly | `rn = RealizationNodeId{entity:"Foo", index:3}` | `FeatureId::from(&rn).entity()=="Foo" && .index()==3`; `Display=="Foo#realization[3]"` | producer |
| B2 | Derived (mid-surface) preserves root | `f = derived_mid_surface(&FeatureId::from(&rn))` | `f.entity()=="Foo" && f.index()==3`; `Display=="Foo#realization[3]/mid_surface"` | producer |
| B3 | Display↔FromStr round-trip (I2) | any `Realization`/`Derived` `x` | `FeatureId::from_str(&x.to_string())==Ok(x)` | round-trip |
| B4 | Malformed string rejected (I3) | `s ∈ {"", "Foo", "Foo#realization[]", "Foo/mid_surface", "x#realization[0]/bogus"}` | `from_str(s).is_err()` | round-trip |
| B5 | Content-hash is structural & frozen (I4) | golden bytes for `Realization` + `Derived` | `content_hash_bytes` equals pinned golden; identical structure ⇒ identical hash; differs by entity/index/kind | producer |
| B6 | Codec round-trips structured FeatureId | `TopologyAttribute` w/ `Realization` + `Derived` feature_ids | `from_disk(to_disk(x))==x` | round-trip (consumer) |
| B7 | Codec rejects corrupt feature_id (I3) | on-disk record w/ `feature_id="@@bad@@"` | `topology_attribute_from_disk(..).unwrap_err().kind()==InvalidData` | round-trip (consumer) |
| B8 | `FORMAT_VERSION` bumped | — | `ShellExtractionResult::FORMAT_VERSION==2`; pin test updated | consumer |
| B9 | Value/Type exhaustiveness | `Value::Feature` added | `m8_m11_regression_checkpoint` oracle green; `Value::Feature` eq/hash/`content_hash` (tag 31); `try_infer_type→Type::Feature` | value model |
| B10 | Production path carries `Value::Feature` | shell-extract mid-surface eval | structure-instance field is `Value::Feature` (not `Value::String`); `engine_admin` reads it; `topology_attribute_e2e` + shell/mid-surface e2e green | producer↔consumer |
| B11 | No regression / cache stability | full topology-attribute e2e suite | all existing `topology_attribute_*_e2e.rs` pass with the structured type; only a one-time content-hash change (re-solve), no behavior change | both |

## Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `P3-feature-provenance-query-surface` | P3 **consumes** | `Value::Feature`/`Type::Feature`, structured `FeatureId`, `CreatedByFeature(FeatureId)`/`SplitByFeature(FeatureId)` | **P1 delivers the type; P3 owns the accessor/selector integration** | P1 produces now; P3 wires when authored (Wave 2, gated on P0). Wire a real `add_dependency` edge P3→P1 at P3 decompose time. |
| `P2-selector-substrate-convergence` | file-adjacency (no mechanism seam) | shared file `crates/reify-ir/src/geometry.rs` — **disjoint regions**: P1 = `FeatureId`/`ModEntry`/`Role` cluster; P2 = `FeatureTagTable`/`TopologyAttributeTable` | independent (no contested ownership) | concurrent (Wave 1). Coordinate edits; whoever lands second rebases. If P2 retires `FeatureTagTable` first, P1's migration skips it; if P1 lands first, P2 rebases its deletion onto the structured type. |
| `P0-region-reference-layer-model` | none | P0 explicitly excludes `FeatureId`/`Feature` (its out-of-scope §1) and does **not** redefine `Value::Feature` (it owns the region-reference/user-label model — a different type) | — | no seam; P1 independent of P0 |
| `P4-region-ref-fea-selector-unification` | none | — | — | no seam |

No reciprocal-ownership ambiguity. No new contested-ownership pair introduced (the three known
contested pairs in the overlay are untouched).

## Decomposition plan

B + H shape: foundation type + migration (α), codec rigor (β), value-model variant (γ), production
wiring (δ), and a cross-cutting integration-gate (ε) that owns the §Boundary-test suite. Greek
labels; task IDs assigned at decompose. The type's API change cannot land green incrementally
without a lossy shim (which is exactly the hazard being removed), so **α is one atomic
workspace-wide migration**.

- **α — Structured `FeatureId` enum + `DerivedKind` + accessors + pinned Display + fallible
  `FromStr` + structural content-hash; migrate all construction/consumption sites.**
  *Modules:* `reify-ir` (owner), `reify-core/identity` (parse helpers if needed), and all consumers
  (`reify-eval`, `reify-shell-extract`, `reify-kernel-manifold`, `reify-mesh-morph`). Replaces
  `FeatureId(String)`; preserves `From<&RealizationNodeId>` + `derived_mid_surface` signatures;
  replaces `::new(String)` with `realization(entity,index)` + `from_str`. The 4 String-reconstruction
  sites (`result.rs:556/564`, `shell_extract_compute.rs:757/815`) move to `from_str`; the codec read
  thereby becomes fallible-via-`?`. Test `::new("…")` sites → structured ctors.
  *Signal (intermediate):* workspace builds green; **existing** `topology_attribute_e2e.rs` (+
  extrude/revolve/sweep/loft + resolver e2e siblings) pass unchanged; new golden test pins
  `content_hash_bytes` for `Realization` + `Derived`; `Display↔from_str` round-trip + malformed-input
  rejection unit tests. *Unlocks:* β, γ. *files:* `[]` (broad mechanical refactor — BRE acquires).

- **β — Fallible codec rigor + `FORMAT_VERSION` bump.** *Modules:* `reify-shell-extract`. Confirms
  `topology_attribute_from_disk` maps `from_str` failure to `InvalidData` (B7), converts the
  `mod_history` closure to a `?`-loop, bumps `ShellExtractionResult::FORMAT_VERSION 1→2`, updates the
  pin test. *Signal (leaf):* codec round-trips a structured `FeatureId` (B6); a corrupt on-disk
  `feature_id` is rejected with `io::ErrorKind::InvalidData` (B7 — a negative-assertion signal, G6
  branch 4); `FORMAT_VERSION` pin test green (B8). *Prereqs:* α. *files:*
  `crates/reify-shell-extract/src/result.rs`.

- **γ — `Value::Feature(FeatureId)` + `Type::Feature` + all exhaustive-match arms.** *Modules:*
  `reify-ir`, `reify-core`, `reify-eval`, `reify-constraints`. Adds the variant + every arm in §C2
  (content-hash tag 31, `try_infer_type`, displays, `PartialEq`/`Ord`, `value_type_kind_matches`,
  `value_kind_label`, `is_representable_cell_type`, `Type::Feature` Display). *Signal (intermediate):*
  `m8_m11_regression_checkpoint` exhaustiveness oracle green with the new variant (B9); `Value::Feature`
  eq/hash/`content_hash` golden. *Prereqs:* α. *Unlocks:* δ. *files:* `[]` (cross-crate matcher arms —
  BRE acquires the exact set; primary anchors `crates/reify-ir/src/value.rs`, `crates/reify-core/src/ty.rs`).

- **δ — Wire `Value::Feature` into the production shell-extract structure-instance path.** *Modules:*
  `reify-eval`. Replaces the `Value::String` feature_id field with `Value::Feature` at
  `shell_extract_compute.rs:757/815`; updates `engine_admin.rs` consumption (`:2266`/`:2317-2328`).
  *Signal (leaf, engine-observable):* the shell-extract mid-surface path carries `Value::Feature`
  end-to-end (not a flattened string); `topology_attribute_e2e` + shell/mid-surface integration tests
  green with the structured value carried (B10). *Prereqs:* γ (and α). *files:*
  `crates/reify-eval/src/shell_extract_compute.rs`, `crates/reify-eval/src/engine_admin.rs`.

- **ε — Integration-gate: the §Boundary-test suite (B + H closure).** *Modules:* `reify-ir`,
  `reify-shell-extract`, `reify-eval` (test artifacts). A committed boundary-test suite implementing
  B1–B11 — the cross-cutting round-trip/stability contract that no single foundation task owns.
  *Signal (LEAF):* the boundary-test suite (B1–B11) is green end-to-end, including the no-regression +
  content-hash-stability rows (B11). *Prereqs:* α, β, γ, δ. *files:* `[]` (boundary tests span
  reify-ir / reify-shell-extract / reify-eval test trees).

### Dependency view

```
α ──┬─► β ───────────────► ε
    └─► γ ──► δ ──────────► ε
```

## Out of scope (owned elsewhere)

- **`feature()` accessor + `created_by_feature`/`split_by_feature` selector surface → P3.** P1
  delivers the `Value::Feature` type and structured `FeatureId`; P3 surfaces them (Wave 2, gated on
  P0). Edge wired at P3 decompose.
- **User-labels / region-reference model / topology-noun layer → P0.**
- **`SelectorKind` unification + `FeatureTagTable` retirement + `LeafQuery::Named` fate → P2.**
- **Persisting shell-extract results (wiring the codec into `compute_persist.rs` dispatch)** — not
  filed; named here as the codec's future consumer-of-record. P1 makes the format correct *before*
  that wiring exists.
- **Edge/vertex `DerivedKind`s beyond `MidSurface`** — add when a producer needs them (the closed
  enum makes this a compile-checked extension).

## Open questions (tactical — not blocking)

1. **`FeatureIdParseError` shape.** A dedicated error enum vs a `String` message. *Suggested:* a
   small enum (`Empty`, `BadRealization`, `UnknownDerivedKind`, `Trailing`) for testable B4 arms.
   Decide during α.
2. **`realization()` vs keeping a renamed `::new`.** Whether the structured convenience ctor is
   `FeatureId::realization(entity,index)` or a typed `FeatureId::from_parts`. *Suggested:*
   `realization(entity,index)` (reads at call sites). Decide during α.
3. **Where the `Display` grammar parser lives** — inline in `geometry.rs` `FromStr` vs a small
   `RealizationNodeId::from_str` reused by `FeatureId`. *Suggested:* add `RealizationNodeId::from_str`
   in `reify-core/identity` (it owns the `#realization[…]` Display) and compose. Decide during α.
4. **Hover/label text for `Value::Feature`** (the `format_hover`/`value_kind_label` arms). Cosmetic.
   Decide during γ.
