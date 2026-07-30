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
  //
  // The `??` branch is reached when the payload was WELL-FORMED and simply did
  // not report ok — a refusal, not an outage. There is no underlying error text
  // to quote, so the payload itself is the diagnosis.
  const diagnosis =
    describeRpcFailure(result, "open_file") ??
    `open_file did not report ok: ${JSON.stringify(result)}`;
  fail(`open_file failed after ${attempts} attempts: ${diagnosis}`);
  return result;
}

/**
 * Diagnose a decoded RPC payload: a message naming the failure, or `null` when
 * the payload is healthy enough for the caller to start reading fields.
 *
 * THE FAILURE MODE THIS EXISTS FOR: a driver guards an RPC with `if (!payload)`,
 * the tool fails in-band with `{error: "<msg>"}`, and the guard passes because
 * that object is TRUTHY. The driver then reads its fields as `undefined` and
 * reports a field-shape problem — "got 0 viewports" — while the one string that
 * says what actually broke is never printed. A tool OUTAGE misreported as a
 * frontend assertion failure, with the real cause discarded.
 *
 * Returning a STRING rather than a boolean is the point: it forces the
 * underlying error text into the operator-visible message at every call site,
 * which a `if (isInBandError(x)) fail('rpc failed')` idiom would not.
 *
 * Branch table, in order:
 *   1. null / undefined  → `<label> returned null`
 *   2. §2a in-band error → `<label> failed: <error>` — the verbatim text
 *   3. non-object        → `<label> returned a non-object payload: <json>`.
 *                          `rpc()` hands back RAW TEXT when a content block is
 *                          not JSON, so this is reachable in a live run and must
 *                          not throw.
 *   4. otherwise         → `null`; the payload is an object and the caller's own
 *                          field checks take over.
 *
 * BRANCH 4 INCLUDES THE EMPTY OBJECT, deliberately. `{}` is a healthy answer
 * that happens to be empty, not a failed RPC, and the caller's own check ("≥2
 * viewports") already words that case accurately. Claiming it here would trade
 * one misdiagnosis for another.
 *
 * The §2a discriminator is NOT re-implemented — {@link isInBandError} from
 * `./rpcEnvelope.mjs` is its single home (task 5731) and
 * `docs/debug-mcp-contract.md` §2a is authoritative. That matters concretely:
 * §2a specifies a STRING `error`, while the sibling drivers' `'error' in X`
 * idiom fires on any `error` key at all. Delegating keeps one definition;
 * `./smokeDriverGuards.test.ts` pins the agreement with a `{error: 500}` case
 * asserted against both functions side by side.
 *
 * Never throws, whatever the shape of `payload`.
 *
 * @param {unknown} payload  A payload as returned by `rpc()` — already decoded
 *        by `normalizeRpcEnvelope`, so BOTH failure dialects arrive as §2a.
 * @param {string} label  The tool name (plus any call-site qualifier) to name in
 *        the message, e.g. `store_state` or `store_state (post-open)`.
 * @returns {string | null} The diagnosis, or `null` when there is nothing wrong.
 */
export function describeRpcFailure(payload, label) {
  if (payload === null || payload === undefined) return `${label} returned null`;
  if (isInBandError(payload)) return `${label} failed: ${payload.error}`;
  if (typeof payload !== "object" || Array.isArray(payload)) {
    return `${label} returned a non-object payload: ${JSON.stringify(payload)}`;
  }
  return null;
}
