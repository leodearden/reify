# PRD: Annotation Args

Status: contract (resolves cluster C-06 grammar fictions for the annotation surface). Authored 2026-05-12 in interactive PRD session. Approved by Leo before queueing tasks.

Foundational, version-agnostic. Filed at top-level `docs/prds/` rather than under a milestone directory because multiple downstream PRDs across v0.2 (shadowing-warning), v0.3 (compute-node-contract / @optimized migration), and v0.5+ (varying-thickness-shells, future field-producer annotations) consume it.

## §0 — Purpose and resolution scope

This PRD is the **unified contract** for Reify's annotation argument surface — both forms flagged by the 2026-05-12 architecture-audit grammar-fiction triage (`docs/architecture-audit/phase-3-grammar-fiction-triage-log.md` O4):

- **Flag-form** — `@allow(shadowing)`. Bare identifier(s) in annotation arg position naming opt-in flags. Consumer: `docs/prds/shadowing-warning.md` (suppression site).
- **Runtime-evaluable named-arg form** — `@shell(thickness = linear_taper(z))`. Named arguments whose RHS is an expression evaluated at the annotated entity's materialization. Consumer: `docs/prds/v0_5/varying-thickness-shells.md` (per-vertex thickness field) and future field-producer annotations.

It also lands the `@optimized(target = "...")` named spelling alongside the existing positional `@optimized("...")` form so the consumer policy generalizes uniformly across annotations.

Resolves the annotation-args portion of `docs/architecture-audit/gap-register.md` GR-009 (cluster C-06). The non-annotation grammar fictions in that cluster are out of scope here; they're tracked by the per-PRD entries in `phase-3-grammar-fiction-triage-log.md`.

This PRD is approach **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline. The G5 heuristic fires on cross-crate blast radius (tree-sitter / syntax / compiler / types / eval / lsp), mechanism count (schema, named-arg grammar, Expr widening, eval-timing, consumer query API), cross-PRD consumers ≥ 2, and load-bearing grammar/parser surface. Contract sections §3–§6; boundary-test sketch §7.

## §1 — Grammar reality (pre-design grounding)

The 2026-05-12 audit triage flagged both forms as "grammar fictions" together. Grammar-gate parse runs against `tree-sitter-reify` `main` (commit at session time) refine the diagnosis:

| Surface | Parses today? | Lowers today? | Layer requiring work |
|---|---|---|---|
| `@allow(shadowing)` | ✅ | ✅ (`AnnotationArg::Ident`) | Consumer only (shadow-lint reads the arg) |
| `@allow(shadowing, unused_param)` | ✅ | ✅ (multiple `Ident` args) | Consumer only |
| `@optimized("string")` | ✅ | ✅ (`AnnotationArg::String`) | None (shipped) |
| `@shell(2.0)` | ✅ | ✅ (`AnnotationArg::Real`) | None (shipped) |
| `@shell(linear_taper(1.0))` | ✅ | ❌ (warning: "complex expression") | Lowering widen + eval hook |
| `@shell(2.0 * 1.5)` | ✅ | ❌ (same) | Lowering widen + eval hook |
| `@optimized(target = "...")` | ❌ | n/a | Grammar (named-arg) + lowering + schema |
| `@shell(thickness = linear_taper(z))` | ❌ | n/a | Grammar + lowering + eval + schema |
| `@solver_hint(method = "cg", max_iter = 1000)` | ❌ | n/a | Same |

The grammar's annotation production is `'@' name optional('(' commaSep($._expression) ')')` (`tree-sitter-reify/grammar.js:898-905`). Arbitrary expressions are already admitted in arg position; the lowering side (`crates/reify-compiler/src/annotations.rs:14-49`) rejects anything beyond literals/idents and a closed-enum `AnnotationArg` (`crates/reify-types/src/annotation.rs:77`).

**Consequence for design**: the audit's framing — "both forms are grammar fictions" — is inaccurate for flag-form. The true grammar fiction is **named-arg syntax** (`name = value` inside arg list). Flag-form is consumer-only work. The design carves into three orthogonal layers (grammar / lowering / consumer); they ship independently. The fixtures live under `/tmp/prd-gate-fixtures/annotation-args/` at session time; the production tests promote them to `tree-sitter-reify/tests/` per task ζ in §8.

## §2 — Resolved design decisions

Walking the thirteen Q-AA-N questions from `docs/architecture-audit/annotation-args-session-prompt.md`:

| Q | Decision |
|---|---|
| Q-AA-1 (positional vs named) | **Both, per annotation.** Each annotation's schema declares its accepted positional fallbacks; named always works (after grammar lands). Forward-compat — existing `@optimized("s")` sites unchanged. |
| Q-AA-2 (flag-form representation) | **Bare ident, positional-only.** `@allow(shadowing)` is `AnnotationArg{ name: None, value: Ident("shadowing") }`. Strict subset of the broader form — same source position parses identically pre- and post-named-arg grammar. |
| Q-AA-3 (eval timing) | **Materialization-time** for annotations on instance-shaped hosts (`structure def`, `occurrence`). **Compile-time const-fold** for annotations on decl-shaped hosts (`fn`, `constraint_def`, `param`, `let`) unless the schema explicitly opts the arg into eval-time. Per-annotation timing is declared in the schema (§4). |
| Q-AA-4 (scope capture) | **Instance scope after param binding** for materialization-time eval. Annotation expressions on a `structure def Foo { param z : Length ... }` see `z` (bound), surrounding `param`/`port`/`let` declarations, and the enclosing module's top-level names. No closure over the parent scope of the host's call site (annotations attach to the def, not to a particular invocation). |
| Q-AA-5 (type discipline) | **Per-annotation typed schema** (§4). The schema declares expected types per argument (positional and named); the validator enforces. Mismatches emit `E_ANNOTATION_ARG_TYPE` with the annotation site span. |
| Q-AA-6 (error semantics) | **Compile-time errors** for schema violations (unknown arg name, missing required, type mismatch, unknown flag, malformed). **Runtime diagnostics** when materialization-time eval fails — surfaced via `Diagnostic::AnnotationEvalFailed` attached to the host entity instance, failing materialization. Unknown annotation names continue to warn (existing behavior, `annotations.rs:207-212`). |
| Q-AA-7 (Value model alignment) | **Eval-time RHS produces a `Value`.** Post-GR-001 this may be `Value::StructureInstance`, `Value::Field<…>`, numeric, etc. Annotation framework treats RHS uniformly as `Value`-producing. Consumers crack open the resulting Value per their schema-declared type. |
| Q-AA-8 (unified dispatch grammar) | **One grammar production for annotation_arg** with two alternatives: bare expression (positional) OR `identifier '=' expression` (named). Grammar does not constrain which annotations accept which — that's the schema's job. |
| Q-AA-9 (existing call sites) | Survey: `@test`, `@deprecated("…")`, `@optimized("…")`, `@solver_hint(<existing args>)`, `@shell` / `@shell(num)`, `@solid`. All continue to parse and lower unchanged. Each gets a schema entry in §4's registry. |
| Q-AA-10 (tree-sitter production) | **Add a single `named_annotation_arg` alternative** alongside the existing expression alternative inside annotation's `commaSep`. Reuse existing expression non-terminals on the RHS — no separate "annotation_expression" sublanguage. Restrictions live in the schema, not the grammar. |
| Q-AA-11 (lowering target — IR shape) | `pub struct AnnotationArg { name: Option<String>, value: AnnotationArgValue }`. `name: None` = positional; `name: Some(_)` = named. `AnnotationArgValue` widens from today's closed enum to add `Expr(reify_syntax::Expr)` for unevaluated expressions. List preserves source order; lookup via `Annotation::arg("thickness")` consults named first, then falls back to positional via the schema's named→position map. |
| Q-AA-12 (first slice scope) | **Flag-form ships first**, end-to-end (§8 Phase 1). Positional-Expr lowering widening + named-arg grammar are separable later slices. Runtime-evaluable on a real consumer (varying-thickness shells) stays v0.5-deferred but is no longer fiction-flagged — its grammar/lowering pre-conditions are this PRD's slices, not an undefined "expand annotation-args". |
| Q-AA-13 (@optimized migration) | **Accept both positional and named.** Schema for `@optimized` declares positional[0] ≡ named `target`. Validator: if both `target = …` and positional[0] present, hard error (E_ANNOTATION_DUPLICATE_ARG). |

