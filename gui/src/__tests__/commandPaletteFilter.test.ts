import { describe, it, expect } from 'vitest';
import {
  fuzzyScore,
  filterCommands,
  flattenSymbols,
  filterSymbols,
  symbolToLocation,
  type FlatSymbol,
} from '../components/commandPaletteFilter';
import type { DocumentSymbol } from '../editor/lspClient';
import type { PaletteCommand } from '../hooks/useKeyboardShortcuts';

// ── fuzzyScore ─────────────────────────────────────────────────────────────

describe('fuzzyScore', () => {
  it('returns a non-null score for a full subsequence match', () => {
    expect(fuzzyScore('open', 'Open file')).not.toBeNull();
  });

  it('is case-insensitive (query lowercase, text mixed case)', () => {
    expect(fuzzyScore('opn', 'Open file')).not.toBeNull();
  });

  it('is case-insensitive (query uppercase)', () => {
    expect(fuzzyScore('OPEN', 'Open file')).not.toBeNull();
  });

  it('returns null for a non-subsequence', () => {
    expect(fuzzyScore('xyz', 'Open file')).toBeNull();
  });

  it('returns null when query character is not present in text', () => {
    expect(fuzzyScore('zzz', 'Save file')).toBeNull();
  });

  it('returns a higher score for a contiguous match than a scattered one', () => {
    // 'open' matches contiguously in 'Open' — should score higher than scattered
    const scoreContiguous = fuzzyScore('open', 'Open viewport');
    const scoreScattered = fuzzyScore('open', 'overlay page entities node');
    expect(scoreContiguous).not.toBeNull();
    expect(scoreScattered).not.toBeNull();
    expect(scoreContiguous!).toBeGreaterThan(scoreScattered!);
  });

  it('returns a higher score for an earlier match position', () => {
    // 'save' at the start scores higher than 'save' buried in middle
    const scoreEarly = fuzzyScore('save', 'Save file');
    const scoreLate = fuzzyScore('save', 'Auto-save mode setting');
    expect(scoreEarly).not.toBeNull();
    expect(scoreLate).not.toBeNull();
    expect(scoreEarly!).toBeGreaterThan(scoreLate!);
  });

  it('returns a number (not null) when query equals text (exact match)', () => {
    const score = fuzzyScore('save', 'save');
    expect(score).not.toBeNull();
    expect(typeof score).toBe('number');
  });
});

// ── filterCommands ──────────────────────────────────────────────────────────

describe('filterCommands', () => {
  const CMDS: PaletteCommand[] = [
    { id: 'save',       title: 'Save file',       key: 'Ctrl+S' },
    { id: 'open',       title: 'Open file',       key: 'Ctrl+O' },
    { id: 'export',     title: 'Export',          key: 'Ctrl+E' },
    { id: 'reEvaluate', title: 'Re-evaluate',     key: 'F5'     },
  ];

  it('empty query returns all commands in original order', () => {
    const result = filterCommands(CMDS, '');
    expect(result.map((c) => c.id)).toEqual(['save', 'open', 'export', 'reEvaluate']);
  });

  it('filtering by "save" puts the Save command first', () => {
    const result = filterCommands(CMDS, 'save');
    expect(result.length).toBeGreaterThan(0);
    expect(result[0].id).toBe('save');
  });

  it('filtering by "open" puts the Open command first', () => {
    const result = filterCommands(CMDS, 'open');
    expect(result.length).toBeGreaterThan(0);
    expect(result[0].id).toBe('open');
  });

  it('drops non-matching entries', () => {
    const result = filterCommands(CMDS, 'zzz');
    expect(result).toHaveLength(0);
  });

  it('is case-insensitive', () => {
    const result = filterCommands(CMDS, 'SAVE');
    expect(result.some((c) => c.id === 'save')).toBe(true);
  });
});

// ── flattenSymbols ──────────────────────────────────────────────────────────

const makeRange = (l: number, c: number) => ({
  start: { line: l, character: c },
  end: { line: l, character: c + 5 },
});

