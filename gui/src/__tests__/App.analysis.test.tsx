import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import type { EntityTreeNode, DisplayDirective } from '../types';
import { flushMacrotasks, deferred, withSuppressedRejectionsAndErrorSpy, expectNoUnhandledRejections, makeNode } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';
import { splitGridTracks } from './test_utils/gridTracks';
// Real (unmocked) formatter, shared with BucklingPanel.test.tsx case (g), so the
// App-level buckling assertions pin the rendered payload rather than a literal.
import { formatEigenvalue } from '../panels/BucklingPanel';

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
import { SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS } from '../stores/engineStore';
import { renderAndWaitForReady } from './appHarness/harness';
import {
  capturedDualViewportProps,
  capturedMultiViewportProps,
  mockViewportFitToView,
} from './appHarness/renderStubs';
import { makeMesh } from './test_utils/meshData';

// ── MechanismPanel integration (step-21) ─────────────────────────────────────

/** One prismatic-joint mechanism: enough to mount MechanismPanel in the side panel. */
function singleJointMechanism() {
  return [
    {
      cell_id: 'Kinematic.m', entity_path: 'Kinematic', name: 'm', bodies_count: 2,
      joints: [{
        joint_index: 0, kind: 'prismatic', dimension: 'length',
        range_lower_si: 0, range_upper_si: 0.8, axis: [0, 1, 0],
        driving_param_cell_id: 'Kinematic.y', current_value_si: 0.1,
        binding: { kind: 'param_bound' as const, param_cell_id: 'Kinematic.y', current_value_si: 0.1 },
      }],
    },
  ];
}

describe('App MechanismPanel integration', () => {
  it('renders mechanism-panel inside the side panel', async () => {
    // Provide a mechanism descriptor so the panel is visible
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

    await renderAndWaitForReady();

    // MechanismPanel should be rendered in the side panel
    await waitFor(() => {
      expect(screen.getByTestId('mechanism-panel')).toBeTruthy();
    });
  });

  it('calls getMechanismDescriptors on non-idle to idle transition', async () => {
    let evalStatusCallback: ((data: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue([]);

    await renderAndWaitForReady();
    await waitFor(() => expect(evalStatusCallback).toBeDefined());

    const callsBefore = vi.mocked((bridge as any).getMechanismDescriptors).mock.calls.length;

    // Trigger a non-idle to idle transition
    evalStatusCallback!({ phase: 'evaluating' });
    evalStatusCallback!({ phase: 'idle' });

    await waitFor(() => {
      const callsAfter = vi.mocked((bridge as any).getMechanismDescriptors).mock.calls.length;
      expect(callsAfter).toBeGreaterThan(callsBefore);
    });
  });

  // Regression: when mechanism descriptors render alongside the chat panel, the
  // side-panel grid template had an extra 4px track before the mechanism row that
  // didn't correspond to any DOM splitter. Children after ConstraintPanel (mech,
  // splitter-constraint, chat) all shifted up one track, collapsing the chat
  // container into a 4px slot and clipping its content under overflow:hidden.
  // Assert track count == direct-child count for the affected configurations.
  it('side-panel grid track count matches child count with mechanism + chat', async () => {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue(singleJointMechanism());

    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('mechanism-panel')).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId('chat-panel')).toBeTruthy());

    const sidePanel = screen.getByTestId('side-panel') as HTMLElement;
    const tracks = splitGridTracks(sidePanel.style.gridTemplateRows).length;
    const children = sidePanel.children.length;
    expect(tracks).toBe(children);
  });

  it('side-panel grid track count matches child count with mechanism + no chat', async () => {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue(singleJointMechanism());

    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('mechanism-panel')).toBeTruthy());

    // Toggle chat off via the StatusBar control (the only user path).
    const sidePanel = screen.getByTestId('side-panel') as HTMLElement;
    // Use the chat-toggle control surfaced by StatusBar; falls back to keyboard
    // shortcut if exposed differently — current StatusBar uses claude-status as
    // the toggle target via onToggleChat. Click the claude-status span.
    const claudeStatus = screen.getByTestId('claude-status') as HTMLElement;
    claudeStatus.click();

    await waitFor(() => expect(screen.queryByTestId('chat-panel')).toBeNull());

    const tracks = splitGridTracks(sidePanel.style.gridTemplateRows).length;
    const children = sidePanel.children.length;
    expect(tracks).toBe(children);
  });
});

// ─── Side-panel entry list: every child owns its own track (β #9063) ─────────