The materialization-vs-compile-time timing decision (Q-AA-3) is the only nominally novel framing — Reify already has structure-instance materialization as a graph event (post-GR-001's `Value::StructureInstance` runtime construction), so "eval at materialization" reuses that event. It does not require new lifecycle machinery; the annotation eval becomes one more action the engine performs when constructing a `StructureInstance`.

## §3 — Contract: the annotation arg IR

```rust
// crates/reify-types/src/annotation.rs (widened)
// NOTE (task 3555): the `Expr` variant carries `reify_types::ast::Expr`, the
// parsed AST relocated *into* reify-types — not `reify_syntax::Expr`. See the
// cycle-break note below §3's invariants.

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationArg {
    /// None = positional, Some = named.
    pub name: Option<String>,
    pub value: AnnotationArgValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationArgValue {
    String(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Ident(String),
    /// Unevaluated expression. Evaluation timing + result type per
    /// annotation schema (see ArgSchema in §4). Carries `reify_types::ast::Expr`
    /// (re-exported as `reify_syntax::Expr`) — see the cycle-break note below.
    Expr(reify_types::ast::Expr),
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<AnnotationArg>,        // source-order
    pub span: SourceSpan,
}

impl Annotation {
    /// Lookup by named arg, falling back to schema-declared positional
    /// fallback. Returns None if the annotation has no arg matching either.
    pub fn arg(&self, name: &str, schema: &AnnotationSchema) -> Option<&AnnotationArg> { /* ... */ }

    /// True iff a bare flag identifier `flag` appears among positional args
    /// (used by flag-form annotations like @allow).
    pub fn has_flag(&self, flag: &str) -> bool { /* ... */ }
}
```

**Cycle-break note (task 3555 / annotation-args δ).** The original draft placed
`Expr(reify_syntax::Expr)` in `reify-types`. That is an impossible crate cycle:
`reify-syntax` already depends on `reify-types`, so referencing `reify_syntax::Expr`
from reify-types would form `reify-types → reify-syntax → reify-types`. The first
remediation considered — relocating the compiled annotation types *up* into
reify-syntax — is also infeasible, because `reify_types::CompiledFunction` embeds
`Vec<Annotation>` (and is itself embedded across reify-types' `constraint.rs`), so the
compiled annotation IR must remain reachable from reify-types.

**Resolution (Option B).** The *parsed* expression AST (`Expr`, `ExprKind`, `TypeExpr`,
`TypeExprKind`, `MatchArm`, `LambdaParam`, `DimOp`) was relocated *down* into
`reify-types` as the `reify_types::ast` module and re-exported from `reify-syntax` (so
`reify_syntax::Expr` and every existing call site resolve unchanged). The annotation IR
stays in `reify-types::annotation`; `AnnotationArgValue::Expr` therefore carries
`reify_types::ast::Expr` with no cycle. The parsed `Expr` (not `CompiledExpr`) is the
correct representation: annotation exprs on instance hosts bind per-instance params
(e.g. `@shell(thickness = linear_taper(z))`) and must stay unresolved until
materialization (§4).

This is a deliberate stepping stone. reify-types now holds core primitives, the parsed
AST, *and* the compiled IR; the cleaner long-term layering is a `reify-core ← reify-ast
← reify-ir` split that re-homes the AST into a dedicated crate strictly below the IR. A
separate PRD tracks that transition.

**Invariants.**

1. The `args` `Vec` preserves source order. Roundtrip from parsed → lowered → diagnostic-formatted is byte-stable for positional args, and order-stable (insertion order) for named args.
2. `AnnotationArg::value` is `Expr(_)` **only** when the schema for this annotation declares the arg's expected-type with `eval_time = true` (see §4). Lowering for non-eval-time args const-folds and stores a literal-domain variant directly.
3. Named-arg names within a single annotation are unique. Duplicate named args (`@solver_hint(method = "cg", method = "gmres")`) emit `E_ANNOTATION_DUPLICATE_ARG` at lowering.
4. Positional args may not follow named args within a single annotation (`@optimized("s", method = "cg", "extra")` is a hard error — the trailing positional is malformed). Aligns with Python/Rust convention; minimizes parser ambiguity.

## §4 — Contract: per-annotation schema registry

```rust
// crates/reify-compiler/src/annotations/schema.rs (new)

pub struct AnnotationSchema {
    pub name: &'static str,
    pub valid_contexts: &'static [&'static str],   // "structure", "fn", ...
    pub args: Vec<ArgSchema>,
    /// Flag-form annotations: positional bare idents drawn from this set.
    /// None = not a flag-form annotation.
    pub flag_set: Option<&'static [&'static str]>,
    /// Diagnostic policy for unknown / extra args.
    pub on_extra: ExtraArgsPolicy,  // Error | WarnIgnore
}

pub struct ArgSchema {
    pub name: &'static str,
    pub positional_index: Option<u8>,  // Some(0..) if also accepted positionally
    pub required: bool,
    pub ty: ArgType,                   // String | Int | Real | Bool | Length | Field<X,Y> | Any
    pub eval_time: EvalTime,
}

pub enum EvalTime {
    /// Must reduce to a literal at lowering. Stored as a literal-domain
    /// AnnotationArgValue variant.
    CompileConst,
    /// Stored as AnnotationArgValue::Expr; evaluated at the annotated
    /// entity's materialization (structure-instance construction).
    /// Result is a Value matching the schema's declared type.
    AtMaterialization,
}

pub enum ExtraArgsPolicy { Error, WarnIgnore }
```

**Registry seeding.** A `static ANNOTATION_REGISTRY: Lazy<HashMap<&str, AnnotationSchema>>` populated at crate init lists every annotation the language knows. Phase 1 ships the registry with the existing annotations (`@test`, `@deprecated`, `@optimized`, `@solver_hint`, `@shell`, `@solid`) plus `@allow`. The schema for each absorbs the validation logic currently in `validate_annotations` (`crates/reify-compiler/src/annotations.rs:69-260`); behaviour is preserved bit-for-bit on existing call sites. Subsequent annotations are added by appending schema entries, not by extending a match arm.

**Unknown annotations** continue to warn (W_UNKNOWN_ANNOTATION) — existing behaviour preserved.

**Per-annotation Phase-1 schemas (illustrative; final form lives in the registry file).**

| Annotation | Args | Notes |
|---|---|---|
| `@test` | (none) | Bare marker. Context: structure / occurrence / fn / constraint_def. |
| `@deprecated` | `message?: String` (positional[0] ≡ named `message`) | Any context. |
| `@optimized` | `target: String` (positional[0] ≡ named `target`, required when context ∈ {constraint_def, fn}) | The §2 Q-AA-13 migration: named spelling allowed alongside positional. |
| `@solver_hint` | (existing args + schema) | Out-of-scope for this PRD to redesign; covered by its own future PRD. Registry entry mirrors today's match-arm. |
| `@shell` | `thickness?: Length \| Field<Point3, Length>` (positional[0] ≡ named `thickness`, `eval_time = AtMaterialization`) | Phase 1 keeps `Length` literal-only behaviour; Phase 4 (v0.5) opens `Field<…>` + AtMaterialization. |
| `@solid` | (none) | Bare marker; context: structure / occurrence. |
| `@allow` | flag-form: positional bare idents drawn from `flag_set = ["shadowing"]` (Phase 1; flag set grows as new lint suppressions are added) | New annotation in Phase 1. |

**Materialization-time eval pipeline.** For an annotation with at least one `AtMaterialization` arg attached to a structure-shaped host:

1. At lowering, args carrying `eval_time = AtMaterialization` lower to `AnnotationArgValue::Expr(_)` — stored unevaluated.
2. At structure-instance materialization (post-GR-001 `Value::StructureInstance` construction event), the eval driver iterates the host's annotations:
   - For each `Expr` arg, evaluate in the instance scope (params bound, lets / ports / sub-entities visible).
   - Type-check the resulting `Value` against the schema's declared type.
   - On success, the materialized annotation's args list holds the evaluated `Value` instead of the `Expr`. (This is a per-instance overlay; the `CompiledStructure`'s annotation arg list stays unchanged.)
   - On failure (eval error or type mismatch), emit `Diagnostic::AnnotationEvalFailed` and fail materialization for the instance.
3. Consumers query the materialized annotation via `instance.annotation("shell").and_then(|a| a.arg_value("thickness"))` and receive the evaluated Value.

This integrates cleanly with the ComputeNode contract's atomic completion (`docs/prds/v0_3/compute-node-contract.md` §3): annotation-eval is one of the steps performed inside the materialization critical section, so consumers never observe a half-evaluated annotation.

## §5 — Contract: the named-arg grammar production

```javascript
// tree-sitter-reify/grammar.js (delta)

annotation: $ => seq(
  '@',
  field('name', alias($.immediate_identifier, $.identifier)),
  optional(seq('(', commaSep($.annotation_arg), ')')),
),

annotation_arg: $ => choice(
  $._expression,
  $.named_annotation_arg,
),

named_annotation_arg: $ => seq(
  field('name', $.identifier),
  '=',
  field('value', $._expression),
),
```

**Production scope: annotation-only.** `named_annotation_arg` is reachable only inside `annotation`'s arg list, not in general expression position. This narrows the grammar delta per Q-AA-1 / Q-AA-8 / Q-AA-10: future general-kwarg work (fn calls, struct ctors) can land independently without coupling to this production.

**Positional-then-named ordering rule.** Enforced at lowering (§3 invariant 4), not by the grammar — the grammar admits any order to keep error recovery clean; lowering emits the rejection diagnostic on the first positional-after-named.

**Existing-site stability.** Every parsing-clean call site enumerated in §1's grammar-reality table continues to parse under the broader grammar. Fixtures in `tree-sitter-reify/tests/` pin this:

- `tests/annotation_existing_positional_unchanged.txt`
- `tests/annotation_named_arg_parses.txt`
- `tests/annotation_flag_form_unchanged.txt`
- `tests/annotation_mixed_positional_then_named.txt`
- `tests/annotation_named_then_positional_rejects_at_lowering.txt`

## §6 — Consumer policy and query surface

**Consumer rule.** Every annotation has a single canonical consumer (Rust-side) that owns reading + acting on its args. The schema's `name` field doubles as the consumer-discovery key. Consumers query via `Annotation::arg(name, schema)` (which handles named/positional fallback) or `Annotation::has_flag(name)` for flag-form.

**Mapping consumers (post-Phase-1):**

| Annotation | Consumer site |
|---|---|
| `@test` | test-discovery (`crates/reify-runner/src/test_discovery.rs`) |
| `@deprecated` | LSP diagnostics + compile pipeline |
| `@optimized` | `eval_user_function_call` + ComputeNode dispatch lowering (per `compute-node-contract.md` §4) |
| `@solver_hint` | constraint solver / minimize pipeline |
| `@shell` | T18 auto-classification (today literal-thickness); v0.5 varying-thickness shells (Field<Point3, Length>) |
| `@solid` | T18 auto-classification (force-tet) |
| `@allow` | shadow-lint (Phase 1 consumer); future lint-suppression callers |

**Multi-flag suppression policy.** `@allow(shadowing, unused_param)` is one annotation with two positional flag args. `@allow(shadowing) @allow(unused_param)` is two annotations, each with one flag. Both forms suppress; the single-annotation form is the canonical spelling. Lint consumers iterate all `@allow`s on the entity and union their flags.

**Forward-compat — unknown flags warn but don't fail.** A consumer that encounters `@allow(future_flag)` where `future_flag` isn't in `flag_set` emits W_UNKNOWN_FLAG (a non-fatal warning) so PRD-promised future suppressions can be drafted into example files ahead of their consumer landing. Aligns with the W_UNKNOWN_ANNOTATION precedent (`annotations.rs:207-212`).

## §7 — Boundary test sketch (cross-crate; facing both ways)

Producer side: tests in `crates/reify-compiler/src/annotations/tests.rs` + `tree-sitter-reify/tests/corpus/`. Consumer side: tests in the consumer crate (`reify-compiler/src/scope_check/`, `reify-runner/`, etc.). Both directions named.

### 7.1 Producer-side (parser + lowering + schema validation)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Flag-form round-trip.** Parse `@allow(shadowing) structure def Foo { param x : Real = 1 }`. | Annotation registry has `@allow` schema with `flag_set = ["shadowing"]`. | Lowering produces `Annotation { name: "allow", args: [AnnotationArg{name: None, value: Ident("shadowing")}], ... }`. `ann.has_flag("shadowing")` returns true. No diagnostics. |
| **Unknown flag warns.** `@allow(nonexistent_flag) ...`. | As above. | Lowering succeeds; `W_UNKNOWN_FLAG` warning emitted with annotation span; `ann.has_flag("nonexistent_flag")` returns true (consumer still sees the request, just unknown). |
| **Existing @optimized positional unchanged.** `@optimized("solver::elastic_static") fn solve(...) ...`. | Schema entry for `@optimized`. | Lowering produces named-equivalent: `ann.arg("target", schema)` returns `String("solver::elastic_static")`. Existing `CompiledFunction::optimized_target` consumer path unchanged. |
| **@optimized named spelling.** `@optimized(target = "solver::elastic_static") fn solve(...) ...`. | As above. | Identical lowered Value; identical consumer behaviour. Verified via direct equality between the two ann.arg() returns. |
| **@optimized both forms is duplicate.** `@optimized("s", target = "t") ...`. | Schema declares positional[0] ≡ `target`. | `E_ANNOTATION_DUPLICATE_ARG` at lowering. Lowering does not produce a half-formed Annotation; pipeline errors. |
| **Positional-then-named ordering.** `@solver_hint(method = "cg", "extra_positional") ...`. | Schema entry. | `E_ANNOTATION_POSITIONAL_AFTER_NAMED` at lowering. |
| **Type mismatch.** `@shell(thickness = "two_mm") structure def Plate { ... }`. | Phase-1 schema accepts `Length` literal positional[0]; named spelling lands in Phase 3. After Phase 3: schema `thickness: Length`. | `E_ANNOTATION_ARG_TYPE` at lowering, naming `thickness` and the expected `Length`. |
| **Grammar fixture suite.** `tree-sitter parse --quiet` over `tree-sitter-reify/tests/corpus/annotation_*.txt` exits 0 with no ERROR / MISSING nodes for the matrix of (positional, named, flag, mixed, edge-case) inputs. | Phase-3 grammar production landed. | All fixtures parse cleanly. |
| **Schema-registry replay parity.** For every annotation in the registry, the new schema-driven validator produces a byte-identical diagnostic set to the prior match-arm validator on every fixture in `crates/reify-compiler/tests/annotation_*.ri`. | Phase-1 schema absorbs current validate_annotations behavior. | Diff is empty across the existing fixture corpus. |

### 7.2 Consumer-side (downstream readers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Shadow-lint suppression.** A `.ri` file with `structure def Outer { param x : Real = 1; sub inner : Inner { ... } } @allow(shadowing) structure def Inner { param x : Real = 1 }`. | Phase-1 ships shadow-lint consumer reading `@allow`. | `reify check` emits Shadowing for Outer.x ↔ Inner.x by default; with `@allow(shadowing)` on Inner, the warning is suppressed. CLI exit code unchanged (warning suppression doesn't change pass/fail). |
| **Multi-flag aggregation.** `@allow(shadowing, future_flag) structure def Foo { ... }`. | Phase-1 + future Phase-N. | Shadow-lint sees `has_flag("shadowing") == true`; future lint sees `has_flag("future_flag") == true`. |
| **@optimized named spelling routes identically.** A stdlib `fn solve_elastic_static(...) -> ElasticResult @optimized(target = "solver::elastic_static")` evaluates via the ComputeNode trampoline registered for `"solver::elastic_static"` (per `compute-node-contract.md` §4). | Phase-3 lands @optimized named spelling; compute-node-contract.md Phase 6 (`η`) registers `solver::elastic_static`. | Result is identical to the positional spelling. ComputeNode inspection in the graph confirms dispatch. |
| **Materialization-time eval — scalar case.** Phase-2 + Phase-3 + Phase-4: `@shell(thickness = 2 mm) structure def Plate { param dummy : Real = 0 }` materializes; consumer reads `instance.annotation("shell").arg_value("thickness")` and gets `Value::Length(2mm)`. | Phase-4 schema accepts `Length` eval-time arg. | Result matches; type check passes; no eval error. |
| **Materialization-time eval — field case (v0.5).** `param z : Length = 100mm; @shell(thickness = linear_taper(0 mm, z)) structure def Wing { ... }` materializes; varying-thickness-shells consumer reads `instance.annotation("shell").arg_value("thickness")` and gets `Value::Field<Point3, Length>`. | Phase-4 schema accepts `Field<Point3, Length>` eval-time arg; varying-thickness-shells PRD activated; stdlib `linear_taper` field-producer fn exists. | Result is a `Value::Field`; kernel queries it at Gauss points; integrates with shell element stiffness assembly. |
| **Materialization eval failure.** `@shell(thickness = undefined_ident) structure def Bad { ... }` materializes. | Phase-4. | `Diagnostic::AnnotationEvalFailed` emitted; materialization fails for the instance; downstream consumers see `Freshness::Failed` on the host's output cells. No partial result observable. |
| **Forward-compat replay.** Every `.ri` file in `examples/` parses and evaluates identically after Phase 1, Phase 2, Phase 3, and Phase 4 landings. | Existing examples corpus. | No regressions; CI gate. |

## §8 — Decomposition DAG

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal**. Producer-only tasks closed in isolation are not acceptable (`feedback_task_chain_user_observable`).

Tasks are labelled with Greek letters; orchestrator-assigned IDs land at decompose-mode filing time.

### Phase 1 — Flag-form ships (consumer-only; grammar+lowering already done)

- **Task α — Annotation schema registry framework.**
  - **What:** Centralize the per-annotation validation logic in `crates/reify-compiler/src/annotations.rs:69-260` into a declarative `AnnotationSchema` registry per §4. No new annotations, no grammar change. Each existing annotation (`@test`, `@deprecated`, `@optimized`, `@solver_hint`, `@shell`, `@solid`) gets a schema entry mirroring its current match-arm semantics bit-for-bit. The match-arm validator becomes a thin dispatch over registry lookup.
  - **Observable signal:** `cargo test -p reify-compiler -- annotation_schema_registry_parity` passes — over every annotation fixture in `crates/reify-compiler/tests/fixtures/annotations*/`, the new schema-driven validator produces a byte-identical diagnostic set to the prior implementation. No `.ri` source change required.
  - **Crates touched:** `reify-compiler` (annotations.rs + new `annotations/schema.rs`).
  - **Prereqs:** None.

- **Task β — `@allow` schema entry + flag-form lowering plumbing.**
  - **What:** Add `@allow` to the registry with `flag_set = ["shadowing"]`. Wire `Annotation::has_flag` per §3. Wire the `W_UNKNOWN_FLAG` diagnostic when an unknown flag ident appears in an `@allow` arg.
  - **Observable signal:** A fixture `.ri` file with `@allow(shadowing) structure def Foo { param x : Real = 1 }` lowers without errors; the resulting Annotation IR contains a positional `AnnotationArg{name:None, value:Ident("shadowing")}` and `ann.has_flag("shadowing")` returns true. A second fixture with `@allow(future_flag)` emits W_UNKNOWN_FLAG via `reify check` and `ann.has_flag("future_flag")` still returns true. Both fixtures wired into the integration test suite.
  - **Crates touched:** `reify-compiler` (annotations/schema.rs registry), `reify-types` (Annotation::has_flag).
  - **Prereqs:** α.

- **Task γ — Shadow-lint reads `@allow(shadowing)` (LEAF; G2 user-observable signal).**
  - **What:** Wire the shadow-lint single-pass walker (per `docs/prds/shadowing-warning.md`) to consult `@allow` annotations on each entity it visits. When the entity has `@allow(shadowing)`, suppress Shadowing emission for that entity's shadowed names. Aggregation across multiple `@allow` annotations per §6.
  - **Observable signal:** A `.ri` file under `examples/m_allow_shadowing.ri` (new) containing a deliberately shadowing inner structure annotated with `@allow(shadowing)`, parsed with `reify check`, emits **zero** Shadowing diagnostics for that entity. A second `.ri` file with the same shadow shape but without the annotation emits exactly one Shadowing. Both verified via CLI text diff in `crates/reify-runner/tests/cli_allow_shadowing.rs`. Closes shadowing-warning.md acceptance criterion 6.
  - **Crates touched:** `reify-compiler` (scope-check / shadow-lint), `reify-runner` (CLI integration test), `examples/` (new fixture).
  - **Prereqs:** β. Also depends on shadowing-warning.md's shadow-detection task landing (sibling consumer slot); §8 task κ does the consumer-PRD prose tidy.

### Phase 2 — Positional-Expr lowering widening (foundation for v0.5)

- **Task δ — `AnnotationArgValue::Expr` variant.**
  - **What:** Widen `AnnotationArgValue` to include an `Expr(reify_syntax::Expr)` variant. Update `lower_annotations` to store any non-literal expression arg as `Expr(...)` instead of warning + dropping it (current `annotations.rs:38-46` behaviour). Schemas that don't declare `eval_time = AtMaterialization` for any arg still reject the Expr at validation (so existing annotations' behaviour is unchanged); only annotations whose schema accepts `eval_time = AtMaterialization` carry Expr through.
  - **Observable signal:** A fixture `.ri` file under `crates/reify-compiler/tests/fixtures/` containing `@shell(linear_taper(1.0)) structure def Plate { ... }` lowers — the resulting Annotation IR contains `args[0] = AnnotationArg{name:None, value:Expr(<call linear_taper(1.0)>)}`. Validation still emits the schema mismatch (because `@shell`'s Phase-1 schema doesn't accept Expr yet) — but the IR is preserved through lowering. Test pins both: the IR shape and the schema-mismatch diagnostic.
  - **Crates touched:** `reify-types` (AnnotationArgValue widen), `reify-compiler` (annotations.rs lowering).
  - **Prereqs:** α.

