import { LRLanguage, LanguageSupport, foldNodeProp, indentNodeProp, delimitedIndent } from '@codemirror/language';
import type { SyntaxNode } from '@lezer/common';
import { parser } from './reifyParser.js';

/**
 * Brace-delimited bodies whose `{` is their FIRST child.
 *
 * `Block` is `"{" Member* "}"`. `FnBody` holds `FnLetBinding* result` rather
 * than members (grammar.js:239-246), so it could not reuse `Block`;
 * `SpecializationBody` additionally admits `ParamAssignment` (:900-920);
 * `PortBody` admits a narrower member list plus the two settings (:970-980);
 * `ConnectBody` is comma-separated (:1006-1013); `ImportItems` is the
 * destructured import's `{A, B}`.
 */
export const BRACE_FIRST_BODIES = [
  'Block',
  'FnBody',
  'SpecializationBody',
  'PortBody',
  'ConnectBody',
  'ImportItems',
];

/**
 * Brace-delimited bodies introduced by a KEYWORD or a NAME before the `{` —
 * `match k { … }`, `enum E { … }`, `meta { … }`, `set { … }`, `map { … }`,
 * `Circle { radius: 5mm }`, `field def f : A -> B { source = … }`,
 * `analytical { … }`, `purpose P(…) { … }`.
 *
 * They are separated from the list above because the leading token changes
 * which helper is correct — see `foldBody` and the indent note below.
 */
export const KEYWORD_LED_BODIES = [
  'MatchExpression',
  'EnumDeclaration',
  'EnumVariant',
  'MetaBlock',
  'SetLiteral',
  'MapLiteral',
  'VariantConstruction',
  'VariantBindingPattern',
  'FieldDefinition',
  'FieldSource',
  'PurposeDeclaration',
];

/**
 * Fold range for a brace-delimited body: everything between the body's OWN
 * `{` and `}`.
 *
 * `foldInside` cannot serve every body here. It keys off the first and last
 * child, which is the `{`/`}` pair only when the brace OPENS the node. MEASURED
 * on `enum Color { Red, Green }`: the children are `enum`, `Identifier`, `{`,
 * `EnumVariant`, …, `}` — so `foldInside` would start the range immediately
 * after `enum` and fold the enum's NAME away. Literal tokens are real nodes in
 * the generated tree (also measured), so the `{` is addressable directly and
 * one helper covers both lists.
 *
 * Returns null when there is no brace at all, which is what `FnBody`'s second
 * arm (`= expression`, grammar.js:239-246) needs.
 */
function foldBody(node: SyntaxNode): { from: number; to: number } | null {
  const open = node.getChild('{');
  const close = node.lastChild;
  if (!open || !close || close.name !== '}' || open.to >= close.from) return null;
  return { from: open.to, to: close.from };
}

function propsFor<T>(names: readonly string[], value: T): Record<string, T> {
  return Object.fromEntries(names.map((name) => [name, value]));
}

/**
 * LR language definition for Reify.
 *
 * Configures the generated Lezer parser with folding and indentation for every
 * brace-delimited body the grammar declares.
 * Syntax highlighting is provided via `@external propSource` in the grammar.
 */
export const reifyLRLanguage = LRLanguage.define({
  name: 'reify',
  parser: parser.configure({
    props: [
      // Every brace-delimited body in the grammar needs an entry here, not
      // just `Block`: a body that is its own node type is invisible to these
      // props and silently loses folding and indentation while still parsing
      // cleanly — a failure the corpus test cannot see, because it counts
      // error nodes. `reifyGrammarCorpus.test.ts` derives the brace-delimited
      // node types from the grammar SOURCE and requires that both lists above
      // cover them, so the invariant is enforced rather than asserted.
      //
      // ONE DELIBERATE EXCLUSION: `Interpolation` (`"{" expression "}"`, the
      // hole inside an interpolated string). It is brace-delimited but it is
      // not a body — folding a string hole hides the string's own content, and
      // it lives in the grammar's no-skip context where indentation of the
      // surrounding line is not the editor's to manage.
      foldNodeProp.add({
        ...propsFor(BRACE_FIRST_BODIES, foldBody),
        ...propsFor(KEYWORD_LED_BODIES, foldBody),
      }),
      indentNodeProp.add({
        ...propsFor(BRACE_FIRST_BODIES, delimitedIndent({ closing: '}' })),
        // `align: false` for the keyword-led bodies. `delimitedIndent`'s
        // alignment path also keys off the FIRST child, so with the default
        // `align: true` the arms of `match k { … }` would align to the column
        // just past `match` rather than indenting one unit from the statement.
        // Turning alignment off falls back to `baseIndent + one unit`, which is
        // the correct reading for a keyword-led body.
        ...propsFor(KEYWORD_LED_BODIES, delimitedIndent({ closing: '}', align: false })),
      }),
    ],
  }),
  languageData: {
    closeBrackets: { brackets: ['(', '{', '[', '"'] },
    commentTokens: { line: '//', block: { open: '/*', close: '*/' } },
  },
});

/**
 * Returns a `LanguageSupport` instance for the Reify language,
 * suitable for use with CodeMirror 6 editors.
 */
export function reifyLanguage(): LanguageSupport {
  return new LanguageSupport(reifyLRLanguage);
}
