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

/** Count nodes of a given type in a parse tree. */
function countNodes(tree: ReturnType<typeof parser.parse>, name: string): number {
  let count = 0;
  const cursor = tree.cursor();
  do {
    if (cursor.name === name) count++;
  } while (cursor.next());
  return count;
}

/** Find the first node of a given type and return its text. */
function findFirstNodeText(
  tree: ReturnType<typeof parser.parse>,
  name: string,
  source: string,
): string | null {
  const cursor = tree.cursor();
  do {
    if (cursor.name === name) {
      return source.slice(cursor.from, cursor.to);
    }
  } while (cursor.next());
  return null;
}

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

describe('Lezer grammar – bracket.ri node structure', () => {
  const bracketRi = readFileSync(
    resolve(__dirname, 'fixtures/bracket.ri'),
    'utf-8',
  );
  const tree = parser.parse(bracketRi);

  it('has StructureDefinition with Identifier Bracket', () => {
    expect(countNodes(tree, 'StructureDefinition')).toBe(1);
    // StructureDefinition is inside a Declaration wrapper
    const declNode = tree.topNode.getChild('Declaration');
    expect(declNode).not.toBeNull();
    const structNode = declNode!.getChild('StructureDefinition');
    expect(structNode).not.toBeNull();
    const nameNode = structNode!.getChild('Identifier');
    expect(nameNode).not.toBeNull();
    expect(bracketRi.slice(nameNode!.from, nameNode!.to)).toBe('Bracket');
  });

  it('has 5 ParamDeclarations', () => {
    expect(countNodes(tree, 'ParamDeclaration')).toBe(5);
  });

  it('has 2 LetDeclarations', () => {
    expect(countNodes(tree, 'LetDeclaration')).toBe(2);
  });

  it('has 3 ConstraintDeclarations', () => {
    expect(countNodes(tree, 'ConstraintDeclaration')).toBe(3);
  });

  it('has QuantityLiteral for 80mm', () => {
    const text = findFirstNodeText(tree, 'QuantityLiteral', bracketRi);
    expect(text).toBe('80mm');
  });

  it('has BinaryExpression nodes for arithmetic', () => {
    // width * height * thickness produces nested BinaryExpression
    expect(countNodes(tree, 'BinaryExpression')).toBeGreaterThanOrEqual(1);
  });
});
