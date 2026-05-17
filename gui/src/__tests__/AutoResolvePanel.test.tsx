import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup, within } from '@solidjs/testing-library';
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
    // Metric name appears in the constraint row (may also appear in the chart y-axis label
    // when driving_metric matches the constraint name — use getAllByText to allow either case)
    expect(screen.getAllByText('max_von_mises').length).toBeGreaterThanOrEqual(1);
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

  it('(b.4) target_lower only renders ≥ X unit', () => {
    // formatTarget branch: only target_lower is set
    const iterations = [
      makeIteration(1, {
        constraints: {
          min_thickness: {
            name: 'min_thickness',
            value: 3.8,
            unit: 'mm',
            target_lower: 3.0,
            satisfied: true,
          },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Should render ≥ 3mm (lower-bound only branch)
    expect(screen.getByText(/≥\s*3mm/)).toBeTruthy();
    // Should NOT render ≤ anywhere for this constraint
    expect(screen.queryByText(/≤/)).toBeNull();
  });

  it('(b.5) both bounds present renders X – Y unit', () => {
    // formatTarget branch: both target_lower and target_upper are set
    const iterations = [
      makeIteration(1, {
        constraints: {
          displacement: {
            name: 'displacement',
            value: 1.2,
            unit: 'mm',
            target_lower: 0.5,
            target_upper: 2.0,
            satisfied: true,
          },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Should render the dual-bound format "0.5mm – 2mm"
    expect(screen.getByText(/0\.5mm\s*–\s*2mm/)).toBeTruthy();
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
    // 'thickness' appears in both the Parameters row and the sparkline label.
    // Scope the sparkline-label check via within() so future additions of the
    // text elsewhere in the panel don't break this assertion.
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    const sparklineRow = sparklineSvg.closest('div')!;
    expect(within(sparklineRow).getByText('thickness')).toBeTruthy();
    // Scope cell-id and display-value assertions to the Parameters section
    // explicitly, so a regression that drops them from the Parameters row
    // (but not the sparkline row) is caught.
    const parametersSection = screen.getByTestId('auto-resolve-parameters');
    expect(within(parametersSection).getByText('thickness')).toBeTruthy();
    expect(within(parametersSection).getByText('4.2mm')).toBeTruthy();
  });

  it('(a.4) parameters section shows the LATEST iteration values when multiple iterations differ', () => {
    // Two iterations share the same parameter cell-id but with different display values.
    // The Parameters section must show the LAST iteration's value, not the first.
    const iterations = [
      makeIteration(1, {
        parameters: {
          'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' },
        },
      }),
      makeIteration(2, {
        parameters: {
          'Bracket.thickness': { value: 5.5, unit: 'mm', display: '5.5mm' },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const parametersSection = screen.getByTestId('auto-resolve-parameters');
    // Latest iteration's display value must appear in the Parameters section
    expect(within(parametersSection).getByText('5.5mm')).toBeTruthy();
    // Earlier iteration's display value must NOT appear in the Parameters section
    expect(within(parametersSection).queryByText('4.2mm')).toBeNull();
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
    const svg = screen.getByTestId('auto-resolve-chart');
    expect(svg).toBeInstanceOf(SVGSVGElement);
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

  it('(c.3) SVG contains the driving metric name as a text/label element inside the chart SVG', () => {
    const iterations = [220, 200, 185, 178].map((v, i) =>
      makeIteration(i + 1, { driving_metric: 'max_von_mises', driving_metric_value: v }),
    );
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Must find the metric name INSIDE the chart SVG element, not just anywhere on the page
    const svg = screen.getByTestId('auto-resolve-chart');
    expect(within(svg).getByText(/max_von_mises/)).toBeTruthy();
  });

  it('(c.3.b) chart SVG renders the driving_metric label when name differs from constraint names', () => {
    // driving_metric is 'driving_metric_z' but constraints are keyed by 'max_von_mises' only.
    // This proves the chart-side label is genuinely inside the SVG (not accidentally matching
    // the constraints section which only shows 'max_von_mises').
    const iterations = [220, 200, 185, 178].map((v, i) =>
      makeIteration(i + 1, {
        driving_metric: 'driving_metric_z',
        driving_metric_value: v,
        constraints: {
          max_von_mises: {
            name: 'max_von_mises',
            value: v,
            unit: 'MPa',
            target_upper: 200,
            satisfied: v <= 200,
          },
        },
      }),
    );
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const svg = screen.getByTestId('auto-resolve-chart');
    // 'driving_metric_z' must appear as a text element inside the chart SVG
    expect(within(svg).getByText('driving_metric_z')).toBeTruthy();
    // 'max_von_mises' should NOT appear inside the chart SVG (it's only in the constraints section)
    expect(within(svg).queryByText('max_von_mises')).toBeNull();
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

// ── Test group (d): per-parameter sparklines ─────────────────────────────────

describe('AutoResolvePanel (d) per-parameter sparklines', () => {
  it('(d.1) renders one sparkline SVG per parameter across 3 iterations', () => {
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' }, length: { value: 100, unit: 'mm', display: '100mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: 4.5, unit: 'mm', display: '4.5mm' }, length: { value: 98, unit: 'mm', display: '98mm' } } }),
      makeIteration(3, { parameters: { thickness: { value: 4.8, unit: 'mm', display: '4.8mm' }, length: { value: 95, unit: 'mm', display: '95mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // Should have exactly 2 sparklines (one per parameter)
    const sparklines = screen.getAllByTestId('auto-resolve-sparkline');
    expect(sparklines).toHaveLength(2);
  });

  it('(d.2) each sparkline contains a polyline reflecting parameter values across iterations', () => {
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' }, length: { value: 100, unit: 'mm', display: '100mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: 4.5, unit: 'mm', display: '4.5mm' }, length: { value: 98, unit: 'mm', display: '98mm' } } }),
      makeIteration(3, { parameters: { thickness: { value: 4.8, unit: 'mm', display: '4.8mm' }, length: { value: 95, unit: 'mm', display: '95mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklines = screen.getAllByTestId('auto-resolve-sparkline');
    // Each sparkline should have a polyline with 3 points (one per iteration)
    for (const sparkline of sparklines) {
      const polyline = sparkline.querySelector('polyline');
      expect(polyline).toBeTruthy();
      const points = (polyline!.getAttribute('points') ?? '').trim().split(/\s+/);
      expect(points).toHaveLength(3);
    }
  });

  it('(d.3) only 1 iteration: sparkline rows exist but no polyline inside', () => {
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklines = screen.getAllByTestId('auto-resolve-sparkline');
    expect(sparklines.length).toBeGreaterThanOrEqual(1);
    // No polyline when only 1 data point
    for (const sparkline of sparklines) {
      expect(sparkline.querySelector('polyline')).toBeNull();
    }
  });

  it('(d.4) sparkline labels include the parameter cell-id', () => {
    const iterations = [
      makeIteration(1, { parameters: { 'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' } } }),
      makeIteration(2, { parameters: { 'Bracket.thickness': { value: 4.5, unit: 'mm', display: '4.5mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    // The cell-id label should appear in the sparkline row
    // Note: 'Bracket.thickness' also appears in the parameters section; use getAllByText
    const labels = screen.getAllByText('Bracket.thickness');
    expect(labels.length).toBeGreaterThanOrEqual(1);
  });
});

// ── Test group (e): sparkline null-filter for non-scalar values ──────────────

describe('AutoResolvePanel (e) non-scalar value sparkline null-filter', () => {
  it('(e.1) sparkline polyline excludes a null-value iteration', () => {
    // 4 iterations for 'thickness' with values [1.0, 2.0, null, 4.0].
    // After null-filtering, 3 finite points survive (iterations 1, 2, 4).
    // The x-axis uses original iteration numbers, not filtered array indices,
    // so the gap introduced by the null at iteration 3 is preserved in x-spacing.
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 1.0, unit: 'mm', display: '1mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: 2.0, unit: 'mm', display: '2mm' } } }),
      makeIteration(3, { parameters: { thickness: { value: null, unit: '', display: '<non-scalar>' } } }),
      makeIteration(4, { parameters: { thickness: { value: 4.0, unit: 'mm', display: '4mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    const polyline = sparklineSvg.querySelector('polyline');
    expect(polyline).toBeTruthy();
    const points = (polyline!.getAttribute('points') ?? '').trim().split(/\s+/);
    // Exactly 3 coordinate pairs — the null iteration is filtered out
    expect(points).toHaveLength(3);
    // Parse the three coordinate pairs and verify all y-values are finite and
    // the outer y-values are distinct — guards against a filter inversion that
    // kept the null and dropped a finite value.
    const yValues = points.map((pair) => parseFloat(pair.split(',')[1]!));
    expect(Number.isFinite(yValues[0]!)).toBe(true);
    expect(Number.isFinite(yValues[1]!)).toBe(true);
    expect(Number.isFinite(yValues[2]!)).toBe(true);
    expect(yValues[0]).not.toBe(yValues[2]); // 1.0 and 4.0 map to different y coords
    // Middle-point x assertion: iteration-indexed x for point[1] (iteration 2,
    // xMin=1, xMax=4) = SPARK_PAD + (2-1)/(4-1) * (SPARK_W - 2*SPARK_PAD)
    //   = 2 + (1/3)*76 ≈ 27.33
    // A filtered-index regression would place it at linearScale(1,0,2,2,78) = 40.
    // The ≈12.7-unit separation is well above toFixed(1) rounding error, so
    // this assertion cleanly distinguishes the two implementations.
    const xs = points.map((pair) => parseFloat(pair.split(',')[0]!));
    expect(xs[1]).toBeCloseTo(27.333, 1);
  });

  it('(e.2) all-null sparkline draws no polyline but the sparkline SVG still renders', () => {
    // 2 iterations, both with thickness.value = null.
    // After null-filtering the series is empty → hasLine = false → no polyline.
    // The SVG row itself must still render (cellId remains in the union set).
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: null, unit: '', display: '<non-scalar>' } } }),
      makeIteration(2, { parameters: { thickness: { value: null, unit: '', display: '<non-scalar>' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    expect(sparklineSvg).toBeTruthy();
    // No polyline — after filtering, 0 finite values remain
    expect(sparklineSvg.querySelector('polyline')).toBeNull();
  });

  it('(e.3) mixed [5.0, null] sparkline: no polyline, cellId label still renders', () => {
    // 2 iterations with values [5.0, null] → after filter: [5.0] → length 1 → no polyline.
    // The sparkline SVG row and its cellId label must still be present.
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 5.0, unit: 'mm', display: '5mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: null, unit: '', display: '<non-scalar>' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    // SVG exists but no polyline (only 1 finite value after filtering)
    expect(sparklineSvg).toBeTruthy();
    expect(sparklineSvg.querySelector('polyline')).toBeNull();
    // cellId label still renders in the sparkline row
    const sparklineRow = sparklineSvg.closest('div')!;
    expect(within(sparklineRow).getByText('thickness')).toBeTruthy();
  });

  it('(e.4) sparkline polyline excludes a non-null non-finite (NaN) iteration', () => {
    // NaN can arrive from JSON parsing or arithmetic in upstream stores;
    // Number.isFinite rejects it where !== null would not.  The cast
    // `as unknown as number` simulates that runtime scenario without touching
    // the production type — this test specifically guards the NaN/Infinity case,
    // not all possible type bypasses.
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: NaN as unknown as number, unit: 'mm', display: 'NaN' } } }),
      makeIteration(3, { parameters: { thickness: { value: 4.8, unit: 'mm', display: '4.8mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    const polyline = sparklineSvg.querySelector('polyline');
    expect(polyline).toBeTruthy();
    const pointsAttr = polyline!.getAttribute('points') ?? '';
    // Exactly 2 pairs — the non-finite iteration is filtered out just like null
    const points = pointsAttr.trim().split(/\s+/);
    expect(points).toHaveLength(2);
    // Parse-then-finite: mirrors the production Number.isFinite predicate and is
    // robust to serialization changes (unlike a /NaN/ regex check).
    for (const pair of points) {
      const [x, y] = pair.split(',').map(parseFloat);
      expect(Number.isFinite(x!)).toBe(true);
      expect(Number.isFinite(y!)).toBe(true);
    }
  });

  it('(e.5) sparkline polyline excludes a non-null Infinity iteration', () => {
    // Parallel to (e.4): Infinity also arrives via arithmetic (e.g. divide-by-zero)
    // and bypasses !== null.  Number.isFinite rejects both NaN and Infinity, so this
    // test locks the full non-finite half of the contract.
    const iterations = [
      makeIteration(1, { parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' } } }),
      makeIteration(2, { parameters: { thickness: { value: Infinity as unknown as number, unit: 'mm', display: 'Infinity' } } }),
      makeIteration(3, { parameters: { thickness: { value: 4.8, unit: 'mm', display: '4.8mm' } } }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const sparklineSvg = screen.getByTestId('auto-resolve-sparkline');
    const polyline = sparklineSvg.querySelector('polyline');
    expect(polyline).toBeTruthy();
    const pointsAttr = polyline!.getAttribute('points') ?? '';
    // Exactly 2 pairs — the Infinity iteration is filtered out
    const points = pointsAttr.trim().split(/\s+/);
    expect(points).toHaveLength(2);
    // Parse-then-finite: parallels (e.4) and covers the Infinity half of the
    // contract. Mirrors the production Number.isFinite predicate exactly.
    for (const pair of points) {
      const [x, y] = pair.split(',').map(parseFloat);
      expect(Number.isFinite(x!)).toBe(true);
      expect(Number.isFinite(y!)).toBe(true);
    }
  });
});

// ── Test group (f): non-scalar error chip in Parameters section ──────────────

describe('AutoResolvePanel (f) non-scalar error chip in Parameters section', () => {
  it('(f.1) non-scalar row renders display text and carries data-non-scalar="true"', () => {
    // One iteration with thickness.value = null → display '<non-scalar>'.
    // The value span must have data-non-scalar="true" and contain the display text.
    const iterations = [
      makeIteration(1, {
        parameters: { thickness: { value: null, unit: '', display: '<non-scalar>' } },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const parametersSection = screen.getByTestId('auto-resolve-parameters');
    // Display text renders inside the Parameters section
    expect(within(parametersSection).getByText('<non-scalar>')).toBeTruthy();
    // An element with data-non-scalar="true" exists in the Parameters section
    const chip = parametersSection.querySelector('[data-non-scalar="true"]');
    expect(chip).toBeTruthy();
    // That element contains the display text
    expect(chip!.textContent).toBe('<non-scalar>');
  });

  it('(f.2) scalar parameter row does NOT carry data-non-scalar attribute', () => {
    // One iteration with a normal scalar value → no data-non-scalar attribute.
    const iterations = [
      makeIteration(1, {
        parameters: { thickness: { value: 4.2, unit: 'mm', display: '4.2mm' } },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const parametersSection = screen.getByTestId('auto-resolve-parameters');
    // Display value renders normally
    expect(within(parametersSection).getByText('4.2mm')).toBeTruthy();
    // No data-non-scalar attribute present
    expect(parametersSection.querySelector('[data-non-scalar="true"]')).toBeNull();
  });

  it('(f.3) mixed rows: only the null-value row carries data-non-scalar="true"', () => {
    // One iteration with two parameters: one scalar, one non-scalar.
    // Exactly one element in the Parameters section must have data-non-scalar="true".
    const iterations = [
      makeIteration(1, {
        parameters: {
          thickness: { value: 4.2, unit: 'mm', display: '4.2mm' },
          area: { value: null, unit: '', display: '<non-scalar>' },
        },
      }),
    ];
    const state: AutoResolveLoopState = { active: true, iterations };
    render(() => <AutoResolvePanel state={state} />);
    const parametersSection = screen.getByTestId('auto-resolve-parameters');
    const chips = parametersSection.querySelectorAll('[data-non-scalar="true"]');
    // Exactly one chip
    expect(chips).toHaveLength(1);
    // That chip's text is the non-scalar display string
    expect(chips[0].textContent).toBe('<non-scalar>');
  });
});
