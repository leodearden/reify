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
 *   2. `exists !== true`                  → the testid is not in the DOM at all
 *   3. `matchCount` ABSENT                → exactly ONE element matched ⇒ VACUOUS
 *   4. `matchCount` present, not a number → the payload is MALFORMED
 *   5. `matchCount !== expected`          → names expected vs observed
 *   6. `viewportId` not a string          → the first match belongs to no pane
 *
 * BRANCH 3 IS THE LOAD-BEARING ONE, and branch 4 is deliberately NOT folded into
 * it as one `typeof` check: the two payloads have different repairs. Since
 * `paneDiagnostics` OMITS the field entirely below two matches, an ABSENT
 * `matchCount` is not missing data — it is a positive report of one match, i.e.
 * the fixture regressed to a single pane and every scoped assertion below it
 * would be vacuous. A PRESENT non-number cannot come from `paneDiagnostics` at
 * all and says the wire contract broke. Telling an operator "the field was
 * absent, so exactly one element matched" of a payload that carried `'2'` would
 * send them after the wrong one.
 *
 * NEITHER may be left to a magnitude-only guard: `undefined < 2` and `'2' < 2`
 * are both FALSE, so comparing without checking the type reports green on exactly
 * the payloads these branches exist to catch — the one-sided hazard
 * `./smoke_multi_pane_e2e.mjs` documents at its design-main meshCount check.
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

  if (payload.matchCount === undefined) {
    return (
      `${label} reported no matchCount at all; paneDiagnostics omits the field entirely below 2 ` +
      `matches, so its absence means exactly ONE element matched. Expected ${expectedMatchCount} ` +
      `candidates — with only one, the panes are not ambiguous and every viewportId-scoped ` +
      `assertion below would be vacuous.`
    );
  }

  if (typeof payload.matchCount !== "number") {
    return (
      `${label} reported a MALFORMED matchCount (${JSON.stringify(payload.matchCount)}) — ` +
      `paneDiagnostics either omits the field or sets it to a number above 1, so a present ` +
      `non-number means the dom_query payload broke its own contract. This is a wire-shape ` +
      `regression, NOT the fixture dropping to a single pane; do not read it as a count.`
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

/**
 * Diagnose one `dom_query` presence probe: healthy, boolean-valued, and equal to
 * `expectedExists` — or the first of those that failed.
 *
 * THE BOOLEAN CHECK IS THE POINT, not the equality one. {@link describeRpcFailure}
 * documents its own blind spot (a NON-STRING `error` such as `{error: 500}` is not
 * §2a and folds to "healthy"), so a guard that went straight to `payload.exists
 * !== false` would read such a payload as "not present" and certify a PASS from a
 * failed RPC. `dom_query` answers `exists: true | false` for resolvable and
 * unresolvable testids alike, so anything else is not a `dom_query` result at all.
 *
 * @param {any} payload  The decoded `dom_query` result.
 * @param {string} label  Names the probe (tool, testid, pane, and when) in the message.
 * @param {boolean} expectedExists  What this probe must have observed.
 * @param {string} mismatch  The caller's own wording for "healthy, but the wrong
 *        answer" — each call site's mismatch means something different, and a
 *        generic "expected false, got true" would say none of it.
 * @returns {string | null}
 */
function describeExistsProbe(payload, label, expectedExists, mismatch) {
  const failure = describeRpcFailure(payload, label);
  if (failure) return failure;
  if (typeof payload.exists !== "boolean") {
    return (
      `${label} did not report a boolean exists (got ${JSON.stringify(payload.exists)}) — ` +
      `dom_query answers exists:true|false for resolvable and unresolvable testids alike, so ` +
      `anything else means the payload is not a dom_query result. Comparing it directly would let ` +
      `it read as "not present" and certify a silent pass.`
    );
  }
  if (payload.exists !== expectedExists) return mismatch;
  return null;
}

/**
 * Diagnose a viewportId-scoped `click_element` on a per-pane TOGGLE: a message
 * naming how the cross-pane property broke, or `null` when the driven pane
 * changed and the other pane did not.
 *
 * THE LIVE COUNTERPART of the jsdom pin in `gui/src/__tests__/debugBridge.test.tsx`
 * ("(a) a viewportId-scoped click_element drives that pane only — no cross-pane
 * bleed"). That case reads `pane1.state.enabled` and `designMain.state.enabled`
 * straight off the stores; a driver cannot, because THE WIRE EXPOSES NO PER-PANE
 * FEA STATE — `store_state` projects `viewports[id]` as `{meshCount}` alone, no
 * `get_fea_mode_state` tool exists, and `dom_query`'s literal
 * (`{exists, visible, text, tagName, bounds, ...paneDiagnostics}`) cannot read
 * `checked`. So the ONLY observable proxy for a pane's `enabled` flag is
 * conditional rendering: `FeaModeToolbar.tsx` gates its body on
 * `<Show when={!collapsed() && props.store.state.enabled}>` and renders
 * `fea-mode-channel-select` unconditionally inside it — an empty `<For>` option
 * list when there are no channels, but the `<select>` itself is present.
 *
 * THAT PROXY HAS ONE PRECONDITION, and it is the caller's to keep: the driver must
 * never click `fea-mode-collapse-toggle`. Collapsing makes `bodyTestId` absent for
 * a reason unrelated to `enabled`, which would turn this guard's central reading
 * into a silent lie rather than a failure.
 *
 * Assert on `exists` only, never on `visible`: `visible` folds in `rect.width > 0`,
 * and an option-less `<select>`'s intrinsic width is a WebKit default rather than
 * anything this property is about. `exists` maps one-to-one onto the `<Show>` gate.
 *
 * Branch table, in order — the order is a CONTRACT, pinned by the suite:
 *   1. unhealthy `clickResult`          → {@link describeRpcFailure}'s verbatim text
 *   2. `clickResult.ok !== true`        → answered and declined
 *   3. `clickResult.matchCount` present → the scope did not disambiguate
 *   4. `otherToggle.exists !== true`    → `otherPaneId` is not ADDRESSABLE at all
 *   5. `otherBefore.exists !== false`   → dirty baseline; no verdict is possible
 *   6. `drivenBefore.exists !== false`  → the click toggled the driven pane OFF
 *   7. `drivenAfter.exists !== true`    → the click never landed
 *   8. `otherAfter.exists !== false`    → CROSS-PANE BLEED
 *
 * BRANCH 3 IS WHY A BARE `{ok: true}` IS EVIDENCE, not just an absence: since
 * `paneDiagnostics` emits `{viewportId, matchCount}` only ABOVE one match, a click
 * made while several candidates exist in the DOM — which the caller must have
 * established first, that is what `describeUnscopedAmbiguity` is for — answers
 * bare `{ok: true}` ONLY if `viewportId` narrowed the resolve to exactly one. The
 * pair's presence says the pane was still guessed, one level down.
 *
 * BRANCH 4 IS THE SECOND ANTI-VACUITY PRECONDITION, and it closes a hole
 * `describeUnscopedAmbiguity` cannot reach. `dom_query` collapses BOTH resolver
 * absences — `notFound` and `notFoundForViewport` — to `{exists: false}`, so a
 * `viewportId` naming a pane that does not exist answers `exists: false` both
 * before and after the click, and branches 5-8 would certify "untouched" having
 * observed literally nothing. The unscoped probe establishes only the COUNT (two
 * elements carry the testid SOMEWHERE); it says nothing about WHICH panes, and the
 * driver deliberately does not assert the pane it names, since DOM order is not a
 * contract. So `otherPaneId` must be shown addressable POSITIVELY, by resolving
 * its own `toggleTestId`. The DRIVEN pane needs no such probe: if it were
 * unaddressable the scoped click itself would fail with the resolver's
 * `notFoundForViewport`, which branch 1 already quotes verbatim.
 *
 * BRANCH 6 EXISTS BECAUSE THE CLICK IS A TOGGLE, NOT A SET. If the driven pane's
 * body is ALREADY open when the scenario runs — a re-run against a GUI that a
 * previous failed run left dirty, persisted view state, a reordered scenario —
 * then the click CLOSES it, `drivenAfter.exists` is false, and branch 7 would
 * report "the click never landed" of a click that landed and worked perfectly.
 * Accurate attribution is this module's whole job, so the driven pane gets the
 * same baseline probe the other pane does rather than having its start state
 * assumed.
 *
 * BRANCH 7 PRECEDES BRANCH 8 deliberately: a click that never landed also explains
 * the other pane's state, so reporting the bleed first would send the operator
 * after the wrong cause.
 *
 * Never throws, whatever the shape of the five payloads.
 *
 * @param {object} options
 * @param {string} options.drivenPaneId  The `viewportId` the click was scoped to.
 * @param {string} options.otherPaneId   The pane that must be untouched.
 * @param {string} options.toggleTestId  The testid that was clicked; also the probe
 *        that proves `otherPaneId` is addressable.
 * @param {string} options.bodyTestId    The testid whose presence proxies `enabled`.
 * @param {any} options.clickResult      Decoded scoped `click_element` result.
 * @param {any} options.otherToggle      `dom_query(toggleTestId, otherPaneId)` — the
 *        addressability probe; valid at any time, since the toggle sits in the
 *        toolbar HEADER, outside the body the `<Show>` gate opens and closes.
 * @param {any} options.otherBefore      `dom_query(bodyTestId, otherPaneId)` BEFORE.
 * @param {any} options.drivenBefore     `dom_query(bodyTestId, drivenPaneId)` BEFORE.
 * @param {any} options.drivenAfter      `dom_query(bodyTestId, drivenPaneId)` AFTER.
 * @param {any} options.otherAfter       `dom_query(bodyTestId, otherPaneId)` AFTER.
 * @returns {string | null} The diagnosis, or `null` when the property holds.
 */
export function describeScopedToggleNoBleed({
  drivenPaneId,
  otherPaneId,
  toggleTestId,
  bodyTestId,
  clickResult,
  otherToggle,
  otherBefore,
  drivenBefore,
  drivenAfter,
  otherAfter,
}) {
  const clickLabel = `click_element(viewportId: '${drivenPaneId}')`;
  const clickFailure = describeRpcFailure(clickResult, clickLabel);
  if (clickFailure) return clickFailure;

  if (clickResult.ok !== true) {
    return `${clickLabel} did not report ok: ${JSON.stringify(clickResult)}`;
  }

  if (clickResult.matchCount !== undefined) {
    return (
      `${clickLabel} still resolved ${clickResult.matchCount} elements ` +
      `(first match attributed to viewportId ${JSON.stringify(clickResult.viewportId)}) — ` +
      `paneDiagnostics reports that pair ONLY above one match, so the viewportId scope failed to ` +
      `disambiguate and the driven pane was still guessed.`
    );
  }

  const notAddressable = describeExistsProbe(
    otherToggle,
    `dom_query('${toggleTestId}', viewportId: '${otherPaneId}')`,
    true,
    `${otherPaneId} is not ADDRESSABLE: its own ${toggleTestId} does not resolve, so that pane was ` +
      `renamed, is absent, or renders no FEA toolbar at all. dom_query collapses BOTH resolver ` +
      `absences (notFound and notFoundForViewport) to exists:false, so every ${bodyTestId} probe ` +
      `below would answer "closed" for a pane that is not there and no-bleed would be certified ` +
      `having observed nothing.`,
  );
  if (notAddressable) return notAddressable;

  const dirtyBaseline = describeExistsProbe(
    otherBefore,
    `dom_query('${bodyTestId}', viewportId: '${otherPaneId}') pre-click`,
    false,
    `the no-bleed baseline was already dirty: ${otherPaneId}'s ${bodyTestId} was present BEFORE the ` +
      `click scoped to ${drivenPaneId}, so "still present afterwards" proves nothing and an ` +
      `unchanged verdict would be meaningless. Re-run against a freshly launched GUI.`,
  );
  if (dirtyBaseline) return dirtyBaseline;

  const drivenAlreadyOpen = describeExistsProbe(
    drivenBefore,
    `dom_query('${bodyTestId}', viewportId: '${drivenPaneId}') pre-click`,
    false,
    `${drivenPaneId}'s ${bodyTestId} was ALREADY present before the click, so this click toggled it ` +
      `OFF rather than on — click_element calls el.click(), a TOGGLE and not an idempotent set. Any ` +
      `"the click never landed" verdict below would be a misattribution: the click landed and did ` +
      `exactly what it was asked. Re-run against a freshly launched GUI; a previous failed run may ` +
      `have exited before restoring this pane.`,
  );
  if (drivenAlreadyOpen) return drivenAlreadyOpen;

  const notLanded = describeExistsProbe(
    drivenAfter,
    `dom_query('${bodyTestId}', viewportId: '${drivenPaneId}') post-click`,
    true,
    `the scoped click did not land on ${drivenPaneId}: its ${bodyTestId} is still absent afterwards, ` +
      `so that pane's FEA body never opened. Reported before any cross-pane verdict, since a click ` +
      `that never landed also explains the other pane's state.`,
  );
  if (notLanded) return notLanded;

  const bleed = describeExistsProbe(
    otherAfter,
    `dom_query('${bodyTestId}', viewportId: '${otherPaneId}') post-click`,
    false,
    `CROSS-PANE BLEED: the click scoped to ${drivenPaneId} also changed ${otherPaneId} — its ` +
      `${bodyTestId} appeared, so that pane's FEA mode was enabled too. This is precisely the ` +
      `pre-#5891 behaviour, where an unscoped resolver drove whichever pane matched first.`,
  );
  if (bleed) return bleed;

  return null;
}
