#!/usr/bin/env node
/**
 * e2e integration gate for task 5098: an AI agent lengthens printer_v01's Y
 * rails through `reify_set_parameter`, and the GUI keeps up.
 *
 * PRD docs/prds/v0_6/ai-native-editing.md §7 leaf ζ. Two edits, in order:
 *
 *   1. `CoreXY.y_rail_len`  800mm -> 1100mm — the rails overrun the frame, and
 *      the rail-span pin flips VIOLATED.
 *   2. `AFrame.rail_span_m` 800mm -> 1100mm — the frame catches up, the pin
 *      returns to SATISFIED, `AFrame.travel_avail` moves 510mm -> 810mm, and the
 *      ToolDock's pinned `yh_min_today` literal is knocked out of range, which
 *      is a SECOND expected flip rather than a defect.
 *
 * NOT A VISUAL-REGRESSION GATE, despite the directory: no screenshot is captured
 * and no baseline diffed (that is gui/test/visual/run.ts).
 *
 * EVERY PASS/FAIL DECISION LIVES IN `./railLengtheningGate.mjs`, never here.
 * This file is transport and sequencing: it opens the subject, reads payloads,
 * hands them to `extractGateInputs`/`checkRailLengtheningGate`, and prints
 * `formatFailures`. That split is what gives the gate a CI signal at all —
 * `./railLengtheningGate.test.ts` exercises the whole decision function as pure
 * data, on every verify run, while this file can only ever run live.
 *
 * IT DRIVES A COPY, NOT THE TRACKED FILE. `reify_set_parameter` rewrites the
 * `.ri` SOURCE ON DISK, which is the point of the B2 assertion — so the subject
 * genuinely changes. Mutating the tracked design would leave a dirty tree on any
 * crash, make the gate non-idempotent, and put a half-restored engineering
 * design one interrupted run away. Instead the whole `prj/printer_v01/` directory
 * is copied to a `mkdtemp` dir and removed in a `finally`. Copying the DIRECTORY
 * (not just the one file) keeps `dev_capstan.ri` and `tools/` in place, and the
 * filename must stay `printer.ri` because the module path is filename-derived.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 (real webview + OCCT).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_rail_lengthening_e2e.mjs
 * or, self-launching:
 *   npm --prefix gui run test:smoke:rail-lengthening
 *
 * Exit 0 on all-pass, 1 on an asserted failure, 2 on an unexpected throw.
 */

import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  PRINTER_RELPATH,
  SUBJECT_BASENAME,
  checkRailLengtheningGate,
  extractGateInputs,
  formatFailures,
} from './railLengtheningGate.mjs';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import { describeRpcFailure, openFileWithRetry } from './smokeDriverGuards.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');
/** The tracked original. Never opened, never written — only copied from. */
const SUBJECT_DIR = path.dirname(path.join(REPO_ROOT, PRINTER_RELPATH));

/**
 * How long to let the engine settle after an open or an edit.
 *
 * printer_v01 is a whole machine, not a fixture: reading `engine_state` while it
 * is still tessellating yields a partially-populated snapshot, which the gate's
 * non-vacuity floor would report as a failed load. That would be a READINESS
 * TIMEOUT charged to the wrong subsystem.
 */
const IDLE_TIMEOUT_MS = 120_000;

// ─── Port resolution (mirrors endpoint.ts / lib_portable.sh logic) ────────────
// Inline rather than imported: a bare-`node` driver cannot load endpoint.ts.

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

// TWO FAILURE DIALECTS, one normalised shape — the fold that unifies them and
// the `tools/call` request shape both live in ./rpcEnvelope.mjs (CI-covered by
// rpcEnvelope.test.ts) rather than inline here, because this file can never run
// in CI. A top-level envelope error is a TRANSPORT failure and still throws, so
// waitForServer's catch keeps polling.
const rpc = makeDebugRpc(DEBUG_URL);

// ─── Helpers ─────────────────────────────────────────────────────────────────

let stepNum = 0;
function log(msg) {
  stepNum++;
  console.log(`[step ${stepNum}] ${msg}`);
}
function fail(msg) {
  console.error(`\nFAIL: ${msg}`);
  process.exit(1);
}
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

/**
 * Wait for the engine to stop evaluating, and ASSERT the verdict.
 *
 * `wait_for_idle` answers `{ok: true, idle_after_ms}` or an in-band
 * `{error: 'timeout' | 'engine_phase' | 'engine_not_started'}`. Logging it
 * without asserting is how a mid-flight read becomes a "the model failed to
 * load" verdict — see IDLE_TIMEOUT_MS.
 */
async function waitForIdle(what) {
  const result = await rpc('wait_for_idle', { timeout_ms: IDLE_TIMEOUT_MS });
  console.log(`  wait_for_idle (${what}):`, JSON.stringify(result));
  if (!result || result.ok !== true) {
    fail(
      `wait_for_idle did not reach idle within ${IDLE_TIMEOUT_MS}ms after ${what}: ` +
        `${JSON.stringify(result)}. This is a READINESS TIMEOUT, not a gate violation — the ` +
        `engine was still evaluating, so every reading below would have been taken mid-flight. ` +
        `Retry, or raise IDLE_TIMEOUT_MS, before suspecting a value-flow regression.`,
    );
  }
}

/**
 * Copy the tracked `prj/printer_v01/` into a fresh temp directory and return the
 * absolute path of the COPY's `printer.ri`.
 *
 * The caller removes the directory in a `finally`; see this file's header for
 * why the tracked design is never the subject.
 */
