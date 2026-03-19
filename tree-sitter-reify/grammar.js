/// @file Tree-sitter grammar for the Reify CAD language (M1 subset)
/// @author Reify Authors

module.exports = grammar({
  name: 'reify',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  rules: {
    source_file: $ => repeat($._declaration),

    _declaration: $ => choice(
      $.structure_definition,
      $.import_declaration,
    ),

    // ── Imports ──────────────────────────────────────────────
    import_declaration: $ => seq(
      'import',
      $.string_literal,
    ),

    // ── Structure ───────────────────────────────────────────
    structure_definition: $ => seq(
      'structure',
      field('name', $.identifier),
      '{',
      repeat($._member),
      '}',
    ),

    _member: $ => choice(
      $.param_declaration,
      $.let_declaration,
      $.constraint_declaration,
      $.sub_declaration,
    ),

    // ── Param ───────────────────────────────────────────────
    param_declaration: $ => seq(
      'param',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
      optional(seq('=', field('default', choice($.auto_keyword, $._expression)))),
    ),

    // ── Auto keyword (for solver-determined params) ───────
    auto_keyword: $ => 'auto',

    // ── Let ─────────────────────────────────────────────────
    let_declaration: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
      '=',
      field('value', $._expression),
    ),

    // ── Constraint ──────────────────────────────────────────
    // Note: optional label support deferred — M1 constraints have no labels.
    // Label syntax would need disambiguation (e.g., `constraint "label" expr`).
    constraint_declaration: $ => seq(
      'constraint',
      field('expr', $._expression),
    ),

    // ── Sub ─────────────────────────────────────────────────
    sub_declaration: $ => seq(
      'sub',
      field('name', $.identifier),
      '=',
      field('structure_name', $.identifier),
      '(',
      optional($.named_argument_list),
      ')',
    ),

    named_argument_list: $ => seq(
      $.named_argument,
      repeat(seq(',', $.named_argument)),
      optional(','),
    ),

    named_argument: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    // ── Types ───────────────────────────────────────────────
    type_expr: $ => $.identifier,

    // ── Expressions ─────────────────────────────────────────
    // Precedence (low → high):
    //   1: || (or)
    //   2: && (and)
    //   3: ==, != (equality)
    //   4: <, >, <=, >= (comparison)
    //   5: +, - (additive)
    //   6: *, / (multiplicative)
    //   7: unary -, ! (unary)
    //   8: postfix (member access, function call)

    _expression: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.conditional_expression,
      $._primary_expression,
    ),

    binary_expression: $ => choice(
      prec.left(1, seq(field('left', $._expression), field('op', '||'), field('right', $._expression))),
      prec.left(2, seq(field('left', $._expression), field('op', '&&'), field('right', $._expression))),
      prec.left(3, seq(field('left', $._expression), field('op', '=='), field('right', $._expression))),
      prec.left(3, seq(field('left', $._expression), field('op', '!='), field('right', $._expression))),
      prec.left(4, seq(field('left', $._expression), field('op', '>'), field('right', $._expression))),
      prec.left(4, seq(field('left', $._expression), field('op', '<'), field('right', $._expression))),
      prec.left(4, seq(field('left', $._expression), field('op', '>='), field('right', $._expression))),
      prec.left(4, seq(field('left', $._expression), field('op', '<='), field('right', $._expression))),
      prec.left(5, seq(field('left', $._expression), field('op', '+'), field('right', $._expression))),
      prec.left(5, seq(field('left', $._expression), field('op', '-'), field('right', $._expression))),
      prec.left(6, seq(field('left', $._expression), field('op', '*'), field('right', $._expression))),
      prec.left(6, seq(field('left', $._expression), field('op', '/'), field('right', $._expression))),
    ),

    unary_expression: $ => choice(
      prec(7, seq(field('op', '-'), field('operand', $._expression))),
      prec(7, seq(field('op', '!'), field('operand', $._expression))),
    ),

    conditional_expression: $ => prec.right(0, seq(
      'if',
      field('condition', $._expression),
      'then',
      field('then', $._expression),
      'else',
      field('else', $._expression),
    )),

    _primary_expression: $ => choice(
      $.quantity_literal,
      $.number_literal,
      $.string_literal,
      $.bool_literal,
      $.function_call,
      $.member_access,
      $.identifier,
      $.parenthesized_expression,
    ),

    // Quantity literal: number immediately followed by unit identifier (e.g. 80mm)
    // Use token.immediate to require no whitespace between number and unit
    quantity_literal: $ => seq(
      field('value', $.number_literal),
      field('unit', alias($.immediate_identifier, $.unit)),
    ),

    // An identifier that must immediately follow the previous token (no whitespace)
    immediate_identifier: $ => token.immediate(/[a-zA-Z_][a-zA-Z0-9_]*/),

    function_call: $ => prec(8, seq(
      field('name', $.identifier),
      '(',
      optional($.argument_list),
      ')',
    )),

    argument_list: $ => seq(
      $._expression,
      repeat(seq(',', $._expression)),
      optional(','),
    ),

    member_access: $ => prec.left(8, seq(
      field('object', $._expression),
      '.',
      field('member', $.identifier),
    )),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    // ── Literals ────────────────────────────────────────────
    number_literal: $ => token(/\d+(\.\d+)?/),

    string_literal: $ => token(seq(
      '"',
      repeat(choice(
        /[^"\\]/,
        seq('\\', /./),
      )),
      '"',
    )),

    bool_literal: $ => choice('true', 'false'),

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // ── Comments ────────────────────────────────────────────
    line_comment: $ => token(seq('//', /.*/)),

    block_comment: $ => token(seq(
      '/*',
      /[^*]*\*+([^/*][^*]*\*+)*/,
      '/',
    )),
  },
});
