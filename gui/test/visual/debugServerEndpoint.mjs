/**
 * Where the reify-debug server is and when it answers: the ONE home of the
 * debug-port rule and of the drivers' readiness poll, used by every
 * `smoke_*.mjs` driver and by `./endpoint.ts` / `./run.ts`.
 *
 * PLAIN ESM with no imports and no `process` global: the drivers load it under
 * a bare `node`, while vitest loads it through vite's browser resolver, so the
 * caller passes `env` in. Pinned by `./debugServerEndpoint.test.ts`.
 */

/**
 * The port reify-gui's debug server binds when `REIFY_DEBUG_PORT` is unset or
 * invalid. Mirrors the fallback of `parse_debug_port` in
 * `gui/src-tauri/src/debug_server.rs`; keep the two in lockstep.
 */
export const DEFAULT_DEBUG_PORT = 3939;

/**
 * A TCP port from a raw env value, or undefined unless it is 1..65535 written
 * as pure decimal digits — strict, so whitespace-padded (" 4500 ") and
 * trailing garbage ("4500x") that parseInt would silently accept are rejected.
 * Same grammar as the Rust `parse_tcp_port` in `gui/src-tauri/src/tcp_port.rs`;
 * keep the two in lockstep.
 *
 * @param {string | undefined} raw
 * @returns {number | undefined}
 */
export function parsePort(raw) {
  if (raw === undefined || !/^\d+$/.test(raw)) return undefined;
  const parsed = parseInt(raw, 10);
  return parsed >= 1 && parsed <= 65535 ? parsed : undefined;
}

/**
 * The reify-debug port the GUI binds: what {@link parsePort} accepts in
 * `env.REIFY_DEBUG_PORT`, else {@link DEFAULT_DEBUG_PORT}.
 *
 * @param {Record<string, string | undefined>} env  Normally `process.env`.
 * @returns {number}
 */
export function resolveDebugPort(env) {
  return parsePort(env['REIFY_DEBUG_PORT']) ?? DEFAULT_DEBUG_PORT;
}

/**
 * @param {number} port
 * @returns {string} The debug server's MCP endpoint on localhost.
 */
export function debugUrlForPort(port) {
  return `http://127.0.0.1:${port}/mcp`;
}

/** How long {@link waitForServer} waits for a first `health` answer by default. */
const SERVER_READY_TIMEOUT_MS = 60_000;

/** The pause between {@link waitForServer}'s `health` polls by default. */
const SERVER_READY_POLL_MS = 500;

/**
 * Poll `health` until the debug server answers, and FAIL LOUDLY when the budget
 * runs out.
 *
 * Per rpcEnvelope's throw/resolve split, a rejecting `rpc` is a transport that
 * is not up yet and a `null` payload carries no answer, so both keep polling.
 * Any other payload — an in-band `{error}` included — means the server
 * answered, and waiting longer cannot fix a tool error.
 *
 * @param {(method: string, args?: Record<string, unknown>) => Promise<unknown>} rpc
 *        The driver's `rpc` helper, normally from `makeDebugRpc`.
 * @param {number} port  Named in the failure message.
 * @param {object} [options]
 * @param {number} [options.timeoutMs]  Defaults to {@link SERVER_READY_TIMEOUT_MS}.
 * @param {number} [options.pollMs]     Defaults to {@link SERVER_READY_POLL_MS}.
 * @param {(ms: number) => Promise<unknown>} [options.sleepImpl]  Defaults to a real timer.
 * @param {() => number} [options.now]  Defaults to `Date.now`; the budget is
 *        measured on this clock, not by counting polls.
 * @param {(message: string) => void} [options.fail]  Defaults to THROWING. A
 *        driver passes its own `fail`; the throwing default exists so a caller
 *        that forgets cannot silently continue against an unready server.
 * @returns {Promise<void>}
 */
export async function waitForServer(rpc, port, options = {}) {
  const {
    timeoutMs = SERVER_READY_TIMEOUT_MS,
    pollMs = SERVER_READY_POLL_MS,
    sleepImpl = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    now = () => Date.now(),
    fail = (message) => {
      throw new Error(message);
    },
  } = options;

  const deadline = now() + timeoutMs;
  while (now() < deadline) {
    try {
      const r = await rpc('health');
      if (r !== null) return;
    } catch {}
    await sleepImpl(pollMs);
  }
  fail(`Debug server not ready on port ${port} after ${timeoutMs}ms`);
}
