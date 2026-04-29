import { type Component, For, Show, createSignal } from 'solid-js';
import type { MechanismDescriptor, JointDescriptor } from '../types';
import styles from './MechanismPanel.module.css';

// ---------------------------------------------------------------------------
// Display-unit conversion helpers
// ---------------------------------------------------------------------------

/** Convert SI metres to millimetres. */
function mToMm(si: number): number {
  return si * 1000;
}

/** Convert SI radians to degrees. */
function radToDeg(si: number): number {
  return si * (180 / Math.PI);
}

/** Format a slider display-unit value as a string with unit suffix for `set_parameter`. */
function formatParamValue(displayValue: number, kind: string): string {
  if (kind === 'prismatic') {
    return `${displayValue}mm`;
  } else if (kind === 'revolute') {
    return `${displayValue}deg`;
  }
  return `${displayValue}`;
}

/** Convert a joint's SI range bounds to display-unit values. */
function siToDisplay(si: number | null, kind: string): number | null {
  if (si === null) return null;
  if (kind === 'prismatic') return mToMm(si);
  if (kind === 'revolute') return radToDeg(si);
  return si;
}

/** Convert a joint's current SI value to display units. */
function currentSiToDisplay(si: number | null, kind: string): number | null {
  return siToDisplay(si, kind);
}

// ---------------------------------------------------------------------------
// JointRow component
// ---------------------------------------------------------------------------

interface JointRowProps {
  joint: JointDescriptor;
  onSetParameter: (cellId: string, value: string) => void;
  onScrubLocal: (cellId: string | null, jointIndex: number, valueSi: number) => void;
  mechanismCellId: string;
}

const JointRow: Component<JointRowProps> = (props) => {
  const joint = () => props.joint;
  const kind = () => joint().kind;
  const dimension = () => joint().dimension;
  const drivingParam = () => joint().driving_param_cell_id;

  // Whether this joint supports scrubbing (prismatic or revolute with a param binding)
  const isScrubbable = () =>
    (kind() === 'prismatic' || kind() === 'revolute') && drivingParam() !== null;

  // Display-unit range
  const minDisplay = () => siToDisplay(joint().range_lower_si, kind()) ?? 0;
  const maxDisplay = () => siToDisplay(joint().range_upper_si, kind()) ?? 100;

  // Initial slider value from current_value_si
  const initialDisplay = () => {
    const disp = currentSiToDisplay(joint().current_value_si, kind());
    return disp ?? minDisplay();
  };

  const [sliderValue, setSliderValue] = createSignal(initialDisplay());

  // RAF coalescing: one pending setParameter per joint slot
  let pendingValue: number | null = null;
  let rafId: number | null = null;

  function scheduleSetParameter(displayValue: number): void {
    pendingValue = displayValue;
    if (rafId === null) {
      rafId = requestAnimationFrame(() => {
        rafId = null;
        if (pendingValue === null) return;
        const val = pendingValue;
        pendingValue = null;
        const param = drivingParam();
        if (param !== null) {
          props.onSetParameter(param, formatParamValue(val, kind()));
        }
      });
    }
  }

  function handleInput(event: Event): void {
    const input = event.target as HTMLInputElement;
    const displayValue = Number(input.value);
    setSliderValue(displayValue);

    // Notify the store for optimistic UI update
    const param = drivingParam();
    props.onScrubLocal(param, joint().joint_index, displayValue);

    // Schedule the actual set_parameter IPC call via RAF coalescing
    scheduleSetParameter(displayValue);
  }

  return (
    <div
      class={styles.jointRow}
      data-testid={`joint-row-${joint().joint_index}`}
      data-kind={kind()}
    >
      <div class={styles.jointLabel}>
        <span class={styles.jointKind}>{kind()}</span>
        <span class={styles.jointIndex}>#{joint().joint_index}</span>
        <span class={styles.jointDimension}>({dimension()})</span>
      </div>

      <Show
        when={isScrubbable()}
        fallback={
          <div class={styles.jointReadOnly}>
            <Show
              when={kind() === 'coupling' || kind() === 'fixed'}
              fallback={
                <span class={styles.literalBoundBadge} title="Bound to a literal expression — edit source to scrub">
                  literal-bound
                </span>
              }
            >
              <span class={styles.noSliderBadge}>
                {kind() === 'coupling' ? 'coupling (derived)' : 'fixed (no motion)'}
              </span>
            </Show>
          </div>
        }
      >
        <input
          type="range"
          class={styles.jointSlider}
          min={minDisplay()}
          max={maxDisplay()}
          step={kind() === 'prismatic' ? 1 : 0.1}
          value={sliderValue()}
          onInput={handleInput}
          aria-label={`${kind()} #${joint().joint_index} slider`}
        />
        <span class={styles.sliderValue}>
          {kind() === 'prismatic'
            ? `${sliderValue().toFixed(1)} mm`
            : `${sliderValue().toFixed(1)}°`}
        </span>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// MechanismSection component
// ---------------------------------------------------------------------------

interface MechanismSectionProps {
  descriptor: MechanismDescriptor;
  onSetParameter: (cellId: string, value: string) => void;
  onScrubLocal: (cellId: string | null, jointIndex: number, valueSi: number) => void;
}

const MechanismSection: Component<MechanismSectionProps> = (props) => {
  return (
    <div class={styles.mechanismSection} data-testid={`mechanism-section-${props.descriptor.cell_id}`}>
      <div class={styles.mechanismHeader}>
        <span class={styles.mechanismName}>{props.descriptor.name}</span>
        <span class={styles.bodiesCount}>{props.descriptor.bodies_count} bodies</span>
      </div>
      <div class={styles.jointList}>
        <For each={props.descriptor.joints}>
          {(joint) => (
            <JointRow
              joint={joint}
              onSetParameter={props.onSetParameter}
              onScrubLocal={(_cellId, jointIndex, valueSi) =>
                props.onScrubLocal(props.descriptor.cell_id, jointIndex, valueSi)
              }
              mechanismCellId={props.descriptor.cell_id}
            />
          )}
        </For>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// MechanismPanel component (public)
// ---------------------------------------------------------------------------

export interface MechanismPanelProps {
  /** Mechanism descriptors from the backend (filtered: no errored mechanisms). */
  descriptors: MechanismDescriptor[];
  /** Called when a slider value is committed (RAF-coalesced). */
  onSetParameter: (cellId: string, value: string) => void;
  /**
   * Called on every slider input for optimistic UI updates.
   * `cellId` is the mechanism's cell_id (not the joint's driving param).
   */
  onScrubLocal: (cellId: string | null, jointIndex: number, valueSi: number) => void;
}

export const MechanismPanel: Component<MechanismPanelProps> = (props) => {
  return (
    <div class={styles.panel} data-testid="mechanism-panel">
      <Show
        when={props.descriptors.length > 0}
        fallback={
          <div class={styles.emptyState}>No mechanisms in scope</div>
        }
      >
        <For each={props.descriptors}>
          {(descriptor) => (
            <MechanismSection
              descriptor={descriptor}
              onSetParameter={props.onSetParameter}
              onScrubLocal={props.onScrubLocal}
            />
          )}
        </For>
      </Show>
    </div>
  );
};
