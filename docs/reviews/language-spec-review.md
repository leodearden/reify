# Reify Language Specification v0.1 — Critical Review

## 1. Self-Consistency

### 1.1 "Currency" used for two completely unrelated concepts
**Location:** Section 9.6 vs. Section 3.2
**Severity: Critical**

The term `Currency` is used as both a physical dimension (Section 3.2: the 9th base dimension for monetary values like `USD`, `25USD/kg`) and as an internal evaluation-graph enum (Section 9.6: `Currency` with variants `Final`, `Intermediate`, `Pending`, `Failed`). Line 1475 even says a failed node's result is "marked `Failed` (variant of `Currency` enum)" — using the exact same name as the dimensional type that engineers will work with daily. This is a namespace collision that will cause deep confusion in both human comprehension and implementation. The evaluation-graph enum should be renamed (e.g., `Freshness`, `Validity`, `NodeStatus`).

### 1.2 "Elastic" trait defined twice with different semantics
**Location:** Section 11.6 (`std.structural`) and Section 11.8 (`std.materials.mechanical`)
**Severity: Critical**

`trait Elastic : Flexible` appears in `std.structural` (line 2023) as a structural behavior trait inheriting from `Flexible : Physical`. Separately, `trait Elastic : Material` appears in `std.materials.mechanical` (line 2207) as a material property trait carrying `youngs_modulus` and `poissons_ratio`. These are in different modules so they are technically distinct, but sharing the same unqualified name for two commonly-used traits in the same domain violates the stated regularity principle and will be a constant source of confusion. One should be renamed (e.g., `ElasticBehavior` vs. `ElasticMaterial`, or `Elastic` for the material trait and `ElasticallyDeformable` for the structural one).

### 1.3 Function overloading prohibited but used throughout the standard library
**Location:** Section 4.2.1 vs. Sections 11.5-11.6
**Severity: Critical**

Section 4.2.1 states: "Definition overloading by parameter type is not supported in v0.1." Yet the standard library is full of overloaded functions:
- `fn union(a: Solid, b: Solid)` and `fn union(a: Surface, b: Surface)` (Section 11.5, `std.geometry.boolean`)
- `fn project(point: Point3<Length>, ...)` and `fn project(vector: Vector3<Length>, ...)` (Section 11.5, `std.geometry.constructors`)
- `fn rotate<G>(geometry: G, axis: ..., angle: ...)` and `fn rotate<G>(geometry: G, orientation: ...)` (Section 11.5, `std.geometry.transform`)
- `fn offset_curve` with three different signatures (Section 11.5, `std.geometry.modify`)
- `fn area(surface: Surface)` and `fn area(solid: Solid)` (Section 11.5, `std.geometry.query`)
- `fn curvature(curve: ...)` and `fn curvature(surface: ...)` (Section 11.5, `std.geometry.query`)
- `fn scale<G>(geometry: G, factor: Real)` and `fn scale<G>(geometry: G, factors: Vector3<Real>)` (Section 11.5, `std.geometry.transform`)

Either the "no overloading" rule needs an exception for built-in/`@optimised` functions, or these need distinct names, or the rule needs to be relaxed to permit overloading by arity or by trait-bounded type parameter (which is just trait dispatch).

### 1.4 `Scalar<Q>` has a phantom `N` parameter
**Location:** Section 3.3.1
**Severity: Significant**

`Scalar<Q>` is defined as `Tensor<0, N, Q>`. But `Scalar<Q>` only takes one type parameter (`Q`). What is `N`? For a rank-0 tensor, spatial dimensionality is meaningless, so `N` is a phantom parameter. The spec should state explicitly that `N` is universally quantified (i.e., `Scalar<Q>` is `forall N. Tensor<0, N, Q>`) or that `Scalar<Q>` is a genuinely independent type that happens to be compatible with tensors.

### 1.5 `constraint_expr` referenced but never defined
**Location:** Section 13 (Grammar)
**Severity: Significant**

