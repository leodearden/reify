# PRD: Generic Data-Carrying Enums (type-parameterized ADTs)

Status: deferred (spec-gap batch `spec-gap-2026-05-27`, cluster `generic-data-carrying-enums`). Decomposition style **B + H** (design-first contract + boundary tests) per `preferences_implementation_chain_portfolio`. Authored 2026-05-27.

Resolves the **explicitly-deferred** scope item of `docs/prds/v0_6/data-carrying-enums.md` §10 ("Generic / type-parameterized variant payloads … a separate future PRD; v1 payloads are concrete types") and spec §3.8's note that recursive ADTs ("variant type base case", §8.9) build on the non-generic data-carrying-enums feature. **Leo decided 2026-05-27: add generic enums + `Result<T,E>`.** This PRD owns the generic-enum substrate; `docs/prds/v0_6/result-and-fallback.md` Layer B (a sibling-batch follow-up) consumes it to build `Result<T,E>`.

**This PRD is a thin extension, not a new generics system.** Reify already has type parameters + `auto`-resolution for structures, occurrences, traits, and free functions (spec §3.9; `crates/reify-compiler/src/auto_type_param.rs`; `reify-core/src/ty.rs` `Type::TypeParam`; IR `reify-ir/src/traits.rs` `TypeParam`; grammar `type_parameters` / `type_parameter` rules). The work here is **plugging `enum` declarations + their named-field variant payloads into that existing machinery** — exactly as `structure_definition`, `occurrence_definition`, `trait_declaration`, and `function_definition` already carry `optional($.type_parameters)`. `enum_declaration` is the one major declaration form that does NOT (verified 2026-05-27, §4.4). Reuse-not-reinvent is the load-bearing discipline.

**Hard dependency on the non-generic feature.** Generic enums are built on the named-field-payload data-carrying-enums (DCE) feature, which is itself deferred in the same batch (`cluster:"data-carrying-enums"`, tasks 3936/3938/3940/3942/3944/3946/3949/3951). Every leaf here `depends_on` the relevant DCE leaves (named-field decl grammar, IR payload slot, pattern grammar, pattern compile, payload-binding eval). These intra-batch edges are wired (the DCE tasks now exist). See §6.

## §1 — Goal & observable surface

DCE (the sibling PRD) makes enum variants carry **concrete-typed** named-field payloads:

```reify
enum Shape { Circle { radius: Length }, Rect { width: Length, height: Length }, Point }
```

This PRD adds **type parameters** to the enum declaration and lets variant payload field types reference them:

```reify
enum Result<T, E> { Ok { value: T }, Err { error: E } }

enum Tree<T> {
    Leaf { value: T },
    Node { left: Tree<T>, right: Tree<T> }      // recursive generic enum
}
```

What a user can do when this lands (the observable surface):

```reify
structure def Demo {
    // Type args INFERRED from the construction payload: Ok { value: 5mm } ⇒ Result<Length, ?>.
    // The other parameter (E) is left as an unbound/`undef`-of-type-param until pinned by
    // annotation or by an Err construction — per the conservative-inference rule (§5 D3).
    param r : Result<Length, String> = Ok { value: 5mm }

    let bore : Length = match r {
        Ok  { value: v }  => v,        // v : Length  (T bound to Length by the annotation)
        Err { error: msg } => 6mm      // msg : String (E bound to String)
    }
}
```

Running `reify check demo.ri` accepts the file; `reify eval` reports `bore = 5 mm`. Switch the default to `Err { error: "bad" }` → `bore = 6 mm`. Constructing `Ok { value: 5mm }` against a `param r : Result<Length, String>` whose `T` is `Force` (e.g. `param r : Result<Force, String> = Ok { value: 5mm }`) produces a payload-type diagnostic (`Length` ≠ `Force`). A recursive `Tree<Length>` value built `Node { left: Leaf { value: 1mm }, right: Leaf { value: 2mm } }` matches and sums its leaves to `3mm`. That is the end-to-end signal (§8 task ε).

## §2 — Consumer (G1)

This is a **core-language / type-system capability** (parser + type resolution + IR + eval), not an in-engine seam — no `engine-integration-norm.md` §3 seam is touched (enum type-parameter resolution is a compile-time + `reify-expr` evaluation concern, never a kernel hook). It reuses the existing structure/trait/fn type-param resolver, which is likewise not an engine seam.

Named consumers:

1. **Downstream PRD — `docs/prds/v0_6/result-and-fallback.md` Layer B (`Result<T,E>`).** Layer B is *literally* a generic data-carrying enum `enum Result<T,E> { Ok { value: T }, Err { error: E } }` plus `Ok`/`Err` construction, `match`-on-`Result`, and recovery combinators. It is the **primary consumer** and the reason this PRD exists. Its tasks `depends_on` this PRD's grammar + IR + resolution leaves (§6). Direction: that PRD (Layer B) consumes this one; this PRD does not depend back.
2. **User surface — CLI eval.** `reify check` / `reify eval` over an `.ri` file declaring a generic enum, constructing a variant with inferred/annotated type args, and matching it with type-preserving payload binding. This is the primary G2 signal-bearer (§8 task ε).
3. **User surface — stdlib `.ri` example.** A checked-in example (`examples/m6_generic_enum.ri`) exercising a generic enum (incl. the recursive `Tree<T>` form) that runs in CI.
4. **Spec self-consistency — §3.8 / §3.9 / §8.9.** §3.8 says "v0.1 enums are C-style"; DCE's companion task corrects it to named-field; this PRD's companion (§8 task ζ) adds the type-parameter form `enum Name<T> { Variant { f: T } }`, ties §3.9 (type params resolved at definition/compile time) to enums, and confirms §8.9's "variant type base case" termination for recursive generic enums.

