import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as os from 'node:os';

// Mock node:child_process.spawnSync before importing sandbox
vi.mock('node:child_process', () => ({
  spawnSync: vi.fn(),
}));

import { spawnSync } from 'node:child_process';
import { isLandlockAvailable, _resetLandlockCache, wrapClaudeArgs } from '../sandbox.js';

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
