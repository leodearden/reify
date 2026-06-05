import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { OutboundMessage, InboundMessage, NoticeMessage } from '../types.js';

// Mock the claude CLI subprocess spawning
vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

// Mock the sandbox module — wrapClaudeArgs only.
// Landlock state is supplied via SessionConfig.landlockAvailable (task 3281);
// the synchronous probe helpers have been removed from sandbox.ts.
vi.mock('../sandbox.js', () => ({
  wrapClaudeArgs: vi.fn((args: string[], _ws: string, le?: string) =>
    le ? { cmd: 'python3', args: [le, ...args] } : { cmd: 'claude', args: [...args] }
  ),
}));

// Mock node:fs with an in-memory virtual filesystem so tests never touch real /tmp.
// session.ts calls mkdtempSync + writeFileSync on the first invokeSdk turn; tests that
// read back the config (e.g. "writes reify-debug MCP config unconditionally") use
// readFileSync and roundtrip through the same virtual map. destroy() calls unlinkSync
// and rmdirSync, which are safe no-ops here since session.ts wraps them in try/catch.
//
// vi.importActual('node:fs') inside individual tests still reaches the real fs module,
// which is how the /tmp leak-guard test (task 3283) observes actual disk state.
vi.mock('node:fs', () => {
  const virtualFiles = new Map<string, string>();
  const virtualDirs = new Set<string>();
  let mkdtempCounter = 0;

  return {
    // Reset helper — called in the top-level beforeEach to clear per-test state.
    // Without this, a path written by test A is visible to test B, and mkdtempCounter
    // grows across the file yielding non-deterministic synthetic paths.
    __resetVirtualFs: (): void => {
      virtualFiles.clear();
      virtualDirs.clear();
      mkdtempCounter = 0;
    },
    mkdtempSync: vi.fn((prefix: string): string => {
      const dir = `${prefix}mock${++mkdtempCounter}`;
      virtualDirs.add(dir);
      return dir;
    }),
    // Accepts string, Buffer (Uint8Array subclass), or any Uint8Array.
    // Throws on other types so a future caller that passes an object gets a clear error
    // rather than the silent '[object Object]' that String(data) would produce.
    writeFileSync: vi.fn((filePath: string, data: unknown): void => {
      if (typeof data === 'string') {
        virtualFiles.set(filePath, data);
      } else if (data instanceof Uint8Array) {
        // Buffer extends Uint8Array; both are handled here.
        virtualFiles.set(filePath, Buffer.from(data).toString('utf-8'));
      } else {
        throw new TypeError(
          `virtual fs writeFileSync: expected string/Buffer/Uint8Array, got ${Object.prototype.toString.call(data)}`
        );
      }
    }),
    // Honors the encoding argument to mirror real Node.js behavior:
    //   readFileSync(path)                        → Buffer
    //   readFileSync(path, 'utf-8')               → string
    //   readFileSync(path, {})                    → Buffer (options object, no encoding key)
    //   readFileSync(path, { encoding: 'utf-8' }) → string
    // This surfaces a divergence loudly if a future caller omits the encoding and
    // expects a Buffer but receives a string (or vice versa). Matching Node's
    // options-object semantics matters because `{}.encoding === undefined`, so a
    // future caller passing `readFileSync(p, opts)` with opts lacking an `encoding`
    // would silently get a string here but a Buffer in production.
    // Note: `{ encoding: '' }` is not fully modeled — real Node throws
    // ERR_UNKNOWN_ENCODING for an empty-string encoding; this mock returns Buffer.
    readFileSync: vi.fn((filePath: string, enc?: unknown): string | Buffer => {
      if (!virtualFiles.has(filePath)) {
        const err = Object.assign(
          new Error(`ENOENT: no such file or directory, open '${filePath}'`),
          { code: 'ENOENT' }
        );
        throw err;
      }
      const content = virtualFiles.get(filePath)!;
      const encoding = typeof enc === 'string' ? enc : (enc as { encoding?: unknown } | null | undefined)?.encoding;
      return typeof encoding === 'string' && encoding.length > 0 ? content : Buffer.from(content);
    }),
    unlinkSync: vi.fn((filePath: string): void => {
      virtualFiles.delete(filePath);
    }),
    rmdirSync: vi.fn((dirPath: string): void => {
      virtualDirs.delete(dirPath);
    }),
  };
});

import { spawn } from 'node:child_process';
import { SidecarSession, SPAWN_ERROR_LOG_PREFIX } from '../session.js';
import { wrapClaudeArgs } from '../sandbox.js';
import * as os from 'node:os';
import { main } from '../index.js';
import * as mockFs from 'node:fs';

// Clear virtual filesystem state before every test.
// The vi.mock('node:fs') factory above holds module-scoped state (virtualFiles,
// virtualDirs, mkdtempCounter) that persists across tests by default. Without this
// reset, a path written by test A is visible to test B, and mkdtempCounter yields
// non-deterministic paths relative to each test's expectations.
beforeEach(() => {
  (mockFs as any).__resetVirtualFs();
});

/**
 * Extract the constructed prompt from the most recent spawn() call.
 * Reads the first JSON line written to the spawned process's stdin PassThrough,
 * parses it as a stream-json user message, and returns the text content.
 */
async function getBuiltPrompt(callIndex = 0): Promise<string> {
  const mockProc = vi.mocked(spawn).mock.results[callIndex]?.value as any;
  const parsed = await drainStdinFirstLine(mockProc);
  return (parsed as any).message.content[0].text as string;
}

/**
 * Drain the first JSON line written to a spawned process's stdin PassThrough.
 * Returns the parsed object.
 */
function drainStdinFirstLine(mockProc: any): Promise<unknown> {
  const stdin = mockProc?.stdin as PassThrough;
  if (!stdin) throw new Error('No stdin on mock process');
  return new Promise((resolve, reject) => {
    let buf = '';
    const onData = (chunk: Buffer | string) => {
      buf += chunk.toString();
      const nlIdx = buf.indexOf('\n');
      if (nlIdx !== -1) {
        stdin.removeListener('data', onData);
        try {
          resolve(JSON.parse(buf.slice(0, nlIdx)));
        } catch (e) {
          reject(e);
        }
      }
    };
    stdin.on('data', onData);
  });
}

/**
 * Event-driven helper: resolves when the next outbound matching predicate is emitted.
 * Hooks session.onOutput, restores the original after match, and resolves with the matching message.
 * Set this up BEFORE the action that produces the event.
 *
 * Accepts an optional `options.timeoutMs` (default 5000ms). If the predicate is never
 * satisfied within that window, rejects with a named error and restores session.onOutput
 * so test isolation is preserved.
 *
 * The returned promise carries a `.cancel()` method that immediately restores
 * `session.onOutput` without settling the promise. **After calling `.cancel()`, do not
 * `await` the returned promise** — it will remain pending until the surrounding test
 * timeout fires. Use `.cancel()` only when you have already obtained a result via
 * `Promise.race` and want to release the output hook without waiting for the
 * default timeout.
 */
function waitForOutput(
  session: SidecarSession,
  predicate: (m: OutboundMessage) => boolean,
  options: { timeoutMs?: number } = {}
): Promise<OutboundMessage> & { cancel: () => void } {
  const timeoutMs = options.timeoutMs ?? 5000;
  const prev = session.onOutput;
  let settled = false;
  let timer: ReturnType<typeof setTimeout>;

  const cancel = () => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    session.onOutput = prev;
    // The promise is left pending (never resolved or rejected).
  };

  const promise = new Promise<OutboundMessage>((resolve, reject) => {
    timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      session.onOutput = prev;
      reject(new Error('waitForOutput timed out waiting for predicate'));
    }, timeoutMs);
    session.onOutput = (msg: OutboundMessage) => {
      prev(msg);
      if (predicate(msg)) {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        session.onOutput = prev;
        resolve(msg);
      }
    };
  });

  return Object.assign(promise, { cancel });
}

/**
 * Event-driven helper: resolves when `count` outbounds matching predicate have been emitted.
 * Hooks session.onOutput, restores the original after the nth match, and resolves with the
 * last matching message. Decouples match counting from the predicate so the predicate stays
 * side-effect-free.
 *
 * Accepts an optional `options.timeoutMs` (default 5000ms). If the nth match is never
 * reached within that window, rejects with a named error and restores session.onOutput
 * so test isolation is preserved.
 */
function waitForOutputs(
  session: SidecarSession,
  predicate: (m: OutboundMessage) => boolean,
  count: number,
  options: { timeoutMs?: number } = {}
): Promise<OutboundMessage> {
  const timeoutMs = options.timeoutMs ?? 5000;
  const prev = session.onOutput;
  return new Promise((resolve, reject) => {
    let matched = 0;
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      session.onOutput = prev;
      reject(new Error('waitForOutputs timed out waiting for predicate'));
    }, timeoutMs);
    session.onOutput = (msg: OutboundMessage) => {
      prev(msg);
      if (predicate(msg)) {
        matched++;
        if (matched >= count) {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          session.onOutput = prev;
          resolve(msg);
        }
      }
    };
  });
}

/**
 * Event-driven helper: resolves when stdinLines.length reaches at least `count`.
 * Hooks stdin's 'data' event and resolves immediately if the threshold is already met.
 * Must be called AFTER the stdinLines accumulator listener is attached to stdin.
 */
function waitForStdinLines(stdin: PassThrough, stdinLines: unknown[], count: number): Promise<void> {
  if (stdinLines.length >= count) return Promise.resolve();
  return new Promise((resolve) => {
    const onData = () => {
      if (stdinLines.length >= count) {
        stdin.removeListener('data', onData);
        resolve();
      }
    };
    stdin.on('data', onData);
  });
}

/**
 * Create a hand-rolled mock subprocess with a PassThrough trio (stdout/stderr/stdin),
 * a stdin-line accumulator (attached BEFORE spawn), and a vi.mocked(spawn) setup that
 * pushes one assistant message with the given content blocks on process.nextTick and
 * leaves stdout open (caller controls when to close it).
 *
 * Use this instead of duplicating the EventEmitter+PassThrough boilerplate in each
 * describe block. The stdinLines array is pre-wired so it captures all writes including
 * the initial prompt write.
 */