describe('App side-panel entry list (β #9063)', () => {
  /** The track of the side-panel child that is, or contains, the element with `testId`. */
  function trackHolding(sidePanel: HTMLElement, testId: string): string {
    const el = screen.getByTestId(testId);
    const children = Array.from(sidePanel.children);
    const index = children.findIndex((child) => child === el || child.contains(el));
    expect(index).toBeGreaterThanOrEqual(0);
    return splitGridTracks(sidePanel.style.gridTemplateRows)[index];
  }

  async function closeChat(): Promise<void> {
    screen.getByTestId('claude-status').click();
    await waitFor(() => expect(screen.queryByTestId('chat-panel')).toBeNull());
  }

  it('buckling + FEA + chat mounted: one track per child, chat in its minmax(160px, 1fr) track', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      fea_diagnostics: [{ kind: 'Unconstrained', rigid_body_modes: ['TranslationX', 'RotationZ'] }],
    })));
    let modeShapeCallback: ((f: any) => void) | undefined;
    vi.mocked((bridge as any).onModeShapeFrame).mockImplementation(async (cb: (f: any) => void) => {
      modeShapeCallback = cb;
      return vi.fn();
    });
    // BucklingPanel probes getContext on mount; jsdom has no canvas backend.
    const getContextSpy = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    try {
      await renderAndWaitForReady();
      await waitFor(() => expect(modeShapeCallback).toBeDefined());
      modeShapeCallback!({ mode_index: 0, phase: 0.0, displaced_positions: [0, 0, 0, 1, 0, 0, 0, 1, 0] });
      modeShapeCallback!({
        mode_index: 0,
        phase: 1.0,
        displaced_positions: [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0],
        eigenvalue: 1000,
      });
      await waitFor(() => {
        expect(screen.getByTestId('buckling-panel')).toBeTruthy();
        expect(screen.getByTestId('fea-diagnostics-panel')).toBeTruthy();
        expect(screen.getByTestId('chat-panel')).toBeTruthy();
      });

      const sidePanel = screen.getByTestId('side-panel');
      expect(splitGridTracks(sidePanel.style.gridTemplateRows)).toHaveLength(sidePanel.children.length);
      expect(trackHolding(sidePanel, 'chat-panel')).toBe('minmax(160px, 1fr)');
      expect(trackHolding(sidePanel, 'splitter-constraint')).toBe('4px');
      expect(trackHolding(sidePanel, 'buckling-panel')).toBe('auto');
      expect(trackHolding(sidePanel, 'fea-diagnostics-panel')).toBe('auto');
    } finally {
      getContextSpy.mockRestore();
    }
  });

  it('solver progress overlay renders outside the side-panel grid', async () => {
    let progressCb: ((p: any) => void) | undefined;
    vi.mocked((bridge as any).onSolverProgress).mockImplementation(async (cb: (p: any) => void) => {
      progressCb = cb;
      return () => {};
    });
    await renderAndWaitForReady();
    await waitFor(() => expect(progressCb).toBeDefined());

    vi.useFakeTimers();
    try {
      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });
      await vi.advanceTimersByTimeAsync(1000);

      const overlay = screen.getByTestId('solver-progress-overlay');
      const sidePanel = screen.getByTestId('side-panel');
      expect(sidePanel.contains(overlay)).toBe(false);
      expect(screen.getByTestId('app-layout').contains(overlay)).toBe(true);
      expect(splitGridTracks(sidePanel.style.gridTemplateRows)).toHaveLength(sidePanel.children.length);
    } finally {
      vi.useRealTimers();
    }
  });

  it('chat closed with no optional panel: the constraint panel absorbs the remaining height', async () => {
    await renderAndWaitForReady();
    await closeChat();

    expect(screen.getByTestId('side-panel').style.gridTemplateRows).toMatch(/^\d+px 4px \d+px 4px 1fr$/);
  });

  it('chat closed with a mechanism mounted: no track absorbs the remaining height', async () => {
    vi.mocked((bridge as any).getMechanismDescriptors).mockResolvedValue(singleJointMechanism());
    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('mechanism-panel')).toBeTruthy());
    await closeChat();

    expect(screen.getByTestId('side-panel').style.gridTemplateRows).toMatch(/^\d+px 4px \d+px 4px \d+px auto$/);
  });
});

// ── AutoResolvePanel integration (step-13) ────────────────────────────────────

describe('App AutoResolvePanel integration', () => {
  it('AutoResolvePanel mounts on DATA and stays readable after the loop completes', async () => {
    // Capture the auto-resolve bridge callbacks registered by engineStore.subscribeToEvents
    let startCb: (() => void) | undefined;
    let iterCb: ((iter: any) => void) | undefined;
    let completeCb: (() => void) | undefined;
    vi.mocked((bridge as any).onAutoResolveStart).mockImplementation(async (cb: () => void) => {
      startCb = cb;
      return () => {};
    });
    vi.mocked((bridge as any).onAutoResolveIteration).mockImplementation(async (cb: (iter: any) => void) => {
      iterCb = cb;
      return () => {};
    });
    vi.mocked((bridge as any).onAutoResolveComplete).mockImplementation(async (cb: () => void) => {
      completeCb = cb;
      return () => {};
    });

    await renderAndWaitForReady();

    // Wait for subscribeToEvents to register the callbacks
    await waitFor(() => expect(startCb).toBeDefined());
    await waitFor(() => expect(iterCb).toBeDefined());
    await waitFor(() => expect(completeCb).toBeDefined());

    // (1) Nothing has happened yet — no panel.
    expect(screen.queryByTestId('auto-resolve-panel')).toBeNull();

    // (2) The loop starts but has reported nothing. The mount is data-gated,
    // not active-gated, so an empty panel must NOT be promoted.
    startCb!();
    await Promise.resolve();
    expect(screen.queryByTestId('auto-resolve-panel')).toBeNull();

    // (3) The first iteration arrives — shaped exactly as
    // `emit_auto_resolve_if_any` emits it today: iteration 0, one parameter,
    // one constraint, and no driving metric at all.
    iterCb!({
      iteration: 0,
      parameters: { 'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' } },
      constraints: {
        max_von_mises: { name: 'max_von_mises', value: 180, unit: 'MPa', target_upper: 200, satisfied: true },
      },
    });
    await waitFor(() => {
      expect(screen.queryByTestId('auto-resolve-panel')).toBeTruthy();
    });

    // (4) The leaf signal: the loop completes and the panel STAYS. Before this
    // change the panel unmounted here, so the result of the loop the user just
    // ran vanished the instant it landed.
    completeCb!();
    await Promise.resolve();
    expect(screen.queryByTestId('auto-resolve-panel')).toBeTruthy();
    const parameters = screen.getByTestId('auto-resolve-parameters');
    expect(within(parameters).getByText('Bracket.thickness')).toBeTruthy();
    expect(within(parameters).getByText('4.2mm')).toBeTruthy();
    expect(screen.getAllByText('max_von_mises').length).toBeGreaterThanOrEqual(1);

    // (5) While the panel is mounted, the side-panel grid must still have one
    // track per child — a desync collapses the chat pane into a 4px strip.
    const sidePanel = screen.getByTestId('side-panel');
    expect(splitGridTracks(sidePanel.style.gridTemplateRows).length).toBe(sidePanel.children.length);
  });
});

