import * as net from 'node:net';

const DEFAULT_DEBUG_PORT = 3939;

/**
 * A TCP port from a raw env value, or undefined unless it is 1..65535 written
 * as pure decimal digits — strict, so whitespace-padded (" 4500 ") and
 * trailing garbage ("4500x") that parseInt would silently accept are rejected.
 */
function parsePort(raw: string | undefined): number | undefined {
  if (raw === undefined || !/^\d+$/.test(raw)) return undefined;
  const parsed = parseInt(raw, 10);
  return parsed >= 1 && parsed <= 65535 ? parsed : undefined;
}

/**
 * Resolve the reify-debug port from the environment.
 *
 * Accepts only what `parsePort` accepts, matching the Rust `parse_debug_port`
 * contract in `debug_server.rs`. Falls back to DEFAULT_DEBUG_PORT (3939) for
 * unset / empty / non-digit / out-of-range input.
 *
 * Cross-ref: `gui/sidecar/src/session.ts` `resolveReifyDebugUrl` uses identical
 * validation logic; `gui/src-tauri/src/debug_server.rs` `parse_debug_port` is
 * the Rust source-of-truth.  Keep all three in lockstep if rules change.
 */
export function resolveDebugPort(env: Record<string, string | undefined> = process.env): number {
  return parsePort(env['REIFY_DEBUG_PORT']) ?? DEFAULT_DEBUG_PORT;
}

/**
 * Resolve the vite dev-server port for one harness run: a valid
 * REIFY_VITE_PORT is honoured, as for the debug port; otherwise a free port is
 * allocated, so concurrent harness runs never contend for :1420. reify-gui
 * follows it via its startup devUrl retarget (gui/src-tauri/src/dev_url.rs).
 */
export async function resolveVitePort(
  env: Record<string, string | undefined> = process.env,
  allocate: () => Promise<number> = allocateFreePort,
): Promise<number> {
  return parsePort(env['REIFY_VITE_PORT']) ?? allocate();
}

export function debugUrlForPort(port: number): string {
  return `http://127.0.0.1:${port}/mcp`;
}

/**
 * Allocate a free ephemeral port on localhost by briefly binding port 0.
 *
 * There is an inherent TOCTOU window between when this server closes the port
 * and when the child process re-binds it — another process can grab the port
 * in the gap.  This is an accepted limitation for a test harness.  Callers
 * that need stronger collision avoidance can retry the GUI spawn on
 * EADDRINUSE with a freshly allocated port.
 */
export function allocateFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      const port = typeof addr === 'object' && addr !== null ? addr.port : 0;
      server.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
    server.on('error', reject);
  });
}
