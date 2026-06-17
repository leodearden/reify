# Type-system substrate: type-args-at-the-type-level + associated-type projection

**Status:** authored 2026-06-14 · **Approach:** B + H (contract + two-way boundary tests) · **Stakes:** high (adds variants to the `Type` enum)

**Origin.** Surfaced as *unowned* by the architect on task **4312** (mechanism-completion L3, deferred) via **esc-4312-164 (L1) / esc-4312-165 (L2)**; Leo chose to PRD the substrate (2026-06-14). This is a **version-agnostic type-system foundation** — it sits beneath the v0_6 mechanism-completion PRD (task 4312) and the v0_3 auto-type-param-resolution-completion PRD — hence the root `docs/prds/` placement.

---

## §0 — Supersession / prior-decision reversal

This PRD **reverses** a prior, narrower decision recorded in `docs/prds/v0_6/trait-associated-functions.md` **§3.5 item 4**:

> "**Type representation — adequate, no new `Type` variant needed.** A bound associated type resolves to an existing `Type` (`Type::StructureRef(name)` … or `Type::TraitObject(trait)`). … No new variant is required; the work is **resolution + substitution**, not a new kind."

That decision was **correct for its scope** — the *bare concrete-structure base* (`Beam::Material`), which the task-3974 (ι_e) machinery fully reduces at resolution time to a concrete `Type` and never needs to persist. The comment cementing it is still in the tree at `crates/reify-compiler/src/type_resolution.rs:830` ("PRD §3.5 adds no associated-type-projection `Type` variant"), guarding the two **rejection sites** this PRD removes (`:818-827`, `:832-845`).

The two cases §3.5 *did not* cover, and which this PRD adds, are exactly the cases that **cannot** be reduced to an existing variant at definition time:

1. **A type-arg-applied generic-structure base** — `Coupling<Prismatic>::MotionValue`. The base `Coupling<Prismatic>` is not representable today: `Type::StructureRef(String)` (`crates/reify-core/src/ty.rs:108`) carries only a name, so `Coupling<Prismatic>` and `Coupling<Revolute>` are the *same* `Type`.
2. **A type-param base inside a generic context** — `P::MotionValue` where `Coupling<P: DrivingJoint + HasMotion>`. `P` is unbound at the definition site, so the projection is irreducibly **symbolic** until `P` is substituted; it has no concrete `Type` to collapse to.

§3.5's "resolution + substitution, no new kind" holds for case 0 (bare base); cases 1 and 2 require **two** new kinds — a type-arg carrier and a symbolic projection.

---

## §1 — Consumer + user-observable surface (G1)

Every mechanism here has a named consumer; nothing is built orphaned.

| Mechanism | Consumer | Observable surface |
|---|---|---|
| `Type::Applied { name, args }` (type-args at the type level) | **(a)** task 4312 / mechanism-completion L3 — generic `Coupling<P>` dimensional typing; **(b)** auto-type-param-resolution-completion — candidate distinction by type-args (see §8, *not in scope to migrate here*) | `reify check` distinguishes `Coupling<Prismatic>` from `Coupling<Revolute>` |
| `Type::Projection { base, member }` (associated-type projection) | task 4312 — `MotionValue` motion-variable dimensional coherence | `reify check`: `Coupling<Prismatic>::MotionValue` ⇒ `Length`, `Coupling<Revolute>::MotionValue` ⇒ `Angle` |
| stdlib generic `Coupling<P>` + `HasMotion`/`MotionValue` | the .ri author writing kinematic models | a `.ri` fixture under `reify check` type-checks; a dimensional-mismatch fixture emits **one targeted diagnostic**, not a poison cascade |