// ── SolverProgressOverlay integration (step-15) ────────────────────────────

describe('App SolverProgressOverlay integration', () => {
  it('(a) solver-progress-overlay is absent by default', async () => {
    await renderAndWaitForReady();
    expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();
  });

  it('(b) solver-progress-overlay renders after >1s of solver-progress ticks', async () => {
    let progressCb: ((p: any) => void) | undefined;
    vi.mocked((bridge as any).onSolverProgress).mockImplementation(
      async (cb: (p: any) => void) => {
        progressCb = cb;
        return () => {};
      },
    );

    // Render with real timers so App init (waitFor) works
    await renderAndWaitForReady();
    await waitFor(() => expect(progressCb).toBeDefined());

    // Overlay absent before any tick
    expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();

    // Switch to fake timers so the debounce setTimeout uses the fake clock
    vi.useFakeTimers();
    try {
      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });

      // Still absent — debounce has not expired yet
      expect(screen.queryByTestId('solver-progress-overlay')).toBeNull();

      // Advance past the 1000ms debounce
      await vi.advanceTimersByTimeAsync(1000);

      expect(screen.queryByTestId('solver-progress-overlay')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('(c) clicking Cancel in overlay invokes bridge cancelSolve', async () => {
    let progressCb: ((p: any) => void) | undefined;
    vi.mocked((bridge as any).onSolverProgress).mockImplementation(
      async (cb: (p: any) => void) => {
        progressCb = cb;
        return () => {};
      },
    );

    await renderAndWaitForReady();
    await waitFor(() => expect(progressCb).toBeDefined());

    vi.useFakeTimers();
    try {
      progressCb!({ solver_kind: 'cg', iter: 1, residual: 0.5 });
      await vi.advanceTimersByTimeAsync(1000);

      expect(screen.queryByTestId('solver-progress-overlay')).toBeTruthy();

      // Scope the query to the overlay to avoid ambiguity if other Cancel
      // controls (e.g. AutoResolvePanel) are present in the same render tree.
      fireEvent.click(
        within(screen.getByTestId('solver-progress-overlay')).getByRole('button', { name: 'Cancel' }),
      );

      // cancelSolve is async — flush microtasks
      await Promise.resolve();
      await Promise.resolve();

      expect(vi.mocked((bridge as any).cancelSolve)).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ── Buckling mode-shape-frame subscription (task 6035) ─────────────────────
//
// Covers the App-level buckling wiring end-to-end: initApp -> subscribeModeShapeFrames
// -> bucklingStore.ingestFrame -> the <Show> gate that mounts BucklingPanel -> the
// onCleanup detach. Before task 6035 the `../bridge` mock factory omitted
// `onModeShapeFrame`, so subscribeModeShapeFrames threw on every mount and initApp's
// catch swallowed it — leaving this whole path dead under App.test.tsx.

describe('App buckling mode-shape-frame subscription (task 6035)', () => {
  let modeShapeCallback: ((f: any) => void) | undefined;
  let modeShapeUnlisten: ReturnType<typeof vi.fn>;
  // Structural type: `ReturnType<typeof vi.spyOn>` erases the generic to
  // `unknown` args and does not accept the HTMLCanvasElement.getContext
  // overload set. We only ever call mockRestore on it.
  let getContextSpy: { mockRestore: () => void } | undefined;

  /**
   * Force `canvas.getContext` to return null for the remainder of the current
   * test. Only case (b) mounts BucklingPanel, whose onMount probes getContext
   * to decide whether a WebGL path is available; jsdom has no canvas backend,
   * so it returns null *and* emits a multi-line "Not implemented:
   * HTMLCanvasElement.prototype.getContext" jsdomError to stderr. Returning
   * null explicitly takes the identical guard branch (hasWebGL === false ->
   * early return, no three.js work) with no log.
   *
   * Installed per-test rather than in `beforeEach` so cases (a), (c) and (d) —
   * none of which mount BucklingPanel — run against the unmodified canvas
   * prototype instead of forcing a null-context branch on every other
   * component the App mounts. Restored in `afterEach` so it cannot leak.
   */
  function stubNullCanvasContext() {
    getContextSpy = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  }

  beforeEach(() => {
    modeShapeCallback = undefined;
    modeShapeUnlisten = vi.fn();
    vi.mocked((bridge as any).onModeShapeFrame).mockImplementation(
      async (cb: (f: any) => void) => {
        modeShapeCallback = cb;
        return modeShapeUnlisten;
      },
    );
  });

  afterEach(() => {
    getContextSpy?.mockRestore();
    getContextSpy = undefined;
  });

  it('(a) initApp establishes the mode-shape-frame subscription', async () => {
    await renderAndWaitForReady();
    await waitFor(() => expect(modeShapeCallback).toBeDefined());
    expect(vi.mocked((bridge as any).onModeShapeFrame)).toHaveBeenCalledTimes(1);
  });

  it('(b) emitted mode-shape frames route into the buckling store and reveal BucklingPanel', async () => {
    stubNullCanvasContext();
    await renderAndWaitForReady();
    await waitFor(() => expect(modeShapeCallback).toBeDefined());

    // Gate is closed until the store has ingested at least one frame.
    expect(screen.queryByTestId('buckling-panel')).toBeNull();

    // Same fixture vectors + phase convention as bucklingStore.test.ts (BASE/PEAK):
    // phase < 0.5 -> base frame, phase >= 0.5 -> peak frame for that mode index.
    modeShapeCallback!({ mode_index: 0, phase: 0.0, displaced_positions: [0, 0, 0, 1, 0, 0, 0, 1, 0] });
    modeShapeCallback!({
      mode_index: 0,
      phase: 1.0,
      displaced_positions: [0.1, 0, 0, 1.1, 0, 0, 0.1, 1, 0],
      eigenvalue: 1000,
    });

    await waitFor(() => {
      expect(screen.getByTestId('buckling-panel')).toBeTruthy();
      expect(screen.getByTestId('buckling-mode-row-0')).toBeTruthy();
    });

    // Payload fidelity, not just frame arrival. Row existence alone is carried
    // by the phase-0 base frame (it opens the <Show> gate on state.base), so
    // without these two the peak frame's own fields would go unobserved:
    //   - eigenvalue: rendered as `λ = <formatted>` (same assertion shape as
    //     BucklingPanel.test.tsx case (g)); a wiring bug that dropped it would
    //     render the '—' placeholder and still keep the row present.
    //   - thumbnail: computeModeThumbnail only returns non-null when BOTH the
    //     base and the peak displaced_positions arrays are present and
    //     non-empty, so its presence pins that both frames' geometry landed.
    // Mutation-checked: forcing `eigenvalue: null` in bucklingStore.ingestFrame
    // renders 'Mode 1 · λ = —' and fails the first assertion; removing the
    // App-level subscribe wiring fails both.
    const modeRow = screen.getByTestId('buckling-mode-row-0');
    expect(modeRow.textContent).toContain(formatEigenvalue(1000));
    expect(within(modeRow).getByTestId('buckling-mode-thumbnail-0')).toBeTruthy();
  });

  it('(c) unmount detaches the mode-shape-frame listener', async () => {
    const { unmount } = await renderAndWaitForReady();
    await waitFor(() => expect(modeShapeCallback).toBeDefined());
    // The unsub is assigned one microtask after the callback is captured; let
    // initApp's await settle so we assert against the live subscription.
    await flushMacrotasks();

    expect(modeShapeUnlisten).not.toHaveBeenCalled();

    unmount();

    expect(modeShapeUnlisten).toHaveBeenCalledTimes(1);
  });

  it('(d) unmounting while the subscription is still in flight still detaches the late listener', async () => {
    // Case (c) covers the settled subscription, which onCleanup detaches by
    // disposing the subscription set that holds it. This case covers the race
    // on the other side of that hold: if the component unmounts while
    // `subscribeModeShapeFrames(...)` is still pending, the handle is never
    // held, so onCleanup has nothing to call — the set's dispose-on-arrival of
    // a handle that outlived its disposeAll is the ONLY thing that can detach
    // it. Losing that leaks a live Tauri listener per mount, and case (c)
    // would not notice.
    const gate = deferred<() => void>();
    vi.mocked((bridge as any).onModeShapeFrame).mockImplementation((cb: (f: any) => void) => {
      modeShapeCallback = cb;
      return gate.promise;
    });

    // NOT renderAndWaitForReady(): initApp parks on the pending subscribe
    // promise, so setInitPhase('ready') is downstream of the
    // very await this case suspends and `app-layout` would never appear.
    // Waiting on the subscribe call itself is the correct sync point here.
    const { unmount } = render(() => <App />);
    await waitFor(() =>
      expect(vi.mocked((bridge as any).onModeShapeFrame)).toHaveBeenCalledTimes(1),
    );

    // Unmount with the subscribe promise deliberately still pending.
    unmount();
    expect(modeShapeUnlisten).not.toHaveBeenCalled();

    // Now let the subscription land, too late to be stored.
    gate.resolve(modeShapeUnlisten);
    await flushMacrotasks();

    expect(modeShapeUnlisten).toHaveBeenCalledTimes(1);
  });
});

// Deliberately a SIBLING describe, not a fifth `it` inside the block above:
// this case must observe the plain factory / global-beforeEach default rather
// than the describe-scoped capturing mockImplementation, because it installs a
// rejecting implementation of its own.
describe('App buckling mode-shape-frame subscription — failure path (task 6035)', () => {
  it('a rejected subscription degrades gracefully: App still reaches ready, no BucklingPanel', async () => {
    // Positive assertion on the buckling row's onFailure in App.tsx. The buckling
    // subscription is best-effort: a bridge that cannot deliver mode-shape
    // frames must not abort initApp or wedge the App short of ready.
    //
    // Deliberately NOT written as a negative match on the console.error
    // wording (`not.toHaveBeenCalledWith('[buckling] subscribeModeShapeFrames
    // failed:', ...)`). That shape is unfalsifiable on a reword of the log
    // literal — it would keep reporting green while asserting nothing — and it
    // is already subsumed by case (a): if `onModeShapeFrame` is ever dropped
    // from the `../bridge` mock factory again (the original task-6035 defect,
    // which emitted this stderr line 148 times in one App.test.tsx run),
    // bridgeMockCoverage.test.ts check (b) names it, and the describe above
    // fails in its own beforeEach, which configures that mock.
    await withSuppressedRejectionsAndErrorSpy(async () => {
      vi.mocked((bridge as any).onModeShapeFrame).mockRejectedValue(
        new Error('mode-shape channel unavailable'),
      );

      await renderAndWaitForReady();

      // Non-vacuity anchor: the rejecting path really was exercised.
      await waitFor(() =>
        expect(vi.mocked((bridge as any).onModeShapeFrame)).toHaveBeenCalledTimes(1),
      );
      await flushMacrotasks();

      // initApp swallowed the rejection and carried on past the buckling block:
      // the App is mounted and the later init work still ran.
      expect(screen.getByTestId('app-layout')).toBeTruthy();
      expect(vi.mocked(bridge.isDebugEnabled)).toHaveBeenCalled();

      // …and with no frames ever ingested the <Show> gate stays shut.
      expect(screen.queryByTestId('buckling-panel')).toBeNull();
    });
  });
});

// ── App selective-demand enforcement sync (task 6045) ───────────────────────
//
// Covers the App-level selective-demand ENFORCEMENT wiring end-to-end:
// initApp -> createSelectiveDemandSync -> engineStore.syncDemand ->
// bridge.syncDemand. Before task 6045 the `../bridge` mock factory carried
// the passive-measurement `syncObservedDemand` but omitted the distinct,
// genuine `syncDemand` export (`bridge.ts`), so any test that let the
// 150ms debounce elapse threw "No 'syncDemand' export is defined" — 5 times
// across a full App.test.tsx run — leaving this whole enforcement path
// unexercised.

// waitFor's default budget already covers the debounce with margin; naming
// the wait timeout off the exported constant ties correctness to the real
// debounce value instead of a hardcoded guess that could silently drift.
const DEMAND_SYNC_WAIT_TIMEOUT_MS = SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS + 850;

describe('App selective-demand enforcement sync (task 6045)', () => {
  it('(a) the visible-realization set is synced to bridge.syncDemand, excluding non-realization mesh keys', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [
        makeMesh('Bracket#realization[0]'),
        makeMesh('Bracket#realization[1]'),
        makeMesh('Bracket.geometry'),
      ],
    })));
    // getEntityTree stays at the factory default `[]`: with an empty tree,
    // viewStateStore's `nodeByPath.size === 0` short-circuit makes
    // `getEffectiveVisibility` return 'show' for every path, so the payload
    // below is a pure function of engineStore's '#realization[' key filter.

    await renderAndWaitForReady();

    await waitFor(
      () => expect(vi.mocked(bridge.syncDemand)).toHaveBeenCalled(),
      { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
    );

    // Non-realization exclusion is the non-vacuity anchor: a regression that
    // echoed the whole mesh key set would satisfy a realizations-only
    // membership assertion but fails this exact-set assertion.
    const payload = vi.mocked(bridge.syncDemand).mock.calls.at(-1)![0];
    expect([...payload].sort()).toEqual(['Bracket#realization[0]', 'Bracket#realization[1]']);
  });

  // Case (b) was authored under task 6045 (commit 72951c2d9f) and dropped as
  // out-of-scope there: it failed for a live-code reason, not a mock-factory
  // one — the demand-sync selector took its first tracked read before the entity
  // tree loaded, hitting the same empty-tree short-circuit case (a) relies on,
  // and so subscribed to nothing (esc-6045-1). Restored under task #6052; the
  // selector now re-subscribes through viewStateStore's treeVersion, which every
  // query reads before that short-circuit (PRD §4.5).
  it('(b) hiding a realization via the eye icon prunes it from the next payload while a sibling realization survives', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('Bracket#realization[0]'), makeMesh('Bracket#realization[1]')],
    })));
    // Unlike case (a), this case needs a POPULATED tree: the eye icon must
    // render, and effective visibility must resolve per node rather than via
    // the empty-tree short-circuit. `defaultVisibilityFor` returns 'show' for a
    // plain realization node, so both start visible and the sibling genuinely
    // survives the pruning assertion below.
    vi.mocked(bridge.getEntityTree).mockResolvedValue([
      makeNode({ entity_path: 'Bracket#realization[0]', kind: 'realization', has_mesh: true }),
      makeNode({ entity_path: 'Bracket#realization[1]', kind: 'realization', has_mesh: true }),
    ]);

    await renderAndWaitForReady();
    await waitFor(() => expect(screen.getByTestId('eye-icon-Bracket#realization[0]')).toBeTruthy());

    // Let the initial 0 -> N mesh-set sync land...
    await waitFor(
      () => expect(vi.mocked(bridge.syncDemand)).toHaveBeenCalled(),
      { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
    );
    // ...and THEN wait for quiescence before snapshotting the baseline. Do not
    // "simplify" this wait away: there are two distinct mount-time triggers
    // (meshes 0->N, then the tree-ready readiness change) which may or may not
    // coalesce inside the 150ms debounce. Snapshotting between them would let
    // the all-visible tree-ready payload satisfy the `count > before` anchor
    // below and then fail the pruning assertion — a flake, not a real signal.
    await flushMacrotasks(SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS + 50);
    const before = vi.mocked(bridge.syncDemand).mock.calls.length;

    // viewStateStore's `cycleCascading`: show -> ghost -> hidden.
    fireEvent.click(screen.getByTestId('eye-icon-Bracket#realization[0]'));
    fireEvent.click(screen.getByTestId('eye-icon-Bracket#realization[0]'));
    // DesignTree renders aria-label from its row's getEffectiveVisibility memo,
    // so this also proves the click actually reached the view-state store.
    expect(
      screen.getByTestId('eye-icon-Bracket#realization[0]').getAttribute('aria-label'),
    ).toBe('hidden');

    // One waitFor over the LAST payload, so the assertion is order-robust
    // rather than dependent on exactly one trailing call.
    await waitFor(
      () => {
        expect(vi.mocked(bridge.syncDemand).mock.calls.length).toBeGreaterThan(before);
        const payload = vi.mocked(bridge.syncDemand).mock.calls.at(-1)![0];
        expect(payload).not.toContain('Bracket#realization[0]');
        // Non-vacuity anchor: an implementation that pushed an EMPTY demand set
        // would satisfy the exclusion above on its own.
        expect(payload).toContain('Bracket#realization[1]');
      },
      { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
    );
  });

  // Unlike (b), the tree resolves only AFTER the layout has mounted, so the
  // DesignTree rows and their eye memos exist before the store has a tree:
  // the production slow-backend ordering (PRD §4.5 B8).
  it('(c) B8: with the entity tree arriving after mount, the first eye toggle reaches the DesignTree eye and bridge.syncDemand', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('Bracket#realization[0]'), makeMesh('Bracket#realization[1]')],
    })));
    const tree = deferred<EntityTreeNode[]>();
    vi.mocked(bridge.getEntityTree).mockImplementation(() => tree.promise);

    await renderAndWaitForReady();
    expect(screen.queryByTestId('eye-icon-Bracket#realization[0]')).toBeNull();

    tree.resolve([
      makeNode({ entity_path: 'Bracket#realization[0]', kind: 'realization', has_mesh: true }),
      makeNode({ entity_path: 'Bracket#realization[1]', kind: 'realization', has_mesh: true }),
    ]);
    await waitFor(() => expect(screen.getByTestId('eye-icon-Bracket#realization[0]')).toBeTruthy());
    await waitFor(
      () => expect(vi.mocked(bridge.syncDemand)).toHaveBeenCalled(),
      { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
    );
    // Quiescence before the baseline, for the same reason as (b).
    await flushMacrotasks(SELECTIVE_DEMAND_SYNC_DEBOUNCE_MS + 50);
    const before = vi.mocked(bridge.syncDemand).mock.calls.length;

    fireEvent.click(screen.getByTestId('eye-icon-Bracket#realization[0]'));
    fireEvent.click(screen.getByTestId('eye-icon-Bracket#realization[0]'));
    expect(
      screen.getByTestId('eye-icon-Bracket#realization[0]').getAttribute('aria-label'),
    ).toBe('hidden');

    await waitFor(
      () => {
        expect(vi.mocked(bridge.syncDemand).mock.calls.length).toBeGreaterThan(before);
        const payload = vi.mocked(bridge.syncDemand).mock.calls.at(-1)![0];
        expect(payload).not.toContain('Bracket#realization[0]');
        expect(payload).toContain('Bracket#realization[1]');
      },
      { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
    );
  });
});

