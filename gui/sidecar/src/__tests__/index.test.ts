/**
 * Tests for the main() entrypoint's permission-server lifecycle (step-5).
 *
 * Verifies:
 * (a) main() creates + starts a PermissionServer before emitting `ready`
 * (b) SidecarSession is constructed with permissionMcp.url = server.url()
 * (c) When input is destroyed (or SIGTERM fires), server.stop() is called exactly once
 *
 * Uses vi.mock to stub createPermissionServer so tests never bind a real port.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { PermissionServer } from '../permission-server.js';

// --- Module mocks (hoisted by vitest) ---

vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

vi.mock('../permission-server.js', () => ({
  createPermissionServer: vi.fn(),
}));

// Sandbox helpers are imported by session.ts (and index.ts after task 3281).
// Mock them so tests never invoke real python3.
vi.mock('../sandbox.js', () => ({
  wrapClaudeArgs: vi.fn((args: string[], _ws: string, le?: string) =>
    le ? { cmd: 'python3', args: [le, ...args] } : { cmd: 'claude', args: [...args] }
  ),
  probeLandlockAsync: vi.fn().mockResolvedValue(false),
}));

// --- Imports (after mocks) ---

import { spawn } from 'node:child_process';
import { createPermissionServer } from '../permission-server.js';
import { wrapClaudeArgs, probeLandlockAsync } from '../sandbox.js';
import { main } from '../index.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TEST_PERMISSION_URL = 'http://127.0.0.1:12345/mcp';

function makePermissionServerMock(): PermissionServer & {
  start: ReturnType<typeof vi.fn>;
  stop: ReturnType<typeof vi.fn>;
  url: ReturnType<typeof vi.fn>;
  onRequest: ReturnType<typeof vi.fn>;
  decide: ReturnType<typeof vi.fn>;
  setRemembered: ReturnType<typeof vi.fn>;
  cancelAll: ReturnType<typeof vi.fn>;
} {
  return {
    start: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined),
    url: vi.fn().mockReturnValue(TEST_PERMISSION_URL),
    onRequest: vi.fn(),
    decide: vi.fn(),
    setRemembered: vi.fn(),
    cancelAll: vi.fn(),
  };
}

/**
 * Create a minimal mock subprocess that never emits events and keeps stdout open.
 * The test drives closing manually so main() stays blocked in the read loop.
 */
function makeIdleProcess(): any {
  const proc = new EventEmitter() as any;
  proc.stdout = new PassThrough();
  proc.stderr = new PassThrough();
  proc.stdin = new PassThrough();
  proc.exitCode = null;
  return proc;
}

/**
 * Collect newline-delimited JSON messages from `output` until `timeoutMs` elapses.
 */
function collectOutput(output: PassThrough, timeoutMs = 1000): Promise<any[]> {
  return new Promise((resolve) => {
    const msgs: any[] = [];
    let buffer = '';
    const onData = (chunk: Buffer | string) => {
      buffer += chunk.toString();
      let idx: number;
      while ((idx = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 1);
        if (line.length > 0) msgs.push(JSON.parse(line));
      }
    };
    output.on('data', onData);
    setTimeout(() => {
      output.removeListener('data', onData);
      resolve(msgs);
    }, timeoutMs);
    output.on('end', () => {
      clearTimeout(0 as any); // no-op; let timer fire
    });
  });
}

/**
 * Wait until `predicate` returns true for one of the messages collected from `output`,
 * or until `timeoutMs` elapses.
 */
