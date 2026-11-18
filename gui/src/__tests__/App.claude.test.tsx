import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@solidjs/testing-library';
import { withSuppressedRejections, withSuppressedRejectionsAndErrorSpy } from './test-utils';
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
import { primeAppHarness } from './appHarness/harness';

beforeEach(primeAppHarness);

describe('App Claude error handling', () => {
  let consoleSpy: ReturnType<typeof vi.spyOn> | undefined;

  afterEach(() => {
    consoleSpy?.mockRestore();
    consoleSpy = undefined;
  });

  it('logs error to console when subscribeToClaudeEvents fails', async () => {
    const subscribeError = new Error('subscribe failed');
    vi.mocked(bridge.subscribeToClaudeEvents).mockRejectedValue(subscribeError);
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    await waitFor(() => {
      expect(consoleSpy).toHaveBeenCalledWith(
        '[claude] subscribeToClaudeEvents failed:',
        subscribeError,
      );
    });

    // Verify the toast DOM element appears with the correct error message
    await waitFor(() => {
      const toasts = screen.getAllByTestId('toast');
      const claudeToast = toasts.find((t) =>
        t.textContent?.includes('Claude assistant unavailable'),
      );
      expect(claudeToast).toBeTruthy();
      expect(claudeToast!.dataset.type).toBe('error');
      expect(claudeToast!.textContent).toContain('chat features may not work');
    });
  });

  it('shows toast when claudeAbort fails', async () => {
    const abortError = new Error('abort failed');
    vi.mocked(bridge.claudeAbort).mockRejectedValue(abortError);
    consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    // Capture the handler passed to subscribeToClaudeEvents
    let claudeHandler: ((msg: any) => void) | undefined;
    vi.mocked(bridge.subscribeToClaudeEvents).mockImplementation(async (handler) => {
      claudeHandler = handler;
      return () => {};
    });

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Guard: ensure subscribeToClaudeEvents was called and captured the handler
    await waitFor(() => expect(claudeHandler).toBeDefined());

    // Fire a text_delta event to put claudeStore into 'responding' state
    claudeHandler!({ type: 'text_delta', id: 'msg-1', content: 'Hello' });

    // Wait for abort button to appear
    await waitFor(() => {
      expect(screen.getByTestId('abort-button')).toBeTruthy();
    });

    // Click the abort button
    fireEvent.click(screen.getByTestId('abort-button'));

    // Wait for the toast to appear (claudeAbort rejection triggers async catch)
    await waitFor(() => {
      expect(consoleSpy).toHaveBeenCalledWith(
        '[claude] abort failed:',
        abortError,
      );
    });

    // Verify the toast DOM element matches the error pattern used by other error-path tests
    await waitFor(() => {
      const toastEl = screen.getByTestId('toast');
      expect(toastEl).toBeTruthy();
      expect(toastEl.dataset.type).toBe('error');
      expect(toastEl.textContent).toContain('Abort failed: abort failed');
    });
  });
});

describe('App onSend context forwarding', () => {
  it('forwards currentFile and attachedContexts to claudeSendMessage', async () => {
    // Set up initial state with a file so activeFile is set in ChatPanel
    const testState = makeGuiState({
      files: [{ path: 'bracket.ri', content: 'structure Bracket {}' }],
    });
    vi.mocked(bridge.getInitialState).mockResolvedValue(published(testState));

    render(() => <App />);

    await waitFor(() => {
      expect(screen.getByTestId('app-layout')).toBeTruthy();
    });

    // Open context picker and attach 'file' context
    const pickerBtn = screen.getByTestId('context-picker-btn');
    fireEvent.click(pickerBtn);

    await waitFor(() => {
      expect(screen.getByTestId('context-picker-dropdown')).toBeTruthy();
    });

    // Click "Current file" option (4th button in the dropdown)
    const dropdown = screen.getByTestId('context-picker-dropdown');
    const options = dropdown.querySelectorAll('button');
    const fileOption = Array.from(options).find((btn) => btn.textContent === 'Current file');
    expect(fileOption).toBeTruthy();
    fireEvent.click(fileOption!);

    // Type a message in the chat input
    const chatInput = screen.getByTestId('chat-input');
    fireEvent.input(chatInput, { target: { value: 'help with this file' } });

    // Click send button
    const sendBtn = screen.getByTestId('send-button');
    fireEvent.click(sendBtn);

    // Verify claudeSendMessage was called with currentFile and attachedContexts
    await waitFor(() => {
      expect(bridge.claudeSendMessage).toHaveBeenCalledTimes(1);
    });

    const callArgs = vi.mocked(bridge.claudeSendMessage).mock.calls[0];
    const contextArg = callArgs[1];
    expect(contextArg).toBeDefined();
    expect(contextArg!.currentFile).toBe('bracket.ri');
    expect(contextArg!.attachedContexts).toContain('file');
  });
});

