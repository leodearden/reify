/**
 * FeaModeToolbar component tests — smoke-and-toggle suite (task 2961, step-15).
 *
 * Mirrors the Toolbar.test.tsx / ExportDialog.test.tsx pattern:
 * - render with @solidjs/testing-library
 * - query via screen.getByTestId / getByText
 * - drive interactions via fireEvent
 * - store created outside createRoot (same pattern as createSignal in ExportDialog tests)
 */
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createFeaModeStore } from '../../stores';
import { FeaModeToolbar } from '../../viewport/FeaModeToolbar';

describe('FeaModeToolbar — smoke-and-toggle suite', () => {
  it('(a) renders with data-testid="fea-mode-toolbar"', () => {
    const store = createFeaModeStore();
    render(() => <FeaModeToolbar store={store} />);
    expect(screen.getByTestId('fea-mode-toolbar')).toBeTruthy();
  });

  it('(b) contains a toggle with data-testid="fea-mode-enable-toggle"', () => {
    const store = createFeaModeStore();
    render(() => <FeaModeToolbar store={store} />);
    expect(screen.getByTestId('fea-mode-enable-toggle')).toBeTruthy();
  });

  it('(c) clicking toggle when enabled=false calls store.setEnabled(true) and body controls appear', () => {
    const store = createFeaModeStore();
    render(() => <FeaModeToolbar store={store} />);

    // Before toggle: enabled is false
    expect(store.state.enabled).toBe(false);
    // Body controls should not be rendered when disabled
    expect(screen.queryByTestId('fea-mode-channel-select')).toBeNull();

    // Click the toggle
    fireEvent.click(screen.getByTestId('fea-mode-enable-toggle'));

    // After toggle: store.state.enabled is true
    expect(store.state.enabled).toBe(true);
    // Body controls should now appear
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();
  });

  it('(d) body controls are NOT rendered when enabled is false', () => {
    const store = createFeaModeStore();
    // Ensure enabled is false (default)
    expect(store.state.enabled).toBe(false);

    render(() => <FeaModeToolbar store={store} />);

    // Channel select, palette select, and range controls should be absent
    expect(screen.queryByTestId('fea-mode-channel-select')).toBeNull();
    expect(screen.queryByTestId('fea-mode-palette-select')).toBeNull();
    expect(screen.queryByTestId('fea-mode-range-mode')).toBeNull();
  });
});

/** Helper: render toolbar with store pre-enabled so body controls are visible. */
function renderEnabled(overrides?: {
  availableChannels?: string[];
  onLockCurrent?: () => void;
  maxValue?: number | null;
  convergence?: { converged: boolean; reason?: string | null };
}) {
  const store = createFeaModeStore();
  store.setEnabled(true);
  render(() => <FeaModeToolbar store={store} {...overrides} />);
  return store;
}

