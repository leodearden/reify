import { vi } from 'vitest';
import type { LauncherDeps } from '../../app/launcherActions';
import type { RecentEntry } from '../../types';

/** A fresh copy of the Recent list every stubbed recordRecent and listRecent resolves to. */
export function stubRecent(): RecentEntry[] {
  return [
    { path: '/designs/bracket.ri', opened_at: 2000 },
    { path: '/designs/flange.ri', opened_at: 1000 },
  ];
}

/** A set_mode with its own unbound listener, replying as launch_mode.rs::set_mode_impl does. */
export function fakeSetMode(): LauncherDeps['setMode'] {
  let livePort: number | null = null;
  return async (mode) => {
    if (mode === 'debug') livePort = 3939;
    return { mode, debug_port: livePort };
  };
}

/**
 * LauncherDeps whose every dep succeeds: loads resolve, a created file comes
 * back as /designs/created.ri, Recent is stubRecent(), and set_mode behaves
 * like fakeSetMode(): once a Debug call binds the listener at 3939, every
 * reply carries that port. Each call returns fresh mocks to reconfigure.
 */
export function makeLauncherDeps() {
  return {
    loadDocument: vi.fn<LauncherDeps['loadDocument']>().mockResolvedValue(undefined),
    createNewFile: vi
      .fn<LauncherDeps['createNewFile']>()
      .mockResolvedValue({ path: '/designs/created.ri', content: '// New design\n' }),
    recordRecent: vi.fn<LauncherDeps['recordRecent']>().mockImplementation(async () => stubRecent()),
    listRecent: vi.fn<LauncherDeps['listRecent']>().mockImplementation(async () => stubRecent()),
    setMode: vi.fn<LauncherDeps['setMode']>().mockImplementation(fakeSetMode()),
    showAssistant: vi.fn<LauncherDeps['showAssistant']>(),
    ensureDebugBridge: vi.fn<LauncherDeps['ensureDebugBridge']>().mockResolvedValue(undefined),
    notify: vi.fn<LauncherDeps['notify']>(),
  };
}
