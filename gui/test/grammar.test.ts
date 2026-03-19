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

describe('Lezer grammar – additional syntax constructs', () => {
  it('parses import declaration', () => {
    const tree = parser.parse('import "std/prelude"');
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'ImportDeclaration')).toBe(1);
    const text = findFirstNodeText(tree, 'String', 'import "std/prelude"');
    expect(text).toBe('"std/prelude"');
  });

  it('parses sub declaration with named arguments', () => {
    const src = 'structure S { sub h = Hole(diameter: 6mm) }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'SubDeclaration')).toBe(1);
    expect(countNodes(tree, 'NamedArgument')).toBe(1);
  });

  it('parses where clause on param', () => {
    const src = 'structure S { param x = 1mm where active }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'ParamDeclaration')).toBe(1);
    expect(countNodes(tree, 'WhereClause')).toBe(1);
  });

  it('parses guarded block', () => {
    const src = 'structure S { where cond { param x = 1mm } }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'GuardedBlock')).toBe(1);
    // GuardedBlock contains a Block which contains a ParamDeclaration
    expect(countNodes(tree, 'ParamDeclaration')).toBe(1);
  });

  it('parses conditional expression', () => {
    const src = 'structure S { let x = if a then b else c }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'ConditionalExpression')).toBe(1);
  });

  it('parses member access', () => {
    const src = 'structure S { let x = a.b.c }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    // a.b.c = (a.b).c = two nested MemberAccess nodes
    expect(countNodes(tree, 'MemberAccess')).toBe(2);
  });
});

describe('Lezer grammar – expression precedence', () => {
  /** Parse an expression inside a let declaration and return the expression's top node. */
  function parseExpr(expr: string) {
    const src = `structure S { let x = ${expr} }`;
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    // Navigate: SourceFile > Declaration > StructureDefinition > Block > Member > LetDeclaration
    const letDecl = tree.topNode
      .getChild('Declaration')
      ?.getChild('StructureDefinition')
      ?.getChild('Block')
      ?.getChild('Member')
      ?.getChild('LetDeclaration');
    expect(letDecl).not.toBeNull();
    // The expression is the child after "=" (skip "let", Identifier, "=")
    // In Lezer, the expression child of LetDeclaration is the rightmost Expression group child
    const children: { name: string; from: number; to: number }[] = [];
    for (let c = letDecl!.firstChild; c; c = c.nextSibling) {
      children.push({ name: c.name, from: c.from, to: c.to });
    }
    // Find the expression (BinaryExpression, UnaryExpression, MemberAccess, etc.)
    const exprNode = children.filter(
      c => !['let', 'Identifier', '=', 'TypeAnnotation', 'WhereClause'].includes(c.name)
    ).pop();
    return { tree, letDecl: letDecl!, exprNode, src };
  }

  it('multiplication binds tighter than addition: a + b * c', () => {
    const { letDecl, src } = parseExpr('a + b * c');
    // Top expression should be BinaryExpression with +
    // Find the top-level expression child of LetDeclaration
    const topExpr = letDecl.lastChild?.prevSibling; // last non-} child
    // The outermost BinaryExpression should be addition
    const cursor = letDecl.cursor();
    const binExprs: { text: string; from: number; to: number }[] = [];
    do {
      if (cursor.name === 'BinaryExpression') {
        binExprs.push({ text: src.slice(cursor.from, cursor.to), from: cursor.from, to: cursor.to });
      }
    } while (cursor.next());
    // Should have 2 binary expressions: 'a + b * c' (outer) and 'b * c' (inner)
    expect(binExprs.length).toBe(2);
    // The longer one is the outer addition
    binExprs.sort((a, b) => (b.to - b.from) - (a.to - a.from));
    expect(binExprs[0].text).toBe('a + b * c');
    expect(binExprs[1].text).toBe('b * c');
  });

  it('&& binds tighter than ||: a || b && c', () => {
    const { letDecl, src } = parseExpr('a || b && c');
    const binExprs: string[] = [];
    const cursor = letDecl.cursor();
    do {
      if (cursor.name === 'BinaryExpression') {
        binExprs.push(src.slice(cursor.from, cursor.to));
      }
    } while (cursor.next());
    binExprs.sort((a, b) => b.length - a.length);
    expect(binExprs[0]).toBe('a || b && c');
    expect(binExprs[1]).toBe('b && c');
  });

  it('comparison binds tighter than equality: a == b < c', () => {
    const { letDecl, src } = parseExpr('a == b < c');
    const binExprs: string[] = [];
    const cursor = letDecl.cursor();
    do {
      if (cursor.name === 'BinaryExpression') {
        binExprs.push(src.slice(cursor.from, cursor.to));
      }
    } while (cursor.next());
    binExprs.sort((a, b) => b.length - a.length);
    expect(binExprs[0]).toBe('a == b < c');
    expect(binExprs[1]).toBe('b < c');
  });

  it('unary minus binds tighter than addition: -a + b', () => {
    const { letDecl, src } = parseExpr('-a + b');
    // Should have one UnaryExpression (-a) and one BinaryExpression (-a + b)
    let hasUnary = false;
    let topBin = '';
    const cursor = letDecl.cursor();
    do {
      if (cursor.name === 'UnaryExpression') hasUnary = true;
      if (cursor.name === 'BinaryExpression') topBin = src.slice(cursor.from, cursor.to);
    } while (cursor.next());
    expect(hasUnary).toBe(true);
    expect(topBin).toBe('-a + b');
  });

  it('member access binds tighter than addition: a.b + c', () => {
    const { letDecl, src } = parseExpr('a.b + c');
    let hasMember = false;
    let topBin = '';
    const cursor = letDecl.cursor();
    do {
      if (cursor.name === 'MemberAccess') hasMember = true;
      if (cursor.name === 'BinaryExpression') topBin = src.slice(cursor.from, cursor.to);
    } while (cursor.next());
    expect(hasMember).toBe(true);
    expect(topBin).toBe('a.b + c');
  });
});

describe('Lezer grammar – comments', () => {
  it('handles line comment before structure', () => {
    const src = '// line comment\nstructure S {}';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'LineComment')).toBe(1);
    expect(countNodes(tree, 'StructureDefinition')).toBe(1);
  });

  it('handles block comment before structure', () => {
    const src = '/* block\ncomment */ structure S {}';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'BlockComment')).toBe(1);
    expect(countNodes(tree, 'StructureDefinition')).toBe(1);
  });

  it('handles inline block comment inside structure body', () => {
    const src = 'structure S { /* inline */ param x = 1mm }';
    const tree = parser.parse(src);
    expect(hasErrors(tree)).toBe(false);
    expect(countNodes(tree, 'BlockComment')).toBe(1);
    expect(countNodes(tree, 'ParamDeclaration')).toBe(1);
  });
});