The grammar production `where_clause ::= 'where' constraint_expr (',' constraint_expr)*` references `constraint_expr`, but this non-terminal is never defined anywhere in the grammar. It should presumably be `expr` (restricted to boolean expressions), or it needs its own production.

### 1.6 Per-declaration `where` guards not in the grammar
**Location:** Section 6.3 vs. Section 13
**Severity: Significant**

Section 6.3 describes per-declaration `where` guards:
```
sub fan_mount : FanMount where needs_cooling { ... }
constraint vent_count >= 2 where needs_cooling
```
But the grammar productions for `sub_decl`, `param_decl`, `constraint_line`, and `let_decl` in Section 13 have no `where` clause attachment point. The grammar only has `where_block` (the block-level form) and `where_clause` (on entity/trait definitions). Per-declaration guards are a core feature that cannot be parsed from the given grammar.

### 1.7 `match` block desugars to multiple `sub` with same name
**Location:** Section 6.4
**Severity: Significant**

The `match` block desugars to:
```
sub head : HexHead where head_type == HeadType.Hex { ... }
sub head : SocketHead where head_type == HeadType.Socket { ... }
```
Multiple `sub` declarations with the name `head` but different types. Section 8.5 (Shadowing) says shadowing is warn-not-forbid for names in child/parent scopes, but says nothing about multiple declarations of the same name in the same scope. The spec needs to explicitly state that `match`-guarded declarations with the same name but mutually exclusive guards are permitted and treated as a single logical declaration.

### 1.8 Map literal syntax collides with parenthesised expressions
**Location:** Section 3.4, Section 13
**Severity: Significant**

Map literals use `("key" => value, "k2" => v2)`. But parenthesised expressions also use `(expr)`. A single-entry map `("key" => value)` could be ambiguous — the parser must look inside the parentheses to find `=>` to distinguish `(expr)` from map literal. Worse, a single-element tuple type `(A, B, C)` uses the same delimiters. This contradicts the LL(1) parseability goal (Section 1.3). Consider using a prefix like `map(...)` or `map{...}`.

### 1.9 `in` is both a keyword and an imperial unit
**Location:** Section 2.10 (keywords), Section 11.4 (`std.units.imperial`)
**Severity: Significant**

`in` is listed as a direction keyword (Section 2.10). In `std.units.imperial` (line 1741), `in` is defined as the unit for inches (`in` = 25.4mm). Since unit expressions must be attached to numbers with no space (`5in`), the parser can probably disambiguate, but the spec should explicitly address this collision and confirm that `5in` is unambiguously a quantity literal while `in` alone is always the keyword.

### 1.10 Keyword count mismatch
**Location:** Section 2.10 vs. Section 15
**Severity: Minor**

Section 15 lists 43 keywords. Section 2.10 groups them by category. Counting the Section 2.10 list: `structure`, `occurrence`, `constraint`, `field` (4) + `def`, `param`, `port`, `sub`, `let`, `fn`, `pub`, `module`, `import`, `trait`, `purpose`, `enum`, `unit` (13) + `connect`, `chain` (2) + `where`, `match`, `if`, `then`, `else` (5) + `and`, `or`, `not`, `implies`, `forall`, `exists` (6) + `in`, `out` (2) + `true`, `false`, `undef`, `auto`, `some`, `none` (6) + `minimize`, `maximize` (2) + `meta` (1) + `self` (1) + `set` (1) + `as` (1) = 44. Section 15 says 43. The discrepancy is small but needs reconciliation.

---

## 2. Completeness

### 2.1 No specification of operator semantics across types
**Location:** Section 5.1, Section 5.2
**Severity: Critical**

