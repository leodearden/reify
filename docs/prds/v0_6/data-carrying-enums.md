# PRD: Data-Carrying Enums (Algebraic Data Types)

Status: deferred (spec-gap batch `spec-gap-2026-05-27`, cluster `data-carrying-enums`). Decomposition style **B + H** (design-first contract + boundary tests) per `preferences_implementation_chain_portfolio`. Authored 2026-05-27.

Resolves spec §18.6 roadmap item 6 ("Data-carrying enums — algebraic data types with associated values"). Extends the existing `match` seam (spec §5.10 expr / §6.4 decl-level) for payload binding.

**Payload shape decision (Leo, 2026-05-27): variant payloads are NAMED-FIELD ONLY.** Declaration `Rect { width: Length, height: Length }`; pattern `Rect { width: w, height: h }`. The positional-inline form (`Rect(Length, Length)`) is **dropped entirely** — it is not part of this design. Rationale: consistent with Reify's no-tuples / "structure covers named aggregates" stance; avoids positional-identity fragility (the exact smell the v0_6 keyed-collection PRD removes). The former fork F3 (positional vs named) is therefore **resolved and closed**; this PRD no longer presents it.

## §1 — Goal

Today Reify enums are C-style bare variants only: `enum Directionality { In, Out, Bidi }` (spec §3.8, §4.5; grammar `enum_declaration` = identifiers; `EnumDef.variants: Vec<String>`; runtime `Value::Enum { type_name, variant }` with no payload slot). This PRD adds **associated data to variants** plus **pattern matching that binds the payloads**, turning enums into algebraic data types.

What a user can do when this lands (the observable surface):

```reify
enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point
}

structure def Widget {
    param outline : Shape = Rect { width: 20mm, height: 10mm }

    // Bind the payload fields by name in a match arm; the bound names are in scope in the body.
    let area = match outline {
        Circle { radius: r }        => 3.14159 * r * r,
        Rect { width: w, height: h } => w * h,
        Point                        => 0mm * 0mm
    }
}
```

Running `reify check widget.ri` accepts the file; `reify eval` (or the existing value-print path) reports `area = 200 mm^2` for the default `Rect { width: 20mm, height: 10mm }`. Change the default to `Circle { radius: 5mm }` and re-eval → `area ≈ 78.54 mm^2`. Mis-spelling a variant, omitting a non-`_` arm, or binding an unknown payload field each produces a specific compile diagnostic. That is the end-to-end signal.

## §2 — Consumer (G1)

This is a **core-language / grammar capability**. Its consumers are user surfaces and downstream PRDs, not an in-engine seam (no `engine-integration-norm.md` §3 seam is touched — pattern matching is a compile-time + `reify-expr` evaluation concern, never a kernel hook).

Named consumers:

1. **User surface — CLI eval.** `reify check` / `reify eval` over an `.ri` file that declares a data-carrying enum, constructs a variant, and matches it with payload binding. This is the primary G2 signal-bearer (§8 task ζ).
2. **User surface — stdlib `.ri` example.** A checked-in example (`examples/m6_data_carrying_enum.ri`) that exercises construction + payload-binding match and runs in CI.
3. **Downstream PRD — `docs/prds/match-block-decls.md`.** That PRD explicitly scoped OUT "pattern-matching with payload-bound names (v0.1 enums are C-style per §3.8)" — i.e. it relies on *this* PRD to provide payload binding. Decl-level `match` blocks (§6.4) that bind payloads in a `sub`-producing arm consume this PRD's pattern grammar + binding semantics. **This PRD owns the payload-binding extension of the match seam; match-block-decls.md consumes it.** (See §6.)
4. **Spec self-consistency — `Option<T>` patterns.** Spec §5.10 / §9.2.8 already document `some(c) => ...` payload binding and `some(some(x))` nesting, but the tree-sitter `match_pattern` rule (`identifier | _`) does **not** parse `some(c) =>` today (verified: parse fails). The `Option` payload is a *single anonymous value*, not a named-field record — so `some(c)` is a **positional 1-binder** pattern, which does **not** fit the named-field-only variant grammar this PRD adds. The named-field switch therefore **decouples** the `Option`-pattern gap from this PRD: closing `some(c)`/`none` either needs its own single-binder `some(IDENT)` production (Option is compiler-intrinsic, §3.8) or a named-field `Option` field name. This is now a **design fork** (see `## DESIGN FORKS FOR LEO`, F4 below), no longer an automatic side-effect.

No mechanism in this PRD is a producer without one of the above consumers.

## §3 — Background: current implementation chain

The full chain that must change, end to end (verified 2026-05-27):

