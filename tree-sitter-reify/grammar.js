/// @file Tree-sitter grammar for the Reify CAD language (M1 subset)
/// @author Reify Authors

/**
 * Comma-separated list of `rule`, with optional trailing comma.
 * Returns `optional(seq(rule, repeat(seq(',', rule)), optional(',')))`.
 */
function commaSep(rule) {
  return optional(seq(rule, repeat(seq(',', rule)), optional(',')));
}

/**
 * Parenthesised argument-list tail shared by call-shaped rules:
 * function_call, ad_hoc_selector, trait_method_call.
 * Returns `seq('(', optional($.argument_list), ')')`.
 */
function callTail($) {
  return seq('(', optional($.argument_list), ')');
}

module.exports = grammar({
  name: 'reify',

  externals: $ => [
    $._unit_expr_start,
    $._unit_mul_op,
    $._unit_div_op,
    // AUTO_TOKEN: emitted (consuming 'auto') by the external scanner.
    // Leading underscore keeps the CST node hidden so (auto_keyword) stays
    // the visible node — not (auto_keyword (auto_token)) — preserving corpus
    // compatibility with auto_type_arg.txt and existing tests.
    $._auto_token,
    // AUTO_RESERVATION_SENTINEL: referenced from `extras` so the external
    // scanner is invoked at EVERY lex position.  The scanner NEVER emits this
    // token; it exists only to keep the scanner subscribed so that it can emit
    // AUTO_TOKEN even at operand positions where AUTO_TOKEN is not in
    // valid_symbols (producing ERROR via out-of-valid emission).
    $._auto_reservation_sentinel,
    // RADIX_LITERAL: emitted (consuming the whole 0x.../0b... run) by the
    // external scanner.  A plain token() regex alternative for hex/binary would
    // be defeated by $._auto_reservation_sentinel in `extras`: the decimal DFA
    // matches only the leading `0`, then _unit_expr_start fires at `x`/`b`
    // (both unit-start chars) and the DFA consumes the rest as a unit_expr
    // unit_name — yielding quantity_literal(number_literal "0", unit_expr "xFF").
    // Only an external token consuming the WHOLE 0xFF/0b1010 run defeats this.
    $._radix_literal,
  ],

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
    // Sentinel that keeps the external scanner subscribed at every position
    // so it can fire AUTO_TOKEN (and force ERROR) at operand positions.
    $._auto_reservation_sentinel,
  ],

  conflicts: $ => [
    [$.param_declaration],
    [$.let_declaration],
    [$.constraint_declaration],
    [$.constraint_instantiation],
    [$.minimize_declaration],
    [$.maximize_declaration],
    [$.sub_declaration],
    [$.param_assignment],
    [$.port_declaration],
    [$.pragma],
    [$.named_argument_list, $.argument_list],
    [$.constraint_instantiation, $.constraint_declaration],
    [$.type_expr, $.parameterized_type],
    // function_definition and function_signature share a common prefix (fn name
    // type_params '(' fn_param_list ')' optional('->' type_expr)) and diverge
    // only at '{' (body) vs end-of-member.  This entry keeps tree-sitter's GLR
    // split stable even if a future type_expr change introduces a brace-shaped
    // right edge.
    [$.function_definition, $.function_signature],
    // variant_construction (`Name { field: value }`) shares a common prefix with
    // any `_primary_expression` that reduces to a bare `identifier` when the next
    // token is `{`.  The GLR resolves the ambiguity by keeping both forks alive;
    // the construction fork dies if the brace content is not `field: value` pairs
    // (e.g. match arms or guarded-block members).
    // tree-sitter generate error: "Add a conflict for these rules:
    //   `_primary_expression`, `variant_construction`".
    // PRD §4.4 / task α (data-carrying-enums, step-6).
    [$._primary_expression, $.variant_construction],
  ],

  rules: {
    source_file: $ => seq(optional($.module_declaration), repeat($._declaration)),

    // `module company.products.actuators`
    // Placed ONLY here (not in _declaration) so that a module decl after any
    // other declaration is a natural parse ERROR — the grammar enforces the
    // top-of-file / one-per-file rule (PRD D-5, §7.2) with no extra code.
    module_declaration: $ => seq('module', field('path', $.import_path)),

    _declaration: $ => choice(
      $.structure_definition,
      $.occurrence_definition,
      $.import_declaration,
      $.enum_declaration,
      $.function_definition,
      $.trait_declaration,
      $.field_definition,
      $.purpose_declaration,
      $.constraint_definition,
      $.unit_declaration,
      $.type_alias_declaration,
      $.pragma,
      $.annotation,
    ),

    // ── Enum ──────────────────────────────────────────────────
    enum_declaration: $ => seq(
      optional('pub'),
      'enum',
      field('name', $.identifier),
      optional($.type_parameters),
      '{',
      commaSep($.enum_variant),
      '}',
    ),

    // A single variant in an enum body: either a bare name (`Point`) or a
    // named-field form (`Circle { radius: Length }`).
    // PRD §4.4 / task α (data-carrying-enums).
    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(seq(
        '{',
        $.variant_field_decl,
        repeat(seq(',', $.variant_field_decl)),
        optional(','),
        '}',
      )),
    ),

    // A single `field: Type` pair inside an enum_variant payload.
    // PRD §4.4 / task α.
    variant_field_decl: $ => seq(
      field('field', $.identifier),
      ':',
      field('type', $.type_expr),
    ),

    // ── Function ─────────────────────────────────────────────
    // NOTE: optional('pub') is retained here because function_definition serves
    // both top-level and trait_member contexts.  In the trait_member arm, `pub`
    // is grammatically accepted but semantically vacuous — trait visibility is
    // governed by the trait declaration itself.  The lowering pass (task γ)
    // diagnoses `pub fn` inside a trait body.  function_signature (below) omits
    // `pub` because it is only reachable via trait_member.
    function_definition: $ => seq(
      optional('pub'),
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      '(',
      optional($.fn_param_list),
      ')',
      optional(seq('->', field('return_type', $.type_expr))),
      $.fn_body,
    ),

    // Bodyless fn signature — only reachable via trait_member (not in _declaration).
    // Represents a required (no-default) associated function in a trait body.
    // Sibling of function_definition: same prefix but no fn_body, no optional('pub').
    function_signature: $ => seq(
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      '(',
      optional($.fn_param_list),
      ')',
      optional(seq('->', field('return_type', $.type_expr))),
    ),

    fn_param_list: $ => choice(
      // Self-led: `self` receiver with optional following typed params.
      // Downstream uses child_by_field_name("receiver") to detect the receiver.
      seq(
        field('receiver', 'self'),
        optional(seq(',', $.fn_param, repeat(seq(',', $.fn_param)), optional(','))),
      ),
      // Typed params only (existing behaviour — no self receiver).
      seq(
        $.fn_param,
        repeat(seq(',', $.fn_param)),
        optional(','),
      ),
    ),

    fn_param: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $.type_expr),
      optional(seq('=', field('default', $._expression))),
    ),

    fn_body: $ => choice(
      // Block form: `{ [let y = e;]* result_expr }`
      seq('{', repeat($.fn_let_binding), field('result', $._expression), '}'),
      // Expression form: `= result_expr`  (spec §18 #10 — pure desugar)
      // The `=` token disambiguates from the block form at the grammar level;
      // lower_fn_body is unchanged because both arms share the `result` field.
      seq('=', field('result', $._expression)),
    ),

    fn_let_binding: $ => seq(
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
      '=',
      field('value', $._expression),
      ';',
    ),

    // ── Imports ──────────────────────────────────────────────
    import_declaration: $ => seq(
      optional('pub'),
      'import',
      field('path', $.import_path),
      optional(choice(
        // Destructured: import a.b.{C, D}
        field('items', $.import_items),
        // Aliased: import a.b as x  OR  import a.b.C as X
        seq('as', field('alias', $.identifier)),
      )),
    ),

    // Dot-separated module path: `std.mechanical.fasteners`
    import_path: $ => seq(
      $.identifier,
      repeat(seq('.', $.identifier)),
    ),

    // Destructured import items: `{Bolt, Nut}`
    import_items: $ => seq(
      '{',
      commaSep($.identifier),
      '}',
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
      $.function_definition,
      $.function_signature,
      $.pragma,
    ),

    // ── Field definition ─────────────────────────────────────
    field_definition: $ => seq(
      optional('pub'),
      'field',
      'def',
      field('name', $.identifier),
      ':',
      field('domain', $.type_expr),
      '->',
      field('codomain', $.type_expr),
      '{',
      'source',
      '=',
      field('source', $.field_source),
      '}',
    ),

    field_source: $ => choice(
      $.field_source_analytical,
      $.field_source_sampled,
      $.field_source_composed,
      $.field_source_imported,
    ),

    field_source_analytical: $ => seq(
      'analytical',
      '{',
      field('expr', $._expression),
      '}',
    ),

    field_source_sampled: $ => seq(
      'sampled',
      '{',
      repeat($.field_config_entry),
      '}',
    ),

    field_config_entry: $ => seq(
      field('key', $.identifier),
      '=',
      field('value', $._expression),
    ),

    field_source_composed: $ => seq(
      'composed',
      '{',
      field('expr', $._expression),
      '}',
    ),

    field_source_imported: $ => seq(
      'imported',
      '{',
      repeat($.field_config_entry),
      '}',
    ),

    // ── Purpose ───────────────────────────────────────────────
    purpose_declaration: $ => seq(
      optional('pub'),
      'purpose',
      field('name', $.identifier),
      optional($.type_parameters),
      '(',
      commaSep($.purpose_param),
      ')',
      '{',
      repeat($.purpose_member),
      '}',
    ),

    purpose_param: $ => seq(
      field('name', $.identifier),
      ':',
      field('entity_kind', $.identifier),
    ),

    purpose_member: $ => choice(
      $.constraint_declaration,
      $.let_declaration,
      $.minimize_declaration,
      $.maximize_declaration,
      $.guarded_block,
      $.pragma,
    ),

    // ── Constraint definition (top-level) ────────────────────
    // `constraint def Name<T> { param x : Length  x > 0 }`
    // Distinct from member-level `constraint_declaration` which starts with
    // `constraint <expr>`. The required `def` keyword disambiguates.
    constraint_definition: $ => seq(
      optional('pub'),
      'constraint',
      'def',
      field('name', $.identifier),
      optional($.type_parameters),
      '{',
      repeat($._constraint_def_body_item),
      '}',
    ),

    _constraint_def_body_item: $ => choice(
      $.param_declaration,
      $.let_declaration,
      $.constraint_def_predicate,
      $.pragma,
    ),

    // A bare expression predicate inside a constraint def body.
    // Named node so the lowering code can identify it by kind.
    constraint_def_predicate: $ => field('expr', $._expression),

    // ── Unit declaration (top-level) ─────────────────────────
    // `unit meter : Length`
    // `unit mm : Length = 0.001`
    // `unit degC : Temperature = 1 offset 273.15`
    unit_declaration: $ => seq(
      optional('pub'),
      'unit',
      field('name', $.identifier),
      ':',
      field('type', $.type_expr),
      optional(seq('=', field('conversion', $._expression))),
      optional(seq('offset', field('offset', $._expression))),
    ),

    // ── Type alias (top-level) ─────────────────────────────
    // `type Pressure = Force / Area`
    // `type Stress<T> = Force / Area`
    type_alias_declaration: $ => seq(
      optional('pub'),
      'type',
      field('name', $.identifier),
      optional($.type_parameters),
      '=',
      field('type', $.dimensional_type_expr),
    ),

    // ── Associated type ─────────────────────────────────────
    associated_type: $ => seq(
      'type',
      field('name', $.identifier),
      optional(seq('=', field('default', $.type_expr))),
    ),

    // ── Trait bound list (used by trait refinements and structure bounds) ──
    trait_bound_list: $ => seq(
      $.trait_bound_entry,
      repeat(seq('+', $.trait_bound_entry)),
    ),

    trait_bound_entry: $ => seq(
      field('name', $.identifier),
      optional(field('type_args', seq('<', $.type_arg_list, '>'))),
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
      optional(seq('=', field('default', $.type_expr))),
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

    // ── Occurrence ────────────────────────────────────────────
    occurrence_definition: $ => seq(
      optional('pub'),
      'occurrence',
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
      $.constraint_instantiation,
      $.constraint_declaration,
      $.sub_declaration,
      $.minimize_declaration,
      $.maximize_declaration,
      $.guarded_block,
      $.port_declaration,
      $.connect_statement,
      $.chain_statement,
      $.forall_statement,
      $.meta_block,
      $.annotation,
      $.pragma,
      $.match_arm_decl_block,
    ),

    // ── Meta block ──────────────────────────────────────────
    meta_block: $ => seq(
      'meta',
      '{',
      commaSep($.meta_entry),
      '}',
    ),

    meta_entry: $ => seq(
      field('key', $.identifier),
      '=',
      field('value', $.string_literal),
    ),

    // ── Constraint instantiation (member-level) ──────────────
    // `constraint ConstraintName(arg: expr, ...)` inside structure bodies.
    // The required named_argument_list (name: value) disambiguates from
    // constraint_declaration (which parses an arbitrary expression).
    //
    // prec.dynamic(1, ...) breaks the tie against constraint_declaration —
    // since `function_call` now accepts named arguments via `argument_list`,
    // `constraint MinWall(wall: t)` is a valid constraint_declaration too
    // (a function-call expression). Prefer the named-arg-list interpretation.
    constraint_instantiation: $ => prec.dynamic(1, seq(
      'constraint',
      field('name', $.identifier),
      '(',
      $.named_argument_list,
      ')',
      optional(field('guard', $.where_clause)),
    )),

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
      optional(seq('=', field('default', $._binding_value))),
      optional(field('guard', $.where_clause)),
    ),

    // ── Auto keyword (for solver-determined params) ───────
    // Accepts bare `auto` or `auto(free)`.  The presence of the `modifier`
    // field child indicates the free modifier is present.  The longer
    // `auto(free)` form is given higher precedence to resolve the shift-reduce
    // conflict that arises when `(` immediately follows `auto`.
    //
    // Uses $._auto_token (external scanner token) instead of the string
    // literal 'auto' so that the lexer-level reservation via the external
    // scanner is enforced.  _auto_token is leading-underscore hidden so the
    // CST shape remains (auto_keyword) / (auto_keyword (modifier)) — no
    // (auto_keyword (auto_token)) wrapper node.
    auto_keyword: $ => choice(
      prec(1, seq($._auto_token, '(', field('modifier', 'free'), ')')),
      $._auto_token,
    ),

    // ── Let ─────────────────────────────────────────────────
    let_declaration: $ => seq(
      optional('pub'),
      optional('aux'),
      'let',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
      '=',
      field('value', $._binding_value),
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
    sub_declaration: $ => choice(
      // Instantiation form: sub name = StructName<TypeArgs>(args)
      seq(
        optional('aux'),
        'sub',
        field('name', $.identifier),
        '=',
        field('structure_name', $.identifier),
        optional(field('type_args', seq('<', $.type_arg_list, '>'))),
        '(',
        optional($.named_argument_list),
        ')',
        optional(field('guard', $.where_clause)),
        optional(seq('at', field('pose', $._expression))),
      ),
      // Collection form: sub name : List<StructName>
      // The bare `'List'` token is reached only on exact-length matches —
      // see the long comment on the specialization arm below for the full
      // tree-sitter rule #1 / rule #2 reasoning and the regression lock.
      //
      // NOTE: `at <pose>` is grammatically accepted on the collection form
      // for uniformity with the other two arms, but per-element collection
      // placement is semantically out-of-scope (PRD §10). The compiler (T2,
      // sub-placement lowering) is responsible for emitting a diagnostic when
      // `pose_expr.is_some()` on a collection-form `SubDecl`; this grammar
      // deliberately does not enforce that restriction at parse time.
      seq(
        optional('aux'),
        'sub',
        field('name', $.identifier),
        ':',
        'List',
        '<',
        field('structure_name', $.identifier),
        '>',
        optional(field('guard', $.where_clause)),
        optional(seq('at', field('pose', $._expression))),
      ),
      // Specialization form: sub name : StructName <typeargs>? where? { body }?
      //
      // Disambiguation from the collection arm above relies on tree-sitter's
      // documented lexer rules — NOT on choice-arm order or on `prec(...)`:
      //
      //   Rule #1 (longest match, evaluated FIRST): the lexer picks the token
      //   whose match consumes the most characters. For `Listicle<Foo>`, the
      //   $.identifier regex matches 8 chars while the bare string `'List'`
      //   matches only 4 — so the identifier wins and this specialization arm
      //   is taken with structure_name == "Listicle".
      //
      //   Rule #2 (string-vs-regex tie-break, on equal-length matches): an
      //   anonymous string/keyword token wins over a regex token of the same
      //   length. For `List<Foo>`, both `'List'` and $.identifier match exactly
      //   4 chars on "List", so `'List'` wins and the collection arm above is
      //   taken — leaving "Foo" to be matched as structure_name.
      //
      // Together these two rules give: `List<X>` → collection arm; everything
      // else (`Foo<X>`, `Listicle<X>`, `MyList<X>`, …) → this specialization
      // arm. The invariant is pinned by four tests in
      // `crates/reify-syntax/tests/sub_decl_specialization_body_parser_tests.rs`:
      //   - `sub_decl_collection_form_regression`: AST-level positive case —
      //     `List<Foo>` → collection arm (rule #2 win, pre-existing regression pin)
      //   - `sub_decl_non_list_specialization_arm`: rule #2 negative control —
      //     `Foo<Bar>` must NOT be captured by the collection arm
      //   - `sub_decl_listicle_longest_match`: rule #1 longest-match guard —
      //     `Listicle<Foo>` must reach this specialization arm (not collection)
      //   - `sub_decl_cst_shape_for_list_collection`: CST-level pin — confirms
      //     `List` is consumed as the collection keyword (not as structure_name)
      //
      // History: an earlier plan proposed `token(prec(1, 'List'))` here to make
      // the precedence "explicit" in the grammar. That mechanism does NOT
      // respect rule #1 — `token(prec(...))` causes the lexer to emit 'List'
      // even when 'Listicle' would be a longer match, breaking Case 3.
      // Bare `'List'` (relying on rules #1 + #2) is the correct mechanism.
      // See escalation esc-3712-201 for the empirical evidence.
      seq(
        optional('aux'),
        'sub',
        field('name', $.identifier),
        ':',
        field('structure_name', $.identifier),
        optional(field('type_args', seq('<', $.type_arg_list, '>'))),
        optional(field('guard', $.where_clause)),
        optional(field('body', choice($.specialization_body, $.keyed_member_block))),
        optional(seq('at', field('pose', $._expression))),
      ),
    ),

    // ── Specialization body ──────────────────────────────────
    // Body of a specialization-scope sub: `{ repeat(param_assignment | _member) }`.
    // Accepts both permitted (let, constraint, connect, where) and forbidden
    // (param, port, sub) member kinds — rejection is deferred to the validator
    // (task 3571/3573) per spec §8.7 and triage-log §B3.
    specialization_body: $ => seq(
      '{',
      repeat(choice($.param_assignment, $._member)),
      '}',
    ),

    // ── Keyed sub-member block (task 3929, PRD §2.2) ─────────────────────────
    // Keyed sub-member block: `{ "key" => { overrides }  ... }`.
    // Used as the body of `sub name : Keyed<T> { "k" => { overrides } }`.
    //
    // Disambiguation from `specialization_body`:
    //   Both block forms open with `{`.  `keyed_member_block` requires at
    //   least ONE entry (`repeat1`), so an empty `{}` is unambiguously a
    //   `specialization_body` (which uses `repeat`, i.e. zero-or-more).
    //   When the block is non-empty, the first token after `{` determines
    //   the winner: a `string_literal` leads to `keyed_member_block`; an
    //   identifier or member-keyword leads to `specialization_body`.
    //   A `conflicts` entry is added only if `tree-sitter generate` reports
    //   an unresolved conflict between the two rules.
    //
    // Scope note: the grammar accepts a keyed block after ANY `structure_name<…>`,
    // not only after `Keyed<…>`.  Restricting the keyed block to the Keyed
    // collection kind is a resolution/compiler concern (PRD task β), mirroring
    // the established pattern where `at <pose>` is grammatically accepted on
    // the collection form but semantically rejected by the compiler (PRD §10).
    keyed_member_block: $ => seq(
      '{',
      repeat1($.keyed_member_entry),
      '}',
    ),

    // A single keyed entry: `"key" => { overrides }`.
    // The `overrides` specialization_body is reused verbatim (PRD §2.2/§9-Q4)
    // so override blocks inside a keyed sub parse identically to a
    // standalone specialization-scope body.
    keyed_member_entry: $ => seq(
      field('key', $.string_literal),
      '=>',
      field('overrides', $.specialization_body),
    ),

    // ── Param assignment (specialization body only) ──────────
    // Bare `name = expr where?` parameter assignments permitted in §8.7.
    // Scoped to specialization_body only — not added to _member — because
    // no existing _member starts with bare `identifier =`, so scoping avoids
    // widening the general member grammar.
    // Related: `connect_param_assignment` (below, line ~600) has the same
    // `name = value` shape but is scoped to connect-body and has no `where`
    // guard.  The distinct names prevent confusion between the two contexts.
    param_assignment: $ => seq(
      field('name', $.identifier),
      '=',
      field('value', $._binding_value),
      optional(field('guard', $.where_clause)),
    ),

    // ── Port ─────────────────────────────────────────────────
    port_declaration: $ => seq(
      'port',
      field('name', $.identifier),
      ':',
      optional(field('direction', $.port_direction_keyword)),
      field('type', $.identifier),
      optional(field('body', $.port_body)),
      optional(field('guard', $.where_clause)),
    ),

    port_direction_keyword: $ => choice('in', 'out', 'bidi'),

    port_body: $ => seq(
      '{',
      repeat(choice(
        $.param_declaration,
        $.let_declaration,
        $.constraint_declaration,
        $.port_direction_setting,
        $.port_frame_setting,
      )),
      '}',
    ),

    port_direction_setting: $ => seq(
      'direction',
      '=',
      field('value', $.port_direction_keyword),
    ),

    port_frame_setting: $ => seq(
      'frame',
      '=',
      field('value', $._expression),
    ),

    // ── Connect ───────────────────────────────────────────────
    connect_statement: $ => seq(
      'connect',
      field('left', $.port_ref),
      field('operator', $.connect_operator),
      field('right', $.port_ref),
      optional(seq(':', field('connector_type', $.identifier))),
      optional(field('body', $.connect_body)),
    ),

    connect_operator: $ => choice('->', '<-', '<->'),

    port_ref: $ => $._expression,

    connect_body: $ => seq(
      '{',
      commaSep(choice(
        $.port_mapping,
        $.connect_param_assignment,
      )),
      '}',
    ),

    port_mapping: $ => seq(
      field('from', $.identifier),
      '->',
      field('to', $.identifier),
    ),

    connect_param_assignment: $ => seq(
      field('name', $.identifier),
      '=',
      field('value', $._binding_value),
    ),

    // ── Chain ─────────────────────────────────────────────────
    chain_statement: $ => seq(
      'chain',
      field('first', $._expression),
      repeat1(seq('->', $._expression)),
    ),

    named_argument_list: $ => seq(
      $.named_argument,
      repeat(seq(',', $.named_argument)),
      optional(','),
    ),

    named_argument: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._binding_value),
    ),

    // ── Types ───────────────────────────────────────────────
    type_expr: $ => choice(
      $.parameterized_type,
      $.identifier,
    ),

    // A type with type arguments: `Box<T>`, `Map<String, Int>`
    parameterized_type: $ => seq(
      field('name', $.identifier),
      '<',
      field('type_args', $.type_arg_list),
      '>',
    ),

    // type_arg_list: comma-separated list of type arguments. Each element is either
    // a type expression (`Box<T>`, `Vec3<Force>`), an integer literal — required
    // for parametric types like `Tensor<rank, n, quantity>` and `Matrix<m, n, q>` —
    // or an auto type-arg (`auto: Seal`, `auto(free): Seal`).
    // The integer-vs-float / non-negative-integer constraint is enforced at type
    // resolution, not at parse time.
    type_arg_list: $ => seq(
      choice($.type_expr, $.number_literal, $.auto_type_arg),
      repeat(seq(',', choice($.type_expr, $.number_literal, $.auto_type_arg))),
      optional(','),
    ),

    // auto_type_arg: solver-determined type argument with a trait/kind bound.
    // `Bearing<auto: Seal>` and `Bearing<auto(free): Seal>` — the auto_keyword
    // child carries the strict-vs-free flag via its `modifier` field (same
    // mechanism used at param-default position, grammar.js:430-433).
    // The `bound` field is the trait or kind identifier the candidate must
    // satisfy. Composite bounds (`auto: A + B`) and parametric bounds
    // (`auto: Container<T>`) are deferred — start with a bare identifier,
    // widen to `$.trait_bound_list` in a follow-up when the PRD AC criterion
    // 9 work needs it.
    auto_type_arg: $ => seq(
      $.auto_keyword,
      ':',
      field('bound', $.identifier),
    ),

    // Dimensional type expression: supports `*`, `/` binary ops on types.
    // Used in type alias RHS to express dimensional analysis (e.g., `Force / Area`).
    dimensional_type_expr: $ => choice(
      prec.left(1, seq(field('left', $.dimensional_type_expr), field('op', '*'), field('right', $.dimensional_type_expr))),
      prec.left(1, seq(field('left', $.dimensional_type_expr), field('op', '/'), field('right', $.dimensional_type_expr))),
      $.type_expr,
    ),

    // ── Binding-site value ──────────────────────────────────
    // Shared value rule for the five binding-site slots:
    //   param_declaration.default, let_declaration.value,
    //   param_assignment.value, named_argument.value,
    //   connect_param_assignment.value.
    //
    // Admits an `auto_keyword` (solver-delegated value, strict or free)
    // OR any `_expression` (ordinary value expression).
    //
    // Design invariant: `auto_keyword` is intentionally NOT a member of
    // `_expression`, so operand positions (arithmetic, function-call args,
    // constraint bodies, list literals, etc.) reject `auto` as a parse
    // error once the external scanner reservation lands in step-12.
    _binding_value: $ => choice(
      $.auto_keyword,
      $._expression,
    ),

    // ── Expressions ─────────────────────────────────────────
    // Precedence (low → high):
    //  -15: implies (keyword, right-assoc) — loosest in language
    //  -14: or  (keyword, left-assoc)
    //  -13: and (keyword, left-assoc)
    //  -12: not (keyword, unary prefix)   ─╮ keyword logical-operator band
    //         NOTE: tree-sitter prec is higher=tighter, the INVERSE of spec §16's
    //         "1 (highest) … 15 (lowest)" numbering. Spec levels 12–15 are negated
    //         here so the ordering not(−12) > and(−13) > or(−14) > implies(−15)
    //         matches the spec exactly. The whole band sits below range(0) and the
    //         symbol forms (||=1, &&=2), making keyword ops the outermost layer.
    //         Keyword `not`(−12) is intentionally LOOSER than symbol `!`(7) per
    //         spec §16: `not a == b` → not(a==b), `!a == b` → (!a)==b.
    //   0: range (.., ..<, single-sided)
    //   1: || (or)
    //   2: && (and)
    //   3: ==, != (equality)
    //   4: <, >, <=, >= (comparison)
    //   5: +, - (additive)
    //   6: *, /, % (multiplicative)
    //   7: unary -, ! (unary)
    //   8: ^ exponentiation (right-assoc)
    //   9: postfix index access ([]), qualified access (::)
    //  10: postfix ad-hoc selector (@)
    //  11: postfix member access (.), function call

    _expression: $ => choice(
      $.range_expression,
      $.binary_expression,
      $.unary_expression,
      $.conditional_expression,
      $.match_expression,
      $.lambda_expression,
      $.quantifier_expression,
      $.ad_hoc_selector,
      $.index_access,
      $.trait_method_call,
      $.qualified_access,
      $.instance_qualified_access,
      $._primary_expression,
    ),

    // ── Lambda expression ─────────────────────────────────
    // |params| body — body extends as far right as possible (lowest precedence)
    lambda_expression: $ => prec.right(0, seq(
      '|',
      commaSep($.lambda_param),
      '|',
      field('body', $._expression),
    )),

    lambda_param: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $.type_expr))),
    ),

    // ── Forall statement (member-level) ──────────────────────
    // forall x in collection: connect ...
    // forall x in collection: constraint ...
    // forall x in collection: chain ...
    // Disambiguation: token after ':' must be 'connect', 'constraint', or 'chain'.
    // Reachable only through _member (not through _expression), so there is
    // no GLR conflict with quantifier_expression.
    //
    // Note: collection is $._expression, which syntactically includes
    // quantifier_expression (i.e. a nested forall is valid grammar).
    // Pinned by the 'nested-quantifier collection' corpus test in
    // test/corpus/forall_statement.txt; GLR resolves cleanly because the
    // outer body still requires a leading 'connect', 'chain', or 'constraint'
    // keyword.
    forall_statement: $ => seq(
      'forall',
      field('variable', $.identifier),
      'in',
      field('collection', $._expression),
      ':',
      field('body', choice(
        $.connect_statement,
        $.chain_statement,
        $.constraint_declaration,
        $.constraint_instantiation,
      )),
    ),

    // ── Quantifier expression ─────────────────────────────
    // forall x in collection: predicate
    // exists x in collection: predicate
    quantifier_expression: $ => prec.right(0, seq(
      field('quantifier', choice('forall', 'exists')),
      field('variable', $.identifier),
      'in',
      field('collection', $._expression),
      ':',
      field('predicate', $._expression),
    )),

    // ── Match expression ────────────────────────────────────
    match_expression: $ => prec.right(0, seq(
      'match',
      field('discriminant', $._expression),
      '{',
      seq($.match_arm, repeat(seq(',', $.match_arm)), optional(',')),
      '}',
    )),

    match_arm: $ => seq(
      field('pattern', $.match_pattern),
      '=>',
      field('body', $._expression),
    ),

    match_pattern: $ => choice(
      $.variant_binding_pattern,
      seq($.identifier, repeat(seq('|', $.identifier))),
      '_',
    ),

    // Named-field payload-binding pattern: `Circle { radius: r }` or
    // `Rect { width: w, height: h }`.  The variant name is followed by a
    // brace-delimited, comma-separated list of field_binding entries.
    // A trailing comma is permitted (optional(',')).
    // PRD §4.4 / task β (data-carrying-enums).
    variant_binding_pattern: $ => seq(
      field('variant', $.identifier),
      '{',
      $.field_binding,
      repeat(seq(',', $.field_binding)),
      optional(','),
      '}',
    ),

    // A single `field: binder` pair inside a variant_binding_pattern.
    field_binding: $ => seq(
      field('field', $.identifier),
      ':',
      field('binder', $.identifier),
    ),

    // ── Decl-level match block (B2, tasks 3563 + 3564) ──────────────────────
    // `match <discriminant> { Pattern => sub head : StructName, ... }` reachable
    // from `_member`. Parallel to `match_expression` (grammar.js above) but the
    // arm body is a declaration (sub form), not an expression. Lowering to
    // `MemberDecl::MatchArmDeclGroup` is wired via `lower_match_arm_decl_group`
    // in `crates/reify-syntax/src/ts_parser.rs` (task 3564).
    match_arm_decl_block: $ => seq(
      'match',
      field('discriminant', $._expression),
      '{',
      seq($.match_arm_decl_arm, repeat(seq(',', $.match_arm_decl_arm)), optional(',')),
      '}',
    ),

    match_arm_decl_arm: $ => seq(
      field('pattern', $.match_pattern),
      '=>',
      field('member', $.match_arm_sub_decl),
    ),

    // Restricted arm-body form: `sub head : HexHead`. Audit M-006 (compiler
    // entity.rs:2506-2521) explicitly rejects bodies and where clauses inside
    // match-arm sub decls today, so the grammar matches that constraint. Body
    // form `sub head : T { ... }` is deferred to B3 chain (task 3569).
    match_arm_sub_decl: $ => seq(
      'sub',
      field('name', $.identifier),
      ':',
      field('structure_name', $.identifier),
    ),

    binary_expression: $ => choice(
      // ── Keyword logical-operator band (spec §16 levels 13–15, negated for tree-sitter) ──
      prec.left(-13, seq(field('left', $._expression), field('op', 'and'), field('right', $._expression))),
      prec.left(-14, seq(field('left', $._expression), field('op', 'or'), field('right', $._expression))),
      prec.right(-15, seq(field('left', $._expression), field('op', 'implies'), field('right', $._expression))),
      // ── Symbol logical operators (kept for back-compat; deprecation deferred per PRD §10 Q3) ──
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
      prec.left(6, seq(field('left', $._expression), field('op', '%'), field('right', $._expression))),
      // ── Exponentiation (right-associative, prec 8 — tighter than unary (7) and multiplicative (6)) ──
      // PRD §3.3/§4.3: value-level ^ is right-associative; -2^2 = -(2^2), 2*3^2 = 2*(3^2).
      // Disambiguated from unit_expr pow by whitespace: unit_expr uses token.immediate('^')
      // (no whitespace), while this value-level ^ fires in the normal (whitespace-permitted) context.
      prec.right(8, seq(field('left', $._expression), field('op', '^'), field('right', $._expression))),
    ),

    // ── Range expressions ───────────────────────────────────
    // Precedence 0: lower than all other binary operators so that
    // `2mm + 1mm .. 10mm - 1mm` parses as `(2mm+1mm) .. (10mm-1mm)`.
    range_expression: $ => choice(
      // Two-sided inclusive: lower..upper
      prec.left(0, seq(field('lower', $._expression), '..', field('upper', $._expression))),
      // Two-sided exclusive upper: lower..<upper
      prec.left(0, seq(field('lower', $._expression), '..<', field('upper', $._expression))),
      // Single-sided prefix forms: >expr, >=expr, <expr, <=expr
      // op:    named field on anonymous token — accessible via childByFieldName('op').text,
      //        but NOT rendered in the S-expression (tree-sitter's named-node-only convention;
      //        matches binary_expression's op: field treatment).
      // bound: named field for the bound expression — rendered in S-expression as bound: (...).
      // Downstream ζ discriminates single-sided from two-sided by absence of lower/upper fields;
      // presence of 'bound' does not defeat that discriminator.
      prec.left(0, seq(field('op', choice('>', '>=', '<', '<=')), field('bound', $._expression))),
    ),

    unary_expression: $ => choice(
      // ── Keyword logical-operator band (spec §16 level 12, negated for tree-sitter) ──
      prec(-12, seq(field('op', 'not'), field('operand', $._expression))),
      // ── Symbol unary operators (kept for back-compat; `!` deprecation deferred per PRD §10 Q3) ──
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
      $.imaginary_literal,
      $.quantity_literal,
      // alias($._radix_literal, $.number_literal): makes hex/binary literals (0xFF,
      // 0b1010) appear as number_literal nodes in the CST.  number_literal is kept
      // as a plain token() rule so error recovery inserts a MISSING leaf (is_missing=true)
      // rather than a degenerate rule node — preserving task-3725's auto_type_arg
      // span-narrowing behaviour.  The standalone alias here handles expression context
      // (let x = 0xFF); the quantity_literal radix arm handles the unit-suffix case
      // (0xFFmm).
      alias($._radix_literal, $.number_literal),
      $.number_literal,
      $.string_literal,
      $.bool_literal,
      $.function_call,
      $.member_access,
      $.list_literal,
      $.set_literal,
      $.map_literal,
      $.identifier,
      $.parenthesized_expression,
      // Named-field brace construction: `Name { field: value, ... }` (≥1 field).
      // Symmetric with variant_binding_pattern.  Disambiguated from
      // match/where bodies by GLR (see conflicts entry above).
      // PRD §4.4 / task α (data-carrying-enums, step-6).
      $.variant_construction,
    ),

    // ── Variant brace construction ──────────────────────────────
    // `Rect { width: 20mm, height: 10mm }` — named-field variant construction.
    // ≥1 field required (no empty-brace form).  Symmetric with variant_binding_pattern
    // (pattern side) and variant_field_decl (declaration side).
    // PRD §4.4 / task α, F2-a (brace form, Leo-ratified 2026-05-27).
    variant_construction: $ => seq(
      field('name', $.identifier),
      '{',
      $.variant_construction_field,
      repeat(seq(',', $.variant_construction_field)),
      optional(','),
      '}',
    ),

    // A single `field: value` pair inside a variant_construction.
    variant_construction_field: $ => seq(
      field('field', $.identifier),
      ':',
      field('value', $._expression),
    ),

    // Imaginary literal: a decimal/scientific number immediately followed by the
    // lowercase letter `j` with NO whitespace and NO further word characters.
    // Examples: 4.1j, 2j, 1.5e-3j.
    //
    // The scanner's UNIT_EXPR_START block (src/scanner.c) special-cases bare `j`:
    // when the lookahead is `j` and the char AFTER it is NOT a word character
    // [A-Za-z0-9_], the scanner refuses to emit UNIT_EXPR_START, allowing
    // token.immediate('j') to match instead.  Multi-char j-units (`jk`, `joule`)
    // and capital-J (Joule) are unaffected — they fall through to quantity_literal.
    //
    // CST shape: (imaginary_literal value: (number_literal))
    // PRD v0_6 complex-literals-and-stdmath, slice 1, D1 — `j` suffix only (not `i`).
    imaginary_literal: $ => seq(
      field('value', $.number_literal),
      token.immediate('j'),
    ),

    // Quantity literal: number immediately followed by a unit expression (e.g. 80mm, 9.81m/s^2)
    // _unit_expr_start (external scanner) fires only when next char is a unit-start char
    // with no whitespace, enforcing the contiguity invariant from PRD §3.1.
    //
    // Two arms handle radix literals (0xFFmm) and decimal literals (5mm) separately.
    // alias($._radix_literal, $.number_literal) makes the radix prefix appear in the CST
    // as a number_literal node (same as the decimal arm) so callers see a uniform
    // quantity_literal { value: (number_literal), unit: (unit_expr) } structure.
    quantity_literal: $ => choice(
      // Radix arm: 0xFFmm → quantity_literal(value: number_literal "0xFF", unit: unit_expr "mm")
      seq(
        field('value', alias($._radix_literal, $.number_literal)),
        $._unit_expr_start,
        field('unit', $.unit_expr),
      ),
      // Decimal arm: 5mm → quantity_literal(value: number_literal "5", unit: unit_expr "mm")
      seq(
        field('value', $.number_literal),
        $._unit_expr_start,
        field('unit', $.unit_expr),
      ),
    ),

    // Unit expression: composite unit with mul (*), div (/), and pow (^) operators.
    // */  use external scanner tokens (_unit_mul_op, _unit_div_op) that peek one
    // character ahead and only fire when the operator is immediately adjacent AND the
    // next character is a valid unit-start ([A-Za-z_(]).  This prevents `25USD/1kg`
    // from greedily attempting the div arm when `/` is followed by a digit.
    // ^ uses token.immediate to disambiguate from the value-level binary `^` operator in
    // binary_expression (prec.right 8): unit_expr pow fires only when ^ is adjacent (no
    // whitespace), while value-level ^ fires in the normal whitespace-permitted context.
    // PRD §3.2: ^ binds tighter than */; */ are left-associative.
    unit_expr: $ => choice(
      prec.left(1, seq(
        field('left', $.unit_expr),
        field('op', choice($._unit_mul_op, $._unit_div_op)),
        field('right', $.unit_expr),
      )),
      // NOTE: `field('base', $.unit_expr)` technically allows another pow expression
      // as base (e.g. `m^2^3`), producing Pow(Pow(m,2),3) deterministically (left-to-
      // right, since token.immediate('^') greedy-matches the second ^ immediately after
      // the integer exponent).  PRD §3.2 does not address nested-pow in unit_expr; this
      // grammar accepts it without ambiguity.  If future PRD revisions restrict pow-base
      // to atoms only, replace `$.unit_expr` here with a narrower hidden rule (_unit_atom:
      // alias(immediate_identifier, unit_name) | paren-unit_expr) and update corpus tests.
      prec(2, seq(
        field('base', $.unit_expr),
        field('op', token.immediate('^')),
        field('exponent', $.signed_integer),
      )),
      seq(token.immediate('('), $.unit_expr, token.immediate(')')),
      alias($.immediate_identifier, $.unit_name),
    ),

    // Integer exponent for unit_expr pow arm (e.g. ^2, ^-1).
    // token.immediate enforces contiguity with the preceding ^ operator.
    signed_integer: $ => token.immediate(/-?\d+/),

    // An identifier that must immediately follow the previous token (no whitespace)
    immediate_identifier: $ => token.immediate(/[a-zA-Z_][a-zA-Z0-9_]*/),

    function_call: $ => prec(11, seq(
      field('name', $.identifier),
      callTail($),
    )),

    argument_list: $ => seq(
      choice($.named_argument, $._expression),
      repeat(seq(',', choice($.named_argument, $._expression))),
      optional(','),
    ),

    member_access: $ => prec.left(11, seq(
      field('object', $._expression),
      '.',
      field('member', $.identifier),
    )),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    // ── Collection literals ─────────────────────────────────
    list_literal: $ => seq('[', commaSep($._expression), ']'),

    set_literal: $ => seq('set', '{', commaSep($._expression), '}'),

    map_literal: $ => seq('map', '{', commaSep($.map_entry), '}'),

    map_entry: $ => seq(
      field('key', $._expression),
      '=>',
      field('value', $._expression),
    ),

    // ── Ad-hoc port selector ────────────────────────────────
    // expr @ ident(args) — selects a port on a substructure using a named selector
    // Binds tighter than index_access (prec 9) but looser than member_access (prec 11)
    ad_hoc_selector: $ => prec.left(10, seq(
      field('base', $._expression),
      '@',
      field('selector', $.identifier),
      callTail($),
    )),

    // ── Index access ────────────────────────────────────────
    index_access: $ => prec.left(9, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    // ── Qualified access ─────────────────────────────────────
    // Foo::bar — qualified name access (e.g. module/type member lookup)
    qualified_access: $ => prec.left(9, seq(
      field('qualifier', $._expression),
      '::',
      field('member', $.identifier),
    )),

    // obj.(Foo::bar) — instance-qualified access (e.g. trait-qualified method call)
    // Inner 'qualified' field accepts any expression; lowering validates it's a
    // qualified_access and emits a specific diagnostic if not.
    instance_qualified_access: $ => prec.left(9, seq(
      field('object', $._expression),
      '.',
      '(',
      field('qualified', $._expression),
      ')',
    )),

    // Trait::fn(args) or obj.(Trait::fn)(args) — callable qualified path
    trait_method_call: $ => prec(11, seq(
      field('callee', choice($.qualified_access, $.instance_qualified_access)),
      callTail($),
    )),

    // ── Literals ────────────────────────────────────────────
    //
    // number_literal is kept as a PLAIN token() (not a choice) so that
    // tree-sitter's error recovery inserts a MISSING leaf node rather than a
    // degenerate rule node.  A plain-token MISSING node has is_missing()=true
    // which is detectable by first_error_or_missing_descendant; a choice-rule
    // MISSING node gets has_error()=true but is_missing()=false with no children
    // — invisible to that function — and breaks task-3725's auto_type_arg
    // span-narrowing test.
    //
    // Radix literals (hex/binary) are integrated via alias($._radix_literal,
    // $.number_literal) at each call site (_primary_expression and
    // quantity_literal) so they appear as number_literal in the CST without
    // modifying this token rule.
    number_literal: $ => token(/\d(_?\d)*(\.\d(_?\d)*)?([eE][+-]?\d(_?\d)*)?/),

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

    // ── Pragma ──────────────────────────────────────────────
    // `#optimize` or `#config(level=3, name="test")`
    // '#' must be immediately followed by the name (no whitespace allowed).
    pragma: $ => seq(
      '#',
      field('name', alias($.immediate_identifier, $.identifier)),
      optional(seq('(', commaSep($.pragma_arg), ')')),
    ),

    // A pragma argument: either `key=value` or a bare value.
    pragma_arg: $ => choice(
      seq(field('key', $.identifier), '=', field('value', $._pragma_value)),
      field('value', $._pragma_value),
    ),

    // Pragma values are restricted to compile-time constants.
    _pragma_value: $ => choice(
      $.quantity_literal,
      $.number_literal,
      $.string_literal,
      $.bool_literal,
      $.identifier,
    ),

    // ── Annotation ──────────────────────────────────────────
    // `@test` or `@deprecated("use NewS")` — attaches to the next declaration.
    // '@' must be immediately followed by the name (no whitespace allowed).
    annotation: $ => seq(
      '@',
      field('name', alias($.immediate_identifier, $.identifier)),
      optional(seq('(', commaSep($._expression), ')')),
    ),

    // ── Comments ────────────────────────────────────────────
    line_comment: $ => token(seq('//', /.*/)),

    block_comment: $ => token(seq(
      '/*',
      /[^*]*\*+([^/*][^*]*\*+)*/,
      '/',
    )),
  },
});