The spec says arithmetic is "dimensionally checked" but never specifies what operations are valid on what types. A language implementer would need to know:
- Can you add/subtract `Int` values? (Presumably yes, but result type?)
- What does `Real * Scalar<Length>` produce? Is `Real` the same as `Scalar<Dimensionless>`?
- What is the result type of `Int * 5mm`? Is it `Scalar<Length>`?
- What does `==` mean for each type? Structural? Bitwise? Tolerance-based?
- Can you compare `Int == Real`? Does implicit promotion apply?
- What does `<` mean for `String`? Is it even defined?
- What does `%` (modulo) do on dimensioned integers?
- `Point + Vector -> Point` is specified, but what about `Vector * Scalar`?

A complete type-indexed table of binary operators and their return types is needed.

### 2.2 Relationship between `Real`, `Scalar<Dimensionless>`, and `Int`
**Location:** Sections 3.1, 3.2, 3.3.1
**Severity: Critical**

The spec introduces `Real` (Section 3.1), `Scalar<Q>` (Section 3.3.1), and `Scalar<Dimensionless>` (implied). Are `Real` and `Scalar<Dimensionless>` the same type? If not, how do they interact? Can a `Real` be passed where `Scalar<Dimensionless>` is expected? The `normalize` function returns `Vector<N, Dimensionless>` — can its components be used as `Real`? The `lerp` function takes `t: Real` — could you pass a `Scalar<Dimensionless>`?

This is architecturally fundamental and completely unspecified. If they are distinct, the type system has a seam that will generate constant friction. If they are the same, the spec should say so.

### 2.3 No formal semantics for `undef` propagation
**Location:** Section 9.2
**Severity: Critical**

The spec says `undef` "propagates through dependent computations" and "may be contained/swallowed where downstream computation doesn't depend on the undefined value." This is far too vague for implementation. Consider:
- `if true then 5mm else undef` — is the result `5mm` or `undef`?
- `0 * undef` — is the result `0` or `undef`?
- `list.count` where the list is `undef` — is the result `undef`?
- `x and false` where `x` is `undef` — is the result `false`?
- `some(undef)` — is this `some(undef)` or `undef`?

Without precise rules, two implementations will disagree on fundamental programs.

### 2.4 Edge case: empty collections
**Location:** Section 3.4
**Severity: Significant**

- `[].sum` — what is the sum of an empty numeric list? Zero of what type?
- `List.generate(0, |i| ...)` — is this valid? Returns `[]`?
- `set{}` — empty set. What type?
- `forall x in []: predicate(x)` — vacuously true? The spec doesn't say.
- `exists x in []: predicate(x)` — vacuously false?

These are standard edge cases that a spec should address explicitly.

### 2.5 `Option<Option<T>>` semantics
**Location:** Section 3.4
**Severity: Significant**

The spec never addresses nested optionals. Is `Option<Option<T>>` a valid type? If so, how does pattern matching work? `some(none)` vs `none` — are these distinguishable? This matters for any generic code that wraps values in `Option`.

### 2.6 No error model for constraint violations during checking
**Location:** Sections 9, 10
**Severity: Significant**

The spec describes three modes (checking, solving, proposing) but never specifies what happens when checking finds a violation. Is it a compile error? An evaluation-graph failure? A warning? Is the design still valid in some degraded state? The distinction matters enormously for interactive workflows.

### 2.7 `forall`/`exists` interaction with `where` guards
**Location:** Section 5.4, Section 6.3
**Severity: Significant**

What happens with:
```
forall v in vents: v.spacing > 10mm
```
when `vents` is guarded by `where needs_cooling`? Is the quantifier vacuously true when `needs_cooling` is false? Is it a reference safety error? The interaction between quantifiers and guarded declarations is unspecified.

### 2.8 No specification of what happens with circular constraints
**Location:** Section 10
**Severity: Significant**

Two parameters that constrain each other circularly:
```
constraint a == b + 1mm
constraint b == a + 1mm
```
This is unsatisfiable, but the spec doesn't say whether this is detected statically, at solve time, or results in evaluation-graph failure. More subtle circular dependencies need treatment.

### 2.9 No specification for generic/parametric constraint definitions
**Location:** Section 4.1.3, Section 10.1
**Severity: Minor**

