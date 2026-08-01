import { LRLanguage, LanguageSupport, foldNodeProp, foldInside, indentNodeProp, delimitedIndent } from '@codemirror/language';
import { parser } from './reifyParser.js';

/**
 * LR language definition for Reify.
 *
 * Configures the generated Lezer parser with folding (Block nodes)
 * and indentation (delimited indent for `{ }` blocks).
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
      // error nodes.
      foldNodeProp.add({
        Block: foldInside,
        // `FnBody` holds `FnLetBinding* result` rather than members
        // (grammar.js:239-246), so it could not reuse `Block`.
        FnBody: foldInside,
        // `SpecializationBody` additionally admits `ParamAssignment`
        // (grammar.js:900-920); `PortBody` admits a narrower member list plus
        // the two settings (:970-980); `ConnectBody` is comma-separated
        // (:1006-1013).
        SpecializationBody: foldInside,
        PortBody: foldInside,
        ConnectBody: foldInside,
      }),
      indentNodeProp.add({
        Block: delimitedIndent({ closing: '}' }),
        FnBody: delimitedIndent({ closing: '}' }),
        SpecializationBody: delimitedIndent({ closing: '}' }),
        PortBody: delimitedIndent({ closing: '}' }),
        ConnectBody: delimitedIndent({ closing: '}' }),
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