describe('FeaModeToolbar — channel + palette dropdown suite', () => {
  it('(a) channel select is present when enabled', () => {
    renderEnabled();
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();
  });

  it('(a) channel select lists availableChannels when provided', () => {
    renderEnabled({ availableChannels: ['vonMises', 'displacement_magnitude', 'principal_stress'] });
    const select = screen.getByTestId('fea-mode-channel-select') as HTMLSelectElement;
    const options = Array.from(select.options).map((o) => o.value);
    expect(options).toEqual(['vonMises', 'displacement_magnitude', 'principal_stress']);
  });

  it('(a) channel select falls back to [vonMises, displacement_magnitude] when no availableChannels', () => {
    renderEnabled();
    const select = screen.getByTestId('fea-mode-channel-select') as HTMLSelectElement;
    const options = Array.from(select.options).map((o) => o.value);
    expect(options).toContain('vonMises');
    expect(options).toContain('displacement_magnitude');
    expect(options).toHaveLength(2);
  });

  it('(b) channel select value reflects store.state.channel', () => {
    const store = renderEnabled();
    // Default channel is 'vonMises'
    const select = screen.getByTestId('fea-mode-channel-select') as HTMLSelectElement;
    expect(select.value).toBe(store.state.channel);
    expect(select.value).toBe('vonMises');
  });

  it('(c) changing channel select updates store.state.channel', () => {
    const store = renderEnabled({ availableChannels: ['vonMises', 'displacement_magnitude'] });
    const select = screen.getByTestId('fea-mode-channel-select');
    fireEvent.change(select, { target: { value: 'displacement_magnitude' } });
    expect(store.state.channel).toBe('displacement_magnitude');
  });

  it('(d) palette select lists exactly viridis, magma, rainbow', () => {
    renderEnabled();
    const select = screen.getByTestId('fea-mode-palette-select') as HTMLSelectElement;
    const options = Array.from(select.options).map((o) => o.value);
    expect(options).toEqual(['viridis', 'magma', 'rainbow']);
  });

  it('(d) palette select defaults to viridis', () => {
    renderEnabled();
    const select = screen.getByTestId('fea-mode-palette-select') as HTMLSelectElement;
    expect(select.value).toBe('viridis');
  });

  it('(d) changing palette select calls store.setPalette', () => {
    const store = renderEnabled();
    const select = screen.getByTestId('fea-mode-palette-select');
    fireEvent.change(select, { target: { value: 'magma' } });
    expect(store.state.palette).toBe('magma');
  });

  it('(e) palette select has title attribute containing "perceptual"', () => {
    renderEnabled();
    const select = screen.getByTestId('fea-mode-palette-select');
    const title = select.getAttribute('title') ?? '';
    expect(title.toLowerCase()).toContain('perceptual');
  });
});

describe('FeaModeToolbar — range-mode suite', () => {
  it('(a) range-mode container is present when enabled', () => {
    renderEnabled();
    expect(screen.getByTestId('fea-mode-range-mode')).toBeTruthy();
  });

  it('(a) range-mode container has three radio inputs: auto, fixed, locked', () => {
    renderEnabled();
    expect(screen.getByTestId('fea-mode-range-mode-auto')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-range-mode-fixed')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-range-mode-locked')).toBeTruthy();
  });

  it('(b) selecting fixed radio calls store.setRange with mode fixed', () => {
    const store = renderEnabled();
    // Default range mode is auto
    expect(store.state.range.mode).toBe('auto');

    fireEvent.click(screen.getByTestId('fea-mode-range-mode-fixed'));

    expect(store.state.range.mode).toBe('fixed');
  });

  it('(c) when range.mode is auto, min/max number inputs are NOT rendered', () => {
    const store = renderEnabled();
    expect(store.state.range.mode).toBe('auto');

    expect(screen.queryByTestId('fea-mode-range-min')).toBeNull();
    expect(screen.queryByTestId('fea-mode-range-max')).toBeNull();
  });

  it('(d) when range.mode is fixed, min and max inputs are rendered with current values', () => {
    const store = renderEnabled();
    store.setRange({ mode: 'fixed', min: 5, max: 50 });

    expect(screen.getByTestId('fea-mode-range-min')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-range-max')).toBeTruthy();

    const minInput = screen.getByTestId('fea-mode-range-min') as HTMLInputElement;
    const maxInput = screen.getByTestId('fea-mode-range-max') as HTMLInputElement;
    expect(parseFloat(minInput.value)).toBe(5);
    expect(parseFloat(maxInput.value)).toBe(50);
  });

  it('(d) when range.mode is locked, min and max inputs are rendered', () => {
    const store = renderEnabled();
    store.lockCurrent(10, 100);

    expect(screen.getByTestId('fea-mode-range-min')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-range-max')).toBeTruthy();
  });

  it('(e) typing into min input calls store.setRange with new min', () => {
    const store = renderEnabled();
    store.setRange({ mode: 'fixed', min: 0, max: 100 });

    const minInput = screen.getByTestId('fea-mode-range-min');
    fireEvent.input(minInput, { target: { value: '25' } });

    expect(store.state.range.mode).toBe('fixed');
    expect((store.state.range as { min: number }).min).toBe(25);
  });

  it('(f) Lock current button is rendered when enabled and clicking calls onLockCurrent once', () => {
    const onLockCurrent = vi.fn();
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => <FeaModeToolbar store={store} onLockCurrent={onLockCurrent} />);

    const btn = screen.getByTestId('fea-mode-lock-current');
    expect(btn).toBeTruthy();
    fireEvent.click(btn);
    expect(onLockCurrent).toHaveBeenCalledTimes(1);
  });
});

