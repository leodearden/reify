#!/usr/bin/env node
/**
 * Acceptance smoke for task-4892 (surface-finish-functional ε): B9 MCP gate.
 *
 * LIVE-ONLY — NOT verify/CI-gated.  Requires a running reify-gui launched with
 * REIFY_DEBUG=1 and the committed examples/surface_finish_viewport.ri open.
 * The deterministic gate (Rust backstops + examples_smoke) is the CI green signal.
 * This smoke is the B9 user-observable deliverable but is WAIVABLE when blocked
 * solely on the debug-port gap (esc-4202-61,
 * Leo's standing policy: feedback_waive_gui_smoke_blocked_on_debug_port).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_surface_finish_viewport_e2e.mjs
 *
 * Drives the running GUI debug server over JSON-RPC and asserts the B9 boundary
 * rows from docs/prds/v0_6/surface-finish-functional.md §9 via the per-mesh
 * viewport_state meshInfo[].material probe:
 *
 *   Boot  — open_file(examples/surface_finish_viewport.ri) → wait_for_idle;
 *            activeFile contains 'surface_finish_viewport'.
 *
 *   B9-P  — PolishedSteelBody mesh (entityPath prefix 'PolishedSteelBody#realization['):
 *            material.roughness ≤ ~0.2 (Polished → Gloss, low roughness vs the
 *            colorForEntity hash) AND material.metalness ≈ 0.90 (editorial
 *            Steel_AISI_1045; proves functional Layer 2 is active).
 *
 *   B9-A  — AnodizedBody mesh (entityPath prefix 'AnodizedBody#realization['):
 *            material.color all-channels dark (≈ [0.055, 0.055, 0.063], each < ~0.2);
 *            proves functional Layer 1 (coating_appearance RAL9005) is active.
 *
 *   B9-O  — OverriddenBody mesh (entityPath prefix 'OverriddenBody#realization['):
 *            material.color ≈ [0.96, 0.96, 0.95] (bright override beats the
 *            functional anodize-dark — decision 6: DisplayOutput.style sits above the
 *            functional layer in the frontend meshManager compose order).
 *
 *   B5    — best-effort: session recolor not easily triggered headlessly; covered by
 *            meshManager vitest backstop (mirror #4775 B5).
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import { describeRpcFailure, openFileWithRetry } from './smokeDriverGuards.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURE = path.join(REPO_ROOT, 'examples', 'surface_finish_viewport.ri');

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

// The tools/call request shape and the envelope decode live in rpcEnvelope.mjs
// (CI-covered by rpcEnvelope.test.ts) rather than inline here, because this file
// can never run in CI. `rpc()` now reports BOTH failure dialects as the ONE
// in-band shape `{error: "<msg>"}`: frontend-mediated tools (viewport_state, ...
// via query_frontend) already answer in-band that way, and Rust-dispatched
// `isError: true` + plain-text `Error: <msg>` blocks are folded into it. The
// `typeof vpState !== 'object'` guard below — this file's own idiom, and the
// in-repo precedent the sibling drivers were hardened to — already handled the
// bare-string case, so only the payload this driver prints changes. A top-level
// envelope error is a TRANSPORT failure and still throws, so waitForServer keeps
// polling.
const rpc = makeDebugRpc(DEBUG_URL);

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
  console.log(`smoke_surface_finish_viewport_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURE: ${FIXTURE}`);

  // ── Health ──────────────────────────────────────────────────────────────────
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // ── Boot: open surface_finish_viewport.ri (with retry for WebView init) ────
  // The retry budget, the `.ok` verdict and the failure wording live in
  // ./smokeDriverGuards.mjs, where vitest can cover them; this file needs a live
  // GUI, so nothing inline here ever could be. `fail` is passed (not `log`) so an
  // exhausted retry exits 1 naming the underlying RPC error.
  log('Opening examples/surface_finish_viewport.ri via open_file (with retry for WebView init)…');
  await openFileWithRetry(rpc, FIXTURE, { fail });

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  // Boot: activeFile must contain 'surface_finish_viewport'
  const storeAfterOpen = await rpc('store_state');
  // The optional chain below reads straight past an in-band `{error: '<msg>'}`
  // and reports `activeFile: undefined` — a store_state OUTAGE misreported as
  // the wrong file being open. See ./smokeDriverGuards.mjs (describeRpcFailure)
  // for why a truthy error payload defeats a falsy guard.
  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, 'store_state (post-open)');
  if (storeAfterOpenFailure) fail(storeAfterOpenFailure);
  if (!storeAfterOpen?.editor?.activeFile?.includes('surface_finish_viewport')) {
    fail(`Expected activeFile to contain 'surface_finish_viewport', got: ${storeAfterOpen?.editor?.activeFile}`);
  }
  console.log('  OK: surface_finish_viewport.ri is active');

  // ── Collect meshInfo via viewport_state ──────────────────────────────────────

  log('Collecting viewport_state for B9 material-state probes…');
  const vpState = await rpc('viewport_state', { viewportId: 'design-main' });
  if (!vpState || typeof vpState !== 'object' || 'error' in vpState) {
    fail(`viewport_state('design-main') failed: ${JSON.stringify(vpState)}`);
  }

  const meshInfo = vpState.meshInfo ?? [];
  console.log('  meshInfo count:', meshInfo.length);
  console.log(
    '  entity paths:',
    meshInfo.map(m => m.entityPath),
  );

  if (meshInfo.length < 3) {
    fail(`Expected ≥3 meshes (PolishedSteelBody + AnodizedBody + OverriddenBody); got ${meshInfo.length}`);
  }

  // Find the three B9 meshes.
  const polishedInfo = meshInfo.find(m =>
    typeof m.entityPath === 'string' &&
    m.entityPath.startsWith('PolishedSteelBody#realization['),
  );
  const anodizedInfo = meshInfo.find(m =>
    typeof m.entityPath === 'string' &&
    m.entityPath.startsWith('AnodizedBody#realization['),
  );
  const overriddenInfo = meshInfo.find(m =>
    typeof m.entityPath === 'string' &&
    m.entityPath.startsWith('OverriddenBody#realization['),
  );

  if (!polishedInfo) {
    fail(
      `Expected a PolishedSteelBody mesh; got: ${JSON.stringify(meshInfo.map(m => m.entityPath))}`,
    );
  }
  if (!anodizedInfo) {
    fail(
      `Expected an AnodizedBody mesh; got: ${JSON.stringify(meshInfo.map(m => m.entityPath))}`,
    );
  }
  if (!overriddenInfo) {
    fail(
      `Expected an OverriddenBody mesh; got: ${JSON.stringify(meshInfo.map(m => m.entityPath))}`,
    );
  }

  console.log('  PolishedSteelBody entityPath:', polishedInfo.entityPath);
  console.log('  AnodizedBody entityPath:', anodizedInfo.entityPath);
  console.log('  OverriddenBody entityPath:', overriddenInfo.entityPath);

  // ════════════════════════════════════════════════════════════════════════════
  // B9-P: PolishedSteelBody — Gloss/low-roughness + high metalness (Layer 2)
  // ════════════════════════════════════════════════════════════════════════════

  log('B9-P: PolishedSteelBody has Gloss finish (low roughness) + editorial metalness ≈ 0.90…');
  const polishedMat = polishedInfo.material ?? null;
  console.log('  PolishedSteelBody material probe:', JSON.stringify(polishedMat));

  if (!polishedMat) {
    fail('B9-P: PolishedSteelBody must carry a material-state probe; got null/undefined');
  }

  // B9-P roughness: Polished → Gloss / roughness ≤ ~0.2 (functional Layer 2 lowers from 0.40).
  // Absence of roughness means the functional material layer is absent — hard fail.
  const polishedRoughness = polishedMat.roughness ?? null;
  if (polishedRoughness == null) {
    fail(
      'B9-P: PolishedSteelBody material.roughness is null/undefined — ' +
      'the mesh must carry a functional PBR material from finish_modulation Layer 2',
    );
  }
  if (polishedRoughness > 0.2) {
    fail(
      `B9-P: PolishedSteelBody roughness expected ≤ 0.2 (Polished/Gloss); ` +
      `got ${polishedRoughness} — functional finish_modulation Layer 2 may not be active`,
    );
  }
  console.log(`  OK B9-P: roughness=${polishedRoughness.toFixed(3)} ≤ 0.2 (Gloss finish)`);

  // B9-P metalness: Steel_AISI_1045 editorial metalness 0.90 preserved through Layer 2.
  // Absence of metalness means the editorial material wiring is absent — hard fail.
  const polishedMetalness = polishedMat.metalness ?? null;
  if (polishedMetalness == null) {
    fail(
      'B9-P: PolishedSteelBody material.metalness is null/undefined — ' +
      'the mesh must carry a functional PBR material from the steel editorial appearance',
    );
  }
  if (polishedMetalness < 0.7) {
    fail(
      `B9-P: PolishedSteelBody metalness expected ≈ 0.90 (editorial Steel_AISI_1045); ` +
      `got ${polishedMetalness} — material wiring may not be active`,
    );
  }
  console.log(`  OK B9-P: metalness=${polishedMetalness.toFixed(3)} ≈ 0.90 (editorial steel wiring active)`);

  // ════════════════════════════════════════════════════════════════════════════
  // B9-A: AnodizedBody — dark color (RAL9005 ≈ 0.055 per channel; Layer 1)
  // ════════════════════════════════════════════════════════════════════════════

  log('B9-A: AnodizedBody has dark RAL9005 color (≈ 0.055 per channel; Layer 1)…');
  const anodizedMat = anodizedInfo.material ?? null;
  console.log('  AnodizedBody material probe:', JSON.stringify(anodizedMat));

  if (!anodizedMat) {
    fail('B9-A: AnodizedBody must carry a material-state probe; got null/undefined');
  }

  // B9-A color: RAL9005 jet black → each channel < ~0.2 (≈ 14/255 ≈ 0.055)
  const [ar, ag, ab] = anodizedMat.color ?? [1, 1, 1];
  const DARK_THRESHOLD = 0.2;
  if (ar > DARK_THRESHOLD || ag > DARK_THRESHOLD || ab > DARK_THRESHOLD) {
    fail(
      `B9-A: AnodizedBody color expected dark (all channels < ${DARK_THRESHOLD}); ` +
      `got [${ar.toFixed(3)}, ${ag.toFixed(3)}, ${ab.toFixed(3)}] — ` +
      `coating_appearance RAL9005 Layer 1 may not be active`,
    );
  }
  console.log(`  OK B9-A: color=[${ar.toFixed(3)},${ag.toFixed(3)},${ab.toFixed(3)}] dark (RAL9005 Layer 1 active)`);

  // ════════════════════════════════════════════════════════════════════════════
  // B9-O: OverriddenBody — bright override color ≈ [0.96, 0.96, 0.95] beats functional dark
  // ════════════════════════════════════════════════════════════════════════════

  log('B9-O: OverriddenBody shows bright RAL9016 override ≈ [0.96, 0.96, 0.95] (decision 6)…');
  const overriddenMat = overriddenInfo.material ?? null;
  console.log('  OverriddenBody material probe:', JSON.stringify(overriddenMat));

  if (!overriddenMat) {
    fail('B9-O: OverriddenBody must carry a material-state probe; got null/undefined');
  }

  // B9-O color: DisplayStyle override RAL9016 ≈ [0.96, 0.96, 0.95] (bright white)
  // This beats the functional anodize-dark (≈ 0.055 per channel) — decision 6.
  const [or_, og, ob_] = overriddenMat.color ?? [0, 0, 0];
  const B9O_EPS = 0.1;
  if (or_ < (0.96 - B9O_EPS)) {
    fail(
      `B9-O: OverriddenBody color[r] expected ≈ 0.96 (RAL9016 override); ` +
      `got ${or_.toFixed(3)} — DisplayStyle override (decision 6) may not be composing above the functional layer`,
    );
  }
  if (og < (0.96 - B9O_EPS)) {
    fail(
      `B9-O: OverriddenBody color[g] expected ≈ 0.96 (RAL9016 override); ` +
      `got ${og.toFixed(3)}`,
    );
  }
  if (ob_ < (0.95 - B9O_EPS)) {
    fail(
      `B9-O: OverriddenBody color[b] expected ≈ 0.95 (RAL9016 override); ` +
      `got ${ob_.toFixed(3)}`,
    );
  }
  console.log(
    `  OK B9-O: color=[${or_.toFixed(3)},${og.toFixed(3)},${ob_.toFixed(3)}] ` +
    `≈ RAL9016 bright (override beats functional anodize-dark; decision 6 proven)`,
  );

  // ════════════════════════════════════════════════════════════════════════════
  // B5: best-effort — session recolor not easily triggered headlessly
  // ════════════════════════════════════════════════════════════════════════════

  log('B5 (best-effort): colorize session-layer check (covered by meshManager vitest)…');
  // Layer4 (session colorize) requires an active FEA result on a body.
  // This smoke does not include a FEA solve step — B5 is covered by the
  // meshManager.test.ts B5 vitest consumer backstop (mirror of #4775 B5 pattern).
  console.log('  SKIP B5: colorize-active test not applicable to this fixture (no FEA result)');
  console.log('  B5 covered by meshManager.test.ts deterministic vitest gate.');

  // ════════════════════════════════════════════════════════════════════════════

  console.log('\n=== SMOKE PASS: smoke_surface_finish_viewport_e2e all B9 scenarios PASSED ===');
  process.exit(0);
}

main().catch(err => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
