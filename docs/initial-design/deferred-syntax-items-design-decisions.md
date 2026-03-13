# Deferred Syntax Items: Design Decisions

**Status:** Partial — covers `let` bindings, `where` guards, optional types, pattern matching, geometric primitive literals, frame projection, and pure functions. Port mapping syntax, multi-line continuation rules, and metadata blocks deferred to a subsequent session.  
**Version:** 0.1 — First crystallization from deferred syntax design sessions  
**Builds on:** `syntax-design-decisions.md` v0.1, `name-resolution-and-scoping-design-decisions.md` v0.1, `module-system-design-decisions.md` v0.1, `structural-graph-changes-design-decisions.md` v0.1

---

## 1. Design approach

The syntax design (v0.1) left several items explicitly deferred. The module system and name resolution phases introduced or assumed constructs (`let`, `pub let`, `require`) that had no formal syntax specification. This phase resolves the deferred items in dependency order, guided by the same principles as the original syntax design: regularity, concision, explicitness, readability, and parseability.

The central finding is that **one new declaration kind is needed (`fn` for pure functions) and one keyword is replaced (`derived` → `let`), but no new language-level concepts are introduced**. Optional types, pattern matching, geometric literals, and frame projection are all handled through existing mechanisms (the type system, syntactic sugar over `where` guards, library functions) without new core constructs.

---

## 2. `let` bindings

### 2.1 Replacing `derived`

The `derived` keyword from the original syntax design is replaced by `let`. These are the same concept — a named, computed value with a mandatory initialiser expression.

```
let volume = thickness * width * (width + height)
let mass : Mass = volume * material.density
pub let torque_constant : Torque/Current = back_emf / rated_speed
```

**Rationale:** `let` is concise, universally understood, and consistent with the language's Rust-influenced style family. In a declarative language with no mutation and no sequencing, `let` reads naturally as "let X be Y" without carrying the imperative connotations it has in procedural languages. The name resolution document already refers to these as "local bindings," aligning with `let` semantics.

### 2.2 Semantics

- Named, typed, computed value with a mandatory initialiser expression.
- Cannot be set from outside — not a configurable parameter.
- Private by default; `pub let` to expose across scope and module boundaries (module-system-design-decisions §4.4).
- Evaluated as a ValueCell in the evaluation graph. Early cutoff applies — if the computed value doesn't change, downstream dependents are unaffected.

### 2.3 Type annotation is optional

The `: Type` annotation is optional. When omitted, the type is inferred from the initialiser expression. When present, a mismatch between the annotation and the inferred type is a compile error.

```
let volume = thickness * width * height                    // Type inferred as Volume
let mass : Mass = volume * material.density                // Type annotated and checked
let oops : Force = volume * material.density               // Compile error: expression is Mass, not Force
```

**Rationale:** `let` values are always fully determined by their expression, so the type is always inferrable. The dimensional analysis system catches errors at use sites regardless. Mandatory annotations would impose a tax on every `let` binding to catch occasional errors that tooling (inferred-type display, mechanistic annotation insertion) catches more naturally. Explicit annotations remain valuable as documentation and as cross-checks — they are encouraged but not required.

**Contrast with `param`:** `param` requires a type annotation because parameters are the public interface of a definition — the type is part of the contract, and there may be no initialiser to infer from (`param thickness : Length`). `let` always has an initialiser, so inference is always available.

---

## 3. `where` guard syntax

### 3.1 Uniform semantics confirmed

The `where` keyword has uniform semantics across all entity types: it controls **structural presence**. When the guard is false, the entity does not exist in the evaluation graph. This applies to structures, occurrences, constraints, and fields identically. No distinct syntax is needed for different entity types.

**Post-name placement** (on declarations with bodies):
```
occurrence fan_mount : FanMount where needs_cooling { ... }
```

**Post-expression placement** (on bodyless declarations):
```
constraint vent_count >= 2 where needs_cooling
```

The rule is: `where` comes after the "what" (if any) and before the body (if any).

### 3.2 `where` blocks

A `where` block factors out a repeated guard, applying it to every declaration inside:

```
where needs_cooling {
    constraint vent_count >= 2
    occurrence fan_mount : FanMount { ... }
    occurrence vents : Vent[vent_count] { ... }
}
```

This is syntactic sugar. The compiler desugars to per-declaration guards:

```
constraint vent_count >= 2 where needs_cooling
occurrence fan_mount : FanMount where needs_cooling { ... }
occurrence vents : Vent[vent_count] where needs_cooling { ... }
```

### 3.3 Nesting

`where` blocks nest, with guards composing conjunctively:

```
where needs_cooling {
    occurrence fan_mount : FanMount { ... }

    where high_airflow {
        // Present only when needs_cooling AND high_airflow
        occurrence secondary_fan : Fan { ... }
    }
}
```

Desugars to:
```
occurrence fan_mount : FanMount where needs_cooling { ... }
occurrence secondary_fan : Fan where needs_cooling && high_airflow { ... }
```

### 3.4 Mixing with per-declaration guards

A declaration inside a `where` block may have its own `where` clause. The guards compose conjunctively:

```
where needs_cooling {
    occurrence fan_mount : FanMount { ... }
    occurrence backup_fan : Fan where redundancy_required { ... }
    // backup_fan present when needs_cooling AND redundancy_required
}
```

### 3.5 `where` blocks are not scopes

`where` blocks do not introduce a new lexical scope. Declarations inside are visible at the enclosing scope level, exactly as if they had been written with individual guards.

**Rationale:** Making `where` blocks scopes would break uniformity (every other `{ }` block is an entity scope with a name), and would create an access problem — a `where` block has no name to dot-notate into.

### 3.6 Reference safety rule

Referencing a guarded entity from an unguarded context is a compile error. A reference is valid only if the referencing declaration's guard **implies** the referenced entity's guard. The compiler checks this statically as a simple implication check on boolean guard expressions.

Valid references:
- Guarded → same-or-stronger-guarded: valid (the referencing context is always present when the referenced entity is)
- Guarded → unguarded: valid (unguarded entities are always present)

Invalid references:
- Unguarded → guarded: compile error (the referenced entity may be absent when the reference is evaluated)

```
where needs_cooling {
    occurrence fan_mount : FanMount { size = 40mm }
}

// Compile error: fan_mount is guarded by needs_cooling, but case_width is not
let case_width = fan_mount.width + 2 * wall_t

// Valid: same guard covers the reference
let case_width = fan_mount.width + 2 * wall_t where needs_cooling
```

---

## 4. Optional types

### 4.1 Two distinct mechanisms

Structural presence/absence and value-level optionality are orthogonal concepts handled by different mechanisms:

- **Structural presence/absence** — handled by `where` guards. The entity either exists in the evaluation graph or it doesn't. No `Option` type involvement.
- **Value-level optionality** — `Option<T>` with `some(value)` and `none` literals. The parameter always exists; its value is either present or absent.

### 4.2 `Option<T>` syntax

```
param coating : Option<CoatingSpec> = none
param annotation : Option<String> = some("default label")
```

Literals: `some(value)` and `none`. Unwrapping via `match` (§5).

### 4.3 Recursive termination

Recursive structures terminate via `where` guards, not `Option` types:

```
structure def TreeBracket {
    param depth : Int

    sub left : TreeBracket where depth > 0 {
        depth = self.depth - 1
    }
    sub right : TreeBracket where depth > 0 {
        depth = self.depth - 1
    }
}
```

When `depth == 0`, the children are structurally absent — they don't exist in the evaluation graph. The reference safety rule (§3.6) prevents unguarded references to these children.

**Rationale:** `where` guards already express structural presence/absence with full compiler support for reference safety. Wrapping recursive children in `Option` would add unwrapping ceremony for no additional safety benefit.

---

## 5. Pattern matching

### 5.1 `match` for value-level discrimination

`match` is included for exhaustiveness-checked branching on enum values and `Option<T>`:

**As a declaration block** (desugars to `where` guards with exhaustiveness checking):

