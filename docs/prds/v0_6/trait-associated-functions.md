# Trait Associated Members (Functions + Types)

> **Filename note:** this file remains `trait-associated-functions.md` (not renamed to `trait-associated-members.md`) because the eight already-filed function tasks (α 3935, β 3934, γ 3937, δ 3939, ζ 3941, ε 3943, η 3945, θ 3948) carry `prd_path: docs/prds/v0_6/trait-associated-functions.md` in their metadata; renaming would orphan that traceability for in-flight work. The minor name-vs-scope mismatch is outweighed by keeping the anchor stable. The title and §0 scope below carry the broadened intent.

Status: deferred (v0.2+ per spec §18 deferred-item #5 "Associated `fn` in traits", plus the in-grammar-but-uncompiled associated-**type** gap; authored 2026-05-27 in the spec-gap-filling batch `spec-gap-2026-05-27`, cluster `trait-associated-functions`). **Not yet approved for queueing** — see §11 DESIGN FORKS.

## §0 — Scope (broadened 2026-05-27)

This PRD specifies the surface, grammar, dispatch/resolution semantics, and conformance rules for **both** flavours of associated member a trait body may declare:

1. **Associated functions** (`fn` — procedural code; spec §18 deferred-item #5). Surface, grammar, dispatch, overload interaction, conformance. **Tasks α/β/γ/δ/ζ/ε/η/θ — already filed.**
2. **Associated types** (`type Material` — type-level members conformers must bind; spec §4.2 lists them as already-part-of-a-trait, but compilation is a no-op). Resolution semantics, conformance (a conforming structure must supply the binding), interaction with type-parameter/auto resolution, and qualified access to associated types. **Tasks ιₐ–ιₑ — filed by this broadening pass.**

**Why folded together (Leo's decision 2026-05-27, reversing the original FORK-B "out of scope"):** associated types are *already parsed* in trait bodies (grammar `associated_type`, AST `AssociatedTypeDecl`, ts_parser `lower_associated_type` all exist) but **compilation is deferred** — three live no-op sites: `crates/reify-compiler/src/entity.rs:1446` (structure member), `crates/reify-compiler/src/traits.rs:205` (trait-compile skip), and a hard reject in purpose bodies (`traits.rs:417`). Unblocking it alongside associated fns closes two §18/§4.2 spec gaps in one coherent trait-member-completion effort and shares the conformance / qualified-access / `RequirementKind`/`DefaultKind` scaffolding the fn work already extends.

Spec gap (fns): spec §4.2 lists what a trait contains (param / port / sub / associated-type / constraint / `let`) and explicitly excludes "Implementation logic -- no procedural code, no method bodies." Deferred-item #5 reverses that exclusion for `fn`.

Spec gap (types): spec §4.2 already lists "Associated types — type-level members that implementing types must bind" and §4.2 trait-refinement permits "narrow associated types". The EBNF (`assoc_type_decl ::= 'type' TYPE_IDENT (':' trait_bound)?`, spec line 2333) is in the grammar surface, **but no compiler path consumes the parsed node** — a user who writes `type Material` (or `type Material = Steel`) in a trait body today parses cleanly and is silently dropped. This PRD makes the binding observable: conformance enforced, type resolvable, qualified-access readable.

Resolution mode: **B + H** (vertical slice under design-first contract + boundary-test sketch). Triggered by the G5 heuristic: cross-crate blast radius ≥ 5 (`tree-sitter-reify`, `reify-syntax`, `reify-ast`, `reify-compiler`, `reify-expr`), touches the load-bearing grammar/parser/type-resolution seam, and the dispatch/resolution surface composes with the already-shipped `Type::member` / `obj.(Trait::member)` qualified-access machinery (spec §5.8) and the `auto:` type-param machinery (`crates/reify-compiler/src/auto_type_param.rs`, soft-coupled — see §9).

---

## §1 — Goal and user-observable surface

A trait may declare a **function** — procedural code, a real `fn` body — that conforming structures inherit and that callers invoke through the trait. Two flavours:

- **Default-providing associated fn** (has a body): the trait supplies an implementation; conforming structures get it for free and may override it.
- **Required associated fn** (no body, signature only): the trait demands that every conforming structure provide a `fn` of that name and signature; absence is a conformance error.

What a user observes when this lands (the motivating slice, §8 task ζ):

```reify
trait Cylindrical {
    param diameter : Length
    param length : Length

    // Default-providing associated fn: computed from this trait's own params.
    fn lateral_area(self) -> Scalar<Area> {
        pi * diameter * length
    }
}

structure def Pin : Cylindrical {
    param diameter : Length = 8mm
    param length : Length = 40mm
}
```

and a caller writes:

```reify
structure def Assembly {
    sub pin : Pin
    let wetted = pin.(Cylindrical::lateral_area)()
}
```

`reify check assembly.ri` resolves `pin.(Cylindrical::lateral_area)()` to the trait's default body bound to `pin`'s `diameter`/`length`, evaluates it, and `wetted` holds a `Scalar<Area>` value. A worked `.ri` example in `examples/` runs in CI and prints/asserts the result (the leaf signal for §8 task ζ).

### The `self` receiver

An associated fn whose first parameter is the keyword `self` is an **instance method**: `self` binds to the conforming structure instance the fn is dispatched on, and the fn body resolves the trait's other members (`diameter`, `length`) against `self`'s members. An associated fn with no `self` parameter is a **trait-static fn** — it has no receiver and is called `Trait::name(args)` (no instance). This PRD scopes **instance methods (`self`-receiver) as the primary mechanism**; trait-static fns are a thin sibling (§6, no extra dispatch surface). See §11 FORK-A for the receiver-syntax decision.

---

## §2 — Consumers (G1)

Every mechanism this PRD introduces names a consumer that is either an existing user surface or a filed/extant PRD. No "future unfiled PRD" consumers.

| Mechanism | Consumer |
|---|---|
| Trait-body `fn` grammar production | `examples/trait_assoc_fn_*.ri` worked examples (CI-run); the in-GUI assistant syntax chunks (`crates/reify-mcp/src/tools/chunks/`). |
| `obj.(Trait::fn)(args)` call lowering | The motivating `.ri` example (§1); `reify check` CLI diagnostics. |
| Default-providing assoc fn injection | `docs/prds/geometry-traits.md` — currently approximates trait "methods" with free `fn`s + an inference table; a default-providing `fn area(self) -> Scalar<Area>` on a geometry trait is the natural home. (G4 seam below.) |
| Required assoc fn + conformance check | `docs/prds/stdlib-trait-breadth.md` — declares the long-tail named traits; a required `fn` lets a trait like `Damping` demand `fn loss_factor(self) -> Real` from conformers. |
| Overload interaction (assoc fn vs free fn same name) | `reify check` ambiguity diagnostic (user-facing `E_*` signal). |
| **Associated-type decl + conformance** | `examples/trait_assoc_type_*.ri` worked examples (CI-run). `docs/prds/stdlib-trait-breadth.md` — a trait like `Damped` can declare `type DampingModel` that conformers bind. The existing no-op sites (`entity.rs:1446`, `traits.rs:205`) are the producers being unblocked. |
| **Associated-type as a resolvable type** (`param x : Material`, `let y : T::Material`) | `reify check` type-resolution + member-typing; the `Type::StructureRef` / `Type::TraitObject` machinery (`crates/reify-core/src/ty.rs:59-71`). |
| **Qualified associated-type access** (`Beam::Material` / `T::Material` as a type-expr) | `reify check` type annotations; the type-side analogue of value-side `Type::member` (spec §5.8). |

**Engine-integration sub-check:** This PRD touches **no in-engine seam** (no kernel module, dispatcher, walk, hook, or runtime trampoline). Associated-fn dispatch is a compile-time name-resolution + expression-lowering concern that reuses the existing `UserFunctionCall` evaluation path (`reify-expr/src/lib.rs::eval_user_function_call`); associated-**type** compilation is a pure compile-time conformance + type-resolution concern (no value cells, no eval-graph nodes, no runtime). `engine-integration-norm.md` §3 is therefore not engaged. Noted explicitly to satisfy the G1 engine sub-check.

---

## §3 — Background and the dispatch surface (premise validation, G6)

The current state of the four interacting machineries — each verified against the codebase at authoring time (2026-05-27):

1. **`fn` decl machinery (spec §4.3).** Fully built. `FnDef` AST (`crates/reify-ast/src/decl.rs:575`), `CompiledFunction` (`crates/reify-ir/src/expr.rs:244`), `compile_function` (`crates/reify-compiler/src/functions.rs:6`), and `eval_user_function_call` (`crates/reify-expr/src/lib.rs:955`). An assoc fn is a `FnDef` that lives in a trait/structure member list rather than at module top level.

2. **Overload resolution (spec §4.2.1).** Built: `resolve_function_overload(name, &arg_types, functions)` (`crates/reify-compiler/src/expr.rs:1022`) matches argument types against same-name candidates over the **flat free-function list**; exactly-one-match required; `Int→Real` promotion NOT considered. Associated fns must participate in (a parallel of) this.

3. **Trait conformance / member merging (spec §8.8).** Built: `CompiledTrait` (`crates/reify-compiler/src/types.rs:21`) stores `required_members: Vec<TraitRequirement>` (kinds: Param/Let/Sub) and `defaults: Vec<TraitDefault>` (kinds: Param/Let/Constraint). Merging injects trait defaults into conforming structures. There is **no `Fn` variant** in `RequirementKind` or `DefaultKind` — that is the core compiler extension.

4. **Qualified trait access `Type::member` / `obj.(Trait::member)` (spec §5.8).** Built **for value members only**. `ExprKind::QualifiedAccess` and `ExprKind::InstanceQualifiedAccess` (`crates/reify-ast/src/ast.rs:99,104`) parse and lower (`crates/reify-compiler/src/expr.rs:2738,2802`). **Critically: today both resolve only to a member ValueCell** (a `param`/`let` value) — there is no call form. `scope.trait_members` is a `HashMap<String, HashSet<String>>` of member *names* (`crates/reify-compiler/src/scope.rs:30`).

### The dispatch-surface gap (verified by grammar gate — §10)

The grammar gate (§10, run 2026-05-27) establishes the precise call-surface gap. **`Type::member` and `obj.(Trait::member)` are member *accessors*, not call forms.** The grammar binds them at precedence 8; `function_call` (precedence 10) requires the callee to be a **bare `$.identifier`**. Therefore:

- `Sized::density` parses (qualified access, no call).
- `Sized::density(mass)` **does NOT parse** — the call operator cannot apply to a `::`-qualified path.
- `obj.(Trait::fn)` parses (instance-qualified access, no call).
- `obj.(Trait::fn)(args)` **does NOT parse** — same reason.

So **both the declaration form (trait-body `fn`) and the call form (`...(args)` applied to a qualified path) require grammar work.** This PRD owns both, queued as the explicit G3 prerequisite chain (§8 tasks α, β). The dispatch surface this PRD declares as its own:

> **Dispatch surface (this-PRD-owned):** `instance_qualified_access` extended to accept a trailing argument list (`obj.(Trait::fn)(args...)`), lowering to a new `ExprKind::TraitMethodCall { object, trait_name, method, args }`. The compiler resolves `(trait_name, method)` against the receiver's static structure type → the structure's inherited-or-overridden assoc-fn `CompiledFunction` → an existing `UserFunctionCall` with the receiver injected as the `self` argument. Trait-static form `Trait::fn(args)` lowers via `qualified_access`-with-call to `ExprKind::TraitStaticCall { trait_name, method, args }`.

This reuses the shipped `UserFunctionCall` eval path (premise (1)) — **no new evaluator entry point, no kernel hook**.

### §3.5 — Associated-type machinery (premise validation, G6; verified 2026-05-27)

The associated-type state is the mirror image of the assoc-fn state: **parsed but not compiled**, plus two grammar gaps on the consumption side.

1. **Trait-body declaration — built and parsing.** Grammar `associated_type` (`tree-sitter-reify/grammar.js:314`, admitted in `trait_member` at line 170), AST `AssociatedTypeDecl { name, default_type: Option<TypeExpr>, .. }` (`crates/reify-ast/src/decl.rs:773`), ts_parser `lower_associated_type` (`crates/reify-syntax/src/ts_parser.rs:1386`), and the IR variant `TraitMember::AssociatedType { name, default }` (`crates/reify-ir/src/traits.rs:72`) all exist. Grammar gate (§10): `trait { type Material }` and `trait { type Material = Steel }` both **parse clean** (exit 0). **`grammar_confirmed=true` for the trait-body declaration form.**

2. **Compilation — three live no-op / reject sites.** `crates/reify-compiler/src/entity.rs:1445-1447` (structure-member arm: `// Associated type compilation deferred to a later milestone`), `crates/reify-compiler/src/traits.rs:204-206` (trait-compile `_ =>` arm: "Minimize, Maximize, GuardedGroup, AssociatedType — skip for now"), and `crates/reify-compiler/src/traits.rs:417` (purpose body: hard-rejected). The lint/guard match sites that already enumerate `MemberDecl::AssociatedType` as a no-op (`guards.rs:580`, `compile_builder/shadow_lint.rs:333,485`, `dot_chain_lint.rs:280`) are correct and need no change.

3. **Conformance / member model — no associated-type variant.** `RequirementKind` (`types.rs:51`) and `DefaultKind` (`types.rs:75`) have only Param/Let/Sub (+ fn variants from task δ). `CompiledTrait` does not store associated types. `collect_all_requirements` / `check_trait_conformance` (`trait_requirements.rs`) never check that a conformer binds a declared associated type. This is the core compiler extension for the type half.

4. **Type representation — adequate, no new `Type` variant needed.** A bound associated type resolves to an existing `Type` (`Type::StructureRef(name)` for `type Material = Steel`, or a constrained `Type::TraitObject(trait)` if the declaration carries a bound). `crates/reify-core/src/ty.rs` already has `TypeParam`, `StructureRef`, and `TraitObject` (lines 59-71). No new variant is required; the work is **resolution + substitution**, not a new kind.

#### The associated-type gap (verified by grammar gate — §10)

The trait-body *declaration* parses, but the two **consumption** surfaces do not:

- **Structure-side binding does NOT parse.** `_member` (structure member set, `grammar.js:372`) does **not** include `associated_type`. So `structure def Beam : HasMaterial { type Material = Steel }` — the way a conformer *supplies* the binding the trait requires — **fails to parse** (exit 1, gate §10). This is a hard grammar prerequisite.
- **Qualified associated-type access does NOT parse as a type-expr.** `structure def Asm { param m : Beam::Material }` and `let y : T::Material` **fail to parse** (exit 1) — `::`-qualified paths are not admitted in type-expression position. The value-side `Type::member` exists; the type-side analogue does not.

So the type half needs grammar work on **two** new productions (structure-body `type X = …` binding; `::`-qualified type-expr), plus the compiler conformance + resolution work. The trait-body declaration itself needs none.

> **Resolution surface (this-PRD-owned):** trait-body `type X` (no default) → a **required** associated type (`RequirementKind::AssocType`); `type X = Default` → a **default-providing** associated type (`DefaultKind::AssocType`). A conforming structure supplies a binding via a structure-body `type X = Concrete` member; absence-without-default → conformance error. Inside the conformer, an unqualified `X` in type position and a qualified `Self::X` / `Concrete::X` / `sub-typed `obj`-target`::X` resolve to the bound concrete `Type`. The bound type is substituted into member type annotations at compile time — **no eval-graph node, no value cell, no runtime artifact.**

---

## §4 — Contract: declaration, signature, and member model

### 4.1 Grammar (the G3 deliverable, §8 tasks α + β)

Extend `trait_member` and `structure` member sets to admit a function declaration, and extend the call grammar so qualified paths are callable.

**Declaration.** `trait_member` (grammar.js:165) gains `$.function_definition` as a choice arm. `function_definition` (grammar.js:86) already supports `pub?`, type params, params, return type, and a `fn_body`. Two adjustments:
- A **bodyless** form (required assoc fn) — `fn name(params) -> T` with no `{ }`. This is new: top-level `fn` requires a body. The grammar gains a sibling `function_signature` rule (no `fn_body`) usable only inside trait bodies.
- The `self` receiver: the first `fn_param` may be the bare keyword `self` (no `: Type`). Grammar gains an optional leading `self` token in `fn_param_list` for member-context fns.

**Call.** `instance_qualified_access` (grammar.js:1053) and `qualified_access` (grammar.js:1044) each gain an **optional trailing `( argument_list? )`** at the same postfix precedence, producing a callable qualified path. (Alternatively a dedicated `trait_method_call` rule — decided at §8 task β; either parses the same surface.)

### 4.2 AST

- `MemberDecl` (`crates/reify-ast/src/decl.rs:87`) gains a `Fn(FnDef)` variant — assoc fns are members of trait/structure bodies, reusing the existing `FnDef` shape. `FnDef.body: FnBody` becomes `FnDef.body: Option<FnBody>` (None = required/bodyless) **OR** a sibling field — decided at §8 task γ (FORK-C); the `Option` shape is the default.
- `FnParam` gains an `is_self: bool` (or the `self` receiver is modeled as a `FnParam` with a reserved name + sentinel type) — §8 task γ.
- `ExprKind` gains `TraitMethodCall { object, trait_name, method, args }` and `TraitStaticCall { trait_name, method, args }` (`crates/reify-ast/src/ast.rs`).

### 4.3 Compiled model

- `RequirementKind` (`types.rs:51`) gains `Fn(CompiledAssocFnSig)` — a required assoc fn the conformer must provide (name + param types + return type + `self`-ness).
- `DefaultKind` (`types.rs:75`) gains `Fn(FnDef)` — a default-providing assoc fn injected into conformers that don't override it.
- A conforming structure's compiled form gains an **assoc-fn table** keyed by `(trait_name, fn_name)` → resolved `CompiledFunction` (override-or-injected-default). This is the lookup target for `TraitMethodCall` lowering.

### 4.4 `self` binding and body scope

Inside an assoc-fn body with a `self` receiver:
- `self` is in scope as the receiver instance.
- The trait's other members (`diameter`, `length`, …) resolve **as members of `self`** (consistent with §8.8 member merging — the trait's members are already part of the conforming structure's body).
- Bare member references (`diameter`) are sugar for `self.diameter` inside an assoc-fn body, matching the spec §4.2 `let`-binding precedent where `let volume = pi * (diameter/2)^2 * length` references sibling members unqualified.
- The body is otherwise a normal `fn` body (spec §4.3): `let` bindings + final expression, pure, no determinacy state of its own.

A trait-static fn (no `self`) has **no** access to instance members — its body may reference only its own params and module-level names. Referencing a trait member from a static fn is a compile error.

### §4.5 — Associated types: declaration, binding, and member model

**Declaration (trait body).** Two forms (mirroring required-vs-default for params/fns):
- **Required associated type** — `type Material` (no default). Every conforming structure MUST bind it. Absence → conformance error.
- **Default-providing associated type** — `type Material = Steel` (the grammar's `= type_expr` form). A conformer that omits the binding inherits the default; a conformer may override it with its own binding. (See FORK-E for the bound-vs-default semantics decision — the spec EBNF and the live grammar diverge here.)

**Binding (structure body).** A conformer supplies the binding with a structure-body `type Name = Concrete` member (the new `_member` grammar production, task ιₐ). The right-hand side is a `type_expr` resolving to a concrete `Type` (a structure name → `Type::StructureRef`, or another resolvable type). The binding is **not** a value cell — it produces no eval-graph node; it is a compile-time entry in the conformer's resolved-associated-type table keyed by `(trait_name, type_name)`.

**Compiled model.**
- `RequirementKind` gains `AssocType(Option<AssocTypeBound>)` — a required associated type the conformer must bind; the optional bound (FORK-E) constrains what may be bound.
- `DefaultKind` gains `AssocType(Type)` (or `AssocType(TypeExpr)` resolved at injection) — a default binding injected into conformers that don't override it.
- `CompiledTrait` carries the associated-type requirements/defaults (already has `required_members` / `defaults` Vecs — the new variants slot in, no struct-shape change beyond the enum arms).
- The conforming structure's compiled form gains a **resolved associated-type table** keyed by `(trait_name, type_name)` → resolved `Type`. This is the lookup target for in-conformer type resolution and qualified access.

**Member-type resolution inside the conformer.** Once `Beam : HasMaterial` binds `type Material = Steel`, a `param mass : Material` (or any member annotation referencing `Material`) inside `Beam` resolves `Material` → `Type::StructureRef("Steel")` via the resolved table. Unbound-required-type references during error recovery resolve to `Type::Error` (poison, anti-cascade), consistent with the existing unresolved-type pattern in `traits.rs`.

**Refinement.** Per spec §4.2 "narrow associated types": a refining trait may add new associated types and may **narrow** (tighten the bound of) an inherited associated type, but may not widen or rebind it incompatibly. A conformer's binding must satisfy the (possibly-narrowed) bound.

---

## §5 — Contract: dispatch and call resolution

### 5.1 Instance dispatch (`obj.(Trait::fn)(args)`)

1. Parse → `ExprKind::TraitMethodCall { object, trait_name, method, args }`.
2. Compile: resolve `object`'s static structure type `S` (via `scope.sub_component_types` for `sub`s, or the param's declared type).
3. Verify `S : Trait` (conformance) and that `Trait` declares assoc fn `method`. If not → `E_TRAIT_METHOD_UNKNOWN` (user-facing).
4. Resolve the concrete `CompiledFunction`: the structure's **override** if present, else the trait's **injected default**. If neither (required fn with no conformer impl) → conformance error (caught earlier at §5.3).
5. Lower to a `UserFunctionCall` of that `CompiledFunction`, prepending `object` as the bound `self` argument. Member references inside the body that resolve to `self.member` lower to the receiver's ValueCells.
6. Evaluate via the existing `eval_user_function_call` path.

### 5.2 Static dispatch (`Trait::fn(args)`)

`Trait::fn(args)` resolves directly to the trait's (single) assoc-fn definition — traits are the namespace, no instance, no override. Lower to a `UserFunctionCall`. (If a future "static fns are overridable per conformer" need arises, it's deferred — §11 not-a-fork; static fns are trait-level only.)

### 5.3 Overload interaction (spec §4.2.1)

Spec §4.2.1: "Function (`fn`) overloading by parameter types is permitted." Associated fns interact with this in three places; the contract resolves each:

- **Assoc fn vs assoc fn, same trait, same name, different param types** → permitted overload set within the trait, resolved by the existing `resolve_function_overload` adapted to the trait's assoc-fn list. Exactly-one-match; ambiguity → `E_AMBIGUOUS_CALL`.
- **Assoc fn vs free fn, same name** → **disjoint namespaces by call syntax.** A free fn is called `name(args)`; an assoc fn is called `obj.(Trait::name)(args)` or `Trait::name(args)`. The qualified call syntax disambiguates at the grammar level — there is no overload contest between them. (This is the key simplifying invariant: the qualified-path requirement means assoc fns never silently shadow or compete with free fns.)
- **Two traits, same assoc-fn name, one conformer** → `obj.(TraitA::f)()` vs `obj.(TraitB::f)()` disambiguate by the qualifier. A bare `obj.f()` form is **not introduced** by this PRD (no method-call sugar — consistent with memory GR-040 "Reify has NO method-call syntax `x.foo()`"), so no ambiguity arises. This is the deliberate reason the call syntax is qualified.

`Int→Real` promotion remains excluded from overload resolution (spec §4.2.1), unchanged.

### 5.3a Associated-type resolution and qualified access

1. **Binding resolution.** At conformer-compile time, each declared associated type of each conformed trait is resolved to a concrete `Type` from: the conformer's own `type X = Concrete` binding (override) if present, else the trait's default binding, else (no default) → conformance error `E_TRAIT_ASSOC_TYPE_NOT_BOUND` naming trait + missing type. Stored in the resolved associated-type table.
2. **Unqualified reference.** Inside the conformer body, `X` in type position (e.g. `param p : Material`) resolves to the bound `Type` via the table. This is the type-resolution-order rule: trait-supplied associated types are in scope as type names within conforming structures, alongside module-level type names and the structure's own type params.
3. **Qualified access `Concrete::X` / `Self::X` / `obj`-target`::X`** (task ιₑ). The type-side analogue of value-side `Type::member`: a `::`-qualified path in **type position** resolves to a structure's bound associated type. `Beam::Material` (where `Beam` is a known structure) → `Beam`'s binding for whichever trait declares `Material`. Ambiguity (two conformed traits both declare `Material`) → `E_AMBIGUOUS_ASSOC_TYPE`, disambiguated by `Beam::(HasMaterial::Material)` (reusing the value-side qualifier syntax shape) — see FORK-G.
4. **No runtime.** All of the above is compile-time `Type` substitution. No `ValueCell`, no eval-graph node, no `eval_user_function_call`. This is the cleanest possible engine-integration story (none).

### 5.4 Conformance (spec §8.8)

- A **required** assoc fn (`RequirementKind::Fn`) must be satisfied by a conforming structure providing a `fn` of matching name + signature (`self`-ness, param types, return type — exact match, no subtyping, per §8.8 "exact type match required"). Absence → `E_TRAIT_FN_NOT_SATISFIED` naming the trait + missing fn.
- A **default-providing** assoc fn (`DefaultKind::Fn`) is injected into conformers that don't declare an override of the same name. An override must match the signature (§8.8 same-name-different-type → error).
- Trait refinement (spec §4.2 "additive requirements only"): a refining trait may add new assoc fns and may override an inherited default's body, but may not change an inherited assoc fn's signature.
- Conformance × determinacy (spec §4.2.20): an assoc fn body is procedural and pure; it does not gate conformance on determinacy. Calling it on an instance with `undef` members evaluates per normal `undef` propagation (spec §9.2) — the call is well-typed; the result may be `undef`.

**Associated types (spec §8.8 + §4.2 "narrow associated types"):**
- A **required** associated type (`RequirementKind::AssocType`) must be satisfied by a conforming structure supplying a `type Name = Concrete` binding whose resolved type satisfies the (optional) bound. Absence → `E_TRAIT_ASSOC_TYPE_NOT_BOUND` naming trait + missing type.
- A **default-providing** associated type (`DefaultKind::AssocType`) is injected into conformers that don't bind it; a conformer may override by supplying its own binding.
- Composition (two traits declare `type Material`): same name + identical/compatible bound → merge; incompatible bound → `E_CONFLICTING_TRAIT_ASSOC_TYPE` (parallel to the existing `ConflictingTraitRequirements` path in `collect_all_requirements`).
- Refinement: a refining trait may narrow an inherited associated type's bound (per spec §4.2), never widen/incompatibly rebind.

---

## §6 — Trait-static associated fns (the no-`self` sibling)

A trait-static fn (`fn make_default() -> Length { 10mm }`, no `self`) is a namespaced free fn. It needs:
- the same declaration grammar (just no `self` in the param list),
- the `Trait::fn(args)` call form (§4.1 `qualified_access`-with-call),
- **no** instance dispatch, **no** receiver injection, **no** override table — it resolves directly to the trait's definition.

It is a strictly smaller mechanism than instance dispatch and is delivered as one task (§8 task η) layered on the same grammar + the static-call lowering. It is included because the spec's deferred-item #5 ("procedural code in traits") encompasses both, and excluding statics would leave an obvious half-feature.

---

## §7 — Boundary-test sketch (cross-crate; facing both ways)

The seam is between the **parse/lower side** (`tree-sitter-reify` → `reify-syntax` → `reify-ast`) and the **compile/dispatch/eval side** (`reify-compiler` → `reify-expr`). Tests cross it from both directions.

### 7.1 Parse/lower side (grammar + AST produce the right shape)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Trait-body fn with `self` parses. | Grammar extended (α). | `tree-sitter parse --quiet examples/trait_assoc_fn_default.ri` exits 0; CST has a `function_definition` under `trait_member`. |
| Bodyless required fn parses. | Grammar extended (α). | `fn loss_factor(self) -> Real` inside a trait body parses; CST node is `function_signature` (no `fn_body`). |
| Qualified call parses. | Grammar extended (β). | `obj.(Trait::f)(args)` and `Trait::f(args)` both exit 0; prior bare forms `obj.(Trait::f)` and `Trait::f` still parse (no regression). |
| Lowering produces `MemberDecl::Fn` + call exprs. | ts_parser lowering (α, β). | AST round-trip test: trait body yields `MemberDecl::Fn(FnDef{ body: Some/None })`; call site yields `ExprKind::TraitMethodCall` / `TraitStaticCall`. |

### 7.2 Compile/dispatch/eval side (resolution + evaluation)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Default-providing fn resolves + evaluates. | δ, ζ landed; conformer declares no override. | `pin.(Cylindrical::lateral_area)()` evaluates to `pi * 8mm * 40mm` within `Scalar<Area>` tolerance; `reify check` clean. |
| Override beats default. | δ, ζ; conformer declares its own `lateral_area`. | The override's body is evaluated, not the trait default; dispatch-resolution test pins which `CompiledFunction` was selected. |
| Required fn unsatisfied → diagnostic. | δ (conformance); trait has bodyless fn; conformer omits it. | `reify check` emits `E_TRAIT_FN_NOT_SATISFIED` naming trait + fn; no panic. |
| Unknown trait method → diagnostic. | δ; call `obj.(Trait::nonexistent)()`. | `E_TRAIT_METHOD_UNKNOWN`; poison literal prevents cascade (existing anti-cascade pattern). |
| Two-trait same-name disambiguation. | conformer implements `TraitA::f` and `TraitB::f`. | `obj.(TraitA::f)()` and `obj.(TraitB::f)()` resolve to distinct fns; no ambiguity diagnostic. |
| Assoc-fn overload by param type. | trait declares `f(self, x: Length)` and `f(self, x: Angle)`. | `obj.(T::f)(5mm)` and `obj.(T::f)(30deg)` resolve to distinct candidates; `resolve_function_overload` exactly-one-match holds. |
| Static fn (no `self`) evaluates. | η landed. | `Trait::make_default()` evaluates to the trait's body result; referencing a trait member inside it → compile error. |
| `undef` member propagation. | conformer instance has `diameter = undef`. | `pin.(Cylindrical::lateral_area)()` evaluates to `undef` (spec §9.2), call is well-typed, no error. |

### 7.3 Associated-type side (parse + conformance + resolution)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Trait-body assoc-type decl parses (regression pin). | none (already parses). | `tree-sitter parse --quiet` exits 0 on `trait { type Material }` and `trait { type Material = Steel }` — pinned so a future grammar refactor can't regress. |
| Structure-body type binding parses. | Grammar extended (ιₐ). | `structure def Beam : HasMaterial { type Material = Steel }` exits 0; corpus test asserts the `associated_type` node under `_member`. |
| Qualified assoc-type access parses. | Grammar extended (ιₐ). | `param m : Beam::Material` / `let y : T::Material` exit 0 in type position. |
| Required assoc-type unbound → diagnostic. | ιᵦ; trait declares `type Material` (no default); conformer omits binding. | `reify check` emits `E_TRAIT_ASSOC_TYPE_NOT_BOUND` naming trait + type; no panic. |
| Default assoc-type injected; override wins. | ιᵦ; trait `type Material = Steel`; conformer A omits, conformer B binds `= Aluminium`. | A resolves `Material → Steel`; B resolves `Material → Aluminium`; resolution test pins which `Type` each gets. |
| Bound association resolves in a member annotation. | ιᵧ; `Beam` binds `type Material = Steel`. | `param mass : Material` inside `Beam` types as `StructureRef("Steel")`; `reify check` clean. |
| Conflicting assoc-type across two traits. | ιᵦ; conformer conforms to two traits both declaring incompatible `type Material`. | `E_CONFLICTING_TRAIT_ASSOC_TYPE`; no silent last-writer-wins. |
| Qualified `T::Material` two-trait disambiguation. | ιₑ; `Beam` conforms to two traits each declaring `Material`. | bare `Beam::Material` → `E_AMBIGUOUS_ASSOC_TYPE`; `Beam::(HasMaterial::Material)` resolves distinctly. |

---

## §8 — Decomposition DAG (proposed; not yet filed)

Style: **B + H**. Greek labels; task IDs assigned at decompose time. Each leaf names a user-observable signal; intermediates name the downstream task they unlock. Phase 2 (tasks α–β) is the G3 grammar prerequisite chain; everything downstream `depends_on` it.

### Phase 1 — Grammar (G3 prerequisite; grammar_confirmed=false)

- **Task α** — Grammar: trait-body `fn` (with-body + bodyless `function_signature`) + `self` receiver param.
  - **Signal:** `tree-sitter parse --quiet` exits 0 on fixtures `trait_assoc_fn_default.ri` (with body + `self`) and `trait_assoc_fn_required.ri` (bodyless); a `test/corpus/trait_assoc_fn.txt` corpus test asserts the `function_definition` / `function_signature` nodes under `trait_member`.
  - **Unlocks:** γ (AST lowering). **Prereqs:** none. **Crates:** tree-sitter-reify.
  - grammar_confirmed=false (this task creates the production).

- **Task β** — Grammar: callable qualified path (`obj.(Trait::fn)(args)`, `Trait::fn(args)`).
  - **Signal:** `tree-sitter parse --quiet` exits 0 on `trait_assoc_fn_call.ri`; corpus test asserts the trailing-arg-list production AND that bare `obj.(Trait::m)` / `Trait::m` still parse (regression pin).
  - **Unlocks:** γ. **Prereqs:** none (parallel with α). **Crates:** tree-sitter-reify.
  - grammar_confirmed=false.

### Phase 2 — AST + lowering (intermediate)

- **Task γ** — AST: `MemberDecl::Fn`, `FnDef.body: Option<FnBody>`, `FnParam.is_self`, `ExprKind::TraitMethodCall` / `TraitStaticCall`; ts_parser lowering for all of the above.
  - **Signal:** AST round-trip unit tests in `reify-syntax`: a trait-body fn lowers to `MemberDecl::Fn(FnDef{body: Some})`; a bodyless fn → `body: None`; `obj.(T::f)(x)` → `TraitMethodCall`; `T::f(x)` → `TraitStaticCall`.
  - **Unlocks:** δ. **Prereqs:** α, β. **Crates:** reify-ast, reify-syntax.

### Phase 3 — Conformance + member model (intermediate)

- **Task δ** — Compiler: `RequirementKind::Fn` + `DefaultKind::Fn`; assoc-fn table on compiled conformers; conformance check for required fns (`E_TRAIT_FN_NOT_SATISFIED`); default injection + override-beats-default; signature exact-match on override (§8.8).
  - **Signal:** compiler unit tests: required-fn-unsatisfied emits `E_TRAIT_FN_NOT_SATISFIED`; override replaces default in the assoc-fn table; signature-mismatch override → error. (Producer-only at this stage; roped into ζ as the integration gate per G2 escape hatch.)
  - **Unlocks:** ζ, η. **Prereqs:** γ. **Crates:** reify-compiler.

### Phase 4 — Vertical slice: instance dispatch end-to-end (LEAF — the motivating signal)

- **Task ζ** — Compile + dispatch + eval `obj.(Trait::fn)(args)` for default-providing instance methods; `self` binding; member-ref-as-`self.member` resolution; lower to `UserFunctionCall`.
  - **Signal (user-observable):** `examples/trait_assoc_fn_cylinder.ri` (the §1 example) — `reify check` is clean AND CLI evaluation of `wetted = pin.(Cylindrical::lateral_area)()` yields a `Scalar<Area>` equal to `pi * 8mm * 40mm` within tolerance. Override test: a second conformer with an explicit `lateral_area` override evaluates the override. The example runs in CI.
  - **Prereqs:** δ. **Crates:** reify-compiler (expr.rs lowering), reify-expr (reuses eval path), reify-stdlib/examples.
  - **This is the H integration-gate leaf** — it consumes δ (producer) and proves the §7.2 boundary scenarios.

### Phase 5 — Overload + multi-trait disambiguation (LEAF)

- **Task ε** — Assoc-fn overload resolution (param-type dispatch within a trait) + two-trait same-name disambiguation by qualifier.
  - **Signal:** `examples/trait_assoc_fn_overload.ri` — `obj.(T::f)(5mm)` and `obj.(T::f)(30deg)` resolve to distinct bodies (asserted via differing results); `obj.(TraitA::f)()` vs `obj.(TraitB::f)()` resolve distinctly; an ambiguous call emits `E_AMBIGUOUS_CALL`. `reify check` diagnostics observable.
  - **Prereqs:** ζ. **Crates:** reify-compiler.

### Phase 6 — Trait-static fns (LEAF)

- **Task η** — `Trait::fn(args)` static dispatch (no `self`, no receiver, no override); reject trait-member reference inside a static fn body.
  - **Signal:** `examples/trait_assoc_fn_static.ri` — `Trait::make_default()` evaluates to the body result; a static fn referencing a trait member → `reify check` compile error naming the offending member.
  - **Prereqs:** δ (conformance/member model), β (call grammar). **Crates:** reify-compiler.

### Phase 7 — Companion: assistant chunks + cross-PRD prose (LEAF, doc-only)

- **Task θ** — Add trait-associated-fn syntax to `crates/reify-mcp/src/tools/chunks/syntax.md` (or the trait chunk); update `docs/prds/geometry-traits.md` and `docs/prds/stdlib-trait-breadth.md` cross-references (G4 seam owner = this PRD); spec §4.2 amendment note (procedural code now permitted via assoc fn).
  - **Signal:** chunk file updated; doc lint passes; the two consumer PRDs reference this PRD as the assoc-fn owner. No code changes.
  - **Prereqs:** ζ. **Crates:** reify-mcp (chunks), docs.

### Phase 8 — Grammar: associated-type consumption surface (G3 prerequisite for the type half)

- **Task ιₐ** — Grammar: structure-body `type Name = Concrete` binding (`associated_type` admitted in `_member`) **and** `::`-qualified type-expr (`Beam::Material` / `T::Material` / `Beam::(Trait::Material)` in type position).
  - **Signal:** `tree-sitter parse --quiet` exits 0 on `trait_assoc_type_bind.ri` (structure binding) and `trait_assoc_type_qual.ri` (qualified type access); corpus test asserts the `associated_type` node under `_member` AND the qualified type-expr production AND a **regression pin** that trait-body `type Material` / `type Material = Steel` still parse.
  - **Unlocks:** ιᵦ. **Prereqs:** none (parallel with α, β; independent grammar surface). **Crates:** tree-sitter-reify + reify-syntax ts_parser lowering of the two new productions.
  - **grammar_confirmed=false** for the two new productions (structure binding + qualified type-expr — both FAIL today, §10); **grammar_confirmed=true** for the trait-body declaration form (already parses — this task only pins it against regression).

### Phase 9 — Compiler: associated-type conformance + member model (intermediate)

- **Task ιᵦ** — Compiler: `RequirementKind::AssocType` + `DefaultKind::AssocType`; store associated types on `CompiledTrait`; resolved associated-type table on compiled conformers; conformance check for required associated types (`E_TRAIT_ASSOC_TYPE_NOT_BOUND`); default injection + override-beats-default; cross-trait conflict (`E_CONFLICTING_TRAIT_ASSOC_TYPE`); refinement narrowing. Replaces the three no-op/reject sites (`entity.rs:1446`, `traits.rs:205`; relax `traits.rs:417` purpose-reject only if Leo wants purpose-body associated types — default NO, keep rejected — see FORK-F).
  - **Signal:** compiler unit tests: required-assoc-type-unbound emits `E_TRAIT_ASSOC_TYPE_NOT_BOUND`; conformer binding overrides default in the resolved table; two-trait incompatible declaration → `E_CONFLICTING_TRAIT_ASSOC_TYPE`. (Producer-only; integration-gated by ιᵧ per the G2 escape hatch.)
  - **Unlocks:** ιᵧ, ιₑ. **Prereqs:** ιₐ. **Crates:** reify-compiler.

### Phase 10 — Vertical slice: associated-type resolution end-to-end (LEAF — the type-half motivating signal)

- **Task ιᵧ** — Resolve a bound associated type in member type annotations end-to-end: unqualified `X` in type position inside a conformer resolves to the bound `Type`; type-resolution-order wiring so trait-supplied associated types are in scope alongside module type names + structure type params.
  - **Signal (user-observable):** `examples/trait_assoc_type_material.ri` — a trait `HasMaterial { type Material }`, a structure `Beam : HasMaterial { type Material = Steel; param mass : Material = 5kg }`; `reify check` is clean AND a member annotated `: Material` types as `Steel` (asserted via a type-probe test or a downstream constraint that only holds for `Steel`). A no-binding variant emits `E_TRAIT_ASSOC_TYPE_NOT_BOUND`. Example runs in CI.
  - **This is the H integration-gate leaf for the type half** — it consumes ιᵦ and proves the §7.3 boundary scenarios.
  - **Prereqs:** ιᵦ. **Crates:** reify-compiler (type_resolution.rs, entity.rs), reify-stdlib/examples.

- **Task ιₑ** — Qualified associated-type access as a type-expr: `Beam::Material` / `T::Material` resolves to a structure's bound associated type; two-trait ambiguity → `E_AMBIGUOUS_ASSOC_TYPE` with `Beam::(Trait::Material)` disambiguation (FORK-G).
  - **Signal:** `examples/trait_assoc_type_qualified.ri` — `param m : Beam::Material` resolves to `Beam`'s binding; ambiguous bare `Beam::Material` (two conformed traits declaring `Material`) emits `E_AMBIGUOUS_ASSOC_TYPE`; the qualified `Beam::(HasMaterial::Material)` resolves distinctly. `reify check` diagnostics observable.
  - **Prereqs:** ιᵦ (resolved table), ιₐ (qualified type-expr grammar). **Crates:** reify-compiler.

> **Note — docs/companion:** the associated-type syntax + cross-PRD prose folds into the existing **task θ** (3948), whose scope is widened to also document the associated-type declaration/binding/qualified-access forms. No separate doc leaf is filed; θ gains a dependency on ιᵧ so documented syntax matches shipped behaviour. (This widening is recorded here; θ's metadata is not rewritten by this pass — the in-flight task already exists.)

### Dependency view (full)

```
α ─┐
   ├─→ γ ─→ δ ─┬─→ ζ ─┬─→ ε
β ─┘            │      ├─→ θ ──(also gains dep on ιᵧ)
                └──────┴─→ η   (η also needs β)

ιₐ ─→ ιᵦ ─┬─→ ιᵧ ─→ θ
          └─→ ιₑ  (ιₑ also needs ιₐ)
```

The fn chain (α…η) and the type chain (ιₐ…ιₑ) are **independent** through Phase 9 — they share no task-level prerequisite. They re-converge only at the doc leaf θ. This independence is deliberate: the two halves touch disjoint code (fn → expr lowering + eval; type → type resolution + conformance), so they can be implemented in parallel.

---

## §9 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/geometry-traits.md` | this produces, that consumes | default-providing assoc fn as the home for trait "methods" (geometry-traits currently uses free `fn` + inference table) | **this-prd** | queued (task θ prose update; no reciprocal-ownership ambiguity — geometry-traits does not claim to own assoc-fn machinery) |
| `docs/prds/stdlib-trait-breadth.md` | this produces, that consumes | required assoc fn (`RequirementKind::Fn`) lets long-tail traits demand behaviour | **this-prd** | queued (task θ); breadth PRD declares trait *existence* only, defers conformance machinery — no contest |
| spec §5.8 qualified-access machinery | this extends | `instance_qualified_access` / `qualified_access` gain a callable trailing arg-list | **this-prd** | this-PRD-owned extension (declared in §3/§4.1), not a contested seam |
| spec §4.2.1 overload resolution | this composes with | `resolve_function_overload` adapted to assoc-fn lists; free-vs-assoc disambiguated by call syntax | **this-prd** | this-PRD-owned (§5.3) |
| `docs/prds/auto-type-param-resolution.md` + `docs/prds/v0_2/auto-resolution-backtracking.md` (task **3558**, pending, dep 3526) | **soft / shared-machinery, NOT a hard dep** | both touch `satisfies_trait_bound` and `Type::TypeParam → Type::StructureRef` substitution; `auto:` resolves a *type-arg slot* by enumerating structures satisfying a trait bound, associated types bind a *type member inside a trait body* — **disjoint surfaces, disjoint grammar, disjoint call sites** | each PRD owns its own | **declared, not wired.** ιₐ–ιₑ do **not** `depends_on` 3558. If both land, a future cleanup may unify the shared `satisfies_trait_bound` helper, but neither blocks the other. The original agent's note about "interaction with 3477/3558" is resolved here: **3477 is spurious** (it is `engine_hash_algo` editor-debris filtering, unrelated, already done); **3558 is the `auto:` resolver wiring** (the real but soft relationship above). |

No reciprocal "the other PRD owns it" statements detected. This PRD does not introduce a fourth contested-ownership pair (the three known pairs are PNv2↔multi-kernel, imported-field-source↔multi-kernel, topology-selectors↔PNv2 — none involve traits). The `auto:`/associated-type relationship is shared-machinery, not contested ownership.

---

## §10 — Grammar gate evidence (G3)

Run from `tree-sitter-reify/` on 2026-05-27. **All declaration and call fixtures FAIL today → grammar work is a hard prerequisite (tasks α, β); grammar_confirmed=false for those, true for downstream tasks once α/β land.**

| Fixture | Form | `tree-sitter parse --quiet` |
|---|---|---|
| trait-body `fn` with body | `trait { fn density(m:Real)->Real { m/volume } }` | **exit 1** (MISSING `}`) |
| trait-body bodyless `fn` | `trait { fn density(m:Real)->Real }` | **exit 1** (MISSING `}`) |
| `pub fn` in trait | `trait { pub fn density(...)->Real {...} }` | **exit 1** (ERROR) |
| `Trait::fn(args)` call | `let d = Sized::density(mass)` | **exit 1** (ERROR — call op can't apply to `::` path) |
| `obj.(Trait::fn)(args)` call | `let d = beam.(Sized::density)(mass)` | **exit 1** (ERROR) |
| `Trait::member` (no call) | `let d = Sized::density` | exit 0 (existing — accessor) |
| `obj.(Trait::member)` (no call) | `let d = beam.(Sized::density)` | exit 0 (existing — accessor) |
| free `fn` call in trait `let` | `trait { let d = density(volume) }` | exit 0 (existing) |

**Associated-type fixtures (re-run 2026-05-27, parser regenerated from working-tree `grammar.js`):**

| Fixture | Form | `tree-sitter parse` |
|---|---|---|
| trait-body assoc-type decl | `trait { type Material }` | exit 0 (existing — `associated_type` in `trait_member`) |
| trait-body assoc-type default | `trait { type Material = Steel }` | exit 0 (existing) |
| structure-body type binding | `structure def Beam : HasMaterial { type Material = Steel }` | **ERROR** (`associated_type` NOT in `_member`, grammar.js:372) |
| qualified assoc-type as type | `param m : Beam::Material` | **ERROR** (`::`-qualified path not admitted in type-expr position) |

Resolution: **(b) queue grammar work** for both halves. Fn half: tasks α, β (`grammar_confirmed=false`). Type half: task ιₐ adds the structure-binding + qualified-type-expr productions (`grammar_confirmed=false` for those two; **true** for the trait-body decl, which only gets a regression pin). The deferral the type half unblocks is the compiler no-op at `entity.rs:1446` / `traits.rs:205`, not a parse failure of the declaration itself.

---

## §11 — Pre-conditions for activating

- **Grammar productions** — fn half: tasks α, β; type half: task ιₐ (structure-binding + qualified-type-expr). All in-PRD prerequisites, no external grammar dependency.
- **No GR-001 dependency.** Assoc fns do not require struct-constructor runtime evaluation; the receiver is an already-instantiated `sub`/param instance, and `self.member` resolves to existing ValueCells. Associated types are compile-time-only (no value cells, no runtime). (Distinct from the FEA path that gates on GR-001.)
- **Associated-TYPE compilation IS in scope** (FORK-B resolved 2026-05-27 — fold in). It unblocks the three no-op/reject sites (`entity.rs:1446`, `traits.rs:205`, and — only if FORK-F flips — `traits.rs:417`). The trait-body *declaration* already parses; the prerequisite is the *consumption* grammar (ιₐ) + conformance/resolution (ιᵦ/ιᵧ/ιₑ).
- **`auto:` resolver (task 3558) is NOT a prerequisite** — soft-coupled shared machinery only (§9); the type half does not depend on it.

---

## DESIGN FORKS FOR LEO

### FORK-A — Receiver syntax: `self` keyword vs implicit-receiver
- **Default (chosen):** explicit `self` first parameter (`fn lateral_area(self) -> Scalar<Area>`); no-`self` ⇒ trait-static fn. Call form is always qualified (`obj.(Trait::fn)(args)` / `Trait::fn(args)`); **no** bare `obj.fn()` method-call sugar (consistent with memory GR-040 — Reify has no method-call syntax).
- **Alt 1:** implicit receiver — all trait fns are instance methods, members referenced unqualified, no `self` token. Simpler grammar but no way to declare a trait-static fn, and `self`-passing to nested calls is impossible.
- **Alt 2:** introduce bare `obj.fn()` method-call sugar resolved by conformance. Rejected: directly contradicts GR-040 and reintroduces the free-vs-method ambiguity the qualified syntax avoids.
- **Impact:** FORK-A shapes the grammar (tasks α, β) and the `self`-binding contract (§4.4). The default keeps the no-method-call invariant intact and makes free-vs-assoc disambiguation free (§5.3).

### FORK-B — Scope: associated fns only, or also unblock associated TYPES? — **RESOLVED 2026-05-27: FOLD BOTH IN.**
- **Decision (Leo, 2026-05-27):** unblock **both** in this PRD. The original "fns only" default is reversed. Rationale: associated types are *already parsed* (grammar + AST + ts_parser lowering all exist) and only compilation is deferred (`entity.rs:1446`, `traits.rs:205`); the two halves share the conformance / qualified-access / `RequirementKind`/`DefaultKind` scaffolding; and closing both spec gaps (§18 #5 and §4.2 associated-type) in one trait-member-completion effort is more coherent than two PRDs. The blast-radius concern is mitigated by the §8 DAG showing the two chains are independent through Phase 9 (disjoint code), so the META "is this good?" gate holds — the type half does not entangle the fn half.
- **Superseded alt (the old default):** a separate `docs/prds/v0_6/trait-associated-types.md`. Not pursued.
- **Impact:** adds tasks ιₐ–ιₑ (§8 Phases 8-10) and forks E/F/G below. §11 pre-conditions now *include* the associated-type grammar work (ιₐ) as an in-PRD prerequisite.

### FORK-C — Bodyless required-fn AST shape: `Option<FnBody>` vs sibling type
- **Default (chosen):** `FnDef.body: Option<FnBody>` (None = required/bodyless). Minimal churn; one field change; matches the trait required-vs-default split already present in `RequirementKind`/`DefaultKind`.
- **Alt:** a distinct `FnSig` AST type for bodyless decls, kept separate from `FnDef`. Cleaner type-level distinction (can't construct a bodyless top-level fn by accident) but duplicates the param/type-param/return-type plumbing.
- **Impact:** Local to task γ; recoverable either way. Listed for completeness; lean default unless Leo prefers the stricter type.

### FORK-D — Default-providing fn: is overriding allowed, and is the override signature-locked?
- **Default (chosen):** overriding a default-providing assoc fn **is allowed** (mirrors spec §4.2 "`let` bindings ... overridable" and "override defaults"); the override **must match the signature exactly** (§8.8 same-name-different-type → error).
- **Alt:** defaults are final (non-overridable). Rejected: contradicts the §4.2 let-default-override precedent and removes the main ergonomic win (trait provides a sensible default, structure specializes).
- **Impact:** Shapes task δ (override table + signature-match check) and the §7.2 "override beats default" boundary test.

### FORK-E — Associated-type declaration semantics: spec EBNF (`: bound`) vs live grammar (`= default`). **(LOAD-BEARING — assoc-type)**
- **The divergence:** the spec EBNF says `assoc_type_decl ::= 'type' TYPE_IDENT (':' trait_bound)?` (spec line 2333) — an associated type carries an optional **trait bound** constraining what conformers may bind (`type Material : Metal` ⇒ conformer must bind a structure conforming to `Metal`). But the **live grammar** implements `'type' identifier ('=' type_expr)?` (grammar.js:314) — an optional **default binding** (`type Material = Steel` ⇒ conformer inherits `Steel` unless it overrides). These are different features: a bound *restricts*; a default *supplies*.
- **Default (recommended):** ship the **grammar's `= default` form** as the v1 semantics (it is what parses today, requires no grammar change to the declaration, and matches the param/let "default-or-required" model the conformance machinery already uses). Treat a trait-body `type X` (no `=`) as a **required** associated type (RequirementKind::AssocType with no bound); `type X = Default` as **default-providing**. Defer the `: bound` form (constrained associated types) to a follow-up, OR add it as a *second* optional clause (`type Material : Metal = Steel`) if Leo wants both — but that is a grammar extension on the declaration, raising ιₐ's scope.
- **Alt:** implement the spec EBNF `: bound` form, change the grammar's declaration to match the spec, and treat `= default` as unsupported (or a separate extension). Pro: matches the written spec exactly; gives "narrow associated types" (spec §4.2 refinement) a concrete bound to narrow. Con: requires changing the *already-parsing* declaration grammar (regresses the two passing fixtures), and the bound-checking conformance logic is heavier than default-injection.
- **Impact:** This decides ιₐ's grammar scope and ιᵦ's conformance semantics. The PRD body (§4.5, §5.3a) is written to the **default `= default` semantics** with the bound as an optional `AssocTypeBound`; if Leo picks the spec-EBNF alt, §4.5/§5.4 conformance flips from "must supply a binding" to "binding must satisfy the bound" and the spec-vs-grammar reconciliation becomes part of ιₐ. **Recommend the `= default` form; flag the spec EBNF as needing an amendment note (θ) either way, since spec and grammar currently disagree.**

### FORK-F — Associated types in `purpose` / `occurrence` / `constraint` / `field` bodies?
- **Default (recommended):** **trait + structure only.** Keep the `traits.rs:417` purpose-body hard-reject; associated-type bindings are meaningful only where conformance is declared (structures conforming to traits). A `purpose` body has no conformance relationship to bind against.
- **Alt:** allow associated types anywhere a member list appears. Rejected for v1: no consumer, and the reject diagnostic is already in place.
- **Impact:** Local to ιᵦ — leave the purpose-body reject; only the trait-compile (`traits.rs:205`) and structure-member (`entity.rs:1446`) no-ops become real.

### FORK-G — Qualified associated-type disambiguation syntax.
- **Default (recommended):** reuse the value-side qualifier shape: bare `Beam::Material` works when unambiguous; `Beam::(HasMaterial::Material)` disambiguates when two conformed traits both declare `Material`. This mirrors the value-side `obj.(Trait::member)` convention the fn half already extends — one syntactic idea, two positions (value + type).
- **Alt:** require the trait qualifier always (`Beam::(HasMaterial::Material)` mandatory, no bare form). Rejected: noisy for the common single-trait case.
- **Impact:** Local to ιₑ grammar + resolution. Consistent with GR-040 (no method-call sugar) — `::` qualified paths only, never inferred.

---

## §12 — Out of scope

- Bare `obj.fn()` method-call sugar (FORK-A Alt 2; contradicts GR-040).
- Data-carrying enums, `Result<T>`/`fallback` (spec §18 deferred #4, #6 — independent).
- `@optimized` on assoc fns routing through ComputeNode — assoc fns are compile-time-resolved pure code; if a future expensive trait fn needs a trampoline, that composes with `compute-node-contract.md` separately (out of scope here; no shared machinery in v1).
- **Tuples — explicitly NOT a dependency and NOT being added to Reify; no mechanism here (fn return, assoc-type binding, qualified access) uses or requires a tuple type.**
- Trait-static fn per-conformer override (statics are trait-level only; §5.2).
- **Associated types in `purpose`/`occurrence`/`field` bodies** (FORK-F; trait + structure only).
- **Constrained associated types (`type X : Bound` spec-EBNF form)** if FORK-E picks the `= default` semantics — deferred to a follow-up unless Leo opts into the dual-clause `type X : Bound = Default` grammar in ιₐ.
- **`auto:` type-param resolution wiring** (task 3558) — soft-coupled shared machinery, not owned here (§9).
- A `Self` type-name (uppercase) — Reify has none; associated-type qualified access uses concrete structure names / type-params, not `Self` (§13 Q1).

## §13 — Open questions (tactical; not design-blocking)

1. **`self` keyword vs `Self` type-name collision.** Reify has no `Self` type today, and this PRD does **not** introduce one — associated-type qualified access (ιₑ) uses concrete structure names / type-params, not `Self`. Confirm lowercase `self` (value-position keyword, fn half) and any future uppercase `Self` (type) won't lex-collide. **Suggested:** keep `self` value-position-only; do not add `Self`. Decide during tasks α (fn) and ιₐ (type).
2. **Diagnostic codes.** `E_TRAIT_FN_NOT_SATISFIED`, `E_TRAIT_METHOD_UNKNOWN`, reuse of `E_AMBIGUOUS_CALL` — confirm against the existing `DiagnosticCode` enum naming convention. Decide during task δ.
3. **`self` in a default expression.** Can an assoc-fn param default reference `self`? Free-fn defaults compile in a neutral scope (no sibling refs — `CompiledFunction::param_defaults` doc). **Suggested:** assoc-fn param defaults follow the same neutral-scope rule — no `self`, no sibling params. Decide during task ζ.
4. **Recursion across assoc fns.** Spec §4.3 permits self/mutual recursion for free fns. Confirm an assoc fn may call another assoc fn (qualified) recursively; runtime stack-overflow is the documented failure (not compile-time). Decide during task ζ.
