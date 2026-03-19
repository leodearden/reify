import { describe, it, expect } from 'vitest';
import { LanguageSupport } from '@codemirror/language';
import { reifyLanguage } from '../src/editor/reifyLanguage.js';

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
