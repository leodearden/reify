# Grammar gate (G3) mechanics

The grammar gate's job: for every Reify-syntax fragment in the PRD prose, confirm that `tree-sitter-reify` either parses it today **or** the PRD queues the grammar work as an explicit prerequisite task.

## Verification mechanism: try-parse-then-confirm

### Step 1 — Extract fixtures

Walk the PRD prose and identify every Reify-syntax fragment. Typical sources:
- Code-block examples (` ```reify ... ``` ` or unfenced indented blocks).
- Inline `code-span` snippets that look like declarations, calls, annotations, or expressions.
- Pseudo-code that's *almost* Reify but uses suggestive operators / keywords.

For each fragment, produce a `.ri` fixture — the minimum surrounding context that makes the fragment parseable in isolation. Heuristic templates:

| Fragment shape | Wrap as |
|---|---|
| Expression `foo(x).bar` | `let _x = foo(x).bar` inside a `structure def Test { ... }` |
| Declaration `param x : T = v` | inside `structure def Test : Rigid { ... }` |
| `@annotation(...)` on a fn | `@annotation(...) fn name() -> Unit { ... }` |
| Trait declaration | bare `trait def Name { ... }` |
| Structure | bare `structure def Name : SomeTrait { ... }` |
| `pragma #foo` | at file top-level |

Write fixtures to `/tmp/prd-gate-fixtures/<slug>-<n>.ri` or a directory the skill picks. Keep them around for the duration of the session in case Leo wants to re-run.

### Step 2 — Parse each fixture

**Preconditions** (both must hold — if either doesn't, `tree-sitter parse` fails with "No language found" / exit 1 regardless of whether the fixture's syntax is valid):

1. **CWD is `tree-sitter-reify/`** — the CLI reads `src/grammar.json` relative to CWD; no such file exists outside `tree-sitter-reify/`.
2. **The generated grammar files exist**: `src/parser.c`, `src/grammar.json`, `src/node-types.json`. They're gitignored (see `tree-sitter-reify/.gitignore`) and therefore absent in a fresh warm-lane or task worktree even when CWD is already correct (`build.rs` regenerates them into `src/` on a cargo build — only the staleness stamp goes to `OUT_DIR`). If missing, regenerate once first, from anywhere: `bash "$(git rev-parse --show-toplevel)/scripts/tree-sitter-generate.sh"`.

Then, from `tree-sitter-reify/`, parse the fixture:

```bash
cd /home/leo/src/reify/tree-sitter-reify
tree-sitter parse --quiet /tmp/prd-gate-fixtures/<slug>-<n>.ri
```

- **Exit 0** → fixture parses. Gate passes for that fragment.
- **Exit 1** → fixture fails. Gate fails for that fragment — but only once both preconditions above hold (correct CWD, generated grammar present).

**Before escalating any failure to Step 3:** if *every* fixture in the batch fails identically with "No language found", that is not a grammar fiction — it's a sign one of the two preconditions above isn't met (wrong CWD, or the generated grammar files are missing). Re-check both, re-run, and only carry fixtures that fail for a genuine parse reason forward to Step 3's resolution paths. Filing a prerequisite grammar task off an environmental failure wastes the PRD's decomposition on phantom work.

To inspect a failure, drop `--quiet` and look for `(ERROR ...)` nodes in the CST:

```bash
tree-sitter parse /tmp/prd-gate-fixtures/<slug>-<n>.ri 2>&1 | grep -E "ERROR|MISSING"
```

The ERROR-node line ranges tell you which token in the fixture confused the parser.

### Step 3 — Resolve failures

For every failing fixture, surface to Leo and propose two valid resolutions:

**(a) Rewrite the PRD prose to use existing grammar.** Look for an idiomatic Reify alternative that achieves the same intent. The `crates/reify-mcp/src/tools/chunks/*.md` files (which back the in-GUI assistant) are the authoritative reference for what does parse. Rewrite the PRD's example, then re-run the parse on the rewritten fragment.

**(b) Queue grammar work as an explicit prerequisite task.** Add a task to the PRD's decomposition plan:
- Title: "Grammar production: <feature> tree-sitter rule + parser test + lowering wire"
- Observable signal: "fixture `<path>` parses (`tree-sitter parse --quiet` exits 0); parser test in `tree-sitter-reify/tests/` asserts the new production; lowering wired in `reify-compiler/src/...`"
- Make every downstream task in the PRD `depends_on` this grammar task.
- Reference this task in the PRD's `Pre-conditions for activating` section.

