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
function renderEnabled(overrides?: { availableChannels?: string[]; onLockCurrent?: () => void }) {
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
