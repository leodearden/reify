# jcodemunch-serve activation runbook

**Status (2026-05-30):** Active. `jcodemunch-serve.service` is enabled and running.

Design: `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §8 (L-SERVE, leaf 1)

---

## Overview

`jcodemunch-serve.service` runs a persistent jcodemunch streamable-HTTP QUERY server
on `127.0.0.1:8901`. It is distinct from `jcodemunch-watcher.service`, which only
keeps the on-disk index fresh and does NOT serve queries. Both share the same
schema-v16 on-disk index at `~/.code-index` (dark-factory parity: one persistent
watcher + one persistent serve off a single index).

---

## Resolved identifiers (spike output — 2026-05-30)

| Field | Value |
|---|---|
| **Repo identifier** | `leodearden-reify` |
| **Storage path** | `~/.code-index` (default; from `~/.code-index/config.jsonc`) |
| **Shared DB file** | `~/.code-index/leodearden-reify.db` |
| **Serve version** | v1.108.27 (matching watcher pin) |
| **Index schema** | v16 |
| **Commit range (smoke)** | resolved in step-4; see smoke script `COMMIT_FROM`/`COMMIT_TO` |

These were resolved live against the running serve during the L-SERVE spike (task 4102).

---

## Systemd unit

- **Committed unit:** `deploy/systemd/jcodemunch-serve.service`
- **Installed symlink:** `/home/leo/.config/systemd/user/jcodemunch-serve.service`

```
ExecStart=/home/leo/.local/bin/uvx \
  --python 3.12 \
  --from "jcodemunch-mcp @ git+https://github.com/jgravelle/jcodemunch-mcp.git@v1.108.27" \
  jcodemunch-mcp serve \
  --transport streamable-http \
  --host 127.0.0.1 \
  --port 8901 \
  --watcher=false
```

`--watcher=false`: the serve answers queries only. `jcodemunch-watcher.service` owns indexing.

Pin rationale: PyPI `jcodemunch-mcp` was quarantined (admin review #308, 2026-05-30);
pinned to git source matching the watcher unit to avoid version/schema skew.

---

## Smoke test

```
bash scripts/smoke-jcodemunch-serve.sh
```

Exits 0 when all three assertions pass:
1. MCP handshake at `http://127.0.0.1:8901/mcp` returns HTTP 200 + JSON-RPC body.
2. `get_changed_symbols` for `leodearden-reify` returns non-empty changed_symbols.
3. `jcodemunch-watcher.service` is concurrently active (watcher-write + serve-read non-fatal).

---

## Enable / reload commands

```bash
# Install (after task lands on main, symlink to canonical path):
ln -sf /home/leo/src/reify/deploy/systemd/jcodemunch-serve.service \
    /home/leo/.config/systemd/user/jcodemunch-serve.service
systemctl --user daemon-reload
systemctl --user enable --now jcodemunch-serve.service

# Reload after unit file changes:
systemctl --user daemon-reload
systemctl --user restart jcodemunch-serve.service

# Status / logs:
systemctl --user status jcodemunch-serve.service
journalctl --user -u jcodemunch-serve.service -f
```

---

## Concurrency note (dark-factory parity)

`jcodemunch-watcher.service` and `jcodemunch-serve.service` both read `~/.code-index`.
The watcher writes index deltas while the serve answers queries on the same SQLite database.
This is the dark-factory model: one persistent watcher (indexing only) + one persistent
serve (queries only) sharing a single on-disk index. Confirmed non-fatal during the L-SERVE
spike — assertion 3 of the smoke test verifies this concurrently on every run.

---

## Operator action required

**After the task lands on main**, update the symlink to point at the canonical path:

```bash
ln -sf /home/leo/src/reify/deploy/systemd/jcodemunch-serve.service \
    /home/leo/.config/systemd/user/jcodemunch-serve.service
systemctl --user daemon-reload
systemctl --user restart jcodemunch-serve.service
```

The current symlink points at the worktree path
(`/media/leo/data_lv_1/leo/reify-build/worktrees/4102/deploy/systemd/jcodemunch-serve.service`)
and will break when the worktree is cleaned up.

---

## Procedural memory

Entry keyed `jcodemunch-serve streamable-HTTP activation` in fused-memory memory store:
repo-id = `leodearden-reify`, storage_path = `~/.code-index`, port = 8901,
serve unit = `deploy/systemd/jcodemunch-serve.service`, pin = v1.108.27.
