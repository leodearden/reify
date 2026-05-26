# Core / AST / IR Layering — split `reify-types` into a clean three-layer stack

**Status:** active · version-agnostic foundation · authored 2026-05-26
**Type:** refactor PRD (no new language surface). Resolves the stepping-stone introduced by task 3555 (annotation-args δ).
**Approach:** B + H (contract + boundary/DAG-assertion sketch). G5 fires hard — universal blast radius.

---

## 0. Forcing context & supersession

Task 3555 (annotation-args δ, landed on `main` in `55c8decd6c`) needed
`AnnotationArgValue::Expr(reify_syntax::Expr)` — a deferred, unevaluated parse-tree
fragment carried through the compiled IR for materialization-time annotation eval
(`docs/prds/annotation-args.md` §4, task ε). But `reify_syntax::Expr` lived **above**
`reify-types` (`reify-syntax → reify-types`), while the compiled IR that must embed it
(`AnnotationArgValue`, `CompiledFunction::annotations`) lives **in** `reify-types`.

To break the cycle, **Option B** was chosen: relocate the parsed `Expr`/`TypeExpr` AST
**down** into a new `reify_types::ast` module (re-exported from `reify-syntax` so
`reify_syntax::Expr` still resolves). It works, but it muddies `reify-types`, which now
holds, in one foundational leaf crate, three conceptually distinct things:

1. **core primitives** — `SourceSpan`, `ContentHash`, `Type`, dimensions, identity;
2. **the parsed AST** — `Expr`, `TypeExpr` (`reify_types::ast`);
3. **the compiled IR + runtime** — `CompiledExpr`, `CompiledFunction`, `Value`, geometry.

Option B was **explicitly chosen as a stepping stone** toward the split this PRD designs
(the breadcrumb lives in `crates/reify-types/src/ast.rs` lines 21–26). This PRD makes
"compiled IR embeds an unevaluated AST fragment" both **natural** (IR strictly above AST)
and **layering-pure** (the dependency edge `ir → ast` exists; the back-edge `ast → ir`
is mechanically forbidden), restoring the invariant that **meaning never depends on
surface syntax** while still permitting deferred-code patterns.

---

## 1. Goal, invariant, and consumer (G1)

**Goal.** Split the foundational `reify-types` crate into a strict three-layer stack:

```
reify-core  ←  reify-ast  ←  reify-ir
```

and narrow `reify-syntax` to a pure concrete-syntax parser that *produces* `reify-ast`.

**The invariant (the thing being made true and machine-checked):**

> Meaning never depends on surface syntax. Encoded as a dependency-edge direction:
> `reify-ir → reify-ast` is allowed (IR may embed an unevaluated parse fragment as
> pure data); `reify-ast → reify-ir` is **forbidden** and fails CI.

**Consumers (G1 — real today, not speculative):**

| Consumer | What it consumes | Status |
|---|---|---|
| `docs/prds/annotation-args.md` task ε (materialization-eval driver, a filed leaf) | `AnnotationArgValue::Expr(ast::Expr)` embedded in compiled IR, with IR strictly above AST | filed PRD; ε gates on this split for a clean layering |
| The layering **invariant itself**, via a machine check | `scripts/assert-crate-dag.sh` (+ `#[test]` wrapper) asserts the exact per-crate dependency sets | this PRD's integration-gate leaf (task η) |
| Every workspace crate (23 direct dependents of `reify-types`) | a decongested, legible foundational layer; faster incremental rebuilds (touching `ty` no longer rebuilds `value`'s 8.4k-line module's dependents) | continuous |

The deferred-code pattern is treated as a **first-class concern** (§6): the layering must
support not just annotation-`Expr` args but lazy fields, late-bound constraint
expressions, and eventual metaprogramming/quasiquote — *without* re-introducing a
syntax→meaning back-edge. Those are **anticipated** consumers; they are listed in §6 as
robustness requirements on the design, **not** as the G1 justification (which rests on the
filed annotation-args ε and the machine-checked invariant above).

**Grammar (G3):** trivially satisfied. This PRD introduces **no novel `.ri` syntax** — every
fragment in this document is Rust or `cargo`/shell. No `tree-sitter parse` fixtures apply.

---

## 2. Background — current state and blast radius

