import { describe, it, expect } from 'vitest';
import { computeUnifiedDiff, type DiffLine } from '../utils/diff';

describe('computeUnifiedDiff', () => {
  it('returns all context lines for identical strings', () => {
    const text = 'line 1\nline 2\nline 3';
    const result = computeUnifiedDiff(text, text);
    expect(result.every((l) => l.type === 'context')).toBe(true);
    expect(result.map((l) => l.content)).toEqual(['line 1', 'line 2', 'line 3']);
  });

  it('returns all add lines for pure additions (empty before)', () => {
    const result = computeUnifiedDiff('', 'new line 1\nnew line 2');
    expect(result.every((l) => l.type === 'add')).toBe(true);
    expect(result.map((l) => l.content)).toEqual(['new line 1', 'new line 2']);
  });

  it('returns all remove lines for pure deletions (empty after)', () => {
    const result = computeUnifiedDiff('old line 1\nold line 2', '');
    expect(result.every((l) => l.type === 'remove')).toBe(true);
    expect(result.map((l) => l.content)).toEqual(['old line 1', 'old line 2']);
  });

  it('produces one remove + one add for a single line change', () => {
    const before = 'line 1\noriginal\nline 3';
    const after = 'line 1\nmodified\nline 3';
    const result = computeUnifiedDiff(before, after);
    const removes = result.filter((l) => l.type === 'remove');
    const adds = result.filter((l) => l.type === 'add');
    expect(removes).toHaveLength(1);
    expect(removes[0].content).toBe('original');
    expect(adds).toHaveLength(1);
    expect(adds[0].content).toBe('modified');
  });

  it('handles mixed changes with context lines', () => {
    const before = 'a\nb\nc\nd\ne';
    const after = 'a\nB\nc\nd\nE';
    const result = computeUnifiedDiff(before, after);
    // 'a' context, 'b' remove, 'B' add, 'c' context, 'd' context, 'e' remove, 'E' add
    const types = result.map((l) => l.type);
    expect(types).toContain('context');
    expect(types).toContain('remove');
    expect(types).toContain('add');
    // Check specific changes
    expect(result.find((l) => l.type === 'remove' && l.content === 'b')).toBeTruthy();
    expect(result.find((l) => l.type === 'add' && l.content === 'B')).toBeTruthy();
    expect(result.find((l) => l.type === 'remove' && l.content === 'e')).toBeTruthy();
    expect(result.find((l) => l.type === 'add' && l.content === 'E')).toBeTruthy();
  });

  it('returns empty array for both empty strings', () => {
    const result = computeUnifiedDiff('', '');
    expect(result).toEqual([]);
  });

  it('handles insertions in the middle', () => {
    const before = 'line 1\nline 3';
    const after = 'line 1\nline 2\nline 3';
    const result = computeUnifiedDiff(before, after);
    const adds = result.filter((l) => l.type === 'add');
    expect(adds).toHaveLength(1);
    expect(adds[0].content).toBe('line 2');
    // Original lines should be context
    const contexts = result.filter((l) => l.type === 'context');
    expect(contexts.map((l) => l.content)).toEqual(['line 1', 'line 3']);
  });
});
