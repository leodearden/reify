# Capability manifest — `debug-server-gating-boundary`

PRD: `docs/prds/debug-server-gating-boundary.md`
Sidecar twin: `docs/prds/debug-server-gating-boundary.capability-manifest.yaml`
Built 2026-08-19 against `main` @ `2a8598c679`. Every binding was resolved first-hand; no binding is inherited from the spawn brief without re-derivation.

## Scope note — why the D3 `.ri` substrate workflow was not run

The overlay's decompose-time workflow (`scripts/prd-decompose-verify.mjs` → `scripts/prd-capability-check.py`) dispatches exactly three probe kinds: `grammar` (`tree-sitter parse --quiet <fixture.ri>`), `check` (`reify check <fixture.ri>`) and `ir` (`reify eval <fixture.ri>`). **This PRD asserts zero `.ri`-language capabilities** — no grammar production, no builtin, no type name, no diagnostic, no evaluation semantics. It introduces no `.ri` fixture. All three vectors are therefore N/A, and running the harness would return `UNPROVABLE` for every premise, which the overlay treats as blocking — an artifact of vector mismatch, not of a false premise.

Every capability below is instead bound to a **Rust/Cargo/verify-pipeline fact**, verifiable by `git grep` and re-derived here with file:line evidence. That is a strictly stronger binding than the `.ri` harness could have produced for this PRD, and the ten `delivered_check`s in the sidecar were each confirmed **discriminating** (currently true, required-false after the leaf lands, or vice versa) on 2026-08-19.

**G3 grammar gate: N/A** (no novel `.ri` syntax). **G6 branches 1 and 2: N/A** (no numeric bound, no closed-form exactness claim — see PRD §7 D8, which forbids an absolute test count in any leaf signal precisely to keep branch 1 out of scope). **G6 branch 3** applies to α, **branch 4** applies to β. Both are discharged below.

---

## α — Move the non-Tauri half of `debug_server` into an ungated `debug_protocol` module

Signal: `comm -13` over `cargo nextest list -p reify-gui` and `… --features gui` returns a set exactly 50 smaller than on the base commit, containing zero `debug_server::` and zero `debug_protocol::` entries; and `cargo nextest run -p reify-gui` (no `--features gui`) executes all 50 formerly-gated tests green, naming them.

