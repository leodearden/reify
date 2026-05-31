/**
 * SolverProgressOverlay — FEA solver progress overlay panel (task 3543, GR-016 ζ).
 *
 * Pure-render Solid component showing live CG solver iteration progress.
 * Renders nothing when `progress` is null (no solve in flight).
 * When non-null, renders a fixed-position card with solver kind, iteration
 * count, residual norm, optional ETA, and a Cancel button.
 *
 * Subscription lifecycle owner is a follow-on task; this component is
 * props-driven matching the FeaCasePickerDropdown precedent.
 *
 * Per PRD §2.2 task ζ and docs/gui-event-channels/solver-progress.md.
 */

import { Show, createMemo, type JSX } from 'solid-js';
import type { SolverProgress } from '../types';
import styles from './SolverProgressOverlay.module.css';

export interface SolverProgressOverlayProps {
  /** Live solver progress; null when no solve is in flight. */
  progress: SolverProgress | null;
  /** Called when the user clicks the Cancel button. */
  onCancel: () => void;
  /** Accumulated tick history for the convergence mini-chart. */
  trace?: SolverProgress[];
  /** True once residual first crossed below 1e-2 (coarse phase done). */
  coarseReached?: boolean;
}

// Chart layout constants (200×60 mini-chart)
const CW = 200;
const CH = 60;
const PAD = 4;

function linearScale(v: number, dMin: number, dMax: number, rMin: number, rMax: number): number {
  if (dMax === dMin) return (rMin + rMax) / 2;
  return rMin + ((v - dMin) / (dMax - dMin)) * (rMax - rMin);
}

function buildPolylinePoints(trace: SolverProgress[]): string {
  const iters = trace.map((t) => t.iter);
  const logRes = trace.map((t) => Math.log10(Math.max(t.residual, 1e-300)));
  const iMin = iters.reduce((m, v) => (v < m ? v : m), Infinity);
  const iMax = iters.reduce((m, v) => (v > m ? v : m), -Infinity);
  const rMin = logRes.reduce((m, v) => (v < m ? v : m), Infinity);
  const rMax = logRes.reduce((m, v) => (v > m ? v : m), -Infinity);
  return iters
    .map((iter, i) => {
      const sx = linearScale(iter, iMin, iMax, PAD, CW - PAD);
      const sy = linearScale(logRes[i], rMin, rMax, CH - PAD, PAD);
      return `${sx.toFixed(1)},${sy.toFixed(1)}`;
    })
    .join(' ');
}

/** Format residual using scientific notation with 2 decimal places. */
function formatResidual(r: number): string {
  return r.toExponential(2);
}

/** Format ETA: seconds when >= 1000 ms, otherwise milliseconds. */
function formatEta(eta_ms: number): string {
  if (eta_ms >= 1000) {
    return `${(eta_ms / 1000).toFixed(1)} s`;
  }
  return `${eta_ms} ms`;
}

/**
 * Solver progress overlay card.
 *
 * Renders nothing when progress is null. When non-null, renders a card
 * in the top-right corner with iteration metrics and a Cancel button.
 */
export function SolverProgressOverlay(props: SolverProgressOverlayProps): JSX.Element {
  // Memoize the polyline computation so buildPolylinePoints (5 reduce/map
  // passes over the full trace) only re-runs when props.trace changes, not on
  // every other reactive update in the component tree.
  const polylinePoints = createMemo(() => {
    if (!props.trace || props.trace.length < 2) return '';
    return buildPolylinePoints(props.trace);
  });

  return (
    <Show when={props.progress !== null && props.progress}>
      {(p) => (
        <div class={styles.overlay} data-testid="solver-progress-overlay">
          <div class={styles.row}>
            <span class={styles.label}>Solver</span>
            <span class={styles.value}>{p().solver_kind}</span>
          </div>
          <div class={styles.row}>
            <span class={styles.label}>Iteration</span>
            <span class={styles.value}>{p().iter}</span>
          </div>
          <div class={styles.row}>
            <span class={styles.label}>Residual</span>
            <span class={styles.value}>{formatResidual(p().residual)}</span>
          </div>
          <Show when={p().eta_ms !== undefined}>
            <div class={styles.row}>
              <span class={styles.label}>ETA</span>
              <span class={styles.value}>{formatEta(p().eta_ms!)}</span>
            </div>
          </Show>
          <Show when={(props.trace?.length ?? 0) >= 2}>
            <svg
              data-testid="solver-progress-chart"
              width={CW}
              height={CH}
              class={styles.chart}
            >
              <polyline
                points={polylinePoints()}
                fill="none"
                stroke="var(--accent, #4fc3f7)"
                stroke-width="1.5"
              />
            </svg>
          </Show>
          <Show when={props.coarseReached}>
            <span class={styles.refining}>refining…</span>
          </Show>
          <button class={styles.cancelButton} onClick={() => props.onCancel()}>
            Cancel
          </button>
        </div>
      )}
    </Show>
  );
}
