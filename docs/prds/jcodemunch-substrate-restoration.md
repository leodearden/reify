# jcodemunch substrate restoration

**Status:** design — decompose-ready. Authored 2026-08-07.
**Type:** repair + evidence-contract PRD extending `docs/prds/reify-audit-p1-jcodemunch-substrate.md`.
**Approach:** B + H (vertical slice + seam contract and two-way boundary tests).

---

## 1. Goal

`reify-audit`'s four jcodemunch-backed detectors — `P1ProducerOrphan`, `PDeadCode`, `PUntested`,
`PLayerViolation` — are reify's only *unmarked-debt discovery* surface (the PTODO gate inspects only
debt already labelled `TODO`/`FIXME`/`HACK`/`todo!()`/`#[ignore]`). All four have been returning zero
findings behind a single stderr breadcrumb since at least 2026-06-11. This PRD restores the substrate
they need, and — more importantly — replaces the vacuous evidence layer that let the whole chain go
inert without any signal.

**What a user observes when this lands:** `bash scripts/with-jcodemunch-serve.sh reify-audit --pattern P1
--project-root .` completes a real MCP session against a freshly-indexed reify corpus and emits a
well-formed findings array; and if the index is stale or empty, the same command **exits non-zero
naming the drift** instead of printing `0 findings` and exiting 0.

---

## 2. Background — three stacked breakages, all verified 2026-08-07

Against main `16cfb015bc`. Each was reproduced, not inferred.

### 2.1 The serve unit's file is gone
`~/.config/systemd/user/jcodemunch-serve.service` and
`~/.config/systemd/user/default.target.wants/jcodemunch-serve.service` are **both dangling symlinks**
into reaped task worktree `4102`. `systemctl` reports `not-found`; `list-unit-files` reports
`bad enabled`; the journal logs `Failed to open …: No such file or directory` every few minutes.
`docs/architecture-audit/jcodemunch-serve-activation.md:109-122` predicted this exact failure under
"Operator action required"; the step was never performed.

### 2.2 The reify index does not exist, and its maintenance was deliberately disabled
A scan of `meta.source_root` across all ~1400 indexes in `~/.code-index` finds **no index for
`/home/leo/src/reify`** and **no `leodearden/*` git-identity index at all**. The 103 reify-path indexes
are `.eval-worktrees/`/`.claude/worktrees/` trees created by per-agent stdio servers; the newest dates
from 2026-05-30. The four legacy git-identity indexes were deleted by hand on 2026-06-11 and reify was
removed from `jcodemunch-watcher.service`'s `--repos` the same day.

### 2.3 The Rust client cannot complete an MCP handshake — and probably never could
`crates/reify-audit/src/jcodemunch_client.rs:769` mints its own session id (`random_hex_32()`) and
`:811` sends it as `mcp-session-id` on **every** POST including `initialize`. Under MCP
streamable-HTTP the *server* assigns the session id in the `initialize` **response**; jcodemunch
rejects an unknown inbound id with HTTP 404. `post()` maps any 4xx to `LoadError::Http` (`:813`), so
`RealJCodemunchOps::new` errors and — per task 4109's contract — the binary fail-softs to
`NoopJCodemunchOps`.

Reproduced deterministically against a transient serve:

| serve version | `mcp` SDK | `initialize` **with** client-minted id | **without** |
|---|---|---|---|
| 1.108.54 (current watcher pin) | 1.29.0 | **404** | 200 |
| **1.108.27** (what the unit pins) | 1.29.0 | **404** | 200 |
| **1.108.27** | **1.27.2** (resolved on 2026-05-30) | **404** | 200 |

The floating-transport hypothesis is ruled out: `jcodemunch-mcp` pins `mcp>=1.10.0,<2.0.0`, but the
era-correct SDK behaves identically. `scripts/smoke-jcodemunch-serve.sh` passes because it performs the
session flow *correctly* (`:68-113`) — it never exercised the Rust client.

### 2.4 Why nothing caught it — the vacuous evidence layer
The prior PRD's capability manifest contains **no binding asserting the Rust client completes a
handshake**. Its live-serve bindings route around the client: `L-SERVE` binds a curl smoke, `L-CLIENT`
binds a static decode of captured fixtures, `L-WIRE` binds a construction-site substitution. The one
binding that would have caught this — `L-SMOKE` — named `scripts/smoke-jcodemunch-audit.sh`, **which
does not exist**; task 4101 shipped `crates/reify-audit/tests/jcodemunch_live.rs` instead, which is
`#[ignore]`-gated (`:227`) *and* graceful-skips on a bare TCP-connect probe (`:155-175`). It is
PASS-shaped whether or not the chain works.