**This is the in-engine-norm-exempt case** (overlay G1 / `engine-integration-norm.md §3.10`): a pure compile-time **type-system / compile-pipeline** change. It introduces **no** new engine seam and need not cite any of the 7 in-engine seams (§3.1–§3.7). The runtime `Coupling` primitive (`couple`/`gear`/`screw`/`rack_and_pinion`, FK-derived coupling motion) is **already fully functional** (`crates/reify-stdlib/src/joints.rs:1489` `make_coupling`); this substrate is **compile-time dimensional-safety sugar only**. Accordingly every leaf signal is a **diagnostic / type-check** signal (`reify check` CLI-output difference), never a runtime-behaviour change.

---

## §2 — Sketch of approach

Two new `Type` variants, an erase-at-runtime (phantom) discipline, and a projection-reduction step that reuses the existing per-structure associated-type table.

```
enum Type {
  …
  StructureRef(String),                       // UNCHANGED — zero-arg structures
  Applied  { name: String, args: Vec<Type> }, // Coupling<Prismatic>  (NEW)
  Projection { base: Box<Type>, member: String }, // P::MotionValue   (NEW)
  …
}
```

- **Type-args carrier — `Applied`.** When `TypeExprKind::Named { name, type_args }` resolves to a **structure** and `type_args` is non-empty, produce `Applied { name, args: resolved_args }` instead of silently dropping the args (the current bug — `type_resolution.rs:1358-1365`). Invariant: **`args` non-empty ⇒ `Applied`; empty ⇒ `StructureRef`** (one canonical form per arity). Sibling-variant strategy (not extending `StructureRef` in place) was chosen for **lowest migration churn**: every existing `Type::StructureRef(name)` *construction* site is untouched; precedent is the existing `Type::Tuple(Vec<Type>)`/`Type::Union(Vec<Type>)` multi-`Type` variants.
- **Projection — `Projection`.** Represents `base::member` while `base` is not yet concrete. `base ∈ { TypeParam, Applied, StructureRef }`. A normalization step **reduces** it as the base becomes concrete (§4.3). The type-param base (`P::MotionValue`) is the case that *forces* a persistent variant — it is irreducible until `P` is substituted.
- **Phantom args (runtime erasure).** `Applied`'s `args` are **compile-time-only**, erased before eval — identical to how generic-fn monomorphization erases `type_params` (`auto-type-param-resolution-completion.md §4.2`) and to the `ScalarParam` "erased before eval" precedent (`ty.rs:177`). At runtime a `Coupling<Prismatic>` cell holds an ordinary `Value::Map` (kind-tag `"coupling"`); `value_type_kind_matches` checks the **name only**, ignoring args. So `Coupling<Prismatic>` and `Coupling<Revolute>` are **distinct at compile time** (dimensional safety) but the **same** runtime kind.
- **Reduction reuses the shipped assoc-type table.** The per-structure `assoc_types` table (`CompiledAssocType { trait_name, type_name, resolved, is_override }`, `crates/reify-compiler/src/types.rs:93-104,741-751`) and the bare-base resolver `resolve_qualified_assoc_type` (task 3974) already resolve `Prismatic::MotionValue`. This PRD threads the structure's **type-params** into how that table is *built* (so a binding may be `P::MotionValue`), and threads `Applied`'s **args** into how it is *read* (substitute `P := arg` before reduction).

---

## §3 — Pre-conditions (substrate verification)

All verified on current `main` (2026-06-14).

### 3.1 The gap is real (G6 premise)

- `Type::StructureRef(String)` carries no args (`ty.rs:108`).
- Structure type-args are **silently dropped**: a `Named { name, type_args }` resolving to a user structure returns `Type::StructureRef(name)` via `resolve_type_with_aliases` (`type_resolution.rs:659-660`, returned at `:1358-1365`) **without examining `type_args`** — no error, no warning.
- `resolve_qualified_assoc_type` **rejects** a base with type-args (`:818-827`) and a type-param base (`:832-845`).
- Assoc-type bindings **cannot** reference the structure's type-params: `collect_structure_assoc_type_bindings` hardcodes `empty_params` (`crates/reify-compiler/src/conformance/checker.rs:910`), so `type MotionValue = P::MotionValue` fails to resolve today.

### 3.2 Grammar reality check (G3) — verified by the grammar gate

