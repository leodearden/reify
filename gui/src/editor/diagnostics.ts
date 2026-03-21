/**
 * Diagnostics bridge: subscribes to Tauri "diagnostics" events from the
 * in-process LSP server and converts LSP diagnostics to CodeMirror lint format.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Text } from '@codemirror/state';

// ── Types ──────────────────────────────────────────────────────────────

/** LSP Diagnostic as received from the backend. */
export interface LspDiagnostic {
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  severity?: number;
  message: string;
  source?: string;
  code?: string | number;
}

/** Diagnostics event payload from the Tauri backend. */
export interface DiagnosticsEvent {
  uri: string;
  diagnostics: LspDiagnostic[];
}

/** CodeMirror lint Diagnostic format. */
export interface CmDiagnostic {
  from: number;
  to: number;
  severity: 'error' | 'warning' | 'info';
  message: string;
  source?: string;
}

// ── Helpers ────────────────────────────────────────────────────────────

/** Map LSP DiagnosticSeverity to CodeMirror severity string. */
function lspSeverityToCm(severity?: number): 'error' | 'warning' | 'info' {
  switch (severity) {
    case 1:
      return 'error';
    case 2:
      return 'warning';
    case 3:
    case 4:
    default:
      return 'info';
  }
}

/**
 * Convert an LSP diagnostic to a CodeMirror lint Diagnostic.
 *
 * Requires a document (or doc-like object with `line(n)`) to convert
 * LSP line/character positions to absolute offsets.
 */
export function lspDiagnosticToCodeMirror(
  diag: LspDiagnostic,
  doc: { line(n: number): { from: number; to: number } },
): CmDiagnostic {
  // LSP lines are 0-based, CodeMirror doc.line() is 1-based
  const startLine = doc.line(diag.range.start.line + 1);
  const endLine = doc.line(diag.range.end.line + 1);

  return {
    from: startLine.from + diag.range.start.character,
    to: endLine.from + diag.range.end.character,
    severity: lspSeverityToCm(diag.severity),
    message: diag.message,
    source: diag.source,
  };
}

// ── Event listener ─────────────────────────────────────────────────────

/**
 * Subscribe to the "diagnostics" Tauri event from the LSP bridge.
 *
 * The callback receives the raw DiagnosticsEvent payload containing
 * the document URI and an array of LSP diagnostics.
 *
 * Returns an unlisten function to unsubscribe.
 */
export async function createDiagnosticsListener(
  callback: (event: DiagnosticsEvent) => void,
): Promise<UnlistenFn> {
  return listen<DiagnosticsEvent>('diagnostics', (event) => {
    callback(event.payload);
  });
}