```
match head_type {
    Hex => sub head : HexHead { ... }
    Socket => sub head : SocketHead { ... }
    Button => sub head : ButtonHead { ... }
    Flat => sub head : FlatHead { ... }
}
```

**As an expression** (for use in `let`, `param` defaults, constraint expressions):

```
let drive_size = match head_type {
    Hex => across_flats * 0.9
    Socket => socket_diameter
    Button => socket_diameter
    Flat => none
}
```

**`Option` unwrapping:**

```
let total = match coating {
    some(c) => base + c.thickness
    none => base
}
```

### 5.2 Syntax details

- **Exhaustiveness** is enforced — omitting a variant is a compile error.
- **Wildcard** `_` catches remaining cases: `_ => default_value`.
- **Multiple variants** with `|` for shared arms: `Socket | Button => recessed_drive`.
- **No fall-through.** Each arm is independent.

### 5.3 Declaration block desugaring

A `match` block containing declarations desugars to per-declaration `where` guards with mutual exclusivity tracked for exhaustiveness:

```
// Sugar:
match head_type {
    Hex => sub head : HexHead { ... }
    Socket => sub head : SocketHead { ... }
}

// Desugars to:
sub head : HexHead where head_type == HeadType.Hex { ... }
sub head : SocketHead where head_type == HeadType.Socket { ... }
// + compiler tracks exhaustiveness across all arms
```

### 5.4 No type-level overloading

Definition overloading by parameter type (multiple definitions of the same name disambiguated by parameter types) is not included. The trait system is the mechanism for type-level dispatch:

```
// Not supported:
field def Inside : Boolean { param scalar_field : ScalarField ... }
field def Inside : Boolean { param volume : BRepStructure ... }

// Use traits instead:
field def Inside<T: Insideable> : Boolean { param subject : T ... }
```

**Rationale:** Overloading interacts badly with the determinacy spectrum — when a parameter is `auto` or partially constrained, the compiler may not know which overload to select. Traits handle this cleanly through the existing type system machinery. If real use cases emerge where traits feel insufficient, overloading can be reconsidered.

---

## 6. Pure functions (`fn`)

### 6.1 The gap

Examples throughout the design documents assume function-call syntax (`distance(a, b)`, `union(a, b)`, `clamp(x, lo, hi)`, `von_mises(tensor)`) without any mechanism to define such functions. These are not entities — they have no identity, no determinacy state, no evaluation graph presence. They are pure computations.

### 6.2 Why not fields?

Fields are semantically different from pure functions. A field has a domain you sample — it maps from something with a spatial/continuous character to a codomain. `clamp(x, lo, hi)` is a trivial computation with no domain to sample, no interpolation, no resolution. Treating pure computations as fields would inflate the evaluation graph with nodes that gain nothing from field machinery (caching, warm-starting, tolerance management) while muddying the semantic meaning of "field."

### 6.3 `fn` definition syntax

A `fn` definition is a named, parameterised, pure computation:

```
fn von_mises(t : Tensor<2, 3, Pressure>) -> Scalar<Pressure> {
    let dx = t.xx - t.yy
    let dy = t.yy - t.zz
    let dz = t.zz - t.xx
    sqrt(0.5 * (dx^2 + dy^2 + dz^2))
}

fn clamp(x : Real, lo : Real, hi : Real) -> Real {
    if x < lo then lo else if x > hi then hi else x
}
```

### 6.4 Semantics

- **Pure** — no side effects, no state, no evaluation graph presence.
- **Block body** — `{ }` block containing `let` bindings and a final expression (the return value). No `return` keyword.
- **Type annotations mandatory** on parameters and return type. Unlike `let`, there is no enclosing context to infer from — the function signature is its contract.
- **Can be `pub`** for cross-module reuse.
- **Supports type parameters:** `fn distance<Q: Dimension>(a : Point3<Q>, b : Point3<Q>) -> Scalar<Q> { ... }`
- **`@optimised` hook available** for built-in fast paths (e.g., geometric kernel implementations of `distance`, `union`, etc.).
- **Not an entity type.** `fn` is a fifth kind of declaration alongside the four entity types (structure, occurrence, constraint, field). It is explicitly not an entity — it has no identity, no determinacy state, and no presence in the evaluation graph. This is an acknowledgment that computation exists alongside entities, not a weakening of the four-entity ontology.

