import { Show, createEffect, createMemo, onCleanup } from 'solid-js';
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
  state: { meshes: Record<string, any> };
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

  // Clear inner ref captures when the design viewport unmounts.
  // Equivalent to the onCleanup inside the former function-children pattern,
  // but compatible with SolidJS's JSX.Element children type for <Show>.
  createEffect(() => {
    if (designEffective()) {
      onCleanup(() => {
        innerFitToView = null;
        innerFlyToEntity = null;
      });
    }
  });

  // Label for the def-preview strip
  const defPreviewLabel = createMemo(() => {
    const name = props.defName();
    return name ? `Preview: ${name}` : 'Preview';
  });

  return (
    <div
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
          <div class={styles.viewportWrapper}>
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
            onResize={() => {}}
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
          <div class={styles.viewportWrapper}>
            <Viewport
              viewportId="design-main"
              viewportStore={props.viewportStore}
              meshes={props.engineStore.state.meshes}
              onSelect={props.onSelect}
              onHover={props.onHover}
              hoveredEntity={props.hoveredEntity}
              selectedEntity={props.selectedEntity}
              selectedEntities={props.selectedEntities}
              evalStatus={props.evalStatus}
              fitToViewRef={(fn) => { innerFitToView = fn; }}
              flyToEntityRef={(fn) => { innerFlyToEntity = fn; }}
              entityVisibility={props.entityVisibility}
            />
          </div>
        </Show>
      </Show>
    </div>
  );
}