The overlay grammar gate (`tree-sitter parse --quiet`) was run on every novel form. **Most of the surface is already grammar-clear** — only **one** production is missing:

| Form | Parses on main? | Note |
|---|---|---|
| `structure def Coupling<P: DrivingJoint + HasMotion> { … }` (bounded structure type-param) | ✅ | `type_parameters` on `structure_definition`, grammar.js:507 |
| `structure def Prismatic : DrivingJoint + HasMotion { … }` (multi-trait bound) | ✅ | **separator is `+`, not `,`** — `trait_bound_list` = `entry (+ entry)*`, grammar.js:476 |
| `structure def S : HasMotion { type MotionValue = Length }` (structure-body binding) | ✅ | task 3971 (ι_a) |
| `trait HasMotion { type MotionValue }` (trait assoc-type decl) | ✅ | shipped (trait-associated-functions) |
| `param c : Coupling<Prismatic>` (type-arg application at a use site) | ✅ | `TypeExprKind::Named.type_args` already parses |
| `param n : Prismatic::MotionValue` (bare-structure projection in type pos) | ✅ | task 3971 (ι_a) |
| `param n : P::MotionValue` (type-param projection in type pos) | ✅ | task 3971 (ι_a) |
| **`param m : Coupling<Prismatic>::MotionValue` (applied-base projection)** | ❌ **FAIL** | `Name<Args>::member` — **the one grammar prerequisite** (leaf α) |

**Conclusion:** G3 is **not** clear out of the box — `grammar_confirmed=false` for the applied-base-projection form. It is scoped as an explicit grammar producer leaf (α), and every downstream leaf depends on it; committed fixtures pin all forms.

### 3.3 Stdlib current state (G6 premise for ε)

`crates/reify-compiler/stdlib/kinematic.ri`: `trait Joint { }` and `trait DrivingJoint : Joint { }` exist; `structure def Prismatic : DrivingJoint`, `Revolute : DrivingJoint`, … exist; `structure def Coupling : Joint { }` is **non-generic** (`:149`). There is **no** `HasMotion` trait and **no** `MotionValue` associated type yet — ε declares them. `couple` is typed `Coupling` (non-generic) by `joint_signatures.rs::joint_ctor_result_type` (task 4311); ε upgrades that to `Applied { "Coupling", [parent-joint type] }`.

### 3.4 Landed machinery this builds on

- **Generic *functions*** (call-site type-arg inference, `unify`, `substitute_type_params`): `generic-user-functions.md`, task 4231 (done). The `Applied` variant is the generic-*structure*-instantiation analogue; this PRD **extends** `substitute_type_params`/`unify` rather than inventing a walker.
- **Bare-base associated-type projection**: tasks 3971/3972/3973/3974 (done) — grammar, conformance model (`RequirementKind::AssocType`/`DefaultKind::AssocType`), the resolved `assoc_types` table, and `resolve_qualified_assoc_type`.
- **Trait-bound conformance check** (`satisfies_trait_bound` / `check_trait_conformance`) — reused for `Applied` arg-vs-bound checking (γ).
- **Variant-addition precedent**: cancelled task 3924 added `Type::Tuple(Vec<Type>)` the same way (new variant + resolution wiring + structural eq) — a migration template.

---

## §4 — Contract

### 4.1 The variants

```rust
// crates/reify-core/src/ty.rs
/// A generic structure applied to type arguments, e.g. `Coupling<Prismatic>`.
///
/// `args` are PHANTOM: compile-time only, erased before eval. At runtime the
/// cell holds the structure's ordinary value; `value_type_kind_matches` checks
/// `name` only. INVARIANT: produced only with `!args.is_empty()`; the zero-arg
/// case stays `StructureRef(name)`.
Applied { name: String, args: Vec<Type> },

/// Associated-type projection `base::member` held until `base` is concrete.
/// `base` ∈ { TypeParam, Applied, StructureRef }. Reduced by `normalize_type`
/// (§4.3); a surviving Projection on a TypeParam base is legitimately symbolic
/// inside a generic context, and poisons to `Type::Error` (anti-cascade) only
/// if it reaches a concreteness-requiring site unsubstituted.
Projection { base: Box<Type>, member: String },
```

