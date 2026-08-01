import { styleTags, tags as t } from '@lezer/highlight';

/**
 * Every word `reify.grammar` promotes to a real `kw<"…">` production.
 *
 * This array is the SINGLE source of truth for keyword styling: it is joined
 * into the `styleTags` selector below, so a word listed here is styled and a
 * word missing from it renders unstyled.
 *
 * Why the list must exist at all: a word promoted out of the `ReservedWord`
 * `@specialize` list into its own `kw<>` production stops matching the
 * `ReservedWord: t.keyword` rule (lezer-generator permits only one replacement
 * name per (base token, literal) pair, so the promotion *requires* removing it
 * from that list). Without an explicit entry here it would parse fine and
 * render unstyled — the silent-degradation failure mode.
 *
 * `gui/src/__tests__/reifyGrammarCorpus.test.ts` guards this data-driven:
 * it extracts every `kw<"…">` literal from `reify.grammar` and asserts each is
 * a member of this array, so a FUTURE promotion fails CI rather than silently
 * losing its styling.
 */
export const KEYWORDS = [
  'structure',
  'import',
  'param',
  'let',
  'constraint',
  'sub',
  'minimize',
  'maximize',
  'where',
  'else',
  'if',
  'then',
  'auto',
  'def',
  'module',
  'as',
  'pub',
  'set',
  'map',
  'not',
  'and',
  'or',
  'implies',
  'occurrence',
  'priv',
  'aux',
  'at',
];

export const reifyHighlighting = styleTags({
  // Current keywords (see KEYWORDS above).
  [KEYWORDS.join(' ')]: t.keyword,
  // M5 reserved keywords (forward-compat via @extend)
  ReservedWord: t.keyword,
  // Literals
  Number: t.number,
  QuantityLiteral: t.number,
  String: t.string,
  Boolean: t.bool,
  // Names
  "Identifier": t.variableName,
  // Operators
  ArithOp: t.operator,
  CompareOp: t.operator,
  // Comments
  LineComment: t.lineComment,
  BlockComment: t.blockComment,
  // Delimiters
  "( )": t.paren,
  "{ }": t.brace,
});