`reify-types` today: **26.5 k lines across 24 modules** (`value.rs` alone is 8.4 k,
`geometry.rs` 5.8 k, `expr.rs` 2.8 k). It is depended on, directly, by **23 of the 28
workspace members** — effectively everything except a handful of leaf binaries. It is the
single most-rebuilt crate in the workspace.

The conflation in `reify-syntax` is the same smell one size down: its `lib.rs` welds
together (a) the parser (`ts_parser.rs`, tree-sitter behaviour) and (b) the **declaration
AST data** (`ParsedModule`, `Declaration`, `StructureDef`, `OccurrenceDef`, `FnDef`,
`TraitDecl`, `EnumDecl`, `FieldDef`, `Pragma`, parsed `Annotation`, …). The expression AST
already sank into `reify_types::ast` (the stepping stone); the declaration AST did not.

This PRD's split applies the **same medicine** to both crates: *data crates hold data,
behaviour crates hold behaviour, and behaviour depends on data.*

---

## 3. Target architecture (CONTRACT — the crate DAG)

```
reify-core    meaning-free primitives: SourceSpan/Diagnostic, ContentHash, dimensions,
              Type, identity, source-location, spanned-ident, shared vocabulary tags.
   ↑          deps: NO reify-* crates (pure leaf).
   |
reify-ast     abstract syntax — "what was written", pure data: Expr/ExprKind,
              TypeExpr/TypeExprKind, MatchArm, LambdaParam, DimOp, QuantifierKind,
              parsed Annotation + Pragma, the declaration AST (ParsedModule, Declaration,
              StructureDef, OccurrenceDef, FnDef, TraitDecl, EnumDecl, FieldDef, ParseError).
   ↑          deps: { reify-core } ONLY. NO tree-sitter.
   |
reify-ir      meaning — compiled/typed/resolved IR + runtime model: CompiledExpr,
              CompiledFunction, Value, constraints, geometry, structure_registry, traits,
              node_traits, compiled Annotation (embeds ast::Expr), persistent, warm, sampled.
              deps: { reify-core, reify-ast }.
   ↖
reify-syntax  concrete syntax — the grammar + parser (tree-sitter CST → reify-ast).
              deps: { reify-core, reify-ast, tree-sitter, tree-sitter-reify }. PRODUCES ast.
```

**Allowed intra-workspace dependency sets (this is the contract the DAG-assertion enforces):**

| Crate | May depend on (reify-* only) | Must NOT depend on |
|---|---|---|
| `reify-core` | — (none) | any reify-* crate |
| `reify-ast` | `reify-core` | `reify-ir`, `reify-syntax`, `reify-types` |
| `reify-ir` | `reify-core`, `reify-ast` | `reify-syntax`, `reify-types` |
| `reify-syntax` | `reify-core`, `reify-ast` | `reify-ir`, `reify-types` |
| `reify-types` | — | **must not exist after task η** |

### 3.1 Naming rationale

- **`reify-core` / `reify-ast` / `reify-ir`** — adopted. "AST" = abstract syntax (data);
  "IR" = the post-parse compiled/semantic representation. `Value` is the IR's *runtime
  inhabitant* (the evaluator operates on it); geometry handles are IR-level realization
  references. The name names the meaning layer. Alternatives `reify-sem` / `reify-model`
  were rejected as less conventional / vaguer.
- **`reify-syntax` retained**, not renamed. After the split it means *exactly* one thing —
  the concrete-syntax parser — so the name is finally accurate ("syntax" = the grammar /
  parsing concern; the abstract syntax tree is `reify-ast`). A rename to `reify-parser`
  was considered and **deferred** (it would add 9 crates of churn for legibility only; see
  §11).
- **`reify-expr` is unchanged and is NOT the expression AST.** It is the expression
  **evaluator** (`interp`, `calculus`, `EvalContext` over `CompiledExpr` + `Value`); it
  consumes `reify-ir`. Naming note carried in §11 to forestall the `reify-ast`/`reify-expr`
  confusion.

---

## 4. Module → crate partition (CONTRACT — the symbol map)

Every current `reify-types` module, plus the `reify-syntax` declaration AST, with its
destination crate. This table **is** the symbol-map the cutover script (task η) consumes.

