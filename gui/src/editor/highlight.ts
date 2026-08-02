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
  'trait',
  'fn',
  'type',
  // Contextual (`ekw<"self">`): styled as a keyword in the fn receiver slot,
  // and left as an ordinary identifier in expression position, which is how
  // tree-sitter reads it too.
  'self',
  // The `auto(free)` modifier — a `kw<>` production inside AutoKeyword.
  'free',
  // Topology member families (PortDeclaration, ConnectStatement,
  // ChainStatement, ForallStatement) and the quantifier expression.
  'port',
  'connect',
  'chain',
  'forall',
  'exists',
  'in',
  'out',
  'bidi',
  // Contextual (`ekw<>`), for the same reason `at` and `self` are: all three
  // are attested as ordinary identifiers in committed `.ri` (`let direction =
  // normalize(velocity)`, `param frame : Frame3 = …`, `frame: mid_f`), so they
  // are keywords only in a port body's setting slot and plain names elsewhere.
  'direction',
  'frame',
  // EnumDeclaration and MatchExpression.
  'enum',
  'match',
  // The remaining declaration families. Half are `ekw<>` (contextual) because
  // the corpus uses them as ordinary identifiers: `source` 37 times, `offset`
  // 8, `field:`/`unit:` as argument labels, `composed`/`imported` as
  // let/sub names.
  'field',
  'source',
  'analytical',
  'sampled',
  'composed',
  'imported',
  'purpose',
  'unit',
  'offset',
  'default',
  'meta',
  // The `relate { … }` member block. Contextual (`ekw<"relate">`) to match
  // upstream (grammar.js:714-719), even though the corpus uses `relate` as an
  // identifier nowhere — see the note on RelateBlock in reify.grammar.
  'relate',
];

export const reifyHighlighting = styleTags({
  // Current keywords (see KEYWORDS above).
  [KEYWORDS.join(' ')]: t.keyword,
  // M5 reserved keywords (forward-compat via @extend)
  ReservedWord: t.keyword,
  // Literals
  Number: t.number,
  QuantityLiteral: t.number,
  // `0xFF` / `0b1010` and `4.1j`. Node names, not `kw<>` words, so they belong
  // here rather than in KEYWORDS.
  RadixLiteral: t.number,
  ImaginaryLiteral: t.number,
  String: t.string,
  // An interpolated string is styled per PART, which is the whole point of
  // giving it a shape: the literal runs are string-coloured and each `{expr}`
  // hole keeps the ordinary expression styling of its contents.
  StringChunk: t.string,
  Interpolation: t.special(t.string),
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