| Layer | File | Today | Needs |
|---|---|---|---|
| Grammar | `tree-sitter-reify/grammar.js` `enum_declaration` | `identifier (',' identifier)*` | variant with optional named-field payload `Name { field: Type, ... }` |
| Grammar | `tree-sitter-reify/grammar.js` `match_pattern` | `identifier (\| identifier)* \| '_'` | named-field binding sub-patterns `Variant { field: binder, ... }` |
| AST | `reify-ast/src/decl.rs` `EnumDecl.variants` | `Vec<String>` | `Vec<EnumVariantDecl>` (name + named-field payload spec) |
| AST | `reify-ast/src/ast.rs` `MatchArm.patterns` | `Vec<String>` | structured patterns carrying named-field binders |
| AST | `reify-ast/src/ast.rs` `EnumAccess { type_name, variant }` | bare variant ref | variant **construction** with named-field args |
| IR | `reify-ir/src/traits.rs` `EnumDef.variants` | `Vec<String>` | `Vec<EnumVariantDef>` (name + named-field payload types) |
| IR | `reify-ir/src/expr.rs` `CompiledMatchArm.patterns` | `Vec<String>` | structured `CompiledPattern` with field-name→binder cell IDs |
| Value | `reify-ir/src/value.rs` `Value::Enum { type_name, variant }` | no payload | named-field payload slot (fork F1) |
| Construct | `reify-compiler/src/expr.rs` `ExprKind::EnumAccess` arm | literal `Value::Enum` | named-field variant construction `Rect { width: w, height: h }` |
| Eval | `reify-expr/src/lib.rs:503` `Match` arm | string-eq on `variant`, no bind | crack payload field map, bind names into scope, eval body |
| Exhaustiveness | `reify-compiler/src/expr.rs:2292` | covered-set of variant names vs `EnumDef.variants` | unchanged at variant granularity; payload field-set/types validated separately |

`Shape.Round` parses as `member_access` then a disambiguation pass (`ts_parser.rs` ~line 2988) rewrites to `EnumAccess` when the head is a known enum. Bare `Point` keeps that path. A payload variant **construction** is the named-field form `Rect { width: w, height: h }` (fork F2 covers whether construction reuses the *brace* form or the call-shaped `Rect(width: w, height: h)` form; the latter already parses today as a `function_call` with `named_argument`s — see §4.2 / F2). Whichever surface F2 picks, a disambiguation pass rewrites the parsed node into a variant-construction node when the callee/head resolves to a known payload-carrying variant of an in-scope enum.

## §4 — Sketch of approach (surface syntax)

### 4.1 Variant declaration (named-field only)

```reify
enum Shape { Circle { radius: Length }, Rect { width: Length, height: Length }, Point }
```

A variant is `Name` (bare, as today) or `Name { field: Type, field: Type, ... }` (named-field payload). There is **no positional payload form** — `Rect(Length, Length)` is not legal syntax (positional-inline dropped, Leo 2026-05-27). A one-field variant is still written `Circle { radius: Length }` (no positional sugar). Field names within a variant are unique; field order in the declaration is the canonical order (used for content-hash field ordering, §11 Q4).

### 4.2 Variant construction (expression position)

```reify
Circle { radius: 5mm }                 // named-field payload variant
Rect { width: 20mm, height: 10mm }
Point                                   // bare variant — unchanged (EnumAccess path)
```

Construction names every field; all declared fields must be supplied (no partial / defaulted fields in v1). Construction is type-checked: the **field-name set** must equal the variant's declared field set, and each field value's type must match its declared type. `Rect { width: 20mm }` (missing `height`) → missing-field diagnostic; `Circle { radius: "x" }` → type diagnostic; `Circle { diameter: 5mm }` → unknown-field diagnostic.

**Construction surface (fork F2, see DESIGN FORKS):** the *brace* form `Circle { radius: 5mm }` shown above is the default-presented form but requires net-new grammar. The call-shaped alternative `Circle(radius: 5mm)` **already parses today** as a `function_call` with `named_argument`s (verified 2026-05-27) and mirrors how structures are instantiated (`StructName(field: value)`); it would need only the disambiguation pass, no new construction grammar. F2 picks which surface ships. The §1 example and §8 ζ fixture use the brace form pending that decision.

### 4.3 Payload-binding patterns (match arm)

```reify
match outline {
    Circle { radius: r }         => 3.14159 * r * r,   // binds r : Length
    Rect { width: w, height: h } => w * h,             // binds w, h : Length
    Point                        => 0mm * 0mm
}
```

A binding pattern names the fields to bind: `Variant { field: binder, ... }`, where each `binder` is a fresh identifier brought into scope in that arm's body. Bound names are in scope only in that arm's body. The pattern must name a subset of the variant's declared fields (binding all of them is the common case; partial binding / field-omission ergonomics are §11 Q4-adjacent — v1 requires naming every field to keep it simple, see §10). Pipe-alternation (`A | B =>`) is preserved for bare variants; payload-binding arms are single-variant (a `Circle{radius:r} | Rect{...} =>` arm would bind incompatible name sets — diagnose, out of scope §10). Wildcard `_` and a bare variant name (ignoring the payload) remain legal arms.

### 4.4 Grammar reality check (G3) — named-field fixtures FAIL today

Fixtures parsed with `tree-sitter parse` (tree-sitter 0.26.8), re-run 2026-05-27 for the named-field design. **Per the silent-misparse trap, the signal is the CST shape (presence of `ERROR` nodes), not the exit code alone:**

| Fixture | Syntax | Result |
|---|---|---|
| `dce-2-nameddecl.ri` | `enum Shape { Point, Circle { radius: Length }, Rect { width: Length, height: Length } }` | **FAIL** — `enum_declaration` entered but the `{ field: Type }` body collapses into `ERROR` subtrees (no production for braced variant payload) |
| `dce-4-namedbind.ri` | `match outline { ..., Circle { radius: r } => r, Rect { width: w, height: h } => w }` | **FAIL** — the whole `structure def` collapses into a top-level `ERROR`; the `{ field: binder }` pattern derails parsing |
| `dce-0-baseline.ri` | bare enum + bare-variant match incl. `In \| Out` pipe | **PASS** — parses with **0 `ERROR` nodes** (true clean parse, the regression floor) |
| (reference) `Circle(radius: 5mm)` | call-shaped named construction | **PASS** — parses today as `function_call` + `named_argument` (informs F2; needs only disambiguation, not new grammar) |

