import { type Component, createMemo, For, Show } from 'solid-js';
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

// Sparkline layout constants
export const SPARK_W = 80;
const SPARK_H = 24;
export const SPARK_PAD = 2;

/**
 * Build the polyline points string mapping data coordinates to SVG space.
 *
 * @param xs      - data x values
 * @param ys      - data y values
 * @param destX1  - leftmost SVG x (maps to min data x)
 * @param destX2  - rightmost SVG x (maps to max data x)
 * @param destY1  - top SVG y (maps to MAX data y — SVG y-axis is inverted)
 * @param destY2  - bottom SVG y (maps to min data y)
 */
function buildPolylinePoints(
  xs: number[],
  ys: number[],
  destX1: number,
  destX2: number,
  destY1: number,
  destY2: number,
): string {
  // Use reduce instead of spread-into-Math.min/max to avoid the V8 argument-count
  // limit (~65k entries) that would silently crash for very long auto-resolve loops.
  const xMin = xs.reduce((m, v) => (v < m ? v : m), Infinity);
  const xMax = xs.reduce((m, v) => (v > m ? v : m), -Infinity);
  const yMin = ys.reduce((m, v) => (v < m ? v : m), Infinity);
  const yMax = ys.reduce((m, v) => (v > m ? v : m), -Infinity);

  return xs
    .map((x, i) => {
      const sx = linearScale(x, xMin, xMax, destX1, destX2);
      // SVG y-axis is inverted: high data value → low SVG-y (top of chart)
      const sy = linearScale(ys[i], yMin, yMax, destY2, destY1);
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
  const latestIteration = createMemo(() =>
    props.state.iterations.length > 0
      ? props.state.iterations[props.state.iterations.length - 1]
      : null,
  );

  /**
   * Driving metric name — invariant across the loop. Reads from the O(1) cached
   * `canonicalDrivingMetric` field set by `engineStore.applyAutoResolveIteration`
   * when the first metric-bearing iteration is accepted. Falls back to scanning
   * `iterations` for state objects constructed directly (e.g. in unit tests that
   * do not go through `applyAutoResolveIteration`). The engineStore enforces the
   * invariant: conflicting iterations are dropped before reaching the panel.
   */
  const chartMetricName = () =>
    props.state.canonicalDrivingMetric ??
    props.state.iterations.find((it) => it.driving_metric)?.driving_metric ??
    null;

  /**
   * (iteration_number, driving_metric_value) pairs with finite values only.
   * Memoised so that multi-read per render (Show predicate + polyline points)
   * drives a single filter+map rather than one per read.
   */
  const chartPoints = createMemo(() =>
    props.state.iterations
      .filter((it) => it.driving_metric_value !== undefined && Number.isFinite(it.driving_metric_value))
      .map((it) => ({ x: it.iteration, y: it.driving_metric_value! }))
  );

  /**
   * Per-parameter sparkline data: { cellId, series } memoised across all
   * iterations. Merges what were previously separate sparklineCellIds() and
   * sparklineSeries(cellId) calls into a single O(params × iters) sweep.
   */
  const sparklineData = createMemo(() => {
    const cellIds = Array.from(
      new Set(props.state.iterations.flatMap((it) => Object.keys(it.parameters))),
    );
    return cellIds.map((cellId) => ({
      cellId,
      // Keep iteration number as x so a null- or non-finite-filtered gap shows
      // as a visual hole (wider x-spacing) rather than silently collapsing to
      // even spacing.  Mirrors the chartPoints pattern (x: it.iteration, y: value).
      // Number.isFinite rejects null, NaN, and ±Infinity in one predicate,
      // giving symmetric defensive posture with the chartPoints filter on line 126.
      series: props.state.iterations
        .filter((it) => cellId in it.parameters && Number.isFinite(it.parameters[cellId].value))
        .map((it) => ({ x: it.iteration, y: it.parameters[cellId].value as number })),
    }));
  });

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
        <section class={styles.section} data-testid="auto-resolve-parameters">
          <div class={styles.sectionLabel}>Parameters</div>
          <For each={Object.entries(latestIteration()!.parameters)}>
            {([cellId, paramValue]) => (
              <div class={styles.row}>
                <span class={styles.cellId}>{cellId}</span>
                <span
                  class={styles.value}
                  data-non-scalar={paramValue.value === null ? 'true' : undefined}
                >
                  {paramValue.display}
                </span>
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

        {/* ── Per-parameter sparklines ────────────────────────────────── */}
        <section class={styles.section}>
          <div class={styles.sectionLabel}>Parameters over time</div>
          <For each={sparklineData()}>
            {({ cellId, series }) => {
              const hasLine = series.length >= 2;
              // Build points in sparkline SVG coordinate space (SPARK_W × SPARK_H)
              const pts = hasLine
                ? buildPolylinePoints(
                    series.map((p) => p.x),
                    series.map((p) => p.y),
                    SPARK_PAD,
                    SPARK_W - SPARK_PAD,
                    SPARK_PAD,
                    SPARK_H - SPARK_PAD,
                  )
                : '';
              return (
                <div class={styles.sparklineRow}>
                  <span class={styles.sparklineCellId}>{cellId}</span>
                  <svg
                    class={styles.sparkline}
                    width={SPARK_W}
                    height={SPARK_H}
                    data-testid="auto-resolve-sparkline"
                  >
                    <Show when={hasLine}>
                      <polyline
                        class={styles.sparklineLine}
                        fill="none"
                        points={pts}
                      />
                    </Show>
                  </svg>
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
            {/* Y-axis label — driving metric name, rotated 90° to sit alongside left edge */}
            <Show when={chartMetricName() !== null}>
              <text
                x={4}
                y={CHART_H / 2}
                class={styles.chartMetricLabel}
                text-anchor="middle"
                transform={`rotate(-90, 4, ${CHART_H / 2})`}
              >
                {chartMetricName()}
              </text>
            </Show>
            {/* Polyline — only when 2+ data points */}
            <Show when={chartPoints().length >= 2}>
              <polyline
                class={styles.chartLine}
                fill="none"
                points={buildPolylinePoints(
                  chartPoints().map((p) => p.x),
                  chartPoints().map((p) => p.y),
                  PLOT_X1,
                  PLOT_X2,
                  PLOT_Y1,
                  PLOT_Y2,
                )}
              />
            </Show>
          </svg>
        </section>
      </Show>
    </div>
  );
};
