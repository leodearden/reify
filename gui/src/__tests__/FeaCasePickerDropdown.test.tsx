/**
 * FeaCasePickerDropdown component tests (task 3545, step-11/step-12).
 *
 * Mirrors the FeaModeToolbar.test.tsx pattern:
 * - render with @solidjs/testing-library
 * - query via screen.getByTestId / queryByTestId
 * - drive interactions via fireEvent.change
 * - store created via createFeaModeStore()
 */
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createFeaModeStore } from '../stores';
import { FeaCasePickerDropdown } from '../panels/FeaCasePickerDropdown';

describe('FeaCasePickerDropdown', () => {
  it('(a) renders nothing when availableCases is empty', () => {
    const store = createFeaModeStore();
    render(() => <FeaCasePickerDropdown store={store} availableCases={[]} />);
    expect(screen.queryByTestId('fea-case-picker-dropdown')).toBeNull();
  });

  it('(b) renders a select with one option per case name when availableCases is non-empty', () => {
    const store = createFeaModeStore();
    render(() => (
      <FeaCasePickerDropdown
        store={store}
        availableCases={['operating', 'overload', 'transport']}
      />
    ));
    const select = screen.getByTestId('fea-case-picker-dropdown');
    expect(select).toBeTruthy();
    const options = select.querySelectorAll('option');
    expect(options.length).toBe(3);
    expect(options[0].value).toBe('operating');
    expect(options[1].value).toBe('overload');
    expect(options[2].value).toBe('transport');
  });

  it("(c) select value reflects store.state.activeCaseId when set to 'overload'", () => {
    const store = createFeaModeStore();
    store.setActiveCaseId('overload');
    render(() => (
      <FeaCasePickerDropdown
        store={store}
        availableCases={['operating', 'overload', 'transport']}
      />
    ));
    const select = screen.getByTestId('fea-case-picker-dropdown') as HTMLSelectElement;
    expect(select.value).toBe('overload');
  });

  it('(d) fireEvent.change updates store.state.activeCaseId', () => {
    const store = createFeaModeStore();
    render(() => (
      <FeaCasePickerDropdown
        store={store}
        availableCases={['operating', 'overload', 'transport']}
      />
    ));
    const select = screen.getByTestId('fea-case-picker-dropdown');
    fireEvent.change(select, { target: { value: 'overload' } });
    expect(store.state.activeCaseId).toBe('overload');
  });

  it('(e) when activeCaseId is null, select defaults to first available case and store is initialized', () => {
    const store = createFeaModeStore();
    // activeCaseId defaults to null
    expect(store.state.activeCaseId).toBeNull();
    render(() => (
      <FeaCasePickerDropdown
        store={store}
        availableCases={['operating', 'overload', 'transport']}
      />
    ));
    const select = screen.getByTestId('fea-case-picker-dropdown') as HTMLSelectElement;
    // createEffect syncs store to availableCases[0] on mount; select and store agree
    expect(select.value).toBe('operating');
    expect(store.state.activeCaseId).toBe('operating');
  });
});