| Source module / type | Dest crate | Notes |
|---|---|---|
| `diagnostics` (`Diagnostic`, `DiagnosticCode`, `SourceSpan`, `Severity`, …) | **core** | |
| `hash` (`ContentHash`) | **core** | |
| `dimension` (`DimensionVector`, `Rational`, `NAMED_DIMENSIONS`) | **core** | |
| `ty` (`Type`) | **core** | deps `dimension` only |
| `identity` (`ModulePath`, `ValueCellId`, …) | **core** | |
| `source_location` | **core** | |
| `spanned_ident` (`SpannedIdent`) | **core** | |
| `PortDirection` (peeled out of `traits`) | **core** | §5 relocation #2 — meaning-free tag shared by ast decls + ir traits |
| annotation **name constants** (`TEST_ANNOTATION`, `OPTIMIZED_ANNOTATION`, …, peeled out of `annotation`) | **core** | §5 relocation #3 — shared vocab used by parser + compiler |
| `ast` (`Expr`, `ExprKind`, `TypeExpr`, `TypeExprKind`, `MatchArm`, `LambdaParam`, `DimOp`) | **ast** | already a module today |
| `QuantifierKind` (relocated from `expr`) | **ast** | §5 relocation #1 — the load-bearing back-edge |
| parsed `Annotation` + `has_test_annotation` (the **parser-produced** struct, `reify-syntax/lib.rs:871`) | **ast** | distinct from the compiled `Annotation` below |
| `Pragma`, `PragmaArg`, `PragmaValue` (`reify-syntax/lib.rs:831`) | **ast** | |
| declaration AST: `ParsedModule`, `Declaration`, `StructureDef`, `OccurrenceDef`, `FnDef`, `TraitDecl`, `EnumDecl`, `FieldDef`, `ImportDecl`, `ParseError`, … (`reify-syntax/lib.rs`) | **ast** | references **only** core+ast types (verified: no `EnumDef`/`TraitDef`/`Type`/`Value` refs) |
| `expr` (`CompiledExpr`, `CompiledExprKind`, `CompiledFunction`, `BinOp`, `UnOp`, `SelectorKind`, `ResolvedFunction`, TAG_* …) **minus** `QuantifierKind` | **ir** | embeds `QuantifierKind` from ast (ir→ast) |
| `value` (`Value`, `ValueMap`, `SampledField`, `EvalError`, `Freshness`, …) | **ir** | |
| `geometry` (`Mesh`, `GeometryHandle`, `GeometryKernel`, `KernelAttributeHook`, …) | **ir** | |
| `constraint` (`ConstraintSolver`, `OptimizedImpl`, …) | **ir** | |
| `structure_registry` (`StructureRegistry`, `StructureMeta`, `StructureTypeId`) | **ir** | |
| `traits` (`EnumDef`, `TraitDef`, `TraitMember`, `TraitRef`, `TraitBound`, `TypeParam`) **minus** `PortDirection` | **ir** | |
| `node_traits` (`NodeKind`, `NodeTraits`, …) | **ir** | |
| compiled `annotation` (`Annotation`, `AnnotationArg`, `AnnotationArgValue`) **minus** name consts | **ir** | `AnnotationArgValue::Expr(reify_ast::Expr)` — the ir→ast embed |
| `persistent` (`PersistentMap`) | **ir** | intra-ir cycle with `value` is fine (same crate) |
| `provenance` (`FieldImportProvenance`, `SnapshotProvenance`) | **ir** | placed with consumers (value/geometry) |
| `kernel_validation` (validation-message consts) | **ir** | geometry-adjacent |
| `boundary_attachment` (`BoundaryAssociation`, `NodeAttachment`) | **ir** | |
| `sampled` (grid/interpolation helpers) | **ir** | |
| `warm`, `warm_registry` (`OpaqueState`, `WarmStartable`, registry) | **ir** | `warm_registry → geometry` |

**Two `Annotation` types, deliberately.** `reify_ast::Annotation` (parser-produced,
pre-lowering) is the parsed form embedded in declarations. `reify_ir::annotation::Annotation`
(lowering-produced) is the compiled form whose `AnnotationArgValue::Expr` carries a deferred
`reify_ast::Expr`. The forward edge is `ir → ast`. The parser depends on ast, never on ir.

---

## 5. Cycle-breakers — three downward relocations (CONTRACT)

The partition is acyclic **after** three small misfiled types/consts move down. Each is a
behaviour-preserving, single-crate move done *before* any crate is created (Phase 0), so the
later crate extractions are mechanical.

