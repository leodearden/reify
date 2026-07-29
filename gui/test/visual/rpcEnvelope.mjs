/**
 * The single JS-side home for decoding debug-MCP `tools/call` responses
 * (task 5731).
 *
 * `docs/debug-mcp-contract.md` §2 is authoritative and defines three response
 * shapes a caller must tell apart. This module owns the JS decode of all three:
 *
 *   §2a  frontend in-band error — a handler answers `Ok(json!({"error": "<msg>", ...}))`
 *        with NO MCP `isError` flag, so the failure rides inside a normal text
 *        content block                                    → {@link isInBandError}
 *   §2b  Rust-dispatched handler error — an MCP envelope with `isError: true`
 *        carrying a plain-text `Error: <msg>` block        → folded into the §2a
 *        shape by {@link normalizeRpcEnvelope}
 *   §2c  JSON-RPC method error — a top-level `error` on the envelope itself
 *                                                          → `transportError`
 *
 * Before this module, that decode was copy-pasted across six `.mjs` smoke
 * drivers and `./meshCountParity.mjs`. Only the last copy had any CI cover, so a
 * change to `debug_server.rs`'s dialects had seven places to chase and six of
 * them could only fail during a live run. `./rpcEnvelope.test.ts` is the pin.
 *
 * PLAIN ESM, NOT TypeScript, and free of `node:` imports — deliberately, and it
 * is a hard constraint rather than a style choice. Both consumer families must
 * load it: the vitest suites (the CI signal, resolved by vite under browser
 * conditions) and the live smoke drivers, which their runners invoke with bare
 * `node`, never `tsx`. That constraint is exactly why the driver-side copy could
 * not simply import `./rpc.ts` and became copy 2 in the first place.
 *
 * NOT a replacement for `parseRpcResponse` (`./rpc.ts`). That one is the typed
 * TS harness's rendering: it collapses every failure into `{ok: false, error}`,
 * discarding whether the tool answered at all. This module preserves the in-band
 * shape so {@link isInBandError} can still discriminate a tool OUTAGE from a
 * wrong-shaped answer downstream. The two share the §2a discriminator and the
 * text parse below; the rest of their branch tables deliberately differ, and
 * that divergence is pinned side by side in `./rpc.test.ts`.
 */

/**
 * Detect the in-band error shape returned by reify-debug handlers.
 *
 * Debug handlers report failure as `Ok(json!({"error": "<msg>", ...}))` — no MCP
 * `isError` flag is set, so the error rides inside the content block and is
 * indistinguishable from a success value to a naive caller. The `rpc()` transport
 * ({@link makeDebugRpc}) only throws on TRANSPORT errors and hands back the
 * decoded text block (see {@link normalizeRpcEnvelope}, which shapes it but
 * deliberately does not judge it), so without this check a tool-level failure
 * would surface as `undefined` fields and get misreported as a shape problem
 * instead of the outage it is.
 *
 * Discriminator: a non-null object whose `error` field is a string. This is the
 * single home of the §2a discriminator — `./rpc.ts` imports it rather than
 * keeping its own copy — and `docs/debug-mcp-contract.md` §2a is authoritative.
 *
 * CROSS-LANGUAGE INVARIANT (docs/debug-mcp-contract.md §2a): No success handler
 * in `debug_server.rs` may return a response payload with a top-level string
 * `error` field. If any handler did, its success response would be silently read
 * as a failure here. When adding a handler, use a distinct key (e.g. `lastError`,
 * `warningMessage`) if the success payload needs to surface an error-like field.
 *
 * NOTE — the OTHER failure dialect: Rust-dispatched tools (`engine_state`,
 * `mesh_stats`, `demand_dispatch`) surface a handler error as an MCP envelope
 * with `isError: true` carrying a plain-text `Error: <msg>` block
 * (debug_server.rs), NOT this JSON shape. {@link normalizeRpcEnvelope} folds that
 * envelope into `{error: "<text>"}` on the way out precisely so this one detector
 * covers both dialects.
 *
 * Exported so a driver can distinguish "the tool failed" from "the tool
 * answered" BEFORE it starts interpreting fields — see the selectivity
 * precondition in `./smoke_mesh_count_parity_e2e.mjs`, where reading a failed
 * `demand_dispatch` as `full_scope !== false` would blame the frontend for a
 * tool outage.
 *
 * @param {unknown} v
 * @returns {boolean}
 */
export function isInBandError(v) {
  return v !== null && typeof v === "object" && typeof (/** @type {any} */ (v).error) === "string";
}

/**
 * Decode one MCP text content block: JSON when it parses, the raw text otherwise.
 *
 * The debug server emits `serde_json::to_string_pretty` for a succeeding tool, so
 * in practice the raw-string branch is reached only by §2b's `Error: <msg>` text
 * and by deliberately-plain answers such as `health`'s. Callers must still handle
 * it — a live run is exactly where the unparseable case shows up.
 *
 * SAFE TO SHARE, and this is why: a raw-string fallback is never an object, so it
 * can never satisfy {@link isInBandError}. Hoisting this idiom out of
 * `parseRpcResponse`'s Branch 4 (`./rpc.ts`) therefore cannot introduce a new
 * in-band-error verdict at any call site — the only newly-reachable value on that
 * path is a string, which both decoders already treat as a plain payload.
 *
 * Never throws.
 *
 * @param {string} text
 * @returns {unknown}
 */