This PRD's central claim: **2.1–2.3 are symptoms; 2.4 is the disease.**

### 2.5 Consumer reality
`PDeadCode`, `PUntested` and `PLayerViolation` have **zero executing consumers** — every reference is
prose in `.claude/skills/audit/`. In ~14 months of `data/audit-runs/` artifacts the four detectors have
produced **zero recorded findings**, and three have never been invoked once. `P1ProducerOrphan` has one
real consumer (the `/audit` default sweep, invoked by dark-factory `/review` Phase 2,
`skills/review/SKILL.md:118`) and has run live exactly once (2026-06-09, 0 findings).

### 2.6 PDEAD's findings are currently ~100% false positives on Rust
A live `get_dead_code_v2` against a 48-file reify-audit index returned 100 findings, **all at
confidence 1.0**, all carrying the identical signal set `['unreachable_file', 'no_callers',
'not_barrel_exported']` — including `bin/reify-audit.rs::main` and **every `#[test]` function**.
`not_barrel_exported` is a JS/TS concept; `unreachable_file` fires for everything because the tool
found no `main.py`. The server says so itself in `framework_warning`. Activating PDEAD as-is would be
strictly worse than zero findings.

---

## 3. Resolved design decisions

**D1 — Index identity is per-path and hardcodeable.** `storage/git_root.py:55-57` derives
`local/<basename>-<sha1(abspath)[:8]>` from the absolute path alone — no git HEAD, no content, no salt.
`/home/leo/src/reify` → **`local/reify-4ae45bbd`**, verified by prediction-then-measurement. reify-audit
derives this rather than hardcoding, with `--jcodemunch-repo` retained as an override.

**D2 — Freshness is ours to enforce.** `staleness_days: 3` is **inert** for this index (the code takes
a `git_head != HEAD` branch, `get_repo_outline.py:155-175`) and **none of the five detector tools report
freshness at all** — verified tool by tool. `get_dead_code_v2`'s `confidence` is a dead-code score, not
a freshness score. There is no path from "stale" to "reindex" anywhere in the package. A stale index
produces **false orphans** in P1, which is worse than no findings, so reify-audit refuses rather than
degrades.

**D3 — `watch --once` is the single index primitive, used two ways.** A reify-owned timer keeps the
index warm; the audit sweep runs the same primitive before querying, so freshness holds regardless of
whether the timer fired. Measured: full 77.8 s / incremental 24.8 s for 386 `.rs` files at host load
~100; full-repo extrapolation ~5–10 min. Per-path identity is load-bearing here — git-root identity sets
`_merge_with_existing`, which structurally bypasses the incremental branch (`index_folder.py:1745`).

**D4 — reify is NOT re-added to `jcodemunch-watcher.service`.** Not primarily for CPU: the repos-poller
parses `git worktree list --porcelain` and **deliberately skips the first entry, the main working copy**
(`watcher.py:952,978,988,994`). `--repos /home/leo/src/reify` would watch 238 linked worktrees and give
**zero coverage of the checkout we care about** — measured cost +60% of a core in polling plus ~71
CPU-minutes of initial indexing, for nothing. The 2026-06-11 identity-collision rationale is separately
*stale* (per-path identity cannot collide), but the conclusion stands on stronger grounds.

**D5 — The persistent serve unit is retired.** Port 8901 has exactly one consumer in the world:
`reify-audit`. dark-factory launches jcodemunch as a per-agent **stdio** MCP server
(`mcp_lifecycle.py:1047-1050` — a `command` block, deliberately unlike its `http` siblings for
fused-memory and escalation) and has zero coupling to 8901. The sweep spawns and tears down its own
serve; `deploy/systemd/jcodemunch-serve.service` and both dangling symlinks are deleted. This
permanently removes the failure class in §2.1.

**D6 — Advisory-severity discipline is inherited unchanged.** The prior PRD §4-d/§5 and task 4115's
re-confirmation stand: PDEAD/PUNTESTED/PLAYER stay `Severity::Low`, opt-in via `--pattern`, log-only,
no auto-filed tasks. This PRD does not revisit that.

**D7 — P1 first, ambitious work second.** Phase 1 delivers the predictable win (a working wire + a
non-vacuous capstone). PDEAD/PUNTESTED activation is gated behind a spike that may fail; failing it is
an acceptable, documented outcome, not a blocker on Phases 1–2.

---

## 4. Contract — the reify-audit ↔ jcodemunch seam (H)