describe('FeaModeToolbar — deformation controls', () => {
  it('(a) show-deformed toggle exists, is a checkbox, unchecked by default', () => {
    const store = renderEnabled();
    const toggle = screen.getByTestId('fea-mode-show-deformed-toggle') as HTMLInputElement;
    expect(toggle).toBeTruthy();
    expect(toggle.type).toBe('checkbox');
    expect(toggle.checked).toBe(false);
    expect(store.state.showDeformed).toBe(false);
  });

  it('(b) clicking show-deformed toggle sets store.state.showDeformed to true', () => {
    const store = renderEnabled();
    const toggle = screen.getByTestId('fea-mode-show-deformed-toggle');
    fireEvent.click(toggle);
    expect(store.state.showDeformed).toBe(true);
  });

  it('(c) warp slider is NOT rendered when showDeformed is false', () => {
    renderEnabled();
    expect(screen.queryByTestId('fea-mode-warp-slider')).toBeNull();
  });

  it('(c) warp slider IS rendered after store.setShowDeformed(true), value reflects warpFactor', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);

    const slider = screen.getByTestId('fea-mode-warp-slider') as HTMLInputElement;
    expect(slider).toBeTruthy();
    // Default warpFactor is 1.0
    expect(parseFloat(slider.value)).toBe(store.state.warpFactor);
    expect(parseFloat(slider.value)).toBeCloseTo(1.0, 5);
  });

  it('(d) sliding warp slider updates store.state.warpFactor', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);

    const slider = screen.getByTestId('fea-mode-warp-slider');
    fireEvent.input(slider, { target: { value: '25' } });

    expect(store.state.warpFactor).toBe(25);
  });

  // --- step-19: preset buttons ---

  it('(e) three preset buttons are rendered when showDeformed is true', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);

    const btn1 = screen.getByTestId('fea-mode-warp-preset-1');
    const btn10 = screen.getByTestId('fea-mode-warp-preset-10');
    const btn100 = screen.getByTestId('fea-mode-warp-preset-100');

    expect(btn1.tagName.toLowerCase()).toBe('button');
    expect(btn10.tagName.toLowerCase()).toBe('button');
    expect(btn100.tagName.toLowerCase()).toBe('button');
  });

  it('(e) preset buttons are NOT rendered when showDeformed is false', () => {
    renderEnabled();
    expect(screen.queryByTestId('fea-mode-warp-preset-1')).toBeNull();
    expect(screen.queryByTestId('fea-mode-warp-preset-10')).toBeNull();
    expect(screen.queryByTestId('fea-mode-warp-preset-100')).toBeNull();
  });

  it('(e) clicking preset-10 sets store.state.warpFactor to 10', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);
    fireEvent.click(screen.getByTestId('fea-mode-warp-preset-10'));
    expect(store.state.warpFactor).toBe(10);
  });

  it('(e) clicking preset-100 sets store.state.warpFactor to 100', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);
    fireEvent.click(screen.getByTestId('fea-mode-warp-preset-100'));
    expect(store.state.warpFactor).toBe(100);
  });

  it('(e) clicking preset-1 resets store.state.warpFactor to 1', () => {
    const store = renderEnabled();
    store.setShowDeformed(true);
    store.setWarpFactor(50);
    fireEvent.click(screen.getByTestId('fea-mode-warp-preset-1'));
    expect(store.state.warpFactor).toBe(1);
  });
});

