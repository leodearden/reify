import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { OutboundMessage } from '../types.js';

// Mock the claude CLI subprocess spawning
vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

import { spawn } from 'node:child_process';
import { SidecarSession } from '../session.js';
import { main } from '../index.js';

/**
 * Create a mock child process that emits streaming JSON events on stdout,
 * then closes with the given exit code.
 */
function createMockProcess(events: object[], exitCode = 0): any {
  const proc = new EventEmitter() as any;
  const stdout = new PassThrough();
  proc.stdout = stdout;
  proc.stderr = new PassThrough();
  proc.stdin = new PassThrough();
  proc.exitCode = null;

  process.nextTick(() => {
    for (const event of events) {
      stdout.push(JSON.stringify(event) + '\n');
    }
    stdout.push(null);
    proc.stderr.push(null);
    proc.exitCode = exitCode;
    proc.emit('close', exitCode);
  });

  return proc;
}

describe('SidecarSession', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
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

  it('handleMessage with send_message streams text deltas and emits done', async () => {
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hello' }] } },
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hello world' }] } },
      { type: 'result', session_id: 'sess-123' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-1',
      text: 'Hello',
    });

    const types = outputs.map((o) => o.type);
    expect(types).toContain('text_delta');
    expect(types[types.length - 1]).toBe('done');

    // Check deltas are correct — first "Hello", then " world"
    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    expect(textDeltas[0]).toEqual({ type: 'text_delta', id: 'msg-1', content: 'Hello' });
    expect(textDeltas[1]).toEqual({ type: 'text_delta', id: 'msg-1', content: ' world' });
  });

  it('handleMessage streams thinking deltas', async () => {
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'thinking', thinking: 'Let me' }] } },
      { type: 'assistant', message: { content: [{ type: 'thinking', thinking: 'Let me think' }] } },
      { type: 'assistant', message: { content: [{ type: 'thinking', thinking: 'Let me think' }, { type: 'text', text: 'Answer' }] } },
      { type: 'result', session_id: 'sess-456' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-t', text: 'Think' });

    const thinkDeltas = outputs.filter((o) => o.type === 'thinking_delta');
    expect(thinkDeltas[0]).toEqual({ type: 'thinking_delta', id: 'msg-t', content: 'Let me' });
    expect(thinkDeltas[1]).toEqual({ type: 'thinking_delta', id: 'msg-t', content: ' think' });

    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    expect(textDeltas[0]).toEqual({ type: 'text_delta', id: 'msg-t', content: 'Answer' });
  });

  it('handleMessage emits tool_call events', async () => {
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [
        { type: 'tool_use', id: 'tu-1', name: 'reify_get_source', input: { file: 'main.ri' } },
      ] } },
      { type: 'result', session_id: 'sess-789' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-tc', text: 'Read file' });

    const toolCalls = outputs.filter((o) => o.type === 'tool_call');
    expect(toolCalls).toHaveLength(1);
    expect(toolCalls[0]).toEqual({
      type: 'tool_call',
      id: 'msg-tc',
      tool_name: 'reify_get_source',
      tool_input: { file: 'main.ri' },
    });
  });

  it('handleMessage with abort cancels in-flight request', async () => {
    // Create a process that hangs until aborted
    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();
    mockProc.exitCode = null;

    vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
      // Simulate abort killing the process
      if (opts?.signal) {
        opts.signal.addEventListener('abort', () => {
          stdout.end();
          mockProc.exitCode = null;
          mockProc.emit('close', null);
        });
      }
      return mockProc;
    }) as any);

    await session.init();
    outputs.length = 0;

    // Start a message (will hang waiting for stdout)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-2',
      text: 'Long task',
    });

    // Give it a tick to set up
    await new Promise((r) => setTimeout(r, 10));

    // Abort
    await session.handleMessage({ type: 'abort' });

    // Wait for cleanup
    await msgPromise;

    // Should emit done (not error) on abort
    const doneMsg = outputs.find((o) => o.type === 'done');
    expect(doneMsg).toBeDefined();
    expect(doneMsg).toEqual({ type: 'done', id: 'msg-2' });

    // Should NOT emit an error
    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(0);
  });

  it('handleMessage with clear_session resets session and emits ready', async () => {
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Response' }] } },
      { type: 'result', session_id: 'sess-abc' },
    ])) as any);

    await session.init();
    await session.handleMessage({
      type: 'send_message',
      id: 'msg-3',
      text: 'First message',
    });

    outputs.length = 0;

    await session.handleMessage({ type: 'clear_session' });

    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
    expect((session as any).sessionId).toBeNull();
  });

  it('SDK errors produce error outbound message', async () => {
    // Process exits with non-zero code
    const proc = new EventEmitter() as any;
    const stdout = new PassThrough();
    proc.stdout = stdout;
    proc.stderr = new PassThrough();
    proc.stdin = new PassThrough();
    proc.exitCode = null;

    vi.mocked(spawn).mockImplementation((() => {
      process.nextTick(() => {
        proc.stderr.push('Authentication failed: invalid API key');
        proc.stderr.push(null);
        stdout.push(null);
        proc.exitCode = 1;
        proc.emit('close', 1);
      });
      return proc;
    }) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-4',
      text: 'Hello',
    });

    const errorMsgs = outputs.filter((o) => o.type === 'error');
    expect(errorMsgs).toHaveLength(1);
    expect((errorMsgs[0] as any).message).toContain('Authentication failed');
    expect((errorMsgs[0] as any).id).toBe('msg-4');
  });

  it('tool_result emits correct tool_name from corresponding tool_use block', async () => {
    // The tool_use block has id='toolu_abc' and name='reify_get_source'.
    // The tool_result block references tool_use_id='toolu_abc'.
    // The emitted ToolResult must carry tool_name='reify_get_source', not the UUID 'toolu_abc'.
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      {
        type: 'assistant',
        message: {
          content: [
            { type: 'tool_use', id: 'toolu_abc', name: 'reify_get_source', input: { file: 'main.ri' } },
          ],
        },
      },
      {
        type: 'assistant',
        message: {
          content: [
            { type: 'tool_use', id: 'toolu_abc', name: 'reify_get_source', input: { file: 'main.ri' } },
            { type: 'tool_result', tool_use_id: 'toolu_abc', content: 'file contents' },
          ],
        },
      },
      { type: 'result', session_id: 'sess-tr' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-tr', text: 'Read file' });

    const toolResults = outputs.filter((o) => o.type === 'tool_result');
    expect(toolResults).toHaveLength(1);
    // tool_name must be the actual tool name, NOT the UUID tool_use_id
    expect((toolResults[0] as any).tool_name).toBe('reify_get_source');
    expect((toolResults[0] as any).tool_name).not.toBe('toolu_abc');
    expect((toolResults[0] as any).result).toBe('file contents');
  });

  it('multiple sequential messages use session_id for resume', async () => {
    const mockSpawn = vi.mocked(spawn);

    // First call returns a session_id
    mockSpawn.mockImplementationOnce((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'First' }] } },
      { type: 'result', session_id: 'sess-abc' },
    ])) as any);

    // Second call
    mockSpawn.mockImplementationOnce((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Second' }] } },
      { type: 'result', session_id: 'sess-abc' },
    ])) as any);

    await session.init();
    outputs.length = 0;

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

    // Second spawn call should include --resume with session id
    const secondCallArgs = mockSpawn.mock.calls[1]?.[1] as string[];
    expect(secondCallArgs).toContain('--resume');
    expect(secondCallArgs).toContain('sess-abc');
  });
});

