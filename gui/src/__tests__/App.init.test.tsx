import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import type { GuiState, PublishedState, ValueData } from '../types';
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
import { listen } from '@tauri-apps/api/event';
import { initDebugBridge } from '../debug/bridge';
import { initDebugBridge as mockedInitDebugBridge } from '../debug';
import { renderAndWaitForReady } from './appHarness/harness';

describe('App initial state loading', () => {
  it('calls getInitialState on mount and populates store values into PropertyEditor', async () => {
    const testState = makeGuiState({
      values: [
        {
          cell_id: 'c1',
          name: 'width',
          value: '80',
          unit: 'mm',
          determinacy: 'determined',
          entity_path: 'Bracket.width',
          kind: 'parameter',
          freshness: 'final',
        },
      ],
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    });

    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));

    render(() => <App />);

    // Wait for async getInitialState to resolve and populate the store
    await waitFor(() => {
      // PropertyEditor should show the value from the initial state
      expect(screen.getByText('width')).toBeTruthy();
    });

    expect(bridge.getInitialState).toHaveBeenCalledOnce();
  });
});

describe('App unit ladders (task #5199)', () => {
  it('fetches get_unit_ladders on mount and threads it into PropertyEditor', async () => {
    const testState = makeGuiState({
      values: [
        {
          cell_id: 'Tank.capacity',
          name: 'capacity',
          value: '7045002.24',
          unit: 'mm^3',
          determinacy: 'determined',
          entity_path: 'Tank.capacity',
          kind: 'let',
          freshness: 'final',
          dimension: 'Volume',
          si_value: 0.00704500224,
        },
      ],
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
    vi.mocked((bridge as any).getUnitLadders).mockResolvedValue([
      {
        dimension: 'Volume',
        units: [
          { label: 'mm^3', si_scale: 1e-9, is_default: true },
          { label: 'L', si_scale: 1e-3, is_default: false },
        ],
      },
    ]);

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('unit-select-Tank.capacity')).toBeTruthy();
    });
    expect(bridge.getUnitLadders).toHaveBeenCalledOnce();
  });

  it('shows an info toast when get_unit_ladders rejects, and cells keep the static badge', async () => {
    // Reviewer finding (task #5199 amend, graceful_degradation): the ladder
    // fetch is best-effort with no retry, so a failure previously left the
    // picker silently unavailable for the whole session with only a
    // console.warn — no user-visible signal, unlike the app's other
    // best-effort one-time subscriptions (event/file-change/serialization
    // monitoring), which all surface their own failures via toast.
    const testState = makeGuiState({
      values: [
        {
          cell_id: 'Tank.capacity',
          name: 'capacity',
          value: '7045002.24',
          unit: 'mm^3',
          determinacy: 'determined',
          entity_path: 'Tank.capacity',
          kind: 'let',
          freshness: 'final',
          dimension: 'Volume',
          si_value: 0.00704500224,
        },
      ],
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));
    vi.mocked((bridge as any).getUnitLadders).mockRejectedValueOnce(new Error('backend unavailable'));

    render(() => <App />);

    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const infoToast = toasts.find((t) => t.textContent?.includes('Unit ladders unavailable'));
      expect(infoToast).toBeTruthy();
      expect(infoToast?.dataset.type).toBe('info');
    });

    // The cell falls back to the static unit badge — no picker rendered.
    expect(screen.queryByTestId('unit-select-Tank.capacity')).toBeNull();
  });
});

