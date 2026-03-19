import { styleTags, tags as t } from '@lezer/highlight';

export const reifyHighlighting = styleTags({
  // Current keywords
  "structure import param let constraint sub minimize maximize where else if then auto": t.keyword,
  // M5 reserved keywords (forward-compat)
  "trait fn enum match connect chain occurrence purpose field pub def port type module unit forall exists implies and or not in out meta self undef some none set map as": t.keyword,
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
