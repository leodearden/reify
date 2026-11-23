import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import type { GuiState } from '../types';
import { flushMacrotasks, deferred, withSuppressedRejectionsAndErrorSpy } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStubModule());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStubModule());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStubModule());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import './appHarness/harness';
import { renderAndWaitForReady } from './appHarness/harness';

describe('App handleSetParameter error handling', () => {
  it('shows error toast when bridge.setParameter rejects', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('backend unavailable'));

      vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
        values: [{
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
          kind: 'parameter',
          freshness: 'final',
        }],
      })));

      render(() => <App />);

      // Wait for PropertyEditor to show the value
      await waitFor(() => {
        expect(screen.getByText('width')).toBeTruthy();
      });

      // Find the input and press Enter to trigger onSetParameter
      const row = screen.getByTestId('prop-row-c1');
      const input = row.querySelector('input[type="text"]') as HTMLInputElement;
      expect(input).toBeTruthy();

      fireEvent.keyDown(input, { key: 'Enter' });

      // Wait for the error toast to appear
      await waitFor(() => {
        const toast = screen.getByTestId('toast');
        expect(toast).toBeTruthy();
        expect(toast.dataset.type).toBe('error');
        expect(toast.textContent).toContain('Parameter update failed');
        expect(toast.textContent).toContain('backend unavailable');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'setParameter failed:',
        expect.any(Error),
      );
    });
  });
});

/**
 * CONTRACT PIN — the reconciled parameter-input contract (task #5757).
 *
 * This block was the task-#6028 CHARACTERIZATION of a deliberately accepted
 * degradation: the typed-input gate admitted every curated ladder label, while
 * the commit path parsed only a hard-coded five-entry suffix table, so
 * `5mm^3` and `5L` were accepted by the panel and then refused by the engine
 * with `Cannot parse value '<input>'` — the typed text discarded and replaced
 * by an async error toast. Its clause (d) was named in-source as "the clause
 * the eventual UNIT_TABLE fix must flip". Task #5757 is that fix, so the block
 * is rewritten here to state the contract that now HOLDS rather than the gap
 * that used to.
 *
 * What changed underneath: `parse_value_string` (gui/src-tauri/src/engine.rs)
 * no longer carries its own table. It scans an index composed from
 * `reify_core::display_units::unit_ladders()` — the very table this panel's
 * accept-alphabet is derived from, fetched over `get_unit_ladders` — unioned
 * with `reify_core::BUILTIN_UNITS`. Advertised and parseable are now the same
 * set by construction, not by two tables kept in lockstep.
 *
 * The block pins BOTH directions of the reconciled contract, because widening
 * what is accepted and narrowing it are the same edit and each can break the
 * other:
 *
 *   * a curated ladder unit commits cleanly, with no toast;
 *   * a BARE number in a dimensioned cell is refused INLINE and never reaches
 *     the bridge at all (PRD ratified decision (1), §6 row 16).
 *
 * ON CLAUSE (d), read carefully. The original asserted that the typed text was
 * GONE after commit, and called that the regression to flip. It cannot flip
 * literally: `PropertyEditor`'s input reads
 * `editingCellId() === cell_id ? editValue() : primaryDisplay(val)`, so text is
 * held on screen for exactly as long as the row is still being edited. A
 * SUCCESSFUL commit ends the edit, and the panel then correctly shows what the
 * engine says the cell is — the mocked bridge here does not push a new value
 * back, so that is still `7045002.24`. The same assertion therefore reads the
 * same but MEANS the opposite: the text is gone because the value landed, not
 * because a rejection threw it away. What the original clause was really
 * protecting — typed text preserved for correction — is asserted where it now
 * genuinely belongs, on the bare-number case below, which is refused and does
 * keep the text.
 *
 * Mocking discipline is unchanged (see the standing rules in
 * bridgeMockCoverage.test.ts): no conversion to a partial mock, per-test overrides via `vi.mocked`,
 * and the `waitFor` on the unit picker stays the synchronization point because
 * the ladder fetch is fire-and-forget.
 */
