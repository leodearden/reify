#!/usr/bin/env node
/**
 * Acceptance smoke for task-4769 (multi-pane viewport ζ): end-to-end MCP integration gate.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 and the committed examples/multi_pane_viewport.ri open.
 * Per the H0 harness contract and the diagnostics/find-uses precedent, this
 * smoke is run manually / via the npm script "test:smoke:multipane" which
 * executes run_multi_pane_e2e_smoke.sh (builds + launches the GUI then drives it).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_multi_pane_e2e.mjs
 *
 * Drives the running GUI debug server over JSON-RPC and asserts the §8 boundary
 * rows from docs/prds/v0_6/multi-pane-viewport.md:
 *
 *   Row 1/boot  — open_file(examples/multi_pane_viewport.ri) → wait_for_idle;
 *                 activeFile contains 'multi_pane_viewport'.
 *   Row 1/2     — store_state().viewports enumerates ≥2 entries including 'pane-1'
 *                 with meshCount>=1 (α's exposure; pane count == ≥2 design panes).
 *   Row 1/3     — viewport_state('pane-1').meshCount==2 AND all entityPaths ∈
 *                 engine.meshKeys (many-to-one db/dc → pane 1; inv.1 join-key).
 *   Row 1 (both ways) — viewport_state('design-main').meshCount>=1, all entityPaths
 *                 ∈ engine.meshKeys (join-key holds both directions).
 *   Row 7       — store_state().viewports has NO 'pane-2' key (dx dangling-dropped;
 *                 no phantom pane from the unresolved `param undetermined : Solid`).
 *   Row 5       — orbit_camera('pane-1', dazimuth=0.5) produces azimuthDelta>0;
 *                 viewport_state('design-main').camera.position is unchanged
 *                 (independent per-pane cameras).
 *   Row 6       — best-effort: reload persistence (guard with try/skip; ε already
 *                 owns deterministic persistence coverage).
 *
 * NOTE: entity_path string values are matched by presence-in-meshKeys rather than
 * hard-equality to stay robust to realization-index formatting.
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURE = path.join(REPO_ROOT, 'examples', 'multi_pane_viewport.ri');

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

// ─── Main ────────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_multi_pane_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURE: ${FIXTURE}`);

  // ── Health ──────────────────────────────────────────────────────────────────
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // ── Boot: open multi_pane_viewport.ri (with retry for WebView init) ─────────
  // The debug MCP server comes up before the WebKit WebView finishes loading.
  // Retry open_file up to 8 times (≤45s) to give the WebView time to complete
  // its startup sequence. Mirrors the approach in smoke_diagnostics_e2e.mjs.
  log('Opening examples/multi_pane_viewport.ri via open_file (with retry for WebView init)…');
  let openResult = null;
  for (let attempt = 1; attempt <= 8; attempt++) {
    openResult = await rpc('open_file', { path: FIXTURE });
    console.log(`  open_file attempt ${attempt} result:`, JSON.stringify(openResult));
    if (openResult && openResult.ok) break;
    if (attempt < 8) {
      console.log(`  Retrying in 3s (WebView still initialising)…`);
      await sleep(3000);
    }
  }
  if (!openResult || !openResult.ok) fail(`open_file failed after retries: ${JSON.stringify(openResult)}`);

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  // Row 1/boot: activeFile must contain 'multi_pane_viewport'
  const storeAfterOpen = await rpc('store_state');
  if (!storeAfterOpen?.editor?.activeFile?.includes('multi_pane_viewport')) {
    fail(`Expected activeFile to contain 'multi_pane_viewport', got: ${storeAfterOpen?.editor?.activeFile}`);
  }
  console.log('  OK: multi_pane_viewport.ri is active');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 1: VIEWPORT MAP — store_state shows ≥2 viewports incl pane-1
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 1: store_state viewport map (rows 1/2)…');
  const storeState = await rpc('store_state');
  if (!storeState) fail('store_state returned null');

  const viewports = storeState.viewports ?? {};
  const viewportIds = Object.keys(viewports);
  console.log('  viewport IDs:', viewportIds);
  console.log('  engine meshKeys:', storeState.engine?.meshKeys);

  // At least 2 viewports (design-main + pane-1 at minimum)
  if (viewportIds.length < 2) {
    fail(`Expected ≥2 viewports in store_state; got ${viewportIds.length}: ${JSON.stringify(viewportIds)}`);
  }
  console.log('  OK: ≥2 viewports registered');

  // Row 2: pane-1 must be present with meshCount>=1
  if (!('pane-1' in viewports)) {
    fail(`Expected 'pane-1' in store_state.viewports; got: ${JSON.stringify(viewportIds)}`);
  }
  if (typeof viewports['pane-1'].meshCount !== 'number') {
    fail(`Expected pane-1.meshCount to be a number; got ${typeof viewports['pane-1'].meshCount}: ${viewports['pane-1'].meshCount}`);
  }
  if (viewports['pane-1'].meshCount < 1) {
    fail(`Expected pane-1.meshCount>=1; got ${viewports['pane-1'].meshCount}`);
  }
  console.log(`  OK: pane-1 present, meshCount=${viewports['pane-1'].meshCount}`);

  // Row 7: pane-2 must NOT be present (dangling dx dropped — no phantom pane)
  log('Scenario 1b: pane-2 must be absent (row 7 dangling-drop)…');
  if ('pane-2' in viewports) {
    fail(`'pane-2' must NOT be in viewports (dx dangling-dropped); got: ${JSON.stringify(viewportIds)}`);
  }
  console.log('  OK: pane-2 is absent (dx dangling-dropped, no phantom pane)');

  // Build the set of all mesh entity paths from the engine
  const allMeshKeys = new Set(storeState.engine?.meshKeys ?? []);
  console.log('  All mesh entity paths:', [...allMeshKeys]);
  if (allMeshKeys.size < 3) {
    fail(`Expected ≥3 realized meshes (a, b, c boxes); got ${allMeshKeys.size}: ${JSON.stringify([...allMeshKeys])}`);
  }
  console.log(`  OK: ${allMeshKeys.size} realized meshes in engine`);

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 2: PANE-1 VIEWPORT — meshCount==2 + inv.1 join-key (rows 1/3)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 2: viewport_state(pane-1) — many-to-one b+c (rows 1/3)…');
  const pane1State = await rpc('viewport_state', { viewportId: 'pane-1' });
  console.log('  pane-1 state:', JSON.stringify({
    meshCount: pane1State?.meshCount,
    meshInfo: pane1State?.meshInfo?.map(m => m.entityPath),
  }));

  if (!pane1State || 'error' in pane1State) {
    fail(`viewport_state('pane-1') failed: ${JSON.stringify(pane1State)}`);
  }

  // meshCount must be exactly 2 (db→b, dc→c; many-to-one, two distinct subjects)
  if (pane1State.meshCount !== 2) {
    fail(`Expected pane-1 meshCount==2 (db/dc many-to-one); got ${pane1State.meshCount}: ${JSON.stringify(pane1State.meshInfo?.map(m => m.entityPath))}`);
  }
  console.log('  OK: pane-1 meshCount==2 (many-to-one db/dc → subjects b and c)');

  // inv.1 join-key: every meshInfo entityPath must be in the engine's full mesh set
  const pane1Paths = (pane1State.meshInfo ?? []).map(m => m.entityPath);
  for (const ep of pane1Paths) {
    if (!allMeshKeys.has(ep)) {
      fail(`pane-1 entityPath '${ep}' not in engine.meshKeys (inv.1 join-key violated); allMeshKeys=${JSON.stringify([...allMeshKeys])}`);
    }
  }
  console.log('  OK: all pane-1 entityPaths ∈ engine.meshKeys (inv.1 join-key holds)');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 3: DESIGN-MAIN VIEWPORT — join-key holds both ways (row 1)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 3: viewport_state(design-main) — join-key both ways (row 1)…');
  const mainState = await rpc('viewport_state', { viewportId: 'design-main' });
  console.log('  design-main state:', JSON.stringify({
    meshCount: mainState?.meshCount,
    meshInfo: mainState?.meshInfo?.map(m => m.entityPath),
  }));

  if (!mainState || 'error' in mainState) {
    fail(`viewport_state('design-main') failed: ${JSON.stringify(mainState)}`);
  }

  // design-main must have ≥1 mesh (contains at least subject a from da + dd)
  if (mainState.meshCount < 1) {
    fail(`Expected design-main meshCount>=1; got ${mainState.meshCount}`);
  }
  console.log(`  OK: design-main meshCount=${mainState.meshCount} (includes subject a from da/dd)`);

  // inv.1 join-key both ways: every meshInfo entityPath must be in engine.meshKeys
  const mainPaths = (mainState.meshInfo ?? []).map(m => m.entityPath);
  for (const ep of mainPaths) {
    if (!allMeshKeys.has(ep)) {
      fail(`design-main entityPath '${ep}' not in engine.meshKeys (inv.1 join-key violated both ways); allMeshKeys=${JSON.stringify([...allMeshKeys])}`);
    }
  }
  console.log('  OK: all design-main entityPaths ∈ engine.meshKeys (inv.1 join-key holds both ways)');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 4: INDEPENDENT CAMERAS — orbit pane-1, design-main unchanged (row 5)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 4: orbit_camera(pane-1) — cameras independent (row 5)…');

  // Capture design-main camera before orbit
  const mainBefore = await rpc('viewport_state', { viewportId: 'design-main' });
  if (!mainBefore || 'error' in mainBefore) {
    fail(`viewport_state('design-main') pre-orbit failed: ${JSON.stringify(mainBefore)}`);
  }
  const mainCamBefore = mainBefore.camera;
  if (!mainCamBefore?.position) {
    fail('design-main camera/position missing before orbit');
  }
  console.log('  design-main camera before orbit:', JSON.stringify(mainCamBefore?.position));

  // Orbit pane-1 by dazimuth=0.5
  const orbitResult = await rpc('orbit_camera', { viewportId: 'pane-1', dazimuth: 0.5 });
  console.log('  orbit_camera(pane-1, dazimuth=0.5) result:', JSON.stringify({
    ok: orbitResult?.ok,
    azimuthDelta: orbitResult?.azimuthDelta,
    camera: orbitResult?.camera,
  }));

  if (!orbitResult || 'error' in orbitResult) {
    fail(`orbit_camera('pane-1') failed: ${JSON.stringify(orbitResult)}`);
  }

  // orbit_camera itself reports azimuthDelta > 0 (camera actually moved)
  if (!(orbitResult.azimuthDelta > 0)) {
    fail(`Expected orbit_camera to produce azimuthDelta>0; got ${orbitResult.azimuthDelta}`);
  }
  console.log(`  OK: pane-1 camera orbited (azimuthDelta=${orbitResult.azimuthDelta.toFixed(4)})`);

  await sleep(100);  // allow any reactive update to settle

  // design-main camera must be UNCHANGED (independent cameras)
  const mainAfter = await rpc('viewport_state', { viewportId: 'design-main' });
  if (!mainAfter || 'error' in mainAfter) {
    fail(`viewport_state('design-main') post-orbit failed: ${JSON.stringify(mainAfter)}`);
  }
  const mainCamAfter = mainAfter.camera;
  if (!mainCamAfter?.position) {
    fail('design-main camera/position missing after orbit');
  }
  console.log('  design-main camera after orbit:', JSON.stringify(mainCamAfter?.position));

  const posEq = (a, b) =>
    Math.abs(a.x - b.x) < 1e-9 &&
    Math.abs(a.y - b.y) < 1e-9 &&
    Math.abs(a.z - b.z) < 1e-9;

  if (!posEq(mainCamBefore.position, mainCamAfter.position)) {
    fail(
      `design-main camera CHANGED after orbit on pane-1 (cameras must be independent); ` +
      `before=${JSON.stringify(mainCamBefore.position)}, after=${JSON.stringify(mainCamAfter.position)}`
    );
  }
  console.log('  OK: design-main camera unchanged after orbit on pane-1 (independent cameras)');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 5: BEST-EFFORT reload persistence (row 6)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 5 (best-effort): reload persistence — sizeWeight persists (row 6)…');
  // ε (task 4768) already owns deterministic persistence coverage. This is a
  // best-effort live check only — skip gracefully if no reload RPC is available.
  try {
    // Check that viewport layout storage key is populated (ε contract)
    const lsResult = await rpc('get_local_storage', { key: 'reify-viewport-layout' });
    if (lsResult?.present) {
      console.log('  OK (best-effort): reify-viewport-layout key present in localStorage');
      console.log('  layout value:', lsResult.value?.substring?.(0, 200));
    } else {
      console.log('  SKIP: reify-viewport-layout not yet written (OK — best-effort only)');
    }
  } catch (e) {
    console.log(`  SKIP: persistence check threw (OK — best-effort only): ${e.message}`);
  }

  // ════════════════════════════════════════════════════════════════════════════

  console.log('\n=== SMOKE PASS: smoke_multi_pane_e2e all scenarios PASSED ===');
  process.exit(0);
}

main().catch(err => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
