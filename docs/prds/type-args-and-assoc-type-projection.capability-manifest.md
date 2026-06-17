# Capability manifest — type-args-at-the-type-level + associated-type projection

Mechanizes G3 + G6 per leaf for `docs/prds/type-args-and-assoc-type-projection.md`. Every asserted capability binds to evidence; any **FAIL** binding blocks the batch until resolved. This PRD is a **compile-time type-system** change — the relevant evidence forms are **grammar-fixture** (anti-mismatch) and **wired-on-main / anti-orphan**. There are **no** `field-population` bindings (no `Value` fields produced) and **no** `numeric-floor` bindings (dimensional-type checking over exact `DimensionVector`s — `Length`/`Angle` — not floating-point numerics).

Verified on `main` 2026-06-14.

| Leaf | Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|---|
| **α grammar** | `Name<Args>::member` (applied-base projection) parses in type position | grammar-fixture | **Gap on main** (`tree-sitter parse` FAIL, PRD §3.2); α **produces** it + commits `f_applied_base_projection.ri`. `grammar_confirmed=false` until α lands. | **PASS** (owned producer, not fiction) |
| **α grammar** | every *other* novel form already parses | grammar-fixture | `tree-sitter parse --quiet` 0-ERROR on main for bounded structure type-params, `+`-multi-trait-bound, structure-body `type X = Y`, trait assoc-type decl, type-arg application, bare/type-param projection (PRD §3.2 table) | **PASS** |
| **β variants** | `Type` is not serialized → no wire/persist breakage from a variant addition | wired-on-main (negative) | no `serde`/`bincode`/manual (de)serialize of `Type`; only `Display` is exhaustive (`reify-core/src/ty.rs:466`) | **PASS** |
| **β variants** | all exhaustive `Type` matches enumerated for migration | wired-on-main | ~11 no-wildcard sites + `Display` listed with file:line (PRD §5); precedent task 3924 (`Type::Tuple`) | **PASS** |
| **γ resolution** | structure type-args are dropped today (the bug γ fixes) | wired-on-main | `type_resolution.rs:1358-1365` returns `StructureRef(name)` without examining `type_args`; structure→`StructureRef` at `:659-660` | **PASS** |
| **γ resolution** | arg-vs-bound checking machinery exists to reuse | wired-on-main | `satisfies_trait_bound`/`check_trait_conformance`; `TopologyTemplate.type_params` carries bounds (`types.rs:659`) | **PASS** |
| **δ projection** | assoc-binding cannot reference type-params today (the restriction δ lifts) | wired-on-main | `collect_structure_assoc_type_bindings` hardcodes `empty_params` (`conformance/checker.rs:910`) | **PASS** |
| **δ projection** | bare-base reduction target exists (reused, not re-derived) | wired-on-main | `assoc_types` table `CompiledAssocType` (`types.rs:93-104,741-751`) + `resolve_qualified_assoc_type` (3974, done); rejections to remove at `:818-827`/`:832-845` | **PASS** |
| **δ projection** | `Coupling<Prismatic>::MotionValue` ⇒ `Length` is achievable from δ's own deps (G6) | premise-trace | worked reduction chain PRD §4.3 — all four steps grounded in α (grammar), β (variants), γ (Applied), and the 3974 table | **PASS** |
| **ε stdlib gate** | the consumer is wired to a real `reify check` signal (anti-orphan) | wired-on-main | ε edits real `kinematic.ri` + `joint_signatures.rs`; gate fixtures run under `reify check`; `4312 → ε` dependency wired at decompose | **PASS** |
| **ε stdlib gate** | current stdlib state supports the declarations (G6) | premise-trace | `trait Joint`/`DrivingJoint` + per-kind joint structures exist; `Coupling : Joint` non-generic (`kinematic.ri:149`); `HasMotion`/`MotionValue` absent → ε declares them; `couple` typed by `joint_signatures.rs` (4311) → ε upgrades to `Applied` | **PASS** |
| **ε stdlib gate** | mismatch ⇒ **one** targeted diagnostic, not a cascade | premise-trace | substrate resolves projection to a concrete `Type` so the *existing* dimensional-compat check fires once; anti-cascade poison mirrors `type_resolution.rs:781-789` (PRD §4.4) | **PASS** |
| **ζ doc** | the doc target advertises `MotionValue` as future | wired-on-main | `docs/reify-stdlib-reference.md` ≈ 1357-1418 documents `MotionValue<…>` type-family as desired-future; ζ flips to shipped | **PASS** |

**Batch verdict:** no FAIL bindings. The single grammar gap is an **owned producer** (leaf α), not a fiction — G3 satisfied by queueing the substrate work. Batch may queue.
