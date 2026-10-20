import { describe, it, expect } from 'vitest';
import {
  claimToolUse,
  forgetToolUses,
  initialStreamState,
  mapStreamEvent,
  type StreamState,
  type ToolUseClaim,
} from '../streamEvents.js';
import type { OutboundMessage } from '../types.js';

function fold(events: unknown[], start: StreamState = initialStreamState('inv-1')) {
  let state = start;
  const outputs: OutboundMessage[] = [];
  for (const event of events) {
    const step = mapStreamEvent(event, state);
    outputs.push(...step.outputs);
    state = step.state;
  }
  return { outputs, state };
}

function assistant(content: unknown, messageId: string | null = 'msg_1') {
  const message = messageId === null ? { content } : { id: messageId, content };
  return { type: 'assistant', message };
}

const text = (t: string) => ({ type: 'text', text: t });
const thinking = (t: string) => ({ type: 'thinking', thinking: t });
const toolUse = (id: string, name: string, input?: Record<string, unknown>) =>
  input === undefined ? { type: 'tool_use', id, name } : { type: 'tool_use', id, name, input };
const toolResultBlock = (toolUseId: string, content: unknown) =>
  ({ type: 'tool_result', tool_use_id: toolUseId, content });

const textDelta = (content: string) => ({ type: 'text_delta', id: 'inv-1', content });
const thinkingDelta = (content: string) => ({ type: 'thinking_delta', id: 'inv-1', content });

describe('mapStreamEvent: deltas within a turn', () => {
  it('emits only the new suffix of a growing text block', () => {
    const { outputs } = fold([assistant([text('Hel')]), assistant([text('Hello')])]);
    expect(outputs).toEqual([textDelta('Hel'), textDelta('lo')]);
  });

  it('emits only the new suffix of a growing thinking block', () => {
    const { outputs } = fold([assistant([thinking('Let')]), assistant([thinking('Let me')])]);
    expect(outputs).toEqual([thinkingDelta('Let'), thinkingDelta(' me')]);
  });
});

describe('mapStreamEvent: turn boundary', () => {
  it('restarts the text offset when message.id changes, even for a longer new turn', () => {
    const { outputs } = fold([
      assistant([text('Hi')], 'msg_1'),
      assistant([text('Hello, a much longer second turn')], 'msg_2'),
    ]);
    expect(outputs).toEqual([textDelta('Hi'), textDelta('Hello, a much longer second turn')]);
  });

  it('restarts the thinking offset when message.id changes, even for a longer new turn', () => {
    const { outputs } = fold([
      assistant([thinking('Hm')], 'msg_1'),
      assistant([thinking('Let me reason about this at length')], 'msg_2'),
    ]);
    expect(outputs).toEqual([thinkingDelta('Hm'), thinkingDelta('Let me reason about this at length')]);
  });

  it('notices a missing message.id exactly once, first, without resetting offsets', () => {
    const { outputs } = fold([
      assistant([text('Hello')], null),
      assistant([text('Hello world')], null),
    ]);
    const notices = outputs.filter((o) => o.type === 'notice');
    expect(notices).toHaveLength(1);
    expect(outputs[0]).toMatchObject({ type: 'notice', id: 'inv-1', code: 'degraded_turn_boundary' });
    expect(outputs[0]).toHaveProperty('message', expect.stringMatching(/message\.id/));
    expect(outputs.slice(1)).toEqual([textDelta('Hello'), textDelta(' world')]);
  });

  it('keeps the offset across an event missing message.id, then restarts at the next new id', () => {
    const { outputs } = fold([
      assistant([text('Hello')], 'msg_1'),
      assistant([text('Hello world')], null),
      assistant([text('Goodbye')], 'msg_2'),
    ]);
    expect(outputs.filter((o) => o.type === 'notice')).toHaveLength(1);
    expect(outputs.filter((o) => o.type !== 'notice')).toEqual([
      textDelta('Hello'),
      textDelta(' world'),
      textDelta('Goodbye'),
    ]);
  });
});