function makeMockProc(assistantContent: object[]): {
  mockProc: any;
  stdout: PassThrough;
  stdinLines: unknown[];
} {
  const mockProc = new EventEmitter() as any;
  const stdout = new PassThrough();
  mockProc.stdout = stdout;
  mockProc.stderr = new PassThrough();
  mockProc.stdin = new PassThrough();
  mockProc.exitCode = null;

  // Attach stdin accumulator BEFORE spawn-triggering handleMessage call
  const stdinLines: unknown[] = [];
  let stdinBuf = '';
  mockProc.stdin.on('data', (chunk: Buffer | string) => {
    stdinBuf += chunk.toString();
    let nlIdx: number;
    while ((nlIdx = stdinBuf.indexOf('\n')) !== -1) {
      stdinLines.push(JSON.parse(stdinBuf.slice(0, nlIdx)));
      stdinBuf = stdinBuf.slice(nlIdx + 1);
    }
  });

  vi.mocked(spawn).mockImplementation((() => {
    process.nextTick(() => {
      stdout.push(
        JSON.stringify({
          type: 'assistant',
          message: { content: assistantContent },
        }) + '\n'
      );
      // Leave stdout open — caller controls when to close it
    });
    return mockProc;
  }) as any);

  return { mockProc, stdout, stdinLines };
}

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
      tool_use_id: 'tu-1',
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

  describe('context prompt building', () => {
    beforeEach(async () => {
      vi.mocked(spawn).mockImplementation((() => createMockProcess([
        { type: 'assistant', message: { content: [{ type: 'text', text: 'OK' }] } },
        { type: 'result', session_id: 'sess-ctx' },
      ])) as any);

      await session.init();
      outputs.length = 0;
    });

    it('includes current_file in constructed prompt', async () => {
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-cf',
        text: 'Explain this',
        context: { current_file: 'src/main.ri' },
      });

      const prompt = await getBuiltPrompt();

      expect(prompt).toContain('Explain this');
      expect(prompt).toContain('Current file: src/main.ri');
      expect(prompt).toContain('[Context]');
    });

    it('includes attached_contexts in constructed prompt', async () => {
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-ac',
        text: 'Help me',
        context: { attached_contexts: ['file: lib.ri\nfn add(a, b) = a + b', 'file: util.ri\nfn clamp(v) = max(0, v)'] },
      });

      const prompt = await getBuiltPrompt();

      expect(prompt).toContain('Help me');
      expect(prompt).toContain('[Context]');
      expect(prompt).toContain('Attached contexts:\nfile: lib.ri\nfn add(a, b) = a + b\n\nfile: util.ri\nfn clamp(v) = max(0, v)');
    });

    it('includes all five context fields in prompt', async () => {
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-all',
        text: 'Full context',
        context: {
          current_file: 'src/engine.ri',
          selected_entity: 'Cylinder',
          diagnostics: ['type error on line 5'],
          constraints: ['radius > 0'],
          attached_contexts: ['file: helper.ri\nfn helper() = 42'],
        },
      });

      const prompt = await getBuiltPrompt();

      expect(prompt).toContain('Full context');
      expect(prompt).toContain('[Context]');
      expect(prompt).toContain('Current file: src/engine.ri');
      expect(prompt).toContain('Selected entity: Cylinder');
      expect(prompt).toContain('Diagnostics:\ntype error on line 5');
      expect(prompt).toContain('Constraints:\nradius > 0');
      expect(prompt).toContain('Attached contexts:\nfile: helper.ri\nfn helper() = 42');

      // Verify full ordering chain across all five context fields
      const cfIdx = prompt.indexOf('Current file:');
      const seIdx = prompt.indexOf('Selected entity:');
      const diagIdx = prompt.indexOf('Diagnostics:');
      const constraintsIdx = prompt.indexOf('Constraints:');
      const acIdx = prompt.indexOf('Attached contexts:');
      expect(cfIdx).toBeLessThan(seIdx);
      expect(seIdx).toBeLessThan(diagIdx);
      expect(diagIdx).toBeLessThan(constraintsIdx);
      expect(constraintsIdx).toBeLessThan(acIdx);
    });

    it('empty context object produces no [Context] block', async () => {
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-empty',
        text: 'Some text',
        context: {},
      });

      const prompt = await getBuiltPrompt();

      expect(prompt).not.toContain('[Context]');
      expect(prompt).toBe('Some text');
    });

    it('empty string field is silently skipped with no [Context] block', async () => {
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-falsy',
        text: 'Some text',
        context: { current_file: '' },
      });

      const prompt = await getBuiltPrompt();

      expect(prompt).not.toContain('[Context]');
      expect(prompt).not.toContain('Current file:');
    });
  });

  it('tool_result inbound is forwarded to claude CLI stdin with the matching tool_use_id', async () => {
    // Create a process that emits a tool_use block and stays open
    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();
    mockProc.exitCode = null;

    // Attach stdin accumulator BEFORE the spawn-triggering handleMessage call so no
    // writes are missed even if the PassThrough drains before a late listener attaches.
    const stdinLines: unknown[] = [];
    let stdinBuf = '';
    mockProc.stdin.on('data', (chunk: Buffer | string) => {
      stdinBuf += chunk.toString();
      let nlIdx: number;
      while ((nlIdx = stdinBuf.indexOf('\n')) !== -1) {
        stdinLines.push(JSON.parse(stdinBuf.slice(0, nlIdx)));
        stdinBuf = stdinBuf.slice(nlIdx + 1);
      }
    });

    vi.mocked(spawn).mockImplementation((() => {
      process.nextTick(() => {
        stdout.push(JSON.stringify({
          type: 'assistant',
          message: { content: [
            { type: 'tool_use', id: 'toolu_xyz', name: 'reify_get_diagnostics', input: {} },
          ] },
        }) + '\n');
        // stdout stays open — don't push null yet
      });
      return mockProc;
    }) as any);

    await session.init();
    outputs.length = 0;

    // Set up event-driven wait for tool_call BEFORE the spawn-triggering handleMessage call
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    // Start message without awaiting it (it will hang until stdin closes)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-tr-fwd',
      text: 'Check diagnostics',
    });

    // Wait for the tool_call outbound (event-driven, no polling)
    await toolCallWait;

    // Send the tool_result inbound
    session.handleMessage({
      type: 'tool_result',
      id: 'msg-1',
      tool_use_id: 'toolu_xyz',
      tool_name: 'reify_get_diagnostics',
      result: { diagnostics: [] },
    });

    // Event-driven wait for the second stdin line (no setTimeout flush wait)
    await waitForStdinLines(mockProc.stdin, stdinLines, 2);

    // Close the process
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-tr-fwd' }) + '\n');
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;

    // After the initial user-text message, a second JSON line should have been written
    // with the tool_result content block
    expect(stdinLines.length).toBeGreaterThanOrEqual(2);
    expect(stdinLines[1]).toEqual({
      type: 'user',
      message: {
        role: 'user',
        content: [{
          type: 'tool_result',
          tool_use_id: 'toolu_xyz',
          content: { diagnostics: [] },
        }],
      },
    });
  });

  it('tool_result inbound with no matching tool_use emits error outbound', async () => {
    // No send_message dispatched — pendingToolUseIds is empty
    await session.init();
    outputs.length = 0;

    await session.handleMessage({
      type: 'tool_result',
      id: 'msg-orphan',
      tool_use_id: 'toolu_orphan',
      tool_name: 'reify_unknown',
      result: 'x',
    });

    // Should emit exactly one error with id='msg-orphan' and matching message
    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(1);
    expect((errors[0] as any).id).toBe('msg-orphan');
    expect((errors[0] as any).message).toMatch(/no in-flight|no pending tool_use|no matching tool_use/i);

    // Should NOT have spawned any subprocess
    expect(vi.mocked(spawn)).not.toHaveBeenCalled();
  });

  it('tool_result inbound preserves queue entry when no in-flight stdin', async () => {
    // Simulate stale tool_use carryover: pendingToolUseIds has an entry but currentStdin is null
    // (no invokeSdk in flight). This tests that handleToolResult does NOT shift the queue
    // before confirming currentStdin is available.
    (session as any).pendingToolUseIds.set('reify_get_source', ['toolu_carry']);
    (session as any).toolNameById.set('toolu_carry', 'reify_get_source');
    // currentStdin remains null (no invokeSdk active)

    await session.handleMessage({
      type: 'tool_result',
      id: 'msg-stale',
      tool_use_id: 'toolu_carry',
      tool_name: 'reify_get_source',
      result: 'x',
    });

    // (a) exactly one outbound error should be emitted with matching id and message
    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(1);
    expect((errors[0] as any).id).toBe('msg-stale');
    expect((errors[0] as any).message).toMatch(/no pending tool_use|no in-flight|no matching/i);

    // (b) CRITICAL: the queue entry must still be present — currentStdin was null so
    // handleToolResult must return early WITHOUT consuming (shifting) the FIFO queue.
    // This is a regression guard: if the check order is ever reverted to shift-before-check,
    // this assertion will fail because the queue will be empty.
    const queue = (session as any).pendingToolUseIds.get('reify_get_source');
    expect(queue).toEqual(['toolu_carry']);
  });

  it('invokeSdk uses --input-format stream-json and writes prompt to stdin', async () => {
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hi' }] } },
      { type: 'result', session_id: 'sess-sj' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-x',
      text: 'Hello',
    });

    // Drain stdin BEFORE awaiting the message (so we capture before proc closes)
    const mockProc = vi.mocked(spawn).mock.results[0]?.value as any;
    const stdinMsg = drainStdinFirstLine(mockProc);

    await msgPromise;

    // (a) spawn args include --input-format stream-json
    const callArgs = vi.mocked(spawn).mock.calls[0]?.[1] as string[];
    const inputFormatIdx = callArgs.indexOf('--input-format');
    expect(inputFormatIdx).toBeGreaterThanOrEqual(0);
    expect(callArgs[inputFormatIdx + 1]).toBe('stream-json');

    // (a2) --print + --output-format stream-json requires --verbose (claude CLI rejects
    // the combo otherwise with "When using --print, --output-format=stream-json requires
    // --verbose"). Regression guard.
    expect(callArgs).toContain('--print');
    expect(callArgs).toContain('--verbose');

    // (b) spawn args do NOT contain prompt text as argv tail
    expect(callArgs[callArgs.length - 1]).not.toBe('Hello');

    // (c) stdin received the prompt as a stream-json user message
    const parsed = await stdinMsg;
    expect(parsed).toEqual({
      type: 'user',
      message: {
        role: 'user',
        content: [{ type: 'text', text: 'Hello' }],
      },
    });
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

describe('SidecarSession multi-turn streaming', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('text_delta events emitted for both turns in a multi-turn invocation', async () => {
    // Simulate two assistant turns within a single SDK invocation:
    // Turn 1: thinking + text + tool_use (message.id = 'msg_t1')
    // Turn 2: new text, shorter than turn-1 accumulated length (message.id = 'msg_t2')
    //
    // The new-turn boundary is detected by the message.id change: when event.message.id
    // switches from 'msg_t1' to 'msg_t2', both lastTextLen and lastThinkingLen reset to 0,
    // so turn-2's first delta emits from offset 0 regardless of relative length.
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // Turn 1 partial events (message.id = 'msg_t1')
      { type: 'assistant', message: { id: 'msg_t1', content: [{ type: 'thinking', thinking: 'Let' }] } },
      { type: 'assistant', message: { id: 'msg_t1', content: [{ type: 'thinking', thinking: 'Let me think' }] } },
      { type: 'assistant', message: { id: 'msg_t1', content: [
        { type: 'thinking', thinking: 'Let me think' },
        { type: 'text', text: 'Hello ' },
      ] } },
      { type: 'assistant', message: { id: 'msg_t1', content: [
        { type: 'thinking', thinking: 'Let me think' },
        { type: 'text', text: 'Hello world!' },
      ] } },
      // Turn 1 completes with tool_use
      { type: 'assistant', message: { id: 'msg_t1', content: [
        { type: 'thinking', thinking: 'Let me think' },
        { type: 'text', text: 'Hello world!' },
        { type: 'tool_use', id: 'toolu_mt1', name: 'reify_get_source', input: { file: 'f.ri' } },
      ] } },
      // Turn 2 starts: new message.id triggers counter reset
      { type: 'assistant', message: { id: 'msg_t2', content: [
        { type: 'text', text: 'Hi' },
      ] } },
      { type: 'assistant', message: { id: 'msg_t2', content: [
        { type: 'text', text: 'Hi there!' },
      ] } },
      { type: 'result', session_id: 'sess-mt' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-mt', text: 'Multi-turn' });

    // Collect text_delta events
    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    const deltaContents = textDeltas.map((o) => (o as any).content);

    // Turn 1 should produce: "Hello " then "world!"
    expect(deltaContents).toContain('Hello ');
    expect(deltaContents).toContain('world!');

    // Turn 2 should produce: "Hi" then " there!" — proving counters reset on id change
    expect(deltaContents).toContain('Hi');
    expect(deltaContents).toContain(' there!');
  });

  it('text_delta counters reset on new message.id even when turn-2 text is longer than turn-1', async () => {
    // Turn 1: short text 'Hi' (len 2) + tool_use, under message.id='msg_t1'
    // Turn 2: longer text (len 46) under message.id='msg_t2'
    //
    // Bug (shrink heuristic only): turn-2 text is LONGER than turn-1 accumulated length,
    // so `block.text.length < lastTextLen` never fires. lastTextLen stays at 2 from turn-1.
    // Turn-2 first delta = text.slice(2) = 'llo world this is a much longer turn-2 reply'
    // Fix (id-based reset): message.id change from 'msg_t1' → 'msg_t2' resets lastTextLen=0.
    // Turn-2 first delta = full turn-2 text.
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // Turn 1: short text + tool_use under msg_t1
      { type: 'assistant', message: { id: 'msg_t1', content: [{ type: 'text', text: 'Hi' }] } },
      { type: 'assistant', message: { id: 'msg_t1', content: [
        { type: 'text', text: 'Hi' },
        { type: 'tool_use', id: 'toolu_longer1', name: 'reify_get_source', input: { file: 'f.ri' } },
      ] } },
      // Turn 2: longer text under msg_t2 — no length shrink, only id changes
      { type: 'assistant', message: { id: 'msg_t2', content: [
        { type: 'text', text: 'Hello world this is a much longer turn-2 reply' },
      ] } },
      { type: 'result', session_id: 'sess-longer-text' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-longer-text', text: 'Test longer turn-2' });

    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    const deltaContents = textDeltas.map((o) => (o as any).content);

    // Turn 1 delta present
    expect(deltaContents).toContain('Hi');

    // Turn 2 first delta must be the FULL text — not a slice from offset 2
    // Broken: 'llo world this is a much longer turn-2 reply'
    // Fixed:  'Hello world this is a much longer turn-2 reply'
    expect(deltaContents).toContain('Hello world this is a much longer turn-2 reply');
  });

  it('thinking_delta counters reset on new message.id even when turn-2 thinking is longer than turn-1', async () => {
    // Turn 1: short thinking 'Hm' (len 2) + tool_use, under message.id='msg_t1'
    // Turn 2: longer thinking (len 63) under message.id='msg_t2'
    //
    // Bug (shrink heuristic only): turn-2 thinking is LONGER than turn-1 accumulated length,
    // so `block.thinking.length < lastThinkingLen` never fires. lastThinkingLen stays at 2.
    // Turn-2 first delta = thinking.slice(2) = 'et me carefully reason about a much longer-form chain of thought'
    // Fix (id-based reset): message.id change from 'msg_t1' → 'msg_t2' resets lastThinkingLen=0.
    // Turn-2 first delta = full turn-2 thinking string.
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // Turn 1: short thinking + tool_use under msg_t1
      { type: 'assistant', message: { id: 'msg_t1', content: [{ type: 'thinking', thinking: 'Hm' }] } },
      { type: 'assistant', message: { id: 'msg_t1', content: [
        { type: 'thinking', thinking: 'Hm' },
        { type: 'tool_use', id: 'toolu_think_longer1', name: 'reify_get_source', input: { file: 'f.ri' } },
      ] } },
      // Turn 2: longer thinking under msg_t2 — no length shrink, only id changes
      { type: 'assistant', message: { id: 'msg_t2', content: [
        { type: 'thinking', thinking: 'Let me carefully reason about a much longer-form chain of thought' },
      ] } },
      { type: 'result', session_id: 'sess-longer-thinking' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-longer-thinking', text: 'Test longer turn-2 thinking' });

    const thinkingDeltas = outputs.filter((o) => o.type === 'thinking_delta');
    const deltaContents = thinkingDeltas.map((o) => (o as any).content);

    // Turn 1 delta present
    expect(deltaContents).toContain('Hm');

    // Turn 2 first delta must be the FULL thinking string — not a slice from offset 2
    // Broken: 'et me carefully reason about a much longer-form chain of thought'
    // Fixed:  'Let me carefully reason about a much longer-form chain of thought'
    expect(deltaContents).toContain('Let me carefully reason about a much longer-form chain of thought');
  });

  it('handles assistant event without message.id gracefully — no crash, deltas emit correctly', async () => {
    // Suppress console.error noise from the no-id warning branch
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Verify the fallback branch (event.message.id absent):
    // (a) No exception thrown — session completes normally.
    // (b) Deltas emit correctly for monotonically growing text within a single turn.
    // (c) Counters do not reset spuriously between partial updates.
    //
    // Without message.id the `typeof event.message.id === 'string'` guard never fires,
    // so lastTextLen accumulates normally (no reset) and only incremental deltas emit.
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // No message.id on any event — exercises the fallback path
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hello' }] } },
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hello world' }] } },
      { type: 'result', session_id: 'sess-no-id' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    // (a) Must complete without throwing
    await expect(
      session.handleMessage({ type: 'send_message', id: 'msg-no-id', text: 'Test no id' }),
    ).resolves.toBeUndefined();

    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    const deltaContents = textDeltas.map((o) => (o as any).content);

    // (b) Deltas emit correctly: first partial → 'Hello', second partial → ' world' (incremental only)
    expect(deltaContents).toContain('Hello');
    expect(deltaContents).toContain(' world');

    // (c) No spurious reset: the full 'Hello world' must NOT appear as a single delta,
    // proving lastTextLen was NOT zeroed between the two partial events.
    expect(deltaContents).not.toContain('Hello world');

    // (d) Notice event emitted to host via onOutput — one-shot, first no-id event triggers it.
    // Acceptance criterion #3: existing test must assert the new notice-event surface.
    const noticeEvents = outputs.filter((o) => o.type === 'notice');
    expect(noticeEvents).toHaveLength(1);
    expect((noticeEvents[0] as any).code).toBe('degraded_turn_boundary');
    expect((noticeEvents[0] as any).message).toContain('message.id');
    expect((noticeEvents[0] as any).id).toBe('msg-no-id');

    // (e) console.error fires exactly once across two no-id events (one-shot guard),
    // and the warning text references 'missing message.id' — the sole human-debugging
    // signal that turn-boundary detection has degraded.
    expect(consoleSpy).toHaveBeenCalledTimes(1);
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('missing message.id'));

    consoleSpy.mockRestore();
  });

  it('emits onOutput error event exactly once when assistant events lack message.id (one-shot semantics)', async () => {
    // Verify one-shot guard: across multiple no-id events in a single invokeSdk call,
    // exactly one error event is emitted (the guard must not fire on subsequent no-id events).
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // Three assistant events, all lacking message.id — exercises the one-shot guard
      { type: 'assistant', message: { content: [{ type: 'text', text: 'a' }] } },
      { type: 'assistant', message: { content: [{ type: 'text', text: 'ab' }] } },
      { type: 'assistant', message: { content: [{ type: 'text', text: 'abc' }] } },
      { type: 'result', session_id: 'sess-no-id-oneshot' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-oneshot', text: 'Test' });

    consoleSpy.mockRestore();

    // Exactly one notice emission across three no-id events (one-shot guard fires only once)
    const noticeEvents = outputs.filter((o) => o.type === 'notice');
    expect(noticeEvents).toHaveLength(1);

    // Notice correlates to the in-flight send_message id
    expect((noticeEvents[0] as any).id).toBe('msg-oneshot');

    // Structured discriminator via code field; message references message.id for human readability
    expect((noticeEvents[0] as any).code).toBe('degraded_turn_boundary');
    expect((noticeEvents[0] as any).message).toContain('message.id');

    // Sanity: text deltas still emit — notice event must NOT short-circuit normal streaming
    expect(outputs.some((o) => o.type === 'text_delta')).toBe(true);

    // NOTE: Host-side non-terminal contract is verified by the claudeIntegration test
    // 'claude-notice event preserves in-flight turn (non-terminal)' in
    // gui/src/__tests__/claudeIntegration.test.ts. The 'notice' variant is handled by a
    // dedicated non-terminal case in claudeStore.ts — no cancelAndFlush, no sessionStatus
    // change, no in-flight assistant message mutation.
  });

  it('mixed id-presence within a single invocation: warning fires exactly once, no-id event does not reset, msg_t2 reset emits full delta', async () => {
    // Verify mixed id-presence semantics across a 3-event sequence:
    //   event 1: id='msg_t1'  → id-change reset, currentAssistantMessageId='msg_t1', delta='Hello'
    //   event 2: no id        → one-shot warning fires (console.error + notice), NO reset,
    //                           lastTextLen stays at 5, delta=' world' (continuation)
    //   event 3: id='msg_t2'  → id-change reset (msg_t2 != msg_t1), delta='Goodbye' (full)
    //
    // Key invariants:
    //  - console.error fires exactly once (one-shot across the whole invocation)
    //  - no-id event does NOT reset lastTextLen (delta is ' world', not 'Hello world')
    //  - id-change reset on msg_t2 produces the full 'Goodbye' from offset 0
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      // event 1: id-bearing, triggers turn-1 reset; text 'Hello' (len=5)
      { type: 'assistant', message: { id: 'msg_t1', content: [{ type: 'text', text: 'Hello' }] } },
      // event 2: no message.id, triggers one-shot warning; text 'Hello world' (len=11, delta=' world')
      { type: 'assistant', message: { content: [{ type: 'text', text: 'Hello world' }] } },
      // event 3: new id 'msg_t2', triggers id-change reset; text 'Goodbye' (delta='Goodbye' from offset 0)
      { type: 'assistant', message: { id: 'msg_t2', content: [{ type: 'text', text: 'Goodbye' }] } },
      { type: 'result', session_id: 'sess-mixed-id' },
    ])) as any);

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'send_message', id: 'msg-mixed', text: 'Mixed id test' });

    // 1. console.error fires exactly once across the entire invocation (one-shot guard spans
    //    both the no-id event and the subsequent id-bearing event without re-triggering).
    expect(consoleSpy).toHaveBeenCalledTimes(1);
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('missing message.id'));

    // 2. Exactly one notice event, correlating to the in-flight send_message id.
    const noticeEvents = outputs.filter((o) => o.type === 'notice');
    expect(noticeEvents).toHaveLength(1);
    expect((noticeEvents[0] as any).code).toBe('degraded_turn_boundary');
    expect((noticeEvents[0] as any).message).toContain('message.id');
    expect((noticeEvents[0] as any).id).toBe('msg-mixed');

    // 3. Text delta assertions — verify all three externally observable deltas.
    const textDeltas = outputs.filter((o) => o.type === 'text_delta');
    const deltaContents = textDeltas.map((o) => (o as any).content);

    // event 1 (msg_t1): id-change reset → lastTextLen=0 → full 'Hello' emits as delta
    expect(deltaContents).toContain('Hello');

    // event 2 (no id): no reset → lastTextLen stays at 5 → only ' world' slice emits
    expect(deltaContents).toContain(' world');

    // Absence check: 'Hello world' as a single delta would indicate a spurious reset on the
    // no-id event (i.e. lastTextLen wrongly zeroed), producing a full-string delta from offset 0.
    expect(deltaContents).not.toContain('Hello world');

    // event 3 (msg_t2): id-change reset (msg_t2 !== msg_t1) → lastTextLen=0 → 'Goodbye' from offset 0
    expect(deltaContents).toContain('Goodbye');

    consoleSpy.mockRestore();
  });
});

