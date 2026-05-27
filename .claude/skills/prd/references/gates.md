# Gates — moved (pointer stub)

The project-agnostic gate definitions (G1–G6, META) now live in the shared `/prd` skill:

> `~/.claude/skills/prd/references/gates.md` (→ `dark-factory/skills/prd/references/gates.md`)

Reify-specific gate content lives in the overlay alongside this file:

- **G1 engine-integration sub-check** (the 7 in-engine seams from `docs/prds/v0_3/engine-integration-norm.md` §3, and the cluster-C-14 / GR-017 rationale) → see `../project.md` § "G1 — integration-seam catalogue + examples".
- **G2 signal vocabulary** (CLI / viewport-via-debug-MCP / LSP / stdlib `.ri` / `E_*`–`W_*`) → `../project.md` § "G2".
- **G3 grammar gate** → `grammar-gate.md` (this directory).
- **G4 contested-ownership pairs** → `../project.md` § "G4".
- **G5 load-bearing seams** → `../project.md` § "G5".
- **G6 numerical-domain hazards** → `../project.md` § "G6".

> Note for `docs/prds/v0_3/engine-integration-norm.md` (§12 task β): the engine-integration-norm § 5 G1 cross-reference is carried in `../project.md` § "G1", which catalogues the §3 seams and instructs that an in-engine mechanism's named consumer plug into one of them.