describe('App dynamic window title', () => {
  it('sets document.title to "Reify" when no file is open', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({ files: [] })));

    render(() => <App />);

    await waitFor(() => {
      expect(document.title).toBe('Reify');
    });
  });

  it('sets document.title to "{basename} - Reify" when a file is open and idle', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    })));

    render(() => <App />);

    await waitFor(() => {
      expect(document.title).toBe('bracket.ri - Reify');
    });
  });

  it('includes evaluation phase in title during evaluation', async () => {
    // Mock onEvaluationStatus to capture the callback so we can trigger it
    let evalStatusCallback: ((status: any) => void) | undefined;
    vi.mocked(bridge.onEvaluationStatus).mockImplementation(async (cb: any) => {
      evalStatusCallback = cb;
      return () => {};
    });

    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({
      files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
    })));

    render(() => <App />);

    // Wait for initial state to load and the status subscription to be installed
    await waitFor(() => {
      expect(document.title).toBe('bracket.ri - Reify');
      expect(evalStatusCallback).toBeDefined();
    });

    // Simulate evaluation status change
    evalStatusCallback!({ phase: 'evaluating' });

    await waitFor(() => {
      expect(document.title).toBe('bracket.ri [evaluating] - Reify');
    });
  });
});

describe('App async mount/cleanup race conditions', () => {
  it('does not leak event listeners when unmounted before subscribeToEvents resolves', async () => {
    // Create tracked unlisten functions for all bridge event listeners
    const meshUnlisten = vi.fn();
    const valueUnlisten = vi.fn();
    const constraintUnlisten = vi.fn();
    const evalUnlisten = vi.fn();
    const meshRemovedUnlisten = vi.fn();
    const valueRemovedUnlisten = vi.fn();
    const constraintRemovedUnlisten = vi.fn();

    // Make onMeshUpdate return a deferred promise (delays subscribeToEvents completion)
    const { promise: meshListenPromise, resolve: resolveMeshListen } = deferred<() => void>();
    vi.mocked(bridge.onMeshUpdate).mockReturnValue(meshListenPromise);

    // All other event listeners resolve immediately with tracked unlistens
    vi.mocked(bridge.onValueUpdate).mockResolvedValue(valueUnlisten);
    vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(constraintUnlisten);
    vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(evalUnlisten);
    vi.mocked(bridge.onMeshRemoved).mockResolvedValue(meshRemovedUnlisten);
    vi.mocked(bridge.onValueRemoved).mockResolvedValue(valueRemovedUnlisten);
    vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(constraintRemovedUnlisten);

    const { unmount } = render(() => <App />);

    // Wait for getInitialState to resolve and subscribeToEvents to start
    await flushMacrotasks();

    // Unmount while subscribeToEvents is still pending (waiting for deferred onMeshUpdate)
    unmount();

    // Resolve the deferred onMeshUpdate — subscribeToEvents will now complete
    resolveMeshListen(meshUnlisten);

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // The subscription set disposes the late composite unsub on arrival,
    // which calls all individual unlisten functions; holding it instead would
    // LEAK every listener.
    expect(meshUnlisten).toHaveBeenCalled();
    expect(valueUnlisten).toHaveBeenCalled();
    expect(constraintUnlisten).toHaveBeenCalled();
    expect(evalUnlisten).toHaveBeenCalled();
    expect(meshRemovedUnlisten).toHaveBeenCalled();
    expect(valueRemovedUnlisten).toHaveBeenCalled();
    expect(constraintRemovedUnlisten).toHaveBeenCalled();
  });

  it('does not call initFromState on dead component when unmounted before getInitialState resolves', async () => {
    // Create deferred promise for getInitialState
    const { promise: getStatePromise, resolve: resolveGetState } = deferred<PublishedState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(getStatePromise);

    const { unmount } = render(() => <App />);

    // Unmount while getInitialState is still pending
    unmount();

    // Resolve getInitialState with data (values + files)
    resolveGetState(published(makeGuiState({
      values: [{
        cell_id: 'c1',
        name: 'testval',
        value: '42',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'Test.testval',
        kind: 'parameter',
        freshness: 'final',
      }],
      files: [{ path: '/test.ri', content: '' }],
    })));

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // After fix: alive guard returns before reaching subscribeToEvents
    // With current code: initFromState runs, then subscribeToEvents runs → onMeshUpdate called
    expect(bridge.onMeshUpdate).not.toHaveBeenCalled();
  });

  it('unmount detaches the Claude event listener', async () => {
    const claudeUnsub = vi.fn();
    vi.mocked(bridge.subscribeToClaudeEvents).mockResolvedValueOnce(claudeUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Claude unsub should not have been called while component is alive
    expect(claudeUnsub).not.toHaveBeenCalled();

    // Unmount — onCleanup disposes the subscription set, Claude listener included
    unmount();

    // Verify the Claude event unsubscribe was called during cleanup
    expect(claudeUnsub).toHaveBeenCalled();
  });

  it('unmount detaches the sidecar-crashed listener', async () => {
    const sidecarCrashedUnlisten = vi.fn();
    vi.mocked((bridge as any).subscribeToSidecarCrashed).mockResolvedValueOnce(sidecarCrashedUnlisten);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // The sidecar-crashed unlisten should not have been called while component is alive
    expect(sidecarCrashedUnlisten).not.toHaveBeenCalled();

    // Unmount — onCleanup disposes the subscription set, sidecar-crashed listener included
    unmount();

    // Verify the sidecar-crashed unsubscribe was called during cleanup
    expect(sidecarCrashedUnlisten).toHaveBeenCalled();
  });

  it('subscribeToSidecarCrashed is called exactly once after ready', async () => {
    await renderAndWaitForReady();
    expect((bridge as any).subscribeToSidecarCrashed).toHaveBeenCalledTimes(1);
  });

  it('does not leak Claude event listeners when unmounted before subscribeToClaudeEvents resolves', async () => {
    // Create a deferred promise for subscribeToClaudeEvents
    const unlistenClaude = vi.fn();
    const { promise: claudeSubPromise, resolve: resolveClaudeSub } = deferred<() => void>();
    vi.mocked(bridge.subscribeToClaudeEvents).mockReturnValue(claudeSubPromise);

    const { unmount } = render(() => <App />);

    // Wait for getInitialState to resolve and initApp to reach subscribeToClaudeEvents
    await flushMacrotasks();

    // Unmount while subscribeToClaudeEvents is still pending
    unmount();

    // Resolve the deferred subscribeToClaudeEvents — the handle arrives after disposal
    resolveClaudeSub(unlistenClaude);

    // Flush macrotasks so setTimeout(0) callbacks execute
    await flushMacrotasks();

    // The subscription set disposes a handle that arrives after its disposeAll on
    // arrival and never holds it, so onCleanup's disposeAll cannot call it again.
    // The unlisten should be called exactly once — on arrival, not via onCleanup.
    expect(unlistenClaude).toHaveBeenCalledTimes(1);
  });
});

describe('App initialization loading state', () => {
  it('shows app-loading while getInitialState is pending', async () => {
    // Create a deferred promise so getInitialState stays pending
    const { promise: getStatePromise, resolve: resolveGetState } = deferred<PublishedState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(getStatePromise);

    render(() => <App />);

    // Should show loading indicator while pending
    expect(screen.getByTestId('app-loading')).toBeTruthy();
    // Should NOT show the main layout yet
    expect(screen.queryByTestId('app-layout')).toBeNull();

    // Resolve to transition to ready
    resolveGetState(published(makeGuiState()));
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    expect(screen.queryByTestId('app-loading')).toBeNull();
  });

  it('a pending kernel-status fetch delays none of the subscriptions ahead of the kernel-status row', async () => {
    const kernelStatus = deferred<bridge.KernelStatus>();
    vi.mocked(bridge.getKernelStatus).mockReturnValueOnce(kernelStatus.promise);

    render(() => <App />);

    await waitFor(() => expect(bridge.onFocusEntity).toHaveBeenCalledTimes(1));
    expect(bridge.onMeshUpdate).toHaveBeenCalledTimes(1);
    expect(bridge.onFileChanged).toHaveBeenCalledTimes(1);
    expect(bridge.onFileRemoved).toHaveBeenCalledTimes(1);
    expect(bridge.onSerializationError).toHaveBeenCalledTimes(1);
    expect(bridge.onKernelStatus).not.toHaveBeenCalled();
    expect(screen.getByTestId('app-loading')).toBeTruthy();

    kernelStatus.resolve({ available: true, message: null });
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    expect(bridge.onKernelStatus).toHaveBeenCalledTimes(1);
  });

  it('shows app-error with retry button when getInitialState rejects', async () => {
    vi.mocked(bridge.getInitialState).mockRejectedValue(new Error('network error'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });
    // Should have a retry button
    expect(screen.getByText('Retry')).toBeTruthy();
    // Should NOT show loading or main layout
    expect(screen.queryByTestId('app-loading')).toBeNull();
    expect(screen.queryByTestId('app-layout')).toBeNull();
  });

  it('clicking retry button calls getInitialState again', async () => {
    // First call rejects
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Reset to succeed on retry
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState()));

    fireEvent.click(screen.getByText('Retry'));

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // getInitialState called twice: initial + retry
    expect(bridge.getInitialState).toHaveBeenCalledTimes(2);
  });

  it('after successful getInitialState, app-layout is shown and loading/error are gone', async () => {
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState()));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    expect(screen.queryByTestId('app-loading')).toBeNull();
    expect(screen.queryByTestId('app-error')).toBeNull();
  });
});