1. **`QuantifierKind` (`expr.rs` → `ast`).** The only true `ast → ir` back-edge today:
   `ast.rs:28` does `use crate::QuantifierKind`, but `QuantifierKind` is defined in `expr.rs`
   (compiled IR). It is used by **both** the parsed `ExprKind::Quantifier` and the compiled
   `CompiledExprKind::Quantifier`. IR-above-AST ⇒ it lands in **ast**; the IR references down.
2. **`PortDirection` (`traits.rs` → core).** A trivial `In/Out/Bidi` enum that happens to
   live in `traits.rs` (which as a whole pulls `ty`+`value`, IR-tier). It is referenced by
   the declaration AST (port decls, ast-tier) and by `traits` (ir-tier). A meaning-free tag
   shared across tiers ⇒ **core**.
3. **Annotation name constants (`annotation.rs` → core).** `TEST_ANNOTATION`,
   `OPTIMIZED_ANNOTATION`, `SHELL_ANNOTATION`, etc. are canonical-spelling vocabulary used by
   the parser (ast-tier) *and* the compiler (ir-tier). Shared vocab ⇒ **core**; the compiled
   `Annotation` struct stays in ir.

> Tactical latitude: relocations #2 and #3 are correct in **any** tier at-or-below both their
> consumers; core is chosen for minimality. #1 must be ast (it is intrinsically a
> syntactic-form discriminator). The acyclicity claim holds for the placements above.

---

## 6. The deferred-code pattern as a first-class concern

The split's reason for being is to make "IR carries an unevaluated AST fragment" clean and
extensible. The design must support these consumers **without** an `ast → ir` back-edge:

| Consumer | Mechanism | Layering check |
|---|---|---|
| Annotation materialization-eval (annotation-args ε, **filed**) | `AnnotationArgValue::Expr(ast::Expr)` evaluated at structure-instance materialization | ir embeds ast::Expr — forward edge ✓ |
| Lazy / late-bound fields (anticipated) | a field whose value is an `ast::Expr` resolved on first demand | same forward edge ✓ |
| Late-bound constraint expressions (anticipated) | constraint RHS retained as `ast::Expr` until solve-time scope is known | same forward edge ✓ |
| Metaprogramming / quasiquote (anticipated, far) | construct/splice `ast` nodes programmatically inside IR; embed quoted `ast::StructureDef` etc. | **requires the full declaration AST in `reify-ast`** (option C) so a *declaration* fragment can be embedded as pure data without dragging the parser/tree-sitter — the reason C is chosen over A/B |

**Design rule, enforced by §8's assertion:** any IR construct MAY hold `reify_ast::*` data by
value; no `reify-ast` type may name a `reify-ir` type. "Deferred code" = AST-as-data living
inside meaning; never meaning leaking into syntax.

---

## 7. Migration strategy (phased; transient façades; one critical cutover sweep)

The narrow-file-lock orchestrator starves broad refactors
(`feedback_orchestrator_narrow_locks_favor_upfront_design`). Re-export façades are
acknowledged lock-contention pinch-points (a re-exporter is dragged into the blast radius
whenever a re-exported interface changes). Therefore:

- **Façades are used only *transiently*, during behaviour-preserving relocation, where they
  do genuine work** — decoupling *code-move* from *import-rename* so each move touches only
  the crate boundary being split, never the 23 downstream dependents. Interfaces are stable
  during this window (pure relocation), so the façade is not a live pinch-point.
