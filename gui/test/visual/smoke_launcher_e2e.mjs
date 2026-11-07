#!/usr/bin/env node
/**
 * Acceptance smoke for task 9730 (alpha-spine β): the launcher Start view,
 * PRD B1–B5.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a reify-gui launched with NO file
 * and REIFY_DEBUG=1 (`./run_launcher_e2e_smoke.sh` and
 * `./run_launcher_new_file_e2e_smoke.sh` do the launch). Every decision it makes
 * lives in `./launcherGate.mjs`, CI-covered by `./launcherGate.test.ts`.
 * WAIVABLE when blocked solely on the debug-port gap (esc-4202-61).
 *
 * The GTK file dialogs cannot be driven over the debug MCP, so the Start view is
 * driven through its path field (`start-path-input`) and its Open / Create
 * buttons, which run the same actions as the dialog buttons. No `open_file`.
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> REIFY_LAUNCHER_SMOKE_SCENARIO=open|new \
 *     node gui/test/visual/smoke_launcher_e2e.mjs
 *
 * Scenarios (REIFY_LAUNCHER_SMOKE_SCENARIO, default `open`):
 *   open  B1 the Start view at launch → B5 Debug mode on this port, unchanged by
 *         Design then Debug twice → B4 a missing path fails E_GUI_OPEN_FAILED and
 *         keeps the Start view → B2 examples/m5_geometry.ri opens, renders, is
 *         first in Recent and takes the Start view down.
 *   new   B1 → B3 Create `launcher_new.ri` in a fresh temp dir: on disk with
 *         `module launcher_new` first, no compile errors, renders, loaded.
 *
 * Every observation is printed. Exit 0 on pass, 1 on any graded failure, 2 on
 * an unexpected error.
 */

import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import { debugUrlForPort, resolveDebugPort, waitForServer } from './debugServerEndpoint.mjs';
import { describeRpcFailure } from './smokeDriverGuards.mjs';
import { formatFailures, gradeNewFileScenario, gradeOpenScenario } from './launcherGate.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const OPEN_FIXTURE = path.join(REPO_ROOT, 'examples', 'm5_geometry.ri');
const NEW_STEM = 'launcher_new';
const SCENARIO = process.env.REIFY_LAUNCHER_SMOKE_SCENARIO ?? 'open';

const PORT = resolveDebugPort(process.env);
const DEBUG_URL = debugUrlForPort(PORT);

/** How long the frontend half of the bridge may take to answer after health does. */
const FRONTEND_READY_TIMEOUT_MS = 60_000;
/** A second Debug selection has no observable completion; give its set_mode this long. */
const RESELECT_SETTLE_MS = 1_000;

// ─── Helpers ────────────────────────────────────────────────────────────────────

let stepNum = 0;
function log(msg) {
  stepNum++;
  console.log(`[step ${stepNum}] ${msg}`);
}

/** An infrastructure failure, not a graded one: main() turns it into exit 2. */
function fail(msg) {
  throw new Error(msg);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Request shape and envelope decode live in rpcEnvelope.mjs (CI-covered).
const rpc = makeDebugRpc(DEBUG_URL);

/** Call a tool and fail the run, naming the tool, on any RPC-level failure. */
async function rpcOrFail(method, args, label) {
  const reply = await rpc(method, args);
  const failure = describeRpcFailure(reply, label);
  if (failure) fail(failure);
  return reply;
}

const readStore = (label) => rpcOrFail('store_state', {}, `store_state (${label})`);

/** Poll store_state until the frontend half of the debug bridge answers it. */
async function waitForFrontend() {
  const deadline = Date.now() + FRONTEND_READY_TIMEOUT_MS;
  let last = 'no reply';
  while (Date.now() < deadline) {
    const reply = await rpc('store_state', {});
    const failure = describeRpcFailure(reply, 'store_state (frontend readiness)');
    if (!failure) return;
    last = failure;
    await sleep(500);
  }
  fail(`frontend debug bridge not answering after ${FRONTEND_READY_TIMEOUT_MS}ms: ${last}`);
}

async function startViewVisible(label) {
  const probe = await rpcOrFail('dom_query', { testId: 'start-view' }, `dom_query('start-view') (${label})`);
  return probe.exists === true && probe.visible === true;
}

/** Not fatal: a wait that times out is reported by the store read that follows. */
async function waitForStore(storePath, equals, timeoutMs) {
  const waited = await rpc('wait_for', { predicate: { kind: 'store', path: storePath, equals }, timeout_ms: timeoutMs });
  console.log(`  wait_for(${storePath} === ${JSON.stringify(equals)}):`, JSON.stringify(waited));
}

async function click(testId) {
  await rpcOrFail('click_element', { testId }, `click_element('${testId}')`);
}

async function enterPath(filePath) {
  await rpcOrFail(
    'edit_text_input',
    { selector: '[data-testid="start-path-input"]', value: filePath, commit: 'hold' },
    `edit_text_input(start-path-input ← ${filePath})`,
  );
}

async function healthAnswers() {
  try {
    return (await rpc('health')) !== null;
  } catch {
    return false;
  }
}

function report(label, value) {
  console.log(`  ${label}: ${JSON.stringify(value)}`);
  return value;
}

// ─── Phases ──────────────────────────────────────────────────────────────────────

async function observeStartView() {
  log('B1: the no-file launch shows the Start view');
  const store = await readStore('at launch');
  return report('startView', {
    loaded: store.document?.loaded,
    startViewVisible: await startViewVisible('at launch'),
  });
}

async function selectMode(mode) {
  await click(`start-mode-${mode}`);
  await waitForStore('mode.selected', mode, 10_000);
}

const modeOf = (store) => ({ selected: store.mode?.selected, debugPort: store.mode?.debugPort });

async function observeDebugMode() {
  log(`B5: Debug mode on port ${PORT}, idempotent on re-selection`);
  const atLaunch = modeOf(await readStore('mode at launch'));
  await selectMode('design');
  await selectMode('debug');
  const first = modeOf(await readStore('mode after selecting Debug'));
  await click('start-mode-debug');
  await sleep(RESELECT_SETTLE_MS);
  const second = modeOf(await readStore('mode after selecting Debug again'));
  return report('debugMode', { debugPortEnv: PORT, atLaunch, first, second, healthAfter: await healthAnswers() });
}

async function observeFailedOpen() {
  const missing = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'reify-launcher-smoke-')), 'missing.ri');
  log(`B4: opening a missing path (${missing}) fails coded and keeps the Start view`);
  await enterPath(missing);
  await click('start-open-path');
  await waitForStore('document.error.code', 'E_GUI_OPEN_FAILED', 10_000);
  const store = await readStore('after the failed open');
  return report('failedOpen', {
    loaded: store.document?.loaded,
    errorCode: store.document?.error?.code ?? null,
    startViewVisible: await startViewVisible('after the failed open'),
  });
}