Constraint definitions like `Coincident` take explicit typed parameters. But can a constraint definition be generic over dimension or trait? The grammar allows `type_params` on entity declarations (including constraint), but there are no examples and no discussion.

### 2.10 `field` entity declaration vs. `Field<D,C>` type
**Location:** Sections 3.5, 4.1.4
**Severity: Minor**

`field` is both an entity kind (Section 4.1.4) and a type constructor (`Field<D,C>` in Section 3.5). The relationship is unclear. Is a `field def` producing a value of type `Field<D,C>`? Or is it a distinct entity with identity?

---

## 3. Clarity and Unambiguity

### 3.1 `let` for both value bindings and type aliases is ambiguous
**Location:** Section 4.7
**Severity: Significant**

```
let Pressure = Force / Area          // type alias
let volume = thickness * width * height  // value binding
```

The spec says "the compiler determines from context." But `Force / Area` looks syntactically identical to dividing two values. The grammar makes `type_expr` and `expr` overlap significantly. A spec must not rely on naming conventions for disambiguation. Consider a dedicated `type` keyword for type aliases.

### 3.2 Unclear: what is a "port type"?
**Location:** Section 4.7
**Severity: Significant**

Ports are declared as `port name : SomeType { ... }`. But what determines what a valid port type is? Must it implement the `Port` trait? Can any trait be a port type?

### 3.3 "Specialisation scope" is under-specified
**Location:** Section 8.7
**Severity: Significant**

What exactly is permitted inside a specialisation body? Can you add new `param` declarations? New `sub` declarations? New `port` declarations? The spec needs an explicit list.

### 3.4 Unclear semantics of `auto` for type parameters
**Location:** Section 3.9
**Severity: Significant**

The syntax `auto: Seal` suggests "pick a type that satisfies `Seal`." But what does "pick" mean? Is there a search space? An objective function? The spec needs to specify the resolution mechanism.

### 3.5 Unclear: what is a "purpose" parameter type?
**Location:** Section 4.4
**Severity: Minor**

The purpose example takes `(subject : Structure)`. But `Structure` is not defined as a type anywhere in the spec.

### 3.6 `@optimised` semantics unclear
**Location:** Section 10.1, Section 12.1
**Severity: Minor**

The spec says `@optimised` "registers that a language-level definition has a semantically equivalent optimised implementation." But it also says domain libraries can use `@optimised` to register "smarter defaults for specific parameter types." These are fundamentally different uses — one preserves semantics, the other changes solver behavior.

---

## 4. Quality Assessment

### 4.1 Strengths

**The determinacy model is the standout feature.** The `undef`/constrained/`auto`/determined spectrum is genuinely novel and well-motivated.

**Dimensional analysis as a type system feature** is well-designed. The 9-dimensional exponent vector, the treatment of Angle as a distinct dimension, and the handling of temperature offsets show deep domain understanding.

**The entity/trait/module design is sound.** The uniform entity declaration shape, the clear separation between entities and non-entities, and the trait composition rules are well-thought-out.

**The `where` guard system for conditional structure** is elegant and more principled than what most engineering tools offer.

### 4.2 Concerns

#### Complexity Budget
**43 keywords is high for a v0.1.** For comparison, Go has 25, Rust has ~39, Python has 35. Several keywords could be deferred:
- `chain` is sugar for `connect`
- `purpose` could initially be modeled as a `structure` with activation semantics
- `field` as a separate entity kind adds conceptual weight
- `implies` is rare; `not a or b` works

#### Footguns and Surprising Behaviors
1. **`undef` propagation is viral.** Engineers may not realize leaving one parameter unspecified invalidates half their design.
2. **`auto` strict vs. free is subtle.** An engineer writing `= auto` gets strict mode and may receive cryptic errors. The default should arguably be `free` for v0.1.
3. **Newline significance with trailing-operator continuation** is fragile for LLM generation.
4. **No space between number and unit** (`5mm` not `5 mm`) will be error-prone for LLM generation.