// Deliberately a SIBLING describe, not a third `it` inside the block above:
// this case must observe the plain factory / global-beforeEach default
// rather than a describe-scoped mockImplementation, because it installs a
// rejecting implementation of its own (same precedent as the task-6035
// sibling failure-path describe above).
describe('App selective-demand enforcement sync — failure path (task 6045)', () => {
  it('a rejected sync degrades gracefully: App stays healthy, no unhandled rejection', async () => {
    // The enforcement sync runs on createSelectiveDemandSync's own debounced
    // timer, strictly after renderAndWaitForReady() (and initApp) has already
    // resolved — unlike task 6035's onModeShapeFrame subscription, this
    // rejection is never on initApp's own await chain, so it cannot abort
    // init. What this test verifies is narrower and downstream of that: the
    // catch arm in engineStore.syncDemand consumes the rejection, so it never
    // escapes as a genuine unhandled promise rejection (asserted for real by
    // expectNoUnhandledRejections below, not merely suppressed) and is logged
    // rather than silently dropped.
    //
    // Deliberately NOT asserted via a match on the console.warn wording — see
    // the design decision recorded for this task: an argument-matched
    // assertion is unfalsifiable on a reword of the log literal and is
    // already subsumed by (i) case (a) above, which fails outright if
    // `syncDemand` is missing from the factory, and (ii)
    // bridgeMockCoverage.test.ts check (b), which names it if the factory
    // entry is ever dropped again.
    await expectNoUnhandledRejections(async () => {
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      try {
        vi.mocked(bridge.syncDemand).mockRejectedValue(new Error('demand channel unavailable'));
        vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
          meshes: [makeMesh('Bracket#realization[0]')],
        })));

        await renderAndWaitForReady();

        // Non-vacuity anchor: the rejecting path really was exercised.
        await waitFor(
          () => expect(vi.mocked(bridge.syncDemand)).toHaveBeenCalled(),
          { timeout: DEMAND_SYNC_WAIT_TIMEOUT_MS },
        );
        await flushMacrotasks();

        // Positive assertion that the catch arm actually ran — existence
        // only, not a match on the log wording (see above). Deleting the
        // catch arm would make this fail (nothing left to log) as well as
        // trip expectNoUnhandledRejections above.
        expect(warnSpy).toHaveBeenCalled();

        // Sanity: the render tree is still mounted after the rejection
        // resolved.
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      } finally {
        warnSpy.mockRestore();
      }
    });
  });
});

