#!/usr/bin/env node
/**
 * e2e cross-layer parity smoke for task 5367: three-way mesh-count parity.
 *
 * NOT a visual-regression gate, despite living beside the ones in this
 * directory: it captures no screenshot and diffs no baseline (that is
 * gui/test/visual/run.ts). Its ONLY assertion is the numeric one below, taken
 * across three layers that a headless test cannot reach at once.
 *
 * It enforces, end to end, the invariant `./meshCountParity.mjs` decides
 * (tasks 5367, 6498): the viewport's mesh count equals each debug read's
 * `default_visible === true` count, and each read's hidden count clears its
 * per-fixture floor. Rationale and the legitimate breakers:
 * `docs/debug-mcp-contract.md`, "Full realized scene".
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 (real webview + OCCT). The deterministic CI backstops are:
 *   * gui/test/visual/meshCountParity.test.ts — this smoke's whole decision
 *     function (parity, the floors, the vacuity gate, payload extraction) as
 *     pure data.
 *   * gui/src-tauri/src/tests/mesh_count_parity_fixture_tests.rs — the exact
 *     rendered/hidden composition of every FIXTURES row, through the same two
 *     debug projections this smoke reads, and that each row's floors are those
 *     counts.
 * Everything below is thin glue around that already-covered logic.
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_mesh_count_parity_e2e.mjs
 * or, self-launching:
 *   npm --prefix gui run test:smoke:mesh-count-parity
 *
 * THE VACUITY TRAP — why the selective-demand poll is a hard precondition, not a
 * log line: under `full_scope == true`, `build_gui_state` and
 * `build_gui_state_full_scene` agree BY CONSTRUCTION, so all three counts match
 * trivially and a green run would prove nothing — it would NOT have caught the
 * 5348 regression. A run that never reaches selective demand FAILS; it is never
 * skipped or soft-passed. `demand_dispatch` is the only debug tool exposing the
 * scope flag. Opening each fixture is a cold load that resets the scope to full,
 * which is why the poll runs per fixture.
 *
 * THREE THINGS THIS DRIVER AND ITS FIXTURES MUST NOT DO (why: the contract doc
 * above, except where noted):
 *   * Toggle visibility. Nothing needs hiding either: `set_demand_selective`
 *     builds a fresh DemandRegistry with `full_scope: false`, so the
 *     load-triggered sync already yields the state under test.
 *   * Edit source before the parity read. A cold `eval()` resets `full_scope`
 *     back to `true` and would re-trip the precondition.
 *   * Carry a DisplayOutput.
 *
 * Exit 0 on all-pass, non-zero on any failure.
 */

import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  checkMeshCountParity,
  extractMeshCountInputs,
  formatFailures,
} from './meshCountParity.mjs';
import { isInBandError, makeDebugRpc } from './rpcEnvelope.mjs';
import { describeRpcFailure, openFileWithRetry } from './smokeDriverGuards.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
const FIXTURES_DIR = path.join(REPO_ROOT, 'gui', 'test', 'fixtures');

/**
 * The fixtures this run opens, in order, each with its non-vacuity floors: the
 * fixture's KNOWN rendered and hidden body counts. The table lives in
 * ./meshCountParityFixtures.json so that
 * gui/src-tauri/src/tests/mesh_count_parity_fixture_tests.rs can hold every row
 * to its CI-pinned composition; a JSON row with no pin there reds.
 *
 * Why not the module default floor (MESH_COUNT_PARITY_MIN_RENDERED_BODIES = 2)?
 * That generic floor only catches a TOTAL load failure. A PARTIAL load — say 2
 * to 6 of large_assembly's 7 bodies, because tessellation was still catching up
 * — clears it, and if all three layers agree on the same truncated scene the run
 * reports PASS. Pinning the floors to each fixture's own counts closes that
 * window. They stay FLOORS, not equalities, so a fixture that legitimately grows
 * still passes.
 */
const FIXTURES = Object.freeze(
  JSON.parse(fs.readFileSync(path.join(__dirname, 'meshCountParityFixtures.json'), 'utf8')).map(
    row => Object.freeze(row),
  ),
);

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

// TWO FAILURE DIALECTS, one normalised shape — and both the fold that unifies
// them and the tools/call request shape live in rpcEnvelope.mjs (CI-covered by
// rpcEnvelope.test.ts) rather than inline here, because this file can never run
// in CI. Frontend-mediated tools (viewport_state, via query_frontend) report
// failure in-band as JSON `{error: "<msg>"}`; Rust-dispatched tools — all THREE
// this smoke leans on hardest (engine_state, mesh_stats, demand_dispatch) —
// instead answer with `isError: true` plus a plain-text `Error: <msg>` block
// (debug_server.rs). Normalising the latter into the former routes both through
// the one `isInBandError` check below. A top-level envelope error is a TRANSPORT
// failure and still throws, so waitForServer's catch keeps polling.
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

/**
 * Sorted unique entity-path set, for the symmetric-difference triage dump.
 */
function pathSet(list, key) {
  return [...new Set((list ?? []).map(e => e?.[key]).filter(p => typeof p === 'string'))].sort();
}

