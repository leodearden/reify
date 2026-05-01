import type { Readable, Writable } from 'node:stream';
import type { InboundMessage, OutboundMessage } from './types.js';

const VALID_INBOUND_TYPES = new Set(['send_message', 'abort', 'clear_session', 'tool_result']);

/**
 * Parse a JSON line into an InboundMessage.
 * Throws on invalid JSON or unknown message type.
 */
export function parseInboundMessage(line: string): InboundMessage {
  const parsed = JSON.parse(line);
  if (!parsed || typeof parsed !== 'object' || !('type' in parsed)) {
    throw new Error('Invalid inbound message: missing type field');
  }
  if (!VALID_INBOUND_TYPES.has(parsed.type)) {
    throw new Error(`Unknown inbound message type: ${parsed.type}`);
  }
  if (parsed.type === 'send_message') {
    if (typeof parsed.id !== 'string' || !parsed.id) {
      throw new Error('send_message requires a non-empty "id" field');
    }
    if (typeof parsed.text !== 'string') {
      throw new Error('send_message requires a "text" field');
    }
  }
  return parsed as InboundMessage;
}

/**
 * Serialize an OutboundMessage to a JSON line (with trailing newline).
 */
export function formatOutboundMessage(msg: OutboundMessage): string {
  return JSON.stringify(msg) + '\n';
}

/**
 * Async generator that reads from a Readable stream and yields
 * complete lines (newline-delimited). Buffers partial reads.
 */
export async function* createLineReader(readable: Readable): AsyncGenerator<string> {
  let buffer = '';
  for await (const chunk of readable) {
    buffer += typeof chunk === 'string' ? chunk : (chunk as Buffer).toString('utf-8');
    let newlineIndex: number;
    while ((newlineIndex = buffer.indexOf('\n')) !== -1) {
      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      if (line.length > 0) {
        yield line;
      }
    }
  }
  // Yield any remaining content after stream ends
  if (buffer.length > 0) {
    yield buffer;
  }
}

/**
 * Write an OutboundMessage to a Writable stream as JSON + newline.
 * Handles backpressure by waiting for 'drain' if needed.
 */
export async function sendMessage(writable: Writable, msg: OutboundMessage): Promise<void> {
  const data = formatOutboundMessage(msg);
  const canContinue = writable.write(data);
  if (!canContinue) {
    await new Promise<void>((resolve) => writable.once('drain', resolve));
  }
}
