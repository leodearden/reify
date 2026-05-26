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
  UNIT_MUL_OP,
  UNIT_DIV_OP,
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

/* ── step-4 audit: whitespace-guard ordering lock ────────────────────────────
 *
 * Tree-sitter calls external scanners BEFORE consuming `extras` (whitespace,
 * comments).  This means the whitespace check MUST come before the unit-start
 * check — the invariants verified here:
 *
 *   (A) valid_symbols guard first — zero work if token not wanted.
 *   (B) Whitespace refusal BEFORE unit-start check — if c is a whitespace
 *       character we refuse immediately; the unit-start branch is never
 *       reached for a space/tab/newline lookahead.
 *   (C) PRD §3.1 contiguity invariant exercised by §7 negative fixtures:
 *         "5 kg"   → c==' '(ASCII 32) → c < 33 → false ✓
 *         "5 kg/m" → c==' '(ASCII 32) → c < 33 → false ✓
 *         "5kg"    → c=='k'(ASCII107) → letter  → true  ✓
 *
 * The scanner was already correct when written in step-2; this comment block
 * is the regression-lock making the ordering invariant auditable in git log.
 *
 * NOTE: ASCII 32 (space) satisfies c < 33, so `c < 33` alone would cover all
 * explicit whitespace chars below.  The explicit checks are kept for reviewer
 * clarity; `c < 33` is the belt-and-suspenders safety net.
 * ──────────────────────────────────────────────────────────────────────────── */

/* Helper: returns true iff c is a valid start character for a unit_expr.
 * Matches [A-Za-z_(] — the same set checked by _unit_expr_start. */
static bool is_unit_start(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' ||
         c == '(';
}

/**
 * Emit UNIT_EXPR_START (zero-width) iff:
 *  1. The parser is requesting it (valid_symbols[UNIT_EXPR_START]).  [invariant A]
 *  2. lexer->lookahead is NOT a whitespace character.               [invariant B]
 *  3. lexer->lookahead is a valid unit-start character: [A-Za-z_(]  [invariant C]
 *
 * Emit UNIT_MUL_OP ('*') or UNIT_DIV_OP ('/') iff:
 *  1. The parser is requesting the respective token.
 *  2. lexer->lookahead is '*' (for MUL) or '/' (for DIV).
 *  3. The character AFTER the operator is a valid unit-start character.
 *
 * The one-character lookahead in UNIT_MUL_OP / UNIT_DIV_OP prevents the
 * unit_expr mul/div arm from greedily consuming `*` or `/` when followed by
 * a non-unit character (e.g. a digit in `25USD/1kg`).  Without this guard,
 * tree-sitter's immediate-token preference would cause `token.immediate('/')`
 * to win over the binary `/` operator, producing an ERROR node.
 *
 * Order matters: tree-sitter calls external scanners BEFORE consuming extras.
 * We call mark_end without advancing so UNIT_EXPR_START is truly zero-width.
 * For UNIT_MUL_OP/UNIT_DIV_OP we advance past the operator before mark_end
 * (so the token IS the operator character), then peek at the following char.
 */
bool tree_sitter_reify_external_scanner_scan(void *payload, TSLexer *lexer,
                                              const bool *valid_symbols) {
  (void)payload;

  int32_t c = lexer->lookahead;

  /* ── UNIT_MUL_OP: '*' immediately followed by a unit-start character ──────
   *
   * Only emit when the character after '*' can start a unit_expr, so that
   * `5kg * m` (space before '*') and `5kg*1` ('1' is not unit-start) do not
   * greedily consume the '*' as a unit operator.
   */
  if (valid_symbols[UNIT_MUL_OP] && c == '*') {
    lexer->advance(lexer, false); /* consume '*' */
    lexer->mark_end(lexer);      /* token = '*' */
    if (is_unit_start(lexer->lookahead)) {
      lexer->result_symbol = UNIT_MUL_OP;
      return true;
    }
    return false;
  }

  /* ── UNIT_DIV_OP: '/' immediately followed by a unit-start character ──────
   *
   * Same rationale as UNIT_MUL_OP.  Critical example: `25USD/1kg` — after
   * 'USD', lookahead='/' and the char after is '1' (digit, not unit-start),
   * so we return false.  The binary '/' operator then handles the division.
   */
  if (valid_symbols[UNIT_DIV_OP] && c == '/') {
    lexer->advance(lexer, false); /* consume '/' */
    lexer->mark_end(lexer);      /* token = '/' */
    if (is_unit_start(lexer->lookahead)) {
      lexer->result_symbol = UNIT_DIV_OP;
      return true;
    }
    return false;
  }

  /* ── UNIT_EXPR_START: zero-width boundary before a unit_expr ─────────────
   *
   * [A] Bail out immediately if the parser is not requesting UNIT_EXPR_START.
   */
  if (!valid_symbols[UNIT_EXPR_START]) {
    return false;
  }

  /* [B] WHITESPACE GUARD — ordered before unit-start-char check (invariant B).
   *
   * PRD §3.1: "A quantity literal is number immediately adjacent to its unit —
   * no whitespace may appear between them."
   *
   * §7 negative fixture "5 kg":   c == ' ' → refuse → no quantity_literal
   * §7 negative fixture "5 kg/m": c == ' ' → refuse → no quantity_literal
   *
   * `c < 33` catches NUL (EOF=0), ASCII control chars (1–31), and space (32).
   * Explicit WS chars before it make the intent obvious; `c < 33` is the
   * belt-and-suspenders net for any non-printable that slips through.
   */
  if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c < 33) {
    return false;
  }

  /* [C] Accept only valid unit-expression start characters (PRD §3.2 unit_name
   * production matching [A-Za-z_][A-Za-z0-9_]*, plus '(' for grouped units. */
  if (is_unit_start(c)) {
    lexer->result_symbol = UNIT_EXPR_START;
    lexer->mark_end(lexer); /* zero-width: do not advance past the lookahead */
    return true;
  }

  return false;
}
