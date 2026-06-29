import { Show, createMemo, onCleanup } from 'solid-js';
import { Viewport } from './Viewport';
import type { ViewportProps } from './Viewport';
import { Splitter } from '../components/Splitter';
import type { DefPreviewStore } from '../stores/defPreviewStore';
import type { ViewportStore } from '../stores/viewportStore';
import { createFeaModeStore } from '../stores/feaModeStore';
import styles from './DualViewport.module.css';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Minimal subset of engineStore needed by DualViewport. */
interface EngineLike {
  state: { meshes: Record<string, any>; tensegrityWires: any[]; tensegritySurfaces: any[] };
}

/** Passthrough props forwarded to the design Viewport instance. */
type PassthroughProps = Pick<
  ViewportProps,
  | 'onSelect'
  | 'onHover'
  | 'hoveredEntity'
  | 'selectedEntity'
  | 'selectedEntities'
  | 'evalStatus'
  | 'entityVisibility'
  | 'displayAppearance'
  | 'feaDiagnostics'
>;

/**
 * Ref-registration callbacks accepted at the DualViewport level.
 * These are NOT in PassthroughProps — DualViewport intercepts them and
 * installs stable proxies so they work across Show mount/unmount transitions.
 */
interface RefProps {
  fitToViewRef?: (fn: () => void) => void;
  flyToEntityRef?: (fn: (entityPath: string) => void) => void;
}

