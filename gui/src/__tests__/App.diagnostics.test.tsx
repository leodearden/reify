import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@solidjs/testing-library';
import type { DiagnosticInfo } from '../types';
import { flushMacrotasks, deferred } from './test-utils';
import { published } from './test_utils/publishedState';
import { makeGuiState } from './test_utils/guiState';

vi.mock('@tauri-apps/api/core', async () => (await import('./appHarness/moduleMocks')).tauriCoreMock());
vi.mock('@tauri-apps/api/event', async () => (await import('./appHarness/moduleMocks')).tauriEventMock());
vi.mock('../viewport', async () => (await import('./appHarness/renderStubs')).viewportStub());
vi.mock('../editor/Editor', async () => (await import('./appHarness/renderStubs')).editorStub());
vi.mock('../editor/FileTabs', async () => (await import('./appHarness/renderStubs')).fileTabsStub());
vi.mock('../bridge', async () => (await import('./appHarness/moduleMocks')).appBridgeMock());
vi.mock('../debug', async () => (await import('./appHarness/moduleMocks')).debugMock());
vi.mock('../stores/sidecarPersistence', async () => (await import('./appHarness/moduleMocks')).sidecarPersistenceMock());
vi.mock('../stores/viewPersistence', async (importOriginal) => (await import('./appHarness/moduleMocks')).viewPersistenceMock(await importOriginal()));

import App from '../App';
import * as bridge from '../bridge';
import { primeAppHarness, renderAndWaitForReady } from './appHarness/harness';
import {
  capturedEditorStore,
  capturedEditorScrollToLocation,
  capturedEditorCompileDiagnostics,
} from './appHarness/renderStubs';

beforeEach(primeAppHarness);

describe('App serialization-error subscription', () => {
  let serializationErrorCallback: ((data: { item_type: string; item_id: string; error: string }) => void) | undefined;

  beforeEach(() => {
    serializationErrorCallback = undefined;
    vi.mocked(bridge.onSerializationError).mockImplementation(async (cb: any) => {
      serializationErrorCallback = cb;
      return () => {};
    });
  });

  it('subscribes to serialization-error events and shows toast after debounce window', async () => {
    render(() => <App />);
    await waitFor(() => expect(serializationErrorCallback).toBeDefined());

    // Switch to fake timers so we can advance the 500ms debounce window instantly
    vi.useFakeTimers();
    try {
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'Bracket.body', error: 'non-finite f32 value' });

      // Toast must NOT appear before the window elapses
      expect(screen.queryAllByTestId('toast').find((t) => t.dataset.type === 'error')).toBeUndefined();

      // Advance past the debounce window
      vi.advanceTimersByTime(500);
      // Let SolidJS flush any pending reactivity
      await Promise.resolve();

      const toasts = screen.getAllByTestId('toast');
      const errorToast = toasts.find((t) => t.dataset.type === 'error');
      expect(errorToast).toBeTruthy();
      expect(errorToast!.textContent).toContain("Failed to serialize mesh 'Bracket.body': non-finite f32 value");
    } finally {
      vi.useRealTimers();
    }
  });

  it('rapid-fire serialization errors produce a single summary toast', async () => {
    render(() => <App />);
    await waitFor(() => expect(serializationErrorCallback).toBeDefined());

    vi.useFakeTimers();
    try {
      // Fire multiple distinct errors within the debounce window
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'A', error: 'err1' });
      serializationErrorCallback!({ item_type: 'mesh', item_id: 'B', error: 'err2' });
      serializationErrorCallback!({ item_type: 'value', item_id: 'C', error: 'err3' });

      // No toast yet
      expect(screen.queryAllByTestId('toast').find((t) => t.dataset.type === 'error')).toBeUndefined();

      vi.advanceTimersByTime(500);
      await Promise.resolve();

      const errorToasts = screen.getAllByTestId('toast').filter((t) => t.dataset.type === 'error');
      // Exactly one summary toast, not three individual toasts
      expect(errorToasts).toHaveLength(1);
      expect(errorToasts[0].textContent).toContain('3 items failed to serialize');
    } finally {
      vi.useRealTimers();
    }
  });

  it('cleans up serialization-error subscription on unmount', async () => {
    const serializationErrorUnsub = vi.fn();
    vi.mocked(bridge.onSerializationError).mockResolvedValueOnce(serializationErrorUnsub);

    const { unmount } = render(() => <App />);
    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    expect(serializationErrorUnsub).not.toHaveBeenCalled();

    unmount();

    expect(serializationErrorUnsub).toHaveBeenCalled();
  });

  it('shows fallback toast when onSerializationError rejects', async () => {
    vi.mocked(bridge.onSerializationError).mockRejectedValueOnce(new Error('listen failed'));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const errorToast = toasts.find((t) =>
        t.textContent?.includes('Serialization error monitoring unavailable'),
      );
      expect(errorToast).toBeTruthy();
    });
  });

  it('does not leak serialization-error listener when unmounted before onSerializationError resolves', async () => {
    const unlistenSerialization = vi.fn();
    const { promise, resolve } = deferred<() => void>();
    vi.mocked(bridge.onSerializationError).mockReturnValue(promise);

    const { unmount } = render(() => <App />);

    // Wait for initApp to reach the onSerializationError await
    await flushMacrotasks();

    // Unmount while onSerializationError is still pending
    unmount();

    // Resolve the deferred promise — the handle arrives after disposal
    resolve(unlistenSerialization);

    // Flush so the on-arrival unlistenSerialization() call executes
    await flushMacrotasks();

    // The subscription set disposes it once on arrival and never holds it, so onCleanup cannot call it again.
    expect(unlistenSerialization).toHaveBeenCalledTimes(1);
  });
});

