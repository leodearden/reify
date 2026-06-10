# Ambient default Material: scoped `default Material = …` declarations

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-06-10 · **Approach: B+H** (grammar producer + trait-conformance seam — both G5 triggers)

Companion to `docs/prds/v0_6/type-hygiene.md` (decision 9 there): this PRD owns replacing `body_mass_props`' default-water rung with an explicit, scoped, overridable default-Material mechanism, and the flip of "no density anywhere" from warn+water to **hard error with a hint**.

## 1. Goal

A designer doing exploratory work writes **one line** —

```reify
default Material = steel
```

— at the top of a file (or inside a `purpose`), and every structure in that scope subtree gets Material properties without per-structure ritual: `Physical.mass` derives, `body_mass_props` resolves density, `Rigid.moment_of_inertia` (post-4229) auto-derives. Remove the line and any density-needing path **fails loudly** with an error that names this mechanism — no silent or surprising defaults (the water default dies). Ratified direction (Leo, 2026-06-10): both "no surprising silent defaults ⇒ hard error" AND "low-ritual exploratory design ⇒ explicit scoped default".

## 2. Sketch of approach

**Type-keyed** (Leo 2026-06-10): `default <TypeName> = <expr>` fills **any** unfilled param whose declared type is `<TypeName>`, within the lexical scope subtree, innermost declaration wins. V1 restricts `<TypeName>` to `Material` (the machinery is written type-generically; widening to other types is a later PRD's call). Multi-Material-param structures: the ambient fills *all* unfilled Material-typed params.

Mechanism = **a scoped rung in trait-param default injection**, machinery the conformance checker already has (inject-if-absent, `conformance/checker.rs:1561-1844`; param-default registration `:501-554`). Resolution order per param: explicit conformer member > trait-declared default > **ambient default in scope** > (none → required-member error / density hard error downstream). Compile-time only — no eval-layer context threading (the instantiation-tree dynamic-scoping variant is named out of scope; it can layer on non-breakingly).

`Material(name: …, density: 7850kg/m^3, …)` ctor evaluation is verified working substrate (RigidPost.mass = 23.55 kg, probe-verified 2026-06-10) — the default's *value* side needs nothing new.

## 3. Resolved design decisions

1. **Type-keyed**, not member-path- or bare-name-keyed (Leo, AskUserQuestion 2026-06-10). One line covers `Physical.material` and any future Material-typed param; no trait-name coupling; no naming-convention coupling.
2. **Lexical/purpose scoping, v1:** file top-level and `purpose` blocks; applies to the scope subtree; innermost wins; two declarations for the same type in the same scope = compile error (ambiguity).
3. **Required params become ambient-satisfiable.** A required trait param of type Material counts as satisfied by an in-scope ambient default (it injects exactly like a trait default; per-structure explicit members still override). The injected member passes the type-hygiene conformance collision rule (η) like any other member.
4. **`body_mass_props` ladder rung swap:** explicit density arg > body Material.density > **ambient default Material's density** > **hard error** `E_DynamicsNoDensity` whose message names all three fixes (pass a density / give the body a Material / `default Material = …`). `W_DynamicsDefaultDensity` + water are deleted in the same change.
5. **Grammar is new** — gate-confirmed 2026-06-10: `default Material = steel` (top-level and inside `purpose`) fails `tree-sitter parse` today (fixtures `/tmp/prd-gate-fixtures/ambient-default-{1,2}.ri`). Task A is the grammar producer; everything downstream depends on it. `grammar_confirmed=false` on A; true elsewhere.

## 4. Out of scope (named)

- Ambient defaults for types other than Material (machinery type-generic; surface restricted v1).
- Instantiation-tree / assembly-level dynamic scoping (eval-time context threading).
- Module/import propagation beyond single-file + purpose nesting (revisit when the module system firms up).
- Any other implicit-default surface — type-hygiene PRD decision 9 stands: no NEW surface gets a built-in default.

## 5. Pre-conditions

- Task A (grammar production) — intra-batch producer; all other tasks depend on it.
- type-hygiene δ (shared Density acceptance in `resolve_body_density`) — task C builds on the cleaned ladder. Real dep edge at decompose.
- Conformance injection machinery — present (`checker.rs:1561-1844`), verified 2026-06-10.

## 6. Cross-PRD relationships

| Seam | Direction | Mechanism | Owner |
|---|---|---|---|
| type-hygiene PRD | consumes δ; produces the flip | `resolve_body_density` ladder; water rung | hygiene-δ keeps warn+water interim; THIS PRD's C deletes it (dep edge C → δ) |
| structural-traits-reconciliation δ=4229 | produces | ambient-filled `material` is an ordinary injected param, visible to trait lets (`mass`, post-4229 `moment_of_inertia`) | this PRD; no 4229 edge needed (param injection precedes let compilation, `checker.rs:501-554`) |
| type-hygiene η (collision rule) | consumes | injected ambient member type-checks like any member | η; no edge needed (η has no deps, lands independently) |
| geometric-relations / DSL grammar producers (e.g. 4384, 4395) | sibling pattern | `grammar_confirmed=false` producer-task convention | each PRD its own production; no shared files beyond grammar.js (locks serialize) |

## 7. Contract section (H)

**Declaration:** `default <TypeName> = <expr>` — statement position: file top-level or directly inside `purpose` body. `<expr>` compiles in the declaring scope; its type must `implicitly_converts_to` the named type, else error at the declaration (not at use sites).

**Resolution (per structure instantiation, compile time):** for each param of declared type T with no explicit member and no trait default: walk lexically outward from the structure's definition site; first `default T = e` found supplies the injected default (same code path as trait-default injection — kind Param, overridable). None found → existing required-member diagnostics (or, on density-consuming runtime paths, `E_DynamicsNoDensity` per decision 4).

**Invariants:** (i) injection never overrides an explicit member or a trait-declared default; (ii) two same-scope same-type declarations = compile error; (iii) removing a `default` declaration can only move code from "compiles/evaluates" to "loud error" — never to a different silent value (this is the property that kills water).

## 8. Decomposition plan

- **A — grammar production: `default` declaration** (tree-sitter rule + parser corpus test + AST/lowering wire to a typed `DefaultDecl` node; no semantics). `grammar_confirmed=false` producer. Signal: fixtures ambient-default-{1,2}.ri parse with 0 ERROR nodes (`tree-sitter parse --quiet` exit 0; committed to the tree-sitter corpus); compiler accepts-and-ignores with a "not yet wired" diagnostic.
- **B — scope resolution + injection rung** (compiler: DefaultDecl table per scope; conformance-checker rung per §7; same-scope-dup error; declaration-site type check). Deps: A. Signal: a structure omitting `material` inside a `default Material = steel` scope conforms to Physical and `mass` evaluates to the steel-density value; outside the scope, the existing missing-required-member error; duplicate declaration errors.
- **C — body_mass_props rung swap + water deletion + hard error.** Deps: B, **type-hygiene δ** (out-of-batch). Signal: `body_mass_props(b)` with no density anywhere → `E_DynamicsNoDensity` naming the three fixes (today: warn + water); with an in-scope `default Material = steel` → real steel-density mass props; `W_DynamicsDefaultDensity` no longer exists in the codebase.
- **D — integration gate (CRITICAL leaf).** One exploratory `.ri` in CI: N structures, zero per-structure materials, one `default Material = steel` line → Physical.mass + body_mass_props evaluate; the same file with the line removed → compile/eval errors naming the mechanism; purpose-nested override beats file-level. Deps: B, C. Signal: both directions pinned in a reify-eval e2e + the example runs in CI.

## 9. Boundary-test sketch

| # | Scenario | Pre | Post |
|---|---|---|---|
| 1 | parse forms | task A landed | top-level + purpose-nested `default Material = …` parse; corpus test green |
| 2 | injection fills required param | no explicit member, ambient in scope | conforms; `mass` = steel value; param still overridable per-structure |
| 3 | explicit member wins | structure declares material | ambient ignored |
| 4 | innermost wins | file-level steel, purpose-level aluminum | aluminum inside the purpose |
| 5 | duplicate same scope | two `default Material` at top level | compile error |
| 6 | wrong value type | `default Material = 5mm` | error at declaration site |
| 7 | no ambient, no material | density-needing path | `E_DynamicsNoDensity` w/ 3-fix hint; NOT water |
| 8 | water gone | grep | `W_DynamicsDefaultDensity` absent from codebase |

## 10. Open questions (tactical)

1. **Keyword vs contextual `default`** — check identifier-collision fallout in the grammar (existing `.ri` using `default` as a name). Decide in A.
2. **Diagnostic code naming** for the declaration-site errors. Decide in B.
3. **LSP surface** (hover on injected member showing the providing declaration) — nice-to-have; file follow-up at D if wanted.