- **`Display`**: `Applied` → `Coupling<Prismatic, …>`; `Projection` → `Coupling<Prismatic>::MotionValue`.
- **Eq/Hash**: derived; structural. `Applied{"Coupling",[Prismatic]} != Applied{"Coupling",[Revolute]}`; `Applied{"Coupling",[]}` is **never constructed** (use `StructureRef`).
- **`is_representable_cell_type`** (`engine_eval.rs:100`): `Applied` ⇒ **true** (a concrete structure type, like `StructureRef`); `Projection` ⇒ **false** (must be reduced before eval; a survivor is a bug → poison, like an unresolved `TypeParam`).
- **`value_type_kind_matches`** (`reify-eval/src/lib.rs:243`): `Applied{name,_}` matches a value exactly as `StructureRef(name)` does — **name only**, args ignored (phantom).

### 4.2 Resolution — `Named { name, type_args }` → `Applied` (leaf γ)

In `resolve_type_expr_with_aliases_kinded` (`type_resolution.rs:1279`), at the structure arm where args are currently dropped:

1. If `name` is a structure and `type_args` is non-empty: recursively resolve each arg (reusing the builtin-parametric recursion pattern, `:1311-1324`), producing `Applied { name, args }`.
2. **Arity check** against the structure's declared `type_params` (`TopologyTemplate.type_params`, `types.rs:659`): mismatch ⇒ **`E_TYPE_ARG_ARITY`**.
3. **Bound check** each arg against the corresponding `type_param`'s bounds via `satisfies_trait_bound`: violation ⇒ **`E_TYPE_ARG_BOUND`** (e.g. `Coupling<SomeNonHasMotion>`).
4. A non-generic structure given args (`Foo<Bar>` where `Foo` declares none) ⇒ `E_TYPE_ARG_ARITY` (arity 0 vs N).

### 4.3 Projection binding + reduction (leaf δ)

**Binding (build side).** Lift the `empty_params` restriction at `conformance/checker.rs:910`: pass the structure's `type_params` into `collect_structure_assoc_type_bindings` so `type MotionValue = P::MotionValue` resolves its binding to `Projection { TypeParam("P"), "MotionValue" }` and stores it in `CompiledAssocType.resolved`.

**Reduction (read side).** A `normalize_type(ty)` pass (an extension of the existing `substitute_type_params` walker, `type_resolution.rs:1516`) reduces `Projection { base, member }`:

| `base` after substitution | reduction |
|---|---|
| `StructureRef(S)` | existing 3974 lookup: `S`'s `assoc_types` entry for `member` (`resolve_qualified_assoc_type` core) |
| `Applied { name, args }` | substitute `name`'s `type_params := args` into `name`'s **member binding** for `member`, then recurse `normalize_type` |
| `TypeParam(P)` | **irreducible** — stays `Projection` (legitimately symbolic; reduced when `P` is later substituted) |
| anything else / `member` not an assoc type of the reduced base | `Type::Error` (poison) + the *existing* 3974 diagnostic (`UnresolvedType`/`AmbiguousAssocType`); **no second diagnostic** |

**Remove the two rejections** at `type_resolution.rs:818-827` (applied base) and `:832-845` (type-param base); route both into the reducer above.

**Worked chain** (`Coupling<Prismatic>::MotionValue`):
`Projection{ Applied{"Coupling",[StructureRef("Prismatic")]}, "MotionValue" }`
→ Coupling's `MotionValue` binding is `Projection{TypeParam("P"),"MotionValue"}`; substitute `P := StructureRef("Prismatic")`
→ `Projection{ StructureRef("Prismatic"), "MotionValue" }`
→ 3974 lookup of Prismatic's `MotionValue` ⇒ **`Length`**. ∎

