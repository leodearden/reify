import type { Readable, Writable } from 'node:stream';
import { createLineReader, parseInboundMessage, sendMessage } from './ipc.js';
import { SidecarSession } from './session.js';
import { buildSystemPrompt } from './system-prompt.js';

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
  const systemPrompt = buildSystemPrompt({
    workingDirectory: process.cwd(),
  });

  const session = new SidecarSession({
    model: 'claude-opus-4-6',
    workingDirectory: process.cwd(),
    systemPrompt,
  });

  // Wire session output to the writable stream
  session.onOutput = (msg) => {
    sendMessage(output, msg).catch((err: unknown) => {
      console.error('Failed to send outbound message:', err);
    });
  };

  // Handle graceful shutdown
  const shutdown = () => {
    input.destroy();
  };
  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);

  // Initialize session (emits ready)
  await session.init();

  // Process inbound messages
  try {
    for await (const line of createLineReader(input)) {
      try {
        const msg = parseInboundMessage(line);
        await session.handleMessage(msg);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        await sendMessage(output, { type: 'error', id: '', message });
      }
    }
  } finally {
    process.removeListener('SIGTERM', shutdown);
    process.removeListener('SIGINT', shutdown);
  }
}