describe('flattenSymbols', () => {
  it('returns [] for empty input', () => {
    expect(flattenSymbols([])).toEqual([]);
  });

  it('returns a flat list of depth-0 entries for top-level symbols', () => {
    const syms: DocumentSymbol[] = [
      { name: 'Foo', kind: 5, range: makeRange(0, 0), selectionRange: makeRange(0, 10) },
      { name: 'Bar', kind: 5, range: makeRange(5, 0), selectionRange: makeRange(5, 10) },
    ];
    const flat = flattenSymbols(syms);
    expect(flat).toHaveLength(2);
    expect(flat[0].name).toBe('Foo');
    expect(flat[0].depth).toBe(0);
    expect(flat[1].name).toBe('Bar');
    expect(flat[1].depth).toBe(0);
  });

  it('flattens nested children with depth-first order', () => {
    const syms: DocumentSymbol[] = [
      {
        name: 'Bracket',
        kind: 5,
        range: makeRange(0, 0),
        selectionRange: makeRange(0, 10),
        children: [
          { name: 'width',  kind: 13, range: makeRange(1, 2), selectionRange: makeRange(1, 2) },
          { name: 'height', kind: 13, range: makeRange(2, 2), selectionRange: makeRange(2, 2) },
        ],
      },
    ];
    const flat = flattenSymbols(syms);
    expect(flat).toHaveLength(3);
    expect(flat[0].name).toBe('Bracket');
    expect(flat[0].depth).toBe(0);
    expect(flat[1].name).toBe('width');
    expect(flat[1].depth).toBe(1);
    expect(flat[1].containerName).toBe('Bracket');
    expect(flat[2].name).toBe('height');
    expect(flat[2].depth).toBe(1);
    expect(flat[2].containerName).toBe('Bracket');
  });

  it('preserves source order across siblings', () => {
    const syms: DocumentSymbol[] = [
      { name: 'Alpha', kind: 5, range: makeRange(0, 0), selectionRange: makeRange(0, 0) },
      { name: 'Beta',  kind: 5, range: makeRange(2, 0), selectionRange: makeRange(2, 0) },
      { name: 'Gamma', kind: 5, range: makeRange(4, 0), selectionRange: makeRange(4, 0) },
    ];
    const flat = flattenSymbols(syms);
    expect(flat.map((s) => s.name)).toEqual(['Alpha', 'Beta', 'Gamma']);
  });
});

// ── filterSymbols ───────────────────────────────────────────────────────────

describe('filterSymbols', () => {
  const FLAT: FlatSymbol[] = [
    { name: 'Bracket', kind: 5, selectionRange: makeRange(0, 10), depth: 0, containerName: '' },
    { name: 'width',   kind: 13, selectionRange: makeRange(1, 2), depth: 1, containerName: 'Bracket' },
    { name: 'height',  kind: 13, selectionRange: makeRange(2, 2), depth: 1, containerName: 'Bracket' },
  ];

  it('empty query returns all symbols', () => {
    expect(filterSymbols(FLAT, '')).toHaveLength(3);
  });

  it('filters by name (subsequence match)', () => {
    const result = filterSymbols(FLAT, 'wid');
    expect(result.some((s) => s.name === 'width')).toBe(true);
    expect(result.some((s) => s.name === 'Bracket')).toBe(false);
  });

  it('returns [] when nothing matches', () => {
    expect(filterSymbols(FLAT, 'zzz')).toHaveLength(0);
  });
});

// ── symbolToLocation ────────────────────────────────────────────────────────

describe('symbolToLocation', () => {
  it('converts 0-based LSP line/character to 1-based SourceLocation', () => {
    const sym: FlatSymbol = {
      name: 'width',
      kind: 13,
      selectionRange: { start: { line: 9, character: 4 }, end: { line: 9, character: 9 } },
      depth: 1,
      containerName: 'Bracket',
    };
    const loc = symbolToLocation(sym, 'main.ri');
    expect(loc.file_path).toBe('main.ri');
    expect(loc.line).toBe(10);    // 0-based 9 → 1-based 10
    expect(loc.column).toBe(5);   // 0-based 4 → 1-based 5
    expect(loc.end_line).toBe(10);
    expect(loc.end_column).toBe(5);
  });

  it('uses selectionRange.start for both start and end column of the SourceLocation', () => {
    const sym: FlatSymbol = {
      name: 'height',
      kind: 13,
      selectionRange: { start: { line: 0, character: 0 }, end: { line: 0, character: 6 } },
      depth: 0,
      containerName: '',
    };
    const loc = symbolToLocation(sym, '/a/b.ri');
    expect(loc.line).toBe(1);
    expect(loc.column).toBe(1);
    expect(loc.end_line).toBe(1);
    expect(loc.end_column).toBe(1);
  });
});
