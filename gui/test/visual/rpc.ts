/**
 * Typed result from parseRpcResponse — either a successful value or an error string.
 */
export type RpcResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

/**
 * Detect the in-band error shape returned by reify-debug handlers.
 *
 * Debug handlers return failures as Ok(json!({"error":"<msg>",...})) — no MCP
 * isError flag is set, so the error rides inside the text/result content block
 * and must be distinguished from success values at the JS layer.
 *
 * Discriminator: a non-null object whose `.error` field is a string.
 *
 * CROSS-LANGUAGE INVARIANT (docs/debug-mcp-contract.md §2a): No success handler
 * in debug_server.rs may return a response payload with a top-level string `error`
 * field. If any handler did, its success response would be silently turned into
 * {ok:false} here. When adding a new handler to debug_server.rs, use a distinct
 * key (e.g. `lastError`, `warningMessage`) if the success payload needs to surface
 * an error-like field. The §2a contract note is the authoritative source; this
 * comment mirrors it for discoverability from the JS consumer side.
 */
function inBandError(v: unknown): v is { error: string } {
  return v !== null && typeof v === "object" && typeof (v as Record<string, unknown>).error === "string";
}

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
      const text = first.text;
      try {
        const parsed = JSON.parse(text);
        if (inBandError(parsed)) {
          return { ok: false, error: parsed.error };
        }
        return { ok: true, value: parsed as T };
      } catch {
        return { ok: true, value: text as unknown as T };
      }
    }
  }

  // Branch 5: no recognisable content — return result object.
  // Also check for the in-band {error:<string>} envelope (defensive coverage
  // for Ok({error}) results that arrive as a bare object rather than text).
  if (inBandError(result)) {
    return { ok: false, error: result.error };
  }
  return { ok: true, value: result as unknown as T };
}
