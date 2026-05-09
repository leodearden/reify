import { spawnSync } from 'node:child_process';
import * as path from 'node:path';
import * as os from 'node:os';

/** Module-level cache for the landlock availability probe result. */
let cached: boolean | undefined;

/**
 * Probe whether the Landlock FS sandbox is available on this kernel.
 *
 * When `landlockHelperPath` is undefined, returns false without spawning anything.
 * Otherwise runs `python3 -c "..."` to invoke the vendored `landlock.is_landlock_available()`.
 * Result is cached for the lifetime of the process; call `_resetLandlockCache()` to re-probe.
 */
export function isLandlockAvailable(landlockHelperPath?: string): boolean {
  // When no path is provided, return false immediately WITHOUT writing to `cached`.
  // Poisoning the cache here would silently short-circuit every subsequent call —
  // even a real call with a helper path — returning false forever until reset.
  if (!landlockHelperPath) {
    return false;
  }
  if (cached !== undefined) {
    return cached;
  }

  const helperDir = path.dirname(landlockHelperPath);
  const probeCode = [
    'import sys',
    `sys.path.insert(0, ${JSON.stringify(helperDir)})`,
    'from landlock import is_landlock_available',
    'sys.exit(0 if is_landlock_available() else 1)',
  ].join('; ');

  try {
    // timeout:2000 ensures a hung python3 (broken interpreter, slow FS) fails fast
    // rather than blocking session startup indefinitely on the first invokeSdk call.
    // Treat a non-zero exit OR a signal (timeout/kill) as unavailable.
    const result = spawnSync('python3', ['-c', probeCode], { stdio: 'pipe', timeout: 2000 });
    cached = result.status === 0 && result.signal == null;
  } catch {
    cached = false;
  }
  return cached;
}

/**
 * Reset the cached landlock probe result. For testing only.
 */
export function _resetLandlockCache(): void {
  cached = undefined;
}

/**
 * Build the effective spawn command and args, optionally wrapping with landlock.
 *
 * When `landlockExec` is truthy, prepends `python3 <landlockExec> --writable <workspace>
 * --writable ~/.claude --writable /tmp --` before `claude <claudeArgs>`.
 * Otherwise returns `{cmd:'claude', args:[...claudeArgs]}` directly.
 */
export function wrapClaudeArgs(
  claudeArgs: string[],
  workspace: string,
  landlockExec?: string,
): { cmd: string; args: string[] } {
  if (landlockExec) {
    return {
      cmd: 'python3',
      args: [
        landlockExec,
        '--writable', workspace,
        '--writable', os.homedir() + '/.claude',
        '--writable', '/tmp',
        '--',
        'claude',
        ...claudeArgs,
      ],
    };
  }
  return { cmd: 'claude', args: [...claudeArgs] };
}
