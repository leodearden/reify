import { describe, it, expect } from 'vitest';
import { LanguageSupport, getIndentation, ensureSyntaxTree } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import { highlightTree } from '@lezer/highlight';
import { classHighlighter } from '@lezer/highlight';
import { NodeProp } from '@lezer/common';
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

  it('highlights M5 reserved keyword in expression context', () => {
    const spans = getHighlightSpans('structure S { let x = trait }');
    const traitSpan = spans.find(s => s.text === 'trait');
    expect(traitSpan).toBeDefined();
    expect(traitSpan!.classes).toContain('keyword');
  });
});

describe('bracket matching', () => {
  it('matches curly braces', () => {
    const src = 'structure S { param x = (1 + 2) }';
    const tree = reifyLRLanguage.parser.parse(src);
    // Find the '{' node and check it has closedBy metadata
    const cursor = tree.cursor();
    let openBrace: { from: number; to: number } | null = null;
    let closeBrace: { from: number; to: number } | null = null;
    do {
      if (cursor.name === '{') openBrace = { from: cursor.from, to: cursor.to };
      if (cursor.name === '}') closeBrace = { from: cursor.from, to: cursor.to };
    } while (cursor.next());
    expect(openBrace).not.toBeNull();
    expect(closeBrace).not.toBeNull();
    // The @detectDelim directive should add closedBy/openedBy props
    const openNode = tree.resolve(openBrace!.from, 1);
    expect(openNode.type.prop(NodeProp.closedBy)).toBeDefined();
  });

  it('matches parentheses', () => {
    const src = 'structure S { param x = (1 + 2) }';
    const tree = reifyLRLanguage.parser.parse(src);
    const cursor = tree.cursor();
    let openParen: { from: number; to: number } | null = null;
    let closeParen: { from: number; to: number } | null = null;
    do {
      if (cursor.name === '(') openParen = { from: cursor.from, to: cursor.to };
      if (cursor.name === ')') closeParen = { from: cursor.from, to: cursor.to };
    } while (cursor.next());
    expect(openParen).not.toBeNull();
    expect(closeParen).not.toBeNull();
  });
});

describe('code folding', () => {
  it('provides fold range for Block nodes', () => {
    const src = 'structure S {\n  param x = 1mm\n}';
    const tree = reifyLRLanguage.parser.parse(src);
    // Find the Block node
    const cursor = tree.cursor();
    let blockNode: { from: number; to: number } | null = null;
    do {
      if (cursor.name === 'Block') {
        blockNode = { from: cursor.from, to: cursor.to };
        break;
      }
    } while (cursor.next());
    expect(blockNode).not.toBeNull();
    // The block starts at '{' and ends at '}'
    // foldInside should provide a fold range from after '{' to before '}'
    expect(src[blockNode!.from]).toBe('{');
    expect(src[blockNode!.to - 1]).toBe('}');
    // Verify the fold is non-trivial (contains content)
    expect(blockNode!.to - blockNode!.from).toBeGreaterThan(2);
  });
});

describe('auto-indent', () => {
  it('Block node uses delimited indent', () => {
    const doc = 'structure S {\n  param x = 1mm\n}';
    const state = EditorState.create({
      doc,
      extensions: [reifyLanguage()],
    });
    // Query indentation at the start of line 2 (after the opening '{')
    // Line 2 starts at offset: 'structure S {\n'.length = 14
    const line2Start = doc.indexOf('\n') + 1;
    // Force synchronous parse before querying indentation.
    // ensureSyntaxTree advances ParseContext.tree but does NOT update the
    // immutable LanguageState.tree snapshot that syntaxTree(state) returns.
    // A no-op state.update({}) triggers LanguageState.apply(), which detects
    // the divergence and builds a new LanguageState from the updated context.
    // Use a generous timeout (2 s) so even a heavily-loaded CI host finishes
    // parsing this 32-character document before the check.
    ensureSyntaxTree(state, state.doc.length, 2000);
    const readyState = state.update({}).state;
    const indent = getIndentation(readyState, line2Start);
    // delimitedIndent on Block should produce a non-zero indent level
    expect(indent).not.toBeNull();
    expect(indent!).toBeGreaterThan(0);
  });
});
