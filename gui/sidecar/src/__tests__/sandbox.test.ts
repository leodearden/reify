import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import * as os from 'node:os';
import { EventEmitter } from 'node:events';

// Mock node:child_process — exposes both spawnSync (for isLandlockAvailable tests)
// and spawn (for probeLandlockAsync tests).
vi.mock('node:child_process', () => ({
  spawnSync: vi.fn(),
  spawn: vi.fn(),
}));

import { spawnSync, spawn } from 'node:child_process';
import { isLandlockAvailable, _resetLandlockCache, wrapClaudeArgs, probeLandlockAsync } from '../sandbox.js';

describe('sandbox helpers (task 3210)', () => {
  beforeEach(() => {
    // Reset the module-level cache before each test
    _resetLandlockCache();
    vi.mocked(spawnSync).mockReset();
  });

  describe('isLandlockAvailable', () => {
    it('(a) returns true when spawnSync exits with status 0', () => {
      vi.mocked(spawnSync).mockReturnValue({ status: 0, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: null, error: undefined });
      expect(isLandlockAvailable('/path/to/landlock_exec.py')).toBe(true);
    });

    it('(b) returns false when spawnSync exits with status !== 0', () => {
      vi.mocked(spawnSync).mockReturnValue({ status: 1, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: null, error: undefined });
      expect(isLandlockAvailable('/path/to/landlock_exec.py')).toBe(false);
    });

    it('(c) returns false when spawnSync throws (python3 missing)', () => {
      vi.mocked(spawnSync).mockImplementation(() => {
        const err = new Error('spawn python3 ENOENT') as Error & { code: string };
        err.code = 'ENOENT';
        throw err;
      });
      expect(isLandlockAvailable('/path/to/landlock_exec.py')).toBe(false);
    });

    it('(d) caches result — second call with same arg does not re-invoke spawnSync', () => {
      vi.mocked(spawnSync).mockReturnValue({ status: 0, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: null, error: undefined });
      isLandlockAvailable('/path/to/landlock_exec.py');
      isLandlockAvailable('/path/to/landlock_exec.py');
      expect(vi.mocked(spawnSync)).toHaveBeenCalledTimes(1);
    });

    it('(e) _resetLandlockCache() re-enables probing', () => {
      vi.mocked(spawnSync).mockReturnValue({ status: 0, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: null, error: undefined });
      isLandlockAvailable('/path/to/landlock_exec.py');
      _resetLandlockCache();
      isLandlockAvailable('/path/to/landlock_exec.py');
      expect(vi.mocked(spawnSync)).toHaveBeenCalledTimes(2);
    });

    it('(f) isLandlockAvailable() (no arg) returns false without invoking spawnSync', () => {
      expect(isLandlockAvailable()).toBe(false);
      expect(vi.mocked(spawnSync)).not.toHaveBeenCalled();
    });

    it('(f2) no-arg call does NOT poison the cache — subsequent call with real path still probes', () => {
      // A no-arg call returns false without setting `cached`, so the next call with a real
      // helper path must still run spawnSync rather than short-circuiting to false.
      vi.mocked(spawnSync).mockReturnValue({ status: 0, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: null, error: undefined });
      expect(isLandlockAvailable()).toBe(false);             // no-arg: no probe, no cache write
      expect(vi.mocked(spawnSync)).not.toHaveBeenCalled();  // probe hasn't run yet
      expect(isLandlockAvailable('/path/x')).toBe(true);    // real path: probe runs
      expect(vi.mocked(spawnSync)).toHaveBeenCalledTimes(1); // probe was invoked
    });

    it('(f3) spawnSync timeout/signal result treated as unavailable', () => {
      // Simulate python3 being killed by SIGTERM (e.g. 2 s timeout expired)
      vi.mocked(spawnSync).mockReturnValue({ status: null, pid: 1, output: [], stdout: Buffer.from(''), stderr: Buffer.from(''), signal: 'SIGTERM', error: undefined });
      expect(isLandlockAvailable('/path/to/landlock_exec.py')).toBe(false);
    });
  });

  describe('wrapClaudeArgs', () => {
    it('(g) wraps with landlock when landlockExec provided', () => {
      const claudeArgs = ['--print', '--verbose'];
      const workspace = '/tmp/ws';
      const landlockExec = '/path/landlock_exec.py';
      const result = wrapClaudeArgs(claudeArgs, workspace, landlockExec);
      expect(result.cmd).toBe('python3');
      expect(result.args).toEqual([
        landlockExec,
        '--writable', workspace,
        '--writable', os.homedir() + '/.claude',
        '--writable', '/tmp',
        '--',
        'claude',
        '--print',
        '--verbose',
      ]);
    });

    it('(h) no wrap when landlockExec is undefined', () => {
      const claudeArgs = ['--print', '--verbose'];
      const workspace = '/tmp/ws';
      const result = wrapClaudeArgs(claudeArgs, workspace, undefined);
      expect(result.cmd).toBe('claude');
      expect(result.args).toEqual(['--print', '--verbose']);
    });
  });
});

describe('probeLandlockAsync (task 3281)', () => {
  type FakeProc = EventEmitter & { kill: ReturnType<typeof vi.fn>; exitCode: number | null; signalCode: string | null };

  let fakeProc: FakeProc;

  beforeEach(() => {
    vi.mocked(spawn).mockReset();
    fakeProc = Object.assign(new EventEmitter(), {
      kill: vi.fn(),
      exitCode: null,
      signalCode: null,
    }) as FakeProc;
    vi.mocked(spawn).mockReturnValue(fakeProc as any);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('(i) returns true when spawn closes with code 0 and no signal', async () => {
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    fakeProc.emit('close', 0, null);
    expect(await promise).toBe(true);
  });

  it('(ii) returns false when spawn closes with non-zero exit code', async () => {
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    fakeProc.emit('close', 1, null);
    expect(await promise).toBe(false);
  });

  it('(iii) returns false when spawn closes with a signal (SIGTERM)', async () => {
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    fakeProc.emit('close', null, 'SIGTERM');
    expect(await promise).toBe(false);
  });

  it('(iv) returns false when spawn emits an error event (e.g. ENOENT)', async () => {
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    const err = Object.assign(new Error('spawn python3 ENOENT'), { code: 'ENOENT' });
    fakeProc.emit('error', err);
    expect(await promise).toBe(false);
  });

  it('(iv-idempotent) close after error does not change the already-settled false result', async () => {
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    const err = Object.assign(new Error('spawn python3 ENOENT'), { code: 'ENOENT' });
    fakeProc.emit('error', err);
    await promise;  // already settled false
    // Emit close with code 0 — should not change the settled result
    fakeProc.emit('close', 0, null);
    expect(await promise).toBe(false);
  });

  it('(v-undefined) returns false immediately without invoking spawn when landlockHelperPath is undefined', async () => {
    expect(await probeLandlockAsync(undefined)).toBe(false);
    expect(vi.mocked(spawn)).not.toHaveBeenCalled();
  });

  it('(v-empty) returns false immediately without invoking spawn when landlockHelperPath is empty string', async () => {
    expect(await probeLandlockAsync('')).toBe(false);
    expect(vi.mocked(spawn)).not.toHaveBeenCalled();
  });

  it('(vi) returns false when the watchdog timeout fires (2000ms) — uses fake timers', async () => {
    vi.useFakeTimers();
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    // Advance time to fire the 2000ms watchdog
    vi.advanceTimersByTime(2001);
    expect(await promise).toBe(false);
    // The watchdog must have sent SIGTERM to kill the hanging process
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGTERM');
  });
});
