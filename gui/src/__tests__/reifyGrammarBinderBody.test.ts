import { describe, it, expect } from 'vitest';
import { countErrorNodes, sourceOfNodeNamed } from './lezerProbeHelpers';

/**
 * Trailing binder bodies on the GUI Lezer grammar: a quantifier predicate, a
 * conditional's `else` branch and a lambda body each extend past every keyword
 * operator, `implies` included. Ruling: docs/reify-language-spec.md §16. The
 * tree-sitter twin is tree-sitter-reify/tests/binder_body_precedence_grammar_tests.rs.
 * Both groupings parse with zero errors, so the node's span is what pins the shape.
 */

/** Parse `expr` as a let value and assert the first `node` spans exactly `span`. */
function expectSpan(expr: string, node: string, span: string): void {
  const src = `structure def F { let r = ${expr} }`;
  expect(countErrorNodes(src)).toBe(0);
  expect(sourceOfNodeNamed(src, node)).toBe(span);
}

describe('reify.grammar — a trailing binder body extends past every keyword operator', () => {
  it.each([
    'forall v in xs: v > 0 and v < 3',
    'exists v in xs: v > 0 or v < 3',
    'forall v in xs: v > 0 implies v < 3',
  ])('QuantifierExpression spans all of `%s`', (expr) => {
    expectSpan(expr, 'QuantifierExpression', expr);
  });

  it.each(['if k then x else y and z', 'if k then x else y implies z'])(
    'ConditionalExpression spans all of `%s`',
    (expr) => {
      expectSpan(expr, 'ConditionalExpression', expr);
    },
  );

  it.each(['|v| v > 0 and v < 3', '|| a or b'])('LambdaExpression spans all of `%s`', (expr) => {
    expectSpan(expr, 'LambdaExpression', expr);
  });

  it('extends under a leading `not`', () => {
    expectSpan(
      'not forall v in xs: v > 0 and v < 3',
      'QuantifierExpression',
      'forall v in xs: v > 0 and v < 3',
    );
  });

  it('extends as the right operand of `and`', () => {
    expectSpan('a and forall v in xs: p or q', 'QuantifierExpression', 'forall v in xs: p or q');
  });
});

describe('reify.grammar — parentheses end a binder body', () => {
  it('stops the QuantifierExpression at the closing paren', () => {
    expectSpan('(forall v in xs: p) and q', 'QuantifierExpression', 'forall v in xs: p');
  });
});
