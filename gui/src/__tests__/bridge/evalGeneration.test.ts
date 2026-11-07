/**
 * Bridge tests for the eval-generation Tauri event channel.
 *
 * Per `docs/gui-event-channels/eval-generation.md` (task 7853). Exercises the
 * `onEvalGeneration` inline shape-guard idiom in bridge.ts: happy-path delivery
 * + three malformed-payload cases.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts —
// matches the established pattern in convention_smoke.test.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onEvalGeneration } from '../../bridge';
import type { EvalGeneration } from '../../types';

describe('eval-generation bridge (task 7853)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) happy-path: callback fires once with the announced generation', async () => {
    const handle = mockTauriEvent<EvalGeneration>('eval-generation');
    const cb = vi.fn();

    await onEvalGeneration(cb);
    handle.emit({ generation: 42 });

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith(42);
  });

  it.each([
    ['a non-object payload', 42],
    ['a missing generation', {}],
    ['a string generation', { generation: '42' }],
  ])('drops %s without calling back, warning about eval-generation', async (_label, payload) => {
    const handle = mockTauriEvent<unknown>('eval-generation');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onEvalGeneration(cb);
    handle.emit(payload);

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('eval-generation');
  });
});
