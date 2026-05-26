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

/* IMPORTANT: enum order MUST match the grammar.js externals array order.
 * grammar.js externals: [_unit_expr_start(0), _unit_mul_op(1), _unit_div_op(2),
 *                         _auto_token(3), _auto_reservation_sentinel(4)] */
enum TokenType {
  UNIT_EXPR_START,            /* index 0 — zero-width quantity-literal gate    */
  UNIT_MUL_OP,                /* index 1 — '*' inside unit_expr                */
  UNIT_DIV_OP,                /* index 2 — '/' inside unit_expr                */
  AUTO_TOKEN,                 /* index 3 — the bare 'auto' keyword token        */
  AUTO_RESERVATION_SENTINEL,  /* index 4 — never emitted; keeps scanner active */
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
   * [A] Only attempt if the parser is requesting UNIT_EXPR_START.
   *
   * WHITESPACE GUARD: When UNIT_EXPR_START is valid but lookahead is whitespace,
   * we do NOT return false from the entire scan function.  We simply skip the
   * UNIT_EXPR_START emission and fall through to the AUTO_TOKEN block.  The
   * AUTO_TOKEN block skips whitespace itself.  If we returned false for the
   * entire function here, the AUTO_TOKEN block would never be reached at
   * whitespace positions — which is exactly the positions where tree-sitter
   * calls the scanner when `auto` follows whitespace in a binding site.
   *
   * PRD §3.1 contiguity invariant is preserved because UNIT_EXPR_START is only
   * emitted (zero-width, no advance) when lookahead is a non-whitespace
   * unit-start character.  When there IS whitespace before a unit name, the
   * scanner does not emit UNIT_EXPR_START, the parser falls back to the regular
   * DFA which processes the whitespace as an extras token, and the resulting
   * parser state transition prevents UNIT_EXPR_START from being valid at the
   * unit-name character — exactly as before.
   */
  if (valid_symbols[UNIT_EXPR_START]) {
    bool is_ws = (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c < 33);
    if (!is_ws && is_unit_start(c)) {
      /* [C] ORDERING INVARIANT (PRD §10 / design decision): at post-number
       * positions (e.g. `5auto`), UNIT_EXPR_START is valid AND 'a' is a
       * unit_start char, so UNIT_EXPR_START fires here (zero-width, no advance)
       * BEFORE the AUTO_TOKEN block.  The regular DFA then consumes 'auto' as a
       * unit_name.  If AUTO_TOKEN ran first it would consume 'auto' and break
       * quantity parsing. */
      lexer->result_symbol = UNIT_EXPR_START;
      lexer->mark_end(lexer); /* zero-width: do not advance past the lookahead */
      return true;
    }
    /* Whitespace or non-unit-start: fall through to AUTO_TOKEN check. */
  }

  /* ── AUTO_TOKEN: narrow lexer-level reservation of `auto` ─────────────────
   *
   * Emits AUTO_TOKEN (consuming 'auto', 4 chars) when:
   *   1. lookahead (after skipping whitespace) is 'a'.
   *   2. The next three chars are 'u','t','o'.
   *   3. The char after 'auto' is NOT a word char [A-Za-z0-9_] (word boundary).
   *
   * WHITESPACE SKIPPING: tree-sitter calls the external scanner at the
   * whitespace position immediately before 'auto' (because AUTO_TOKEN or
   * AUTO_RESERVATION_SENTINEL is in valid_symbols).  Without whitespace skipping,
   * the scanner sees ' ' not 'a' and returns false; tree-sitter then processes
   * the whitespace via the regular DFA and transitions to a new parser state
   * where AUTO_TOKEN is no longer in valid_symbols.  The 'auto' token is then
   * lexed as an `identifier` — wrong.  Skipping whitespace here lets the scanner
   * reach 'a' and emit AUTO_TOKEN correctly.  If the scanner subsequently returns
   * false (e.g. not 'auto', or word-boundary fail), tree-sitter rewinds ALL
   * advances (including the whitespace skips) — the multi-char advance + return
   * false contract guarantees this.
   *
   * The scanner emits AUTO_TOKEN regardless of valid_symbols[AUTO_TOKEN]:
   *   - When valid (5 binding sites + auto_type_arg): parser uses auto_keyword. ✓
   *   - When invalid (operand positions): parser produces ERROR — exactly
   *     the §8.1 operand-rejection mechanism required by the PRD.
   *
   * The scanner is always reachable because AUTO_RESERVATION_SENTINEL is in
   * grammar.js `extras` — this keeps AUTO_RESERVATION_SENTINEL in valid_symbols
   * at every parser state, guaranteeing the scanner is invoked everywhere.
   * AUTO_RESERVATION_SENTINEL is NEVER emitted by this function.
   *
   * Why not `word: $ => $.identifier`? Would over-reserve 'source', 'frame',
   * 'direction', 'in', etc. — 36+ stdlib conflicts (materials_fea.ri,
   * fdm.ri, io.ri, solver_elastic.ri, tolerancing.ri, units.ri).
   */
  {
    /* Skip leading whitespace before the 'auto' check.  advance(skip=true)
     * marks the char as skipped (not included in token); if we return false
     * afterwards, tree-sitter rewinds to the original position. */
    int32_t ch = lexer->lookahead;
    while (ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r') {
      lexer->advance(lexer, true);
      ch = lexer->lookahead;
    }

    if (ch != 'a') return false;
    lexer->advance(lexer, false); /* consume 'a' */
    if (lexer->lookahead != 'u') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead != 't') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead != 'o') return false;
    lexer->advance(lexer, false);
    /* Word boundary check: bare `auto` must NOT be a prefix of a longer
     * identifier.  `automatic` → lookahead='m' (word char) → refuse. */
    int32_t c2 = lexer->lookahead;
    if ((c2 >= 'a' && c2 <= 'z') || (c2 >= 'A' && c2 <= 'Z') ||
        (c2 >= '0' && c2 <= '9') || c2 == '_') {
      return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = AUTO_TOKEN;
    return true;
  }

  return false;
}
