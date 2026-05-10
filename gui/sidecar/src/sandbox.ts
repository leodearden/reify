import { spawn } from 'node:child_process';
import * as path from 'node:path';
import * as os from 'node:os';

/**
 * Asynchronously probe whether the Landlock FS sandbox is available on this kernel.
 *
 * When `landlockHelperPath` is falsy, returns `false` immediately without spawning.
 * Otherwise runs `python3 -c "..."` using `spawn()` (never blocks the caller).
 * `stdio: 'ignore'` is used because the probe only cares about exit status — no pipes
 * are created. A 2000ms watchdog sends SIGTERM and resolves `false` if python3 hangs.
 *
 * Resolution rules (first event wins; subsequent events are ignored):
 * - `'close'` with `code === 0` and `signal === null` → `true`
 * - `'close'` with any other code/signal → `false`
 * - `'error'` event → `false`
 * - 2000ms timeout → `false` (watchdog sends SIGTERM before resolving)
 *
 * Kill calls (SIGTERM and SIGKILL) are guarded with try/catch: Node's ChildProcess.kill
 * can throw synchronously (e.g. EPERM, ESRCH) if libuv has already reaped the pid.
 *
 * SIGKILL escalation: if the proc ignores SIGTERM, a second timer fires 500ms later
 * and sends SIGKILL. The escalation timer is cleared if 'close' or 'error' fires first.
 * The promise resolves `false` at the 2000ms watchdog — escalation is best-effort
 * cleanup that happens after the promise has already settled.
 *
 * Invariant: this Promise never rejects; all error paths resolve to `false`.
 */
export async function probeLandlockAsync(landlockHelperPath?: string): Promise<boolean> {
  if (!landlockHelperPath) {
    return false;
  }

  const helperDir = path.dirname(landlockHelperPath);
  const probeCode = [
    'import sys',
    `sys.path.insert(0, ${JSON.stringify(helperDir)})`,
    'from landlock import is_landlock_available',
    'sys.exit(0 if is_landlock_available() else 1)',
  ].join('; ');

  return new Promise<boolean>((resolve) => {
    let settled = false;
    let killEscalation: ReturnType<typeof setTimeout> | null = null;

    const settle = (result: boolean): void => {
      if (!settled) {
        settled = true;
        clearTimeout(watchdog);
        resolve(result);
      }
    };

    const proc = spawn('python3', ['-c', probeCode], { stdio: 'ignore' });

    // Watchdog: if python3 hangs beyond 2000ms, kill it and resolve false.
    const watchdog = setTimeout(() => {
      try { proc.kill('SIGTERM'); } catch { /* proc may have been reaped already; ignore. */ }
      settle(false);
      // Escalation: if the proc ignores SIGTERM, send SIGKILL after 500ms.
      killEscalation = setTimeout(() => {
        try { proc.kill('SIGKILL'); } catch { /* proc may have been reaped already; ignore. */ }
      }, 500);
    }, 2000);

    proc.on('close', (code: number | null, signal: string | null) => {
      if (killEscalation !== null) clearTimeout(killEscalation);
      settle(code === 0 && signal === null);
    });

    proc.on('error', (_err: Error) => {
      if (killEscalation !== null) clearTimeout(killEscalation);
      settle(false);
    });
  });
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
