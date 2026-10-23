import type { OutboundMessage } from './types.js';

/**
 * What one claude CLI invocation's stream-json stdout has established so far. Every
 * transition in this module is pure: it returns a new state and never mutates its input.
 */
export interface StreamState {
  /** The send_message id every output of this invocation is correlated to. */
  readonly invocationId: string;
  /** How much of the current turn's text / thinking block has already been emitted. */
  readonly lastTextLen: number;
  readonly lastThinkingLen: number;
  /** The current turn's message.id; a different id starts a new turn. */
  readonly assistantMessageId: string | null;
  readonly missingMessageIdNoticed: boolean;
  /** Every tool_use announced in this invocation, by tool_use_id; names its tool_result blocks. */
  readonly toolNameById: ReadonlyMap<string, string>;
}

export interface StreamStep {
  readonly outputs: OutboundMessage[];
  readonly state: StreamState;
  /** Present when the event was the result that ends the turn: the CLI session id it carries. */
  readonly finished?: { readonly sessionId: string };
}

const DEGRADED_TURN_BOUNDARY_DETAIL =
  'assistant event missing message.id — turn-boundary detection disabled for this invocation; ' +
  'multi-turn delta offsets may be incorrect if accumulated text length is not reset between turns.';

type JsonObject = Record<string, unknown>;

export function initialStreamState(invocationId: string): StreamState {
  return {
    invocationId,
    lastTextLen: 0,
    lastThinkingLen: 0,
    assistantMessageId: null,
    missingMessageIdNoticed: false,
    toolNameById: new Map(),
  };
}

/**
 * The outputs one parsed stream-json event implies, and the state after it. Event and
 * block kinds the chat does not render map to nothing with the same state; an assistant
 * event whose content is not an array of block objects throws.
 */
export function mapStreamEvent(event: unknown, state: StreamState): StreamStep {
  if (!isObject(event)) return withoutOutputs(state);
  switch (event.type) {
    case 'assistant':
      return mapAssistantMessage(event.message, state);
    case 'result':
      return mapResult(event.session_id, state);
    default:
      return withoutOutputs(state);
  }
}

function mapAssistantMessage(message: unknown, state: StreamState): StreamStep {
  if (!isObject(message) || message.content === undefined || message.content === null) {
    return withoutOutputs(state);
  }
  const blocks = contentBlocks(message.content);
  return blocks.reduce(
    (step, block) => andThen(step, (current) => mapBlock(block, current)),
    enterTurn(message.id, state),
  );
}

function contentBlocks(content: unknown): JsonObject[] {
  if (!Array.isArray(content)) {
    throw new Error(`malformed assistant event: message.content is ${kindOf(content)}, expected an array of content blocks`);
  }
  const blocks: JsonObject[] = [];
  for (const [index, block] of content.entries()) {
    if (!isObject(block)) {
      throw new Error(`malformed assistant event: message.content[${index}] is ${kindOf(block)}, expected a content block object`);
    }
    blocks.push(block);
  }
  return blocks;
}

function enterTurn(messageId: unknown, state: StreamState): StreamStep {
  if (typeof messageId === 'string') {
    if (messageId === state.assistantMessageId) return withoutOutputs(state);
    return withoutOutputs({ ...state, assistantMessageId: messageId, lastTextLen: 0, lastThinkingLen: 0 });
  }
  if (state.missingMessageIdNoticed) return withoutOutputs(state);
  return {
    outputs: [{ type: 'notice', id: state.invocationId, code: 'degraded_turn_boundary', message: DEGRADED_TURN_BOUNDARY_DETAIL }],
    state: { ...state, missingMessageIdNoticed: true },
  };
}

function mapBlock(block: JsonObject, state: StreamState): StreamStep {
  switch (block.type) {
    case 'text':
      return mapTextBlock(block.text, state);
    case 'thinking':
      return mapThinkingBlock(block.thinking, state);
    case 'tool_use':
      return mapToolUseBlock(block, state);
    case 'tool_result':
      return mapToolResultBlock(block, state);
    default:
      return withoutOutputs(state);
  }
}

function mapTextBlock(text: unknown, state: StreamState): StreamStep {
  const delta = unseenSuffix(text, state.lastTextLen);
  if (delta === null) return withoutOutputs(state);
  return {
    outputs: [{ type: 'text_delta', id: state.invocationId, content: delta }],
    state: { ...state, lastTextLen: state.lastTextLen + delta.length },
  };
}

function mapThinkingBlock(thinking: unknown, state: StreamState): StreamStep {
  const delta = unseenSuffix(thinking, state.lastThinkingLen);
  if (delta === null) return withoutOutputs(state);
  return {
    outputs: [{ type: 'thinking_delta', id: state.invocationId, content: delta }],
    state: { ...state, lastThinkingLen: state.lastThinkingLen + delta.length },
  };
}

function mapToolUseBlock(block: JsonObject, state: StreamState): StreamStep {
  const { id, name, input } = block;
  if (typeof id !== 'string' || id === '' || typeof name !== 'string' || state.toolNameById.has(id)) {
    return withoutOutputs(state);
  }
  return {
    outputs: [{
      type: 'tool_call',
      id: state.invocationId,
      tool_use_id: id,
      tool_name: name,
      tool_input: isObject(input) ? input : {},
    }],
    state: { ...state, toolNameById: new Map(state.toolNameById).set(id, name) },
  };
}

function mapToolResultBlock(block: JsonObject, state: StreamState): StreamStep {
  const toolUseId = block.tool_use_id;
  const toolName = typeof toolUseId === 'string' ? state.toolNameById.get(toolUseId) ?? '' : '';
  return {
    outputs: [{ type: 'tool_result', id: state.invocationId, tool_name: toolName, result: block.content }],
    state,
  };
}

function mapResult(sessionId: unknown, state: StreamState): StreamStep {
  if (typeof sessionId !== 'string' || sessionId === '') return withoutOutputs(state);
  return { outputs: [], state, finished: { sessionId } };
}

function unseenSuffix(full: unknown, emittedLen: number): string | null {
  return typeof full === 'string' && full.length > emittedLen ? full.slice(emittedLen) : null;
}

function andThen(step: StreamStep, next: (state: StreamState) => StreamStep): StreamStep {
  const after = next(step.state);
  return { outputs: [...step.outputs, ...after.outputs], state: after.state };
}

function withoutOutputs(state: StreamState): StreamStep {
  return { outputs: [], state };
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function kindOf(value: unknown): string {
  if (value === null) return 'null';
  return Array.isArray(value) ? 'an array' : `a ${typeof value}`;
}
