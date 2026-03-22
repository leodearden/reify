import { describe, it, expect, vi, beforeEach } from 'vitest';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { OutboundMessage } from '../types.js';

// Mock the claude SDK subprocess spawning
vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

import { spawn } from 'node:child_process';
import { SidecarSession } from '../session.js';
import { main } from '../index.js';

describe('SidecarSession', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-sonnet-4-20250514',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
    });
    session.onOutput = (msg) => outputs.push(msg);
  });

  it('constructor creates session with config', () => {
    expect(session).toBeDefined();
    expect(session).toBeInstanceOf(SidecarSession);
  });

  it('init() calls onOutput with ready message', async () => {
    await session.init();
    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
  });

  it('handleMessage with send_message eventually emits done message', async () => {
    await session.init();
    outputs.length = 0;

    // Mock the internal SDK call to resolve immediately
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockResolvedValue('Test response');

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-1',
      text: 'Hello',
    });

    // Should emit at least a text_delta and done
    const types = outputs.map((o) => o.type);
    expect(types).toContain('text_delta');
    expect(types[types.length - 1]).toBe('done');

    mockInvoke.mockRestore();
  });

  it('handleMessage with abort sets abort flag', async () => {
    await session.init();

    // Start a long-running message
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockImplementation(
      () => new Promise((resolve) => setTimeout(resolve, 10000))
    );

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-2',
      text: 'Long task',
    });

    // Abort immediately
    await session.handleMessage({ type: 'abort' });

    // The abort should cause the SDK call to be cancelled
    expect((session as any).abortController?.signal.aborted).toBe(true);

    mockInvoke.mockRestore();
  });

  it('handleMessage with clear_session resets history and emits ready', async () => {
    await session.init();
    outputs.length = 0;

    // Add some conversation history
    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockResolvedValue('Response');
    await session.handleMessage({
      type: 'send_message',
      id: 'msg-3',
      text: 'First message',
    });

    outputs.length = 0;

    await session.handleMessage({ type: 'clear_session' });

    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
    expect((session as any).conversationHistory).toHaveLength(0);

    mockInvoke.mockRestore();
  });

  it('SDK errors produce error outbound message', async () => {
    await session.init();
    outputs.length = 0;

    const mockInvoke = vi.spyOn(session as any, 'invokeSdk').mockRejectedValue(
      new Error('Authentication failed: invalid API key')
    );

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-4',
      text: 'Hello',
    });

    const errorMsgs = outputs.filter((o) => o.type === 'error');
    expect(errorMsgs).toHaveLength(1);
    expect((errorMsgs[0] as any).message).toContain('Authentication failed');

    mockInvoke.mockRestore();
  });

  it('multiple sequential messages maintain conversation context', async () => {
    await session.init();
    outputs.length = 0;

    const mockInvoke = vi.spyOn(session as any, 'invokeSdk');
    mockInvoke.mockResolvedValueOnce('First response');
    mockInvoke.mockResolvedValueOnce('Second response');

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-5',
      text: 'Message one',
    });

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-6',
      text: 'Message two',
    });

    // Should have accumulated conversation history
    const history = (session as any).conversationHistory;
    expect(history.length).toBeGreaterThanOrEqual(4); // 2 user msgs + 2 assistant msgs

    mockInvoke.mockRestore();
  });
});

describe('entrypoint wiring', () => {
  /**
   * Helper to collect all newline-delimited JSON messages from
   * the output stream until the stream ends or a timeout.
   */
  function collectOutput(output: PassThrough, timeoutMs = 2000): Promise<OutboundMessage[]> {
    return new Promise((resolve) => {
      const msgs: OutboundMessage[] = [];
      let buffer = '';

      const onData = (chunk: Buffer | string) => {
        buffer += chunk.toString();
        let idx: number;
        while ((idx = buffer.indexOf('\n')) !== -1) {
          const line = buffer.slice(0, idx);
          buffer = buffer.slice(idx + 1);
          if (line.length > 0) {
            msgs.push(JSON.parse(line));
          }
        }
      };

      output.on('data', onData);

      const timer = setTimeout(() => {
        output.removeListener('data', onData);
        resolve(msgs);
      }, timeoutMs);

      output.on('end', () => {
        clearTimeout(timer);
        // flush remaining buffer
        if (buffer.length > 0) {
          msgs.push(JSON.parse(buffer));
        }
        resolve(msgs);
      });
    });
  }

  it('main() reads from provided input stream and writes to provided output stream', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    const collecting = collectOutput(output, 500);

    // Start main with injected streams
    const mainPromise = main(input, output);

    // Give it a moment to set up, then close
    await new Promise((r) => setTimeout(r, 50));
    input.end();
    await mainPromise;

    const msgs = await collecting;
    // main() should produce at least a ready message on the output
    expect(msgs.length).toBeGreaterThanOrEqual(1);
    // All messages should be valid OutboundMessage objects with a 'type' field
    for (const msg of msgs) {
      expect(msg).toHaveProperty('type');
    }
  });

  it('sending a valid JSON line through input produces outbound messages on output', async () => {
    // Configure spawn mock to return a process that emits a successful response
    const mockSpawn = vi.mocked(spawn);
    mockSpawn.mockImplementation((() => {
      const proc = new EventEmitter() as any;
      proc.stdout = new PassThrough();
      proc.stderr = new PassThrough();
      proc.stdin = new PassThrough();
      // Simulate successful CLI response after a tick
      process.nextTick(() => {
        proc.stdout.push('Test response');
        proc.stdout.push(null);
        proc.stderr.push(null);
        proc.emit('close', 0);
      });
      return proc;
    }) as any);

    const input = new PassThrough();
    const output = new PassThrough();

    const collecting = collectOutput(output, 1000);

    const mainPromise = main(input, output);

    // Wait for ready, then send a message
    await new Promise((r) => setTimeout(r, 50));
    input.write(JSON.stringify({ type: 'send_message', id: 'e2e-1', text: 'Hello' }) + '\n');

    // Give session time to process, then close
    await new Promise((r) => setTimeout(r, 200));
    input.end();
    await mainPromise;

    const msgs = await collecting;
    const types = msgs.map((m) => m.type);

    // Should have ready from init, then done from the message processing
    expect(types).toContain('ready');
    expect(types).toContain('done');

    mockSpawn.mockReset();
  });

  it('process sends ready message on startup', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    const collecting = collectOutput(output, 500);

    const mainPromise = main(input, output);

    // Wait for ready
    await new Promise((r) => setTimeout(r, 50));
    input.end();
    await mainPromise;

    const msgs = await collecting;
    // The first message should be 'ready'
    expect(msgs.length).toBeGreaterThanOrEqual(1);
    expect(msgs[0]).toEqual({ type: 'ready' });
  });
});