Do **not** accept "the grammar will exist by the time this PRD activates" without a filed task tracking the work. That's the failure mode the gate exists to prevent.

### Step 4 — Ambiguous extraction → ask Leo

If fixture-wrapping is ambiguous (the fragment is too short, or could legitimately be inside multiple contexts, or contains pseudo-code that's clearly not literal Reify), ask Leo:

> "PRD includes `<fragment>`. I can't unambiguously wrap it as a `.ri` fixture. Is this literal Reify syntax, or pseudo-code for exposition? If literal, what's the surrounding context?"

If Leo confirms pseudo-code, mark the fragment as **not a grammar gate target** and move on. If literal, get the surrounding context and re-extract.

## What counts as "novel syntax"

The gate isn't about every Reify fragment in the PRD — only fragments that *might* not parse. Heuristics for "novel" (parse-test these):

- **Annotation forms not in `examples/`**: `@shell(thickness = linear_taper(...))`, `@optimized("target::name")` (this one does parse — verify), `@deterministic`, etc.
- **Operators / keywords not in `crates/reify-mcp/src/tools/chunks/syntax.md`**: `implies`, `subject to`, `chain`, `forall ... : <body>`, decl-level `match`.
- **Type forms with novel shape**: `Field<X,Y>` in param position (known TODO #3117 — does not parse in param context as of 2026-05-12), `auto: Nat` kind-bound, `Length(mm)` dimensioned literal in a type position.
- **Comprehension / list-builder syntax**: `sum(... for ... in ...)`, `[expr for x in xs]`, etc.
- **Bracketed forms**: `#[allow(shadowing)]` Rust-style attribute bracketing.
- **Structure / trait constructions**: `sub name : Type { body }`, `structure def Name : Trait1 + Trait2 { ... }`, `name = "..."` user-label form.

Fragments that are uncontroversially in the language (`param x : Length = 5mm`, `let y = box(10mm, 5mm, 2mm)`, `constraint foo > bar`) don't need parse-testing — these match `examples/*.ri` patterns. Use judgment.

## Examples from the 2026-05-12 audit

Documented grammar fictions (cluster C-06 in `docs/architecture-audit/phase-3-files-synthesis.md`):

| Fiction | Where seen | Resolution path |
|---|---|---|
| `auto:` in type_arg_list | auto-resolution-backtracking PRD | grammar work prereq |
| `sub name : Type { body }` | specialization-scope, shells PRD | grammar work prereq |
| decl-level `match` | match-block-decls PRD | grammar work in progress (task 2372) |
| `forall ... : <body>` | forall-statement-form PRD | grammar work prereq |
| `subject to` (in PRD prose) | structural-analysis-fea | rewrite — Reify uses constraint blocks, not `subject to` |
| `= auto` literal | auto-type-param-resolution | grammar work prereq |
| `chain` body | forall-statement-form | future PRD |
| `schema = { x: Length(mm) }` | auto-type-param-resolution | rewrite or grammar work |
| `Length(mm)` typed column | money-dimension, varying-thickness-shells | task #3115 |
| `@shell(thickness = linear_taper(...))` Expr annotation arg | varying-thickness-shells | grammar work prereq |
| `#[allow(shadowing)]` bracket form | shadowing-warning | rewrite to pragma `#allow(...)` form |
| `RegularGrid1` struct ctor | imported-field-source-hdf5-csv | gated on GR-001 |
| `name = "..."` user-label | persistent-naming-v2 | rewrite (v0.1 feature never landed) |
| `implies` operator | kleene-logic, spec §8.10 | grammar work prereq |
| `sum(... for ... in ...)` comprehension | money-dimension | grammar work prereq |

The skill should treat these as known-failing precedents; if a PRD's fragment matches one, expect the parse to fail and skip directly to the resolution conversation.

## When the gate is overkill

For PRDs that **introduce no novel syntax** (e.g. pure-infrastructure PRDs: a new evaluator module, a new test suite, a config-file consumer wired up), the gate is a no-op. Note "no novel syntax — G3 N/A" in the gate walk and move on.

## Performance note

`tree-sitter parse` is fast (sub-millisecond per fixture). Even a PRD with 20 fixtures parses in under a second. The gate has no perceptible session-time cost.
