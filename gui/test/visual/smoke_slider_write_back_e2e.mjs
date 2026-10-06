#!/usr/bin/env node
/**
 * e2e gate for task 7680: the real GUI slider and edit box write the `.ri` file
 * on disk, and an Export afterwards carries what was written (INV-GUI-3; the
 * live half of task 5099 η, closing esc-7281-4 and esc-5099-10).
 *
 * The gestures are driven through the controls themselves, via the debug
 * bridge's `scrub_range_input` / `edit_text_input`: they assign `.value` and
 * dispatch only the DOM events the controls bind, so everything from the
 * component's handlers onward — the RAF preview coalescer, the commit, the IPC
 * hop, the engine, the write-back, the exporter — is the production path.
 *
 * Phases, all against a `mkdtemp` COPY of gui/test/fixtures/slider_write_back.ri,
 * whose disk bytes are read with node fs (never `reify_open_file`, which would
 * re-open the file):
 *
 *   BASELINE  the disk holds the default literal; the export's X extent is it.
 *   HOLD      a held slider gesture reaches the engine, and leaves disk alone.
 *   RELEASE   releasing rewrites exactly the default literal's span.
 *   EXPORT    the export carries the released value, not the default.
 *   ENTER     an edit-box Enter commit rewrites the span again, and exports.
 *   BLUR      an edit-box blur commit rewrites it once more.
 *
 * EVERY PASS/FAIL DECISION LIVES IN `./sliderWriteBackGate.mjs`, covered in CI
 * by `./sliderWriteBackGate.test.ts`. This file is transport and sequencing.
 *
 * LIVE-ONLY — NOT verify/CI-gated. Requires a running reify-gui launched with
 * REIFY_DEBUG=1 (real webview).
 *
 * Usage:
 *   REIFY_DEBUG_PORT=<port> node gui/test/visual/smoke_slider_write_back_e2e.mjs
 * or, self-launching:
 *   npm --prefix gui run test:smoke:slider-write-back
 *
 * Exit 0 on all-pass, 1 on an asserted failure, 2 on an unexpected throw.
 */

import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  CELL_ID,
  DEFAULT_LITERAL,
  EDIT_BLUR_VALUE,
  EDIT_ENTER_FRAMES,
  EDIT_ENTER_VALUE,
  FIXTURE_RELPATH,
  LENGTH_TOLERANCE_MM,
  SLIDER_HOLD_FRAMES,
  SLIDER_HOLD_VALUE,
  SLIDER_RELEASE_VALUE,
  SUBJECT_BASENAME,
  cellMm,
  checkCommitRewroteLiteral,
  checkExportExtent,
  checkHeldGestureLeftDiskUntouched,
  expectedSourceAfterCommit,
  formatFailures,
  propInputSelector,
  sliderSelector,
} from './sliderWriteBackGate.mjs';
import { makeDebugRpc } from './rpcEnvelope.mjs';
import { describeRpcFailure, openFileWithRetry } from './smokeDriverGuards.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..', '..', '..');

const IDLE_TIMEOUT_MS = 60_000;

/** Bound on every poll below: the subject is one box, so seconds are generous. */
const POLL_TIMEOUT_MS = 15_000;
const POLL_INTERVAL_MS = 100;

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
const rpc = makeDebugRpc(DEBUG_URL);

// ─── Helpers ─────────────────────────────────────────────────────────────────

let stepNum = 0;
function log(msg) {
  stepNum++;
  console.log(`[step ${stepNum}] ${msg}`);
}

/**
 * An ASSERTED failure, as opposed to an unexpected throw: `main().catch` exits
 * 1 vs 2. A throw rather than `process.exit(1)` so `main`'s `finally` still
 * removes the temporary copy.
 */
class AssertedFailure extends Error {}