describe('SidecarSession reset-boundary tool correlation preservation', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('new-turn reset-boundary preserves pending tool_use for subsequent tool_result (text channel)', async () => {
    // Turn 1: long text + tool_use (registers toolu_pending in toolNameById).
    // Turn 2: new message.id triggers id-change reset (length counters only).
    // Invariant: the id-change reset must NOT clear toolNameById/pendingToolUseIds,
    // so a subsequent tool_result for toolu_pending forwards correctly.
    const { mockProc, stdout, stdinLines } = makeMockProc([
      { type: 'text', text: 'Hello world!' },
      { type: 'tool_use', id: 'toolu_pending', name: 'reify_x', input: {} },
    ]);

    // Set up wait for tool_call BEFORE dispatching so we don't miss the event
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-rb-text',
      text: 'Reset boundary text test',
    });

    // Wait for tool_call outbound — guarantees tool_use is registered in toolNameById
    await toolCallWait;

    // Set up wait for text_delta 'Hi' BEFORE pushing the turn-2 event
    const textDeltaHiWait = waitForOutput(
      session,
      (m) => m.type === 'text_delta' && (m as any).content === 'Hi',
    );

    // Push turn 2: new message.id triggers id-change reset (lastTextLen=0, lastThinkingLen=0)
    stdout.push(
      JSON.stringify({ type: 'assistant', message: { id: 'msg_t2', content: [{ type: 'text', text: 'Hi' }] } }) + '\n',
    );

    // Wait for text_delta 'Hi' — guarantees the id-change reset has been processed
    await textDeltaHiWait;

    // Now dispatch tool_result for the pending tool_use. On buggy code this fails because
    // toolNameById was cleared by the shrink branch; on fixed code it forwards correctly.
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-pending',
      tool_name: 'reify_x',
      result: 'result-ok',
      tool_use_id: 'toolu_pending',
    });

    // Race: if the unknown-id error fires before stdin line 2, fail immediately with a
    // clear message rather than waiting for vitest's default timeout.  On buggy code
    // (maps cleared at reset boundary) the error fires first; on fixed code the stdin
    // line arrives first.
    const unexpectedErrorWait = waitForOutput(
      session,
      (m) => m.type === 'error' && /unknown.*tool_use_id/i.test((m as any).message ?? ''),
    );
    const winner = await Promise.race([
      waitForStdinLines(mockProc.stdin, stdinLines, 2).then(() => 'stdin' as const),
      unexpectedErrorWait.then(() => 'error' as const),
    ]);
    expect(winner).toBe('stdin'); // fails fast with a clear message if maps were cleared
    unexpectedErrorWait.cancel(); // release session.onOutput immediately; don't wait for the 5s default timeout

    // Assert the tool_result was forwarded with the correct tool_use_id
    expect((stdinLines[1] as any).message.content[0].tool_use_id).toBe('toolu_pending');

    // Cleanup: close stdout so msgPromise can resolve
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-rb-text' }) + '\n');
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
    // Pin the contract over the full output stream: exactly one text_delta 'Hi' across the
    // entire turn (guards against duplicate emissions if the streaming loop restarts).
    expect(outputs.filter((m) => m.type === 'text_delta' && (m as any).content === 'Hi').length).toBe(1);
  });

  it('new-turn reset-boundary preserves pending tool_use for subsequent tool_result (thinking channel)', async () => {
    // Turn 1: long thinking + tool_use (registers toolu_th_pending in toolNameById).
    // Turn 2: new message.id triggers id-change reset (length counters only).
    // Invariant: the id-change reset must NOT clear toolNameById/pendingToolUseIds,
    // so a subsequent tool_result for toolu_th_pending forwards correctly.
    const { mockProc, stdout, stdinLines } = makeMockProc([
      { type: 'thinking', thinking: 'Let me reason about this for a while' },
      { type: 'tool_use', id: 'toolu_th_pending', name: 'reify_x', input: {} },
    ]);

    // Set up wait for tool_call BEFORE dispatching so we don't miss the event
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-rb-think',
      text: 'Reset boundary thinking test',
    });

    // Wait for tool_call outbound — guarantees tool_use is registered in toolNameById
    await toolCallWait;

    // Set up wait for thinking_delta 'Hm' BEFORE pushing the turn-2 event
    const thinkingDeltaHmWait = waitForOutput(
      session,
      (m) => m.type === 'thinking_delta' && (m as any).content === 'Hm',
    );

    // Push turn 2: new message.id triggers id-change reset (lastTextLen=0, lastThinkingLen=0)
    stdout.push(
      JSON.stringify({
        type: 'assistant',
        message: { id: 'msg_t2', content: [{ type: 'thinking', thinking: 'Hm' }] },
      }) + '\n',
    );

    // Wait for thinking_delta 'Hm' — guarantees the id-change reset has been processed
    await thinkingDeltaHmWait;

    // Now dispatch tool_result for the pending tool_use. On buggy code this fails because
    // toolNameById was cleared by the thinking-shrink branch; on fixed code it forwards correctly.
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-th-pending',
      tool_name: 'reify_x',
      result: 'result-ok',
      tool_use_id: 'toolu_th_pending',
    });

    // Race: if the unknown-id error fires before stdin line 2, fail immediately with a
    // clear message rather than waiting for vitest's default timeout.  On buggy code
    // (maps cleared at reset boundary) the error fires first; on fixed code the stdin
    // line arrives first.
    const unexpectedErrorWait = waitForOutput(
      session,
      (m) => m.type === 'error' && /unknown.*tool_use_id/i.test((m as any).message ?? ''),
    );
    const winner = await Promise.race([
      waitForStdinLines(mockProc.stdin, stdinLines, 2).then(() => 'stdin' as const),
      unexpectedErrorWait.then(() => 'error' as const),
    ]);
    expect(winner).toBe('stdin'); // fails fast with a clear message if maps were cleared
    unexpectedErrorWait.cancel(); // release session.onOutput immediately; don't wait for the 5s default timeout

    // Assert the tool_result was forwarded with the correct tool_use_id
    expect((stdinLines[1] as any).message.content[0].tool_use_id).toBe('toolu_th_pending');

    // Cleanup: close stdout so msgPromise can resolve
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-rb-think' }) + '\n');
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
    // Pin the contract over the full output stream: exactly one thinking_delta 'Hm' across
    // the entire turn (guards against duplicate emissions if the streaming loop restarts).
    expect(outputs.filter((m) => m.type === 'thinking_delta' && (m as any).content === 'Hm').length).toBe(1);
  });
});

describe('SidecarSession stale-state lifecycle', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('invokeSdk start clears stale maps from a previous turn', async () => {
    // Pre-populate stale state from a hypothetical prior turn that left data behind
    (session as any).pendingToolUseIds.set('reify_old', ['toolu_stale']);
    (session as any).toolNameById.set('toolu_stale', 'reify_old');

    // Configure spawn mock to return a process that emits no tool_use (just text + result)
    vi.mocked(spawn).mockImplementation((() => createMockProcess([
      { type: 'assistant', message: { content: [{ type: 'text', text: 'OK' }] } },
      { type: 'result', session_id: 'sess-clean' },
    ])) as any);

    await session.handleMessage({ type: 'send_message', id: 'msg-fresh', text: 'Hi' });

    // After the invocation completes, the stale entries must have been cleared at start
    expect((session as any).pendingToolUseIds.has('reify_old')).toBe(false);
    expect((session as any).toolNameById.has('toolu_stale')).toBe(false);
  });

  it('destroy() clears toolNameById and pendingToolUseIds maps', () => {
    // Pre-populate both maps
    (session as any).pendingToolUseIds.set('reify_old', ['toolu_stale']);
    (session as any).toolNameById.set('toolu_stale', 'reify_old');

    session.destroy();

    expect((session as any).toolNameById.size).toBe(0);
    expect((session as any).pendingToolUseIds.size).toBe(0);
  });

  it('clear_session clears toolNameById and pendingToolUseIds maps', async () => {
    // Pre-populate both maps
    (session as any).pendingToolUseIds.set('reify_old', ['toolu_stale']);
    (session as any).toolNameById.set('toolu_stale', 'reify_old');

    await session.init();
    outputs.length = 0;

    await session.handleMessage({ type: 'clear_session' });

    expect((session as any).toolNameById.size).toBe(0);
    expect((session as any).pendingToolUseIds.size).toBe(0);

    // Verify the existing handleClearSession contract is preserved: ready is still emitted
    expect(outputs).toHaveLength(1);
    expect(outputs[0]).toEqual({ type: 'ready' });
  });
});

