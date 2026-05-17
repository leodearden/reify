/**
 * Bridge tests for the fea-case-changed Tauri event channel.
 *
 * Per `docs/gui-event-channels/fea-case-changed.md` and PRD §2.2 task η.
 * Exercises the `onFeaCaseChanged` inline shape-guard idiom in bridge.ts:
 * happy-path delivery + three malformed-payload cases.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts —
// matches the established pattern in convention_smoke.test.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onFeaCaseChanged } from '../../bridge';
import type { FeaCaseChanged } from '../../types';

describe('fea-case-changed bridge (GR-016 η)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) happy-path: callback fires once with correctly-typed payload', async () => {
    const handle = mockTauriEvent<FeaCaseChanged>('fea-case-changed');
    const cb = vi.fn();

    await onFeaCaseChanged(cb);
    handle.emit({ active_case_id: 'A', available_cases: ['A', 'B'] });

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith({ active_case_id: 'A', available_cases: ['A', 'B'] });
  });

  it('(b) malformed: missing available_cases — callback NOT invoked, console.warn mentions fea-case-changed', async () => {
    const handle = mockTauriEvent<unknown>('fea-case-changed');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onFeaCaseChanged(cb);
    handle.emit({ active_case_id: 'A' }); // missing available_cases

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('fea-case-changed');
  });

  it('(c) malformed: available_cases is not an array — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('fea-case-changed');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onFeaCaseChanged(cb);
    handle.emit({ active_case_id: 'A', available_cases: 'not-an-array' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('fea-case-changed');
  });

  it('(d) malformed: active_case_id is not a string — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('fea-case-changed');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onFeaCaseChanged(cb);
    handle.emit({ active_case_id: 42, available_cases: ['A', 'B'] });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('fea-case-changed');
  });
});