- **Task ε — Materialization-time eval driver (LEAF).**
  - **What:** Implement the eval driver per §4: for every `AnnotationArgValue::Expr` arg on an instance-shaped host, evaluate at structure-instance materialization (post-GR-001 hook). Result becomes a per-instance `materialized_args` overlay attached to the instance. Type-check evaluated Value against the schema's declared type. Emit `Diagnostic::AnnotationEvalFailed` on failure.
  - **Observable signal:** A test fixture stdlib annotation `@test_eval(value: Real, eval_time = AtMaterialization)` declared in the test harness registry. A `.ri` file `crates/reify-compiler/tests/fixtures/eval_annotation_smoke.ri` with `@test_eval(value = 2.0 * 1.5) structure def Foo { param dummy : Real = 0 }` materializes; the materialized instance's `annotation("test_eval").arg_value("value")` returns `Value::Real(3.0)`. A second fixture with `@test_eval(value = undefined_ident)` produces `AnnotationEvalFailed` and the instance materialization fails. Both verified via integration test.
  - **Crates touched:** `reify-eval` (materialization hook), `reify-types` (per-instance annotation overlay), `reify-compiler` (test fixture stdlib annotation).
  - **Prereqs:** δ. Plus GR-001 `Value::StructureInstance` runtime materialization hook (gates on `docs/prds/v0_3/structure-instance-runtime.md` landing).

