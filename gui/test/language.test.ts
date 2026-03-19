import { describe, it, expect } from 'vitest';
import { LanguageSupport } from '@codemirror/language';
import { highlightTree } from '@lezer/highlight';
import { classHighlighter } from '@lezer/highlight';
import { reifyLanguage, reifyLRLanguage } from '../src/editor/reifyLanguage.js';

describe('reifyLanguage()', () => {
  it('returns a LanguageSupport instance', () => {
    const support = reifyLanguage();
    expect(support).toBeInstanceOf(LanguageSupport);
  });

  it('has language named reify', () => {
    const support = reifyLanguage();
    expect(support.language.name).toBe('reify');
  });

  it('has an accessible parser', () => {
    const support = reifyLanguage();
    expect(support.language.parser).toBeDefined();
  });
});

describe('syntax highlighting', () => {
  /** Collect highlight spans from source code. */
  function getHighlightSpans(src: string): { from: number; to: number; classes: string; text: string }[] {
    const tree = reifyLRLanguage.parser.parse(src);
    const spans: { from: number; to: number; classes: string; text: string }[] = [];
    highlightTree(tree, classHighlighter, (from, to, classes) => {
      spans.push({ from, to, classes, text: src.slice(from, to) });
    });
    return spans;
  }

  it('highlights "structure" as keyword', () => {
    const spans = getHighlightSpans('structure Bracket { param width = 80mm }');
    const structureSpan = spans.find(s => s.text === 'structure');
    expect(structureSpan).toBeDefined();
    expect(structureSpan!.classes).toContain('keyword');
  });

  it('highlights "param" as keyword', () => {
    const spans = getHighlightSpans('structure Bracket { param width = 80mm }');
    const paramSpan = spans.find(s => s.text === 'param');
    expect(paramSpan).toBeDefined();
    expect(paramSpan!.classes).toContain('keyword');
  });

  it('highlights quantity literal as number', () => {
    const spans = getHighlightSpans('structure Bracket { param width = 80mm }');
    const numSpan = spans.find(s => s.text === '80mm');
    expect(numSpan).toBeDefined();
    expect(numSpan!.classes).toContain('number');
  });

  it('highlights identifier as variableName', () => {
    const spans = getHighlightSpans('structure Bracket { param width = 80mm }');
    const widthSpan = spans.find(s => s.text === 'width');
    expect(widthSpan).toBeDefined();
    expect(widthSpan!.classes).toContain('variableName');
  });
});