function waitForMessage(
  output: PassThrough,
  predicate: (m: any) => boolean,
  timeoutMs = 2000
): Promise<any> {
  return new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => {
      output.removeListener('data', onData);
      reject(new Error('waitForMessage timed out'));
    }, timeoutMs);

    const onData = (chunk: Buffer | string) => {
      buffer += chunk.toString();
      let idx: number;
      while ((idx = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 1);
        if (line.length > 0) {
          try {
            const msg = JSON.parse(line);
            if (predicate(msg)) {
              clearTimeout(timer);
              output.removeListener('data', onData);
              resolve(msg);
            }
          } catch {
            // ignore parse errors
          }
        }
      }
    };
    output.on('data', onData);
  });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('main() permission server lifecycle (step-5)', () => {
  let serverMock: ReturnType<typeof makePermissionServerMock>;

  beforeEach(() => {
    serverMock = makePermissionServerMock();
    vi.mocked(createPermissionServer).mockReturnValue(serverMock);
    vi.mocked(spawn).mockReset();
    vi.mocked(spawn).mockImplementation((() => makeIdleProcess()) as any);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // (a) server.start() is called before ready is emitted
  it('(a) createPermissionServer() is called and start() resolves before ready is emitted', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    // Record the resolution order: 'start' when start() resolves, 'ready' when we observe
    // the ready message. If start() is awaited before session.init(), 'start' precedes 'ready'.
    const order: string[] = [];

    let resolveStart!: () => void;
    const startSignal = new Promise<void>((r) => { resolveStart = r; });

    vi.mocked(serverMock.start).mockImplementation(async () => {
      // Ensure start() is asynchronous so ordering is detectable
      await new Promise<void>((r) => setImmediate(r));
      order.push('start');
    });

    const readyWatcher = waitForMessage(output, (m) => m.type === 'ready').then((m) => {
      order.push('ready');
      return m;
    });

    const mainPromise = main(input, output);

    // Wait for ready
    await readyWatcher;

    // Assertions: createPermissionServer was called once
    expect(vi.mocked(createPermissionServer)).toHaveBeenCalledOnce();
    // start() was invoked
    expect(serverMock.start).toHaveBeenCalledOnce();
    // start() completed before ready was emitted
    expect(order.indexOf('start')).toBeLessThan(order.indexOf('ready'));

    // Cleanup — use end() not destroy() to avoid ERR_STREAM_PREMATURE_CLOSE
    input.end();
    await mainPromise;
  });

  // (b) SidecarSession receives permissionMcp wired with the server's url()
  //     Verified indirectly: when a send_message triggers spawn(), the spawn args
  //     include --permission-prompt-tool (only added when permissionMcp is configured).
  it('(b) spawn args include --permission-prompt-tool when permissionMcp is wired', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    const mainPromise = main(input, output);

    // Wait for ready
    await waitForMessage(output, (m) => m.type === 'ready');

    // Send a message to trigger invokeSdk → spawn()
    input.write(
      JSON.stringify({ type: 'send_message', id: 'msg-b1', text: 'hello' }) + '\n'
    );

    // Wait until spawn is called
    await new Promise<void>((resolve, reject) => {
      const deadline = Date.now() + 2000;
      const check = setInterval(() => {
        if (vi.mocked(spawn).mock.calls.length > 0) {
          clearInterval(check);
          resolve();
        } else if (Date.now() > deadline) {
          clearInterval(check);
          reject(new Error('spawn was never called'));
        }
      }, 10);
    });

    const spawnArgs: string[] = vi.mocked(spawn).mock.calls[0][1] as string[];
    expect(spawnArgs).toContain('--permission-prompt-tool');
    expect(spawnArgs).toContain('mcp__reify-permission__approve_tool');
    // The URL returned by our mock server should appear in an --mcp-config file
    // (we verify the flag is present; the file contents are tested in session.test.ts)
    expect(spawnArgs).toContain('--mcp-config');

    // Cleanup — use end() not destroy() to avoid ERR_STREAM_PREMATURE_CLOSE
    input.end();
    await mainPromise;
  });

  // (c) server.stop() is called exactly once when input ends (graceful shutdown)
  it('(c) server.stop() is called exactly once when input stream ends', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    const mainPromise = main(input, output);

    // Wait for ready (ensures main() is fully initialized)
    await waitForMessage(output, (m) => m.type === 'ready');

    // End input gracefully — triggers the for-await loop to finish, then finally block runs
    input.end();
    await mainPromise;

    expect(serverMock.stop).toHaveBeenCalledOnce();
  });

  // (c-sigterm) SIGTERM also triggers server.stop() exactly once.
  // SIGTERM calls input.destroy() (not input.end()), so main() may reject with
  // ERR_STREAM_PREMATURE_CLOSE. We pre-attach .catch() to suppress the unhandled
  // rejection, then wait for settlement and assert stop() was called.
  it('(c-sigterm) SIGTERM triggers server.stop() exactly once', async () => {
    const input = new PassThrough();
    const output = new PassThrough();

    const mainPromise = main(input, output);
    // Pre-attach to suppress "unhandled rejection" noise from ERR_STREAM_PREMATURE_CLOSE.
    // The rejection is caused by the shutdown handler calling input.destroy().
    const mainSettled = mainPromise.catch(() => undefined);

    await waitForMessage(output, (m) => m.type === 'ready');

    // Snapshot SIGTERM listeners before emitting so we can clean up any that
    // main() fails to remove if the test fails before its own finally block runs.
    // (main() *does* call process.removeListener in its finally, but this guard
    // makes the test robust against future regressions and vitest worker reuse.)
    const sigtermBefore = process.rawListeners('SIGTERM').slice() as ((...args: unknown[]) => void)[];

    try {
      // Simulate SIGTERM — shutdown handler calls input.destroy() which breaks
      // the for-await loop; the finally block should call server.stop().
      process.emit('SIGTERM');
      await mainSettled;
    } finally {
      // Remove any SIGTERM listeners that main() may have left registered.
      const added = (process.rawListeners('SIGTERM') as ((...args: unknown[]) => void)[])
        .filter((fn) => !sigtermBefore.includes(fn));
      for (const fn of added) {
        process.removeListener('SIGTERM', fn);
      }
    }

    expect(serverMock.stop).toHaveBeenCalledOnce();
  });
});