### 6.5 `fn` bodies are lexical scopes

A `fn` body's `{ }` block is a lexical scope for name resolution — `let` bindings inside are local to the function. However, it is not an entity scope — there is no `self`, no determinacy tracking, no evaluation graph node.

---

## 7. Geometric primitive literals

### 7.1 Library functions, not special syntax

Geometric primitive constructors are ordinary library-defined functions, not dedicated syntax. The standard library provides:

```
point3(x, y, z)      // → Point3<inferred dimension>
vec3(x, y, z)        // → Vector3<inferred dimension>
point2(x, y)         // → Point2<inferred dimension>
vec2(x, y)           // → Vector2<inferred dimension>
```

The dimensional analysis system infers the quantity type from the arguments: `point3(1mm, 2mm, 3mm)` is `Point3<Length>`, `point3(1N, 2N, 3N)` is `Point3<Force>`.

### 7.2 Orientation constructors

`Orientation` has multiple representations. Named-argument construction disambiguates:

```
Orientation.from_axis_angle(axis = vec3(0, 0, 1), angle = 45deg)
Orientation.from_quaternion(w = 1.0, x = 0.0, y = 0.0, z = 0.0)
Orientation.from_euler(convention = ZYX, a = 0deg, b = 0deg, c = 90deg)
Orientation.from_basis(x_axis = vec3(1, 0, 0), y_axis = vec3(0, 1, 0), z_axis = vec3(0, 0, 1))
Orientation.identity
```

### 7.3 Composite types

`Frame` and `Transform` use standard instantiation syntax:

```
let mount_frame = Frame3 {
    origin = point3(0mm, 0mm, 50mm)
    basis = Orientation.from_axis_angle(axis = vec3(0, 0, 1), angle = 90deg)
}
```

### 7.4 Rationale

No new grammar rules are needed. The `fn` mechanism (§6) provides the definition infrastructure. Short function names (`point3`, `vec3`) provide concision. If usage patterns later reveal that geometric literals are frequent enough to warrant dedicated syntax, sugar can be added without breaking existing code.

---

## 8. Frame projection

### 8.1 Library function, not operator

Frame projection — expressing a geometric value in a different coordinate frame — is provided as a standard library function:

```
let tip_in_housing = project(motor.shaft.tip, to = housing.frame)
```

No dedicated operator syntax. `project` is a function (or `@optimised` built-in) that computes the transform chain through the containment tree from the source frame to the target frame.

### 8.2 Geometric values carry their frame

Geometric values (`Point3`, `Vector3`, etc.) carry their coordinate frame as part of their runtime representation. This is implicit — the frame is not part of the type, but is tracked by the runtime.

When a geometric value is defined within a structure, its frame is that structure's local coordinate frame. When extracted and passed elsewhere, the frame context travels with it. `project` reads the source frame from the value and computes the transform chain to the target frame.

**Rationale:** This is consistent with geometric values as opaque handles (geometry-engine-design-decisions §3.3). It eliminates a class of errors (specifying the wrong source frame) and matches how most CAD systems work internally.

### 8.3 Collections share frames efficiently

Geometric collections (point clouds, meshes) store coordinates relative to a single frame — the containing structure's local frame. Individual coordinate tuples within the collection do not carry independent frame references. The frame context attaches to the collection as a whole.

When an individual point is extracted from a collection for standalone use, it acquires the collection's frame as its own frame context at that point.

**Rationale:** This avoids per-element overhead. A `PointCloud` with 10,000 points has one frame reference, not 10,000. The data model is "a frame plus a collection of coordinate tuples," not "a collection of points each carrying independent frames." This mirrors how meshes work — vertex positions are defined in a coordinate system, not independently located.

---

## 9. Syntax updates to prior documents

### 9.1 Grammar changes

