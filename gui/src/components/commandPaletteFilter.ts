/**
 * Pure filtering and coordinate-conversion utilities for the command palette.
 *
 * All functions are DOM-free and unit-testable without rendering.
 * Precedent: gui/src/stores/fuzzyPathMatcher.ts (pure matcher in its own module).
 */
import type { DocumentSymbol, Range } from '../editor/lspClient';
import type { PaletteCommand } from '../hooks/useKeyboardShortcuts';
import type { SourceLocation } from '../types';

export type { PaletteCommand };

// ── FlatSymbol ──────────────────────────────────────────────────────────────

export interface FlatSymbol {
  name: string;
  detail?: string;
  kind: number;
  selectionRange: Range;
  depth: number;
  containerName: string;
}

// ── fuzzyScore ──────────────────────────────────────────────────────────────

/**
 * Case-insensitive subsequence scorer.
 *
 * Returns a numeric score (higher = better match) when `query` is a
 * subsequence of `text`, otherwise returns `null`.
 *
 * Scoring rewards:
 *  - contiguous runs of matching characters (each step +10)
 *  - earlier match positions (penalty proportional to starting index)
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();

  if (q.length === 0) return 0;

  let qi = 0;
  let firstMatchIndex = -1;
  let score = 0;
  let consecutive = 0;

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      if (firstMatchIndex === -1) firstMatchIndex = ti;
      consecutive++;
      // Reward consecutive matches
      score += 10 * consecutive;
      qi++;
    } else {
      consecutive = 0;
    }
  }

  if (qi < q.length) {
    // Not all query characters matched
    return null;
  }

  // Penalise later start positions
  score -= firstMatchIndex;

  return score;
}

// ── filterCommands ──────────────────────────────────────────────────────────

/**
 * Filter and rank palette commands by a fuzzy query.
 *
 * Empty query returns all commands in original order.
 * Non-empty query drops non-matches and sorts by descending score.
 */
export function filterCommands(
  commands: PaletteCommand[],
  query: string,
): PaletteCommand[] {
  if (query === '') return [...commands];

  const scored: Array<{ cmd: PaletteCommand; score: number }> = [];
  for (const cmd of commands) {
    const score = fuzzyScore(query, cmd.title);
    if (score !== null) {
      scored.push({ cmd, score });
    }
  }

  // Stable sort: sort by score descending, preserving original order on ties.
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.cmd);
}

// ── flattenSymbols ──────────────────────────────────────────────────────────

/**
 * Depth-first flatten of a nested DocumentSymbol tree into a flat list.
 * Each entry carries its nesting depth and parent container name.
 */
export function flattenSymbols(
  symbols: DocumentSymbol[],
  depth = 0,
  containerName = '',
): FlatSymbol[] {
  const result: FlatSymbol[] = [];
  for (const sym of symbols) {
    result.push({
      name: sym.name,
      detail: sym.detail,
      kind: sym.kind,
      selectionRange: sym.selectionRange,
      depth,
      containerName,
    });
    if (sym.children && sym.children.length > 0) {
      result.push(...flattenSymbols(sym.children, depth + 1, sym.name));
    }
  }
  return result;
}

// ── filterSymbols ───────────────────────────────────────────────────────────

/**
 * Filter and rank flat symbols by a fuzzy query against their name.
 *
 * Empty query returns all symbols in original order.
 */
export function filterSymbols(flat: FlatSymbol[], query: string): FlatSymbol[] {
  if (query === '') return [...flat];

  const scored: Array<{ sym: FlatSymbol; score: number }> = [];
  for (const sym of flat) {
    const score = fuzzyScore(query, sym.name);
    if (score !== null) {
      scored.push({ sym, score });
    }
  }

  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.sym);
}

// ── symbolToLocation ────────────────────────────────────────────────────────

/**
 * Convert a FlatSymbol's 0-based LSP selectionRange.start into a 1-based
 * SourceLocation for the scrollToLocation / Editor reveal path.
 *
 * The Editor effect uses doc.line(location.line) (1-based) and
 * line.from + column - 1 (1-based column). Both start and end of the
 * returned SourceLocation use the symbol's selection-start so the cursor
 * lands precisely at the symbol name.
 */
export function symbolToLocation(sym: FlatSymbol, filePath: string): SourceLocation {
  const { line, character } = sym.selectionRange.start;
  return {
    file_path: filePath,
    line: line + 1,
    column: character + 1,
    end_line: line + 1,
    end_column: character + 1,
  };
}
