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

/**
 * Emit UNIT_EXPR_START (zero-width) iff:
 *  1. The parser is requesting it (valid_symbols[UNIT_EXPR_START]).  [invariant A]
 *  2. lexer->lookahead is NOT a whitespace character.               [invariant B]
 *  3. lexer->lookahead is a valid unit-start character: [A-Za-z_(]  [invariant C]
 *
 * Order matters: tree-sitter calls external scanners BEFORE consuming extras.
 * We call mark_end without advancing so the token is truly zero-width.
 */
bool tree_sitter_reify_external_scanner_scan(void *payload, TSLexer *lexer,
                                              const bool *valid_symbols) {
  (void)payload;

  /* [A] Bail out immediately if the parser is not requesting UNIT_EXPR_START. */
  if (!valid_symbols[UNIT_EXPR_START]) {
    return false;
  }

  int32_t c = lexer->lookahead;

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

  /* Accept only valid unit-expression start characters (PRD §3.2 unit_name
   * production: /[A-Za-z_][A-Za-z0-9_]*/, plus '(' for grouped units). */
  if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' ||
      c == '(') {
    lexer->result_symbol = UNIT_EXPR_START;
    lexer->mark_end(lexer); /* zero-width: do not advance past the lookahead */
    return true;
  }

  return false;
}
