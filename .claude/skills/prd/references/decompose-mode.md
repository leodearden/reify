# Decompose mode — moved (pointer stub)

The decompose-mode mechanics (filing a `planning_mode=True` batch via fused-memory, wiring all dependencies, then bulk-flipping `deferred` → `pending`) are project-agnostic and now live in the shared `/prd` skill:

> `~/.claude/skills/prd/references/decompose-mode.md` (→ `dark-factory/skills/prd/references/decompose-mode.md`)

Reify identity (`project_id="reify"`, `project_root="/home/leo/src/reify"`), the PRD path convention, the `grammar_confirmed` substrate flag, and the Reify memory namespace are in `../project.md`.

> Step 3 (the synchronous, curator-bypassing `planning_mode=True` path that returns `task_id` directly — no `resolve_ticket` round trip) is unchanged; see the generic `decompose-mode.md` Step 3. This is the pattern `.claude/skills/audit/references/severity-routing.md` refers to.

## Step 3 extension — metadata.files guard (reify-specific, task 4677 β)

**Before each leaf's `submit_task`**, run the α guard predicate on the leaf's `metadata.files` list:

```bash
scripts/lock-charter-guard.sh check <file1> <file2> …
# or pipe newline-separated paths:
printf '%s\n' "${files[@]}" | scripts/lock-charter-guard.sh check
```

- **Exit 0** → the list is file-level or empty (`[]`); proceed to `submit_task`.
- **Exit 1** → a directory-shaped entry was found (`REJECT <path>` printed to stdout); **do NOT call `submit_task`** — rewrite the offending entry to a file anchor, or drop the whole list to `[]`, then re-run the guard until it exits 0.

This is the **primary enforcement site** (PRD decision 4, `docs/prds/task-lock-charter-lifecycle.md §5`). The `submit_task` / `commit_planning` backstop (γ, dark-factory) is the secondary catch.

### metadata.files authoring rule (OQ#5 — tight-or-empty, never a directory)

For each leaf's `metadata.files`, name a path **ONLY** when the task text gives a high-confidence file anchor — the PRD/task names the file explicitly, or there is exactly one obvious file for the change.

If you would name a directory (any path with no recognized code extension), or you are unsure which files, or the change is a broad refactor of unknown extent → file `[]` and defer to the architect (BRE acquires the real footprint before editing).

**NEVER put a directory in `metadata.files`.**

`[]` is a first-class value that subsumes the broad-refactor case — there is **no** refactor exception. Under-declaration is the safe error direction; over-declaration serializes dispatch.
