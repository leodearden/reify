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