### Phase 3 — Named-arg grammar (foundation for v0.5)

- **Task ζ — tree-sitter named_annotation_arg production + parser tests.**
  - **What:** Add `annotation_arg` and `named_annotation_arg` rules per §5. Update `annotation` to consume `commaSep($.annotation_arg)`. Add corpus tests under `tree-sitter-reify/tests/corpus/annotation_*.txt` covering the matrix in §7.1's grammar-fixture row. Rebuild the parser via `tree-sitter generate` (committed).
  - **Observable signal:** `tree-sitter parse --quiet tree-sitter-reify/tests/corpus/annotation_named.txt` exits 0 with no ERROR / MISSING nodes; corpus assertions cover positional, named, mixed, flag, and named-then-positional shapes. `cargo test -p reify-syntax -- annotation_named_arg_parses` passes.
  - **Crates touched:** `tree-sitter-reify` (grammar.js, regenerated parser.c, tests), `reify-syntax` (parser-test wiring).
  - **Prereqs:** None (grammar-only; parallelizable with Phase 1).

- **Task η — Lowering wires named-arg into AnnotationArg{name: Some(_), ...}.**
  - **What:** Update `lower_annotations` to recognize the named_annotation_arg parse node and populate `AnnotationArg.name = Some(<ident>)`. Enforce the duplicate-name and positional-after-named lowering invariants from §3. Wire `Annotation::arg(name, schema)` per §3.
  - **Observable signal:** A `.ri` fixture `@solver_hint(method = "cg") structure def Foo { ... }` lowers to `args = [AnnotationArg{name:Some("method"), value:String("cg")}]`. Duplicate-named-arg fixture (`method = "cg", method = "gmres"`) emits E_ANNOTATION_DUPLICATE_ARG. Positional-after-named fixture emits E_ANNOTATION_POSITIONAL_AFTER_NAMED. Schema-registry parity unaffected.
  - **Crates touched:** `reify-syntax` (Annotation AST possibly extended), `reify-compiler` (annotations.rs).
  - **Prereqs:** ζ, α.