export function parseTextPayload(text) {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/**
 * @typedef {object} NormalizedRpcEnvelope
 * @property {string} [transportError] Set exactly when the JSON-RPC envelope itself
 *           carried a top-level `error` — the caller should THROW, not interpret.
 * @property {unknown} [payload] The tool's answer, normalised so that BOTH failure
 *           dialects satisfy {@link isInBandError}. `null` means "no text block to
 *           interpret" (e.g. an image content block from `screenshot`).
 */

/**
 * Normalise an MCP `tools/call` response envelope into one interpretable payload.
 *
 * This is the transport seam of every live smoke driver, lifted out of them so
 * the one genuinely subtle branch — the `isError` fold below — is covered by the
 * vitest suite rather than living only in files CI can never run.
 *
 * Branch table, in order:
 *   1. top-level `error`      → `{transportError}`; the caller throws. NOT a payload:
 *                               a dead/unreachable server must not read as a tool answer.
 *   2. `result.isError === true` → `{payload: {error: "<text block>"}}` — THE FOLD.
 *                               Rust-dispatched handlers (`engine_state`, `mesh_stats`,
 *                               `demand_dispatch`) report failure this way, with a plain-text
 *                               `Error: <msg>` block (debug_server.rs), while frontend-mediated
 *                               tools use the in-band JSON dialect. Folding the former into the
 *                               latter is what lets ONE `isInBandError` check cover both; without
 *                               it an engine-thread-died failure arrives as the bare string
 *                               "Error: …" and gets misreported downstream as
 *                               `mesh_stats payload is not an object` — a tool OUTAGE dressed up
 *                               as a shape problem.
 *   3. no text block          → `{payload: null}` (image content, empty/absent content).
 *   4. text block, JSON       → `{payload: <parsed>}`. A frontend in-band `{error: "<msg>"}`
 *                               passes through UNCHANGED — it already speaks the target dialect.
 *   5. text block, non-JSON   → `{payload: <raw text>}`.
 *
 * DELIBERATELY NOT `parseRpcResponse` (./rpc.ts), which the typed TS harness uses:
 * that one collapses every failure into `{ok: false, error}`, discarding whether the
 * tool answered at all. This normaliser preserves the in-band shape precisely so
 * `isInBandError` can discriminate outage-from-answer downstream. The two decoders
 * now live one import apart and share the §2a discriminator and
 * {@link parseTextPayload}, so the remaining divergence is easy to mistake for an
 * oversight: it is not, and it is characterized side by side in ./rpc.test.ts.
 *
 * Never throws, whatever the shape of `envelope`.
 *
 * @param {unknown} envelope
 * @returns {NormalizedRpcEnvelope}
 */
export function normalizeRpcEnvelope(envelope) {
  const env = /** @type {any} */ (
    envelope !== null && typeof envelope === "object" ? envelope : {}
  );

  if (env.error) {
    return { transportError: `RPC error: ${JSON.stringify(env.error)}` };
  }

  const content = env.result?.content;
  const textBlock = Array.isArray(content)
    ? content.find((c) => c !== null && typeof c === "object" && c.type === "text")
    : undefined;

  if (env.result?.isError === true) {
    return {
      payload: {
        error:
          typeof textBlock?.text === "string"
            ? textBlock.text
            : "tool reported isError with no text content block",
      },
    };
  }

  if (!textBlock || typeof textBlock.text !== "string") return { payload: null };

  return { payload: parseTextPayload(textBlock.text) };
}

/**
 * Build the `rpc(method, args)` helper every live smoke driver calls.
 *
 * This is the ONE home for the debug-MCP `tools/call` request shape. All six
 * drivers had a byte-identical copy of it; one definition here means a change to
 * the wire format has a single edit site, and — because the drivers themselves
 * can never run in CI — it is the only form in which that request shape can be
 * covered by a test at all (`./rpcEnvelope.test.ts`).
 *
 * TWO FAILURE DIALECTS, one normalised shape — see {@link normalizeRpcEnvelope}
 * for the fold. Frontend-mediated tools (`viewport_state` and friends, via
 * `query_frontend`) report failure in-band as JSON `{error: "<msg>"}`;
 * Rust-dispatched tools (`engine_state`, `mesh_stats`, `demand_dispatch`) instead
 * answer with `isError: true` plus a plain-text `Error: <msg>` block
 * (debug_server.rs). Both reach the caller as `{error: "<msg>"}`, so one
 * {@link isInBandError} check covers every tool.
 *
 * THE THROW/RESOLVE SPLIT is the contract, and it is load-bearing: a §2c
 * TRANSPORT error THROWS, while a tool error RESOLVES to `{error}`. That is what
 * lets a driver's `waitForServer` poll inside a bare `catch {}` — a throw means
 * "server not up yet, keep waiting", whereas a resolved `{error}` means the
 * server answered and the TOOL failed, which no amount of waiting will fix.
 *
 * `id: 1` is inherited verbatim from the drivers: each issues its requests
 * strictly sequentially and never matches a response back to its id, so a
 * constant is safe. A concurrent caller would need real ids.
 *
 * @param {string} debugUrl  The `/mcp` endpoint, e.g. `http://127.0.0.1:3939/mcp`.
 * @param {{fetchImpl?: typeof fetch}} [options]  `fetchImpl` defaults to the
 *        global `fetch` (Node 18+, which the drivers already assume). It exists
 *        so the suite can exercise this function without a live server.
 * @returns {(method: string, args?: Record<string, unknown>) => Promise<unknown>}
 */
export function makeDebugRpc(debugUrl, options = {}) {
  const fetchImpl = options.fetchImpl ?? fetch;

  return async function rpc(method, args = {}) {
    const res = await fetchImpl(debugUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "tools/call",
        params: { name: method, arguments: args },
      }),
    });
    const { transportError, payload } = normalizeRpcEnvelope(await res.json());
    if (transportError !== undefined) throw new Error(transportError);
    return payload;
  };
}
