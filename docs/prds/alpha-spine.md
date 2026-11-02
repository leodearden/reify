# PRD — Alpha spine: a binary a tester can install, the first alpha user's write-ups and asks landed, the language breaks shipped first

**Milestone:** alpha-1 (cross-milestone; placed at root `docs/prds/` because it spans v0_6 PRDs and release engineering) · **Status:** active · **Approach:** B + H-lite (two short contracts — the launcher seam and the bundle contents — plus a boundary table; no kernel/FEA/grammar seam is touched) · **Authored:** 2026-11-02 by the portfolio-review follow-up session (brief `~/.claude/spawn-briefs/2026-11-02-portfolio/reify-alpha-spine-prd.md`; Leo's rulings of 2026-11-02 quoted in §2)

**Code anchors** verified against main `f8e47e232c` (2026-11-02). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

---

## 1. Goal & user-observable surface

A named alpha tester on a stock Ubuntu 24.04 machine downloads one file from a GitHub release, installs it, and — without `git clone`, `setup-dev.sh`, `cargo`, a PPA or a conda env — clicks the Reify icon, picks or creates a `.ri` file and a mode in a start window, sees the model render, and can switch the viewport to an "edges in black" view. The four issues the first external user filed on 2026-09-28 (`leodearden/reify` #1–#4) are closed by landed fixes. Every source-breaking language change that is already decided is released with a migration note before more users arrive: the small set in **v0.2-alpha** with the binary, the two large ones (direction typing, stdlib namespace) in **v0.3-alpha** behind it. The reify scheduler, paused on 2026-11-02 for exactly this PRD, is resumed by a capstone this PRD files, and nothing outside this spine dispatches until the spine is complete.

**G1 consumer:** the alpha users — Zizi today (`mission-control/src-reports/reify/sources/2026-09-30-zizi-first-alpha-user-signal.txt`), the one to three humans Leo invites next. Every mechanism below is consumed by that person through the installed GUI, the CLI in the same package, the GitHub release page, or the migration note.

**G2 leaf signal (the completion definition, verbatim the gate Leo closes):** a named tester installed Reify from the v0.2-alpha `.deb` or AppImage on Ubuntu 24.04 without the build-from-source path, opened the launcher, built a model, used the edges view; GitHub issues #1–#4 are closed with a landed fix each; v0.2-alpha and v0.3-alpha are published with migration notes.

## 2. Background — what the portfolio review measured and what Leo ruled

- `plans/portfolio-review-2026-11-01/portfolio-review.md` §2.2 and `seats/P2.md` §5: 0 of 9,640 reify tasks reference the external alpha user or the two feature asks; no alpha tree, milestone or completion definition; two PRDs justify source breaks with "no external users yet", which the 09-30 signal contradicts. Review-all: 26 streams decomposed, 0 landed. Pending 2,355; agent follow-ups 1,051.
- Leo, 2026-11-02: approved R1 (three consumer spines; park everything with no consumer clock); "Pause the reify scheduler until the spine PRDs have landed. Those PRDs should include a capstone for unpausing it"; "we have exactly one extremely light external user for reify as yet. I want to get the language breaking changes out of the way as quickly as possible before we get a bunch more users."
- The scheduler pause is live: reify `data/orchestrator/runs.db` `scheduler_state` row `pause_class=operator`, `pause_at=2026-11-02T17:40:28Z`, reason "pause reify dispatch until the alpha spine PRD lands; its capstone resumes the scheduler. In-flight tasks and the merge lane continue." A `halt_scheduler` pause is persisted to runs.db and survives restarts (`escalation/src/escalation/server.py::halt_scheduler`), so the 8 h fleet redeploy does not undo it.
- The first alpha prep (2026-05-06, tag `v0.1-alpha`) shipped README, getting-started, setup-dev preflight and a smoke test; its tester flow was `git clone --branch v0.1-alpha`. README still says "No installer, no binary releases. You build from source."
- The alpha user's write-ups are the four GitHub issues, author `ZiziSolomon`, 2026-09-28, each with a root cause and a tested workaround. **None is fixed on main at `f8e47e232c`**: `scripts/setup-dev.sh` still runs `cargo install tree-sitter-cli --locked` unpinned and never installs `gui/sidecar`; `scripts/run-gui.sh` still builds without `tauri/custom-protocol` (task #8116 already names this one); `gui/sidecar/tsup.sidecar.ts` still has `noExternal: ['@reify/shared-utils']` only. The two feature asks from the Signal chat: "a 'just click on it' bit of gui so it doesn't need launching from terminal, with a window from which you select your .ri or create a new one, and the mode which you want to launch in"; "an 'edges in black' view is one i like to use … i do have something close to that when i click on a thing" (the selection wireframe in `gui/src/viewport/selection.ts`, orange, selection-only).

## 3. Resolved design decisions

| # | Decision | Why |
|---|---|---|
| D1 | **Two releases, one spine.** v0.2-alpha = binary + issue fixes + both asks + the small breaking set (dimensioned-construction δ₁/δ₂/ε, FEA selector strings Bmig2, check error-exit allowlist burned to zero) + migration note A. v0.3-alpha = direction typing + stdlib namespace + migration note B. The spine-complete gate Ω waits on both. | The tester's binary should not wait behind the two deepest chains in the store (direction typing pulls S15 α–υ; stdlib κ/λ pulls resolution-unification β–κ: 54 open tasks, critical path ≈ 12 landings). Leo's "before a bunch more users" is still honoured: nothing else dispatches until v0.3-alpha ships, and 1–3 alpha testers get one more migration note. Lever for Leo: `remove_dependency(Ω ← ρ2)` releases the backlog after v0.2-alpha instead. |
| D2 | **Parking by dependency on Ω**, the spine-complete human gate: every reify task that is `pending` at decompose time and not in the spine closure gets `add_dependency(task → Ω)`. `deferred`, `blocked` and `in-progress` rows are untouched. Follow-ups spawned by spine tasks after decompose are not parked (small, on-path). | Leo 2026-10-29: park by adding dependencies, never by deferring. One holder, one `remove_dependency` per early release. Closing Ω releases the backlog in one act; Leo may instead re-park it behind the next spine. |
| D3 | **The resume capstone is a dark-factory-store deterministic deploy task** (`task_kind=deterministic`, `before_done` = committed `scripts/resume_project_scheduler.py` calling `resume_scheduler` on `http://127.0.0.1:8100/mcp`, `always_escalates=false`), filed by the decompose session after parking is verified. A paused scheduler dispatches nothing — deterministic tasks included (`acquire_next()` returns `None`) — so the capstone cannot live in reify's store; a session-performed resume would leave no store record and no re-runnable act. The session verifies the `scheduler_resumed` event in reify runs.db and falls back to calling the tool itself only if the df task has not fired within one df tick cycle. | Auditable (`done_provenance` on the df task, `scheduler_resumed` event with the reason), re-runnable, and ordered strictly after parking. |
| D4 | **Launcher = a Start view inside the existing Tauri app**, shown whenever no document is loaded (`reify-gui` with no argument already yields `resolve_initial_file_path("") == None`). Open… uses the already-wired `tauri-plugin-dialog`; New… writes a minimal module template to a path the user picks; Recent is a persisted list; Mode ∈ {Design, Design + Assistant, Debug}. Debug starts the MCP listener on demand in-process (today it is read from `REIFY_DEBUG=1` at startup); Assistant shows the sidecar panel and states that it needs a Claude Code login (`@anthropic-ai/claude-code` is the sidecar's dependency). The desktop entry and `.ri` file association come from the Tauri bundler config already in `gui/src-tauri/tauri.conf.json`. | No second process, no re-exec, no new IPC; the three modes map to existing capabilities. "Dev mode" (vite hot reload) is a contributor mode and stays on `scripts/run-gui-dev.sh`. |
| D5 | **Edges view = a persistent viewport toggle** rendering a black `EdgesGeometry` overlay over every mesh (threshold angle as a constant), independent of selection, persisted in the GUI store, toolbar button + key, exposed in `store_state` for the debug MCP. Default off. | Matches the ask; reuses the overlay mechanism selection already uses; observable without a screenshot. |
| D6 | **Installer = `cargo tauri build` for `deb` + `appimage`** via a committed `scripts/build-bundle.sh`, bundling the runtime `.so` set from `/opt/reify-deps` (OCCT 7.9, gmsh, OpenVDB, oneTBB pin, FLTK, conda `libstdc++`/`libgcc_s`; measured by `ldd target/release/reify`) and shipping the `reify` CLI in the same package. Proof is a clean `ubuntu:24.04` container with no PPA and no conda: install, `reify check examples/m5_geometry.ri`, then the GUI under `xvfb-run` with `REIFY_DEBUG=1` answering a debug-MCP `store_state` with ≥ 1 mesh. The proof script is a heavy, off-gate infra test (docker + a cold bundle build). | The engine links those libraries dynamically from a conda prefix the bundler does not know about; a package that works only where `/opt/reify-deps` exists is the build-from-source path with extra steps. The container proof is the only honest signal. |
| D7 | **Releases are pure human gates**, one each (ρ1, ρ2): the L2 names the two commands (`scripts/release-alpha.sh <tag>`, delivered by δ, which builds the bundle, tags, creates the GitHub release with the assets and the note, and closes the issues the note names). | The GitHub token is Leo's; agent subprocess env strips secrets by design. One act per release is inside R1's "bounded list of his acts". |
| D8 | **Migration notes are tested documents**: `docs/releases/<tag>.md` lists each source-breaking change with a before/after `.ri` snippet and the diagnostic code; every "before" snippet is a committed fixture under `tests/prd-gate/fixtures/` that an infra test runs through `reify check` expecting that code, and every "after" fixture passes. Spec §14.5 is the obligation; the test is what keeps the note true. | G6 branch 4: a rejection claimed in prose must be observed to fire. |
| D9 | **Existing breaking-change leaves are adopted by dependency, never re-filed.** v0.2 set: #5646, #5762, #5763 (dimensioned-construction), #5312 (+ its dep #8102), #5404; #8116 for issue #3. v0.3 set: direction-typing ν #9652 and stdlib-namespace κ #5503 / λ #5504 with their open closures (resolution-unification β–κ #5516–#5524, #6953; S15 #8674, #8679, #8683, #8686, #8697, #8706, #9293, #8730; #6941, #7456). | SPOT: those PRDs own their leaves; this PRD only orders the release gates behind them. |

## 4. Pre-conditions for activating

Verified at `f8e47e232c`: `tauri-cli 2.10.1` on the host and the bundle config with `deb`/`appimage` targets, `externalBin` sidecar and the `.ri` association; `tauri-plugin-dialog` wired (`open_file` command in `gui/src-tauri/src/main.rs`); `tree-sitter 0.26.8` supports `generate --abi 14` (`TREE_SITTER_ABI_VERSION` env), workspace pins `tree-sitter = "0.24"`; docker 29 on the host; `gh` authenticated for `leodearden/reify`; debug-MCP tools `reify_open_file`, `screenshot_window`, `store_state` (`docs/debug-mcp-contract.md`); the e2e smoke lifecycle `gui/test/visual/lib_e2e_smoke.sh`. No novel grammar — G3 grammar gate N/A; the semantic substrate used by the migration-note fixtures is delivered by the adopted leaves this PRD depends on.

## 5. Contract (H-lite)

**5.1 Launcher seam.** Frontend state `document.loaded: boolean` selects `StartView` (false) or the editor (true). Backend commands, all registered in `main.rs`'s handler list: `open_file_engine(path)` (exists), `create_new_file(path) -> Result<FileData, String>` (writes the template, then loads it through the same funnel as open), `list_recent() -> Vec<RecentEntry{path, opened_at}>` and `record_recent(path)` (persisted under the Tauri app-data dir), `set_mode(mode: "design"|"assistant"|"debug") -> Result<ModeState, String>` where `debug` starts the MCP listener on `REIFY_DEBUG_PORT` (default 3939) if not running and returns the bound port, `assistant` only flips the panel visibility. Invariants: a failed open or create is a loud error with a code (`E_GUI_OPEN_FAILED`, `E_GUI_TEMPLATE_WRITE_FAILED`) in the Start view, never a silent return to it (INV-SF-6); a mode is idempotent; no new process is spawned by any mode.

**5.2 Bundle contents.** A `.deb` and an AppImage, each containing: `reify-gui`; `reify` CLI on `PATH` (`/usr/bin/reify` for the deb); `reify-sidecar` + the landlock resources; the full runtime shared-library closure of both binaries that is not in a stock Ubuntu 24.04 install (derived at build time from `ldd`, never a hand list), with an `RPATH`/`LD_LIBRARY_PATH` wrapper so the system loader finds them; a desktop entry with the `.ri` association. Invariant: the package installs and runs on `ubuntu:24.04` with no extra apt sources and no `/opt/reify-deps`.

## 6. Boundary-test sketch (H-lite)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | `reify-gui` with no argument | installed package | Start view shown; `store_state` reports `document.loaded=false` |
| B2 | Open… → `examples/m5_geometry.ri` | B1 | model rendered; `store_state` path equals the chosen file; mesh count ≥ 1; Recent lists it |
| B3 | New… → `/tmp/x.ri` | B1 | file exists, parses, loads; Start view not shown |
| B4 | Open… → an unreadable path | B1 | Start view stays with `E_GUI_OPEN_FAILED` visible; no crash |
| B5 | Mode = Debug | B1 | listener bound on `REIFY_DEBUG_PORT`; a second Debug selection is a no-op |
| B6 | Edges toggle on, then off | B2 | `store_state.viewport.edges` true then false; a screenshot differs between the two states |
| B7 | Clean container install (deb, then AppImage) | `ubuntu:24.04`, no PPA, no conda | `reify check examples/m5_geometry.ri` exits 0; GUI under xvfb answers `store_state` with ≥ 1 mesh |
| B8 | Fresh clone with `tree-sitter-cli 0.27` | setup-dev.sh | generated `parser.c` has `LANGUAGE_VERSION 14`; smoke test passes |
| B9 | `run-gui.sh` on a fresh clone | setup-dev.sh | sidecar builds (deps installed), release window renders (custom-protocol), Assistant panel's sidecar starts (SDK bundled) |
| B10 | Each migration-note "before" fixture | v0.2 / v0.3 breaking leaves landed | `reify check` exits non-zero with the named code; each "after" fixture exits 0 |

## 7. Cross-PRD relationship (G4)

| Other PRD / program | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/dimensioned-construction-strictness.md` | consumes | δ₁ #5646, δ₂ #5762, ε #5763 land the hard errors the v0.2 note documents | other-prd | wired (ρ1 ← ζ ← #5646/#5762/#5763) |
| `docs/prds/v0_6/fea-load-support-selector-migration.md` | consumes | Bmig2 #5312 (string selector forms become errors) | other-prd | wired (ζ ← #5312 ← #8102) |
| `docs/prds/v0_6/eradicate-silent-undef.md` | consumes | #5404 burns the check error-exit allowlist | other-prd | wired (ζ ← #5404) |
| `docs/prds/v0_6/direction-typing.md` | consumes | ν #9652 (PRD close; scalar builtin forms retired) | other-prd | wired (ζ2 ← #9652) |
| `docs/prds/v0_6/stdlib-namespace.md` | consumes | κ #5503 strict visibility, λ #5504 corpus migration + `fix --imports` | other-prd | wired (ζ2 ← #5503, #5504) |
| `docs/prds/v0_6/resolution-unification.md`, S15 trampoline-decode, compiler-declaration-passes α | transitive | closure of the v0.3 leaves above | other-prd | wired by their own edges |
| GUI PRDs S25/S26 (`gui-backend-engine-modules-events-tool-contract.md`, `gui-frontend-wire-bindings-bridge-port.md`), `gui-purpose-surface.md` | produces | Start view + edges overlay in `gui/src`, three new commands in `main.rs` | this-prd | parked behind Ω; their later refactors move these like any other command |
| review-all program `plans/review-all-program-reify-20261012.md` and human gate #8612 | parked | dependency on Ω | this-prd | wired at decompose (D2) |
| dark-factory (project `dark_factory`) | produces | the resume capstone κ (external task) | this-prd | filed at decompose |

No contested pair from the overlay's §G4 list is touched.

## 8. Decomposition plan

Labels are the sidecar's; task ids backfilled at decompose on 2026-11-02 (reify #9727–#9740, dark_factory #8275). Parking performed: 2,332 pending tasks → Ω #9739 (record on #9739's details). Priority high unless stated. Gates are `task_kind=deterministic`, `always_escalates=true`, no `before_done`.

| Label | Task | Prereqs | Observable signal |
|---|---|---|---|
| α1 #9727 | Issue #1: `scripts/tree-sitter-generate.sh` always generates ABI 14 (`--abi 14`, matching the `tree-sitter = "0.24"` runtime); `setup-dev.sh` records the CLI version it was tested with and its ABI pin | — | B8: with `tree-sitter-cli 0.27` installed, `scripts/tree-sitter-generate.sh --force` writes `LANGUAGE_VERSION 14` and `reify check examples/m5_geometry.ri` passes; `tests/infra/test_tree_sitter_abi_pin.sh` registered in `run-all-classification.manifest` |
| α2 #9728 | Issue #2: `setup-dev.sh` installs `gui/sidecar` (`npm ci`), and `gui/sidecar/build-sidecar.sh` refuses loudly when `node_modules` is absent | — | B9 first clause: a fresh clone's `run-gui.sh` reaches the sidecar build without npx prompting; `tests/infra/test_run_gui_scripts.sh` extended |
| — | Issue #3: existing **#8116** (release `run-gui.sh` builds with `tauri/custom-protocol`), raised to high | — | its own: release window renders `gui/dist` |
| α4 #9729 | Issue #4: `tsup.sidecar.ts` bundles the sidecar's runtime dependencies (`@modelcontextprotocol/sdk` and the rest) so `target/release/reify-sidecar` runs with no `node_modules` on its path | — | B9 third clause: the sidecar started from an empty temp dir completes the handshake; sidecar vitest asserts the built bundle imports nothing bare |
| β #9730 | Launcher: `StartView` + `create_new_file` / `list_recent` / `record_recent` / `set_mode` per §5.1; Debug mode starts the MCP listener on demand | — | B1–B5 as an e2e smoke scenario in `gui/test/visual` plus vitest; `store_state` carries `document.loaded` and `mode` |
| γ #9731 | Edges-in-black viewport toggle per D5 | — | B6: `store_state.viewport.edges` toggles; screenshot delta in the e2e smoke; vitest on `meshManager` |
| δ #9732 | Installer: `scripts/build-bundle.sh` (deb + AppImage per §5.2, CLI included, runtime `.so` closure from `ldd`), `scripts/release-alpha.sh <tag>` (bundle → tag → `gh release create` with assets + `docs/releases/<tag>.md` → close the issues the note names), and the clean-container proof `tests/infra/test_bundle_clean_install.sh` (heavy bucket, off the merge gate) | #8116 | B7 passes on the host for both package forms |
| ε #9733 | README + getting-started: binary install first, source build under "For contributors", launcher in §4, "What to expect" rewritten (installer exists, Ubuntu 24.04 still the only platform) | β, γ, δ | README names the release asset download; `scripts/review-readme.sh` passes; B1 walkthrough matches the doc |
| ζ #9734 | Migration note v0.2-alpha (`docs/releases/v0.2-alpha.md`) per D8: every source-breaking change since `v0.1-alpha` (real/dimensionless unification and bare `Scalar`, struct-ctor field conformance, dimensioned-construction strictness, FEA selector strings, undef allowlist burn, angle units, units-length gate — derived from `git log v0.1-alpha..` and the PRDs' Status lines, not from this list) + "coming in v0.3-alpha" naming direction typing and stdlib namespace; fixtures + `tests/infra/test_migration_note_v0_2.sh` | #5646, #5762, #5763, #5312, #5404 | B10 for the v0.2 set |
| ρ1 #9735 | **Release v0.2-alpha** (gate): L2 to Leo naming `scripts/release-alpha.sh v0.2-alpha` and the issue closes | α1, α2, #8116, α4, β, γ, δ, ε, ζ | the release page exists with both assets and the note; issues #1–#4 closed |
| Ω1 #9736 | **Alpha-1 running** (gate): Leo closes with the tester's name when §1's G2 signal holds for v0.2-alpha | ρ1 | the resolved escalation names the tester and the date |
| ζ2 #9737 | Migration note v0.3-alpha (`docs/releases/v0.3-alpha.md`) per D8 for direction typing and stdlib namespace (explicit imports, strict visibility, `fix --imports`), fixtures + `tests/infra/test_migration_note_v0_3.sh` | #9652, #5503, #5504 | B10 for the v0.3 set |
| ρ2 #9738 | **Release v0.3-alpha** (gate): L2 naming `scripts/release-alpha.sh v0.3-alpha` | ζ2, ρ1 | the release page exists with both assets and the note |
| Ω #9739 | **Alpha spine complete** (gate, the parking holder): Leo closes it to release the backlog, or re-parks the backlog behind the next spine first | Ω1, ρ2 | closing it makes the parked tasks dispatchable |
| ω #9740 | PRD close: Status → SHIPPED with landed ids, AS-AUTHORED freeze, LIVE map; same header on the manifest (`complexity=simple`) | Ω and every leaf above | the committed header |
| κ dark_factory:#8275 | *(dark-factory store)* Resume reify's scheduler: deterministic deploy, `before_done` = `scripts/resume_project_scheduler.py --url http://127.0.0.1:8100/mcp --reason …`, filed after parking is verified | none | reify runs.db carries a `scheduler_resumed` event after `2026-11-02T17:40:28Z`; the df task's `done_provenance.kind` is `deterministic-deploy` |

**Parking (D2), performed at decompose, recorded in the sidecar note and in `plans/`-free form on Ω's details:** the spine closure = the labels above + the adopted leaves + their open transitive closure at decompose time; every other `pending` reify task receives `add_dependency(task → Ω)` through the fused-memory client, and the count written is verified by a read-only count of `dependencies` rows with `depends_on = Ω`.

G7 walk (reify family INV-SF-1..7, INV-PD-1..2; advisory in author mode): β's failure paths carry codes (INV-SF-6) and are not silent (INV-SF-2); δ's proof is a real install, not a file-list check; ζ/ζ2's claims are executed fixtures. No waiver needed.

## 9. Out of scope

macOS/Windows packages; code signing; an auto-updater; a launcher outside the Tauri app; dev-mode (vite) in the launcher; any further language break not already decided in a committed PRD; the review-all program and the follow-up backlog (parked, not cancelled); `verify_admission_order` and priority overrides (parking makes them moot for the spine; R1's lever stays available to Leo).

## 10. Open questions (tactical; decide at the named leaf)

1. **Default for the edges toggle** (off, per D5) and the threshold angle — decide at γ with one screenshot of `large_assembly.ri` each way.
2. **AppImage vs deb as the headline asset** in README — decide at ε from which one B7 passes more cleanly; ship both.
3. **Template contents for New…** — decide at β; `examples/m5_geometry.ri` minus the warning (add the `module` line) is the suggested default.
4. **Whether `scripts/release-alpha.sh` also opens a draft release first** — decide at δ; default: create the release directly, since ρ1's L2 is the human check.