- **No permanent façade survives.** Both the `reify-types` façade and `reify-syntax`'s
  transient AST re-export are removed in a **single critical-priority scripted cutover**
  (task η) that rewrites all downstream imports in one fast atomic sweep. One atomic
  everything-touch, run when the queue is quiet, is strictly less contentious than many
  overlapping narrow renames — and is the form Leo authorized ("a critical priority
  'run this script' task that forcibly pushes ahead of the narrower tasks; keep it simple
  and quick").

**Sequence (details in §10):**

- **Phase 0** — the three §5 relocations, in-place inside `reify-types`/`reify-syntax`,
  re-exports preserved. Downstream untouched.
- **Phases 1–3** — create `reify-core`, then `reify-ast`, then `reify-ir`, moving modules
  out of `reify-types` (and the declaration AST out of `reify-syntax` into `reify-ast`).
  `reify-types` re-exports everything transiently; `reify-syntax` transiently re-exports the
  AST types it moved. After Phase 3 the whole workspace still builds **through the façades**;
  no downstream import has changed yet.
- **Phase 4 (η)** — the single scripted cutover: rewrite every `reify_types::SYM` and every
  `reify_syntax::<AST type>` to its owning crate per §4, delete `reify-types`, delete
  `reify-syntax`'s AST re-export, and land the permanent DAG-assertion. Critical priority.
- **Phase 5** — companion prose corrections (annotation-args, CLAUDE.md, breadcrumbs).

Phases 0–3 do **not** touch downstream crates, so concurrent narrow tasks are not starved.
Only η briefly touches everything, by design.

---

## 8. Boundary / DAG-assertion sketch (H component — the machine consumer, G2 signal)

For a refactor, the "boundary test" is the **dependency-DAG assertion** plus per-crate
compile gates and a behaviour-preservation gate (full workspace test-green). The assertion
faces both sides of every seam: it constrains what the **producer** crates may depend on and
proves the **consumer** crates still build against the new layering.

Implemented as `scripts/assert-crate-dag.sh` parsing `cargo metadata --format-version 1`,
wrapped in a `#[test]` so `cargo test` covers it (exact host crate tactical — likely
`reify-build-utils`).

| # | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|
| B1 | core is a pure leaf | reify-core exists | reify-core has **zero** reify-* deps |
| B2 | ast sits on core only | reify-ast exists | reify-ast intra-workspace deps == `{reify-core}`; **no** tree-sitter dep |
| B3 | ir sits on core+ast | reify-ir exists | reify-ir intra-workspace deps ⊆ `{reify-core, reify-ast}` |
| B4 | parser sits on core+ast | reify-syntax narrowed | reify-syntax intra-workspace deps ⊆ `{reify-core, reify-ast}` |
| B5 | the forbidden back-edge | full workspace | reify-ast's manifest does **not** list reify-ir/reify-syntax/reify-types (the `ast → ir` ban) |
| B6 | façade is gone | post-η | `reify-types` absent from `cargo metadata`; no crate depends on it |
| B7 | the ir→ast embed compiles | post-Phase-3 | `AnnotationArgValue::Expr(reify_ast::Expr)` type-checks; a round-trip unit test constructs one and reads it back |
| B8 | behaviour preserved | every phase | full `cargo test` workspace-green; `cargo clippy` clean; GUI npm build unaffected |
| B9 | per-crate compile gate | each new crate | `cargo build -p reify-core && -p reify-ast && -p reify-ir && -p reify-syntax` each green in isolation |

B1–B6 are the layering invariant made executable; B7 proves the deferred-code pattern still
works through the new edge; B8/B9 are the behaviour-preservation contract. **Task η's
observable signal is "B1–B9 all green."**

---

## 9. Cross-PRD relationship (G4)

| Other PRD / artifact | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/annotation-args.md` (task ε) | annotation-args **consumes** the IR-above-AST layering | `AnnotationArgValue::Expr(reify_ast::Expr)` embedded in compiled IR; ε's eval driver reads it | **this PRD** owns the relocation; **annotation-args** owns the ε eval consumer | this PRD unblocks a clean ε; §10 task θ updates annotation-args prose (`reify_types::ast::Expr` → `reify_ast::Expr`) |
| **All active orchestrator work** | every in-flight task's worktree branches from `main` and builds on `reify-types` | the workspace dependency surface | this PRD (sequencing) | mitigated by §7 (Phases 0–3 don't touch downstream; η is one atomic critical sweep) |
| `CLAUDE.md` "Vendored sandbox" / crate-layout references | docs reference crate names | prose | this PRD | task θ |

No contested-ownership seam (the three known pairs in
`docs/architecture-audit/phase-3-breadcrumb-map.md` §3 are untouched). This PRD does not
introduce a fourth.

---

## 10. Decomposition plan

Greek labels; real IDs assigned at decompose time. Crate-touch counts drive the lock-window
analysis. Per-phase signals are concrete (cargo / DAG-assertion), per the refactor-G2
framing Leo authorized.

### Phase 0 — cycle-breaker relocations (in-place, behaviour-preserving)

- **α — Relocate `QuantifierKind` into `reify_types::ast`.**
  - *What:* Move the `QuantifierKind` enum from `expr.rs` to `ast.rs`; `expr.rs` imports it
    from `ast` (intra-crate). Re-export unchanged at crate root.
  - *Observable signal:* `cargo build -p reify-types` + `cargo test -p reify-types` green;
    `reify_types::ast::QuantifierKind` resolves; `grep` shows `expr.rs` no longer defines it.
  - *Crates:* reify-types. *Prereqs:* —.
- **β — Peel `PortDirection` and the annotation name-constants out of their IR-tier modules.**
  - *What:* Move `PortDirection` out of `traits.rs` and the `*_ANNOTATION` consts out of
    `annotation.rs` into a small primitives/consts module, re-exported at crate root so all
    paths (`reify_types::PortDirection`, `reify_types::TEST_ANNOTATION`) still resolve.
  - *Observable signal:* build+test green; `reify_types::PortDirection` / `TEST_ANNOTATION`
    still resolve; `traits.rs`/`annotation.rs` no longer define them.
  - *Crates:* reify-types. *Prereqs:* —. (parallel-safe with α)

### Phase 1 — `reify-core` (intermediate; unlocks δ, ζ)

- **γ — Extract `reify-core`.**
  - *What:* New crate `crates/reify-core`. Move the core modules + `PortDirection` +
    annotation consts (per §4). `reify-types` depends on `reify-core` and re-exports every
    moved symbol (transient façade). No downstream crate touched.
  - *Observable signal (DAG B1 + B8/B9):* `cargo build -p reify-core` green; `cargo metadata`
    shows reify-core has **zero** reify-* deps; full `cargo test` workspace-green via façade.
  - *Crates:* reify-core (new), reify-types. *Prereqs:* α, β.

### Phase 2 — `reify-ast` (intermediate; unlocks ζ, and η)

- **δ — Extract `reify-ast` (expression/type AST).**
  - *What:* New crate `crates/reify-ast`. Move `reify_types::ast` (Expr/TypeExpr/MatchArm/
    LambdaParam/DimOp/QuantifierKind). `reify-ast → reify-core` only. `reify-types`
    re-exports transiently.
  - *Observable signal (DAG B2 + B9):* `cargo build -p reify-ast` green; `cargo metadata`
    shows reify-ast deps == `{reify-core}`, no tree-sitter; workspace test-green.
  - *Crates:* reify-ast (new), reify-types. *Prereqs:* γ.
- **ε — Move parsed `Annotation`/`Pragma` + the declaration AST into `reify-ast`.**
  - *What:* Relocate the parsed `Annotation`/`has_test_annotation`, `Pragma`/`PragmaArg`/
    `PragmaValue`, and the declaration AST (`ParsedModule`, `Declaration`, `StructureDef`,
    `OccurrenceDef`, `FnDef`, `TraitDecl`, `EnumDecl`, `FieldDef`, `ImportDecl`, `ParseError`,
    …) from `reify-syntax/lib.rs` into `reify-ast`. `reify-syntax` depends on `reify-ast` and
    **transiently** re-exports these types so its 9 dependents still build. This is the large
    single-file move (`reify-syntax/lib.rs`, ~870 LOC); bounded to one task.
  - *Observable signal (DAG B2 holds + B9):* `cargo build -p reify-ast -p reify-syntax` green;
    reify-ast deps **still** == `{reify-core}` (declaration AST references only core+ast —
    verified); reify-syntax deps now include reify-ast; workspace test-green.
  - *Crates:* reify-ast, reify-syntax. *Prereqs:* δ.

### Phase 3 — `reify-ir` (intermediate; unlocks η)

- **ζ — Extract `reify-ir` (compiled IR + runtime).**
  - *What:* New crate `crates/reify-ir`. Move the IR/runtime modules per §4 (`expr` minus
    `QuantifierKind`, `value`, `geometry`, `constraint`, `structure_registry`, `traits` minus
    `PortDirection`, `node_traits`, compiled `annotation`, `persistent`, `provenance`,
    `kernel_validation`, `boundary_attachment`, `sampled`, `warm`, `warm_registry`).
    `reify-ir → {reify-core, reify-ast}`; `AnnotationArgValue::Expr(reify_ast::Expr)` realises
    the forward embed. `reify-types` becomes a pure transient re-export façade across all 3.
  - *Observable signal (DAG B3 + B7 + B8/B9):* `cargo build -p reify-ir` green; `cargo
    metadata` shows reify-ir deps ⊆ `{reify-core, reify-ast}`; B7 embed round-trip test
    passes; workspace test-green entirely through façades.
  - *Crates:* reify-ir (new), reify-types. *Prereqs:* γ, δ (ε not required for ir, but see η).

### Phase 4 — the cutover (LEAF / integration gate)

- **η — Scripted critical-priority cutover + permanent DAG assertion.**
  - *What:* One atomic sweep. (1) Add `reify-core`/`reify-ast`/`reify-ir` deps to each
    dependent's `Cargo.toml`; (2) rewrite every `reify_types::SYM` → its owning crate per the
    §4 symbol map, and every `reify_syntax::<AST type>` → `reify_ast::<…>`; (3) delete the
    `reify-types` crate and `reify-syntax`'s transient AST re-export; (4) commit
    `scripts/assert-crate-dag.sh` + its `#[test]` wrapper. Filed **critical** priority; kept
    deliberately simple/scripted so it lands fast and pushes ahead of narrow tasks.
  - *Observable signal (the integration gate — B1–B9 all green):* `scripts/assert-crate-dag.sh`
    exits 0; `reify-types` absent from `cargo metadata`; per-crate dep-sets match §3's
    contract table; full `cargo test` + `cargo clippy` workspace-green.
  - *Crates:* all (the sweep). *Prereqs:* γ, δ, ε, ζ.

### Phase 5 — companion corrections

- **θ — Prose & breadcrumb corrections.**
  - *What:* Update `docs/prds/annotation-args.md` (§4/§8 `reify_types::ast::Expr` →
    `reify_ast::Expr`; ε's "Crates touched" to reference `reify-ir`/`reify-ast`); remove the
    stepping-stone breadcrumb in the former `reify_types::ast` docstring; update any
    crate-layout references in `CLAUDE.md`.
  - *Observable signal:* `grep -r 'reify_types::ast' docs/ crates/` returns no stale hits;
    annotation-args ε description names the new layering. Prose-only.
  - *Crates:* docs. *Prereqs:* η.

### DAG

```
α ─┐
   ├─→ γ (reify-core) ─→ δ (reify-ast) ─→ ε (decl-AST into ast) ─┐
β ─┘                          └─────────→ ζ (reify-ir) ──────────┤
                                                                 ├─→ η (cutover, LEAF) ─→ θ (prose)
                                                                 │
              (γ, δ, ζ also feed η directly)  ───────────────────┘
```

---

## 11. Out of scope (with future-pointers)

- **Renaming `reify-syntax` → `reify-parser`.** Deferred; legibility-only, +9 crates of
  churn. The crate's *contents* are corrected here (parser only); the *name* can be tidied in
  a future low-priority PRD if desired.
- **Splitting `reify-ir` further** (e.g. peeling `geometry` 5.8 k / `value` 8.4 k into their
  own crates). The runtime/IR layer is large but internally cohesive; a second-order split is
  a separate future PRD once this stack is stable.
- **Changing any public API signature.** This is a pure relocation: every type keeps its
  shape; only its crate path changes. No behavioural change is in scope (B8 enforces this).
- **Migrating `reify-expr`'s name** (the evaluator). Untouched. (Naming note: `reify-expr` is
  the *evaluator*, not the expression AST — that is `reify-ast`. Kept distinct deliberately.)

---

## 12. Open questions (tactical — decide at implementation time)

1. **Host crate for the DAG-assertion `#[test]`.** `reify-build-utils` vs. a new tiny
   `reify-arch-test` crate vs. a workspace-root `tests/`. *Suggested:* `reify-build-utils`
   (already exists, no new member). Decide at task η.
2. **Cutover-script mechanics for glob/multi imports.** `use reify_types::{A, B, C}` where the
   members split across crates needs the symbol map (the §4 table) rather than naive sed.
   *Suggested:* generate the map from the three crates' `pub use` surfaces, then rewrite. Keep
   it a checked-in script under `scripts/`. Decide at task η; keep simple/quick per Leo.
3. **Final home of `ParseError`.** Travels with `ParsedModule` into `reify-ast` (it is
   embedded in the parsed module). *Suggested:* `reify-ast`; it references only `SourceSpan`
   (core). Confirm during ε.
4. **Serde feature plumbing.** `reify-types` has an optional `serde` feature; the three new
   crates must each re-expose it and the feature graph must stay consistent. *Suggested:*
   mirror the feature on each crate, wire `reify-ast`/`reify-ir` serde to depend on
   `reify-core/serde`. Decide per-crate at γ/δ/ζ.