/**
 * Triage aid for a parity failure: a bare number mismatch says nothing about
 * WHICH bodies went missing. The symmetric difference between what the viewport
 * draws and what mesh_stats calls rendered does, and the hidden list makes an
 * aux misclassification visible at a glance.
 */
function dumpParityTriage(viewportState, meshStats) {
  const meshes = Array.isArray(meshStats?.meshes) ? meshStats.meshes : [];
  const viewportPaths = pathSet(viewportState?.meshInfo, 'entityPath');
  const renderedPaths = pathSet(meshes.filter(m => m?.default_visible === true), 'entity_path');
  const hiddenPaths = pathSet(meshes.filter(m => m?.default_visible === false), 'entity_path');
  console.error('\n  viewport_state.meshInfo entityPaths:            ', JSON.stringify(viewportPaths));
  console.error('  mesh_stats entity_paths, default_visible true: ', JSON.stringify(renderedPaths));
  console.error('  mesh_stats entity_paths, default_visible false:', JSON.stringify(hiddenPaths));
  console.error(
    '  only in viewport_state:            ',
    JSON.stringify(viewportPaths.filter(p => !renderedPaths.includes(p))),
  );
  console.error(
    '  only in mesh_stats (rendered half):',
    JSON.stringify(renderedPaths.filter(p => !viewportPaths.includes(p))),
  );
}

// ─── Main ────────────────────────────────────────────────────────────────────────

/**
 * One fixture's whole lifecycle: open, settle, confirm it is active, reach
 * selective demand, read, extract, check. Exits the process on any failure,
 * naming the fixture.
 */
