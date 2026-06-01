/**
 * FeaModeToolbar — collapsible top-right FEA overlay component.
 *
 * Props:
 *   store            — shared FeaModeStore (state + mutations)
 *   availableChannels — channel names to list in the channel dropdown;
 *                       defaults to ['vonMises', 'displacement_magnitude']
 *   onLockCurrent    — callback fired when the "Lock current" button is clicked;
 *                       the parent computes the actual min/max from the active mesh set.
 *   maxValue         — maximum value of the active scalar channel across all meshes;
 *                       computed by the parent from the active mesh set (ε, task 2962).
 *                       When null/undefined the readout row is hidden.
 */
import { Show, createSignal, For, type Component } from 'solid-js';
import type { FeaModeStore } from '../stores';
import type { Palette } from './colormap';

const PALETTE_OPTIONS: Palette[] = ['viridis', 'magma', 'rainbow'];
const DEFAULT_CHANNELS = ['vonMises', 'displacement_magnitude'];

const PALETTE_TITLE =
  'Viridis is perceptually uniform — colour distance ≈ value distance. ' +
  'Magma for hot-spot work. Rainbow available for engineering convention compatibility.';

export interface FeaModeToolbarProps {
  store: FeaModeStore;
  availableChannels?: string[];
  onLockCurrent?: () => void;
  /**
   * Maximum value of the active scalar channel across all meshes; passed from
   * the parent (Viewport) which computes it via computeScalarRange. When
   * null/undefined the max-readout row is hidden entirely.
   */
  maxValue?: number | null;
}

/**
 * Format a scalar channel value for display in the max readout.
 * - Values with |v| >= 1e4 or (0 < |v| < 1e-2) use exponential notation (2 sig figs).
 * - Other finite values use toPrecision(4) with trailing-zero stripping.
 */
function formatScalar(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1e4 || (abs > 0 && abs < 1e-2)) {
    return v.toExponential(2);
  }
  return String(Number(v.toPrecision(4)));
}

