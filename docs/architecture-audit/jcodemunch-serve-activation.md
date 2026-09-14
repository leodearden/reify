# jcodemunch serve + index activation runbook

**Status (2026-09-10):** The persistent query-serve unit is **RETIRED** (task η).
`git ls-files deploy/systemd/` no longer lists it and both `~/.config/systemd/user`
symlinks are gone. Queries are answered by a **transient** serve that each consumer
spawns for the duration of one command and tears down on every exit path
(`scripts/with-jcodemunch-serve.sh`). Index freshness is a separate concern with a
separate owner (`scripts/jcodemunch-index-reify.sh` + a daily timer).

Design: `docs/prds/jcodemunch-substrate-restoration.md` §4 (δ), §2.1 (η), ζ
Prior design: `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §8 (L-SERVE, leaf 1)

---

## Overview

Two moving parts, deliberately separate:

| Concern | Owner | Lifetime |
|---|---|---|
| **Answering queries** | `scripts/with-jcodemunch-serve.sh` — a transient serve, default `127.0.0.1:8901` | one wrapped command |
| **Keeping the index current** | `scripts/jcodemunch-index-reify.sh`, driven by `deploy/systemd/reify-jcodemunch-index.timer` | daily, `Persistent=true` |

Port 8901 has exactly one consumer in the world — `reify-audit` — so a machine-wide
always-on daemon bought nothing and rotted silently. Its installed symlink pointed into
task worktree `4102`, which was later reaped; `systemctl` reported `not-found` and the
journal logged a failure every few minutes for roughly two months while the status line
of *this* document still read "Active". That is the failure the transient wrapper exists
to make impossible: a serve that only exists inside a command cannot outlive the tree it
was spawned from. (The prediction was recorded here under a since-deleted "Operator
action required" section and was never acted on — cited from
`docs/prds/jcodemunch-substrate-restoration.md` §2.1.)

`jcodemunch-watcher.service` is a **different, still-live** host unit and was not touched
by the retirement. It indexes five other repositories; reify was removed from its
`--repos` list on 2026-06-11 and was never added back. Nothing about reify's index
freshness depends on it — that is precisely why the ζ timer below exists.

---

## Resolved identifiers

| Field | Value |
|---|---|
| **Repo identifier** | `local/reify-4ae45bbd` (for `--project-root /home/leo/src/reify`) |
| **Storage path** | `~/.code-index` (default; overridable per §"Index freshness") |
| **Shared DB file** | `~/.code-index/local-reify-4ae45bbd.db` |
| **Serve version** | the pin in `scripts/with-jcodemunch-serve.sh` — that script owns it; do not copy it here |
| **Serve URL** | `http://127.0.0.1:8901/mcp` — **no trailing slash** (see the wrapper's header for why a `/mcp/` redirect silently breaks the session contract) |

The identifier is **derived, not configured**: `local/<basename>-<sha1(abs project_root)[..8]>`,
computed identically by `scripts/jcodemunch-index-reify.sh` and by `reify-audit`'s
`--jcodemunch-repo` default.

### Why the identity is forced

jcodemunch ships `"git_root_identity": True` as a **default** (1.108.54, `config.py:384`),
so any checkout with a parseable `origin` resolves to its **git** identity — for this
repository, `leodearden/reify`. Reify does not want that: a `<owner>/<project>` index names
the **project**, not the **checkout**, and reify has ~239 worktrees of one project. They
would all resolve to one identifier carrying different `git_root`s, and jcodemunch's
`index_folder` collision guard hard-refuses on the mismatch.

So every invocation site carries an explicit `env JCODEMUNCH_GIT_ROOT_IDENTITY=0` prefix —
`scripts/jcodemunch-index-reify.sh` for the indexer, `scripts/with-jcodemunch-serve.sh` for
the serve. Measured against the pinned wheel with a clean store: default config resolves to
`git leodearden/reify`; with the lever set it resolves to `local/reify-4ae45bbd`.

