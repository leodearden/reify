/**
 * Driver-side ASSERTION POLICY for the live `smoke_*.mjs` e2e drivers
 * (task 5827).
 *
 * Layered on `./rpcEnvelope.mjs`, and deliberately a separate module from it.
 * rpcEnvelope owns the WIRE DECODE — the three response shapes of
 * `docs/debug-mcp-contract.md` §2 — and its charter is explicitly that narrow.
 * This module owns what a DRIVER does with a decoded payload: how long it waits
 * for a WebView that is not ready yet, and how it words a failure so the
 * operator sees the underlying cause. Keeping the two one import apart leaves
 * rpcEnvelope's "single home for the decode" claim literally true.
 *
 * WHY THESE LIVE HERE AT ALL: the drivers need a running reify-gui with a
 * WebKit WebView and OCCT, so CI can never execute them. Logic that stays
 * inline in a driver is logic that can only fail during a live run. Hoisting it
 * into a module vitest CAN load is what makes it testable — the pattern task
 * 5731 established on this same seam, and `./smokeDriverGuards.test.ts` is the
 * pin.
 *
 * PLAIN ESM, NOT TypeScript, and free of `node:` imports — a hard constraint
 * inherited from `./rpcEnvelope.mjs`, not a style choice. Both consumer families
 * must load it: the vitest suite (resolved by vite under browser conditions) and
 * the live drivers, whose runners invoke them with bare `node`, never `tsx`.
 * `setTimeout` is a global and is fine.
 */

import { isInBandError } from "./rpcEnvelope.mjs";

/**
 * How many times {@link openFileWithRetry} issues `open_file` before giving up.
 *
 * EMPIRICAL, NOT TUNABLE-BY-TASTE: 8 attempts × 3000 ms (≤45 s) is lifted
 * verbatim from the five drivers that already carry this loop inline
 * (smoke_diagnostics_e2e, smoke_appearance_e2e, smoke_mesh_count_parity_e2e,
 * smoke_surface_finish_viewport_e2e, smoke_multi_pane_e2e). The debug MCP server
 * answers `health` before the WebKit WebView has finished initialising its
 * EGL/GLX context, and until it has, `open_file` returns the in-band error
 * "debug-request timed out after 5000ms". These constants are the measured
 * envelope of that startup window — a future reader should re-measure against a
 * live GUI before changing them, not reason about them from here.
 */
export const OPEN_FILE_ATTEMPTS = 8;

/** @see OPEN_FILE_ATTEMPTS — the paired empirical constant. */
export const OPEN_FILE_RETRY_DELAY_MS = 3000;

/**
 * Open a file over the debug MCP bridge, retrying while the WebView boots, and
 * FAIL LOUDLY when the budget is exhausted.
 *
 * `open_file` answers `{ok: true, path}` on success and the in-band
 * `{error: "<msg>"}` on failure (`gui/src/debug/bridge.ts`). A driver that
 * neither asserts `.ok` nor retries — which is what `smoke_find_uses.mjs` did
 * before this — swallows the failure and carries on, so the outage resurfaces
 * several steps later as an `activeFile` mismatch and gets blamed on the
 * frontend. The whole point of this helper is that the FIRST thing to go wrong
 * is the thing that gets reported.
 *
 * The effectful edges are injectable for exactly the reason `makeDebugRpc`
 * takes a `fetchImpl`: so the suite can drive this real code path with no
 * server and no wall-clock delay.
 *
 * @param {(method: string, args?: Record<string, unknown>) => Promise<unknown>} rpc
 *        The driver's `rpc` helper, normally from `makeDebugRpc`.
 * @param {string} filePath  Absolute path to the `.ri` fixture to open.
 * @param {object} [options]
 * @param {number} [options.attempts]   Defaults to {@link OPEN_FILE_ATTEMPTS}.
 * @param {number} [options.delayMs]    Defaults to {@link OPEN_FILE_RETRY_DELAY_MS}.
 * @param {(ms: number) => Promise<unknown>} [options.sleepImpl]  Defaults to a real timer.
 * @param {(...args: unknown[]) => void} [options.log]  Defaults to `console.log`.
 * @param {(message: string) => void} [options.fail]  Defaults to THROWING. A
 *        driver passes its own `fail` (which `process.exit(1)`s); the throwing
 *        default exists so a caller that forgets cannot silently continue past
 *        an exhausted retry — that silence is the bug this helper removes.
 * @returns {Promise<unknown>} The last payload `open_file` returned.
 */
export async function openFileWithRetry(rpc, filePath, options = {}) {
  const {
    attempts = OPEN_FILE_ATTEMPTS,
    delayMs = OPEN_FILE_RETRY_DELAY_MS,
    sleepImpl = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    log = console.log,
    fail = (message) => {
      throw new Error(message);
    },
  } = options;

  let result = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    result = await rpc("open_file", { path: filePath });
    log(`  open_file attempt ${attempt} result:`, JSON.stringify(result));
    if (result && /** @type {any} */ (result).ok) return result;
    // No trailing wait after the final attempt: it would delay the failure
    // report by a full `delayMs` and change nothing about the verdict.
    if (attempt < attempts) {
      log(`  Retrying in ${delayMs}ms (WebView still initialising)…`);
      await sleepImpl(delayMs);
    }
  }

  // Name the UNDERLYING cause. `JSON.stringify` alone would render an in-band
  // failure as `{"error":"debug-request timed out after 5000ms"}` — readable,
  // but it buries the one string the operator needs behind punctuation, and it
  // renders a null payload as the bare literal `null` with no subject.
  const diagnosis = isInBandError(result)
    ? `open_file failed: ${result.error}`
    : result === null || result === undefined
      ? "open_file returned null"
      : `open_file did not report ok: ${JSON.stringify(result)}`;
  fail(`open_file failed after ${attempts} attempts: ${diagnosis}`);
  return result;
}
