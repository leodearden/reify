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
 *   #5891       — click_element({testId:'fea-mode-enable-toggle', viewportId:'pane-1'})
 *                 opens ONLY pane-1's FEA body; design-main's stays closed
 *                 (cross-pane no-bleed for the viewportId-scoped testid resolvers).
 *                 Per-pane FEA state is read via `fea-mode-channel-select` PRESENCE,
 *                 not via store_state: the wire exposes no FEA state at all
 *                 (viewports[id] is {meshCount} alone) and dom_query cannot read
 *                 `checked`. Three preconditions keep that reading honest, since
 *                 dom_query answers exists:false for an unresolvable pane exactly as
 *                 it does for a closed body: an UNSCOPED probe must match one toggle
 *                 per registered viewport (else there is nothing to disambiguate),
 *                 design-main's OWN toggle must resolve (else the pane is not there
 *                 to be left untouched), and BOTH panes' bodies must start closed
 *                 (the click is a toggle, so a pre-opened pane would be closed by it).
 *
 * NOTE: entity_path string values are matched by presence-in-meshKeys rather than
 * hard-equality to stay robust to realization-index formatting.
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import {
  describeRpcFailure,
  describeScopedToggleNoBleed,
  describeUnscopedAmbiguity,
  openFileWithRetry,
} from './smokeDriverGuards.mjs';

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

