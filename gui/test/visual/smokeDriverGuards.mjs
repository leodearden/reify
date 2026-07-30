/**
 * Driver-side ASSERTION POLICY for the live `smoke_*.mjs` e2e drivers (task 5827).
 * CANONICAL EXPLANATION for both functions — the suite and the driver call sites
 * point here rather than restating it.
 *
 * Separate from `./rpcEnvelope.mjs` deliberately: that owns the WIRE DECODE
 * (`docs/debug-mcp-contract.md` §2), this owns what a DRIVER does with a decoded
 * payload — how long it waits for a WebView that is not ready, and how it words a
 * failure so the operator sees the underlying cause.
 *
 * WHY NOT INLINE: a driver needs a live reify-gui (WebKit WebView + OCCT), so CI
 * can never run one and inline logic can only fail during a live run. Hoisting it
 * somewhere vitest CAN load is what makes it testable — task 5731's pattern on
 * this seam. `./smokeDriverGuards.test.ts` is the pin.
 *
 * PLAIN ESM, no `node:` imports — a hard constraint, not style: vitest resolves
 * this under vite's browser conditions while the drivers' runners invoke bare
 * `node`, never `tsx`. `setTimeout` is a global and is fine.
 */

import { isInBandError } from "./rpcEnvelope.mjs";

/**
 * How many times {@link openFileWithRetry} issues `open_file` before giving up.
 *
 * EMPIRICAL, NOT TUNABLE-BY-TASTE: 8 × 3000 ms (≤45 s) is lifted verbatim from
 * the drivers that already carry this loop inline. The debug MCP server answers
 * `health` before the WebKit WebView has finished initialising its EGL/GLX
 * context, and until it has, `open_file` returns the in-band error
 * "debug-request timed out after 5000ms". Re-measure against a live GUI before
 * changing these, rather than reasoning about them from here.
 *
 * TODO(#5857): four drivers still carry that loop inline (smoke_appearance_e2e,
 * smoke_diagnostics_e2e, smoke_mesh_count_parity_e2e and
 * smoke_surface_finish_viewport_e2e) — migrate them onto this helper so the
 * constants, the `.ok` verdict and the failure wording have one home.
 */
export const OPEN_FILE_ATTEMPTS = 8;

/** @see OPEN_FILE_ATTEMPTS — the paired empirical constant. */
export const OPEN_FILE_RETRY_DELAY_MS = 3000;

/**
 * Open a file over the debug MCP bridge, retrying while the WebView boots, and
 * FAIL LOUDLY when the budget is exhausted.
 *
 * `open_file` answers `{ok: true, path}` or the in-band `{error: "<msg>"}`
 * (`gui/src/debug/bridge.ts`). A driver that neither asserts `.ok` nor retries
 * swallows the failure, so the outage resurfaces steps later as an `activeFile`
 * mismatch and gets blamed on the frontend. The effectful edges are injectable
 * for the same reason `makeDebugRpc` takes a `fetchImpl`: so the suite can drive
 * this real path with no server and no wall-clock delay.
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
 *        default exists so a caller that forgets cannot silently continue past an
 *        exhausted retry — that silence is the bug this helper removes.
 * @returns {Promise<unknown>} The last payload `open_file` returned (or, for a
 *          transport failure, the in-band shape its message was folded into).
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
    try {
      result = await rpc("open_file", { path: filePath });
    } catch (err) {
      // THE TRANSPORT HALF OF THE BOOT WINDOW. Per rpcEnvelope's throw/resolve
      // split a §2c error THROWS, and `fetchImpl` rejects on ECONNREFUSED while
      // the GUI settles. Letting that escape would abort the whole budget on
      // attempt 1 and surface as the driver's exit-2 "Unexpected error" instead
      // of the clean exit-1 verdict this helper guarantees. Folding it into the
      // in-band dialect makes it ONE failed attempt whose text still reaches the
      // final diagnosis.
      result = { error: `open_file threw: ${err?.message ?? String(err)}` };
    }
    log(`  open_file attempt ${attempt} result:`, JSON.stringify(result));
    if (result && /** @type {any} */ (result).ok) return result;
    // No trailing wait after the final attempt: it would delay the failure report
    // by a full `delayMs` and change nothing about the verdict.
    if (attempt < attempts) {
      log(`  Retrying in ${delayMs}ms (WebView still initialising)…`);
      await sleepImpl(delayMs);
    }
  }

  // The `??` branch is reached only when the payload was WELL-FORMED and simply
  // did not report ok — a refusal, not an outage, so there is no error text to
  // quote and the payload itself IS the diagnosis.
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
 * the tool fails in-band with `{error: "<msg>"}`, the guard passes because that
 * object is TRUTHY, and the driver reports a field-shape problem ("got 0
 * viewports") while the one string saying what actually broke is never printed.
 * Returning a STRING rather than a boolean is the point: it forces the underlying
 * error text into the operator-visible message at every call site.
 *
 * Branch table, in order:
 *   1. null / undefined  → `<label> returned null`
 *   2. §2a in-band error → `<label> failed: <error>` — the verbatim text
 *   3. non-object        → `<label> returned a non-object payload: <json>`.
 *                          `rpc()` hands back RAW TEXT when a content block is
 *                          not JSON, so this is reachable live and must not throw.
 *   4. otherwise         → `null`; the caller's own field checks take over.
 *
 * BRANCH 4 INCLUDES THE EMPTY OBJECT, deliberately: `{}` is a healthy answer that
 * happens to be empty, not a failed RPC, and the caller's own check ("≥2
 * viewports") already words that case accurately.
 *
 * The §2a discriminator is NOT re-implemented — {@link isInBandError} is its
 * single home (task 5731). That matters concretely: §2a specifies a STRING
 * `error`, while the sibling drivers' `'error' in X` idiom fires on any `error`
 * key at all. The suite pins the agreement with a `{error: 500}` case asserted
 * against both functions side by side.
 *
 * Never throws, whatever the shape of `payload`.
 *
 * @param {unknown} payload  A payload as returned by `rpc()` — already decoded by
 *        `normalizeRpcEnvelope`, so BOTH failure dialects arrive as §2a.
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