The seam that failed silently. Pinned here so an implementer cannot re-derive it wrongly.

### 4.1 MCP session lifecycle
1. `initialize` POST carries **no** `mcp-session-id` request header.
2. The client reads `Mcp-Session-Id` from the `initialize` **response** headers and stores it.
3. Every subsequent POST (`notifications/initialized`, `tools/call`) carries that stored value.
4. A 4xx on `initialize` is a **hard** seam failure, never a fall-through to `NoopJCodemunchOps`
   without the operator-visible breadcrumb.
5. `notifications/initialized` legitimately returns 202; `initialize` returning 202/empty/`{}` does
   **not** constitute a live seam (this is the hole pending task **#5832** describes — that task's own
   premise, "the handshake succeeds and we accept too much", is falsified by §2.3 and it must be
   re-read before it is worked).

Reference implementation of the correct flow already in-repo: `scripts/smoke-jcodemunch-serve.sh:68-113`.

### 4.2 Repo-identity resolution
`repo_id = "local/" + basename(project_root) + "-" + sha1(abs(project_root))[:8]`, matching
`storage/git_root.py:55-57`. `--jcodemunch-repo` overrides. The legacy default `leodearden/reify` is
removed — no such index exists or should be created (a git-identity index would collide across reify's
239 worktrees and is never GC'd).

### 4.3 Freshness precondition (evaluated before any detector query)
```
index_head  := meta.git_head       from the resolved index
live_head   := git rev-parse HEAD  in project_root
symbol_count:= count of indexed symbols for repo_id

PROCEED   iff index_head == live_head AND symbol_count > 0
REFUSE    otherwise → non-zero exit naming index_head, live_head, symbol_count
```
The `symbol_count > 0` conjunct is not redundant: `delete-index` leaves a schema-only husk that
re-registers as an empty repo, so *presence* of an index proves nothing — an empty index yields zero
findings silently, the mirror image of the staleness failure.

### 4.4 Indexing invariants
- The nightly/sweep path runs a **bare** `watch --once <project_root>`.
- **`--paths-from` is destructive and must never be used against the production identity.** It
  short-circuits discovery (`index_folder.py:1505-1511`) and every previously-indexed file absent from
  the list is classified deleted and `DELETE`d (`sqlite_store.py:1698`, `:1480-1483`). No warning.
- `--no-ai-summaries` is safe and free: none of the five detector tools read summaries or embeddings;
  their real dependency is the `index.imports` graph.

---

## 5. Boundary-test sketch (H)

The integration-gate task's observable signal. Faces both sides of the seam.

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Handshake conformance | serve up, any index | `initialize` sent without a session header; server-assigned id echoed on the next POST; `tools/list` returns jcodemunch's tool set |
| B2 | Handshake rejection is loud | serve up | client sending a self-minted session id observes 404 — pins the §2.3 regression so it cannot silently return |
| B3 | Non-jcodemunch responder rejected | an HTTP server returning `{}`/202 on `initialize` | `RealJCodemunchOps::new` errors; the seam is not declared live (closes #5832's hole) |
| B4 | Stale index refused | index at commit `N-1`, HEAD at `N` | non-zero exit naming both SHAs; **no** findings array emitted |
| B5 | Empty-husk index refused | index exists, `symbol_count == 0` | non-zero exit naming the symbol count |
| B6 | Fresh index proceeds | `watch --once` just run, HEAD unchanged | detector runs to completion over a non-empty symbol set; well-formed findings array (count may legitimately be 0) |
| B7 | Serve unreachable still fail-softs P2/P5 | no serve on 8901 | `--pattern P2,P5` unaffected and exit 0 — preserves task 4109's contract |
| B8 | Capstone does not skip | serve unreachable, capstone invoked | test **FAILS**; it must not print a skip note and return early |

---

## 6. Decomposition plan

Greek labels; task IDs assigned at decompose time. Phase 1 is the vertical slice; **ε** is the
C-as-integration-gate leaf that Phase 1's foundation tasks unlock.

### Phase 1 — make the wire work and prove it

- **α — Fix `JcodemunchClient` MCP session handling.**
  Modules: `crates/reify-audit/src/jcodemunch_client.rs`.
  *Signal:* a test drives a real handshake against a spawned serve and asserts `tools/list` returns
  jcodemunch's tool set (B1); a companion assertion pins that a self-minted session id is rejected (B2).
  Not a mock — mocks are what let §2.3 survive ten weeks.
  *Note:* `fused_memory_client.rs` is a self-described near-clone with the same pattern but **works
  today** against fused-memory. Do not "fix" it blind; changing it is out of scope here.
  *Prereqs:* none.

- **β — `scripts/jcodemunch-index-reify.sh`.**
  Bare `watch --once`, `--no-ai-summaries`, never `--paths-from`; fails non-zero if the resulting
  symbol count is 0.
  *Signal:* running it prints the resolved identity and `N sym` with N>0; an immediate second run
  reports `changed: 0`. *Prereqs:* none.

- **γ — Identity resolution + freshness gate in reify-audit.**
  Implements §4.2 and §4.3.
  *Signal:* against a deliberately-stale index, `reify-audit --pattern P1` exits non-zero naming both
  SHAs (B4); against an empty-husk index it exits non-zero naming the symbol count (B5); against a
  fresh index it proceeds (B6). *Prereqs:* none (independent of α).

- **δ — `scripts/with-jcodemunch-serve.sh`.**
  Spawn serve → readiness-poll `/mcp` → run the wrapped command → tear down, trapping on exit.
  *Signal:* `bash scripts/with-jcodemunch-serve.sh reify-audit --pattern P1 --project-root .` emits a
  findings array and leaves nothing listening on 8901. *Prereqs:* α.

- **ε — Non-vacuous live capstone (INTEGRATION GATE).**
  Replaces the graceful skip in `crates/reify-audit/tests/jcodemunch_live.rs`.
  *Signal:* with the serve up and the index fresh, the real binary completes a real MCP session and the
  detector runs to completion over a **non-empty symbol set**, emitting a well-formed findings array
  (B6); with the serve unreachable the test **hard-fails** rather than skipping (B8).
  *Explicitly NOT asserted:* "≥1 P1 finding". P1's one recorded live run produced zero findings, so a
  count assertion is an unvalidated premise — precisely the shape that made `L-SMOKE` vacuous.
  *Prereqs:* α, β, γ, δ.

### Phase 2 — durable freshness and unit hygiene

- **ζ — `deploy/systemd/reify-jcodemunch-index.{service,timer}` + installer.**
  Follows the in-repo precedent `scripts/install-warm-lane-units.sh` (which already installs
  `deploy/systemd/reify-warm-lane-gc.timer` entirely reify-side).
  *Signal:* `systemctl --user list-timers` lists it; after one trigger, the index's `meta.git_head`
  equals main's HEAD. *Prereqs:* β.

- **η — Retire `jcodemunch-serve.service`.**
  Delete `deploy/systemd/jcodemunch-serve.service`, remove both dangling symlinks, update the runbook.
  *Signal:* `systemctl --user list-unit-files | grep jcodemunch-serve` returns nothing and the journal
  stops logging `Failed to open`. *Prereqs:* δ.

### Phase 3 — the ambitious half (may fail; failure is a documented outcome)

- **θ — SPIKE: can `entry_point_patterns` express Rust roots?**
  Determine whether `main.rs`, `lib.rs`, `[[bin]]` targets, `#[test]` and `#[cfg(test)]` can be
  expressed such that `unreachable_file` stops firing universally.
  *Signal:* a committed before/after record — today PDEAD flags `bin/reify-audit.rs::main` and every
  `#[test]` at confidence 1.0; after configuration it does not, **or** the spike documents that the
  heuristic is not configurable enough from outside the tool and ι/κ are cancelled.
  *Prereqs:* α, β, γ.

- **ι — Activate PDEAD.** *Signal:* a hand-reviewed sample of 30 findings contains **zero** instances of
  `main`, `#[test]` or `#[cfg(test)]` symbols being flagged. A checkable negative assertion, not a
  guessed false-positive percentage. *Prereqs:* θ (pass).

- **κ — Activate PUNTESTED.** Same shape as ι; rides the same reachability substrate. *Prereqs:* θ (pass).

- **λ — PLAYER: activate or retire.**
  `.jcodemunch.jsonc`'s own header states a healthy run "returns an empty finding set", so its output is
  indistinguishable from a broken one by construction; and B1–B6 are already hard-gated without
  jcodemunch via `scripts/assert-crate-dag.sh`, invoked from
  `crates/reify-build-utils/tests/crate_dag_assertion.rs` plus per-crate `dag_invariant.rs` tests.
  *Signal:* a committed determination naming the exact class of violation PLAYER catches that
  `assert-crate-dag.sh` cannot — or, if there is none, PLAYER's retirement with the overlap documented.
  *Prereqs:* α, β, γ.

### Phase 4 — docs truth

- **μ — Correct the record.**
  `docs/architecture-audit/jcodemunch-serve-activation.md` status line ("Active", false for ~2 months)
  and its resolved-identifier table (`leodearden-reify`, a DB file that does not exist); the prior PRD's
  stale `:171` row claiming the detectors "degrade to exit 125 when serve is down" (superseded by 4109);
  `.claude/skills/audit/SKILL.md:42-44` serve-prerequisite prose.
  *Signal:* no doc asserts a jcodemunch identifier, unit, or degradation contract that contradicts the
  landed behaviour. *Prereqs:* η.

---

## 7. Cross-PRD relationship (G4)

| Other PRD / repo | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/reify-audit-p1-jcodemunch-substrate.md` | repairs | the four detectors + `JCodemunchOps` trait | **this PRD** (evidence layer); prior PRD retains detector design §4-d/§5 | queued (μ corrects its `:171`) |
| `docs/prds/reify-audit-ptodo-detector.md` | sibling | `reify-audit` CLI + `--pattern` dispatch | ptodo PRD | wired, untouched |
| dark-factory `skills/review/` | consumes | `/audit --pattern P1,P2,P5` in `/review` Phase 2 | dark-factory | wired, untouched |
| dark-factory orchestrator | none | uses jcodemunch via **stdio**, not port 8901 | n/a | **no seam** — D5 |

No new cross-repo seam: reify already owns `deploy/systemd/*.timer` and its installer
(`scripts/install-warm-lane-units.sh`), so ζ needs no dark-factory counterpart. This PRD does not
introduce a fourth instance of any known contested-ownership pair.

**Live neighbour:** task **#5830** (in-progress 2026-08-07) touches the same `initialize` path and the
same `cli.rs` mock while de-flaking `default_sweep_survives_unreachable_jcodemunch`. α must land after
or coordinate with it. Task **#5832** (pending) and **#5834** (pending, depends on 5832) sit on the
adjacent response-validation hole — see §4.1 item 5.

---

## 8. Pre-conditions for activating

- `uvx` at `/home/leo/.local/bin/uvx` and network access to PyPI for `jcodemunch-mcp==1.108.54`.
  Note **1.108.27 is no longer on PyPI** (only the git tag survives); the pin moves to 1.108.54 to match
  the watcher.
- `~/.code-index` writable; no `leodearden/reify` git-identity index present (there is none — if one
  ever appears, `resolve_index_identity` raises `IdentityModeAmbiguous`, which is a loud, correct
  failure).
- No task in this PRD requires the orchestrator to be stopped.

---

## 9. Out of scope

- **Revisiting advisory severity / auto-filing** for PDEAD/PUNTESTED/PLAYER — settled by prior PRD §5
  and task 4115. D6.
- **Fixing `fused_memory_client.rs`** — same session-id pattern, but it works today against
  fused-memory. Touching it is a separate, evidence-first change.
- **Pruning the ~700 stale indexes in `~/.code-index`** — a real shared tax (every re-index scans all
  752 DBs via `list_repos()`), but it affects all five watched projects and is not reify's to decide
  unilaterally. File separately.
- **Adding the detectors to a hard verify gate.** They stay opt-in. INV-SF-2's corollary is explicit
  that a diagnostic expected on a healthy path must not be Error-severity, and jcodemunch is
  legitimately absent in task worktrees.
- **Re-adding reify to `jcodemunch-watcher.service`** — D4.

---

## 10. Open questions (tactical)

1. **Where does the freshness gate read `meta.git_head` from?** Options: direct read-only `sqlite3` on
   the index DB, or the `get_repo_outline` MCP tool. The MCP route avoids coupling to the on-disk
   schema; the sqlite route avoids a second round-trip and works before the serve is up. **Suggested:**
   MCP, since δ guarantees a serve is running by then. Decide during γ.
2. **Timer cadence for ζ.** Daily is the obvious default; hourly is affordable given the measured ~25 s
   incremental cost. **Suggested:** daily with `Persistent=true`, matching `reify-warm-lane-gc.timer`.
   Decide during ζ.
3. **Does ε belong in `tests/infra/` rather than as a `cargo test`?** A `tests/infra/test_*.sh` would
   run under `run_all.sh` on the merge gate but needs its drift-guard registration in
   `tests/infra/run-all-classification.manifest` in the **same diff** (per the overlay's gate-test
   drift-guard rule; the esc-4914-162 precedent). A `cargo test` avoids that but needs an env flag to
   escape `#[ignore]`. Decide during ε.
4. **`max_folder_files: 10000`** currently exceeds reify's 3,829 tracked files with headroom, but it
   truncates with a warning rather than erroring if crossed. Worth a guard in β? Decide during β.
