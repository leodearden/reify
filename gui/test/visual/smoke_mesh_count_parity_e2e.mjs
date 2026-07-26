#!/usr/bin/env node
/**
 * e2e visual-regression smoke for task 5367: cross-layer mesh-count parity.
 *
 * Follow-up from task 5348, which routed `engine_state` and `mesh_stats` through
 * `build_gui_state_full_scene` so both report the whole realized scene rather
 * than only the currently-demanded subset. `docs/debug-mcp-contract.md` asserts
 * that the two stay consistent with `viewport_state.meshCount`; this smoke is
 * the gate that ENFORCES it end-to-end:
 *
 *   viewport_state.meshCount === mesh_stats.meshes.length
 *                            === engine_state.meshes.length
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 (real webview + OCCT). The deterministic CI backstops are:
 *   * gui/test/visual/meshCountParity.test.ts — this smoke's whole decision
 *     function (parity, both vacuity gates, payload extraction) as pure data.
 *   * 5348's full-scene tests in gui/src-tauri/src/commands_tests.rs.
 * Everything below is thin glue around that already-covered logic.
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_mesh_count_parity_e2e.mjs
 * or, self-launching:
 *   npm --prefix gui run test:smoke:mesh-count-parity
 *
 * THE VACUITY TRAP — why step 5 below is a hard precondition, not a log line:
 * under `full_scope == true`, `build_gui_state` and `build_gui_state_full_scene`
 * agree BY CONSTRUCTION, so all three counts match trivially and a green run
 * would prove nothing — it would NOT have caught the 5348 regression. A run that
 * never reaches selective demand FAILS; it is never skipped or soft-passed.
 * `demand_dispatch` is the only debug tool exposing the scope flag.
 *
 * TWO THINGS THIS DRIVER MUST NOT DO:
 *   * Toggle visibility. `viewport_state.meshCount` counts
 *     `meshManager.getSceneMeshes()`, which includes only `show`-state meshes
 *     (ghost and hidden are excluded), while engine_state/mesh_stats report the
 *     full realized scene — hiding a body would break parity legitimately and
 *     produce a false failure. Nothing needs hiding: `set_demand_selective`
 *     builds a fresh DemandRegistry with `full_scope: false`, so the ALL-visible
 *     load-triggered sync already yields the state under test.
 *   * Edit source before the parity read. A cold `eval()` resets `full_scope`
 *     back to `true` and would re-trip the precondition.
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkMeshCountParity, extractMeshCountInputs } from './meshCountParity.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURE = path.join(REPO_ROOT, 'gui', 'test', 'fixtures', 'large_assembly.ri');

// ─── Port resolution (mirrors endpoint.ts / lib_portable.sh logic) ─────────────

function resolveDebugPort(env = process.env) {
  const raw = env['REIFY_DEBUG_PORT'];
  if (raw === undefined) return 3939;
  if (!/^\d+$/.test(raw)) return 3939;
  const parsed = parseInt(raw, 10);
  if (parsed < 1 || parsed > 65535) return 3939;
  return parsed;
}

const PORT = resolveDebugPort();
const DEBUG_URL = `http://127.0.0.1:${PORT}/mcp`;

// ─── Helpers ────────────────────────────────────────────────────────────────────

let stepNum = 0;
function log(msg) {
  stepNum++;
  console.log(`[step ${stepNum}] ${msg}`);
}
function fail(msg) {
  console.error(`\nFAIL: ${msg}`);
  process.exit(1);
}

async function rpc(method, args = {}) {
  const res = await fetch(DEBUG_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name: method, arguments: args },
    }),
  });
  const envelope = await res.json();
  if (envelope.error) throw new Error(`RPC error: ${JSON.stringify(envelope.error)}`);
  const content = envelope?.result?.content;
  if (!content || content.length === 0) return null;
  const textBlock = content.find(c => c.type === 'text');
  if (!textBlock) return null;
  try {
    return JSON.parse(textBlock.text);
  } catch {
    return textBlock.text;
  }
}

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

async function waitForServer(timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await rpc('health');
      if (r !== null) return;
    } catch {}
    await sleep(500);
  }
  fail(`Debug server not ready on port ${PORT} after ${timeoutMs}ms`);
}

/**
 * Sorted unique entity-path set, for the symmetric-difference triage dump.
 */
function pathSet(list, key) {
  return [...new Set((list ?? []).map(e => e?.[key]).filter(p => typeof p === 'string'))].sort();
}