No mechanism in this PRD is a producer without one of the above consumers.

## §3 — Background: current implementation chain

Verified 2026-05-27. The chain that must change, end to end. **DCE (sibling PRD) owns the named-field-payload widenings** (rows marked **[DCE]**); this PRD owns only the **type-parameter** additions on top of them (rows marked **[here]**).

| Layer | File / site | Today | Needs |
|---|---|---|---|
| Grammar | `tree-sitter-reify/grammar.js` `enum_declaration` (lines 85–92) | `identifier (',' identifier)*` — **no `optional($.type_parameters)`, no payload** | **[DCE α]** named-field payload body; **[here]** add `optional($.type_parameters)` after the enum name (the existing `type_parameters` rule, exactly as `structure_definition` line 361 / `trait_declaration` line 167 / `function_definition` line 99 already do) so `enum Result<T, E> { … }` parses; allow variant payload field types to be a `type_expr` naming a type param |
| Grammar | `type_parameters` / `type_parameter` (lines 341–353) | exist, reused by structure/occurrence/trait/fn | **[here]** reused verbatim — wire into `enum_declaration` |
| Grammar | `type_arg_list` (lines 719–724) in type position | `Result<Length, String>` in a **type annotation** already parses (verified §4.4: 0 ERROR) | **[here]** no change — the type-position side already works; only the decl head + payload-field type-param reference is new |
| AST | `reify-ast/src/decl.rs` `EnumDecl` | `variants` (bare today) | **[DCE]** named-field variant spec; **[here]** `type_params: Vec<TypeParamDecl>` on `EnumDecl` (mirror the structure/trait AST), and variant-payload field types may be `TypeExpr::Named` referencing a param |
| IR | `reify-ir/src/traits.rs` `EnumDef` (lines 10–17) | `variants: Vec<String>`, **no `type_params`** | **[DCE γ]** `Vec<EnumVariantDef>` w/ `VariantPayload::Named(Vec<(String, Type)>)`; **[here]** add `type_params: Vec<TypeParam>` to `EnumDef` (the IR `TypeParam` struct at traits.rs:30 already exists — `TraitDef` carries `type_params: Vec<TypeParam>` at line 94; mirror it), and payload field `Type`s may be `Type::TypeParam(name)` |
| Type | `reify-core/src/ty.rs` `Type::Enum(String)` (line 44) | enum name only, **no type-args slot** | **[here]** decide: carry resolved type args on the enum-typed value/annotation, or erase them at compile time (fork F-Mono; default = **erase**, matching the structure side — see below) |
| Type | `Type::TypeParam(String)` (line 59) | exists; used by structure/trait/fn generics | **[here]** reused for enum payload field types pre-resolution |
| Construct | `reify-compiler` variant construction (DCE δ) | DCE checks concrete payload field types | **[here]** when the variant's declared field type is a `Type::TypeParam`, **infer** the type arg from the supplied payload value's type and check consistency across fields (§5 D3) |
| Pattern | `reify-compiler` pattern compile (DCE ε) | DCE binds payload fields to cells at concrete types | **[here]** binders bound at the **substituted** type (`T` → inferred/annotated arg), so `match` arms see correctly-typed binders (§5 D4) |
| Eval | `reify-expr` payload-binding match eval (DCE ζ) | DCE cracks the payload field map, binds names | **[here]** unchanged at value level — payload is `Vec<(String, Value)>`; type erasure means eval is type-arg-agnostic (the binder cell already holds the right `Value`); this PRD adds **no eval change** under the erase default (fork F-Mono) |

**Resolved fork F-Mono — type-erasure at compile time (matches the structure side).** Verified 2026-05-27: structure generics carry **no** type args at runtime — `Value::StructureInstance(StructureInstanceData)` (value.rs:611) stores only `type_name`, no resolved type args; spec §3.9 states type parameters are "Resolved at definition time (compile time)." Generic enums follow the same model: type args are resolved/checked at compile time and **erased** before eval. A `Value::Enum` (DCE's `{ type_name, variant, payload }`) carries no type-arg tags at runtime — the payload `Value`s are already concrete. This means **eval is unchanged** (DCE ζ already handles the value level); the generic work is purely parser + AST + IR-shape + compile-time resolution/inference. No monomorphization-by-cloning (the spec's value model never materializes per-instantiation enum copies — a generic enum has one `EnumDef` with `Type::TypeParam` payload fields, resolved per-use-site). This is the smallest blast radius and is consistent with the rest of the language.

## §4 — Sketch of approach

### 4.1 Generic enum declaration

```reify
enum Result<T, E> { Ok { value: T }, Err { error: E } }
enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }
```

`enum_declaration` gains `optional($.type_parameters)` immediately after the name field — the **identical** position structure/trait/fn use. Type parameters may carry bounds (`T: SomeTrait`) and defaults (`T = Length`) via the existing `type_parameter` rule (lines 349–353), at no extra grammar cost. A variant payload field type (`value: T`, `left: Tree<T>`) is an ordinary `type_expr` that may name an in-scope type parameter (lowered to `Type::TypeParam`) or apply the enum recursively (`Tree<T>`, a `Type::Enum("Tree")` whose type-arg is the param — though under F-Mono the arg is checked then erased).

Field types that reference an undeclared type parameter (`value: U` where `U ∉ {T, E}`) are a compile diagnostic (`E_ENUM_UNKNOWN_TYPE_PARAM`).

### 4.2 Construction with type-arg inference

```reify
Ok { value: 5mm }                          // infers T = Length; E unbound (undecided)
Err { error: "parse failed" }              // infers E = String; T unbound
let r : Result<Length, String> = Ok { value: 5mm }   // T,E pinned by annotation; payload checked against T
Leaf { value: 1mm }                        // Tree<Length>, T = Length
```

