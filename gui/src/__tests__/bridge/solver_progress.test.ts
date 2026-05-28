/**
 * Bridge tests for the solver-progress Tauri event channel (GR-016 ζ).
 *
 * Per `docs/gui-event-channels/solver-progress.md` and PRD §2.2 task ζ.
 * Exercises the `onSolverProgress` inline shape-guard idiom in bridge.ts:
 * two happy-path cases (eta_ms present / absent) + three malformed-payload cases.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts —
// matches the established pattern in feaCaseChanged.test.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onSolverProgress } from '../../bridge';
import type { SolverProgress } from '../../types';

describe('solver-progress bridge (GR-016 ζ)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('(a) happy-path: full payload with eta_ms — callback fires once with exact object', async () => {
    const handle = mockTauriEvent<SolverProgress>('solver-progress');
    const cb = vi.fn();

    await onSolverProgress(cb);
    handle.emit({ solver_kind: 'cg', iter: 5, residual: 1.2e-6, eta_ms: 1500 });

    expect(cb).toHaveBeenCalledOnce();
    expect(cb).toHaveBeenCalledWith({ solver_kind: 'cg', iter: 5, residual: 1.2e-6, eta_ms: 1500 });
  });

  it('(b) happy-path: eta_ms omitted — callback fires once, payload.eta_ms is undefined', async () => {
    const handle = mockTauriEvent<Omit<SolverProgress, 'eta_ms'>>('solver-progress');
    const cb = vi.fn();

    await onSolverProgress(cb);
    handle.emit({ solver_kind: 'cg', iter: 3, residual: 4.5e-5 });

    expect(cb).toHaveBeenCalledOnce();
    const received = cb.mock.calls[0]?.[0] as SolverProgress;
    expect(received.solver_kind).toBe('cg');
    expect(received.iter).toBe(3);
    expect(received.residual).toBe(4.5e-5);
    expect(received.eta_ms).toBeUndefined();
  });

  it('(c) malformed: iter is not a number — callback NOT invoked, console.warn mentions solver-progress', async () => {
    const handle = mockTauriEvent<unknown>('solver-progress');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onSolverProgress(cb);
    handle.emit({ solver_kind: 'cg', iter: 'not-a-number', residual: 1.0e-5 });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('solver-progress');
  });

  it('(d) malformed: residual missing — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('solver-progress');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onSolverProgress(cb);
    handle.emit({ solver_kind: 'cg', iter: 7 }); // missing residual

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('solver-progress');
  });

  it('(e) malformed: solver_kind is not a string — callback NOT invoked, console.warn fires', async () => {
    const handle = mockTauriEvent<unknown>('solver-progress');
    const cb = vi.fn();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await onSolverProgress(cb);
    handle.emit({ solver_kind: 42, iter: 7, residual: 1.0e-5 });

    expect(cb).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalled();
    expect(warnSpy.mock.calls[0]?.[0]).toContain('solver-progress');
  });
});