describe('App parameter input: the reconciled unit/bare-number contract (task #5757)', () => {
  /** The task #5199 Volume-ladder cell, rebuilt per test so no case can mutate another's fixture. */
  function capacityState(): GuiState {
    return makeGuiState({
      values: [
        {
          cell_id: 'Tank.capacity',
          name: 'capacity',
          value: '7045002.24',
          unit: 'mm³',
          determinacy: 'determined',
          entity_path: 'Tank.capacity',
          kind: 'let',
          freshness: 'final',
          dimension: 'Volume',
          si_value: 0.00704500224,
          declared_dimension: 'Volume',
        },
      ],
    });
  }

  /**
   * Render <App /> over the Volume cell and hand back its input, ready to type
   * into.
   *
   * The `waitFor` on the picker is the synchronization point: the ladder fetch
   * is fire-and-forget (App.tsx), and the picker only renders once it resolves —
   * so waiting on it also proves the widened, ladder-derived alphabet is live
   * here rather than the static five-unit floor.
   */
  async function renderCapacityInput(): Promise<HTMLInputElement> {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(capacityState()));
    vi.mocked((bridge as any).getUnitLadders).mockResolvedValue([
      {
        dimension: 'Volume',
        units: [
          { label: 'mm³', si_scale: 1e-9, is_default: true },
          { label: 'L', si_scale: 1e-3, is_default: false },
        ],
      },
    ]);

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('unit-select-Tank.capacity')).toBeTruthy();
    });

    const row = screen.getByTestId('prop-row-Tank.capacity');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.value).toBe('7045002.24');
    return input;
  }

  // `mm^3` is the ASCII spelling of the ladder's own default rung; `L` is the
  // second rung. Both are advertised by `get_unit_ladders`, and since #5757 the
  // engine resolves both — `L` notably being reachable ONLY through the display
  // ladders, since reify-compiler/stdlib/units.ri declares no SI volume units.
  it.each([
    ['5mm^3'],
    ['5L'],
  ])('submits %s to the engine cleanly, with no inline rejection and no error toast', async (typed) => {
    await withSuppressedRejectionsAndErrorSpy(async () => {
      // RESOLVES. The pre-#5757 version of this test hand-mocked a
      // `Cannot parse value '<input>'` rejection here — a failure the engine no
      // longer produces for these literals.
      vi.mocked((bridge as any).setParameter).mockResolvedValue(undefined);

      const input = await renderCapacityInput();

      fireEvent.focus(input);
      fireEvent.input(input, { target: { value: typed } });
      fireEvent.keyDown(input, { key: 'Enter' });

      // (a) the frontend gate accepted it — no inline invalid marker.
      expect(input.hasAttribute('data-invalid')).toBe(false);

      // (b) it was submitted to the backend verbatim: the panel never rewrites
      // a literal by the picked unit's conversion factor.
      expect(bridge.setParameter).toHaveBeenCalledWith('Tank.capacity', typed);

      // (c) THE FLIP. No error toast arrives, because nothing rejects.
      await flushMacrotasks();
      expect(screen.queryByTestId('toast')).toBeNull();

      // (d) the edit ended and the row reverted to the engine-derived display
      // value. Reverting is now the CORRECT outcome — the value landed. See the
      // block doc above for why this assertion reads the same as the one it
      // replaces while meaning the opposite.
      expect(input.value).toBe('7045002.24');
    });
  });

  it('refuses a bare number on a dimensioned cell inline, never calling the bridge', async () => {
    await withSuppressedRejectionsAndErrorSpy(async () => {
      vi.mocked((bridge as any).setParameter).mockResolvedValue(undefined);

      const input = await renderCapacityInput();

      // `20` in a Volume cell is ambiguous — 20 what? The engine used to answer
      // that silently, reading it as 20 CUBIC METRES on a cell whose SI value is
      // 0.00704500224. `parse_value_string_for_cell` now refuses it, and the
      // frontend mirrors that refusal so the feedback is immediate.
      fireEvent.focus(input);
      fireEvent.input(input, { target: { value: '20' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      // Inline, on the cell itself.
      expect(input.hasAttribute('data-invalid')).toBe(true);

      // The bridge is mocked, so ONLY the frontend gate can stop this call —
      // which is what makes this an end-to-end check of the panel's own rule
      // rather than of the engine's.
      expect(bridge.setParameter).not.toHaveBeenCalled();

      // No toast: the whole point of gating inline is that the user is not sent
      // an async failure for something the panel already knew was wrong.
      await flushMacrotasks();
      expect(screen.queryByTestId('toast')).toBeNull();

      // AND THE TYPED TEXT SURVIVES for correction — the row is still in edit
      // mode, so the user adds a unit rather than retyping the number. This is
      // the feedback quality the #6028 characterization block recorded as lost.
      expect(input.value).toBe('20');
    });
  });

  /**
   * The same fixture plus a cell whose dimension NO curated ladder covers — the
   * real in-tree `Money` shape, badge and all.
   *
   * Same mocking discipline as `renderCapacityInput`: the non-partial
   * `vi.mock('../bridge')` factory with per-test `vi.mocked` overrides, the
   * ladder map supplied through `getUnitLadders`, and the `waitFor` on the
   * Volume picker as the synchronization point — which also proves the fetch
   * resolved, so the uncovered cell below is genuinely uncovered rather than
   * merely un-fetched.
   */
  async function renderWithUncoveredCell(): Promise<{
    covered: HTMLInputElement;
    uncovered: HTMLInputElement;
  }> {
    const state = capacityState();
    state.values.push({
      cell_id: 'Tank.unit_cost',
      name: 'unit_cost',
      value: '5',
      unit: 'USD',
      determinacy: 'determined',
      entity_path: 'Tank.unit_cost',
      kind: 'param',
      freshness: 'final',
      dimension: 'Money',
      si_value: 5,
      declared_dimension: 'Money',
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(state));
    vi.mocked((bridge as any).getUnitLadders).mockResolvedValue([
      {
        dimension: 'Volume',
        units: [
          { label: 'mm³', si_scale: 1e-9, is_default: true },
          { label: 'L', si_scale: 1e-3, is_default: false },
        ],
      },
    ]);

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('unit-select-Tank.capacity')).toBeTruthy();
    });

    const inputFor = (cellId: string) =>
      screen
        .getByTestId(`prop-row-${cellId}`)
        .querySelector('input[type="text"]') as HTMLInputElement;
    return { covered: inputFor('Tank.capacity'), uncovered: inputFor('Tank.unit_cost') };
  }

  it('sends a bare number straight through for a cell no curated ladder covers', async () => {
    await withSuppressedRejectionsAndErrorSpy(async () => {
      vi.mocked((bridge as any).setParameter).mockResolvedValue(undefined);

      const { covered, uncovered } = await renderWithUncoveredCell();

      // The Money cell: `USD` is reachable only through the compiler's
      // per-module `UnitRegistry`, which the engine's composed index excludes,
      // so the bare number is the ONLY input either end accepts. Gating it would
      // brick the row — 16 such declarations exist across examples/*.ri.
      fireEvent.focus(uncovered);
      fireEvent.input(uncovered, { target: { value: '6' } });
      fireEvent.keyDown(uncovered, { key: 'Enter' });

      expect(uncovered.hasAttribute('data-invalid')).toBe(false);
      expect(bridge.setParameter).toHaveBeenCalledWith('Tank.unit_cost', '6');

      await flushMacrotasks();
      expect(screen.queryByTestId('toast')).toBeNull();

      // The LADDERED neighbour in the same panel is discriminated, in the same
      // render: still refused inline, still never reaching the bridge. Without
      // this the test could pass by the gate having been removed outright.
      vi.mocked((bridge as any).setParameter).mockClear();
      fireEvent.focus(covered);
      fireEvent.input(covered, { target: { value: '20' } });
      fireEvent.keyDown(covered, { key: 'Enter' });

      expect(covered.hasAttribute('data-invalid')).toBe(true);
      expect(bridge.setParameter).not.toHaveBeenCalled();
    });
  });
});