describe('SidecarSession destroy() lifecycle', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  it('destroy() aborts in-flight request and emits done', async () => {
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

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.init();
    outputs.length = 0;

    // Start a hanging send_message
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-destroy',
      text: 'Hang',
    });

    // Give it a tick to set up
    await new Promise((r) => setTimeout(r, 10));

    // Destroy should abort the in-flight request
    session.destroy();

    await msgPromise;

    // Should emit done (not error) on destroy
    const dones = outputs.filter((o) => o.type === 'done');
    expect(dones).toHaveLength(1);
    expect(dones[0]).toEqual({ type: 'done', id: 'msg-destroy' });

    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(0);
  });

  it('handleMessage after destroy() is a no-op (does not spawn or emit)', async () => {
    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.init();
    outputs.length = 0;

    session.destroy();

    // After destroy, handleMessage should be a no-op
    await session.handleMessage({ type: 'send_message', id: 'msg-after', text: 'Post-destroy' });

    // spawn should NOT have been called (no subprocess spawned)
    expect(vi.mocked(spawn)).not.toHaveBeenCalled();

    // No messages emitted (not even done or error)
    expect(outputs).toHaveLength(0);
  });

  it('destroy() is idempotent — calling twice does not throw', async () => {
    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.init();

    // Both calls should complete without throwing
    expect(() => session.destroy()).not.toThrow();
    expect(() => session.destroy()).not.toThrow();
  });

  // This test proves deregistration via mock.calls[1][0] === null. The former duplicate test
  // `destroyed session: triggerRequest throws because onRequest(null) cleared the handler` was
  // deleted: it pinned the mock-helper's internal error wording, not observable production
  // behavior, and added no coverage beyond what mock.calls[1][0]).toBeNull() already asserts.
  it('destroy() deregisters the permission handler by calling server.onRequest(null)', () => {
    const { server } = makeMockPermissionServer();
    const permUrl = 'http://127.0.0.1:29999/mcp';

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: permUrl, server },
    } as any);

    const onRequestMock = server.onRequest as ReturnType<typeof vi.fn>;

    // Constructor should have registered a handler (first call with a function)
    expect(onRequestMock).toHaveBeenCalledTimes(1);
    expect(onRequestMock.mock.calls[0][0]).toEqual(expect.any(Function));

    session.destroy();

    // destroy() must deregister by calling onRequest(null)
    expect(onRequestMock).toHaveBeenCalledTimes(2);
    expect(onRequestMock.mock.calls[1][0]).toBeNull();
  });

  // Pins the destroyed-guard branch: after destroy(), the production handler must short-circuit.
  it('destroyed-guard short-circuits the constructor onRequest handler', () => {
    const { server, triggerLatchedHandler } = makeMockPermissionServer();
    const permUrl = 'http://127.0.0.1:29999/mcp';

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: permUrl, server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    session.destroy();

    // Invoke via the latched handler — the destroyed-guard must fire
    triggerLatchedHandler({ request_id: 'req-after-destroy', tool_name: 'Write', tool_input: { path: '/tmp/x' } });

    // Guard short-circuited: no output was emitted (neither permission_request nor notice)
    expect(outputs).toHaveLength(0);

    // Guard short-circuited: the orphan-deny branch was bypassed, so decide was never called
    expect(server.decide as ReturnType<typeof vi.fn>).not.toHaveBeenCalled();
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
      { type: 'assistant', message: { id: 'msg_ok_1', content: [{ type: 'text', text: 'Response' }] } },
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

  it('tool_result inbound is accepted end-to-end via entrypoint and forwarded to claude CLI stdin', async () => {
    // Create a mock process that emits a tool_use and stays open
    const mockProc = new EventEmitter() as any;
    const mockStdout = new PassThrough();
    mockProc.stdout = mockStdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();
    mockProc.exitCode = null;

    vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
      process.nextTick(() => {
        // Emit a tool_use event and leave stdout open
        mockStdout.push(JSON.stringify({
          type: 'assistant',
          message: { id: 'msg_e2e_1', content: [
            { type: 'tool_use', id: 'toolu_e2e', name: 'reify_get_diagnostics', input: {} },
          ] },
        }) + '\n');
      });
      if (opts?.signal) {
        opts.signal.addEventListener('abort', () => {
          mockStdout.end();
          mockProc.exitCode = null;
          mockProc.emit('close', null);
        });
      }
      return mockProc;
    }) as any);

    const input = new PassThrough();
    const output = new PassThrough();

    // Collect outbound messages as they arrive
    const receivedMsgs: OutboundMessage[] = [];
    let outputBuf = '';
    output.on('data', (chunk: Buffer) => {
      outputBuf += chunk.toString();
      let idx: number;
      while ((idx = outputBuf.indexOf('\n')) !== -1) {
        const line = outputBuf.slice(0, idx);
        outputBuf = outputBuf.slice(idx + 1);
        if (line.length > 0) receivedMsgs.push(JSON.parse(line));
      }
    });

    const mainPromise = main(input, output);

    // Wait for ready
    await new Promise((r) => setTimeout(r, 50));

    // Set up stdin capture BEFORE writing the send_message
    const stdinLines: unknown[] = [];
    let stdinBuf = '';
    mockProc.stdin.on('data', (chunk: Buffer | string) => {
      stdinBuf += chunk.toString();
      let nlIdx: number;
      while ((nlIdx = stdinBuf.indexOf('\n')) !== -1) {
        stdinLines.push(JSON.parse(stdinBuf.slice(0, nlIdx)));
        stdinBuf = stdinBuf.slice(nlIdx + 1);
      }
    });

    // Send a send_message through the entrypoint input
    input.write(JSON.stringify({ type: 'send_message', id: 'e2e-tr-1', text: 'Check diagnostics' }) + '\n');

    // Wait for tool_call outbound to arrive (signals tool_use was parsed and pendingToolUseIds populated)
    const toolCallDeadline = Date.now() + 500;
    while (Date.now() < toolCallDeadline) {
      if (receivedMsgs.some((m) => m.type === 'tool_call')) break;
      await new Promise((r) => setTimeout(r, 10));
    }
    expect(receivedMsgs.some((m) => m.type === 'tool_call')).toBe(true);

    // Send tool_result through the entrypoint input (the previous failure mode: parseInboundMessage threw)
    input.write(JSON.stringify({
      type: 'tool_result',
      id: 'e2e-tr-1',
      tool_use_id: 'toolu_e2e',
      tool_name: 'reify_get_diagnostics',
      result: { diagnostics: [] },
    }) + '\n');

    // Give time for the write to propagate through the session
    await new Promise((r) => setTimeout(r, 50));

    // (a) NO outbound error event was emitted
    const errors = receivedMsgs.filter((m) => m.type === 'error');
    expect(errors).toHaveLength(0);

    // (b) The mock claude CLI's stdin received the tool_result content block
    // stdinLines[0] = initial user prompt; stdinLines[1] = tool_result block
    expect(stdinLines.length).toBeGreaterThanOrEqual(2);
    expect(stdinLines[1]).toEqual({
      type: 'user',
      message: {
        role: 'user',
        content: [{ type: 'tool_result', tool_use_id: 'toolu_e2e', content: { diagnostics: [] } }],
      },
    });

    // Clean up: close the mock process and end input
    mockStdout.push(JSON.stringify({ type: 'result', session_id: 'sess-e2e-tr' }) + '\n');
    mockStdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);

    await new Promise((r) => setTimeout(r, 50));
    input.end();

    await Promise.race([mainPromise, new Promise((r) => setTimeout(r, 500))]);

    vi.mocked(spawn).mockReset();
  });

  it('abort message is processed while send_message is in-flight (non-blocking input loop)', async () => {
    // Create a mock process that hangs until the abort signal fires
    const hangingProc = new EventEmitter() as any;
    const hangStdout = new PassThrough();
    hangingProc.stdout = hangStdout;
    hangingProc.stderr = new PassThrough();
    hangingProc.stdin = new PassThrough();
    hangingProc.exitCode = null;

    vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
      if (opts?.signal) {
        opts.signal.addEventListener('abort', () => {
          hangStdout.end();
          hangingProc.exitCode = null;
          hangingProc.emit('close', null);
        });
      }
      return hangingProc;
    }) as any);

    const input = new PassThrough();
    const output = new PassThrough();

    // Collect messages as they arrive (don't wait for end)
    const receivedMsgs: OutboundMessage[] = [];
    let outputBuf = '';
    output.on('data', (chunk: Buffer) => {
      outputBuf += chunk.toString();
      let idx: number;
      while ((idx = outputBuf.indexOf('\n')) !== -1) {
        const line = outputBuf.slice(0, idx);
        outputBuf = outputBuf.slice(idx + 1);
        if (line.length > 0) receivedMsgs.push(JSON.parse(line));
      }
    });

    const mainPromise = main(input, output);

    // Wait for ready
    await new Promise((r) => setTimeout(r, 30));

    // Send a hanging message, then immediately send abort
    input.write(JSON.stringify({ type: 'send_message', id: 'nb-1', text: 'Hang' }) + '\n');
    // Small delay so send_message is dispatched before abort is written
    await new Promise((r) => setTimeout(r, 10));
    input.write(JSON.stringify({ type: 'abort' }) + '\n');

    // Wait up to 200ms for the abort to be processed and done emitted
    const deadline = Date.now() + 200;
    while (Date.now() < deadline) {
      if (receivedMsgs.some((m) => m.type === 'done')) break;
      await new Promise((r) => setTimeout(r, 10));
    }

    // End input so main's for-await loop can exit
    input.end();

    // Race main against a ceiling so we don't hang the test suite if main stalls
    await Promise.race([mainPromise, new Promise((r) => setTimeout(r, 500))]);

    // The abort must have been processed while send_message was in-flight:
    // done should have been emitted for 'nb-1'
    const dones = receivedMsgs.filter((m) => m.type === 'done');
    expect(dones).toHaveLength(1);
    expect(dones[0]).toEqual({ type: 'done', id: 'nb-1' });
  });
});

describe('close-on-result race', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('tool_result after result event produces "no in-flight" error, not "stdin write error"', async () => {
    const { mockProc, stdout } = makeMockProc([
      { type: 'tool_use', id: 'toolu_race', name: 'reify_get_diagnostics', input: {} },
    ]);

    // Set up wait for tool_call BEFORE dispatch
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    // Start send_message (don't await — will hang waiting for stdout to close)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-race',
      text: 'Check diagnostics',
    });

    // Wait for tool_call outbound (event-driven)
    await toolCallWait;

    // Push the 'result' event to stdout, but do NOT close stdout yet.
    // This triggers proc.stdin?.end() inside the for-await loop,
    // but currentStdin is not nulled until the finally block (Bug #2).
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-race' }) + '\n');

    // Wait for proc.stdin to finish (stdin.end() has been called by the result branch).
    // This confirms the result branch executed before we send the tool_result.
    await new Promise<void>((resolve) => {
      if (mockProc.stdin.writableEnded) {
        resolve();
      } else {
        mockProc.stdin.once('finish', resolve);
      }
    });

    // Set up wait for error outbound BEFORE dispatching tool_result
    const errorWait = waitForOutput(session, (m) => m.type === 'error');

    // Dispatch tool_result AFTER the result event fired (race window).
    // Bug #2: currentStdin is still non-null (not yet cleared), so the write
    // proceeds against an ended stream → "stdin write error"
    // Fix: currentStdin is nulled immediately after proc.stdin?.end() → "no in-flight"
    session.handleMessage({
      type: 'tool_result',
      id: 'tool-race',
      tool_use_id: 'toolu_race',
      tool_name: 'reify_get_diagnostics',
      result: { diagnostics: [] },
    });

    // Wait for the error outbound
    const errorMsg = await errorWait;

    // THE KEY ASSERTION: must be the clean "no in-flight" error, NOT "stdin write error"
    expect((errorMsg as any).message).toMatch(/no in-flight claude CLI process/i);
    expect((errorMsg as any).message).not.toMatch(/stdin write error/i);

    // Cleanup: close stdout so msgPromise can resolve
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });
});

describe('stdin write error correlation', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('async stdin error after tool_result write is tagged with tool_result id, not send_message id', async () => {
    const { mockProc, stdout } = makeMockProc([
      { type: 'tool_use', id: 'toolu_x', name: 'reify_get_diagnostics', input: {} },
    ]);

    // Set up wait for tool_call BEFORE dispatch so we don't miss it
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    // Start send_message (don't await — will hang waiting for stdout to close)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-A',
      text: 'Check diagnostics',
    });

    // Wait for the tool_call outbound (event-driven, not polling)
    await toolCallWait;

    // Override mockProc.stdin.write to simulate an async EPIPE error on the NEXT write.
    // The error fires on process.nextTick via both the 'error' event and the write callback.
    let errorOverrideActive = false;
    const originalWrite = mockProc.stdin.write.bind(mockProc.stdin);
    mockProc.stdin.write = (chunk: any, encodingOrCb?: any, callback?: any) => {
      if (errorOverrideActive) {
        const cb = typeof encodingOrCb === 'function' ? encodingOrCb : callback;
        const err = new Error('write EPIPE');
        process.nextTick(() => {
          // Fire the async stream 'error' event (current code's listener uses wrong id)
          mockProc.stdin.emit('error', err);
          // Also invoke the per-write callback with the error (fix target)
          if (typeof cb === 'function') cb(err);
        });
        return false;
      }
      return originalWrite(chunk, encodingOrCb, callback);
    };
    errorOverrideActive = true;

    // Set up wait for error outbound BEFORE dispatching tool_result
    const errorWait = waitForOutput(session, (m) => m.type === 'error');

    // Dispatch tool_result with id='tool-B' — the write override fires async EPIPE
    session.handleMessage({
      type: 'tool_result',
      id: 'tool-B',
      tool_use_id: 'toolu_x',
      tool_name: 'reify_get_diagnostics',
      result: { diagnostics: [] },
    });

    // Wait for the error outbound (event-driven)
    const errorMsg = await errorWait;

    // THE KEY ASSERTION: error must be tagged with the tool_result id, NOT the send_message id.
    // On current code this fails because the async 'error' listener captures the outer
    // send_message id ('send-A') rather than the tool_result id ('tool-B').
    expect((errorMsg as any).id).toBe('tool-B');
    expect((errorMsg as any).id).not.toBe('send-A');
    expect((errorMsg as any).message).toMatch(/stdin write error|EPIPE/i);

    // Cleanup: close stdout so msgPromise can resolve
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });
});

