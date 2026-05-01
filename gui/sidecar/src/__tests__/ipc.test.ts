import { describe, it, expect } from 'vitest';
import { PassThrough } from 'node:stream';
import { parseInboundMessage, formatOutboundMessage, createLineReader, sendMessage } from '../ipc.js';
import type { OutboundMessage } from '../types.js';

describe('parseInboundMessage', () => {
  it('correctly parses send_message with text', () => {
    const line = JSON.stringify({
      type: 'send_message',
      id: 'msg-1',
      text: 'Hello world',
    });
    const msg = parseInboundMessage(line);
    expect(msg).toEqual({
      type: 'send_message',
      id: 'msg-1',
      text: 'Hello world',
    });
  });

  it('correctly parses send_message with text and context', () => {
    const line = JSON.stringify({
      type: 'send_message',
      id: 'msg-2',
      text: 'Fix this',
      context: {
        selected_entity: 'bracket',
        diagnostics: ['error at line 5'],
        constraints: ['width > 0'],
      },
    });
    const msg = parseInboundMessage(line);
    expect(msg.type).toBe('send_message');
    if (msg.type === 'send_message') {
      expect(msg.text).toBe('Fix this');
      expect(msg.context?.selected_entity).toBe('bracket');
      expect(msg.context?.diagnostics).toEqual(['error at line 5']);
      expect(msg.context?.constraints).toEqual(['width > 0']);
    }
  });

  it('correctly parses abort message', () => {
    const line = JSON.stringify({ type: 'abort' });
    const msg = parseInboundMessage(line);
    expect(msg).toEqual({ type: 'abort' });
  });

  it('correctly parses clear_session message', () => {
    const line = JSON.stringify({ type: 'clear_session' });
    const msg = parseInboundMessage(line);
    expect(msg).toEqual({ type: 'clear_session' });
  });

  it('throws on invalid JSON', () => {
    expect(() => parseInboundMessage('not json {')).toThrow();
  });

  it('throws on unknown message type', () => {
    const line = JSON.stringify({ type: 'unknown_type' });
    expect(() => parseInboundMessage(line)).toThrow(/unknown.*type/i);
  });

  it('throws on send_message missing id field', () => {
    const line = JSON.stringify({ type: 'send_message', text: 'Hello' });
    expect(() => parseInboundMessage(line)).toThrow(/id/i);
  });

  it('throws on send_message with empty id', () => {
    const line = JSON.stringify({ type: 'send_message', id: '', text: 'Hello' });
    expect(() => parseInboundMessage(line)).toThrow(/id/i);
  });

  it('throws on send_message missing text field', () => {
    const line = JSON.stringify({ type: 'send_message', id: 'msg-1' });
    expect(() => parseInboundMessage(line)).toThrow(/text/i);
  });

  it('correctly parses tool_result inbound message', () => {
    const line = JSON.stringify({
      type: 'tool_result',
      id: 'msg-1',
      tool_name: 'reify_get_diagnostics',
      result: { ok: true },
    });
    const msg = parseInboundMessage(line);
    expect(msg).toEqual({
      type: 'tool_result',
      id: 'msg-1',
      tool_name: 'reify_get_diagnostics',
      result: { ok: true },
    });
  });

  it('throws on tool_result missing id field', () => {
    const line = JSON.stringify({ type: 'tool_result', tool_name: 'reify_get_diagnostics', result: {} });
    expect(() => parseInboundMessage(line)).toThrow(/id/i);
  });

  it('throws on tool_result with empty id', () => {
    const line = JSON.stringify({ type: 'tool_result', id: '', tool_name: 'reify_get_diagnostics', result: {} });
    expect(() => parseInboundMessage(line)).toThrow(/id/i);
  });

  it('throws on tool_result missing tool_name field', () => {
    const line = JSON.stringify({ type: 'tool_result', id: 'msg-1', result: {} });
    expect(() => parseInboundMessage(line)).toThrow(/tool_name/i);
  });

  it('throws on tool_result missing result field', () => {
    const line = JSON.stringify({ type: 'tool_result', id: 'msg-1', tool_name: 'reify_get_diagnostics' });
    expect(() => parseInboundMessage(line)).toThrow(/result/i);
  });
});

