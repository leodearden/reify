import { describe, it, expect } from 'vitest';
import { highlightTree, classHighlighter } from '@lezer/highlight';
import { parser } from '../editor/reifyParser.js';
import { reifyLRLanguage } from '../editor/reifyLanguage';

/**
 * The constrained-2d-sketch editor surface on the GUI Lezer grammar
 * (PRD constrained-2d-sketch): the `pub`/`priv`/`aux` prefix of grammar.js
 * `let_declaration`, the `sketch_block` member, and the positional
 * `auto_seed` argument. Shapes are pinned by span plus count, never by an
 * error count alone.
 */

// The helpers below are re-declared from `reifyGrammarCorpus.test.ts`, which
// documents why each exists (a vitest test file exports nothing). Read it for
// why a count and a span are both needed (#5957).

/** Parse `src` and count the error nodes the Lezer parser inserted. */
function countErrorNodes(src: string): number {
  const cursor = parser.parse(src).cursor();
  let errors = 0;
  do {
    if (cursor.type.isError) errors++;
  } while (cursor.next());
  return errors;
}

/** Parse `src` and count the nodes named `name`. */
function countNodesNamed(src: string, name: string): number {
  const cursor = parser.parse(src).cursor();
  let count = 0;
  do {
    if (cursor.type.name === name) count++;
  } while (cursor.next());
  return count;
}

/**
 * Node type names of every node whose span is EXACTLY the first occurrence of
 * `text` in `src`, outermost first. Throws when `text` is absent.
 */
function nodeNamesSpanning(src: string, text: string): string[] {
  const from = src.indexOf(text);
  if (from < 0) throw new Error(`no ${JSON.stringify(text)} in: ${src}`);
  const to = from + text.length;
  const cursor = parser.parse(src).cursor();
  const names: string[] = [];
  do {
    if (cursor.from === from && cursor.to === to) names.push(cursor.type.name);
  } while (cursor.next());
  return names;
}

/**
 * Source text spanned by the first node named `name`. Throws when no such node
 * exists.
 */
function sourceOfNodeNamed(src: string, name: string): string {
  const cursor = parser.parse(src).cursor();
  do {
    if (cursor.type.name === name) return src.slice(cursor.from, cursor.to);
  } while (cursor.next());
  throw new Error(`no ${name} node in parse of: ${src}`);
}

/** Source text of every span the editor's highlighter gave the class `cls`. */
function spansWithClass(src: string, cls: string): string[] {
  const tree = reifyLRLanguage.parser.parse(src);
  const spans: string[] = [];
  highlightTree(tree, classHighlighter, (from, to, classes) => {
    if (classes.split(' ').includes(cls)) spans.push(src.slice(from, to));
  });
  return spans;
}

/** Source text of every span the highlighter styled as a keyword. */
function keywordSpans(src: string): string[] {
  return spansWithClass(src, 'tok-keyword');
}

describe('reifyGrammarSketch — helper guards', () => {
  it('countErrorNodes still reports error nodes on input that cannot parse', () => {
    expect(countErrorNodes('@@@ !!! ???')).toBeGreaterThan(0);
  });

  it('countErrorNodes reports zero on input that parses', () => {
    expect(countErrorNodes('structure def Foo { }')).toBe(0);
  });
});

// ── `let` prefixes ──────────────────────────────────────────────────────────

describe('reify.grammar — `pub` / `priv` / `aux` prefixes on a let declaration', () => {
  it('absorbs `aux` into the LetDeclaration rather than leaving it beside it', () => {
    const src = 'structure def S { aux let cl = line(a, b) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'Member')).toBe(1);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe('aux let cl = line(a, b)');
  });

  it.each(['pub let x = 1mm', 'priv let y = 2mm', 'priv aux let z = 3mm'])(
    'starts the LetDeclaration at the prefix of `%s`',
    (decl) => {
      const src = `structure def S { ${decl} }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe(decl);
    },
  );

  it('admits the prefix in a constraint-definition body', () => {
    const src = 'constraint def C {\n  aux let x = 1\n  x > 0\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe('aux let x = 1');
    expect(countNodesNamed(src, 'ConstraintDefPredicate')).toBe(1);
  });

  it('admits the prefix in a port body', () => {
    expect(countErrorNodes('structure def S { port p : in F { aux let x = 1mm } }')).toBe(0);
  });

  it.each(['aux sub a = B()', 'priv aux sub a = B()'])(
    'still reads `%s` as one SubDeclaration',
    (decl) => {
      const src = `structure def S { ${decl} }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(countNodesNamed(src, 'SubDeclaration')).toBe(1);
    },
  );

  it('still reads `priv param` as one ParamDeclaration', () => {
    const src = 'structure def S { priv param t : Real = 5 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'ParamDeclaration')).toBe(1);
  });
});
