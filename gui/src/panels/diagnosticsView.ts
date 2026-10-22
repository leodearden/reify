import type { DiagnosticEntry } from './DiagnosticsPanel';
import type { DiagnosticInfo, SourceLocation } from '../types';

// ────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────

export type DiagnosticSource = 'compile' | 'tessellation';

export interface GroupedDiagnostic {
  /** Representative entry — always the first occurrence in input order. */
  diagnostic: DiagnosticEntry;
  /** Number of identical entries collapsed into this group (≥1). */
  count: number;
}

// ────────────────────────────────────────────────────────────
// diagnosticKey
// ────────────────────────────────────────────────────────────

/** Returns a stable string key that uniquely identifies a diagnostic by the
 *  six fields used to determine "sameness" for dedup/grouping purposes:
 *  source, severity, file_path, line, column, and message.
 *
 *  Uses a NUL-character delimiter (U+0000) because it never appears in
 *  user-visible strings, eliminating false-positive collisions between
 *  e.g. key("ab", "c") and key("a", "bc"). */
export function diagnosticKey(d: DiagnosticEntry): string {
  return [d.source, d.severity, d.file_path, d.line, d.column, d.message].join('\0');
}

// ────────────────────────────────────────────────────────────
// filterDiagnostics
// ────────────────────────────────────────────────────────────

export interface DiagnosticFilter {
  /** Which pipeline sources to include. */
  sources: Set<DiagnosticSource>;
  /** Which severity levels to include (e.g. 'Error', 'Warning', 'Info'). */
  severities: Set<string>;
}

/** Returns entries where d.source is in filter.sources AND d.severity is in
 *  filter.severities, preserving input order. */
export function filterDiagnostics(
  diags: DiagnosticEntry[],
  filter: DiagnosticFilter
): DiagnosticEntry[] {
  return diags.filter(
    (d) => filter.sources.has(d.source) && filter.severities.has(d.severity)
  );
}

// ────────────────────────────────────────────────────────────
// groupDiagnostics
// ────────────────────────────────────────────────────────────

/** Collapses identical diagnostics (same diagnosticKey) into GroupedDiagnostic
 *  entries preserving first-occurrence order.  The representative `diagnostic`
 *  in each group is the first occurrence in the input array. */
export function groupDiagnostics(diags: DiagnosticEntry[]): GroupedDiagnostic[] {
  const seen = new Map<string, GroupedDiagnostic>();
  const order: string[] = [];

  for (const d of diags) {
    const key = diagnosticKey(d);
    const existing = seen.get(key);
    if (existing) {
      existing.count += 1;
    } else {
      const group: GroupedDiagnostic = { diagnostic: d, count: 1 };
      seen.set(key, group);
      order.push(key);
    }
  }

  return order.map((k) => seen.get(k)!);
}

// ────────────────────────────────────────────────────────────
// diagnosticSourceLocation
// ────────────────────────────────────────────────────────────

/** The editor location a diagnostic points at, or null for a span-less one
 *  (`has_location === false`; absent means line-tied). */
export function diagnosticSourceLocation(d: DiagnosticInfo): SourceLocation | null {
  if (d.has_location === false) return null;
  return {
    file_path: d.file_path,
    line: d.line,
    column: d.column,
    end_line: d.end_line,
    end_column: d.end_column,
  };
}