/**
 * Shared setup for orphan-stdin-error tests. Creates a mock proc with one tool_use,
 * installs a console.warn spy, dispatches handleMessage, and awaits the tool_call
 * output to confirm the stdin 'error' listener is attached before returning.
 *
 * Returns { mockProc, warnSpy, finish } where finish() restores the spy, closes the
 * mock process (using stdout/msgPromise captured in its closure), and resolves the
 * pending handleMessage.
 */
async function setupOrphanStdinErrorScenario(session: SidecarSession) {
  const { mockProc, stdout } = makeMockProc([
    { type: 'tool_use', id: 'toolu_orphan', name: 'reify_x', input: {} },
  ]);

  const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

  const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

  const msgPromise = session.handleMessage({
    type: 'send_message',
    id: 'send-orphan',
    text: 'Hi',
  });

  // Await tool_call — proves the prompt has been written to stdin and the 'error' listener is attached
  await toolCallWait;

  const finish = async () => {
    warnSpy.mockRestore();
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-orphan' }) + '\n');
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  };

  return { mockProc, warnSpy, finish };
}

describe('stdin orphan-error diagnostic', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('orphan stdin error emits a console.warn diagnostic', async () => {
    const { mockProc, warnSpy, finish } = await setupOrphanStdinErrorScenario(session);

    // Synthesize an orphan EPIPE: emits to the 'error' listener but no per-write callback is in flight.
    mockProc.stdin.emit('error', new Error('synthetic orphan EPIPE'));

    // Allow the current tick to flush any synchronous handler side-effects
    await new Promise(setImmediate);

    // Assert console.warn was called with the error message (prefix is cosmetic, not load-bearing)
    expect(warnSpy).toHaveBeenCalled();
    const allCallArgs = warnSpy.mock.calls.map((args) => args.join(' '));
    const matchingCall = allCallArgs.find((s) => s.includes('synthetic orphan EPIPE'));
    expect(matchingCall).toBeDefined();

    await finish();
  });

  it('orphan stdin error one-shot guard fires console.warn exactly once across multiple emits', async () => {
    const { mockProc, warnSpy, finish } = await setupOrphanStdinErrorScenario(session);

    // First orphan EPIPE — wait until the warn fires before dispatching the second emit,
    // so the ordering invariant holds even if the handler ever becomes async.
    mockProc.stdin.emit('error', new Error('synthetic orphan EPIPE 1'));
    await vi.waitFor(() => expect(warnSpy).toHaveBeenCalledTimes(1));

    // Second orphan EPIPE with a distinct message — one-shot guard must suppress this
    mockProc.stdin.emit('error', new Error('synthetic orphan EPIPE 2 distinct'));
    await new Promise(setImmediate);

    // Filter to guard fires (identified by the synthetic error message they carry; robust to prefix changes)
    const stdinErrorWarns = warnSpy.mock.calls.filter((args) =>
      args.join(' ').includes('synthetic orphan EPIPE')
    );

    // One-shot guard: exactly one warn fired despite two error emits
    expect(stdinErrorWarns).toHaveLength(1);

    // The surviving warn references the FIRST error message
    expect(stdinErrorWarns[0].join(' ')).toContain('synthetic orphan EPIPE 1');

    // The surviving warn does NOT reference the suppressed second error
    expect(stdinErrorWarns[0].join(' ')).not.toContain('synthetic orphan EPIPE 2');

    await finish();
  });
});

describe('echoed tool_use_id validation', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  /**
   * Shared setup: mock subprocess that emits ONE tool_use (id="toolu_real", name="reify_x")
   * and leaves stdout open. Returns { mockProc, stdout, stdinLines, msgPromise, toolCallWait }.
   * Uses makeMockProc to avoid boilerplate duplication.
   */
  function setupValidationTest() {
    const { mockProc, stdout, stdinLines } = makeMockProc([
      { type: 'tool_use', id: 'toolu_real', name: 'reify_x', input: {} },
    ]);

    // Set up wait for tool_call BEFORE dispatch
    const toolCallWait = waitForOutput(session, (m) => m.type === 'tool_call');

    // Start send_message (don't await — hangs until stdout closes)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-val',
      text: 'Trigger tool call',
    });

    return { mockProc, stdout, stdinLines, msgPromise, toolCallWait };
  }

  it('emits structured error and does not forward when echoed tool_use_id is unknown', async () => {
    const { stdout, stdinLines, msgPromise, toolCallWait, mockProc } = setupValidationTest();

    // Wait for tool_call outbound (event-driven)
    await toolCallWait;

    // Set up wait for error outbound BEFORE dispatching the bad tool_result
    const errorWait = waitForOutput(session, (m) => m.type === 'error');

    // Dispatch tool_result with a BOGUS tool_use_id (not in toolNameById)
    session.handleMessage({
      type: 'tool_result',
      id: 'tool-bogus',
      tool_name: 'reify_x',
      result: 'ok',
      tool_use_id: 'toolu_BOGUS',
    });

    // Wait for the error outbound
    const errorMsg = await errorWait;

    // Assertions: structured error with correct id and matching message
    expect((errorMsg as any).id).toBe('tool-bogus');
    expect((errorMsg as any).message).toMatch(/unknown.*tool_use_id|invalid.*tool_use_id/i);

    // No tool_result was forwarded to stdin (only the initial prompt write).
    // Yield all pending microtasks so any stray write has time to land in stdinLines
    // before we assert — waitForStdinLines(1) would be a no-op here because stdinLines
    // is already at length 1 from the initial prompt write.
    await new Promise(setImmediate);
    expect(stdinLines.length).toBe(1);

    // Cleanup
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });

  it('emits structured error and does not forward when echoed tool_use_id name does not match tool_name', async () => {
    const { stdout, stdinLines, msgPromise, toolCallWait, mockProc } = setupValidationTest();

    // Wait for tool_call outbound (event-driven)
    await toolCallWait;

    // Set up wait for error outbound BEFORE dispatching the mismatched tool_result
    const errorWait = waitForOutput(session, (m) => m.type === 'error');

    // Dispatch tool_result with the REAL tool_use_id but WRONG tool_name (mismatch)
    session.handleMessage({
      type: 'tool_result',
      id: 'tool-mismatch',
      tool_name: 'reify_OTHER',  // registered id maps to 'reify_x', not 'reify_OTHER'
      result: 'ok',
      tool_use_id: 'toolu_real',
    });

    // Wait for the error outbound
    const errorMsg = await errorWait;

    // Assertions: structured error with correct id and name-mismatch message
    expect((errorMsg as any).id).toBe('tool-mismatch');
    // Message must indicate a structural name-mismatch (not just incidentally contain a tool name)
    expect((errorMsg as any).message).toMatch(/does not match tool_name=/);
    expect((errorMsg as any).message).toContain('reify_OTHER');
    expect((errorMsg as any).message).toContain('reify_x');

    // No tool_result was forwarded to stdin (only the initial prompt write).
    // Yield all pending microtasks so any stray write has time to land in stdinLines
    // before we assert — waitForStdinLines(1) would be a no-op here because stdinLines
    // is already at length 1 from the initial prompt write.
    await new Promise(setImmediate);
    expect(stdinLines.length).toBe(1);

    // Cleanup
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });

  it('stale tool_use_id re-dispatch emits structured error and does not forward a second stdin line', async () => {
    // This test covers the "toolNameById drain" property through observable behavior:
    // once a tool_result has been successfully forwarded for a given tool_use_id, the id is
    // deleted from toolNameById (the toolNameById.delete after the FIFO splice/shift).
    // Re-dispatching a tool_result with the same id must hit the unknown-id guard in
    // handleToolResult and emit a structured error — not forward a second stdin line.
    const { mockProc, stdout, stdinLines, msgPromise, toolCallWait } = setupValidationTest();

    // (1) Wait for tool_call outbound — tool_use is now registered in toolNameById
    await toolCallWait;

    // (2) Dispatch first tool_result — this is a valid forward
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-first',
      tool_name: 'reify_x',
      result: 'ok-1',
      tool_use_id: 'toolu_real',
    });

    // (3) Wait for stdin line 2 (initial prompt = line 1, forwarded tool_result = line 2)
    await waitForStdinLines(mockProc.stdin, stdinLines, 2);

    // (4) Assert first forward used the correct tool_use_id
    expect((stdinLines[1] as any).message.content[0].tool_use_id).toBe('toolu_real');

    // (5) Set up wait for error outbound BEFORE dispatching the stale re-dispatch
    const errorWait = waitForOutput(session, (m) => m.type === 'error');

    // (6) Re-dispatch a tool_result with the SAME (already consumed) tool_use_id
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-stale',
      tool_name: 'reify_x',
      result: 'ok-2',
      tool_use_id: 'toolu_real',
    });

    // (7) Wait for the structured error outbound
    const errorMsg = await errorWait;

    // (8) Assert correct id and unknown-id error message
    expect((errorMsg as any).id).toBe('tr-stale');
    expect((errorMsg as any).message).toMatch(/unknown.*tool_use_id|invalid.*tool_use_id/i);

    // (9) Yield microtasks so any stray write would land in stdinLines before we assert
    await new Promise(setImmediate);

    // (10) Assert no extra stdin line was written (stale re-dispatch must not forward)
    expect(stdinLines.length).toBe(2);

    // Cleanup
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });
});

describe('FIFO consumption on echoed-id path', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'Test.',
    });
    session.onOutput = (msg) => outputs.push(msg);
    vi.mocked(spawn).mockReset();
  });

  it('echoed-id forward drains its id from both maps; subsequent fallback uses correct remaining id', async () => {
    // Two tool_use blocks with same name, different ids — makeMockProc wires up stdin
    // accumulator before spawn and leaves stdout open.
    const { mockProc, stdout, stdinLines } = makeMockProc([
      { type: 'tool_use', id: 'toolu_1', name: 'reify_x', input: {} },
      { type: 'tool_use', id: 'toolu_2', name: 'reify_x', input: {} },
    ]);

    // Wait for BOTH tool_call outbounds before dispatching any tool_result.
    // waitForOutputs decouples the count from the predicate, avoiding the
    // side-effect-in-predicate pattern that is fragile when non-tool_call
    // outbounds are interspersed.
    const bothToolCallsWait = waitForOutputs(session, (m) => m.type === 'tool_call', 2);

    // Start send_message (don't await)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'send-fifo',
      text: 'Trigger two tool calls',
    });

    await bothToolCallsWait;

    // --- Step (a): Dispatch tool_result with ECHOED tool_use_id toolu_1 ---
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-1',
      tool_name: 'reify_x',
      result: 'result-1',
      tool_use_id: 'toolu_1',
    });

    // Wait for the second stdin line (initial prompt = line 1, this = line 2)
    await waitForStdinLines(mockProc.stdin, stdinLines, 2);

    // Verify correct tool_use_id was forwarded
    const line2 = stdinLines[1] as any;
    expect(line2.message.content[0].tool_use_id).toBe('toolu_1');

    // --- Step (b): Dispatch tool_result WITHOUT tool_use_id (fallback path) ---
    /**
     * Test-local relaxed shape that lets THIS test deliberately bypass the
     * wire-contract type to exercise the FIFO-by-tool_name fallback path
     * inside `handleToolResult`. The public `InboundToolResult` requires
     * `tool_use_id`; this is the one site where the runtime fallback (kept
     * as defense-in-depth) is exercised, by-design.
     */
    type FifoFallbackToolResult = { type: 'tool_result'; id: string; tool_name: string; result: unknown };
    const fallbackMsg: FifoFallbackToolResult = {
      type: 'tool_result',
      id: 'tr-2',
      tool_name: 'reify_x',
      result: 'result-2',
      // no tool_use_id — fallback to FIFO
    };
    session.handleMessage(fallbackMsg as unknown as InboundMessage);

    // Wait for the third stdin line
    await waitForStdinLines(mockProc.stdin, stdinLines, 3);

    // Verify the fallback used toolu_2 (the remaining id), NOT toolu_1 (already consumed).
    // This proves toolu_1 was drained from the FIFO on the echoed-id forward above.
    const line3 = stdinLines[2] as any;
    expect(line3.message.content[0].tool_use_id).toBe('toolu_2');

    // --- Step (c): Prove toolNameById was drained for toolu_2 on the fallback path ---
    // Re-dispatch an echoed-id tool_result for toolu_2 (already consumed by the FIFO
    // fallback above). If toolNameById was correctly drained it must hit the unknown-id
    // guard and emit a structured error with no extra stdin write.
    const staleIdErrorWait = waitForOutput(
      session,
      (m) => m.type === 'error' && /unknown.*tool_use_id/i.test((m as any).message ?? ''),
    );
    session.handleMessage({
      type: 'tool_result',
      id: 'tr-stale-2',
      tool_name: 'reify_x',
      result: 'result-stale',
      tool_use_id: 'toolu_2',
    });
    const staleIdError = await staleIdErrorWait;
    expect((staleIdError as any).id).toBe('tr-stale-2');
    // Yield microtasks so any stray write would land before we assert length
    await new Promise(setImmediate);
    expect(stdinLines.length).toBe(3); // no extra forward

    // Cleanup: close stdout so msgPromise can resolve
    stdout.push(JSON.stringify({ type: 'result', session_id: 'sess-fifo' }) + '\n');
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });
});

describe('waitForOutput / waitForOutputs deadline', () => {
  let session: SidecarSession;

  beforeEach(() => {
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
    });
    vi.mocked(spawn).mockReset();
  });

  it('waitForOutput rejects with named timeout error when predicate never matches; restores session.onOutput', async () => {
    const originalOnOutput = session.onOutput;
    await expect(
      waitForOutput(session, () => false, { timeoutMs: 50 })
    ).rejects.toThrow(/waitForOutput timed out waiting for predicate/);
    expect(session.onOutput).toBe(originalOnOutput);
  });

  it('waitForOutputs rejects with named timeout error when predicate never matches; restores session.onOutput', async () => {
    const originalOnOutput = session.onOutput;
    await expect(
      waitForOutputs(session, () => false, 1, { timeoutMs: 50 })
    ).rejects.toThrow(/waitForOutputs timed out waiting for predicate/);
    expect(session.onOutput).toBe(originalOnOutput);
  });
});