export interface DualViewportProps extends PassthroughProps, RefProps {
  engineStore: EngineLike;
  defPreviewStore: DefPreviewStore;
  viewportStore: ViewportStore;
  /** Auto-activation signal: true when the cursor is inside a definition. */
  defPreviewActive: () => boolean;
  /** Auto-activation signal: true when design meshes are loaded. */
  designViewportActive: () => boolean;
  /** Current definition name used in the minimized strip label. */
  defName: () => string | null;
  /** Called when the user clicks a minimized strip to force-expand it. */
  onForceExpand: (viewportId: string) => void;
  /** Optional data-testid for the root container. */
  'data-testid'?: string;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/**
 * DualViewport — the legacy two-pane special case (design + def-preview).
 *
 * **Generalized by MultiViewport (task β, #4766).** MultiViewport renders N
 * panes from a config array in a CSS grid/tiling layout and supersedes the
 * scalar `splitRatio` with per-pane `sizeWeight` from `viewportStore`. App.tsx
 * migrates from DualViewport to MultiViewport in task δ (#4767).
 *
 * The def-preview strip / minimize / mesh-gate UX is orthogonal to the N-pane
 * model grid (PRD §7.2 inv.4) and intentionally stays here. Do NOT entangle
 * this UX into MultiViewport; it would pollute the clean N-pane contract that δ
 * consumes and risk regressing this well-tested component.
 *
 * **Back-compat note:** the two-viewport layout that DualViewport implements is
 * equivalent to a degenerate two-pane MultiViewport grid (1 row × 2 columns
 * side-by-side) verified by the `(degenerate-2up)` test in MultiViewport.test.tsx.
 */
export function DualViewport(props: DualViewportProps) {
  // ── FEA-mode store — process-wide singleton for the design viewport ────────
  // Created once at DualViewport mount (mirrors how feaDiagnostics is threaded).
  // Passed to the design-main <Viewport> so contour/deformed rendering is live.
  const feaModeStore = createFeaModeStore();

  // ── Container ref for resize calculations ─────────────────────────────────
  let containerRef!: HTMLDivElement;

  // ── Stable ref proxies ────────────────────────────────────────────────────
  // These closures capture the inner fn registered by the mounted Viewport.
  // They are installed unconditionally at setup time so the parent always
  // receives a stable function reference regardless of Show mount state.
  // When no inner Viewport is mounted, the proxies are safe no-ops.
  let innerFitToView: (() => void) | null = null;
  let innerFlyToEntity: ((entityPath: string) => void) | null = null;

  props.fitToViewRef?.(() => innerFitToView?.());
  props.flyToEntityRef?.((p) => innerFlyToEntity?.(p));

  // ── Effective activation: auto signal OR user forceExpanded override ───────
  // Mirrors App.tsx's `hasMeshes` gate on the design pane (Object.keys idiom):
  // automatic expansion requires preview content so a cursor move into a
  // definition whose preview has not yet loaded leaves the pane minimized
  // instead of opening a blank grid. forceExpanded is an unconditional manual
  // override (clicking the strip must always expand, even before meshes load).
  //
  // Layering note: the design-pane equivalent gate lives in App.tsx
  // (`designViewportActive={hasMeshes}`), while this gate is applied here.
  // Both enforce the same "don't open a blank pane" rule, just at different
  // layers. App.tsx already computes `hasMeshes` for other purposes and passes
  // it as a prop; `defPreviewStore` is only consumed by DualViewport, so the
  // gate naturally belongs here rather than requiring a parallel `hasPreviewMeshes`
  // signal to be threaded through App.tsx.
  const defPreviewHasPreviewMeshes = createMemo(
    () => Object.keys(props.defPreviewStore.state.meshes).length > 0,
  );
  const defPreviewEffective = createMemo(
    () =>
      (props.defPreviewActive() && defPreviewHasPreviewMeshes()) ||
      (props.viewportStore.state.viewports['def-preview']?.forceExpanded ?? false),
  );
  const designEffective = createMemo(
    () =>
      props.designViewportActive() ||
      (props.viewportStore.state.viewports['design-main']?.forceExpanded ?? false),
  );


  // True when both viewports are simultaneously visible (wrapper flex styles apply)
  const bothActive = createMemo(() => defPreviewEffective() && designEffective());

  // ── Dual-splitter resize handler ──────────────────────────────────────────
  function handleDualResize(delta: number) {
    if (!containerRef) return;  // SSR: Solid assigns `let ref!` synchronously at DOM mount;
                                 //       in SSR (no DOM), the ref stays undefined — accessing
                                 //       .clientHeight without this guard would throw a TypeError.
    const h = containerRef.clientHeight;
    if (h <= 0) return;
    const current = props.viewportStore.state.splitRatio;
    props.viewportStore.setSplitRatio(current + delta / h);
  }

  // Label for the def-preview strip
  const defPreviewLabel = createMemo(() => {
    const name = props.defName();
    return name ? `Preview: ${name}` : 'Preview';
  });

  return (
    <div
      ref={containerRef}
      class={styles.container}
      data-testid={props['data-testid'] ?? 'dual-viewport'}
    >
      <Show
        when={defPreviewEffective() || designEffective()}
        fallback={
          <div class={styles.empty} data-testid="dual-viewport-empty">
            No active viewport
          </div>
        }
      >
        {/* ── Def-preview area ─────────────────────────────────── */}
        <Show
          when={defPreviewEffective()}
          fallback={
            <div
              class={styles.strip}
              data-testid="strip-def-preview"
              onClick={() => props.onForceExpand('def-preview')}
            >
              {defPreviewLabel()}
            </div>
          }
        >
          <div
            class={styles.viewportWrapper}
            data-testid="dual-viewport-def-preview-wrapper"
            style={bothActive() ? { flex: `${props.viewportStore.state.splitRatio} 0 0%` } : undefined}
          >
            <Viewport
              viewportId="def-preview"
              viewportStore={props.viewportStore}
              meshes={props.defPreviewStore.state.meshes}
            />
          </div>
        </Show>

        {/* Splitter between the two — only when both are active */}
        <Show when={defPreviewEffective() && designEffective()}>
          <Splitter
            orientation="horizontal"
            onResize={handleDualResize}
            data-testid="splitter-dual"
          />
        </Show>

        {/* ── Design area ───────────────────────────────────────── */}
        <Show
          when={designEffective()}
          fallback={
            <div
              class={styles.strip}
              data-testid="strip-design"
              onClick={() => props.onForceExpand('design-main')}
            >
              Design
            </div>
          }
        >
          <div
            class={styles.viewportWrapper}
            data-testid="dual-viewport-design-wrapper"
            style={bothActive() ? { flex: `${1 - props.viewportStore.state.splitRatio} 0 0%` } : undefined}
          >
            <Viewport
              viewportId="design-main"
              viewportStore={props.viewportStore}
              meshes={props.engineStore.state.meshes}
              tensegrityWires={props.engineStore.state.tensegrityWires}
              tensegritySurfaces={props.engineStore.state.tensegritySurfaces}
              onSelect={props.onSelect}
              onHover={props.onHover}
              hoveredEntity={props.hoveredEntity}
              selectedEntity={props.selectedEntity}
              selectedEntities={props.selectedEntities}
              evalStatus={props.evalStatus}
              fitToViewRef={(fn) => {
                innerFitToView = fn;
                onCleanup(() => { innerFitToView = null; });
              }}
              flyToEntityRef={(fn) => {
                innerFlyToEntity = fn;
                onCleanup(() => { innerFlyToEntity = null; });
              }}
              entityVisibility={props.entityVisibility}
              displayAppearance={props.displayAppearance}
              feaDiagnostics={props.feaDiagnostics}
              feaModeStore={feaModeStore}
            />
          </div>
        </Show>
      </Show>
    </div>
  );
}
