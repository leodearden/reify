/**
 * SolverProgressOverlay panel tests (task 3543, GR-016 ζ step-11).
 *
 * Mirrors FeaCasePickerDropdown.test.tsx structurally:
 * - null progress → renders nothing
 * - non-null progress → overlay with iter/residual/solver_kind/ETA content
 * - Cancel button click → onCancel invoked
 */
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { SolverProgressOverlay } from '../panels/SolverProgressOverlay';
import type { SolverProgress } from '../types';

const sampleProgress: SolverProgress = {
  solver_kind: 'cg',
  iter: 42,
  residual: 1.234e-6,
  eta_ms: 2500,
};

describe('SolverProgressOverlay', () => {
  it('(a) renders nothing when progress prop is null', () => {
    render(() => (
      <SolverProgressOverlay progress={null} onCancel={vi.fn()} />
    ));
    expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();
  });

  it('(b) renders iter/residual/solver_kind and ETA when progress is provided', () => {
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={vi.fn()} />
    ));
    const overlay = screen.getByTestId('solver-progress-overlay');
    expect(overlay).toBeTruthy();

    // solver_kind
    expect(overlay.textContent).toContain('cg');
    // iter
    expect(overlay.textContent).toContain('42');
    // residual in scientific notation (toExponential(2) → "1.23e-6")
    expect(overlay.textContent).toMatch(/1\.2[34]e-6/);
    // ETA present (2500 ms = 2.5 s)
    expect(overlay.textContent).toMatch(/2\.5\s*s|2500\s*ms/);
  });

  it('(c) clicking Cancel button invokes onCancel', () => {
    const onCancel = vi.fn();
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={onCancel} />
    ));
    const cancelBtn = screen.getByRole('button', { name: /cancel/i });
    fireEvent.click(cancelBtn);
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
