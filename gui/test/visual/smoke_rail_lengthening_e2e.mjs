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
  RAIL_SPAN_CELL,
  SUBJECT_BASENAME,
  X_RAIL_LEN_CELL,
  Y_RAIL_LEN_CELL,
  checkRailLengtheningGate,
  extractGateInputs,
  formatFailures,
  observeThenExtras,
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

/**
 * The FS watcher's debounce window, and how far past it to wait.
 *
 * `reify_set_parameter` writes disk, so the watcher fires on the AI's own edit.
 * B5 is that the resulting reload changes nothing — but only a read taken AFTER
 * the window has elapsed can tell "idempotent" from "hasn't happened yet", and a
 * read taken exactly at the boundary would make the gate flaky in the direction
 * that PASSES. The margin buys the difference.
 */
const WATCHER_DEBOUNCE_MS = 100;
const WATCHER_DEBOUNCE_MARGIN = 10;

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
 * That holds ACROSS the phase, not just inside this function, which is why
 * `gradePhase` routes through `observeThenExtras` rather than taking an object:
 * a phase's extras are gathered only once this returns. The two enforcers are
 * `observeThenExtras` (./railLengtheningGate.mjs, runtime) and
 * `awaited-extras-literal` (./smokeDriverConventions.ts, source-level, CI).
 *
 * @returns {Promise<{inputs: object, verdict: {ok: boolean, failures: object[]}}>}
 */
async function observePhase(phase, subject) {
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

  return inputs;
}

/**
 * Observe the live state and grade it, folding in whatever extra PRD rows this
 * phase exercises.
 *
 * `gatherExtras` is a THUNK, not an object, and the sequencing lives in
 * `observeThenExtras` (./railLengtheningGate.mjs) rather than here: an object
 * literal in this argument position would be evaluated — its awaits included —
 * BEFORE this function is entered, hoisting its reads above observePhase's and
 * reloading the file from disk in the middle of them. See observePhase's READ
 * ORDER IS LOAD-BEARING paragraph, and ./smokeDriverConventions.ts's
 * `awaited-extras-literal`, which is what stops the literal coming back.
 *
 * The extras are merged into the inputs rather than checked separately so the
 * verdict stays ONE `{ok, failures}` — the driver has a single decision seam and
 * a single place where a record becomes English, which is the whole point of
 * keeping the decision in ./railLengtheningGate.mjs.
 */
async function gradePhase(phase, subject, gatherExtras) {
  const inputs = await observeThenExtras(() => observePhase(phase, subject), gatherExtras);
  const verdict = checkRailLengtheningGate(inputs);
  console.log(`  graded '${phase}': ok=${verdict.ok}`);
  return { inputs, verdict };
}

/**
 * Issue one AI parameter write, returning the RAW envelope.
 *
 * Deliberately undiagnosed: half this gate's calls EXPECT an in-band error (B7),
 * so routing them through `requireLiveRead` would abort the run on the very
 * outcome being asserted. The success calls are checked by their phase verdict
 * instead — if a write silently failed, every cell reads its old value and the
 * value gate says so with the number it actually found.
 */
async function setParameter(cellId, value) {
  const envelope = await rpc('reify_set_parameter', { cell_id: cellId, value });
  console.log(`  reify_set_parameter(${cellId}, ${value}) -> ${JSON.stringify(envelope)}`);
  return envelope;
}

/** The `.ri` text currently ON DISK, re-read through `reify_open_file`. */
async function readSource(phase, subject) {
  const opened = await rpc('reify_open_file', { file_path: subject });
  requireLiveRead(phase, describeRpcFailure(opened, 'reify_open_file (source re-read)'));
  return opened?.source;
}

/**
 * The B2 reading: the on-disk literals, plus the save that must not change them.
 *
 * `reify_save_file` is issued BETWEEN the two source reads because B2's second
 * half is that the save is a NO-OP — the engine's buffer already matches disk,
 * so writing it must leave the bytes alone.
 */
async function readSourceCanonical(phase, subject, expected) {
  const source = await readSource(phase, subject);
  const saveFile = await rpc('reify_save_file', { file_path: subject });
  console.log(`  reify_save_file -> ${JSON.stringify(saveFile)}`);
  const sourceAfterSave = await readSource(phase, subject);
  return { source, expected, saveFile, sourceAfterSave };
}

/**
 * The B7 reading: a write that must be REFUSED, with the source either side.
 *
 * The source is read before and after through the same `reify_open_file` path,
 * so "no partial mutation" is asserted against what is actually on disk rather
 * than against what the engine says it holds.
 */
