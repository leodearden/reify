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

  it('(d) renders svg chart with polyline when trace has >=2 points', () => {
    const trace: SolverProgress[] = [
      { solver_kind: 'cg', iter: 1, residual: 0.5 },
      { solver_kind: 'cg', iter: 2, residual: 0.1 },
      { solver_kind: 'cg', iter: 3, residual: 0.01 },
    ];
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={vi.fn()} trace={trace} />
    ));
    const chart = screen.getByTestId('solver-progress-chart');
    expect(chart.tagName.toLowerCase()).toBe('svg');
    const polyline = chart.querySelector('polyline');
    expect(polyline).not.toBeNull();
    // points attribute should have same count of coordinate pairs as trace.length
    const pairs = polyline!.getAttribute('points')!.trim().split(/\s+/);
    expect(pairs).toHaveLength(trace.length);
  });

  it('(e) no polyline when trace is absent or has <2 points', () => {
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={vi.fn()} />
    ));
    // chart may exist but polyline must not
    const chart = screen.queryByTestId('solver-progress-chart');
    if (chart) {
      expect(chart.querySelector('polyline')).toBeNull();
    }
  });

  it('(f) shows refining indicator when coarseReached is true', () => {
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={vi.fn()} coarseReached={true} />
    ));
    const overlay = screen.getByTestId('solver-progress-overlay');
    expect(overlay.textContent).toMatch(/refin/i);
  });

  it('(f2) no refining indicator when coarseReached is false or absent', () => {
    render(() => (
      <SolverProgressOverlay progress={sampleProgress} onCancel={vi.fn()} coarseReached={false} />
    ));
    const overlay = screen.getByTestId('solver-progress-overlay');
    expect(overlay.textContent).not.toMatch(/refin/i);
  });
});
