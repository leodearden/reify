import { For, Show } from 'solid-js';
import { Viewport } from './Viewport';
import type { ViewportProps } from './Viewport';
import { Splitter } from '../components/Splitter';
import type { ViewportStore } from '../stores/viewportStore';
import styles from './MultiViewport.module.css';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Per-pane passthrough props forwarded to each Viewport instance. */
type PanePassthroughProps = Pick<
  ViewportProps,
  | 'onSelect'
  | 'onHover'
  | 'hoveredEntity'
  | 'selectedEntity'
  | 'selectedEntities'
  | 'evalStatus'
  | 'entityVisibility'
  | 'tensegrityWires'
  | 'tensegritySurfaces'
  | 'fitToViewRef'
  | 'flyToEntityRef'
>;

/** Configuration for a single pane in the MultiViewport grid. */
export interface PaneConfig extends PanePassthroughProps {
  /** Stable viewport id (must exist in viewportStore or store will return undefined for sizeWeight). */
  viewportId: string;
  /** Meshes to render in this pane. */
  meshes: Record<string, any>;
}

/** Props for the MultiViewport component. */
export interface MultiViewportProps {
  /** Ordered list of panes to render. Length N drives the grid heuristic. */
  panes: PaneConfig[];
  /** Shared viewport store — supplies per-pane sizeWeight (for fr tracks) and camera. */
  viewportStore: ViewportStore;
  /** Optional data-testid for the root container (defaults to 'multi-viewport'). */
  'data-testid'?: string;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/**
 * MultiViewport — renders N panes from a config array in a CSS grid/tiling
 * layout with per-pane resize via column-level Splitters.
 *
 * Layout heuristic: `columns = ceil(sqrt(N))`, `rows = ceil(N/columns)`,
 * row-major fill. CSS Grid with inline `grid-template-columns`/`-rows` (set
 * inline so jsdom can round-trip them deterministically in tests; static
 * styling lives in MultiViewport.module.css). Each column track is
 * `<sizeWeight>fr` read from viewportStore (default 1fr). C-1 vertical
 * Splitters between columns drive `setSizeWeight` for per-pane resize.
 *
 * **Column-weight contract:** each column track's weight is taken from the
 * first-row pane at that column index (`props.panes[col]`). For grids with
 * more than one row (N > columns), panes in lower rows have their `sizeWeight`
 * ignored for column sizing, and dragging a splitter mutates only the
 * first-row pane's weight. Per-cell splitter trees (free-form resize for N≥3)
 * are deferred to PRD §10.
 *
 * **Splitter placement:** C-1 Splitters are absolutely positioned (taken out
 * of grid flow) at column boundaries. The `left` of splitter `col` is
 * `sum(weights[0..col]) / total * 100%` from the first-row column weights.
 *
 * @see DualViewport — the legacy two-pane special case generalized here.
 *   The scalar `splitRatio` is superseded by per-pane `sizeWeight` in this
 *   component. The def-preview strip/minimize/mesh-gate UX is orthogonal
 *   (PRD §7.2 inv.4) and intentionally stays in DualViewport. App.tsx
 *   migrates from DualViewport to MultiViewport in task δ (#4770).
 */
export function MultiViewport(props: MultiViewportProps) {
  // ── Container ref for resize calculations ─────────────────────────────────
  let containerRef!: HTMLDivElement;

  // ── Grid geometry ─────────────────────────────────────────────────────────
  // columns = ceil(sqrt(N)), rows = ceil(N/columns), row-major fill.
  // N=1→1×1, N=2→1×2 side-by-side, N=4→2×2, N=5→3 cols.
  // Guard: when panes is empty the Show fallback renders; clamp to 1 to avoid
  // Array(NaN)/Array(0) errors in the grid-template derivations.
  const columns = () => props.panes.length > 0 ? Math.ceil(Math.sqrt(props.panes.length)) : 1;
  const rows = () => props.panes.length > 0 ? Math.ceil(props.panes.length / columns()) : 1;

  // Grid-template-columns: one track per column, weight = first-row pane's
  // sizeWeight from the store (falls back to 1 if the viewport is not in store).
  const gridCols = () =>
    Array.from({ length: columns() }, (_, col) => {
      const pane = props.panes[col];
      const weight = pane
        ? (props.viewportStore.getViewport(pane.viewportId)?.sizeWeight ?? 1)
        : 1;
      return `${weight}fr`;
    }).join(' ');

  // Grid-template-rows: equal-weight rows, one track per row.
  const gridRowsStr = () => Array(rows()).fill('1fr').join(' ');

  // ── Splitter items: C-1 entries with { col, left } for absolute placement ──
  // Taken out of grid flow; left = cumulative fr-weight percentage.
  const splitterItems = () => {
    const c = columns();
    if (c <= 1) return [];
    const weights = Array.from({ length: c }, (_, col) => {
      const pane = props.panes[col];
      return pane ? (props.viewportStore.getViewport(pane.viewportId)?.sizeWeight ?? 1) : 1;
    });
    const total = weights.reduce((s, w) => s + w, 0);
    let cumul = 0;
    return Array.from({ length: c - 1 }, (_, col) => {
      cumul += weights[col];
      return { col, left: total > 0 ? `${(cumul / total) * 100}%` : '0%' };
    });
  };

  // ── Per-pane resize handler (mirrors DualViewport.handleDualResize) ────────
  function handlePaneResize(col: number, delta: number) {
    if (!containerRef) return;
    const w = containerRef.clientWidth;
    if (w <= 0) return;
    const id = props.panes[col].viewportId;
    const cur = props.viewportStore.getViewport(id)?.sizeWeight ?? 1;
    props.viewportStore.setSizeWeight(id, cur + delta / w);
  }

  return (
    <div
      ref={containerRef}
      data-testid={props['data-testid'] ?? 'multi-viewport'}
      class={styles.container}
      style={{
        'grid-template-columns': gridCols(),
        'grid-template-rows': gridRowsStr(),
      }}
    >
      <Show
        when={props.panes.length > 0}
        fallback={
          <div class={styles.empty} data-testid="multi-viewport-empty">
            No active viewport
          </div>
        }
      >
        <For each={props.panes}>
          {(pane) => (
            <div
              data-testid={`multi-viewport-pane-${pane.viewportId}`}
              class={styles.paneWrapper}
            >
              <Viewport
                viewportId={pane.viewportId}
                viewportStore={props.viewportStore}
                meshes={pane.meshes}
                onSelect={pane.onSelect}
                onHover={pane.onHover}
                hoveredEntity={pane.hoveredEntity}
                selectedEntity={pane.selectedEntity}
                selectedEntities={pane.selectedEntities}
                evalStatus={pane.evalStatus}
                entityVisibility={pane.entityVisibility}
                tensegrityWires={pane.tensegrityWires}
                tensegritySurfaces={pane.tensegritySurfaces}
                fitToViewRef={pane.fitToViewRef}
                flyToEntityRef={pane.flyToEntityRef}
              />
            </div>
          )}
        </For>
        <For each={splitterItems()}>
          {(s) => (
            <div
              data-testid={`multi-viewport-splitter-wrapper-${s.col}`}
              style={{ position: 'absolute', top: '0', bottom: '0', left: s.left, 'z-index': '1' }}
            >
              <Splitter
                orientation="vertical"
                data-testid={`multi-viewport-splitter-${s.col}`}
                onResize={(d: number) => handlePaneResize(s.col, d)}
              />
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}
