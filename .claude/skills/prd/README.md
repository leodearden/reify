# `.claude/skills/prd/` — Reify PRD overlay (NOT a skill)

This directory deliberately has **no `SKILL.md`**, so Claude Code skill discovery ignores it. The `/prd` skill itself is the **generic** skill at `~/.claude/skills/prd/` (a symlink to `dark-factory/skills/prd/`); it reads `project.md` here at invocation (its "Step 0") and applies Reify's specializations on top of the universal gates.

History: this used to be reify's full standalone `/prd` skill. On 2026-05-27 it was refactored — the project-agnostic gate machinery (G1–G6, META, author/decompose flow) moved to the shared `dark-factory/skills/prd/`, and everything Reify-specific stayed here as an overlay. The generalization makes `/prd` available in every project; Reify keeps its full prior behavior via this overlay.

Contents:
- `project.md` — the overlay: identity/paths, audit provenance, G1 engine-seam catalogue, G2 signal vocabulary, G3 grammar gate pointer, G4 contested pairs, G5 seams, G6 numerical-domain hazards, Stage-2 mechanism patterns, exemplars, memory namespace.
- `references/grammar-gate.md` — the G3 substrate verifier (tree-sitter mechanics). Reify-specific; lives here.
- `references/gates.md`, `references/decompose-mode.md` — **pointer stubs** kept so older in-repo docs that linked to these paths still resolve; they redirect to the generic skill + `project.md`.

To change Reify's PRD behavior, edit `project.md` / `references/grammar-gate.md` here. To change the universal gate machinery, edit `dark-factory/skills/prd/`.
