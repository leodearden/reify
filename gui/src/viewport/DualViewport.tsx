import { Show, createMemo, onCleanup } from 'solid-js';
import { Viewport } from './Viewport';
import type { ViewportProps } from './Viewport';
import { Splitter } from '../components/Splitter';
import type { DefPreviewStore } from '../stores/defPreviewStore';
import type { ViewportStore } from '../stores/viewportStore';
import styles from './DualViewport.module.css';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Minimal subset of engineStore needed by DualViewport. */
interface EngineLike {
  state: { meshes: Record<string, any>; tensegrityWires: any[] };
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

export function DualViewport(props: DualViewportProps) {
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
  const defPreviewEffective = createMemo(
    () =>
      props.defPreviewActive() ||
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
            />
          </div>
        </Show>
      </Show>
    </div>
  );
}