describe('waitForOutput cancel hook', () => {
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

  it('cancel() before match restores session.onOutput immediately and keeps the promise pending', async () => {
    const originalOnOutput = session.onOutput;

    const cancellable = waitForOutput(session, (m) => m.type === 'text_delta');

    // session.onOutput should be swapped to the wrapped handler
    expect(session.onOutput).not.toBe(originalOnOutput);

    // Cancel BEFORE emitting any matching output
    cancellable.cancel();

    // (b) session.onOutput should be restored immediately after cancel
    expect(session.onOutput).toBe(originalOnOutput);

    // (a) Emit a matching message — lands in outputs via restored handler; wait stays pending
    session.onOutput({ type: 'text_delta', content: 'not-captured' } as any);
    expect(outputs).toHaveLength(1);

    // (c) Cancelled promise stays pending — a short race should time out, not resolve
    const raceResult = await Promise.race([
      cancellable.then(() => 'resolved' as const),
      new Promise<'timeout'>((res) => setTimeout(() => res('timeout'), 50)),
    ]);
    expect(raceResult).toBe('timeout');
  });

  it('cancel() after match is a no-op (idempotent — does not throw, does not re-restore)', async () => {
    const originalOnOutput = session.onOutput;

    const cancellable = waitForOutput(session, (m) => m.type === 'text_delta', { timeoutMs: 2000 });

    // Emit a matching message so the wait settles via match
    session.onOutput({ type: 'text_delta', content: 'match' } as any);
    const result = await cancellable;
    expect(result.type).toBe('text_delta');

    // session.onOutput should already be restored
    expect(session.onOutput).toBe(originalOnOutput);

    // cancel() after resolution must not throw and must leave session.onOutput as-is
    expect(() => cancellable.cancel()).not.toThrow();
    expect(session.onOutput).toBe(originalOnOutput);
  });
});