Construction reuses DCE's named-field variant construction (DCE δ) and adds **type-arg inference from payload values** (§5 D3):
- Each payload field whose declared type is a `Type::TypeParam(P)` **binds** `P` to the supplied value's type. All fields binding the same `P` must agree (`E_ENUM_TYPE_ARG_CONFLICT` if `Node { left: Leaf{value:1mm}, right: Leaf{value:1N} }` would bind `T` to both `Length` and `Force`).
- A type parameter not mentioned by any constructed field's payload (e.g. `E` when only `Ok { value: … }` is built) is **left unbound** — it becomes determined only by an explicit type annotation, by a sibling `Err` construction in the same value's context, or stays an unresolved param the way the structure-side leaves unconstrained `auto` params (conservative inference, spec §3.9 "Infer type parameters when context unambiguously determines them; never over-infer").
- A type annotation (`param r : Result<Length, String>`) **pins** all params; the construction payload is then checked against the pinned types (`Ok { value: 5mm }` against `T = Length` ✓; against `T = Force` → `E_VARIANT_PAYLOAD_TYPE`, the DCE diagnostic, now type-param-aware).

`auto`-type-param resolution (`Result<auto: SomeTrait, E>`) reuses the existing `resolve_auto_type_params` machinery (auto_type_param.rs) — it is **not re-implemented**; the enum's `EnumDef.type_params` feed the same Phase A/B/C resolver structures already used for structures (§9 G6 validates this is decidable for enums).

### 4.3 Type-preserving pattern matching

```reify
match r {
    Ok  { value: v }  => v,        // v : T-substituted  (Length, given Result<Length, String>)
    Err { error: msg } => msg       // msg : E-substituted (String)
}
```