| # | Capability | Evidence | Verdict |
|---|---|---|---|
| α1 | **One `cfg` line is the sole gate.** The 50 tests are unreachable without `--features gui` for exactly one reason, so relocating the items removes the barrier. | `grep:gui/src-tauri/src/lib.rs:15-16` — `#[cfg(feature = "gui")]` / `pub mod debug_server;`. `debug_server.rs` contains **no `tauri::` reference at all** (whole-file scan, 4090 lines). | **PASS** |
| α2 | **The 45 non-morph tests' substrate is ungated.** `EngineSession`, `make_test_engine`, `crate::{commands,diff,types,engine_lock,large_stack,path_key}` all compile without the feature. | `grep:gui/src-tauri/src/lib.rs:11-30` — of 18 module declarations only `debug_server` and `event_bus` carry a cfg. `grep:gui/src-tauri/src/tests/mod.rs:34` — `pub(crate) fn make_test_engine()` builds a real `EngineSession` over `MockGeometryKernel`, ungated. | **PASS** |
| α3 | **`reify-mesh-morph` is available ungated at ~zero cost** (the D3 decision that takes the split from 45 to 50). | `grep:gui/src-tauri/Cargo.toml` `[dev-dependencies]` — already an **unconditional** entry (`features = ["testing"]`), so the ungated *test* build already links it. `grep:crates/reify-eval/Cargo.toml:31` — `reify-solver-elastic`, its heaviest dep, is already in reify-gui's non-gui graph via reify-eval. Promotion adds exactly one small crate to the ungated *lib* build. | **PASS** |
| α4 | **No clippy regression** when the moved code becomes visible to the existing `cargo clippy --workspace --all-targets -D warnings` pass. | Task **5841**'s first-hand 2026-08-19 measurement of `cargo clippy -p reify-gui --features gui --all-targets`: 4 debug_server warnings, at `:1788` (`collapsible_if`, in `handle_wait_for_selector` — **stays gated**) and `:2143`/`:2213`/`:2235` (`await_holding_lock`, the three morph tests — **α owns the idiom, PRD §7 D4**). The 35 pure items and 10 engine-helper tests carry **zero** warnings. | **PASS** |
| α5 | **The workspace pass picks the module up with no plan change.** | `grep:scripts/verify.sh:2029` — `emit_nextest_pass "--workspace"`. `.config/nextest.toml` has no `default-filter`, no `package(reify-gui)` group and no reify-gui override (only reify-kernel-occt / reify-kernel-conformance / reify-eval / reify-cli / reify-config appear). Nothing excludes it. | **PASS** |
| α6 | **(b8) survives — the gui pass is not turned into a filter.** | `grep:scripts/verify.sh:2223` — `_gui_feat_cmd` is built with no `-E` fragment. `grep:tests/infra/test_compute_trampoline_registration_wired.sh:1417-1418` — (b8) asserts `! grep -qF -- " -E "`. α adds no filterset; it re-classifies which code is gui-gated, and the pass still runs 100 % of the remainder. PRD §5 C4. | **PASS** |
| α7 | **No plan-line delta ⇒ no `THROUGHPUT-COUNTS` bump.** | α's only `verify.sh` touch is the stale comment at `:2036-2041`; it changes no `add "` site. `tests/infra/test_verify_throughput.sh:306-313` counts plan **lines**, not tests. | **PASS** |
| α8 | **No `release-sensitive-crates.txt` delta.** | reify-gui's release-sensitivity comes from `tests/engine_tests.rs`'s `#[cfg(not(debug_assertions))]` sites (mechanism B, per the file's own header). `debug_server.rs`'s lone `#[cfg(debug_assertions)]` at `:1421` is inside `open_path_into_engine` (stays) and is **not** one of the three declared mechanisms. | **PASS** |
| α9 | **DAG-direction (anti-inversion): 6338 is upstream.** | Task 6338 is `pending`, `depends_on: []`, files `["gui/src-tauri/src/debug_server.rs"]`, and adds `TEST_LOCK` to the two morph tests α relocates. Wired as a hard `add_dependency` edge **6338 → α**, never the reverse. | **PASS** |

**G6 branch 3 trace (end-to-end capability).** α's signal needs: (a) an ungated reify-gui lib build — exists on main today, it is what the workspace pass already builds; (b) `make_test_engine` + `MockGeometryKernel` — α2; (c) `reify_mesh_morph::{stats,diagnostics}` reachable without the feature — α3, delivered *by α itself*; (d) a green `-D warnings` clippy pass over the moved code — α4 + α's own D4 work. **Every required capability is delivered by α or already present on main. None is owed by a task that depends on α.**

---

## β — Land the gating-boundary drift guard

Signal: a **seeded violation is caught, and only while seeded**. Adding a `#[test] fn` with no gui dependency to `debug_server.rs` makes `bash tests/infra/test_debug_server_gating_boundary.sh` exit non-zero naming that file and clause C2; reverting returns exit 0. Seeding `use tauri::AppHandle;` into `debug_protocol.rs` trips clause C3 by name.

| # | Capability | Evidence | Verdict |
|---|---|---|---|
| β1 | **`run_all.sh` discovers and runs a new `tests/infra/test_*.sh`.** | `grep:scripts/verify.sh:2493` — the `--include-infra` arm emits the single `run_all.sh` plan line. `tests/infra/run-all-classification.manifest` holds 217 rows today; `tests/infra/test_run_all_classification.sh` is the declared-vs-discovered drift guard that makes an unregistered test fail loudly rather than silently skip. | **PASS** |
| β2 | **The bucket row lands in the same diff as the test.** | Overlay gate-test drift-guard rule; the esc-4914-162 precedent (task 4914 landed a gate-resident test without its registrations and turned main RED for every subsequent merge). β's diff is exactly `{the script, the manifest row}` — the registration cannot be ordered after the test because they are one commit. | **PASS** |
| β3 | **Rejection mechanism exists and fires (G6 branch 4).** | β's signal is not "the guard exists" but "the guard *rejects*". It is discharged by authoring the violation, running the guard, and **observing the non-zero exit and the named clause** — then observing exit 0 on revert. Both directions are required; a guard that never goes red is the `feedback_graceful_skip_helper_is_a_vacuity_vector` shape. The `kind: script` delivered_check rides alongside the discriminating manifest-row grep, never alone (the `merge-gate-compile-cost` sidecar convention). | **PASS** |
| β4 | **DAG-direction (anti-inversion): α is upstream.** | β asserts the post-move state (C1–C4). Landing it before α would be red; writing it to skip when `debug_protocol.rs` is absent would make it vacuously green. Hard `add_dependency` edge **α → β**. | **PASS** |
| β5 | **No plan-line delta.** | β adds a `tests/infra/test_*.sh`, discovered behind the *existing* `--include-infra` plan line (β1). No `THROUGHPUT-COUNTS` bump. No wall-clock assertion ⇒ no `test_no_new_wallclock_upper_bounds.sh` registration. Not a `crates/*/tests/*.rs` compile unit ⇒ no `.config/nextest.toml` partition entry. | **PASS** |

---

## Cross-PRD / out-of-batch bindings

| Binding | Direction | Evidence | Verdict |
|---|---|---|---|
| `6338 → α` | prerequisite | Task 6338 `pending`, no deps, relocated-by-α files. PRD §4. | **PASS** (upstream) |
| `α → 5841` | dependent | Task 5841 `pending`, already `depends_on: [6338]`. Its prose anchors (`debug_server.rs:2143/2213/2235`) and its "apply ONE idiom to all five" instruction assume the pre-move layout; α establishes that idiom and updates 5841's description to its post-move scope. PRD §4. | **PASS** (downstream — α does not depend on it) |
| `docs/prds/v0_6/reify-debug-mcp-expansion.md` | consumer | Draft, **no filed tasks** — nothing to block on. This PRD takes seam ownership (§7 D5) and β is the mechanism that keeps its ~30 future tool-def tests out of the gated surface. | **PASS** (no orphan: β is the wiring) |

## Bindings deliberately NOT claimed

- **No merge-gate wall-clock saving.** PRD §1.1 forbids it and no leaf signal asserts it. The gui pass is unfiltered ((b8)) and `affected_crates()` is a reverse closure, so neither leaf makes the gate cheaper. A leaf that claimed otherwise would be the `bound≤floor`-shaped failure this manifest exists to catch — asserted here as an explicit non-claim so a later reader cannot mistake its absence for an oversight.
- **No absolute post-move test count.** PRD §7 D8. reify-gui gains tests weekly; `885`/`900`/`911` are cited as 2026-08-19 provenance (`review/briefing.yaml:561`, `data/verify-logs/6200/attempt-1.test-20260819T035137Z.log`), never as an acceptance target.
- **No claim about the residual gui-only set's size.** PRD §2 records a `6` structural count against a `911 − 850 = 61` arithmetic implying ~11, and states the discrepancy is unresolved and not load-bearing. Neither leaf signal depends on it. Labelled rather than papered over.