// ─── Slider cadence: a drag previews, the release writes the source ──────────

describe('App slider parameter cadence', () => {
  /** One prismatic joint driven by `Kinematic.y_pos`, so the panel renders a slider. */
  function mockOneSliderMechanism(): void {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([
      {
        cell_id: 'Kinematic.m',
        entity_path: 'Kinematic',
        name: 'm',
        bodies_count: 2,
        joints: [
          {
            joint_index: 0,
            kind: 'prismatic',
            dimension: 'length',
            range_lower_si: 0.0,
            range_upper_si: 0.8,
            axis: [0, 1, 0],
            driving_param_cell_id: 'Kinematic.y_pos',
            current_value_si: 0.1,
            binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y_pos', current_value_si: 0.1 },
          },
        ],
      },
    ]);
  }

  async function renderSlider(): Promise<HTMLInputElement> {
    mockOneSliderMechanism();
    await renderAndWaitForReady();
    const panel = await waitFor(() => screen.getByTestId('mechanism-panel'));
    return within(panel).getByRole('slider') as HTMLInputElement;
  }

  it('a drag frame reaches bridge.previewParameter, never bridge.setParameter', async () => {
    const slider = await renderSlider();

    fireEvent.input(slider, { target: { value: '400' } });

    await waitFor(() => {
      expect(vi.mocked((bridge as any).previewParameter)).toHaveBeenCalledWith('Kinematic.y_pos', '400mm');
    });
    expect(vi.mocked(bridge.setParameter)).not.toHaveBeenCalled();
  });

  it('a refused preview raises no toast — a drag would storm one per frame', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked((bridge as any).previewParameter).mockRejectedValue(new Error('parameter is not a literal'));
      const slider = await renderSlider();

      fireEvent.input(slider, { target: { value: '400' } });

      await waitFor(() => {
        expect(vi.mocked((bridge as any).previewParameter)).toHaveBeenCalled();
      });
      await flushMacrotasks();

      expect(screen.queryByTestId('toast')).toBeNull();
      expect(errorSpy).toHaveBeenCalled();
    });
  });

  it('a refused commit drops the optimistic override so the slider stops showing it', async () => {
    // The frontend half of the discard. A refused commit reverts the engine to
    // the source value, and the backend now emits that restored state — but the
    // scrub's optimistic override outranks it in `getEffectiveValueSi`, and
    // `mechanismStore.refresh()` retires an override only when the committed
    // value CATCHES UP to it, which a refusal guarantees will never happen. So
    // without an explicit clear the slider stays parked on 400mm, a value
    // neither the engine nor the file carries.
    await withSuppressedRejectionsAndErrorSpy(async () => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('parameter is not a literal'));
      const slider = await renderSlider();

      fireEvent.input(slider, { target: { value: '400' } });
      fireEvent.change(slider, { target: { value: '400' } });

      await waitFor(() => {
        expect(vi.mocked(bridge.setParameter)).toHaveBeenCalled();
      });
      await flushMacrotasks();

      expect(slider.value).toBe('100');
    });
  });

  it('releasing the slider reaches bridge.setParameter with the final value', async () => {
    const slider = await renderSlider();

    fireEvent.input(slider, { target: { value: '400' } });
    fireEvent.change(slider, { target: { value: '400' } });

    await waitFor(() => {
      expect(vi.mocked(bridge.setParameter)).toHaveBeenCalledWith('Kinematic.y_pos', '400mm');
    });
    expect(vi.mocked(bridge.setParameter)).toHaveBeenCalledTimes(1);
  });

  it('the commit waits for the in-flight preview before writing the source', async () => {
    const inFlight = deferred<undefined>();
    vi.mocked((bridge as any).previewParameter).mockReturnValue(inFlight.promise);
    const slider = await renderSlider();

    fireEvent.input(slider, { target: { value: '250' } });
    await waitFor(() => {
      expect(vi.mocked((bridge as any).previewParameter)).toHaveBeenCalled();
    });

    fireEvent.change(slider, { target: { value: '250' } });
    await flushMacrotasks();
    expect(vi.mocked(bridge.setParameter)).not.toHaveBeenCalled();

    inFlight.resolve(undefined);

    await waitFor(() => {
      expect(vi.mocked(bridge.setParameter)).toHaveBeenCalledWith('Kinematic.y_pos', '250mm');
    });
  });

  it('a refused commit does raise a toast — the durable write is what the user must hear about', async () => {
    await withSuppressedRejectionsAndErrorSpy(async () => {
      vi.mocked(bridge.setParameter).mockRejectedValue(new Error('parameter is not a literal'));
      const slider = await renderSlider();

      fireEvent.change(slider, { target: { value: '400' } });

      await waitFor(() => {
        expect(screen.getByTestId('toast').textContent).toMatch(/parameter is not a literal/i);
      });
    });
  });
});