#### LLM Generation Reliability
Several features will challenge LLM generation:
- The `let` dual-use for types and values
- Position-sensitive `where` guard placement
- The `..` vs `..<` distinction
- Knowing when to use `Scalar<Q>` vs bare `Q` vs `Real`

#### Over-Design for v0.1
1. Full tensor algebra type system (most users need `Point3`, `Vector3`, maybe `Matrix3x3`)
2. 34 named dimension aliases in prelude
3. Complete GD&T system in std

#### Under-Design for v0.1
1. No error handling story at all (geometry failures, IO failures)
2. No iteration/mapping over sub-structures for aggregation
3. No versioning or compatibility story

---

## 5. Specific Recommendations

| # | Location | Issue | Severity | Recommendation |
|---|----------|-------|----------|----------------|
| 1 | S9.6/S3.2 | `Currency` names both a dimension and an eval-graph enum | Critical | Rename eval-graph enum to `NodeStatus` or `Freshness` |
| 2 | S11.6/S11.8 | `Elastic` trait defined twice with different parents | Critical | Rename one: `ElasticMaterial` vs `ElasticBody` |
| 3 | S4.2.1/S11.5 | "No overloading" contradicted by stdlib | Critical | Permit trait-dispatched overloading, or give distinct names |
| 4 | S3.1/S3.3.1 | `Real` vs `Scalar<Dimensionless>` undefined | Critical | State explicitly whether they are identical types |
| 5 | S5.1-5.2 | No type-indexed operator semantics | Critical | Add a complete result-type table |
| 6 | S9.2 | `undef` propagation too vague | Critical | Specify as partial evaluation with explicit short-circuit rules |
| 7 | S3.3.1 | `Scalar<Q>` has dangling `N` | Significant | Define independently or state `N` is universally quantified |
| 8 | S13 | `constraint_expr` undefined | Significant | Define it or replace with `expr` |
| 9 | S6.3/S13 | Per-declaration `where` not in grammar | Significant | Add `where_guard` to relevant productions |
| 10 | S6.4 | `match` desugars to same-name `sub` | Significant | Explicitly permit mutually-exclusive guarded same-name declarations |
| 11 | S3.4/S13 | Map literal `(k=>v)` ambiguous with parens | Significant | Use `map{k => v}` prefix syntax |
| 12 | S2.10/S11.4 | `in` keyword vs `in` unit | Significant | Document disambiguation; consider `inch` |
| 13 | S4.7 | `let` dual-use ambiguous | Significant | Use `type` keyword for type aliases |
| 14 | S3.4 | Empty collection semantics unspecified | Significant | Specify standard edge cases |
| 15 | S3.4 | `Option<Option<T>>` unspecified | Significant | Either forbid or specify fully |
| 16 | S8.7 | Specialisation scope under-specified | Significant | List permitted member kinds explicitly |
| 17 | S3.9 | `auto` type parameter resolution unspecified | Significant | Specify enumeration and selection mechanism |
| 18 | S4.7 | Port type requirements unstated | Significant | State whether `Port` trait is required |
| 19 | S9.6 | No v0.1 error handling for failures | Significant | Add `fallback(expr, default)` or `??` operator |
| 20 | S13.1 | Leading-operator continuation hazardous for LLMs | Minor | Allow leading `and`/`or` continuation |
| 21 | S2.10/S15 | Keyword count mismatch | Minor | Recount and reconcile |
| 22 | S3.3.1 | Full tensor system over-engineered for v0.1 | Minor | Consider deferring general Tensor/Matrix |
| 23 | S10.7 | `@optimised` conflates optimization with behavior change | Minor | Separate into `@optimised` and `@solver_hint` |
| 24 | S4.4 | `Structure` as purpose param type undefined | Minor | Define as built-in metatype or use trait bound |
| 25 | S1.3 | LL(1) goal not achieved | Nit | Acknowledge grammar is closer to LL(k) |