describe('App tessellation diagnostics end-to-end wiring', () => {
  let tessellationDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    tessellationDiagnosticsCallback = undefined;
    vi.mocked(bridge.onTessellationDiagnostics).mockImplementation(async (cb: any) => {
      tessellationDiagnosticsCallback = cb;
      return () => {};
    });
  });

  it('tessellation-diagnostics event with one Error: StatusBar shows data-has-errors="true" and "Tessellation error"', async () => {
    render(() => <App />);
    // Wait until the app is ready and the callback has been registered
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    // Fire the tessellation-diagnostics event with one Error diagnostic
    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'geometry error: kernel failure',
        code: null,
      },
    ]);

    // Wait for the reactive update to propagate through engineStore → App → StatusBar
    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="tessellation-errors"]');
      expect(badge).toBeTruthy();
      expect(badge?.getAttribute('data-has-errors')).toBe('true');
      expect(statusBar.textContent).toMatch(/tessellation error/i);
    });
  });

  it('docked panel always present: data-collapsed="true" by default (no rows before clicking badge)', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: 'helper.ri',
        line: 5, column: 3, end_line: 5, end_column: 10,
        severity: 'Error',
        message: 'tess kernel boom',
        code: null,
      },
    ]);

    // Wait for badge to appear (panel should already be in the DOM, collapsed)
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // Panel is docked and always present — collapsed by default, no rows yet
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.getAttribute('data-collapsed')).toBe('true');
    expect(document.querySelector('[data-testid="diagnostic-row"]')).toBeNull();
  });

  it('clicking tessellation-errors badge expands the panel (data-collapsed="false") and shows diagnostic', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: 'helper.ri',
        line: 5, column: 3, end_line: 5, end_column: 10,
        severity: 'Error',
        message: 'tess kernel boom',
        code: null,
      },
    ]);

    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // Click the tessellation-errors badge — should expand (toggle collapsed)
    fireEvent.click(screen.getByTestId('tessellation-errors'));

    // Panel expands: data-collapsed="false" and the diagnostic message is visible
    await waitFor(() => {
      const panel = screen.getByTestId('diagnostics-panel');
      expect(panel.getAttribute('data-collapsed')).toBe('false');
      expect(panel.textContent).toContain('tess kernel boom');
    });
  });

  it('clicking the tessellation-errors badge twice collapses the panel (data-collapsed="true")', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());

    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'tess toggle test',
        code: null,
      },
    ]);

    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // First click: expand the panel
    fireEvent.click(screen.getByTestId('tessellation-errors'));
    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });

    // Second click: collapse the panel again
    fireEvent.click(screen.getByTestId('tessellation-errors'));
    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('true');
    });
  });

  it('clicking a tessellation diagnostic row in the panel triggers setScrollToLocation', async () => {
    render(() => <App />);
    await waitFor(() => expect(tessellationDiagnosticsCallback).toBeDefined());
    // Seed main.ri as active so this remains a pure in-file navigation test
    // (avoids incidentally exercising the cross-file open path added in γ task-4403).
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.openFile({ path: 'main.ri', content: '' });

    tessellationDiagnosticsCallback!([
      {
        file_path: 'main.ri',
        line: 7, column: 4, end_line: 7, end_column: 9,
        severity: 'Error',
        message: 'tess nav test',
        code: null,
      },
    ]);

    // Wait for badge to appear then expand the panel
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // Expand the panel via badge click
    fireEvent.click(screen.getByTestId('tessellation-errors'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });

    // Click the diagnostic row
    const row = document.querySelector('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row!);

    // scrollToLocation should be set to the tessellation diagnostic's location
    await waitFor(() => {
      const loc = capturedEditorScrollToLocation?.();
      expect(loc).toMatchObject({
        file_path: 'main.ri',
        line: 7,
        column: 4,
        end_line: 7,
        end_column: 9,
      });
    });
  });
});

