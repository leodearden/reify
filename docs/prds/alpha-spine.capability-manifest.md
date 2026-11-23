# Capability manifest — alpha-spine

PRD: `docs/prds/alpha-spine.md`. Evidence verified against main `f8e47e232c` (2026-11-02). Bindings are pattern-anchored (symbol, file, section), never `file:line`. The YAML twin `alpha-spine.capability-manifest.yaml` carries the mechanical `delivered_check`s; both are kept in sync by hand after landing (overlay: "Correcting a landed check is THREE writes").

Every mechanical check below was confirmed ABSENT on main at the pin (polarity: a sound check fails when written, passes when its producer lands).

## α1 — Issue #1, tree-sitter ABI pin
- `tree-sitter generate --abi 14` exists → PASS: `tree-sitter 0.26.8 generate --help` lists `--abi <VERSION>` with env `TREE_SITTER_ABI_VERSION`; workspace `Cargo.toml` pins `tree-sitter = "0.24"` (ABI ≤ 14). Rejection observed by the alpha user: `LanguageError { version: 15 }` (issue #1).
- `scripts/tree-sitter-generate.sh` passes `--abi 14` → delivered_check grep `--abi 14` present in that script (absent at pin).

## α2 — Issue #2, sidecar npm dependencies
- `gui/sidecar/package-lock.json` exists and `gui/package.json` workspaces exclude `sidecar` → PASS (read at pin). `setup-dev.sh` installs `gui/sidecar` → delivered_check grep `gui/sidecar` present in `scripts/setup-dev.sh` (absent at pin; the only `sidecar` mention there is `write_owner_stamp`'s tmp file).

## #8116 — Issue #3 (adopted, not in this batch)
- Existing task; its own description carries the `custom-protocol` evidence. No block here.

## α4 — Issue #4, sidecar bundles its runtime deps
- `gui/sidecar/tsup.sidecar.ts` has `noExternal: ['@reify/shared-utils']` only → PASS (read at pin). delivered_check grep `@modelcontextprotocol/sdk` present in `tsup.sidecar.ts` (absent at pin).

## β — Launcher Start view
- No-argument launch yields no document: `commands.rs::resolve_initial_file_path` returns `None` for `""` → PASS (read at pin).
- Open dialog substrate: `tauri-plugin-dialog` in the `gui` feature of `gui/src-tauri/Cargo.toml`, `open_file` and `open_file_engine` registered in `main.rs` → PASS, wired.
- Debug listener on demand: today gated by `REIFY_DEBUG=1` at startup (`main.rs` wrapper "REIFY_DEBUG=1 state"); β makes it start from `set_mode` → producer-self. delivered_checks: grep `fn create_new_file` present in `gui/src-tauri/src/main.rs`; grep `StartView` present under `gui/src` (both absent at pin).
- `store_state` debug tool exists (`docs/debug-mcp-contract.md`) → PASS; β extends its payload.

## γ — Edges-in-black toggle
- `EdgesGeometry` + `LineSegments` overlay mechanism exists for selection (`gui/src/viewport/selection.ts::addWireframeFor`) → PASS, wired; γ generalises it in `meshManager.ts`. delivered_check grep `edgesOverlay` present in `gui/src/viewport/meshManager.ts` (absent at pin).

## δ — Installer + release script + clean-container proof
- `tauri-cli 2.10.1` on the host; `tauri.conf.json` bundle `active: true`, targets include `deb` and `appimage`, `externalBin` sidecar, `.ri` association → PASS (read at pin).
- Runtime library closure is dynamic from `/opt/reify-deps` (`ldd target/release/reify`: libTK* 7.9, libgmsh 4.15, libopenvdb 13, libtbb pin, libstdc++) → the bundling is producer-self; the clean container (`docker 29` on host) is the proof. delivered_check grep `tauri build` present in `scripts/build-bundle.sh` (file absent at pin).
- `gh` authenticated for `leodearden/reify` (issue reads succeeded at pin) → PASS for `release-alpha.sh`'s `gh release create`.

## ε — README + getting-started
- delivered_check grep `AppImage` present in `README.md` (absent at pin: README says "No installer, no binary releases").

## ζ — Migration note v0.2-alpha
- Rejections documented must fire (G6 branch 4): delivered by the adopted upstream leaves #5646/#5762/#5763 (dimensioned-construction hard errors), #5312 (string selector forms), #5404 (allowlist burn); each is `producer:task-N upstream` of ζ. Already-landed breaks (real-dimensionless `E_BARE_SCALAR` #4375, struct-ctor conformance, units-length gate) are wired on main. delivered_check grep `v0.1-alpha` present in `docs/releases/v0.2-alpha.md` (file absent at pin).

## ζ2 — Migration note v0.3-alpha
- Producers upstream: #9652 (direction-typing close), #5503/#5504 (stdlib strict visibility + corpus migration). delivered_check grep `direction` present in `docs/releases/v0.3-alpha.md` (file absent at pin).

## ρ1, Ω1, ρ2, Ω — human gates
- `kind: manual`: the resolving escalation is the evidence (`done_provenance.escalation_id`).

## ω — PRD close
- `kind: manual`: the committed SHIPPED header.

## κ — Resume capstone (dark-factory store, external)
- `scripts/resume_project_scheduler.py` committed in dark-factory; `resume_scheduler` tool on reify's `http://127.0.0.1:8100/mcp` (tool list read at pin). Evidence of delivery: a `scheduler_resumed` event in reify `data/orchestrator/runs.db` after `2026-11-02T17:40:28Z`. `kind: manual` here (the producer is in another registry; the df task's own `done_provenance` records it).