// The tools/call request shape and the envelope decode live in rpcEnvelope.mjs
// (CI-covered by rpcEnvelope.test.ts) rather than inline here, because this file
// can never run in CI. `rpc()` now reports BOTH failure dialects as the ONE
// in-band shape `{error: "<msg>"}`: frontend-mediated tools (viewport_state,
// orbit_camera, ... via query_frontend) already answer in-band that way, and
// Rust-dispatched `isError: true` + plain-text `Error: <msg>` blocks are folded
// into it. That fold is what lets EVERY guard below delegate to
// describeRpcFailure: both dialects arrive as §2a, so one diagnosis covers them.
// Without it a bare string reached a guard that assumed an object, threw a
// TypeError, and killed this driver at exit 2 instead of reporting a clean
// exit-1 failure. A top-level envelope error is a TRANSPORT failure and still
// throws, so waitForServer keeps polling.
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
  console.log(`smoke_multi_pane_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURE: ${FIXTURE}`);

  // ── Health ──────────────────────────────────────────────────────────────────
  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  // ── Boot: open multi_pane_viewport.ri (with retry for WebView init) ─────────
  // The retry budget, the `.ok` verdict and the failure wording live in
  // ./smokeDriverGuards.mjs, where vitest can cover them; this file needs a live
  // GUI, so nothing inline here ever could be. `fail` is passed (not `log`) so an
  // exhausted retry exits 1 naming the underlying RPC error.
  log('Opening examples/multi_pane_viewport.ri via open_file (with retry for WebView init)…');
  await openFileWithRetry(rpc, FIXTURE, { fail });

  log('Waiting for engine idle…');
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));

  // Row 1/boot: activeFile must contain 'multi_pane_viewport'
  const storeAfterOpen = await rpc('store_state');
  // Diagnose the RPC before the optional chain below reads past an in-band
  // `{error: '<msg>'}` and reports `activeFile: undefined` — see
  // ./smokeDriverGuards.mjs for why a truthy error payload defeats a `!x` guard.
  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, 'store_state (post-open)');
  if (storeAfterOpenFailure) fail(storeAfterOpenFailure);
  if (!storeAfterOpen?.editor?.activeFile?.includes('multi_pane_viewport')) {
    fail(`Expected activeFile to contain 'multi_pane_viewport', got: ${storeAfterOpen?.editor?.activeFile}`);
  }
  console.log('  OK: multi_pane_viewport.ri is active');

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 1: VIEWPORT MAP — store_state shows ≥2 viewports incl pane-1
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 1: store_state viewport map (rows 1/2)…');
  const storeState = await rpc('store_state');
  // `if (!storeState)` was not a guard here either: it passed a truthy in-band
  // error through and this scenario reported "got 0 viewports" instead of the RPC
  // error text. describeRpcFailure returns null for a healthy (including empty)
  // object, so the count check below still owns the genuine empty-map case.
  const storeStateFailure = describeRpcFailure(storeState, 'store_state');
  if (storeStateFailure) fail(storeStateFailure);

  const viewports = storeState.viewports ?? {};
  const viewportIds = Object.keys(viewports);
  console.log('  viewport IDs:', viewportIds);
  console.log('  engine meshKeys:', storeState.engine?.meshKeys);

  // At least 2 viewports (design-main + pane-1 at minimum). Now that the guard
  // above catches a failed RPC, this wording is only reachable for a store_state
  // that genuinely answered — so name its top-level keys, which distinguishes
  // "no viewports registered" from "the payload has no viewports field at all".
  if (viewportIds.length < 2) {
    fail(`Expected ≥2 viewports in store_state; got ${viewportIds.length}: ${JSON.stringify(viewportIds)} (store_state keys: ${JSON.stringify(Object.keys(storeState))})`);
  }
  console.log('  OK: ≥2 viewports registered');

  // THE OBSERVED pane count, reused by scenario 5's anti-vacuity probe rather than
  // restated there as a literal. Deriving it means a fixture that grew a third pane
  // surfaces once, at the check above that enumerates panes, instead of again as a
  // downstream count mismatch three scenarios later.
  //
  // THE COUPLING THIS RELIES ON, stated because it is a property of ONE rendering
  // branch and not of the app: "registered viewports" equals "elements carrying
  // fea-mode-enable-toggle" only because App's feaModeStoreRegistry.get() is
  // get-or-create and MultiViewport forwards `feaModeStore` to every pane
  // unconditionally, while Viewport gates the whole toolbar on
  // `<Show when={props.feaModeStore}>`. A viewport that registers itself into
  // __REIFY_DEBUG__.viewports WITHOUT a feaModeStore renders no toolbar and breaks
  // the equality — DualViewport's `def-preview` pane in the single-pane fallback is
  // exactly that shape. It is not mounted in the MultiViewport branch this fixture
  // takes, so the two counts agree today; if that ever changes, the guard below will
  // report a changed element set when what actually changed is the rendering branch.
  const PANE_COUNT = viewportIds.length;

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

  const pane1StateFailure = describeRpcFailure(pane1State, "viewport_state('pane-1')");
  if (pane1StateFailure) fail(pane1StateFailure);

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

  const mainStateFailure = describeRpcFailure(mainState, "viewport_state('design-main')");
  if (mainStateFailure) fail(mainStateFailure);

  // design-main must have ≥1 mesh (contains at least subject a from da + dd)
  //
  // The type check is load-bearing, exactly as it is for pane-1 above: `<` is a
  // ONE-SIDED guard, and `undefined < 1` is FALSE, so a payload that carried an
  // `error` describeRpcFailure does not diagnose — §2a admits only a STRING
  // `error`, so a hypothetical `{error: 500}` folds to "healthy" — would sail
  // past here, leave meshInfo empty (making the inv.1 loop below a no-op) and
  // print `meshCount=undefined` as an OK. A smoke driver reporting green on a
  // failed RPC is the one outcome these guards exist to prevent, so the absence
  // of a number is named before its magnitude is compared.
  if (typeof mainState.meshCount !== 'number') {
    fail(`Expected design-main meshCount to be a number; got ${typeof mainState.meshCount}: ${JSON.stringify(mainState.meshCount)} (viewport_state keys: ${JSON.stringify(Object.keys(mainState))})`);
  }
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
  const mainBeforeFailure = describeRpcFailure(mainBefore, "viewport_state('design-main') pre-orbit");
  if (mainBeforeFailure) fail(mainBeforeFailure);
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

  const orbitResultFailure = describeRpcFailure(orbitResult, "orbit_camera('pane-1')");
  if (orbitResultFailure) fail(orbitResultFailure);

  // orbit_camera itself reports azimuthDelta > 0 (camera actually moved)
  if (!(orbitResult.azimuthDelta > 0)) {
    fail(`Expected orbit_camera to produce azimuthDelta>0; got ${orbitResult.azimuthDelta}`);
  }
  console.log(`  OK: pane-1 camera orbited (azimuthDelta=${orbitResult.azimuthDelta.toFixed(4)})`);

  await sleep(100);  // allow any reactive update to settle

  // design-main camera must be UNCHANGED (independent cameras)
  const mainAfter = await rpc('viewport_state', { viewportId: 'design-main' });
  const mainAfterFailure = describeRpcFailure(mainAfter, "viewport_state('design-main') post-orbit");
  if (mainAfterFailure) fail(mainAfterFailure);
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
  // Scenario 5: CROSS-PANE TESTID SCOPING (#5891) — scoped click drives ONE pane
  // ════════════════════════════════════════════════════════════════════════════
  //
  // The live counterpart of the jsdom pin in gui/src/__tests__/debugBridge.test.tsx
  // ("(a) a viewportId-scoped click_element drives that pane only"). Every verdict
  // below is computed in ./smokeDriverGuards.mjs, where vitest covers its truth
  // table; this file needs a live GUI, so nothing inline here ever could be. What
  // remains here is RPC sequencing.
  //
  // THIS SCENARIO MUST NEVER CLICK 'fea-mode-collapse-toggle'. The per-pane
  // observable is `fea-mode-channel-select` PRESENCE, and FeaModeToolbar gates its
  // body on `!collapsed() && enabled` — collapsing would make the select absent for
  // a reason unrelated to `enabled` and silently invalidate the probe rather than
  // fail it.

  log('Scenario 5: viewportId-scoped click_element drives ONE pane (#5891)…');

  // ANTI-VACUITY PRECONDITION, first: both panes must really carry the toggle.
  // paneDiagnostics reports {viewportId, matchCount} only above one match, so this
  // is what makes the scoped assertions below load-bearing rather than trivially
  // true. UNSCOPED on purpose — no viewportId param.
  const ambiguity = await rpc('dom_query', { testId: 'fea-mode-enable-toggle' });
  console.log('  unscoped dom_query(fea-mode-enable-toggle):', JSON.stringify(ambiguity));
  const ambiguityFailure = describeUnscopedAmbiguity(ambiguity, {
    testId: 'fea-mode-enable-toggle',
    // Derived from the map scenario 1 enumerated, NOT a literal 2, so the two checks
    // cannot disagree about the fixture. See PANE_COUNT for the one-toolbar-per-
    // registered-pane coupling that equality rests on.
    expectedMatchCount: PANE_COUNT,
  });
  // Both guards in this scenario answer `{code, message} | null` — `code` is the
  // stable branch identity the vitest suite asserts on, `message` is the
  // operator-visible prose, and only the latter belongs in a failure report.
  if (ambiguityFailure) fail(ambiguityFailure.message);
  console.log(`  OK: fea-mode-enable-toggle is ambiguous across ${PANE_COUNT} panes (scoping has real work)`);
  // INFORMATIONAL ONLY — never asserted. This names the pane a pre-#5891 unscoped
  // click would have driven, which is useful context, but pane DOM order is
  // emergent from MultiViewport's <For each={props.panes}> and is not a contract.
  console.log(`  (an unscoped click would have driven '${ambiguity.viewportId}')`);

  // ADDRESSABILITY probe — the precondition the unscoped count CANNOT supply. It
  // proves one toggle per registered pane exists SOMEWHERE, not that design-main is
  // one of the panes carrying them, and the line above is explicitly not an
  // assertion (the pane it names is emergent DOM order). Since dom_query
  // collapses an unresolvable viewportId to the same {exists:false} a closed body
  // gives, without this a renamed design-main would answer "closed" to every probe
  // below and the no-bleed verdict would be certified against a pane that is not there.
  const otherToggle = await rpc('dom_query', { testId: 'fea-mode-enable-toggle', viewportId: 'design-main' });
  console.log('  design-main own toggle (addressability):', JSON.stringify(otherToggle));

  // CLEAN-BASELINE probes: BOTH panes' FEA bodies must be closed before the click.
  // The driven pane needs one too — the click is a TOGGLE, so a pane that starts
  // open is CLOSED by it, which would otherwise be misreported as "the click never
  // landed" rather than as the dirty start state it is.
  const otherBefore = await rpc('dom_query', { testId: 'fea-mode-channel-select', viewportId: 'design-main' });
  const drivenBefore = await rpc('dom_query', { testId: 'fea-mode-channel-select', viewportId: 'pane-1' });
  console.log('  design-main FEA body before click:', JSON.stringify(otherBefore));
  console.log('  pane-1 FEA body before click:', JSON.stringify(drivenBefore));

  // THE SCOPED CLICK. A bare {ok:true} while the several candidates counted above
  // exist in the DOM is itself the evidence that viewportId narrowed the resolve to
  // exactly one — paneDiagnostics would have echoed the pair otherwise.
  const feaClick = await rpc('click_element', { testId: 'fea-mode-enable-toggle', viewportId: 'pane-1' });
  console.log('  click_element(fea-mode-enable-toggle, pane-1):', JSON.stringify(feaClick));

  await sleep(100);  // let Solid's reactive update commit, as after orbit_camera above

  // Read BOTH panes after the click — the driven one, then the one that must not have moved.
  const drivenAfter = await rpc('dom_query', { testId: 'fea-mode-channel-select', viewportId: 'pane-1' });
  const otherAfter = await rpc('dom_query', { testId: 'fea-mode-channel-select', viewportId: 'design-main' });
  console.log('  pane-1 FEA body after click:', JSON.stringify(drivenAfter));
  console.log('  design-main FEA body after click:', JSON.stringify(otherAfter));

  const noBleedFailure = describeScopedToggleNoBleed({
    drivenPaneId: 'pane-1',
    otherPaneId: 'design-main',
    toggleTestId: 'fea-mode-enable-toggle',
    bodyTestId: 'fea-mode-channel-select',
    clickResult: feaClick,
    otherToggle,
    otherBefore,
    drivenBefore,
    drivenAfter,
    otherAfter,
  });
  if (noBleedFailure) fail(noBleedFailure.message);
  console.log('  OK: scoped click opened ONLY pane-1 FEA body; design-main unchanged (#5891 no bleed)');

  // RESTORE the baseline. click_element calls el.click(), which is a TOGGLE and
  // not an idempotent set, so leaving pane-1 enabled would leak mutated UI state
  // into the scenario below and into any manual inspection after the run. A restore
  // hiccup is NON-FATAL: it must not turn a genuine PASS into a failure.
  const feaRestore = await rpc('click_element', { testId: 'fea-mode-enable-toggle', viewportId: 'pane-1' });
  const feaRestoreFailure = describeRpcFailure(feaRestore, "click_element('pane-1') FEA restore");
  if (feaRestoreFailure || feaRestore?.ok !== true) {
    console.log(`  NOTE (non-fatal): FEA toggle restore did not report ok — pane-1 may be left enabled: ${feaRestoreFailure ?? JSON.stringify(feaRestore)}`);
  } else {
    console.log('  OK: pane-1 FEA toggle restored to its pre-scenario state');
  }

  // ════════════════════════════════════════════════════════════════════════════
  // Scenario 6: BEST-EFFORT reload persistence (row 6)
  // ════════════════════════════════════════════════════════════════════════════

  log('Scenario 6 (best-effort): reload persistence — sizeWeight persists (row 6)…');
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
