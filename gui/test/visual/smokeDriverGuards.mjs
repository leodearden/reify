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
 * the six drivers that used to carry this loop inline. The debug MCP server
 * answers `health` before the WebKit WebView has finished initialising its
 * EGL/GLX context, and until it has, `open_file` returns the in-band error
 * "debug-request timed out after 5000ms". Re-measure against a live GUI before
 * changing these, rather than reasoning about them from here.
 *
 * THIS IS NOW THEIR ONLY HOME. Every `smoke_*.mjs` driver reaches `open_file`
 * through {@link openFileWithRetry}, so the constants, the `.ok` verdict and the
 * failure wording cannot drift apart across six copies again. That is enforced,
 * not merely intended: `./smokeDriverConventions.test.ts` fails on any driver
 * that calls the tool directly — the only executable signal available, since a
 * driver needs a live GUI and CI can never run one.
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

/**
 * Diagnose an UNSCOPED `dom_query` that is supposed to be AMBIGUOUS: a message
 * naming why it was not, or `null` when it matched exactly `expectedMatchCount`
 * elements attributed to a pane.
 *
 * THE ANTI-VACUITY PRECONDITION for every viewportId-scoped assertion that
 * follows it (task 5932, covering #5891). `paneDiagnostics` echoes `{viewportId,
 * matchCount}` only ABOVE one match (`gui/src/debug/bridge.ts:336-341`), which is
 * what makes a scoped call's bare `{ok: true}` evidence that the scope narrowed
 * the resolve. That evidence is worth nothing unless there were several
 * candidates to narrow: in any future where only ONE pane renders the testid, a
 * scoped click trivially does not bleed — there is nothing to bleed into — and
 * the whole no-bleed scenario reports green while testing nothing. Asserting the
 * ambiguity FIRST is what keeps the rest load-bearing, the same "an empty table
 * is not a pass" discipline the sibling suites apply.
 *
 * Branch table, in order:
 *   1. unhealthy payload → {@link describeRpcFailure}'s verbatim diagnosis
 *   2. `exists !== true`             → the testid is not in the DOM at all
 *   3. `matchCount` not a number     → only one element matched ⇒ VACUOUS
 *   4. `matchCount !== expected`     → names expected vs observed
 *   5. `viewportId` not a string     → the first match belongs to no pane
 *
 * BRANCH 3 IS THE LOAD-BEARING ONE and its type check is not defensive
 * dressing: `undefined < 2` and `'2' < 2` are both FALSE, so a magnitude-only
 * guard reports green on exactly the payloads it exists to catch — the one-sided
 * hazard `./smoke_multi_pane_e2e.mjs` documents at its design-main meshCount
 * check. Since `paneDiagnostics` OMITS the field entirely below two matches, an
 * absent `matchCount` is not missing data: it is a positive report of one match.
 *
 * Never throws, whatever the shape of `payload`.
 *
 * @param {unknown} payload  The decoded `dom_query` result, unscoped.
 * @param {object} options
 * @param {string} options.testId  The `data-testid` the probe asked for; named in
 *        every message so an operator sees which probe failed.
 * @param {number} options.expectedMatchCount  How many panes must carry it.
 * @returns {string | null} The diagnosis, or `null` when the probe is ambiguous
 *          exactly as required.
 */
export function describeUnscopedAmbiguity(payload, { testId, expectedMatchCount }) {
  const label = `dom_query('${testId}') unscoped`;
  const failure = describeRpcFailure(payload, label);
  if (failure) return failure;

  if (payload.exists !== true) {
    return (
      `${label} matched no element at all (exists: ${JSON.stringify(payload.exists)}) — ` +
      `data-testid="${testId}" is nowhere in the DOM, so there is nothing to scope. ` +
      `Payload: ${JSON.stringify(payload)}`
    );
  }

  if (typeof payload.matchCount !== "number") {
    return (
      `${label} did not report a numeric matchCount (got ${JSON.stringify(payload.matchCount)}); ` +
      `paneDiagnostics omits the field entirely below 2 matches, so its absence means exactly ONE ` +
      `element matched. Expected ${expectedMatchCount} candidates — with only one, the panes are not ` +
      `ambiguous and every viewportId-scoped assertion below would be vacuous.`
    );
  }

  if (payload.matchCount !== expectedMatchCount) {
    return (
      `${label} matched ${payload.matchCount} elements, expected ${expectedMatchCount}. ` +
      `The scoped assertions below are calibrated to that count, so a different one means the ` +
      `fixture's pane set changed rather than that scoping regressed.`
    );
  }

  if (typeof payload.viewportId !== "string") {
    return (
      `${label} matched ${payload.matchCount} elements but the first match is attributed to no pane ` +
      `(viewportId: ${JSON.stringify(payload.viewportId)}) — resolveByTestId reads it off ` +
      `el.closest('[data-viewport-id]'), so a non-string means the element carries no pane ancestor ` +
      `and viewportId scoping cannot resolve it.`
    );
  }

  return null;
}
