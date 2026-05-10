import { type Component, For, Show } from 'solid-js';
import type { AutoResolveLoopState } from '../stores/engineStore';
import type { AutoResolveConstraintProgress } from '../types';
import styles from './AutoResolvePanel.module.css';

// ---------------------------------------------------------------------------
// Constraint target-bound formatting helper
// ---------------------------------------------------------------------------

/** Format the target-bound expression for a constraint row (e.g. "≤ 200MPa"). */
function formatTarget(c: AutoResolveConstraintProgress): string | null {
  const unit = c.unit ?? '';
  if (c.target_lower !== undefined && c.target_upper !== undefined) {
    return `${c.target_lower}${unit} – ${c.target_upper}${unit}`;
  }
  if (c.target_upper !== undefined) return `≤ ${c.target_upper}${unit}`;
  if (c.target_lower !== undefined) return `≥ ${c.target_lower}${unit}`;
  return null;
}

// ---------------------------------------------------------------------------
// Chart scaling helpers
// ---------------------------------------------------------------------------

/** Linear scale: map a value from [domainMin, domainMax] to [rangeMin, rangeMax]. */
function linearScale(
  value: number,
  domainMin: number,
  domainMax: number,
  rangeMin: number,
  rangeMax: number,
): number {
  if (domainMax === domainMin) return (rangeMin + rangeMax) / 2;
  return rangeMin + ((value - domainMin) / (domainMax - domainMin)) * (rangeMax - rangeMin);
}

// Chart layout constants
const CHART_W = 300;
const CHART_H = 200;
const CHART_PAD_LEFT = 32;   // room for y-axis label
const CHART_PAD_BOTTOM = 20; // room for x-axis label
const CHART_PAD_TOP = 10;
const CHART_PAD_RIGHT = 10;

const PLOT_X1 = CHART_PAD_LEFT;
const PLOT_X2 = CHART_W - CHART_PAD_RIGHT;
const PLOT_Y1 = CHART_PAD_TOP;          // top (high values)
const PLOT_Y2 = CHART_H - CHART_PAD_BOTTOM; // bottom (low SVG-y = visual top)

/** Build the polyline points string from paired (x, y) SVG coordinates. */
function buildPolylinePoints(
  xs: number[],
  ys: number[],
): string {
  const xMin = Math.min(...xs);
  const xMax = Math.max(...xs);
  const yMin = Math.min(...ys);
  const yMax = Math.max(...ys);

  return xs
    .map((x, i) => {
      const sx = linearScale(x, xMin, xMax, PLOT_X1, PLOT_X2);
      // SVG y-axis is inverted: high data value → low SVG-y (top of chart)
      const sy = linearScale(ys[i], yMin, yMax, PLOT_Y2, PLOT_Y1);
      return `${sx.toFixed(1)},${sy.toFixed(1)}`;
    })
    .join(' ');
}

// ---------------------------------------------------------------------------
// AutoResolvePanel — surfaces `param x = auto` loop iteration progress
// (Task 2967). Conditionally mounted by App.tsx when autoResolve.active.
// ---------------------------------------------------------------------------

export interface AutoResolvePanelProps {
  state: AutoResolveLoopState;
}

export const AutoResolvePanel: Component<AutoResolvePanelProps> = (props) => {
  const latestIteration = () =>
    props.state.iterations.length > 0
      ? props.state.iterations[props.state.iterations.length - 1]
      : null;

  /** Driving metric name taken from the first iteration that declares one. */
  const chartMetricName = () =>
    props.state.iterations.find((it) => it.driving_metric)?.driving_metric ?? null;

  /** (iteration_number, driving_metric_value) pairs with finite values only. */
  const chartPoints = (): { x: number; y: number }[] =>
    props.state.iterations
      .filter((it) => it.driving_metric_value !== undefined && Number.isFinite(it.driving_metric_value))
      .map((it) => ({ x: it.iteration, y: it.driving_metric_value! }));

  return (
    <div class={styles.panel} data-testid="auto-resolve-panel">
      <header class={styles.panelHeader} data-testid="panel-title-auto-resolve">
        <Show
          when={props.state.iterations.length > 0}
          fallback={<span class={styles.panelTitle}>Auto-Resolve</span>}
        >
          <span class={styles.panelTitle}>
            Iteration {props.state.iterations.length}
          </span>
        </Show>
      </header>

      {/* ── Parameters section ──────────────────────────────────────────── */}
      <Show when={latestIteration() !== null}>
        <section class={styles.section}>
          <div class={styles.sectionLabel}>Parameters</div>
          <For each={Object.entries(latestIteration()!.parameters)}>
            {([cellId, paramValue]) => (
              <div class={styles.row}>
                <span class={styles.cellId}>{cellId}</span>
                <span class={styles.value}>{paramValue.display}</span>
              </div>
            )}
          </For>
        </section>

        {/* ── Constraints section ──────────────────────────────────────── */}
        <section class={styles.section}>
          <div class={styles.sectionLabel}>Constraints</div>
          <For each={Object.entries(latestIteration()!.constraints)}>
            {([, constraint]) => {
              const target = formatTarget(constraint);
              const unit = constraint.unit ?? '';
              return (
                <div class={styles.row}>
                  <span class={styles.cellId}>{constraint.name}</span>
                  <span class={styles.value}>
                    {constraint.value}{unit}
                  </span>
                  <Show when={target !== null}>
                    <span class={styles.targetBound}>{target}</span>
                  </Show>
                  <span
                    class={styles.statusMarker}
                    data-status={constraint.satisfied ? 'ok' : 'violation'}
                  >
                    {constraint.satisfied ? '✓' : '✗'}
                  </span>
                </div>
              );
            }}
          </For>
        </section>

        {/* ── Line chart section ───────────────────────────────────────── */}
        <section class={styles.chartSection}>
          <svg
            class={styles.chart}
            width={CHART_W}
            height={CHART_H}
            data-testid="auto-resolve-chart"
          >
            {/* X-axis label */}
            <text
              x={CHART_W / 2}
              y={CHART_H - 4}
              class={styles.chartAxisLabel}
              text-anchor="middle"
            >
              Iteration
            </text>
            {/* Polyline — only when 2+ data points */}
            <Show when={chartPoints().length >= 2}>
              <polyline
                class={styles.chartLine}
                fill="none"
                points={buildPolylinePoints(
                  chartPoints().map((p) => p.x),
                  chartPoints().map((p) => p.y),
                )}
              />
            </Show>
          </svg>
        </section>
      </Show>
    </div>
  );
};