function fail(msg) {
  throw new AssertedFailure(msg);
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

/** Wait for the engine to stop evaluating, and ASSERT the verdict. */
async function waitForIdle(what) {
  const result = await rpc('wait_for_idle', { timeout_ms: IDLE_TIMEOUT_MS });
  console.log(`  wait_for_idle (${what}):`, JSON.stringify(result));
  if (!result || result.ok !== true) {
    fail(
      `wait_for_idle did not reach idle within ${IDLE_TIMEOUT_MS}ms after ${what}: ` +
        `${JSON.stringify(result)}. A READINESS TIMEOUT, not a write-back violation.`,
    );
  }
}

/**
 * Poll `read` until `done(value)` holds or the bound elapses; returns the last
 * value either way, so the gate predicate — not this loop — renders the verdict.
 */
async function pollUntil(read, done) {
  const deadline = Date.now() + POLL_TIMEOUT_MS;
  let value = await read();
  while (!done(value) && Date.now() < deadline) {
    await sleep(POLL_INTERVAL_MS);
    value = await read();
  }
  return value;
}

/**
 * Abort on a failed tool call, naming it as what it is: the phase was never
 * TESTED, so it is an outage or a broken premise, never a write-back violation.
 */
function failUntested(phase, diagnosis) {
  fail(
    `${diagnosis}\n  Phase '${phase}' was NEVER TESTED: a debug-MCP tool outage or a broken ` +
      `premise (docs/debug-mcp-contract.md §2a), not a write-back violation.`,
  );
}

/** A read tool answered with a healthy payload (read tools carry no `ok` flag). */
function requireRpcAnswer(phase, payload, label) {
  const diagnosis = describeRpcFailure(payload, label);
  if (diagnosis !== null) failUntested(phase, diagnosis);
}

/** An acting tool answered healthy AND reported that it acted. */
function requireToolOk(phase, payload, label) {
  requireRpcAnswer(phase, payload, label);
  if (payload.ok !== true && payload.success !== true) {
    failUntested(phase, `${label} did not report ok: ${JSON.stringify(payload)}`);
  }
}

/** Report a phase's failure list, failing the run when it is non-empty. */
function requirePhase(phase, failures) {
  if (failures.length === 0) {
    console.log(`  OK: phase '${phase}' holds`);
    return;
  }
  fail(`phase '${phase}' violated:\n${formatFailures(failures)}`);
}

/** Copy the tracked fixture into a fresh temp directory, keeping its basename. */
function copySubject() {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'reify-slider-write-back-'));
  const subject = path.join(work, SUBJECT_BASENAME);
  try {
    fs.copyFileSync(path.join(REPO_ROOT, FIXTURE_RELPATH), subject);
  } catch (err) {
    fs.rmSync(work, { recursive: true, force: true });
    throw err;
  }
  return { work, subject };
}

function readDisk(subject) {
  return fs.readFileSync(subject, 'utf8');
}

/** Wait until exactly one element matches `selector` (the write tools refuse 0 or >1). */
async function waitForSoleControl(phase, selector) {
  const probe = await pollUntil(
    () => rpc('query_selector_all', { selector }),
    (r) => describeRpcFailure(r, 'query_selector_all') === null && r.count === 1,
  );
  requireRpcAnswer(phase, probe, 'query_selector_all');
  if (probe.count !== 1) {
    fail(
      `phase '${phase}': ${probe.count} elements match ${selector} after ${POLL_TIMEOUT_MS}ms ` +
        `(need exactly 1). A PREMISE failure: the control never rendered as addressed.`,
    );
  }
}

/** Export STL to `<work>/<name>.stl` and grade its X extent. */
async function exportAndCheckExtent(phase, work, name, expectedMm) {
  const outputPath = path.join(work, `${name}.stl`);
  const exported = await rpc('reify_export', { format: 'stl', output_path: outputPath });
  console.log(`  reify_export -> ${JSON.stringify(exported)}`);
  requireToolOk(phase, exported, 'reify_export');
  return checkExportExtent({ bytes: fs.readFileSync(outputPath), expectedMm });
}

/** Drive one committing gesture, then wait for the disk to move off `before`. */
async function commitAndReadDisk(phase, subject, before, tool, args) {
  const result = await rpc(tool, args);
  console.log(`  ${tool}(${args.commit}) -> ${JSON.stringify(result)}`);
  requireToolOk(phase, result, tool);
  // The tool returns once its events are dispatched; the set_parameter IPC and
  // the write-back finish after that.
  return pollUntil(
    () => readDisk(subject),
    (text) => text !== before,
  );
}

// ─── Main ────────────────────────────────────────────────────────────────────