describe('App claudeSendMessage error-path integration', () => {
  it('claudeSendMessage failure renders system-message with ipc_error type and original error', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (errorSpy) => {
      vi.mocked(bridge.claudeSendMessage).mockRejectedValueOnce(new Error('IPC channel broken'));
      vi.mocked(bridge.getInitialState).mockResolvedValueOnce(published(makeGuiState({
        files: [{ path: 'bracket.ri', content: 'structure Bracket {}' }],
      })));

      render(() => <App />);

      await waitFor(() => {
        expect(screen.getByTestId('app-layout')).toBeTruthy();
      });

      // Type a message in the chat input
      const chatInput = screen.getByTestId('chat-input');
      fireEvent.input(chatInput, { target: { value: 'help me' } });

      // Click send button to trigger onSend -> claudeSendMessage
      const sendBtn = screen.getByTestId('send-button');
      fireEvent.click(sendBtn);

      // Wait for the system-message element to appear (not a toast — this path uses addSystemMessage)
      await waitFor(() => {
        const sysMsg = screen.getByTestId('system-message');
        expect(sysMsg).toBeTruthy();
        expect(sysMsg.getAttribute('data-error-type')).toBe('ipc_error');
        expect(sysMsg.textContent).toContain('Failed to send message');
        expect(sysMsg.textContent).toContain('IPC channel broken');
      });

      // console.error should NOT be called (error is routed through addSystemMessage)
      expect(errorSpy).not.toHaveBeenCalledWith(
        '[claude] sendMessage failed:',
        expect.any(Error),
      );
    });
  });
});

describe('App error-path integration: errorMessage propagation', () => {
  async function triggerExportDialog() {
    // Wait for app to reach ready state
    await waitFor(() => expect(screen.getByTestId('app-layout')).toBeTruthy());
    // Open export dialog via toolbar Export button
    fireEvent.click(screen.getByText('Export'));
    await waitFor(() => expect(screen.getByTestId('export-dialog')).toBeTruthy());
    // Click the Export button inside the dialog
    const dialog = screen.getByTestId('export-dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Export' }));
  }

  it('export failure toast contains the original error message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.exportGeometry).mockRejectedValueOnce(new Error('geometry kernel crashed'));
      render(() => <App />);
      await triggerExportDialog();

      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.dataset.type === 'error');
        expect(errorToast).toBeTruthy();
        expect(errorToast!.textContent).toContain('Export failed');
        expect(errorToast!.textContent).toContain('geometry kernel crashed');
      });
    });
  });

  it('pickSavePath failure toast contains the original error message', async () => {
    await withSuppressedRejections(async () => {
      vi.mocked(bridge.pickSavePath).mockRejectedValueOnce(new Error('dialog permission denied'));
      render(() => <App />);
      await triggerExportDialog();

      await waitFor(() => {
        const toasts = screen.getAllByTestId('toast');
        const errorToast = toasts.find((t) => t.dataset.type === 'error');
        expect(errorToast).toBeTruthy();
        expect(errorToast!.textContent).toContain('Could not open save dialog');
        expect(errorToast!.textContent).toContain('dialog permission denied');
      });
    });
  });
});
