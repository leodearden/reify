/**
 * Diagnostics bridge: subscribes to Tauri "diagnostics" events from the
 * in-process LSP server and converts LSP diagnostics to CodeMirror lint format.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Text } from '@codemirror/state';
import type { DiagnosticInfo } from '../types';
import { lspRangeToCmRange, lspPositionToOffset } from './lspRange';

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
 * Convert an LSP diagnostic to a CodeMirror lint Diagnostic, or null when the
 * diagnostic's line range is outside the current document (version skew).
 *
 * Uses `lspRangeToCmRange` for the offset mapping, which clamps over-long
 * characters to `line.to` and returns null on out-of-range lines.  The sole
 * caller (Editor.tsx:506-514) already filters null values with
 * `.filter((d): d is CmDiagnostic => d !== null)`, so null is transparently
 * absorbed without a caller change.
 */
export function lspDiagnosticToCodeMirror(
  diag: LspDiagnostic,
  doc: { line(n: number): { from: number; to: number } },
): CmDiagnostic | null {
  const r = lspRangeToCmRange(doc, diag.range);
  if (!r) return null;

  return {
    from: r.from,
    to: r.to,
    severity: lspSeverityToCm(diag.severity),
    message: diag.message,
    source: diag.source,
  };
}

// ── Engine compile-diagnostic mappers ─────────────────────────────────

/** Map PascalCase DiagnosticInfo severity to CodeMirror severity string. */
export function diagnosticInfoSeverityToCm(severity: string): 'error' | 'warning' | 'info' {
  switch (severity) {
    case 'Error':
      return 'error';
    case 'Warning':
      return 'warning';
    default:
      return 'info';
  }
}

/**
 * Convert a DiagnosticInfo (from the engine compile-diagnostics channel) to a
 * CodeMirror lint Diagnostic, or null when the position data is out of range.
 *
 * DiagnosticInfo uses 1-based line/column.  Returns null when line or end_line
 * exceeds the document's line count so callers can safely filter(Boolean).
 */
export function diagnosticInfoToCmDiagnostic(
  diag: DiagnosticInfo,
  doc: { lines: number; line(n: number): { from: number; to: number } },
): CmDiagnostic | null {
  if (diag.line > doc.lines || diag.end_line > doc.lines) {
    return null;
  }
  try {
    // DiagnosticInfo uses 1-based line/column; lspPositionToOffset expects 0-based.
    // doc.line((line-1)+1) == doc.line(line), so the arithmetic is identical.
    const from = lspPositionToOffset(doc, diag.line - 1, diag.column - 1);
    const to = lspPositionToOffset(doc, diag.end_line - 1, diag.end_column - 1);

    return {
      from,
      to,
      severity: diagnosticInfoSeverityToCm(diag.severity),
      message: diag.message,
      source: 'compile',
    };
  } catch {
    return null;
  }
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