/** Wait for the document to load and the engine to settle, then read the result. */
async function observeLoaded(label) {
  await waitForStore('document.loaded', true, 60_000);
  const idle = await rpc('wait_for_idle', { timeout_ms: 30_000 });
  console.log('  wait_for_idle:', JSON.stringify(idle));
  const store = await readStore(label);
  return {
    loaded: store.document?.loaded,
    activeFile: store.editor?.activeFile,
    meshCount: store.engine?.meshCount,
    recentFirstPath: store.document?.recent?.[0]?.path,
    startViewVisible: await startViewVisible(label),
  };
}

async function observeOpened() {
  log(`B2: opening ${OPEN_FIXTURE} through the Start view`);
  await enterPath(OPEN_FIXTURE);
  await click('start-open-path');
  // The Recent entry is recorded only after the load completes, so wait for it
  // (under its canonical path) before reading, or the read races the write.
  await waitForStore('document.recent.0.path', fs.realpathSync(OPEN_FIXTURE), 60_000);
  const loaded = await observeLoaded('after opening the fixture');
  return report('opened', { ...loaded, fixtureName: path.basename(OPEN_FIXTURE) });
}

async function observeCreated() {
  const target = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'reify-launcher-smoke-')), `${NEW_STEM}.ri`);
  log(`B3: creating ${target} through the Start view`);
  await enterPath(target);
  await click('start-create-path');
  const { recentFirstPath: _recent, ...loaded } = await observeLoaded('after creating the design');
  const existsOnDisk = fs.existsSync(target);
  const firstLine = existsOnDisk ? fs.readFileSync(target, 'utf8').split('\n')[0] : null;
  const diagnostics = await rpcOrFail('get_diagnostics', {}, 'get_diagnostics (after creating the design)');
  const compileErrorCount = (diagnostics.compile ?? []).filter((d) => d.severity === 'Error').length;
  return report('created', {
    ...loaded,
    compileErrorCount,
    created: { existsOnDisk, firstLine, stem: NEW_STEM },
  });
}

// ─── Main ────────────────────────────────────────────────────────────────────────

const SCENARIOS = {
  async open() {
    const startView = await observeStartView();
    const debugMode = await observeDebugMode();
    const failedOpen = await observeFailedOpen();
    const opened = await observeOpened();
    return gradeOpenScenario({ startView, debugMode, failedOpen, opened });
  },
  async new() {
    const startView = await observeStartView();
    const created = await observeCreated();
    return gradeNewFileScenario({ startView, ...created });
  },
};

async function main() {
  const scenario = SCENARIOS[SCENARIO];
  if (!scenario) fail(`unknown REIFY_LAUNCHER_SMOKE_SCENARIO ${JSON.stringify(SCENARIO)} (want open or new)`);
  console.log(`smoke_launcher_e2e (${SCENARIO}): targeting debug server at ${DEBUG_URL}`);

  log('Waiting for debug server…');
  await waitForServer(rpc, PORT, { fail });
  log('Waiting for the frontend debug bridge…');
  await waitForFrontend();

  const failures = await scenario();
  if (failures.length > 0) {
    console.error(`\nFAIL: launcher gate (${SCENARIO}):\n${formatFailures(failures)}`);
    process.exit(1);
  }
  console.log(`\nPASS: launcher ${SCENARIO} scenario`);
}

main().catch((err) => {
  console.error(`\nERROR: ${err?.stack ?? err}`);
  process.exit(2);
});
