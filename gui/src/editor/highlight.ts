import { styleTags, tags as t } from '@lezer/highlight';

export const reifyHighlighting = styleTags({
  // Current keywords.
  // `def`, `module`, `as` and `pub` are named explicitly because they were
  // promoted out of the ReservedWord @specialize list into real productions
  // (lezer-generator permits only one replacement name per (base token,
  // literal) pair). Without them here they would parse fine but render
  // unstyled. Any future keyword promotion must be added to this list in the
  // same commit — gui/src/__tests__/reifyGrammarCorpus.test.ts asserts it.
  "structure import param let constraint sub minimize maximize where else if then auto def module as pub":
    t.keyword,
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
