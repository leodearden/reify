import { describe, it, expect, vi, beforeEach } from 'vitest';
import { flushMacrotasks, deferred } from './test-utils';

// Mock Tauri API modules
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { listen } from '@tauri-apps/api/event';
import { createDiagnosticsListener, lspDiagnosticToCodeMirror } from '../editor/diagnostics';

const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('lspDiagnosticToCodeMirror', () => {
  it('converts an LSP diagnostic to CodeMirror lint Diagnostic format', () => {
    const lspDiag = {
      range: {
        start: { line: 0, character: 5 },
        end: { line: 0, character: 15 },
      },
      severity: 1, // Error
      message: 'unexpected token',
    };

    // Provide a mock doc for line offset calculation
    const mockDoc = {
      line: (n: number) => ({
        from: n === 1 ? 0 : 20,
        to: n === 1 ? 19 : 39,
      }),
    };

    const result = lspDiagnosticToCodeMirror(lspDiag, mockDoc as any);

    expect(result).toBeDefined();
    expect(result.from).toBe(5); // line 1 offset 0 + character 5
    expect(result.to).toBe(15); // line 1 offset 0 + character 15
    expect(result.severity).toBe('error');
    expect(result.message).toBe('unexpected token');
  });

  it('maps LSP severity to CodeMirror severity', () => {
    const mockDoc = {
      line: (_n: number) => ({ from: 0, to: 50 }),
    };

    const baseDiag = {
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
      message: 'test',
    };

    // Error (1)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 1 }, mockDoc as any).severity).toBe('error');
    // Warning (2)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 2 }, mockDoc as any).severity).toBe('warning');
    // Info (3)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 3 }, mockDoc as any).severity).toBe('info');
    // Hint (4)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 4 }, mockDoc as any).severity).toBe('info');
  });
});

describe('diagnostics listener lifecycle', () => {
  it('unlisten is called when cleanup races with async setup', async () => {
    // Simulate the race: listen() resolves after "cleanup" has occurred.
    // This tests the cancelled-flag pattern used in Editor.tsx:
    //   let cancelled = false;
    //   createDiagnosticsListener(cb).then(unlisten => {
    //     if (cancelled) unlisten();   // <-- component already unmounted
    //     else unlistenRef = unlisten;
    //   });
    //   // ... later, onCleanup: cancelled = true;

    const unlisten = vi.fn();
    const { promise: listenPromise, resolve: resolveListenPromise } = deferred<() => void>();
    mockListen.mockReturnValue(listenPromise);

    const callback = vi.fn();

    // Start the listener (not yet resolved)
    let unlistenRef: (() => void) | undefined;
    let cancelled = false;
    createDiagnosticsListener(callback).then((fn) => {
      if (cancelled) {
        fn(); // Component already gone — tear down immediately
      } else {
        unlistenRef = fn;
      }
    });

    // Simulate cleanup happening BEFORE the promise resolves
    cancelled = true;

    // Now resolve the listen promise (simulating Tauri responding late)
    resolveListenPromise!(unlisten);

    // Let microtasks flush
    await flushMacrotasks();

    // The unlisten should have been called because we were cancelled
    expect(unlisten).toHaveBeenCalled();
    // And unlistenRef should NOT have been set
    expect(unlistenRef).toBeUndefined();
  });
});

describe('createDiagnosticsListener', () => {
  it('subscribes to diagnostics Tauri event', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const callback = vi.fn();
    const unsub = await createDiagnosticsListener(callback);

    expect(mockListen).toHaveBeenCalledWith('diagnostics', expect.any(Function));
    expect(unsub).toBe(unlisten);
  });

  it('passes parsed diagnostics to the callback', async () => {
    const unlisten = vi.fn();
    mockListen.mockImplementation(async (_event, handler) => {
      // Simulate Tauri emitting a diagnostics event
      const payload = {
        uri: 'file:///test.ri',
        diagnostics: [
          {
            range: {
              start: { line: 0, character: 0 },
              end: { line: 0, character: 10 },
            },
            severity: 1,
            message: 'syntax error',
          },
        ],
      };
      (handler as (event: { payload: unknown }) => void)({ payload });
      return unlisten;
    });

    const callback = vi.fn();
    await createDiagnosticsListener(callback);

    expect(callback).toHaveBeenCalledWith({
      uri: 'file:///test.ri',
      diagnostics: expect.arrayContaining([
        expect.objectContaining({
          severity: 1,
          message: 'syntax error',
        }),
      ]),
    });
  });
});