describe('FeaModeToolbar — collapsible suite', () => {
  it('(a) collapse toggle button is always rendered', () => {
    const store = createFeaModeStore();
    // Even with enabled=false (default), collapse toggle is present
    render(() => <FeaModeToolbar store={store} />);
    expect(screen.getByTestId('fea-mode-collapse-toggle')).toBeTruthy();
  });

  it('(b) initial collapsed state is false — body visible when enabled', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => <FeaModeToolbar store={store} />);
    // Body is visible (not collapsed)
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();
  });

  it('(c) clicking collapse toggle hides body even when enabled is true', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => <FeaModeToolbar store={store} />);

    // Body is visible initially
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();

    // Collapse
    fireEvent.click(screen.getByTestId('fea-mode-collapse-toggle'));

    // Body should be hidden
    expect(screen.queryByTestId('fea-mode-channel-select')).toBeNull();
  });

  it('(d) clicking collapse toggle again restores the body', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => <FeaModeToolbar store={store} />);

    // Collapse then expand
    fireEvent.click(screen.getByTestId('fea-mode-collapse-toggle'));
    expect(screen.queryByTestId('fea-mode-channel-select')).toBeNull();

    fireEvent.click(screen.getByTestId('fea-mode-collapse-toggle'));
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();
  });

  it('(e) collapse state is local — collapsing one instance does not affect another sharing the same store', () => {
    // Both toolbars share a single store — proves collapsed signal is NOT in the store.
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => (
      <div>
        <FeaModeToolbar store={store} />
        <FeaModeToolbar store={store} />
      </div>
    ));

    // Both instances should be uncollapsed initially — two channel selects visible
    const selectsBefore = screen.getAllByTestId('fea-mode-channel-select');
    expect(selectsBefore.length).toBe(2);

    // Collapse the first instance only (first toggle button)
    const toggles = screen.getAllByTestId('fea-mode-collapse-toggle');
    fireEvent.click(toggles[0]);

    // After collapsing the first, only one channel select should remain (the second instance)
    const selectsAfter = screen.getAllByTestId('fea-mode-channel-select');
    expect(selectsAfter.length).toBe(1);
  });
});

describe('FeaModeToolbar — case picker (task 3026 step-11)', () => {
  it('(a) case picker is present when availableCases is non-empty and FEA enabled', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    store.applyFeaCaseChanged({
      active_case_id: 'operating',
      available_cases: ['operating', 'overload', 'transport'],
    });
    render(() => <FeaModeToolbar store={store} />);

    // FeaCasePickerDropdown should be mounted (its Show guard passes with 3 cases)
    expect(screen.getByTestId('fea-case-picker-dropdown')).toBeTruthy();
  });

  it('(b) case picker is absent when availableCases is empty (single-case scene)', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    // availableCases defaults to [] — single-case scene, dropdown hidden
    render(() => <FeaModeToolbar store={store} />);

    expect(screen.queryByTestId('fea-case-picker-dropdown')).toBeNull();
  });

  it('(c) case picker is absent when FEA is disabled (body not rendered)', () => {
    const store = createFeaModeStore();
    // enabled=false, but cases present — picker must be hidden with the body
    store.applyFeaCaseChanged({
      active_case_id: 'operating',
      available_cases: ['operating', 'overload', 'transport'],
    });
    render(() => <FeaModeToolbar store={store} />);

    expect(screen.queryByTestId('fea-case-picker-dropdown')).toBeNull();
  });

  it('(d) existing toolbar controls are still present alongside the case picker', () => {
    const store = createFeaModeStore();
    store.setEnabled(true);
    store.applyFeaCaseChanged({
      active_case_id: 'operating',
      available_cases: ['operating', 'overload', 'transport'],
    });
    render(() => <FeaModeToolbar store={store} />);

    // Pre-existing controls must remain unaffected
    expect(screen.getByTestId('fea-mode-channel-select')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-palette-select')).toBeTruthy();
    expect(screen.getByTestId('fea-mode-range-mode')).toBeTruthy();
    // And the new case picker is also present
    expect(screen.getByTestId('fea-case-picker-dropdown')).toBeTruthy();
  });
});

