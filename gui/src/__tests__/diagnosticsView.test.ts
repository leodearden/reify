import { describe, it, expect } from 'vitest';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';
import { diagnosticKey, groupDiagnostics } from '../panels/diagnosticsView';

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

function makeEntry(
  overrides: Partial<DiagnosticEntry> = {}
): DiagnosticEntry {
  return {
    file_path: 'test.ri',
    line: 1,
    column: 1,
    end_line: 1,
    end_column: 5,
    severity: 'Warning',
    message: 'default message',
    code: null,
    source: 'compile',
    ...overrides,
  };
}

// ────────────────────────────────────────────────────────────
// diagnosticKey
// ────────────────────────────────────────────────────────────

describe('diagnosticKey', () => {
  it('returns equal keys for identical entries', () => {
    const a = makeEntry();
    const b = makeEntry();
    expect(diagnosticKey(a)).toBe(diagnosticKey(b));
  });

  it('returns different keys when source differs', () => {
    const a = makeEntry({ source: 'compile' });
    const b = makeEntry({ source: 'tessellation' });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });

  it('returns different keys when severity differs', () => {
    const a = makeEntry({ severity: 'Error' });
    const b = makeEntry({ severity: 'Warning' });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });

  it('returns different keys when file_path differs', () => {
    const a = makeEntry({ file_path: 'alpha.ri' });
    const b = makeEntry({ file_path: 'beta.ri' });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });

  it('returns different keys when line differs', () => {
    const a = makeEntry({ line: 5 });
    const b = makeEntry({ line: 6 });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });

  it('returns different keys when column differs', () => {
    const a = makeEntry({ column: 1 });
    const b = makeEntry({ column: 2 });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });

  it('returns different keys when message differs', () => {
    const a = makeEntry({ message: 'foo' });
    const b = makeEntry({ message: 'bar' });
    expect(diagnosticKey(a)).not.toBe(diagnosticKey(b));
  });
});

// ────────────────────────────────────────────────────────────
// groupDiagnostics
// ────────────────────────────────────────────────────────────

describe('groupDiagnostics', () => {
  it('N identical entries collapse into one group with count N', () => {
    const entry = makeEntry({ message: 'same warning' });
    const dupes = [entry, makeEntry({ message: 'same warning' }), makeEntry({ message: 'same warning' })];
    const groups = groupDiagnostics(dupes);
    expect(groups).toHaveLength(1);
    expect(groups[0].count).toBe(3);
  });

  it('distinct entries produce separate count-1 groups', () => {
    const a = makeEntry({ message: 'warning A' });
    const b = makeEntry({ message: 'warning B' });
    const c = makeEntry({ message: 'warning C' });
    const groups = groupDiagnostics([a, b, c]);
    expect(groups).toHaveLength(3);
    expect(groups.every((g) => g.count === 1)).toBe(true);
  });

  it('preserves first-occurrence order', () => {
    const a = makeEntry({ message: 'first' });
    const b = makeEntry({ message: 'second' });
    const c = makeEntry({ message: 'third' });
    // Mix in a duplicate of 'first' after 'third'
    const groups = groupDiagnostics([a, b, c, makeEntry({ message: 'first' })]);
    expect(groups).toHaveLength(3);
    expect(groups[0].diagnostic.message).toBe('first');
    expect(groups[1].diagnostic.message).toBe('second');
    expect(groups[2].diagnostic.message).toBe('third');
    expect(groups[0].count).toBe(2);
  });

  it('representative diagnostic is the first occurrence (same object reference)', () => {
    const first = makeEntry({ message: 'repeated' });
    const second = makeEntry({ message: 'repeated' });
    const groups = groupDiagnostics([first, second]);
    expect(groups[0].diagnostic).toBe(first);
  });

  it('empty input produces empty output', () => {
    expect(groupDiagnostics([])).toEqual([]);
  });

  it('single entry produces one group with count 1', () => {
    const entry = makeEntry();
    const groups = groupDiagnostics([entry]);
    expect(groups).toHaveLength(1);
    expect(groups[0].count).toBe(1);
    expect(groups[0].diagnostic).toBe(entry);
  });

  it('groups by all key fields: source+severity+file+line+column+message', () => {
    // Same message + severity + file + line + column but different source → two groups
    const compile = makeEntry({ source: 'compile', message: 'dup' });
    const tess = makeEntry({ source: 'tessellation', message: 'dup' });
    const groups = groupDiagnostics([compile, tess, makeEntry({ source: 'compile', message: 'dup' })]);
    expect(groups).toHaveLength(2);
    const compileGroup = groups.find((g) => g.diagnostic.source === 'compile')!;
    const tessGroup = groups.find((g) => g.diagnostic.source === 'tessellation')!;
    expect(compileGroup.count).toBe(2);
    expect(tessGroup.count).toBe(1);
  });
});
