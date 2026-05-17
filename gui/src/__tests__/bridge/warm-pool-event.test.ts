/**
 * §6.2 shape test for the `warm-pool-event` Tauri channel (GR-016 ε).
 *
 * Exercises `onWarmPoolEvent` from `bridge.ts` via the `mockTauriEvent`
 * utility (PRD §6.3), confirming:
 *  - valid payloads are forwarded to the callback
 *  - malformed payloads are dropped with `console.warn`
 *  - the returned unlisten function stops further delivery
 *
 * Pattern mirrors `convention_smoke.test.ts` and `mesh_update.test.ts`.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before imports from mockEvents.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onWarmPoolEvent } from '../../bridge';
import type { WarmPoolEvent } from '../../types';

describe('onWarmPoolEvent (GR-016 ε)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) callback fires with a valid evicted payload', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    const cb = vi.fn();

    await onWarmPoolEvent(cb);

    const payload: WarmPoolEvent = { kind: 'evicted', size_bytes: 1024, node_id: 'Body.thickness' };
    handle.emit(payload);

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith(payload);
  });

  it('(b) callback fires for kind="donated" too', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    const cb = vi.fn();

    await onWarmPoolEvent(cb);

    const payload: WarmPoolEvent = { kind: 'donated', size_bytes: 4096, node_id: 'Plate.width' };
    handle.emit(payload);

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith(payload);
  });

  it('(c) malformed payload — missing size_bytes — is dropped with console.warn', async () => {
    const handle = mockTauriEvent<unknown>('warm-pool-event');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onWarmPoolEvent(cb);

    // Missing size_bytes field
    handle.emit({ kind: 'evicted', node_id: 'Body.thickness' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('warm-pool-event');
  });

  it('(c) malformed payload — size_bytes wrong type — is dropped with console.warn', async () => {
    const handle = mockTauriEvent<unknown>('warm-pool-event');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onWarmPoolEvent(cb);

    handle.emit({ kind: 'evicted', size_bytes: 'not-a-number', node_id: 'Body.thickness' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('(c) malformed payload — kind="banana" — is dropped with console.warn', async () => {
    const handle = mockTauriEvent<unknown>('warm-pool-event');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onWarmPoolEvent(cb);

    handle.emit({ kind: 'banana', size_bytes: 512, node_id: 'Body.thickness' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('(d) unlisten removes the handler so subsequent emits are no-ops', async () => {
    const handle = mockTauriEvent<WarmPoolEvent>('warm-pool-event');
    const cb = vi.fn();

    const unlisten = await onWarmPoolEvent(cb);

    // First emit is delivered
    handle.emit({ kind: 'donated', size_bytes: 256, node_id: 'A.b' });
    expect(cb).toHaveBeenCalledTimes(1);

    // After unlistening, no further delivery
    unlisten();
    handle.emit({ kind: 'donated', size_bytes: 512, node_id: 'A.c' });
    expect(cb).toHaveBeenCalledTimes(1);
  });
});