async function checkFixture({ file, minRenderedBodies, minHiddenBodies }) {
  const fixturePath = path.join(FIXTURES_DIR, file);
  const name = path.basename(file, '.ri');

  // The retry budget, the `.ok` verdict and the failure wording live in
  // ./smokeDriverGuards.mjs, where vitest can cover them; this file needs a live
  // GUI, so nothing inline here ever could be. `fail` is passed (not `log`) so an
  // exhausted retry exits 1 naming the underlying RPC error.
  log(`[${file}] Opening via open_file (with retry for WebView init)…`);
  await openFileWithRetry(rpc, fixturePath, { fail });

  log(`[${file}] Waiting for engine idle…`);
  const idleResult = await rpc('wait_for_idle', { timeout_ms: 15000 });
  console.log('  wait_for_idle result:', JSON.stringify(idleResult));
  // ASSERT the readiness verdict — do not merely log it. wait_for_idle returns
  // `{ok: true, idle_after_ms}` once evalStatus settles, and an in-band
  // `{error: 'timeout' | 'engine_phase' | 'engine_not_started'}` otherwise.
  // Reading the mesh counts while the engine is still tessellating yields a
  // partially-populated viewport, which the parity gate below would report as a
  // loud 5348-class regression — a flaky false failure charged to the wrong
  // subsystem. Name it as the readiness timeout it is, here.
  if (!idleResult || idleResult.ok !== true) {
    fail(
      `[${file}] wait_for_idle did not reach idle within 15000ms: ${JSON.stringify(idleResult)}. ` +
        `This is a READINESS TIMEOUT, not a parity violation — the engine was still ` +
        `evaluating/tessellating, so the three mesh counts below would have been read ` +
        `mid-flight against a partially-populated viewport. Retry, or raise the idle budget, ` +
        `before suspecting a cross-layer regression.`,
    );
  }

  // activeFile must contain the fixture's name — proves the intended model is
  // loaded, not a leftover from the previous fixture or an earlier run.
  const storeAfterOpen = await rpc('store_state');
  // Diagnose the RPC before the optional chain reads past an in-band error —
  // see ./smokeDriverGuards.mjs (describeRpcFailure).
  const storeAfterOpenFailure = describeRpcFailure(storeAfterOpen, `store_state (post-open ${file})`);
  if (storeAfterOpenFailure) fail(storeAfterOpenFailure);
  if (!storeAfterOpen?.editor?.activeFile?.includes(name)) {
    fail(`Expected activeFile to contain '${name}', got: ${storeAfterOpen?.editor?.activeFile}`);
  }
  console.log(`  OK: ${file} is active`);

  // ── PRECONDITION: demand must actually be SELECTIVE ─────────────────────────
  // The frontend's load-triggered selective-demand sync is debounced
  // (SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS = 150) and then crosses IPC, so poll
  // rather than sleeping a fixed interval. Without this the whole run is
  // vacuous — see the header.
  log(`[${file}] Waiting for selective demand (demand_dispatch.full_scope === false)…`);
  let demandDispatch = null;
  for (let attempt = 1; attempt <= 10; attempt++) {
    demandDispatch = await rpc('demand_dispatch');
    // A FAILED demand_dispatch is a tool outage, not a reading about the scope.
    // Interpreting it as `full_scope !== false` would abort below with the
    // vacuity diagnosis — blaming the frontend for never calling sync_demand
    // when in fact the precondition could not be read at all. rpc() normalises
    // the Rust `isError` dialect into this same in-band shape, so one check
    // covers both.
    if (isInBandError(demandDispatch)) {
      fail(
        `[${file}] demand_dispatch itself failed: ${JSON.stringify(demandDispatch.error)}. The ` +
          `selectivity precondition could not be READ at all — this is a debug-tool / engine ` +
          `outage, NOT evidence that demand stayed full-scope, and not a parity result.`,
      );
    }
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
      `[${file}] demand never became selective: demand_dispatch.full_scope is ` +
        `${JSON.stringify(demandDispatch?.full_scope)} after ~5s of polling. This run would be ` +
        `VACUOUS — under full scope build_gui_state and build_gui_state_full_scene agree by ` +
        `construction, so three-way mesh-count parity is trivially satisfied and proves nothing. ` +
        `Either the frontend never called sync_demand, or a cold eval() reset the scope back to ` +
        `full. Last payload: ${JSON.stringify(demandDispatch)}`,
    );
  }
  console.log('  OK: demand is selective — the parity assertion below is meaningful');

  // ── The three reads ─────────────────────────────────────────────────────────
  log(`[${file}] Reading viewport_state, mesh_stats and engine_state…`);
  const viewportState = await rpc('viewport_state', { viewportId: 'design-main' });
  const meshStats = await rpc('mesh_stats');
  const engineState = await rpc('engine_state');

  // rpc() above only NORMALISES the two failure dialects into one shape; it
  // throws on transport errors and otherwise hands the payload back verbatim.
  // Actually DETECTING the in-band `{error: ...}` (docs/debug-mcp-contract.md
  // §2a) and naming the tool that failed is extractMeshCountInputs' job.
  const { inputs, failures: extractionFailures } = extractMeshCountInputs({
    viewportState,
    meshStats,
    engineState,
    demandDispatch,
  });
  console.log('  extracted:', JSON.stringify(inputs));

  // A failed READ is not a failed INVARIANT — and must not be reported as one.
  // When a tool answers with an in-band error, extraction names the outage and
  // leaves that read's fields `undefined`; running the parity checker over them
  // would then append further, misleading messages under a headline blaming a
  // 5348-class cross-layer regression. That is precisely the misdiagnosis
  // meshCountParity.mjs exists to prevent, so short-circuit: parity is simply
  // NOT EVALUATED when the inputs could not be read.
  if (extractionFailures.length > 0) {
    // `gate` is data, so the headline can name what actually went wrong: a tool
    // that FAILED (outage, §2a) reads differently from one that answered with
    // something unreadable (shape) — different first move for whoever triages.
    const outages = extractionFailures.filter(f => f.gate === 'outage').map(f => f.tool);
    const kind = outages.length > 0 ? `debug-MCP tool outage (${outages.join(', ')})` : 'debug-MCP read';
    fail(
      `[${file}] ${kind} — parity NOT evaluated:\n  - ` +
        `${formatFailures(extractionFailures).join('\n  - ')}\n` +
        `These are tool/engine-level read failures (docs/debug-mcp-contract.md §2a), NOT ` +
        `evidence of a mesh-count regression: the invariant was never tested. Restore the ` +
        `failing read, then re-run.`,
    );
  }

  const parity = checkMeshCountParity({ ...inputs, minRenderedBodies, minHiddenBodies });

  if (!parity.ok) {
    dumpParityTriage(viewportState, meshStats);
    fail(
      `[${file}] mesh-count parity violated under selective demand:\n  - ` +
        `${formatFailures(parity.failures).join('\n  - ')}`,
    );
  }

  console.log(
    `  OK [${file}]: viewport_state.meshCount === rendered(mesh_stats) === ` +
      `rendered(engine_state) === ${inputs.viewportMeshCount}; hidden: ` +
      `mesh_stats ${inputs.meshStatsHidden}, engine_state ${inputs.engineStateHidden}`,
  );
}

async function main() {
  console.log(`smoke_mesh_count_parity_e2e: targeting debug server at ${DEBUG_URL}`);
  console.log(`FIXTURES: ${FIXTURES.map(f => f.file).join(', ')} (in ${FIXTURES_DIR})`);

  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  for (const fixture of FIXTURES) {
    await checkFixture(fixture);
  }

  // NO screenshot step, deliberately. One used to sit here labelled "capturing
  // the visual-regression record", and it recorded nothing: `screenshot` answers
  // with an image content block, normalizeRpcEnvelope maps that to null, and the
  // result was discarded — no PNG written, no baseline compared (that is
  // gui/test/visual/run.ts's job, with its own SCREENSHOTS_DIR and differ). A
  // step that only LOOKS like it captures evidence is worse than none: it lends
  // this smoke a visual-regression framing its single numeric assertion does not
  // earn. Re-adding one means persisting and diffing it, or not at all.

  console.log(
    `\n=== SMOKE PASS: smoke_mesh_count_parity_e2e — default_visible parity PASSED on ` +
      `${FIXTURES.map(f => f.file).join(', ')} ===`,
  );
  process.exit(0);
}

main().catch(err => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