async function main() {
  console.log(`smoke_slider_write_back_e2e: targeting debug server at ${DEBUG_URL}`);

  log('Waiting for debug server…');
  await waitForServer(60_000);
  console.log('  OK: server ready');

  const { work, subject } = copySubject();
  console.log(`  driving a COPY: ${subject}`);
  try {
    log('Opening the copy via open_file (with retry for WebView init)…');
    const opened = await openFileWithRetry(rpc, subject, { fail });
    if (!String(opened?.path ?? '').includes(SUBJECT_BASENAME)) {
      fail(`open_file returned a path that is not the subject: ${JSON.stringify(opened)}`);
    }
    await waitForIdle('open');

    // ── BASELINE ────────────────────────────────────────────────────────────
    log('BASELINE: the disk holds the default literal, and the export is its length…');
    const baseline = readDisk(subject);
    const splice = expectedSourceAfterCommit(
      baseline,
      DEFAULT_LITERAL,
      `${SLIDER_RELEASE_VALUE}mm`,
    );
    if (typeof splice !== 'string') {
      fail(`FIXTURE PREMISE failure, the gate cannot run:\n${formatFailures([splice])}`);
    }
    const baselineExtent = await exportAndCheckExtent(
      'baseline',
      work,
      'baseline',
      Number.parseFloat(DEFAULT_LITERAL),
    );
    if (baselineExtent.length > 0) {
      fail(
        `FIXTURE/MEASUREMENT PREMISE failure (the export's X axis or units are not what the ` +
          `gate measures), nothing below would mean anything:\n${formatFailures(baselineExtent)}`,
      );
    }
    console.log('  OK: phase \'baseline\' holds');

    // ── HOLD ────────────────────────────────────────────────────────────────
    log(`HOLD: scrub the slider through ${SLIDER_HOLD_FRAMES.join(',')} to ${SLIDER_HOLD_VALUE} and keep holding…`);
    await waitForSoleControl('hold', sliderSelector());
    const held = await rpc('scrub_range_input', {
      selector: sliderSelector(),
      value: SLIDER_HOLD_VALUE,
      frames: [...SLIDER_HOLD_FRAMES],
      commit: 'hold',
    });
    console.log(`  scrub_range_input(hold) -> ${JSON.stringify(held)}`);
    requireToolOk('hold', held, 'scrub_range_input');
    const heldMm = Number(SLIDER_HOLD_VALUE);
    const engineState = await pollUntil(
      () => rpc('engine_state'),
      (state) => {
        const mm = cellMm(state, CELL_ID);
        return mm !== undefined && Math.abs(mm - heldMm) <= LENGTH_TOLERANCE_MM;
      },
    );
    requireRpcAnswer('hold', engineState, 'engine_state');
    const engineMm = cellMm(engineState, CELL_ID);
    console.log(`  engine reads ${CELL_ID} = ${engineMm} mm`);
    requirePhase(
      'hold',
      checkHeldGestureLeftDiskUntouched({
        baseline,
        afterHold: readDisk(subject),
        engineMm,
        heldMm,
      }),
    );

    // ── RELEASE ─────────────────────────────────────────────────────────────
    log(`RELEASE: release the slider at ${SLIDER_RELEASE_VALUE}…`);
    const releaseLiteral = `${SLIDER_RELEASE_VALUE}mm`;
    const afterRelease = await commitAndReadDisk('release', subject, baseline, 'scrub_range_input', {
      selector: sliderSelector(),
      value: SLIDER_RELEASE_VALUE,
      commit: 'change',
    });
    requirePhase(
      'release',
      checkCommitRewroteLiteral({
        before: baseline,
        after: afterRelease,
        fromLiteral: DEFAULT_LITERAL,
        toLiteral: releaseLiteral,
      }),
    );
    await waitForIdle('the release');

    // ── EXPORT ──────────────────────────────────────────────────────────────
    log(`EXPORT: the exported X extent is the released ${releaseLiteral}, not ${DEFAULT_LITERAL}…`);
    requirePhase(
      'export',
      await exportAndCheckExtent('export', work, 'release', Number(SLIDER_RELEASE_VALUE)),
    );

    // ── EDIT BOX / Enter ────────────────────────────────────────────────────
    log(`ENTER: type ${EDIT_ENTER_FRAMES.join(',')} then ${EDIT_ENTER_VALUE} into the edit box, press Enter…`);
    await waitForSoleControl('enter', propInputSelector());
    const afterEnter = await commitAndReadDisk('enter', subject, afterRelease, 'edit_text_input', {
      selector: propInputSelector(),
      value: EDIT_ENTER_VALUE,
      frames: [...EDIT_ENTER_FRAMES],
      commit: 'enter',
    });
    requirePhase(
      'enter',
      checkCommitRewroteLiteral({
        before: afterRelease,
        after: afterEnter,
        fromLiteral: releaseLiteral,
        toLiteral: EDIT_ENTER_VALUE,
      }),
    );
    await waitForIdle('the Enter commit');
    requirePhase(
      'enter export',
      await exportAndCheckExtent('enter export', work, 'enter', Number.parseFloat(EDIT_ENTER_VALUE)),
    );

    // ── EDIT BOX / blur ─────────────────────────────────────────────────────
    log(`BLUR: type ${EDIT_BLUR_VALUE} into the edit box, then blur…`);
    await waitForSoleControl('blur', propInputSelector());
    const afterBlur = await commitAndReadDisk('blur', subject, afterEnter, 'edit_text_input', {
      selector: propInputSelector(),
      value: EDIT_BLUR_VALUE,
      commit: 'blur',
    });
    requirePhase(
      'blur',
      checkCommitRewroteLiteral({
        before: afterEnter,
        after: afterBlur,
        fromLiteral: EDIT_ENTER_VALUE,
        toLiteral: EDIT_BLUR_VALUE,
      }),
    );
    await waitForIdle('the blur commit');
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }

  console.log('\n=== SMOKE PASS: smoke_slider_write_back_e2e ===');
  process.exit(0);
}

main().catch((err) => {
  if (err instanceof AssertedFailure) {
    console.error(`\nFAIL: ${err.message}`);
    process.exit(1);
  }
  console.error('\nUnexpected error:', err);
  process.exit(2);
});