`dce-1-posdecl.ri` and `dce-3-posbind.ri` (positional fixtures) are **removed** — positional syntax is no longer part of this design.

**G3 resolution: grammar work is a hard prerequisite, queued as explicit tasks (path (b)).** `grammar_confirmed=false` on the grammar-production leaves (§8 tasks α, β); every downstream leaf `depends_on` them. The named-field decl fixture (`dce-2-nameddecl.ri`) is α's exit-0 + zero-ERROR signal; the named-field pattern fixture (`dce-4-namedbind.ri`) is β's.

## §5 — Resolved design decisions

- **D1 — Payload values are first-class `Value`s, eagerly evaluated, keyed by field name.** A variant payload is a **named-field map** of `Value`s (`field_name → Value`). Construction `Circle { radius: expr }` evaluates `expr` to a `Value` and stores it under `radius`. No laziness, no thunks. Consistent with the rest of the pure-functional eval model and with `structure`'s named-field aggregation.
- **D2 — `undef` propagation: variant tag is structure, payload field is value.** Mirrors the §9.2.6 collection rule ("structure determined, elements may be undef"). `Circle { radius: undef }` is a **determined** `Shape` whose tag is `Circle` and whose `radius` field is `undef`. A `match` on it **selects the `Circle` arm** (tag is known) and binds `r = undef`; the body then propagates `undef` per normal rules (§9.2.7). This differs from a bare-`undef` discriminant, which selects no arm and yields `undef` (§9.2.5, unchanged). **This is a premise the spec does not yet state for payload enums — §6 G6 validates it; §9 flags the spec-text companion.**
- **D3 — Determinacy of the discriminant tag.** A `match` selects an arm iff the discriminant's **tag** is determined. `Value::Enum { variant: undef-of-enum-type }` (the whole value undef) selects no arm → `undef`. A determined-tag-with-undef-payload selects the tagged arm. The selector key is the tag string, exactly as today; payload determinacy never blocks arm selection.
- **D4 — Exhaustiveness is over tags only.** The existing check (`expr.rs:2292`, covered-set of variant **names** vs `EnumDef`) is correct as-is at tag granularity and is preserved. Payloads add **no new exhaustiveness obligation** (no payload-value guards in v1 — see §10). Adding a payload to a variant does not change which arms are needed to be exhaustive.
- **D5 — Construction is disambiguated like `EnumAccess`.** A named-field construction (`Rect { width: w, height: h }` brace form, or `Rect(width: w, height: h)` call-shaped form per fork F2) is rewritten by a disambiguation pass to a variant-construction node when the head/callee name resolves to a known payload-carrying variant of an in-scope enum. Bare `Point` keeps the `EnumAccess` path. Name collision between a variant and a user `fn`/`structure` of the same name resolves variant-first within a position where the expected type is the enum (tactical; §11 Q3). (The call-shaped F2 form shares its surface with `structure` instantiation `StructName(field: value)`, so the disambiguation must additionally distinguish a known variant from a known structure name — see F2.)
- **D6 — Content-hash includes payload (field-name keyed).** `Value::Enum` content-hashing (`value.rs:857`) currently combines `type_name` + `variant`; it must additionally combine each payload field's name + value content hash so `Circle { radius: 5mm } ≠ Circle { radius: 6mm }` for cache-key / freshness purposes. Fields are folded in **declaration order** (§11 Q4) so the hash is order-stable regardless of construction-site field order. Determinacy-stable, NaN-canonical per the existing `content_hash` contract.
- **D7 — Equality/Ord over payload (field-name keyed).** `Value::Enum` `PartialEq`/`Ord` (`value.rs:1833`) extend to compare payload field maps after tag. Two same-tag values compare field-by-field in declaration order; different tags order by tag as today. Required for `Set<Shape>` / `Map<Shape, _>` membership.
- **D8 — Backward compatibility.** Bare-variant enums (`enum Directionality { In, Out, Bidi }`) and bare-variant matches keep parsing, constructing, and matching exactly as today. A bare variant is the empty-field-map case of the general form (`VariantPayload::Unit`). The `dce-0-baseline.ri` fixture is the regression pin (verified 2026-05-27: parses with 0 `ERROR` nodes).