// ---------------------------------------------------------------------------
// proc error handling — ABORT_ERR suppression and non-ABORT logging
// ---------------------------------------------------------------------------
describe('SidecarSession proc error handling', () => {
  let session: SidecarSession;
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('ABORT_ERR from spawned ChildProcess on timeout abort does not crash and is suppressed silently', async () => {
    vi.useFakeTimers();

    // Spy on console.error to assert the ABORT_ERR is suppressed silently (not logged)
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      session = new SidecarSession({
        model: 'claude-opus-4-6',
        workingDirectory: '/tmp/test-project',
        systemPrompt: 'You are a test assistant.',
        timeoutMs: 1000,
      });
      session.onOutput = (msg) => outputs.push(msg);

      // Create a mock process that hangs forever
      const mockProc = new EventEmitter() as any;
      const stdout = new PassThrough();
      mockProc.stdout = stdout;
      mockProc.stderr = new PassThrough();
      mockProc.stdin = new PassThrough();
      mockProc.exitCode = null;

      vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
        if (opts?.signal) {
          opts.signal.addEventListener('abort', () => {
            // Mimic Node's abortChildProcess: emit 'error' with ABORT_ERR BEFORE 'close'
            const abortErr = Object.assign(new Error('The operation was aborted'), {
              code: 'ABORT_ERR',
              name: 'AbortError',
            });
            mockProc.emit('error', abortErr);
            stdout.end();
            mockProc.exitCode = null;
            mockProc.emit('close', null);
          });
        }
        return mockProc;
      }) as any);

      await session.init();
      outputs.length = 0;

      // Start a message — it will hang until the timeout fires
      const msgPromise = session.handleMessage({
        type: 'send_message',
        id: 'msg-timeout-aborterr',
        text: 'Hanging task',
      });

      // Advance past the 1000ms timeout
      vi.advanceTimersByTime(1001);

      // Await the message promise — if ABORT_ERR is not caught, this throws
      await msgPromise;

      // (a) Exactly one error outbound with 'timed out' message
      const errors = outputs.filter((o) => o.type === 'error');
      expect(errors).toHaveLength(1);
      expect((errors[0] as any).message).toContain('timed out');
      expect((errors[0] as any).id).toBe('msg-timeout-aborterr');

      // (b) No done outbound (timeout, not clean completion)
      const dones = outputs.filter((o) => o.type === 'done');
      expect(dones).toHaveLength(0);

      // (c) No spurious extra error outbounds beyond the timeout one
      expect(outputs.filter((o) => o.type === 'error')).toHaveLength(1);

      // (d) ABORT_ERR is suppressed silently — must NOT appear in console.error
      expect(consoleErrorSpy).not.toHaveBeenCalledWith(
        SPAWN_ERROR_LOG_PREFIX,
        expect.anything()
      );
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it('non-ABORT_ERR proc error is logged via console.error', async () => {
    session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are a test assistant.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      // Create a mock process that will emit a non-ABORT error then close normally
      const mockProc = new EventEmitter() as any;
      const stdout = new PassThrough();
      mockProc.stdout = stdout;
      mockProc.stderr = new PassThrough();
      mockProc.stdin = new PassThrough();
      mockProc.exitCode = null;

      vi.mocked(spawn).mockImplementation((() => {
        // Emit 'error' on nextTick, then 'close' on setImmediate — split across ticks to
        // more faithfully model Node's real ordering (abortChildProcess emits 'error'
        // then 'close' across at least one I/O turn). This avoids incidental coupling
        // where an already-ended stream is passed to consumers that haven't yet attached.
        process.nextTick(() => {
          const spawnErr = Object.assign(new Error('spawn ENOENT'), { code: 'ENOENT' });
          mockProc.emit('error', spawnErr);
          setImmediate(() => {
            stdout.push(null);
            mockProc.exitCode = 1;
            mockProc.emit('close', 1);
          });
        });
        return mockProc;
      }) as any);

      await session.init();
      outputs.length = 0;

      // Send a message — it will complete via the close path (exit code 1 → error outbound)
      await session.handleMessage({
        type: 'send_message',
        id: 'msg-enoent',
        text: 'Task that fails to spawn',
      });

      // (a) console.error was called with the diagnostic prefix and the error
      expect(consoleErrorSpy).toHaveBeenCalledWith(
        SPAWN_ERROR_LOG_PREFIX,
        expect.objectContaining({ code: 'ENOENT' })
      );

      // (b) The close-path emits exactly one error outbound (exitCode === 1)
      const errors = outputs.filter((o) => o.type === 'error');
      expect(errors).toHaveLength(1);
      if (errors[0].type !== 'error') throw new Error('Expected error message');
      expect(errors[0].message).toMatch(/Claude CLI exited with code 1/);
      expect(errors[0].id).toBe('msg-enoent');
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it('ABORT_ERR from spawned ChildProcess on user abort emits done (not error) and is not logged', async () => {
    // Covers the second documented branch of the proc.on('error') listener:
    // handleAbort() → abortController.abort() (no reason) → abortChildProcess emits
    // ABORT_ERR → listener swallows silently → emitAbortOrDone emits 'done' (reason ≠ 'timeout')
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    try {
      session = new SidecarSession({
        model: 'claude-opus-4-6',
        workingDirectory: '/tmp/test-project',
        systemPrompt: 'You are a test assistant.',
        timeoutMs: 60000, // Long timeout — abort is user-driven, not timer-driven
      });
      session.onOutput = (msg) => outputs.push(msg);

      const mockProc = new EventEmitter() as any;
      const stdout = new PassThrough();
      mockProc.stdout = stdout;
      mockProc.stderr = new PassThrough();
      mockProc.stdin = new PassThrough();
      mockProc.exitCode = null;

      vi.mocked(spawn).mockImplementation(((_cmd: string, _args: string[], opts: any) => {
        if (opts?.signal) {
          opts.signal.addEventListener('abort', () => {
            // Mimic Node's abortChildProcess: emit ABORT_ERR BEFORE 'close'
            const abortErr = Object.assign(new Error('The operation was aborted'), {
              code: 'ABORT_ERR',
              name: 'AbortError',
            });
            mockProc.emit('error', abortErr);
            stdout.end();
            mockProc.exitCode = null;
            mockProc.emit('close', null);
          });
        }
        return mockProc;
      }) as any);

      await session.init();
      outputs.length = 0;

      // Start a hanging message
      const msgPromise = session.handleMessage({
        type: 'send_message',
        id: 'msg-user-abort',
        text: 'Hanging task',
      });

      // Wait deterministically for the abortController to be initialised.
      // handleSendMessage sets abortController synchronously before invokeSdk's first
      // await, so this resolves within one microtask tick in practice; the deadline
      // guard prevents a silent hang if the implementation ever adds an async step
      // before that initialisation.
      {
        const deadline = Date.now() + 1000;
        while (!session.isInvocationActive()) {
          if (Date.now() > deadline) throw new Error('timed out waiting for abortController');
          await Promise.resolve();
        }
      }

      // User-initiated abort (reason is undefined, NOT 'timeout')
      await session.handleMessage({ type: 'abort' });
      await msgPromise;

      // (a) Single 'done' outbound — user abort, not timeout error
      const dones = outputs.filter((o) => o.type === 'done');
      expect(dones).toHaveLength(1);
      expect(dones[0]).toEqual({ type: 'done', id: 'msg-user-abort' });

      // (b) No error outbound
      const errors = outputs.filter((o) => o.type === 'error');
      expect(errors).toHaveLength(0);

      // (c) ABORT_ERR was suppressed silently — must NOT appear in console.error
      expect(consoleErrorSpy).not.toHaveBeenCalledWith(
        SPAWN_ERROR_LOG_PREFIX,
        expect.anything()
      );
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });
});


// === Step-3 failing tests: permission-prompt wiring ===
// These fail because session.ts does not yet support permissionMcp config,
// and ipc.ts does not yet accept permission_decision inbound messages.

import { readFileSync } from 'node:fs';
import type { PermissionServer } from '../permission-server.js';

/**
 * Build a mock PermissionServer whose onRequest() captures the registered handler.
 */
function makeMockPermissionServer(): {
  server: PermissionServer;
  triggerRequest: (req: { request_id: string; tool_name: string; tool_input: Record<string, unknown> }) => void;
  triggerLatchedHandler: (req: { request_id: string; tool_name: string; tool_input: Record<string, unknown> }) => void;
} {
  let capturedHandler: ((req: any) => void) | null = null;
  let latchedHandler: ((req: any) => void) | null = null;

  const server: PermissionServer = {
    start: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined),
    url: vi.fn().mockReturnValue('http://127.0.0.1:29999/mcp'),
    onRequest: vi.fn((handler: ((req: any) => void) | null) => {
      capturedHandler = handler;
      if (handler !== null) { latchedHandler = handler; }
    }),
    decide: vi.fn(),
    setRemembered: vi.fn(),
    cancelAll: vi.fn(),
  };

  return {
    server,
    triggerRequest: (req) => {
      if (!capturedHandler) throw new Error('onRequest handler was never registered by the session');
      capturedHandler(req);
    },
    triggerLatchedHandler: (req) => {
      if (!latchedHandler) throw new Error('no non-null onRequest handler has ever been registered on this mock');
      latchedHandler(req);
    },
  };
}

describe('SidecarSession permission-prompt wiring (step-3)', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  // (a) spawn args include --permission-prompt-tool and --mcp-config when permissionMcp configured
  it('(a) spawn includes --permission-prompt-tool and --mcp-config when permissionMcp is configured', async () => {
    const { server } = makeMockPermissionServer();
    const permUrl = 'http://127.0.0.1:29999/mcp';

    let capturedMcpConfig: unknown = null;
    vi.mocked(spawn).mockImplementation((((_cmd: string, args: string[]) => {
      const mcpConfigIdx = args.indexOf('--mcp-config');
      if (mcpConfigIdx !== -1) {
        const filePath = args[mcpConfigIdx + 1];
        try {
          capturedMcpConfig = JSON.parse(readFileSync(filePath, 'utf-8'));
        } catch {}
      }
      return createMockProcess([{ type: 'result', session_id: 'sess-perm-a' }]);
    }) as any));

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: permUrl, server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-perm-a', text: 'Hello' });

    const callArgs = vi.mocked(spawn).mock.calls[0]?.[1] as string[];

    // (a1) --permission-prompt-tool with the correct tool path
    const ptIdx = callArgs.indexOf('--permission-prompt-tool');
    expect(ptIdx).toBeGreaterThanOrEqual(0);
    expect(callArgs[ptIdx + 1]).toBe('mcp__reify-permission__approve_tool');

    // (a2) --mcp-config points to a temp file
    const mcpIdx = callArgs.indexOf('--mcp-config');
    expect(mcpIdx).toBeGreaterThanOrEqual(0);

    // (a3) The temp file contains both reify-debug and reify-permission entries
    expect(capturedMcpConfig).toEqual({
      mcpServers: {
        'reify-debug': {
          type: 'http',
          url: 'http://127.0.0.1:3939/mcp',
        },
        'reify-permission': {
          type: 'http',
          url: permUrl,
        },
      },
    });
  });

  // (b) onRequest fires → session emits permission_request outbound
  it('(b) onRequest callback causes session to emit permission_request outbound', async () => {
    const { server, triggerRequest } = makeMockPermissionServer();

    // Use a hanging process so the invocation stays alive while we test mid-flight events
    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();

    vi.mocked(spawn).mockImplementation((() => mockProc) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: 'http://127.0.0.1:29999/mcp', server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    // Start an invocation (don't await — it's hanging until stdout closes)
    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-perm-b',
      text: 'Write something',
    });

    // Give the session a tick to call onRequest
    await new Promise((r) => setTimeout(r, 10));

    // (b1) The session must have registered an onRequest handler with the permission server
    expect((server.onRequest as ReturnType<typeof vi.fn>)).toHaveBeenCalledTimes(1);

    // (b2) Triggering the handler should cause a permission_request outbound
    triggerRequest({
      request_id: 'req-b1',
      tool_name: 'Write',
      tool_input: { path: '/tmp/x' },
    });

    await new Promise((r) => setTimeout(r, 10));

    const permReq = outputs.find((o) => o.type === 'permission_request') as any;
    expect(permReq).toBeDefined();
    expect(permReq.id).toBe('msg-perm-b');
    expect(permReq.request_id).toBe('req-b1');
    expect(permReq.tool_name).toBe('Write');
    expect(permReq.tool_input).toEqual({ path: '/tmp/x' });

    // Cleanup: close the process
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });

  // (c) handleMessage(permission_decision) calls server.decide()
  it('(c) handleMessage(permission_decision) calls server.decide()', async () => {
    const { server, triggerRequest } = makeMockPermissionServer();

    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();

    vi.mocked(spawn).mockImplementation((() => mockProc) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: 'http://127.0.0.1:29999/mcp', server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-perm-c',
      text: 'Write something',
    });

    await new Promise((r) => setTimeout(r, 10));

    // Trigger the onRequest to register a pending request_id
    triggerRequest({ request_id: 'req-c1', tool_name: 'Write', tool_input: { path: '/tmp/y' } });
    await new Promise((r) => setTimeout(r, 10));

    // Send the permission_decision inbound
    await session.handleMessage({
      type: 'permission_decision',
      request_id: 'req-c1',
      behavior: 'allow',
    } as any);

    // server.decide() must have been called with the correct args
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('req-c1', { behavior: 'allow' });

    // Cleanup
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });

  // (d) handleMessage(permission_decision) with remember: true calls server.setRemembered()
  it('(d) permission_decision with remember: true calls server.setRemembered(tool_name)', async () => {
    const { server, triggerRequest } = makeMockPermissionServer();

    const mockProc = new EventEmitter() as any;
    const stdout = new PassThrough();
    mockProc.stdout = stdout;
    mockProc.stderr = new PassThrough();
    mockProc.stdin = new PassThrough();

    vi.mocked(spawn).mockImplementation((() => mockProc) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: 'http://127.0.0.1:29999/mcp', server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    const msgPromise = session.handleMessage({
      type: 'send_message',
      id: 'msg-perm-d',
      text: 'Write something',
    });

    await new Promise((r) => setTimeout(r, 10));

    // Register a pending request for 'Write'
    triggerRequest({ request_id: 'req-d1', tool_name: 'Write', tool_input: {} });
    await new Promise((r) => setTimeout(r, 10));

    // Send permission_decision with remember: true
    await session.handleMessage({
      type: 'permission_decision',
      request_id: 'req-d1',
      behavior: 'allow',
      remember: true,
    } as any);

    // Both decide() and setRemembered() should have been called
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('req-d1', expect.objectContaining({ behavior: 'allow' }));
    expect((server.setRemembered as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('Write');

    // Cleanup
    stdout.push(null);
    mockProc.exitCode = 0;
    mockProc.emit('close', 0);
    await msgPromise;
  });

  // (e) permission_decision with no permission server configured produces error outbound
  it('(e) permission_decision with no permissionMcp configured emits a structured error', async () => {
    // Session created without permissionMcp
    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({
      type: 'permission_decision',
      request_id: 'req-e1',
      behavior: 'allow',
    } as any);

    const errors = outputs.filter((o) => o.type === 'error');
    expect(errors).toHaveLength(1);
    expect((errors[0] as any).message).toMatch(/permission/i);
    // Should not crash or throw
  });

  // (f) onRequest arriving outside an in-flight invocation (currentInvocationId is null)
  //     must deny immediately, emit a diagnostic notice, and NOT emit permission_request
  it('(f) orphan permission request (no in-flight invocation) is denied and emits a notice', async () => {
    const { server, triggerRequest } = makeMockPermissionServer();

    // Create the session but do NOT call handleMessage — currentInvocationId stays null
    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: 'http://127.0.0.1:29999/mcp', server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    // Simulate a late/orphan permission request arriving with no active invocation
    triggerRequest({
      request_id: 'req-orphan',
      tool_name: 'Write',
      tool_input: { path: '/tmp/x' },
    });

    // (f1) No permission_request outbound must have been emitted
    const permReqs = outputs.filter((o) => o.type === 'permission_request');
    expect(permReqs).toHaveLength(0);

    // (f2) server.decide must have been called exactly once with deny
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledTimes(1);
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('req-orphan', { behavior: 'deny' });

    // (f3) Exactly one diagnostic notice with code 'permission_request_orphaned' must have been emitted
    const notices = outputs.filter((o) => o.type === 'notice') as any[];
    expect(notices).toHaveLength(1);
    expect(notices[0]).toMatchObject({
      type: 'notice',
      id: '',
      code: 'permission_request_orphaned',
      message: expect.any(String),
    });

    // (f4) A follow-up permission_decision for req-orphan does NOT trigger setRemembered —
    //      proving the orphan branch never registered the entry in pendingPermissionRequests
    //      (see the 'Do NOT register in pendingPermissionRequests' comment in session.ts).
    (server.decide as ReturnType<typeof vi.fn>).mockClear();
    await session.handleMessage({ type: 'permission_decision', request_id: 'req-orphan', behavior: 'allow', remember: true } as any);
    expect(server.decide as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
      'req-orphan',
      expect.objectContaining({ behavior: 'allow' }),
    );
    expect((server.setRemembered as ReturnType<typeof vi.fn>)).not.toHaveBeenCalled();
  });

  // (g) onRequest arriving AFTER a completed invocation (currentInvocationId reset to null
  //     at session.ts:266 in the finally block) must also deny immediately and emit a notice.
  //     This documents the post-completion race explicitly called out in the source comment.
  it('(g) orphan permission request after a completed invocation is denied and emits a notice', async () => {
    const { server, triggerRequest } = makeMockPermissionServer();

    // Wire spawn to complete the invocation immediately with a result event
    vi.mocked(spawn).mockImplementation(
      () => createMockProcess([{ type: 'result', session_id: 'sess-g' }]) as any
    );

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      permissionMcp: { url: 'http://127.0.0.1:29999/mcp', server },
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    // Run a full invocation to completion — currentInvocationId is reset to null in the finally block
    await session.handleMessage({ type: 'send_message', id: 'msg-g', text: 'Hello' });

    // Reset output collector and mock call state so we get clean post-completion assertions
    outputs.length = 0;
    (server.decide as ReturnType<typeof vi.fn>).mockClear();

    // Simulate a late CLI permission request arriving after the invocation ended
    triggerRequest({
      request_id: 'req-post',
      tool_name: 'Bash',
      tool_input: { command: 'rm -rf /' },
    });

    // (g1) No permission_request outbound must have been emitted
    const permReqs = outputs.filter((o) => o.type === 'permission_request');
    expect(permReqs).toHaveLength(0);

    // (g2) server.decide must have been called exactly once with deny
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledTimes(1);
    expect((server.decide as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('req-post', { behavior: 'deny' });

    // (g3) Exactly one diagnostic notice with code 'permission_request_orphaned' must have been emitted
    const notices = outputs.filter((o) => o.type === 'notice') as any[];
    expect(notices).toHaveLength(1);
    expect(notices[0]).toMatchObject({
      type: 'notice',
      id: '',
      code: 'permission_request_orphaned',
      message: expect.any(String),
    });

    // (g4) A follow-up permission_decision for req-post does NOT trigger setRemembered —
    //      proving the orphan branch never registered the entry in pendingPermissionRequests
    //      (see the 'Do NOT register in pendingPermissionRequests' comment in session.ts).
    (server.decide as ReturnType<typeof vi.fn>).mockClear();
    await session.handleMessage({ type: 'permission_decision', request_id: 'req-post', behavior: 'allow', remember: true } as any);
    expect(server.decide as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
      'req-post',
      expect.objectContaining({ behavior: 'allow' }),
    );
    expect((server.setRemembered as ReturnType<typeof vi.fn>)).not.toHaveBeenCalled();
  });
});

describe('SidecarSession reify-debug MCP wiring (task 3210)', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  it('writes reify-debug MCP config unconditionally (no permissionMcp)', async () => {
    let capturedMcpConfig: unknown = null;
    let capturedArgs: string[] = [];

    vi.mocked(spawn).mockImplementation((((_cmd: string, args: string[]) => {
      capturedArgs = args;
      const mcpConfigIdx = args.indexOf('--mcp-config');
      if (mcpConfigIdx !== -1) {
        const filePath = args[mcpConfigIdx + 1];
        try {
          capturedMcpConfig = JSON.parse(readFileSync(filePath, 'utf-8'));
        } catch {}
      }
      return createMockProcess([{ type: 'result', session_id: 'sess-debug-mcp' }]);
    }) as any));

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      // No permissionMcp — reify-debug should still be written
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-debug-mcp', text: 'Hello' });

    // --mcp-config must be present even without permissionMcp
    const mcpIdx = capturedArgs.indexOf('--mcp-config');
    expect(mcpIdx).toBeGreaterThanOrEqual(0);
    expect(capturedArgs[mcpIdx + 1]).toBeTruthy();

    // MCP config contains reify-debug only (no reify-permission)
    expect(capturedMcpConfig).toEqual({
      mcpServers: {
        'reify-debug': {
          type: 'http',
          url: 'http://127.0.0.1:3939/mcp',
        },
      },
    });
  });

  it('does NOT include --permission-prompt-tool when permissionMcp is not configured', async () => {
    let capturedArgs: string[] = [];

    vi.mocked(spawn).mockImplementation((((_cmd: string, args: string[]) => {
      capturedArgs = args;
      return createMockProcess([{ type: 'result', session_id: 'sess-debug-nopp' }]);
    }) as any));

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-debug-nopp', text: 'Hello' });

    expect(capturedArgs).not.toContain('--permission-prompt-tool');
  });
});

describe('SidecarSession permission-mode + allowed-tools (task 3210)', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  function captureSpawnArgs(): string[] {
    let args: string[] = [];
    vi.mocked(spawn).mockImplementation((((_cmd: string, a: string[]) => {
      args = a;
      return createMockProcess([{ type: 'result', session_id: 'sess-perm-mode' }]);
    }) as any));
    return args; // reference captured by closure
  }

  async function runSession(withPermissionMcp: boolean): Promise<string[]> {
    let capturedArgs: string[] = [];
    vi.mocked(spawn).mockImplementation((((_cmd: string, a: string[]) => {
      capturedArgs = a;
      return createMockProcess([{ type: 'result', session_id: 'sess-pm' }]);
    }) as any));

    const config: any = {
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
    };
    if (withPermissionMcp) {
      const { server } = makeMockPermissionServer();
      config.permissionMcp = { url: 'http://127.0.0.1:29999/mcp', server };
    }
    const session = new SidecarSession(config);
    session.onOutput = (msg) => outputs.push(msg);
    await session.handleMessage({ type: 'send_message', id: 'msg-pm', text: 'Hello' });
    return capturedArgs;
  }

  it('(a) args contain --permission-mode bypassPermissions (with permissionMcp)', async () => {
    const args = await runSession(true);
    const idx = args.indexOf('--permission-mode');
    expect(idx).toBeGreaterThanOrEqual(0);
    expect(args[idx + 1]).toBe('bypassPermissions');
  });

  it('(b) args contain --allowed-tools with correct string (with permissionMcp)', async () => {
    const args = await runSession(true);
    const idx = args.indexOf('--allowed-tools');
    expect(idx).toBeGreaterThanOrEqual(0);
    expect(args[idx + 1]).toBe('Read Edit Write Bash Glob Grep mcp__reify-debug__*');
  });

  it('(c) args do NOT contain --dangerously-skip-permissions (with permissionMcp)', async () => {
    const args = await runSession(true);
    expect(args).not.toContain('--dangerously-skip-permissions');
  });

  it('(a) args contain --permission-mode bypassPermissions (no permissionMcp)', async () => {
    const args = await runSession(false);
    const idx = args.indexOf('--permission-mode');
    expect(idx).toBeGreaterThanOrEqual(0);
    expect(args[idx + 1]).toBe('bypassPermissions');
  });

  it('(b) args contain --allowed-tools with correct string (no permissionMcp)', async () => {
    const args = await runSession(false);
    const idx = args.indexOf('--allowed-tools');
    expect(idx).toBeGreaterThanOrEqual(0);
    expect(args[idx + 1]).toBe('Read Edit Write Bash Glob Grep mcp__reify-debug__*');
  });

  it('(c) args do NOT contain --dangerously-skip-permissions (no permissionMcp)', async () => {
    const args = await runSession(false);
    expect(args).not.toContain('--dangerously-skip-permissions');
  });
});

describe('SidecarSession sandbox wrap (task 3210)', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
    // Landlock state is supplied via SessionConfig.landlockAvailable (task 3281) —
    // no synchronous probe mock needed here. Set by index.ts after the async startup probe.
    // Restore wrapClaudeArgs default implementation: passthrough with/without landlockExec
    vi.mocked(wrapClaudeArgs).mockReset();
    vi.mocked(wrapClaudeArgs).mockImplementation((args: string[], _ws: string, le?: string) =>
      le ? { cmd: 'python3', args: [le, ...args] } : { cmd: 'claude', args: [...args] }
    );
  });

  // (a) landlockAvailable: true in config → spawn called with python3 wrap
  it('(a) landlockAvailable=true in config: spawn uses python3 wrap, wrapClaudeArgs called with landlockExec', async () => {
    const le = '/path/landlock_exec.py';
    const ws = '/tmp/ws';

    // landlockAvailable is supplied via config — override wrapClaudeArgs to produce full python3 wrap
    vi.mocked(wrapClaudeArgs).mockImplementation((args: string[], workspace: string, landlockExec?: string) => ({
      cmd: 'python3',
      args: [landlockExec!, '--writable', workspace, '--writable', os.homedir() + '/.claude', '--writable', '/tmp', '--', 'claude', ...args],
    }));

    let spawnCmd = '';
    let spawnArgs: string[] = [];
    vi.mocked(spawn).mockImplementation(((cmd: string, args: string[]) => {
      spawnCmd = cmd;
      spawnArgs = args;
      return createMockProcess([{ type: 'result', session_id: 'sess-sandbox-a' }]);
    }) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      workspace: ws,
      landlockExec: le,
      landlockAvailable: true,
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-sandbox-a', text: 'Hello' });

    // spawn called with python3 (not claude directly)
    expect(spawnCmd).toBe('python3');
    // first arg in args is the landlockExec path
    expect(spawnArgs[0]).toBe(le);
    // writable flags and workspace are present
    expect(spawnArgs).toContain('--writable');
    expect(spawnArgs).toContain(ws);
    // '--' separator present
    expect(spawnArgs).toContain('--');
    // 'claude' appears right after '--'
    const ddIdx = spawnArgs.indexOf('--');
    expect(spawnArgs[ddIdx + 1]).toBe('claude');
    // claude-specific args follow (including permission-mode and mcp-config from step-6/step-4)
    const postClaude = spawnArgs.slice(ddIdx + 2);
    expect(postClaude).toContain('--permission-mode');
    expect(postClaude).toContain('bypassPermissions');
    expect(postClaude).toContain('--mcp-config');

    // wrapClaudeArgs was called with (claudeArgs, workspace, landlockExec)
    expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalledTimes(1);
    const [wrapArgs, wrapWs, wrapLe] = vi.mocked(wrapClaudeArgs).mock.calls[0] as [string[], string, string | undefined];
    expect(wrapWs).toBe(ws);
    expect(wrapLe).toBe(le);
    // the claude args passed to wrapClaudeArgs include the allowlist args
    expect(wrapArgs).toContain('--permission-mode');
  });

  // (b) landlockAvailable: false in config (but landlockExec provided) — wrapClaudeArgs gets undefined
  it('(b) landlockAvailable=false in config: wrapClaudeArgs called with undefined landlockExec, spawn uses claude', async () => {
    // landlockAvailable: false → no wrap regardless of landlockExec being set
    // wrapClaudeArgs uses default mock (returns {cmd:'claude',...} when le is undefined)

    let spawnCmd = '';
    vi.mocked(spawn).mockImplementation(((cmd: string, _args: string[]) => {
      spawnCmd = cmd;
      return createMockProcess([{ type: 'result', session_id: 'sess-sandbox-b' }]);
    }) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      workspace: '/tmp/ws',
      landlockExec: '/path/le.py',  // provided, but landlockAvailable=false → no wrap
      landlockAvailable: false,
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-sandbox-b', text: 'Hello' });

    // spawn called with claude (no python3 wrap since landlockAvailable=false)
    expect(spawnCmd).toBe('claude');

    // wrapClaudeArgs was called with undefined landlockExec (landlockAvailable=false → no wrap)
    expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalledTimes(1);
    const [, , wrapLe] = vi.mocked(wrapClaudeArgs).mock.calls[0] as [string[], string, string | undefined];
    expect(wrapLe).toBeUndefined();
  });

  // (c) no landlockExec configured — direct claude spawn, no wrap
  it('(c) no landlockExec: spawn called with claude directly', async () => {
    let spawnCmd = '';
    vi.mocked(spawn).mockImplementation(((cmd: string, _args: string[]) => {
      spawnCmd = cmd;
      return createMockProcess([{ type: 'result', session_id: 'sess-sandbox-c' }]);
    }) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      // No workspace or landlockExec — sandbox not requested
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-sandbox-c', text: 'Hello' });

    // spawn called with claude (no python3 wrap)
    expect(spawnCmd).toBe('claude');
  });

  // (notice-a) landlockAvailable: false + landlockExec set → emits one notice + console.warn
  it('(notice-a) landlockAvailable=false with landlockExec: emits sandbox_unavailable notice and console.warn', async () => {
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-notice-a' }])
    ) as any);

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      workspace: '/tmp/ws',
      landlockExec: '/path/le.py',
      landlockAvailable: false,
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-notice-a', text: 'Hello' });

    // exactly one notice with code 'sandbox_unavailable'
    const notices = outputs.filter((o) => o.type === 'notice') as Array<{ type: 'notice'; id: string; code: string; message: string }>;
    expect(notices).toHaveLength(1);
    expect(notices[0].code).toBe('sandbox_unavailable');
    expect(notices[0].id).toBe('msg-notice-a');
    expect(notices[0].message).toMatch(/unrestricted/i);

    // console.warn called with a message mentioning sandbox
    expect(warnSpy).toHaveBeenCalledTimes(1);
    expect(warnSpy.mock.calls[0][0]).toMatch(/sandbox unavailable/i);

    warnSpy.mockRestore();
  });

  // (notice-b) landlockAvailable: true + landlockExec set → no notice, no console.warn
  it('(notice-b) landlockAvailable=true with landlockExec: no sandbox_unavailable notice', async () => {
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-notice-b' }])
    ) as any);

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      workspace: '/tmp/ws',
      landlockExec: '/path/le.py',
      landlockAvailable: true,
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-notice-b', text: 'Hello' });

    // no sandbox_unavailable notice
    const notices = outputs.filter((o) => o.type === 'notice' && (o as any).code === 'sandbox_unavailable');
    expect(notices).toHaveLength(0);

    // no console.warn
    expect(warnSpy).not.toHaveBeenCalled();

    warnSpy.mockRestore();
  });

  // (notice-c) no landlockExec → no notice, no console.warn
  it('(notice-c) no landlockExec: no sandbox_unavailable notice emitted', async () => {
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-notice-c' }])
    ) as any);

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      // No landlockExec — no sandbox was requested
    });
    session.onOutput = (msg) => outputs.push(msg);

    await session.handleMessage({ type: 'send_message', id: 'msg-notice-c', text: 'Hello' });

    // no notice of any kind for sandbox
    const notices = outputs.filter((o) => o.type === 'notice' && (o as any).code === 'sandbox_unavailable');
    expect(notices).toHaveLength(0);
    expect(warnSpy).not.toHaveBeenCalled();

    warnSpy.mockRestore();
  });

  // (notice-d) idempotency: notice emitted only once across multiple send_message calls
  it('(notice-d) sandbox_unavailable notice emitted exactly once per session', async () => {
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-notice-d' }])
    ) as any);

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
      workspace: '/tmp/ws',
      landlockExec: '/path/le.py',
      landlockAvailable: false,
    } as any);
    session.onOutput = (msg) => outputs.push(msg);

    // First call
    await session.handleMessage({ type: 'send_message', id: 'msg-notice-d1', text: 'Hello' });
    // Reset spawn mock for the second call
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-notice-d2' }])
    ) as any);
    // Second call
    await session.handleMessage({ type: 'send_message', id: 'msg-notice-d2', text: 'World' });

    // notice emitted exactly once (not twice)
    const notices = outputs.filter(
      (o): o is NoticeMessage => o.type === 'notice' && (o as NoticeMessage).code === 'sandbox_unavailable'
    );
    expect(notices).toHaveLength(1);
    expect((notices[0] as NoticeMessage).id).toBe('msg-notice-d1');
    expect(warnSpy).toHaveBeenCalledTimes(1);

    warnSpy.mockRestore();
  });
});

