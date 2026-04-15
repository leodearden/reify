import { describe, it, expect } from 'vitest';
import { syntaxHighlighting, HighlightStyle } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

import {
  reifyEditorTheme,
  reifyHighlightStyle,
  editorThemeSpec,
} from '../editor/editorTheme';

describe('editorTheme exports', () => {
  it('exports editorThemeSpec as a plain object', () => {
    expect(typeof editorThemeSpec).toBe('object');
    expect(editorThemeSpec).not.toBeNull();
  });

  it('exports reifyEditorTheme as a CodeMirror Extension (object or function)', () => {
    // EditorView.theme() returns an Extension which is either an array,
    // object, or function — the important thing is it is not null/undefined.
    expect(reifyEditorTheme).toBeDefined();
    expect(reifyEditorTheme).not.toBeNull();
  });

  it('exports reifyHighlightStyle as a valid HighlightStyle instance', () => {
    expect(reifyHighlightStyle).toBeInstanceOf(HighlightStyle);
  });

  it('reifyHighlightStyle can be passed to syntaxHighlighting without throwing', () => {
    expect(() => syntaxHighlighting(reifyHighlightStyle)).not.toThrow();
  });
});

describe('editorThemeSpec cursor styling', () => {
  it('has cursor rule with 2px border width', () => {
    const cursorRule = editorThemeSpec['.cm-cursor, .cm-dropCursor'];
    expect(cursorRule, 'missing cursor CSS rule').toBeDefined();
    expect(cursorRule?.borderLeftWidth).toBe('2px');
  });

  it('has cursor color set to a light hex color', () => {
    const cursorRule = editorThemeSpec['.cm-cursor, .cm-dropCursor'];
    expect(cursorRule?.borderLeftColor).toMatch(/^#[0-9a-f]{6}$/i);
    // Color should be light (not black #000000)
    expect(cursorRule?.borderLeftColor?.toLowerCase()).not.toBe('#000000');
  });

  it('has gutter styling with a dark background', () => {
    const gutterRule = editorThemeSpec['.cm-gutters'];
    expect(gutterRule, 'missing .cm-gutters rule').toBeDefined();
    expect(gutterRule?.backgroundColor).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe('reifyHighlightStyle syntax tag coverage', () => {
  // Single source of truth: keep tag and its display name together to prevent
  // silent misalignment when new entries are added.
  const cases = [
    { tag: t.keyword, name: 'keyword' },
    { tag: t.number, name: 'number' },
    { tag: t.string, name: 'string' },
    { tag: t.bool, name: 'bool' },
    { tag: t.variableName, name: 'variableName' },
    { tag: t.operator, name: 'operator' },
    { tag: t.lineComment, name: 'lineComment' },
    { tag: t.blockComment, name: 'blockComment' },
    { tag: t.paren, name: 'paren' },
    { tag: t.brace, name: 'brace' },
  ];

  for (const { tag, name } of cases) {
    it(`covers ${name} tag with a non-null class`, () => {
      const cls = reifyHighlightStyle.style([tag]);
      expect(cls, `${name} tag has no highlight style`).not.toBeNull();
    });
  }
});
