/**
 * Mock-emitter test utility per `docs/prds/v0_3/gui-event-channel-inventory.md` §6.3.
 *
 * Caller must declare `vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))`
 * at module scope in their test file before importing this helper. This module
 * lazily wires `vi.mocked(listen).mockImplementation(...)` to route registrations
 * into a per-channel handler registry, so multiple test files can share the helper
 * without coordinating mock setup.
 */
import { vi } from 'vitest';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

type EventHandler<T> = (event: { payload: T }) => void;

const handlersByChannel = new Map<string, Set<EventHandler<unknown>>>();
let installed = false;

function ensureMockInstalled(): void {
  if (installed) return;
  vi.mocked(listen).mockImplementation(async (channel: string, handler) => {
    if (!handlersByChannel.has(channel)) handlersByChannel.set(channel, new Set());
    handlersByChannel.get(channel)!.add(handler as EventHandler<unknown>);
    const unlisten: UnlistenFn = () => {
      handlersByChannel.get(channel)?.delete(handler as EventHandler<unknown>);
    };
    return unlisten;
  });
  installed = true;
}

export function mockTauriEvent<T>(channel: string): {
  emit: (payload: T) => void;
  reset: () => void;
} {
  ensureMockInstalled();
  return {
    emit(payload: T) {
      const set = handlersByChannel.get(channel);
      if (!set) return;
      for (const h of set) h({ payload });
    },
    reset() {
      handlersByChannel.delete(channel);
    },
  };
}

/**
 * Clear all registered handlers and reset the mock installation state.
 * Useful in beforeEach hooks when cross-test isolation is needed.
 */
export function clearAllMockEvents(): void {
  handlersByChannel.clear();
  installed = false;
}