Pattern matching reuses DCE's payload-binding pattern (DCE β grammar, DCE ε compile). The **only** addition: when the matched discriminant's enum type has resolved type args, the binder cells are typed at the **substituted** payload-field type (`T` → `Length`), so the arm body type-checks against the right type. Under F-Mono erasure, the *value* in the binder cell is already concrete; this addition is a **compile-time typing** refinement (the binder's static type), not an eval change.

### 4.4 Recursive generic enums (termination)

`enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }` is recursive. **Termination is by data construction, not eager structural unfolding** (§5 D5): a `Value::Enum` is a finite runtime value built bottom-up (`Leaf` first, then `Node` referencing already-built children). This differs fundamentally from recursive *structures* (spec §8.9), whose eager unfolding is depth-controlled by a `where`-guarded `sub`. An enum *value* cannot be infinite — there is no construction expression that builds an unbounded value in one step. The spec §8.9 "variant type base case" termination mechanism is exactly the `Leaf` (non-recursive) variant. **No termination checker is needed** (consistent with spec §note: "The compiler does not attempt termination checking"; infinite recursion in a *function* building such a value is a runtime stack overflow, as today). G6 §9 validates this.

### 4.5 Grammar reality check (G3) — fixtures (tree-sitter 0.26.8, 2026-05-27)

Per the silent-misparse trap, the signal is the **CST ERROR/MISSING-node count**, not exit code alone. Run from `tree-sitter-reify/`.

| Fixture | Syntax | ERROR/MISSING | Verdict |
|---|---|---|---|
| `gde-0-baseline.ri` | bare enum + bare-variant match | **0** | clean floor (regression pin) |
| `gde-5-typeann.ri` | `param r : Result<Length, String> = undef` (type position only) | **0** | **`Result<…>` as a TYPE already parses** (existing `type_arg_list`) — only the decl head + value side is novel |
| `gde-6-genbarevariants.ri` | `enum Maybe<T> { Nothing, Just }` (type param, **bare** variants) | **3** | **isolates the type-param-on-enum gap**: even with no payload, `<T>` after the enum name derails parsing → `enum_declaration` needs `optional($.type_parameters)` |
| `gde-7-nongennamed.ri` | `enum Shape { Circle { radius: Length }, Point }` (named-field, **non**-generic) | **7** | the DCE named-field payload gap (DCE α) — owned by DCE, not here |
| `gde-1-genenumdecl.ri` | `enum Result<T, E> { Ok { value: T }, Err { error: E } }` | **10** | **both gaps**: type-param-on-enum [here] + named-field payload referencing a param [DCE α + here] |
| `gde-2-rectree.ri` | `enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }` | **13** | recursive generic — same two gaps + recursive payload-field type |
| `gde-3-genconstruct.ri` | `let r = Ok { value: 5mm }` (brace construction) | **5** | DCE brace-construction gap (DCE δ / DCE fork F2) — inherited, owned by DCE |
| `gde-4-genmatch.ri` | `match r { Ok { value: x } => x, Err { error: e } => 0mm }` | **14** | DCE payload-binding pattern gap (DCE β/ε) — inherited, owned by DCE |

**G3 resolution.** Two grammar gaps, with clean ownership:
- **Type parameters on `enum_declaration`** (`<T, E>` head + payload-field type-param references) — **net-new for enum, owned HERE** (§8 task α). Isolated by `gde-6` (3 ERROR nodes from `<T>` alone). `grammar_confirmed=false` on task α.
- **Named-field variant payload grammar + binding patterns + brace construction** — **owned by DCE** (DCE tasks α/β, brace-construction per DCE fork F2 at DCE δ). Generic-enum tasks `depends_on` them; `grammar_confirmed` on the consuming generic-enum leaves is `false` until DCE α/β land (they create the substrate this PRD parses against).

## §5 — Resolved design decisions

- **D0 — Thin extension of existing type-param machinery, not a new generics system.** Verified: `type_parameters`/`type_parameter` grammar rules exist and are reused by structure/occurrence/trait/fn; `Type::TypeParam`, IR `TypeParam`, and `resolve_auto_type_params` exist. `enum_declaration` is the one declaration form missing `optional($.type_parameters)`. This PRD wires enum into all of that; it does not invent resolution, substitution, or `auto` machinery.
- **D1 — Type-erasure at compile time (fork F-Mono, RESOLVED).** Type args are resolved/checked at compile time and **erased** before eval, matching the structure side (`Value::StructureInstance` carries no type args; spec §3.9 "resolved at definition time"). `Value::Enum` carries **no** type-arg runtime tag; payload `Value`s are already concrete. **Eval is unchanged** beyond DCE ζ. No monomorphization-by-cloning. Smallest blast radius; consistent with the language.
- **D2 — One `EnumDef` per generic enum, with `Type::TypeParam` payload fields.** A generic enum has a single `EnumDef { type_params, variants: [EnumVariantDef { payload: Named([(field, Type::TypeParam("T")), …]) }] }`. Per-use-site resolution substitutes `T` → concrete type for **type-checking only**; the IR `EnumDef` is not cloned per instantiation. Mirrors how `CompiledTrait`/structure generics keep one definition + per-site substitution.
- **D3 — Construction type-arg inference is conservative + payload-driven.** Each payload field of declared type `Type::TypeParam(P)` binds `P` to the supplied value's type; all fields binding the same `P` must agree (else `E_ENUM_TYPE_ARG_CONFLICT`). Params not mentioned by any constructed field stay **unbound** until pinned by annotation. A type annotation pins all params and the payload is checked against the pinned types. This reuses the spec §3.9 "infer when context unambiguously determines" rule — never over-infer an unconstrained param.
- **D4 — Pattern binders are typed at the substituted payload field type.** In `match r { Ok { value: v } => … }` over `Result<Length, String>`, `v` has static type `Length` (the substituted `T`). Compile-time only (under D1 erasure the runtime value is already concrete). Reuses DCE ε's binder-cell mechanism; adds the substitution of the binder's *type*.
- **D5 — Recursive generic enums terminate by data construction, not eager unfolding.** A `Value::Enum` is a finite bottom-up-built value; there is no single construction expression that builds an unbounded value (unlike depth-guarded recursive *structures*, §8.9). The non-recursive variant (`Leaf`) is the §8.9 "variant type base case". No termination checker; infinite recursion via a recursive *function* building such a value is a runtime stack overflow as today (spec §note). This honours §8.9 without new machinery.
- **D6 — `auto` type-param resolution reuses `resolve_auto_type_params`.** `Result<auto: SomeTrait, E>` (if a user writes it) feeds `EnumDef.type_params` through the **existing** Phase A/B/C resolver (auto_type_param.rs). No enum-specific `auto` algorithm. (v1 does not require `auto` on enums to be exercised by `Result`; it falls out of the reuse for free — see §10 / §11 Q2 for the tactical scope.)
- **D7 — `undef`/back-compat.** Bare-variant enums and bare-variant matches (today + DCE) keep working: a non-generic enum is the empty-`type_params` case. DCE's D2 undef-payload rule (determined tag + undef payload field selects the arm, binds undef) is inherited unchanged. `Value::Enum` with empty `type_params` and `Unit` payload reproduces today's bare-enum behaviour bit-for-bit (DCE INV-5 preserved transitively).

## §6 — Cross-PRD / cross-cluster relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/data-carrying-enums.md` (cluster `data-carrying-enums`, tasks 3936…3951) | this **consumes**, that **produces** | named-field variant payload grammar/IR (DCE α/γ), payload-binding `match` pattern (DCE β/ε), payload-binding eval (DCE ζ), brace/call construction (DCE δ / DCE fork F2) | **DCE owns** the named-field payload mechanism; this PRD owns only the **type-parameter** layer on top | **wired** — this PRD's tasks `depends_on` DCE α (3936 named-field decl grammar), DCE γ (3940 IR payload slot), DCE β-pattern (3944), DCE ε pattern-compile, DCE ζ eval (3946). Intra-batch edges declared §8 + wired (DCE tasks exist). |
| `docs/prds/v0_6/result-and-fallback.md` Layer B (`Result<T,E>`) | this **produces**, that **consumes** | type-param grammar on `enum`, generic construction inference, type-preserving generic pattern match — the substrate for `enum Result<T,E> { Ok {value:T}, Err {error:E} }` | **this-prd** (generic-data-carrying-enums) | Layer B's tasks `depends_on` this PRD's α (grammar), γ (IR), δ (construction inference), ε (eval/integration). Result-and-fallback PRD updated this session: fork F1 flipped to "Layer B built", §6 generic-enum row resolved to "owned by generic-data-carrying-enums". |
| `docs/prds/v0_3/structure-instance-runtime.md` (GR-001) | independent | — | n/a | F-Mono default = type-erasure with no runtime type-arg carrier; payload stays DCE's inline `Vec<(String, Value)>` map (DCE fork F1 default). **No GR-001 edge.** |

**Seam ownership statement (G4).** The match seam's payload-binding extension is owned by DCE; its **type-parameter** extension (typed binders, type-arg inference at construction) is owned **here**. No contested-ownership pair from `phase-3-breadcrumb-map.md` §3 is touched (none involves enums or generics). The `auto`-resolution seam is **reused, not re-owned** — this PRD plugs enum `type_params` into the existing `resolve_auto_type_params` owner; it does not fork or duplicate it.

**Tuple constraint (Leo, this session):** tuples are NOT being added. Type parameters parameterize **named-field** payloads only (`Ok { value: T }`, never `Ok(T)` and never a `(T,E)` tuple). `Result<T,E>` is a two-type-param enum, not a tuple-payload enum. Confirmed consistent with DCE's named-field-only decision.

## §7 — Contract section (B+H)

The seam is between `reify-compiler` (parses type params, resolves/infers type args, substitutes for type-checking, checks payload field types against substituted params) and `reify-expr` (evaluates — **unchanged** under D1 erasure; the contract's eval side is "the value model is type-arg-agnostic"). The two sides face these structures.

### 7.1 AST / IR data structures (the contract surface)

```rust
// reify-ast/src/decl.rs — EnumDecl gains type_params (DCE adds named-field variant spec)
pub struct EnumDecl {
    pub name: String,
    pub type_params: Vec<TypeParamDecl>,   // [here] empty for non-generic; mirrors StructureDecl
    pub variants: Vec<EnumVariantDecl>,    // [DCE] name + named-field payload (field types may be TypeExpr::Named("T"))
    // …doc, span…
}

// reify-ir/src/traits.rs — EnumDef gains type_params (DCE adds Vec<EnumVariantDef>/VariantPayload)
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<TypeParam>,       // [here] reuses the existing IR TypeParam struct (traits.rs:30)
    pub variants: Vec<EnumVariantDef>,     // [DCE] payload: VariantPayload::Named(Vec<(String, Type)>)
    pub doc: Option<String>,               //        — a payload field Type MAY be Type::TypeParam(name)
}

// reify-core/src/ty.rs — Type::Enum: NO type-arg slot added (F-Mono = erase). Resolved args
// live only in the compiler's per-site substitution map, never on the persisted Type or Value.
//   Type::Enum(String)        // unchanged — name only
//   Type::TypeParam(String)   // unchanged — reused for unresolved enum payload field types

// reify-ir/src/value.rs — Value::Enum (DCE shape) is UNCHANGED by this PRD:
//   Value::Enum { type_name: String, variant: String, payload: Vec<(String, Value)> }
//   — no type-arg tag (D1 erasure); payload Values are already concrete.
```

(`type_params` on `EnumDef` is the IR `TypeParam` struct already used by `TraitDef.type_params` — reused verbatim. The substitution map `T → concrete Type` is a compile-time-only structure analogous to the structure-side resolution; it is **not** persisted on the IR or Value.)

### 7.2 Invariants

- **INV-1 (reuse, not reinvent).** Enum type parameters use the same `type_parameters`/`type_parameter` grammar, the same `Type::TypeParam`, the same IR `TypeParam`, and the same `resolve_auto_type_params` resolver as structure/trait/fn generics. No enum-specific generics machinery is introduced. (Verified by a test that the enum `EnumDef.type_params` are the same `TypeParam` type and feed the same resolver entry point.)
- **INV-2 (erasure / value model unchanged).** `Value::Enum` carries no type-arg tag; eval is type-arg-agnostic; a generic enum value at runtime is indistinguishable from a non-generic enum value with the same concrete payload. (Boundary test: `Ok { value: 5mm }` and a hypothetical non-generic `OkLength { value: 5mm }` produce structurally-identical `Value::Enum` payloads.)
- **INV-3 (conservative inference).** A type parameter is bound only when payload values or an annotation unambiguously determine it (D3). An unconstrained param is left unbound, never guessed. Construction with conflicting payload types for the same param → `E_ENUM_TYPE_ARG_CONFLICT`; never silently picks one.
- **INV-4 (typed binders).** A `match` binder over a resolved generic enum has the substituted payload-field type (D4); the arm body type-checks at that type. (Test: `Ok { value: v }` over `Result<Length, String>` makes `v + 1mm` check and `v + 1N` a type error.)
- **INV-5 (recursive termination by construction).** A recursive generic enum needs no termination checker; its non-recursive variant is the §8.9 base case; a finite value is built bottom-up (D5). (Test: a 3-node `Tree<Length>` evaluates; no static termination error is emitted for a well-formed recursive enum decl.)
- **INV-6 (back-compat).** Empty `type_params` reproduces DCE / today's bare-enum behaviour bit-for-bit (D7); `gde-0-baseline.ri` and `m5_guarded_enum.ri` stay green.

### 7.3 Error semantics (user-visible diagnostics — G2 leaf signals)

| Code (illustrative) | Trigger | Where |
|---|---|---|
| `E_ENUM_UNKNOWN_TYPE_PARAM` | `enum E<T> { V { x: U } }` — payload field type names an undeclared param `U` | compiler, decl |
| `E_ENUM_TYPE_ARG_CONFLICT` | `Node { left: Leaf {value: 1mm}, right: Leaf {value: 1N} }` — same param `T` bound to two types | compiler, construction |
| `E_VARIANT_PAYLOAD_TYPE` (DCE, extended) | `Ok { value: 5mm }` against pinned `T = Force` — payload value type ≠ substituted param type | compiler, construction |
| `E_ENUM_TYPE_ARG_UNRESOLVED` | a param undetermined by inference and unpinned by annotation at a site that needs it concrete | compiler, resolution |
| (existing `auto`) `E_AUTO_TYPE_PARAM_*` | `Result<auto: Trait, E>` resolution failure — reused from the existing resolver | compiler (unchanged) |

## §8 — Decomposition plan (DAG; not yet filed) — Greek labels; real IDs at decompose

**B + H.** Grammar leaf first (the one net-new grammar gap owned here, gated on DCE's named-field grammar), then IR widening, then construction-inference + pattern-typing seam sides, then the end-to-end consumer leaf (integration gate), then the spec companion. All leaves `depends_on` the relevant DCE leaves (the named-field substrate).

### Phase 1 — Grammar (the one net-new gap owned here; `grammar_confirmed=false`)

- **Task α — Type parameters on `enum_declaration` + payload-field type-param references + parser test + AST lowering.**
  - Add `optional($.type_parameters)` to `enum_declaration` (the existing rule, exactly as structure/trait/fn). Allow a variant payload field type to be a `type_expr` naming a type param (`value: T`) or a recursive enum application (`left: Tree<T>`). Lower to `EnumDecl.type_params: Vec<TypeParamDecl>` + payload field types as `TypeExpr` in `reify-ast`. Emit `E_ENUM_UNKNOWN_TYPE_PARAM` for an undeclared param reference.
  - **Observable signal:** `gde-6-genbarevariants.ri` (`enum Maybe<T> { Nothing, Just }`) parses with **0 ERROR/MISSING nodes** (isolates the type-param head); `gde-1-genenumdecl.ri` (`enum Result<T,E> { Ok {value:T}, Err {error:E} }`) parses with **0 ERROR/MISSING nodes** once DCE α (named-field body) is in; a parser test in `tree-sitter-reify/tests/` asserts the type-param-on-enum production + a payload field referencing a param; `gde-0-baseline.ri` (bare enum) still parses with 0 ERROR nodes. `grammar_confirmed=false`.
  - **Crates:** tree-sitter-reify, reify-ast, reify-syntax. **Prereqs (intra-batch):** DCE 3936 (named-field decl grammar — the variant body this layers `<T>` onto).

### Phase 2 — IR widening (intermediate; unlocks the seam sides)

- **Task β — `EnumDef.type_params` (reuse IR `TypeParam`) + `Type::TypeParam` payload field types.**
  - Add `type_params: Vec<TypeParam>` to `EnumDef` (the existing IR `TypeParam` struct, mirroring `TraitDef.type_params`); allow a `VariantPayload::Named` field `Type` to be `Type::TypeParam(name)`. Lower `EnumDecl.type_params` → `EnumDef.type_params`. Confirm one `EnumDef` per generic enum (D2), not per-instantiation.
  - **Observable signal (intermediate):** unit tests in `reify-ir`/`reify-compiler` pin: `enum Result<T,E>{…}` lowers to an `EnumDef` with `type_params == [T, E]` (same `TypeParam` type as a trait's) and a payload field of `Type::TypeParam("T")`; a non-generic enum lowers to empty `type_params` (INV-6); INV-1 reuse test (enum `type_params` feed the same resolver entry point as structure/trait). **Unlocks:** γ, δ. **Consumer:** §8 tasks γ/δ.
  - **Crates:** reify-ir (traits.rs), reify-compiler (enum lowering). **Prereqs (intra-batch):** α; DCE 3940 (IR payload slot — the `EnumVariantDef`/`VariantPayload` this adds `type_params` alongside).

### Phase 3 — Producer side (compiler: construction-inference + pattern-typing)

- **Task γ — Type-arg inference at generic-variant construction + payload-vs-param type check.**
  - Extend DCE's named-field construction (DCE δ): when a variant payload field's declared type is `Type::TypeParam(P)`, infer `P` from the supplied value's type, enforce agreement across same-`P` fields (`E_ENUM_TYPE_ARG_CONFLICT`), check against an annotation's pinned args (extend `E_VARIANT_PAYLOAD_TYPE` to be type-param-aware), leave unmentioned params unbound (D3/INV-3), emit `E_ENUM_TYPE_ARG_UNRESOLVED` where a concrete arg is required but undetermined. Plug `EnumDef.type_params` into the existing `resolve_auto_type_params` for the (optional) `auto`-on-enum path (D6) — reuse, no new resolver.
  - **Observable signal:** `reify check` over `Ok { value: 5mm }` against `param r : Result<Force, String>` emits the (type-param-aware) payload-type diagnostic; `Node { left: Leaf {value: 1mm}, right: Leaf {value: 1N} }` emits `E_ENUM_TYPE_ARG_CONFLICT`; a valid `Ok { value: 5mm } : Result<Length, String>` checks clean with `T = Length` inferred/pinned. (CLI diagnostics — user-observable leaf.)
  - **Crates:** reify-compiler (construction disambiguation/field_check, type_resolution, auto_type_param reuse). **Prereqs (intra-batch):** β; DCE 3942 (DCE δ named-field construction — extends it). *(DCE δ id read from the DCE task set; see §return.)*

- **Task δ — Type-preserving pattern binders over generic enums.**
  - Extend DCE's pattern compile (DCE ε): a `match` binder over a discriminant of a resolved generic enum type is typed at the **substituted** payload field type (D4); the arm body type-checks at that type. No new grammar (pattern grammar is DCE β). Preserve tag-only exhaustiveness (DCE D4).
  - **Observable signal:** `reify check` over `match r { Ok { value: v } => v + 1mm, … }` with `r : Result<Length, String>` checks clean (`v : Length`); changing the body to `v + 1N` emits a type-mismatch diagnostic; `match` on a non-generic enum is unchanged (INV-6). (CLI diagnostics — user-observable leaf. Parallel with γ.)
  - **Crates:** reify-compiler (pattern compile, type_resolution). **Prereqs (intra-batch):** β; DCE 3944 (DCE β/ε payload-binding pattern — extends its binder typing).

### Phase 4 — Consumer side (eval) + end-to-end integration gate

- **Task ε — Generic-enum end-to-end eval example (THE integration gate / boundary test).**
  - Confirm eval is **unchanged** under D1 erasure (the boundary test for INV-2: a generic-enum `Value::Enum` evaluates through DCE ζ's payload-binding match with no type-arg awareness). Author the consumer example and pin the recursive case.
  - **Observable signal (LEAF — primary):** `examples/m6_generic_enum.ri` declares `enum Result<T,E> { Ok {value:T}, Err {error:E} }` and `enum Tree<T> { Leaf {value:T}, Node {left:Tree<T>, right:Tree<T>} }`, sets `param r : Result<Length, String> = Ok { value: 5mm }`, computes `let bore = match r { Ok {value:v} => v, Err {error:m} => 6mm }`. `reify eval` reports `bore = 5 mm`; switch to `Err { error: "bad" }` → `bore = 6 mm`. A recursive `Tree<Length>` value `Node { left: Leaf {value: 1mm}, right: Leaf {value: 2mm} }` sums (via a recursive fn or a two-arm match) to `3 mm`. The INV-2 boundary test (in `reify-expr`/`reify-eval` tests) asserts the generic-enum value evaluates with a type-arg-agnostic eval path. Example runs in CI. (This is the §1 signal; the B+H integration gate — α/β/γ/δ are its intermediates.)
  - **Crates:** reify-expr (confirm no change), reify-eval (boundary test), examples/, reify-cli (eval path). **Prereqs (intra-batch):** γ, δ; DCE 3946 (DCE ζ payload-binding eval — this evaluates through it unchanged).

### Phase 5 — Companion correction (doc; independent)

- **Task ζ — Spec §3.8 / §3.9 / §8.9 update for generic data-carrying enums.**
  - Add the type-parameter enum form `enum Name<T> { Variant { f: T } }` to §3.8 (after DCE's named-field correction), tie §3.9 (type params resolved at definition/compile time + `auto` resolution) to enums, document type-erasure (D1) and conservative construction inference (D3), and state §8.9's "variant type base case" termination for recursive generic enums (D5). Update grammar EBNF for the enum-with-type-params production.
  - **Observable signal:** `docs/reify-language-spec.md` updated; the `gde-*` fixtures (`gde-0-baseline`, `gde-1-genenumdecl`, `gde-2-rectree`, `gde-6-genbarevariants`) referenced; no code change; doc lint passes.
  - **Crates:** none (docs). **Prereqs (intra-batch):** ε (describe what landed).

### Dependency view

```
DCE 3936 ─→ α ─┐
DCE 3940 ──────┴─→ β ─┬─→ γ ─┐
DCE 3942 ────────────→ γ      ├─→ ε ─→ ζ
DCE 3944 ────────────→ δ ─────┘
DCE 3946 ────────────────────→ ε
```

(α adds the type-param grammar onto DCE's named-field body; β widens the IR; γ/δ are the construction-inference and pattern-typing seam sides; ε is the integration gate evaluating through DCE's unchanged eval; ζ documents what landed.) Every generic-enum leaf has an intra-batch DCE prerequisite (the named-field substrate). **No out-of-batch prereqs** (DCE is in the same `spec-gap-2026-05-27` batch). `grammar_confirmed=false` on α (creates grammar); γ/δ/ε/ζ are `grammar_confirmed=false` only because they consume DCE α/β grammar not yet landed — flip to true once DCE α/β are confirmed.

## §9 — Premise validation (G6)

Every §8 leaf signal classified:

- **ε primary signal — end-to-end capability** (`bore = 5 mm` / `6 mm`; `Tree<Length>` sums to `3 mm`). Trace: requires (a) type-param-on-enum parse [α], (b) `EnumDef.type_params` + `Type::TypeParam` payload [β], (c) construction inference [γ], (d) typed pattern binders [δ], (e) type-arg-agnostic eval through DCE ζ [ε + DCE 3946]. Every capability is in ε's dependency set (α→β→γ/δ→ε, + DCE leaves); none is owned by a task depending on ε. **Passes** the dependency-set trace. The arithmetic (`1mm + 2mm = 3mm`, value selection `Ok{value:5mm} → 5mm`) is exact `Scalar<Length>` already in the language — no new numeric capability. **Achievable.**
- **Type-arg inference decidability premise (D3).** Construction inference is **payload-driven unification** of each `Type::TypeParam` field against the supplied value's concrete type — a finite, decidable, single-pass check (no general HM inference, no recursion across use sites). Conflicting bindings for one param → diagnostic; unmentioned params → unbound (not guessed). This is strictly **less** than the existing structure-side `auto` resolution (which is already shipped and decidable, capped at 10 candidates). **Decidable — premise holds.**
- **Recursive generic enum termination premise (D5/INV-5).** A `Value::Enum` is a finite bottom-up-built value; no single construction expression builds an unbounded value (unlike eager depth-unfolded recursive *structures*, §8.9). The non-recursive variant is the §8.9 "variant type base case". No termination checker needed (spec §note: compiler does not attempt termination checking; infinite recursion in a value-building *function* is a runtime stack overflow as today). **Consistent with spec §8.9 — premise holds, no impossible static-termination claim.**
- **`auto`-on-enum reuse premise (D6/INV-1).** Plugging `EnumDef.type_params` into the existing `resolve_auto_type_params` Phase A/B/C resolver is a **reuse**, not a new algorithm; the resolver is type-source-agnostic (it takes `TypeParam` bounds + an in-scope candidate pool). Verified the resolver exists and is parameterized over `TypeParam`/bounds, not over "structure" specifically. **Reuse is sound — premise holds.**
- **α signal — `tree-sitter parse` 0 ERROR/MISSING.** Mechanically verifiable (CST shape). `gde-6` (3 ERROR today) / `gde-1` (10 ERROR today) re-verified 2026-05-27 (§4.5), which is exactly why α exists. **No false premise.**
- **γ, δ signals — diagnostic emission** (`E_ENUM_TYPE_ARG_CONFLICT`, type-param-aware `E_VARIANT_PAYLOAD_TYPE`, body type-mismatch). No quantitative premise; pass trivially. Codes illustrative (§7.3, tactical §11 Q1).

No leaf asserts an accuracy bound, closed-form reproduction, or a capability owned downstream. **G6 clear.**

## §10 — Out of scope for this PRD

- **Generic *structures*/*traits*/*fns*.** Already exist (spec §3.9); not re-touched. This PRD adds only the **enum** declaration form to the existing machinery.
- **Higher-kinded / variance / where-clauses on enum type params** beyond the existing `T: Trait` bound + `T = Default` forms the `type_parameter` rule already provides. No `T: Trait where …`, no variance annotations.
- **Monomorphization-by-cloning / runtime type-arg reflection.** F-Mono = erasure (D1); no per-instantiation `EnumDef` copies, no runtime `type_args` on `Value::Enum`. A future PRD could add reified type args if a use case demands runtime type introspection — not now.
- **Generic `auto`-on-enum as a *required* exercised path.** The `auto` reuse (D6) falls out for free, but v1's acceptance does not hinge on a `Result<auto: Trait, E>` example; `Result<T,E>` (the primary consumer) uses explicit/inferred args. Exercising `auto`-on-enum end-to-end is deferred (§11 Q2) — the machinery is wired (INV-1) but not gated by a leaf signal here.
- **Positional payloads / tuples.** Out (Leo): payloads are DCE named-field only; type params parameterize named-field types, never positional or tuple types.
- **Nested / multi-level destructuring of generic payloads** (`Node { left: Leaf { value: v } }` in a single pattern) — DCE defers nested patterns (DCE §10); inherited here.
- **`Result<T,E>` itself + recovery combinators + `?`/fallback over `Result`.** Owned by `result-and-fallback.md` Layer B (consumes this PRD); not filed in this cluster.

## §11 — Open questions (tactical; decide at impl)

1. **Exact diagnostic codes/strings** (`E_ENUM_UNKNOWN_TYPE_PARAM`, `E_ENUM_TYPE_ARG_CONFLICT`, `E_ENUM_TYPE_ARG_UNRESOLVED` illustrative). Decide at α/γ against the diagnostic-code registry; reuse/extend DCE's `E_VARIANT_PAYLOAD_TYPE` rather than minting a new payload-type code.
2. **`auto`-on-enum test surface** — whether to add a `Result<auto: Trait, E>` resolution test in this batch or defer to a follow-up once a real `auto`-enum use case appears. Tactical; the machinery is reused either way (INV-1). Decide at γ.
3. **Substitution-map representation** — a `Vec<(String, Type)>` (param → resolved type), reusing the structure-side `resolve_auto_type_params` return shape (`Vec<(String, String)>`-analogous), vs a dedicated enum-resolution struct. Tactical; either drives the same compile-time check. Decide at γ.
4. **Type-arg display in diagnostics** — whether a payload-type error prints `Result<Length, String>` (resolved) or `Result<T, E>` (declared) form. UX-tactical; decide at γ against the existing `Display` for `Type::Enum`/`Type::TypeParam`.

## DESIGN FORKS FOR LEO

> AskUserQuestion does not route to this session; defaults below are reasoned and the PRD is internally consistent under them.

### F-Mono — type-erasure vs monomorphization for generic enums *(default: ERASURE — RESOLVED, matches structure side; flagged for confirmation)*

The one genuinely architectural choice. Resolved to **erasure** in §5 D1, but surfaced here because it sets the runtime value model.

- **Default F-Mono-a — type-erasure at compile time (RESOLVED, recommended).** Type args resolved/checked at compile time, erased before eval; `Value::Enum` carries no type-arg tag; eval unchanged beyond DCE ζ. **Matches the structure side exactly** (`Value::StructureInstance` carries no type args; spec §3.9 "resolved at definition time, compile time"). Smallest blast radius; one `EnumDef` per generic enum. **Con:** no runtime type-arg introspection (you can't ask a `Result` value "what is E?" at eval — but nothing in scope needs that).
- **Alt F-Mono-b — monomorphization / runtime type args.** Carry resolved type args on `Value::Enum` (or clone `EnumDef` per instantiation). **Pro:** runtime type introspection; closer to Rust's monomorphized generics. **Con:** diverges from the structure side (which erases), adds a runtime type-arg carrier to the value model (blast radius into content-hash/Eq/Ord, serialization, every `Value::Enum` site), and buys nothing the primary consumer (`Result<T,E>`) needs. Inconsistent with spec §3.9's "resolved at compile time" stance.
- **Impact:** load-bearing for the runtime value model + eval scope. Erasure keeps eval unchanged (ε is a no-eval-change boundary test) and consistent with the rest of the language. *Lean: F-Mono-a — erase, match the structure side. Flagging only because it's the one architectural commitment; if Leo wants runtime type-arg reflection on enums, that's F-Mono-b and a larger value-model change.*

## Assumptions

- The `type_parameters`/`type_parameter` grammar rules (lines 341–353) are reusable into `enum_declaration` exactly as they are reused by `structure_definition`/`trait_declaration`/`function_definition`. **Verified 2026-05-27** — the rules are standalone; `gde-6` confirms `<T>` after the enum name fails only because `enum_declaration` lacks `optional($.type_parameters)`, not because the rule is incompatible.
- Structure/trait/fn generics are **type-erased at compile time** (no runtime type args). **Verified** — `Value::StructureInstance(StructureInstanceData)` (value.rs:611) stores only `type_name`; spec §3.9 states "resolved at definition time (compile time)". Generic enums match this model (D1/F-Mono-a).
- `resolve_auto_type_params` (auto_type_param.rs) is parameterized over `TypeParam` bounds + an in-scope candidate pool, not hard-wired to structures. **Verified** — the Phase A/B/C resolver takes `AutoTypeParam`/bounds; enum `type_params` (the same IR `TypeParam` type as `TraitDef.type_params`, traits.rs:30/94) feed it without a signature change (D6/INV-1).
- DCE (`data-carrying-enums.md`, tasks 3936…3951) lands the named-field payload substrate (decl grammar, IR payload slot, pattern grammar, pattern compile, payload-binding eval). **This PRD's leaves `depends_on` the relevant DCE leaves** — generic enums are a thin type-parameter layer on top, not independently shippable without DCE.
