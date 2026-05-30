/**
 * Bridge tests for the mode-shape-frame Tauri event channel (task ι/3458).
 *
 * Per `docs/gui-event-channels.md` §2 (mode-shape-frame row, ACTIVE, owned by 3458).
 * Exercises the `onModeShapeFrame` inline shape-guard idiom in bridge.ts:
 * one happy-path case + six malformed-payload cases.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts —
// matches the established pattern in solver_progress.test.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onModeShapeFrame } from '../../bridge';
import type { ModeShapeFrame } from '../../types';

describe('mode-shape-frame bridge (task ι/3458)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) happy-path: valid payload — callback fires once with exact object', async () => {
    const handle = mockTauriEvent<ModeShapeFrame>('mode-shape-frame');
    const cb = vi.fn();

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 0.5, displaced_positions: [1.0, 2.0, 3.0] });

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith({
      mode_index: 0,
      phase: 0.5,
      displaced_positions: [1.0, 2.0, 3.0],
    });
  });

  it('(b) malformed: mode_index not a number — callback NOT invoked, console.warn mentions mode-shape-frame', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 'zero', phase: 0.5, displaced_positions: [1.0, 2.0, 3.0] });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });

  it('(c) malformed: phase not a number — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 'halfway', displaced_positions: [1.0, 2.0, 3.0] });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });

  it('(d) malformed: displaced_positions not an array — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 0.5, displaced_positions: 'not-an-array' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });

  it('(e) malformed: displaced_positions contains a non-number — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 0.5, displaced_positions: [1.0, 'nan', 3.0] });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });

  it('(f) malformed: payload not a plain object — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit('not-an-object' as unknown);

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });

  // ── task-4072 step-5: eigenvalue bridge guard ──────────────────────────────

  it('(g) happy peak frame: eigenvalue present as number — callback fires, payload includes eigenvalue', async () => {
    const handle = mockTauriEvent<ModeShapeFrame>('mode-shape-frame');
    const cb = vi.fn();

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 1.0, displaced_positions: [1, 2, 3], eigenvalue: 1000 } as unknown as ModeShapeFrame);

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith({
      mode_index: 0,
      phase: 1.0,
      displaced_positions: [1, 2, 3],
      eigenvalue: 1000,
    });
  });

  it('(h) base frame: no eigenvalue key — callback still fires (eigenvalue is optional)', async () => {
    const handle = mockTauriEvent<ModeShapeFrame>('mode-shape-frame');
    const cb = vi.fn();

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 0.0, displaced_positions: [1, 2, 3] } as unknown as ModeShapeFrame);

    expect(cb).toHaveBeenCalledOnce();
  });

  it('(i) malformed: eigenvalue present but non-number — callback NOT invoked, console.warn mentions mode-shape-frame', async () => {
    const handle = mockTauriEvent<unknown>('mode-shape-frame');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onModeShapeFrame(cb);
    handle.emit({ mode_index: 0, phase: 1.0, displaced_positions: [1, 2, 3], eigenvalue: 'big' });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('mode-shape-frame');
  });
});
