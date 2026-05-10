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
    // Security-critical idempotency: a late 'close' event with code 0 arriving
    // AFTER the watchdog has already killed the proc and settled the promise to
    // false must NOT flip the result to true. The settled guard prevents this.
    fakeProc.emit('close', 0, null);
    expect(await promise).toBe(false);
  });

  it("(vii) does not crash when proc.kill('SIGTERM') throws (e.g. ESRCH for already-reaped pid); promise still resolves false", async () => {
    vi.useFakeTimers();
    fakeProc.kill.mockImplementation((sig: string) => {
      if (sig === 'SIGTERM') throw Object.assign(new Error('kill ESRCH'), { code: 'ESRCH' });
    });
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    vi.advanceTimersByTime(2001);
    expect(await promise).toBe(false);
    // The watchdog reached the kill site and the throw was swallowed
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGTERM');
  });

  it("(viii) escalates to SIGKILL ~500ms after watchdog SIGTERM if proc has not closed", async () => {
    vi.useFakeTimers();
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    // Fire the 2000ms watchdog
    vi.advanceTimersByTime(2001);
    expect(await promise).toBe(false);
    // SIGTERM sent, but SIGKILL not yet
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGTERM');
    expect(fakeProc.kill).not.toHaveBeenCalledWith('SIGKILL');
    // Advance the 500ms escalation window
    vi.advanceTimersByTime(500);
    // Now SIGKILL must have been sent
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGKILL');
  });

  it("(ix) cancels SIGKILL escalation when 'close' fires after SIGTERM but before the 500ms grace expires", async () => {
    vi.useFakeTimers();
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    // Fire the 2000ms watchdog
    vi.advanceTimersByTime(2001);
    expect(await promise).toBe(false);
    // SIGTERM sent; escalation timer is now pending
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGTERM');
    // Process responded to SIGTERM — emit 'close' within the grace period
    fakeProc.emit('close', null, 'SIGTERM');
    // Advance past the 500ms escalation window
    vi.advanceTimersByTime(500);
    // SIGKILL must NOT have been sent — escalation was cancelled
    expect(fakeProc.kill).not.toHaveBeenCalledWith('SIGKILL');
  });

  it("(x) cancels SIGKILL escalation when 'error' fires after SIGTERM but before the 500ms grace expires", async () => {
    vi.useFakeTimers();
    const promise = probeLandlockAsync('/path/to/landlock_exec.py');
    // Fire the 2000ms watchdog
    vi.advanceTimersByTime(2001);
    expect(await promise).toBe(false);
    // SIGTERM sent; escalation timer is now pending
    expect(fakeProc.kill).toHaveBeenCalledWith('SIGTERM');
    // Asynchronous error surfaces after the watchdog (e.g. spawn failure post-watchdog)
    const err = Object.assign(new Error('spawn python3 ENOENT'), { code: 'ENOENT' });
    fakeProc.emit('error', err);
    // Advance past the 500ms escalation window
    vi.advanceTimersByTime(500);
    // SIGKILL must NOT have been sent — escalation was cancelled by the error handler
    expect(fakeProc.kill).not.toHaveBeenCalledWith('SIGKILL');
  });
});
