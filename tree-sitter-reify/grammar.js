/// @file Tree-sitter grammar for the Reify CAD language (M1 subset)
/// @author Reify Authors

module.exports = grammar({
  name: 'reify',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  conflicts: $ => [
    [$.param_declaration],
    [$.let_declaration],
    [$.constraint_declaration],
    [$.minimize_declaration],
    [$.maximize_declaration],
    [$.sub_declaration],
  ],

  rules: {
    source_file: $ => repeat($._declaration),

    _declaration: $ => choice(
      $.structure_definition,
      $.import_declaration,
      $.enum_declaration,
      $.trait_declaration,
    ),

    // ── Enum ──────────────────────────────────────────────────
    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      '{',
      optional(seq($.identifier, repeat(seq(',', $.identifier)), optional(','))),
      '}',
    ),

    // ── Imports ──────────────────────────────────────────────
    import_declaration: $ => seq(
      'import',
      $.string_literal,
    ),

    // ── Trait ────────────────────────────────────────────────
    trait_declaration: $ => seq(
      optional('pub'),
      'trait',
      field('name', $.identifier),
      optional($.type_parameters),
      optional(seq(':', $.trait_bound_list)),
      '{',
      repeat($.trait_member),
      '}',
    ),

    trait_member: $ => choice(
      $.param_declaration,
      $.let_declaration,
      $.constraint_declaration,
      $.sub_declaration,
      $.associated_type,
    ),

    // ── Associated type ─────────────────────────────────────
    associated_type: $ => seq(
      'type',
      field('name', $.identifier),
      optional(seq('=', field('default', $.type_expr))),
    ),

    // ── Trait bound list (used by trait refinements and structure bounds) ──
    trait_bound_list: $ => seq(
      $.identifier,
      repeat(seq('+', $.identifier)),
    ),

    // ── Type parameters ─────────────────────────────────────
    type_parameters: $ => seq(
      '<',
      $.type_parameter,
      repeat(seq(',', $.type_parameter)),
      optional(','),
      '>',
    ),

    type_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('bounds', $.trait_bound_list))),
    ),

    // ── Structure ───────────────────────────────────────────
    structure_definition: $ => seq(
      optional('pub'),
      'structure',
      optional('def'),
      field('name', $.identifier),
      optional($.type_parameters),
      optional(seq(':', $.trait_bound_list)),
      '{',
      repeat($._member),
      '}',
    ),

    _member: $ => choice(
      $.param_declaration,
      $.let_declaration,
      $.constraint_declaration,
      $.sub_declaration,
      $.minimize_declaration,
      $.maximize_declaration,
      $.guarded_block,
    ),

    // ── Where clause (guard) ────────────────────────────────
    where_clause: $ => seq(
      'where',
      field('condition', $._expression),
    ),

    // ── Guarded block ─────────────────────────────────────
    guarded_block: $ => seq(
      'where',
      field('condition', $._expression),
      '{',
      repeat($._member),
      '}',
      optional(seq('else', '{', repeat($._member), '}')),
    ),

    // ── Param ───────────────────────────────────────────────
    param_declaration: $ => seq(
      'param',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
      optional(seq('=', field('default', choice($.auto_keyword, $._expression)))),
      optional(field('guard', $.where_clause)),
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
      optional(field('guard', $.where_clause)),
    ),

    // ── Constraint ──────────────────────────────────────────
    // Note: optional label support deferred — M1 constraints have no labels.
    // Label syntax would need disambiguation (e.g., `constraint "label" expr`).
    constraint_declaration: $ => seq(
      'constraint',
      field('expr', $._expression),
      optional(field('guard', $.where_clause)),
    ),

    // ── Minimize ───────────────────────────────────────────
    minimize_declaration: $ => seq(
      'minimize',
      field('expr', $._expression),
      optional(field('guard', $.where_clause)),
    ),

    // ── Maximize ──────────────────────────────────────────
    maximize_declaration: $ => seq(
      'maximize',
      field('expr', $._expression),
      optional(field('guard', $.where_clause)),
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
      optional(field('guard', $.where_clause)),
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