describe('main() workspace + landlock env propagation (task 3210)', () => {
  let serverMock: ReturnType<typeof makePermissionServerMock>;

  beforeEach(() => {
    serverMock = makePermissionServerMock();
    vi.mocked(createPermissionServer).mockReturnValue(serverMock);
    vi.mocked(spawn).mockReset();
    vi.mocked(spawn).mockImplementation((() => makeIdleProcess()) as any);
    vi.mocked(wrapClaudeArgs).mockReset();
    vi.mocked(wrapClaudeArgs).mockImplementation((args, _ws, le) =>
      le ? { cmd: 'python3', args: [le, ...args] } : { cmd: 'claude', args: [...args] }
    );
    // probeLandlockAsync is the startup probe (task 3281); default to false (no sandbox).
    vi.mocked(probeLandlockAsync).mockReset();
    vi.mocked(probeLandlockAsync).mockResolvedValue(false);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /**
   * Trigger invokeSdk by sending a send_message, then await a promise that resolves
   * exactly when spawn() is called — no interval polling, no 2 s ceiling.
   *
   * wrapClaudeArgs is called before spawn in session.ts, so by the time this promise
   * settles, wrapClaudeArgs.mock.calls[0] is already populated.
   */
  async function triggerInvokeSdk(input: PassThrough): Promise<void> {
    let resolveSpawnCalled!: () => void;
    const spawnCalledPromise = new Promise<void>((resolve) => {
      resolveSpawnCalled = resolve;
    });
    vi.mocked(spawn).mockImplementation((() => {
      resolveSpawnCalled();
      return makeIdleProcess();
    }) as any);
    input.write(JSON.stringify({ type: 'send_message', id: 'msg-ws', text: 'hello' }) + '\n');
    await spawnCalledPromise;
  }

  // (a) REIFY_WORKSPACE propagates to wrapClaudeArgs workspace arg
  it('(a) REIFY_WORKSPACE env var → wrapClaudeArgs workspace arg matches', async () => {
    const origWs = process.env.REIFY_WORKSPACE;
    process.env.REIFY_WORKSPACE = '/foo/bar';
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');

      await triggerInvokeSdk(input);

      // wrapClaudeArgs should have been called with workspace = '/foo/bar'
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const wsArg = vi.mocked(wrapClaudeArgs).mock.calls[0][1];
      expect(wsArg).toBe('/foo/bar');

      input.end();
      await mainPromise;
    } finally {
      if (origWs === undefined) delete process.env.REIFY_WORKSPACE;
      else process.env.REIFY_WORKSPACE = origWs;
    }
  });

  // (b) REIFY_LANDLOCK_EXEC propagates — when async probe succeeds, landlockExec reaches wrapClaudeArgs
  it('(b) REIFY_LANDLOCK_EXEC env var → landlockExec passed to wrapClaudeArgs when probe succeeds', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/landlock_exec.py';
    // Simulate async probe succeeding so landlockAvailable=true is forwarded to the session
    vi.mocked(probeLandlockAsync).mockResolvedValue(true);
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');

      await triggerInvokeSdk(input);

      // wrapClaudeArgs should have been called with landlockExec = '/sb/landlock_exec.py'
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const leArg = vi.mocked(wrapClaudeArgs).mock.calls[0][2];
      expect(leArg).toBe('/sb/landlock_exec.py');

      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (c) REIFY_WORKSPACE unset → workspace defaults to process.cwd()
  it('(c) REIFY_WORKSPACE unset → wrapClaudeArgs workspace arg equals process.cwd()', async () => {
    const origWs = process.env.REIFY_WORKSPACE;
    delete process.env.REIFY_WORKSPACE;
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');

      await triggerInvokeSdk(input);

      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const wsArg = vi.mocked(wrapClaudeArgs).mock.calls[0][1];
      expect(wsArg).toBe(process.cwd());

      input.end();
      await mainPromise;
    } finally {
      if (origWs === undefined) delete process.env.REIFY_WORKSPACE;
      else process.env.REIFY_WORKSPACE = origWs;
    }
  });

  // (d) REIFY_LANDLOCK_EXEC unset → landlockExec is undefined in wrapClaudeArgs call
  it('(d) REIFY_LANDLOCK_EXEC unset → wrapClaudeArgs called with undefined landlockExec', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    delete process.env.REIFY_LANDLOCK_EXEC;
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');

      await triggerInvokeSdk(input);

      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const leArg = vi.mocked(wrapClaudeArgs).mock.calls[0][2];
      expect(leArg).toBeUndefined();

      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (e) workspace is pinned at startup — mutating REIFY_WORKSPACE after main() begins
  //     does NOT change the workspace passed to subsequent wrapClaudeArgs calls.
  it('(e) workspace is pinned at main() startup — env mutation after ready has no effect', async () => {
    const origWs = process.env.REIFY_WORKSPACE;
    process.env.REIFY_WORKSPACE = '/pinned/workspace';
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');

      // Mutate the env var AFTER main() has captured its value
      process.env.REIFY_WORKSPACE = '/mutated/workspace';

      await triggerInvokeSdk(input);

      // wrapClaudeArgs should have been called with the ORIGINAL workspace,
      // not the mutated one — index.ts reads env once at startup.
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const wsArg = vi.mocked(wrapClaudeArgs).mock.calls[0][1];
      expect(wsArg).toBe('/pinned/workspace');

      input.end();
      await mainPromise;
    } finally {
      if (origWs === undefined) delete process.env.REIFY_WORKSPACE;
      else process.env.REIFY_WORKSPACE = origWs;
    }
  });
});

describe('main() startup landlock probe (task 3281)', () => {
  let serverMock: ReturnType<typeof makePermissionServerMock>;

  /**
   * Trigger invokeSdk by sending a send_message, then await spawn() being called.
   * wrapClaudeArgs is called before spawn, so its mock.calls[0] is populated by then.
   */
  async function triggerInvokeSdk(input: PassThrough): Promise<void> {
    let resolveSpawnCalled!: () => void;
    const spawnCalledPromise = new Promise<void>((resolve) => {
      resolveSpawnCalled = resolve;
    });
    vi.mocked(spawn).mockImplementation((() => {
      resolveSpawnCalled();
      return makeIdleProcess();
    }) as any);
    input.write(JSON.stringify({ type: 'send_message', id: 'msg-probe', text: 'hello' }) + '\n');
    await spawnCalledPromise;
  }

  beforeEach(() => {
    serverMock = makePermissionServerMock();
    vi.mocked(createPermissionServer).mockReturnValue(serverMock);
    vi.mocked(spawn).mockReset();
    vi.mocked(spawn).mockImplementation((() => makeIdleProcess()) as any);
    vi.mocked(wrapClaudeArgs).mockReset();
    vi.mocked(wrapClaudeArgs).mockImplementation((args, _ws, le) =>
      le ? { cmd: 'python3', args: [le, ...args] } : { cmd: 'claude', args: [...args] }
    );
    vi.mocked(probeLandlockAsync).mockReset();
    vi.mocked(probeLandlockAsync).mockResolvedValue(false);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // (a) REIFY_LANDLOCK_EXEC set → probeLandlockAsync called once with that path before ready
  it('(a) REIFY_LANDLOCK_EXEC=/sb/le.py → probeLandlockAsync called once with /sb/le.py before ready', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/le.py';
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      // ready is emitted by session.init(), which runs AFTER Promise.all([start, probe]) resolves.
      // By the time ready is observed, probe must have been called and its result resolved.
      await waitForMessage(output, (m) => m.type === 'ready');
      expect(vi.mocked(probeLandlockAsync)).toHaveBeenCalledOnce();
      expect(vi.mocked(probeLandlockAsync)).toHaveBeenCalledWith('/sb/le.py');
      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (b) REIFY_LANDLOCK_EXEC unset → probeLandlockAsync is NOT called
  it('(b) REIFY_LANDLOCK_EXEC unset → probeLandlockAsync is not called', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    delete process.env.REIFY_LANDLOCK_EXEC;
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');
      expect(vi.mocked(probeLandlockAsync)).not.toHaveBeenCalled();
      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (c) probe resolves true → wrapClaudeArgs receives non-undefined landlockExec
  it('(c) probe resolves true → wrapClaudeArgs receives landlockExec on first invokeSdk', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/le.py';
    vi.mocked(probeLandlockAsync).mockResolvedValue(true);
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');
      await triggerInvokeSdk(input);
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const leArg = vi.mocked(wrapClaudeArgs).mock.calls[0][2];
      expect(leArg).toBe('/sb/le.py');
      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (d) probe resolves false → wrapClaudeArgs receives undefined landlockExec
  it('(d) probe resolves false → wrapClaudeArgs receives undefined landlockExec on first invokeSdk', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/le.py';
    vi.mocked(probeLandlockAsync).mockResolvedValue(false);
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      await waitForMessage(output, (m) => m.type === 'ready');
      await triggerInvokeSdk(input);
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      const leArg = vi.mocked(wrapClaudeArgs).mock.calls[0][2];
      expect(leArg).toBeUndefined();
      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (f) probeLandlockAsync rejection (impossible per contract) → main() resolves normally with
  // landlockAvailable=false (regression guard for dead-branch removal).
  // probeLandlockAsync's contract (sandbox.ts) guarantees no rejection — every error path resolves
  // to false. If the contract is ever violated by a future regression, main() must fall back to
  // landlockAvailable=false (not emit an error). This test pins that invariant.
  it('(f) probeLandlockAsync rejects (contract violation) → main() emits ready with landlockAvailable=false', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/le.py';
    vi.mocked(probeLandlockAsync).mockRejectedValue(new Error('unexpected probe failure'));
    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);
      // main() must emit 'ready' (not 'error') even when probe rejects
      await waitForMessage(output, (m) => m.type === 'ready');
      // Trigger an invokeSdk to inspect what wrapClaudeArgs received
      await triggerInvokeSdk(input);
      expect(vi.mocked(wrapClaudeArgs)).toHaveBeenCalled();
      // Safe-default: landlockAvailable=false → landlockExec arg must be undefined
      const leArg = vi.mocked(wrapClaudeArgs).mock.calls[0][2];
      expect(leArg).toBeUndefined();
      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });

  // (e) probe and permissionServer.start() run concurrently (both in-flight before ready)
  it('(e) probe and permissionServer.start() are both in-flight simultaneously before ready', async () => {
    const origLe = process.env.REIFY_LANDLOCK_EXEC;
    process.env.REIFY_LANDLOCK_EXEC = '/sb/le.py';

    const callOrder: string[] = [];

    // Make start async with a yield so the event loop can interleave
    vi.mocked(serverMock.start).mockImplementation(async () => {
      callOrder.push('start-begin');
      await new Promise<void>((r) => setImmediate(r));
      callOrder.push('start-end');
    });

    vi.mocked(probeLandlockAsync).mockImplementation(async () => {
      callOrder.push('probe-begin');
      await new Promise<void>((r) => setImmediate(r));
      callOrder.push('probe-end');
      return false;
    });

    try {
      const input = new PassThrough();
      const output = new PassThrough();
      const mainPromise = main(input, output);

      await waitForMessage(output, (m) => m.type === 'ready');

      // Both must have been called and completed before ready
      expect(callOrder).toContain('start-begin');
      expect(callOrder).toContain('probe-begin');
      expect(callOrder).toContain('start-end');
      expect(callOrder).toContain('probe-end');

      // Concurrency assertion: probe-begin appears BEFORE start-end.
      // Sequential order would be: [start-begin, start-end, probe-begin, probe-end].
      // Concurrent (Promise.all) order: [start-begin, probe-begin, ..., start-end, probe-end].
      const probeBeginIdx = callOrder.indexOf('probe-begin');
      const startEndIdx = callOrder.indexOf('start-end');
      expect(probeBeginIdx).toBeLessThan(startEndIdx);

      input.end();
      await mainPromise;
    } finally {
      if (origLe === undefined) delete process.env.REIFY_LANDLOCK_EXEC;
      else process.env.REIFY_LANDLOCK_EXEC = origLe;
    }
  });
});