describe('FeaModeToolbar — max readout (step-3 RED)', () => {
  it('(a) passing maxValue renders data-testid="fea-mode-max-readout" with formatted value and channel label', () => {
    // Default channel is 'vonMises'; readout should show the channel name and the value.
    renderEnabled({ maxValue: 6543210.5 });
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout).toBeTruthy();
    // Should mention the active channel name
    expect(readout.textContent).toMatch(/vonMises/);
    // Should mention the numeric value in some formatted form
    expect(readout.textContent).toMatch(/6\.54e\+6|6\.543e\+6|6.54/);
  });

  it('(b) maxValue=undefined → fea-mode-max-readout is NOT in the DOM', () => {
    renderEnabled({ maxValue: undefined });
    expect(screen.queryByTestId('fea-mode-max-readout')).toBeNull();
  });

  it('(b2) maxValue=null → fea-mode-max-readout is NOT in the DOM', () => {
    renderEnabled({ maxValue: null });
    expect(screen.queryByTestId('fea-mode-max-readout')).toBeNull();
  });

  it('(c) readout is NOT rendered when the store is disabled (body hidden)', () => {
    // When enabled=false the whole body is hidden — readout must not leak through
    const store = createFeaModeStore();
    // enabled defaults to false
    render(() => <FeaModeToolbar store={store} maxValue={12345} />);
    expect(screen.queryByTestId('fea-mode-max-readout')).toBeNull();
  });

  it('(d) readout reflects the active channel name when channel is changed', () => {
    const store = renderEnabled({ maxValue: 0.0025 });
    // Default channel = 'vonMises'; change to 'displacement_magnitude'
    store.setChannel('displacement_magnitude');
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout.textContent).toMatch(/displacement_magnitude/);
  });

  it('(a2) small value below 1e-2 is rendered in exponential notation', () => {
    renderEnabled({ maxValue: 0.0025 });
    const readout = screen.getByTestId('fea-mode-max-readout');
    // 0.0025 should render in exponential notation (abs < 1e-2)
    expect(readout.textContent).toMatch(/e/i);
  });

  it('(a3) ordinary value renders without exponential', () => {
    renderEnabled({ maxValue: 123.456 });
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout.textContent).toMatch(/123/);
  });
});

describe('FeaModeToolbar \u2014 max-readout unit label (task #6185)', () => {
  /** Render with the store pre-enabled so the readout row is visible. */
  function renderWithUnit(props: { maxValue?: number | null; unitLabel?: string | null }) {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => <FeaModeToolbar store={store} {...props} />);
    return store;
  }

  it('(a) appends the unit label after the formatted value', () => {
    renderWithUnit({ maxValue: 1.234e6, unitLabel: 'Pa' });
    const readout = screen.getByTestId('fea-mode-max-readout');
    // The composed string, not just "contains Pa" \u2014 pins channel, value and
    // unit in the right order with the right separators.
    expect(readout.textContent).toMatch(/max vonMises: 1\.23e\+6 Pa/);
  });

  it('(b) renders byte-identically to today when unitLabel is omitted', () => {
    renderWithUnit({ maxValue: 1.234e6 });
    const readout = screen.getByTestId('fea-mode-max-readout');
    // No trailing space, and certainly no "undefined"/"null" leaking into the UI.
    expect(readout.textContent).toBe('max vonMises: 1.23e+6');
  });

  it('(c) renders byte-identically to today when unitLabel is null', () => {
    // channelUnit returns null for untagged channels, so this is the common path.
    renderWithUnit({ maxValue: 1.234e6, unitLabel: null });
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout.textContent).toBe('max vonMises: 1.23e+6');
  });

  it('(d) keeps the readout hidden when maxValue is null, unit label or not', () => {
    renderWithUnit({ maxValue: null, unitLabel: 'Pa' });
    expect(screen.queryByTestId('fea-mode-max-readout')).toBeNull();
  });

  it('(e) renders a NEGATIVE max alongside its unit \u2014 the signed-channel legend', () => {
    renderWithUnit({ maxValue: -0.25, unitLabel: 'rad' });
    const readout = screen.getByTestId('fea-mode-max-readout');
    expect(readout.textContent).toMatch(/-0\.25/);
    expect(readout.textContent).toMatch(/rad/);
  });
});

