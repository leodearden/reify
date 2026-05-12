# PRD: Shadowing Warning

> **2026-05-12 grammar-fiction sweep** (docs/architecture-audit/phase-3-grammar-fiction-triage-log.md):
> Suppression syntax respelled `#[allow(shadowing)]` → `@allow(shadowing)`
> to match the in-repo annotation framework (`@test`, `@optimized`,
> `@solver_hint`, `@shell`, `@solid` — `crates/reify-compiler/src/annotations.rs`).
> Rust-bracket form `#[...]` is not in Reify's grammar. Also dropped
> "type aliases" from the scoped collision targets — `TypeAlias` is
> only a top-level `Declaration` (`crates/reify-syntax/src/lib.rs:30`) and
> has no nested decl form, so module-scope shadow detection has no
> language position to fire from. No behavioural change.

> **2026-05-12 annotation-args PRD landed** (`docs/prds/annotation-args.md`):
> The `@allow(shadowing)` suppression spelling is no longer
> blocked on undesigned grammar/lowering — both already parse and
> lower today (`@allow` adds a schema entry to the annotation-args
> registry; the bare ident lowers to `AnnotationArg::Ident("shadowing")`).
> The annotation-args PRD's Phase 1 task γ is the joint integration
> gate: wires the shadow-lint walker (per this PRD's task 1) to
> consult `@allow` annotations and suppress W_SHADOW when
> `has_flag("shadowing")` returns true. See `docs/prds/annotation-args.md`
> §6 (consumer policy) + §8 task γ (LEAF observable signal).

## Goal

Emit a compile-time warning whenever an inner-scope declaration uses the same name as a declaration visible from a parent scope, per spec §8.5. The shadow is permitted (the inner declaration takes precedence in the inner scope); we just want it surfaced as a diagnostic.

## Background

- Spec §8.5 (line 1521-1523): "Warn, not forbid. When a declaration in a child scope uses the same name as a declaration visible from a parent scope, the compiler emits a warning. The shadowing is permitted -- the child's declaration takes precedence within the child scope."
- Distinct from §8.8 trait-merge collisions (same scope, different requirements) and §6.4 same-name guarded match decls (mutually exclusive guards, see `match-block-decls.md`).
- Distinct from imported-name conflicts (§8.11) — imports do not participate in upward visibility.

## Scope

- A single-pass scope analyzer that, when registering a name in a child scope, walks parent scopes and checks for collision against parameters, ports, sub-entities, and `let` bindings.
- New diagnostic code (e.g. `W_SHADOW`) with: shadowed name, shadowing-site span, original-declaration span.
- Apply to: structure / occurrence / constraint / field / trait / fn bodies, and nested specialization scopes.
- Lint-style: warning by default. Suppressible via `@allow(shadowing)`. The `@allow` annotation is shipped by `docs/prds/annotation-args.md` (§8 tasks α+β); this PRD's task 3 wires the consumer side (the shadow-lint walker reads `Annotation::has_flag("shadowing")` per `annotation-args.md` §3). Note: `#[allow(shadowing)]` Rust-bracket form is **not** Reify's annotation grammar.

## Out of scope

- Imported-name shadow against module-level declarations (§8.11 says imports do not participate in upward visibility — this is a separate diagnostic class if needed).
- Trait-merge same-name conflicts (§8.8) — already a hard error, not a shadow.
- Same-name guarded `match` decls (§6.4) — explicitly permitted, no warning.
- `self` references — `self` is a keyword, not subject to shadow checks.

## Acceptance criteria

1. Declaring `param x` in a sub-structure body when an enclosing structure already has `param x` emits warning W_SHADOW with both spans.
2. `let` shadowing a parent `param`, `port`, or another `let` warns.
3. Shadowing across more than one scope hop (grandparent) warns and points at the nearest visible parent declaration.
4. `match` block same-name guarded decls do NOT warn (the per-arm decls are siblings under mutually-exclusive guards, not shadowing each other).
5. Trait-merged members satisfying multiple traits via the same declaration do NOT warn.
6. Test coverage: positive (warns), negative (no false-positive on match blocks, trait merging, sibling scopes).

## Task breakdown

1. Implement single-pass scope-walk shadow detector in name-resolution / scope analyzer. Emits W_SHADOW with both spans.
2. Wire diagnostic code, span pairs, formatting; add to LSP diagnostics path.
3. Consult `@allow(shadowing)` to suppress W_SHADOW on annotated entities. Reads via `Annotation::has_flag("shadowing")` per `annotation-args.md` §3 / §6. This task is the joint integration-gate with `annotation-args.md` §8 task γ — its observable signal (a `.ri` file with `@allow(shadowing)` emits zero W_SHADOW) closes the loop for both PRDs.
4. Tests: positive shadow cases, match-block exception, sibling-scope no-warn, trait-merge no-warn, multi-hop shadow, `@allow(shadowing)` suppression.