## §6 — Cross-PRD / cross-cluster relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/match-block-decls.md` | this **produces**, that **consumes** | payload-binding `match_pattern` grammar + binder semantics in decl-level `match` arms | **this-prd** (data-carrying-enums) | match-block-decls.md already scopes payload-binding OUT and defers to here; no reciprocal ambiguity. Companion-correction task §8 ι cross-links. |
| `docs/prds/v0_6/keyed-collection-identity.md` | independent (note only) | both touch `forall`/pattern binders, but keyed-collection's `(k,v)` binder is deferred to a (non-existent) tuple PRD; this PRD's binders are variant-payload only | n/a | no shared grammar production; **no edge**. |
| `docs/prds/v0_3/structure-instance-runtime.md` (GR-001) | **only if** fork F1 = StructureInstance-backed payload | `Value::StructureInstance` as the payload carrier | structure-instance-runtime owns the type | **avoided by default**: resolved F1 = inline named-field payload (`Vec<(String, Value)>` / field map) on `Value::Enum`, which has **no** GR-001 dependency. Note: named-field-only makes the payload structurally similar to `StructureInstanceData`'s field map — but **reusing** `Value::StructureInstance` as the carrier (F1-b) is still a separate choice that would add a hard GR-001 edge. The inline named-field map is self-contained and keeps this PRD independently shippable. |

**Seam ownership statement (G4):** the match seam's *payload-binding* extension is owned **here**. Bare-variant match (§5.10/§6.4) shipped earlier; decl-level match-block grouping is owned by `match-block-decls.md`; payload binding inside either is owned by this PRD. No contested-ownership pair (checked against `phase-3-breadcrumb-map.md` §3 — none of the three known pairs is touched).

**Tuple constraint (Leo, this session):** tuples are NOT being added. Variant payloads are **named-field only** — never positional, never a tuple type. No payload shape in this PRD depends on a `(A, B)` tuple value or type. Confirmed: D1's payload is a flat `field_name → Value` map, not a tuple `Value` and not a positional list. The named-field-only decision is reinforced *by* the no-tuples stance: there is no positional aggregate in the language, so a positional payload would have been the only positional aggregate — exactly the inconsistency this avoids.

## §7 — Contract section (B+H)

The seam is between `reify-compiler` (lowers patterns + construction; checks field-set/types/exhaustiveness) and `reify-expr` (cracks the payload field map at eval, binds into scope). Both sides face the same two data structures.

### 7.1 IR data structures (the contract surface)

```rust
// reify-ir/src/traits.rs — replaces EnumDef.variants: Vec<String>
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariantDef>,
    pub doc: Option<String>,
}
pub struct EnumVariantDef {
    pub name: String,
    pub payload: VariantPayload,         // Unit for bare variants
}
pub enum VariantPayload {
    Unit,                                // bare variant (today's case)
    Named(Vec<(String, Type)>),          // named-field: Rect { width: Length, height: Length }
                                         // declaration order is canonical (content-hash / Ord order, §11 Q4)
}

// reify-ir/src/value.rs — Value::Enum gains a named-field payload slot (F1 = inline name→Value map)
Value::Enum {
    type_name: String,
    variant: String,
    payload: Vec<(String, Value)>,       // field_name → Value, in declaration order;
                                         // empty for bare variants; preserves D8
}

// reify-ir/src/expr.rs — replaces CompiledMatchArm.patterns: Vec<String>
pub struct CompiledMatchArm {
    pub patterns: Vec<CompiledPattern>,
    pub body: CompiledExpr,
}
pub enum CompiledPattern {
    Wildcard,                            // _
    Variant { name: String },            // bare variant, ignores any payload
    VariantBind {                        // Circle { radius: r } / Rect { width: w, height: h }
        name: String,
        binders: Vec<(String, ValueCellId)>,  // (field_name, binder cell) per bound field
    },
}
```

(`Vec<(String, Value)>` / `Vec<(String, Type)>` are illustrative — an ordered map type may be used so long as declaration order is the canonical iteration order for D6/D7. Field-name keying, not positional, is the load-bearing requirement.)

### 7.2 Invariants

- **INV-1 (field-set match).** A `VariantBind`'s bound field names are a subset of the matched variant's declared field set (v1 requires all fields named — §4.3); every bound field name exists on the variant. Compiler rejects unknown / missing-field patterns with a diagnostic before IR is built. (Replaces the old positional-arity invariant.)
- **INV-2 (binder scope).** A `VariantBind`'s binder cells are live **only** during evaluation of that arm's body. Eval inserts them into a child scope; no leakage to sibling arms or the enclosing scope.
- **INV-3 (tag-only selection).** Arm selection compares `discriminant.variant` (tag string) to the pattern's variant name; payload field values never enter selection (D3).
- **INV-4 (undef payload field, determined tag).** A determined-tag value selects its arm even when a payload field is `undef`; the binder cell receives `Value::Undef` and the body propagates per §9.2.7 (D2).
- **INV-5 (back-compat).** `VariantPayload::Unit` + empty `payload` map + `CompiledPattern::Variant` reproduces today's bare-enum behavior bit-for-bit (content hash, eq, ord, eval). The `dce-0-baseline.ri` + existing `m5_guarded_enum.ri` fixtures stay green.
- **INV-6 (content-addressing).** `content_hash` and `Ord`/`PartialEq` of `Value::Enum` fold in the payload **field map in declaration order** (D6/D7), so the value is order-stable regardless of construction-site field order, preserving the NaN-canonical / determinacy-stable contract documented at `value.rs:774`.

### 7.3 Error semantics (user-visible diagnostics — G2 leaf signals)

