# Associated-type projection: two new `Type` variants (as-built design note)

**Status:** as-built record — shipped across tasks β/γ/δ/ε (#4602 / #4603 / #4604 / #4605); documented here in ζ (#4606).

**PRD (normative):** `docs/prds/type-args-and-assoc-type-projection.md`  
**Stdlib reference (shipped surface):** `docs/reify-stdlib-reference.md` §13.1 (`std.mechanism.joints`)

---

## 1. The two new `Type` variants

Two variants were added to `crates/reify-core/src/ty.rs` (PRD §4.1):

```rust
/// A generic structure applied to type arguments, e.g. `Coupling<Prismatic>`.
///
/// INVARIANT: produced only with `!args.is_empty()`; a zero-arg reference
/// stays `StructureRef(name)` — one canonical form per arity.
Applied { name: String, args: Vec<Type> },      // ty.rs:266 — introduced task 4602 β

/// Assoc-type projection `base::member` held until `base` is concrete.
/// `base` ∈ { TypeParam, Applied, StructureRef }.
Projection { base: Box<Type>, member: String }, // ty.rs:275 — introduced task 4602 β
```

Both variants are **compile-time only**. `Display` renders them as `Coupling<Prismatic>` (Applied)
and `Coupling<Prismatic>::MotionValue` (Projection). `Eq`/`Hash` are derived; structural.

The variant-exhaustive `match` on `Type` forced the full migration blast radius at the β leaf
(PRD §5): `is_representable_cell_type`, `value_type_kind_matches`, `type_carries_*`,
`substitute_type_params → normalize_type`, and the `Display`/`Eq`/`Hash` impls — all updated in
task β (#4602) as a single atomic, multi-crate commit.

---

## 2. Phantom-arg runtime-erasure discipline

`Applied.args` are **compile-time only**, erased before evaluation — the same discipline as
generic-fn `type_params` erasure (`auto-type-param-resolution-completion.md` §4.2) and the
`ScalarParam` "erased before eval" precedent (`ty.rs:177`).

At runtime a `Coupling<Prismatic>` cell holds an ordinary `Value::StructureInstance` identified
by name (kind-tag `"coupling"`). The evaluation-layer discriminator `value_type_kind_matches`
(`reify-eval/src/lib.rs:243`) checks the **name only**, ignoring `args`.

The doc-comment at `ty.rs:260–263` states this invariant verbatim:

```
Applied{"Coupling",[Prismatic]} != Applied{"Coupling",[Revolute]}   ← COMPILE TIME (dimensional safety)
both hold an ordinary Value::StructureInstance{"coupling",...} at runtime  ← phantom erasure
```

Consequence: `Coupling<Prismatic>` and `Coupling<Revolute>` are **statically distinct types**
(a `J::MotionValue` mismatch fails the dimensional check at compile time), but they are the
**same runtime kind** (the FK evaluator treats all `Coupling` cells uniformly).

PRD references: §2 (phantom args / runtime erasure), §4.1 (`is_representable_cell_type` /
`value_type_kind_matches` behaviour).

---

## 3. Projection-reduction algorithm

`normalize_type` (an extension of `substitute_type_params`, `type_resolution.rs:1516`) reduces
`Projection { base, member }` once `base` is known (PRD §4.3):

| `base` after substitution | reduction |
|---|---|
| `StructureRef(S)` | existing 3974 lookup — `S`'s `assoc_types` entry for `member` (`resolve_qualified_assoc_type`) |
| `Applied { name, args }` | substitute `name`'s `type_params := args` into `name`'s `member` binding, then `normalize_type` recursively |
| `TypeParam(P)` | **irreducible** — stays `Projection` (legitimately symbolic; reduced when `P` is later substituted) (`type_resolution.rs:1100–1116`) |
| unknown member / wrong base | `Type::Error` (anti-cascade poison) + existing 3974 diagnostic; **no second diagnostic** |

**Worked chain** (`Coupling<Prismatic>::MotionValue`, from PRD §4.3):

```
Projection{ Applied{"Coupling",[StructureRef("Prismatic")]}, "MotionValue" }
  → Coupling's `MotionValue` binding is Projection{TypeParam("P"),"MotionValue"}
  → substitute P := StructureRef("Prismatic")
  → Projection{ StructureRef("Prismatic"), "MotionValue" }
  → 3974 lookup of Prismatic's `MotionValue` → Length   ∎
```

This reduction means that a `Coupling<Prismatic>::MotionValue` argument to a function expecting
`Length` passes the dimensional check cleanly, and a `Coupling<Revolute>::MotionValue` argument
to the same function produces exactly **one** targeted dimensional-mismatch diagnostic — not a
cascade.

The `Projection` binding at the definition site is stored in `CompiledAssocType.resolved` for
`Coupling<P>` as `Projection{TypeParam("P"),"MotionValue"}` (leaf δ, task #4604, which removed
the two rejection sites at `type_resolution.rs:818–827` and `:832–845` and introduced
`resolve_type_arg_for_projection`). The irreducible-TypeParam path is the exact case §3.5 did
not cover — see §4 below.

---

## 4. The §0 reversal — prior decision reversed and why

**The prior decision** (`docs/prds/v0_6/trait-associated-functions.md` §3.5 item 4, authored
2026-05-27):

> "**Type representation — adequate, no new `Type` variant needed.** A bound associated type
> resolves to an existing `Type` (`Type::StructureRef(name)` for `type Material = Steel`, or a
> constrained `Type::TraitObject(trait)` if the declaration carries a bound). … No new variant
> is required; the work is **resolution + substitution**, not a new kind."

This decision was **correct for its scope** — the *bare concrete-structure base* (`Beam::Material`),
which the task-3974 (ι_e) machinery fully reduces at resolution time to a concrete `Type`. The
comment cementing it was at `crates/reify-compiler/src/type_resolution.rs:830`:

> "PRD §3.5 adds no associated-type-projection `Type` variant"

That comment (preserved verbatim in PRD §0 L15; removed in task δ, #4604) guarded two rejection
sites (`:818–827` applied base, `:832–845` type-param base).

**Why §3.5 stayed correct for case 0 but not cases 1 and 2:**

§3.5 covered only *case 0* — a bare concrete-structure base (`Beam::Material`, fully reduced at
resolution time to a `StructureRef`). The two cases that forced new variants are:

1. **Applied base** — `Coupling<Prismatic>::MotionValue`. The base `Coupling<Prismatic>` is not
   representable as `StructureRef` (which carries no args, so `Coupling<Prismatic>` and
   `Coupling<Revolute>` would be the same `Type`). A new `Applied` variant was unavoidable.

2. **TypeParam base** — `P::MotionValue` inside `Coupling<P: DrivingJoint + HasMotion>`. `P`
   is unbound at the definition site; the projection is irreducibly symbolic until `P` is
   substituted at a call site. No existing variant can represent a pending symbolic projection.

In §3.5's terms: "resolution + substitution, not a new kind" is still correct for case 0.
Cases 1 and 2 cannot be reduced at *definition time*, so a persistent representation — the two
new variants — was unavoidable.

`docs/prds/v0_6/trait-associated-functions.md` §3.5 is not amended — it correctly describes its
scope. The reversal is an extension to a broader scope, fully documented in PRD §0
(`docs/prds/type-args-and-assoc-type-projection.md` lines 9–22).

---

**Back-links:**  
- PRD (normative spec): `docs/prds/type-args-and-assoc-type-projection.md`  
- Stdlib reference (shipped surface): `docs/reify-stdlib-reference.md` §13.1  
- Type variants + phantom-arg comment: `crates/reify-core/src/ty.rs` lines 255–275  
- Projection reducer: `crates/reify-compiler/src/type_resolution.rs` (`normalize_type` / `resolve_type_arg_for_projection`)  
- Prior decision (in-scope for case 0): `docs/prds/v0_6/trait-associated-functions.md` §3.5 item 4