function copySubject() {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'reify-rail-lengthening-'));
  fs.cpSync(SUBJECT_DIR, path.join(work, path.basename(SUBJECT_DIR)), { recursive: true });
  return { work, subject: path.join(work, path.basename(SUBJECT_DIR), SUBJECT_BASENAME) };
}

/**
 * Abort on a failed READ, naming it as the outage it is.
 *
 * A tool that failed means the phase was never TESTED; reporting it as a gate
 * violation is the most expensive misattribution this driver can produce. `null`
 * — the healthy answer from `describeRpcFailure` — passes straight through.
 */
function requireLiveRead(phase, diagnosis) {
  if (diagnosis === null || diagnosis === undefined) return;
  fail(
    `${diagnosis}\n  Phase '${phase}' was NEVER TESTED: this is a debug-MCP tool outage ` +
      `(docs/debug-mcp-contract.md §2a), not a value-flow or constraint-sync violation.`,
  );
}

/**
 * Read one observation of the live state and grade it against `phase`.
 *
 * READ ORDER IS LOAD-BEARING. `reify_open_file` RE-OPENS the file from disk, so
 * it must come LAST: the three reads before it observe state the engine already
 * held, which is what makes "the geometry followed the edit WITHOUT a reload" an
 * assertion rather than a tautology. Moving the open earlier would silently turn
 * every phase into a reload test.
 *
 * @returns {Promise<{inputs: object, verdict: {ok: boolean, failures: object[]}}>}
 */
async function gradePhase(phase, subject) {
  const engineState = await rpc('engine_state');
  const storeAfterOpen = await rpc('store_state');
  const demandDispatch = await rpc('demand_dispatch');
  const openFile = await rpc('reify_open_file', { file_path: subject });

  // Diagnose each read BEFORE anything optional-chains into it: an in-band
  // `{error: '<msg>'}` envelope is TRUTHY, so `storeAfterOpen?.editor?.activeFile`
  // sails past an outage to `undefined` and the run blames the frontend for what
  // was a tool failure (./smokeDriverGuards.mjs). Spelled out one call per
  // payload rather than looped, so each diagnosis names its own tool and the
  // `describeRpcFailure(storeAfterOpen, …)` pairing ./smokeDriverConventions.ts
  // checks for is visible where a reader — and its regex — expects it.
  requireLiveRead(phase, describeRpcFailure(engineState, 'engine_state'));
  requireLiveRead(phase, describeRpcFailure(storeAfterOpen, 'store_state (post-open)'));
  requireLiveRead(phase, describeRpcFailure(demandDispatch, 'demand_dispatch'));
  requireLiveRead(phase, describeRpcFailure(openFile, 'reify_open_file'));
  console.log(`  active file: ${JSON.stringify(storeAfterOpen?.editor?.activeFile)}`);

  const { inputs, failures: extractionFailures } = extractGateInputs({
    phase,
    engineState,
    storeState: storeAfterOpen,
    openFile,
    demandDispatch,
  });

  // A failed READ is not a failed INVARIANT. Extraction names the outage and
  // leaves the field undefined; grading that would append a second, misleading
  // message under a headline blaming the AI write path.
  if (extractionFailures.length > 0) {
    const outages = extractionFailures.filter((f) => f.gate === 'outage').map((f) => f.tool);
    const kind = outages.length > 0 ? `debug-MCP tool outage (${outages.join(', ')})` : 'debug-MCP read';
    fail(
      `${kind} — phase '${phase}' NOT evaluated:\n  - ` +
        `${formatFailures(extractionFailures).join('\n  - ')}\n` +
        `The invariant was never tested. Restore the failing read, then re-run.`,
    );
  }

  const verdict = checkRailLengtheningGate(inputs);
  console.log(`  graded '${phase}': ok=${verdict.ok}`);
  return { inputs, verdict };
}

/** Report a phase verdict, failing the run when it did not hold. */
function requirePhase(phase, verdict) {
  if (verdict.ok) {
    console.log(`  OK: phase '${phase}' holds`);
    return;
  }
  fail(
    `phase '${phase}' violated:\n  - ${formatFailures(verdict.failures).join('\n  - ')}`,
  );
}

// ─── Main ────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_rail_lengthening_e2e: targeting debug server at ${DEBUG_URL}`);

  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  const { work, subject } = copySubject();
  console.log(`  driving a COPY: ${subject}`);
  try {
    // The retry budget, the `.ok` verdict and the failure wording live in
    // ./smokeDriverGuards.mjs, where vitest can cover them; a direct
    // rpc('open_file', …) here would re-implement all three and is a hard
    // violation of ./smokeDriverConventions.ts.
    log(`Opening the copy via open_file (with retry for WebView init)…`);
    const opened = await openFileWithRetry(rpc, subject, { fail });
    // `.includes`, not equality: `open_path_into_engine` canonicalizes, which
    // resolves /tmp's symlink, so the path that comes back is not the one that
    // went in.
    if (!String(opened?.path ?? '').includes(SUBJECT_BASENAME)) {
      fail(`open_file returned a path that is not the subject: ${JSON.stringify(opened)}`);
    }

    log('Waiting for the engine to finish realizing printer_v01…');
    await waitForIdle('open');

    log('Grading the baseline…');
    const baseline = await gradePhase('baseline', subject);
    requirePhase('baseline', baseline.verdict);
  } finally {
    // Removed whatever happened above, so a crashed run leaves no half-edited
    // copy of an engineering design behind and the next run starts clean.
    fs.rmSync(work, { recursive: true, force: true });
  }

  console.log('\n=== SMOKE PASS: smoke_rail_lengthening_e2e ===');
  process.exit(0);
}

main().catch((err) => {
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