### 4.4 Diagnostics + anti-cascade contract (the brief's explicit ask)

The substrate's job at a **dimensional mismatch** is to resolve the projection to a **concrete** `Type` (`Length`/`Angle`) so the **existing** dimensional-compat check fires exactly **one** clean mismatch — replacing today's behaviour (reject → `Type::Error` → silent-drop or cascade). New codes: `E_TYPE_ARG_ARITY`, `E_TYPE_ARG_BOUND`. Reused: `UnresolvedType`/`AmbiguousAssocType` (3974) for unknown/ambiguous members. The anti-cascade rule (poison without a second diagnostic) mirrors the established `resolve_qualified_assoc_type` contract (`type_resolution.rs:781-789`).

---

## §5 — Migration of `StructureRef` sites + blast radius (G5)

`Type` is **not serialized** (no serde/bincode) — **no wire/persist breakage**. Adding two variants breaks **exhaustive (no-wildcard) matches**; these all fail to compile, forcing awareness. The migration (leaf β) is **one atomic, multi-crate commit** — a variant addition cannot be split across the exhaustive-match boundary (same shape as task 3924).

| Site | File:line | `Applied` arm | `Projection` arm |
|---|---|---|---|
| `Display` | `reify-core/src/ty.rs:466` | `Coupling<…>` | `…::member` |
| `is_representable_cell_type` | `reify-eval/src/engine_eval.rs:100` | true (like StructureRef) | false |
| `value_type_kind_matches` | `reify-eval/src/lib.rs:243` | name-only match | unreachable → false |
| `implicitly_converts_to`, `type_compatible` | `reify-compiler/src/type_compat.rs:51,219` | nominal-by-name + args structural | structural |
| `type_carries_type_param` (×2, **verbatim-synced**) | `type_compat.rs:377`, `reify-expr/src/lib.rs:1138` | recurse `args` | recurse `base` |
| `type_carries_dim_param` (×2) | `type_compat.rs:457`, `reify-expr/src/lib.rs:1223` | recurse `args` | recurse `base` |
| `type_carries_trait_object` (×2) | `type_compat.rs:340`, `reify-expr/src/lib.rs:1103` | recurse `args` | recurse `base` |
| `unify` | `type_compat.rs:549` | unify name + element-wise args | unify base |
| `substitute_type_params` → `normalize_type` | `type_resolution.rs:1516` | substitute into `args` | substitute `base` **then reduce** (§4.3) |

**Sync constraints (must move together):** the `type_carries_type_param` and `type_carries_dim_param` pairs (compiler ↔ reify-expr, "MUST remain verbatim-synced", esc-4231-120/126), and the `unify` ↔ `substitute_type_params` pair. ~11 exhaustive sites + `Display`.

---

## §6 — Out of scope

- **Migrating auto-type-param-resolution onto `Applied`.** That PRD distinguishes candidates via *monomorphization* (template clones `Bearing$cand`, `auto-type-param-resolution-completion.md §4.2`). This substrate **provides** `Applied` so a future migration *can* unify the two; this PRD does **not** perform it and does **not** touch auto-type-param code (G4, §8).
- **Higher-kinded / multi-param projection, where-clauses, variance.** Single-base `base::member` projection only.
- **Runtime behaviour.** Args are phantom; no `Value` variant, no eval-graph node, no kernel hook.
- **Multi-DOF joints conforming to `HasMotion`.** Only single-DOF driving joints (`Prismatic`→`Length`, `Revolute`→`Angle`) bind `MotionValue`; the `+ HasMotion` bound naturally excludes `Cylindrical`/`Planar`/`Spherical`.
- **Modifying task 4312 or any escalation state** (per the originating brief; 4312 is parked pending this PRD).

---

## §7 — Boundary-test sketch (H — faces both ways)

The ε integration gate is the two-way boundary test; the sketch names both sides explicitly:

- **Producer side (type-resolution / projection):** given the ε stdlib declarations, `reify check` resolves `Coupling<Prismatic>::MotionValue` to `Length` and `Coupling<Revolute>::MotionValue` to `Angle` — asserted on real stdlib types, not a synthetic fixture.
- **Consumer side (stdlib dimensional check):** a `.ri` program that flows a `Coupling<Revolute>` motion variable (Angle) into a `Length` slot emits **exactly one** targeted dimensional-mismatch diagnostic under `reify check` — **no** `Type::Error` cascade, **no** extra "unknown type" noise. A correct program (`Coupling<Prismatic>` → `Length`) type-checks clean (0 diagnostics).
- **Arg-bound boundary:** `Coupling<NotAJoint>` emits `E_TYPE_ARG_BOUND` naming the unsatisfied `DrivingJoint + HasMotion` bound; `Coupling<Prismatic, Revolute>` emits `E_TYPE_ARG_ARITY`.

---

## §8 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Other side | Direction | Owner | Resolution |
|---|---|---|---|---|
| `Type::Applied`/`Type::Projection` variant addition + all `StructureRef`-match migration | `reify-core` `Type`, all exhaustive matches | this PRD **owns** | **this PRD (β)** | the variant pair is added and migrated here, once |
| `trait-associated-functions.md §3.5` "no new variant" decision | bare-base projection (3974) | this PRD **reverses** for applied/type-param bases | **this PRD (§0)** | §3.5 stays correct for the bare base; this PRD adds the two cases it didn't cover |
| mechanism-completion L3 (task 4312) generic `Coupling<P>`/`MotionValue` | the deferred L3 forward-stub | this PRD **delivers** (Leo folded the real stdlib wiring into ε) | **this PRD (ε)** | wire `4312 → ε` at decompose; 4312's L3 signal is delivered by ε, so 4312 is **superseded** — its owner reconciles its status once ε lands (this PRD does **not** modify 4312) |
| auto-type-param-resolution-completion `StructureRef`-carries-no-args wall (esc-4596/esc-4437) | monomorphization (`Bearing$cand` clones) | reciprocal "same wall" | **auto-type-param PRD** (independent) | this PRD provides `Applied` so a *future* migration can consume it; **no migration here** (§6); a follow-up bookmark may be filed at decompose |
| generic-*function* substrate (`unify`/`substitute_type_params`, task 4231) | generic-user-functions.md | precondition (landed) | generic-user-functions PRD | reused/extended, not re-derived |
| in-engine-norm §3 seams | `engine-integration-norm.md` | **exempt** | n/a | compile-pipeline change, §3.10 — cites no engine seam |

---

## §9 — Decomposition plan (one bullet per leaf → observable signal)

Vertical-slice DAG. Each leaf names a **user-observable** signal (`reify check` CLI/diagnostic difference or a parse fixture), never a synthetic-input unit test. The foundation leaf (β) has no isolated user signal and is **roped to the ε integration gate** (C-as-integration-gate).

