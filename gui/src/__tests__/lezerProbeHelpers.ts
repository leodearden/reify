import { highlightTree, classHighlighter } from '@lezer/highlight';
import { parser } from '../editor/reifyParser.js';
import { reifyLRLanguage } from '../editor/reifyLanguage';

/**
 * Probes that parse a Reify snippet with the GUI Lezer grammar
 * (`gui/src/editor/reify.grammar`) and measure the resulting tree, shared by
 * the grammar test files. An error count alone pins little: a regression that
 * parses the same input into a different shape keeps it at zero. The count,
 * span and styling probes below are what pin the shape.
 *
 * `countErrorNodes` carries an anti-vacuity guard in
 * `lezerProbeHelpers.test.ts`.
 */

/** Parse `src` and count the error nodes the Lezer parser inserted. */
export function countErrorNodes(src: string): number {
  const cursor = parser.parse(src).cursor();
  let errors = 0;
  do {
    if (cursor.type.isError) errors++;
  } while (cursor.next());
  return errors;
}

/** Parse `src` and collect the set of node type names in the resulting tree. */
export function nodeNames(src: string): Set<string> {
  const cursor = parser.parse(src).cursor();
  const names = new Set<string>();
  do {
    names.add(cursor.type.name);
  } while (cursor.next());
  return names;
}

/**
 * Parse `src` and count the nodes named `name`.
 *
 * `nodeNames` collapses to a set, so a production that emits the SAME node
 * type nested inside itself reads as one hit and the redundancy is invisible.
 * That is not hypothetical: naming a rule and the token it wraps identically
 * made every wildcard arm come out as `WildcardPattern > WildcardPattern`
 * while `toContain('WildcardPattern')` stayed green (#5957). Use this wherever
 * the tree SHAPE, not just the presence of a name, is the contract.
 */
export function countNodesNamed(src: string, name: string): number {
  const cursor = parser.parse(src).cursor();
  let count = 0;
  do {
    if (cursor.type.name === name) count++;
  } while (cursor.next());
  return count;
}

/**
 * Node type names of every node whose span is EXACTLY the first occurrence of
 * `text` in `src`, outermost first — the discriminator for "what did this token
 * reduce TO?".
 *
 * `countNodesNamed(src, 'X') === 0` answers only what a token is NOT, and stays
 * green for every misparse that drops the token from the tree, absorbs it into
 * its parent with no child of its own, or brings it back under a third name.
 * Naming what actually spans the token separates those from the intended tree.
 * MEASURED: `A | _` gives `['Identifier']`; bare `_ => …` gives
 * `['MatchPattern', 'WildcardPattern']`. Throws rather than returning `[]` when
 * `text` is absent, so a typo'd needle fails loudly instead of vacuously.
 */
export function nodeNamesSpanning(src: string, text: string): string[] {
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
 * Source text SPANNED by the first node named `name` — the discriminator for
 * "was this token absorbed into that node, or left beside it?".
 *
 * A count cannot answer that. An optional leading keyword parsed as a stray
 * SIBLING leaves the count of the node it should have joined at exactly 1, so
 * `countNodesNamed(..., 'ParamDeclaration') === 1` passes on both the right
 * tree and the wrong one; only the node's `from` offset separates them. Throws
 * rather than returning `''` when the node is absent, so a typo'd name fails
 * loudly instead of vacuously.
 */
export function sourceOfNodeNamed(src: string, name: string): string {
  const cursor = parser.parse(src).cursor();
  do {
    if (cursor.type.name === name) return src.slice(cursor.from, cursor.to);
  } while (cursor.next());
  throw new Error(`no ${name} node in parse of: ${src}`);
}

/**
 * Drives `reifyLRLanguage` — the exact object the editor uses, already wired
 * with the `@external propSource` — through `highlightTree`, and collects the
 * source text of every span that received the class `cls`. A styleTags
 * selector names a NODE, so a token with no node in the tree yields no span.
 */
function spansWithClass(src: string, cls: string): string[] {
  const tree = reifyLRLanguage.parser.parse(src);
  const spans: string[] = [];
  highlightTree(tree, classHighlighter, (from, to, classes) => {
    if (classes.split(' ').includes(cls)) spans.push(src.slice(from, to));
  });
  return spans;
}

/** Source text of every span the highlighter styled as a keyword. */
export function keywordSpans(src: string): string[] {
  return spansWithClass(src, 'tok-keyword');
}

/**
 * The same measurement for punctuation — `t.brace`, `t.paren` and friends all
 * land in `tok-punctuation` under `classHighlighter`.
 */
export function punctuationSpans(src: string): string[] {
  return spansWithClass(src, 'tok-punctuation');
}
