import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import * as os from 'node:os';
import { EventEmitter } from 'node:events';

// Mock node:child_process — spawn is used by probeLandlockAsync and wrapClaudeArgs tests.
// spawnSync is no longer used (deleted in task 3281 cleanup step).
vi.mock('node:child_process', () => ({
  spawn: vi.fn(),
}));

import { spawn } from 'node:child_process';
import { wrapClaudeArgs, probeLandlockAsync } from '../sandbox.js';

describe('sandbox helpers (task 3210)', () => {
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
