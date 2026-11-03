#!/usr/bin/env node
/**
 * Acceptance smoke for task 9731 (alpha-spine γ): the edges-in-black toggle,
 * PRD B6.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 (`./run_edges_toggle_e2e_smoke.sh` does the launch). Every
 * decision it makes lives in `./edgesToggleGate.mjs`, CI-covered by
 * `./edgesToggleGate.test.ts`. WAIVABLE when blocked solely on the debug-port
 * gap (esc-4202-61).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_edges_toggle_e2e.mjs
 *
 * Environment:
 *   REIFY_EDGES_SMOKE_FIXTURE    repo-relative .ri to open (default examples/m5_geometry.ri)
 *   REIFY_EDGES_SMOKE_SHOTS_DIR  where the three PNGs are written (default: a fresh temp dir)
 *
 * Flow: open the fixture → wait_for_idle → normalise edges to off → screenshot
 * design-main (edges-off.png) → click the toolbar Edges toggle → wait for
 * viewport.edges true → read store_state → screenshot (edges-on.png) → click
 * again → wait for viewport.edges false → read store_state → screenshot
 * (edges-off-again.png) → grade. The third shot must match the first, so the
 * on/off difference is the overlay's and not drift between the shots. Every
 * PNG is kept and its path always printed, so a failing run can be inspected.
 *
 * Exit 0 on pass, 1 on any graded failure, 2 on an unexpected error.
 */

import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import { describeRpcFailure, openFileWithRetry } from './smokeDriverGuards.mjs';
import { countChangedPixels, formatFailures, gradeEdgesToggle } from './edgesToggleGate.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURE_RELPATH = process.env.REIFY_EDGES_SMOKE_FIXTURE ?? 'examples/m5_geometry.ri';
const FIXTURE = path.join(REPO_ROOT, FIXTURE_RELPATH);
const VIEWPORT_ID = 'design-main';
const TOGGLE_TEST_ID = 'toolbar-edges-toggle';

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

// Request shape and envelope decode live in rpcEnvelope.mjs (CI-covered).
const rpc = makeDebugRpc(DEBUG_URL);

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
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

/** Call a tool and fail the run, naming the tool, on any RPC-level failure. */
async function rpcOrFail(method, args, label) {
  const reply = await rpc(method, args);
  const failure = describeRpcFailure(reply, label);
  if (failure) fail(failure);
  return reply;
}

async function readEdges(label) {
  const store = await rpcOrFail('store_state', {}, label);
  return store?.viewport?.edges;
}

async function clickToggleAndWait(edges) {
  await rpcOrFail('click_element', { testId: TOGGLE_TEST_ID }, `click_element('${TOGGLE_TEST_ID}')`);
  // Not fatal: a wait that times out leaves the flag unflipped, which the
  // store_state read that follows hands to gradeEdgesToggle to report.
  const waited = await rpc('wait_for', {
    predicate: { kind: 'store', path: 'viewport.edges', equals: edges },
    timeout_ms: 5000,
  });
  console.log(`  wait_for(viewport.edges === ${edges}):`, JSON.stringify(waited));
}

async function screenshotTo(savePath) {
  await rpcOrFail(
    'screenshot',
    { viewportId: VIEWPORT_ID, save_path: savePath },
    `screenshot('${VIEWPORT_ID}' → ${savePath})`,
  );
  const png = PNG.sync.read(fs.readFileSync(savePath));
  return { width: png.width, height: png.height, data: new Uint8Array(png.data) };
}

// ─── Main ────────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_edges_toggle_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURE: ${FIXTURE}`);

  log('Waiting for debug server…');
  await waitForServer(60_000);

  log(`Opening ${FIXTURE_RELPATH} via openFileWithRetry…`);
  await openFileWithRetry(rpc, FIXTURE, { fail });

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 30000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  const storeAfterOpen = await rpc('store_state');
  // Diagnose the RPC before reading past an in-band error — see ./smokeDriverGuards.mjs.
  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, 'store_state (post-open)');
  if (storeAfterOpenFailure) fail(storeAfterOpenFailure);
  const fixtureName = path.basename(FIXTURE_RELPATH);
  if (!storeAfterOpen?.editor?.activeFile?.includes(fixtureName)) {
    fail(`Expected activeFile to contain '${fixtureName}', got: ${storeAfterOpen?.editor?.activeFile}`);
  }
  if (storeAfterOpen.viewport?.edges === true) {
    log('Edges were persisted ON from an earlier session — toggling off to start from a known state');
    await clickToggleAndWait(false);
  }

  const shotsDir =
    process.env.REIFY_EDGES_SMOKE_SHOTS_DIR ?? fs.mkdtempSync(path.join(os.tmpdir(), 'reify-edges-smoke-'));
  fs.mkdirSync(shotsDir, { recursive: true });
  const offPath = path.join(shotsDir, 'edges-off.png');
  const onPath = path.join(shotsDir, 'edges-on.png');
  const restoredPath = path.join(shotsDir, 'edges-off-again.png');

  log(`Screenshot with edges off → ${offPath}`);
  const offShot = await screenshotTo(offPath);

  log('Toggling edges on via the toolbar…');
  await clickToggleAndWait(true);
  const edgesAfterOn = await readEdges('store_state (after toggling on)');

  log(`Screenshot with edges on → ${onPath}`);
  const onShot = await screenshotTo(onPath);

  log('Toggling edges back off via the toolbar…');
  await clickToggleAndWait(false);
  const edgesAfterOff = await readEdges('store_state (after toggling off)');

  log(`Screenshot with edges off again → ${restoredPath}`);
  const restoredShot = await screenshotTo(restoredPath);

  console.log(`\nedges-off screenshot:       ${offPath}`);
  console.log(`edges-on screenshot:        ${onPath}`);
  console.log(`edges-off-again screenshot: ${restoredPath}`);
  console.log(`pixels changed off→on: ${countChangedPixels(offShot, onShot)}; off→off-again: ${countChangedPixels(offShot, restoredShot)}`);

  const failures = gradeEdgesToggle({ edgesAfterOn, edgesAfterOff, offShot, onShot, restoredShot });
  if (failures.length > 0) fail(`edges toggle gate:\n${formatFailures(failures)}`);
  console.log('\nPASS: edges toggle flips store_state, visibly changes the design viewport, and restores it');
}

main().catch((err) => {
  console.error(`\nERROR: ${err?.stack ?? err}`);
  process.exit(2);
});
