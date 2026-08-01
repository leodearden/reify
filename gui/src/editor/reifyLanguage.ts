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
      foldNodeProp.add({
        Block: foldInside,
        // `FnBody` is the one brace-delimited body that is NOT a `Block`
        // (grammar.js:239-246 gives a fn body its own rule, and its block arm
        // holds `FnLetBinding* result` rather than members). Without an entry
        // here a multi-line fn body would silently lose folding/indentation.
        FnBody: foldInside,
      }),
      indentNodeProp.add({
        Block: delimitedIndent({ closing: '}' }),
        FnBody: delimitedIndent({ closing: '}' }),
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