| Code (illustrative) | Trigger | Where |
|---|---|---|
| `E_VARIANT_MISSING_FIELD` | `Rect { width: 20mm }` — declared field `height` not supplied | compiler, construction |
| `E_VARIANT_UNKNOWN_FIELD` | `Circle { diameter: 5mm }` — field not declared on variant | compiler, construction |
| `E_VARIANT_PAYLOAD_TYPE` | `Circle { radius: "x" }` — payload field type mismatch | compiler, construction |
| `E_PATTERN_UNKNOWN_FIELD` | `Circle { diameter: d } =>` — pattern names a field not on the variant | compiler, pattern |
| `E_PATTERN_MISSING_FIELD` | `Rect { width: w } =>` — pattern omits declared field `height` (v1 requires all) | compiler, pattern |
| `E_UNKNOWN_VARIANT` | `Triangle { x: v } =>` — variant not in enum | compiler (existing, extended) |
| (existing) non-exhaustive | missing tag with no `_` | compiler (unchanged, D4) |

## §8 — Decomposition plan (DAG; not yet filed)

**B + H.** Grammar leaves first (G3 prereq), then IR widening, then the two seam sides, then the end-to-end consumer leaf (the integration gate carrying the user-observable signal), then companions. Greek labels; real IDs assigned at decompose.

### Phase 1 — Grammar (G3 prerequisite; `grammar_confirmed=false`)

- **Task α — `enum_declaration` named-field payload grammar + parser test + AST lowering.**
  - Extend `enum_declaration` to accept `Name` (bare) and `Name { field: Type, field: Type, ... }` (named-field payload). No positional form. Lower to `EnumVariantDecl` (name + ordered named-field list) in `reify-ast`; update `lower_enum`.
  - **Observable signal:** `dce-2-nameddecl.ri` parses with **0 `ERROR` nodes** in the CST (not exit-code alone — silent-misparse trap); a parser test in `tree-sitter-reify/tests/` asserts the named-field variant production; `m5_guarded_enum.ri` still parses (bare-enum regression). `grammar_confirmed=false`.
  - **Crates:** tree-sitter-reify, reify-ast, reify-syntax. **Prereqs:** none.

