import { type Component, For, Show, createSignal, createEffect } from 'solid-js';
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
// Inverse display→SI helpers (used by onScrubLocal so mechanismStore
// receives SI values and its equality check with current_value_si fires)
// ---------------------------------------------------------------------------

/** Convert millimetres to SI metres. */
function mmToM(display: number): number {
  return display / 1000;
}

/** Convert degrees to SI radians. */
function degToRad(display: number): number {
  return display * (Math.PI / 180);
}

/**
 * Convert a slider display-unit value back to SI for `onScrubLocal`.
 *
 * @param displayValue - Value in display units (mm for prismatic, deg for revolute).
 * @param kind         - Joint kind string.
 * @returns SI value: metres for prismatic, radians for revolute, identity otherwise.
 *
 * Contract: the returned value uses the same units as `JointDescriptor.current_value_si`,
 * enabling `mechanismStore.refresh()` to clear optimistic overrides via equality check.
 */
function displayToSi(displayValue: number, kind: string): number {
  if (kind === 'prismatic') return mmToM(displayValue);
  if (kind === 'revolute') return degToRad(displayValue);
  return displayValue;
}

// ---------------------------------------------------------------------------
// Binding-aware helpers (shared between JointRow and MechanismSection)
// ---------------------------------------------------------------------------

/**
 * Return the authoritative current-SI value for a joint using its `binding`
 * field (Task 3788 η-frontend).
 *
 * - `param_bound` → `binding.current_value_si` (falls back to legacy field)
 * - `literal_bound` → `binding.initial_value_si`  (the AST literal baseline)
 * - `coupling_derived` / `fixed_no_motion` → `null`
 */
function jointCurrentSi(joint: { binding: import('../types').JointBinding; current_value_si: number | null }): number | null {
  const b = joint.binding;
  if (b.kind === 'param_bound') return b.current_value_si ?? joint.current_value_si;
  if (b.kind === 'literal_bound') return b.initial_value_si;
  return null;
}

// ---------------------------------------------------------------------------
// JointRow component
// ---------------------------------------------------------------------------

interface JointRowProps {
  joint: JointDescriptor;
  onSetParameter: (cellId: string, value: string) => void;
  onScrubLocal: (cellId: string | null, jointIndex: number, valueSi: number) => void;
  mechanismCellId: string;
  /**
   * Returns the effective SI value for this joint (optimistic override from
   * mechanismStore, falling back to joint.current_value_si when no override is
   * active).  When provided, the slider syncs to this value reactively so that
   * external changes (e.g. PropertyEditor set_parameter, MCP scrub) are
   * reflected without the user having to reload the panel.
   *
   * When omitted (e.g. in unit tests), the slider retains its local signal and
   * only updates on mount.
   */
  effectiveValueSi?: () => number | null;
}