describe('formatOutboundMessage', () => {
  it('serializes text_delta correctly', () => {
    const msg: OutboundMessage = { type: 'text_delta', id: 'msg-1', content: 'Hello' };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({ type: 'text_delta', id: 'msg-1', content: 'Hello' });
  });

  it('serializes thinking_delta correctly', () => {
    const msg: OutboundMessage = { type: 'thinking_delta', id: 'msg-1', content: 'Hmm...' };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({ type: 'thinking_delta', id: 'msg-1', content: 'Hmm...' });
  });

  it('serializes tool_call correctly', () => {
    const msg: OutboundMessage = {
      type: 'tool_call',
      id: 'msg-1',
      tool_name: 'reify_get_source',
      tool_input: { file: 'main.ri' },
    };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({
      type: 'tool_call',
      id: 'msg-1',
      tool_name: 'reify_get_source',
      tool_input: { file: 'main.ri' },
    });
  });

  it('serializes tool_result correctly', () => {
    const msg: OutboundMessage = {
      type: 'tool_result',
      id: 'msg-1',
      tool_name: 'reify_get_source',
      result: 'structure Bracket {}',
    };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({
      type: 'tool_result',
      id: 'msg-1',
      tool_name: 'reify_get_source',
      result: 'structure Bracket {}',
    });
  });

  it('serializes done correctly', () => {
    const msg: OutboundMessage = { type: 'done', id: 'msg-1' };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({ type: 'done', id: 'msg-1' });
  });

  it('serializes error correctly', () => {
    const msg: OutboundMessage = { type: 'error', id: 'msg-1', message: 'Auth failed' };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({ type: 'error', id: 'msg-1', message: 'Auth failed' });
  });

  it('serializes ready correctly', () => {
    const msg: OutboundMessage = { type: 'ready' };
    const result = formatOutboundMessage(msg);
    const parsed = JSON.parse(result.trim());
    expect(parsed).toEqual({ type: 'ready' });
  });

  it('always ends with newline', () => {
    const messages: OutboundMessage[] = [
      { type: 'text_delta', id: 'x', content: 'hi' },
      { type: 'thinking_delta', id: 'x', content: 'hmm' },
      { type: 'done', id: 'x' },
      { type: 'error', id: 'x', message: 'fail' },
      { type: 'ready' },
    ];
    for (const msg of messages) {
      const result = formatOutboundMessage(msg);
      expect(result.endsWith('\n')).toBe(true);
    }
  });
});

describe('createLineReader', () => {
  it('yields complete JSON lines from a readable stream', async () => {
    const stream = new PassThrough();
    const lines: string[] = [];

    const reader = createLineReader(stream);
    const collecting = (async () => {
      for await (const line of reader) {
        lines.push(line);
      }
    })();

    stream.write('{"type":"abort"}\n');
    stream.write('{"type":"clear_session"}\n');
    stream.end();

    await collecting;
    expect(lines).toEqual(['{"type":"abort"}', '{"type":"clear_session"}']);
  });

  it('handles partial reads (message split across chunks)', async () => {
    const stream = new PassThrough();
    const lines: string[] = [];

    const reader = createLineReader(stream);
    const collecting = (async () => {
      for await (const line of reader) {
        lines.push(line);
      }
    })();

    stream.write('{"type":');
    stream.write('"abort"}\n');
    stream.end();

    await collecting;
    expect(lines).toEqual(['{"type":"abort"}']);
  });

  it('handles multiple messages in one chunk', async () => {
    const stream = new PassThrough();
    const lines: string[] = [];

    const reader = createLineReader(stream);
    const collecting = (async () => {
      for await (const line of reader) {
        lines.push(line);
      }
    })();

    stream.write('{"type":"abort"}\n{"type":"clear_session"}\n');
    stream.end();

    await collecting;
    expect(lines).toEqual(['{"type":"abort"}', '{"type":"clear_session"}']);
  });
});

describe('sendMessage', () => {
  it('writes JSON + newline to a writable stream', async () => {
    const stream = new PassThrough();
    const chunks: string[] = [];
    stream.on('data', (chunk: Buffer) => chunks.push(chunk.toString()));

    const msg: OutboundMessage = { type: 'text_delta', id: 'msg-1', content: 'hello' };
    await sendMessage(stream, msg);

    expect(chunks.join('')).toBe('{"type":"text_delta","id":"msg-1","content":"hello"}\n');
  });

  it('handles backpressure (waits for drain)', async () => {
    const stream = new PassThrough({ highWaterMark: 1 }); // tiny buffer
    const chunks: string[] = [];
    stream.on('data', (chunk: Buffer) => chunks.push(chunk.toString()));

    const msg: OutboundMessage = { type: 'text_delta', id: 'msg-1', content: 'a long message to fill the buffer' };
    await sendMessage(stream, msg);

    const fullOutput = chunks.join('');
    const parsed = JSON.parse(fullOutput.trim());
    expect(parsed).toEqual({ type: 'text_delta', id: 'msg-1', content: 'a long message to fill the buffer' });
  });
});