export const FeaModeToolbar: Component<FeaModeToolbarProps> = (props) => {
  const [collapsed, setCollapsed] = createSignal(false);

  const channels = () => props.availableChannels ?? DEFAULT_CHANNELS;

  function handleEnableChange(e: Event) {
    props.store.setEnabled((e.currentTarget as HTMLInputElement).checked);
  }

  function handleChannelChange(e: Event) {
    props.store.setChannel((e.currentTarget as HTMLSelectElement).value);
  }

  function handlePaletteChange(e: Event) {
    props.store.setPalette((e.currentTarget as HTMLSelectElement).value as Palette);
  }

  function handleRangeModeChange(mode: 'auto' | 'fixed' | 'locked') {
    const cur = props.store.state.range;
    if (mode === 'auto') {
      // Reset to sentinel defaults — the consumer (Viewport) is the source of truth
      // for auto bounds and must recompute them from the active scalar data.
      // Preserving the previous fixed/locked bounds here would bake stale values into
      // auto state that consumers read while range.mode === 'auto'.
      props.store.setRange({ mode: 'auto', min: 0, max: 1 });
    } else if (mode === 'fixed') {
      props.store.setRange({ mode: 'fixed', min: cur.min, max: cur.max });
    } else {
      // locked — preserve source if already locked, otherwise default 'current'
      const source = cur.mode === 'locked' ? cur.source : 'current';
      props.store.setRange({ mode: 'locked', min: cur.min, max: cur.max, source });
    }
  }

  function handleMinChange(e: Event) {
    const val = parseFloat((e.currentTarget as HTMLInputElement).value);
    if (!Number.isFinite(val)) return;
    const cur = props.store.state.range;
    if (cur.mode === 'fixed') {
      props.store.setRange({ mode: 'fixed', min: val, max: cur.max });
    } else if (cur.mode === 'locked') {
      props.store.setRange({ mode: 'locked', min: val, max: cur.max, source: cur.source });
    }
  }

  function handleMaxChange(e: Event) {
    const val = parseFloat((e.currentTarget as HTMLInputElement).value);
    if (!Number.isFinite(val)) return;
    const cur = props.store.state.range;
    if (cur.mode === 'fixed') {
      props.store.setRange({ mode: 'fixed', min: cur.min, max: val });
    } else if (cur.mode === 'locked') {
      props.store.setRange({ mode: 'locked', min: cur.min, max: val, source: cur.source });
    }
  }

  return (
    <div
      data-testid="fea-mode-toolbar"
      style={{
        position: 'absolute',
        top: '12px',
        right: '12px',
        background: 'rgba(30,30,40,0.88)',
        color: '#e0e0e0',
        'border-radius': '6px',
        padding: '8px 10px',
        'min-width': '180px',
        'z-index': 10,
        'font-size': '12px',
      }}
    >
      {/* Header row: collapse button + label + enable toggle */}
      <div style={{ display: 'flex', 'align-items': 'center', gap: '6px' }}>
        <button
          data-testid="fea-mode-collapse-toggle"
          onClick={() => setCollapsed((c) => !c)}
          style={{ background: 'none', border: 'none', color: 'inherit', cursor: 'pointer', 'font-size': '11px' }}
          aria-label={collapsed() ? 'Expand FEA panel' : 'Collapse FEA panel'}
        >
          {collapsed() ? '▶' : '▼'}
        </button>
        <span style={{ 'flex': 1, 'font-weight': 600 }}>FEA</span>
        <input
          type="checkbox"
          data-testid="fea-mode-enable-toggle"
          checked={props.store.state.enabled}
          onChange={handleEnableChange}
          aria-label="Enable FEA mode"
        />
      </div>

      {/* Body: channel / palette / range controls — visible when not collapsed AND enabled */}
      <Show when={!collapsed() && props.store.state.enabled}>
        <div style={{ 'margin-top': '8px', display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
          {/* Channel dropdown */}
          <label style={{ display: 'flex', 'flex-direction': 'column', gap: '2px' }}>
            <span>Channel</span>
            <select
              data-testid="fea-mode-channel-select"
              value={props.store.state.channel}
              onChange={handleChannelChange}
            >
              <For each={channels()}>
                {(ch) => <option value={ch}>{ch}</option>}
              </For>
            </select>
          </label>

          {/* Max readout — hidden when maxValue is null/undefined (no FEA data yet) */}
          <Show when={props.maxValue != null}>
            <div
              data-testid="fea-mode-max-readout"
              style={{ 'font-size': '11px', color: '#b0c4de' }}
            >
              max {props.store.state.channel}: {formatScalar(props.maxValue!)}
            </div>
          </Show>

          {/* Palette dropdown */}
          <label style={{ display: 'flex', 'flex-direction': 'column', gap: '2px' }}>
            <span>Palette</span>
            <select
              data-testid="fea-mode-palette-select"
              value={props.store.state.palette}
              onChange={handlePaletteChange}
              title={PALETTE_TITLE}
            >
              <For each={PALETTE_OPTIONS}>
                {(p) => <option value={p}>{p}</option>}
              </For>
            </select>
          </label>

          {/* Range-mode radio group */}
          <fieldset
            data-testid="fea-mode-range-mode"
            style={{ border: 'none', padding: 0, margin: 0 }}
          >
            <legend style={{ 'font-size': '11px', 'margin-bottom': '2px' }}>Range</legend>
            <For each={(['auto', 'fixed', 'locked'] as const)}>
              {(mode) => (
                <label style={{ display: 'inline-flex', 'align-items': 'center', gap: '3px', 'margin-right': '6px' }}>
                  <input
                    type="radio"
                    name="fea-mode-range-mode"
                    data-testid={`fea-mode-range-mode-${mode}`}
                    value={mode}
                    checked={props.store.state.range.mode === mode}
                    onChange={() => handleRangeModeChange(mode)}
                  />
                  {mode}
                </label>
              )}
            </For>
          </fieldset>

          {/* Min/max inputs — visible only when range mode is fixed or locked */}
          <Show when={props.store.state.range.mode !== 'auto'}>
            <label style={{ display: 'flex', 'align-items': 'center', gap: '4px' }}>
              <span>Min</span>
              <input
                type="number"
                data-testid="fea-mode-range-min"
                value={props.store.state.range.min}
                onInput={handleMinChange}
                style={{ width: '60px' }}
              />
            </label>
            <label style={{ display: 'flex', 'align-items': 'center', gap: '4px' }}>
              <span>Max</span>
              <input
                type="number"
                data-testid="fea-mode-range-max"
                value={props.store.state.range.max}
                onInput={handleMaxChange}
                style={{ width: '60px' }}
              />
            </label>
          </Show>

          {/* Lock current button */}
          <button
            data-testid="fea-mode-lock-current"
            onClick={() => props.onLockCurrent?.()}
            style={{ cursor: 'pointer', 'margin-top': '4px' }}
          >
            Lock current
          </button>

          {/* Deformed-shape view controls */}
          <label style={{ display: 'flex', 'align-items': 'center', gap: '6px', 'margin-top': '6px' }}>
            <input
              type="checkbox"
              data-testid="fea-mode-show-deformed-toggle"
              checked={props.store.state.showDeformed}
              onChange={(e) => props.store.setShowDeformed(e.currentTarget.checked)}
            />
            Show deformed
          </label>

          <Show when={props.store.state.showDeformed}>
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '4px' }}>
              <label style={{ display: 'flex', 'align-items': 'center', gap: '4px' }}>
                <span>Warp</span>
                <input
                  type="range"
                  data-testid="fea-mode-warp-slider"
                  min="0"
                  max="100"
                  step="0.1"
                  value={props.store.state.warpFactor}
                  onInput={(e) => {
                    const v = parseFloat(e.currentTarget.value);
                    if (Number.isFinite(v)) props.store.setWarpFactor(v);
                  }}
                  style={{ flex: 1 }}
                />
                <span>{props.store.state.warpFactor.toFixed(1)}×</span>
              </label>
              <div style={{ display: 'flex', gap: '4px' }}>
                <For each={[1, 10, 100] as const}>
                  {(v) => (
                    <button
                      data-testid={`fea-mode-warp-preset-${v}`}
                      onClick={() => props.store.setWarpFactor(v)}
                      style={{ cursor: 'pointer', padding: '2px 6px', 'font-size': '11px' }}
                    >
                      {v}×
                    </button>
                  )}
                </For>
              </div>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};
