/**
 * External scanner for the Reify tree-sitter grammar.
 *
 * Implements the zero-width boundary token `_unit_expr_start` that guards
 * the transition from a number_literal into a unit_expr.  The scanner emits
 * the token iff the next character is a valid unit-start character (letter,
 * underscore, or open-paren) AND there is no preceding whitespace — enforcing
 * the contiguity invariant from PRD §3.1:
 *
 *   "A quantity literal is number immediately adjacent to its unit — no
 *    whitespace may appear between them, or anywhere inside the unit_expr."
 *
 * Negative fixtures that this gate pins (PRD §7):
 *   "5 kg"   → no quantity_literal: space before 'k', scanner refuses
 *   "5 kg/m" → same: space before 'k', scanner refuses
 *
 * The unit_expr rule uses token.immediate(...) for every internal token
 * (*,/,^,parens,unit_name,signed_integer) to enforce contiguity declaratively
 * inside the expression; this scanner only decides the entry point.
 */

#include "tree_sitter/parser.h"

enum TokenType {
  UNIT_EXPR_START,
};

void *tree_sitter_reify_external_scanner_create(void) {
  return NULL; /* stateless scanner — no heap allocation needed */
}

void tree_sitter_reify_external_scanner_destroy(void *payload) {
  (void)payload; /* nothing to free */
}

unsigned tree_sitter_reify_external_scanner_serialize(void *payload,
                                                       char *buffer) {
  (void)payload;
  (void)buffer;
  return 0; /* no state to serialize */
}

void tree_sitter_reify_external_scanner_deserialize(void *payload,
                                                     const char *buffer,
                                                     unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length; /* no state to restore */
}

/**
 * Emit UNIT_EXPR_START (zero-width) iff:
 *  1. The parser is requesting it (valid_symbols[UNIT_EXPR_START]).
 *  2. lexer->lookahead is a valid unit-start character: [A-Za-z_(]
 *  3. lexer->lookahead is NOT a whitespace character.
 *
 * The whitespace check (condition 3) must be ordered before the unit-start
 * check (condition 2) because tree-sitter calls external scanners BEFORE
 * consuming extras (whitespace/comments).  If whitespace appears between the
 * number and the unit, lookahead will be the whitespace character itself, and
 * we must refuse — PRD §3.1 contiguity invariant.
 *
 * We call mark_end without advancing so the token is truly zero-width: the
 * scanner observes but does not consume the lookahead character.
 */
bool tree_sitter_reify_external_scanner_scan(void *payload, TSLexer *lexer,
                                              const bool *valid_symbols) {
  (void)payload;

  if (!valid_symbols[UNIT_EXPR_START]) {
    return false;
  }

  int32_t c = lexer->lookahead;

  /* Reject any whitespace — PRD §3.1 contiguity invariant, §7 negative fixtures */
  if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c < 33) {
    return false;
  }

  /* Accept only valid unit-expression start characters */
  if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' ||
      c == '(') {
    lexer->result_symbol = UNIT_EXPR_START;
    lexer->mark_end(lexer); /* zero-width: do not advance past the lookahead */
    return true;
  }

  return false;
}