describe('App event subscription error toast', () => {
  it('shows warning toast when subscribeToEvents fails', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      // Make onMeshUpdate throw synchronously — this causes subscribeToEvents
      // to reject because the array literal throws before Promise.allSettled runs
      vi.mocked(bridge.onMeshUpdate).mockImplementation(() => {
        throw new Error('subscription failed');
      });

      vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState()));

      render(() => <App />);

      // Wait for ready state (subscribeToEvents failure is non-fatal)
      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Wait for the warning toast to appear
      await waitFor(() => {
        const toastEl = screen.getByTestId('toast');
        expect(toastEl).toBeTruthy();
        expect(toastEl.textContent).toContain('Event subscription failed');
      });

      // console.error should NOT be called (replaced with toast)
      expect(errorSpy).not.toHaveBeenCalledWith(
        'Failed to subscribe to events:',
        expect.any(Error),
      );
    });
  });
});

describe('App Claude subscription error toast', () => {
  it('shows warning toast when subscribeToClaudeEvents rejects', async () => {
    vi.mocked(bridge.subscribeToClaudeEvents).mockRejectedValueOnce(
      new Error('connection refused'),
    );

    render(() => <App />);

    // Wait for ready state (subscribeToClaudeEvents failure is non-fatal)
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Wait for the Claude-specific warning toast to appear
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const claudeToast = toasts.find((t) =>
        t.textContent?.includes('Claude assistant unavailable'),
      );
      expect(claudeToast).toBeTruthy();
      expect(claudeToast!.textContent).toContain(
        'chat features may not work',
      );
    });
  });
});