describe('mapStreamEvent: tool blocks', () => {
  it('emits one tool_call for a tool_use repeated by later partial events', () => {
    const block = toolUse('toolu_1', 'reify_x');
    const { outputs } = fold([assistant([block]), assistant([block]), assistant([text('ok'), block])]);
    expect(outputs.filter((o) => o.type === 'tool_call')).toEqual([
      { type: 'tool_call', id: 'inv-1', tool_use_id: 'toolu_1', tool_name: 'reify_x', tool_input: {} },
    ]);
  });

  it('forwards the tool_use input when present', () => {
    const { outputs } = fold([assistant([toolUse('toolu_1', 'reify_x', { path: 'a.ri' })])]);
    expect(outputs).toEqual([
      { type: 'tool_call', id: 'inv-1', tool_use_id: 'toolu_1', tool_name: 'reify_x', tool_input: { path: 'a.ri' } },
    ]);
  });

  it('names a tool_result block after its registered tool_use', () => {
    const { outputs } = fold([
      assistant([toolUse('toolu_1', 'reify_x')]),
      assistant([toolResultBlock('toolu_1', 'the result')]),
    ]);
    expect(outputs.at(-1)).toEqual({ type: 'tool_result', id: 'inv-1', tool_name: 'reify_x', result: 'the result' });
  });

  it('names a tool_result block for an unregistered tool_use with the empty string', () => {
    const { outputs } = fold([assistant([toolResultBlock('toolu_unknown', { ok: true })])]);
    expect(outputs).toEqual([{ type: 'tool_result', id: 'inv-1', tool_name: '', result: { ok: true } }]);
  });
});

describe('mapStreamEvent: result event', () => {
  it('reports the turn finished with the result session id, emitting nothing', () => {
    const { state: registered } = fold([assistant([toolUse('toolu_1', 'reify_x')])]);
    const step = mapStreamEvent({ type: 'result', session_id: 'sess-1' }, registered);
    expect(step.outputs).toEqual([]);
    expect(step.finished).toEqual({ sessionId: 'sess-1' });
    expect(step.state).toBe(registered);
  });

  it.each([
    ['no session id', { type: 'result' }],
    ['an empty session id', { type: 'result', session_id: '' }],
  ])('does not report the turn finished for a result carrying %s', (_label, event) => {
    const step = mapStreamEvent(event, initialStreamState('inv-1'));
    expect(step.outputs).toEqual([]);
    expect(step.finished).toBeUndefined();
  });

  it('never reports an assistant event as finishing the turn', () => {
    const step = mapStreamEvent(assistant([text('Hi')]), initialStreamState('inv-1'));
    expect(step.finished).toBeUndefined();
  });
});

describe('mapStreamEvent: unrecognised input', () => {
  it.each([
    ['a system event', { type: 'system', subtype: 'init' }],
    ['a user event', { type: 'user', message: { role: 'user', content: [] } }],
    ['a stream_event', { type: 'stream_event', event: { type: 'content_block_delta' } }],
    ['null', null],
    ['a number', 42],
    ['a string', 'x'],
    ['an array', []],
  ])('maps %s to nothing and returns the same state', (_label, event) => {
    const state = initialStreamState('inv-1');
    const step = mapStreamEvent(event, state);
    expect(step.outputs).toEqual([]);
    expect(step.state).toBe(state);
    expect(step.finished).toBeUndefined();
  });

  it('skips unrecognised block kinds but still maps their siblings', () => {
    const { outputs } = fold([assistant([{ type: 'server_tool_use', id: 'srv_1' }, text('Hi')])]);
    expect(outputs).toEqual([textDelta('Hi')]);
  });
});

describe('mapStreamEvent: malformed recognised events', () => {
  it('throws naming message.content when content is not an array', () => {
    expect(() => mapStreamEvent(assistant({ type: 'text', text: 'x' }), initialStreamState('inv-1')))
      .toThrow(/message\.content/);
  });

  it('throws naming message.content when a content entry is not an object', () => {
    expect(() => mapStreamEvent(assistant([null]), initialStreamState('inv-1')))
      .toThrow(/message\.content/);
  });
});

describe('mapStreamEvent: purity', () => {
  it('never mutates the input state and returns a new state when anything changed', () => {
    const { state: registered } = fold([assistant([text('Hel'), toolUse('toolu_1', 'reify_x')])]);
    const events = [
      assistant([text('Hello'), toolUse('toolu_1', 'reify_x'), toolUse('toolu_2', 'reify_x')]),
      assistant([text('Hi')], 'msg_2'),
      assistant([text('No id')], null),
    ];
    for (const event of events) {
      const snapshot = structuredClone(registered);
      const step = mapStreamEvent(event, registered);
      expect(registered).toEqual(snapshot);
      expect(step.state).not.toBe(registered);
    }
  });
});