// ─── Main ────────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_mesh_count_parity_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURE: ${FIXTURE}`);

  // ── Health ──────────────────────────────────────────────────────────────────
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // ── Boot: open large_assembly.ri (with retry for WebView init) ──────────────
  // The debug MCP server comes up before the WebKit WebView finishes loading,
  // so a single open_file races. Retry up to 8 times (≤45s).
  log('Opening gui/test/fixtures/large_assembly.ri via open_file (with retry for WebView init)…');
  let openResult = null;
  for (let attempt = 1; attempt <= 8; attempt++) {
    openResult = await rpc('open_file', { path: FIXTURE });
    console.log(`  open_file attempt ${attempt} result:`, JSON.stringify(openResult));
    if (openResult && openResult.ok) break;
    if (attempt < 8) {
      console.log('  Retrying in 3s (WebView still initialising)…');
      await sleep(3000);
    }
  }
  if (!openResult || !openResult.ok) {
    fail(`open_file failed after retries: ${JSON.stringify(openResult)}`);
  }

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  // Boot: activeFile must contain 'large_assembly' — proves the intended model
  // is loaded, not a leftover from a previous run against the same window.
  const storeAfterOpen = await rpc('store_state');
  if (!storeAfterOpen?.editor?.activeFile?.includes('large_assembly')) {
    fail(
      `Expected activeFile to contain 'large_assembly', got: ${storeAfterOpen?.editor?.activeFile}`,
    );
  }
  console.log('  OK: large_assembly.ri is active');

  // ── PRECONDITION: demand must actually be SELECTIVE ─────────────────────────
  // The frontend's load-triggered selective-demand sync is debounced
  // (SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS = 150) and then crosses IPC, so poll
  // rather than sleeping a fixed interval. Without this the whole run is
  // vacuous — see the header.
  log('Waiting for selective demand (demand_dispatch.full_scope === false)…');
  let demandDispatch = null;
  for (let attempt = 1; attempt <= 10; attempt++) {
    demandDispatch = await rpc('demand_dispatch');
    const scope = demandDispatch?.full_scope;
    const evalSetLen = Array.isArray(demandDispatch?.eval_set)
      ? demandDispatch.eval_set.length
      : 'n/a';
    console.log(`  attempt ${attempt}: full_scope=${JSON.stringify(scope)} eval_set=${evalSetLen}`);
    if (scope === false) break;
    if (attempt < 10) await sleep(500);
  }
  if (demandDispatch?.full_scope !== false) {
    fail(
      `demand never became selective: demand_dispatch.full_scope is ` +
        `${JSON.stringify(demandDispatch?.full_scope)} after ~5s of polling. This run would be ` +
        `VACUOUS — under full scope build_gui_state and build_gui_state_full_scene agree by ` +
        `construction, so three-way mesh-count parity is trivially satisfied and proves nothing. ` +
        `Either the frontend never called sync_demand, or a cold eval() reset the scope back to ` +
        `full. Last payload: ${JSON.stringify(demandDispatch)}`,
    );
  }
  console.log('  OK: demand is selective — the parity assertion below is meaningful');

  // ── The three reads ─────────────────────────────────────────────────────────
  log('Reading viewport_state, mesh_stats and engine_state…');
  const viewportState = await rpc('viewport_state', { viewportId: 'design-main' });
  const meshStats = await rpc('mesh_stats');
  const engineState = await rpc('engine_state');

  // extractMeshCountInputs also performs the in-band `{error: ...}` detection
  // (docs/debug-mcp-contract.md §2a) that the inlined rpc() above does NOT — it
  // only throws on transport errors and returns the payload verbatim.
  const { inputs, failures: extractionFailures } = extractMeshCountInputs({
    viewportState,
    meshStats,
    engineState,
    demandDispatch,
  });
  console.log('  extracted:', JSON.stringify(inputs));

  const parity = checkMeshCountParity(inputs);
  const allFailures = [...extractionFailures, ...parity.failures];

  if (allFailures.length > 0) {
    // Triage aid: a bare number mismatch says nothing about WHICH bodies went
    // missing. The symmetric difference of the two entity-path sets does.
    const viewportPaths = pathSet(viewportState?.meshInfo, 'entityPath');
    const statsPaths = pathSet(meshStats?.meshes, 'entity_path');
    const onlyViewport = viewportPaths.filter(p => !statsPaths.includes(p));
    const onlyStats = statsPaths.filter(p => !viewportPaths.includes(p));
    console.error('\n  viewport_state.meshInfo entityPaths:', JSON.stringify(viewportPaths));
    console.error('  mesh_stats.meshes entity_paths:      ', JSON.stringify(statsPaths));
    console.error('  only in viewport_state:', JSON.stringify(onlyViewport));
    console.error('  only in mesh_stats:    ', JSON.stringify(onlyStats));
    fail(`mesh-count parity violated under selective demand:\n  - ${allFailures.join('\n  - ')}`);
  }

  console.log(
    `  OK: viewport_state.meshCount === mesh_stats.meshes.length === ` +
      `engine_state.meshes.length === ${inputs.viewportMeshCount}`,
  );

  // ── Visual-regression record ────────────────────────────────────────────────
  // Best-effort: the parity verdict is already decided above, and `screenshot`
  // returns an image content block that the inlined rpc() reports as null.
  log('Capturing screenshot for the visual-regression record…');
  try {
    await rpc('screenshot', { viewportId: 'design-main' });
    console.log('  OK: screenshot captured');
  } catch (err) {
    console.log(`  (screenshot unavailable, not fatal: ${err})`);
  }

  console.log('\n=== SMOKE PASS: smoke_mesh_count_parity_e2e — three-way parity PASSED ===');
  process.exit(0);
}

main().catch(err => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