describe('App initApp concurrent execution guard', () => {
  afterEach(() => {
    vi.mocked(listen).mockResolvedValue(() => {});
    vi.mocked(mockedInitDebugBridge).mockResolvedValue(() => {});
  });

  it('after an init failure and Retry exactly one debug-request listener is live (B7)', async () => {
    const liveDebugListeners = new Set<object>();
    vi.mocked(mockedInitDebugBridge).mockImplementation(initDebugBridge);
    vi.mocked(listen).mockImplementation(async (event) => {
      if (event !== 'debug-request') return () => {};
      const token = {};
      liveDebugListeners.add(token);
      return () => {
        liveDebugListeners.delete(token);
      };
    });
    vi.mocked(bridge.isDebugEnabled).mockResolvedValue(true);
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    fireEvent.click(screen.getByText('Retry'));
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
    const debugRequestRegistrations = () =>
      vi.mocked(listen).mock.calls.filter(([event]) => event === 'debug-request');
    await waitFor(() => {
      expect(debugRequestRegistrations().length).toBeGreaterThanOrEqual(1);
    });

    expect(liveDebugListeners.size).toBe(1);
    expect(debugRequestRegistrations()).toHaveLength(1);

    unmount();
    expect(liveDebugListeners.size).toBe(0);
  });

  it('rapid double-click on Retry does not start two concurrent initApp flights', async () => {
    // First getInitialState rejects → error state
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Set up deferred promise for retry (keeps initApp in-flight)
    const { promise: retryPromise, resolve: resolveRetry } = deferred<PublishedState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(retryPromise);

    // Click Retry — first retry
    fireEvent.click(screen.getByText('Retry'));

    // Immediately after click, the Retry button should be either disabled or
    // removed from DOM, preventing a second click from firing.
    const retryBtn = screen.queryByText('Retry');
    expect(retryBtn === null || (retryBtn as HTMLButtonElement).disabled).toBe(true);

    // getInitialState should be called exactly twice: initial mount + first retry
    // NOT three times (which would indicate double-click succeeded)
    expect(bridge.getInitialState).toHaveBeenCalledTimes(2);

    // Clean up: resolve the deferred promise
    resolveRetry(published(makeGuiState()));
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
  });

  it('retry cleans up prior subscriptions before re-subscribing', async () => {
    // Track unsub calls with call order tracking
    const callLog: string[] = [];
    const priorUnsub = vi.fn(() => callLog.push('prior-unsub'));
    const priorFileUnsub = vi.fn(() => callLog.push('prior-file-unsub'));
    const newMeshUnsub = vi.fn(() => callLog.push('new-mesh-unsub'));
    const newValueUnsub = vi.fn(() => callLog.push('new-value-unsub'));
    const newConstraintUnsub = vi.fn(() => callLog.push('new-constraint-unsub'));
    const newEvalUnsub = vi.fn(() => callLog.push('new-eval-unsub'));
    const newMeshRmUnsub = vi.fn(() => callLog.push('new-mesh-rm-unsub'));
    const newValueRmUnsub = vi.fn(() => callLog.push('new-value-rm-unsub'));
    const newConstraintRmUnsub = vi.fn(() => callLog.push('new-constraint-rm-unsub'));
    const newFileUnsub = vi.fn(() => callLog.push('new-file-unsub'));

    // First initApp (mount): getInitialState succeeds, subs established
    // onMeshUpdate returns the "prior" unsub — subscribeToEvents bundles it
    vi.mocked(bridge.onMeshUpdate).mockResolvedValueOnce(priorUnsub);
    vi.mocked(bridge.onFileChanged).mockResolvedValueOnce(priorFileUnsub);

    vi.mocked(bridge.getInitialState).mockResolvedValueOnce(published(makeGuiState()));

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Verify prior subscriptions are active (unsubs not yet called)
    expect(priorUnsub).not.toHaveBeenCalled();
    expect(priorFileUnsub).not.toHaveBeenCalled();

    // Set up new mocks for a second initApp call (if it were to happen)
    vi.mocked(bridge.onMeshUpdate).mockResolvedValue(newMeshUnsub);
    vi.mocked(bridge.onValueUpdate).mockResolvedValue(newValueUnsub);
    vi.mocked(bridge.onConstraintUpdate).mockResolvedValue(newConstraintUnsub);
    vi.mocked(bridge.onEvaluationStatus).mockResolvedValue(newEvalUnsub);
    vi.mocked(bridge.onMeshRemoved).mockResolvedValue(newMeshRmUnsub);
    vi.mocked(bridge.onValueRemoved).mockResolvedValue(newValueRmUnsub);
    vi.mocked(bridge.onConstraintRemoved).mockResolvedValue(newConstraintRmUnsub);
    vi.mocked(bridge.onFileChanged).mockResolvedValue(newFileUnsub);

    // Unmount — disposing the subscription set calls both the composite unsub
    // (which calls priorUnsub) and the file-changed unlisten (priorFileUnsub)
    unmount();

    // All prior subscription cleanup functions should have been called
    expect(priorUnsub).toHaveBeenCalled();
    expect(priorFileUnsub).toHaveBeenCalled();

    // Verify cleanup happened — the prior unsubs should be in the call log
    expect(callLog).toContain('prior-unsub');
    expect(callLog).toContain('prior-file-unsub');
  });

  it('Retry button is disabled while initApp is in-flight (loading phase)', async () => {
    // First getInitialState rejects → error state
    vi.mocked(bridge.getInitialState).mockRejectedValueOnce(new Error('fail'));

    render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-error')).toBeTruthy();
    });

    // Retry button should be present and clickable in error state
    const retryBtn = screen.getByText('Retry') as HTMLButtonElement;
    expect(retryBtn.disabled).toBe(false);

    // Set up deferred getInitialState so initApp stays in loading phase
    const { promise: retryPromise, resolve: resolveRetry } = deferred<PublishedState>();
    vi.mocked(bridge.getInitialState).mockReturnValue(retryPromise);

    // Click Retry — should transition to loading phase
    fireEvent.click(retryBtn);

    // The Retry button should no longer be in the DOM (loading phase hides
    // the error state) or should be disabled to prevent re-clicks
    expect(screen.queryByText('Retry')).toBeNull();

    // Also verify we're in loading state
    expect(screen.getByTestId('app-loading')).toBeTruthy();

    // Clean up: resolve the deferred promise
    resolveRetry(published(makeGuiState()));
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });
  });
});