- **Task β — `match_pattern` named-field binding grammar + parser test + AST lowering.**
  - Extend `match_pattern` to `Variant { field: binder, ... }` (named-field binders). Lower to structured `MatchArm.patterns` carrying `(field_name, binder)` pairs in `reify-ast`/`reify-syntax`. (`some(IDENT)`/`none` are NOT subsumed here — Option's payload is anonymous/positional and does not fit named-field; see F4.)
  - **Observable signal:** `dce-4-namedbind.ri` parses with **0 `ERROR` nodes** in the CST; a parser test asserts the named-field binding production; `dce-0-baseline.ri` (bare + pipe arms) still parses with 0 `ERROR` nodes. `grammar_confirmed=false`.
  - **Crates:** tree-sitter-reify, reify-ast, reify-syntax. **Prereqs:** none (parallel with α).

### Phase 2 — IR widening (intermediate; unlocks the seam sides)

- **Task γ — `EnumDef`/`EnumVariantDef`/`VariantPayload::Named` + `Value::Enum` named-field payload slot.**
  - Widen IR per §7.1: `VariantPayload ∈ {Unit, Named(Vec<(String, Type)>)}`; `Value::Enum.payload: Vec<(String, Value)>` (field map, declaration order). `VariantPayload::Unit` + empty payload keeps INV-5. Extend `content_hash` (D6) and `PartialEq`/`Ord` (D7) to fold the field map in declaration order.
  - **Observable signal (intermediate):** unit tests in `reify-ir` pin: `Circle { radius: 5mm } != Circle { radius: 6mm }` by content hash and by `Ord`; construction-site field order does not affect the hash (declaration-order canonicalization); `Value::Enum` with `Unit` payload round-trips identically to the old shape (INV-5). **Unlocks:** δ, ε. **Consumer:** §8 tasks δ/ε.
  - **Crates:** reify-ir (traits.rs, value.rs, expr.rs — `CompiledPattern`/`CompiledMatchArm`). **Prereqs:** α, β (AST shapes feed IR).

### Phase 3 — Producer side (compiler)

- **Task δ — Named-field variant construction + payload field-set/type checking.**
  - Disambiguate the parsed construction node → variant-construction when the head/callee is a known payload variant (D5; F2 picks brace `Rect { width: w, height: h }` vs call-shaped `Rect(width: w, height: h)`); emit `E_VARIANT_MISSING_FIELD` / `E_VARIANT_UNKNOWN_FIELD` / `E_VARIANT_PAYLOAD_TYPE`; build `Value::Enum { payload: field map }` literal/expr. If F2 = call-shaped, the disambiguation must also distinguish a known variant from a known `structure` name (both use `Name(field: value)`).
  - **Observable signal:** `reify check` over `Rect { width: 20mm }` (missing `height`) emits `E_VARIANT_MISSING_FIELD`; `Circle { diameter: 5mm }` emits `E_VARIANT_UNKNOWN_FIELD`; `Circle { radius: "x" }` emits `E_VARIANT_PAYLOAD_TYPE`; a valid `Rect { width: 20mm, height: 10mm }` checks clean. (CLI diagnostic — user-observable leaf.)
  - **Crates:** reify-compiler (expr.rs EnumAccess/construction disambiguation, field_check). **Prereqs:** γ.

- **Task ε — Pattern compilation + field-set check + exhaustiveness preserved.**
  - Compile `match_pattern` to `CompiledPattern` with `(field_name, binder cell)` pairs; check `E_PATTERN_UNKNOWN_FIELD` / `E_PATTERN_MISSING_FIELD` (INV-1); preserve tag-only exhaustiveness (D4).
  - **Observable signal:** `reify check` over `Circle { diameter: d } =>` (unknown field) emits `E_PATTERN_UNKNOWN_FIELD`; `Rect { width: w } =>` (omits `height`) emits `E_PATTERN_MISSING_FIELD`; a non-exhaustive payload-enum match (missing tag, no `_`) still emits the existing non-exhaustive diagnostic; an exhaustive payload-binding match checks clean. (CLI diagnostics — user-observable leaf.)
  - **Crates:** reify-compiler (expr.rs Match arm). **Prereqs:** γ. (Parallel with δ.)

### Phase 4 — Consumer side (eval) + end-to-end integration gate

- **Task ζ — Named-field payload-binding match evaluation (THE integration gate / boundary test).**
  - Extend `reify-expr` `Match` arm (`lib.rs:503`): on `Value::Enum`, select arm by tag (INV-3), crack the payload field map into the pattern's named binder cells (INV-2), eval body; `undef`-field-but-determined-tag selects the arm and binds `undef` (INV-4/D2).
  - **Observable signal (LEAF — primary):** `examples/m6_data_carrying_enum.ri` declares `enum Shape { Circle { radius: Length }, Rect { width: Length, height: Length }, Point }`, sets `param outline : Shape = Rect { width: 20mm, height: 10mm }`, computes `let area = match outline { Circle { radius: r } => 3.14159*r*r, Rect { width: w, height: h } => w*h, Point => 0mm*0mm }`. `reify eval` reports `area = 200 mm^2`. Switch default to `Circle { radius: 5mm }` → `area ≈ 78.54 mm^2`. A second fixture with `Circle { radius: undef }` → `area = undef` with the `Circle` arm selected (D2 observable via trace). Example runs in CI. (This is the §1 signal; it is the B+H integration gate — δ, ε, γ are its intermediates.) (Construction surface follows F2; if F2 = call-shaped, the example uses `Rect(width: 20mm, height: 10mm)`.)
  - **Crates:** reify-expr (lib.rs), examples/, reify-cli (eval path, no change expected). **Prereqs:** γ, δ, ε.

### Phase 5 — Companion corrections (doc; independent)

- **Task η — Spec §3.8 / §4.5 / §5.10 / §9.2.5 update for named-field payload variants + D2 determinacy rule.**
  - Replace "v0.1 enums are C-style" prose with the data-carrying **named-field** form (`Name { field: Type, ... }`; no positional); add the §9.2.x determinacy row for "determined tag, undef payload field → arm selected, binder = undef" (D2/INV-4); update grammar EBNF (`enum_variant`, `pattern`) to the landed named-field grammar. Update spec roadmap §18.6 item 6 from "v0.2+" planned to landed.
  - **Observable signal:** `docs/reify-language-spec.md` updated; the named-field `dce-*` fixtures (`dce-0-baseline`, `dce-2-nameddecl`, `dce-4-namedbind`) are referenced; no code change; doc lint passes.
  - **Crates:** none (docs). **Prereqs:** ζ (describe what actually landed).

- **Task ι — `match-block-decls.md` cross-link + payload-binding note.**
  - Update that PRD's "Out of scope" line ("Pattern-matching with payload-bound names … C-style per §3.8") to reference this PRD as the now-landed provider, and note that decl-level `match` arms may bind payloads once both land.
  - **Observable signal:** `docs/prds/match-block-decls.md` updated with the cross-reference; doc lint passes.
  - **Crates:** none (docs). **Prereqs:** ζ. (Independent of η.)

### Dependency view

```
α ─┐                     ┌─→ δ ─┐
   ├─→ γ ─────────────────┤      ├─→ ζ ─┬─→ η
β ─┘                      └─→ ε ─┘      └─→ ι
```

`grammar_confirmed=false` for α, β (they *create* the grammar). γ–ι are `grammar_confirmed=true` (they consume the grammar α/β land). No out-of-batch prereqs under the resolved forks (F1 inline named-field map ⇒ no GR-001 edge). **DAG unchanged by the named-field switch** — the edge set (α,β→γ→δ,ε→ζ→η,ι) is shape-independent; named-field only changed leaf *signals* and IR shapes, not the dependency topology.

## §9 — Premise validation (G6)

Every §8 leaf signal classified:

- **ζ primary signal — end-to-end capability** ("`area = 200 mm^2`"). Trace: requires (a) named-field payload-variant parse [α], (b) `Value::Enum` field-map payload slot + content/eq [γ], (c) named-field variant construction `Rect { width: 20mm, height: 10mm }` [δ], (d) pattern compile + named binder cells [ε], (e) payload field-map crack eval [ζ]. Every capability is in ζ's dependency set (α→γ→δ/ε→ζ); none is owned by a task that depends on ζ. **Passes** the dependency-set trace. The arithmetic premise (`20mm * 10mm = 200 mm^2`, `3.14159 * 5mm * 5mm ≈ 78.54 mm^2`) is exact `Scalar<Area>` multiplication already in the language (no new numeric capability). **Achievable.**
- **D2 undef-payload premise** (`Circle { radius: undef }` selects Circle arm, binds undef). The mathematical identity: arm selection keys on the **tag** (a determined String when the value is `Value::Enum{variant:"Circle",...}`), independent of payload-field determinacy. This is *new spec behavior* not previously stated — it is **resolved here** (D2/D3) and validated as internally consistent with §9.2.6 (collection structure-vs-element determinacy) and §9.2.7 (strict undef propagation through the body). The configuration that earns it: the discriminant value must itself be determined (tag known); a wholly-`undef` discriminant still yields `undef` per §9.2.5 (unchanged). **Consistent; flagged for spec companion η.**
- **δ, ε signals — diagnostic emission** (`E_VARIANT_MISSING_FIELD`, `E_PATTERN_UNKNOWN_FIELD`, etc.). No quantitative premise; pass trivially. The diagnostic *codes* are illustrative (§7.3); exact strings are tactical (§11 Q1).
- **α, β signals — `tree-sitter parse` yields a CST with 0 `ERROR` nodes.** Mechanically verifiable (CST shape, not exit-code alone — silent-misparse trap); the grammar work is exactly what makes them true. No false premise: the named-field fixtures `dce-2-nameddecl.ri` / `dce-4-namedbind.ri` produce `ERROR` subtrees *today* (re-verified 2026-05-27, §4.4), which is why α/β exist.

No leaf asserts an accuracy bound, closed-form reproduction, or a capability owned downstream. **G6 clear.**

## §10 — Out of scope for this PRD

- **Payload-value guards / refutable nested patterns** beyond one level of field binding. `Circle { radius: r } where r > 5mm =>` or nested ADT destructuring (`Rect { width: Circle { ... } }`) — deferred. v1 binds one named-field level per variant.
- **Partial / defaulted fields, field omission in patterns.** v1 requires construction to name **all** declared fields and patterns to name all fields (§4.2/§4.3). Partial-binding ergonomics (`Rect { width: w, .. }`) and field defaults are deferred.
- **Pipe-alternation across payload-binding arms** (`Circle { radius: r } | Rect { ... } =>`) — incompatible binder sets; diagnose, deferred.
- **Generic / type-parameterized variant payloads** (`enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }`). Recursive ADTs and type parameters on enums are a separate future PRD; v1 payloads are concrete types. (Spec §4.5 recursive-termination rule §note already anticipates "variant type base case" — that PRD builds on this one.)
- **Positional payloads.** Dropped (Leo, 2026-05-27) — `Rect(Length, Length)` is not legal. Named-field is the sole form.
- **Tuples.** Not being added (Leo). Payloads are named-field only, never a tuple value/type.
- **`Option<T>`'s `some(c)`/`none` patterns.** `Option` stays compiler-intrinsic (spec §3.8). Its payload is a single *anonymous* value, which does **not** fit this PRD's named-field-only pattern grammar — so the `some(IDENT)`/`none` parse gap is **no longer closed automatically** by this PRD. Closing it is a separate decision (see F4); deferred unless F4 says otherwise.
- **Decl-level (§6.4) payload binding in `sub`-producing arms** — the grammar/binding lands here, but wiring payload binders into `match-block-decls.md`'s same-name-group machinery is that PRD's task (consumes this PRD).

## §11 — Open questions (tactical; decide at impl)

1. **Exact diagnostic codes/strings** (`E_VARIANT_MISSING_FIELD`, `E_VARIANT_UNKNOWN_FIELD`, `E_VARIANT_PAYLOAD_TYPE`, `E_PATTERN_*` are illustrative). Decide at δ/ε against the existing diagnostic-code registry conventions.
2. **Construction node representation** — reuse a generalized `EnumAccess`-with-named-args AST node vs. a new `VariantConstruct` node. Tactical; either compiles to the same IR literal/expr. Decide at δ (coupled to F2's brace-vs-call surface choice).
3. **Variant/`fn`/`structure` name-collision resolution** — when a payload-variant name equals a user `fn` or `structure` name and the expected type at the call site is the enum, resolve variant-first (D5). With the call-shaped F2 surface this collision is sharper (a variant and a structure both use `Name(field: value)`). Exact tie-break when expected type is ambiguous: decide at δ; **suggested** — variant wins only when the enum type is expected, else `fn`/`structure`.
4. **Named-field payload field ordering in content hash** — **declaration order** (chosen, §7.1 D6/D7) vs sorted-by-name. Decide at γ; **resolved to declaration order** (the variant decl is the single source of canonical order, and unlike `StructureInstanceData` there is no external merge requiring name-sort). Construction-site field order is normalized to declaration order before hashing so `Rect { width: a, height: b }` and `Rect { height: b, width: a }` content-hash equal.

## DESIGN FORKS FOR LEO

> **F3 (payload shape) is RESOLVED: named-field only (Leo, 2026-05-27). It is removed from this list.** The remaining forks below are re-scoped to the named-field design; F2 and F4 changed materially *because* of the named-field switch, and F2 is now genuinely open.

### F1 — `Value::Enum` payload carrier *(default: inline name→Value map)*

- **Default — inline `payload: Vec<(String, Value)>` (field map) on `Value::Enum`.** Self-contained, **no GR-001 / `structure-instance-runtime.md` dependency**, smallest blast radius, declaration-order canonical (D6/D7). Cons: a second named-field value shape alongside `StructureInstanceData` (now structurally similar, since named-field-only).
- **Alt F1-b — reuse `Value::StructureInstance` as the payload carrier** (variant tag + a `StructureInstanceData`-style field map). With named-field-only this is now *more* tempting (the payload IS a named-field map). Pro: one aggregate mechanism, no duplicated field-map machinery. **Con: adds a hard cross-PRD edge on GR-001 (`structure-instance-runtime.md`) into the critical path** — exactly the starvation the audit warns against; couples this PRD's landing to GR-001.
- **Impact:** F1 default keeps this PRD independently shippable (no out-of-batch prereq) — **recommended**. The named-field switch makes F1-b more architecturally attractive (avoids two field-map types) but the cross-PRD coupling cost is unchanged; defer F1-b to a later consolidation refactor rather than a v1 dependency. *No hard reason found to pull in GR-001 — kept inline.*

### F2 — Named-field construction surface *(NOW OPEN — surfaced by the named-field switch)*

This fork is **newly live**. Positional construction (`Rect(w, h)`) was unambiguously call-shaped; named-field construction has two plausible surfaces and Reify has no existing brace-construction precedent:

- **Option F2-a — brace form `Circle { radius: 5mm }`.** Reads like Rust/Swift struct literals; visually distinct from a function call. **Con: net-new grammar** — Reify has **no** `Name { field: value }` expression today (verified 2026-05-27: structures are instantiated `StructName(field: value)`, never with braces). Adds a brace-construction production and its disambiguation.
- **Option F2-b — call-shaped form `Circle(radius: 5mm)`.** **Already parses today** as a `function_call` with `named_argument`s (verified 2026-05-27, 0 ERROR nodes) — needs only the disambiguation pass, **no new construction grammar**. Mirrors exactly how structures are instantiated (`StructName(field: value)`), so it's consistent with the existing language surface. **Con:** construction and pattern become asymmetric (pattern is `Circle { radius: r }` braced — see F2 note — while construction is `Circle(radius: r)` parens), and the variant-vs-`structure` name collision (Q3) is sharper since both use `Name(field: value)`.
- **Pattern surface note:** the *pattern* side `Circle { radius: r }` is braced in both options (a pattern is not an expression, no function-call ambiguity, and `{` after a variant name in pattern position is unambiguous). So F2-b yields construction-`()` / pattern-`{}` asymmetry; F2-a yields construction-`{}` / pattern-`{}` symmetry at the cost of new grammar.
- **Impact:** **load-bearing for α/δ.** F2-a = more grammar, symmetric, struct-literal-familiar. F2-b = least grammar (reuses what parses), consistent with structure instantiation, but asymmetric and sharpens Q3. **Recommend Leo pick.** The §1 example and ζ fixture currently show the brace form (F2-a); flip to `Rect(width:…, height:…)` if F2-b. *(My lean: F2-b — it reuses the existing `Name(field: value)` surface that already parses and matches structure instantiation, keeping the language's construction grammar singular; the construction/pattern asymmetry is mild and mirrors Rust's own `Struct { .. }` value vs `Struct { .. }` pattern being the same only by coincidence of design.)*

### F4 — Close the `Option` `some(c)`/`none` parse gap here? *(default: NO — decoupled by named-field switch)*

The named-field switch **changed this fork's default**. Previously (positional) the new pattern grammar would have subsumed `some(IDENT)` for free. Now:

- **Default — NO, defer.** `Option`'s payload is a single *anonymous* value; `some(c)` is a **positional 1-binder** pattern that does **not** fit the named-field-only `Variant { field: binder }` grammar. Subsuming it would require either (i) adding a positional `some(IDENT)` special-case production (re-introducing a positional pattern form this PRD otherwise excludes), or (ii) giving intrinsic `Option` a named field (`some { value: c }`) — a spec change to a compiler-intrinsic. Neither is in this PRD's named-field-enum scope. The `some(c) =>` parse gap (spec §5.10/§9.2.8 documents it but it doesn't parse today) stays open.
- **Alt F4-b — add a positional `some(IDENT)`/`none` special-case** alongside the named-field enum grammar, scoped to intrinsic `Option` only. Pro: closes the latent spec/grammar divergence. **Con: re-introduces a positional binding form** (the one shape the named-field decision deliberately excludes), creating two pattern idioms.
- **Impact:** default keeps the grammar uniformly named-field; the Option gap becomes a separate follow-up (its own mini-PRD or a tactical grammar patch). F4-b closes the gap but at the cost of grammar non-uniformity. **This is a genuine new decision the named-field switch forced** — under positional it was free; now it is a trade-off. **Recommend Leo decide** whether the Option-pattern gap is closed here (F4-b) or punted (default).

## Assumptions

- The disambiguation-pass infrastructure (`ts_parser.rs` member-access→`EnumAccess` rewrite, ~line 2988) generalizes to function-call→variant-construct (D5/F2). Verified the pass exists; generalizing it is δ's work.
- `reify-expr`'s `Match` eval arm (`lib.rs:503`) is the *only* match-evaluation site for expression-position matches (decl-level match-block eval is `match-block-decls.md`'s separate path). Verified: the expr eval arm handles `Value::Enum`; no second eval site found.
- Exhaustiveness checking (`expr.rs:2292`) is tag-granular and needs no change for payloads (D4). Verified against current code.