describe('App compile diagnostics end-to-end wiring', () => {
  let compileDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    compileDiagnosticsCallback = undefined;
    vi.mocked((bridge as any).onCompileDiagnostics).mockImplementation(async (cb: any) => {
      compileDiagnosticsCallback = cb;
      return () => {};
    });
  });

  const warningDiag = {
    file_path: 'helper.ri',
    line: 3,
    column: 1,
    end_line: 3,
    end_column: 10,
    severity: 'Warning',
    message: "unknown port type 'Foo'",
    code: null,
  };

  it('compile-diagnostics event: diagnostics-count badge appears in StatusBar with "1 warning"', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="diagnostics-count"]');
      expect(badge).toBeTruthy();
      expect(badge!.textContent).toMatch(/1 warning/i);
    });
  });

  it('docked panel always present with data-collapsed="true" before clicking the badge (no rows)', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    // Panel is docked and always mounted — collapsed by default
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.getAttribute('data-collapsed')).toBe('true');
    expect(document.querySelector('[data-testid="diagnostic-row"]')).toBeNull();
  });

  it('clicking diagnostics-count badge expands the panel (data-collapsed="false")', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    fireEvent.click(screen.getByTestId('diagnostics-count'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });
  });

  it('clicking a diagnostic row triggers navigation via setScrollToLocation', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());
    // Seed helper.ri as active so this remains a pure in-file navigation test
    // (warningDiag.file_path = 'helper.ri'; avoids cross-file open path added in γ task-4403).
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());
    capturedEditorStore.openFile({ path: 'helper.ri', content: '' });

    compileDiagnosticsCallback!([warningDiag]);

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-count')).toBeTruthy();
    });

    // Expand the panel via badge click
    fireEvent.click(screen.getByTestId('diagnostics-count'));

    await waitFor(() => {
      expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
    });

    // Click the diagnostic row
    const row = document.querySelector('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row!);

    // Editor scrollToLocation should update with the diagnostic's location
    await waitFor(() => {
      const loc = capturedEditorScrollToLocation?.();
      expect(loc).toMatchObject({
        file_path: 'helper.ri',
        line: 3,
        column: 1,
      });
    });
  });

  it('compile-diagnostics event with Error severity: badge shows "1 error"', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());

    const errorDiag = {
      file_path: 'main.ri',
      line: 1,
      column: 1,
      end_line: 1,
      end_column: 5,
      severity: 'Error',
      message: 'import failed',
      code: null,
    };
    compileDiagnosticsCallback!([errorDiag]);

    await waitFor(() => {
      const statusBar = screen.getByTestId('status-bar');
      const badge = statusBar.querySelector('[data-testid="diagnostics-count"]');
      expect(badge).toBeTruthy();
      expect(badge!.textContent).toMatch(/1 error/i);
    });
  });

  it('γ task-4403: cross-file navigate activates file, scrolls, and panel stays expanded', async () => {
    render(() => <App />);
    await waitFor(() => expect(compileDiagnosticsCallback).toBeDefined());
    await waitFor(() => expect(capturedEditorStore).toBeTruthy());

    // Seed: main.ri active, helper.ri also open but not active
    capturedEditorStore.openFile({ path: 'main.ri', content: '' });
    capturedEditorStore.openFile({ path: 'helper.ri', content: '' });
    capturedEditorStore.setActiveFile('main.ri');

    // Fire a compile diagnostic on helper.ri (the non-active file)
    compileDiagnosticsCallback!([{
      file_path: 'helper.ri',
      line: 5, column: 2, end_line: 5, end_column: 10,
      severity: 'Error',
      message: 'cross-file nav integration test',
      code: null,
    }]);

    await waitFor(() => expect(screen.getByTestId('diagnostics-count')).toBeTruthy());

    // Expand the panel via badge click
    fireEvent.click(screen.getByTestId('diagnostics-count'));
    await waitFor(() => expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false'));

    // Click the diagnostic row → triggers cross-file navigation
    const row = document.querySelector('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(row).toBeTruthy();
    fireEvent.click(row!);

    // Active file must switch to helper.ri
    await waitFor(() => {
      expect(capturedEditorStore.state.activeFile).toBe('helper.ri');
    });

    // Scroll target must be set to the helper.ri location
    const loc = capturedEditorScrollToLocation?.();
    expect(loc).toMatchObject({ file_path: 'helper.ri', line: 5, column: 2, end_line: 5, end_column: 10 });

    // Panel must stay expanded (docked design — no modal to dismiss)
    expect(screen.getByTestId('diagnostics-panel').getAttribute('data-collapsed')).toBe('false');
  });
});

