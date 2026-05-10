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

// ── Test group (c): line chart ───────────────────────────────────────────────

describe('AutoResolvePanel (c) line chart', () => {
  it('(c.1) renders SVG chart with data-testid="auto-resolve-chart", width=300, height=200', () => {
    const iterations = [220, 200, 185, 178].map((v, i) =>
      makeIteration(i + 1, { driving_metric: 'max_von_mises', driving_metric_value: v }),
    );
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const svg = screen.getByTestId('auto-resolve-chart') as SVGElement;
    expect(svg).toBeTruthy();
    expect(svg.getAttribute('width')).toBe('300');
    expect(svg.getAttribute('height')).toBe('200');
  });

  it('(c.2) SVG contains one polyline whose points attribute has 4 coordinate pairs', () => {
    const iterations = [220, 200, 185, 178].map((v, i) =>
      makeIteration(i + 1, { driving_metric: 'max_von_mises', driving_metric_value: v }),
    );
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const svg = screen.getByTestId('auto-resolve-chart');
    const polyline = svg.querySelector('polyline');
    expect(polyline).toBeTruthy();
    // "x1,y1 x2,y2 x3,y3 x4,y4" — split on whitespace, expect 4 pairs
    const points = (polyline!.getAttribute('points') ?? '').trim().split(/\s+/);
    expect(points).toHaveLength(4);
    // Each pair should be "number,number"
    for (const pt of points) {
      expect(pt).toMatch(/^[\d.]+,[\d.]+$/);
    }
  });

  it('(c.3) SVG contains the driving metric name as a text/label element', () => {
    const iterations = [220, 200, 185, 178].map((v, i) =>
      makeIteration(i + 1, { driving_metric: 'max_von_mises', driving_metric_value: v }),
    );
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // The metric name "max_von_mises" should appear somewhere in the chart region
    expect(screen.getByText(/max_von_mises/)).toBeTruthy();
  });

  it('(c.4) no polyline rendered when fewer than 2 iterations carry driving_metric_value', () => {
    const iterations = [
      makeIteration(1, { driving_metric: 'max_von_mises', driving_metric_value: 180 }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const svg = screen.getByTestId('auto-resolve-chart');
    const polyline = svg.querySelector('polyline');
    expect(polyline).toBeNull();
  });
});