// ─── App feaMode debug-context registration (#5670) ──────────────────────────
// `__REIFY_DEBUG__.feaModes` is a viewportId-keyed map (mirroring the existing
// `viewports` map), and the debug bridge's set_fea_channel resolves a pane's
// store through it. App owns the ONE FeaModeStore registry serving both render
// branches, so its registration is unconditional and branch-independent: the
// registered value is the registry's live backing record, so entries the panes
// mapper adds later are visible through the same reference.
//
// The old branch-scoping rationale — that an unconditional registration would,
// in single-pane mode, land after DualViewport's own (Solid runs child onMount
// before parent onMount) and leave the slot pointing at a store no rendered
// toolbar drives — dissolved with #5670: DualViewport neither creates nor
// registers a store, so there is exactly one writer and nothing to clobber.
// `ctx.feaMode` survives as the legacy scalar mirror of feaModes['design-main'].

describe('App feaMode debug-context registration (keyed, #5670)', () => {
  beforeEach(() => {
    // Minimal stub — registerDebugPanel only checks for the global's presence.
    (window as any).__REIFY_DEBUG__ = { stores: {} as any, testMode: () => false };
  });

  afterEach(() => {
    delete (window as any).__REIFY_DEBUG__;
  });

  it('(a) multi-pane: EVERY rendered pane is addressable by its viewportId', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: [
        { subject: 'A#realization[0]', pane: 0 },
        { subject: 'B#realization[0]', pane: 1 },
      ],
    })));

    await renderAndWaitForReady();

    const feaModes = (window as any).__REIFY_DEBUG__.feaModes;
    expect(feaModes).toBeDefined();
    expect(typeof feaModes['design-main'].setChannel).toBe('function');

    // Identity, not mere presence: a registration pointing at a different store
    // than the one driving the rendered toolbar is exactly the didNotReachStore
    // failure the debug bridge's store guard exists to catch. Asserting it for
    // pane 1 as well as pane 0 is what pins the map as keyed-per-viewport —
    // under the old single-slot design pane 1 was unaddressable entirely.
    const panes = capturedMultiViewportProps.panes;
    expect(feaModes['design-main']).toBe(panes[0].feaModeStore);
    expect(feaModes['pane-1']).toBe(panes[1].feaModeStore);
    expect(feaModes['design-main']).not.toBe(feaModes['pane-1']);
  });

  it('(b) single-pane: the DualViewport branch is served by the SAME App registry', async () => {
    // Default mock has display_panes: [] → DualViewport fallback branch.
    await renderAndWaitForReady();

    expect(screen.queryByTestId('dual-viewport')).toBeTruthy();
    expect(capturedMultiViewportProps.panes).toBeUndefined();

    // The unification proof (#5670). DualViewport used to create and register
    // its own store here, so App deliberately left the slot alone; now App owns
    // the one registry and hands DualViewport its 'design-main' entry, so the
    // registered store and the one the rendered branch drives are one object.
    const feaModes = (window as any).__REIFY_DEBUG__.feaModes;
    expect(feaModes['design-main']).toBeDefined();
    expect(feaModes['design-main']).toBe(capturedDualViewportProps.feaModeStore);
  });

  it('(c) branch flip: the SAME store — and its state — survives multi → single → multi', async () => {
    // hasModelPanes() is live — onDisplayPanesUpdate can flip the layout
    // mid-session — so the registry's cross-branch claim has to survive a real
    // transition, not just a fixed initial render. Capture the subscription so
    // we can drive display_panes after mount.
    let displayPanesCallback: ((panes: DisplayDirective[]) => void) | undefined;
    vi.mocked((bridge as any).onDisplayPanesUpdate).mockImplementation(async (cb: any) => {
      displayPanesCallback = cb;
      return () => {};
    });

    const twoPanes: DisplayDirective[] = [
      { subject: 'A#realization[0]', pane: 0 },
      { subject: 'B#realization[0]', pane: 1 },
    ];
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: twoPanes,
    })));

    await renderAndWaitForReady();
    const designMain = (window as any).__REIFY_DEBUG__.feaModes['design-main'];
    expect(designMain).toBeDefined();
    expect(designMain).toBe(capturedMultiViewportProps.panes[0].feaModeStore);

    // Mutate before the flip: losing this on a layout change is the concrete
    // user-visible regression the two-singleton design caused.
    designMain.setChannel('errorIndicator');

    // ── Flip out: no directives → every mesh collapses onto pane 0 →
    // hasModelPanes() false → <Show> swaps to the DualViewport branch.
    displayPanesCallback!([]);
    await flushMacrotasks();
    expect(screen.queryByTestId('dual-viewport')).toBeTruthy();
    expect(screen.queryByTestId('multi-viewport')).toBeNull();
    // The entry must SURVIVE, not be released: the registrar lives outside the
    // <Show>, registry entries are never evicted, and the DualViewport branch is
    // now handed that very store — so the slot keeps driving a rendered toolbar.
    expect((window as any).__REIFY_DEBUG__.feaModes['design-main']).toBe(designMain);
    expect(capturedDualViewportProps.feaModeStore).toBe(designMain);
    expect(designMain.state.channel).toBe('errorIndicator');

    // ── Flip back: the MultiViewport branch is re-created and draws the same
    // keyed entry from the registry, state intact.
    displayPanesCallback!(twoPanes);
    await flushMacrotasks();
    expect(screen.queryByTestId('multi-viewport')).toBeTruthy();
    expect((window as any).__REIFY_DEBUG__.feaModes['design-main']).toBe(designMain);
    expect(capturedMultiViewportProps.panes[0].feaModeStore).toBe(designMain);
    expect(designMain.state.channel).toBe('errorIndicator');
  });

  it('(d) legacy scalar slot mirrors feaModes[design-main]', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      meshes: [makeMesh('A#realization[0]'), makeMesh('B#realization[0]')],
      display_panes: [
        { subject: 'A#realization[0]', pane: 0 },
        { subject: 'B#realization[0]', pane: 1 },
      ],
    })));

    await renderAndWaitForReady();

    // `feaMode` is retained beside `feaModes` exactly as `viewport` is retained
    // beside `viewports`: the debug bridge's fallback path and the
    // direct-stub-injection tests in debugBridge.test.tsx resolve through it.
    const ctx = (window as any).__REIFY_DEBUG__;
    expect(ctx.feaMode).toBeDefined();
    expect(ctx.feaMode).toBe(ctx.feaModes['design-main']);
  });
});

