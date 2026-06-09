# FEA Multi-Load Baseline Screenshots

This directory holds the golden-master PNG baselines for the three per-case
FEA visual-regression scenarios added by task 3026:

| File                | Load case   |
|---------------------|-------------|
| `operating.png`     | operating   |
| `overload.png`      | overload    |
| `transport.png`     | transport   |

## Capturing baselines

Baselines are **out-of-headless-gate** artifacts. They must be captured with a
live GUI build and are then committed so the `npm run test:visual` pixel-diff
(≤ 1 % mismatch, `mismatchPctLimit=0.01`) can run regression checks.

Run from the repository root on a host with a display (or via `Xvfb`):

```bash
# 1. Build the GUI (release or debug)
scripts/run-gui.sh examples/fea_multi_case_bracket.ri &
# … or use scripts/run-gui-dev.sh for a debug build

# 2. Capture all scenarios (including the three fea-multi-load ones)
UPDATE_BASELINES=1 npm --prefix gui run test:visual
```

The harness writes:
- `gui/test/screenshots/fea-multi-load/operating.png`
- `gui/test/screenshots/fea-multi-load/overload.png`
- `gui/test/screenshots/fea-multi-load/transport.png`

Commit the three PNGs once captured.

## Why the baselines aren't present yet

The task-3026 implementation agent ran in a headless worktree without a
release binary, so the live GUI could not be spawned. The headless gate
(steps 1–20) is fully green (137 test files, 3520 tests). The baseline
PNG capture is the only remaining deliverable.

See: `gui/test/visual/scenarios.ts` (scenario catalogue)
     `gui/test/visual/run.ts`       (harness — case selection via `set_fea_case` debug-MCP)
     `gui/src-tauri/src/debug_server.rs` (`set_fea_case` tool handler)
