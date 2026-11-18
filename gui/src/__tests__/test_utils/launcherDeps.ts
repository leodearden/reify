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

/**
 * LauncherDeps whose every dep succeeds: loads resolve, a created file comes
 * back as /designs/created.ri, Recent is stubRecent(), and set_mode reports a
 * debug port only for Debug. Each call returns fresh mocks to reconfigure.
 */
export function makeLauncherDeps() {
  return {
    loadDocument: vi.fn<LauncherDeps['loadDocument']>().mockResolvedValue(undefined),
    createNewFile: vi
      .fn<LauncherDeps['createNewFile']>()
      .mockResolvedValue({ path: '/designs/created.ri', content: '// New design\n' }),
    recordRecent: vi.fn<LauncherDeps['recordRecent']>().mockImplementation(async () => stubRecent()),
    listRecent: vi.fn<LauncherDeps['listRecent']>().mockImplementation(async () => stubRecent()),
    setMode: vi
      .fn<LauncherDeps['setMode']>()
      .mockImplementation(async (mode) => ({ mode, debug_port: mode === 'debug' ? 3939 : null })),
    showAssistant: vi.fn<LauncherDeps['showAssistant']>(),
    ensureDebugBridge: vi.fn<LauncherDeps['ensureDebugBridge']>().mockResolvedValue(undefined),
    notify: vi.fn<LauncherDeps['notify']>(),
  };
}