const JointRow: Component<JointRowProps> = (props) => {
  const joint = () => props.joint;
  const kind = () => joint().kind;
  const dimension = () => joint().dimension;
  const drivingParam = () => joint().driving_param_cell_id;

  /**
   * Binding-aware param-cell-id resolver.
   * - param_bound → binding.param_cell_id (falls back to legacy driving_param_cell_id)
   * - literal_bound → binding.synth_param_name (the engine-session virtual param)
   * - coupling_derived / fixed_no_motion → null (not scrubbable)
   *
   * This is the id passed to onSetParameter and used as the first arg to
   * onScrubLocal; the RAF-coalesced set_parameter IPC reuses it unchanged.
   */
  const effectiveParamCellId = (): string | null => {
    const b = joint().binding;
    if (b.kind === 'param_bound') return b.param_cell_id ?? drivingParam();
    if (b.kind === 'literal_bound') return b.synth_param_name;
    return null;
  };

  // Whether this joint supports scrubbing.
  // Prismatic/revolute joints with a param binding OR a scrubbable literal binding
  // are scrubbable; coupling and fixed joints are never scrubbable.
  const isScrubbable = () => {
    if (kind() !== 'prismatic' && kind() !== 'revolute') return false;
    const b = joint().binding;
    if (b.kind === 'param_bound') return b.param_cell_id !== null;
    if (b.kind === 'literal_bound') return b.scrubbable === true;
    return false;
  };

  /**
   * Binding-aware current-SI value helper.
   * - param_bound → binding.current_value_si ?? legacy current_value_si
   * - literal_bound → binding.initial_value_si  (the AST literal baseline)
   * - else → null
   *
   * Used for initialDisplay() and as the fallback for effectiveValueSi
   * so that a literal-bound joint initializes from its literal baseline,
   * not the null legacy current_value_si field.
   */
  const bindingCurrentSi = (): number | null => {
    const b = joint().binding;
    if (b.kind === 'param_bound') return b.current_value_si ?? joint().current_value_si;
    if (b.kind === 'literal_bound') return b.initial_value_si;
    return null;
  };

  // Display-unit range
  const minDisplay = () => siToDisplay(joint().range_lower_si, kind()) ?? 0;
  const maxDisplay = () => siToDisplay(joint().range_upper_si, kind()) ?? 100;

  // Initial slider value from the binding-aware current SI.
  const initialDisplay = () => {
    const disp = currentSiToDisplay(bindingCurrentSi(), kind());
    return disp ?? minDisplay();
  };

  const [sliderValue, setSliderValue] = createSignal(initialDisplay());

  // Sync slider with external state changes (e.g. PropertyEditor edits,
  // MCP set_parameter, or mechanismStore optimistic-override updates).
  // When effectiveValueSi is provided it is a reactive accessor — the effect
  // re-runs whenever its value changes.  When the accessor is absent (tests)
  // the effect only runs once on mount and never again.
  createEffect(() => {
    const eff = props.effectiveValueSi?.() ?? bindingCurrentSi();
    if (eff === null || eff === undefined) return;
    const disp = siToDisplay(eff, kind());
    if (disp !== null) setSliderValue(disp);
  });

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
        const param = effectiveParamCellId();
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

    // Convert display value → SI before notifying the store so that
    // mechanismStore.setOptimistic receives the same units as
    // JointDescriptor.current_value_si, enabling refresh()'s equality
    // check to clear the override once the backend confirms the value.
    const valueSi = displayToSi(displayValue, kind());
    const param = effectiveParamCellId();
    props.onScrubLocal(param, joint().joint_index, valueSi);

    // Schedule the actual set_parameter IPC call via RAF coalescing
    // (display units are correct here — the backend parser reads "400mm" / "90deg")
    scheduleSetParameter(displayValue);
  }

  // Compute the data-binding marker for testability and visual distinction.
  const bindingMarker = (): 'literal' | 'param' | undefined => {
    const b = joint().binding;
    if (b.kind === 'literal_bound') return 'literal';
    if (b.kind === 'param_bound') return 'param';
    return undefined;
  };

  const isLiteralBound = () => joint().binding.kind === 'literal_bound';

  return (
    <div
      class={styles.jointRow}
      data-testid={`joint-row-${joint().joint_index}`}
      data-kind={kind()}
      data-binding={bindingMarker()}
    >
      <div class={styles.jointLabel}>
        <span class={styles.jointKind}>{kind()}</span>
        <span class={styles.jointIndex}>#{joint().joint_index}</span>
        <span class={styles.jointDimension}>({dimension()})</span>
        <Show when={isLiteralBound()}>
          <span class={styles.literalBoundIcon} title="Literal-bound — scrub is session-only">~</span>
        </Show>
      </div>

      <Show
        when={isScrubbable()}
        fallback={
          <div class={styles.jointReadOnly}>
            <span class={styles.noSliderBadge}>
              {kind() === 'coupling' ? 'coupling (derived)' : 'fixed (no motion)'}
            </span>
          </div>
        }
      >
        <input
          type="range"
          class={`${styles.jointSlider}${isLiteralBound() ? ` ${styles.literalBoundSlider}` : ''}`}
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
  getEffectiveValueSi: (cellId: string, jointIndex: number, fallback: number | null) => number | null;
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
              effectiveValueSi={() =>
                props.getEffectiveValueSi(
                  props.descriptor.cell_id,
                  joint.joint_index,
                  joint.current_value_si,
                )
              }
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
  /**
   * Mechanism descriptors from the backend (filtered: no errored mechanisms).
   *
   * NOTE: The backend returns one descriptor per mechanism *cell*, including
   * intermediate builder results (e.g. m0, m1, m2 from a `body()` chain).
   * All non-errored cells are rendered here.  Consumers that want to display
   * only the "final" (largest) mechanism should pre-filter this array.
   */
  descriptors: MechanismDescriptor[];
  /** Called when a slider value is committed (RAF-coalesced). */
  onSetParameter: (cellId: string, value: string) => void;
  /**
   * Called on every slider input for optimistic UI updates.
   *
   * `cellId` is the mechanism's cell_id (not the joint's driving param).
   * `valueSi` is in SI units (m for prismatic, rad for revolute) and matches
   * `JointDescriptor.current_value_si`'s units, so that `mechanismStore.refresh()`
   * can clear the optimistic override once the backend confirms the parameter value.
   */
  onScrubLocal: (cellId: string | null, jointIndex: number, valueSi: number) => void;
  /**
   * Optional: returns the effective SI value for a joint, preferring an
   * optimistic override (in-flight scrub) over the descriptor's
   * `current_value_si`.  Provide `mechanismStore.getEffectiveValueSi` in
   * production so that the slider stays in sync with external parameter
   * changes (PropertyEditor, MCP) and with the optimistic-override store.
   *
   * When omitted, JointRow only uses its local `createSignal` — instant
   * feedback on user input still works, but external changes won't be
   * reflected until the descriptor object itself is replaced.
   */
  getEffectiveValueSi?: (cellId: string, jointIndex: number, fallback: number | null) => number | null;
}

export const MechanismPanel: Component<MechanismPanelProps> = (props) => {
  // Default: identity fallback (no optimistic store wired).
  const effectiveFn = (cellId: string, jointIndex: number, fallback: number | null) =>
    props.getEffectiveValueSi
      ? props.getEffectiveValueSi(cellId, jointIndex, fallback)
      : fallback;

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
              getEffectiveValueSi={effectiveFn}
            />
          )}
        </For>
      </Show>
    </div>
  );
};