describe('FeaModeToolbar — convergence badge (task 3001 step-13)', () => {
  it('(a) convergence={converged:false, reason:"MaxDofs"} renders a badge containing "MaxDofs"', () => {
    renderEnabled({ convergence: { converged: false, reason: 'MaxDofs' } });
    const badge = screen.getByTestId('fea-mode-convergence-badge');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toContain('MaxDofs');
  });

  it('(b) convergence={converged:true} renders NO badge', () => {
    renderEnabled({ convergence: { converged: true } });
    expect(screen.queryByTestId('fea-mode-convergence-badge')).toBeNull();
  });

  it('(c) omitting the convergence prop renders NO badge', () => {
    renderEnabled();
    expect(screen.queryByTestId('fea-mode-convergence-badge')).toBeNull();
  });

  it('(d) badge is NOT rendered when the store is disabled (body hidden)', () => {
    const store = createFeaModeStore();
    // enabled defaults to false
    render(() => (
      <FeaModeToolbar store={store} convergence={{ converged: false, reason: 'MaxDofs' }} />
    ));
    expect(screen.queryByTestId('fea-mode-convergence-badge')).toBeNull();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// FeaModeToolbar viewport identity (#5670)
//
// Once every pane owns a keyed FeaModeStore, every pane renders its own
// toolbar — so N toolbars stop being N duplicates driving one state and become
// N legitimate per-pane controls. What they still lack is *addressability*: the
// debug bridge resolves the channel select by a document-wide
// querySelectorAll('[data-testid="fea-mode-channel-select"]') and hard-fails on
// more than one match. Stamping data-viewport-id makes each one individually
// targetable, which is what set_fea_channel's new viewportId param selects on.
// ─────────────────────────────────────────────────────────────────────────────
describe('FeaModeToolbar viewport identity (#5670)', () => {
  /** Same shape as renderEnabled, plus the new optional id. */
  function renderEnabledWithId(viewportId?: string) {
    const store = createFeaModeStore();
    store.setEnabled(true); // the channel select is gated behind store.state.enabled
    render(() => <FeaModeToolbar store={store} viewportId={viewportId} />);
    return store;
  }

  it('(a) viewportId stamps data-viewport-id on the root AND on the channel select', () => {
    renderEnabledWithId('pane-1');
    // The root carries it for query_selector-style DOM inspection...
    expect(screen.getByTestId('fea-mode-toolbar').getAttribute('data-viewport-id')).toBe('pane-1');
    // ...and the select carries it because that is the element the bridge
    // resolves and drives; keying the store lookup off the element actually
    // written to is what prevents reading some other pane's store.
    expect(screen.getByTestId('fea-mode-channel-select').getAttribute('data-viewport-id')).toBe(
      'pane-1',
    );
  });

  it('(b) the attribute tracks the prop rather than being hard-coded to design-main', () => {
    renderEnabledWithId('design-main');
    expect(screen.getByTestId('fea-mode-toolbar').getAttribute('data-viewport-id')).toBe(
      'design-main',
    );
    expect(screen.getByTestId('fea-mode-channel-select').getAttribute('data-viewport-id')).toBe(
      'design-main',
    );
  });

  it('(c) omitting viewportId emits NO data-viewport-id attribute at all', () => {
    renderEnabledWithId(undefined);
    // Solid omits an attribute whose value is undefined, so an id-less toolbar
    // renders byte-identically to before #5670. That is what keeps the eight
    // existing describes above — and debugBridge.test.tsx's
    // renderToolbarWithErrorIndicator, which drives the legacy no-id fallback
    // path — valid without a single edit.
    expect(screen.getByTestId('fea-mode-toolbar').hasAttribute('data-viewport-id')).toBe(false);
    expect(screen.getByTestId('fea-mode-channel-select').hasAttribute('data-viewport-id')).toBe(
      false,
    );
  });
});