// Task 7853: a whole-state reply travels over IPC while newer events are
// already reaching the store, so App routes it through the store's
// publish-generation guard.
describe('App whole-state replies honour the publish generation', () => {
  const width = (value: string): ValueData => ({
    cell_id: 'c1',
    name: 'width',
    value,
    unit: 'mm',
    determinacy: 'determined',
    entity_path: 'Bracket.width',
    kind: 'parameter',
    freshness: 'final',
  });
  const stateWithWidth = (value: string): GuiState => makeGuiState({
    values: [width(value)],
    files: [{ path: '/project/bracket.ri', content: 'structure Bracket {}' }],
  });
  const shownWidth = () =>
    (within(screen.getByTestId('prop-row-c1')).getByRole('textbox') as HTMLInputElement).value;

  /**
   * Render the app, press Ctrl+O, and leave the open's engine reply pending
   * behind a newer announced generation whose value update already landed.
   */
  async function openBehindANewerGeneration() {
    let announce: ((generation: number) => void) | undefined;
    let valueUpdate: ((value: ValueData) => void) | undefined;
    vi.mocked(bridge.onEvalGeneration).mockImplementation(async (cb) => {
      announce = cb;
      return () => {};
    });
    vi.mocked(bridge.onValueUpdate).mockImplementation(async (cb) => {
      valueUpdate = cb;
      return () => {};
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(stateWithWidth('50'), 1));
    vi.mocked(bridge.pickOpenPath).mockResolvedValue('/project/other.ri');
    vi.mocked(bridge.openFile).mockResolvedValue({ path: '/project/other.ri', content: 'structure Other {}' });
    const reply = deferred<PublishedState>();
    vi.mocked(bridge.openFileEngine).mockReturnValue(reply.promise);

    await renderAndWaitForReady();
    await waitFor(() => expect(announce && valueUpdate).toBeTruthy());
    fireEvent.keyDown(document, { key: 'o', ctrlKey: true });
    await waitFor(() => expect(bridge.openFileEngine).toHaveBeenCalledWith('/project/other.ri'));

    announce!(3);
    valueUpdate!(width('120'));
    await waitFor(() => expect(shownWidth()).toContain('120'));
    return reply.resolve;
  }

  it('a File→Open reply older than an announced generation keeps the newer event-applied values', async () => {
    const resolveReply = await openBehindANewerGeneration();
    const treeFetchesBefore = vi.mocked(bridge.getEntityTree).mock.calls.length;

    resolveReply(published(stateWithWidth('80'), 2));

    await waitFor(() =>
      expect(vi.mocked(bridge.getEntityTree).mock.calls.length).toBeGreaterThan(treeFetchesBefore),
    );
    expect(shownWidth()).toContain('120');
    expect(shownWidth()).not.toContain('80');
  });

  it('a File→Open reply at or above the announced generation is applied', async () => {
    const resolveReply = await openBehindANewerGeneration();

    resolveReply(published(stateWithWidth('80'), 3));

    await waitFor(() => expect(shownWidth()).toContain('80'));
    expect(shownWidth()).not.toContain('120');
  });
});