function claimedState(claim: ToolUseClaim): StreamState {
  if (!claim.ok) throw new Error(`expected a successful claim, got: ${claim.error}`);
  return claim.state;
}

const announced = (...blocks: unknown[]) => fold([assistant(blocks)]).state;

describe('claimToolUse: echoed tool_use_id', () => {
  it('claims a registered id under its own name exactly once', () => {
    const claim = claimToolUse(announced(toolUse('toolu_1', 'reify_x')), 'reify_x', 'toolu_1');
    expect(claim).toMatchObject({ ok: true, toolUseId: 'toolu_1' });

    const again = claimToolUse(claimedState(claim), 'reify_x', 'toolu_1');
    expect(again).toEqual({
      ok: false,
      error: expect.stringMatching(/unknown tool_use_id=toolu_1 for tool_name=reify_x/),
    });
  });

  it('refuses a registered id claimed under a different name', () => {
    const claim = claimToolUse(announced(toolUse('toolu_1', 'reify_x')), 'reify_y', 'toolu_1');
    expect(claim).toEqual({
      ok: false,
      error: expect.stringMatching(/tool_use_id=toolu_1 does not match tool_name=reify_y \(registered as reify_x\)/),
    });
  });

  it('drains the claimed id from the per-name queue', () => {
    const state = announced(toolUse('toolu_1', 'reify_x'), toolUse('toolu_2', 'reify_x'));
    const afterEcho = claimedState(claimToolUse(state, 'reify_x', 'toolu_1'));
    expect(claimToolUse(afterEcho, 'reify_x')).toMatchObject({ ok: true, toolUseId: 'toolu_2' });
  });
});

describe('claimToolUse: per-name queue without an echoed id', () => {
  it('claims same-name tool_uses oldest first, then reports none pending', () => {
    const state = announced(toolUse('toolu_1', 'reify_x'), toolUse('toolu_2', 'reify_x'));

    const first = claimToolUse(state, 'reify_x');
    expect(first).toMatchObject({ ok: true, toolUseId: 'toolu_1' });
    const second = claimToolUse(claimedState(first), 'reify_x');
    expect(second).toMatchObject({ ok: true, toolUseId: 'toolu_2' });
    expect(claimToolUse(claimedState(second), 'reify_x')).toEqual({
      ok: false,
      error: expect.stringMatching(/no pending tool_use for tool_name=reify_x/),
    });
  });
});

describe('claimToolUse: lifetime and purity', () => {
  it('keeps a tool_use claimable across a turn boundary', () => {
    const { state } = fold([
      assistant([toolUse('toolu_1', 'reify_x')], 'msg_1'),
      assistant([text('next turn')], 'msg_2'),
    ]);
    expect(claimToolUse(state, 'reify_x', 'toolu_1')).toMatchObject({ ok: true, toolUseId: 'toolu_1' });
  });

  it('never consumes the tool_use from the state it was given', () => {
    const state = announced(toolUse('toolu_1', 'reify_x'));
    claimToolUse(state, 'reify_x', 'toolu_1');
    claimToolUse(state, 'reify_y', 'toolu_1');
    claimToolUse(state, 'reify_x');
    expect(claimToolUse(state, 'reify_x', 'toolu_1')).toMatchObject({ ok: true, toolUseId: 'toolu_1' });
  });
});

describe('forgetToolUses', () => {
  it('makes every announced tool_use unknown', () => {
    const forgotten = forgetToolUses(announced(toolUse('toolu_1', 'reify_x'), toolUse('toolu_2', 'reify_x')));
    expect(claimToolUse(forgotten, 'reify_x', 'toolu_1')).toEqual({
      ok: false,
      error: expect.stringMatching(/unknown tool_use_id=toolu_1 for tool_name=reify_x/),
    });
    expect(claimToolUse(forgotten, 'reify_x')).toEqual({
      ok: false,
      error: expect.stringMatching(/no pending tool_use for tool_name=reify_x/),
    });
  });

  it('keeps the text offset, so the next delta is only the new suffix', () => {
    const forgotten = forgetToolUses(announced(text('Hello'), toolUse('toolu_1', 'reify_x')));
    const { outputs } = fold([assistant([text('Hello world')])], forgotten);
    expect(outputs).toEqual([textDelta(' world')]);
  });
});