- **Task θ — @optimized accepts named `target = "..."` (LEAF).**
  - **What:** Update `@optimized` schema entry: `target: String` at positional[0] ≡ named `target`. The `CompiledFunction::optimized_target` consumer reads via `ann.arg("target", schema)`, so both spellings resolve identically. Add an example `.ri` file demonstrating both spellings.
  - **Observable signal:** A `.ri` file `examples/m11b_optimized_named.ri` with `fn solve(x: Real) -> Real @optimized(target = "compute::identity") { x }` and a sibling site with `fn solve2(x: Real) -> Real @optimized("compute::identity") { x }` both lower to identical `CompiledFunction::optimized_target` values. CLI `reify check` emits no diagnostics. Re-running the existing `@optimized` corpus emits zero regressions.
  - **Crates touched:** `reify-compiler` (annotations/schema.rs), `examples/`.
  - **Prereqs:** η.

### Phase 4 — Runtime-evaluable @shell (v0.5-gated, ships in varying-thickness-shells DAG)

- **Task ι — `@shell(thickness = <expr>)` Field-typed schema arg (LEAF, gated).**
  - **What:** Extend `@shell` schema: `thickness?: Length | Field<Point3, Length>` at positional[0] ≡ named `thickness`, `eval_time = AtMaterialization`. The varying-thickness-shells consumer reads `instance.annotation("shell").arg_value("thickness")` and dispatches on the resulting Value variant.
  - **Observable signal:** A `.ri` file `examples/v0_5_wing_taper.ri` (placeholder; lands when v0.5 activates) with `param z : Length = 100mm; @shell(thickness = linear_taper(0 mm, z)) structure def Wing { ... }`, run through `reify check`, evaluates the thickness expression at materialization, producing `Value::Field<Point3, Length>`. The varying-thickness-shells kernel queries this Field at Gauss points. End-to-end FEA smoke test asserts tip-displacement matches the analytical linear-taper solution within tolerance.
  - **Crates touched:** `reify-compiler` (annotations/schema.rs), `reify-solver-elastic` or `reify-kernel-shells` (kernel-side Gauss-point sampling — owned by varying-thickness-shells PRD), `examples/`, `reify-stdlib` (linear_taper field-producer fn).
  - **Prereqs:** ε, θ. **Plus**: `docs/prds/v0_4/structural-analysis-shells.md` constant-thickness path shipped; `docs/prds/v0_5/varying-thickness-shells.md` activated; stdlib `linear_taper` field-producer fn (gated on `Field<X,Y>` in param position — GR-006 / task #3117); GR-001 resolved for any non-trivial stdlib field-producer.
  - **Disposition:** This task lives in varying-thickness-shells.md's DAG, not annotation-args' own DAG. Cross-referenced here for completeness; annotation-args itself does not block on it.

### Phase 5 — Companion correction tasks

- **Task κ — shadowing-warning.md prose tidy.**
  - **What:** Update `docs/prds/shadowing-warning.md` §Scope / §Acceptance to reference this PRD as the suppression-syntax foundation. Remove the dangling "once the suppression-annotation key is added to the annotation framework" hedge; replace with a hard reference to §8 task γ.
  - **Observable signal:** `docs/prds/shadowing-warning.md` updated; cross-reference added; no code change; doc-lint passes.
  - **Crates touched:** docs only.
  - **Prereqs:** None.

- **Task λ — varying-thickness-shells.md prose tidy.**
  - **What:** Update `docs/prds/v0_5/varying-thickness-shells.md`'s "Pre-conditions for activating" section. Remove the "Annotation-args expansion shipped — file a separate annotation-args PRD" hedge; replace with hard references to §8 tasks δ + ε + ζ + η + ι of this PRD. The PRD's user-specification surface `@shell(thickness = linear_taper(...))` can now be referenced as a designed feature pending v0.5 activation.
  - **Observable signal:** `docs/prds/v0_5/varying-thickness-shells.md` updated; cross-references added; the audit-Updates note at the top is amended (not removed) to reflect resolution; no code change.
  - **Crates touched:** docs only.
  - **Prereqs:** None.

- **Task μ — gap-register.md GR-009 cross-link.**
  - **What:** Update `docs/architecture-audit/gap-register.md` GR-009's Notes field to reference this PRD as the resolution path for the C-06 annotation-args items (`@shell(thickness = linear_taper(...))`, `#[allow(shadowing)]`). The shadowing-warning bracket-form respelling was already remediated by `phase-3-grammar-fiction-triage-log.md` A6; this PRD ships the actual surface. Other C-06 grammar fictions (`auto:`, `sub name : Type { body }`, decl-level `match`, etc.) are out of scope here.
  - **Observable signal:** `gap-register.md` GR-009 Notes amended; cross-link present.
  - **Crates touched:** docs only.
  - **Prereqs:** None.

### Dependency view

```
α ─┬─→ β ─→ γ (LEAF: shadowing-warning suppression)
   ├─→ δ ─→ ε (LEAF: materialization-time eval driver) ──┐
   │                                                      ├─→ ι (v0.5 LEAF, in varying-thickness DAG)
ζ ─┴─→ η ─→ θ (LEAF: @optimized named spelling) ─────────┘

κ, λ, μ (independent doc-edit companions)
```

GR-001 resolution gates task ε's full integration (materialization hook). Tasks α, β, γ, δ, ζ, η, θ are GR-001-independent.

### G5 confirmation

B+H confirmed by §7's boundary-test sketch facing both producer and consumer sides, plus per-Phase contract sections §3-§6. The integration-gate signal for each phase is its leaf task's user-observable signal (γ for Phase 1; ε for Phase 2; θ for Phase 3; ι for Phase 4). This closes the G2 loop into the integration-gate-task pattern from `preferences_implementation_chain_portfolio` approach C.

## §9 — Pre-conditions for activating

- **Phase 1 (flag-form):** None — grammar + lowering already present; activation immediate.
- **Phase 2 (positional-Expr lowering widening):** Phase 1 schema-registry land (α). Then δ is parallel-safe with ε.
- **Phase 2 task ε specifically:** Plus `docs/prds/v0_3/structure-instance-runtime.md` (GR-001 follow-up PRD) shipped — materialization-time eval needs the structure-instance materialization event.
- **Phase 3 (named-arg grammar):** None — grammar is parallelizable with Phase 1. Task η's lowering wires depend on Phase 1's schema (α).
- **Phase 4 (runtime-evaluable @shell):** v0.4 shells PRD shipped; v0.5 varying-thickness-shells activated; `Field<X,Y>` in param position (GR-006 / task #3117); stdlib `linear_taper` field-producer fn; GR-001 resolved.

## §10 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/shadowing-warning.md` | consumes (flag-form) | `@allow(shadowing)` lowering + `Annotation::has_flag` query | this PRD | shadowing-warning ships the shadow detector (separate task in its own DAG); this PRD's task γ is the joint integration-gate observable signal. Prose tidy via §8 task κ. |
| `docs/prds/v0_5/varying-thickness-shells.md` | consumes (runtime-evaluable form) | `@shell(thickness = ...)` named arg + materialization-time eval + `Value::Field<…>` interpretation | this PRD ships the framework (Phases 1-3); varying-thickness-shells owns the kernel-side consumer (Phase 4 task ι). | varying-thickness-shells PRD remains v0.5-deferred but no longer fiction-flagged. Prose tidy via §8 task λ. |
| `docs/prds/v0_3/structure-instance-runtime.md` (GR-001 follow-up) | produces (materialization event) | `Value::StructureInstance` construction site that eval driver hooks into | structure-instance-runtime | task ε gates on this PRD's materialization hook. Annotation framework treats RHS uniformly as `Value`-producing per §3 invariant + §4 schema. |
| `docs/prds/v0_3/compute-node-contract.md` | adjacent | `@optimized` schema entry (named spelling); lowering still routes through `CompiledFunction::optimized_target` to ComputeNode dispatch | compute-node-contract owns dispatch; this PRD owns @optimized arg lowering | task θ delivers the named spelling. Schema-driven validator confirms identical lowered IR. No reciprocal ambiguity. |
| `docs/prds/auto-type-param-resolution.md` and friends | unrelated | n/a | n/a | C-06 grammar fictions for non-annotation surfaces (e.g. `auto:` in type-arg list) are out of scope here; tracked by their own per-PRD grammar chains (`phase-3-grammar-fiction-triage-log.md` B1). |

No reciprocal-ownership ambiguity. Each cross-PRD seam has a single owner; the integration tasks live in the owning PRD's DAG; this PRD records the seam contract.

## §11 — Out of scope

- **General-purpose kwarg syntax** outside annotations (fn calls, struct ctors). The narrow named-arg production in §5 is annotation-only by design. If a future PRD adopts general kwargs, that PRD designs the broader surface; this PRD's annotation_arg production remains a forward-compatible subset.
- **User-declarable annotation defs.** The schema registry is Rust-side / hardcoded in Phase 1. Stdlib `.ri`-side `annotation_def Foo { ... }` declarations are a future surface — pulled out as a separate PRD when there's a real consumer driving the need.
- **`@solver_hint` arg redesign.** This PRD seeds `@solver_hint` with a schema entry mirroring its current match-arm semantics; any redesign of its arg surface lives in a separate solver-hint-payloads PRD (`docs/prds/solver-hint-payloads.md` already exists for this).
- **Migration tooling.** `@optimized("s")` → `@optimized(target = "s")` migration is opt-in; no automated rewrite. Existing sites stay positional.
- **Hot-reload of the schema registry.** Schemas are compile-time constants; runtime schema modification is not in scope.
- **Other C-06 grammar fictions** that aren't annotation-args (e.g. `auto:` in type-arg list, decl-level `match`, `sub name : Type { body }` body). Tracked by their own per-PRD chains.

## §12 — Open questions (tactical; explicit deferral)

1. **Diagnostic codes spelling.** `E_ANNOTATION_DUPLICATE_ARG`, `E_ANNOTATION_POSITIONAL_AFTER_NAMED`, `E_ANNOTATION_ARG_TYPE`, `W_UNKNOWN_FLAG`, `Diagnostic::AnnotationEvalFailed` — exact spellings + diagnostic-registry slot assignments lined up with the existing `crates/reify-diagnostics/` codebase at task α/β/η. Tactical: pick spellings consistent with neighbours; not architectural.

2. **`AnnotationArgValue::Expr` vs `AnnotationArgValue::Unevaluated(Arc<Expr>)`.** Arc'ing the Expr avoids deep clones in the per-instance `materialized_args` overlay. Tactical: profile at task δ; switch is local.

3. **Per-instance materialized_args storage location.** Live on `Value::StructureInstance.fields` as a reserved field name like `__annotations`? Side-table keyed by `(StructureTypeId, instance_id)`? Tactical: pick the lower-friction option at task ε; doesn't affect contract.

4. **Schema-declared `valid_contexts` policy when an annotation appears in an invalid context.** Today: warn but accept the annotation (consumer never sees it). Hard-error is a tighter discipline. Tactical: keep warn-and-accept for backward-compat; revisit if drift accrues.

5. **Are `@deprecated` args eval-time or compile-time?** Phase-1 keeps them compile-time string (existing behaviour). If a future deprecation reason wants `@deprecated(message = f"use {NewType} instead")` style interpolation, that's a separate Expr eval question. Tactical: leave compile-time const-fold; revisit on demand.

6. **`Annotation::arg(name, schema)` API ergonomics.** Requiring the schema at every call is verbose. Alternative: cache schema lookup on `Annotation` itself (`Annotation { name, args, schema: &'static AnnotationSchema, ... }`) so consumers call `ann.arg("name")` directly. Tactical: pick at task α; consumer surface migration is mechanical.

7. **Flag-form negation.** `@allow(!shadowing)` or `@deny(shadowing)` as the spelling for "explicitly enable a normally-suppressed warning"? Out of scope — no consumer drives the need yet.

8. **Lexical span granularity for named-args.** The named arg's span — full span or just the `name = expr` value span? Tactical: pick at task ζ for consistency with adjacent grammar productions; affects diagnostic-label placement but not behaviour.