describe('session.test.ts /tmp leak guard (task 3283)', () => {
  // Match exactly the prefix session.ts:309 uses (`mkdtempSync(..., 'reify-mcp-')`).
  // Narrower than /^reify-/ on purpose: parallel tests in this repo create sibling
  // prefixes (`reify-tools-` from discover-mcp-tools.test.ts, `reify-commit-`,
  // `reify-import-test`, `reify-jobserver`, …) whose appearance between snapshots
  // would otherwise trigger a spurious false-positive leak report.
  const REIFY_MCP_TMP_PREFIX = /^reify-mcp-/;

  it('does not create real /tmp/reify-mcp-* directories during session.handleMessage', async () => {
    // Obtain handles to the REAL filesystem and os, bypassing any vi.mock('node:fs')
    // that may be active. This lets the test observe actual on-disk state.
    const realFs = await vi.importActual<typeof import('node:fs')>('node:fs');
    const realOs = await vi.importActual<typeof import('node:os')>('node:os');

    // Snapshot /tmp before the test.
    // Filter matches only the prefix session.ts:309 creates (`reify-mcp-`), not
    // broader sibling prefixes that concurrent tests may produce between snapshots.
    const tmpdir = realOs.tmpdir();
    const before = new Set<string>(
      realFs.readdirSync(tmpdir).filter((name: string) => REIFY_MCP_TMP_PREFIX.test(name))
    );

    vi.mocked(spawn).mockReset();
    vi.mocked(spawn).mockImplementation((() =>
      createMockProcess([{ type: 'result', session_id: 'sess-leak' }])
    ) as any);

    const session = new SidecarSession({
      model: 'claude-opus-4-6',
      workingDirectory: '/tmp/test-project',
      systemPrompt: 'You are helpful.',
    });
    session.onOutput = () => {};

    try {
      await session.handleMessage({ type: 'send_message', id: 'msg-leak', text: 'Hello' });

      // Snapshot /tmp after — the delta (new entries not in `before`) must be empty.
      // If node:fs is NOT mocked, session.ts calls real mkdtempSync and this fails.
      // Once vi.mock('node:fs', factory) is active, no real dirs are created.
      const after = new Set<string>(
        realFs.readdirSync(tmpdir).filter((name: string) => REIFY_MCP_TMP_PREFIX.test(name))
      );
      // `leaked` = after \ before (set-difference): if empty, after ⊆ before and
      // no new /tmp entries appeared. Using set-difference rather than set-equality
      // is intentional: a concurrent process removing a pre-existing entry would
      // shrink `after` relative to `before`, but the filter still yields [] (correct).
      const leaked = [...after].filter((d) => !before.has(d));
      expect(leaked, 'leaked /tmp/reify-mcp-* dirs: ' + leaked.join(', ')).toHaveLength(0);
    } finally {
      // Destroy runs even if the leak assertion throws, so session state doesn't
      // linger across tests in the same worker. Matches the convention at lines 1414,
      // 1487, 1511, 1534, 1559, 1588: destroy() is void/synchronous, no await needed.
      session.destroy();
    }
  });

  it('REIFY_MCP_TMP_PREFIX does not match sibling reify-* prefixes from concurrent tests', () => {
    // Pure regex assertion — no real FS I/O. Guards against future regex widening
    // (e.g. someone changing /^reify-mcp-/ back to /^reify-/) without going through a
    // disk-side-effect round-trip. The comment on REIFY_MCP_TMP_PREFIX explains why the
    // prefixes below (`reify-tools-`, `reify-commit-`, etc.) must not match.
    expect(REIFY_MCP_TMP_PREFIX.test('reify-tools-abc')).toBe(false);
    expect(REIFY_MCP_TMP_PREFIX.test('reify-commit-xyz')).toBe(false);
    expect(REIFY_MCP_TMP_PREFIX.test('reify-import-test')).toBe(false);
    expect(REIFY_MCP_TMP_PREFIX.test('reify-jobserver')).toBe(false);
    // And that it still matches what session.ts:309 actually creates:
    expect(REIFY_MCP_TMP_PREFIX.test('reify-mcp-ab12CD')).toBe(true);
  });
});

describe('virtual node:fs mock readFileSync semantics (task 3306)', () => {
  it('returns Buffer unless an encoding is explicitly supplied (matches Node.js options-object semantics)', () => {
    const p = '/virt/hello.txt';
    (mockFs as any).writeFileSync(p, 'hello');

    // Case 1: no second arg → Buffer (regression guard, already passes)
    const buf = mockFs.readFileSync(p);
    expect(Buffer.isBuffer(buf)).toBe(true);
    expect((buf as Buffer).toString('utf-8')).toBe('hello');

    // Case 2: string encoding → string (regression guard, already passes)
    const str = mockFs.readFileSync(p, 'utf-8');
    expect(typeof str).toBe('string');
    expect(str).toBe('hello');

    // Case 3: empty options object → Buffer (regression guard for options-object handling)
    const bufFromOpts = mockFs.readFileSync(p, {});
    expect(Buffer.isBuffer(bufFromOpts)).toBe(true);

    // Case 4: options object with encoding → string (must keep passing after the fix)
    const strFromOpts = mockFs.readFileSync(p, { encoding: 'utf-8' });
    expect(typeof strFromOpts).toBe('string');
    expect(strFromOpts).toBe('hello');
  });
});

describe('REIFY_DEBUG_PORT env override (task 4340)', () => {
  let outputs: OutboundMessage[];

  beforeEach(() => {
    outputs = [];
    vi.mocked(spawn).mockReset();
  });

  it('uses REIFY_DEBUG_PORT=4500 in the MCP config url when set', async () => {
    const savedPort = process.env['REIFY_DEBUG_PORT'];
    process.env['REIFY_DEBUG_PORT'] = '4500';
    try {
      let capturedMcpConfig: unknown = null;
      vi.mocked(spawn).mockImplementation((((_cmd: string, args: string[]) => {
        const mcpConfigIdx = args.indexOf('--mcp-config');
        if (mcpConfigIdx !== -1) {
          const filePath = args[mcpConfigIdx + 1];
          try {
            capturedMcpConfig = JSON.parse(readFileSync(filePath, 'utf-8'));
          } catch {}
        }
        return createMockProcess([{ type: 'result', session_id: 'sess-4340-port' }]);
      }) as any));

      const session = new SidecarSession({
        model: 'claude-opus-4-6',
        workingDirectory: '/tmp/test-project',
        systemPrompt: 'You are helpful.',
      });
      session.onOutput = (msg) => outputs.push(msg);
      await session.handleMessage({ type: 'send_message', id: 'msg-4340-port', text: 'Hello' });

      expect((capturedMcpConfig as any)?.mcpServers?.['reify-debug']?.url).toBe(
        'http://127.0.0.1:4500/mcp',
      );
    } finally {
      if (savedPort === undefined) {
        delete process.env['REIFY_DEBUG_PORT'];
      } else {
        process.env['REIFY_DEBUG_PORT'] = savedPort;
      }
    }
  });

  it('uses default port 3939 when REIFY_DEBUG_PORT is unset (back-compat)', async () => {
    const savedPort = process.env['REIFY_DEBUG_PORT'];
    delete process.env['REIFY_DEBUG_PORT'];
    try {
      let capturedMcpConfig: unknown = null;
      vi.mocked(spawn).mockImplementation((((_cmd: string, args: string[]) => {
        const mcpConfigIdx = args.indexOf('--mcp-config');
        if (mcpConfigIdx !== -1) {
          const filePath = args[mcpConfigIdx + 1];
          try {
            capturedMcpConfig = JSON.parse(readFileSync(filePath, 'utf-8'));
          } catch {}
        }
        return createMockProcess([{ type: 'result', session_id: 'sess-4340-default' }]);
      }) as any));

      const session = new SidecarSession({
        model: 'claude-opus-4-6',
        workingDirectory: '/tmp/test-project',
        systemPrompt: 'You are helpful.',
      });
      session.onOutput = (msg) => outputs.push(msg);
      await session.handleMessage({ type: 'send_message', id: 'msg-4340-default', text: 'Hello' });

      expect((capturedMcpConfig as any)?.mcpServers?.['reify-debug']?.url).toBe(
        'http://127.0.0.1:3939/mcp',
      );
    } finally {
      if (savedPort !== undefined) {
        process.env['REIFY_DEBUG_PORT'] = savedPort;
      }
    }
  });
});
