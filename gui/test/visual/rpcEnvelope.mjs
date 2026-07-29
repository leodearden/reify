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