describe('SidecarSession timeout', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('invokeSdk times out after configured timeoutMs', async () => {
    vi.useFakeTimers();

    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
      timeoutMs: 1000,
    });
    session.onOutput = (msg) => outputs.push(msg);

    // Create a process that hangs forever (stdout stays open, no close event)
    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();
    mockProc.exitCode = null;

    vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
      // When aborted, end the process
      if (opts?.signal) {
        opts.signal.addEventListener('abort', () => {
          stdout.end();
          mockProc.exitCode = null;
          mockProc.emit('close', null);
        });
      }
      return mockProc;
    }) as any);

    await session.init();
    outputs.length = 0;

    // Start a message (will hang)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-timeout',
      text: 'Hanging task',
    });

    // Advance time past the timeout
    vi.advanceTimersByTime(1001);

    // Wait for the promise to resolve
    await msgPromise;

    // Should emit an error with timeout message, NOT a done
    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(1);
    expect((errors[0] as any).message).toContain('timed out');
    expect((errors[0] as any).id).toBe('msg-timeout');

    // Should NOT emit a bare done
    const dones = outputs.filter((o) => o.type === 'done');
    expect(dones).toHaveLength(0);

    vi.useRealTimers();
  });

  it('normal completion clears timeout without error', async () => {
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
      timeoutMs: 5000,
    });
    session.onOutput = (msg) => outputs.push(msg);

    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Response' }] } },
      { type: 'result', session_id: 'sess-ok' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({
      type: 'send_message',
      id: 'msg-ok',
      text: 'Quick task',
    });

    // Should emit done, NOT error
    const dones = outputs.filter((o) => o.type === 'done');
    expect(dones).toHaveLength(1);
    expect(dones[0]).toEqual({ type: 'done', id: 'msg-ok' });

    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(0);
  });

  it('user abort still emits done, not timeout error', async () => {
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
      timeoutMs: 60000,
    });
    session.onOutput = (msg) => outputs.push(msg);

    // Create a hanging process that responds to abort
    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();
    mockProc.exitCode = null;

    vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
      if (opts?.signal) {
        opts.signal.addEventListener('abort', () => {
          stdout.end();
          mockProc.exitCode = null;
          mockProc.emit('close', null);
        });
      }
      return mockProc;
    }) as any);

    await session.init();
    outputs.length = 0;

    // Start a message
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-abort',
      text: 'Long task',
    });

    // Give it a tick to set up
    await new Promise((r) => setTimeout(r, 10));

    // User abort (not timeout)
    await session.handleMessage({ type: 'abort' });
    await msgPromise;

    // Should emit done (user abort), NOT error
    const doneMsg = outputs.find((o) => o.type === 'done');
    expect(doneMsg).toBeDefined();
    expect(doneMsg).toEqual({ type: 'done', id: 'msg-abort' });

    // Should NOT emit error
    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(0);
  });

  it('stdout stream error does not leak timeout to next request', async () => {
    vi.useFakeTimers();

    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
      timeoutMs: 60_000,
    });
    session.onOutput = (msg) => outputs.push(msg);

    // First spawn: process whose stdout emits an I/O error
    vi.mocked(spawn).mockImplementationOnce((() => {
      const proc = new EventEmitter() as any;
      const stdout = new PassThrough();
      proc.stdout = stdout;
      proc.stderr = new PassThrough();
      proc.stdin = new PassThrough();
      proc.exitCode = null;

      process.nextTick(() => {
        stdout.destroy(new Error('I/O error'));
        proc.exitCode = 1;
        proc.emit('close', 1);
      });

      return proc;
    }) as any);

    // First request — will fail due to stdout error
    await session.handleMessage({
      type: 'send_message',
      id: 'msg-leak-1',
      text: 'First',
    });

    // Verify first request produced an error
    const firstErrors = outputs.filter((o) => o.type === 'error');
    expect(firstErrors).toHaveLength(1);
    expect((firstErrors[0] as any).id).toBe('msg-leak-1');

    outputs.length = 0;

    // Advance clock partway so leaked timeout fires during second request
    vi.advanceTimersByTime(30_000);

    // Second spawn: normal mock process that completes successfully
    vi.mocked(spawn).mockImplementationOnce((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'OK' }] } },
      { type: 'result', session_id: 'sess-leak' },
    ])) as any);

    // Start second request (registers its own timeout at clock 30000 + 60000 = 90000)
    const secondPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-leak-2',
      text: 'Second',
    });

    // Advance to fire the first request's leaked timeout (at 60000)
    // but NOT the second request's timeout (at 90000)
    vi.advanceTimersByTime(30_001);

    await secondPromise;

    // Second request should complete with done, NOT a spurious timeout error
    const dones = outputs.filter((o) => o.type === 'done');
    expect(dones).toHaveLength(1);
    expect(dones[0]).toEqual({ type: 'done', id: 'msg-leak-2' });

    // Should NOT have a spurious timeout error from the leaked timer
    const timeoutErrors = outputs.filter(
      (o) => o.type === 'error' && 'message' in o && (o as any).message.includes('timed out')
    );
    expect(timeoutErrors).toHaveLength(0);
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
    // Configure spawn mock to return a process that emits streaming events
    const mockSpawn = vi.mocked(spawn);
    mockSpawn.mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Test response' }] } },
      { type: 'result', session_id: 'sess-e2e' },
    ])) as any);

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

    // Should have ready from init, then text_delta and done from the message
    expect(types).toContain('ready');
    expect(types).toContain('text_delta');
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
