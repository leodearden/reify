import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@solidjs/testing-library';
import type { AutoResolveIteration } from '../types';
import type { AutoResolveLoopState } from '../stores/engineStore';
import { AutoResolvePanel } from '../panels/AutoResolvePanel';

// ── Fixture helpers ──────────────────────────────────────────────────────────

function makeIteration(
  iteration: number,
  overrides?: Partial<AutoResolveIteration>,
): AutoResolveIteration {
  return {
    iteration,
    parameters: overrides?.parameters ?? {
      'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' },
    },
    constraints: overrides?.constraints ?? {
      max_von_mises: {
        name: 'max_von_mises',
        value: 180,
        unit: 'MPa',
        target_upper: 200,
        satisfied: true,
      },
    },
    driving_metric: overrides?.driving_metric ?? 'max_von_mises',
    driving_metric_value: overrides?.driving_metric_value ?? 180,
  };
}

afterEach(() => {
  cleanup();
});

// ── Test group (b): constraint rows with status marker ───────────────────────

describe('AutoResolvePanel (b) constraint rows with status marker', () => {
  it('(b.1) renders metric name, value+unit, target bound, and ok status marker', () => {
    const iterations = [
      makeIteration(1, {
        constraints: {
          max_von_mises: {
            name: 'max_von_mises',
            value: 180,
            unit: 'MPa',
            target_upper: 200,
            satisfied: true,
          },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Metric name
    expect(screen.getByText('max_von_mises')).toBeTruthy();
    // Value with unit
    expect(screen.getByText('180MPa')).toBeTruthy();
    // Target bound text (≤ 200MPa)
    expect(screen.getByText(/≤\s*200MPa/)).toBeTruthy();
    // Status marker with data-status="ok"
    const marker = document.querySelector('[data-status="ok"]');
    expect(marker).toBeTruthy();
  });

  it('(b.2) violated constraint renders data-status="violation"', () => {
    const iterations = [
      makeIteration(1, {
        constraints: {
          max_von_mises: {
            name: 'max_von_mises',
            value: 220,
            unit: 'MPa',
            target_upper: 200,
            satisfied: false,
          },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const marker = document.querySelector('[data-status="violation"]');
    expect(marker).toBeTruthy();
  });

  it('(b.3) no constraint rows when iterations is empty', () => {
    const state: AutoResolveLoopState = { active: true, iterations: [] };
    render(() => <AutoResolvePanel state={state} />);
    // No data-status markers should be present
    expect(document.querySelector('[data-status]')).toBeNull();
  });
});

// ── Test group (a): header and parameter rows ────────────────────────────────

describe('AutoResolvePanel (a) header and parameter rows', () => {
  it('(a.1) mounts with data-testid="auto-resolve-panel" and data-testid="panel-title-auto-resolve"', () => {
    const state: AutoResolveLoopState = { active: true, iterations: [] };
    render(() => <AutoResolvePanel state={state} />);
    expect(screen.getByTestId('auto-resolve-panel')).toBeTruthy();
    expect(screen.getByTestId('panel-title-auto-resolve')).toBeTruthy();
  });

  it('(a.2) header shows iteration count when at least one iteration is present', () => {
    const iterations = [makeIteration(1), makeIteration(2), makeIteration(3)];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Should show "Iteration 3" (or "Iteration N" for N iterations)
    expect(screen.getByTestId('panel-title-auto-resolve').textContent).toMatch(/Iteration 3/);
  });

  it('(a.3) renders a parameter row with cell-id label and display value', () => {
    const iterations = [
      makeIteration(1, {
        parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' } },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Should render the parameter key "thickness" and display "4.2mm"
    expect(screen.getByText('thickness')).toBeTruthy();
    expect(screen.getByText('4.2mm')).toBeTruthy();
  });
});
