import type { InboundMessage, OutboundMessage } from './types.js';

const VALID_INBOUND_TYPES = new Set(['send_message', 'abort', 'clear_session']);

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
  return parsed as InboundMessage;
}

/**
 * Serialize an OutboundMessage to a JSON line (with trailing newline).
 */
export function formatOutboundMessage(msg: OutboundMessage): string {
  return JSON.stringify(msg) + '\n';
}
