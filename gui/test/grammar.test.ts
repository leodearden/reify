import { describe, it, expect } from 'vitest';
import { parser } from '../src/editor/reifyParser.js';

describe('Lezer grammar – basic', () => {
  it('parses an empty string as SourceFile', () => {
    const tree = parser.parse('');
    expect(tree.topNode.name).toBe('SourceFile');
  });
});