The semi-formal grammar from `syntax-design-decisions.md` §11 is updated:

```
member          ::= param_decl | port_decl | sub_decl | let_decl
                  | constraint_line | connect_stmt | chain_stmt
                  | entity_decl | field_body | fn_decl
                  | where_block | match_block

let_decl        ::= 'pub'? 'let' IDENT (':' type_expr)? '=' expr

fn_decl         ::= 'pub'? 'fn' IDENT type_params? '(' fn_params ')' '->' type_expr
                     '{' (let_decl)* expr '}'
fn_params       ::= fn_param (',' fn_param)*
fn_param        ::= IDENT ':' type_expr

where_block     ::= 'where' expr '{' member* '}'

match_block     ::= 'match' expr '{' match_arm* '}'
match_arm       ::= pattern '=>' (member | expr)
pattern         ::= IDENT ('|' IDENT)* | 'some' '(' IDENT ')' | 'none' | '_'

derived_decl    ::= (removed — replaced by let_decl)
```

### 9.2 Keyword changes

- `derived` is removed from the keyword set.
- `let` is added (was not previously a keyword).
- `fn` is added.
- `match` is added.
- `some` and `none` are added as value literals.

---

## 10. Open questions for subsequent sessions

### 10.1 Port mapping syntax

How connections with non-default port mapping work — when `chain` is insufficient and explicit `connect` with port mapping is needed. Deferred to next session.

### 10.2 Multi-line continuation rules

Line-continuation rules for long constraint expressions, field lambdas, connection chains, and `fn` bodies. Deferred to next session.

### 10.3 Metadata blocks

String-keyed metadata for unstructured engineering information (descriptions, revision notes, vendor part numbers). Deferred to next session.

### 10.4 `require` vs `constraint`

The module system document uses `require` inside purpose definitions. Whether this is a distinct keyword or an alias for `constraint` needs resolution. Deferred to next session.

---

## 11. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| `derived` keyword | Replaced by `let` | Concise, familiar, consistent with Rust-influenced style |
| `let` type annotation | Optional | Always inferrable from initialiser; tooling provides display and cross-checking |
| `where` guard syntax | Single keyword, uniform semantics | Structural presence is the same concept across all entity types |
| `where` blocks | Syntactic sugar desugaring to per-declaration guards | Factors out repeated guards; no new scope; no new semantics |
| `where` block scoping | Not a lexical scope | Uniformity (only entity bodies are scopes); no name to dot-notate into |
| Reference safety | Guarded entity referenced from unguarded context is compile error | Static implication check on guard expressions; prevents absent-entity access |
| Structural presence | `where` guards | Not `Option` — structural absence is distinct from value absence |
| Value-level optionality | `Option<T>` with `some(value)` / `none` | Orthogonal to structural presence |
| Recursive termination | `where` guards on recursive children | No `Option` wrapping needed; reference safety rule prevents access when absent |
| Pattern matching | `match` on values with exhaustiveness checking | Catches missing enum variants; desugars to `where` guards for declaration blocks |
| Type-level overloading | Not included | Interacts badly with determinacy spectrum; traits handle type dispatch |
| Pure functions | `fn` — a fifth declaration kind, not an entity type | Computation is not an entity; no identity, no determinacy, no graph presence |
| `fn` body syntax | Block body only (`{ }`) for v0.1 | Single form; expression-body sugar (`= expr`) deferred until needed |
| Geometric literals | Library functions (`point3`, `vec3`, etc.) | No new grammar; `fn` mechanism provides definition; dimensional inference handles types |
| Frame projection | `project(value, to = frame)` library function | No new operator; function is honest about the computation involved |
| Geometric frame context | Values carry frame implicitly | Consistent with opaque handles; eliminates wrong-frame errors |
| Collection frame sharing | One frame per collection, not per element | Efficient; matches mesh/point-cloud conventions |

---

*Document generated from deferred syntax design sessions. Covers items 1–7 of the deferred syntax agenda. Port mapping syntax, multi-line continuation rules, and metadata blocks to be resolved in subsequent sessions.*