describe('App merged diagnostics rendering', () => {
  let tessellationDiagnosticsCallback: ((diags: any[]) => void) | undefined;
  let compileDiagnosticsCallback: ((diags: any[]) => void) | undefined;

  beforeEach(() => {
    tessellationDiagnosticsCallback = undefined;
    compileDiagnosticsCallback = undefined;
    vi.mocked(bridge.onTessellationDiagnostics).mockImplementation(async (cb: any) => {
      tessellationDiagnosticsCallback = cb;
      return () => {};
    });
    vi.mocked((bridge as any).onCompileDiagnostics).mockImplementation(async (cb: any) => {
      compileDiagnosticsCallback = cb;
      return () => {};
    });
  });

  it('panel shows both compile and tessellation diagnostic messages once expanded', async () => {
    render(() => <App />);
    await waitFor(() => {
      expect(tessellationDiagnosticsCallback).toBeDefined();
      expect(compileDiagnosticsCallback).toBeDefined();
    });

    compileDiagnosticsCallback!([
      {
        file_path: 'main.ri',
        line: 2, column: 1, end_line: 2, end_column: 5,
        severity: 'Warning',
        message: 'compile warn xyz',
        code: null,
      },
    ]);
    tessellationDiagnosticsCallback!([
      {
        file_path: '<unknown>',
        line: 1, column: 1, end_line: 1, end_column: 1,
        severity: 'Error',
        message: 'tess boom abc',
        code: null,
      },
    ]);

    // Wait for tessellation badge to appear
    await waitFor(() => {
      expect(screen.getByTestId('tessellation-errors')).toBeTruthy();
    });

    // Expand the panel via tessellation badge
    fireEvent.click(screen.getByTestId('tessellation-errors'));

    await waitFor(() => {
      const panel = screen.getByTestId('diagnostics-panel');
      expect(panel.getAttribute('data-collapsed')).toBe('false');
      expect(panel.textContent).toContain('compile warn xyz');
      expect(panel.textContent).toContain('tess boom abc');
    });
  });
});

describe('App forwards compileDiagnostics to Editor (task-4252)', () => {
  it('passes engineStore.state.compileDiagnostics to the Editor compileDiagnostics prop', async () => {
    const errorDiag: DiagnosticInfo = {
      file_path: '/project/src/bracket.ri',
      line: 3,
      column: 5,
      end_line: 3,
      end_column: 14,
      severity: 'Error',
      message: 'unresolved name: rot_to_z',
      code: null,
    };

    // Seed getInitialState with an Error compile diagnostic
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(makeGuiState({ compile_diagnostics: [errorDiag] })));

    await renderAndWaitForReady();

    // App should forward the compile_diagnostics from the engine store to <Editor>
    expect(capturedEditorCompileDiagnostics).toBeDefined();
    expect(capturedEditorCompileDiagnostics).toContainEqual(
      expect.objectContaining({ severity: 'Error', message: 'unresolved name: rot_to_z' }),
    );
  });
});