- **α — grammar: applied-base projection `Name<Args>::member`.** Extend the qualified-type-expr production (tree-sitter-reify) + ts_parser lowering so `Coupling<Prismatic>::MotionValue` parses in type position. Commit fixtures. *Signal:* `tree-sitter parse --quiet` 0-ERROR on the new fixture **and** `reify check` on it gets past parsing (no parse error). `grammar_confirmed=true` (this leaf produces it). *Deps:* none.
- **β — substrate variants + exhaustive-match migration.** Add `Type::Applied`/`Type::Projection` (+ `Display`, factories) to `reify-core`; migrate all ~11 exhaustive matches + extend `substitute_type_params`/`unify`/`type_carries_*`/`is_representable_cell_type`/`value_type_kind_matches` (§5). *Signal:* **roped to ε**; plus no-regression — full `reify check` corpus + workspace build green; the two variants `Display` correctly. *Deps:* none. *(intentionally wide / atomic — a variant addition cannot be split.)*
- **γ — resolution: un-drop structure type-args → `Applied` + arity/bound diagnostics.** §4.2. *Signal:* `reify check` fixture — `Coupling<Prismatic>` resolves distinctly from `Coupling<Revolute>` (LSP-hover / type-resolution observable); `Coupling<NotHasMotion>` ⇒ `E_TYPE_ARG_BOUND`; `Coupling<A,B>` ⇒ `E_TYPE_ARG_ARITY`. *Deps:* β.
- **δ — assoc-binding-references-type-param + `Projection` reduction + anti-cascade.** §4.3 + §4.4: lift `checker.rs:910` `empty_params`; implement `normalize_type` reduction; remove the `:818-827`/`:832-845` rejections. *Signal:* `reify check` — `Coupling<Prismatic>::MotionValue` ⇒ `Length`, `Coupling<Revolute>::MotionValue` ⇒ `Angle`. *Deps:* α, β, γ.
- **ε — stdlib integration + integration gate (H two-way boundary leaf).** `kinematic.ri`: declare `trait HasMotion { type MotionValue }`; `Prismatic : DrivingJoint + HasMotion { type MotionValue = Length }`, `Revolute : … { type MotionValue = Angle }`; `structure def Coupling<P: DrivingJoint + HasMotion> : Joint + HasMotion { type MotionValue = P::MotionValue }`; type `couple()`'s result as `Applied{"Coupling",[parent-joint type]}` in `joint_signatures.rs`. Commit the §7 gate fixtures. *Signal:* real stdlib `reify check` — correct fixture clean, mismatch fixture ⇒ **one** targeted diagnostic, no cascade. *Deps:* α, β, γ, δ. **(This is task 4312's delivered signal.)**
- **ζ — doc / contract artifact.** Flip `docs/reify-stdlib-reference.md` MotionValue section (≈ lines 1357-1418) from "future" to "shipped"; add a short design note recording the `Applied`/`Projection` variants, the reduction algorithm, and the §0 §3.5 reversal. *Signal:* doc reflects shipped reality; stdlib-reference no longer lists `MotionValue` as unimplemented. *Deps:* ε.

**Dependency wiring:** α→δ; β→γ→δ; {α,β,γ,δ}→ε→ζ. **Out-of-batch:** `4312 → ε`.

---

## §10 — Grammar gate (G3 evidence)

Verified 2026-06-14 via `tree-sitter parse --quiet` (§3.2 table). **One** production is missing — applied-base projection `Name<Args>::member` (`grammar_confirmed=false`, owned by leaf α). All other forms (bounded structure type-params, `+`-separated multi-trait bound, structure-body `type X = Y` binding, trait assoc-type decl, type-arg application at a use site, bare/type-param projection) **parse 0-ERROR on current main**. Fixtures to commit beside the grammar tests: `f_generic_structure.ri`, `f_type_arg_application.ri`, `f_applied_base_projection.ri` (the α deliverable). Capability-manifest grammar-fixture bindings cite these.

---

## §11 — Open (tactical) questions

1. **`couple()` arg-inference vs annotation (ε).** Minimum: `couple()` result-types to `Applied{"Coupling",[P]}` from the parent arg's static joint type. If `joint_signatures.rs` cannot cleanly read the parent arg's resolved structure type, fall back to requiring an explicit `Coupling<Prismatic>` annotation at the call site and file the inference as an ε-follow-up — the dimensional-safety signal holds either way.
2. **Exact diagnostic strings/codes.** `E_TYPE_ARG_ARITY` / `E_TYPE_ARG_BOUND` names and wording are implementer's choice at γ; must be distinct from the 3974 `UnresolvedType`/`AmbiguousAssocType` family.
3. **Auto-type-param migration bookmark.** Whether to file a deferred follow-up to migrate auto-type-param's monomorphization onto `Applied` (kept out of scope, §6) — decide at decompose.
4. **`normalize_type` placement.** Extend `substitute_type_params` in place vs a sibling `normalize_type` that calls it — implementer's choice at δ, provided the `unify`/`substitute_type_params` sync invariant (§5) is preserved.