async function attemptRefusedWrite(phase, subject, cellId, value) {
  const sourceBefore = await readSource(phase, subject);
  const envelope = await setParameter(cellId, value);
  const sourceAfter = await readSource(phase, subject);
  return { tool: `reify_set_parameter(${cellId}, ${value})`, error: envelope, sourceBefore, sourceAfter };
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

    // A THUNK, never an object literal — here and at every gradePhase call
    // below. A literal's awaits run before gradePhase is entered, which puts
    // these reads above observePhase's and (for any phase that gathers
    // sourceCanonical) reloads the file from disk between them. observePhase's
    // READ ORDER IS LOAD-BEARING paragraph is the invariant; observeThenExtras
    // enforces it at runtime and ./smokeDriverConventions.ts's
    // `awaited-extras-literal` stops the literal form coming back.
    log('Grading the baseline…');
    const baseline = await gradePhase('baseline', subject, async () => ({
      requires: ['fieldCoverage'],
      fieldCoverage: await rpc('engine_state'),
    }));
    requirePhase('baseline', baseline.verdict);

    // ── Edit 1: the rails overrun the frame ─────────────────────────────────
    log(`Setting ${Y_RAIL_LEN_CELL} to 1100mm via reify_set_parameter…`);
    await setParameter(Y_RAIL_LEN_CELL, '1100mm');
    await waitForIdle('the y_rail_len edit');

    // B1 lives in observePhase's READ ORDER, not in a predicate: engine_state,
    // store_state and demand_dispatch are all read BEFORE reify_open_file, so
    // the lengthened geometry and the moved cell are observed on state the
    // engine already held. Nothing reloaded the file to produce them.
    // READ-ONLY EXTRAS FIRST, and B2 LAST — the ordering this phase is built
    // around. readSourceCanonical WRITES DISK (reify_save_file), and watcher.rs
    // is a pure path/time trailing-edge debouncer (DEBOUNCE_DURATION = 100ms,
    // :17) with NO content hashing, so even a byte-identical save fires a
    // reload. Interposed between B5's two readings it would make B5 measure
    // churn from the SAVE as well as from the AI write — a second cause under
    // one verdict. So: grade B1/B3 and the pin flip on reads alone, settle B5,
    // and only then touch disk for B2.
    log('Grading after-y-rail (B1 live sync, B3 coverage, the pin flip)…');
    const afterYRail = await gradePhase('after-y-rail', subject, async () => ({
      requires: ['fieldCoverage'],
      fieldCoverage: await rpc('engine_state'),
    }));
    requirePhase('after-y-rail', afterYRail.verdict);

    // ── B5: the FS watcher re-fires on the write, and must change nothing ────
    log(`Waiting past the ${WATCHER_DEBOUNCE_MS}ms watcher debounce, then re-reading…`);
    await sleep(WATCHER_DEBOUNCE_MS * WATCHER_DEBOUNCE_MARGIN);
    await waitForIdle('the watcher re-read');
    const reReadInputs = await observePhase('after-y-rail', subject);
    const settled = await gradePhase('after-y-rail', subject, async () => ({
      requires: ['idempotentReload'],
      idempotentReload: { before: afterYRail.inputs, after: reReadInputs },
    }));
    requirePhase('after-y-rail (post-debounce)', settled.verdict);

    // ── B2: the edit landed in the SOURCE, and saving it changes nothing ─────
    log('Grading after-y-rail B2 (the literal is on disk, and the save is a no-op)…');
    const onDisk = await gradePhase('after-y-rail', subject, async () => ({
      requires: ['sourceCanonical'],
      sourceCanonical: await readSourceCanonical('after-y-rail', subject, {
        [Y_RAIL_LEN_CELL]: '1100mm',
      }),
    }));
    requirePhase('after-y-rail (B2 on disk)', onDisk.verdict);

    // ── Edit 2: the frame catches up ────────────────────────────────────────
    log(`Setting ${RAIL_SPAN_CELL} to 1100mm via reify_set_parameter…`);
    await setParameter(RAIL_SPAN_CELL, '1100mm');
    await waitForIdle('the rail_span_m edit');

    // The rail-span pin returns to Satisfied, travel_avail reads 810mm, and the
    // ToolDock pin goes Violated — a REQUIRED cascade, not a tolerated one. See
    // RAIL_GATE_PHASES: this phase's expectations encode all three.
    // No settle step follows this phase, so B2's disk write has no B5 reading to
    // sit between and the thunk alone suffices — but the same ordering holds:
    // inside the thunk these reads still come after observePhase's.
    log('Grading after-rail-span (the pin recovers, travel_avail 810, ToolDock cascades)…');
    const afterRailSpan = await gradePhase('after-rail-span', subject, async () => ({
      requires: ['sourceCanonical', 'fieldCoverage'],
      sourceCanonical: await readSourceCanonical('after-rail-span', subject, {
        [Y_RAIL_LEN_CELL]: '1100mm',
        [RAIL_SPAN_CELL]: '1100mm',
      }),
      fieldCoverage: await rpc('engine_state'),
    }));
    requirePhase('after-rail-span', afterRailSpan.verdict);

    // ── B7: two refusals, neither of which may touch disk ───────────────────
    // Two DIFFERENT reasons on purpose. x_rail_len is a derived `let` with no
    // default literal for `resolve_param_default_span` to return a span for, so
    // it is refused before any value is parsed; '45deg' is a well-formed literal
    // of the WRONG DIMENSION on a Length cell, so it is refused after. A gate
    // that only ever exercised one would not notice the other path losing its
    // atomicity.
    log('Attempting two writes that must be REFUSED, leaving disk byte-identical…');
    // The two attempts stay OUTSIDE the thunk, deliberately. They are the only
    // extras in this file that WRITE rather than read, and the phase observation
    // must post-date them: a refusal that corrupted the engine shows up in the
    // cells and pins graded below, which it could not if the observation were
    // taken first. The thunk then carries pure data, so nothing is hoisted.
    const rejections = [
      await attemptRefusedWrite('after-rail-span', subject, X_RAIL_LEN_CELL, '900mm'),
      await attemptRefusedWrite('after-rail-span', subject, RAIL_SPAN_CELL, '45deg'),
    ];
    const refused = await gradePhase('after-rail-span', subject, async () => ({
      requires: ['rejectionAtomicity'],
      rejectionAtomicity: rejections,
    }));
    requirePhase('after-rail-span (B7 rejections)', refused.verdict);
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
