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
import { createDiagnosticsListener, lspDiagnosticToCodeMirror, diagnosticInfoSeverityToCm, diagnosticInfoToCmDiagnostic } from '../editor/diagnostics';

const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('lspDiagnosticToCodeMirror', () => {
  // Mock doc: line 1 (LSP 0) → [0, 19]; line 2 (LSP 1) → [20, 39]; throws otherwise.
  const mockDoc = {
    line: (n: number) => {
      if (n === 1) return { from: 0, to: 19 };
      if (n === 2) return { from: 20, to: 39 };
      throw new RangeError(`line ${n} out of range`);
    },
  };

  it('converts an LSP diagnostic to CodeMirror lint Diagnostic format', () => {
    const lspDiag = {
      range: {
        start: { line: 0, character: 5 },
        end: { line: 0, character: 15 },
      },
      severity: 1, // Error
      message: 'unexpected token',
    };

    const result = lspDiagnosticToCodeMirror(lspDiag, mockDoc as any);

    expect(result).toBeDefined();
    expect(result!.from).toBe(5); // line 1 offset 0 + character 5
    expect(result!.to).toBe(15); // line 1 offset 0 + character 15
    expect(result!.severity).toBe('error');
    expect(result!.message).toBe('unexpected token');
  });

  it('maps LSP severity to CodeMirror severity', () => {
    const wideDoc = {
      line: (_n: number) => ({ from: 0, to: 50 }),
    };

    const baseDiag = {
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
      message: 'test',
    };

    // Error (1)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 1 }, wideDoc as any)!.severity).toBe('error');
    // Warning (2)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 2 }, wideDoc as any)!.severity).toBe('warning');
    // Info (3)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 3 }, wideDoc as any)!.severity).toBe('info');
    // Hint (4)
    expect(lspDiagnosticToCodeMirror({ ...baseDiag, severity: 4 }, wideDoc as any)!.severity).toBe('info');
  });

  // --- NEW hardening tests (RED against current unguarded/unclamped impl) ---

  it('clamps an over-long end character to line.to', () => {
    // end character 999 exceeds line 1 (.to=19) → should clamp to 19, not overflow.
    const lspDiag = {
      range: {
        start: { line: 0, character: 5 },
        end: { line: 0, character: 999 },
      },
      severity: 2,
      message: 'wide diagnostic',
    };
    const result = lspDiagnosticToCodeMirror(lspDiag, mockDoc as any);
    expect(result).not.toBeNull();
    expect(result!.to).toBe(19); // clamped to line 1.to
  });

  it('returns null (does NOT throw) when an LSP line is out of range', () => {
    // LSP line 50 → doc.line(51) throws RangeError → must return null, not throw.
    const lspDiag = {
      range: {
        start: { line: 50, character: 0 },
        end: { line: 50, character: 5 },
      },
      severity: 1,
      message: 'stale diagnostic',
    };
    expect(() => lspDiagnosticToCodeMirror(lspDiag, mockDoc as any)).not.toThrow();
    expect(lspDiagnosticToCodeMirror(lspDiag, mockDoc as any)).toBeNull();
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

describe('diagnosticInfoSeverityToCm', () => {
  it('maps Error to error', () => {
    expect(diagnosticInfoSeverityToCm('Error')).toBe('error');
  });

  it('maps Warning to warning', () => {
    expect(diagnosticInfoSeverityToCm('Warning')).toBe('warning');
  });

  it('maps Info to info', () => {
    expect(diagnosticInfoSeverityToCm('Info')).toBe('info');
  });

  it('maps unknown/other strings to info', () => {
    expect(diagnosticInfoSeverityToCm('unknown')).toBe('info');
    expect(diagnosticInfoSeverityToCm('')).toBe('info');
    expect(diagnosticInfoSeverityToCm('HINT')).toBe('info');
  });
});

describe('diagnosticInfoToCmDiagnostic', () => {
  // Mock doc: line 1 spans [0..29], line 2 spans [30..49]
  const mockDoc = {
    lines: 2,
    line: (n: number) => {
      if (n === 1) return { from: 0, to: 29 };
      if (n === 2) return { from: 30, to: 49 };
      throw new Error(`line ${n} out of range`);
    },
  };

  it('maps a typical Error diagnostic to a CmDiagnostic', () => {
    const diag = {
      file_path: '/project/src/bracket.ri',
      line: 1,
      column: 5,   // 1-based
      end_line: 1,
      end_column: 14, // 1-based
      severity: 'Error',
      message: 'unresolved name: rot_to_z',
      code: null,
    };

    const result = diagnosticInfoToCmDiagnostic(diag, mockDoc as any);

    expect(result).not.toBeNull();
    expect(result!.from).toBe(4);   // line 1 starts at 0, col 5 => offset 4
    expect(result!.to).toBe(13);    // col 14 => offset 13
    expect(result!.severity).toBe('error');
    expect(result!.message).toBe('unresolved name: rot_to_z');
  });

  it('clamps to to line.to when end_column exceeds line length', () => {
    const diag = {
      file_path: '/project/src/bracket.ri',
      line: 1,
      column: 1,
      end_line: 1,
      end_column: 999, // beyond line length
      severity: 'Warning',
      message: 'wide diagnostic',
      code: null,
    };

    const result = diagnosticInfoToCmDiagnostic(diag, mockDoc as any);
    expect(result).not.toBeNull();
    expect(result!.to).toBe(29); // clamped to line 1.to
  });

  it('returns null when line is out of range (line > doc.lines)', () => {
    const diag = {
      file_path: '/project/src/bracket.ri',
      line: 999,  // beyond doc
      column: 1,
      end_line: 999,
      end_column: 5,
      severity: 'Error',
      message: 'out of range',
      code: null,
    };

    const result = diagnosticInfoToCmDiagnostic(diag, mockDoc as any);
    expect(result).toBeNull();
  });

  it('maps source field to compile', () => {
    const diag = {
      file_path: '/project/src/bracket.ri',
      line: 1,
      column: 1,
      end_line: 1,
      end_column: 5,
      severity: 'Info',
      message: 'hint',
      code: null,
    };

    const result = diagnosticInfoToCmDiagnostic(diag, mockDoc as any);
    expect(result).not.toBeNull();
    expect(result!.source).toBe('compile');
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
