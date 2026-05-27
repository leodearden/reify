# Decompose mode — moved (pointer stub)

The decompose-mode mechanics (filing a `planning_mode=True` batch via fused-memory, wiring all dependencies, then bulk-flipping `deferred` → `pending`) are project-agnostic and now live in the shared `/prd` skill:

> `~/.claude/skills/prd/references/decompose-mode.md` (→ `dark-factory/skills/prd/references/decompose-mode.md`)

Reify identity (`project_id="reify"`, `project_root="/home/leo/src/reify"`), the PRD path convention, the `grammar_confirmed` substrate flag, and the Reify memory namespace are in `../project.md`.

> Step 3 (the synchronous, curator-bypassing `planning_mode=True` path that returns `task_id` directly — no `resolve_ticket` round trip) is unchanged; see the generic `decompose-mode.md` Step 3. This is the pattern `.claude/skills/audit/references/severity-routing.md` refers to.
