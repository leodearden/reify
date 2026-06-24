# PRD Brief — P1: Structured `FeatureId` + first-class `Feature` value

> **Brief for a `/prd` author session** (not a finished PRD). Read `./00-findings.md` FIRST.
> This is a **foundation refactor** — independent of the keystone (P0) for its internal type
> work; can be authored & landed concurrently with P0/P2 (Wave 1). The user-facing `feature()`
> accessor surface is **out of scope here** — it belongs to P3, which consumes the type this PRD
> delivers.
>
> **Do NOT touch task 3523 or esc-3523-75/76.** Today is 2026-06-24. Line numbers accurate at
> time of writing — G3-verify against current `main`.

## Why this PRD exists

`FeatureId(String)` is the worst stringly-typed offender in the subsystem (findings §2). It
lossily flattens structured `RealizationNodeId { entity: String, index: u32 }` via `.to_string()`,
has no parse-back, and builds derived ids by raw `format!` concat. The on-disk codec treats the
sibling `Role` field with a pinned fallible codec while passing `feature_id` through as an
unvalidated string. Promoting this representation to a first-class `Value::Feature` (the charter's
D2) would entrench the lossiest type in the language. Fix the representation **before** surfacing it.

## Scope / deliverables

1. **Structured `FeatureId`.** Replace the `String` newtype with a structured form that preserves
   `RealizationNodeId` (entity + index) and represents derived ids with a **typed, closed**
   `DerivedKind` (replacing `format!("{parent}/mid_surface")`). Add `entity()`/`index()` accessors.
   `Display` stays for diagnostics (`"Foo#realization[0]/mid_surface"`) but is **output, not
   identity**. Precedent to mirror: `Value::GeometryHandle` already keeps the structured
   `RealizationNodeId` (`crates/reify-ir/src/value.rs:1042`).
2. **First-class `Feature` value type.** Add `Value::Feature(FeatureId)` (+ a `Type::Feature`)
   wrapping the **structured** id. Equality/content-hash defined on structure, not on a formatted
   string (avoid the `format!`-as-contract hazard the `Role` codec was rewritten to dodge —
   `crates/reify-ir/src/geometry.rs:3833-3850`).
3. **Fallible on-disk codec.** Make `feature_id` (de)serialization fallible + validating, mirroring
   `role_from_u8` (`crates/reify-shell-extract/src/result.rs:504`); reject corrupt ids at
   `InvalidData` instead of silently constructing a valid-looking `FeatureId`. Version-bump the
   on-disk format.
4. **Migration.** Update all `FeatureId` construction/consumption sites; `ModEntry.splitting_feature_id`
   (`geometry.rs:3731`) rides along.

## Design questions to resolve

- Exact structured shape: enum `{ Realization(RealizationNodeId), Derived { base, kind: DerivedKind } }`
  vs a struct carrying an optional derivation chain. (`/prd` decides; sketch in findings §2.)
- Does `Value::Feature` need its own `Type` variant, or can it reuse an existing one? (Check the
  `Value`/`Type` enums; `Type` is in `crates/reify-core/src/ty.rs`.)
- Content-hash stability contract for cache keys (mirror `Role::content_hash_bytes` discipline).

## Key code pointers (verify against current main)

- `FeatureId(String)`: `crates/reify-ir/src/geometry.rs:3653`; `From<&RealizationNodeId>` `:3682`;
  `derived_mid_surface` (format! concat) `:3672`.
- `RealizationNodeId { entity, index }`: `crates/reify-core/src/identity.rs:163`; `Display` `:178`.
- Structured-id precedent: `Value::GeometryHandle` `crates/reify-ir/src/value.rs:1042`;
  `GeometryHandleRef` `:417`.
- `ModEntry`: `crates/reify-ir/src/geometry.rs:3731`. `Role` hash-codec cautionary precedent:
  `:3833-3850`.
- On-disk codec: `crates/reify-shell-extract/src/result.rs:337-356` (role fallible `:504`;
  feature_id passthrough `:539`/`:556`).
- Production producers of `FeatureId`: `crates/reify-eval/src/engine_build.rs:6184`;
  `crates/reify-eval/src/primitive_attribute_seed.rs`; `crates/reify-eval/src/mid_surface_naming.rs`.

## Out of scope

- The `feature()` accessor + `created_by_feature`/`split_by_feature` selector surface → **P3**.
- Any decision about user-labels / region-reference model → **P0**.

## Dependencies

- **Upstream:** none (independent foundation; Wave 1).
- **Downstream:** P3 consumes `Value::Feature`. P3 should not be authored until P0 is committed
  (its surface form depends on P0), but it depends on P1's *type* landing.

## SOP reminders

- Commit the PRD before creating tasks. Cross-crate refactor: scope `metadata.files` tightly or
  leave `[]` (overlay rule — never a directory). Cite `./00-findings.md` for evidence.
