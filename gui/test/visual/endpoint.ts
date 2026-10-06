import * as net from 'node:net';

const DEFAULT_DEBUG_PORT = 3939;

/**
 * A TCP port from a raw env value, or undefined unless it is 1..65535 written
 * as pure decimal digits — strict, so whitespace-padded (" 4500 ") and
 * trailing garbage ("4500x") that parseInt would silently accept are rejected.
 * Same grammar as the Rust `parse_tcp_port` in `gui/src-tauri/src/tcp_port.rs`;
 * keep the two in lockstep.
 */
function parsePort(raw: string | undefined): number | undefined {
  if (raw === undefined || !/^\d+$/.test(raw)) return undefined;
  const parsed = parseInt(raw, 10);
  return parsed >= 1 && parsed <= 65535 ? parsed : undefined;
}

/**
 * Resolve the reify-debug port the GUI binds: what `parsePort` accepts, else
 * DEFAULT_DEBUG_PORT (3939), mirroring the fallback of `parse_debug_port` in
 * `gui/src-tauri/src/debug_server.rs`.
 *
 * Cross-ref: `gui/sidecar/src/session.ts` `resolveReifyDebugUrl` uses identical
 * logic.  Keep all three in lockstep if the rules change.
 */
export function resolveDebugPort(env: Record<string, string | undefined> = process.env): number {
  return parsePort(env['REIFY_DEBUG_PORT']) ?? DEFAULT_DEBUG_PORT;
}

/** The GUI ports a harness run chooses for itself and hands to the launcher. */
export type PerRunPortVar = 'REIFY_DEBUG_PORT' | 'REIFY_VITE_PORT';

/**
 * Resolve one GUI port for a single harness run: a valid `env[name]` is
 * honoured; unset or invalid, a free port is allocated, so concurrent runs
 * never contend for a shared default (:3939, :1420).  The caller must pass the
 * result to the launched GUI — reify-gui binds REIFY_DEBUG_PORT and retargets
 * its devUrl to REIFY_VITE_PORT (gui/src-tauri/src/dev_url.rs).  Same rule as
 * `resolve_port <VAR>` in lib_e2e_smoke.sh.
 */
export async function resolvePerRunPort(
  name: PerRunPortVar,
  env: Record<string, string | undefined> = process.env,
  allocate: () => Promise<number> = allocateFreePort,
): Promise<number> {
  return parsePort(env[name]) ?? allocate();
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