**The husk is expected and benign.** Every run re-creates an **empty** `leodearden-*.db`
in `~/.code-index` as an upstream side effect — 0 symbols, 0 files, no `source_root` in
`meta`. Deleting it and re-running recreates it, so its presence is *not* evidence that the
lever failed, and "no such index exists" is not a state anyone can assert or maintain. It is
inert for identity resolution precisely **because** it is empty: `_existing_git_identity`
skips any entry whose stored `git_root` does not contain the path, and the husk stores none.
If it ever stops being inert it will have acquired a `source_root` — which is exactly what
`jcodemunch-index-reify.sh`'s `E_JC_INDEX_MISSING` hijack diagnostic keys on.

**Response encoding**: `get_changed_symbols` returns MUNCH-encoded data
(`#MUNCH/1 tool=get_changed_symbols enc=gen1`) for non-empty results. The smoke script
detects this format directly rather than attempting JSON decoding.

---

## Getting a serve

```bash
scripts/with-jcodemunch-serve.sh <command> [args...]
scripts/with-jcodemunch-serve.sh --port 8917 -- <command> [args...]
scripts/with-jcodemunch-serve.sh --dry-run     # print the serve argv, spawn nothing
```

Spawn → readiness-poll (an **identity** check, not a bare TCP connect) → run the wrapped
command with `JCODEMUNCH_URL` exported → unconditional teardown. When it refuses, it says
why with a machine-greppable marker: `E_JC_SERVE_PORT_BUSY`, `E_JC_SERVE_SPAWN_FAILED`,
`E_JC_SERVE_NOT_READY`, `E_JC_SERVE_LEAKED`. **That script's header is the single source of
truth** for the marker semantics, the pinned jcodemunch version and the trailing-slash rule —
read it there rather than trusting a copy.

The wrapper owns the serve **lifecycle only**; it deliberately does not rewrite the wrapped
command's argv. A client still has to ask for the right identifier itself.

---

## Index freshness

```bash
scripts/jcodemunch-index-reify.sh                  # index the canonical checkout
scripts/jcodemunch-index-reify.sh --check-only     # refuse, do not index, if stale/missing
```

Automated by the ζ units `deploy/systemd/reify-jcodemunch-index.{service,timer}`
(`OnCalendar=daily`, `Persistent=true` so a missed tick after host downtime is caught up),
installed by `scripts/install-jcodemunch-index-units.sh` — which `scripts/setup-dev.sh` runs
non-fatally on every dev setup.

`reify-audit` probes this index before running any jcodemunch-backed detector and refuses
with `E_JC_INDEX_STALE` / `E_JC_INDEX_EMPTY` / `E_JC_INDEX_UNREADABLE` rather than reporting
findings computed from a stale corpus. The directory it probes resolves
`--jcodemunch-index-dir` → `$JCODEMUNCH_INDEX_DIR` → `$CODE_INDEX_PATH` → `$HOME/.code-index`;
`jcodemunch-index-reify.sh` resolves its DB under `$CODE_INDEX_PATH` for the same reason, so
the two agree by construction. Codes and remedies:
`.claude/skills/audit/references/cli-invocation.md` §4.1.

---

## Smoke test

```bash
scripts/with-jcodemunch-serve.sh --port 8901 -- \
    bash scripts/smoke-jcodemunch-serve.sh --repo local/reify-4ae45bbd
```

`--repo` is **not** optional there. The wrapper's serve answers for the per-path index; the
default husk would clear assertion 1 and then fail assertion 2 with an empty result. The
script's own connection-failure hint prints this exact recipe — it is the canonical wording.

Exits 0 when all three assertions pass:

1. MCP handshake at `http://127.0.0.1:8901/mcp` returns HTTP 200 + a JSON-RPC body.
2. `get_changed_symbols` for `local/reify-4ae45bbd` returns non-empty `changed_symbols`.
3. `jcodemunch-watcher.service` is concurrently active — a watcher-write + serve-read
   concurrency check against the shared SQLite store, confirmed non-fatal. It is a
   concurrency assertion about the store, **not** a claim that the watcher indexes reify.
