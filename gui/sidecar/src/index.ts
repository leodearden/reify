import type { Readable, Writable } from 'node:stream';
import { createLineReader, parseInboundMessage, sendMessage } from './ipc.js';
import { createPermissionServer } from './permission-server.js';
import { SidecarSession } from './session.js';
import { probeLandlockAsync } from './sandbox.js';
import { buildSystemPrompt } from './system-prompt.js';
import { errorMessage } from './utils.js';

/**
 * Sidecar entrypoint. Wires IPC streams to a SidecarSession.
 *
 * @param input  Readable stream for inbound messages (defaults to process.stdin)
 * @param output Writable stream for outbound messages (defaults to process.stdout)
 */
export async function main(
  input: Readable = process.stdin,
  output: Writable = process.stdout
): Promise<void> {
  // Workspace dir: parent dir of the active editor file at sidecar-spawn time.
  // Set by the Rust host via REIFY_WORKSPACE; falls back to cwd when absent.
  const workspace = process.env.REIFY_WORKSPACE ?? process.cwd();

  // Path to the vendored landlock_exec.py sandbox helper.
  // Set by the Rust host via REIFY_LANDLOCK_EXEC when the resource file exists.
  // Empty string is treated as absent (no sandbox).
  const landlockExec = process.env.REIFY_LANDLOCK_EXEC || undefined;

  // Start the in-process MCP permission server and run the async landlock probe
  // concurrently. Total startup latency = max(perm-server-start, probe) rather
  // than their sum. Both must complete before the session is constructed and
  // ready is emitted. If the permission server fails, exit with a structured error.
  const permissionServer = createPermissionServer();
  let landlockAvailable = false;
  try {
    [, landlockAvailable] = await Promise.all([
      permissionServer.start(),
      landlockExec ? probeLandlockAsync(landlockExec) : Promise.resolve(false),
    ]);
  } catch (err: unknown) {
    // Surface the failure as a structured outbound so the host sees a clean
    // error rather than a silent hang, then exit.
    const message = `Failed to start permission server: ${errorMessage(err)}`;
    await sendMessage(output, { type: 'error', id: '', message });
    return;
  }

  const systemPrompt = buildSystemPrompt({
    workingDirectory: process.cwd(),
  });

  const session = new SidecarSession({
    model: 'claude-opus-4-6',
    workingDirectory: process.cwd(),
    systemPrompt,
    permissionMcp: {
      url: permissionServer.url(),
      server: permissionServer,
    },
    workspace,
    landlockExec,
    landlockAvailable,
  });

  // Wire session output to the writable stream
  session.onOutput = (msg) => {
    sendMessage(output, msg).catch((err: unknown) => {
      console.error('Failed to send outbound message:', err);
    });
  };

  // Handle graceful shutdown
  const shutdown = () => {
    session.destroy();
    input.destroy();
  };
  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);

  // Initialize session (emits ready)
  await session.init();

  // Process inbound messages — fire-and-forget so the loop can read the
  // next line (e.g. abort) while a send_message handler is still in flight.
  try {
    for await (const line of createLineReader(input)) {
      try {
        const msg = parseInboundMessage(line);
        session.handleMessage(msg).catch((err: unknown) => {
          const message = errorMessage(err);
          sendMessage(output, { type: 'error', id: '', message }).catch((e: unknown) => {
            console.error('Failed to send error message:', e);
          });
        });
      } catch (err: unknown) {
        const message = errorMessage(err);
        await sendMessage(output, { type: 'error', id: '', message });
      }
    }
  } finally {
    process.removeListener('SIGTERM', shutdown);
    process.removeListener('SIGINT', shutdown);
    session.destroy();
    // Stop the permission server last (idempotent; safe to call even if already stopped).
    await permissionServer.stop();
  }
}
