// The §2a in-band-error discriminator and the text-block parse live in
// ./rpcEnvelope.mjs — the single JS-side home for decoding debug-MCP responses,
// shared with the six live smoke drivers (task 5731). The `.mjs` extension is
// literal and correct: those drivers run under bare `node`, so the shared module
// cannot be TypeScript, and gui/tsconfig.json's `include: ["src"]` means tsc
// never resolves this directory anyway (vite/vitest do). isInBandError's doc
// there carries the CROSS-LANGUAGE INVARIANT that used to live here.
//
// Only those two primitives are shared. parseRpcResponse keeps its OWN branch
// table below, and that is deliberate: it collapses every failure into
// {ok:false,error} for the typed harness, while normalizeRpcEnvelope preserves
// the in-band shape so a driver can tell a tool OUTAGE from a wrong-shaped
// answer. The two differ in five branches — the transport-error rendering, a
// missing `result`, isError's positional-vs-search text lookup, the image branch
// below, and a malformed text field — each pinned side by side in
// ./rpc.test.ts's "documented divergence" suite. Read that before collapsing
// these two into one.
import { isInBandError, parseTextPayload } from "./rpcEnvelope.mjs";

/**
 * Typed result from parseRpcResponse — either a successful value or an error string.
 */
export type RpcResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

/**
 * Parse an MCP tools/call response envelope into a typed RpcResult.
 *
 * Branch table (evaluated in order):
 * 1. Envelope has top-level `error` field → ok:false from error.message
 * 2. `result.isError === true` → ok:false from content[0].text or "(unknown error)"
 * 3. `content[0].type === "image"` → ok:true with { data: content[0].data }
 * 4. `content[0].type === "text"` → try JSON.parse:
 *    - if parsed value is an in-band error object ({error:<string>}) → ok:false
 *    - otherwise → ok:true with parsed value (or raw string if non-JSON)
 * 5. Otherwise → if result object is an in-band error → ok:false; else ok:true
 *
 * In-band errors (Branches 4 & 5): debug handlers return Ok({error:<string>,...})
 * rather than setting MCP isError. See docs/debug-mcp-contract.md §2a.
 */
export function parseRpcResponse<T = unknown>(envelope: unknown): RpcResult<T> {
  const env = envelope as Record<string, unknown>;

  // Branch 1: transport-level error
  if (env.error !== undefined) {
    const err = env.error as { message?: string };
    return { ok: false, error: err.message ?? String(env.error) };
  }

  const result = env.result as Record<string, unknown> | undefined;
  if (result === undefined) {
    return { ok: false, error: "No result in RPC response" };
  }

  // Branch 2: tool-level error (isError flag)
  if (result.isError === true) {
    const content = result.content as Array<{ type: string; text?: string }> | undefined;
    const text = content?.[0]?.text ?? "(unknown error)";
    return { ok: false, error: text };
  }

  const content = result.content as Array<Record<string, unknown>> | undefined;

  if (Array.isArray(content) && content.length > 0) {
    const first = content[0];

    // Branch 3: image content
    if (first.type === "image") {
      if (typeof first.data !== "string") {
        return { ok: false, error: "image content missing data field" };
      }
      return { ok: true, value: { data: first.data } as unknown as T };
    }

    // Branch 4: text content — try JSON parse, fall back to raw string.
    // After parsing, check for the in-band {error:<string>} envelope before
    // returning ok:true — debug handlers use Ok({error:...}) for failures.
    if (first.type === "text") {
      if (typeof first.text !== "string") {
        return { ok: false, error: "text content missing text field" };
      }
      // Equivalent to the try/JSON.parse/catch this replaced: the only value
      // parseTextPayload newly routes through the in-band check is the raw
      // string from a failed parse, and a string is never an in-band error.
      const parsed = parseTextPayload(first.text);
      if (isInBandError(parsed)) {
        return { ok: false, error: parsed.error };
      }
      return { ok: true, value: parsed as T };
    }
  }

  // Branch 5: no recognisable content — return result object.
  // Also check for the in-band {error:<string>} envelope (defensive coverage
  // for Ok({error}) results that arrive as a bare object rather than text).
  if (isInBandError(result)) {
    return { ok: false, error: result.error };
  }
  return { ok: true, value: result as unknown as T };
}
