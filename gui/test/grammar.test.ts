import { describe, it, expect } from 'vitest';
import { parser } from '../src/editor/reifyParser.js';
import { readFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

/** Collect all node names (including error nodes) from a parse tree. */
function collectNodes(tree: ReturnType<typeof parser.parse>): string[] {
  const nodes: string[] = [];
  const cursor = tree.cursor();
  do {
    nodes.push(cursor.name);
  } while (cursor.next());
  return nodes;
}

/** Check if a parse tree contains any error nodes. */
function hasErrors(tree: ReturnType<typeof parser.parse>): boolean {
  return collectNodes(tree).some(name => name === '⚠');
}

describe('Lezer grammar – basic', () => {
  it('parses an empty string as SourceFile', () => {
    const tree = parser.parse('');
    expect(tree.topNode.name).toBe('SourceFile');
  });
});

describe('Lezer grammar – bracket.ri fixture', () => {
  const bracketRi = readFileSync(
    resolve(__dirname, 'fixtures/bracket.ri'),
    'utf-8',
  );

  it('parses bracket.ri without error nodes', () => {
    const tree = parser.parse(bracketRi);
    expect(tree.topNode.name).toBe('SourceFile');
    expect(hasErrors(tree)).toBe(false);
  });
});