// ─── FEA diagnostics end-to-end wiring (#2966) ───────────────────────────────
// RED (step-15) — fails because App does not yet render FeaDiagnosticsPanel
// nor pass feaDiagnostics to the Viewport/DualViewport prop.
// GREEN (step-16) — App.tsx wires both.
describe('App FEA diagnostics wiring (#2966)', () => {
  /** GuiState with a non-empty fea_diagnostics array (Unconstrained variant). */
  const stateWithFeaDiagnostics = makeGuiState({
    fea_diagnostics: [
      { kind: 'Unconstrained', rigid_body_modes: ['TranslationX', 'RotationZ'] },
    ],
  });

  it('renders fea-diagnostics-panel with rows when engine state carries fea_diagnostics', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(stateWithFeaDiagnostics));
    await renderAndWaitForReady();
    // FeaDiagnosticsPanel must be present (added to sidebar)
    const panel = screen.getByTestId('fea-diagnostics-panel');
    expect(panel).toBeTruthy();
    // At least one row for the Unconstrained diagnostic
    expect(document.querySelectorAll('[data-testid="fea-diagnostic-row"]').length).toBeGreaterThan(0);
    // Row content includes the DOF mode names
    expect(panel.textContent).toContain('TranslationX');
  });

  // NOTE: This test verifies only the App→DualViewport hand-off (capturedDualViewportProps),
  // because DualViewport is fully stubbed in this test file and the inner design-main Viewport
  // is never rendered here. The authoritative assertion that feaDiagnostics actually reaches
  // the inner Viewport (and thus feaDiagnosticOverlay.sync) lives in:
  //   gui/src/__tests__/viewport/DualViewport.test.tsx
  //   → describe('DualViewport feaDiagnostics threading (γ)', ...)
  it('passes feaDiagnostics prop to DualViewport (App→DualViewport hand-off)', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(stateWithFeaDiagnostics));
    await renderAndWaitForReady();
    // capturedDualViewportProps captures ALL props App passes to DualViewport
    expect(capturedDualViewportProps.feaDiagnostics).toBeDefined();
    expect(Array.isArray(capturedDualViewportProps.feaDiagnostics)).toBe(true);
    expect(capturedDualViewportProps.feaDiagnostics.length).toBeGreaterThan(0);
  });

  it('activating a panel row triggers the fitToView camera-focus handle', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(stateWithFeaDiagnostics));
    await renderAndWaitForReady();
    // Ensure fitToViewRef registration has run (mock registers mockViewportFitToView)
    expect(mockViewportFitToView).toBeDefined();
    // Click the first fea-diagnostic-row
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row);
    // onFocusDiagnostic should have called fitToViewFn()
    expect(mockViewportFitToView).toHaveBeenCalled();
  });

  it('fea-diagnostics-panel is not rendered when fea_diagnostics is empty', async () => {
    // Default mock returns empty state (no fea_diagnostics field → defaults to [])
    await renderAndWaitForReady();
    expect(screen.queryByTestId('fea-diagnostics-panel')).toBeNull();
  });
});
